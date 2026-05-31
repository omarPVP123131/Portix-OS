#![allow(dead_code)]

use core::mem::size_of;
use core::sync::atomic::{AtomicUsize, Ordering};

pub const PORTIX_BOOT_MAGIC: u64 = 0x5052_5458_424F_4F54; // "PRTXBOOT"
pub const PORTIX_BOOT_ABI_VERSION: u32 = 1;

pub const ARCH_X86_64: u32 = 1;
pub const ENDIAN_LITTLE: u32 = 1;

pub const BOOT_SOURCE_BIOS: u32 = 1;
pub const BOOT_SOURCE_UEFI: u32 = 2;

pub const BOOT_PROTOCOL_BIOS_STAGE2: u32 = 1;
pub const BOOT_PROTOCOL_UEFI_NATIVE: u32 = 2;

pub const FLAG_FRAMEBUFFER_VALID: u64 = 1 << 0;
pub const FLAG_MEMORY_MAP_VALID: u64 = 1 << 1;
pub const FLAG_FIRMWARE_TABLES_VALID: u64 = 1 << 2;
pub const FLAG_ACPI_RSDP_VALID: u64 = 1 << 3;
pub const FLAG_EFI_RUNTIME_PRESENT: u64 = 1 << 4;
pub const FLAG_EFI_RUNTIME_UNSUPPORTED: u64 = 1 << 5;
pub const FLAG_SECURE_BOOT: u64 = 1 << 6;
pub const FLAG_SMP_POSSIBLE: u64 = 1 << 7;
pub const FLAG_MODULES_PRESENT: u64 = 1 << 8;

pub const MEM_USABLE_MAPPED: u32 = 1;
pub const MEM_USABLE_UNMAPPED: u32 = 2;
pub const MEM_RESERVED: u32 = 3;
pub const MEM_ACPI_RECLAIM: u32 = 4;
pub const MEM_ACPI_NVS: u32 = 5;
pub const MEM_MMIO: u32 = 6;
pub const MEM_FRAMEBUFFER: u32 = 7;
pub const MEM_KERNEL: u32 = 8;
pub const MEM_KERNEL_STACK: u32 = 9;
pub const MEM_PAGE_TABLES: u32 = 10;
pub const MEM_LOADER_CODE: u32 = 11;
pub const MEM_LOADER_DATA: u32 = 12;
pub const MEM_LOADER_STACK: u32 = 13;
pub const MEM_LOADER_HEAP: u32 = 14;
pub const MEM_FIRMWARE_RUNTIME: u32 = 15;
pub const MEM_BAD_MEMORY: u32 = 16;

pub const OWNER_FIRMWARE: u32 = 1;
pub const OWNER_LOADER: u32 = 2;
pub const OWNER_KERNEL: u32 = 3;
pub const OWNER_DEVICE: u32 = 4;
pub const OWNER_RESERVED: u32 = 5;

pub const RECLAIM_NEVER: u32 = 0;
pub const RECLAIM_AFTER_KERNEL_INIT: u32 = 1;
pub const RECLAIM_AFTER_PAGING_TRANSITION: u32 = 2;
pub const RECLAIM_AFTER_ACPI_INIT: u32 = 3;
pub const RECLAIM_AFTER_MODULE_LOAD: u32 = 4;

pub const CACHE_UNKNOWN: u32 = 0;
pub const CACHE_UC: u32 = 1;
pub const CACHE_WC: u32 = 2;
pub const CACHE_WT: u32 = 3;
pub const CACHE_WB: u32 = 4;

pub const FB_SOURCE_BIOS_VESA: u32 = 1;
pub const FB_SOURCE_EFI_GOP: u32 = 2;

pub const FB_CANONICAL_UNKNOWN: u32 = 0;
pub const FB_CANONICAL_XRGB8888: u32 = 1;
pub const FB_CANONICAL_ARGB8888: u32 = 2;
pub const FB_CANONICAL_BGRX8888: u32 = 3;
pub const FB_CANONICAL_RGB565: u32 = 4;
pub const FB_CANONICAL_RGB888: u32 = 5;

pub const FW_TABLE_ACPI_RSDP: u32 = 1;
pub const FW_TABLE_SMBIOS: u32 = 2;
pub const FW_TABLE_EFI_SYSTEM_TABLE: u32 = 3;
pub const FW_TABLE_EFI_RUNTIME_SERVICES: u32 = 4;
pub const FW_TABLE_MP_TABLE: u32 = 5;
pub const FW_TABLE_DEVICE_TREE: u32 = 6;

static BOOTINFO_PTR: AtomicUsize = AtomicUsize::new(0);

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct PortixBootInfo {
    pub magic: u64,
    pub abi_version: u32,
    pub arch: u32,
    pub endian: u32,
    pub header_size: u32,
    pub total_size: u32,
    pub checksum: u32,
    pub flags: u64,
    pub cpu_caps: u64,
    pub firmware_caps: u64,
    pub loader_caps: u64,
    pub boot_source: u32,
    pub boot_protocol: u32,
    pub memory_map_generation: u64,
    pub framebuffer: FramebufferInfo,
    pub kernel_base: u64,
    pub kernel_size: u64,
    pub memory_map_offset: u32,
    pub memory_map_count: u32,
    pub memory_map_entry_size: u32,
    pub reserved_ranges_offset: u32,
    pub reserved_ranges_count: u32,
    pub reserved_range_entry_size: u32,
    pub firmware_tables_offset: u32,
    pub firmware_tables_count: u32,
    pub firmware_table_entry_size: u32,
    pub modules_offset: u32,
    pub modules_count: u32,
    pub strings_offset: u32,
    pub strings_size: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FramebufferInfo {
    pub base: u64,
    pub framebuffer_size: u64,
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: u32,
    pub bpp: u32,
    pub source: u32,
    pub canonical_format: u32,
    pub pixel_format: u32,
    pub pixels_per_scanline: u32,
    pub cache_policy: u32,
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub reserved_mask: u32,
    pub red_shift: u8,
    pub green_shift: u8,
    pub blue_shift: u8,
    pub reserved_shift: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PortixMemoryRegion {
    pub base: u64,
    pub length: u64,
    pub kind: u32,
    pub owner: u32,
    pub reclaim_policy: u32,
    pub cache_attributes: u32,
    pub firmware_type: u32,
    pub attributes: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ReservedRange {
    pub base: u64,
    pub length: u64,
    pub kind: u32,
    pub owner: u32,
    pub reclaim_policy: u32,
    pub cache_attributes: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FirmwareTableEntry {
    pub kind: u32,
    pub flags: u32,
    pub address: u64,
    pub length: u64,
}

pub unsafe fn init(ptr: *const PortixBootInfo) {
    BOOTINFO_PTR.store(ptr as usize, Ordering::Release);
}

pub fn get() -> Option<&'static PortixBootInfo> {
    let ptr = BOOTINFO_PTR.load(Ordering::Acquire) as *const PortixBootInfo;
    if ptr.is_null() {
        return None;
    }
    let info = unsafe { &*ptr };
    if validate(info) {
        Some(info)
    } else {
        None
    }
}

pub fn validate(info: &PortixBootInfo) -> bool {
    if info.magic != PORTIX_BOOT_MAGIC {
        return false;
    }
    if info.abi_version != PORTIX_BOOT_ABI_VERSION {
        return false;
    }
    if info.arch != ARCH_X86_64 || info.endian != ENDIAN_LITTLE {
        return false;
    }
    if info.header_size < size_of::<PortixBootInfo>() as u32 {
        return false;
    }
    if info.total_size < info.header_size {
        return false;
    }
    checksum_words(info as *const _ as *const u32, info.total_size as usize / 4) == 0
}

pub fn memory_regions(info: &PortixBootInfo) -> &'static [PortixMemoryRegion] {
    if info.memory_map_offset == 0
        || info.memory_map_entry_size as usize != size_of::<PortixMemoryRegion>()
    {
        return &[];
    }
    unsafe {
        core::slice::from_raw_parts(
            (info as *const _ as usize + info.memory_map_offset as usize)
                as *const PortixMemoryRegion,
            info.memory_map_count as usize,
        )
    }
}

pub fn firmware_tables(info: &PortixBootInfo) -> &'static [FirmwareTableEntry] {
    if info.firmware_tables_offset == 0
        || info.firmware_table_entry_size as usize != size_of::<FirmwareTableEntry>()
    {
        return &[];
    }
    unsafe {
        core::slice::from_raw_parts(
            (info as *const _ as usize + info.firmware_tables_offset as usize)
                as *const FirmwareTableEntry,
            info.firmware_tables_count as usize,
        )
    }
}

pub fn checksum_bytes(ptr: *const u8, len: usize) -> u32 {
    let mut sum = 0u32;
    for i in 0..len {
        let b = unsafe { core::ptr::read_volatile(ptr.add(i)) };
        sum = sum.wrapping_add(b as u32);
    }
    sum
}

pub fn checksum_words(ptr: *const u32, len: usize) -> u32 {
    let mut sum = 0u32;
    for i in 0..len {
        let w = unsafe { core::ptr::read_volatile(ptr.add(i)) };
        sum = sum.wrapping_add(w);
    }
    sum
}
