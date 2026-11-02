#!/usr/bin/env python3
"""Comprueba sin modificar la imagen BIOS construida (T-029/T-030).

Contrasta por SHA-256 la partición ext2 temporal con la P3 de la imagen final,
y consulta inodos reales con debugfs. No constituye un arranque QEMU.
"""
import hashlib
import os
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DISCO = ROOT / "build/dist/portix.img"
PARTICION = ROOT / "build/dist/.trabajo/portix-ext2-part.img"
PRESENTES = ("/bin/hello", "/bin/calc", "/bin/pkg")
AUSENTES = ("/bin/hello_c", "/bin/calc_c", "/bin/doom", "/bin/gcc", "/bin/cc",
            "/bin/rustc", "/bin/cargo", "/bin/portix-cc", "/usr/bin/gcc", "/usr/bin/cc",
            "/usr/bin/rustc", "/usr/bin/cargo", "/usr/bin/tcc", "/usr/lib/libc.a")


def localizar_ext2(mbr, tam_disco):
    if len(mbr) != 512 or mbr[510:512] != b"\x55\xaa":
        raise ValueError("firma MBR inválida")
    entrada = mbr[0x1BE + 2 * 16:0x1BE + 3 * 16]
    inicio, sectores = struct.unpack_from("<II", entrada, 8)
    if entrada[4] != 0x83 or not inicio or not sectores:
        raise ValueError("P3 no es una partición Linux válida")
    offset, tamano = inicio * 512, sectores * 512
    if offset + tamano > tam_disco:
        raise ValueError("P3 excede el tamaño del disco")
    return offset, tamano


def huella_en_flujo(ruta, offset, tamano):
    h = hashlib.sha256()
    with open(ruta, "rb") as f:
        f.seek(offset)
        restante = tamano
        while restante:
            bloque = f.read(min(restante, 8 * 1024 * 1024))
            if not bloque:
                raise ValueError(f"imagen truncada: {ruta}")
            h.update(bloque)
            restante -= len(bloque)
    return h.hexdigest()


def estado_inode(codigo, texto, ruta):
    if codigo != 0:
        return "error"
    if re.search(r"^Inode:\s*[1-9]\d*\s+Type:\s*regular\b", texto, re.MULTILINE):
        return "regular"
    if f"{ruta}: File not found by ext2_lookup" in texto:
        return "ausente"
    return "error"


def main():
    debugfs = shutil.which("debugfs") or next((p for p in ("/usr/sbin/debugfs", "/sbin/debugfs") if Path(p).is_file()), None)
    if not debugfs or not DISCO.is_file() or not PARTICION.is_file():
        print(f"[FAIL] debugfs={debugfs} disco={DISCO.is_file()} particion={PARTICION.is_file()}")
        return 1
    fallos = 0
    try:
        with DISCO.open("rb") as f:
            offset, tamano = localizar_ext2(f.read(512), DISCO.stat().st_size)
        if PARTICION.stat().st_size != tamano:
            raise ValueError("el tamaño de la P3 no coincide con la partición temporal")
        h_part = huella_en_flujo(PARTICION, 0, tamano)
        h_disco = huella_en_flujo(DISCO, offset, tamano)
        iguales = h_part == h_disco
        print(f"[{'PASS' if iguales else 'FAIL'}] P3 del disco final y ext2 temporal: SHA-256 {h_part} / {h_disco}")
        if not iguales:
            fallos += 1
    except (OSError, ValueError) as e:
        print(f"[FAIL] No se pudo comparar P3: {e}")
        return 1
    env = dict(os.environ, LC_ALL="C")
    for ruta, esperado in [(p, "regular") for p in PRESENTES] + [(p, "ausente") for p in AUSENTES]:
        try:
            p = subprocess.run([debugfs, "-R", f"stat {ruta}", str(PARTICION)],
                               capture_output=True, text=True, timeout=10, env=env)
            observado = estado_inode(p.returncode, p.stdout + p.stderr, ruta)
        except (OSError, subprocess.TimeoutExpired) as e:
            observado = f"error ({e})"
        ok = observado == esperado
        print(f"[{'PASS' if ok else 'FAIL'}] {ruta}: esperado={esperado}, observado={observado}")
        if not ok:
            fallos += 1
    print(f"SONDA IMAGEN T-029: {len(PRESENTES) + len(AUSENTES) + 1 - fallos}/{len(PRESENTES) + len(AUSENTES) + 1} PASS; fallos={fallos}")
    return 1 if fallos else 0


if __name__ == "__main__":
    sys.exit(main())
