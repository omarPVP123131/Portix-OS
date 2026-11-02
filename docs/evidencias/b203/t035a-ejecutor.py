#!/usr/bin/env python3
"""T-035: una VM BIOS, ciclo de vida de páginas y controles negativos acotados."""
import argparse
import hashlib
import os
import re
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
    from .rutas_imagenes import RUN, dentro_de_dist
    from .sonda_ata_b203 import clasificar_arranque, sha256
    from .sonda_imagen_t029 import localizar_ext2
    from .verificaciones_t035_memoria import MODOS, NOMBRE, resultado_vm
else:
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import RUN, dentro_de_dist
    from sonda_ata_b203 import clasificar_arranque, sha256
    from sonda_imagen_t029 import localizar_ext2
    from verificaciones_t035_memoria import MODOS, NOMBRE, resultado_vm

ROOT = Path(__file__).resolve().parents[1]
BINARIO = ROOT / "build/packages/t035_memoria"
FUENTE = ROOT / "scripts/sondas/t035_memoria.rs"


def seleccionar_modos(cadena):
    nombres = tuple(cadena.split(",")) if cadena else MODOS
    if not nombres or len(set(nombres)) != len(nombres) or any(n not in MODOS for n in nombres):
        raise ValueError("modos inválidos o duplicados")
    return nombres


def probar(r, modo, limite):
    inicio = r.get_log_len()
    t0 = time.monotonic()
    r.send_command(f"run {NOMBRE} {modo}")
    texto = ""
    ok = terminado = False
    detalle = "sin salida"
    while time.monotonic() - t0 < limite:
        with r.log_lock:
            texto = r.log[inicio:]
        ok, terminado, detalle = resultado_vm(texto, modo)
        if terminado or r.proc is None or r.proc.poll() is not None:
            break
        time.sleep(0.05)
    print(f"[{'PASS' if ok else 'FAIL'}] T-035 {modo}: {detalle}; {time.monotonic()-t0:.1f} s; límite {limite} s", flush=True)
    if not terminado:
        try:
            r._stdin_write(b"\x1d")
            r.proc.stdin.flush()
        except (OSError, AttributeError):
            pass
        return False, False
    pos = texto.rfind("PROC: exit PID=")
    listo, _ = r.wait_for_pattern("root@portix:", start_idx=inicio + max(0, pos), timeout=8)
    if not listo:
        print("[FAIL] La consola no volvió tras la sonda", flush=True)
    return ok and listo, listo


def verificar_elf_disco_detenido(r, huella):
    """Inspecciona exclusivamente una copia privada tras detener QEMU."""
    if r.proc is None or r.proc.poll() is None or not r._img_copia:
        print("[FAIL] QEMU debe estar detenido antes de leer el disco", flush=True)
        return False
    imagen = Path(r._img_copia)
    debugfs = shutil.which("debugfs") or next((p for p in ("/usr/sbin/debugfs", "/sbin/debugfs") if Path(p).is_file()), None)
    if not dentro_de_dist(imagen) or not imagen.is_file() or not debugfs or not dentro_de_dist(RUN):
        print("[FAIL] Copia privada o debugfs no disponible", flush=True)
        return False
    RUN.mkdir(parents=True, exist_ok=True)
    fd, nombre = tempfile.mkstemp(prefix="t035-ext2-", suffix=".img", dir=RUN)
    os.close(fd)
    extraida = Path(nombre)
    try:
        with imagen.open("rb") as origen, extraida.open("wb") as destino:
            inicio, tamano = localizar_ext2(origen.read(512), imagen.stat().st_size)
            origen.seek(inicio)
            restante = tamano
            while restante:
                bloque = origen.read(min(restante, 8 * 1024 * 1024))
                if not bloque:
                    raise ValueError("partición truncada")
                destino.write(bloque)
                restante -= len(bloque)
        resultado = subprocess.run([debugfs, "-R", f"cat {NOMBRE}", str(extraida)],
                                   capture_output=True, timeout=20, env=dict(os.environ, LC_ALL="C"))
        esperado = BINARIO.read_bytes()
        encontrado = hashlib.sha256(resultado.stdout).hexdigest()
        ok = resultado.returncode == 0 and bool(esperado) and resultado.stdout == esperado and encontrado == huella
        print(f"[{'PASS' if ok else 'FAIL'}] ELF ext2 idéntico byte a byte; SHA-256={encontrado}; bytes={len(resultado.stdout)}", flush=True)
        return ok
    except (OSError, ValueError, subprocess.TimeoutExpired) as e:
        print(f"[FAIL] Inspección offline del ELF: {e}", flush=True)
        return False
    finally:
        extraida.unlink(missing_ok=True)


def ejecutar_en_vm(r, modos, limite):
    if r.httpd is None:
        print("[FAIL] No arrancó el servidor local", flush=True)
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
    print(f"[{'PASS' if ok else 'FAIL'}] T-035 arranque: {detalle}", flush=True)
    if not ok:
        return [], False
    tam = BINARIO.stat().st_size
    if not r.execute_and_verify(f"run curl -o {NOMBRE} http://10.0.2.2:8000/t035_memoria",
                                f"guardado exitosamente en '{NOMBRE}' ({tam} bytes)",
                                "T-035: fixture Rust descargado al ext2", timeout=45):
        return [], False
    resultados = []
    for modo in modos:
        aprobado, listo = probar(r, modo, limite)
        resultados.append((modo, aprobado))
        if not listo:
            print("[INCOMPLETA] Se omiten los siguientes modos: la consola no está disponible", flush=True)
            break
    return resultados, True


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--img", type=Path, required=True)
    ap.add_argument("--sha256", required=True)
    ap.add_argument("--sha-binario", required=True)
    ap.add_argument("--modos", default="", help="lista separada por comas; por defecto todos")
    ap.add_argument("--limite", type=int, default=15, choices=range(1, 31), metavar="1..30")
    args = ap.parse_args()
    try:
        modos = seleccionar_modos(args.modos)
    except ValueError as e:
        print(f"[FAIL] {e}", flush=True)
        return 1
    imagen = args.img.resolve()
    if (not dentro_de_dist(imagen) or not imagen.is_file() or not BINARIO.is_file() or not FUENTE.is_file()
        or not re.fullmatch(r"[0-9a-f]{64}", args.sha256)
        or not re.fullmatch(r"[0-9a-f]{64}", args.sha_binario)):
        print("[FAIL] Imagen, fixture, fuente o hashes inválidos", flush=True)
        return 1
    img_sha, bin_sha, fuente_sha = sha256(imagen), sha256(BINARIO), sha256(FUENTE)
    if img_sha != args.sha256 or bin_sha != args.sha_binario:
        print(f"[FAIL] Hash distinto: imagen={img_sha} binario={bin_sha}", flush=True)
        return 1
    print(f"[INFO] Diagnóstico BIOS; imagen SHA-256={img_sha}; ELF={bin_sha}; fuente={fuente_sha}; modos={','.join(modos)}", flush=True)
    r = None
    resultados = []
    descargado = inspeccion = cierre_bien = False
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=True, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        resultados, descargado = ejecutar_en_vm(r, modos, args.limite)
        if descargado and r.proc is not None:
            if r.proc.poll() is None:
                r.proc.terminate()
                try:
                    r.proc.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    print("[FAIL] QEMU no se detuvo; se prohíbe inspeccionar el disco", flush=True)
            if r.proc.poll() is not None:
                inspeccion = verificar_elf_disco_detenido(r, bin_sha)
    finally:
        if r is not None:
            r.close()
            cierre_bien = not any(v is False for v in r.results.values())
    intactos = sha256(imagen) == img_sha and sha256(BINARIO) == bin_sha and sha256(FUENTE) == fuente_sha
    print(f"[{'PASS' if intactos else 'FAIL'}] Imagen fuente, ELF y fuente Rust intactos", flush=True)
    aprobados = sum(ok for _, ok in resultados)
    fallidos = len(resultados) - aprobados
    omitidos = len(modos) - len(resultados)
    valido = (not fallidos and not omitidos and descargado and inspeccion and intactos and cierre_bien)
    print(f"T-035 RESULTADO: casos={len(resultados)}/{len(modos)}; aprobados={aprobados}; fallidos={fallidos}; omitidos={omitidos}; integridad_ext2={'PASS' if inspeccion else 'FAIL'}; no certifica b203", flush=True)
    return 0 if valido else 1


if __name__ == "__main__":
    raise SystemExit(main())
