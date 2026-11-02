#!/usr/bin/env python3
"""Reproducción aislada de T-026/b203: líder en futex + exit_group desde otro CPU.

Caso real que dejó viva a la JVM y sorda la consola: el líder del grupo duerme
en un FUTEX_WAIT sin plazo, y un hilo del mismo grupo llama a `exit_group` desde
la otra CPU. El grupo entero debe morir con el código pedido y la consola debe
recuperar el control. Si el líder no se cierra, el `run` de la consola espera
para siempre y la suite entera cae en cascada.

No certifica nada: es una prueba acotada de regresión.
"""
import argparse
import time
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
    from .rutas_imagenes import dentro_de_dist
else:
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import dentro_de_dist

ROOT = Path(__file__).resolve().parents[1]
BINARIO = ROOT / "build/packages/t034_exit_group"
MODO = "futex-lider"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--img", required=True, type=Path)
    ap.add_argument("--limite", type=float, default=25.0, help="segundos de plazo para que el grupo muera")
    args = ap.parse_args()
    imagen = args.img.resolve()
    if not dentro_de_dist(imagen) or not imagen.is_file():
        print("[FAIL] imagen fuera de build/dist o inexistente", flush=True)
        return 1
    if not BINARIO.is_file():
        print(f"[FAIL] falta {BINARIO} (rustc -O -C target-feature=+crt-static scripts/sondas/t034_exit_group.rs)", flush=True)
        return 1

    r = None
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=False, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        ok, _ = r.wait_for_pattern("init process PID=1 registered", timeout=60)
        if not ok:
            print("[FAIL] no arrancó init", flush=True)
            return 1
        size = BINARIO.stat().st_size
        if not r.execute_and_verify(
                "run curl -o /usr/local/t034 http://10.0.2.2:8000/t034_exit_group",
                f"guardado exitosamente en '/usr/local/t034' ({size} bytes)",
                "descarga del binario de prueba", timeout=60):
            print("[FAIL] no se pudo descargar el binario", flush=True)
            return 1

        # 1) El caso que falla: líder en futex, exit_group desde el otro CPU.
        idx = r.get_log_len()
        r.send_command(f"run /usr/local/t034 {MODO}")
        t0 = time.monotonic()
        muerto = False
        texto = ""
        while time.monotonic() - t0 < args.limite:
            with r.log_lock:
                texto = r.log[idx:]
            if "PROC: exit" in texto and "code=7" in texto:
                muerto = True
                break
            time.sleep(0.1)
        dt = time.monotonic() - t0
        if not muerto:
            with r.log_lock:
                texto = r.log[idx:]
            print(f"[FAIL] T-026/{MODO}: el grupo NO murió en {dt:.1f} s "
                  f"(el líder quedó vivo y la consola espera)\n--- serie ---\n{texto[-1200:]}", flush=True)
            try:
                r._stdin_write(b"\x1d"); r.proc.stdin.flush()
            except Exception:
                pass
            return 1
        print(f"[PASS] T-026/{MODO}: el grupo murió con code=7 en {dt:.1f} s", flush=True)

        # 2) La consola tiene que volver a atender órdenes de verdad.
        volvio, _ = r.wait_for_pattern("root@portix:", start_idx=idx, timeout=15)
        if not volvio:
            print("[FAIL] la consola no devolvió el prompt tras el cierre del grupo", flush=True)
            return 1
        eco = r.execute_and_verify("run /bin/true", "[OK] Proceso PID", "la consola sigue viva tras el cierre", timeout=20)
        print(f"[{'PASS' if eco else 'FAIL'}] consola viva después del cierre del grupo", flush=True)
        return 0 if eco else 1
    finally:
        if r is not None:
            r.close()


if __name__ == "__main__":
    raise SystemExit(main())
