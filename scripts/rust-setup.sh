#!/usr/bin/env bash
# scripts/rust-setup.sh — Build PORTIX 100% Rust Ring-3 Workspace
#
# Requires: Rust nightly toolchain with rust-src component
# Setup: rustup toolchain install nightly && rustup component add rust-src --toolchain nightly

set -e

export PATH="$HOME/.cargo/bin:$PATH"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
USERLAND_DIR="$ROOT/userland"

echo "=== Building Portix Userland Workspace (100% Pure Rust) ==="
cd "$USERLAND_DIR"
cargo +nightly build --release -Z build-std=core,alloc

echo ""
echo "=== Portix Userland Binaries ==="
ls -lh "$USERLAND_DIR/target/x86_64-userland/release" | grep -v "\.d$" || true

echo ""
echo "Done! 100% Pure Rust Ring-3 userland successfully built."

