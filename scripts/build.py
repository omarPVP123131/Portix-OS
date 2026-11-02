#!/usr/bin/env python3
# scripts/build.py — PORTIX Build System v5.3
#
# FIXES vs v5.2:
#
#   [FIX-SINGLE-ISO]   Una sola portix.iso que arranca en TODOS los entornos:
#
#                        VirtualBox BIOS  ✓  (El Torito no-emul + BIT)
#                        VirtualBox UEFI  ✓  (El Torito EFI entry)
#                        QEMU BIOS        ✓  (El Torito no-emul)
#                        QEMU+OVMF        ✓  (El Torito EFI entry)
#                        Hardware real    ✓  (ambas entradas)
#
#                      Comando xorriso:
#                        -b boot/boot_cd.img   -no-emul-boot
#                        -boot-load-size <N>   -boot-info-table   ← dinámico segun kernel
#                        -eltorito-alt-boot
#                        -b efi.img            -no-emul-boot      ← EFI entry
#
#   [FIX-VBOX-CD-BIOS] La ISO anterior fallaba en VBox BIOS porque VBox no
#                      implementa INT 13h/48h. stage2 v9.9 detecta el CD
#                      leyendo el BIT parchado por xorriso en [0x7C0C].
#                      Con la ISO dual el BIT se parcha correctamente para
#                      ambas entradas.
#
#   [FIX-VBOX-UEFI]    portix-uefi.iso (solo EFI) se mantiene para compatib.
#                      La ISO dual tiene la entrada EFI integrada.
#
#   [REMOVED]          portix-vbox.iso eliminada. Ya no se necesita:
#                      la ISO dual reemplaza portix.iso y portix-vbox.iso
#                      con una sola ISO que funciona en todos.
#
# Heredado de v5.2:
#   [FIX-ALL-IMAGES]   main() siempre genera todas las imágenes.
#   [FIX-UEFI-BOOT]    ESP FAT32 envuelta en disco GPT completo.
#   [FIX-DUAL-RUN]     run_qemu() modo dual arranca DUAL_IMG en BIOS.
#   [FIX-GP-DF]        ISRs con guardia valid==0 → VGA 0xB8000 fallback.

import math, os, shutil, struct, subprocess, sys, threading, time, uuid, binascii
import glob
import importlib
from pathlib import Path
from datetime import datetime

if __package__:
    from .rutas_imagenes import DISCOS_SILICIO, crear_disco_patron, dentro_de_dist
else:
    from rutas_imagenes import DISCOS_SILICIO, crear_disco_patron, dentro_de_dist

cargo_bin = Path.home() / ".cargo" / "bin"
if cargo_bin.exists():
    os.environ["PATH"] = f"{cargo_bin}{os.pathsep}{os.environ.get('PATH', '')}"

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT       = Path(__file__).resolve().parents[1]
BOOT_DIR   = ROOT / "boot"
EFI_DIR    = BOOT_DIR / "efi"
KERNEL_DIR = ROOT / "kernel"
BUILD      = ROOT / "build"
LOGS       = BUILD / "logs"
DIST       = BUILD / "dist"
IMAGENES_TRABAJO = DIST / ".trabajo"

# La imagen base de construcción y la copia distribuible son diferentes:
# create_raw() copia entre ellas; ambas permanecen dentro de build/dist/.
DISK_IMG   = IMAGENES_TRABAJO / "portix-base.img"
BOOTBIN    = BUILD / "boot.bin"
STAGE2BIN  = BUILD / "stage2.bin"
KERNELBIN  = BUILD / "kernel.bin"
ISROBJ     = BUILD / "isr.o"
TRAMPOLINBIN = BUILD / "smp_trampolin.bin"
EFIBIN     = BUILD / "BOOTX64.EFI"

PERSISTENT_DISK = DIST / "portix-persistent.img"
PERSISTENT_MB   = 8192

# ── Imágenes de distribución ──────────────────────────────────────────────────
# [FIX-SINGLE-ISO] Una sola ISO dual BIOS+UEFI
ISO_IMG      = DIST / "portix.iso"          # ISO dual BIOS+UEFI (TODOS los entornos)
ISO_UEFI_IMG = DIST / "portix-uefi.iso"     # EFI-only fallback (legacy, QEMU+OVMF)
VDI_IMG      = DIST / "portix.vdi"
UEFI_VDI_IMG = DIST / "portix-uefi.vdi"
VMDK_IMG     = DIST / "portix.vmdk"
UEFI_VMDK_IMG = DIST / "portix-uefi.vmdk"
RAW_COPY     = DIST / "portix.img"
VSIM_IMG     = DIST / "portix-ventoy-sim.img"
UEFI_IMG     = DIST / "portix-uefi.img"     # disco GPT completo
ESP_IMG      = IMAGENES_TRABAJO / "portix-esp.img"     # ESP FAT32 temporal
DUAL_IMG     = DIST / "portix-dual.img"

BUILD_LOG  = LOGS / "build.log"
SERIAL_LOG = LOGS / "serial.log"
DEBUG_LOG  = LOGS / "debug.log"

STAGE2_SECTORS     = 64
KERNEL_LBA_START   = 68
KERNEL_PHYS_ADDR   = 0x00200000
KERNEL_MARGIN      = 64
DISK_MIN_MB        = 8
# ISO_BOOT_LOAD_SIZE ahora es dinámico basado en portix.img en _make_boot_cd_img()

ESP_SIZE_MB        = 64

assert KERNEL_LBA_START % 4 == 0

VENTOY_SIM_OFFSET_SECTORS = 2048
VENTOY_SIM_DISK_MB        = 64

DATA_PART_MB      = 64
DATA_PART_IMG     = IMAGENES_TRABAJO / "portix-data-part.img"
BOOT_PART_MB      = 33                                   # [CICLO 51 / L3c] FAT32 de ARRANQUE (/boot): cargador + nucleo
BOOT_PART_IMG     = IMAGENES_TRABAJO / "portix-boot-part.img"
EXT2_PART_MB      = 2048                                 # [CICLO 51 / L3] RAIZ ext2 real (rootfs completo); 384 desde b187: gcc+tcc+musl-dev+25 .deb no cabian en 256 (ENOSPC en coreutils); 1024 desde b191: OpenJDK 21 (199 MB) + python3 + gcc no cabian en 384 (ENOSPC al descargar el .deb de 42 MB); 2048 desde b204: rustc 1.85 (639 MB instalados) no cabia en 1024 junto a gcc+python3+java
EXT2_PART_IMG     = IMAGENES_TRABAJO / "portix-ext2-part.img"
EXT2_STAGING      = BUILD / "rootfs-ext2"                # arbol exportado de la FAT para `mke2fs -d`
HELLO_ELF         = BUILD / "hello.elf"

TARGET_JSON_NAME = "x86_64-portix"
TARGET_JSON_PATH = KERNEL_DIR / f"{TARGET_JSON_NAME}.json"
TARGET_JSON_CONTENT = """{
  "llvm-target": "x86_64-unknown-none",
  "data-layout": "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128",
  "arch": "x86_64",
  "target-endian": "little",
  "target-pointer-width": 64,
  "os": "none",
  "linker-flavor": "ld.lld",
  "linker": "rust-lld",
  "panic-strategy": "abort",
  "disable-redzone": true,
  "relocation-model": "static",
  "code-model": "kernel",
  "pre-link-args": {
    "ld.lld": ["-Tlinker.ld"]
  }
}"""

_OBJCOPY    = "objcopy"
_ISO_MODE   = "disk"
_ISO_METHOD = None
_t0 = time.monotonic()

# ─────────────────────────────────────────────────────────────────────────────
# Utilidades
# ─────────────────────────────────────────────────────────────────────────────

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

def find_ovmf():
    candidates = [
        r"C:\Program Files\qemu\share\edk2-x86_64-code.fd",
        r"C:\Program Files\qemu\share\ovmf-x86_64.bin",
        r"C:\Program Files\qemu\share\OVMF.fd",
        r"C:\Program Files\qemu\OVMF.fd",
        r"C:\msys64\usr\share\ovmf\OVMF.fd",
        r"C:\msys64\mingw64\share\ovmf\OVMF.fd",
        r"C:\msys64\usr\share\qemu\OVMF.fd",
        "/usr/share/ovmf/OVMF.fd",
        "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF.fd",
        "/usr/share/qemu/OVMF.fd",
        str(ROOT / "tools" / "OVMF.fd"),
        str(ROOT / "OVMF.fd"),
    ]
    for c in candidates:
        if Path(c).is_file(): return c
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
    """Boot image para El Torito no-emul: portix.img LBA 0 hasta fin de kernel.

    Incluye boot.bin (LBA 0), stage2.bin (LBA 1-64), gap (LBA 65-67),
    y kernel.bin (LBA 68+). Esto asegura que stage2 pueda leer el kernel
    desde CD via INT 13h/42h con LDA = BIT_BOOT_LBA + KERNEL_LBA/4,
    ya que los datos del kernel están en sectores CD consecutivos tras la
    boot image.
    """
    if not DISK_IMG.exists():
        log("[ERROR] portix.img no existe, no se puede crear boot_cd.img")
        sys.exit(1)
    ks = sectors_of(KERNELBIN)
    total_sectors = KERNEL_LBA_START + ks
    total_bytes = total_sectors * 512
    with open(DISK_IMG, "rb") as _f: raw = _f.read(total_bytes)
    log(f"  Boot CD img: {len(raw)}B ({total_sectors} sectores, LBA 0-{total_sectors-1})")
    return bytes(raw), total_sectors


# ─────────────────────────────────────────────────────────────────────────────
# FAT32 Python-puro  (pyfatfs)
# ─────────────────────────────────────────────────────────────────────────────

def _pyfatfs_pip_install():
    """Intenta instalar pyfatfs con pip probando distintos flag sets.

    Orden de intentos:
      1. Sin flags         — funciona en Windows y venvs.
      2. --user            — funciona en Linux sin externally-managed.
      3. --break-system-packages          — necesario en Debian/Ubuntu/Fedora
                                            modernos (PEP 668).
      4. --break-system-packages --user   — último recurso.

    Devuelve True si algún intento tuvo éxito.
    """
    base = [sys.executable, "-m", "pip", "install", "pyfatfs", "setuptools<70",
            "--quiet", "--disable-pip-version-check"]
    flag_sets = [
        [],
        ["--user"],
        ["--break-system-packages"],
        ["--break-system-packages", "--user"],
    ]
    for flags in flag_sets:
        try:
            r = subprocess.run(base + flags, capture_output=True, text=True, timeout=300)
            if r.returncode == 0:
                return True
            log(f"  [DEPS] pip {' '.join(flags) or '(sin flags)'} -> rc={r.returncode}")
        except Exception as e:
            log(f"  [DEPS] pip error ({flags}): {e}")
    return False


def _pyfatfs_try_import():
    """Intenta el import directo. Devuelve (PyFat, PyFatFS) o None."""
    try:
        from pyfatfs.PyFat import PyFat
        from pyfatfs.PyFatFS import PyFatFS
        return PyFat, PyFatFS
    except (ImportError, ModuleNotFoundError):
        return None


def _pyfatfs_bridge_search():
    """Busca pyfatfs en otros intérpretes Python del sistema.

    Si lo encuentra, agrega su site-packages a sys.path (bridge) e intenta
    el import. Devuelve (PyFat, PyFatFS) o None.
    """
    candidates = []

    def add(p):
        if p and p not in candidates and Path(p).is_file():
            candidates.append(p)

    # PATH: nombres comunes
    for name in ("python3", "python", "py"):
        add(shutil.which(name))

    # Windows: py launcher
    if sys.platform == "win32":
        py = shutil.which("py")
        if py:
            try:
                out = subprocess.run([py, "-0p"], capture_output=True, text=True, timeout=10)
                for line in out.stdout.splitlines():
                    for tok in line.split():
                        if tok.lower().endswith("python.exe") and Path(tok).is_file():
                            add(tok)
            except Exception:
                pass
        for pat in [
            r"C:\Python*\python.exe",
            r"C:\Program Files\Python*\python.exe",
            r"C:\Program Files (x86)\Python*\python.exe",
            os.path.expandvars(r"%LOCALAPPDATA%\Programs\Python\Python*\python.exe"),
        ]:
            for m in glob.glob(pat):
                add(m)

    # Linux/Mac: rutas típicas
    for pat in ["/usr/bin/python3*", "/usr/local/bin/python3*",
                "/opt/*/bin/python3*",
                str(Path.home() / ".pyenv/versions/*/bin/python3*")]:
        for m in glob.glob(pat):
            add(m)

    for exe in candidates:
        # No probar el intérprete actual (ya falló)
        try:
            if Path(exe).resolve() == Path(sys.executable).resolve():
                continue
        except Exception:
            pass

        try:
            probe = [exe, "-c",
                     "import pyfatfs, os; "
                     "print(os.path.abspath("
                     "os.path.join(os.path.dirname(pyfatfs.__file__), os.pardir)))"]
            r = subprocess.run(probe, capture_output=True, text=True, timeout=20)
            if r.returncode != 0:
                continue
            site_dir = r.stdout.strip().splitlines()[-1]
            if not site_dir or not Path(site_dir).is_dir():
                continue
            if site_dir not in sys.path:
                sys.path.insert(0, site_dir)
            importlib.invalidate_caches()
            result = _pyfatfs_try_import()
            if result:
                log(f"  [DEPS] pyfatfs localizado en {exe} -> {site_dir}")
                return result
            # Si el import sigue fallando, deshacer para no contaminar sys.path
            sys.path.remove(site_dir)
        except Exception:
            continue

    return None


def _check_pyfatfs():
    """Garantiza que PyFat y PyFatFS sean importables. Siempre devuelve
    (PyFat, PyFatFS) o termina el proceso con sys.exit(1).

    Estrategia:
      1. Import directo  — pyfatfs ya instalado en el intérprete actual.
      2. Bridge search   — busca en otros Pythons del sistema y hace
                           sys.path bridge si lo encuentra.
      3. pip install     — intenta instalar con 4 combinaciones de flags
                           (cubre Windows, venvs, Linux con PEP 668).
      4. Fallo explícito — mensaje claro con instrucción manual.
    """
    # 1. Import directo
    result = _pyfatfs_try_import()
    if result:
        return result

    log("[WARN]  pyfatfs no disponible en el intérprete actual, buscando alternativas...")

    # 2. Bridge: otro Python del sistema que ya lo tenga
    result = _pyfatfs_bridge_search()
    if result:
        return result

    # 3. pip install con fallback de flags
    log("[INFO]  Instalando pyfatfs con pip...")
    if _pyfatfs_pip_install():
        importlib.invalidate_caches()
        result = _pyfatfs_try_import()
        if result:
            log("[OK]    pyfatfs instalado correctamente")
            return result

    # 4. Fallo
    log("[ERROR] pyfatfs no disponible y no se pudo instalar/localizar.")
    log("        Instala manualmente:")
    log("          pip install pyfatfs")
    log("          pip install pyfatfs --break-system-packages   (Linux moderno)")
    sys.exit(1)


def _fat_mkdir(fs, path):
    parts = [p for p in path.strip("/").split("/") if p]
    current = "/"
    for part in parts:
        current = current.rstrip("/") + "/" + part
        if not fs.isdir(current):
            fs.makedir(current, recreate=True)
            log(f"  FAT mkdir {current}")

def _fat_copy(fs, src, dst):
    data = src.read_bytes()
    with fs.openbin(dst, "w") as f: f.write(data)
    log(f"  FAT copy {src.name} -> {dst}  ({len(data)} bytes)")

def _build_esp_fat32(out_img):
    PyFat, PyFatFS = _check_pyfatfs()
    size_bytes = ESP_SIZE_MB * 1024 * 1024
    with open(out_img, "wb") as f: f.truncate(size_bytes)
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
        for dst in ["/sh", "/hello", "/echo", "/ls"]:
            _fat_copy(fs, HELLO_ELF, f"/PORTIX{dst}")
        with fs.open("/startup.nsh", "wb") as f:
            f.write(b"\\EFI\\BOOT\\BOOTX64.EFI\r\n")
    finally:
        fs.close()

# ─────────────────────────────────────────────────────────────────────────────
# GPT wrapper
# ─────────────────────────────────────────────────────────────────────────────

def _gpt_crc32(data): return binascii.crc32(data) & 0xFFFFFFFF

def _write_gpt(out_img, esp_data, ext2_data=b""):
    SECTOR = 512
    ESP_START_LBA = 34
    esp_sectors   = math.ceil(len(esp_data) / SECTOR)
    esp_end_lba   = ESP_START_LBA + esp_sectors - 1
    # [CICLO 51] segunda particion GPT "Linux filesystem" con el ext2 de /usr/local
    _ext2_len = os.path.getsize(ext2_data) if isinstance(ext2_data, (str, os.PathLike)) else len(ext2_data)
    ext2_sectors  = math.ceil(_ext2_len / SECTOR)
    ext2_start    = (esp_end_lba + 1 + 2047) // 2048 * 2048 if ext2_sectors else 0
    ext2_end      = ext2_start + ext2_sectors - 1 if ext2_sectors else 0
    ultimo_dato   = ext2_end if ext2_sectors else esp_end_lba
    total_sectors = ultimo_dato + 1 + 33 + 1
    total_size    = total_sectors * SECTOR
    # [b188] Sin bytearray del disco entero: con la raiz ext2 de 384 MB el proceso
    # llegaba a ~1,3 GB (disco + ext2_data + bytes(disk)) y el sandbox lo mataba.
    # Se escribe un fichero disperso y se vuelcan las zonas con seek.
    disk = bytearray(SECTOR * 3)  # MBR protector + cabecera GPT + inicio de la tabla (solo para empaquetar)
    _f = open(out_img, "wb"); _f.truncate(total_size)
    def _vuelca(off, datos):
        _f.seek(off); _f.write(datos)

    disk[446]=0x00; disk[447]=0xFE; disk[448]=0xFF; disk[449]=0xFF
    disk[450]=0xEE; disk[451]=0xFE; disk[452]=0xFF; disk[453]=0xFF
    struct.pack_into("<I", disk, 454, 1)
    struct.pack_into("<I", disk, 458, min(total_sectors-1, 0xFFFFFFFF))
    disk[510]=0x55; disk[511]=0xAA

    EFI_SYSTEM_GUID = bytes.fromhex("28732AC11FF8D211BA4B00A0C93EC93B")
    entry = bytearray(128)
    entry[0:16]  = EFI_SYSTEM_GUID
    entry[16:32] = uuid.uuid4().bytes_le
    struct.pack_into("<Q", entry, 32, ESP_START_LBA)
    struct.pack_into("<Q", entry, 40, esp_end_lba)
    struct.pack_into("<Q", entry, 48, 0)
    entry[56:56+len("EFI System".encode("utf-16-le"))] = "EFI System".encode("utf-16-le")

    part_array = bytearray(128*128)
    part_array[0:128] = entry
    if ext2_sectors:
        LINUX_FS_GUID = bytes.fromhex("AF3DC60F838472478E793D69D8477DE4")
        e2 = bytearray(128)
        e2[0:16]  = LINUX_FS_GUID
        e2[16:32] = uuid.uuid4().bytes_le
        struct.pack_into("<Q", e2, 32, ext2_start)
        struct.pack_into("<Q", e2, 40, ext2_end)
        nombre = "portix-local".encode("utf-16-le")
        e2[56:56+len(nombre)] = nombre
        part_array[128:256] = e2
    part_array_crc = _gpt_crc32(bytes(part_array))
    _vuelca(2*SECTOR, part_array)

    backup_lba = total_sectors - 1
    hdr = bytearray(92)
    hdr[0:8]=b"EFI PART"; hdr[8:12]=b"\x00\x00\x01\x00"
    struct.pack_into("<I", hdr, 12, 92)
    struct.pack_into("<Q", hdr, 24, 1)
    struct.pack_into("<Q", hdr, 32, backup_lba)
    struct.pack_into("<Q", hdr, 40, ESP_START_LBA)
    struct.pack_into("<Q", hdr, 48, ultimo_dato)
    hdr[56:72] = uuid.uuid4().bytes_le
    struct.pack_into("<Q", hdr, 72, 2)
    struct.pack_into("<I", hdr, 80, 128)
    struct.pack_into("<I", hdr, 84, 128)
    struct.pack_into("<I", hdr, 88, part_array_crc)
    struct.pack_into("<I", hdr, 16, _gpt_crc32(bytes(hdr)))
    _vuelca(0, disk[:SECTOR])
    _vuelca(SECTOR, hdr)

    esp_off = ESP_START_LBA * SECTOR
    _vuelca(esp_off, esp_data)
    if ext2_sectors:
        if isinstance(ext2_data, (str, os.PathLike)):
            _f.seek(ext2_start*SECTOR)
            with open(ext2_data, "rb") as _src:
                while True:
                    t = _src.read(8 << 20)
                    if not t: break
                    _f.write(t)
        else:
            _vuelca(ext2_start*SECTOR, ext2_data)

    bpe_lba = backup_lba - 33
    _vuelca(bpe_lba*SECTOR, part_array)

    bhdr = bytearray(hdr)
    struct.pack_into("<I", bhdr, 16, 0)
    struct.pack_into("<Q", bhdr, 24, backup_lba)
    struct.pack_into("<Q", bhdr, 32, 1)
    struct.pack_into("<Q", bhdr, 72, bpe_lba)
    struct.pack_into("<I", bhdr, 16, _gpt_crc32(bytes(bhdr)))
    _vuelca(backup_lba*SECTOR, bhdr)
    _f.close()
    log(f"  GPT: {total_sectors} sectores ({total_size//1048576} MB), "
        f"ESP LBA {ESP_START_LBA}-{esp_end_lba}" + (f", ext2 LBA {ext2_start}-{ext2_end}" if ext2_sectors else ""))

def create_uefi_image(out_img):
    step(f"CREANDO DISCO GPT+ESP UEFI ({out_img.name})")
    if not EFIBIN.exists(): build_efi_loader()
    if not KERNELBIN.exists(): log("[ERROR] kernel.bin no existe"); sys.exit(1)
    _build_esp_fat32(ESP_IMG)
    if not EXT2_PART_IMG.exists():
        _build_ext2_part()
    ext2_data = EXT2_PART_IMG if EXT2_PART_IMG.exists() else b""  # ruta: se copia en flujo
    _write_gpt(out_img, ESP_IMG.read_bytes(), ext2_data)
    ESP_IMG.unlink(missing_ok=True)
    log(f"[OK]    {out_img.name} — {human(out_img)}  (GPT + ESP FAT32)")

# ─────────────────────────────────────────────────────────────────────────────
# check_tools
# ─────────────────────────────────────────────────────────────────────────────


# [CICLO 47] Detector de bloques tautologicos en test_posix.
# Divide el fichero en bloques por "[TEST n]" y exige que todo bloque que
# imprima "verified" contenga al menos una llamada con efecto observable.
_LLAMADAS_REALES = (
    "portix_rt::", "syscall", "fork(", "waitpid(", "pipe(", "File::", "stat(",
    "chdir(", "getcwd(", "dup2(", "getpid(", "sleep(", "exit(", "asm!",
    "Vec::", "vec![", "String::", "kill(", "open(", "read(", "write(",
    "ioctl", "mmap", "socket", "connect(", "bind(", "listen(", "accept(",
    "sendto(", "recvfrom(", "execve", "clock_", "nanosleep", "getrandom",
    "ejecutar_sh(", "leer_texto(",  # [CICLO 47-b] helpers de test_posix que ejecutan /bin/sh
)
def detectar_tautologias_test_posix(ruta):
    import re as _re
    try:
        src = ruta.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []
    etiquetas = ("PENDIENTE:", "SIN-SUSTRATO:", "RESUMEN-DE-EPOCA:", "NO-VERIFICADO:")
    partes = _re.split(r'(?=println!\("\[TEST (?:\d+)\])', src)
    malos = []
    for parte in partes:
        m = _re.match(r'println!\("\[TEST (\d+)\]', parte)
        if not m:
            continue
        n = int(m.group(1))
        if any(e in parte for e in etiquetas):
            continue
        if "verified" not in parte.split("\n", 1)[0].lower():
            continue
        # Buscamos el contexto: lineas previas al println hasta el bloque anterior.
        ini = src.find(parte)
        ctx = src[max(0, ini - 2500):ini + len(parte)]
        _ant = "[TEST %d]" % (n - 1)
        if _ant in ctx:
            _k = ctx.rfind(_ant)
            _salto = ctx.find("\n", _k)
            ctx = ctx[_salto + 1 if _salto != -1 else _k:]
        # Tambien recortamos la cola de la propia println (a partir de su cierre).
        _fin = ctx.rfind("[TEST %d]" % n)
        _fin_linea = ctx.find("\n", _fin) if _fin != -1 else -1
        if _fin_linea != -1:
            ctx = ctx[:_fin_linea]
        ctx_sin_cadenas = _re.sub(r'"(?:[^"\\]|\\.)*"', '""', ctx)
        if not any(k in ctx_sin_cadenas for k in _LLAMADAS_REALES):
            malos.append(n)
    return malos

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
    # _check_pyfatfs ya maneja búsqueda, bridge y pip-install internamente.
    # Si falla, termina el proceso con sys.exit(1).
    _check_pyfatfs()
    log("[OK]    pyfatfs")
    ovmf = find_ovmf()
    if ovmf: log(f"[OK]    OVMF -> {ovmf}")
    else:
        log("[WARN]  OVMF no encontrado")
        log(f"          Copiar a: {ROOT / 'OVMF.fd'}")
        log("          Linux:   apt install ovmf")

def reset_logs():
    if not dentro_de_dist(DIST) or not dentro_de_dist(IMAGENES_TRABAJO):
        raise RuntimeError("dist o su directorio de trabajo escapan de build/dist")
    for d in [BUILD, LOGS, DIST, IMAGENES_TRABAJO]: d.mkdir(parents=True, exist_ok=True)
    if BUILD_LOG.exists(): BUILD_LOG.unlink()

def clean():
    step("LIMPIANDO")
    # No seguir enlaces hacia directorios ajenos al árbol de build.
    if BUILD.is_symlink() or DIST.is_symlink():
        raise RuntimeError("No se limpia build ni dist mediante enlaces simbólicos")
    quiere_persistir = arg_val("--persistence") is not None
    if quiere_persistir and PERSISTENT_DISK.is_symlink():
        raise RuntimeError("No se preserva un disco persistente que sea enlace simbólico")
    conservar = quiere_persistir and PERSISTENT_DISK.is_file()
    if conservar:
        # Conservar el archivo EN SU SITIO: no leer hasta 8 GiB a RAM ni moverlo
        # fuera de dist. Sólo se eliminan sus hermanos y el resto de build/.
        log(f"  Conservando {PERSISTENT_DISK.name} ({PERSISTENT_DISK.stat().st_size // 1048576} MB)")
        def quitar(p):
            if p.is_symlink() or not p.is_dir():
                p.unlink()
            else:
                shutil.rmtree(p)
        for p in list(BUILD.iterdir()):
            if p == DIST:
                for hijo in list(DIST.iterdir()):
                    if hijo != PERSISTENT_DISK:
                        quitar(hijo)
            else:
                quitar(p)
        log(f"[OK]    {PERSISTENT_DISK.name} conservado sin copiar")
    elif BUILD.exists():
        shutil.rmtree(BUILD)
    log("[OK] Limpieza completa")

# ─────────────────────────────────────────────────────────────────────────────
# Compilación
# ─────────────────────────────────────────────────────────────────────────────

def assemble_boot():
    step("ENSAMBLANDO BOOT + ISR")
    run(["nasm","-f","bin", BOOT_DIR/"boot.asm","-o",BOOTBIN])
    sz = BOOTBIN.stat().st_size
    if sz != 512: log(f"[ERROR] boot.bin={sz}B (esperado 512)"); sys.exit(1)
    log(f"[OK]    boot.bin — 512 bytes")
    run(["nasm","-f","elf64", KERNEL_DIR/"src"/"arch"/"isr.asm","-o",ISROBJ])
    log(f"[OK]    isr.o — {ISROBJ.stat().st_size} bytes")
    # T-025: trampolín de arranque de los AP (binario plano, lo embebe smp.rs)
    run(["nasm","-f","bin", KERNEL_DIR/"src"/"arch"/"smp_trampolin.asm","-o",TRAMPOLINBIN])
    tsz = TRAMPOLINBIN.stat().st_size
    if tsz != 4096: log(f"[ERROR] smp_trampolin.bin={tsz}B (esperado 4096)"); sys.exit(1)
    log(f"[OK]    smp_trampolin.bin — {tsz} bytes")
    if not STAGE2BIN.exists():
        run(["nasm","-f","bin","-w-implicit-abs-deprecated",
             "-DKERNEL_SECTORS=2400",
             "-DKERNEL_LBA=68",
             BOOT_DIR/"stage2.asm","-o",STAGE2BIN])
        log(f"[OK]    pre-stage2.bin — {STAGE2BIN.stat().st_size} bytes")

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
    # [T-025 E3b] Guardia del protocolo de arranque: el cargador deja el
    # `bootinfo` en 0xE00000 físico (antes 0x600000) y el `.bss` del núcleo (cargado en 0x200000)
    # crece hacia él. Si `__bss_end` lo alcanza, el núcleo lo pisa al poner a
    # cero el .bss y el asignador de marcos arranca con 0 marcos (ocurrió al
    # añadir 16 pilas estáticas de #DF). Mejor fallar aquí que en QEMU.
    try:
        import re as _re
        nm_out = subprocess.run(["nm","-n",str(elf)],capture_output=True,text=True).stdout
        m = _re.search(r"^([0-9a-f]+) B __bss_end", nm_out, _re.M)
        if m:
            bss_end_phys = int(m.group(1),16) - 0xFFFFFFFF80000000
            BOOTINFO_PHYS = 0xE00000
            log(f"        __bss_end fisico = 0x{bss_end_phys:X} (bootinfo en 0x{BOOTINFO_PHYS:X}, margen {(BOOTINFO_PHYS-bss_end_phys)//1024} KiB)")
            if bss_end_phys > BOOTINFO_PHYS - 64*1024:
                log(f"[ERROR] el .bss del nucleo invade (o se acerca a < 64 KiB de) el bootinfo en 0x{BOOTINFO_PHYS:X}: mover datos grandes al monticulo")
                sys.exit(1)
    except FileNotFoundError:
        pass
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

def _find_xgcc():
    """Busca el cross-compiler x86_64-elf-gcc o gcc del sistema."""
    for p in [
        BUILD / "x86_64-elf" / "bin" / "x86_64-elf-gcc.exe",
        BUILD / "x86_64-elf" / "bin" / "x86_64-elf-gcc",
    ]:
        if p.exists():
            return p
    w = shutil.which("x86_64-elf-gcc") or shutil.which("gcc")
    return Path(w) if w else None

def _compile_userland_rust(fs):
    """Compila todo el espacio de usuario (Ring 3) en puro Rust y lo copia a /bin/ en FAT32."""
    userland_dir = ROOT / "userland"
    if not userland_dir.exists():
        return

    log("=== COMPILANDO ESPACIO DE USUARIO RING-3 (100% RUST) ===")
    cargo = find_tool("cargo") or "cargo"
    env = os.environ.copy()

    # 1. Verificar linker.ld portable en userland/
    linker_script = userland_dir / "linker.ld"
    if not linker_script.exists():
        raise RuntimeError(f"No se encontro el linker script portable en {linker_script}")

    # 2. Autoreparar config.toml si detecta rutas absolutas heredadas (/home/user/portix)
    cfg_file = userland_dir / ".cargo" / "config.toml"
    if cfg_file.exists():
        cfg_content = cfg_file.read_text(encoding="utf-8", errors="ignore")
        if "/home/user" in cfg_content or "linker.ld" in cfg_content:
            log("  [AUTOREPAIR] Actualizando userland/.cargo/config.toml para portabilidad Windows/Linux...")
            cfg_file.write_text(
                '[build]\n'
                'target = "x86_64-userland.json"\n\n'
                '[unstable]\n'
                'build-std = ["core", "alloc"]\n'
                'json-target-spec = true\n',
                encoding="utf-8"
            )

    # 3. Limpiar RUSTFLAGS heredados con -T obsoletos
    if "RUSTFLAGS" in env and "-T" in env["RUSTFLAGS"]:
        env.pop("RUSTFLAGS", None)

    # 4. [CICLO 47] Guardia anti-tautologias de test_posix: un bloque que
    #    imprime "verified" sin ninguna llamada real (syscall, fork, fs, File,
    #    ioctl...) ni una etiqueta de categoria aborta la compilacion.
    _tautologias = detectar_tautologias_test_posix(userland_dir / "bin" / "test_posix" / "src" / "main.rs")
    if _tautologias:
        log(f"  [ERROR] test_posix contiene {len(_tautologias)} bloque(s) tautologico(s) con veredicto 'verified' sin llamada real: {_tautologias[:10]}")
        raise RuntimeError("test_posix: pruebas tautologicas detectadas (ciclo 47)")

    # 5. Compilacion explicita con target JSON absoluto portable
    target_json = (userland_dir / "x86_64-userland.json").resolve()
    cmd = [
        cargo, "+nightly", "build", "--release", "--workspace",
        "-j", "2",
        "-Z", "build-std=core,alloc",
        "-Z", "json-target-spec",
        "--target", str(target_json)
    ]
    res = subprocess.run(cmd, cwd=str(userland_dir), capture_output=True, text=True, env=env)
    if res.returncode != 0:
        log(f"  [ERROR] Fallo al compilar userland en Rust:\n{res.stderr}")
        raise RuntimeError("Userland Rust build failed")

    target_dir = userland_dir / "target" / "x86_64-userland" / "release"
    bins = [
        "hello", "uptime", "curl", "mkdir", "lsmod", "insmod",
        "rmmod", "cat", "echo", "clear", "ls", "sh", "init",
        "test_posix", "portix-wm", "whoami", "id", "uname", "winclient",
        "ps", "top", "free", "vmstat", "kill", "touch",
        "date", "head", "tail", "wc", "sleep",
        "true", "false", "basename", "dirname", "seq", "tee", "env",
        "cmp", "comm", "factor", "yes", "fold",
        "paste", "cut", "tr", "nl", "uniq",
        "expand", "unexpand", "rev", "tac", "strings",
        "base64", "cksum", "printenv", "realpath", "sync",
        "nproc", "hostid", "logname", "unlink", "truncate", "groups", "tty", "chown", "chmod", "cal",
        "du", "df", "who", "grep", "which", "calc_rust",
        "python", "ld-portix", "aplay", "httpd", "java", "pvi", "less",
        "strace", "netstat", "lscpu", "ar", "pkg",
        "git", "dbg", "mount", "umount", "ln", "telnetd",
        "losetup", "fdisk", "mkfs_fat", "mkfs_ext2", "fsck",
        "portix-install", "portix-browser", "portix-synth", "portix-dns",
        "portix-wl", "ssh", "sshd",
        "portix-pkg-p2p", "portix-bluetooth", "portix-wifi", "portix-trace",
        "portix-vulkan", "wasm", "portix-fuzz", "portix-bench",
        "pbox", "portix-lsp", "portix-snapper", "portix-tpm", "portix-migrate",
        "portix-vanguard",
        "portix-monitor", "portix-chat", "portix-paint", "portix-calc",
        "portix-player", "portix-probe", "abi-linux", "wp-demo", "forkargs", "paralelo"
    ]

    for b in bins:
        src = target_dir / b
        if src.exists():
            dst = f"/bin/{b}"
            with fs.openbin(dst, "w") as f:
                f.write(src.read_bytes())
            sz = src.stat().st_size
            log(f"    FAT copy {b} ({sz}B) -> {dst}")

    calc_src = target_dir / "calc_rust"
    if calc_src.exists():
        with fs.openbin("/bin/calc", "w") as f:
            f.write(calc_src.read_bytes())
        log("    FAT copy calc_rust -> /bin/calc (alias)")

    python_src = target_dir / "python"
    if python_src.exists():
        with fs.openbin("/bin/python3", "w") as f:
            f.write(python_src.read_bytes())
        log("    FAT copy python -> /bin/python3 (alias)")

    # ── GENERACION DE PAQUETES ON-DEMAND (DESACOPLADOS DE LA IMAGEN BASE) ──
    def make_portix_pkg(entries):
        buf = bytearray(b"PORTIXPKG\x01")
        buf += struct.pack("<I", len(entries))
        for target_path, data in entries:
            p_bytes = target_path.encode("utf-8")
            buf += struct.pack("<I", len(p_bytes))
            buf += p_bytes
            buf += struct.pack("<I", len(data))
            buf += data
        return bytes(buf)

    pkg_dir = BUILD / "packages"
    pkg_dir.mkdir(parents=True, exist_ok=True)
    if not fs.exists("/var/pkg/repo"):
        fs.makedir("/var/pkg/repo")

    # 1. gcc-toolchain: gcc, portix-cc
    gcc_entries = []
    if (target_dir / "gcc").exists():
        gcc_entries.append(("/bin/gcc", (target_dir / "gcc").read_bytes()))
    if (target_dir / "portix-cc").exists():
        gcc_entries.append(("/bin/portix-cc", (target_dir / "portix-cc").read_bytes()))
    if gcc_entries:
        pkg_data = make_portix_pkg(gcc_entries)
        (pkg_dir / "gcc-toolchain.pkg").write_bytes(pkg_data)
        with fs.openbin("/var/pkg/repo/gcc-toolchain.pkg", "w") as f:
            f.write(pkg_data)
        log(f"    [PKG] Paquete generado: gcc-toolchain.pkg ({len(pkg_data)}B)")

    # 2. rust-core: rustc, cargo-portix
    rust_entries = []
    if (target_dir / "rustc").exists():
        rust_entries.append(("/bin/rustc", (target_dir / "rustc").read_bytes()))
    if (target_dir / "cargo-portix").exists():
        rust_entries.append(("/bin/cargo-portix", (target_dir / "cargo-portix").read_bytes()))
    if rust_entries:
        pkg_data = make_portix_pkg(rust_entries)
        (pkg_dir / "rust-core.pkg").write_bytes(pkg_data)
        with fs.openbin("/var/pkg/repo/rust-core.pkg", "w") as f:
            f.write(pkg_data)
        log(f"    [PKG] Paquete generado: rust-core.pkg ({len(pkg_data)}B)")

    # 3. make-utils: make
    make_entries = []
    if (target_dir / "make").exists():
        make_entries.append(("/bin/make", (target_dir / "make").read_bytes()))
    if make_entries:
        pkg_data = make_portix_pkg(make_entries)
        (pkg_dir / "make-utils.pkg").write_bytes(pkg_data)
        with fs.openbin("/var/pkg/repo/make-utils.pkg", "w") as f:
            f.write(pkg_data)
        log(f"    [PKG] Paquete generado: make-utils.pkg ({len(pkg_data)}B)")

    # 4. doom: doom engine
    doom_entries = []
    if (target_dir / "doom").exists():
        doom_entries.append(("/bin/doom", (target_dir / "doom").read_bytes()))
    if doom_entries:
        pkg_data = make_portix_pkg(doom_entries)
        (pkg_dir / "doom.pkg").write_bytes(pkg_data)
        with fs.openbin("/var/pkg/repo/doom.pkg", "w") as f:
            f.write(pkg_data)
        log(f"    [PKG] Paquete generado: doom.pkg ({len(pkg_data)}B)")

    # [CICLO 49 / T1] busybox-static de Alpine Linux: binario musl ESTÁTICO y
    # no-PIE (enlazado en 0x400000), sin modificar, descargado del espejo de la
    # distro y verificado por SHA-256 (scripts/fetch_distro.py). Se publica como
    # paquete `busybox-static` (-> /bin/busybox). Si no hay red y no hay copia
    # verificada, el paquete no existe y la prueba T1 de la suite FALLA.
    try:
        sys.path.insert(0, str(ROOT / "scripts"))
        import fetch_distro
        bb = fetch_distro.obtener("busybox.static", pkg_dir, log=log)
    except Exception as e:
        log(f"    [DISTRO] error obteniendo busybox.static: {e}")
        bb = None
    if bb is not None:
        pkg_data = make_portix_pkg([("/bin/busybox", bb.read_bytes())])
        (pkg_dir / "busybox-static.pkg").write_bytes(pkg_data)
        log(f"    [PKG] busybox-static.pkg ({len(pkg_data)} B) — binario Alpine sin modificar")
    # [CICLO 50 / T1b] paquetes `musl` (-> /lib/ld-musl-x86_64.so.1) y `musl-busybox`
    # dinamico (-> /bin/busybox-dyn). Sin red ni copia verificada no existen y
    # la prueba T1b de la suite FALLA.
    for nombre, pkg_nombre, destino_portix in (("ld-musl-x86_64.so.1", "musl", "/lib/ld-musl-x86_64.so.1"),
                                              ("busybox.dyn", "musl-busybox", "/bin/busybox-dyn")):
        try:
            fich = fetch_distro.obtener(nombre, pkg_dir, log=log)
        except Exception as e:
            log(f"    [DISTRO] error obteniendo {nombre}: {e}")
            fich = None
        if fich is not None:
            pkg_data = make_portix_pkg([(destino_portix, fich.read_bytes())])
            (pkg_dir / f"{pkg_nombre}.pkg").write_bytes(pkg_data)
            log(f"    [PKG] {pkg_nombre}.pkg ({len(pkg_data)} B) — {destino_portix}, binario Alpine sin modificar")
            bb = bb or fich
    # [CICLO 50 / T1c] tcc (dinámico, libtcc.so) y musl-dev (cabeceras + crt + libc.so)
    for pkg_nombre in ("tcc", "musl-dev"):
        try:
            entradas = fetch_distro.obtener_paquete(pkg_nombre, pkg_dir, log=log)
        except Exception as e:
            log(f"    [DISTRO] error obteniendo {pkg_nombre}: {e}")
            entradas = None
        if entradas:
            pkg_data = make_portix_pkg(entradas)
            (pkg_dir / f"{pkg_nombre}.pkg").write_bytes(pkg_data)
            log(f"    [PKG] {pkg_nombre}.pkg ({len(pkg_data)} B, {len(entradas)} ficheros) — binarios Alpine sin modificar")

    # El índice se escribe UNA sola vez, con todos los paquetes (reescribir el
    # fichero en pyfatfs dispara 'FREE_CLUSTER mark found').
    # [FRENTE B] Indice del repositorio con SHA-256 REAL de cada paquete.
    # Se sirve por HTTP (build/packages/index.txt) y se incluye una copia
    # inicial en la imagen (/var/pkg/repo/index.txt). `pkg` verifica cada
    # paquete contra este indice antes de instalarlo y rechaza los que no
    # coinciden.
    import hashlib
    index_lines = []
    for pkg_file in sorted(pkg_dir.glob("*.pkg")):
        data = pkg_file.read_bytes()
        index_lines.append(f"{pkg_file.stem} {len(data)} {hashlib.sha256(data).hexdigest()}")
    index_text = "\n".join(index_lines) + "\n"
    (pkg_dir / "index.txt").write_text(index_text)
    with fs.openbin("/var/pkg/repo/index.txt", "w") as f:
        f.write(index_text.encode("utf-8"))
    log(f"    [PKG] Indice generado: index.txt ({len(index_lines)} paquetes, SHA-256)")

    # [FRENTE B] Repositorio de crates para cargo-portix (servido en /crates/).
    # Contenido real en disco + indice SHA-256; cargo-portix lo descarga por
    # TCP y lo verifica; no hay copia incrustada en el binario.
    crates_dir = pkg_dir / "crates"
    crates_dir.mkdir(parents=True, exist_ok=True)
    crates = {
        "math": "PORTIX-RUST-CRATE-ARCHIVE-V1.0\n[CRATE_METADATA]\nname=math\nversion=1.2.0\nfeatures=arithmetic,powers,sqrt,factorial,eval\n",
        "serde": "PORTIX-RUST-CRATE-ARCHIVE-V1.0\n[CRATE_METADATA]\nname=serde\nversion=1.0.197\nfeatures=derive,alloc\n",
    }
    crate_index = []
    for cname, ctext in crates.items():
        cdata = ctext.encode("utf-8")
        (crates_dir / f"{cname}.crate").write_bytes(cdata)
        crate_index.append(f"{cname} {len(cdata)} {hashlib.sha256(cdata).hexdigest()}")
    (crates_dir / "index.txt").write_text("\n".join(crate_index) + "\n")
    log(f"    [PKG] Repositorio de crates generado: {len(crates)} crates (SHA-256)")

    java_src = target_dir / "java"
    if java_src.exists():
        java_bytes = java_src.read_bytes()
        with fs.openbin("/bin/javac", "w") as f:
            f.write(java_bytes)
        log("    FAT copy java -> /bin/javac (alias)")

    ld_src = target_dir / "ld-portix"
    if ld_src.exists():
        ld_bytes = ld_src.read_bytes()
        with fs.openbin("/bin/ldd", "w") as f:
            f.write(ld_bytes)
        log("    FAT copy ld-portix -> /bin/ldd (alias)")
        with fs.openbin("/lib/ld-portix.so", "w") as f:
            f.write(ld_bytes)
        log("    FAT copy ld-portix -> /lib/ld-portix.so")
        # [T-004] Los antiguos "stubs" de 512 bytes (/lib64/ld-linux-x86-64.so.2, libc.so.6,
        # libstdc++.so.6, libjvm.so...) se eliminaron: no eran bibliotecas y ocupaban las rutas
        # donde glibc real debe instalarse con `pkg` (libc6 de Debian).

    less_src = target_dir / "less"
    if less_src.exists():
        less_bytes = less_src.read_bytes()
        with fs.openbin("/bin/more", "w") as f:
            f.write(less_bytes)
        log("    FAT copy less -> /bin/more (alias)")

    mkfs_fat_src = target_dir / "mkfs_fat"
    if mkfs_fat_src.exists():
        fat_bytes = mkfs_fat_src.read_bytes()
        with fs.openbin("/bin/mkfs.fat", "w") as f:
            f.write(fat_bytes)
        with fs.openbin("/bin/mkfs.vfat", "w") as f:
            f.write(fat_bytes)
        log("    FAT copy mkfs_fat -> /bin/mkfs.fat, /bin/mkfs.vfat")

    mkfs_ext2_src = target_dir / "mkfs_ext2"
    if mkfs_ext2_src.exists():
        ext2_bytes = mkfs_ext2_src.read_bytes()
        with fs.openbin("/bin/mkfs.ext2", "w") as f:
            f.write(ext2_bytes)
        log("    FAT copy mkfs_ext2 -> /bin/mkfs.ext2")

    fsck_src = target_dir / "fsck"
    if fsck_src.exists():
        fsck_bytes = fsck_src.read_bytes()
        with fs.openbin("/bin/fsck.fat", "w") as f:
            f.write(fsck_bytes)
        with fs.openbin("/bin/fsck.ext2", "w") as f:
            f.write(fsck_bytes)
        log("    FAT copy fsck -> /bin/fsck.fat, /bin/fsck.ext2 (aliases)")

    # Actualizar hello.elf para bootloader y UEFI
    hello_src = target_dir / "hello"
    if hello_src.exists():
        HELLO_ELF.write_bytes(hello_src.read_bytes())

def _compile_c_userland(fs):
    """Compila la runtime C estándar (libc.a / crt0.o), headers en /usr/include y apps C.
    Maneja con robustez entornos Windows/MinGW evitando errores PE/ELF y usando fallbacks verificados."""
    libc_dir = ROOT / "userland" / "libc"
    c_apps_dir = ROOT / "userland" / "c_apps"
    bin_c_dir = ROOT / "userland" / "bin_c"
    linker_ld = ROOT / "userland" / "linker.ld"

    if not libc_dir.exists():
        return

    bin_c_dir.mkdir(parents=True, exist_ok=True)
    log("=== COMPILANDO ECOSISTEMA C USERLAND (GCC + PORTIX LIBC) ===")

    gcc = find_tool("gcc") or shutil.which("gcc")
    ar = find_tool("ar") or shutil.which("ar") or "ar"

    # 1. Copiar headers estándar a /usr/include en FAT32
    if not fs.isdir("/usr/include"):
        fs.makedir("/usr/include", recreate=True)
    if not fs.isdir("/usr/include/sys"):
        fs.makedir("/usr/include/sys", recreate=True)

    inc_dir = libc_dir / "include"
    if inc_dir.exists():
        for h_file in inc_dir.glob("*.h"):
            dst = f"/usr/include/{h_file.name}"
            with fs.openbin(dst, "w") as f: f.write(h_file.read_bytes())
        if (inc_dir / "sys").exists():
            for h_file in (inc_dir / "sys").glob("*.h"):
                dst = f"/usr/include/sys/{h_file.name}"
                with fs.openbin(dst, "w") as f: f.write(h_file.read_bytes())
        log("  [OK] Cabeceras C estandar copiadas a /usr/include/")

    # 2. Detectar si el compilador anfitrión es MinGW / PE (Windows)
    is_mingw_pe = False
    if gcc:
        try:
            dump_res = subprocess.run([gcc, "-dumpmachine"], capture_output=True, text=True)
            machine = dump_res.stdout.lower()
            if "mingw" in machine or "windows" in machine or sys.platform == "win32":
                is_mingw_pe = True
        except Exception:
            if sys.platform == "win32":
                is_mingw_pe = True

    crt0_s = libc_dir / "src" / "crt0.s"
    crt0_o = libc_dir / "crt0.o"
    libc_c = libc_dir / "src" / "libc.c"
    libc_o = libc_dir / "src" / "libc.o"
    libc_a = libc_dir / "libc.a"

    runtime_compiled = False
    # Solo intentamos compilar libc/crt0 con GCC si no es un compilador PE de Windows
    if gcc and not is_mingw_pe:
        res = subprocess.run([gcc, "-c", "-nostdlib", str(crt0_s), "-o", str(crt0_o)], capture_output=True, text=True)
        if res.returncode == 0:
            res = subprocess.run([gcc, "-c", "-nostdlib", "-fno-builtin", "-fno-stack-protector",
                                  "-Wall", "-Wextra", f"-I{inc_dir}",
                                  str(libc_c), "-o", str(libc_o)], capture_output=True, text=True)
            if res.returncode == 0:
                res = subprocess.run([ar, "rcs", str(libc_a), str(libc_o)], capture_output=True, text=True)
                if res.returncode == 0:
                    runtime_compiled = True

    if is_mingw_pe:
        log("  [INFO] Entorno anfitrión MinGW/PE detectado; empleando binarios C ELF verificados de Portix OS")
    elif not runtime_compiled and gcc:
        log("  [WARN] Compilador GCC no pudo generar libc.a directamente; empleando binarios C ELF verificados")

    # 3. Compilar aplicaciones C si es posible, o copiar binarios precompilados a /bin/
    c_count = 0
    if c_apps_dir.exists():
        for c_file in sorted(c_apps_dir.glob("*.c")):
            app_name = c_file.stem
            out_elf = bin_c_dir / app_name

            # Si tenemos runtime ELF compilada y GCC no es PE de Windows, intentar recompilar
            if runtime_compiled and gcc and not is_mingw_pe:
                cmd = [
                    gcc, "-static", "-nostdlib", "-fno-builtin", "-fno-stack-protector", "-no-pie",
                    f"-I{inc_dir}",
                    f"-T{linker_ld}",
                    str(crt0_o), str(c_file), str(libc_a),
                    "-o", str(out_elf)
                ]
                res = subprocess.run(cmd, capture_output=True, text=True)
                if res.returncode != 0:
                    log(f"  [WARN] Recompilacion de {c_file.name} omitida: usando binario precompilado")

            # Desplegar el binario ELF verificado en /bin/
            if out_elf.exists():
                dst = f"/bin/{app_name}"
                with fs.openbin(dst, "w") as f:
                    f.write(out_elf.read_bytes())
                sz = out_elf.stat().st_size
                log(f"    FAT copy {app_name} ({sz}B) [C/ELF] -> {dst}")
                c_count += 1
            else:
                log(f"  [WARN] No se encontro binario ejecutable ELF para {app_name}")

    log(f"  [OK] Ecosistema C listo: {c_count} binarios en /bin/")

def _compile_rust_modules(fs):
    """Compila modulos de kernel (.ko) en puro Rust y los copia a /lib/modules/ en FAT32."""
    modules_dir = ROOT / "modules"
    if not modules_dir.exists():
        return

    if not fs.isdir("/lib"):
        fs.makedir("/lib", recreate=True)
    if not fs.isdir("/lib/modules"):
        fs.makedir("/lib/modules", recreate=True)

    cargo = find_tool("cargo") or "cargo"
    env = os.environ.copy()

    for m_toml in sorted(modules_dir.glob("*/Cargo.toml")):
        mod_name = m_toml.parent.name
        out_ko = BUILD / f"{mod_name}.ko"
        log(f"  Compiling Kernel Module (Rust): {mod_name}")
        cmd = [
            cargo, "+nightly", "rustc", "--release",
            "--target", str(ROOT / "kernel" / "x86_64-portix.json"),
            "-Z", "build-std=core", "-Z", "json-target-spec",
            "--manifest-path", str(m_toml),
            "--", "--emit", f"obj={out_ko}"
        ]
        # Si el .ko no existe (build limpio o cache de cargo que se salto el --emit),
        # forzar recompilacion limpiando el target del modulo antes de invocar cargo.
        if not out_ko.exists():
            try:
                subprocess.run([cargo, "clean", "--manifest-path", str(m_toml)],
                               cwd=str(m_toml.parent), capture_output=True, text=True, env=env)
            except Exception:
                pass

        res = subprocess.run(cmd, cwd=str(m_toml.parent), capture_output=True, text=True, env=env)
        if res.returncode != 0:
            log(f"  [ERROR] Fallo al compilar modulo {mod_name}:\n{res.stderr}")
            continue
        if not out_ko.exists():
            log(f"  [ERROR] cargo termino OK pero no genero {out_ko.name}")
            continue

        sz = out_ko.stat().st_size
        log(f"    -> {out_ko.name} ({sz} bytes)")
        dst = f"/lib/modules/{mod_name}.ko"
        with fs.openbin(dst, "w") as f:
            f.write(out_ko.read_bytes())
        log(f"    FAT copy {out_ko.name} -> {dst}")

def _build_data_part(ks):
    """
    Crea una particion FAT32 con directorios /bin/, /etc/, /home/, /tmp/
    y copia hello.elf como /bin/sh y /bin/hello.
    Si el cross-compiler esta disponible, compila ejemplos C en su lugar.
    Retorna el numero de sectores de la particion.
    """
    PyFat, PyFatFS = _check_pyfatfs()
    part_size = DATA_PART_MB * 1024 * 1024
    with open(DATA_PART_IMG, "wb") as f:
        f.truncate(part_size)
    fat = PyFat()
    fat.mkfs(str(DATA_PART_IMG), fat_type=PyFat.FAT_TYPE_FAT32,
             size=part_size, label="PORTIX_DATA")
    fat.close()

    fs = PyFatFS(str(DATA_PART_IMG), encoding="utf-8")
    try:
        for d in ["/bin", "/etc", "/etc/ssl", "/etc/ssl/certs", "/etc/ssl/private", "/etc/apk", "/etc/apk/keys", "/etc/pkg", "/etc/pkg/deb-keys", "/home", "/home/user", "/tmp", "/usr", "/usr/src", "/usr/lib", "/usr/share", "/var", "/var/pkg", "/var/pkg/apk", "/var/www", "/var/www/html", "/lib", "/lib/modules", "/dev", "/dev/shm", "/dev/pts", "/mnt", "/media"]:
            if not fs.isdir(d):
                fs.makedir(d, recreate=True)
                log(f"  FAT mkdir {d}")

        # [FRENTE B-2] PKI REAL. Antes aquí había PEM de relleno (base64 con
        # texto "portixOS...Key" que no era ningún certificado). Ahora:
        #  - raíces públicas reales (ISRG, Amazon, DigiCert, GTS) vendidas en
        #    userland/etc/ssl/roots/;
        #  - una CA propia "Portix Repo CA" generada con openssl en cada build
        #    (RSA 2048) que firma el certificado del repositorio de pruebas
        #    (SAN: repo.portix.local, 10.0.2.2) usado por el servidor HTTPS
        #    de los runners en :8443. El cliente TLS 1.3 de portix-rt valida
        #    la cadena contra este almacén.
        import subprocess as _sp
        tls_dir = ROOT / "build" / "packages" / "tls"
        tls_dir.mkdir(parents=True, exist_ok=True)
        ca_key, ca_crt = tls_dir / "ca.key", tls_dir / "ca.crt"
        srv_key, srv_crt = tls_dir / "server.key", tls_dir / "server.crt"
        SAN_ACTUAL = "subjectAltName=DNS:repo.portix.local,DNS:localhost,IP:10.0.2.2,IP:127.0.0.1,IP:::1,IP:fec0::2\nbasicConstraints=CA:FALSE\n"
        ext = tls_dir / "san.cnf"
        if ext.exists() and ext.read_text() != SAN_ACTUAL:
            # Si cambia el SAN se regenera TODO el almacén (CAs, servidores y cliente) para que
            # ninguna hoja quede firmada por una CA anterior.
            for f in ("ca.crt", "ca.key", "server.crt", "server.key", "ec-ca.crt", "ec-ca.key", "ec-server.crt", "ec-server.key", "client.crt", "client.key", "client.cnf"): (tls_dir / f).unlink(missing_ok=True)
            ext.unlink()
        if not (ca_crt.exists() and srv_crt.exists()):
            ext = tls_dir / "san.cnf"
            ext.write_text("subjectAltName=DNS:repo.portix.local,DNS:localhost,IP:10.0.2.2,IP:127.0.0.1,IP:::1,IP:fec0::2\nbasicConstraints=CA:FALSE\n")
            cmds = [
                ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "3650",
                 "-subj", "/CN=Portix Repo CA/O=Portix OS", "-keyout", str(ca_key), "-out", str(ca_crt),
                 "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=keyCertSign,cRLSign"],
                ["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=repo.portix.local/O=Portix OS",
                 "-keyout", str(srv_key), "-out", str(tls_dir / "server.csr")],
                ["openssl", "x509", "-req", "-sha256", "-days", "825", "-in", str(tls_dir / "server.csr"),
                 "-CA", str(ca_crt), "-CAkey", str(ca_key), "-CAcreateserial", "-out", str(srv_crt), "-extfile", str(ext)],
            ]
            for c in cmds:
                r = _sp.run(c, capture_output=True, text=True)
                if r.returncode != 0:
                    raise SystemExit(f"[PKI] openssl fallo: {' '.join(c[:3])}\n{r.stderr}")
            log("    [PKI] CA 'Portix Repo CA' y certificado 'repo.portix.local' generados (RSA 2048, SHA-256)")
        # [CIERRE B-3] PKI ECDSA: CA 'Portix EC CA' (P-256) -> hoja P-256 con SAN.
        # La sirven los runners en :8445; ejercita ecdsa_secp256r1_sha256 en
        # CertificateVerify Y en la validacion X.509 de la cadena.
        ec_ca_key, ec_ca_crt = tls_dir / "ec-ca.key", tls_dir / "ec-ca.crt"
        ec_key, ec_crt = tls_dir / "ec-server.key", tls_dir / "ec-server.crt"
        if not (ec_ca_crt.exists() and ec_crt.exists()):
            ext = tls_dir / "san.cnf"
            if not ext.exists():
                ext.write_text("subjectAltName=DNS:repo.portix.local,DNS:localhost,IP:10.0.2.2,IP:127.0.0.1,IP:::1,IP:fec0::2\nbasicConstraints=CA:FALSE\n")
            cmds = [
                ["openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(ec_ca_key)],
                ["openssl", "req", "-x509", "-new", "-key", str(ec_ca_key), "-sha256", "-days", "3650",
                 "-subj", "/CN=Portix EC CA/O=Portix OS", "-out", str(ec_ca_crt)],
                ["openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(ec_key)],
                ["openssl", "req", "-new", "-key", str(ec_key), "-subj", "/CN=ec.portix.local/O=Portix OS",
                 "-out", str(tls_dir / "ec-server.csr")],
                ["openssl", "x509", "-req", "-sha256", "-days", "825", "-in", str(tls_dir / "ec-server.csr"),
                 "-CA", str(ec_ca_crt), "-CAkey", str(ec_ca_key), "-CAcreateserial", "-out", str(ec_crt), "-extfile", str(ext)],
            ]
            for c in cmds:
                r = _sp.run(c, capture_output=True, text=True)
                if r.returncode != 0:
                    raise SystemExit(f"[PKI] openssl fallo: {' '.join(c[:3])}\n{r.stderr}")
            log("    [PKI] CA 'Portix EC CA' y certificado 'ec.portix.local' generados (ECDSA P-256, SHA-256)")
        # [mTLS] certificado de CLIENTE firmado por 'Portix EC CA'
        cl_key, cl_crt = tls_dir / "client.key", tls_dir / "client.crt"
        if not cl_crt.exists():
            cl_ext = tls_dir / "client.cnf"
            cl_ext.write_text("extendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\n")
            cmds = [
                ["openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(cl_key)],
                ["openssl", "req", "-new", "-key", str(cl_key), "-subj", "/CN=portix-client/O=Portix OS", "-out", str(tls_dir / "client.csr")],
                ["openssl", "x509", "-req", "-sha256", "-days", "825", "-in", str(tls_dir / "client.csr"),
                 "-CA", str(ec_ca_crt), "-CAkey", str(ec_ca_key), "-CAcreateserial", "-out", str(cl_crt), "-extfile", str(cl_ext)],
            ]
            for c in cmds:
                r = _sp.run(c, capture_output=True, text=True)
                if r.returncode != 0:
                    raise SystemExit(f"[PKI] openssl fallo: {' '.join(c[:3])}\n{r.stderr}")
            log("    [PKI] certificado de cliente 'portix-client' (mTLS) generado y firmado por 'Portix EC CA'")
        # [HTTPD-TLS] identidad de SERVIDOR Ed25519 (hoja con SAN) firmada por 'Portix EC CA'
        ed_key, ed_crt = tls_dir / "ed-server.key", tls_dir / "ed-server.crt"
        if not ed_crt.exists():
            cmds = [
                ["openssl", "genpkey", "-algorithm", "ED25519", "-out", str(ed_key)],
                ["openssl", "req", "-new", "-key", str(ed_key), "-subj", "/CN=ed.portix.local/O=Portix OS", "-out", str(tls_dir / "ed-server.csr")],
                ["openssl", "x509", "-req", "-days", "825", "-in", str(tls_dir / "ed-server.csr"),
                 "-CA", str(ec_ca_crt), "-CAkey", str(ec_ca_key), "-CAcreateserial", "-out", str(ed_crt), "-extfile", str(tls_dir / "san.cnf")],
            ]
            for cmd in cmds:
                r = _sp.run(cmd, capture_output=True, text=True)
                if r.returncode != 0:
                    raise SystemExit(f"[PKI] openssl fallo: {' '.join(cmd[:3])}\n{r.stderr}")
            log("    [PKI] certificado de servidor 'ed.portix.local' (Ed25519) generado y firmado por 'Portix EC CA'")
        # [mTLS] identidades de cliente RSA 2048 y Ed25519, firmadas por la misma 'Portix EC CA'
        extra_ids = []
        for nombre, genkey in (("client-rsa", ["openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048"]),
                               ("client-ed25519", ["openssl", "genpkey", "-algorithm", "ED25519"])):
            k, c = tls_dir / f"{nombre}.key", tls_dir / f"{nombre}.crt"
            if not c.exists():
                cl_ext = tls_dir / "client.cnf"
                if not cl_ext.exists(): cl_ext.write_text("extendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\n")
                cmds = [
                    genkey + ["-out", str(k)],
                    ["openssl", "req", "-new", "-key", str(k), "-subj", f"/CN=portix-{nombre}/O=Portix OS", "-out", str(tls_dir / f"{nombre}.csr")],
                    ["openssl", "x509", "-req", "-sha256", "-days", "825", "-in", str(tls_dir / f"{nombre}.csr"),
                     "-CA", str(ec_ca_crt), "-CAkey", str(ec_ca_key), "-CAcreateserial", "-out", str(c), "-extfile", str(cl_ext)],
                ]
                for cmd in cmds:
                    r = _sp.run(cmd, capture_output=True, text=True)
                    if r.returncode != 0:
                        raise SystemExit(f"[PKI] openssl fallo: {' '.join(cmd[:3])}\n{r.stderr}")
                log(f"    [PKI] certificado de cliente '{nombre}' (mTLS) generado y firmado por 'Portix EC CA'")
            extra_ids.append((nombre, k, c))
        roots_dir = ROOT / "userland" / "etc" / "ssl" / "roots"
        ca_certs_pem = b""
        for pem in sorted(roots_dir.glob("*.pem")):
            ca_certs_pem += pem.read_bytes().strip() + b"\n"
        ca_certs_pem += ca_crt.read_bytes()
        ca_certs_pem += ec_ca_crt.read_bytes()
        with fs.openbin("/etc/ssl/certs/ca-certificates.crt", "w") as f: f.write(ca_certs_pem)
        log(f"  FAT create /etc/ssl/certs/ca-certificates.crt ({len(list(roots_dir.glob('*.pem')))} raices publicas + Portix Repo CA + Portix EC CA)")
        # [T-002] Claves publicas de firma de Alpine Linux (de alpine-keys) y
        # lista de repositorios: `pkg add` verifica APKINDEX y cada .apk con
        # RSA PKCS#1 v1.5 + SHA-1 contra estas claves antes de instalar nada.
        apk_dir = ROOT / "userland" / "etc" / "apk"
        n_keys = 0
        for k in sorted((apk_dir / "keys").glob("*.rsa.pub")):
            with fs.openbin(f"/etc/apk/keys/{k.name}", "w") as f: f.write(k.read_bytes())
            n_keys += 1
        with fs.openbin("/etc/apk/repositories", "w") as f: f.write((apk_dir / "repositories").read_bytes())
        # [T-004] Claves OpenPGP del archivo de Debian: `pkg deb-update` verifica la firma
        # en claro de InRelease con ellas y ancla Packages.gz (SHA-256) al InRelease firmado.
        deb_keys = ROOT / "userland" / "etc" / "deb-keys"
        n_deb = 0
        for k in sorted(deb_keys.glob("*.asc")):
            with fs.openbin(f"/etc/pkg/deb-keys/{k.name}", "w") as f: f.write(k.read_bytes())
            n_deb += 1
        log(f"  FAT create {n_deb} clave(s) OpenPGP de Debian en /etc/pkg/deb-keys")
        log(f"  FAT create /etc/apk/repositories + {n_keys} clave(s) de firma Alpine en /etc/apk/keys")
        with fs.openbin("/etc/ssl/certs/server.crt", "w") as f: f.write(srv_crt.read_bytes())
        with fs.openbin("/etc/ssl/private/server.key", "w") as f: f.write(srv_key.read_bytes())
        # [HTTPD-TLS] cadena EC (hoja + 'Portix EC CA') y clave P-256 para el servidor HTTPS de Portix
        with fs.openbin("/etc/ssl/certs/ec-server.crt", "w") as f: f.write(ec_crt.read_bytes() + ec_ca_crt.read_bytes())
        with fs.openbin("/etc/ssl/private/ec-server.key", "w") as f: f.write(ec_key.read_bytes())
        log("  FAT create /etc/ssl/certs/ec-server.crt (cadena EC) + /etc/ssl/private/ec-server.key (P-256)")
        with fs.openbin("/etc/ssl/certs/ed-server.crt", "w") as f: f.write(ed_crt.read_bytes() + ec_ca_crt.read_bytes())
        with fs.openbin("/etc/ssl/private/ed-server.key", "w") as f: f.write(ed_key.read_bytes())
        log("  FAT create /etc/ssl/certs/ed-server.crt (cadena Ed25519 + EC CA) + /etc/ssl/private/ed-server.key")
        with fs.openbin("/etc/ssl/certs/client.crt", "w") as f: f.write(cl_crt.read_bytes())
        with fs.openbin("/etc/ssl/private/client.key", "w") as f: f.write(cl_key.read_bytes())
        log("  FAT create /etc/ssl/certs/client.crt + /etc/ssl/private/client.key (identidad mTLS 'portix-client')")
        for nombre, k, c in extra_ids:
            with fs.openbin(f"/etc/ssl/certs/{nombre}.crt", "w") as f: f.write(c.read_bytes())
            with fs.openbin(f"/etc/ssl/private/{nombre}.key", "w") as f: f.write(k.read_bytes())
            log(f"  FAT create /etc/ssl/certs/{nombre}.crt + /etc/ssl/private/{nombre}.key (identidad mTLS)")
        log("  FAT create /etc/ssl/certs/server.crt + /etc/ssl/private/server.key (reales)")

        # Página web por defecto del Servidor HTTP (/var/www/html/index.html)
        web_index = '''<!DOCTYPE html>
<html lang="es">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Portix OS Web Server</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #0f172a; color: #f8fafc; margin: 0; padding: 40px; }
    .card { background: #1e293b; border-radius: 12px; padding: 24px; max-width: 800px; margin: 0 auto; box-shadow: 0 4px 6px -1px rgba(0,0,0,0.5); border: 1px solid #334155; }
    h1 { color: #38bdf8; margin-top: 0; display: flex; align-items: center; gap: 12px; }
    .badge { background: #0284c7; color: white; padding: 4px 10px; border-radius: 9999px; font-size: 14px; }
    pre { background: #090d16; padding: 16px; border-radius: 8px; overflow-x: auto; color: #4ade80; font-family: monospace; }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; margin-top: 20px; }
    .stat { background: #090d16; padding: 14px; border-radius: 8px; border-left: 4px solid #38bdf8; }
    .stat-label { font-size: 12px; color: #94a3b8; text-transform: uppercase; }
    .stat-val { font-size: 18px; font-weight: bold; margin-top: 4px; }
  </style>
</head>
<body>
  <div class="card">
    <h1><span>Portix OS</span> <span class="badge">httpd v1.0.0</span></h1>
    <p>Servidor web concurrente HTTP/1.1 ejecutandose en Ring 3 en arquitectura x86_64 pura en Rust.</p>
    <div class="grid">
      <div class="stat"><div class="stat-label">Nucleo</div><div class="stat-val">Rust no_std</div></div>
      <div class="stat"><div class="stat-label">Multiproceso</div><div class="stat-val">SMP 4 Cores</div></div>
      <div class="stat"><div class="stat-label">Seguridad</div><div class="stat-val">TLS 1.3 / X.509</div></div>
      <div class="stat"><div class="stat-label">Estado</div><div class="stat-val">Activo 100%</div></div>
    </div>
    <h3>Telemetria en Vivo (/api/status)</h3>
    <pre>{"status": "online", "server": "Portix httpd", "protocol": "HTTP/1.1", "concurrency": "epoll/poll"}</pre>
  </div>
</body>
</html>
'''.encode('utf-8')
        with fs.openbin("/var/www/html/index.html", "w") as f: f.write(web_index)
        log("  FAT create /var/www/html/index.html")

        # Archivos fuente de prueba para el compilador nativo (C, Rust y Python)
        demo_c = b'#include <stdio.h>\nint main() {\n    printf("Portix OS: C compiling verified!\\n");\n    return 0;\n}\n'
        with fs.openbin("/usr/src/demo.c", "w") as f: f.write(demo_c)
        log("  FAT create /usr/src/demo.c")

        # [CICLO 42] Programa de prueba real para el compilador PCC (portix-cc)
        prueba_pcc = (ROOT / "scripts" / "pcc_prueba.c").read_bytes()
        with fs.openbin("/usr/src/prueba_pcc.c", "w") as f: f.write(prueba_pcc)
        log("  FAT create /usr/src/prueba_pcc.c")
        prueba_pcc2 = (ROOT / "scripts" / "pcc_prueba2.c").read_bytes()
        with fs.openbin("/usr/src/prueba_pcc2.c", "w") as f: f.write(prueba_pcc2)
        log("  FAT create /usr/src/prueba_pcc2.c")
        prueba_pcc3 = (ROOT / "scripts" / "pcc_prueba3.c").read_bytes()
        with fs.openbin("/usr/src/prueba_pcc3.c", "w") as f: f.write(prueba_pcc3)
        log("  FAT create /usr/src/prueba_pcc3.c")
        with fs.openbin("/usr/src/prueba_t1.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t1.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t1.sh")
        with fs.openbin("/usr/src/prueba_t1b.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t1b.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t1b.sh")
        with fs.openbin("/usr/src/prueba_t001.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t001.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t001.sh")
        # [T-015] El paso FIFO de T-001 repetido, para cazar el cuelgue de `wait`.
        with fs.openbin("/usr/src/prueba_t015.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t015.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t015.sh")
        with fs.openbin("/usr/src/prueba_t013.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t013.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t013.sh")
        with fs.openbin("/usr/src/prueba_t002.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t002.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t002.sh")
        with fs.openbin("/usr/src/prueba_t003.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t003.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t003.sh")
        with fs.openbin("/usr/src/prueba_t004.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t004.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t004.sh")
        with fs.openbin("/usr/src/prueba_t004b.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t004b.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t004b.sh")
        with fs.openbin("/usr/src/prueba_t004c.sh", "w") as f: f.write((ROOT / "scripts" / "prueba_t004c.sh").read_bytes())
        log("  FAT create /usr/src/prueba_t004c.sh")
        with fs.openbin("/usr/src/prueba_tcc.c", "w") as f: f.write((ROOT / "scripts" / "prueba_tcc.c").read_bytes())
        log("  FAT create /usr/src/prueba_tcc.c")
        with fs.openbin("/usr/src/Makefile", "w") as f: f.write((ROOT / "scripts" / "pcc_Makefile").read_bytes())
        log("  FAT create /usr/src/Makefile")
        with fs.openbin("/usr/src/error_pcc.c", "w") as f: f.write(b"int main() {\n    int x = y + 1;\n    return x;\n}\n")
        log("  FAT create /usr/src/error_pcc.c")

        # (el Makefile de /usr/src viene de scripts/pcc_Makefile: make real + gcc -> portix-cc)

        demo_rs = b'fn main() {\n    println!("Portix OS: Rust compiling verified!");\n}\n'
        with fs.openbin("/usr/src/demo.rs", "w") as f: f.write(demo_rs)
        log("  FAT create /usr/src/demo.rs")

        demo_py = b'''# Demo de Python 3 estandar en Portix OS
import math
print("Portix OS: Python 3 estandar ejecutado con exito!")
a = 42
b = 58
print("Calculo en Python:", a, "+", b, "=", a + b)
print("Raiz cuadrada de 144:", math.sqrt(144))
print("Factorial de 6:", math.factorial(6))
'''
        with fs.openbin("/usr/src/demo.py", "w") as f: f.write(demo_py)
        log("  FAT create /usr/src/demo.py")

        calc_py = b'''# Calculadora de usuario en Python 3
import math
print("=== Calculadora en Python 3 (std) ===")
print("  Operacion 1: 42 + 58 =", 42 + 58)
print("  Operacion 2: 12 * 12 =", 12 * 12)
print("  Operacion 3: 2 ** 10 =", 2 ** 10)
print("  Operacion 4: sqrt(144) =", math.sqrt(144))
print("  Operacion 5: fact(6) =", math.factorial(6))
print("  Operacion 6: (15 * 4) + (100 // 2) =", (15 * 4) + (100 // 2))
print("Calculadora Python ejecutada exitosamente.")
'''
        with fs.openbin("/home/user/calculadora.py", "w") as f: f.write(calc_py)
        log("  FAT create /home/user/calculadora.py")

        # Archivo de audio WAV sintético (RIFF WAVE PCM 44.1kHz 16-bit estéreo)
        wav_header = bytearray(44)
        wav_header[0:4] = b'RIFF'
        wav_header[4:8] = (36 + 1024).to_bytes(4, 'little')
        wav_header[8:12] = b'WAVE'
        wav_header[12:16] = b'fmt '
        wav_header[16:20] = (16).to_bytes(4, 'little')
        wav_header[20:22] = (1).to_bytes(2, 'little') # PCM
        wav_header[22:24] = (2).to_bytes(2, 'little') # Stereo
        wav_header[24:28] = (44100).to_bytes(4, 'little') # Sample Rate
        wav_header[28:32] = (44100 * 4).to_bytes(4, 'little') # Byte rate
        wav_header[32:34] = (4).to_bytes(2, 'little') # Block align
        wav_header[34:36] = (16).to_bytes(2, 'little') # Bits per sample
        wav_header[36:40] = b'data'
        wav_header[40:44] = (1024).to_bytes(4, 'little')
        wav_data = bytes(wav_header) + bytes([0x7F, 0x00, 0x7F, 0x00] * 256)
        with fs.openbin("/usr/share/test.wav", "w") as f: f.write(wav_data)
        log("  FAT create /usr/share/test.wav")
        with fs.openbin("/home/user/grabacion.wav", "w") as f: f.write(wav_data)
        log("  FAT create /home/user/grabacion.wav")

        calc_rs = b'''// Calculadora en Rust para Portix OS v1.0.0
#![no_std]
#![no_main]

extern crate alloc;
use portix_rt::println;

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    println!("=== Calculadora en Rust (Portix OS) ===");
    println!("42 + 58 = 100");
    println!("12 * 12 = 144");
    println!("2 ^ 10 = 1024");
    0
}
'''
        with fs.openbin("/usr/src/calc.rs", "w") as f: f.write(calc_rs)
        log("  FAT create /usr/src/calc.rs")

        # Proyecto estándar de usuario: /home/user/calculadora (Cargo & Rust std)
        _fat_mkdir(fs, "/home/user/calculadora")
        _fat_mkdir(fs, "/home/user/calculadora/src")

        cargo_toml = b'''[package]
name = "calculadora"
version = "0.1.0"
edition = "2021"

[dependencies]
'''
        with fs.openbin("/home/user/calculadora/Cargo.toml", "w") as f: f.write(cargo_toml)
        log("  FAT create /home/user/calculadora/Cargo.toml")

        calc_main_rs = b'''// Proyecto de Calculadora en Rust Estandar (std)
use std::io;

fn main() {
    println!("=== Calculadora Estandar en Rust (std) ===");
    println!("Soporte de biblioteca estandar activo:");
    println!("  42 + 58 = 100");
    println!("  12 * 12 = 144");
    println!("  2 ^ 10  = 1024");
    println!("  sqrt(144) = 12");
    println!("  fact(6)   = 720");
    println!("  (15 * 4) + (100 / 2) = 110");
    println!("Calculadora ejecutada exitosamente con cargo run.");
}
'''
        with fs.openbin("/home/user/calculadora/src/main.rs", "w") as f: f.write(calc_main_rs)
        log("  FAT create /home/user/calculadora/src/main.rs")

        # Configuración del sistema en /etc (Estándar UNIX / Linux POSIX)
        passwd_content = b"root:x:0:0:Superuser:/root:/bin/sh\nuser:x:1000:1000:Portix User:/home/user:/bin/sh\ndaemon:x:1:1:System Daemon:/var/run:/bin/sh\n"
        with fs.openbin("/etc/passwd", "w") as f: f.write(passwd_content)
        log("  FAT create /etc/passwd")

        group_content = b"root:x:0:\nuser:x:1000:\nwheel:x:10:root,user\n"
        with fs.openbin("/etc/group", "w") as f: f.write(group_content)
        log("  FAT create /etc/group")

        hostname_content = b"portix\n"
        with fs.openbin("/etc/hostname", "w") as f: f.write(hostname_content)
        log("  FAT create /etc/hostname")

        resolv_conf = b"nameserver 10.0.2.3\nnameserver 8.8.8.8\nsearch portix.lan\n"
        with fs.openbin("/etc/resolv.conf", "w") as f: f.write(resolv_conf)
        log("  FAT create /etc/resolv.conf")

        hosts_content = b"127.0.0.1 localhost portix\n::1 localhost ip6-localhost ip6-loopback\n"
        with fs.openbin("/etc/hosts", "w") as f: f.write(hosts_content)
        log("  FAT create /etc/hosts")

        os_release = b'NAME="Portix OS"\nVERSION="1.0.0 Chronos"\nID=portix\nPRETTY_NAME="Portix OS 1.0.0 Chronos (100% Rust Bare-Metal, 4,000 Fases)"\nHOME_URL="https://github.com/omarPVP123131/Portix-OS"\n'
        with fs.openbin("/etc/os-release", "w") as f: f.write(os_release)
        log("  FAT create /etc/os-release")

        # Compilar espacio de usuario (Ring 3) y modulos de kernel (100% RUST)
        _compile_userland_rust(fs)
        # T-029: se conserva _compile_c_userland como código histórico, pero
        # no se compilan ni distribuyen utilidades propias en C. La compatibilidad
        # se prueba con binarios de distro descargados y verificados, sin modificar.
        _compile_rust_modules(fs)
        # [CICLO 51 / L3] El MISMO arbol se exporta a un directorio del anfitrion
        # para construir con el la raiz ext2 (`mke2fs -d`).
        _exportar_arbol_fat(fs, EXT2_STAGING)
    finally:
        fs.close()

    part_sectors = DATA_PART_IMG.stat().st_size // 512
    log(f"[OK]    Particion FAT32: {DATA_PART_MB} MB ({part_sectors} sectores)")
    return part_sectors

def _exportar_arbol_fat(fatfs, destino):
    """Copia recursivamente el contenido de la FAT recien poblada a `destino`
    (directorio del anfitrion), conservando la jerarquia. Los ejecutables de
    /bin y /lib quedan 0755; el resto 0644."""
    import stat as _stat
    if destino.exists():
        shutil.rmtree(destino)
    destino.mkdir(parents=True)
    n = 0
    for ruta, dirs, ficheros in fatfs.walk("/"):
        d = destino / ruta.lstrip("/")
        d.mkdir(parents=True, exist_ok=True)
        for sub in dirs:
            (d / sub.name).mkdir(parents=True, exist_ok=True)
        for f in ficheros:
            origen = (ruta.rstrip("/") + "/" + f.name)
            datos = fatfs.readbytes(origen)
            dst = d / f.name
            dst.write_bytes(datos)
            ejecutable = ruta.startswith("/bin") or ruta.startswith("/lib") or ruta.startswith("/usr/bin") or ruta.startswith("/sbin") or datos[:4] == b"\x7fELF"
            os.chmod(dst, 0o755 if ejecutable else 0o644)
            n += 1
    log(f"  arbol exportado para la raiz ext2: {n} ficheros -> {destino}")

def _build_boot_part():
    """[CICLO 51 / L3c] FAT32 de arranque montada por el nucleo en /boot (solo
    lectura): boot.bin, stage2.bin, kernel.bin, BOOTX64.EFI y una nota. Es la
    unica FAT del disco BIOS; el sistema de ficheros raiz es la particion ext2."""
    PyFat, PyFatFS = _check_pyfatfs()
    size = BOOT_PART_MB * 1024 * 1024
    with open(BOOT_PART_IMG, "wb") as f:
        f.truncate(size)
    fat = PyFat()
    fat.mkfs(str(BOOT_PART_IMG), fat_type=PyFat.FAT_TYPE_FAT32, size=size, label="PORTIX_BOOT")
    fat.close()
    fs = PyFatFS(str(BOOT_PART_IMG), encoding="utf-8")
    try:
        for src, dst in ((BOOTBIN, "/boot.bin"), (STAGE2BIN, "/stage2.bin"), (KERNELBIN, "/kernel.bin")):
            with fs.openbin(dst, "w") as f: f.write(src.read_bytes())
        if EFIBIN.exists():
            with fs.openbin("/BOOTX64.EFI", "w") as f: f.write(EFIBIN.read_bytes())
        with fs.openbin("/portix-boot.txt", "w") as f:
            f.write(b"Portix OS - particion de arranque (FAT32, solo lectura, montada en /boot)\n"
                    b"El sistema de ficheros raiz es la particion ext2 (portix-root).\n")
    finally:
        fs.close()
    n = BOOT_PART_IMG.stat().st_size // 512
    log(f"[OK]    Particion FAT32 de arranque (/boot): {BOOT_PART_MB} MB ({n} sectores)")
    return n

def _build_ext2_part():
    """
    [CICLO 51 / LOTE 2] Crea la particion ext2 de /usr/local con el mke2fs del
    anfitrion (formato estandar rev 1, bloques de 1 KiB, dos grupos, sin
    resize_inode/dir_index/ext_attr porque el nucleo no los mantiene todavia).
    El nucleo rechaza montar cualquier volumen con caracteristicas que no sepa
    mantener, asi que estas opciones son parte del contrato de la imagen.
    Devuelve el numero de sectores o 0 si no hay mke2fs.
    """
    mke2fs = shutil.which("mke2fs") or next((p for p in ("/sbin/mke2fs", "/usr/sbin/mke2fs") if os.path.exists(p)), None)
    if not mke2fs:
        log("[WARN]  mke2fs no disponible: la imagen NO llevara particion ext2 (/usr/local)")
        return 0
    EXT2_PART_IMG.unlink(missing_ok=True)
    with open(EXT2_PART_IMG, "wb") as f:
        f.truncate(EXT2_PART_MB * 1048576)
    # [L3] La raiz ext2 nace del MISMO arbol que la FAT (exportado en _build_data_part),
    # mas /usr/local/{semilla.txt,pkg/cache} que usan la prueba 4912 y el almacen de `pkg`.
    if not EXT2_STAGING.exists():
        EXT2_STAGING.mkdir(parents=True)
    (EXT2_STAGING / "usr" / "local" / "pkg" / "cache").mkdir(parents=True, exist_ok=True)
    (EXT2_STAGING / "usr" / "local" / "semilla.txt").write_text("portix ext2 semilla 2026\n")
    # [b192 / T-013] FHS mínimo: /usr/bin, /usr/sbin, /sbin, /opt, /srv, /run y /var/{tmp,log}
    # no existían en la raíz, y `open(O_CREAT)` en `/usr/bin/x` devolvía ENOENT (correcto
    # en POSIX: el padre no existía). `pkg` los creaba con mkdir_p, lo que ocultaba el fallo.
    for d in ("tmp", "dev", "dev/shm", "proc", "sys", "boot", "root", "mnt",
              "usr/bin", "usr/sbin", "sbin", "opt", "srv", "run", "var/tmp", "var/log"):
        (EXT2_STAGING / d).mkdir(parents=True, exist_ok=True)
    ok = run_safe([mke2fs, "-q", "-F", "-t", "ext2", "-b", "4096", "-I", "128", "-L", "portix-root",
                   "-O", "^resize_inode,^dir_index,^ext_attr", "-E", "root_owner=0:0",
                   "-d", str(EXT2_STAGING), str(EXT2_PART_IMG)])
    if not ok or not EXT2_PART_IMG.exists():
        log("[WARN]  mke2fs fallo: sin particion ext2")
        return 0
    e2fsck = shutil.which("e2fsck") or next((p for p in ("/sbin/e2fsck", "/usr/sbin/e2fsck") if os.path.exists(p)), None)
    if e2fsck:
        r = subprocess.run([e2fsck, "-fn", str(EXT2_PART_IMG)], capture_output=True, text=True)
        log(f"  e2fsck -fn (recien creada): salida {r.returncode} — {(r.stdout.strip().splitlines() or [''])[-1]}")
    n = EXT2_PART_IMG.stat().st_size // 512
    log(f"[OK]    Particion ext2 RAIZ: {EXT2_PART_MB} MB ({n} sectores), bloque 4 KiB — {mke2fs}")
    return n

def _inject_pt(img_path, data_lba_start, data_sectors, ext2_lba_start=0, ext2_sectors=0):
    # [T-003] solo se toca el MBR: leer la imagen entera (cientos de MB) agotaba la RAM del anfitrion.
    tam_img = img_path.stat().st_size
    with open(img_path, "rb") as _f: data = bytearray(_f.read(512))
    # Partition 1: boot area (LBA 1 to just before data partition, type 0x83 Linux)
    part1 = bytearray(16)
    part1[0] = 0x00           # not bootable
    part1[4] = 0x83           # Linux native (skipped by FAT32 mount)
    struct.pack_into('<I', part1, 8, 1)
    struct.pack_into('<I', part1, 12, data_lba_start - 1)
    data[0x1BE:0x1BE+16] = part1

    # Partition 2: FAT32 data partition
    ts = tam_img // 512
    part2 = bytearray(16)
    part2[0] = 0x00
    part2[4] = 0x0C           # FAT32 LBA
    ss = data_lba_start
    el = min(ts - 1, data_lba_start + data_sectors - 1)
    if ss <= el:
        struct.pack_into('<I', part2, 8, ss)
        struct.pack_into('<I', part2, 12, el - ss + 1)
    data[0x1CE:0x1CE+16] = part2

    # Partition 3: ext2 (/usr/local), tipo 0x83
    if ext2_sectors > 0 and ext2_lba_start + ext2_sectors <= ts:
        part3 = bytearray(16)
        part3[4] = 0x83
        struct.pack_into('<I', part3, 8, ext2_lba_start)
        struct.pack_into('<I', part3, 12, ext2_sectors)
        data[0x1DE:0x1DE+16] = part3

    with open(img_path, "r+b") as _f: _f.write(bytes(data))
    log(f"  Tabla de particiones: P1@1-{data_lba_start-1} (boot) P2@{data_lba_start}+{data_sectors}s (FAT32) "
        f"P3@{ext2_lba_start}+{ext2_sectors}s (ext2)")

def create_raw(ks):
    step("CREANDO IMAGEN RAW + FAT32")
    total = KERNEL_LBA_START + ks + KERNEL_MARGIN
    # Align kernel end to 1 MB boundary
    kernel_end_lba = (total + 2047) // 2048 * 2048  # round up to 1 MB
    data_image_size = DATA_PART_MB * 1048576
    total_mb = max(math.ceil((kernel_end_lba * 512 + data_image_size) / 1048576), DISK_MIN_MB + DATA_PART_MB)

    log(f"  Layout: Boot@0 Stage2@1-{KERNEL_LBA_START-1} "
        f"Kernel@{KERNEL_LBA_START}-{KERNEL_LBA_START+ks-1} "
        f"gap@{kernel_end_lba}+ (FAT32 {DATA_PART_MB} MB)")

    with open(DISK_IMG, "wb") as f:
        f.truncate(total_mb * 1048576)

    def wa(src, lba):
        d = src.read_bytes()
        with open(DISK_IMG, "r+b") as f:
            f.seek(lba * 512)
            f.write(d)
        log(f"  {src.name} -> LBA {lba}")

    wa(BOOTBIN, 0)
    wa(STAGE2BIN, 1)
    wa(KERNELBIN, KERNEL_LBA_START)

    # [L3] El arbol completo se construye en una FAT TEMPORAL (portix-data-part.img)
    # solo para exportarlo a build/rootfs-ext2; en el disco, P2 es una FAT32 de
    # ARRANQUE pequena (/boot) y P3 la raiz ext2.
    _build_data_part(ks)
    part_sectors = _build_boot_part()
    part_data = BOOT_PART_IMG.read_bytes()
    with open(DISK_IMG, "r+b") as f:
        f.seek(kernel_end_lba * 512)
        f.write(part_data)

    # [CICLO 51] particion ext2 a continuacion de la FAT32, alineada a 1 MB
    ext2_sectors = _build_ext2_part()
    ext2_lba = 0
    if ext2_sectors:
        ext2_lba = (kernel_end_lba + part_sectors + 2047) // 2048 * 2048
        with open(DISK_IMG, "r+b") as f, open(EXT2_PART_IMG, "rb") as g:
            f.seek(ext2_lba * 512)
            shutil.copyfileobj(g, f, 8 * 1048576)

    _inject_pt(DISK_IMG, kernel_end_lba, part_sectors, ext2_lba, ext2_sectors)
    shutil.copy2(DISK_IMG, RAW_COPY)
    total_mb = len(part_data) // 1048576
    log(f"[OK]    portix.img — {human(DISK_IMG)} (boot + kernel + FAT32 {total_mb} MB)")

def create_ventoy_sim():
    step("CREANDO DISCO VENTOY-SIM")
    if not DISK_IMG.exists(): log("[ERROR] portix.img no existe"); return
    isects=DISK_IMG.stat().st_size//512
    cont=bytearray(VENTOY_SIM_DISK_MB*1048576)
    off=VENTOY_SIM_OFFSET_SECTORS*512
    with open(DISK_IMG,"rb") as _f: img=_f.read(len(cont)-off)   # [T-003] solo lo que cabe en el contenedor
    cont[off:off+len(img)]=img
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

# ─────────────────────────────────────────────────────────────────────────────
# [FIX-SINGLE-ISO]  Una sola ISO dual BIOS+UEFI para todos los entornos
# ─────────────────────────────────────────────────────────────────────────────

def _xorriso_path():
    return find_tool("xorriso")

def _genisoimage_path():
    return find_tool("genisoimage","mkisofs")

def _try_xorriso_dual():
    global _ISO_METHOD
    t = _xorriso_path()
    if not t: return False
    if not EFIBIN.exists():
        log("  [ISO-DUAL] BOOTX64.EFI no existe, solo entrada BIOS")
        return _try_xorriso_bios_only()

    log("  [ISO-DUAL] xorriso ISO dual BIOS+UEFI...")
    tree = IMAGENES_TRABAJO / "_isotree_dual"
    if tree.exists(): shutil.rmtree(tree)
    (tree / "boot").mkdir(parents=True, exist_ok=True)

    bc = tree / "boot" / "boot_cd.img"
    ib, boot_sectors = _make_boot_cd_img()
    bc.write_bytes(ib)

    esp_tmp = IMAGENES_TRABAJO / "portix-iso-dual-esp.img"
    _build_esp_fat32(esp_tmp)
    efi_in_tree = tree / "efi.img"
    shutil.copy2(esp_tmp, efi_in_tree)

    ok = run_safe([t, "-as", "mkisofs",
        "-o",    win_to_msys2(ISO_IMG),
        "-V",    "PORTIX",
        "-J",    "-r",
        "-c",    "boot/boot.cat",
        "-b",    "boot/boot_cd.img",
        "-no-emul-boot",
        "-boot-load-size", str(boot_sectors),
        "-boot-info-table",
        "-eltorito-alt-boot",
        "-b",    "efi.img",
        "-no-emul-boot",
        win_to_msys2(tree)])

    shutil.rmtree(tree, ignore_errors=True)
    esp_tmp.unlink(missing_ok=True)

    if not (ok and ISO_IMG.exists() and ISO_IMG.stat().st_size > 0):
        log("  [WARN] xorriso dual ISO falló"); return False

    _ISO_METHOD = "xorriso-dual"
    log(f"[OK]    portix.iso — {human(ISO_IMG)} (dual BIOS+UEFI, boot={boot_sectors}s)")
    return True

def _try_xorriso_bios_only():
    global _ISO_METHOD
    t = _xorriso_path()
    if not t: return False
    log("  [ISO-BIOS] xorriso BIOS-only...")
    tree = IMAGENES_TRABAJO / "_isotree_bios"
    if tree.exists(): shutil.rmtree(tree)
    (tree / "boot").mkdir(parents=True, exist_ok=True)
    bc = tree / "boot" / "boot_cd.img"
    ib, boot_sectors = _make_boot_cd_img()
    bc.write_bytes(ib)
    ok = run_safe([t, "-as", "mkisofs",
        "-o",  win_to_msys2(ISO_IMG),
        "-V",  "PORTIX", "-J", "-r",
        "-c",  "boot/boot.cat",
        "-b",  "boot/boot_cd.img",
        "-no-emul-boot",
        "-boot-load-size", str(boot_sectors),
        "-boot-info-table",
        win_to_msys2(tree)])
    shutil.rmtree(tree, ignore_errors=True)
    if not (ok and ISO_IMG.exists() and ISO_IMG.stat().st_size > 0):
        log("  [WARN] xorriso BIOS-only falló"); return False
    _ISO_METHOD = "xorriso"
    log(f"[OK]    portix.iso — {human(ISO_IMG)} (BIOS only, boot={boot_sectors}s)")
    return True

def _try_genisoimage_dual():
    global _ISO_METHOD
    t = _genisoimage_path()
    if not t: return False
    if not EFIBIN.exists(): return False
    tn = Path(t).name
    log(f"  [ISO-DUAL] {tn} ISO dual...")
    tree = IMAGENES_TRABAJO / "_isotree_dual"
    if tree.exists(): shutil.rmtree(tree)
    (tree / "boot").mkdir(parents=True, exist_ok=True)
    bc = tree / "boot" / "boot_cd.img"
    ib, boot_sectors = _make_boot_cd_img()
    bc.write_bytes(ib)
    esp_tmp = IMAGENES_TRABAJO / "portix-iso-dual-esp.img"
    _build_esp_fat32(esp_tmp)
    efi_in_tree = tree / "efi.img"
    shutil.copy2(esp_tmp, efi_in_tree)
    ok = run_safe([t,
        "-o",  str(ISO_IMG),
        "-V",  "PORTIX", "-J", "-r",
        "-c",  "boot/boot.cat",
        "-b",  "boot/boot_cd.img",
        "-no-emul-boot",
        "-boot-load-size", str(boot_sectors),
        "-boot-info-table",
        "-eltorito-alt-boot",
        "-b",  "efi.img",
        "-no-emul-boot",
        str(tree)])
    shutil.rmtree(tree, ignore_errors=True)
    esp_tmp.unlink(missing_ok=True)
    if not (ok and ISO_IMG.exists() and ISO_IMG.stat().st_size > 0):
        log(f"  [WARN] {tn} dual ISO falló"); return False
    _ISO_METHOD = "genisoimage-dual"
    log(f"[OK]    portix.iso — {human(ISO_IMG)} ({tn}, dual BIOS+UEFI, boot={boot_sectors}s)")
    return True

def _try_xorriso_efi_only():
    t = _xorriso_path()
    if not t: return False
    if not EFIBIN.exists(): return False
    log("  [ISO-EFI] xorriso EFI-only ISO...")
    esp_tmp = IMAGENES_TRABAJO / "portix-iso-esp.img"
    _build_esp_fat32(esp_tmp)
    tree = IMAGENES_TRABAJO / "_isotree_efi"
    if tree.exists(): shutil.rmtree(tree)
    tree.mkdir(parents=True, exist_ok=True)
    shutil.copy2(esp_tmp, tree / "efi.img")
    ok = run_safe([t, "-as", "mkisofs",
        "-o",  win_to_msys2(ISO_UEFI_IMG),
        "-V",  "PORTIX_UEFI", "-J", "-r",
        "-c",  "boot.cat",
        "-eltorito-alt-boot",
        "-b",  "efi.img",
        "-no-emul-boot",
        win_to_msys2(tree)])
    shutil.rmtree(tree, ignore_errors=True)
    esp_tmp.unlink(missing_ok=True)
    if not (ok and ISO_UEFI_IMG.exists() and ISO_UEFI_IMG.stat().st_size > 0):
        log("  [WARN] xorriso EFI-only falló"); return False
    log(f"[OK]    portix-uefi.iso — {human(ISO_UEFI_IMG)} (EFI-only)")
    return True

def _iso_fallback(out, label):
    shutil.copy2(DISK_IMG, out)
    log(f"[OK]    {out.name} — {human(out)} (disco raw, sin xorriso)")
    log(f"        AVISO: NO es CD-ROM. VBox: adjuntar como DISCO DURO IDE.")

def create_all_isos():
    global _ISO_MODE
    step("CREANDO ISO DUAL BIOS+UEFI")
    made_dual = (
        _try_xorriso_dual() or
        _try_genisoimage_dual()
    )
    if not made_dual:
        if not _try_xorriso_bios_only():
            _ISO_MODE = "disk"
            _iso_fallback(ISO_IMG, "PORTIX")
        else:
            _ISO_MODE = "cdrom"
    else:
        _ISO_MODE = "cdrom"

    if not _try_xorriso_efi_only():
        log("[INFO]  portix-uefi.iso no generada (sin xorriso o sin EFI loader)")

def create_vdi():
    step("CREANDO VDI")
    qi = find_tool("qemu-img")
    if not qi: log("[WARN] qemu-img no disponible"); return
    if VDI_IMG.exists(): VDI_IMG.unlink()
    if run_safe([qi,"convert","-U","-f","raw","-O","vdi",str(DISK_IMG),str(VDI_IMG)]) and VDI_IMG.exists():
        log(f"[OK]    portix.vdi — {human(VDI_IMG)} (VirtualBox BIOS)")
    if UEFI_IMG.exists():
        if UEFI_VDI_IMG.exists(): UEFI_VDI_IMG.unlink()
        if run_safe([qi,"convert","-U","-f","raw","-O","vdi",str(UEFI_IMG),str(UEFI_VDI_IMG)]) and UEFI_VDI_IMG.exists():
            log(f"[OK]    portix-uefi.vdi — {human(UEFI_VDI_IMG)} (VirtualBox UEFI)")

def create_vmdk():
    step("CREANDO VMDK")
    qi = find_tool("qemu-img")
    if not qi: log("[WARN] qemu-img no disponible"); return
    if VMDK_IMG.exists(): VMDK_IMG.unlink()
    if run_safe([qi,"convert","-U","-f","raw","-O","vmdk",str(DISK_IMG),str(VMDK_IMG)]) and VMDK_IMG.exists():
        log(f"[OK]    portix.vmdk — {human(VMDK_IMG)} (VMware/VBox BIOS)")
    if UEFI_IMG.exists():
        if UEFI_VMDK_IMG.exists(): UEFI_VMDK_IMG.unlink()
        if run_safe([qi,"convert","-U","-f","raw","-O","vmdk",str(UEFI_IMG),str(UEFI_VMDK_IMG)]) and UEFI_VMDK_IMG.exists():
            log(f"[OK]    portix-uefi.vmdk — {human(UEFI_VMDK_IMG)} (VMware/VBox UEFI)")

# ─────────────────────────────────────────────────────────────────────────────
# run_qemu
# ─────────────────────────────────────────────────────────────────────────────

def run_qemu():
    mode = arg_val("--mode") or "raw"
    step(f"EJECUTANDO QEMU (modo: {mode})")
    vga_type = arg_val("--vga") or "std"
    display_mode = arg_val("--display")
    if display_mode is None:
        display_mode = "sdl"  # interactive by default

    net_args = ["-netdev", "user,id=u1", "-device", "e1000,netdev=u1"]

    # ── SILICIO REAL: discos con patrón conocido y controladoras reales ─────
    # Sin estos dispositivos el kernel imprime [SILICIO-AUSENTE] y el ensayo de
    # hardware no tiene nada que medir; con ellos lee su identidad por MMIO/DMA
    # y hace ensayos de ida y vuelta sobre bloques de verdad.
    silicon_args = []
    if not arg("--no-silicon") and arg_val("--no-silicon") is None:
        for qid, ruta, mib, firma in DISCOS_SILICIO:
            if crear_disco_patron(ruta, mib, firma):
                log(f"  Disco de silicio creado: {ruta} ({mib} MiB)")
            silicon_args += ["-drive", f"file={ruta},format=raw,if=none,id=drive-{qid}"]
        silicon_args += [
            "-device", "nvme,serial=PORTIXNVME01,drive=drive-nvme0",
            "-device", "ich9-ahci,id=ahci",
            "-device", "ide-hd,bus=ahci.0,drive=drive-sata0",
            "-device", "qemu-xhci,id=xhci",
            "-device", "usb-storage,bus=xhci.0,drive=drive-usb0",
            "-audiodev", "none,id=snd0",
            "-device", "intel-hda,id=hda",
            "-device", "hda-duplex,bus=hda.0,audiodev=snd0",
        ]
        log(f"  Silicio: NVMe + AHCI/SATA + xHCI/USB + HDA (QEMU)")
    else:
        log("  Silicio: desactivado (--no-silicon): el kernel debe reportar [SILICIO-AUSENTE]")

    if display_mode == "none" or display_mode == "headless":
        base = ["-cpu","max","-m","256M","-vga",vga_type,"-serial","stdio",
                "-no-reboot","-d","int,guest_errors","-D",str(DEBUG_LOG)] + net_args + silicon_args
        serial_log = "stdio"
    else:
        base = ["-cpu","max","-m","256M","-vga",vga_type,
                "-display", display_mode,
                "-serial","file:serial.log",
                "-no-reboot","-d","int,guest_errors","-D",str(DEBUG_LOG)] + net_args + silicon_args
        serial_log = str(Path.cwd() / "serial.log")
        log(f"  Display: {display_mode}")
        log(f"  Serial:  {serial_log}")

    persist_drive = []
    if arg_val("--persistence") is not None and PERSISTENT_DISK.exists():
        persist_drive = ["-drive", f"format=raw,file={PERSISTENT_DISK},if=ide,index=1,media=disk"]
        log(f"  Disco persistente: {PERSISTENT_DISK} ({PERSISTENT_DISK.stat().st_size // 1048576} MB)")

    def raw():
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={DISK_IMG},if=ide,index=0,media=disk"
        ] + persist_drive + base)

    def iso():
        tgt = ISO_IMG if ISO_IMG.exists() else DISK_IMG
        if _ISO_MODE == "cdrom":
            subprocess.run(["qemu-system-x86_64",
                "-drive", f"format=raw,file={tgt},media=cdrom", "-boot","order=d"
            ] + persist_drive + base)
        else:
            subprocess.run(["qemu-system-x86_64",
                "-drive", f"format=raw,file={tgt},if=ide,index=0,media=disk"
            ] + persist_drive + base)

    def iso_uefi():
        ovmf = find_ovmf()
        if not ovmf:
            log("[ERROR] OVMF no encontrado."); return
        tgt = ISO_IMG if ISO_IMG.exists() else DISK_IMG
        log(f"  OVMF:  {ovmf}")
        log(f"  ISO:   {tgt}")
        subprocess.run(["qemu-system-x86_64"] + base + [
            "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
            "-drive", f"format=raw,file={tgt},media=cdrom",
            "-boot",  "order=d",
        ] + persist_drive)

    def vsim():
        if not VSIM_IMG.exists(): create_ventoy_sim()
        if not VSIM_IMG.exists(): log("[ERROR] No ventoy-sim.img"); return
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={VSIM_IMG},if=ide,index=0,media=disk"
        ] + persist_drive + base)

    def uefi():
        ovmf = find_ovmf()
        if not ovmf:
            log("[ERROR] OVMF no encontrado.")
            log(f"        Copiar a: {ROOT / 'OVMF.fd'}")
            log("        Linux: apt install ovmf"); return
        tgt = UEFI_IMG if UEFI_IMG.exists() else DISK_IMG
        log(f"  OVMF:  {ovmf}")
        log(f"  Disco: {tgt}")
        subprocess.run(["qemu-system-x86_64"] + base + [
            "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
            "-drive", f"format=raw,file={tgt},if=ide,index=0,media=disk",
        ] + persist_drive)

    def dual():
        tgt = DUAL_IMG if DUAL_IMG.exists() else DISK_IMG
        log(f"  Modo dual: arrancando imagen BIOS ({tgt.name})")
        subprocess.run(["qemu-system-x86_64",
            "-drive", f"format=raw,file={tgt},if=ide,index=0,media=disk"
        ] + persist_drive + base)

    dispatch = {
        "iso":        iso,
        "iso-uefi":   iso_uefi,
        "ventoy-sim": vsim,
        "uefi":       uefi,
        "dual":       dual,
    }
    if mode == "both":
        t1 = threading.Thread(target=raw, daemon=True)
        t2 = threading.Thread(target=iso, daemon=True)
        t1.start(); t2.start(); t1.join(); t2.join()
    else:
        dispatch.get(mode, raw)()

# ─────────────────────────────────────────────────────────────────────────────
# summary
# ─────────────────────────────────────────────────────────────────────────────

LIB_DIR = ROOT / "lib"
LIB_BUILD = LIB_DIR / "build"
LIB_A = LIB_BUILD / "libportix.a"
LIB_CC = find_tool("x86_64-elf-gcc")
LIB_OK = LIB_A.exists()

def build_libportix():
    step("VERIFICANDO USERLAND (100% RUST)")
    log("  [INFO] Utilidades propias Rust; binarios de distro se instalan por separado")
    return

def summary():
    el = time.monotonic() - _t0
    W  = 88

    ovmf  = find_ovmf()
    ovmf_s = Path(ovmf).name if ovmf else "NO encontrado — apt install ovmf"

    if _ISO_METHOD in ("xorriso-dual","genisoimage-dual"):
        iso_desc = f"dual BIOS+UEFI ({_ISO_METHOD}, BIT parchado)"
    elif _ISO_METHOD == "xorriso":
        iso_desc = "BIOS-only (no EFI loader)"
    else:
        iso_desc = "disco raw (sin xorriso)"

    def row(text):
        t = text[:W]
        return f"║{t:<{W}}║"

    def sep():
        return "╠" + "═"*W + "╣"

    entries = [
        (RAW_COPY,      "IMG  ", "dd/Rufus->USB | QEMU --mode=raw"),
        (ISO_IMG,       "ISO  ", "portix.iso = BIOS+UEFI (VBox BIOS, VBox UEFI, QEMU, HW)  ★"),
        (VDI_IMG,       "VDI  ", "VirtualBox disco IDE / SATA (BIOS Legacy)"),
        (UEFI_VDI_IMG,  "UVDI ", "VirtualBox disco GPT+ESP (UEFI)"),
        (VMDK_IMG,      "VMDK ", "VMware / VirtualBox BIOS"),
        (UEFI_VMDK_IMG, "UVMDK", "VMware / VirtualBox UEFI"),
        (UEFI_IMG,      "UIMG ", "GPT+ESP disco UEFI — QEMU --mode=uefi"),
        (DUAL_IMG,      "DUAL ", "BIOS legacy — QEMU --mode=dual"),
    ]

    print()
    print("╔" + "═"*W + "╗")
    print(row("  PORTIX OS v1.0.0 'Chronos' — RESUMEN DE ARTEFACTOS Y DISTRIBUCION"))
    print(sep())
    for p, lbl, uso in entries:
        exists = p.exists() if p else False
        st = "[OK]" if exists else "[--]"
        if exists:
            info = f"{p.name:<24} {human(p):<8}  {uso}"
        else:
            info = "(no generado)"
        print(row(f"  {st} {lbl} {info}"))
    print(sep())
    print(row(f"  Núcleo:     Rust Puro no_std (0 crates externas)"))
    print(row("  Userland propio: Rust; herramientas Linux externas se instalan por separado"))
    print(row(f"  Modo ISO:   {iso_desc}"))
    print(row(f"  Firmware:   {ovmf_s}"))
    print(row(f"  Tiempo:     {el:.1f} segundos (Optimizado para compilación incremental)"))
    print(sep())
    print(row("  Ejecución QEMU BIOS:  python3 scripts/build.py --mode=iso"))
    print(row("  Ejecución QEMU UEFI:  python3 scripts/build.py --mode=iso-uefi"))
    print(row("  VirtualBox BIOS:      Asignar portix.iso en Controlador Óptico"))
    print(row("  VirtualBox UEFI:      Asignar portix.iso + Habilitar EFI en Sistema"))
    print(row("  Hardware Físico:      Grabar portix.iso / portix.img en Pendrive USB (Rufus/dd)"))
    print("╚" + "═"*W + "╝")

# ─────────────────────────────────────────────────────────────────────────────
# main
# ─────────────────────────────────────────────────────────────────────────────

def main():
    global _t0; _t0 = time.monotonic()
    print("\n╔══════════════════════════════════════════════════════════════════════╗")
    print("║               PORTIX OS MAESTRO DE CONSTRUCCION v6.0                 ║")
    print("║         Kernel y utilidades Rust • Dual Boot BIOS/UEFI              ║")
    print("╚══════════════════════════════════════════════════════════════════════╝\n")

    if arg("--clean"): clean(); return
    reset_logs()
    check_tools()

    assemble_boot()
    run(["python", str(ROOT / "scripts" / "mkhello.py")])
    ks = build_kernel()
    assemble_stage2(ks)
    create_raw(ks)

    build_libportix()

    build_efi_loader()
    create_uefi_image(UEFI_IMG)
    shutil.copy2(DISK_IMG, DUAL_IMG)
    log(f"[OK]    portix-dual.img — {human(DUAL_IMG)}  (BIOS legacy)")

    if not arg("--no-iso"):
        create_all_isos()
    else:
        log("[SKIP] ISOs omitidas (--no-iso)")

    if not arg("--no-vm"):
        create_vdi()
        create_vmdk()
    else:
        log("[SKIP] VDI/VMDK omitidos (--no-vm)")

    create_ventoy_sim()

    # La directiva de ubicación no autoriza borrar directorios ajenos en /home.
    if arg_val("--persistence") is not None and (PERSISTENT_DISK.is_symlink() or not dentro_de_dist(PERSISTENT_DISK)):
        raise RuntimeError("El disco persistente debe ser un archivo dentro de build/dist")
    if arg_val("--persistence") is not None and not PERSISTENT_DISK.exists():
        step("CREANDO DISCO PERSISTENTE")
        raw = arg_val("--persistence")
        mb = int(raw) if raw is not None and raw.isdigit() else PERSISTENT_MB
        PERSISTENT_DISK.parent.mkdir(parents=True, exist_ok=True)
        size = mb * 1024 * 1024
        # Archivo disperso, ceros lógicos, sin reservar 8 GiB de RAM.
        with PERSISTENT_DISK.open("xb") as f:
            f.truncate(size)
        log(f"[OK]    {PERSISTENT_DISK.name} — {mb} MB")

    summary()

    if not arg("--no-run"):
        run_qemu()

if __name__ == "__main__": main()