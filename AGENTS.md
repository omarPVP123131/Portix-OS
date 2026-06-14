# AGENTS.md — Portix OS Development Guide

## Build Commands
```sh
# BIOS ISO (El Torito)
python scripts/build.py --mode=iso

# UEFI (GPT + ESP)
python scripts/build.py --mode=uefi

# Dual BIOS+UEFI
python scripts/build.py --mode=dual

# QEMU raw disk
python scripts/build.py

# Clean build
python scripts/build.py --clean

# Lint / typecheck (rustc)
cargo +nightly check --release -Z build-std=core,alloc -Z json-target-spec --target kernel/x86_64-portix.json 2>&1
```

## Project Structure (Microkernel)
```
portix/
├── kernel/         Ring-0 microkernel (arch, mem, process, ipc, syscall, time)
├── drivers/        Ring-3 hardware drivers (ata, pci, kbd, mouse, fb)
├── servers/        Ring-3 system servers (vfs, fat32, procfs, console, ui)
├── boot/           Boot loaders (BIOS MBR + stage2, UEFI Rust)
├── lib/            Ring-3 libraries (C runtime, Rust runtime)
├── scripts/        Python build system
├── plans/          Roadmap and planning
└── docs/           Technical documentation
```

## Code Conventions
- **Kernel**: Rust nightly, `no_std`, `no_main`, freestanding
- **Drivers/Servers**: Ring-3 C or Rust, communicate via IPC
- **Assembly**: NASM syntax, `elf64` format for kernel, `bin` for boot
- **No external dependencies** in kernel (zero `[dependencies]` in Cargo.toml)
- **Logging**: All kernel code uses `drivers::serial::log()` or `write_str()`
- **Error handling**: Return `Option`/`Result`; never `unwrap()` in kernel

## Key Design Decisions
- `extern "efiapi"` for UEFI (MS x64 calling convention)
- Double buffer @ `0x100000` (1 MiB), below kernel @ `0x200000`
- Buddy allocator with intrusive free lists
- IRQ forwarding: drivers register via `SYS_REG_IRQ`, kernel forwards via IPC
- VFS routing: longest-prefix match on mount table
- IPC: fixed-size 64B messages, per-process mailbox (16 slots)

## Language
- **Siempre habla español** en todas las interacciones con el usuario.

## Testing
```sh
# QEMU with serial log
python scripts/build.py --display sdl

# Headless (CI)
python scripts/build.py --display none

# UEFI (requires OVMF)
python scripts/build.py --mode=iso-uefi
```
