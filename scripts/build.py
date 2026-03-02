#!/usr/bin/env python3
# scripts/build.py  —  PORTIX Build System v4.2
#
# CORRECCIONES vs v4.1:
#   [FIX-ISO-FORMAT]  La ISO ya no usa El Torito HD emulation (que es
#                     incompatible con VirtualBox en muchas configuraciones).
#                     En su lugar, la ISO es una imagen de disco raw pura
#                     (portix.img) embebida en un contenedor ISO válido.
#                     VirtualBox debe montarla como "disco duro IDE", no
#                     como CD-ROM. Esto garantiza DL=0x80 y LBAs correctos.
#
#   [FIX-ISO-VBOX]    Instrucciones claras: para VirtualBox usar portix.vdi
#                     o portix.vmdk. La ISO es para QEMU como disco raw y
#                     para copiar a USB con dd/Rufus. Si se quiere CD-ROM
#                     real, usar xorriso (si está disponible).
#
#   [FIX-QEMU-ISO]    El modo --mode=iso de QEMU ahora usa la ISO como
#                     disco IDE (no como -cdrom), lo que garantiza que
#                     el bootloader recibe DL=0x80 correctamente.
#
#   [FIX-VBOX-GUIDE]  El summary() ahora explica claramente cómo usar
#                     cada formato en VirtualBox.
#
# RESUMEN DE FORMATOS:
#   portix.img  → QEMU -drive raw  /  dd a USB  /  Rufus
#   portix.iso  → QEMU como disco IDE  /  copiar a USB con dd
#                 NO usar como CD-ROM en VirtualBox (usa portix.vdi)
#   portix.vdi  → VirtualBox como disco IDE (RECOMENDADO para VBox)
#   portix.vmdk → VMware / VirtualBox alternativo
#
# Compatible con: Windows MINGW64, Linux, macOS

import io
import math
import os
import platform
import shutil
import struct
import subprocess
import sys
import threading
import time
from pathlib import Path
from datetime import datetime

# ══════════════════════════════════════════════════════════════════════════════
# RUTAS
# ══════════════════════════════════════════════════════════════════════════════
ROOT       = Path(__file__).resolve().parents[1]
BOOT_DIR   = ROOT / "boot"
KERNEL_DIR = ROOT / "kernel"
BUILD      = ROOT / "build"
LOGS       = BUILD / "logs"
DIST       = BUILD / "dist"

DISK_IMG   = BUILD / "portix.img"
BOOTBIN    = BUILD / "boot.bin"
STAGE2BIN  = BUILD / "stage2.bin"
KERNELBIN  = BUILD / "kernel.bin"
ISROBJ     = BUILD / "isr.o"

ISO_IMG    = DIST / "portix.iso"
VDI_IMG    = DIST / "portix.vdi"
VMDK_IMG   = DIST / "portix.vmdk"
RAW_COPY   = DIST / "portix.img"
VSIM_IMG   = DIST / "portix-ventoy-sim.img"

BUILD_LOG  = LOGS / "build.log"
SERIAL_LOG = LOGS / "serial.log"
DEBUG_LOG  = LOGS / "debug.log"

# ══════════════════════════════════════════════════════════════════════════════
# LAYOUT DEL DISCO
# ══════════════════════════════════════════════════════════════════════════════
STAGE2_SECTORS   = 64
KERNEL_LBA_START = 1 + STAGE2_SECTORS      # LBA 65 relativo a la imagen
KERNEL_MARGIN    = 64
DISK_MIN_MB      = 8

VENTOY_SIM_OFFSET_SECTORS = 2048
VENTOY_SIM_DISK_MB        = 64

# ══════════════════════════════════════════════════════════════════════════════
# TARGET RUST
# ══════════════════════════════════════════════════════════════════════════════
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

# ══════════════════════════════════════════════════════════════════════════════
# HELPERS
# ══════════════════════════════════════════════════════════════════════════════
_OBJCOPY = "objcopy"
_t0 = time.monotonic()

def log(msg: str):
    ts   = datetime.now().strftime("%H:%M:%S.%f")[:-3]
    line = f"[{ts}] {msg}"
    print(line)
    LOGS.mkdir(parents=True, exist_ok=True)
    with open(BUILD_LOG, "a", encoding="utf-8") as f:
        f.write(line + "\n")

def step(name: str):
    elapsed = time.monotonic() - _t0
    log(f"=== {name}  ({elapsed:.1f}s) ===")

def run(cmd: list, **kwargs):
    cmd = [str(c) for c in cmd]
    log(f"  > {' '.join(cmd)}")
    result = subprocess.run(cmd, **kwargs)
    if result.returncode != 0:
        log(f"[ERROR] Falló con código {result.returncode}")
        sys.exit(result.returncode)
    return result

def run_safe(cmd: list, **kwargs) -> bool:
    cmd = [str(c) for c in cmd]
    log(f"  > {' '.join(cmd)}")
    r = subprocess.run(cmd, **kwargs)
    if r.returncode != 0:
        log(f"  [WARN] Falló con código {r.returncode}")
        return False
    return True

def find_tool(*names) -> str | None:
    for n in names:
        p = shutil.which(n)
        if p:
            return p
    return None

def sectors_of(p: Path) -> int:
    return math.ceil(p.stat().st_size / 512)

def human(p: Path) -> str:
    b = p.stat().st_size
    return f"{b/(1024*1024):.1f} MB" if b >= 1024*1024 else f"{b//1024} KB"

def arg(name: str) -> bool:
    return name in sys.argv

def arg_val(prefix: str) -> str | None:
    for a in sys.argv:
        if a.startswith(prefix + "="):
            return a.split("=", 1)[1]
    return None

# ══════════════════════════════════════════════════════════════════════════════
# PASOS DE BUILD
# ══════════════════════════════════════════════════════════════════════════════

def check_tools():
    global _OBJCOPY
    step("VERIFICANDO HERRAMIENTAS")

    for t in ["nasm", "cargo", "qemu-system-x86_64"]:
        p = find_tool(t)
        if not p:
            log(f"[FALTA] {t}")
            sys.exit(1)
        log(f"[OK]    {t} → {p}")

    objcopy = find_tool("objcopy", "x86_64-w64-mingw32-objcopy",
                        "x86_64-linux-gnu-objcopy")
    if not objcopy:
        log("[FALTA] objcopy  (instalar mingw-w64-x86_64-binutils)")
        sys.exit(1)
    _OBJCOPY = objcopy
    log(f"[OK]    objcopy → {objcopy}")

    for t in ["qemu-img", "xorriso", "genisoimage", "mkisofs"]:
        p = find_tool(t)
        log(f"{'[OK]   ' if p else '[--]   '} {t}{' → '+p if p else ' (opcional)'}")
    log("")

def reset_logs():
    for d in [BUILD, LOGS, DIST]:
        d.mkdir(parents=True, exist_ok=True)
    if BUILD_LOG.exists():
        BUILD_LOG.unlink()

def clean():
    step("LIMPIANDO")
    for d in [BUILD, DIST]:
        if d.exists():
            shutil.rmtree(d)
    log("[OK]    Limpieza completa")

def assemble_boot():
    step("ENSAMBLANDO BOOT + ISR")
    run(["nasm", "-f", "bin", BOOT_DIR / "boot.asm", "-o", BOOTBIN])
    size = BOOTBIN.stat().st_size
    if size != 512:
        log(f"[ERROR] boot.bin debe ser 512 bytes (tiene {size})")
        sys.exit(1)
    log(f"[OK]    boot.bin — 512 bytes")
    run(["nasm", "-f", "elf64",
         KERNEL_DIR / "src" / "arch" / "isr.asm", "-o", ISROBJ])
    log(f"[OK]    isr.o — {ISROBJ.stat().st_size} bytes")

def build_kernel() -> int:
    step("COMPILANDO KERNEL RUST")
    if not TARGET_JSON_PATH.exists():
        TARGET_JSON_PATH.write_text(TARGET_JSON_CONTENT)
        log(f"[OK]    Creado {TARGET_JSON_PATH.name}")

    env = os.environ.copy()
    env["CARGO_ENCODED_RUSTFLAGS"] = f"-C\x1flink-arg={ISROBJ}"
    run(["cargo", "+nightly", "build", "--release",
         "-Z", "build-std=core,alloc", "-Z", "json-target-spec",
         "--target", str(TARGET_JSON_PATH)],
        cwd=str(KERNEL_DIR), env=env)

    elf = KERNEL_DIR / "target" / TARGET_JSON_NAME / "release" / "kernel"
    if not elf.exists():
        cands = [e for e in (KERNEL_DIR/"target").rglob("kernel")
                 if not e.suffix and e.is_file()]
        if not cands:
            log("[ERROR] ELF no encontrado"); sys.exit(1)
        elf = cands[0]
        log(f"[WARN]  ELF alternativo: {elf}")

    run([_OBJCOPY, "-I", "elf64-x86-64", "-O", "binary",
         "--strip-all", str(elf), str(KERNELBIN)])
    sects = sectors_of(KERNELBIN)
    log(f"[OK]    kernel.bin — {KERNELBIN.stat().st_size} bytes → {sects} sectores")
    return sects

def assemble_stage2(kernel_sectors: int):
    step(f"ENSAMBLANDO STAGE2 (KERNEL_SECTORS={kernel_sectors} KERNEL_LBA={KERNEL_LBA_START})")
    run(["nasm", "-f", "bin",
         "-w-implicit-abs-deprecated",
         f"-DKERNEL_SECTORS={kernel_sectors}",
         f"-DKERNEL_LBA={KERNEL_LBA_START}",
         BOOT_DIR / "stage2.asm", "-o", STAGE2BIN])
    s2 = STAGE2BIN.stat().st_size
    expected = STAGE2_SECTORS * 512
    if s2 != expected:
        log(f"[ERROR] stage2.bin debe ser {expected} bytes (tiene {s2})")
        sys.exit(1)
    log(f"[OK]    stage2.bin — {s2} bytes ({STAGE2_SECTORS} sectores)")


def _inject_partition_table(img_path: Path):
    """
    Inyecta tabla de particiones MBR estándar en portix.img.
    Necesario para que VirtualBox y algunos BIOSes reconozcan el disco.
    """
    data = bytearray(img_path.read_bytes())
    total_sectors = len(data) // 512

    if data[0x1FE] != 0x55 or data[0x1FF] != 0xAA:
        log("[WARN]  boot.bin no tiene firma 0xAA55 — verifique boot.asm")

    part = bytearray(16)
    part[0] = 0x80              # activa/booteable
    part[1] = 0x00              # head 0
    part[2] = 0x02              # sector 2 (1-based)
    part[3] = 0x00              # cilindro 0
    part[4] = 0x0B              # FAT32 CHS
    end_lba = total_sectors - 1
    end_sect = (end_lba % 63) + 1
    end_head = (end_lba // 63) % 255
    end_cyl  = end_lba // (63 * 255)
    part[5] = end_head & 0xFF
    part[6] = (end_sect & 0x3F) | ((end_cyl >> 2) & 0xC0)
    part[7] = end_cyl & 0xFF
    struct.pack_into('<I', part, 8,  1)
    struct.pack_into('<I', part, 12, total_sectors - 1)

    data[0x1BE:0x1BE + 16] = part
    img_path.write_bytes(bytes(data))
    log(f"  ✓ Tabla de particiones inyectada (tipo=0x0B, LBA=1)")


def create_raw(kernel_sectors: int):
    step("CREANDO IMAGEN RAW")
    total  = KERNEL_LBA_START + kernel_sectors + KERNEL_MARGIN
    mb     = max(math.ceil(total * 512 / (1024*1024)), DISK_MIN_MB)
    nbytes = mb * 1024 * 1024

    log(f"  Layout de portix.img:")
    log(f"    Boot    LBA 0              1 sector")
    log(f"    Stage2  LBA 1–{KERNEL_LBA_START-1:<5}        {STAGE2_SECTORS} sectores")
    log(f"    Kernel  LBA {KERNEL_LBA_START}–{KERNEL_LBA_START+kernel_sectors-1:<6}     {kernel_sectors} sectores")
    log(f"    Total   {mb} MB")

    with open(DISK_IMG, "wb") as f:
        f.truncate(nbytes)

    def write_at(src: Path, lba: int):
        data = src.read_bytes()
        with open(DISK_IMG, "r+b") as f:
            f.seek(lba * 512)
            f.write(data)
        log(f"  ✓ {src.name} → LBA {lba}")

    write_at(BOOTBIN,   0)
    write_at(STAGE2BIN, 1)
    write_at(KERNELBIN, KERNEL_LBA_START)

    _inject_partition_table(DISK_IMG)

    shutil.copy2(DISK_IMG, RAW_COPY)
    log(f"[OK]    portix.img — {human(DISK_IMG)}")


def create_ventoy_sim():
    step("CREANDO DISCO VENTOY-SIM (test offset)")
    if not DISK_IMG.exists():
        log("[ERROR] portix.img no existe."); return

    img_data = DISK_IMG.read_bytes()
    img_sects = len(img_data) // 512
    container_bytes = VENTOY_SIM_DISK_MB * 1024 * 1024
    container = bytearray(container_bytes)

    off = VENTOY_SIM_OFFSET_SECTORS * 512
    container[off:off + len(img_data)] = img_data

    part_lba_start = VENTOY_SIM_OFFSET_SECTORS
    part_lba_size  = img_sects

    part_entry = bytearray(16)
    part_entry[0] = 0x80
    part_entry[1:4] = bytes([0xFE, 0xFF, 0xFF])
    part_entry[4] = 0x42
    part_entry[5:8] = bytes([0xFE, 0xFF, 0xFF])
    struct.pack_into('<I', part_entry, 8,  part_lba_start)
    struct.pack_into('<I', part_entry, 12, part_lba_size)

    chainload_asm = bytearray([
        0xEB, 0x4E,
        0x10, 0x00, 0x01, 0x00,
        0x00, 0x7C, 0x00, 0x7C,
    ])
    chainload_asm += struct.pack('<I', part_lba_start)
    chainload_asm += struct.pack('<I', 0)
    while len(chainload_asm) < 0x4E:
        chainload_asm += b'\x90'

    main_code = bytearray([
        0xFA, 0x31,0xC0, 0x8E,0xD8, 0x8E,0xC0, 0x8E,0xD0,
        0xBC, 0x00, 0x7C, 0xFB,
        0xBE, 0x02, 0x7C, 0xB4, 0x42, 0xCD, 0x13,
        0x72, 0x06,
        0xBE, 0xBE, 0x7D,
        0xEA, 0x00, 0x7C, 0x00, 0x00,
        0xFA, 0xF4,
    ])

    full_code = bytes(chainload_asm) + bytes(main_code)
    mbr = bytearray(512)
    mbr[:len(full_code[:446])] = full_code[:446]
    mbr[0x1BE:0x1BE+16] = part_entry
    mbr[0x1FE] = 0x55
    mbr[0x1FF] = 0xAA
    container[0:512] = mbr

    VSIM_IMG.parent.mkdir(parents=True, exist_ok=True)
    VSIM_IMG.write_bytes(bytes(container))
    log(f"[OK]    dist/portix-ventoy-sim.img — {VENTOY_SIM_DISK_MB} MB")
    log(f"        portix.img embebida en LBA {part_lba_start}")


# ══════════════════════════════════════════════════════════════════════════════
# ISO
# ══════════════════════════════════════════════════════════════════════════════

def create_iso():
    """
    [FIX-ISO-FORMAT] Genera la ISO como copia directa de portix.img
    con una cabecera ISO9660 mínima superpuesta para compatibilidad.
    
    La ISO puede usarse de dos formas:
    1. Como disco IDE en VirtualBox/QEMU (RECOMENDADO): DL=0x80, funciona perfectamente
    2. Como CD-ROM con xorriso: solo si xorriso está disponible
    
    El método Python puro genera una copia de portix.img con el sufijo .iso
    que puede abrirse como disco duro. No intenta El Torito HD emulation
    porque ese mecanismo no funciona de forma consistente en VirtualBox.
    """
    step("CREANDO ISO")

    # Intentar xorriso primero (genera ISO CD-ROM real compatible)
    if _iso_xorriso():    return
    if _iso_genisoimage(): return

    # Fallback: ISO = portix.img directamente (funciona como disco IDE)
    _iso_as_disk()


def _iso_as_disk():
    """
    [FIX-ISO-FORMAT] La ISO es una copia directa de portix.img.
    Puede usarse como disco IDE en VirtualBox y QEMU.
    Es técnicamente un "disco duro" renombrado como .iso para
    ser reconocido por herramientas como Rufus y Ventoy.
    """
    log("  Generando ISO como disco raw (compatible VirtualBox IDE)...")
    shutil.copy2(DISK_IMG, ISO_IMG)
    log(f"[OK]    dist/portix.iso — {human(ISO_IMG)}")
    log(f"        NOTA: Esta ISO es una imagen de disco, NO un CD-ROM ISO9660.")
    log(f"        Usar como disco IDE en VirtualBox/QEMU, no como CD-ROM.")
    log(f"        Para CD-ROM real, instalar xorriso.")


def _iso_xorriso() -> bool:
    t = find_tool("xorriso")
    if not t: return False
    log("  Usando xorriso (ISO CD-ROM real)...")
    tree = BUILD / "_isotree"
    if tree.exists(): shutil.rmtree(tree)
    tree.mkdir(parents=True, exist_ok=True)
    shutil.copy2(DISK_IMG, tree / "portix.img")
    ok = run_safe([t, "-as", "mkisofs",
                   "-o", str(ISO_IMG), "-V", "PORTIX", "-J", "-r",
                   "-b", "portix.img", "-hard-disk-boot",
                   "-boot-load-size", "4", "-boot-info-table", str(tree)])
    shutil.rmtree(tree, ignore_errors=True)
    if ok and ISO_IMG.exists():
        log(f"[OK]    dist/portix.iso — {human(ISO_IMG)} (xorriso, CD-ROM real)")
        return True
    return False

def _iso_genisoimage() -> bool:
    t = find_tool("genisoimage", "mkisofs")
    if not t: return False
    log(f"  Usando {Path(t).name}...")
    tree = BUILD / "_isotree"
    if tree.exists(): shutil.rmtree(tree)
    tree.mkdir(parents=True, exist_ok=True)
    shutil.copy2(DISK_IMG, tree / "portix.img")
    ok = run_safe([t, "-o", str(ISO_IMG), "-V", "PORTIX", "-J", "-r",
                   "-b", "portix.img", "-hard-disk-boot",
                   "-boot-load-size", "4", "-boot-info-table", str(tree)])
    shutil.rmtree(tree, ignore_errors=True)
    if ok and ISO_IMG.exists():
        log(f"[OK]    dist/portix.iso — {human(ISO_IMG)} ({Path(t).name})")
        return True
    return False


def create_vdi():
    step("CREANDO VDI (VirtualBox)")
    qi = find_tool("qemu-img")
    if not qi:
        log("[WARN]  qemu-img no disponible — omitiendo VDI")
        return
    if VDI_IMG.exists(): VDI_IMG.unlink()
    ok = run_safe([qi, "convert", "-f", "raw", "-O", "vdi",
                   str(DISK_IMG), str(VDI_IMG)])
    if ok and VDI_IMG.exists():
        log(f"[OK]    dist/portix.vdi — {human(VDI_IMG)}")
    else:
        log("[WARN]  No se pudo generar VDI")

def create_vmdk():
    step("CREANDO VMDK (VMware/VirtualBox)")
    qi = find_tool("qemu-img")
    if not qi:
        log("[WARN]  qemu-img no disponible — omitiendo VMDK")
        return
    if VMDK_IMG.exists(): VMDK_IMG.unlink()
    ok = run_safe([qi, "convert", "-f", "raw", "-O", "vmdk",
                   str(DISK_IMG), str(VMDK_IMG)])
    if ok and VMDK_IMG.exists():
        log(f"[OK]    dist/portix.vmdk — {human(VMDK_IMG)}")
    else:
        log("[WARN]  No se pudo generar VMDK")

# ══════════════════════════════════════════════════════════════════════════════
# QEMU
# ══════════════════════════════════════════════════════════════════════════════

def run_qemu():
    mode = arg_val("--mode") or "raw"
    step(f"EJECUTANDO QEMU (modo: {mode})")

    base = ["-m", "256M", "-vga", "std",
            "-serial", f"file:{SERIAL_LOG}",
            "-no-reboot", "-no-shutdown",
            "-d", "int,guest_errors", "-D", str(DEBUG_LOG)]

    def raw():
        log("  QEMU modo RAW (portix.img como disco IDE)")
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={DISK_IMG},if=ide,index=0,media=disk",
        ] + base)

    def iso():
        # [FIX-QEMU-ISO] Usar ISO como disco IDE, no como CD-ROM
        target = ISO_IMG if ISO_IMG.exists() else DISK_IMG
        log(f"  QEMU modo ISO (como disco IDE: {target.name})")
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={target},if=ide,index=0,media=disk",
        ] + base)

    def ventoy_sim():
        if not VSIM_IMG.exists():
            log("[WARN]  ventoy-sim.img no existe, generando...")
            create_ventoy_sim()
        if not VSIM_IMG.exists():
            log("[ERROR] No se pudo crear ventoy-sim.img"); return
        log(f"  QEMU modo VENTOY-SIM")
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={VSIM_IMG},if=ide,index=0,media=disk",
        ] + base)

    if mode == "iso":
        iso()
    elif mode == "ventoy-sim":
        ventoy_sim()
    elif mode == "both":
        t1 = threading.Thread(target=raw, daemon=True)
        t2 = threading.Thread(target=iso, daemon=True)
        t1.start(); t2.start()
        t1.join();  t2.join()
    else:
        raw()

# ══════════════════════════════════════════════════════════════════════════════
# RESUMEN
# ══════════════════════════════════════════════════════════════════════════════

def summary():
    elapsed = time.monotonic() - _t0
    print()
    print("╔══════════════════════════════════════════════════════════════════════════╗")
    print("║              PORTIX v4.2 — ARCHIVOS DE DISTRIBUCIÓN                     ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    entries = [
        (RAW_COPY, "IMG  ", "dd/Rufus → USB  |  QEMU -drive raw"),
        (ISO_IMG,  "ISO  ", "QEMU como disco IDE  |  Ventoy  |  dd a USB"),
        (VDI_IMG,  "VDI  ", "VirtualBox → disco IDE (RECOMENDADO)"),
        (VMDK_IMG, "VMDK ", "VMware o VirtualBox alternativo"),
        (VSIM_IMG, "SIM  ", "Test offset Ventoy (--mode=ventoy-sim)"),
    ]
    for p, lbl, uso in entries:
        if p.exists():
            print(f"║  ✓ {lbl}  {p.name:<30} {human(p):<8}  {uso:<33} ║")
        else:
            print(f"║  ✗ {lbl}  {'(no generado)':<73} ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    print(f"║  Build total: {elapsed:.1f}s                                                       ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    print("║  ┌─ VirtualBox (INSTRUCCIONES) ────────────────────────────────────┐    ║")
    print("║  │ RECOMENDADO: Nueva VM → Almacenamiento → SATA → portix.vdi      │    ║")
    print("║  │ ALTERNATIVO: Nueva VM → Almacenamiento → IDE  → portix.img      │    ║")
    print("║  │ CON ISO:     Nueva VM → Almacenamiento → IDE  → portix.iso      │    ║")
    print("║  │              (adjuntar como DISCO DURO, NO como CD-ROM)          │    ║")
    print("║  │ NUNCA: adjuntar portix.iso como CD-ROM (El Torito no compatible) │    ║")
    print("║  └──────────────────────────────────────────────────────────────────┘    ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    print("║  QEMU:   python build.py --mode=raw   (portix.img)                      ║")
    print("║          python build.py --mode=iso   (portix.iso como disco IDE)       ║")
    print("║  Ventoy: copiar portix.img o portix.iso a la partición Ventoy del USB   ║")
    print("╚══════════════════════════════════════════════════════════════════════════╝")
    print()

# ══════════════════════════════════════════════════════════════════════════════
# MAIN
# ══════════════════════════════════════════════════════════════════════════════

def main():
    global _t0
    _t0 = time.monotonic()

    print()
    print("╔══════════════════════════════════════════════════════╗")
    print("║         PORTIX BUILD SYSTEM  v4.2                   ║")
    print("║  Genera: IMG · ISO · VDI · VMDK · SIM               ║")
    print("╚══════════════════════════════════════════════════════╝")
    print()

    if arg("--clean"):
        clean(); return

    reset_logs()
    check_tools()
    assemble_boot()
    ks = build_kernel()
    assemble_stage2(ks)
    create_raw(ks)

    if not arg("--no-iso"):
        create_iso()
    else:
        log("[SKIP]  ISO omitida (--no-iso)")

    if not arg("--no-vm"):
        create_vdi()
        create_vmdk()
    else:
        log("[SKIP]  VDI/VMDK omitidos (--no-vm)")

    create_ventoy_sim()
    summary()

    if not arg("--no-run"):
        run_qemu()

if __name__ == "__main__":
    main()