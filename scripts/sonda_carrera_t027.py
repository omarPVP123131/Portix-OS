#!/usr/bin/env python3
"""Sonda de la carrera mprotect ↔ fallo de página (T-027).

Ejecuta `t027_carrera`, que desde dos hilos (uno por CPU en SMP) alterna
`mprotect` sobre un tramo perezoso mientras el otro escribe en él, y comprueba:

  * que no hay ni un SIGSEGV por «protección perdida» (el síntoma de T-027);
  * cuántas carreras detectó y resolvió el núcleo (`pte: carreras` de /proc/smp).

Uso: python3 -u scripts/sonda_carrera_t027.py [vueltas]
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
BINARIO = ROOT / "build/packages/t027_carrera"


def dentro_de_dist(p) -> bool:
    try:
        p.resolve().relative_to((ROOT / "build/dist").resolve())
        return True
    except ValueError:
        return False


def leer_carreras(r) -> int:
    idx = r.get_log_len()
    r.send_command("cat /proc/smp")
    r.wait_for_pattern("pte: carreras ", start_idx=idx, timeout=20)
    with r.log_lock:
        trozo = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
    m = re.search(r"pte: carreras (\d+)", trozo)
    return int(m.group(1)) if m else -1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("vueltas", nargs="?", type=int, default=400)
    ap.add_argument("--img", default=str(ROOT / "build/dist/portix.img"))
    args = ap.parse_args()
    imagen = Path(args.img)
    if not dentro_de_dist(imagen) or not imagen.is_file():
        print("[FAIL] imagen fuera de build/dist o inexistente", flush=True)
        return 1
    if not BINARIO.is_file():
        print("[FAIL] falta build/packages/t027_carrera (rustc +crt-static)", flush=True)
        return 1

    r = None
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=False, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        ok, _ = r.wait_for_pattern("init process PID=1 registered", timeout=90)
        if not ok:
            print("[FAIL] no arrancó init", flush=True)
            return 1
        if not r.execute_and_verify(
                "run curl -o /usr/local/t027c http://10.0.2.2:8000/t027_carrera",
                f"guardado exitosamente en '/usr/local/t027c' ({BINARIO.stat().st_size} bytes)",
                "descarga del estresor de la carrera", timeout=90):
            print("[FAIL] no se pudo descargar el fixture", flush=True)
            return 1
        antes = leer_carreras(r)
        idx = r.get_log_len()
        t0 = time.time()
        r.send_command(f"run /usr/local/t027c {args.vueltas}")
        fin, _ = r.wait_for_pattern("[T027-CARRERA-PASS]", start_idx=idx, timeout=180)
        fallo, _ = r.wait_for_pattern("[T027-CARRERA-FAIL]", start_idx=idx, timeout=1)
        t1 = time.time()
        despues = leer_carreras(r)
        with r.log_lock:
            texto = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
        segv = re.findall(r"SIGSEGV", texto)
        print(f"[INFO] pared={t1 - t0:.1f} s fin={fin} fallo={fallo} SIGSEGV_en_la_corrida={len(segv)}", flush=True)
        print(f"[INFO] carreras de PTE: antes={antes} despues={despues} "
              f"(las detectadas y resueltas por el núcleo)", flush=True)
        if not fin or fallo:
            print("[FAIL] el estresor falló: una protección se perdió", flush=True)
            print(texto[-700:], flush=True)
            return 1
        if antes < 0 or despues < 0:
            print("[FAIL] /proc/smp no publica `pte: carreras`", flush=True)
            return 1
        print(f"[PASS] {args.vueltas} vueltas de mprotect contra fallo de página concurrente: "
              f"ninguna protección perdida; carreras resueltas={despues - antes}", flush=True)
        return 0
    finally:
        if r is not None:
            try:
                r.close()
            except Exception:  # noqa: BLE001
                pass


if __name__ == "__main__":
    sys.exit(main())
