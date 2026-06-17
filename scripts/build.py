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

PERSISTENT_DISK = BUILD / "portix-persistent.img"
PERSISTENT_MB   = 8192

# ── Imágenes de distribución ──────────────────────────────────────────────────
# [FIX-SINGLE-ISO] Una sola ISO dual BIOS+UEFI
ISO_IMG      = DIST / "portix.iso"          # ISO dual BIOS+UEFI (TODOS los entornos)
ISO_UEFI_IMG = DIST / "portix-uefi.iso"     # EFI-only fallback (legacy, QEMU+OVMF)
VDI_IMG      = DIST / "portix.vdi"
VMDK_IMG     = DIST / "portix.vmdk"
RAW_COPY     = DIST / "portix.img"
VSIM_IMG     = DIST / "portix-ventoy-sim.img"
UEFI_IMG     = DIST / "portix-uefi.img"     # disco GPT completo
ESP_IMG      = BUILD / "portix-esp.img"     # ESP FAT32 temporal
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
DATA_PART_IMG     = BUILD / "portix-data-part.img"
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
  "code-model": "small",
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
    raw = DISK_IMG.read_bytes()[:total_bytes]
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
    base = [sys.executable, "-m", "pip", "install", "pyfatfs",
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
    except ImportError:
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

def _write_gpt(out_img, esp_data):
    SECTOR = 512
    ESP_START_LBA = 34
    esp_sectors   = math.ceil(len(esp_data) / SECTOR)
    esp_end_lba   = ESP_START_LBA + esp_sectors - 1
    total_sectors = ESP_START_LBA + esp_sectors + 33 + 1
    total_size    = total_sectors * SECTOR
    disk = bytearray(total_size)

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
    part_array_crc = _gpt_crc32(bytes(part_array))
    disk[2*SECTOR : 2*SECTOR+len(part_array)] = part_array

    backup_lba = total_sectors - 1
    hdr = bytearray(92)
    hdr[0:8]=b"EFI PART"; hdr[8:12]=b"\x00\x00\x01\x00"
    struct.pack_into("<I", hdr, 12, 92)
    struct.pack_into("<Q", hdr, 24, 1)
    struct.pack_into("<Q", hdr, 32, backup_lba)
    struct.pack_into("<Q", hdr, 40, ESP_START_LBA)
    struct.pack_into("<Q", hdr, 48, esp_end_lba)
    hdr[56:72] = uuid.uuid4().bytes_le
    struct.pack_into("<Q", hdr, 72, 2)
    struct.pack_into("<I", hdr, 80, 128)
    struct.pack_into("<I", hdr, 84, 128)
    struct.pack_into("<I", hdr, 88, part_array_crc)
    struct.pack_into("<I", hdr, 16, _gpt_crc32(bytes(hdr)))
    disk[SECTOR : SECTOR+len(hdr)] = hdr

    esp_off = ESP_START_LBA * SECTOR
    disk[esp_off : esp_off+len(esp_data)] = esp_data

    bpe_lba = backup_lba - 33
    disk[bpe_lba*SECTOR : bpe_lba*SECTOR+len(part_array)] = part_array

    bhdr = bytearray(hdr)
    struct.pack_into("<I", bhdr, 16, 0)
    struct.pack_into("<Q", bhdr, 24, backup_lba)
    struct.pack_into("<Q", bhdr, 32, 1)
    struct.pack_into("<Q", bhdr, 72, bpe_lba)
    struct.pack_into("<I", bhdr, 16, _gpt_crc32(bytes(bhdr)))
    disk[backup_lba*SECTOR : backup_lba*SECTOR+len(bhdr)] = bhdr

    out_img.write_bytes(bytes(disk))
    log(f"  GPT: {total_sectors} sectores ({total_size//1048576} MB), "
        f"ESP LBA {ESP_START_LBA}-{esp_end_lba}")

def create_uefi_image(out_img):
    step(f"CREANDO DISCO GPT+ESP UEFI ({out_img.name})")
    if not EFIBIN.exists(): build_efi_loader()
    if not KERNELBIN.exists(): log("[ERROR] kernel.bin no existe"); sys.exit(1)
    _build_esp_fat32(ESP_IMG)
    _write_gpt(out_img, ESP_IMG.read_bytes())
    ESP_IMG.unlink(missing_ok=True)
    log(f"[OK]    {out_img.name} — {human(out_img)}  (GPT + ESP FAT32)")

# ─────────────────────────────────────────────────────────────────────────────
# check_tools
# ─────────────────────────────────────────────────────────────────────────────

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
    for d in [BUILD,LOGS,DIST]: d.mkdir(parents=True, exist_ok=True)
    if BUILD_LOG.exists(): BUILD_LOG.unlink()

def clean():
    step("LIMPIANDO")
    persist_data = None
    if arg_val("--persistence") is not None and PERSISTENT_DISK.exists():
        persist_data = PERSISTENT_DISK.read_bytes()
        log(f"  Persistiendo {PERSISTENT_DISK.name} ({len(persist_data)//1048576} MB)")
    for d in [BUILD,DIST]:
        if d.exists(): shutil.rmtree(d)
    if persist_data is not None:
        BUILD.mkdir(parents=True, exist_ok=True)
        PERSISTENT_DISK.write_bytes(persist_data)
        log(f"[OK]    {PERSISTENT_DISK.name} restaurado")
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

def _find_xgcc():
    """Busca el cross-compiler x86_64-elf-gcc en build/."""
    gcc = BUILD / "x86_64-elf" / "bin" / "x86_64-elf-gcc.exe"
    if not gcc.exists():
        gcc = BUILD / "x86_64-elf" / "bin" / "x86_64-elf-gcc"
    if gcc.exists():
        bindir = str(gcc.parent)
        if bindir not in os.environ.get("PATH", ""):
            os.environ["PATH"] = bindir + os.pathsep + os.environ.get("PATH", "")
        return gcc
    return None

def _build_libportix():
    """Compila libportix.a con el cross-compiler."""
    gcc = _find_xgcc()
    if not gcc: return None
    as_ = gcc.with_name("x86_64-elf-as.exe")
    ar  = gcc.with_name("x86_64-elf-ar.exe")
    if not as_.exists(): as_ = gcc.with_name("x86_64-elf-as")
    if not ar.exists():  ar  = gcc.with_name("x86_64-elf-ar")

    src_dir = ROOT / "lib" / "src"
    inc_dir = ROOT / "lib" / "include"
    lib_build = BUILD / "libportix"
    lib_build.mkdir(parents=True, exist_ok=True)

    cflags = ["-ffreestanding", "-nostdlib", "-static", "-mno-red-zone",
              "-mno-mmx", "-mno-sse", "-I", str(inc_dir), "-O2", "-Wall", "-c"]

    files = [
        ("crt0.s", str(as_), []),
        ("stdio.c", str(gcc), cflags),
        ("stdlib.c", str(gcc), cflags),
        ("string.c", str(gcc), cflags),
        ("file.c", str(gcc), cflags),
    ]

    objs = []
    for fname, tool, flags in files:
        src = src_dir / fname
        obj = lib_build / fname.replace(".c", ".o").replace(".s", ".o")
        cmd = [tool] + flags + ["-o", str(obj), str(src)]
        subprocess.run(cmd, check=True, capture_output=True)
        objs.append(str(obj))

    liba = lib_build / "libportix.a"
    subprocess.run([str(ar), "rcs", str(liba)] + objs, check=True, capture_output=True)
    log(f"  libportix.a -> {liba} ({liba.stat().st_size} bytes)")
    return lib_build

def _compile_c_examples(lib_build, fs):
    """Compila ejemplos C y los copia a FAT32."""
    if not lib_build: return
    gcc = _find_xgcc()
    ld  = gcc.with_name("x86_64-elf-ld.exe")
    if not ld.exists(): ld = gcc.with_name("x86_64-elf-ld")

    inc_dir = ROOT / "lib" / "include"
    examples_dir = ROOT / "lib" / "examples"
    lds = BUILD / "linker.ld"
    cflags = ["-ffreestanding", "-nostdlib", "-static", "-mno-red-zone",
              "-mno-mmx", "-mno-sse", "-I", str(inc_dir), "-O2", "-Wall", "-c"]

    crt0 = lib_build / "crt0.o"
    liba = lib_build / "libportix.a"

    for src in sorted(examples_dir.glob("*.c")):
        name = src.stem
        obj = lib_build / f"{name}.o"
        out = lib_build / f"{name}.elf"
        log(f"  Compiling C: {src.name}")
        subprocess.run([str(gcc)] + cflags + ["-o", str(obj), str(src)],
                       check=True, capture_output=True)
        link = [str(ld), "-T", str(lds), "-o", str(out), str(obj),
                str(crt0), "-L", str(lib_build), "-lportix",
                "-z", "max-page-size=0x1", "-N"]
        subprocess.run(link, check=True, capture_output=True)
        sz = out.stat().st_size
        log(f"    -> {out.name} ({sz} bytes)")
        obj.unlink()

        # Determinar destino FAT32
        if name == "sh":
            dst = "/bin/sh"
        elif name == "hello":
            dst = "/bin/hello"
        else:
            dst = f"/bin/{name}"
        with fs.openbin(dst, "w") as f:
            f.write(out.read_bytes())
        log(f"    FAT copy {out.name} -> {dst}")

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
        for d in ["/bin", "/etc", "/home", "/home/user", "/tmp", "/usr", "/var"]:
            if not fs.isdir(d):
                fs.makedir(d, recreate=True)
                log(f"  FAT mkdir {d}")

        # Compilar ejemplos C si el cross-compiler existe
        lib_build = _build_libportix()
        if lib_build:
            _compile_c_examples(lib_build, fs)
        elif HELLO_ELF.exists():
            for dst in ["/bin/sh", "/bin/hello", "/bin/echo", "/bin/ls"]:
                with fs.openbin(dst, "w") as f:
                    f.write(HELLO_ELF.read_bytes())
                log(f"  FAT copy hello.elf -> {dst}")
    finally:
        fs.close()

    part_sectors = DATA_PART_IMG.stat().st_size // 512
    log(f"[OK]    Particion FAT32: {DATA_PART_MB} MB ({part_sectors} sectores)")
    return part_sectors

def _inject_pt(img_path, data_lba_start, data_sectors):
    data = bytearray(img_path.read_bytes())
    # Partition 1: boot area (LBA 1 to just before data partition, type 0x83 Linux)
    part1 = bytearray(16)
    part1[0] = 0x00           # not bootable
    part1[4] = 0x83           # Linux native (skipped by FAT32 mount)
    struct.pack_into('<I', part1, 8, 1)
    struct.pack_into('<I', part1, 12, data_lba_start - 1)
    data[0x1BE:0x1BE+16] = part1

    # Partition 2: FAT32 data partition
    ts = len(data) // 512
    part2 = bytearray(16)
    part2[0] = 0x00
    part2[4] = 0x0C           # FAT32 LBA
    ss = data_lba_start
    el = min(ts - 1, data_lba_start + data_sectors - 1)
    if ss <= el:
        struct.pack_into('<I', part2, 8, ss)
        struct.pack_into('<I', part2, 12, el - ss + 1)
    data[0x1CE:0x1CE+16] = part2

    img_path.write_bytes(bytes(data))
    log(f"  Tabla de particiones: P1@1-{data_lba_start-1} (boot) P2@{data_lba_start}+{data_sectors}s (FAT32)")

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

    # Build FAT32 data partition y appendar
    part_sectors = _build_data_part(ks)
    part_data = DATA_PART_IMG.read_bytes()
    with open(DISK_IMG, "r+b") as f:
        f.seek(kernel_end_lba * 512)
        f.write(part_data)

    _inject_pt(DISK_IMG, kernel_end_lba, part_sectors)
    shutil.copy2(DISK_IMG, RAW_COPY)
    total_mb = len(part_data) // 1048576
    log(f"[OK]    portix.img — {human(DISK_IMG)} (boot + kernel + FAT32 {total_mb} MB)")

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
    tree = BUILD / "_isotree_dual"
    if tree.exists(): shutil.rmtree(tree)
    (tree / "boot").mkdir(parents=True, exist_ok=True)

    bc = tree / "boot" / "boot_cd.img"
    ib, boot_sectors = _make_boot_cd_img()
    bc.write_bytes(ib)

    esp_tmp = BUILD / "portix-iso-dual-esp.img"
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
    tree = BUILD / "_isotree_bios"
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
    tree = BUILD / "_isotree_dual"
    if tree.exists(): shutil.rmtree(tree)
    (tree / "boot").mkdir(parents=True, exist_ok=True)
    bc = tree / "boot" / "boot_cd.img"
    ib, boot_sectors = _make_boot_cd_img()
    bc.write_bytes(ib)
    esp_tmp = BUILD / "portix-iso-dual-esp.img"
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
    esp_tmp = BUILD / "portix-iso-esp.img"
    _build_esp_fat32(esp_tmp)
    tree = BUILD / "_isotree_efi"
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
    if run_safe([qi,"convert","-f","raw","-O","vdi",str(DISK_IMG),str(VDI_IMG)]) and VDI_IMG.exists():
        log(f"[OK]    portix.vdi — {human(VDI_IMG)}")

def create_vmdk():
    step("CREANDO VMDK")
    qi = find_tool("qemu-img")
    if not qi: log("[WARN] qemu-img no disponible"); return
    if VMDK_IMG.exists(): VMDK_IMG.unlink()
    if run_safe([qi,"convert","-f","raw","-O","vmdk",str(DISK_IMG),str(VMDK_IMG)]) and VMDK_IMG.exists():
        log(f"[OK]    portix.vmdk — {human(VMDK_IMG)}")

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

    if display_mode == "none" or display_mode == "headless":
        base = ["-cpu","max","-m","256M","-vga",vga_type,"-serial","stdio",
                "-no-reboot","-d","int,guest_errors","-D",str(DEBUG_LOG)]
        serial_log = "stdio"
    else:
        base = ["-cpu","max","-m","256M","-vga",vga_type,
                "-display", display_mode,
                "-serial","file:serial.log",
                "-no-reboot","-d","int,guest_errors","-D",str(DEBUG_LOG)]
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
    step("COMPILANDO libportix (Fase 7)")
    LIB_CC = find_tool("x86_64-elf-gcc")
    LIB_OK = LIB_A.exists()
    if not LIB_CC:
        log("  [SKIP] x86_64-elf-gcc no encontrado")
        log("  Para construir: bash scripts/ring3-toolchain.sh")
        return
    if LIB_OK:
        log(f"[OK]    libportix.a — {human(LIB_A)}")
        return
    run(["make","-C",str(LIB_DIR)])

def summary():
    el = time.monotonic() - _t0
    W  = 78

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
        (RAW_COPY,     "IMG  ", "dd/Rufus->USB | QEMU --mode=raw"),
        (ISO_IMG,      "ISO  ", "portix.iso = BIOS+UEFI (VBox BIOS, VBox UEFI, QEMU, HW)  ★"),
        (ISO_UEFI_IMG, "UEFI ", "portix-uefi.iso = EFI-only fallback"),
        (VDI_IMG,      "VDI  ", "VirtualBox disco IDE"),
        (VMDK_IMG,     "VMDK ", "VMware / VirtualBox"),
        (VSIM_IMG,     "SIM  ", "Test Ventoy (--mode=ventoy-sim)"),
        (UEFI_IMG,     "UIMG ", "GPT+ESP disco UEFI — QEMU --mode=uefi"),
        (DUAL_IMG,     "DUAL ", "BIOS legacy — QEMU --mode=dual"),
    ]

    print()
    print("╔" + "═"*W + "╗")
    print(row("  PORTIX v5.3 — ARCHIVOS DE DISTRIBUCION"))
    print(sep())
    for p, lbl, uso in entries:
        exists = p.exists() if p else False
        st = "OK" if exists else "XX"
        if exists:
            info = f"{p.name:<24} {human(p):<8}  {uso}"
        else:
            info = "(no generado)"
        print(row(f"  {st} {lbl} {info}"))
    print(sep())
    lib_st = "OK" if LIB_OK else "NO (sin x86_64-elf-gcc)"
    print(row(f"  ISO: {iso_desc}"))
    print(row(f"  libportix: {lib_st}"))
    print(row(f"  OVMF: {ovmf_s}"))
    print(row(f"  Build: {el:.1f}s"))
    print(sep())
    print(row("  QEMU BIOS:  python build.py --mode=iso"))
    print(row("  QEMU UEFI:  python build.py --mode=iso-uefi"))
    print(row("  VBox BIOS:  Storage > Optical Drive > portix.iso"))
    print(row("  VBox UEFI:  Storage > Optical Drive > portix.iso  + EFI enabled"))
    print(row("  Hardware:   grabar portix.iso con cualquier grabador"))
    print("╚" + "═"*W + "╝")

# ─────────────────────────────────────────────────────────────────────────────
# main
# ─────────────────────────────────────────────────────────────────────────────

def main():
    global _t0; _t0 = time.monotonic()
    print("\n╔══════════════════════════════════════╗")
    print("║   PORTIX BUILD SYSTEM  v5.3         ║")
    print("╚══════════════════════════════════════╝\n")

    if arg("--clean"): clean(); return
    reset_logs()
    check_tools()

    assemble_boot()
    run(["python", str(ROOT / "scripts" / "mkhello.py")])
    assemble_stage2(1)
    ks = build_kernel()
    assemble_stage2(ks)
    ks = build_kernel()
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

    if arg_val("--persistence") is not None and not PERSISTENT_DISK.exists():
        step("CREANDO DISCO PERSISTENTE")
        raw = arg_val("--persistence")
        mb = int(raw) if raw is not None and raw.isdigit() else PERSISTENT_MB
        PERSISTENT_DISK.parent.mkdir(parents=True, exist_ok=True)
        size = mb * 1024 * 1024
        PERSISTENT_DISK.write_bytes(b'\x00' * size)
        log(f"[OK]    {PERSISTENT_DISK.name} — {mb} MB")

    summary()

    if not arg("--no-run"):
        run_qemu()

if __name__ == "__main__": main()