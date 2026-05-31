# Portix OS — Prerequisites

> Dependencias necesarias para compilar y ejecutar Portix OS en cualquier plataforma.

---

## Tabla de contenidos

- [Dependencias requeridas](#dependencias-requeridas)
- [Windows](#windows)
- [Linux — Debian / Ubuntu](#linux--debian--ubuntu)
- [Linux — Fedora / RHEL / CentOS Stream](#linux--fedora--rhel--centosstream)
- [Linux — Arch Linux / Manjaro](#linux--arch-linux--manjaro)
- [Linux — openSUSE Tumbleweed / Leap](#linux--opensuse-tumbleweed--leap)
- [Linux — Gentoo](#linux--gentoo)
- [Linux — Alpine](#linux--alpine)
- [macOS](#macos)
- [Verificación rápida](#verificación-rápida)
- [Solución de problemas](#solución-de-problemas)

---

## Dependencias requeridas

| Herramienta | Versión mínima | Para qué se usa |
|-------------|----------------|-----------------|
| **Rust nightly** | última nightly | compilar el kernel / bootloader |
| **Python** | 3.8+ | script de build (`build.py`) |
| **NASM** | 2.15+ | ensamblador x86 |
| **QEMU** | 7.0+ | emulación / pruebas |
| **binutils** (`objcopy`) | 2.38+ | conversión de binarios ELF → raw |
| **xorriso** | 1.5+ | generar imágenes ISO El Torito |
| **pyfatfs** | última | *(sólo UEFI)* sistema de archivos FAT |
| **OVMF** | — | *(sólo UEFI)* firmware EFI para QEMU |

---

## Windows

> Probado en Windows 10 (21H2+) y Windows 11. Requiere **PowerShell 5.1+** o Terminal de Windows.

### 1. Rust (nightly)

```powershell
# Instalar rustup (si no lo tienes)
winget install Rustlang.Rustup
# o descargar de https://rustup.rs

# Instalar y activar la toolchain nightly
rustup toolchain install nightly
rustup default nightly

# Verificar
rustc --version   # debe mostrar "nightly-…"
```

### 2. Python 3

```powershell
winget install Python.Python.3
# o descargar de https://python.org

# Verificar
python --version
```

### 3. NASM

```powershell
winget install NASM.NASM
# o descargar de https://nasm.us

# Verificar
nasm --version
```

### 4. QEMU

```powershell
winget install QEMU
# o descargar de https://qemu.org
```

> La instalación por defecto ubica QEMU en `C:\Program Files\qemu\`.

### 5. MSYS2 + binutils / xorriso

```powershell
winget install MSYS2.MSYS2
```

Luego abre una terminal **MSYS2 (UCRT64 o MINGW64)** y ejecuta:

```bash
pacman -Syu --noconfirm
pacman -S --noconfirm \
    mingw-w64-x86_64-binutils \
    mingw-w64-x86_64-libisoburn
```

Esto instala:
- `objcopy` — convierte ELF → binary
- `xorriso` — crea imágenes ISO El Torito

Añade `C:\msys64\mingw64\bin` a tu **PATH** de sistema. `build.py` también lo detecta automáticamente.

### 6. pyfatfs (sólo UEFI)

```powershell
pip install pyfatfs
```

### 7. OVMF (sólo UEFI)

OVMF suele incluirse con QEMU. `build.py` busca en:

```
C:\Program Files\qemu\share\edk2-x86_64-code.fd
C:\Program Files\qemu\share\ovmf-x86_64.bin
```

Si no está presente, puedes obtenerlo de dos formas:

**Opción A — desde MSYS2:**
```bash
pacman -S mingw-w64-x86_64-ovmf
```

**Opción B — descarga manual:**
```
https://github.com/retrage/edk2-nightly/releases
```
Copia `OVMF_CODE.fd` a:
```
C:\Program Files\qemu\share\edk2-x86_64-code.fd
```

---

## Linux — Debian / Ubuntu

> Incluye: Ubuntu 22.04+, Debian 11+, Linux Mint, Pop!_OS, elementary OS, Kali Linux.

```bash
# Rust nightly
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install nightly
rustup default nightly

# Dependencias del sistema
sudo apt update
sudo apt install -y \
    python3 \
    python3-pip \
    nasm \
    qemu-system-x86 \
    binutils \
    xorriso \
    ovmf

# pyfatfs (UEFI)
pip3 install pyfatfs

# Verificar
rustc --version && nasm --version && qemu-system-x86_64 --version
```

---

## Linux — Fedora / RHEL / CentOS Stream

> Incluye: Fedora 38+, RHEL 9+, CentOS Stream 9+, AlmaLinux, Rocky Linux.

```bash
# Rust nightly
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install nightly
rustup default nightly

# Dependencias del sistema
sudo dnf install -y \
    python3 \
    python3-pip \
    nasm \
    qemu-system-x86 \
    binutils \
    xorriso \
    edk2-ovmf

# pyfatfs (UEFI)
pip3 install pyfatfs

# Verificar
rustc --version && nasm --version && qemu-system-x86_64 --version
```

> **RHEL / CentOS Stream:** puede que necesites habilitar el repositorio **EPEL** para `xorriso`:
> ```bash
> sudo dnf install -y epel-release
> sudo dnf install -y xorriso
> ```

---

## Linux — Arch Linux / Manjaro

```bash
# Rust nightly
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install nightly
rustup default nightly

# Dependencias del sistema
sudo pacman -Syu --noconfirm
sudo pacman -S --noconfirm \
    python \
    python-pip \
    nasm \
    qemu-system-x86 \
    binutils \
    libisoburn \
    edk2-ovmf

# pyfatfs (UEFI)
pip install pyfatfs

# Verificar
rustc --version && nasm --version && qemu-system-x86_64 --version
```

---

## Linux — openSUSE Tumbleweed / Leap

```bash
# Rust nightly
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install nightly
rustup default nightly

# Dependencias del sistema
sudo zypper refresh
sudo zypper install -y \
    python3 \
    python3-pip \
    nasm \
    qemu-x86 \
    binutils \
    xorriso \
    qemu-ovmf-x86_64

# pyfatfs (UEFI)
pip3 install pyfatfs

# Verificar
rustc --version && nasm --version && qemu-system-x86_64 --version
```

---

## Linux — Gentoo

```bash
# Rust nightly (via rustup — Portage no siempre tiene la última nightly)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install nightly
rustup default nightly

# Dependencias del sistema
sudo emerge --ask \
    dev-lang/python \
    dev-lang/nasm \
    app-emulation/qemu \
    sys-devel/binutils \
    dev-libs/libisoburn \
    sys-firmware/edk2-ovmf

# pyfatfs (UEFI)
pip install pyfatfs
```

> Para QEMU en Gentoo, asegúrate de añadir `qemu_softmmu_targets_x86_64` a tus `USE` flags antes de hacer emerge.

---

## Linux — Alpine

> Alpine usa **musl libc**. Rust nightly con musl es completamente compatible.

```bash
# Actualizar repositorios
sudo apk update

# Rust nightly
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install nightly
rustup default nightly

# Dependencias del sistema
sudo apk add \
    python3 \
    py3-pip \
    nasm \
    qemu-system-x86_64 \
    binutils \
    xorriso \
    ovmf

# pyfatfs (UEFI)
pip3 install pyfatfs

# Verificar
rustc --version && nasm --version && qemu-system-x86_64 --version
```

---

## macOS

> Requiere **macOS 12 Monterey** o superior. Compatible con Intel y Apple Silicon (M1/M2/M3).

```bash
# Instalar Homebrew si no lo tienes
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Rust nightly
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install nightly
rustup default nightly

# Dependencias del sistema
brew update
brew install \
    python3 \
    nasm \
    qemu \
    binutils \
    xorriso

# OVMF — incluido en qemu de Homebrew
# Localización típica: $(brew --prefix)/share/qemu/edk2-x86_64-code.fd

# pyfatfs (UEFI)
pip3 install pyfatfs

# Verificar
rustc --version && nasm --version && qemu-system-x86_64 --version
```

> **Apple Silicon:** QEMU corre x86_64 via TCG (emulación software). El rendimiento es más lento que en hardware nativo, pero es completamente funcional para desarrollo y pruebas.

---

## Verificación rápida

Una vez instaladas todas las dependencias, puedes verificar y compilar con:

```bash
# Limpiar artefactos anteriores
python scripts/build.py --clean

# Compilar y arrancar en modo BIOS (genera imagen ISO)
python scripts/build.py --mode=iso

# Compilar y arrancar en modo UEFI (requiere OVMF + pyfatfs)
python scripts/build.py --mode=uefi

# Compilar ambos modos en un solo paso
python scripts/build.py --mode=dual
```

La salida serial de debug de QEMU aparece directamente en la terminal gracias a `-serial stdio`.

---

## Solución de problemas

### `objcopy` no encontrado en Windows

Asegúrate de que `C:\msys64\mingw64\bin` está en tu **PATH** de sistema (no sólo de usuario). Abre una nueva terminal después de modificar el PATH.

### `xorriso` no encontrado en RHEL / CentOS

Activa el repositorio EPEL:
```bash
sudo dnf install -y epel-release && sudo dnf install -y xorriso
```

### OVMF no detectado por `build.py`

Localiza el archivo OVMF de tu sistema y crea un symlink o copia al path esperado:

```bash
# Linux — buscar OVMF
find /usr -name "OVMF*.fd" 2>/dev/null

# macOS
find $(brew --prefix) -name "edk2-x86_64-code.fd" 2>/dev/null
```

### `rustup` falla en Alpine / musl

Instala las dependencias de build necesarias antes de rustup:
```bash
sudo apk add musl-dev gcc
```

### `pyfatfs` falla al instalar

Asegúrate de tener `pip` para Python 3 y no Python 2:
```bash
python3 -m pip install pyfatfs
```
