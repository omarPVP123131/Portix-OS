# Portix OS — Storage Subsystem

## Architecture

```
User / Shell
    │
    ├── fs::vfs — Virtual File System
    │     └── Lookup, read, write, open, close
    │
    ├── fs::fat32 — FAT32 Implementation
    │     └── Cluster chain walk, directory entry parsing
    │
    └── drivers::ata — ATA PIO Driver
          └── Read/write sectors via PIO (LBA48)
```

## ATA PIO Driver (`kernel/src/drivers/ata.rs`)

### Features

- Primary and secondary ATA buses
- Master/slave drive detection
- LBA48 (48-bit) sector addressing
- PIO data transfers (no DMA)
- Sector cache to reduce bus resets
- Drive identification via IDENTIFY command

### I/O Ports (Primary Bus)

| Port    | Register               | Direction |
|---------|------------------------|-----------|
| 0x1F0   | Data                   | R/W       |
| 0x1F1   | Features/Error         | R/W       |
| 0x1F2   | Sector count           | R/W       |
| 0x1F3   | LBA low                | R/W       |
| 0x1F4   | LBA mid                | R/W       |
| 0x1F5   | LBA high               | R/W       |
| 0x1F6   | Drive/Head             | R/W       |
| 0x1F7   | Command/Status         | R/W       |
| 0x3F6   | Control                | W         |

### Key Functions

| Function            | Description                             |
|---------------------|-----------------------------------------|
| `ata_init()`        | Detect drives on both buses             |
| `ata_read()`        | Read sectors with LBA48                |
| `ata_write()`       | Write sectors with LBA48               |
| `ata_identify()`    | Send IDENTIFY command, parse response  |

### Protocol (PIO Read)

```
1. Wait for BSY == 0
2. Write LBA48 registers (sector count, LBA low/mid/high × 2)
3. Write DRV bit + LBA bit to 0x1F6
4. Write READ command (0x24) to 0x1F7
5. Poll DRQ bit
6. Read 256 words from 0x1F0
7. Repeat for next sector
```

### Sector Cache

A small internal cache stores recently accessed sectors to avoid unnecessary
bus resets. The cache is a simple fixed-size array indexed by LBA.

## FAT32 Filesystem (`kernel/src/fs/fat32.rs`)

### On-disk Structure

```
MBR → VBR (LBA 0)
 ├── Reserved sectors
 ├── FAT #1
 ├── FAT #2 (mirror)
 ├── Root directory (cluster chain)
 └── Data clusters
```

### BPB Fields (used by driver)

| Field               | Offset | Description                |
|---------------------|--------|----------------------------|
| bytes_per_sector    | 0x0B   | Usually 512                |
| sectors_per_cluster | 0x0D   | Usually 1-128              |
| reserved_count      | 0x0E   | Reserved sector count      |
| num_fats            | 0x10   | Usually 2                  |
| sectors_per_fat     | 0x24   | FAT size in sectors        |
| root_cluster        | 0x2C   | First cluster of root dir  |

### Cluster Chain Walking

```rust
fn get_fat_entry(cluster: u32) -> u32  // Read FAT entry
fn next_cluster(cluster: u32) -> u32   // Get next in chain (or EOC marker)
```

End-of-chain markers: `≥ 0x0FFFFFF8`.

### Directory Entry Format (32 bytes)

| Offset | Size | Field        |
|--------|------|--------------|
| 0      | 8    | Short name   |
| 8      | 3    | Extension    |
| 11     | 1    | Attributes   |
| 13     | 1    | Reserved     |
| 14-15  | 2    | Create time  |
| 16-17  | 2    | Create date  |
| 18-19  | 2    | Access date  |
| 20-21  | 2    | Cluster high |
| 22-23  | 2    | Modify time  |
| 24-25  | 2    | Modify date  |
| 26-27  | 2    | Cluster low  |
| 28-31  | 4    | File size    |

### Long File Names (LFN)

LFN entries precede their short-name entry. Each LFN entry uses attribute
`0x0F` and stores up to 13 UTF-16 characters in a non-standard layout.

## VFS (Virtual File System)

Provides a unified interface:

```rust
trait FileSystem {
    fn read(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize>;
    fn write(&self, path: &str, offset: u64, buf: &[u8]) -> Result<usize>;
    fn list(&self, path: &str) -> Result<Vec<DirEntry>>;
    fn mkdir(&self, path: &str) -> Result<()>;
    fn remove(&self, path: &str) -> Result<()>;
}
```

Currently only FAT32 is implemented. The VFS layer routes calls to the
FAT32 driver based on mount point.
