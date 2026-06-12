#!/usr/bin/env bash
# scripts/rust-setup.sh — Build PORTIX ring-3 Rust runtime and examples
#
# Requires: Rust nightly toolchain with rust-src component
#
# Setup: rustup toolchain install nightly && rustup component add rust-src --toolchain nightly

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
RUST_DIR="$ROOT/lib/rust"

echo "=== Building portix_rt (Rust ring-3 runtime) ==="
cd "$RUST_DIR/portix_rt"
cargo build --target ../x86_64-portix-none.json -Z build-std=core,alloc --release

echo ""
echo "=== Building hello example (Rust ring-3 program) ==="
cd "$RUST_DIR/examples/hello"
cargo build --target ../../x86_64-portix-none.json -Z build-std=core,alloc --release

echo ""
echo "=== Rust artifacts ==="
ls -lh "$RUST_DIR/portix_rt/target/x86_64-portix-none/release/libportix_rt.rlib" 2>/dev/null || true
ls -lh "$RUST_DIR/examples/hello/target/x86_64-portix-none/release/hello" 2>/dev/null || true

echo ""
echo "Done! Rust ring-3 runtime compiled."
echo "To link a Rust program for PORTIX, use:"
echo "  rust-lld -T $ROOT/build/linker.ld -o myapp.elf hello.o libportix_rt.rlib"
