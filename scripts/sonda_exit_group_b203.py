#!/usr/bin/env python3
"""Regresión T-034 en QEMU: códigos de exit_group y señales, sin certificar suite."""
import argparse
import time
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
    from .rutas_imagenes import dentro_de_dist
    from .verificaciones_t034 import resultado
else:
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import dentro_de_dist
    from verificaciones_t034 import resultado

ROOT = Path(__file__).resolve().parents[1]
BINARIO = ROOT / "build/packages/t034_exit_group"
CASOS = (("directo", 0), ("directo", 42), ("sigkill", 0), ("wait", 0), ("wait", 42),
         ("dormido", 0), ("dormido", 42), ("sigkill-hilos", 0), ("wait-dormido", 0), ("wait-dormido", 42))


def probar(r, modo, valor=0, limite=50):
    inicio = r.get_log_len()
    comando = f"run /usr/local/t034 {modo}" + (f" {valor}" if modo not in ("sigkill", "sigkill-hilos") else "")
    r.send_command(comando)
    t0 = time.monotonic()
    ok, termino, detalle = False, False, "sin salida"
    texto = ""
    while time.monotonic() - t0 < limite:
        with r.log_lock:
            texto = r.log[inicio:]
        ok, termino, detalle = resultado(texto, modo, valor)
        if termino or r.proc is None or r.proc.poll() is not None:
            break
        time.sleep(0.1)
    print(f"[{'PASS' if ok else 'FAIL'}] T-034 {modo} {valor}: {detalle}; {time.monotonic()-t0:.1f} s", flush=True)
    if not termino:
        try:
            r._stdin_write(b"\x1d")
            r.proc.stdin.flush()
        except Exception:
            pass
        return False, False
    pos = texto.rfind("PROC: exit PID=")
    listo, _ = r.wait_for_pattern("root@portix:", start_idx=inicio + max(0, pos), timeout=10)
    if not listo:
        print("[FAIL] No regresó la consola después de la salida", flush=True)
    return ok and listo, listo


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--img", type=Path, default=ROOT / "build/dist/portix.img")
    ap.add_argument("--solo", choices=sorted({modo for modo, _ in CASOS}), help="ejecutar sólo los casos del modo indicado")
    args = ap.parse_args()
    casos = tuple(caso for caso in CASOS if not args.solo or caso[0] == args.solo)
    imagen = args.img.resolve()
    if not dentro_de_dist(imagen) or not imagen.is_file() or not BINARIO.is_file():
        print("[FAIL] Falta imagen bajo dist o binario Rust de prueba", flush=True)
        return 1
    print("[INFO] Prueba acotada de regresión; no es suite ni recertificación.", flush=True)
    r = None
    fallos = 0
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=True, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        if r.httpd is None:
            print("[FAIL] No arrancó el servidor de la prueba", flush=True)
            return 1
        r.start_qemu()
        init, _ = r.wait_for_pattern("init process PID=1 registered", timeout=40)
        if not init:
            print("[FAIL] No arrancó init", flush=True)
            return 1
        size = BINARIO.stat().st_size
        descargo = r.execute_and_verify(
            "run curl -o /usr/local/t034 http://10.0.2.2:8000/t034_exit_group",
            f"guardado exitosamente en '/usr/local/t034' ({size} bytes)",
            "T-034: binario Rust descargado al ext2", timeout=45)
        if not descargo:
            return 1
        for modo, valor in casos:
            ok, listo = probar(r, modo, valor)
            fallos += not ok
            if not listo:
                print("[INCOMPLETA] No se envían órdenes adicionales a una consola ocupada", flush=True)
                return 1
    finally:
        if r is not None:
            r.close()
            if any(v is False for v in r.results.values()):
                fallos += 1
    print(f"T-034 RESULTADO: {len(casos)-fallos}/{len(casos)} PASS; {fallos} FAIL; no certifica b203.", flush=True)
    return 1 if fallos else 0


if __name__ == "__main__":
    raise SystemExit(main())
