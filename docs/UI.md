# Portix OS — User Interface

## Architecture

```
Framebuffer (backbuffer at 0x100000)
    │
    └── Layout (proportional geometry)
          │
          ├── Chrome (tab bar + side panel)
          │     ├── System Tab
          │     ├── Terminal Tab
          │     ├── Devices Tab
          │     ├── IDE Tab
          │     └── Explorer Tab
          │
          └── Content Area (per-tab render)
```

## Layout System

Defined in `graphics/driver/framebuffer.rs`. All dimensions computed as
proportions of `(screen_w, screen_h)`.

```rust
struct Layout {
    // Framebuffer size
    screen_w: usize,
    screen_h: usize,

    // Spacing
    margin: usize,    // screen_w / 80
    padding: usize,   // screen_h / 60

    // Side panel
    panel_x: usize,
    panel_y: usize,
    panel_w: usize,   // screen_w / 5
    panel_h: usize,   // screen_h - margin*2

    // Chrome tabs
    chrome_x: usize,
    chrome_y: usize,  // margin*3
    chrome_w: usize,  // screen_w - panel_w - margin*3
    chrome_h: usize,  // screen_h / 12

    // Content area
    content_x: usize,
    content_y: usize,
    content_w: usize, // same as chrome_w
    content_h: usize, // screen_h - chrome_y - chrome_h - margin*2 - padding
}
```

No resolution is hardcoded. The layout adapts to any framebuffer size.

## Tabs

### System Tab
- Kernel version, uptime
- Memory usage (heap allocator stats)
- CPU info (vendor, features, TSC frequency)
- Disk info (drive model, capacity, partition table)
- Serial debug output

### Terminal Tab
- Full terminal emulator with 8×8 font
- Command history (up/down arrow)
- Built-in commands:
  - `help` — list commands
  - `clear` — clear screen
  - `echo` — print arguments
  - `ls` — list files
  - `cat` — print file contents
  - `nano` — simple text editor
  - `sysinfo` — system information
  - `meminfo` — memory statistics
  - `reboot` — system restart
  - `shutdown` — power off

### Devices Tab
- PCI device tree (bus, device, function, vendor, class)
- ATA drive status
- Mouse configuration

### IDE Tab
- Built-in text/code editor
- File navigation
- Line numbers
- Syntax hints (future)

### Explorer Tab
- File browser
- Directory tree
- File metadata

## Font

- Fixed 8×8 bitmap font
- Stored as `[[u8; 8]; 256]` in `graphics/font.rs`
- Each glyph is 8 rows of 8 bits (1 byte per row)
- Used for terminal, tab labels, and all text rendering

## Input

USB keyboard input is not yet supported. Only PS/2 is available.

- PS/2 keyboard IRQ (IRQ1) → input buffer → terminal/chrome
- PS/2 mouse IRQ (IRQ12) → cursor position (future)

## Color Palette

| Name          | Hex     | Usage               |
|---------------|---------|---------------------|
| PORTIX_BG     | 01080F  | Background          |
| PORTIX_PANEL  | 030C18  | Panel background    |
| PORTIX_GOLD   | FFD700  | Accent/highlights   |
| PORTIX_AMBER  | FFAA00  | Secondary accent    |
| WHITE         | FFFFFF  | Primary text        |
| GREEN         | 00CC44  | OK/success          |
| RED           | EE2222  | Error               |
| BLUE          | 0055FF  | Info                |
| YELLOW        | FFFF00  | Warning             |

## Future Improvements

- TrueType/bitmap font scaling
- Window manager (moving/resizing windows)
- Mouse cursor rendering
- GPU acceleration (virtio-gpu, VMWare SVGA II)
- Higher color depth support
- Animated transitions between tabs
