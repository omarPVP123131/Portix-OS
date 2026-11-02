#!/bin/bash
# [T-003] Inyecta boot.bin + stage2.bin + kernel.bin recien construidos en una
# imagen conservada (PORTIX_CONSERVAR_IMG) para iterar sin reinstalar paquetes.
# Uso: scripts/sondas/inyectar_kernel.sh build/dist/img_gcc.img
set -euo pipefail
cd "$(dirname "$0")/../.."
IMG="${1:-}"; [ -f "$IMG" ] || { echo "no existe $IMG"; exit 1; }
IMG="$(realpath -- "$IMG")"
DIST="$(realpath -- build/dist)"
case "$IMG" in
  "$DIST"/*) ;;
  *) echo "La imagen debe estar dentro de build/dist/"; exit 1 ;;
esac
dd if=build/boot.bin   of="$IMG" bs=446 count=1 conv=notrunc status=none   # solo el codigo: la tabla de particiones se conserva
dd if=build/stage2.bin of="$IMG" bs=512 seek=1  conv=notrunc status=none
dd if=build/kernel.bin of="$IMG" bs=512 seek=68 conv=notrunc status=none
echo "kernel $(stat -c %s build/kernel.bin) B inyectado en $IMG (LBA 68)"
