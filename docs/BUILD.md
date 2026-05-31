# Portix OS — Build System

## Prerequisites

| Tool           | Required | Notes                          |
|----------------|----------|--------------------------------|
| Python 3       | Yes      | Build script (build.py v5.0)  |
| Rust nightly   | Yes      | rustup + nightly toolchain    |
| NASM           | Yes      | Assembler for boot + ISR      |
| QEMU           | Yes      | Emulator for testing          |
| objcopy        | Yes      | From binutils (ELF→binary)    |
| xorriso        | Optional | ISO creation (genisoimage also works) |
| qemu-img       | Optional | VDI/VMDK conversion           |

## Usage

```sh
python scripts/build.py [--mode=MODE] [--vga=TYPE] [--clean]
```

### Modes

| Mode          | Image                    | Boot Target        | Description                         |
|---------------|--------------------------|--------------------|-------------------------------------|
| `raw` (default) | portix.img             | BIOS (MBR)         | Legacy BIOS boot                    |
| `iso`         | portix.iso               | BIOS (El Torito)   | ISO9660 with no-emul boot           |
| `uefi`        | portix-uefi.img          | UEFI (GPT+ESP)     | OVMF UEFI boot                      |
| `dual`        | portix-dual.img + uefi   | BIOS + UEFI        | Combined BIOS + separate UEFI image |
| `ventoy-sim`  | portix-ventoy-sim.img    | Ventoy-simulated   | Tests Ventoy compatibility          |

### VGA Adapter Selection

```sh
python scripts/build.py --vga=std
python scripts/build.py --vga=virtio
python scripts/build.py --vga=qxl
python scripts/build.py --vga=vmware  # EXPERIMENTAL
```

QEMU will use the specified VGA adapter. Default is `std`.

### Other Flags

| Flag         | Effect                                  |
|--------------|-----------------------------------------|
| `--clean`    | Clean build artifacts before building   |
| `--no-run`   | Build only, don't launch QEMU           |
| `--no-iso`   | Skip ISO creation (raw mode only)       |

## Build Flow

```
build.py
 ├── Verify tools (nasm, cargo, qemu, objcopy, xorriso, qemu-img)
 ├── Assemble boot.asm → boot.bin (512 bytes)
 ├── Assemble isr.asm → isr.o (NASM elf64)
 ├── cargo +nightly build --release
 │     -Z build-std=core,alloc
 │     -Z json-target-spec
 │     --target kernel/x86_64-portix.json
 ├── objcopy → kernel.bin (ELF → binary, stripped)
 ├── Assemble stage2.asm → stage2.bin (with KERNEL_SECTORS)
 ├── Create portix.img (MBR + stage2 + kernel)
 ├── Create portix.iso (xorriso, no-emul+boot)
 ├── Create portix-uefi.img (GPT+ESP FAT32, Python-puro)
 └── Launch QEMU (mode-dependent)
```

## Kernel Target Specification

File: `kernel/x86_64-portix.json`

```json
{
  "llvm-target": "x86_64-unknown-none",
  "arch": "x86_64",
  "os": "none",
  "linker-flavor": "ld.lld",
  "linker": "rust-lld",
  "panic-strategy": "abort",
  "disable-redzone": true,
  "relocation-model": "static",
  "code-model": "small",
  "pre-link-args": {
    "ld.lld": ["-Tlinker.ld"]
  }
}
```

## Linker Script

File: `kernel/linker.ld`

```
KERNEL_PHYS_ADDR = 0x200000;

.text  : AT(ADDR(.text))  { *(.text*)   } > ram
.rodata: AT(ADDR(.rodata)){ *(.rodata*) } > ram
.data  : AT(ADDR(.data))  { *(.data*)   } > ram
.bss   : AT(ADDR(.bss))   { *(.bss*)    } > ram

/DISCARD/ : { *(.comment*) *(.note*) *(.eh_frame*) }
```

## OVMF Setup

The build script searches for OVMF firmware in:

| Platform | Path                                         |
|----------|----------------------------------------------|
| Windows  | `C:\Program Files\qemu\share\edk2-x86_64-code.fd` |
| Linux    | `/usr/share/edk2-ovmf/x64/OVMF_CODE.fd`     |
| Linux    | `/usr/share/OVMF/OVMF_CODE.fd`               |

If not found, the script prints setup instructions.

## Output Files

| File                       | Size   | Format       | Boot           |
|----------------------------|--------|--------------|----------------|
| `build/dist/portix.img`    | 8 MB   | MBR raw      | BIOS           |
| `build/dist/portix.iso`    | 8.4 MB | ISO9660      | BIOS El Torito |
| `build/dist/portix-uefi.img` | 64 MB | GPT+ESP FAT32 | UEFI          |
| `build/dist/portix.vdi`    | 1 MB   | VDI          | VirtualBox     |
| `build/dist/portix.vmdk`   | 576 KB | VMDK         | VMware         |
