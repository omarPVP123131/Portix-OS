#!/usr/bin/env python3
"""Resumen fiable de un log del ejecutor headless (cuenta con o sin color).

`headless_runner.py` colorea el veredicto (`[\\x1b[31mFAIL\\x1b[0m]`), así que un
`grep -c "[FAIL]"` sobre el log da 0 aunque haya fallos: hay que quitar los
códigos ANSI antes de contar. Este script lo hace y además extrae las líneas de
T-026 (`[IRQ0-HUECO]`, `ventanas_if0`, `irq0_*`) y el veredicto final.

Uso:
    python3 scripts/resumen_suite.py build/suite_b203_1.log [--fallos N]
"""
import argparse
import re
import sys
from pathlib import Path

ANSI = re.compile(r"\x1b\[[0-9;]*m")


def leer(ruta):
    return ANSI.sub("", Path(ruta).read_bytes().decode("utf-8", "replace").replace("\r\n", "\n"))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("logs", nargs="+", type=Path)
    ap.add_argument("--fallos", type=int, default=25, help="cuántos fallos listar por log")
    ap.add_argument("--t026", action="store_true", help="mostrar también las líneas de T-026")
    args = ap.parse_args()
    salida = 0
    for ruta in args.logs:
        if not ruta.is_file():
            print(f"[FAIL] no existe {ruta}")
            salida = 1
            continue
        t = leer(ruta)
        pas = len(re.findall(r"✓", t))
        fal = len(re.findall(r"✗", t))
        print(f"== {ruta.name}: PASS={pas} FAIL={fal}")
        for l in t.splitlines():
            if "✗" in l:
                print(f"   ✗ {l.split('✗', 1)[1].strip()[:140]}")
                if args.fallos <= 0:
                    break
                args.fallos -= 1
        for l in t.splitlines():
            if re.match(r"\s*TOTAL:", l.strip()):
                print(f"   {l.strip()}")
            elif "EXIT=" in l:
                print(f"   {l.strip()}")
        if args.t026:
            for l in t.splitlines():
                if "IRQ0-HUECO" in l or "ventanas_if0" in l or re.search(r"irq0_(peor_hueco|hueco_dentro|perdidos)", l):
                    print(f"   [T-026] {l.strip()[:150]}")
    return salida


if __name__ == "__main__":
    sys.exit(main())
