#!/usr/bin/env python3
# scripts/build.py
# Build system estable para Portix (Boot + Stage2 ASM + Kernel Rust + ISR)

import shutil
import subprocess
import sys
import math
import os
from pathlib import Path
from datetime import datetime

# --------------------------------------------------
# RUTAS
# --------------------------------------------------

ROOT = Path(__file__).resolve().parents[1]
BOOT_DIR = ROOT / "boot"
KERNEL_DIR = ROOT / "kernel"
BUILD = ROOT / "build"
LOGS = BUILD / "logs"

FLOPPY = BUILD / "floppy.img"
BOOTBIN = BUILD / "boot.bin"
STAGE2BIN = BUILD / "stage2.bin"
KERNELBIN = BUILD / "kernel.bin"
ISROBJECT = BUILD / "isr.o"

# --------------------------------------------------
# LOGS
# --------------------------------------------------

BUILD_LOG = LOGS / "build.log"
QEMU_LOG = LOGS / "qemu.log"
SERIAL_LOG = LOGS / "serial.log"
DEBUG_LOG = LOGS / "debug.log"

# --------------------------------------------------
# LIMITES
# --------------------------------------------------

STAGE2_SECTORS_LIMIT = 64
KERNEL_SECTORS_LIMIT = 64

# --------------------------------------------------
# UTILIDADES
# --------------------------------------------------

def log(msg, logfile=None):
    timestamp = datetime.now().strftime("%H:%M:%S.%f")[:-3]
    line = f"[{timestamp}] {msg}"
    print(line)
    if logfile:
        logfile.parent.mkdir(parents=True, exist_ok=True)
        with open(logfile, "a", encoding="utf-8") as f:
            f.write(line + "\n")

def run(cmd, **kwargs):
    log(f"> {' '.join(map(str, cmd))}", BUILD_LOG)
    subprocess.run(cmd, check=True, **kwargs)

# --------------------------------------------------
# CHECK TOOLS
# --------------------------------------------------

def check_tools():
    log("=== VERIFICANDO HERRAMIENTAS ===", BUILD_LOG)
    tools = ["nasm", "cargo", "objcopy", "qemu-system-x86_64"]
    ok = True
    for t in tools:
        path = shutil.which(t)
        if not path:
            log(f"[ERROR] '{t}' no está en PATH", BUILD_LOG)
            ok = False
        else:
            log(f"[OK] {t} -> {path}", BUILD_LOG)
    return ok

# --------------------------------------------------
# BUILD KERNEL RUST
# --------------------------------------------------

def build_rust_kernel():
    log("=== COMPILANDO KERNEL RUST ===", BUILD_LOG)

    target_json = KERNEL_DIR / "i686-portix.json"

    env = os.environ.copy()
    env["CARGO_ENCODED_RUSTFLAGS"] = f"-C\x1flink-arg={ISROBJECT}"

    run([
        "cargo",
        "+nightly",
        "build",
        "--release",
        "-Z", "build-std=core",
        "-Z", "json-target-spec",
        "--target", str(target_json)
    ], cwd=str(KERNEL_DIR), env=env)

    elf = KERNEL_DIR / "target" / "i686-portix" / "release" / "kernel"

    if not elf.exists():
        log("[ERROR] No se generó el ELF del kernel", BUILD_LOG)
        sys.exit(1)

    run([
        "objcopy",
        "-I", "elf32-i386",  # Fuerza entrada ELF 32 bits
        "-O", "binary",      # Salida binario puro
        "--strip-all",       # Elimina símbolos innecesarios
        str(elf),
        str(KERNELBIN)
    ])

    size = KERNELBIN.stat().st_size
    sectors = math.ceil(size / 512)

    if sectors > KERNEL_SECTORS_LIMIT:
        log(f"[ERROR] Kernel demasiado grande ({sectors} sectores)", BUILD_LOG)
        sys.exit(1)

    log(f"[OK] kernel.bin ({size} bytes, {sectors} sectores)", BUILD_LOG)


# --------------------------------------------------
# BUILD ASM
# --------------------------------------------------

def assemble():
    log("=== COMPILANDO ASM ===", BUILD_LOG)

    BUILD.mkdir(parents=True, exist_ok=True)
    LOGS.mkdir(parents=True, exist_ok=True)

    for f in [BUILD_LOG, QEMU_LOG, SERIAL_LOG, DEBUG_LOG]:
        if f.exists():
            f.unlink()

    run(["nasm", "-f", "bin", BOOT_DIR / "boot.asm", "-o", BOOTBIN])
    log(f"[OK] boot.bin ({BOOTBIN.stat().st_size} bytes)", BUILD_LOG)

    run(["nasm", "-f", "bin", BOOT_DIR / "stage2.asm", "-o", STAGE2BIN])
    log(f"[OK] stage2.bin ({STAGE2BIN.stat().st_size} bytes)", BUILD_LOG)

    # ISR ASM → ELF32
    run([
        "nasm",
        "-f", "elf32",
        KERNEL_DIR / "src" / "isr.asm",
        "-o", ISROBJECT
    ])
    log(f"[OK] isr.o ({ISROBJECT.stat().st_size} bytes)", BUILD_LOG)

    build_rust_kernel()

# --------------------------------------------------
# FLOPPY
# --------------------------------------------------

def create_floppy():
    log("=== CREANDO FLOPPY ===", BUILD_LOG)

    with open(FLOPPY, "wb") as f:
        f.truncate(1440 * 1024)

    with open(BOOTBIN, "rb") as src, open(FLOPPY, "r+b") as dst:
        dst.seek(0)
        dst.write(src.read())

    with open(STAGE2BIN, "rb") as src, open(FLOPPY, "r+b") as dst:
        dst.seek(512)
        data = src.read()
        dst.write(data)
        padding = (STAGE2_SECTORS_LIMIT * 512) - len(data)
        if padding > 0:
            dst.write(b"\x00" * padding)

    KERNEL_OFFSET = 65 * 512
    with open(KERNELBIN, "rb") as src, open(FLOPPY, "r+b") as dst:
        dst.seek(KERNEL_OFFSET)
        dst.write(src.read())

    log(f"[OK] floppy.img lista. Kernel en offset {KERNEL_OFFSET}", BUILD_LOG)

# --------------------------------------------------
# QEMU
# --------------------------------------------------

def run_qemu():
    log("=== EJECUTANDO QEMU ===", BUILD_LOG)

    subprocess.run([
        "qemu-system-x86_64",
        "-fda", str(FLOPPY),
        "-boot", "a",
        "-m", "32M",
        "-serial", f"file:{SERIAL_LOG}",
        "-no-reboot",
        "-no-shutdown",
        "-d", "int,guest_errors",
        "-D", str(DEBUG_LOG)
    ])

# --------------------------------------------------
# MAIN
# --------------------------------------------------

def main():
    print("🚀 PORTIX BUILD SYSTEM (ESTABLE)")
    print("=" * 60)

    if not check_tools():
        sys.exit(1)

    assemble()
    create_floppy()
    run_qemu()

if __name__ == "__main__":
    main()
