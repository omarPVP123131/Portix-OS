# Portix OS — Architecture

## Target

| Field               | Value                          |
|---------------------|--------------------------------|
| Architecture        | x86_64 (IA-32e)                |
| Boot                | BIOS (stage2) + UEFI (Cargador nativo) |
| Language            | Rust nightly, no_std, no_main  |
| Linker              | rust-lld + custom linker.ld    |
| Build system        | Python (build.py v5.0)         |
| Freestanding        | Sin libc, sin libstd, sin alloc |
| Heap allocator      | Buddy system (intrusive lists) |

## Module Layout

```
kernel/src/
├── arch/              # CPU init, IDT, ISRs, GDT, serial
│   ├── hardware.rs    # init顺序: serial→GDT→IDT→PIC→PIT
│   ├── idt.rs         # IDT entries, reload_segments
│   ├── isr.asm        # ISR stubs (NASM, elf64)
│   └── isr_handlers.rs# C-level handlers, double fault guard
├── drivers/
│   ├── bus/
│   │   └── pci.rs     # PCI config space, VGA BAR scan
│   ├── serial.rs      # COM1 38400 8N1, log levels
│   ├── pit.rs         # Programmable Interval Timer (100 Hz)
│   ├── ata.rs         # ATA PIO LBA48
│   └── ps2.rs         # PS/2 keyboard + mouse
├── fs/
│   ├── fat32.rs       # FAT32 filesystem driver
│   ├── vfs.rs         # Virtual File System
│   └── fat32_private  # (mod) FAT32 internal helpers
├── graphics/
│   ├── driver/
│   │   └── framebuffer.rs  # VBE init, double buffer, Layout, Color
│   ├── font.rs        # 8x8 bitmap font
│   └── console.rs     # Console overlay (VGA-compatible interface)
├── mem/
│   ├── allocator.rs   # Buddy system allocator
│   ├── paging.rs      # Page table management
│   └── memory.rs      # Physical memory regions
├── ui/
│   ├── chrome.rs      # Tabbed UI chrome (System, Terminal, Devices, IDE, Explorer)
│   ├── input.rs       # Keyboard/mouse input routing
│   └── tabs/          # Individual tab renderers
├── console/
│   ├── terminal.rs    # Terminal emulator
│   └── terminal/commands/  # Built-in commands
├── bootinfo.rs        # PortixBootInfo struct (shared BIOS/UEFI)
└── main.rs            # Kernel entry, init sequence
```

## Interrupt Flow

```
CPU exception / IRQ
  └→ isr_handlers.rs (common_handler)
       ├→ Check crash_frame.valid
       │    ├── 0 → Console fallback + hlt (no framebuffer)
       │    └── 1 → Normal handler dispatch
       ├→ IRQ0 (PIT) timer_tick + EOI (0x20 → 0x20)
       ├→ IRQ1 (PS/2 keyboard) → input buffer
       └→ #GP/#DF → panic with register dump
```

## Boot Sequence (Kernel Entry)

```
hardware::init()
  ├── serial::init()         # COM1 38400 8N1
  ├── gdt_init()             # 64-bit GDT reload
  ├── idt_init()             # IDT + ISR stubs
  ├── pic_remap()            # PIC 1→0x20, PIC 2→0x28
  ├── pit_init()             # 100 Hz
  ├── bootinfo::init()       # Parse PortixBootInfo
  └── memory::init()         # Physical memory regions

main()
  ├── pci::init()            # Enumerate PCI bus
  ├── framebuffer::init()    # Init display (BIOS/GOP/PCI+VBE)
  ├── heap::init()           # Buddy allocator
  ├── ui::chrome::init()     # Boot splash
  ├── ata::init()            # Detect drives
  ├── fat32::init()          # Mount filesystem
  ├── keyboard::init()       # Enable PS/2 IRQ
  └── shell::run()           # Terminal main loop
```

## Key Design Decisions

1. **No global page tables at boot**. UEFI identity map is used as-is. Kernel does not set up its own pagination until explicitly needed.

2. **Double-buffered framebuffer**. LFB (hardware) + backbuffer at 0x100000. `blit()` copies dirty regions only. This avoids tearing and allows software alpha blending.

3. **Buddy allocator with intrusive lists**. Free blocks store list pointers in their own memory — no external metadata structures needed.

4. **PortixBootInfo**. Unified boot information structure passed by both BIOS stage2 and UEFI loader at 0x600000. Contains framebuffer info, memory map, reserved ranges, and firmware table locations.

5. **Serial-only debug**. No framebuffer console during early init. All boot messages go to COM1 at 38400 baud.
