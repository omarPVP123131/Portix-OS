#!/usr/bin/env python3
"""Reproductor acotado de T-045 (consola inundada con memoria cruda).

Arranca UNA vez el invitado y repite en bucle los dos bloques que han fallado
(`dbg --ortest` y `dbg --buftest`). Tras cada vuelta mide cuántos bytes no
imprimibles ha recibido la serie y cuántos marcadores `[SERIE-BULTO]` (escritura
>= 256 B, con pid y punto de llamada) han aparecido. Se detiene en la primera
vuelta sucia y guarda la serie completa.

Uso:  python3 -u scripts/repro_t045.py [vueltas] [--img build/dist/portix.img]
"""
import os
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from headless_runner import PortixHeadlessRunner  # noqa: E402


def sucio(texto: str) -> tuple:
    b = texto.encode("latin1", "replace")
    no_imp = sum(1 for c in b if c < 32 and c not in (9, 10, 13))
    return b.count(0), no_imp


def main():
    vueltas = 12
    img = "build/dist/portix.img"
    args = sys.argv[1:]
    if args and args[0].isdigit():
        vueltas = int(args[0])
    if "--img" in args:
        img = args[args.index("--img") + 1]

    r = PortixHeadlessRunner(img, is_iso=False, verbose=False)
    r.start_qemu()
    ok, log = r.wait_for_pattern("present done", timeout=90.0)
    if ok:
        ok, log = r.wait_for_pattern("init process PID=1 registered", timeout=30.0)
    print(f"[REPRO] arranque: {ok}")
    if not ok:
        print(log[-2000:])
        r.close()
        return 2
    time.sleep(0.5)
    idx0 = r.get_log_len()
    r.send_command("")
    prompt, _ = r.wait_for_pattern("root@portix:", start_idx=idx0, timeout=25.0)
    print(f"[REPRO] consola interactiva: {prompt}")
    if not prompt:
        r.close()
        return 2

    bloques = [("run dbg --ortest hello", "[SESSION-OR-PASS]"),
               ("run dbg --buftest hello", "[SESSION-BUF-PASS]")]
    for v in range(1, vueltas + 1):
        for orden, patron in bloques:
            idx = r.get_log_len()
            r.send_command(orden)
            ok, out = r.wait_for_pattern(patron, start_idx=idx, timeout=120.0)
            r.send_command("")
            time.sleep(0.5)
            with r.log_lock:
                serie = r.log
            ceros, no_imp = sucio(serie)
            bultos = re.findall(r"\[SERIE-BULTO\][^\n]*", serie)
            print(f"[REPRO] vuelta {v} {orden.split()[-1]:<8} ok={ok} "
                  f"NUL={ceros} no_impr={no_imp} bultos={len(bultos)}")
            if bultos:
                for b in bultos[:8]:
                    print("        " + b.strip()[:150])
            if ceros or no_imp > 400:
                
                with r.log_lock:
                    try:
                        with open("build/repro_t045_serie.log", "w",
                                  encoding="latin1", errors="replace") as f:
                            f.write(r.log)
                        print("[REPRO] serie guardada en build/repro_t045_serie.log")
                    except Exception as e:  # noqa: BLE001
                        print("no pude guardar la serie:", e)
                r.close()
                return 1
    r.close()
    print(f"[REPRO] {vueltas} vueltas limpias: el defecto no apareció")
    return 0


if __name__ == "__main__":
    sys.exit(main())
