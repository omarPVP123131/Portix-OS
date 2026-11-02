#!/usr/bin/env python3
"""Inspección QMP de marcos y contraste local de PROT_NONE; una VM BIOS acotada."""
import argparse
import re
import time
from pathlib import Path

if __package__:
    from . import sonda_smp_b203 as base
    from .headless_runner import PortixHeadlessRunner
    from .rutas_imagenes import dentro_de_dist
    from .sonda_ata_b203 import clasificar_arranque, sha256
    from .verificaciones_t033_smp import resultado_smp
    from .verificaciones_t033_marcos import FASES, DIRECCIONES, marca_fase, caminar_pte, evaluar_capturas
    from .qmp_seguro import consultar_qmp
else:
    import sonda_smp_b203 as base
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import dentro_de_dist
    from sonda_ata_b203 import clasificar_arranque, sha256
    from verificaciones_t033_smp import resultado_smp
    from verificaciones_t033_marcos import FASES, DIRECCIONES, marca_fase, caminar_pte, evaluar_capturas
    from qmp_seguro import consultar_qmp

ROOT = Path(__file__).resolve().parents[1]
BINARIO = base.BINARIO
FUENTE = base.FUENTE


def respuesta_qmp(r, comando, argumentos=None):
    """Mismo cliente correlacionado que usa el ejecutor general."""
    return consultar_qmp(r.qmp_path, comando, argumentos)


def cambiar_pausa(r, pausar):
    respuesta_qmp(r, "stop" if pausar else "cont")
    estado = respuesta_qmp(r, "query-status")
    if not isinstance(estado, dict) or estado.get("running") is not (not pausar):
        raise ValueError(f"QEMU no confirmó el estado de pausa solicitado: {estado!r}")


def leer_fisica(r, direccion, ancho):
    if not isinstance(direccion, int) or direccion < 0 or ancho not in ("gx", "bx"):
        raise ValueError("lectura física inválida")
    texto = respuesta_qmp(r, "human-monitor-command", {"command-line": f"xp /1{ancho} 0x{direccion:x}"})
    if not isinstance(texto, str):
        raise ValueError("monitor no devolvió texto")
    m = re.fullmatch(r"\s*(?:0x)?([0-9a-fA-F]+):\s*0x([0-9a-fA-F]+)\s*", texto)
    if not m or int(m.group(1), 16) != direccion:
        raise ValueError(f"lectura física ambigua: {texto!r}")
    valor = int(m.group(2), 16)
    if valor >= (1 << (64 if ancho == "gx" else 8)):
        raise ValueError("lectura fuera de ancho")
    return valor


def cr3_de_lider(texto):
    texto = texto.replace("\r", "")
    entradas = list(re.finditer(r"(?m)^\[R3\] enter PID=(\d+) name='/usr/local/t033smp'(?:\s|$)", texto))
    creaciones = list(re.finditer(r"(?m)^PROC: create PID=(\d+) name='/usr/local/t033smp'[^\n]*\bcr3=0x([0-9a-fA-F]+)(?:\s|$)", texto))
    if len(entradas) != 1 or len(creaciones) != 1 or entradas[0].group(1) != creaciones[0].group(1):
        raise ValueError("PID o CR3 del líder ambiguos")
    return int(creaciones[0].group(2), 16)


def capturar_fase(r, cr3, direcciones):
    cache = {}
    def leer64(addr):
        if addr not in cache:
            cache[addr] = leer_fisica(r, addr, "gx")
        return cache[addr]
    capturada = {}
    for nombre in DIRECCIONES:
        registro = caminar_pte(cr3, direcciones[nombre], leer64)
        registro["byte"] = leer_fisica(r, registro["fisica"], "bx") if registro["fisica"] is not None else None
        capturada[nombre] = registro
    return capturada


def informar_captura(fase, cr3, direcciones, captura):
    for nombre in DIRECCIONES:
        dato = captura[nombre]
        fisica = "ninguna" if dato["fisica"] is None else f"0x{dato['fisica']:x}"
        byte = "ninguno" if dato["byte"] is None else str(dato["byte"])
        print(f"[T033-MARCOS-QMP] fase={fase} cr3=0x{cr3:x} {nombre}=0x{direcciones[nombre]:x} pte=0x{dato['pte']:016x} fisica={fisica} byte={byte}", flush=True)


def probar_marcos(r, limite=40):
    inicio = r.get_log_len()
    t0 = time.monotonic()
    r.send_command("run /usr/local/t033smp marcos")
    capturas = {}
    direcciones_iniciales = None
    cr3_inicial = None
    error = None
    texto = ""
    for fase in FASES:
        while time.monotonic() - t0 < limite:
            with r.log_lock:
                texto = r.log[inicio:]
            direcciones = marca_fase(texto, fase)
            if direcciones is not None:
                break
            if r.proc is None or r.proc.poll() is not None:
                break
            time.sleep(0.02)
        else:
            direcciones = None
        if direcciones is None:
            error = f"marca ausente o ambigua: {fase}"
            break
        try:
            if any(marca_fase(texto, siguiente) is not None for siguiente in FASES[FASES.index(fase)+1:]):
                raise ValueError("se perdió la ventana de la fase")
            cr3 = cr3_de_lider(texto)
            if direcciones_iniciales is None:
                direcciones_iniciales, cr3_inicial = direcciones, cr3
            if direcciones != direcciones_iniciales or cr3 != cr3_inicial:
                raise ValueError("cambió el CR3 o las direcciones entre fases")
            try:
                cambiar_pausa(r, True)
                captura = capturar_fase(r, cr3, direcciones)
            finally:
                # Incluso si la confirmación de STOP falla, no dejar una VM
                # posiblemente detenida a la espera del siguiente caso.
                cambiar_pausa(r, False)
            capturas[fase] = captura
            informar_captura(fase, cr3, direcciones, captura)
        except (OSError, ValueError, KeyError) as e:
            error = f"{fase}: {e}"
            break
    if error:
        print(f"[FAIL] T-033 MARCOS inspección incompleta: {error}", flush=True)
    else:
        ok_fisico, detalle, reusos = evaluar_capturas(capturas)
        print(f"[{'PASS' if ok_fisico else 'INCONCLUSA'}] T-033 MARCOS: {detalle}; parejas={reusos}", flush=True)
    ok_sonda = terminado = False
    detalle_sonda = "sin salida"
    while time.monotonic() - t0 < limite:
        with r.log_lock:
            texto = r.log[inicio:]
        ok_sonda, terminado, detalle_sonda = resultado_smp(texto, "marcos")
        if terminado or r.proc is None or r.proc.poll() is not None:
            break
        time.sleep(0.05)
    if not terminado:
        print(f"[FAIL] T-033 MARCOS: {detalle_sonda}; límite {limite} s", flush=True)
        try:
            r._stdin_write(b"\x1d")
            r.proc.stdin.flush()
        except Exception:
            pass
        return False, False
    pos = texto.rfind("PROC: exit PID=")
    listo, _ = r.wait_for_pattern("root@portix:", start_idx=inicio + max(0, pos), timeout=8)
    fisica_ok = not error and evaluar_capturas(capturas)[0]
    ok = ok_sonda and listo and fisica_ok
    print(f"[{'PASS' if ok else 'FAIL'}] T-033 MARCOS sonda+observación: {detalle_sonda}; {time.monotonic()-t0:.1f} s", flush=True)
    return ok, listo


def ejecutar_en_vm(r):
    if r.httpd is None:
        print("[FAIL] Servidor local no arrancó", flush=True)
        return [], False
    t0 = time.monotonic()
    ok = False
    detalle = "sin arranque"
    while time.monotonic() - t0 < 35:
        with r.log_lock:
            serie = r.log
        ok, terminado, detalle = clasificar_arranque(serie)
        if terminado or r.proc is None or r.proc.poll() is not None:
            break
        time.sleep(0.05)
    print(f"[{'PASS' if ok else 'FAIL'}] T-033 MARCOS arranque: {detalle}", flush=True)
    if not ok:
        return [], False
    tam = BINARIO.stat().st_size
    if not r.execute_and_verify("run curl -o /usr/local/t033smp http://10.0.2.2:8000/t033_smp",
                                f"guardado exitosamente en '/usr/local/t033smp' ({tam} bytes)",
                                "T-033 MARCOS: fixture Rust descargado", timeout=45):
        return [], False
    resultados = []
    for modo in ("control-proteccion-local", "proteccion-local"):
        ok, listo = base.probar(r, modo, limite=25)
        resultados.append((modo, ok))
        if not listo:
            print("[INCOMPLETA] Consola ocupada; no se inspeccionan marcos", flush=True)
            return resultados, True
    ok, _ = probar_marcos(r)
    resultados.append(("marcos", ok))
    return resultados, True


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--img", type=Path, required=True)
    ap.add_argument("--sha256", required=True)
    ap.add_argument("--sha-binario", required=True)
    args = ap.parse_args()
    imagen = args.img.resolve()
    if (not dentro_de_dist(imagen) or not imagen.is_file() or not BINARIO.is_file() or not FUENTE.is_file()
        or not re.fullmatch(r"[0-9a-f]{64}", args.sha256)
        or not re.fullmatch(r"[0-9a-f]{64}", args.sha_binario)):
        print("[FAIL] Imagen, fixture o hashes inválidos", flush=True)
        return 1
    img_sha, bin_sha, fuente_sha = sha256(imagen), sha256(BINARIO), sha256(FUENTE)
    if img_sha != args.sha256 or bin_sha != args.sha_binario:
        print(f"[FAIL] Hash distinto: imagen={img_sha} binario={bin_sha}", flush=True)
        return 1
    print(f"[INFO] Sólo diagnóstico BIOS; imagen SHA-256={img_sha}; ELF SHA-256={bin_sha}; fuente SHA-256={fuente_sha}", flush=True)
    r = None
    resultados = []
    descargado = inspeccion = cierre_bien = False
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=True, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        resultados, descargado = ejecutar_en_vm(r)
        if descargado and r.proc is not None and r.proc.poll() is None:
            r.proc.terminate()
            try:
                r.proc.wait(timeout=8)
                inspeccion = base.verificar_binario_en_disco_detenido(r, bin_sha)
            except Exception as e:
                print(f"[FAIL] Cierre/inspección del disco: {e}", flush=True)
    finally:
        if r is not None:
            r.close()
            cierre_bien = not any(v is False for v in r.results.values())
    intactos = sha256(imagen) == img_sha and sha256(BINARIO) == bin_sha and sha256(FUENTE) == fuente_sha
    print(f"[{'PASS' if intactos else 'FAIL'}] Imagen fuente, ELF y fuente Rust intactos", flush=True)
    aprobados = sum(ok for _, ok in resultados)
    fallidos = len(resultados) - aprobados
    omitidos = 3 - len(resultados)
    valido = len(resultados) == 3 and fallidos == 0 and descargado and inspeccion and intactos and cierre_bien
    print(f"T-033 MARCOS RESULTADO: casos={len(resultados)}/3; aprobados={aprobados}; fallidos={fallidos}; omitidos={omitidos}; integridad_ext2={'PASS' if inspeccion else 'FAIL'}; no certifica b203", flush=True)
    return 0 if valido else 1


if __name__ == "__main__":
    raise SystemExit(main())
