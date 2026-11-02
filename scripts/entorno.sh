#!/bin/bash
# Reinstala las herramientas del entorno de trabajo si el sandbox las perdió
# (rustup nightly + rust-src, nasm, QEMU, xorriso, mtools, dosfstools, pyfatfs).
set -e
export PATH=$HOME/.cargo/bin:$PATH
if ! command -v cargo >/dev/null 2>&1 || [ ! -d "$HOME/.rustup/toolchains" ]; then
  curl -sSf https://sh.rustup.rs -o /tmp/rustup.sh && sh /tmp/rustup.sh -y --default-toolchain nightly --profile minimal -c rust-src -c rustfmt >/dev/null 2>&1
fi
if ! command -v nasm >/dev/null 2>&1 || ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
  sudo apt-get update -qq >/dev/null 2>&1 || true
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq nasm qemu-system-x86 qemu-utils xorriso mtools dosfstools binutils ovmf e2fsprogs >/dev/null 2>&1
fi
# fs 2.x aún importa pkg_resources, eliminado por setuptools reciente.
# Verificar DESPUÉS de instalar; pip con salida 0 por sí solo no prueba que importe.
if ! python3 -c "import fs, pyfatfs" 2>/dev/null; then
  python3 -m pip install -q 'setuptools<81' pyfatfs fs
fi
python3 -c "import fs, pyfatfs; print('entorno: fs y pyfatfs importan correctamente')"
echo "entorno: $(rustc +nightly --version | cut -d' ' -f2) nasm=$(nasm -v | cut -d' ' -f3) qemu=$(qemu-system-x86_64 --version | head -1 | cut -d' ' -f4)"
