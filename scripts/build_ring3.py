#!/usr/bin/env python3
"""
build_ring3.py — Build C programs for PORTIX ring-3 and inject into disk image.

Usage:
    python scripts/build_ring3.py              # uses build/portix.img
    python scripts/build_ring3.py --img=<path>
"""

import os
import sys
import subprocess
import struct
import shutil
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BUILD = os.path.join(ROOT, "build")
LIB = os.path.join(ROOT, "lib")
TOOLCHAIN = os.path.join(BUILD, "x86_64-elf", "bin")
CC = os.path.join(TOOLCHAIN, "x86_64-elf-gcc.exe")
AS = os.path.join(TOOLCHAIN, "x86_64-elf-as.exe")
AR = os.path.join(TOOLCHAIN, "x86_64-elf-ar.exe")
LD = os.path.join(TOOLCHAIN, "x86_64-elf-ld.exe")
IMG = os.path.join(BUILD, "portix.img")

def log(*args):
    print("[R3]", *args)

def step(msg):
    print(f"\n=== {msg} ===")

def check_toolchain():
    if not os.path.exists(CC):
        log("x86_64-elf-gcc not found at", CC)
        log("Download from: https://github.com/trcrsired/windows-hosted-x86_64-elf-toolchains")
        return False
    return True

def build_libportix():
    src_dir = os.path.join(LIB, "src")
    inc_dir = os.path.join(LIB, "include")
    lib_build = os.path.join(LIB, "build")
    os.makedirs(lib_build, exist_ok=True)

    cflags = ["-ffreestanding", "-nostdlib", "-static", "-mno-red-zone",
              "-mno-mmx", "-mno-sse", "-I", inc_dir, "-O2", "-Wall", "-c"]

    files = [("crt0.s", AS, []),
             ("stdio.c", CC, cflags),
             ("stdlib.c", CC, cflags),
             ("string.c", CC, cflags)]

    for fname, tool, flags in files:
        src = os.path.join(src_dir, fname)
        obj = os.path.join(lib_build, fname.replace(".c", ".o").replace(".s", ".o"))
        cmd = [tool] + flags + ["-o", obj, src]
        subprocess.run(cmd, check=True, capture_output=True)
        log(f"  {fname} -> {obj}")

    ar_cmd = [AR, "rcs", os.path.join(lib_build, "libportix.a"),
              os.path.join(lib_build, "stdio.o"),
              os.path.join(lib_build, "stdlib.o"),
              os.path.join(lib_build, "string.o"),
              os.path.join(lib_build, "crt0.o")]
    subprocess.run(ar_cmd, check=True, capture_output=True)
    log(f"  libportix.a built")
    return lib_build

def compile_c(source, output, lib_build):
    inc_dir = os.path.join(LIB, "include")
    cflags = ["-ffreestanding", "-nostdlib", "-static", "-mno-red-zone",
              "-mno-mmx", "-mno-sse", "-I", inc_dir, "-O2", "-Wall", "-c"]
    obj = source + ".o"
    subprocess.run([CC] + cflags + ["-o", obj, source], check=True, capture_output=True)

    lds = os.path.join(BUILD, "linker.ld")
    crt0 = os.path.join(lib_build, "crt0.o")
    liba = os.path.join(lib_build, "libportix.a")
    subprocess.run([LD, "-T", lds, "-o", output, obj, crt0,
                    "-L", lib_build, "-lportix",
                    "-z", "max-page-size=0x1", "-N"],
                   check=True, capture_output=True)
    log(f"  {output} ({os.path.getsize(output)} bytes)")
    os.unlink(obj)

def inject_into_fat32(img_path, files):
    """files: list of (local_path, fat32_path)"""
    from pyfatfs.PyFatFS import PyFatFS
    import tempfile

    with open(img_path, "rb") as f:
        mbr = f.read(512)
        pt2_lba = struct.unpack_from('<I', mbr, 0x1CE+8)[0]
        pt2_offset = pt2_lba * 512
        pt2_size_sectors = struct.unpack_from('<I', mbr, 0x1CE+12)[0]
        pt2_size = pt2_size_sectors * 512

    tmp = tempfile.NamedTemporaryFile(delete=False)
    tmp.close()
    with open(img_path, "rb") as src:
        src.seek(pt2_offset)
        with open(tmp.name, "wb") as dst:
            dst.write(src.read(pt2_size))

    fs = PyFatFS(tmp.name, encoding="utf-8")
    for local, fat in files:
        with open(local, "rb") as f:
            data = f.read()
        fs.writebytes(fat, data)
        log(f"  {local} -> {fat} ({len(data)} bytes)")
    fs.close()

    with open(tmp.name, "rb") as src:
        new_part = src.read()
    with open(img_path, "r+b") as dst:
        dst.seek(pt2_offset)
        dst.write(new_part)
    os.unlink(tmp.name)

def main():
    img = IMG
    if "--img=" in " ".join(sys.argv[1:]):
        for a in sys.argv[1:]:
            if a.startswith("--img="):
                img = a.split("=", 1)[1]

    if not os.path.exists(img):
        log(f"Image not found: {img}")
        log(f"Run: python scripts/build.py --no-iso --no-vm --no-run")
        return 1

    if not check_toolchain():
        log("Skipping ring-3 C build")
        return 0

    step("Building libportix")
    lib_build = build_libportix()

    step("Compiling C programs")
    examples_dir = os.path.join(LIB, "examples")
    prog = {}

    hello_src = os.path.join(examples_dir, "hello.c")
    hello_elf = os.path.join(BUILD, "hello_c.elf")
    compile_c(hello_src, hello_elf, lib_build)
    prog[hello_elf] = "/bin/sh"

    step("Injecting into disk image")
    files = [(local, fat) for local, fat in prog.items()]
    inject_into_fat32(img, files)

    log("Done!")
    return 0

if __name__ == "__main__":
    sys.exit(main())
