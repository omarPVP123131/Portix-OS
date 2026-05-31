# Portix OS — Boot Chain

## Overview

Portix supports two boot paths:

- **BIOS Legacy** (MBR → stage2 → kernel.bin)
- **UEFI** (GPT ESP → BOOTX64.EFI → kernel.bin)

Both paths produce the same `PortixBootInfo` structure at physical address `0x600000`, then jump to kernel at `0x200000`.

---

## 1. BIOS Boot Path

```
BIOS
 └→ boot.asm (LBA 0, 512 bytes)
     └→ Load stage2.bin from LBA 1-67
         └→ stage2.asm (32 KB)
             ├── INT 13h/42h: Read kernel.bin to 0x200000
             ├── VESA mode set (1024×768×32, via INT 10h/4F02)
             ├── Build PortixBootInfo at 0x600000
             └── Far jump to 0x200000
```

### boot.asm
- Standard MBR with partition table.
- Loads stage2 from LBA 1-67 to `0x7E00`.
- Jumps to `0x7E00`.

### stage2.asm
- **CD boot detection**: Uses `INT 13h/48h` to detect CD/DVD media (2048-byte sectors vs 512-byte). Backward-copies boot code from `0x7E00` to `0x8000` when booting from El Torito.
- **Kernel load**: Reads kernel from LBA 68 via `INT 13h/42h` (LBA48) to physical `0x200000`.
- **VESA**: Calls `INT 10h/4F01` (get mode info) and `INT 10h/4F02` (set mode) for 1024×768×32. Falls back to 640×480×32 if the desired mode is unavailable.
- **A20 gate**: Enables via port `0x92` and INT 15h/AX=2401.
- **BootInfo**: Writes `PortixBootInfo` at `0x600000` with magic `0x50525458424F4F54`.

---

## 2. UEFI Boot Path

```
UEFI Firmware
 └── GPT: ESP (FAT32)
      └── /EFI/BOOT/BOOTX64.EFI
           ├── LoadedImageProtocol → device_handle
           ├── Block I/O → read FAT32 partition
           ├── LocateHandleBuffer(GOP) → framebuffer
           ├── Build PortixBootInfo
           ├── ExitBootServices
           └── jmp 0x200000  (RDI = 0x600000)
```

### EFI Loader (`boot/efi/src/main.rs`)

**Protocol table offsets** (x86_64 UEFI boot services):

| Offset | Service             | Used? |
|--------|---------------------|-------|
| 40     | AllocatePages       | Yes   |
| 56     | GetMemoryMap        | Yes   |
| 64     | AllocatePool        | Yes   |
| 72     | FreePool            | Yes   |
| 280    | OpenProtocol        | Yes   |
| 312    | LocateHandleBuffer  | Yes   |
| 320    | LocateProtocol      | Debug |

**GOP detection sequence**:
1. `LocateHandleBuffer(2, &GOP_GUID, NULL, &count, &buffer)` — find all handles supporting GOP
2. If count == 0: fallback to `ConsoleOutHandle` (SystemTable + 0x38)
3. `OpenProtocol(handle, &GOP_GUID, &interface, image, NULL, 0x02)`
4. Read GOP mode info: `interface+8 → mode; mode+24 → fb_base; mode+32 → fb_size`
5. Read mode info: `mode+8 → info; info+4 → width; info+12 → pixel_format; info+32 → pixels_per_scanline`

**Block I/O + FAT32**:
- Opens `Block I/O` protocol on `device_handle` (partition-relative LBA)
- Reads FAT32 VBR at LBA 0 → BPB
- Walks root cluster chain: `/PORTIX/KERNEL.BIN`
- Allocates pages at `0x200000` via `AllocatePages`
- Reads file clusters into kernel memory

**BootInfo construction**:
- Allocates pages at `0x600000`
- Writes magic, flags, framebuffer info, memory map, reserved ranges, ACPI RSDP
- Calculates 32-bit checksum over `BI_TOTAL_SIZE` (0x1A00 bytes)

**ExitBootServices**:
- Single call with memory map key
- Retry with fresh key on failure

---

## 3. PortixBootInfo Format

Physical address: `0x600000`. Size: `0x1A00` (6656) bytes.

| Offset | Size | Field               | Description                        |
|--------|------|---------------------|------------------------------------|
| 0x00   | 8    | magic               | `0x50525458424F4F54` ("PRTXBOOT") |
| 0x08   | 4    | abi_version         | 1                                  |
| 0x0C   | 4    | arch                | 1 = x86_64                         |
| 0x10   | 4    | endian              | 1 = little                         |
| 0x14   | 4    | header_size         | Size of header                     |
| 0x18   | 4    | total_size          | Total BootInfo size                |
| 0x1C   | 4    | checksum            | 32-bit sum = 0                     |
| 0x20   | 8    | flags               | Bitfield                           |
| 0x28   | 8    | cpu_caps            | CPU capabilities                   |
| 0x40   | 4    | boot_source         | 1=BIOS, 2=UEFI                     |
| 0x44   | 4    | boot_protocol       | 1=stage2, 2=UEFI_native            |
| 0x50   | 8    | fb.base             | Framebuffer physical address       |
| 0x58   | 8    | fb.size             | Framebuffer size in bytes          |
| 0x60   | 4    | fb.width            | Horizontal resolution              |
| 0x64   | 4    | fb.height           | Vertical resolution                |
| 0x68   | 4    | fb.pitch            | Bytes per scanline                 |
| 0x6C   | 4    | fb.bpp              | Bits per pixel                     |
| 0x70   | 4    | fb.source           | 1=VESA, 2=EFI_GOP                  |
| 0x74   | 4    | fb.canonical_format | 1=XRGB8888, 3=BGRX8888            |
| 0x78   | 4    | fb.pixel_format     | Raw GOP pixel format               |
| 0x7C   | 4    | fb.pixels_per_scan  | Pixels per scanline                |
| 0x80   | 4    | fb.cache_policy     | 0=UC, 4=WB                         |
| 0x98   | 8    | kernel_base         | Kernel physical address            |
| 0xA0   | 8    | kernel_size         | Kernel size in bytes               |
| 0xA8   | ...  | memory_map          | Array of memory regions            |
| 0x100  | ...  | mmap entries        | Up to 128 × 48 bytes               |
| 0x1900 | ...  | reserved ranges     | Up to 64 × 32 bytes                |
| 0x1A00 | ...  | firmware tables     | Up to 16 × 24 bytes                |

---

## 4. Memory Layout After Boot

| Range               | Usage                    | Owner     |
|---------------------|--------------------------|-----------|
| 0x00000000-0x00000FFF | IVT / BDA (BIOS)        | Firmware  |
| 0x00001000-0x00001FFF | Stage2 scratch (BIOS)   | Loader    |
| 0x00007C00-0x00007DFF | Boot sector (BIOS)      | Firmware  |
| 0x00007E00-0x0000FFFF | Stage2 code (BIOS)      | Loader    |
| 0x00010000-0x0008FFFF | Backbuffer (128 KB)     | Kernel    |
| 0x00100000-0x001FFFFF | EFI loader data (UEFI)  | Loader    |
| **0x00200000**        | **Kernel .text+.data**  | **Kernel** |
| 0x00500000-0x005FFFFF | Heap (buddy allocator)  | Kernel    |
| **0x00600000**        | **PortixBootInfo**      | **Kernel** |
| 0x00700000-0x007FFFFF | EFI memory map (UEFI)   | Kernel    |
| 0x01000000-0x01FFFFFF | Framebuffer LFB         | Hardware  |
