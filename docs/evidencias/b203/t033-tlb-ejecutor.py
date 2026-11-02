#!/usr/bin/env python3
"""T-033: mide reservas y munmap con y sin hilo en CPU 1; no certifica."""
import argparse
import re
import time
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
    from .rutas_imagenes import dentro_de_dist
    from .sonda_ata_b203 import clasificar_arranque, sha256
    from .verificaciones_t033_tlb import resultado_tlb
else:
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import dentro_de_dist
    from sonda_ata_b203 import clasificar_arranque, sha256
    from verificaciones_t033_tlb import resultado_tlb

ROOT = Path(__file__).resolve().parents[1]
BINARIO = ROOT / "build/packages/t033_tlb"


def probar(r, modo, limite=40):
    inicio = r.get_log_len()
    t0 = time.monotonic()
    tiempos = {}
    marcas = ("comienza=mmap", "fase=mmap", "comienza=munmap", "fase=munmap")
    r.send_command(f"run /usr/local/t033tlb {modo}")
    texto = ""
    ok = terminado = False
    detalle = "sin salida"
    cifras = None
    while time.monotonic() - t0 < limite:
        with r.log_lock:
            texto = r.log[inicio:]
        for marca in marcas:
            if marca not in tiempos and marca in texto:
                tiempos[marca] = time.monotonic() - t0
        ok, terminado, detalle, cifras = resultado_tlb(texto, modo)
        if terminado or r.proc is None or r.proc.poll() is not None:
            break
        time.sleep(0.05)
    transcurrido = time.monotonic() - t0
    print(f"[{'PASS' if ok else 'FAIL'}] T-033 TLB {modo}: {detalle}; {transcurrido:.1f} s; límite {limite} s", flush=True)
    for nombre in ("mmap", "munmap"):
        a, b = tiempos.get(f"comienza={nombre}"), tiempos.get(f"fase={nombre}")
        if a is not None and b is not None and b >= a:
            print(f"[T-033 TLB-RELOJ] modo={modo} fase={nombre} pared_host_aprox={b-a:.2f}s resolución_sondeo=0.05s", flush=True)
    if cifras is not None:
        print(f"[T-033 TLB-CUENTAS] modo={modo} paginas=4096 mmap={cifras['mmap']} munmap={cifras['munmap']}", flush=True)
    if not terminado:
        try:
            r._stdin_write(b"\x1d")
            r.proc.stdin.flush()
        except Exception:
            pass
        return False, False, cifras
    pos = texto.rfind("PROC: exit PID=")
    listo, _ = r.wait_for_pattern("root@portix:", start_idx=inicio + max(0, pos), timeout=10)
    if not listo:
        print("[FAIL] No volvió la consola tras la sonda", flush=True)
    return ok and listo, listo, cifras


def ejecutar_en_vm(r):
    if r.httpd is None:
        print("[FAIL] Servidor local no arrancó", flush=True)
        return 1
    inicio = time.monotonic()
    ok = False
    detalle = "sin arranque"
    while time.monotonic() - inicio < 30:
        with r.log_lock:
            serie = r.log
        ok, terminado, detalle = clasificar_arranque(serie)
        if terminado or r.proc is None or r.proc.poll() is not None:
            break
        time.sleep(0.05)
    print(f"[{'PASS' if ok else 'FAIL'}] T-033 TLB arranque: {detalle}", flush=True)
    if not ok:
        return 1
    tam = BINARIO.stat().st_size
    if not r.execute_and_verify("run curl -o /usr/local/t033tlb http://10.0.2.2:8000/t033_tlb",
                                f"guardado exitosamente en '/usr/local/t033tlb' ({tam} bytes)",
                                "T-033: sonda Rust descargada al ext2", timeout=45):
        return 1
    total = 0
    medidas = {}
    for modo in ("solo", "hilos"):
        ok, listo, cifras = probar(r, modo)
        total += int(not ok)
        if cifras is not None:
            medidas[modo] = cifras
        if not listo:
            print("[INCOMPLETA] Se omiten modos restantes: consola ocupada", flush=True)
            return max(total, 1)
    if len(medidas) == 2:
        for fase in ("mmap", "munmap"):
            print(f"[T-033 COMPARACIÓN] {fase}: sin_hilo={medidas['solo'][fase]} con_hilo={medidas['hilos'][fase]} shootdowns; 4096 páginas", flush=True)
    return total


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--img", type=Path, required=True)
    ap.add_argument("--sha256", required=True)
    ap.add_argument("--sha-binario", required=True)
    args = ap.parse_args()
    imagen = args.img.resolve()
    if (not dentro_de_dist(imagen) or not imagen.is_file() or not BINARIO.is_file()
        or not re.fullmatch(r"[0-9a-f]{64}", args.sha256)
        or not re.fullmatch(r"[0-9a-f]{64}", args.sha_binario)):
        print("[FAIL] Imagen, binario o hashes inválidos", flush=True)
        return 1
    img_sha, bin_sha = sha256(imagen), sha256(BINARIO)
    if img_sha != args.sha256 or bin_sha != args.sha_binario:
        print(f"[FAIL] Hash distinto: imagen={img_sha} binario={bin_sha}", flush=True)
        return 1
    print(f"[INFO] Imagen SHA-256 {img_sha}; binario Rust anfitrión SHA-256 {bin_sha}; sólo diagnóstico", flush=True)
    r = None
    fallos = 1
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=True, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        fallos = ejecutar_en_vm(r)
    finally:
        if r is not None:
            r.close()
            if any(v is False for v in r.results.values()):
                fallos += 1
    if sha256(imagen) != img_sha or sha256(BINARIO) != bin_sha:
        print("[FAIL] Cambió la imagen o la sonda del anfitrión", flush=True)
        fallos += 1
    else:
        print("[PASS] Imagen y sonda del anfitrión intactas", flush=True)
    print(f"T-033 TLB RESULTADO: fallos={fallos}; sondeo parcial, no certifica b203", flush=True)
    return 1 if fallos else 0


if __name__ == "__main__":
    raise SystemExit(main())
