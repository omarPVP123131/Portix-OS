# Portix OS — Memory Management

## Physical Memory Map

The physical memory map is provided at boot via `PortixBootInfo.memory_map`. Each entry is a `PortixMemoryRegion` (48 bytes):

```rust
struct PortixMemoryRegion {
    base: u64,              // Physical start address
    length: u64,            // Length in bytes
    kind: u32,              // MEM_USABLE, MEM_RESERVED, MEM_FRAMEBUFFER, etc.
    owner: u32,             // OWNER_FIRMWARE, OWNER_LOADER, OWNER_KERNEL
    reclaim_policy: u32,    // RECLAIM_NEVER, RECLAIM_AFTER_KERNEL_INIT
    cache_attributes: u32,  // CACHE_UC, CACHE_WB
}
```

### Region Types

| Constant              | Value | Description                    |
|-----------------------|-------|--------------------------------|
| MEM_USABLE_MAPPED     | 1     | Usable, already identity-mapped |
| MEM_USABLE            | 2     | Usable memory                  |
| MEM_RESERVED          | 3     | Reserved (do not touch)        |
| MEM_ACPI_RECLAIM      | 4     | ACPI reclaimable               |
| MEM_ACPI_NVS          | 5     | ACPI non-volatile              |
| MEM_MMIO             | 6     | MMIO region                    |
| MEM_FRAMEBUFFER       | 7     | Framebuffer memory             |
| MEM_KERNEL            | 8     | Kernel image                   |
| MEM_KERNEL_STACK      | 9     | Kernel stack                   |
| MEM_PAGE_TABLES       | 10    | Page tables                    |
| MEM_LOADER_CODE       | 11    | Loader code                    |
| MEM_LOADER_DATA       | 12    | Loader data                    |
| MEM_LOADER_STACK      | 13    | Loader stack                   |
| MEM_LOADER_HEAP       | 14    | Loader heap                    |
| MEM_FIRMWARE_RUNTIME  | 15    | Firmware runtime services      |
| MEM_BAD_MEMORY        | 16    | Known bad memory               |

### Reserved Ranges

In addition to the memory map, reserved ranges provide semantic annotations:

```rust
struct ReservedRange {
    base: u64,
    length: u64,
    kind: u32,      // Same as region types
    owner: u32,
    reclaim_policy: u32,
    cache_attributes: u32,
}
```

### Pre-allocated ranges (UEFI boot):

| Range        | Length    | Purpose           | Reclaim       |
|--------------|-----------|-------------------|---------------|
| 0x200000     | kernel_size | Kernel image    | Never         |
| 0x600000     | 0x1A00    | PortixBootInfo    | Never         |
| 0x100000     | 0x2000    | EFI loader code   | After kernel  |
| 0x700000     | 0x100000  | EFI loader data   | After kernel  |

## Address Space Layout

| Range               | Purpose                           |
|---------------------|-----------------------------------|
| 0x000000–0x0FFFFF   | Low memory (IVT, BDA, EBDA, stage2, backbuffer) |
| 0x100000–0x1FFFFF   | EFI runtime / loader data         |
| **0x200000**        | **Kernel image**                  |
| 0x500000–0x5FFFFF   | **Kernel heap (buddy allocator)** |
| **0x600000**        | **PortixBootInfo**                |
| 0x700000–0x7FFFFF   | EFI memory map (UEFI)             |
| 0x1000000–0x1FFFFFF | **Framebuffer LFB** (linear frame buffer) |

## Heap Allocator: Buddy System

The kernel heap at `0x500000` uses a buddy allocator with intrusive free lists.

### Design

- Minimum block size: 64 bytes
- Maximum order: ~20 (up to 1 MB blocks)
- Free list array: `free_lists[ORDER_COUNT]`, each entry is a linked list of free blocks
- Intrusive: free blocks store `next`/`prev` pointers in their own payload area

### Key operations

```rust
fn buddy_init(base: usize, size: usize)    // Initialize heap
fn buddy_alloc(order: usize) -> usize      // Allocate 2^order * MIN_BLOCK
fn buddy_free(ptr: usize, order: usize)    // Return block to pool
```

### Allocation strategy

1. Find smallest free list with available blocks at requested order
2. If not found, split a larger block recursively
3. On free, merge with buddy if both are free
4. Reinsert merged block into appropriate free list

## Future Plans

- Page table management for full virtual memory
- Kernel SLAB allocator for small objects
- User-space address space isolation
