#!/usr/bin/env python3
# scripts/build.py — PORTIX Build System v1.0
#
# FUENTE ÚNICA DE VERDAD para el layout del disco.
# stage2.asm recibe KERNEL_SECTORS real vía -D después de compilar el kernel.
#
# Orden de compilación:
#   1. boot.asm + isr.asm  (sin dependencias)
#   2. kernel Rust          (produce kernel.bin → medimos sectores reales)
#   3. stage2.asm           (recibe -DKERNEL_SECTORS=N exacto)
#   4. create_image         (ensambla todo en portix.img de tamaño auto)

import shutil
import subprocess
import sys
import math
import os
from pathlib import Path
from datetime import datetime

# ── Rutas ─────────────────────────────────────────────────────────────────────
ROOT       = Path(__file__).resolve().parents[1]
BOOT_DIR   = ROOT / "boot"
KERNEL_DIR = ROOT / "kernel"
BUILD      = ROOT / "build"
LOGS       = BUILD / "logs"

DISK_IMG   = BUILD / "portix.img"
BOOTBIN    = BUILD / "boot.bin"
STAGE2BIN  = BUILD / "stage2.bin"
KERNELBIN  = BUILD / "kernel.bin"
ISROBJ     = BUILD / "isr.o"
ISO_IMG    = BUILD / "portix.iso"
ISO_TREE   = BUILD / "iso_tree"

BUILD_LOG  = LOGS / "build.log"
QEMU_LOG   = LOGS / "qemu.log"
SERIAL_LOG = LOGS / "serial.log"
DEBUG_LOG  = LOGS / "debug.log"

# ── Layout del disco (única fuente de verdad) ─────────────────────────────────
STAGE2_SECTORS    = 64
KERNEL_LBA_START  = 1 + STAGE2_SECTORS   # LBA 65
KERNEL_MARGIN     = 64                   # sectores extra de margen
DISK_MIN_MB       = 8                    # tamaño mínimo de imagen

# ── Target Rust ───────────────────────────────────────────────────────────────
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

# ── Helpers ───────────────────────────────────────────────────────────────────

def log(msg: str):
    ts   = datetime.now().strftime("%H:%M:%S.%f")[:-3]
    line = f"[{ts}] {msg}"
    print(line)
    LOGS.mkdir(parents=True, exist_ok=True)
    with open(BUILD_LOG, "a", encoding="utf-8") as f:
        f.write(line + "\n")

def run(cmd: list, **kwargs):
    log(f"  > {' '.join(map(str, cmd))}")
    result = subprocess.run(cmd, **kwargs)
    if result.returncode != 0:
        log(f"[ERROR] Falló con código {result.returncode}")
        sys.exit(result.returncode)
    return result

def sectors_of(path: Path) -> int:
    return math.ceil(path.stat().st_size / 512)

# ── Pasos ─────────────────────────────────────────────────────────────────────

def check_tools():
    log("=== VERIFICANDO HERRAMIENTAS ===")
    required = ["nasm", "cargo", "objcopy", "qemu-system-x86_64"]
    for t in required:
        path = shutil.which(t)
        if not path:
            log(f"[FALTA] {t}")
            sys.exit(1)
        log(f"[OK]    {t} → {path}")

def reset_logs():
    BUILD.mkdir(parents=True, exist_ok=True)
    LOGS.mkdir(parents=True, exist_ok=True)
    for f in [BUILD_LOG, QEMU_LOG, SERIAL_LOG, DEBUG_LOG]:
        if f.exists():
            f.unlink()

def assemble_boot():
    """Paso 1: boot.asm e isr.asm — no dependen del kernel."""
    log("=== ENSAMBLANDO BOOT + ISR ===")

    run(["nasm", "-f", "bin",
         str(BOOT_DIR / "boot.asm"), "-o", str(BOOTBIN)])
    assert BOOTBIN.stat().st_size == 512
    log(f"[OK]    boot.bin — 512 bytes")

    run(["nasm", "-f", "elf64",
         str(KERNEL_DIR / "src" / "isr.asm"), "-o", str(ISROBJ)])
    log(f"[OK]    isr.o    — {ISROBJ.stat().st_size} bytes")

def build_kernel() -> int:
    """
    Paso 2: Compila el kernel Rust.
    Devuelve el número REAL de sectores para pasárselo a stage2.
    """
    log("=== COMPILANDO KERNEL RUST ===")

    if not TARGET_JSON_PATH.exists():
        TARGET_JSON_PATH.write_text(TARGET_JSON_CONTENT)
        log(f"[OK]    Creado {TARGET_JSON_PATH.name}")

    env = os.environ.copy()
    env["CARGO_ENCODED_RUSTFLAGS"] = f"-C\x1flink-arg={ISROBJ}"

    run([
        "cargo", "+nightly", "build", "--release",
        "-Z", "build-std=core",
        "-Z", "json-target-spec",
        "--target", str(TARGET_JSON_PATH),
    ], cwd=str(KERNEL_DIR), env=env)

    # Localizar ELF
    elf = KERNEL_DIR / "target" / TARGET_JSON_NAME / "release" / "kernel"
    if not elf.exists():
        candidates = [e for e in (KERNEL_DIR / "target").rglob("kernel")
                      if not e.suffix and e.is_file()]
        if not candidates:
            log("[ERROR] No se encontró ELF del kernel")
            sys.exit(1)
        elf = candidates[0]
        log(f"[WARN]  ELF alternativo: {elf}")

    # Igual que el original: solo --strip-all, sin --remove-section
    run(["objcopy", "-I", "elf64-x86-64", "-O", "binary",
         "--strip-all", str(elf), str(KERNELBIN)])

    size    = KERNELBIN.stat().st_size
    sectors = sectors_of(KERNELBIN)
    log(f"[OK]    kernel.bin — {size} bytes → {sectors} sectores")
    return sectors

def assemble_stage2(kernel_sectors: int):
    """
    Paso 3: Ensambla stage2.asm pasando KERNEL_SECTORS real via -D.
    stage2.asm es idéntico al original excepto que KERNEL_SECTORS
    viene de aquí en vez de estar hardcodeado.
    """
    log(f"=== ENSAMBLANDO STAGE2 (KERNEL_SECTORS={kernel_sectors}) ===")

    run([
        "nasm", "-f", "bin",
        "-w-implicit-abs-deprecated",
        f"-DKERNEL_SECTORS={kernel_sectors}",
        str(BOOT_DIR / "stage2.asm"), "-o", str(STAGE2BIN),
    ])

    s2 = STAGE2BIN.stat().st_size
    expected = STAGE2_SECTORS * 512
    if s2 != expected:
        log(f"[ERROR] stage2.bin debe ser {expected} bytes exactos (tiene {s2})")
        sys.exit(1)
    log(f"[OK]    stage2.bin — {s2} bytes ({STAGE2_SECTORS} sectores)")

def create_image(kernel_sectors: int):
    """Paso 4: Imagen de disco con tamaño calculado automáticamente."""
    log("=== CREANDO IMAGEN DE DISCO ===")

    total_sectors = KERNEL_LBA_START + kernel_sectors + KERNEL_MARGIN
    disk_mb       = max(math.ceil(total_sectors * 512 / (1024*1024)), DISK_MIN_MB)
    disk_bytes    = disk_mb * 1024 * 1024

    log(f"        Layout:")
    log(f"          Boot    LBA 0                  (512 bytes)")
    log(f"          Stage2  LBA 1  – {KERNEL_LBA_START-1:<5}          ({STAGE2_SECTORS} sectores)")
    log(f"          Kernel  LBA {KERNEL_LBA_START} – {KERNEL_LBA_START+kernel_sectors-1:<5}          ({kernel_sectors} sectores / {KERNELBIN.stat().st_size} bytes)")
    log(f"          Margen  {KERNEL_MARGIN} sectores")
    log(f"          Total   {disk_mb} MB  ({disk_bytes} bytes)")

    with open(DISK_IMG, "wb") as f:
        f.truncate(disk_bytes)

    def write_at(path: Path, lba: int):
        with open(DISK_IMG, "r+b") as f:
            f.seek(lba * 512)
            f.write(path.read_bytes())

    write_at(BOOTBIN,   0)
    write_at(STAGE2BIN, 1)
    write_at(KERNELBIN, KERNEL_LBA_START)

    log(f"[OK]    {DISK_IMG.name} — {DISK_IMG.stat().st_size} bytes")

def create_iso():
    if not _create_iso_pycdlib():
        _create_iso_external()

def _create_iso_pycdlib() -> bool:
    log("=== CREANDO ISO (pycdlib) ===")
    try:
        from pycdlib import PyCdlib
    except Exception as e:
        log(f"[WARN] pycdlib no disponible: {e}")
        return False
    try:
        iso = PyCdlib()
        iso.new(interchange_level=3, joliet=True)
        iso.add_file(str(DISK_IMG), iso_path='/PORTIX.BIN;1', joliet_path='/portix.img')
        iso.add_eltorito('/PORTIX.BIN;1', boot_load_size=4,
                         media_name='noemul', boot_info_table=True)
        try:
            iso.add_isohybrid()
        except Exception as e:
            log(f"[WARN] isohybrid no crítico: {e}")
        iso.write(str(ISO_IMG))
        iso.close()
        log(f"[OK]    {ISO_IMG.name} — {ISO_IMG.stat().st_size} bytes")
        return True
    except Exception as e:
        log(f"[ERROR] pycdlib: {e}")
        return False

def _create_iso_external() -> bool:
    log("=== CREANDO ISO (externo) ===")
    if ISO_TREE.exists():
        shutil.rmtree(ISO_TREE)
    ISO_TREE.mkdir(parents=True, exist_ok=True)
    shutil.copy2(DISK_IMG, ISO_TREE / "portix.img")

    xorriso = shutil.which("xorriso")
    geniso  = shutil.which("genisoimage") or shutil.which("mkisofs")
    ok = False
    try:
        if xorriso:
            run(["xorriso", "-as", "mkisofs", "-o", str(ISO_IMG),
                 "-V", "PORTIX", "-no-emul-boot", "-boot-load-size", "4",
                 "-boot-info-table", "-b", "portix.img", str(ISO_TREE)])
            ok = True
        elif geniso:
            run([geniso, "-o", str(ISO_IMG),
                 "-V", "PORTIX", "-no-emul-boot", "-boot-load-size", "4",
                 "-boot-info-table", "-b", "portix.img", str(ISO_TREE)])
            ok = True
        else:
            log("[WARN] No se encontró xorriso / genisoimage / mkisofs")
    finally:
        shutil.rmtree(ISO_TREE, ignore_errors=True)

    if ok and ISO_IMG.exists():
        log(f"[OK]    {ISO_IMG.name} — {ISO_IMG.stat().st_size} bytes")
    return ok

def run_qemu():
    log("=== EJECUTANDO QEMU ===")
    subprocess.run([
        "qemu-system-x86_64",
        "-drive", f"format=raw,file={DISK_IMG},if=ide",
        "-m", "256M",           # 128 MB — más espacio para LFB PCI mapping
        "-vga", "std",
        "-serial", f"file:{SERIAL_LOG}",
        "-no-reboot",
        "-no-shutdown",
        "-d", "int,guest_errors",
        "-D", str(DEBUG_LOG),
    ])

# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    print()
    print("╔══════════════════════════════════════╗")
    print("║   PORTIX BUILD SYSTEM  v1.0          ║")
    print("╚══════════════════════════════════════╝")
    print()

    reset_logs()
    check_tools()

    assemble_boot()
    kernel_sectors = build_kernel()   # kernel primero → sectores reales
    assemble_stage2(kernel_sectors)   # stage2 sabe exactamente cuántos leer
    create_image(kernel_sectors)
    create_iso()

    if "--no-run" not in sys.argv:
        run_qemu()

if __name__ == "__main__":
    main()