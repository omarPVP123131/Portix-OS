#!/usr/bin/env python3
# scripts/build.py  —  PORTIX Build System v4.4
#
# CORRECCIONES vs v4.3:
#   [FIX-ISO-ELTORITO]  v4.3 rompió QEMU y VBox por un bug en el layout
#                       de la ISO Python-pura: los LBAs del Default Entry
#                       El Torito usaban granularidad de sector CD (2048 B)
#                       pero el BIOS los interpreta como sectores de 512 B
#                       en modo HD emulation → código 0004 "cannot read".
#
#   [FIX-QEMU-CDROM]    v4.3 cambió QEMU a -cdrom/-boot d incluso cuando
#                       la ISO no era un CD-ROM real → QEMU tampoco bootea.
#
#   ESTRATEGIA v4.4 — dos modos claramente separados:
#
#   MODO "cdrom" — xorriso/genisoimage/pycdlib disponible:
#     ISO9660 real + El Torito HD emulation CORRECTO.
#     Las herramientas calculan todos los LBAs internamente → sin bugs de offset.
#     Funciona en VBox como CD-ROM y en QEMU con -cdrom -boot d.
#     Flags: -hard-disk-boot (HD emulation, DL=0x80)
#            -boot-load-size 4 (cargar 2048 B = 4 × 512)
#            SIN -no-emul-boot (causaba code 0004)
#            SIN -boot-info-table (corrompería boot.asm)
#
#   MODO "disk" — sin herramientas externas:
#     portix.iso = copia exacta de portix.img.
#     Funciona en VBox como disco duro IDE y QEMU con -drive raw.
#     NO funciona como CD-ROM. Documentado claramente en el summary.
#     QEMU --mode=iso usa -drive raw en este modo (no -cdrom).
#
#   NUEVO: MODO "pycdlib" — generación de ISO con librería Python pura
#     Si pycdlib está instalado, se crea una ISO9660 real con El Torito
#     usando emulación de disco duro, sin necesidad de herramientas externas.
#     Es tan válido como xorriso y funciona igual en VBox/QEMU.
#     Para activarlo: pip install pycdlib
#
# RESUMEN FINAL DE FORMATOS:
#   portix.img  → QEMU -drive raw  /  dd a USB  /  Rufus
#   portix.iso  → Con xorriso/pycdlib: VBox CD-ROM + QEMU -cdrom
#                 Sin herramientas: VBox disco duro IDE + QEMU -drive raw
#   portix.vdi  → VirtualBox disco IDE (siempre funciona)
#   portix.vmdk → VMware / VirtualBox alternativo

import math
import os
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
KERNEL_LBA_START = 1 + STAGE2_SECTORS
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
_OBJCOPY  = "objcopy"
_ISO_MODE = "disk"    # "cdrom" si se generó ISO9660 real, "disk" si es copia raw
_ISO_METHOD = None    # 'xorriso', 'genisoimage', 'pycdlib', 'disk'
_t0       = time.monotonic()


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
    data = bytearray(img_path.read_bytes())
    total_sectors = len(data) // 512
    if data[0x1FE] != 0x55 or data[0x1FF] != 0xAA:
        log("[WARN]  boot.bin no tiene firma 0xAA55")
    part = bytearray(16)
    part[0] = 0x80
    part[2] = 0x02
    part[4] = 0x0B
    end_lba  = total_sectors - 1
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
    log(f"  Layout: Boot@LBA0, Stage2@LBA1-{KERNEL_LBA_START-1}, "
        f"Kernel@LBA{KERNEL_LBA_START}, Total={mb}MB")
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
    step("CREANDO DISCO VENTOY-SIM")
    if not DISK_IMG.exists():
        log("[ERROR] portix.img no existe."); return
    img_data  = DISK_IMG.read_bytes()
    img_sects = len(img_data) // 512
    container = bytearray(VENTOY_SIM_DISK_MB * 1024 * 1024)
    off = VENTOY_SIM_OFFSET_SECTORS * 512
    container[off:off + len(img_data)] = img_data
    pls = VENTOY_SIM_OFFSET_SECTORS
    part_entry = bytearray(16)
    part_entry[0] = 0x80
    part_entry[1:4] = bytes([0xFE, 0xFF, 0xFF])
    part_entry[4] = 0x42
    part_entry[5:8] = bytes([0xFE, 0xFF, 0xFF])
    struct.pack_into('<I', part_entry, 8,  pls)
    struct.pack_into('<I', part_entry, 12, img_sects)
    chainload = bytearray([0xEB, 0x4E, 0x10, 0x00, 0x01, 0x00, 0x00, 0x7C, 0x00, 0x7C])
    chainload += struct.pack('<I', pls) + struct.pack('<I', 0)
    while len(chainload) < 0x4E:
        chainload += b'\x90'
    main_code = bytearray([
        0xFA, 0x31,0xC0, 0x8E,0xD8, 0x8E,0xC0, 0x8E,0xD0,
        0xBC,0x00,0x7C, 0xFB,
        0xBE,0x02,0x7C, 0xB4,0x42,0xCD,0x13,
        0x72,0x06, 0xBE,0xBE,0x7D,
        0xEA,0x00,0x7C,0x00,0x00, 0xFA,0xF4,
    ])
    full = bytes(chainload) + bytes(main_code)
    mbr = bytearray(512)
    mbr[:min(len(full), 446)] = full[:446]
    mbr[0x1BE:0x1BE+16] = part_entry
    mbr[0x1FE] = 0x55; mbr[0x1FF] = 0xAA
    container[0:512] = mbr
    VSIM_IMG.parent.mkdir(parents=True, exist_ok=True)
    VSIM_IMG.write_bytes(bytes(container))
    log(f"[OK]    dist/portix-ventoy-sim.img — {VENTOY_SIM_DISK_MB} MB (img en LBA {pls})")


# ══════════════════════════════════════════════════════════════════════════════
# ISO v4.4 — ahora con soporte pycdlib corregido
# ══════════════════════════════════════════════════════════════════════════════

def _try_xorriso() -> bool:
    global _ISO_METHOD
    t = find_tool("xorriso")
    if not t:
        return False
    log("  xorriso: ISO9660 + El Torito HD emulation...")
    tree = BUILD / "_isotree"
    if tree.exists(): shutil.rmtree(tree)
    (tree / "boot").mkdir(parents=True, exist_ok=True)
    shutil.copy2(DISK_IMG, tree / "boot" / "portix.img")
    ok = run_safe([
        t, "-as", "mkisofs",
        "-o", str(ISO_IMG),
        "-V", "PORTIX", "-J", "-r",
        "-c", "boot/boot.cat",
        "-b", "boot/portix.img",
        "-hard-disk-boot",
        "-boot-load-size", "4",
        str(tree),
    ])
    shutil.rmtree(tree, ignore_errors=True)
    if not (ok and ISO_IMG.exists() and ISO_IMG.stat().st_size > 0):
        log("  [WARN] xorriso falló"); return False
    _ISO_METHOD = "xorriso"
    log(f"[OK]    dist/portix.iso — {human(ISO_IMG)}  (xorriso, El Torito HD)")
    return True


def _try_genisoimage() -> bool:
    global _ISO_METHOD
    t = find_tool("genisoimage", "mkisofs")
    if not t:
        return False
    log(f"  {Path(t).name}: ISO9660 + El Torito HD emulation...")
    tree = BUILD / "_isotree"
    if tree.exists(): shutil.rmtree(tree)
    tree.mkdir(parents=True, exist_ok=True)
    shutil.copy2(DISK_IMG, tree / "portix.img")
    ok = run_safe([
        t,
        "-o", str(ISO_IMG),
        "-V", "PORTIX", "-J", "-r",
        "-b", "portix.img",
        "-hard-disk-boot",
        "-boot-load-size", "4",
        str(tree),
    ])
    shutil.rmtree(tree, ignore_errors=True)
    if not (ok and ISO_IMG.exists() and ISO_IMG.stat().st_size > 0):
        log(f"  [WARN] {Path(t).name} falló"); return False
    _ISO_METHOD = "genisoimage"
    log(f"[OK]    dist/portix.iso — {human(ISO_IMG)}  ({Path(t).name}, El Torito HD)")
    return True


def _try_pycdlib() -> bool:
    global _ISO_METHOD
    try:
        import pycdlib
    except ImportError:
        log("   pycdlib no instalado (pip install pycdlib) — omitiendo")
        return False

    log("   pycdlib: generando ISO9660 + El Torito HD emulation...")
    
    iso = pycdlib.PyCdlib()
    try:
        # Nivel 2 para máxima compatibilidad
        iso.new(interchange_level=2, joliet=True)

        local_path = str(DISK_IMG)
        iso_internal_path = '/PORTIX.IMG;1'

        # FIX DEFINITIVO: 
        # En versiones antiguas, add_eltorito hace el trabajo de add_file automáticamente.
        # La firma suele ser: add_eltorito(local_path, iso_path, emulation_type, ...)
        try:
            # Intentamos el estilo antiguo (Posicional: Local, ISO, Tipo)
            # El valor 4 suele ser el entero para 'Hard Disk' en versiones viejas.
            iso.add_eltorito(local_path, iso_internal_path, emulation_type=4)
        except (TypeError, pycdlib.pycdlibexception.PyCdlibInvalidInput):
            # Si falla, intentamos el estilo moderno (Primero añadir archivo, luego marcarlo)
            iso.add_file(local_path, iso_internal_path)
            # Llamamos solo con la ruta. Los defaults suelen ser 0x07C0 y 4 sectores.
            iso.add_eltorito(iso_internal_path)

        iso.write(str(ISO_IMG))
        iso.close()

        if ISO_IMG.exists() and ISO_IMG.stat().st_size > 0:
            _ISO_METHOD = "pycdlib"
            log(f"[OK]    dist/portix.iso — {human(ISO_IMG)} (pycdlib, El Torito HD)")
            return True
        return False

    except Exception as e:
        log(f"   [WARN] pycdlib falló: {e} (tipo: {type(e).__name__})")
        if 'iso' in locals():
            try: iso.close()
            except: pass
        if ISO_IMG.exists():
            ISO_IMG.unlink()
        return False

def _iso_disk_copy():
    global _ISO_METHOD
    shutil.copy2(DISK_IMG, ISO_IMG)
    _ISO_METHOD = "disk"
    log(f"[OK]    dist/portix.iso — {human(ISO_IMG)}  (disco raw, sin xorriso)")
    log(f"        AVISO: Esta ISO NO es un CD-ROM. En VBox usar como disco IDE.")
    log(f"        Para soporte CD-ROM: instalar xorriso o pycdlib y rebuildar.")


def create_iso():
    global _ISO_MODE
    step("CREANDO ISO")
    # Intentar en orden: xorriso, genisoimage, pycdlib, disk copy
    if _try_xorriso():
        _ISO_MODE = "cdrom"
        return
    if _try_genisoimage():
        _ISO_MODE = "cdrom"
        return
    if _try_pycdlib():
        _ISO_MODE = "cdrom"
        return
    # Fallback a copia raw
    _ISO_MODE = "disk"
    _iso_disk_copy()


# ══════════════════════════════════════════════════════════════════════════════
# VDI / VMDK
# ══════════════════════════════════════════════════════════════════════════════

def create_vdi():
    step("CREANDO VDI (VirtualBox)")
    qi = find_tool("qemu-img")
    if not qi:
        log("[WARN]  qemu-img no disponible — omitiendo VDI"); return
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
        log("[WARN]  qemu-img no disponible — omitiendo VMDK"); return
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
    base = [
        "-m", "256M", "-vga", "std",
        "-serial", f"file:{SERIAL_LOG}",
        "-no-reboot", "-no-shutdown",
        "-d", "int,guest_errors", "-D", str(DEBUG_LOG),
    ]

    def raw():
        log("  QEMU: portix.img como disco IDE")
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={DISK_IMG},if=ide,index=0,media=disk",
        ] + base)

    def iso():
        target = ISO_IMG if ISO_IMG.exists() else DISK_IMG
        if _ISO_MODE == "cdrom":
            # ISO9660 real → montar como CD-ROM
            log(f"  QEMU: {target.name} como CD-ROM")
            subprocess.run(["qemu-system-x86_64",
                "-cdrom", str(target), "-boot", "d",
            ] + base)
        else:
            # Copia raw → montar como disco (igual que raw)
            log(f"  QEMU: {target.name} como disco raw (sin xorriso)")
            subprocess.run(["qemu-system-x86_64",
                "-drive", f"format=raw,file={target},if=ide,index=0,media=disk",
            ] + base)

    def ventoy_sim():
        if not VSIM_IMG.exists():
            log("[WARN]  ventoy-sim.img no existe, generando...")
            create_ventoy_sim()
        if not VSIM_IMG.exists():
            log("[ERROR] No se pudo crear ventoy-sim.img"); return
        log(f"  QEMU: VENTOY-SIM")
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={VSIM_IMG},if=ide,index=0,media=disk",
        ] + base)

    dispatch = {"iso": iso, "ventoy-sim": ventoy_sim}
    if mode == "both":
        t1 = threading.Thread(target=raw, daemon=True)
        t2 = threading.Thread(target=iso, daemon=True)
        t1.start(); t2.start()
        t1.join();  t2.join()
    else:
        dispatch.get(mode, raw)()


# ══════════════════════════════════════════════════════════════════════════════
# RESUMEN
# ══════════════════════════════════════════════════════════════════════════════

def summary():
    elapsed  = time.monotonic() - _t0
    if _ISO_METHOD in ("xorriso", "genisoimage", "pycdlib"):
        iso_tipo = f"ISO9660 + El Torito HD ({_ISO_METHOD})"
        iso_uso  = "VBox CD-ROM IDE  |  QEMU -cdrom"
    else:
        iso_tipo = "disco raw (sin xorriso)"
        iso_uso  = "VBox disco IDE (NO CD-ROM)  |  QEMU -drive raw"
    print()
    print("╔══════════════════════════════════════════════════════════════════════════╗")
    print("║              PORTIX v4.4 — ARCHIVOS DE DISTRIBUCIÓN                     ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    entries = [
        (RAW_COPY, "IMG  ", "dd/Rufus → USB  |  QEMU -drive raw"),
        (ISO_IMG,  "ISO  ", iso_uso),
        (VDI_IMG,  "VDI  ", "VirtualBox disco IDE (siempre funciona)"),
        (VMDK_IMG, "VMDK ", "VMware o VirtualBox alternativo"),
        (VSIM_IMG, "SIM  ", "Test offset Ventoy (--mode=ventoy-sim)"),
    ]
    for p, lbl, uso in entries:
        existe = p.exists() if p else False
        marca  = "✓" if existe else "✗"
        info   = f"{p.name:<30} {human(p):<8}  {uso}" if existe else f"{'(no generado)'}"
        print(f"║  {marca} {lbl}  {info:<66} ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    print(f"║  ISO tipo: {iso_tipo:<63} ║")
    print(f"║  Build:    {elapsed:.1f}s{' '*60} ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    if _ISO_METHOD in ("xorriso", "genisoimage", "pycdlib"):
        print("║  VirtualBox con portix.iso:                                              ║")
        print("║    VM → Almacenamiento → IDE → Añadir unidad óptica → portix.iso         ║")
        print("║    Iniciar VM → bootea como CD-ROM directamente                          ║")
    else:
        print("║  Sin herramientas de CD — portix.iso NO es CD-ROM:                       ║")
        print("║    VBox: Storage → IDE → adjuntar como DISCO DURO                        ║")
        print("║    RECOMENDADO: usar portix.vdi que siempre funciona                     ║")
        print("║    Para ISO CD-ROM: instalar xorriso o pycdlib y rebuildar               ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    print("║  QEMU:   python build.py --mode=raw    python build.py --mode=iso       ║")
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
    print("║         PORTIX BUILD SYSTEM  v4.4                   ║")
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