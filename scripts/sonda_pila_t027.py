#!/usr/bin/env python3
"""Sonda de la pila de hilo (T-027): guarda + madvise + escritura, 200 vueltas.

El fixture `t027_pila` reserva una pila con `mmap(PROT_NONE)`, hace escribible el
tramo útil con `mprotect`, escribe en todas sus páginas, baja hasta el fondo con
marcos reales (el `push rbp` que reventaba en HotSpot), recicla con
`madvise(MADV_DONTNEED)` como hace glibc y vuelve a escribir. Cualquier SIGSEGV,
pérdida de escritura o `madvise` fallido se cuenta y la sonda falla.

Uso: python3 -u scripts/sonda_pila_t027.py [vueltas] [--firmware bios|uefi]
"""
import argparse
import os
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from headless_runner import PortixHeadlessRunner  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
BINARIO = ROOT / "build/packages/t027_pila"


def dentro_de_dist(p) -> bool:
    try:
        p.resolve().relative_to((ROOT / "build/dist").resolve())
        return True
    except ValueError:
        return False


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("vueltas", nargs="?", type=int, default=200)
    ap.add_argument("--firmware", choices=("bios", "uefi"), default="bios")
    ap.add_argument("--img", default=None)
    args = ap.parse_args()
    imagen = Path(args.img) if args.img else ROOT / ("build/dist/portix-uefi.img" if args.firmware == "uefi" else "build/dist/portix.img")
    if not dentro_de_dist(imagen) or not imagen.is_file():
        print(f"[FAIL] imagen fuera de build/dist o inexistente: {imagen}", flush=True)
        return 1
    if not BINARIO.is_file():
        print("[FAIL] falta build/packages/t027_pila (rustc +crt-static)", flush=True)
        return 1

    if args.firmware == "uefi":
        # La ruta UEFI de esta sonda es la de T-035: misma imagen OVMF y mismo
        # arranque; si no hay OVMF se FALLA en vez de fingir una prueba UEFI.
        print("[FAIL] esta sonda sólo ejecuta BIOS; la paridad UEFI se certifica con "
              "scripts/sonda_t035_uefi.py (mismos modos de memoria)", flush=True)
        return 1

    r = None
    try:
        r = PortixHeadlessRunner(str(imagen), is_iso=False, verbose=False, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        ok, _ = r.wait_for_pattern("init process PID=1 registered", timeout=120 if args.firmware == "uefi" else 90)
        if not ok:
            print("[FAIL] no arrancó init", flush=True)
            return 1
        if not r.execute_and_verify(
                "run curl -o /usr/local/t027 http://10.0.2.2:8000/t027_pila",
                f"guardado exitosamente en '/usr/local/t027' ({BINARIO.stat().st_size} bytes)",
                "descarga del fixture de pila", timeout=90):
            print("[FAIL] no se pudo descargar el fixture", flush=True)
            return 1
        idx = r.get_log_len()
        t0 = time.time()
        r.send_command(f"run /usr/local/t027 {args.vueltas}")
        fin, _ = r.wait_for_pattern("[T027-PASS]", start_idx=idx, timeout=300)
        fallo, _ = r.wait_for_pattern("[T027-FAIL]", start_idx=idx, timeout=1)
        t1 = time.time()
        with r.log_lock:
            texto = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
        segv = re.findall(r"\[SIG\] SIGSEGV[^\n]*PID=(\d+)[^\n]*CR2=(0x[0-9a-f]+)", texto)
        vueltas_ok = len(re.findall(r"\[T027\] vuelta (\d+)/", texto))
        print(f"[INFO] {args.firmware.upper()} pared={t1 - t0:.1f} s marcas_de_progreso={vueltas_ok} "
              f"fin={fin} fallo={fallo} SIGSEGV={len(segv)}", flush=True)
        if segv:
            print(f"[FAIL] SIGSEGV durante la prueba: {segv[:4]}", flush=True)
            return 1
        if not fin or fallo:
            print("[FAIL] el fixture falló o no terminó", flush=True)
            print(texto[-800:], flush=True)
            return 1
        print(f"[PASS] {args.firmware.upper()}: {args.vueltas} vueltas de pila + guarda + madvise + escritura, "
              f"ningún SIGSEGV ni escritura perdida", flush=True)
        return 0
    finally:
        if r is not None:
            try:
                r.close()
            except Exception:  # noqa: BLE001
                pass


if __name__ == "__main__":
    sys.exit(main())
