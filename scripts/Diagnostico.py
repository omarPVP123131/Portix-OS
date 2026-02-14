#!/usr/bin/env python3
# scripts/diagnose.py - Diagnosticar el floppy.img

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FLOPPY = ROOT / "build" / "floppy.img"
KERNELBIN = ROOT / "build" / "kernel.bin"

def hexdump(data, offset=0, length=64):
    """Mostrar hexdump de los datos"""
    for i in range(0, min(len(data), length), 16):
        hex_str = ' '.join(f'{b:02x}' for b in data[i:i+16])
        ascii_str = ''.join(chr(b) if 32 <= b < 127 else '.' for b in data[i:i+16])
        print(f"{offset+i:08x}  {hex_str:<48}  {ascii_str}")

def main():
    print("="*70)
    print("DIAGNÓSTICO DEL FLOPPY")
    print("="*70)
    
    if not FLOPPY.exists():
        print(f"ERROR: {FLOPPY} no existe")
        return 1
    
    print(f"\n✓ Floppy encontrado: {FLOPPY}")
    print(f"  Tamaño: {FLOPPY.stat().st_size} bytes")
    
    with open(FLOPPY, "rb") as f:
        # Sector 0: Boot sector
        print("\n" + "="*70)
        print("SECTOR 0 (Boot Sector):")
        print("="*70)
        boot_sector = f.read(512)
        hexdump(boot_sector, 0, 64)
        
        # Verificar firma de boot (0x55AA)
        if boot_sector[510:512] == b'\x55\xAA':
            print("\n✓ Boot signature válida (0x55AA)")
        else:
            print(f"\n✗ Boot signature inválida: {boot_sector[510:512].hex()}")
        
        # Sector 1: Stage2
        print("\n" + "="*70)
        print("SECTOR 1 (Stage2 inicio):")
        print("="*70)
        stage2_start = f.read(512)
        hexdump(stage2_start, 512, 64)
        
        # Kernel offset (sector 65)
        f.seek(65 * 512)
        print("\n" + "="*70)
        print(f"SECTOR 65 (Kernel @ 0x{65*512:X}):")
        print("="*70)
        kernel_start = f.read(512)
        hexdump(kernel_start, 65*512, 128)
        
        # Verificar si el kernel es todo ceros
        if kernel_start == b'\x00' * 512:
            print("\n✗ PROBLEMA: El kernel está vacío (todo ceros)")
            print("  El kernel no se escribió correctamente en el floppy")
        else:
            print("\n✓ El kernel tiene datos")
            
            # Mostrar primeros 4 bytes como direcciones
            first_dword = int.from_bytes(kernel_start[0:4], 'little')
            print(f"  Primeros 4 bytes: 0x{first_dword:08X}")
    
    # Verificar kernel.bin
    print("\n" + "="*70)
    print("KERNEL.BIN:")
    print("="*70)
    
    if not KERNELBIN.exists():
        print(f"✗ {KERNELBIN} no existe")
        return 1
    
    print(f"✓ Kernel.bin encontrado: {KERNELBIN}")
    print(f"  Tamaño: {KERNELBIN.stat().st_size} bytes")
    
    with open(KERNELBIN, "rb") as f:
        kernel_data = f.read(128)
        hexdump(kernel_data, 0, 128)
    
    print("\n" + "="*70)
    print("RECOMENDACIONES:")
    print("="*70)
    
    # Comparar primeros bytes del kernel en floppy vs kernel.bin
    with open(FLOPPY, "rb") as f:
        f.seek(65 * 512)
        floppy_kernel = f.read(128)
    
    with open(KERNELBIN, "rb") as f:
        bin_kernel = f.read(128)
    
    if floppy_kernel == bin_kernel:
        print("✓ El kernel en el floppy coincide con kernel.bin")
    else:
        print("✗ El kernel en el floppy NO coincide con kernel.bin")
        print("  -> Ejecuta el build.py nuevamente")
    
    if floppy_kernel == b'\x00' * 128:
        print("✗ El kernel en el floppy está vacío")
        print("  -> Verifica que build.py esté copiando el kernel correctamente")
    
    print("\n")
    return 0

if __name__ == "__main__":
    sys.exit(main())