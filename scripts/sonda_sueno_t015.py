#!/usr/bin/env python3
"""Sonda de la precisión de los sueños (T-015 / retornos cortos de nanosleep).

El fixture `t015_sueno` duerme 50 veces 100 ms y compara el reloj del núcleo
antes y después de cada vuelta; el núcleo, por su parte, cuenta en
`/proc/smp` (`suenos: totales … cortos … peor_deficit_ticks …`) los sueños que
terminaron con el plazo sin vencer y sin señal. La sonda exige que las DOS
vistas digan lo mismo: ni un retorno corto, ni una vuelta sin avanzar.

Uso:  python3 -u scripts/sonda_sueno_t015.py [--img build/dist/portix.img]
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
BINARIO = ROOT / "build/packages/t015_sueno"


def dentro_de_dist(p) -> bool:
    try:
        p.resolve().relative_to((ROOT / "build/dist").resolve())
        return True
    except ValueError:
        return False


def leer(r, clave: str):
    idx = r.get_log_len()
    r.send_command("cat /proc/smp")
    r.wait_for_pattern("suenos: totales", start_idx=idx, timeout=20)
    with r.log_lock:
        trozo = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
    m = re.search(rf"\b{re.escape(clave)} (\d+)", trozo)
    return int(m.group(1)) if m else None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--img", default=str(ROOT / "build/dist/portix.img"))
    args = ap.parse_args()
    imagen = Path(args.img)
    if not dentro_de_dist(imagen) or not imagen.is_file():
        print("[FAIL] imagen fuera de build/dist o inexistente", flush=True)
        return 1
    if not BINARIO.is_file():
        print("[FAIL] falta build/packages/t015_sueno (rustc +crt-static)", flush=True)
        return 1

    r = None
    fallos = []
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=False, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        ok, _ = r.wait_for_pattern("init process PID=1 registered", timeout=90)
        if not ok:
            print("[FAIL] no arrancó init", flush=True)
            return 1
        if not r.execute_and_verify(
                "run curl -o /usr/local/t015 http://10.0.2.2:8000/t015_sueno",
                f"guardado exitosamente en '/usr/local/t015' ({BINARIO.stat().st_size} bytes)",
                "descarga del fixture de sueños", timeout=60):
            print("[FAIL] no se pudo descargar el fixture", flush=True)
            return 1

        cortos0 = leer(r, "cortos") or 0
        idx = r.get_log_len()
        t0 = time.time()
        r.send_command("run /usr/local/t015 50 100")
        fin, _ = r.wait_for_pattern("[T015-PASS]", start_idx=idx, timeout=180)
        fail, _ = r.wait_for_pattern("[T015-FAIL]", start_idx=idx, timeout=1)
        t1 = time.time()
        with r.log_lock:
            texto = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
        m = re.search(r"\[T015-STATS\] ceros=(\d+) cortos=(\d+) menor_delta=(\d+) mayor_delta=(\d+) "
                      r"media=(\d+) cortos_kernel=(\d+) peor_deficit=(\d+)", texto)
        if not m:
            print("[FAIL] el fixture no dejó estadísticas (¿se colgó durmiendo?)", flush=True)
            print(texto[-700:])
            return 1
        ceros, cortos, menor, mayor, media, en_kernel, deficit = (int(x) for x in m.groups())
        cortos_totales = leer(r, "cortos") or 0
        print(f"[INFO] vueltas medidas; ceros={ceros} cortos={cortos} menor={menor} mayor={mayor} "
              f"media={media} ticks ({media * 10} ms)", flush=True)
        print(f"[INFO] pared={t1 - t0:.2f} s; el núcleo declara cortos={cortos_totales} "
              f"(antes de la corrida {cortos0}), peor_déficit={deficit} ticks", flush=True)
        if en_kernel != cortos_totales - cortos0:
            fallos.append(f"las dos vistas no coinciden: fixture={en_kernel} kernel={cortos_totales - cortos0}")
        if ceros:
            fallos.append(f"{ceros} sueños sin avanzar ni un tick")
        if cortos:
            fallos.append(f"{cortos} sueños volvieron antes de tiempo")
        if cortos_totales > cortos0:
            fallos.append(f"el núcleo contó {cortos_totales - cortos0} retornos cortos")
        if not fin or fail:
            fallos.append("el fixture declaró el fallo explícitamente")
        # 50 vueltas de 100 ms son ~5 s de pared: menos de 3 s significa que durmió de menos.
        if (t1 - t0) < 3.0:
            fallos.append(f"50 sueños de 100 ms en sólo {t1 - t0:.2f} s de pared")

        if fallos:
            print("[FAIL] " + "; ".join(fallos), flush=True)
            return 1
        print("[PASS] 50 sueños de 100 ms: ninguno volvió antes, y el núcleo y el fixture coinciden", flush=True)
        return 0
    finally:
        if r is not None:
            try:
                r.close()
            except Exception:  # noqa: BLE001
                pass


if __name__ == "__main__":
    sys.exit(main())
