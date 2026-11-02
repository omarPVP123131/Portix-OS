#!/usr/bin/env python3
"""Inventario de las dos ABI de syscalls de Portix, extraído del CÓDIGO.

No contiene listas escritas a mano: lee `kernel/src/syscall/numbers.rs` (tabla
nativa), `kernel/src/syscall/mod.rs` (brazos del traductor Linux) y la cabecera
de Linux del anfitrión para poner nombres a los números.

Uso:
    python3 scripts/abi_inventario.py            # resumen
    python3 scripts/abi_inventario.py --tablas    # tablas markdown
    python3 scripts/abi_inventario.py --json      # datos para otros scripts
"""
import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
NUMEROS = ROOT / "kernel/src/syscall/numbers.rs"
DESPACHO = ROOT / "kernel/src/syscall/mod.rs"
CABECERAS = (
    "/usr/include/x86_64-linux-gnu/asm/unistd_64.h",
    "/usr/include/asm/unistd_64.h",
    "/usr/include/asm-generic/unistd.h",
)


def _lineas():
    return DESPACHO.read_text().split("\n")


def _cuerpo_funcion(prefijo):
    """Líneas de la función que empieza por `prefijo`, contando llaves.

    Se ignoran comentarios y cadenas antes de contar para que un `{` dentro de un
    comentario no descuadre el rango.
    """
    lineas = _lineas()
    i = next(k for k, l in enumerate(lineas) if l.startswith(prefijo))
    prof, fin, abierto = 0, None, False
    for k in range(i, len(lineas)):
        l = re.sub(r"//.*$", "", lineas[k])
        l = re.sub(r'"(?:[^"\\]|\\.)*"', '""', l)
        prof += l.count("{") - l.count("}")
        abierto = abierto or prof > 0
        if abierto and prof == 0:
            fin = k
            break
    if fin is None:
        raise RuntimeError(f"no se pudo delimitar la función {prefijo}")
    return lineas[i:fin + 1], i + 1, fin + 1


def tabla_nativa():
    """{nombre: numero} del dialecto nativo de Portix."""
    nat = {}
    for l in NUMEROS.read_text().split("\n"):
        m = re.match(r"pub const (SYS_\w+)\s*:\s*u64\s*=\s*(\d+);", l.strip())
        if m:
            nat[m.group(1)] = int(m.group(2))
    return nat


def nativa_despachada():
    """Constantes SYS_ con brazo en el despachador nativo."""
    cuerpo, _, _ = _cuerpo_funcion("fn syscall_dispatch_core")
    return sorted(set(re.findall(r"^\s*(SYS_[A-Z0-9_]+)\s*=>", "\n".join(cuerpo), re.M)))


def linux_traducidas():
    """Números de Linux con brazo explícito en el traductor."""
    cuerpo, ini, fin = _cuerpo_funcion("fn linux_syscall_match")
    texto = "\n".join(cuerpo)
    nums = sorted({int(n) for m in re.finditer(r"^\s*(\d+(?:\s*\|\s*\d+)*)\s*=>", texto, re.M)
                   for n in re.split(r"\s*\|\s*", m.group(1))})
    return nums, ini, fin


def tabla_linux_referencia():
    """{numero: nombre} de la ABI Linux x86-64 del anfitrión."""
    for ruta in CABECERAS:
        p = Path(ruta)
        if not p.is_file():
            continue
        ref = {}
        for l in p.read_text().split("\n"):
            m = re.match(r"#define\s+__NR_(\w+)\s+(\d+)", l)
            if m:
                ref[int(m.group(2))] = m.group(1)
        if ref:
            return ref
    raise RuntimeError("no se encontró la cabecera unistd_64.h del anfitrión")


def inventario():
    nat = tabla_nativa()
    desp = set(nativa_despachada())
    impl, ini, fin = linux_traducidas()
    ref = tabla_linux_referencia()
    faltan = sorted(set(ref) - set(impl))
    # Números nativos que en Linux designan OTRA llamada: el riesgo real.
    choques = []
    for nombre, n in sorted(nat.items(), key=lambda x: x[1]):
        if n in ref:
            propio = nombre.replace("SYS_", "").lower()
            parecido = propio == ref[n] or propio in ref[n] or ref[n] in propio
            if not parecido:
                choques.append([n, nombre, ref[n]])
    return {
        "nativa": {"constantes": nat, "con_brazo": sorted(desp),
                   "sin_brazo": sorted(set(nat) - desp)},
        "linux": {"traducidas": impl, "faltan": faltan, "total_linux": len(ref),
                  "lineas": [ini, fin]},
        "referencia": ref,
        "choques": choques,
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--json", action="store_true", help="volcado completo en JSON")
    ap.add_argument("--tablas", action="store_true", help="tablas markdown")
    args = ap.parse_args()
    inv = inventario()
    if args.json:
        print(json.dumps(inv, ensure_ascii=False))
        return 0
    nat, lin = inv["nativa"], inv["linux"]
    if args.tablas:
        pares = sorted(nat["constantes"].items(), key=lambda x: x[1])
        for k in range(0, len(pares), 4):
            print("| " + " | ".join(f"`{n:3d}` {c.replace('SYS_', '')}"
                                    for c, n in pares[k:k + 4]) + " |")
        print()
        for n in lin["traducidas"]:
            print(f"| {n} | `{inv['referencia'].get(n, '?')}` | |")
        return 0
    print(f"Nativa : {len(nat['constantes'])} constantes, "
          f"{len(nat['con_brazo'])} con brazo, sin brazo: "
          f"{', '.join(nat['sin_brazo']) or 'ninguna'}")
    print(f"Linux  : {len(lin['traducidas'])} traducidas de {lin['total_linux']}; "
          f"sin traducir {len(lin['faltan'])}")
    print(f"Choques: {len(inv['choques'])} números nativos que en Linux son otra llamada")
    return 0


if __name__ == "__main__":
    sys.exit(main())
