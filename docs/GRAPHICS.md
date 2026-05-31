# Portix OS — Graphics Subsystem

## Architecture

```
BootInfo.fb
  │
  ├── fb_base → Linear Framebuffer (LFB) — hardware video memory
  │
  └── Kernel creates:
       ├── Backbuffer (0x100000, 128 KB) — software double buffer
       ├── Framebuffer struct (pitch, width, height, bpp)
       └── Layout (proportional UI geometry)
```

## Framebuffer Initialization

Priority order:

1. **PortixBootInfo** — If boot source provides framebuffer info (BIOS VESA or EFI GOP), use it directly.
2. **Legacy pointers** — Read hardcoded addresses from known locations (for legacy BIOS boot).
3. **PCI Fallback + VBE Init** — If bootinfo has no framebuffer (EFI+S GOP failure):
   - `pci::pci_find_vga_framebuffer()` scans PCI class `0x03` (VGA controller)
   - Vendor-specific BAR layout:
     - virtio-vga → BAR1
     - Bochs VGA → BAR0
     - VMware → BAR0
   - Handles 32-bit and 64-bit MMIO BARs
   - **Bochs VBE init**: If VBE DISPI interface is not active, initializes to
     1024×768×32 via I/O ports `0x1CE` (index) / `0x1CF` (data)
   - Logs detected resolution via serial debug

## Bochs VBE Interface

| Port | Direction | Purpose                  |
|------|-----------|--------------------------|
| 0x1CE| Write     | Register index select    |
| 0x1CF| Read/Write| Register data            |

### Registers

| Index | Name     | Description              |
|-------|----------|--------------------------|
| 0     | ID       | VBE ID (0xB0C0–0xB0C4)  |
| 1     | XRES     | Horizontal resolution    |
| 2     | YRES     | Vertical resolution      |
| 3     | BPP      | Bits per pixel           |
| 4     | ENABLE   | Enable + flags           |
| 6     | VIRT_WIDTH | Virtual width (pitch)  |

### Init Sequence

```
VBE_DISPI_INDEX_ID       = 0xB0C4
VBE_DISPI_INDEX_ENABLE   = 0          (disable)
VBE_DISPI_INDEX_XRES     = 1024
VBE_DISPI_INDEX_YRES     = 768
VBE_DISPI_INDEX_BPP      = 32
VBE_DISPI_INDEX_VIRT_WIDTH = 1024
VBE_DISPI_INDEX_ENABLE   = 0x41       (enable + LFB)
```

## Double Buffering

- **LFB**: Hardware video memory. Written only during `blit()`.
- **Backbuffer**: Software buffer at `0x100000`. All draw operations write here.
- **Dirty region**: Tracks modified rectangle. `blit()` copies only dirty region to LFB.
- **Alpha blending**: Look-up table (`ALPHA_LUT[256][256]`) computed at init for fast software alpha.

### Key functions

| Function         | Purpose                                  |
|------------------|------------------------------------------|
| `pixel()`        | Set pixel in backbuffer                  |
| `rect()`         | Fill rectangle (with alpha blend)        |
| `blit()`         | Copy dirty region to LFB                 |
| `clear_dirty()`  | Reset dirty rectangle                    |
| `fast_fill_u32()`| rep stosd-based rectangle fill           |

## Layout System

All UI dimensions are computed proportionally from `(framebuffer_width, framebuffer_height)`.

```
Layout {
    screen_w, screen_h       // Framebuffer dimensions
    margin, padding           // Proportional margins
    panel_x, panel_y          // Side panel position
    panel_w, panel_h          // Side panel size
    chrome_x, chrome_y        // Tab chrome position
    chrome_w, chrome_h        // Tab chrome size
    content_x, content_y      // Content area position
    content_w, content_h      // Content area size
}
```

No hardcoded pixel values — the Layout adapts to any resolution. Font is
8×8 pixels (fixed bitmap), scaling is not yet implemented.

## GOP (UEFI Graphics Output Protocol)

Implemented in `boot/efi/src/main.rs`:

```rust
// GOP interface offsets (x86_64 UEFI):
//   +0:  MaxMode          u32
//   +4:  Mode             u32
//   +8:  Info             *mut ModeInfo
//   +16: SizeOfInfo       usize
//   +24: FrameBufferBase  u64
//   +32: FrameBufferSize  u64

// ModeInfo offsets:
//   +0:  Version              u32
//   +4:  HorizontalResolution u32
//   +8:  VerticalResolution   u32
//   +12: PixelFormat          u32
//   +32: PixelsPerScanline    u32
```

GOP detection uses `LocateHandleBuffer` (offset 312) + `OpenProtocol` (offset 280).
Falls back to `ConsoleOutHandle` if no handles found. Falls back to PCI+VBE
if GOP is unavailable (e.g., OVMF on Windows).
