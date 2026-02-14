# PORTIX — Rust UEFI bootstrap (v0.1)


Objetivo: compilar un `.EFI` mínimo en Rust (`no_std`) que imprima "Hola Mundo" en la consola UEFI, y probarlo en QEMU sin tocar tu PC real.


## Requisitos (Windows)
- Rust (nightly recomendado)
- LLVM / lld (LLVM linker)
- NASM (para asm futuro)
- QEMU
- OVMF (para firmware UEFI en QEMU) o usar `-bios` adecuado
- Herramienta para crear ISO: `oscdimg` (Windows ADK) o `mkisofs`/`genisoimage`


## Pasos
1. Instala dependencias.
2. Desde la raíz del repo ejecuta:


```bash
python scripts/build.py
python scripts/iso.py
python scripts/run_qemu.py