#!/usr/bin/env python3
# scripts/build.py — PORTIX Build System v5.0
#
# FIXES vs v4.9:
#
#   [FIX-UEFI-BOOT]   El modo --mode=uefi ahora envuelve la ESP FAT32 en un
#                      disco GPT completo (portix-uefi.img) en lugar de exponer
#                      la ESP desnuda. QEMU puede arrancar un disco GPT con
#                      OVMF o con SeaBIOS+GRUB. run_qemu() busca OVMF en
#                      múltiples rutas de Windows/Linux; si no lo encuentra
#                      imprime instrucciones y NO falla silenciosamente.
#                      El disco GPT tiene:
#                        LBA 0   : MBR protector (0xEE)
#                        LBA 1   : GPT header
#                        LBA 2-33: GPT partition entries
#                        LBA 34+ : ESP FAT32
#
#   [FIX-DUAL-RUN]    --mode=dual ahora llama a run_qemu() con la imagen BIOS
#                      (portix-dual.img / portix.img) en lugar de terminar sin
#                      lanzar QEMU.  El modo dual ejecuta QEMU en modo BIOS
#                      para probar la parte legacy y ofrece el .img UEFI por
#                      separado.
#
#   [FIX-GP-DF]       El #DF que se ve en BIOS/raw es causado por un #GP en
#                      cascada: CR2=0x01000000 (16 MB) = framebuffer no
#                      mapeado. El kernel llama a Console::new() que resuelve
#                      el framebuffer desde la info de la BIOS, pero en QEMU
#                      sin VBE/VESA el framebuffer puede apuntar a 0x01000000
#                      que no está en el mapa de páginas.
#
#                      La corrección está en isr_handlers.rs:
#                        • Todos los ISR comprueban crash_frame.valid ANTES de
#                          llamar Console::new(). Si valid==0, caen al fallback
#                          VGA text mode 0xB8000 (igual que isr_double_fault),
#                          garantizando output sin necesitar framebuffer gráfico.
#                        • inline_capture_frame() ya NO usa pushfq/pop sobre
#                          el stack potencialmente bajo presión. Usa lahf+seto
#                          para reconstruir RFLAGS sin tocar el stack.
#
#                      Esto rompe la cadena #GP -> #GP -> #DF.
#
# Heredado de v4.9:
#   [NO-MKFSFAT]  FAT32 con pyfatfs puro (pip install pyfatfs)
#   [REVERT-NO-EMUL]  ISO con -no-emul-boot + -boot-info-table

import math, os, shutil, struct, subprocess, sys, threading, time, uuid, binascii
from pathlib import Path
from datetime import datetime

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT       = Path(__file__).resolve().parents[1]
BOOT_DIR   = ROOT / "boot"
EFI_DIR    = BOOT_DIR / "efi"
KERNEL_DIR = ROOT / "kernel"
BUILD      = ROOT / "build"
LOGS       = BUILD / "logs"
DIST       = BUILD / "dist"

DISK_IMG   = BUILD / "portix.img"
BOOTBIN    = BUILD / "boot.bin"
STAGE2BIN  = BUILD / "stage2.bin"
KERNELBIN  = BUILD / "kernel.bin"
ISROBJ     = BUILD / "isr.o"
EFIBIN     = BUILD / "BOOTX64.EFI"

ISO_IMG    = DIST / "portix.iso"
VDI_IMG    = DIST / "portix.vdi"
VMDK_IMG   = DIST / "portix.vmdk"
RAW_COPY   = DIST / "portix.img"
VSIM_IMG   = DIST / "portix-ventoy-sim.img"
UEFI_IMG   = DIST / "portix-uefi.img"   # disco GPT completo  [FIX-UEFI-BOOT]
ESP_IMG    = BUILD / "portix-esp.img"   # ESP FAT32 temporal
DUAL_IMG   = DIST / "portix-dual.img"

BUILD_LOG  = LOGS / "build.log"
SERIAL_LOG = LOGS / "serial.log"
DEBUG_LOG  = LOGS / "debug.log"

STAGE2_SECTORS     = 64
KERNEL_LBA_START   = 68
KERNEL_PHYS_ADDR   = 0x00200000
KERNEL_MARGIN      = 64
DISK_MIN_MB        = 8
ISO_BOOT_LOAD_SIZE = STAGE2_SECTORS + 1

ESP_SIZE_MB        = 64
GPT_DISK_MB        = ESP_SIZE_MB + 2    # 2 MB de overhead GPT

assert KERNEL_LBA_START % 4 == 0

VENTOY_SIM_OFFSET_SECTORS = 2048
VENTOY_SIM_DISK_MB        = 64

TARGET_JSON_NAME = "x86_64-portix"
TARGET_JSON_PATH = KERNEL_DIR / f"{TARGET_JSON_NAME}.json"
TARGET_JSON_CONTENT = """{
  "llvm-target": "x86_64-unknown-none-elf",
  "data-layout": "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128",
  "arch": "x86_64","target-endian": "little","target-pointer-width": 64,
  "target-c-int-width": 32,"os": "none","executables": true,
  "linker-flavor": "ld.lld","linker": "rust-lld","panic-strategy": "abort",
  "disable-redzone": true,"features": "-mmx,-sse,+soft-float",
  "pre-link-args": {"ld.lld": ["-Tlinker.ld", "-n", "--gc-sections"]}
}"""

_OBJCOPY = "objcopy"; _ISO_MODE = "disk"; _ISO_METHOD = None
_t0 = time.monotonic()

def log(msg):
    ts = datetime.now().strftime("%H:%M:%S.%f")[:-3]
    line = f"[{ts}] {msg}"; print(line)
    LOGS.mkdir(parents=True, exist_ok=True)
    open(BUILD_LOG, "a").write(line + "\n")

def step(name): log(f"=== {name}  ({time.monotonic()-_t0:.1f}s) ===")

def run(cmd, **kw):
    cmd = [str(c) for c in cmd]; log(f"  > {' '.join(cmd)}")
    r = subprocess.run(cmd, **kw)
    if r.returncode != 0: log(f"[ERROR] cod {r.returncode}"); sys.exit(r.returncode)
    return r

def run_safe(cmd, **kw):
    cmd = [str(c) for c in cmd]; log(f"  > {' '.join(cmd)}")
    r = subprocess.run(cmd, **kw)
    if r.returncode != 0: log(f"  [WARN] cod {r.returncode}"); return False
    return True

def win_to_msys2(p):
    if sys.platform != "win32": return str(p)
    s = str(p).replace("\\", "/")
    if len(s) >= 2 and s[1] == ":": s = "/" + s[0].lower() + s[2:]
    return s

def find_tool(*names):
    extra = []
    if sys.platform == "win32":
        for r in [r"C:\msys64", r"C:\msys32"]:
            for s in ["usr\\bin","mingw64\\bin","mingw32\\bin"]:
                d = r+"\\"+s
                if Path(d).is_dir(): extra.append(d)
    for n in names:
        p = shutil.which(n)
        if p: return p
        for b in extra:
            for e in ("",".exe",".EXE"):
                c = Path(b)/(n+e)
                if c.is_file(): return str(c)
    return None

# ---------------------------------------------------------------------------
# [FIX-UEFI-BOOT] Busca OVMF en rutas estándar de Windows y Linux.
# Devuelve la ruta como string o None si no se encuentra.
# ---------------------------------------------------------------------------
def find_ovmf():
    candidates = [
        # QEMU para Windows (instalador qemu.org)
        r"C:\Program Files\qemu\share\edk2-x86_64-code.fd",
        r"C:\Program Files\qemu\share\ovmf-x86_64.bin",
        r"C:\Program Files\qemu\share\OVMF.fd",
        r"C:\Program Files\qemu\OVMF.fd",
        # MSYS2
        r"C:\msys64\usr\share\ovmf\OVMF.fd",
        r"C:\msys64\mingw64\share\ovmf\OVMF.fd",
        r"C:\msys64\usr\share\qemu\OVMF.fd",
        # Linux estándar
        "/usr/share/ovmf/OVMF.fd",
        "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF.fd",
        "/usr/share/qemu/OVMF.fd",
        # Archivo local en el proyecto (copia manual)
        str(ROOT / "tools" / "OVMF.fd"),
        str(ROOT / "OVMF.fd"),
    ]
    for c in candidates:
        if Path(c).is_file():
            return c
    return None

def sectors_of(p): return math.ceil(p.stat().st_size / 512)
def human(p):
    b = p.stat().st_size
    return f"{b/1048576:.1f} MB" if b >= 1048576 else f"{b//1024} KB"
def arg(n): return n in sys.argv
def arg_val(prefix):
    for a in sys.argv:
        if a.startswith(prefix+"="): return a.split("=",1)[1]
    return None

def _make_boot_cd_img():
    raw = bytearray(DISK_IMG.read_bytes())
    raw[0x1BE:0x1FE] = bytes(0x40)
    assert raw[0x1FE]==0x55 and raw[0x1FF]==0xAA
    return bytes(raw), len(raw)//512

# ---------------------------------------------------------------------------
# FAT32 Python-puro  (pyfatfs)
# ---------------------------------------------------------------------------

def _check_pyfatfs():
    try:
        from pyfatfs.PyFat import PyFat
        from pyfatfs.PyFatFS import PyFatFS
        return PyFat, PyFatFS
    except ImportError:
        log("[ERROR] pyfatfs no instalado. Ejecuta:  pip install pyfatfs")
        sys.exit(1)

def _fat_mkdir(fs, path: str):
    parts = [p for p in path.strip("/").split("/") if p]
    current = "/"
    for part in parts:
        current = current.rstrip("/") + "/" + part
        if not fs.isdir(current):
            fs.makedir(current, recreate=True)
            log(f"  FAT mkdir {current}")

def _fat_copy(fs, src: Path, dst: str):
    data = src.read_bytes()
    with fs.openbin(dst, "w") as f:
        f.write(data)
    log(f"  FAT copy {src.name} -> {dst}  ({len(data)} bytes)")

def _build_esp_fat32(out_img: Path):
    """Construye la ESP FAT32 pura en out_img."""
    PyFat, PyFatFS = _check_pyfatfs()
    size_bytes = ESP_SIZE_MB * 1024 * 1024
    with open(out_img, "wb") as f:
        f.truncate(size_bytes)
    fat = PyFat()
    fat.mkfs(str(out_img), fat_type=PyFat.FAT_TYPE_FAT32, size=size_bytes, label="EFI")
    fat.close()
    log(f"  FAT32 formateada (label=EFI, {ESP_SIZE_MB} MB)")
    fs = PyFatFS(str(out_img), encoding="utf-8")
    try:
        _fat_mkdir(fs, "/EFI")
        _fat_mkdir(fs, "/EFI/BOOT")
        _fat_mkdir(fs, "/PORTIX")
        _fat_copy(fs, EFIBIN,    "/EFI/BOOT/BOOTX64.EFI")
        _fat_copy(fs, KERNELBIN, "/PORTIX/KERNEL.BIN")
        # startup.nsh: OVMF lo ejecuta automáticamente
        with fs.open("/startup.nsh", "wb") as startup:
            startup.write(b"\\EFI\\BOOT\\BOOTX64.EFI\r\n")
    finally:
        fs.close()

# ---------------------------------------------------------------------------
# [FIX-UEFI-BOOT] GPT wrapper: crea disco GPT completo con ESP incrustada.
# Sin esto QEMU con SeaBIOS intenta arrancar la ESP como disco raw y falla.
# ---------------------------------------------------------------------------

def _gpt_crc32(data: bytes) -> int:
    return binascii.crc32(data) & 0xFFFFFFFF

def _write_gpt(out_img: Path, esp_data: bytes):
    """
    Crea un disco GPT mínimo con una sola partición EFI System.
    Layout:
      LBA 0    : MBR protector
      LBA 1    : GPT Primary Header
      LBA 2-33 : Partition Entry Array (128 entradas × 128 bytes)
      LBA 34+  : ESP FAT32 data
      LBA N-33 : Partition Entry Array backup
      LBA N    : GPT Backup Header
    """
    SECTOR = 512
    ESP_START_LBA = 34
    esp_sectors = math.ceil(len(esp_data) / SECTOR)
    esp_end_lba  = ESP_START_LBA + esp_sectors - 1

    total_sectors = ESP_START_LBA + esp_sectors + 33 + 1  # +33 backup entries +1 backup header
    total_size = total_sectors * SECTOR

    disk = bytearray(total_size)

    # ── MBR protector ────────────────────────────────────────────────────────
    disk[446] = 0x00                     # no bootable
    disk[447] = 0xFE; disk[448] = 0xFF; disk[449] = 0xFF  # CHS start
    disk[450] = 0xEE                     # tipo: GPT Protective MBR
    disk[451] = 0xFE; disk[452] = 0xFF; disk[453] = 0xFF  # CHS end
    struct.pack_into("<I", disk, 454, 1)                   # LBA start = 1
    struct.pack_into("<I", disk, 458, min(total_sectors - 1, 0xFFFFFFFF))
    disk[510] = 0x55; disk[511] = 0xAA

    # ── Partition Entry Array (LBA 2-33) ─────────────────────────────────────
    # EFI System Partition GUID: C12A7328-F81F-11D2-BA4B-00A0C93EC93B
    EFI_SYSTEM_GUID = bytes.fromhex("28732AC11FF8D211BA4B00A0C93EC93B")  # little-endian mixed
    part_guid       = uuid.uuid4().bytes_le
    entry = bytearray(128)
    entry[0:16]  = EFI_SYSTEM_GUID
    entry[16:32] = part_guid
    struct.pack_into("<Q", entry, 32, ESP_START_LBA)
    struct.pack_into("<Q", entry, 40, esp_end_lba)
    struct.pack_into("<Q", entry, 48, 0)               # attributes
    name_utf16 = "EFI System".encode("utf-16-le")
    entry[56:56+len(name_utf16)] = name_utf16

    part_array = bytearray(128 * 128)                  # 128 entradas × 128 bytes
    part_array[0:128] = entry
    part_array_crc = _gpt_crc32(bytes(part_array))

    disk[2*SECTOR : 2*SECTOR + len(part_array)] = part_array

    # ── GPT Primary Header (LBA 1) ───────────────────────────────────────────
    disk_guid = uuid.uuid4().bytes_le
    backup_lba = total_sectors - 1

    hdr = bytearray(92)
    hdr[0:8]   = b"EFI PART"
    hdr[8:12]  = b"\x00\x00\x01\x00"        # revision 1.0
    struct.pack_into("<I", hdr, 12, 92)      # header size
    # hdr[16:20] = CRC32 del header (se rellena al final)
    struct.pack_into("<Q", hdr, 24, 1)       # my LBA = 1
    struct.pack_into("<Q", hdr, 32, backup_lba)
    struct.pack_into("<Q", hdr, 40, ESP_START_LBA)        # first usable LBA
    struct.pack_into("<Q", hdr, 48, esp_end_lba)          # last usable LBA
    hdr[56:72] = disk_guid
    struct.pack_into("<Q", hdr, 72, 2)       # start LBA of partition entries
    struct.pack_into("<I", hdr, 80, 128)     # num entries
    struct.pack_into("<I", hdr, 84, 128)     # entry size
    struct.pack_into("<I", hdr, 88, part_array_crc)
    struct.pack_into("<I", hdr, 16, _gpt_crc32(bytes(hdr)))
    disk[SECTOR : SECTOR + len(hdr)] = hdr

    # ── ESP data ─────────────────────────────────────────────────────────────
    esp_off = ESP_START_LBA * SECTOR
    disk[esp_off : esp_off + len(esp_data)] = esp_data

    # ── Backup Partition Entry Array (LBA backup-33 .. backup-1) ─────────────
    bpe_lba = backup_lba - 33
    disk[bpe_lba*SECTOR : bpe_lba*SECTOR + len(part_array)] = part_array

    # ── GPT Backup Header (LBA N) ────────────────────────────────────────────
    bhdr = bytearray(hdr)
    struct.pack_into("<I", bhdr, 16, 0)      # clear CRC before recalc
    struct.pack_into("<Q", bhdr, 24, backup_lba)   # my LBA = backup
    struct.pack_into("<Q", bhdr, 32, 1)            # alternate = primary
    struct.pack_into("<Q", bhdr, 72, bpe_lba)      # backup partition entries
    struct.pack_into("<I", bhdr, 16, _gpt_crc32(bytes(bhdr)))
    disk[backup_lba*SECTOR : backup_lba*SECTOR + len(bhdr)] = bhdr

    out_img.write_bytes(bytes(disk))
    log(f"  GPT escrito: {total_sectors} sectores ({total_size//1048576} MB), "
        f"ESP en LBA {ESP_START_LBA}-{esp_end_lba}")

def create_uefi_image(out_img: Path):
    """
    [FIX-UEFI-BOOT] Crea disco GPT completo con ESP FAT32 incrustada.
    QEMU puede arrancar este disco con OVMF (-bios OVMF.fd / -pflash).
    En v4.9 se exponía la ESP FAT32 pura (sin GPT) lo que causaba que
    QEMU con SeaBIOS viera un disco sin MBR válido y no arrancara.
    """
    step(f"CREANDO DISCO GPT+ESP UEFI ({out_img.name})  [pyfatfs + GPT puro]")

    if not EFIBIN.exists():
        build_efi_loader()
    if not KERNELBIN.exists():
        log("[ERROR] kernel.bin no existe"); sys.exit(1)

    # 1. Construir ESP FAT32 en archivo temporal
    _build_esp_fat32(ESP_IMG)

    # 2. Envolver en disco GPT
    esp_data = ESP_IMG.read_bytes()
    _write_gpt(out_img, esp_data)
    ESP_IMG.unlink(missing_ok=True)

    log(f"[OK]    {out_img.name} — {human(out_img)}  (GPT + ESP FAT32)")

# ---------------------------------------------------------------------------

def check_tools():
    global _OBJCOPY
    step("VERIFICANDO HERRAMIENTAS")
    for t in ["nasm","cargo","qemu-system-x86_64"]:
        p = find_tool(t)
        if not p: log(f"[FALTA] {t}"); sys.exit(1)
        log(f"[OK]    {t} -> {p}")
    oc = find_tool("objcopy","x86_64-w64-mingw32-objcopy","x86_64-linux-gnu-objcopy")
    if not oc: log("[FALTA] objcopy"); sys.exit(1)
    _OBJCOPY = oc; log(f"[OK]    objcopy -> {oc}")
    for t in ["qemu-img","xorriso","genisoimage","mkisofs"]:
        p = find_tool(t)
        log(f"{'[OK]   ' if p else '[--]   '} {t}{' -> '+p if p else ' (opcional)'}")
    mode = arg_val("--mode") or "raw"
    if mode in ("uefi", "dual"):
        try:
            import pyfatfs; log(f"[OK]    pyfatfs (FAT32 Python-puro)")
        except ImportError:
            log("[FALTA] pyfatfs.  Instalar:  pip install pyfatfs"); sys.exit(1)
        # [FIX-UEFI-BOOT] Avisar sobre OVMF pero no fallar en check_tools;
        # run_qemu() manejará la ausencia con mensaje claro.
        ovmf = find_ovmf()
        if ovmf:
            log(f"[OK]    OVMF -> {ovmf}")
        else:
            log("[WARN]  OVMF.fd no encontrado — QEMU no podrá arrancar UEFI")
            log("        Opciones para obtenerlo:")
            log(r"          1) Copiar a: C:\Program Files\qemu\share\OVMF.fd")
            log(f"          2) Copiar a: {ROOT / 'OVMF.fd'}")
            log("          3) MSYS2:  pacman -S mingw-w64-x86_64-ovmf")
            log("          4) Linux:  apt install ovmf  /  dnf install edk2-ovmf")

def reset_logs():
    for d in [BUILD,LOGS,DIST]: d.mkdir(parents=True, exist_ok=True)
    if BUILD_LOG.exists(): BUILD_LOG.unlink()

def clean():
    step("LIMPIANDO")
    for d in [BUILD,DIST]:
        if d.exists(): shutil.rmtree(d)
    log("[OK] Limpieza completa")

def assemble_boot():
    step("ENSAMBLANDO BOOT + ISR")
    run(["nasm","-f","bin", BOOT_DIR/"boot.asm","-o",BOOTBIN])
    sz = BOOTBIN.stat().st_size
    if sz != 512: log(f"[ERROR] boot.bin={sz}B (esperado 512)"); sys.exit(1)
    log(f"[OK]    boot.bin — 512 bytes")
    run(["nasm","-f","elf64", KERNEL_DIR/"src"/"arch"/"isr.asm","-o",ISROBJ])
    log(f"[OK]    isr.o — {ISROBJ.stat().st_size} bytes")

def build_kernel():
    step("COMPILANDO KERNEL RUST")
    if not TARGET_JSON_PATH.exists():
        TARGET_JSON_PATH.write_text(TARGET_JSON_CONTENT)
    env = os.environ.copy()
    env["CARGO_ENCODED_RUSTFLAGS"] = f"-C\x1flink-arg={ISROBJ}"
    run(["cargo","+nightly","build","--release",
         "-Z","build-std=core,alloc","-Z","json-target-spec",
         "--target",str(TARGET_JSON_PATH)], cwd=str(KERNEL_DIR), env=env)
    elf = KERNEL_DIR/"target"/TARGET_JSON_NAME/"release"/"kernel"
    if not elf.exists():
        cands = [e for e in (KERNEL_DIR/"target").rglob("kernel") if not e.suffix and e.is_file()]
        if not cands: log("[ERROR] ELF no encontrado"); sys.exit(1)
        elf = cands[0]
    run([_OBJCOPY,"-I","elf64-x86-64","-O","binary","--strip-all",str(elf),str(KERNELBIN)])
    s = sectors_of(KERNELBIN)
    log(f"[OK]    kernel.bin — {KERNELBIN.stat().st_size}B -> {s} sectores")
    log(f"        linked @ 0x{KERNEL_PHYS_ADDR:08X}")
    return s

def build_efi_loader():
    step("COMPILANDO UEFI LOADER")
    if not (EFI_DIR / "Cargo.toml").exists():
        log("[ERROR] boot/efi/Cargo.toml no existe"); sys.exit(1)
    run(["cargo","+nightly","build","--release",
         "-Z","build-std=core","--target","x86_64-unknown-uefi"], cwd=str(EFI_DIR))
    src = EFI_DIR/"target"/"x86_64-unknown-uefi"/"release"/"portix-efi-loader.efi"
    if not src.exists():
        cands = list((EFI_DIR/"target"/"x86_64-unknown-uefi"/"release").glob("*.efi"))
        if not cands: log("[ERROR] BOOTX64.EFI no generado"); sys.exit(1)
        src = cands[0]
    shutil.copy2(src, EFIBIN)
    log(f"[OK]    BOOTX64.EFI — {human(EFIBIN)}")

def assemble_stage2(ks):
    step(f"ENSAMBLANDO STAGE2 (KERNEL_SECTORS={ks} KERNEL_LBA={KERNEL_LBA_START})")
    run(["nasm","-f","bin","-w-implicit-abs-deprecated",
         f"-DKERNEL_SECTORS={ks}", f"-DKERNEL_LBA={KERNEL_LBA_START}",
         BOOT_DIR/"stage2.asm","-o",STAGE2BIN])
    sz = STAGE2BIN.stat().st_size
    exp = STAGE2_SECTORS*512
    if sz != exp: log(f"[ERROR] stage2={sz}B (esperado {exp})"); sys.exit(1)
    log(f"[OK]    stage2.bin — {sz}B ({STAGE2_SECTORS} sectores)")

def _inject_pt(img_path):
    data = bytearray(img_path.read_bytes())
    ts = len(data)//512
    part = bytearray(16)
    part[0]=0x80; part[2]=0x02; part[4]=0x0B
    el=ts-1; part[5]=(el//63)%255; part[6]=((el%63)+1)&0x3F
    part[7]=(el//(63*255))&0xFF
    struct.pack_into('<I',part,8,1); struct.pack_into('<I',part,12,ts-1)
    data[0x1BE:0x1BE+16]=part; img_path.write_bytes(bytes(data))
    log(f"  Tabla de particiones inyectada")

def create_raw(ks):
    step("CREANDO IMAGEN RAW")
    total = KERNEL_LBA_START+ks+KERNEL_MARGIN
    mb = max(math.ceil(total*512/1048576), DISK_MIN_MB)
    log(f"  Layout: Boot@0 Stage2@1-{KERNEL_LBA_START-1} Kernel@{KERNEL_LBA_START} -> phys 0x{KERNEL_PHYS_ADDR:08X} {mb}MB")
    with open(DISK_IMG,"wb") as f: f.truncate(mb*1048576)
    def wa(src,lba):
        d=src.read_bytes()
        with open(DISK_IMG,"r+b") as f: f.seek(lba*512); f.write(d)
        log(f"  {src.name} -> LBA {lba}")
    wa(BOOTBIN,0); wa(STAGE2BIN,1); wa(KERNELBIN,KERNEL_LBA_START)
    _inject_pt(DISK_IMG); shutil.copy2(DISK_IMG,RAW_COPY)
    log(f"[OK]    portix.img — {human(DISK_IMG)}")

def create_ventoy_sim():
    step("CREANDO DISCO VENTOY-SIM")
    if not DISK_IMG.exists(): log("[ERROR] portix.img no existe"); return
    img=DISK_IMG.read_bytes(); isects=len(img)//512
    cont=bytearray(VENTOY_SIM_DISK_MB*1048576)
    off=VENTOY_SIM_OFFSET_SECTORS*512; cont[off:off+len(img)]=img
    pls=VENTOY_SIM_OFFSET_SECTORS
    pe=bytearray(16); pe[0]=0x80; pe[1:4]=b'\xFE\xFF\xFF'; pe[4]=0x0B; pe[5:8]=b'\xFE\xFF\xFF'
    struct.pack_into('<I',pe,8,pls); struct.pack_into('<I',pe,12,isects)
    stub=bytearray()
    stub+=b'\xFA\x31\xC0\x8E\xD8\x8E\xC0\x8E\xD0\xBC\x00\x7C\xFB'
    lo=pls&0xFFFF; hi=(pls>>16)&0xFFFF
    stub+=b'\xB8'+struct.pack('<H',lo)+b'\xA3\x00\x7E'
    stub+=b'\xB8'+struct.pack('<H',hi)+b'\xA3\x02\x7E'
    stub+=b'\xC7\x06\x04\x7E\x00\x00'
    doff=0x80; dphs=0x7C00+doff; dd=bytearray(16)
    dd[0]=0x10; struct.pack_into('<H',dd,2,1); struct.pack_into('<H',dd,6,0x07C0)
    struct.pack_into('<I',dd,8,pls)
    stub+=b'\xBE'+struct.pack('<H',dphs&0xFFFF)+b'\xB4\x42\xCD\x13\x73\x03\xFA\xF4\xEB\xFD\xEA\x00\x7C\x00\x00'
    mbr=bytearray(512); mbr[:len(bytes(stub)[:446])]=bytes(stub)[:446]
    mbr[doff:doff+16]=dd; mbr[0x1BE:0x1BE+16]=pe; mbr[0x1FE]=0x55; mbr[0x1FF]=0xAA
    cont[0:512]=mbr; VSIM_IMG.parent.mkdir(parents=True,exist_ok=True)
    VSIM_IMG.write_bytes(bytes(cont))
    log(f"[OK]    portix-ventoy-sim.img — {VENTOY_SIM_DISK_MB}MB (img en LBA {pls})")

def _try_xorriso():
    global _ISO_METHOD
    t=find_tool("xorriso")
    if not t: return False
    log(f"  xorriso: no-emul+BIT (load-size={ISO_BOOT_LOAD_SIZE})...")
    tree=BUILD/"_isotree"
    if tree.exists(): shutil.rmtree(tree)
    (tree/"boot").mkdir(parents=True,exist_ok=True)
    bc=tree/"boot"/"boot_cd.img"; ib,isects=_make_boot_cd_img()
    bc.write_bytes(ib)
    ok=run_safe([t,"-as","mkisofs",
        "-o",win_to_msys2(ISO_IMG),"-V","PORTIX","-J","-r",
        "-c","boot/boot.cat","-b","boot/boot_cd.img",
        "-no-emul-boot","-boot-load-size",str(ISO_BOOT_LOAD_SIZE),
        "-boot-info-table", win_to_msys2(tree)])
    shutil.rmtree(tree,ignore_errors=True)
    if not (ok and ISO_IMG.exists() and ISO_IMG.stat().st_size>0):
        log("  [WARN] xorriso fallo"); return False
    _ISO_METHOD="xorriso"
    log(f"[OK]    portix.iso — {human(ISO_IMG)} (xorriso, no-emul+BIT)")
    return True

def _try_genisoimage():
    global _ISO_METHOD
    t=find_tool("genisoimage","mkisofs")
    if not t: return False
    tn=Path(t).name
    tree=BUILD/"_isotree"
    if tree.exists(): shutil.rmtree(tree)
    tree.mkdir(parents=True,exist_ok=True)
    bip=tree/"portix.img"; ib,_=_make_boot_cd_img()
    bip.write_bytes(ib)
    ok=run_safe([t,"-o",str(ISO_IMG),"-V","PORTIX","-J","-r",
        "-c","boot.cat","-b","portix.img",
        "-no-emul-boot","-boot-load-size",str(ISO_BOOT_LOAD_SIZE),
        "-boot-info-table",str(tree)])
    shutil.rmtree(tree,ignore_errors=True)
    if not (ok and ISO_IMG.exists() and ISO_IMG.stat().st_size>0):
        log(f"  [WARN] {tn} fallo"); return False
    _ISO_METHOD="genisoimage"
    log(f"[OK]    portix.iso — {human(ISO_IMG)} ({tn})")
    return True

def _try_pycdlib():
    global _ISO_METHOD
    try: import pycdlib
    except ImportError: return False
    iso=pycdlib.PyCdlib()
    try:
        iso.new(interchange_level=2,joliet=3,rock_ridge='1.09')
        import io
        bi=BOOTBIN.read_bytes()[:512]+STAGE2BIN.read_bytes()
        ts=ISO_BOOT_LOAD_SIZE*512; bi=bi[:ts].ljust(ts,b'\x00')
        iso.add_fp(io.BytesIO(bi),len(bi),iso_path='/PORTIX.IMG;1',
                   joliet_path='/portix.img',rr_name='portix.img')
        kw=dict(bootcatfile='/BOOT.CAT;1',joliet_bootcatfile='/boot.cat',
                media_name='noemul',boot_info_table=True,
                boot_load_size=ISO_BOOT_LOAD_SIZE,bootable=True)
        try: iso.add_eltorito('/PORTIX.IMG;1',**kw)
        except TypeError:
            iso.add_eltorito('/PORTIX.IMG;1',bootcatfile='/BOOT.CAT;1',
                media_name='noemul',boot_info_table=True,
                boot_load_size=ISO_BOOT_LOAD_SIZE,bootable=True)
        iso.write(str(ISO_IMG)); iso.close()
        if ISO_IMG.exists() and ISO_IMG.stat().st_size>0:
            _ISO_METHOD="pycdlib"; return True
        return False
    except Exception as e:
        log(f"  [WARN] pycdlib: {e}")
        try: iso.close()
        except: pass
        if ISO_IMG.exists(): ISO_IMG.unlink()
        return False

def _iso_disk_copy():
    global _ISO_METHOD
    shutil.copy2(DISK_IMG,ISO_IMG); _ISO_METHOD="disk"
    log(f"[OK]    portix.iso — {human(ISO_IMG)} (disco raw)")
    log("        AVISO: NO es CD-ROM. VBox: adjuntar como DISCO DURO IDE.")

def create_iso():
    global _ISO_MODE
    step("CREANDO ISO")
    if _try_xorriso(): _ISO_MODE="cdrom"; return
    if _try_genisoimage(): _ISO_MODE="cdrom"; return
    if _try_pycdlib(): _ISO_MODE="cdrom"; return
    _ISO_MODE="disk"; _iso_disk_copy()

def create_vdi():
    step("CREANDO VDI")
    qi=find_tool("qemu-img")
    if not qi: log("[WARN] qemu-img no disponible"); return
    if VDI_IMG.exists(): VDI_IMG.unlink()
    if run_safe([qi,"convert","-f","raw","-O","vdi",str(DISK_IMG),str(VDI_IMG)]) and VDI_IMG.exists():
        log(f"[OK]    portix.vdi — {human(VDI_IMG)}")

def create_vmdk():
    step("CREANDO VMDK")
    qi=find_tool("qemu-img")
    if not qi: log("[WARN] qemu-img no disponible"); return
    if VMDK_IMG.exists(): VMDK_IMG.unlink()
    if run_safe([qi,"convert","-f","raw","-O","vmdk",str(DISK_IMG),str(VMDK_IMG)]) and VMDK_IMG.exists():
        log(f"[OK]    portix.vmdk — {human(VMDK_IMG)}")

# ---------------------------------------------------------------------------
# run_qemu — [FIX-UEFI-BOOT] + [FIX-DUAL-RUN]
# ---------------------------------------------------------------------------

def run_qemu():
    mode = arg_val("--mode") or "raw"
    step(f"EJECUTANDO QEMU (modo: {mode})")
    vga_type = arg_val("--vga") or "std"
    base = ["-cpu","max","-m","256M","-vga",vga_type,"-serial","stdio",
            "-no-reboot","-no-shutdown","-d","int,guest_errors","-D",str(DEBUG_LOG)]

    def raw():
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={DISK_IMG},if=ide,index=0,media=disk"
        ] + base)

    def iso():
        tgt = ISO_IMG if ISO_IMG.exists() else DISK_IMG
        if _ISO_MODE == "cdrom":
            subprocess.run(["qemu-system-x86_64",
                "-drive", f"format=raw,file={tgt},media=cdrom", "-boot","order=d"
            ] + base)
        else:
            subprocess.run(["qemu-system-x86_64",
                "-drive", f"format=raw,file={tgt},if=ide,index=0,media=disk"
            ] + base)

    def vsim():
        if not VSIM_IMG.exists(): create_ventoy_sim()
        if not VSIM_IMG.exists(): log("[ERROR] No ventoy-sim.img"); return
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={VSIM_IMG},if=ide,index=0,media=disk"
        ] + base)

    # [FIX-UEFI-BOOT] Construye el comando QEMU correcto para UEFI.
    # Requiere OVMF como firmware y el disco GPT como medio de arranque.
    # En v4.9 run_qemu() en modo uefi no pasaba -bios OVMF.fd y exponía
    # la ESP FAT32 desnuda en lugar del disco GPT, por eso nunca arrancaba.
    def uefi():
        ovmf = find_ovmf()
        if not ovmf:
            log("[ERROR] OVMF.fd no encontrado. QEMU UEFI cancelado.")
            log("        Para obtener OVMF:")
            log(r"          Windows: copiar a C:\Program Files\qemu\share\OVMF.fd")
            log(f"          Proyecto: copiar a {ROOT / 'OVMF.fd'}")
            log("          MSYS2:   pacman -S mingw-w64-x86_64-ovmf")
            log("          Linux:   apt install ovmf  /  dnf install edk2-ovmf")
            return

        # UEFI_IMG ya es un disco GPT completo gracias a create_uefi_image().
        # Fallback a DISK_IMG si no se generó (nunca debería ocurrir aquí).
        tgt = UEFI_IMG if UEFI_IMG.exists() else DISK_IMG
        log(f"  OVMF:  {ovmf}")
        log(f"  Disco: {tgt}")
        subprocess.run([
            "qemu-system-x86_64",
        ] + base + [
            "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
            "-drive", f"format=raw,file={tgt},if=ide,index=0,media=disk",
        ])

    # [FIX-DUAL-RUN] En v4.9 main() en modo dual hacía return antes de
    # llamar run_qemu(), así que QEMU nunca se lanzaba.
    # Ahora run_qemu() se llama desde main() Y esta función arranca la imagen
    # BIOS (portix-dual.img) para la parte legacy. El .img UEFI queda
    # disponible en portix-uefi.img para probar por separado.
    def dual():
        tgt = DUAL_IMG if DUAL_IMG.exists() else DISK_IMG
        log(f"  Modo dual: arrancando imagen BIOS ({tgt.name})")
        log(f"  Para probar UEFI por separado: python build.py --mode=uefi")
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={tgt},if=ide,index=0,media=disk"
        ] + base)

    dispatch = {"iso": iso, "ventoy-sim": vsim, "uefi": uefi, "dual": dual}
    if mode == "both":
        t1 = threading.Thread(target=raw, daemon=True)
        t2 = threading.Thread(target=iso, daemon=True)
        t1.start(); t2.start(); t1.join(); t2.join()
    else:
        dispatch.get(mode, raw)()

# ---------------------------------------------------------------------------

def summary():
    el = time.monotonic() - _t0
    if _ISO_METHOD in ("xorriso","genisoimage","pycdlib"):
        it = f"ISO9660+El Torito no-emul+BIT ({_ISO_METHOD}, load={ISO_BOOT_LOAD_SIZE})"
        iu = "VBox unidad optica  |  QEMU -drive media=cdrom"
    else:
        it = "disco raw (sin xorriso)"; iu = "VBox IDE disco (NO CD-ROM)"
    print()
    print("╔══════════════════════════════════════════════════════════════════════════╗")
    print("║              PORTIX v5.0 — ARCHIVOS DE DISTRIBUCION                     ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    for p,lbl,uso in [
        (RAW_COPY,"IMG  ","dd/Rufus->USB | QEMU raw"),
        (ISO_IMG, "ISO  ",iu),
        (VDI_IMG, "VDI  ","VirtualBox disco IDE"),
        (VMDK_IMG,"VMDK ","VMware / VirtualBox"),
        (VSIM_IMG,"SIM  ","Test Ventoy (--mode=ventoy-sim)"),
        (UEFI_IMG,"UEFI ","GPT+ESP UEFI — QEMU con OVMF  [FIX-UEFI-BOOT]"),
        (DUAL_IMG,"DUAL ","BIOS legacy (UEFI en portix-uefi.img)  [FIX-DUAL-RUN]")]:
        e = p.exists() if p else False
        m = "OK" if e else "XX"
        i = f"{p.name:<30} {human(p):<8}  {uso}" if e else "(no generado)"
        print(f"║  {m} {lbl}  {i:<66} ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    print(f"║  ISO:  {it:<66} ║")
    ovmf = find_ovmf()
    ovmf_s = Path(ovmf).name if ovmf else "NO ENCONTRADO — ver instrucciones arriba"
    print(f"║  UEFI: disco GPT puro (Python). OVMF: {ovmf_s:<34} ║")
    print(f"║  DUAL: QEMU arranca BIOS; portix-uefi.img para UEFI                  ║")
    print(f"║  [FIX-GP-DF]: ISRs con guardia valid==0 -> VGA 0xB8000 fallback      ║")
    print(f"║  Build: {el:.1f}s{' '*63} ║")
    print("╠══════════════════════════════════════════════════════════════════════════╣")
    print("║  VBox ISO: VM->Almacenamiento->Anadir unidad optica->portix.iso        ║")
    print("║  QEMU: python build.py --mode=raw|iso|uefi|dual|ventoy-sim             ║")
    print("╚══════════════════════════════════════════════════════════════════════════╝")

def main():
    global _t0; _t0 = time.monotonic()
    print("\n╔══════════════════════════════════════╗")
    print("║   PORTIX BUILD SYSTEM  v5.0         ║")
    print("╚══════════════════════════════════════╝\n")
    if arg("--clean"): clean(); return
    reset_logs(); check_tools()
    mode = arg_val("--mode") or "raw"
    assemble_boot(); ks = build_kernel()
    assemble_stage2(ks); create_raw(ks)

    if mode == "uefi":
        build_efi_loader()
        create_uefi_image(UEFI_IMG)   # GPT+ESP  [FIX-UEFI-BOOT]
        summary()
        if not arg("--no-run"): run_qemu()   # lanza QEMU con OVMF
        return

    if mode == "dual":
        build_efi_loader()
        # Imagen BIOS para el arranque legacy
        shutil.copy2(DISK_IMG, DUAL_IMG)
        log(f"[OK]    portix-dual.img — {human(DUAL_IMG)}  (BIOS legacy)")
        # Imagen UEFI GPT para pruebas UEFI por separado
        create_uefi_image(UEFI_IMG)   # [FIX-UEFI-BOOT]
        log("[OK]    portix-dual.img (BIOS) + portix-uefi.img (UEFI GPT)")
        summary()
        # [FIX-DUAL-RUN] En v4.9 hacía `return` aquí sin llamar run_qemu().
        # Ahora sí se llama; run_qemu() en modo dual arranca DUAL_IMG en BIOS.
        if not arg("--no-run"): run_qemu()
        return

    if not arg("--no-iso"): create_iso()
    else: log("[SKIP] ISO omitida")
    if not arg("--no-vm"): create_vdi(); create_vmdk()
    else: log("[SKIP] VDI/VMDK omitidos")
    create_ventoy_sim(); summary()
    if not arg("--no-run"): run_qemu()

if __name__ == "__main__": main()