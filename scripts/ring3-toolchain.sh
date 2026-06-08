#!/usr/bin/env bash
# scripts/ring3-toolchain.sh — Build x86_64-elf cross-compiler for PORTIX
#
# Builds binutils + GCC targeting x86_64-elf (freestanding ELF64).
# Output installed to /usr/local/x86_64-elf/ (or $PREFIX).
#
# Prerequisites:
#   Linux: apt install build-essential flex bison libgmp-dev libmpfr-dev libmpc-dev
#   MSYS2: pacman -S base-devel flex bison gmp-devel mpfr-devel mpc-devel
#
# Usage: bash scripts/ring3-toolchain.sh [--prefix=/path]

set -euo pipefail

PREFIX="${1#--prefix=}"
PREFIX="${PREFIX:-/usr/local/x86_64-elf}"
TARGET="x86_64-elf"
GCC_VERSION="13.2.0"
BINUTILS_VERSION="2.41"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK_DIR="${SCRIPT_DIR}/../build/toolchain-src"

mkdir -p "$WORK_DIR" "$PREFIX"
cd "$WORK_DIR"

log() { echo "[TOOLCHAIN] $*"; }

download() {
    local url="$1" file="$2"
    if [ ! -f "$file" ]; then
        log "Downloading $file..."
        wget -q "$url" -O "$file" || curl -sL "$url" -o "$file"
    else
        log "Already have $file"
    fi
}

build_binutils() {
    local src="binutils-${BINUTILS_VERSION}"
    local tar="${src}.tar.xz"

    download "https://ftp.gnu.org/gnu/binutils/${tar}" "$tar"

    if [ ! -d "$src" ]; then
        log "Extracting binutils..."
        tar xf "$tar"
    fi

    mkdir -p "build-binutils"
    cd "build-binutils"
    if [ ! -f "Makefile" ]; then
        log "Configuring binutils..."
        "../${src}/configure" \
            --target="$TARGET" \
            --prefix="$PREFIX" \
            --with-sysroot \
            --disable-nls \
            --disable-werror \
            --enable-gold=yes \
            --enable-ld=default
    fi
    log "Building binutils (make -j$(nproc))..."
    make -j"$(nproc)"
    log "Installing binutils..."
    make install
    cd ..
}

build_gcc() {
    local src="gcc-${GCC_VERSION}"
    local tar="${src}.tar.xz"

    download "https://ftp.gnu.org/gnu/gcc/gcc-${GCC_VERSION}/${tar}" "$tar"

    if [ ! -d "$src" ]; then
        log "Extracting GCC..."
        tar xf "$tar"
    fi

    mkdir -p "build-gcc"
    cd "build-gcc"
    if [ ! -f "Makefile" ]; then
        log "Configuring GCC..."
        "../${src}/configure" \
            --target="$TARGET" \
            --prefix="$PREFIX" \
            --disable-nls \
            --enable-languages=c \
            --without-headers \
            --disable-hosted-libstdcxx \
            --disable-libssp \
            --disable-libgomp \
            --disable-libquadmath \
            --disable-threads \
            --disable-shared \
            --enable-static \
            --with-newlib
    fi
    log "Building GCC (make -j$(nproc) all-gcc)..."
    make -j"$(nproc)" all-gcc
    log "Installing GCC..."
    make install-gcc
    cd ..
}

log "Starting x86_64-elf cross-compiler build"
log "Prefix: $PREFIX"
log "Work dir: $WORK_DIR"
log ""

build_binutils
PATH="$PREFIX/bin:$PATH" build_gcc

log ""
log "=== TOOLCHAIN BUILD COMPLETE ==="
log "x86_64-elf-gcc installed at: ${PREFIX}/bin/x86_64-elf-gcc"
log "Add to PATH: export PATH=${PREFIX}/bin:\$PATH"
log ""
log "Then build libportix:  cd lib && make"
