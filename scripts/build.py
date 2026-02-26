#!/usr/bin/env python3
# scripts/build.py — PORTIX Build System v3.0
#
# Compatible con: Windows MINGW64, Linux, macOS
#
# Genera automáticamente TODOS los formatos de distribución:
#   build/dist/portix.img   → imagen raw  (QEMU, dd a USB, Rufus)
#   build/dist/portix.iso   → ISO booteable (VirtualBox, VMware, DVD, Ventoy)
#   build/dist/portix.vdi   → VirtualBox nativo
#   build/dist/portix.vmdk  → VMware / VirtualBox alternativo
#
# Requisitos MÍNIMOS (Windows MINGW64 con solo QEMU instalado):
#   - nasm
#   - cargo + rust nightly
#   - objcopy  (viene con mingw-w64-x86_64-binutils en MSYS2)
#   - qemu-system-x86_64  (para ejecutar)
#   - qemu-img            (para VDI/VMDK — viene incluido con QEMU)
#
# La ISO se genera en Python puro sin xorriso ni genisoimage.
# Si xorriso o genisoimage están disponibles se usan preferentemente.
#
# Uso:
#   python scripts/build.py                   # build completo + lanzar QEMU raw
#   python scripts/build.py --no-run          # solo build, sin lanzar QEMU
#   python scripts/build.py --mode=iso        # lanzar QEMU con ISO
#   python scripts/build.py --mode=both       # QEMU raw + QEMU iso simultaneos
#   python scripts/build.py --clean           # limpiar build/

import io
import math
import os
import platform
import shutil
import struct
import subprocess
import sys
import threading
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

BUILD_LOG  = LOGS / "build.log"
SERIAL_LOG = LOGS / "serial.log"
DEBUG_LOG  = LOGS / "debug.log"

# ══════════════════════════════════════════════════════════════════════════════
# LAYOUT DEL DISCO
# ══════════════════════════════════════════════════════════════════════════════
STAGE2_SECTORS   = 64
KERNEL_LBA_START = 1 + STAGE2_SECTORS    # LBA 65
KERNEL_MARGIN    = 64
DISK_MIN_MB      = 8

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
_OBJCOPY = "objcopy"   # se sobreescribe en check_tools()

def log(msg: str):
    ts   = datetime.now().strftime("%H:%M:%S.%f")[:-3]
    line = f"[{ts}] {msg}"
    print(line)
    LOGS.mkdir(parents=True, exist_ok=True)
    with open(BUILD_LOG, "a", encoding="utf-8") as f:
        f.write(line + "\n")

def run(cmd: list, **kwargs):
    cmd = [str(c) for c in cmd]
    log(f"  > {' '.join(cmd)}")
    result = subprocess.run(cmd, **kwargs)
    if result.returncode != 0:
        log(f"[ERROR] Falló con código {result.returncode}")
        sys.exit(result.returncode)
    return result

def run_safe(cmd: list, **kwargs) -> bool:
    """Como run() pero retorna True/False sin abortar."""
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

# ══════════════════════════════════════════════════════════════════════════════
# PASOS DE BUILD
# ══════════════════════════════════════════════════════════════════════════════

def check_tools():
    global _OBJCOPY
    log("=== VERIFICANDO HERRAMIENTAS ===")

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
    log("=== LIMPIANDO ===")
    for d in [BUILD, DIST]:
        if d.exists():
            shutil.rmtree(d)
    log("[OK]    Limpieza completa")

def assemble_boot():
    log("=== ENSAMBLANDO BOOT + ISR ===")
    run(["nasm", "-f", "bin", BOOT_DIR / "boot.asm", "-o", BOOTBIN])
    assert BOOTBIN.stat().st_size == 512
    log(f"[OK]    boot.bin — 512 bytes")
    run(["nasm", "-f", "elf64",
         KERNEL_DIR / "src" / "arch" / "isr.asm", "-o", ISROBJ])
    log(f"[OK]    isr.o — {ISROBJ.stat().st_size} bytes")

def build_kernel() -> int:
    log("=== COMPILANDO KERNEL RUST ===")
    if not TARGET_JSON_PATH.exists():
        TARGET_JSON_PATH.write_text(TARGET_JSON_CONTENT)
        log(f"[OK]    Creado {TARGET_JSON_PATH.name}")

    env = os.environ.copy()
    env["CARGO_ENCODED_RUSTFLAGS"] = f"-C\x1flink-arg={ISROBJ}"
    run(["cargo", "+nightly", "build", "--release",
         "-Z", "build-std=core", "-Z", "json-target-spec",
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
    log(f"=== ENSAMBLANDO STAGE2 "
        f"(KERNEL_SECTORS={kernel_sectors} KERNEL_LBA={KERNEL_LBA_START}) ===")
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

def create_raw(kernel_sectors: int):
    log("=== CREANDO IMAGEN RAW ===")
    total  = KERNEL_LBA_START + kernel_sectors + KERNEL_MARGIN
    mb     = max(math.ceil(total * 512 / (1024*1024)), DISK_MIN_MB)
    nbytes = mb * 1024 * 1024

    log(f"  Boot    LBA 0              1 sector")
    log(f"  Stage2  LBA 1–{KERNEL_LBA_START-1}         {STAGE2_SECTORS} sectores")
    log(f"  Kernel  LBA {KERNEL_LBA_START}–{KERNEL_LBA_START+kernel_sectors-1}    "
        f"   {kernel_sectors} sectores  ({KERNELBIN.stat().st_size} B)")
    log(f"  Total   {mb} MB  ({nbytes} bytes)")

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

    shutil.copy2(DISK_IMG, RAW_COPY)
    log(f"[OK]    portix.img — {human(DISK_IMG)}")
    log(f"[OK]    dist/portix.img — listo para dd / Rufus")

# ══════════════════════════════════════════════════════════════════════════════
# ISO — PYTHON PURO (El Torito HD Emulation, sin dependencias externas)
# ══════════════════════════════════════════════════════════════════════════════
#
# Estructura ISO 9660 mínima:
#   LBA 0-15  : área de sistema (LBA 0 = hybrid MBR con boot.bin)
#   LBA 16    : Primary Volume Descriptor
#   LBA 17    : Boot Record Descriptor (El Torito)
#   LBA 18    : Volume Descriptor Set Terminator
#   LBA 19    : Boot Catalog  ← media_type=0x02 (Hard Disk Emulation)
#   LBA 20    : Directorio raíz
#   LBA 21    : Path Table L
#   LBA 22+   : portix.img
#
# Con HD Emulation el BIOS trata portix.img como un HDD real:
#   LBA 0 del HDD virtual = LBA 22 del ISO = boot.bin
#   INT 13h funciona → stage1 puede leer stage2 normalmente

ISECT = 2048  # tamaño de sector ISO

def _pad(d: bytes, n: int) -> bytes:
    return (d + b'\x00'*n)[:n]

def _b16(n): return struct.pack('<H', n) + struct.pack('>H', n)
def _b32(n): return struct.pack('<I', n) + struct.pack('>I', n)
def _le16(n): return struct.pack('<H', n)
def _le32(n): return struct.pack('<I', n)

def _idate(dt: datetime) -> bytes:
    return bytes([dt.year-1900, dt.month, dt.day,
                  dt.hour, dt.minute, dt.second, 0])

def _pvd_date(dt: datetime) -> bytes:
    return (dt.strftime("%Y%m%d%H%M%S00") + "\x00").encode()[:17]

def _dirent(name: bytes, lba: int, size: int, dt: datetime, flags=0) -> bytes:
    nlen = len(name)
    rlen = 33 + nlen
    if rlen % 2: rlen += 1
    e = bytearray(rlen)
    e[0]     = rlen
    e[2:10]  = _b32(lba)
    e[10:18] = _b32(size)
    e[18:25] = _idate(dt)
    e[25]    = flags
    e[28:30] = _b16(1)
    e[30]    = nlen
    e[31:31+nlen] = name
    return bytes(e)

def build_iso_python(img_path: Path, out_path: Path):
    log("  Generando ISO Python puro (El Torito HD emulation)...")
    img   = img_path.read_bytes()
    isecs = math.ceil(len(img) / ISECT)
    now   = datetime.now()

    L_PVD      = 16
    L_BREC     = 17
    L_TERM     = 18
    L_BCAT     = 19
    L_ROOT     = 20
    L_PATH     = 21
    L_IMG      = 22
    L_TOTAL    = L_IMG + isecs

    buf = bytearray(L_TOTAL * ISECT)

    def ws(lba: int, data: bytes):
        off = lba * ISECT
        buf[off:off+len(data)] = data

    # Sector 0: Hybrid MBR (boot.bin en los primeros 512 bytes)
    bb = BOOTBIN.read_bytes()[:512]
    buf[0:len(bb)] = bb
    buf[0x1FE] = 0x55
    buf[0x1FF] = 0xAA

    # PVD
    pvd = bytearray(ISECT)
    pvd[0]     = 1
    pvd[1:6]   = b'CD001'
    pvd[6]     = 1
    pvd[8:40]  = _pad(b'PORTIX', 32)
    pvd[40:72] = _pad(b'PORTIX', 32)
    pvd[80:88] = _b32(L_TOTAL)
    pvd[120:122] = _b16(1)
    pvd[124:126] = _b16(1)
    pvd[128:130] = _b16(ISECT)
    pvd[132:140] = _b32(len(img))
    root_dot = _dirent(b'\x00', L_ROOT, ISECT, now, flags=2)
    pvd[156:156+len(root_dot)] = root_dot
    pvd[190:318] = _pad(b'PORTIX', 128)
    pvd[318:446] = _pad(b'PORTIX PROJECT', 128)
    pvd[446:574] = _pad(b'PORTIX BUILD SYSTEM V3', 128)
    pvd[881:898] = _pvd_date(now)
    pvd[898:915] = _pvd_date(now)
    pvd[1883]   = 1
    ws(L_PVD, bytes(pvd))

    # Boot Record Descriptor (El Torito)
    brd = bytearray(ISECT)
    brd[0]    = 0
    brd[1:6]  = b'CD001'
    brd[6]    = 1
    brd[7:39] = _pad(b'EL TORITO SPECIFICATION', 32)
    brd[71:75] = _le32(L_BCAT)
    ws(L_BREC, bytes(brd))

    # Volume Descriptor Set Terminator
    vdt = bytearray(ISECT)
    vdt[0]   = 0xFF
    vdt[1:6] = b'CD001'
    vdt[6]   = 1
    ws(L_TERM, bytes(vdt))

    # Boot Catalog
    cat = bytearray(ISECT)
    # Validation Entry (32 bytes)
    cat[0]    = 1       # header ID
    cat[1]    = 0       # platform: 80x86
    cat[4:28] = _pad(b'PORTIX', 24)
    cat[30:32] = b'\x55\xAA'
    # Corregir checksum: suma de todos los words = 0
    words = list(struct.unpack('<16H', bytes(cat[:32])))
    total_w = sum(words) & 0xFFFF
    ck = (0x10000 - total_w) & 0xFFFF
    struct.pack_into('<H', cat, 28, ck)

    # Default/Initial Entry (32 bytes @ offset 32)
    cat[32] = 0x88          # bootable
    cat[33] = 0x02          # media type: Hard Disk ← CLAVE para que INT 13h funcione
    cat[34:36] = _le16(0)   # load segment (0 = default 0x07C0)
    cat[36]  = 0            # system type
    cat[37]  = 0
    cat[38:40] = _le16(1)   # sector count
    cat[40:44] = _le32(L_IMG)   # LBA de inicio de portix.img en el ISO
    ws(L_BCAT, bytes(cat))

    # Directorio raíz
    fname = b'PORTIX.IMG;1'
    rdir  = bytearray(ISECT)
    off   = 0
    for ename, elba, eflags in [
        (b'\x00', L_ROOT, 2),
        (b'\x01', L_ROOT, 2),
        (fname,   L_IMG,  0),
    ]:
        esize = ISECT if eflags == 2 else len(img)
        de = _dirent(ename, elba, esize, now, flags=eflags)
        rdir[off:off+len(de)] = de
        off += len(de)
    ws(L_ROOT, bytes(rdir))

    # Path Table L
    ptl = bytearray(ISECT)
    ptl[0] = 1              # nombre length
    ptl[1] = 0
    ptl[2:6]  = _le32(L_ROOT)
    ptl[6:8]  = _le16(1)
    ptl[8]    = 0           # nombre raíz = byte nulo
    ws(L_PATH, bytes(ptl))

    # Datos de portix.img
    off = L_IMG * ISECT
    padded = _pad(img, isecs * ISECT)
    buf[off:off+len(padded)] = padded

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(bytes(buf))
    log(f"[OK]    dist/portix.iso — {human(out_path)} (Python puro, HD emul)")

def create_iso():
    log("=== CREANDO ISO ===")
    if _iso_xorriso():   return
    if _iso_genisoimage(): return
    try:
        build_iso_python(DISK_IMG, ISO_IMG)
    except Exception as e:
        log(f"[ERROR] ISO Python: {e}")
        import traceback; traceback.print_exc()

def _iso_xorriso() -> bool:
    t = find_tool("xorriso")
    if not t: return False
    log("  Usando xorriso...")
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
        log(f"[OK]    dist/portix.iso — {human(ISO_IMG)} (xorriso)")
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

# ══════════════════════════════════════════════════════════════════════════════
# VDI y VMDK via qemu-img
# ══════════════════════════════════════════════════════════════════════════════

def create_vdi():
    log("=== CREANDO VDI (VirtualBox) ===")
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
    log("=== CREANDO VMDK (VMware/VirtualBox) ===")
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
    mode = next((a.split("=",1)[1] for a in sys.argv if a.startswith("--mode=")), "raw")
    log(f"=== EJECUTANDO QEMU (modo: {mode}) ===")

    base = ["-m", "256M", "-vga", "std",
            "-serial", f"file:{SERIAL_LOG}",
            "-no-reboot", "-no-shutdown",
            "-d", "int,guest_errors", "-D", str(DEBUG_LOG)]

    def raw():
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={DISK_IMG},if=ide,index=0,media=disk",
        ] + base)

    def iso():
        if not ISO_IMG.exists():
            log("[WARN]  ISO no disponible, usando raw"); raw(); return
        subprocess.run(["qemu-system-x86_64",
            "-cdrom", str(ISO_IMG), "-boot", "d",
        ] + base)

    if mode == "iso":
        iso()
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
    print()
    print("╔══════════════════════════════════════════════════════════════════════╗")
    print("║                 PORTIX — ARCHIVOS DE DISTRIBUCIÓN                   ║")
    print("╠══════════════════════════════════════════════════════════════════════╣")
    entries = [
        (RAW_COPY, "IMG  ", "dd/Rufus → USB real,  QEMU -drive raw"),
        (ISO_IMG,  "ISO  ", "VirtualBox · VMware · QEMU -cdrom · DVD"),
        (VDI_IMG,  "VDI  ", "VirtualBox: Nueva VM → Almacenamiento → Add"),
        (VMDK_IMG, "VMDK ", "VMware o VirtualBox alternativo"),
    ]
    for p, lbl, uso in entries:
        if p.exists():
            print(f"║  ✓ {lbl}  {p.name:<20} {human(p):<8}  {uso}  ║")
        else:
            print(f"║  ✗ {lbl}  {'(no generado)':<52}  ║")
    print("╠══════════════════════════════════════════════════════════════════════╣")
    print("║  VirtualBox: Nueva VM → Tipo=Other/Unknown 64bit                    ║")
    print("║    · ISO:  Almacenamiento → Controlador IDE → Agregar CD → portix.iso║")
    print("║    · VDI:  Almacenamiento → Controlador SATA → Agregar disco → .vdi ║")
    print("╚══════════════════════════════════════════════════════════════════════╝")
    print()

# ══════════════════════════════════════════════════════════════════════════════
# MAIN
# ══════════════════════════════════════════════════════════════════════════════

def main():
    print()
    print("╔══════════════════════════════════════════════════════╗")
    print("║         PORTIX BUILD SYSTEM  v3.0                   ║")
    print("║  Genera: IMG · ISO · VDI · VMDK automáticamente     ║")
    print("╚══════════════════════════════════════════════════════╝")
    print()

    if "--clean" in sys.argv:
        clean(); return

    reset_logs()
    check_tools()
    assemble_boot()
    ks = build_kernel()
    assemble_stage2(ks)
    create_raw(ks)
    create_iso()
    create_vdi()
    create_vmdk()
    summary()

    if "--no-run" not in sys.argv:
        run_qemu()

if __name__ == "__main__":
    main()