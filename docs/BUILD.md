# Portix OS — Sistema de Build

## Uso

```sh
python scripts/build.py --mode=MODO [--format=FMT] [--vga=TIPO] [--no-run] [--clean]
```

### Modos

| Modo   | Imágenes generadas                                    | Método de arranque       | Caso de uso              |
|--------|-------------------------------------------------------|--------------------------|--------------------------|
| `bios` | `portix.iso`, `portix.img`                            | BIOS (El Torito + MBR)   | Pruebas BIOS, VBox       |
| `uefi` | `portix-uefi.img`, `portix-uefi.iso`                  | UEFI (GPT + ESP)         | OVMF, hardware real      |
| `dual` | `portix-dual.iso` (híbrida), `portix-dual.img` + UEFI | BIOS + UEFI              | Validación completa      |
| `all`  | Todo lo anterior                                      | BIOS + UEFI + híbrida    | Build de release         |

**Por defecto**: `--mode=all` (construye todo)

### Filtro de formato

| Flag            | Efecto                                    |
|-----------------|-------------------------------------------|
| `--format=iso`  | Solo archivos ISO                         |
| `--format=img`  | Solo imágenes de disco IMG                |
| (omitido)       | Todos los formatos del modo indicado      |

Ejemplos:
```sh
python scripts/build.py --mode=bios --format=iso   # solo portix.iso
python scripts/build.py --mode=dual --format=img   # solo portix-dual.img
```

### Adaptador VGA

```sh
python scripts/build.py --vga=std     # VGA estándar QEMU (Bochs VBE)
python scripts/build.py --vga=virtio  # virtio-vga (requiere fallback PCI)
python scripts/build.py --vga=qxl     # SPICE QXL
python scripts/build.py --vga=vmware  # VMware SVGA II (experimental)
```

### Otros flags

| Flag        | Efecto                                        |
|-------------|-----------------------------------------------|
| `--clean`   | Elimina todos los artefactos de compilación   |
| `--no-run`  | Solo compilar, no lanzar QEMU                 |

---

## Flujo de build

```
1.  check_tools()
      Verifica: nasm, cargo, qemu, objcopy, xorriso, qemu-img

2.  assemble_boot()
      nasm boot.asm    → boot.bin
      nasm isr.asm     → isr.o

3.  build_kernel()
      cargo +nightly build --release
        -Z build-std=core,alloc
        -Z json-target-spec
        --target x86_64-portix.json
      objcopy → kernel.bin (ELF sin símbolos de debug)

4.  assemble_stage2()
      nasm stage2.asm  (con define KERNEL_SECTORS)

5.  create_raw()
      Ensambla imagen MBR  → portix.img

6.  create_iso()
      ISO9660 El Torito vía xorriso, genisoimage o pycdlib
      Fallback a copia raw si no hay herramienta ISO disponible
      ISO híbrida (modo dual): agrega -e esp.img para entrada El Torito UEFI

7.  [modo uefi] build_efi_loader()
      cargo +nightly para x86_64-unknown-uefi
    create_uefi_image()
      Constructor GPT en Python + pyfatfs para la ESP
    create_uefi_iso()
      xorriso con flag -e para arranque EFI

8.  run_qemu()
      Lanzamiento de VM según modo (img BIOS, UEFI con OVMF, o dual)
```

---

## Prerrequisitos

Ver `PREREQUISITES.md` en la raíz del proyecto para instrucciones completas
de instalación en Windows y Linux.

### Resumen rápido

| Herramienta          | Requerida    | Instalar (Windows)              | Instalar (Linux)           |
|----------------------|--------------|---------------------------------|----------------------------|
| Python 3             | Sí           | python.org                      | `apt install python3`      |
| Rust nightly         | Sí           | rustup.rs                       | `rustup`                   |
| NASM                 | Sí           | nasm.us                         | `apt install nasm`         |
| QEMU                 | Sí           | qemu.org                        | `apt install qemu-system-x86` |
| objcopy (binutils)   | Sí           | MSYS2 mingw64                   | `apt install binutils`     |
| xorriso              | Recomendada  | MSYS2: `pacman -S libisoburn`   | `apt install xorriso`      |
| pyfatfs              | Solo UEFI    | `pip install pyfatfs`           | `pip install pyfatfs`      |
| OVMF                 | Solo UEFI    | Carpeta share de QEMU (OVMF.fd) | `apt install ovmf`         |

> **Nota**: para la ISO híbrida (modo `dual`) xorriso es obligatorio. Sin
> xorriso, el modo `dual` recurre a una copia de la ISO solo-BIOS.

---

## Archivos de salida

| Archivo                           | Tamaño aprox. | Formato                       | Arranque                   |
|-----------------------------------|---------------|-------------------------------|----------------------------|
| `build/dist/portix.iso`           | ~8 MB         | ISO9660 + El Torito           | BIOS                       |
| `build/dist/portix.img`           | ~8 MB         | MBR raw                       | BIOS MBR                   |
| `build/dist/portix-dual.iso`      | ~8 MB         | ISO9660 + El Torito dual      | BIOS + UEFI (híbrida)      |
| `build/dist/portix-dual.img`      | ~8 MB         | MBR raw                       | BIOS MBR (copia dual)      |
| `build/dist/portix-uefi.iso`      | ~64 MB        | ISO9660 + El Torito EFI       | UEFI                       |
| `build/dist/portix-uefi.img`      | 64 MB         | GPT + ESP FAT32               | UEFI                       |
| `build/portix.img`                | ~8 MB         | MBR raw                       | BIOS MBR (intermedio)      |
