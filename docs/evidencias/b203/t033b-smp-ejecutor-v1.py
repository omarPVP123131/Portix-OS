#!/usr/bin/env python3
"""T-033 SMP: una VM, controles negativos y lecturas reales, sin certificar b203."""
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
    from .verificaciones_t033_smp import MODOS, resultado_smp
else:
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import RUN, dentro_de_dist
    from sonda_ata_b203 import clasificar_arranque, sha256
    from sonda_imagen_t029 import localizar_ext2
    from verificaciones_t033_smp import MODOS, resultado_smp

ROOT = Path(__file__).resolve().parents[1]
BINARIO = ROOT / "build/packages/t033_smp"
FUENTE = ROOT / "scripts/sondas/t033_smp.rs"


def probar(r, modo, limite=25):
    inicio = r.get_log_len()
    t0 = time.monotonic()
    r.send_command(f"run /usr/local/t033smp {modo}")
    texto = ""
    ok = terminado = False
    detalle = "sin salida"
    while time.monotonic() - t0 < limite:
        with r.log_lock:
            texto = r.log[inicio:]
        ok, terminado, detalle = resultado_smp(texto, modo)
        if terminado or r.proc is None or r.proc.poll() is not None:
            break
        time.sleep(0.05)
    print(f"[{'PASS' if ok else 'FAIL'}] T-033 SMP {modo}: {detalle}; {time.monotonic()-t0:.1f} s; límite {limite} s", flush=True)
    if not terminado:
        try:
            r._stdin_write(b"\x1d")
            r.proc.stdin.flush()
        except Exception:
            pass
        return False, False
    pos = texto.rfind("PROC: exit PID=")
    listo, _ = r.wait_for_pattern("root@portix:", start_idx=inicio + max(0, pos), timeout=8)
    if not listo:
        print("[FAIL] La consola no volvió tras la sonda", flush=True)
    return ok and listo, listo


def verificar_binario_en_disco_detenido(r, hash_esperado):
    """Sólo lee la copia privada DESPUÉS de detener QEMU y compara bytes y SHA.

    La partición temporal también vive bajo build/dist y se elimina al terminar.
    Un fallo invalida la atribución de la prueba, no se trata como advertencia.
    """
    if r.proc is None or r.proc.poll() is None or not r._img_copia:
        print("[FAIL] QEMU debe estar detenido antes de inspeccionar el disco", flush=True)
        return False
    imagen = Path(r._img_copia)
    debugfs = shutil.which("debugfs") or next((p for p in ("/usr/sbin/debugfs", "/sbin/debugfs") if Path(p).is_file()), None)
    if not dentro_de_dist(imagen) or not imagen.is_file() or not debugfs or not dentro_de_dist(RUN):
        print("[FAIL] No se puede inspeccionar la copia privada ext2", flush=True)
        return False
    RUN.mkdir(parents=True, exist_ok=True)
    fd, nombre = tempfile.mkstemp(prefix="t033-smp-ext2-", suffix=".img", dir=RUN)
    os.close(fd)
    extraida = Path(nombre)
    try:
        with imagen.open("rb") as fuente, extraida.open("wb") as destino:
            inicio, tamano = localizar_ext2(fuente.read(512), imagen.stat().st_size)
            fuente.seek(inicio)
            restante = tamano
            while restante:
                bloque = fuente.read(min(restante, 8 * 1024 * 1024))
                if not bloque:
                    raise ValueError("partición truncada")
                destino.write(bloque)
                restante -= len(bloque)
        p = subprocess.run([debugfs, "-R", "cat /usr/local/t033smp", str(extraida)],
                           capture_output=True, timeout=20, env=dict(os.environ, LC_ALL="C"))
        esperado = BINARIO.read_bytes()
        encontrado = hashlib.sha256(p.stdout).hexdigest()
        ok = p.returncode == 0 and bool(esperado) and p.stdout == esperado and encontrado == hash_esperado
        print(f"[{'PASS' if ok else 'FAIL'}] ELF del ext2 idéntico byte a byte; SHA-256={encontrado}; bytes={len(p.stdout)}; debugfs={p.returncode}", flush=True)
        return ok
    except (OSError, ValueError, subprocess.TimeoutExpired) as e:
        print(f"[FAIL] Inspección de ELF en ext2: {e}", flush=True)
        return False
    finally:
        extraida.unlink(missing_ok=True)


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
    print(f"[{'PASS' if ok else 'FAIL'}] T-033 SMP arranque: {detalle}", flush=True)
    if not ok:
        return [], False
    tam = BINARIO.stat().st_size
    if not r.execute_and_verify("run curl -o /usr/local/t033smp http://10.0.2.2:8000/t033_smp",
                                f"guardado exitosamente en '/usr/local/t033smp' ({tam} bytes)",
                                "T-033 SMP: sonda Rust descargada al ext2", timeout=45):
        return [], False
    resultados = []
    for modo in MODOS:
        ok, listo = probar(r, modo)
        resultados.append((modo, ok))
        if not listo:
            print("[INCOMPLETA] Se omiten los casos restantes: consola ocupada o VM detenida", flush=True)
            break
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
        print("[FAIL] Imagen, binario, fuente o hashes inválidos", flush=True)
        return 1
    img_sha, bin_sha, fuente_sha = sha256(imagen), sha256(BINARIO), sha256(FUENTE)
    if img_sha != args.sha256 or bin_sha != args.sha_binario:
        print(f"[FAIL] Hash distinto: imagen={img_sha} binario={bin_sha}", flush=True)
        return 1
    print(f"[INFO] Sólo diagnóstico BIOS; imagen SHA-256={img_sha}; ELF SHA-256={bin_sha}; fuente SHA-256={fuente_sha}", flush=True)
    r = None
    resultados = []
    descargado = False
    inspeccion = False
    cierre_bien = False
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=True, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        resultados, descargado = ejecutar_en_vm(r)
        if descargado and r.proc is not None and r.proc.poll() is None:
            # La inspección del ext2 no se hace contra un disco en uso.
            r.proc.terminate()
            try:
                r.proc.wait(timeout=8)
                inspeccion = verificar_binario_en_disco_detenido(r, bin_sha)
            except subprocess.TimeoutExpired:
                print("[FAIL] QEMU no se detuvo en 8 s; no se lee su disco", flush=True)
    finally:
        if r is not None:
            r.close()
            cierre_bien = not any(v is False for v in r.results.values())
    intactos = sha256(imagen) == img_sha and sha256(BINARIO) == bin_sha and sha256(FUENTE) == fuente_sha
    print(f"[{'PASS' if intactos else 'FAIL'}] Imagen fuente, ELF y fuente Rust intactos", flush=True)
    aprobados = sum(ok for _, ok in resultados)
    fallidos = len(resultados) - aprobados
    omitidos = len(MODOS) - len(resultados)
    valido = (len(resultados) == len(MODOS) and fallidos == 0 and descargado and inspeccion and intactos and cierre_bien)
    print(f"T-033 SMP RESULTADO: casos={len(resultados)}/{len(MODOS)}; aprobados={aprobados}; fallidos={fallidos}; omitidos={omitidos}; integridad_ext2={'PASS' if inspeccion else 'FAIL'}; no certifica b203", flush=True)
    return 0 if valido else 1


if __name__ == "__main__":
    raise SystemExit(main())
