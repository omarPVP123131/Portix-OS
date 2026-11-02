# 🚀 Guía de Inicio Rápido — Portix OS v1.0.0 'Cosmos'


> **En marcha en menos de 1 minuto.**  
> Esta guía te enseña a clonar, compilar, probar interactivamente y arrancar Portix OS en tu máquina o en hardware real.

---

## 📦 1. Prerrequisitos

Para compilar Portix OS necesitas herramientas estándar presentes en cualquier distribución Linux moderna (Ubuntu, Debian, Fedora, Arch Linux) o en macOS / Windows (WSL2):

```bash
# Ubuntu / Debian
sudo apt-get update
sudo apt-get install -y nasm qemu-system-x86 python3 python3-pip

# Arch Linux
sudo pacman -S nasm qemu-system-x86 python

# macOS (Homebrew)
brew install nasm qemu python
```

Asegúrate de contar con el compilador Rust en canal **nightly** con el target bare-metal instalado:
```bash
rustup toolchain install nightly
rustup target add x86_64-unknown-none --toolchain nightly
```

---

## 🔨 2. Compilación del Sistema (Sin Lanzar QEMU)

Para compilar el cargador BIOS MBR, el loader UEFI `BOOTX64.EFI`, el kernel en Rust puro (`no_std`) y empaquetar las imágenes booteables:

```bash
python3 scripts/build.py --no-run
```

Todos los artefactos terminados se generan limpiamente en el directorio **`build/dist/`**:
- `portix-dual.img` (Imagen híbrida BIOS + UEFI)
- `portix-uefi.iso` (Imagen ISO El Torito estándar)
- `portix.vmdk` (Disco virtual compatible con VMware / VirtualBox)

---

## 🖥️ 3. Ejecución en QEMU con Escritorio Interactivo

Para arrancar el sistema operativo directamente con la interfaz gráfica en resolución 1024×768:

```bash
# Modo Dual (Híbrido BIOS / UEFI)
python3 scripts/build.py --mode=dual

# Modo UEFI Puro (GPT + ESP con GOP Framebuffer)
python3 scripts/build.py --mode=uefi --format=iso
```

---

## 🧪 4. Ejecución de Pruebas Automatizadas

Portix OS incluye suites de integración continua exhaustivas:

```bash
# 1. Suite Interactiva de Escritorio (44 pruebas con eventos de ratón, teclado, audio y screenshot)
python3 scripts/interactive_runner.py

# 2. Suite CI Headless (5,279 pruebas de syscalls, VFS, módulos y kernel)
python3 scripts/headless_runner.py
```

---

## 💾 5. Flasheo en un Pendrive USB para Hardware Real

Para arrancar Portix OS en una laptop o PC de sobremesa física:

```bash
# 1. Listar dispositivos extraíbles conectados
python3 scripts/flash_usb.py --list

# 2. Grabar la imagen oficial de forma segura en tu unidad USB
python3 scripts/flash_usb.py --device /dev/sdX
```

> Para instrucciones detalladas de configuración de BIOS/UEFI y dual-boot, consulta la [Guía Maestra de Despliegue en Hardware Real](docs/GUIA_INSTALACION_BAREMETAL.md).

---

## 🛠️ 6. Desarrollo de Aplicaciones con el SDK

Para crear tu primera aplicación nativa en Rust con aislamiento de memoria Ring 3:

```bash
# Crear proyecto nuevo
python3 scripts/portix_sdk.py new mi_primera_app

# Empaquetar para la Portix App Store P2P
python3 scripts/portix_sdk.py package mi_primera_app
```

> Para conocer todas las APIs disponibles (`portix_rt::io`, `portix_rt::fs`, `portix_rt::net`, `portix_rt::ai`), consulta la [Guía Oficial del SDK para Desarrolladores](docs/PORTIX_SDK_GUIDE.md).

---

<div align="center">

**🌌 Portix OS v1.0.0 'Cosmos'**  
*El futuro soberano de la computación está en tus manos.*

</div>
