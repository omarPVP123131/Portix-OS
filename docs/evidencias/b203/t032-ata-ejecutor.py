#!/usr/bin/env python3
"""Arranques BIOS acotados y aislados para observar T-032; no certifican b203."""
import argparse
import hashlib
import json
import re
import time
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
    from .rutas_imagenes import dentro_de_dist
else:
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import dentro_de_dist

ROOT = Path(__file__).resolve().parents[1]
EVIDENCIA = ROOT / "docs/evidencias/b203"


def clasificar_arranque(serie):
    """Sólo aprueba disco real, raíz ext2 e init; un modo degradado es fallo."""
    texto = serie.replace("\r", "")
    if "[MODO DEGRADADO]" in texto:
        return False, True, "el núcleo entró en modo degradado"
    if "[R3] init process PID=1 registered" not in texto:
        return False, False, "init aún no aparece"
    pruebas = (
        (r"(?m)^ATA \[pri/master\] ATA\s+LBA(?:28|48)\s+[1-9]\d* MiB\s+\S.*$", "ATA primario ausente o inválido"),
        (r"(?m)^\[FS\] dev0_kind=1$", "device 0 no es ATA"),
        (r"(?m)^\[EXT2\] montado en / \(raiz\): .*", "no se confirmó raíz ext2"),
        (r"(?m)^\[R3\] init process PID=1 registered$", "init no es una línea exacta"),
    )
    for patron, detalle in pruebas:
        if not re.search(patron, texto):
            return False, True, detalle
    return True, True, "ATA primario, raíz ext2 e init presentes"


def sha256(ruta):
    h = hashlib.sha256()
    with ruta.open("rb") as f:
        for trozo in iter(lambda: f.read(8 * 1024 * 1024), b""):
            h.update(trozo)
    return h.hexdigest()


def argumentos_traza(ruta):
    # Sin eventos por palabra de datos. El estado alternativo se habilita por
    # QMP sólo durante la ventana del segundo scan para limitar su volumen.
    eventos = ("ide_ctrl_write", "ide_reset", "ide_bus_exec_cmd", "ide_ioport_read", "ide_ioport_write")
    return [x for evento in eventos for x in ("-trace", f"enable={evento}")] + ["-trace", f"file={ruta}"]


def conmutar_estado_alternativo(r, activo):
    respuesta = r.qmp("trace-event-set-state", {"name": "ide_status_read", "enable": activo})
    try:
        return "return" in json.loads(respuesta.decode("utf8"))
    except (ValueError, UnicodeError, AttributeError):
        return False


def traza_incluye_reset_primario(ruta):
    """Exige lecturas alternativas durante el SRST primario del segundo scan."""
    en_reset = False
    with ruta.open("r", encoding="ascii", errors="replace") as f:
        for linea in f:
            if linea.startswith("ide_ctrl_write") and "@ 0x3f6 " in linea:
                if "val 0x06;" in linea:
                    en_reset = True
                elif "val 0x02;" in linea:
                    en_reset = False
            elif en_reset and linea.startswith("ide_status_read") and "@ 0x3f6 " in linea:
                return True
    return False


def pasada(imagen, salida, plazo, traza=None):
    inicio = time.monotonic()
    r = None
    serie = ""
    cmd = "no iniciado"
    ok = False
    detalle = "sin resultado"
    traza_activa = False
    traza_activada = False
    traza_desactivada = False
    try:
        # El runner crea una copia privada única bajo build/dist y la retira
        # sólo después de detener SU QEMU. No toca la imagen suministrada.
        r = PortixHeadlessRunner(str(imagen), verbose=False, silicon=True, conservar=False)
        r.start_qemu(qemu_args=argumentos_traza(traza) if traza else ())
        cmd = repr(r.proc.args)
        t0 = time.monotonic()
        while time.monotonic() - t0 < plazo:
            with r.log_lock:
                serie = r.log
            if traza is not None and traza.exists() and traza.stat().st_size > 128 * 1024 * 1024:
                detalle = "traza IDE superó 128 MiB: arranque interrumpido por seguridad"
                break
            if traza is not None and not traza_activada and "[ INF ] HW" in serie:
                traza_activada = conmutar_estado_alternativo(r, True)
                traza_activa = traza_activada
                if not traza_activada:
                    detalle = "QMP no activó la traza de estado alternativo"
                    break
            if traza_activa and ("[FS] dev0_kind=" in serie or "[MODO DEGRADADO]" in serie):
                traza_desactivada = conmutar_estado_alternativo(r, False)
                traza_activa = False
                if not traza_desactivada:
                    detalle = "QMP no desactivó la traza de estado alternativo"
                    break
            ok, concluido, detalle = clasificar_arranque(serie)
            if concluido:
                break
            if r.proc.poll() is not None:
                detalle = "QEMU terminó antes de llegar a init"
                break
            time.sleep(0.05)
        else:
            detalle = f"plazo de {plazo:g} s agotado: {detalle}"
    except Exception as exc:
        detalle = f"excepción del ejecutor: {type(exc).__name__}: {exc}"
    finally:
        if r is not None:
            r.close()
            if any(v is False for v in r.results.values()):
                ok = False
                detalle += "; error al cerrar recursos propios"
        if traza is not None:
            if not traza.is_file() or traza.stat().st_size == 0:
                ok = False
                detalle += "; traza IDE de QEMU ausente o vacía"
            elif not traza_activada or not traza_desactivada or not traza_incluye_reset_primario(traza):
                ok = False
                detalle += "; traza alternativa no cubrió el reset primario"
        duracion = time.monotonic() - inicio
        cabecera = (f"imagen={imagen}\nqemu={cmd}\nplazo_vm_s={plazo:g}\n"
                    f"traza_alternativa_activada={traza_activada} desactivada={traza_desactivada}\n"
                    f"duracion_total_s={duracion:.2f}\nresultado={'PASS' if ok else 'FAIL'}\n"
                    f"detalle={detalle}\n--- SERIE ---\n")
        with salida.open("x", encoding="utf8") as f:
            f.write(cabecera + serie)
    return ok, detalle, duracion


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--img", type=Path, required=True)
    ap.add_argument("--sha256", required=True)
    ap.add_argument("--etiqueta", required=True, help="etiqueta nueva para logs, p. ej. base118")
    ap.add_argument("--repeticiones", type=int, default=4, choices=range(1, 6))
    ap.add_argument("--plazo", type=float, default=30.0, help="segundos máximos por VM; entre 5 y 45")
    ap.add_argument("--traza-ata", action="store_true", help="guardar eventos IDE seleccionados de QEMU")
    args = ap.parse_args()
    img = args.img.resolve()
    if not dentro_de_dist(img) or not img.is_file() or not 5 <= args.plazo <= 45:
        print("[FAIL] Imagen o plazo inválidos; la imagen debe estar en build/dist", flush=True)
        return 1
    if not re.fullmatch(r"[a-z0-9-]{1,40}", args.etiqueta) or not re.fullmatch(r"[0-9a-f]{64}", args.sha256):
        print("[FAIL] Etiqueta o SHA-256 inválidos", flush=True)
        return 1
    salidas = [EVIDENCIA / f"t032-{args.etiqueta}-{n:02}.log" for n in range(1, args.repeticiones + 1)]
    trazas = [p.with_name(p.stem + "-ide.log") for p in salidas] if args.traza_ata else [None] * len(salidas)
    if any(p.exists() for p in salidas + [p for p in trazas if p is not None]):
        print("[FAIL] Ya existen registros para esa etiqueta; no se sobrescriben", flush=True)
        return 1
    EVIDENCIA.mkdir(parents=True, exist_ok=True)
    antes = sha256(img)
    if antes != args.sha256:
        print(f"[FAIL] SHA-256 de la imagen no coincide: {antes}", flush=True)
        return 1
    print(f"[INFO] Imagen verificada: {antes}; {len(salidas)} arranques en serie; límite VM {args.plazo:g} s", flush=True)
    fallos = 0
    for n, (salida, traza) in enumerate(zip(salidas, trazas), 1):
        ok, detalle, duracion = pasada(img, salida, args.plazo, traza)
        fallos += not ok
        print(f"[{'PASS' if ok else 'FAIL'}] T-032 arranque {n}/{len(salidas)}: {detalle}; {duracion:.1f} s; {salida.name}", flush=True)
    despues = sha256(img)
    if despues != antes:
        print(f"[FAIL] La imagen de entrada cambió: {antes} -> {despues}", flush=True)
        fallos += 1
    else:
        print(f"[PASS] Imagen de entrada intacta: {despues}", flush=True)
    print(f"T-032 RESULTADO: {len(salidas)-min(fallos, len(salidas))}/{len(salidas)} arranques; {fallos} fallos; no certifica b203.", flush=True)
    return 1 if fallos else 0


if __name__ == "__main__":
    raise SystemExit(main())
