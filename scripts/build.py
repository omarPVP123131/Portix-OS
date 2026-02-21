#!/usr/bin/env python3
# scripts/build.py — Build system PORTIX v4

import shutil
import subprocess
import sys
import math
import os
from pathlib import Path
from datetime import datetime

ROOT       = Path(__file__).resolve().parents[1]
BOOT_DIR   = ROOT / "boot"
KERNEL_DIR = ROOT / "kernel"
BUILD      = ROOT / "build"
LOGS       = BUILD / "logs"

DISK_IMG  = BUILD / "portix.img"
BOOTBIN   = BUILD / "boot.bin"
STAGE2BIN = BUILD / "stage2.bin"
KERNELBIN = BUILD / "kernel.bin"
ISROBJ    = BUILD / "isr.o"

ISO_IMG   = BUILD / "portix.iso"
ISO_TREE  = BUILD / "iso_tree"
BOOT_IMG_NAME = "portix.img"

BUILD_LOG  = LOGS / "build.log"
QEMU_LOG   = LOGS / "qemu.log"
SERIAL_LOG = LOGS / "serial.log"
DEBUG_LOG  = LOGS / "debug.log"

# ── Layout del disco ─────────────────────────────────────────────────────────
STAGE2_SECTORS   = 64
KERNEL_SECTORS   = 256   # ← subido de 192 (kernel actual: 225 sectores)
KERNEL_LBA_START = 65
DISK_SIZE_BYTES  = 4 * 1024 * 1024

TARGET_JSON_NAME = "x86_64-portix"
TARGET_JSON_PATH = KERNEL_DIR / f"{TARGET_JSON_NAME}.json"
TARGET_JSON_CONTENT = """{
  "llvm-target": "x86_64-unknown-none-elf",
  "data-layout": "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128",
  "arch": "x86_64",
  "target-endian": "little",
  "target-pointer-width": 64,
  "target-c-int-width": 32,
  "os": "none",
  "executables": true,
  "linker-flavor": "ld.lld",
  "linker": "rust-lld",
  "panic-strategy": "abort",
  "disable-redzone": true,
  "features": "-mmx,-sse,+soft-float",
  "pre-link-args": {
    "ld.lld": ["-Tlinker.ld", "-n", "--gc-sections"]
  }
}"""

def log(msg):
    ts = datetime.now().strftime("%H:%M:%S.%f")[:-3]
    line = f"[{ts}] {msg}"
    print(line)
    LOGS.mkdir(parents=True, exist_ok=True)
    with open(BUILD_LOG, "a", encoding="utf-8") as f:
        f.write(line + "\n")

def run(cmd, **kwargs):
    log(f"  > {' '.join(map(str, cmd))}")
    result = subprocess.run(cmd, **kwargs)
    if result.returncode != 0:
        log(f"[ERROR] Falló con código {result.returncode}")
        sys.exit(result.returncode)
    return result

def check_tools():
    log("=== VERIFICANDO HERRAMIENTAS ===")
    required = ["nasm", "cargo", "objcopy", "qemu-system-x86_64"]
    for t in required:
        path = shutil.which(t)
        if not path:
            log(f"[FALTA] {t}")
            sys.exit(1)
        log(f"[OK]    {t} → {path}")

def create_target_json():
    if not TARGET_JSON_PATH.exists():
        TARGET_JSON_PATH.write_text(TARGET_JSON_CONTENT)
        log(f"[OK]    Creado {TARGET_JSON_PATH.name}")
    else:
        log(f"[OK]    {TARGET_JSON_PATH.name} ya existe")

def assemble_asm():
    log("=== ENSAMBLANDO ASM ===")
    BUILD.mkdir(parents=True, exist_ok=True)
    LOGS.mkdir(parents=True, exist_ok=True)

    for f in [BUILD_LOG, QEMU_LOG, SERIAL_LOG, DEBUG_LOG]:
        if f.exists(): f.unlink()
    log("=== ENSAMBLANDO ASM ===")

    run(["nasm", "-f", "bin", str(BOOT_DIR / "boot.asm"), "-o", str(BOOTBIN)])
    log(f"[OK]    boot.bin   — {BOOTBIN.stat().st_size} bytes")

    # -w-implicit-abs-deprecated suprime el warning de DEFAULT ABS en NASM ≥ 2.16
    run(["nasm", "-f", "bin", "-w-implicit-abs-deprecated",
         str(BOOT_DIR / "stage2.asm"), "-o", str(STAGE2BIN)])
    s2 = STAGE2BIN.stat().st_size
    log(f"[OK]    stage2.bin — {s2} bytes")
    if s2 != STAGE2_SECTORS * 512:
        log(f"[ERROR] stage2.bin debe ser {STAGE2_SECTORS*512} bytes exactos (tiene {s2})")
        sys.exit(1)

    run(["nasm", "-f", "elf64", str(KERNEL_DIR / "src" / "isr.asm"), "-o", str(ISROBJ)])
    log(f"[OK]    isr.o      — {ISROBJ.stat().st_size} bytes")

def build_kernel():
    log("=== COMPILANDO KERNEL RUST ===")
    create_target_json()

    env = os.environ.copy()
    env["CARGO_ENCODED_RUSTFLAGS"] = f"-C\x1flink-arg={ISROBJ}"

    run([
        "cargo", "+nightly", "build", "--release",
        "-Z", "build-std=core",
        "-Z", "json-target-spec",
        "--target", str(TARGET_JSON_PATH),
    ], cwd=str(KERNEL_DIR), env=env)

    elf = KERNEL_DIR / "target" / TARGET_JSON_NAME / "release" / "kernel"
    if not elf.exists():
        elfs = list((KERNEL_DIR / "target").rglob("kernel"))
        elfs = [e for e in elfs if not e.suffix and e.is_file()]
        if not elfs:
            log("[ERROR] No se encontró ELF del kernel")
            sys.exit(1)
        elf = elfs[0]
        log(f"[WARN]  Usando ELF alternativo: {elf}")

    run(["objcopy", "-I", "elf64-x86-64", "-O", "binary",
         "--strip-all", str(elf), str(KERNELBIN)])

    size    = KERNELBIN.stat().st_size
    sectors = math.ceil(size / 512)
    log(f"[OK]    kernel.bin — {size} bytes ({sectors}/{KERNEL_SECTORS} sectores)")

    if sectors > KERNEL_SECTORS:
        log(f"[ERROR] Kernel muy grande: {sectors} > {KERNEL_SECTORS}")
        sys.exit(1)

def create_image():
    log("=== CREANDO IMAGEN DE DISCO (4MB) ===")
    BUILD.mkdir(parents=True, exist_ok=True)

    with open(DISK_IMG, "wb") as f:
        f.truncate(DISK_SIZE_BYTES)

    data = BOOTBIN.read_bytes()
    assert len(data) == 512
    with open(DISK_IMG, "r+b") as f:
        f.seek(0)
        f.write(data)

    data = STAGE2BIN.read_bytes()
    with open(DISK_IMG, "r+b") as f:
        f.seek(512)
        f.write(data)

    data = KERNELBIN.read_bytes()
    with open(DISK_IMG, "r+b") as f:
        f.seek(KERNEL_LBA_START * 512)
        f.write(data)

    log(f"[OK]    {DISK_IMG.name} — {DISK_IMG.stat().st_size} bytes")
    log(f"        Boot:   LBA 0       ({BOOTBIN.stat().st_size} bytes)")
    log(f"        Stage2: LBA 1-64    ({STAGE2BIN.stat().st_size} bytes)")
    log(f"        Kernel: LBA {KERNEL_LBA_START}+     ({KERNELBIN.stat().st_size} bytes)")

def create_iso_pycdlib():
    log("=== CREANDO ISO BOOTEABLE (pycdlib) ===")
    try:
        from pycdlib import PyCdlib
    except Exception as e:
        log(f"[WARN] pycdlib no disponible: {e}")
        return False

    if not DISK_IMG.exists():
        log(f"[ERROR] No existe la imagen raw: {DISK_IMG}")
        return False

    iso_short  = '/PORTIX.BIN;1'
    joliet_long = '/portix.img'

    try:
        iso = PyCdlib()
        iso.new(interchange_level=3, joliet=True)
        iso.add_file(str(DISK_IMG), iso_path=iso_short, joliet_path=joliet_long)
        log(f"[OK]    Añadido {DISK_IMG.name} como {iso_short} / {joliet_long}")
        iso.add_eltorito(iso_short, boot_load_size=4, media_name='noemul', boot_info_table=True)
        try:
            iso.add_isohybrid()
        except Exception as e:
            log(f"[WARN] isohybrid falló (no crítico): {e}")
        iso.write(str(ISO_IMG))
        iso.close()
        if ISO_IMG.exists():
            log(f"[OK]    {ISO_IMG.name} — {ISO_IMG.stat().st_size} bytes")
            return True
        return False
    except Exception as e:
        log(f"[ERROR] Falló create_iso_pycdlib: {e}")
        return False

def create_iso_external():
    log("=== CREANDO ISO BOOTEABLE (externo) ===")
    BUILD.mkdir(parents=True, exist_ok=True)
    if ISO_TREE.exists():
        shutil.rmtree(ISO_TREE)
    ISO_TREE.mkdir(parents=True, exist_ok=True)
    target_inside = ISO_TREE / BOOT_IMG_NAME
    shutil.copy2(DISK_IMG, target_inside)
    log(f"[OK]    Copiado {DISK_IMG.name} → {target_inside}")

    xorriso = shutil.which("xorriso")
    geniso  = shutil.which("genisoimage") or shutil.which("mkisofs")
    try:
        if xorriso:
            run(["xorriso", "-as", "mkisofs", "-o", str(ISO_IMG),
                 "-V", "PORTIX", "-no-emul-boot", "-boot-load-size", "4",
                 "-boot-info-table", "-b", BOOT_IMG_NAME, str(ISO_TREE)])
        elif geniso:
            run([geniso, "-o", str(ISO_IMG),
                 "-V", "PORTIX", "-no-emul-boot", "-boot-load-size", "4",
                 "-boot-info-table", "-b", BOOT_IMG_NAME, str(ISO_TREE)])
        else:
            log("[WARN] No se encontró xorriso/genisoimage/mkisofs")
            return False
    finally:
        try:
            if ISO_TREE.exists():
                shutil.rmtree(ISO_TREE)
        except Exception as e:
            log(f"[WARN] Error limpiando iso_tree: {e}")

    if ISO_IMG.exists():
        log(f"[OK]    {ISO_IMG.name} — {ISO_IMG.stat().st_size} bytes")
        return True
    return False

def create_iso():
    if not create_iso_pycdlib():
        create_iso_external()

def run_qemu():
    log("=== EJECUTANDO QEMU ===")
    LOGS.mkdir(parents=True, exist_ok=True)
    cmd = [
        "qemu-system-x86_64",
        "-drive", f"format=raw,file={DISK_IMG},if=ide",
        "-m", "64M",
        "-vga", "std",
        "-serial", f"file:{SERIAL_LOG}",
        "-no-reboot",
        "-no-shutdown",
        "-d", "int,guest_errors",
        "-D", str(DEBUG_LOG),
    ]
    log(f"  Debug log → {DEBUG_LOG}")
    subprocess.run(cmd)

def main():
    print()
    print("╔══════════════════════════════════════╗")
    print("║   PORTIX BUILD SYSTEM  v0.5          ║")
    print("╚══════════════════════════════════════╝")
    print()
    check_tools()
    assemble_asm()
    build_kernel()
    create_image()
    create_iso()
    if "--no-run" not in sys.argv:
        run_qemu()

if __name__ == "__main__":
    main()