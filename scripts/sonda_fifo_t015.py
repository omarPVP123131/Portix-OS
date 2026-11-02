#!/usr/bin/env python3
"""Sonda del cuelgue intermitente del paso FIFO de T-001 (T-015).

Repite dentro del invitado, N veces seguidas y en el mismo proceso de shell, el
patrón que se colgó una vez en la pasada 72a:

    cat fifo > out &
    echo > fifo
    wait
    cat out

La sonda vigila las marcas `[T015-FIFO] i=<n>`. Si dejan de llegar, sabe en qué
vuelta se colgó y espera un tiempo acotado antes de declarar el fallo: una
prueba que se queda esperando para siempre no informa de nada.

Uso: python3 -u scripts/sonda_fifo_t015.py [vueltas] [--img build/dist/portix.img]
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


def dentro_de_dist(p) -> bool:
    try:
        p.resolve().relative_to((ROOT / "build/dist").resolve())
        return True
    except ValueError:
        return False


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("vueltas", nargs="?", type=int, default=20)
    ap.add_argument("--img", default=str(ROOT / "build/dist/portix.img"))
    args = ap.parse_args()
    imagen = Path(args.img)
    if not dentro_de_dist(imagen) or not imagen.is_file():
        print("[FAIL] imagen fuera de build/dist o inexistente", flush=True)
        return 1

    r = None
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=False, silicon=True, conservar=False)
        # `pkg` baja los paquetes por HTTPS del anfitrión (8443) y verifica su
        # SHA-256: sin este servidor la instalación de busybox falla por red.
        r.start_https_server(port=8443)
        r.start_http_server(port=8000)
        r.start_qemu()
        ok, _ = r.wait_for_pattern("init process PID=1 registered", timeout=90)
        if not ok:
            print("[FAIL] no arrancó init", flush=True)
            return 1
        # busybox va como paquete (no vive en el rootfs recién construido): la
        # propia suite lo instala con `pkg install busybox-static`.
        if not r.execute_and_verify(
                "run pkg install busybox-static", "[+] SHA-256 verificado: ",
                "instalación de busybox-static (Alpine, sin modificar)", timeout=90):
            print("[FAIL] no se pudo instalar busybox-static", flush=True)
            return 1
        idx = r.get_log_len()
        t0 = time.time()
        r.send_command(f"run /bin/busybox sh /usr/src/prueba_t015.sh {args.vueltas}")
        fin, _ = r.wait_for_pattern("[T015-FIFO-PASS]", start_idx=idx, timeout=20 * args.vueltas + 60)
        t1 = time.time()
        with r.log_lock:
            texto = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
        iniciadas = [int(m) for m in re.findall(r"\[T015-FIFO\] i=(\d+) inicia", texto)]
        completas = re.findall(r"\[T015-FIFO\] i=(\d+) ok leido=(\S+)", texto)
        distintas = re.findall(r"\[T015-FIFO\] i=(\d+) DATO DISTINTO", texto)
        print(f"[INFO] pared={t1 - t0:.1f} s; vueltas iniciadas={len(iniciadas)} "
              f"completas={len(completas)} datos_distintos={len(distintas)} fin={fin}", flush=True)
        if iniciadas:
            print(f"[INFO] última vuelta iniciada={iniciadas[-1]}", flush=True)
        if not fin:
            # Sin marca final: ¿dónde se quedó?
            ultima = iniciadas[-1] if iniciadas else 0
            print(f"[FAIL] el paso FIFO se colgó en la vuelta {ultima} "
                  f"(iniciadas={len(iniciadas)} completas={len(completas)})", flush=True)
            print(texto[-900:], flush=True)
            return 1
        if len(completas) != args.vueltas:
            print(f"[FAIL] sólo {len(completas)} de {args.vueltas} vueltas leyeron el dato correcto", flush=True)
            return 1
        if distintas:
            print(f"[FAIL] vueltas con dato distinto: {distintas}", flush=True)
            return 1
        print(f"[PASS] {args.vueltas} vueltas del paso FIFO de T-001 sin un solo cuelgue", flush=True)
        return 0
    finally:
        if r is not None:
            try:
                r.close()
            except Exception:  # noqa: BLE001
                pass


if __name__ == "__main__":
    sys.exit(main())
