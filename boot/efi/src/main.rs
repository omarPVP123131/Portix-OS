#![no_std]
#![no_main]

use core::ffi::c_void;

// ═══════════════════════════════════════════════════════════════════════════════
//  UEFI types
// ═══════════════════════════════════════════════════════════════════════════════

type EfiHandle = *mut c_void;
type EfiStatus  = usize;
#[allow(unused)]
type _EfiEvent   = *mut c_void;

// ── Serial debug (COM1, I/O port 0x3F8) ──────────────────────────────────
// Must use 'out' instruction, not memory write!
const COM1_DATA: u16 = 0x3F8;
#[allow(unused)]
const _COM1_LSR:  u16 = 0x3FD; // Line Status Register, bit 5 = THRE

#[inline(never)]
unsafe fn serial_putc(c: u8) {
    core::arch::asm!("out dx, al", in("dx") COM1_DATA, in("al") c, options(nostack, preserves_flags));
}

unsafe fn serial_puts(s: &[u8]) {
    for &b in s {
        if b == b'\n' { serial_putc(b'\r'); }
        serial_putc(b);
    }
}

#[inline(never)]
unsafe fn hex64(v: u64) {
    for i in (0..16).rev() {
        let nib = ((v >> (i*4)) & 0xF) as u8;
        serial_putc(if nib < 10 { b'0' + nib } else { b'A' + nib - 10 });
    }
    serial_putc(b'\n');
}

const EFI_SUCCESS:             EfiStatus = 0;
const _EFI_BUFFER_TOO_SMALL:    EfiStatus = 5;

const _OPEN_PROTOCOL_BY_HANDLE:      u32 = 1;
const OPEN_PROTOCOL_GET_PROTOCOL:   u32 = 2;

const ALLOCATE_ADDRESS:        u32 = 2;

const EFI_LOADER_DATA:         u32 = 2;

const KERNEL_PHYS:   u64 = 0x200000;
const BOOTINFO_BASE: u64 = 0x600000;
const BOOTINFO_SIZE: u32 = 0x1A00;

// ── GUID ───────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

// GOP GUID: A531EB5E-38B9-4BCD-ADEC-4AB08A8F9E16
const GOP_GUID: Guid = Guid {
    data1: 0xA531EB5E, data2: 0x38B9, data3: 0x4BCD,
    data4: [0xAD, 0xEC, 0x4A, 0xB0, 0x8A, 0x8F, 0x9E, 0x16],
};

// FS GUID: 0964E5B2-6459-11D2-8E39-00A0C969723B
const FS_GUID: Guid = Guid {
    data1: 0x0964E5B2, data2: 0x6459, data3: 0x11D2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

// FILE INFO GUID: 0x9576E92, 0x6D3F, 0x11D2, ...
const FILE_INFO_GUID: Guid = Guid {
    data1: 0x09576E92, data2: 0x6D3F, data3: 0x11D2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

// ACPI 2.0 GUID: 8868E871-E4F1-11D3-BC22-0080C73C8881
const ACPI2_GUID: Guid = Guid {
    data1: 0x8868E871, data2: 0xE4F1, data3: 0x11D3,
    data4: [0xBC, 0x22, 0x00, 0x80, 0xC7, 0x3C, 0x88, 0x81],
};

// Loaded Image Protocol GUID: 5B1B31A1-9562-11D2-8E3F-00A0C969723B
const LIP_GUID: Guid = Guid {
    data1: 0x5B1B31A1, data2: 0x9562, data3: 0x11D2,
    data4: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

// ── Table header ───────────────────────────────────────────────────────────────

#[repr(C)]
struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

// ── Memory descriptor ──────────────────────────────────────────────────────────

#[repr(C)]
struct EfiMemoryDescriptor {
    mem_type: u32,
    physical_start: u64,
    virtual_start: u64,
    num_pages: u64,
    attribute: u64,
}

// ── System table ───────────────────────────────────────────────────────────────

#[repr(C)]
struct EfiConfigurationTable {
    vendor_guid: Guid,
    vendor_table: *mut c_void,
}

#[repr(C)]
pub struct EfiSystemTable {
    hdr: EfiTableHeader,
    firmware_vendor: *mut u16,
    firmware_revision: u32,
    console_in_handle: EfiHandle,
    con_in: *mut c_void,
    console_out_handle: EfiHandle,
    con_out: *mut c_void,
    standard_error_handle: EfiHandle,
    std_err: *mut c_void,
    runtime_services: *mut c_void,
    boot_services: *mut c_void,
    number_of_table_entries: usize,
    configuration_table: *mut EfiConfigurationTable,
}

// ── Boot services (partial — solo lo que usamos) ───────────────────────────────

type FnAllocPages = unsafe extern "efiapi" fn(u32, u32, usize, *mut u64) -> EfiStatus;
#[allow(unused)]
type _FnFreePages  = unsafe extern "efiapi" fn(u64, usize) -> EfiStatus;
type FnGetMemMap  = unsafe extern "efiapi" fn(*mut usize, *mut EfiMemoryDescriptor, *mut usize, *mut usize, *mut u32) -> EfiStatus;
type FnAllocPool  = unsafe extern "efiapi" fn(u32, usize, *mut *mut c_void) -> EfiStatus;
type FnFreePool   = unsafe extern "efiapi" fn(*mut c_void) -> EfiStatus;
type FnExitBS     = unsafe extern "efiapi" fn(EfiHandle, usize) -> EfiStatus;
type _FnLocProto   = unsafe extern "efiapi" fn(*const Guid, *mut c_void, *mut *mut c_void) -> EfiStatus;
type FnOpenProto  = unsafe extern "efiapi" fn(EfiHandle, *const Guid, *mut *mut c_void, EfiHandle, EfiHandle, u32) -> EfiStatus;
type FnLocateHandleBuffer = unsafe extern "efiapi" fn(u32, *const Guid, *mut c_void, *mut usize, *mut *mut EfiHandle) -> EfiStatus;

// ── Block I/O Protocol ───────────────────────────────────────────────────────────

#[repr(C)]
struct BlockIoMedia {
    media_id: u32,
    removable_media: u8,
    media_present: u8,
    logical_partition: u8,
    read_only: u8,
    write_caching: u8,
    _pad: [u8; 3],
    block_size: u32,
    io_align: u32,
    last_block: u64,
    lowest_aligned_lba: u64,
    logical_blocks_per_physical_block: u32,
    optimal_transfer_granularity: u32,
}

type FnBlkReset = unsafe extern "efiapi" fn(*mut c_void, u8) -> EfiStatus;
type FnBlkRead  = unsafe extern "efiapi" fn(*mut c_void, u32, u64, usize, *mut c_void) -> EfiStatus;
type FnBlkWrite = unsafe extern "efiapi" fn(*mut c_void, u32, u64, usize, *mut c_void) -> EfiStatus;
type FnBlkFlush = unsafe extern "efiapi" fn(*mut c_void) -> EfiStatus;

#[repr(C)]
struct BlockIo {
    revision: u64,
    media: *mut BlockIoMedia,
    reset: FnBlkReset,
    read_blocks: FnBlkRead,
    write_blocks: FnBlkWrite,
    flush_blocks: FnBlkFlush,
}

// ── GPT structures ──────────────────────────────────────────────────────────────

#[repr(C, packed)]
#[allow(unused)]
struct _GptHeader {
    signature: [u8; 8],
    revision: u32,
    header_size: u32,
    crc32: u32,
    _reserved: u32,
    my_lba: u64,
    alternate_lba: u64,
    first_usable_lba: u64,
    last_usable_lba: u64,
    disk_guid: Guid,
    partition_entry_lba: u64,
    num_partition_entries: u32,
    sizeof_partition_entry: u32,
    partition_entries_crc32: u32,
}

#[repr(C, packed)]
#[allow(unused)]
struct _GptEntry {
    partition_type_guid: Guid,
    unique_partition_guid: Guid,
    starting_lba: u64,
    ending_lba: u64,
    attributes: u64,
    name: [u16; 36],
}

// ── FAT32 structures ────────────────────────────────────────────────────────────

#[repr(C, packed)]
#[repr(C, packed)]
struct FatBpb {
    jump_ins: [u8; 3],
    oem_name: [u8; 8],
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sector_count: u16,
    num_fats: u8,
    root_entry_count: u16,
    total_sectors_16: u16,
    media_descriptor: u8,
    sectors_per_fat_16: u16,
    sectors_per_track: u16,
    num_heads: u16,
    hidden_sectors: u32,
    total_sectors_32: u32,
    // FAT32 specific
    sectors_per_fat_32: u32,
    ext_flags: u16,
    fs_version: u16,
    root_cluster: u32,
    fs_info_sector: u16,
    backup_boot_sector: u16,
    _reserved: [u8; 12],
    drive_number: u8,
    _reserved1: u8,
    boot_sig: u8,
    volume_id: u32,
    volume_label: [u8; 11],
    fs_type: [u8; 8],
}

// ── Loaded Image Protocol ──────────────────────────────────────────────────────

#[repr(C)]
struct LoadedImage {
    revision: u32,
    _pad: u32,
    parent_handle: EfiHandle,
    system_table: *mut c_void,
    device_handle: EfiHandle,
}

// ── Portix BootInfo constants ────────────────────────────────────────────────

const BI_MAGIC_LO: u32 = 0x424F4F54;
const BI_MAGIC_HI: u32 = 0x50525458;
const BI_HDR_SIZE: u32 = 0xE0;
const BI_TOTAL_SIZE: u32 = 0x1A00;
const BI_MMAP_OFF: u32 = 0x100;
const BI_MMAP_ESIZE: u32 = 48;
const BI_MMAP_MAX: u32 = 128;
const BI_RANGES_OFF: u32 = 0x1900;
const BI_RANGE_ESIZE: u32 = 32;
const BI_FW_OFF: u32 = 0x1A00;
const BI_FW_ESIZE: u32 = 24;

const FLAG_FB_VALID: u64 = 1;
const FLAG_MEM_VALID: u64 = 2;

const MEM_USABLE_MAPPED: u32 = 1;
const MEM_USABLE: u32 = 2;
const MEM_RESERVED: u32 = 3;
const MEM_ACPI_RECLAIM: u32 = 4;
const MEM_ACPI_NVS: u32 = 5;
const MEM_FRAMEBUFFER: u32 = 7;
const MEM_KERNEL: u32 = 8;
const MEM_LOADER_DATA: u32 = 12;

const OWNER_FIRMWARE: u32 = 1;
const OWNER_LOADER: u32 = 2;
const OWNER_KERNEL: u32 = 3;

const RECLAIM_NEVER: u32 = 0;
const RECLAIM_AFTER_KERNEL_INIT: u32 = 1;

const CACHE_WB: u32 = 4;
const CACHE_UC: u32 = 1;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn bs_field(bs: *mut c_void, off: usize) -> usize {
    unsafe { core::ptr::read_unaligned((bs as *const u8).add(off) as *const usize) }
}

unsafe fn bs_call<T>(bs: *mut c_void, off: usize) -> T {
    let addr = bs_field(bs, off);
    core::mem::transmute_copy::<usize, T>(&addr)
}

unsafe fn zero_mem(ptr: *mut u8, len: usize) {
    for i in 0..len { core::ptr::write_volatile(ptr.add(i), 0); }
}

// ── Main ──────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "efiapi" fn efi_main(image: EfiHandle, st: *mut EfiSystemTable) -> usize {
    let bs = unsafe { (*st).boot_services };
    let mut fb_base: u64 = 0;
    let mut fb_size: u64 = 0;
    let mut fb_width: u32 = 0;
    let mut fb_height: u32 = 0;
    let mut fb_pitch: u32 = 0;
    let mut fb_bpp: u32 = 32;
    let mut fb_fmt: u32 = 1; // XRGB8888
    let mut fb_ok = false;

    // ── GOP: detectar framebuffer via UEFI Graphics Output Protocol ──
    // Estrategia: LocateHandleBuffer encuentra TODOS los handles GOP,
    //             fallback a ConsoleOutHandle si no se encuentra ninguno.
    let locate_hb_gop: FnLocateHandleBuffer = bs_call(bs, 312);
    let open_proto_gop: FnOpenProto = bs_call(bs, 280);
    let free_pool_gop: FnFreePool = bs_call(bs, 72);

    let mut candidates: [EfiHandle; 8] = [core::ptr::null_mut(); 8];
    let mut n_candidates = 0usize;

    let mut nh = 0usize;
    let mut buf: *mut EfiHandle = core::ptr::null_mut();
    let lhb_st = locate_hb_gop(2, &GOP_GUID as *const Guid, core::ptr::null_mut(), &mut nh, &mut buf);
    if lhb_st == EFI_SUCCESS && nh > 0 && !buf.is_null() {
        for i in 0..nh.min(8) {
            candidates[n_candidates] = unsafe { *buf.add(i) };
            n_candidates += 1;
        }
        free_pool_gop(buf as *mut c_void);
    }
    if n_candidates == 0 {
        let con_out_handle: EfiHandle = unsafe {
            core::ptr::read_unaligned((st as *const u8).add(56) as *const EfiHandle)
        };
        if !con_out_handle.is_null() {
            candidates[0] = con_out_handle;
            n_candidates = 1;
        }
    }

    'gop_loop: for i in 0..n_candidates {
        let mut gop_iface: *mut c_void = core::ptr::null_mut();
        if open_proto_gop(candidates[i], &GOP_GUID as *const Guid, &mut gop_iface,
            image, core::ptr::null_mut(), 0x02) == EFI_SUCCESS && !gop_iface.is_null()
        {
            let mode_ptr = unsafe {
                core::ptr::read_unaligned((gop_iface as *const u8).add(24) as *const *mut u8)
            };
            if !mode_ptr.is_null() {
                fb_base = unsafe { core::ptr::read_unaligned(mode_ptr.add(24) as *const u64) };
                fb_size = unsafe { core::ptr::read_unaligned(mode_ptr.add(32) as *const u64) };
                let info_ptr = unsafe {
                    core::ptr::read_unaligned(mode_ptr.add(8) as *const *mut u8)
                };
                if !info_ptr.is_null() {
                    fb_width = unsafe { core::ptr::read_unaligned(info_ptr.add(4) as *const u32) };
                    fb_height = unsafe { core::ptr::read_unaligned(info_ptr.add(8) as *const u32) };
                    let ppsl = unsafe { core::ptr::read_unaligned(info_ptr.add(16) as *const u32) };
                    fb_pitch = ppsl * 4;
                    let pix_fmt = unsafe { core::ptr::read_unaligned(info_ptr.add(12) as *const u32) };
                    fb_fmt = match pix_fmt {
                        0 => 1u32,
                        1 => 3u32,
                        _ => 1u32,
                    };
                    fb_bpp = 32;
                    if fb_width > 0 && fb_height > 0 && fb_base > 0 {
                        fb_ok = true;
                        break 'gop_loop;
                    }
                }
            }
        }
    }
    unsafe { serial_puts(b"EFI:fb ");
    serial_putc(if fb_ok { b'Y' } else { b'N' }); serial_putc(b'\n'); }

    // Debug: signal entry + boot services signature check
    unsafe {
        serial_puts(b"EFI:entry\n");

        // Read the signature at bs+0
        let sig = core::ptr::read_unaligned((bs as *const u8).add(0) as *const u64);
        serial_puts(b"SIG=");
        hex64(sig);

        // Test GetNextMonotonicCount at offset 240 (index 27)
        let fn_wd_off: usize = core::ptr::read_unaligned((bs as *const u8).add(240) as *const usize);
        type FnGetCount = unsafe extern "efiapi" fn(*mut u64) -> EfiStatus;
        let get_count: FnGetCount = core::mem::transmute_copy(&fn_wd_off);
        let mut count_val: u64 = 0;
        let count_st = get_count(&mut count_val);
        serial_puts(b"CNT=");
        serial_putc(if count_st == 0 { b'Y' } else { b'0' + (count_st % 10) as u8 });
        serial_putc(b'\n');

        // Test Stall at offset 248 (index 28)
        let fn_stall: usize = core::ptr::read_unaligned((bs as *const u8).add(248) as *const usize);
        type FnStall = unsafe extern "efiapi" fn(usize) -> EfiStatus;
        let stall_fn: FnStall = core::mem::transmute_copy(&fn_stall);
        let stall_st = stall_fn(1); // 1 microsecond
        serial_puts(b"STL=");
        serial_putc(if stall_st == 0 { b'Y' } else { b'0' + (stall_st % 10) as u8 });
        serial_putc(b'\n');

        // Enhanced debug: test all LocateProtocol + OpenProtocol combos with exact codes
        let fn_lp: usize = core::ptr::read_unaligned((bs as *const u8).add(320) as *const usize);
        type FnLocP = unsafe extern "efiapi" fn(*const Guid, *mut c_void, *mut *mut c_void) -> EfiStatus;
        let lp_fn: FnLocP = core::mem::transmute_copy(&fn_lp);

        // LocateProtocol with multiple GUIDs
        let test_guids: &[(&[u8], &Guid)] = &[
            (b"LIP", &LIP_GUID),
            (b"FS_", &FS_GUID),
            (b"GOP", &GOP_GUID),
            (b"ACPI", &ACPI2_GUID),
            (b"FIN", &FILE_INFO_GUID),
        ];
        for (name, guid) in test_guids {
            let mut out: *mut c_void = core::ptr::null_mut();
            let st = lp_fn(*guid as *const Guid, core::ptr::null_mut(), &mut out);
            serial_puts(name);
            serial_putc(b'=');
            let v = st as u32;
            serial_putc(b'0' + ((v / 10) % 10) as u8);
            serial_putc(b'0' + (v % 10) as u8);
            serial_putc(b' ');
            serial_putc(if st == 0 { b'Y' } else { b'N' });
            serial_putc(b'\n');
        }

        // OpenProtocol with LIP_GUID (should work), then FS_GUID on device handle
        let fn_op: usize = core::ptr::read_unaligned((bs as *const u8).add(280) as *const usize);
        type FnOpP = unsafe extern "efiapi" fn(EfiHandle, *const Guid, *mut *mut c_void, EfiHandle, EfiHandle, u32) -> EfiStatus;
        let op_fn: FnOpP = core::mem::transmute_copy(&fn_op);
        let mut lip_buf: *mut c_void = core::ptr::null_mut();
        let op_li = op_fn(image, &LIP_GUID as *const Guid, &mut lip_buf, image, core::ptr::null_mut(), OPEN_PROTOCOL_GET_PROTOCOL);
        serial_puts(b"OP_LI=");
        let v = op_li as u32;
        serial_putc(b'0' + ((v / 10) % 10) as u8);
        serial_putc(b'0' + (v % 10) as u8);
        serial_putc(b'\n');
        if op_li == EFI_SUCCESS && !lip_buf.is_null() {
            let dev_h = (*(lip_buf as *mut LoadedImage)).device_handle;
            let mut fs_buf: *mut c_void = core::ptr::null_mut();
            let op_fs = op_fn(dev_h, &FS_GUID as *const Guid, &mut fs_buf, image, core::ptr::null_mut(), OPEN_PROTOCOL_GET_PROTOCOL);
            serial_puts(b"OP_FS=");
            let v = op_fs as u32;
            serial_putc(b'0' + ((v / 10) % 10) as u8);
            serial_putc(b'0' + (v % 10) as u8);
            serial_putc(b'\n');
        }

        // Try HandleProtocol (deprecated boot service at offset 152)
        let fn_hp: usize = core::ptr::read_unaligned((bs as *const u8).add(152) as *const usize);
        type FnHndlP = unsafe extern "efiapi" fn(EfiHandle, *const Guid, *mut *mut c_void) -> EfiStatus;
        let hp_fn: FnHndlP = core::mem::transmute_copy(&fn_hp);
        let mut hp_buf: *mut c_void = core::ptr::null_mut();
        let hp_st = hp_fn(image, &LIP_GUID as *const Guid, &mut hp_buf);
        serial_puts(b"HP_LI=");
        let v = hp_st as u32;
        serial_putc(b'0' + ((v / 10) % 10) as u8);
        serial_putc(b'0' + (v % 10) as u8);
        serial_putc(b'\n');

        // Use LocateDevicePath to find partition handle from our image's device path
        // First get DevicePath from device handle via BlockIO to read raw kernel
        const BLKIO_GUID: Guid = Guid {
            data1: 0x964E5B21, data2: 0x6459, data3: 0x11D2,
            data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
        };
        // Open BlockIO on the device handle to read raw sectors
        if op_li == EFI_SUCCESS && !lip_buf.is_null() {
            let dev_h = (*(lip_buf as *mut LoadedImage)).device_handle;
            let fn_op2v: usize = core::ptr::read_unaligned((bs as *const u8).add(280) as *const usize);
            type FnOp2 = unsafe extern "efiapi" fn(EfiHandle, *const Guid, *mut *mut c_void, EfiHandle, EfiHandle, u32) -> EfiStatus;
            let op2_fn: FnOp2 = core::mem::transmute_copy(&fn_op2v);
            let mut bio: *mut c_void = core::ptr::null_mut();
            let bio_st = op2_fn(dev_h, &BLKIO_GUID as *const Guid, &mut bio, image, core::ptr::null_mut(), OPEN_PROTOCOL_GET_PROTOCOL);
            serial_puts(b"BIO=");
            let v = bio_st as u32;
            serial_putc(b'0' + ((v / 10) % 10) as u8);
            serial_putc(b'0' + (v % 10) as u8);
            serial_putc(b'\n');
            // Block IO should work on the disk handle even without FS
        }

        // Print bs pointer
        serial_puts(b"BS=");
        hex64(bs as u64);
    }

    // ── 1. Get LoadedImage + Block I/O ──────────────────────────────────
    const BLKIO_GUID: Guid = Guid {
        data1: 0x964E5B21, data2: 0x6459, data3: 0x11D2,
        data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
    };
    let open_proto: FnOpenProto = bs_call(bs, 280);
    let allocate_pool: FnAllocPool = bs_call(bs, 64);
    let free_pool: FnFreePool = bs_call(bs, 72);
    let allocate_pages: FnAllocPages = bs_call(bs, 40);
    let mut lip_buf: *mut c_void = core::ptr::null_mut();
    let lip_ok = !image.is_null()
        && open_proto(image, &LIP_GUID as *const Guid, &mut lip_buf, image, core::ptr::null_mut(), OPEN_PROTOCOL_GET_PROTOCOL) == EFI_SUCCESS
        && !lip_buf.is_null();
    let mut bio_io: *mut BlockIo = core::ptr::null_mut();
    if lip_ok {
        let dev_h = unsafe { (*(lip_buf as *mut LoadedImage)).device_handle };
        let mut bio_tmp: *mut c_void = core::ptr::null_mut();
        if open_proto(dev_h, &BLKIO_GUID as *const Guid, &mut bio_tmp, image, core::ptr::null_mut(), OPEN_PROTOCOL_GET_PROTOCOL) == EFI_SUCCESS {
            bio_io = bio_tmp as *mut BlockIo;
        }
    }
    unsafe { serial_puts(b"EFI:bio ");
    let c = if bio_io.is_null() { b'N' } else { b'Y' };
    serial_putc(c); serial_putc(b'\n'); }

    // ── 2. Read kernel via Block I/O + raw FAT32 ────────────────────────
    // Note: device_handle is the PARTITION handle, so Block I/O LBA is partition-relative.
    // LBA 0 = partition start = FAT32 VBR.
    let mut kernel_data: *mut u8 = core::ptr::null_mut();
    let mut kernel_size: usize = 0;

    if !bio_io.is_null() {
        let bio = bio_io;
        let rb = unsafe { (*bio).read_blocks };
        let mid = unsafe { (*(*bio).media).media_id };
        let bsiz = unsafe { (*(*bio).media).block_size } as usize;

        macro_rules! rd1 { ($lba:expr, $b:expr) => { rb(bio as *mut c_void, mid, $lba, bsiz, $b.as_mut_ptr() as *mut c_void) == EFI_SUCCESS } }
        macro_rules! rdn { ($lba:expr, $n:expr, $b:expr) => { rb(bio as *mut c_void, mid, $lba, $n * bsiz, $b.as_mut_ptr() as *mut c_void) == EFI_SUCCESS } }

        // FAT32 VBR at partition-relative LBA 0
        let mut vbr: [u8; 512] = [0u8; 512];
        if rd1!(0, vbr) {
            let bpb = unsafe { &*(vbr.as_ptr() as *const FatBpb) };
            if bpb.bytes_per_sector as usize == bsiz && bpb.sectors_per_fat_32 > 0 {
                let spc = bpb.sectors_per_cluster as usize;
                let fat_lba = bpb.reserved_sector_count as u64;
                let data_lba = fat_lba + (bpb.num_fats as u64) * (bpb.sectors_per_fat_32 as u64);
                let csize = spc * bsiz;

                let mut cbuf: *mut c_void = core::ptr::null_mut();
                if allocate_pool(EFI_LOADER_DATA, csize, &mut cbuf) == EFI_SUCCESS && !cbuf.is_null() {
                    macro_rules! rdcl { ($cl:expr) => { rdn!(data_lba + ($cl as u64 - 2) * spc as u64, spc, core::slice::from_raw_parts_mut(cbuf as *mut u8, csize)) } }
                    macro_rules! gf { ($cl:expr) => {{
                        let off = $cl as usize * 4;
                        let sf = off / bsiz;
                        let mut fs = [0u8; 512];
                        if rd1!(fat_lba + sf as u64, fs) {
                            let bo = off % bsiz;
                            u32::from_le_bytes([fs[bo], fs[bo+1], fs[bo+2], fs[bo+3]]) & 0x0FFFFFFF
                        } else { 0x0FFFFFFF }
                    }}}

                    // Find PORTIX directory in root
                    let mut portix_cl = 0u32;
                    let mut pc = bpb.root_cluster;
                    loop {
                        if !rdcl!(pc) { break; }
                        let dirs = unsafe { core::slice::from_raw_parts(cbuf as *const u8, csize) };
                        for i in (0..csize).step_by(32) {
                            let fb = dirs[i];
                            if fb == 0x00 { pc = 0; break; }
                            if fb == 0xE5 { continue; }
                            if dirs[i+11] == 0x0F { continue; }
                            let sn = &dirs[i..i+11];
                            if sn[0] == b'P' && sn[1] == b'O' && sn[2] == b'R' && sn[3] == b'T' && sn[4] == b'I' && sn[5] == b'X'
                                && sn[6] == 0x20 && sn[7] == 0x20 && sn[8] == 0x20 && sn[9] == 0x20 && sn[10] == 0x20
                            {
                                portix_cl = u32::from_le_bytes([dirs[i+26], dirs[i+27], dirs[i+20], dirs[i+21]]);
                                break;
                            }
                        }
                        if portix_cl != 0 { break; }
                        if pc == 0 { break; }
                        let n = gf!(pc);
                        if n < 2 || n >= 0x0FFFFFF8 { break; }
                        pc = n;
                    }

                    // Find KERNEL.BIN in PORTIX
                    let mut kcl = 0u32;
                    let mut ksz = 0u32;
                    if portix_cl != 0 {
                        let mut fc = portix_cl;
                        loop {
                            if !rdcl!(fc) { break; }
                            let dirs = unsafe { core::slice::from_raw_parts(cbuf as *const u8, csize) };
                            for i in (0..csize).step_by(32) {
                                let fb = dirs[i];
                                if fb == 0x00 { fc = 0; break; }
                                if fb == 0xE5 { continue; }
                                if dirs[i+11] == 0x0F { continue; }
                                let sn = &dirs[i..i+11];
                                if sn[0] == b'K' && sn[1] == b'E' && sn[2] == b'R' && sn[3] == b'N' && sn[4] == b'E' && sn[5] == b'L'
                                    && sn[6] == 0x20 && sn[7] == 0x20
                                    && sn[8] == b'B' && sn[9] == b'I' && sn[10] == b'N'
                                {
                                    kcl = u32::from_le_bytes([dirs[i+26], dirs[i+27], dirs[i+20], dirs[i+21]]);
                                    ksz = u32::from_le_bytes([dirs[i+28], dirs[i+29], dirs[i+30], dirs[i+31]]);
                                    break;
                                }
                            }
                            if kcl != 0 { break; }
                            if fc == 0 { break; }
                            let n = gf!(fc);
                            if n < 2 || n >= 0x0FFFFFF8 { break; }
                            fc = n;
                        }
                    }

                    free_pool(cbuf);

                    if kcl != 0 && ksz > 0 && ksz < 4 * 1024 * 1024 {
                        kernel_size = ksz as usize;
                        let kpages = (kernel_size + 4095) / 4096;
                        let mut kphys = KERNEL_PHYS;
                        if allocate_pages(ALLOCATE_ADDRESS, EFI_LOADER_DATA, kpages, &mut kphys) == EFI_SUCCESS && kphys == KERNEL_PHYS {
                            let mut dst = kphys as *mut u8;
                            let mut cl = kcl;
                            loop {
                                let _ = rdn!(data_lba + (cl as u64 - 2) * spc as u64, spc, core::slice::from_raw_parts_mut(dst, csize));
                                dst = unsafe { dst.add(csize) };
                                let n = gf!(cl);
                                if n < 2 || n >= 0x0FFFFFF8 { break; }
                                cl = n;
                            }
                            kernel_data = kphys as *mut u8;
                        }
                    }
                }
            }
        }
    }

    unsafe { serial_puts(b"EFI:kernel "); serial_putc(if kernel_data.is_null() { b'0' } else { b'1' }); serial_putc(b'\n'); }

    // ── Fallback: if we couldn't load the kernel, return error ────────────
    if kernel_data.is_null() || kernel_size == 0 {
        return 1;
    }

    // ── 4. Allocate/find BootInfo memory at 0x600000 ─────────────────────
    let pages_info = (BOOTINFO_SIZE as usize + 4095) / 4096;
    let mut bi_phys: u64 = BOOTINFO_BASE;
    let bi_status = allocate_pages(ALLOCATE_ADDRESS, EFI_LOADER_DATA, pages_info, &mut bi_phys);
    if bi_status != EFI_SUCCESS {
        return 2;
    }

    // ── 5. Build PortixBootInfo at 0x600000 ──────────────────────────────
    let bi = BOOTINFO_BASE as *mut u8;
    zero_mem(bi, BOOTINFO_SIZE as usize);

    // Magic
    core::ptr::write_unaligned(bi.add(0x00) as *mut u32, BI_MAGIC_LO);
    core::ptr::write_unaligned(bi.add(0x04) as *mut u32, BI_MAGIC_HI);
    // Version / arch / endian
    core::ptr::write_unaligned(bi.add(0x08) as *mut u32, 1); // abi_version
    core::ptr::write_unaligned(bi.add(0x0C) as *mut u32, 1); // arch=x86_64
    core::ptr::write_unaligned(bi.add(0x10) as *mut u32, 1); // endian=little
    // Header size / total size
    core::ptr::write_unaligned(bi.add(0x14) as *mut u32, BI_HDR_SIZE);
    core::ptr::write_unaligned(bi.add(0x18) as *mut u32, BI_TOTAL_SIZE);
    // Checksum (offset 0x1C) — calculated last

    // Flags
    let mut flags: u64 = FLAG_MEM_VALID;
    if fb_ok { flags |= FLAG_FB_VALID; }
    core::ptr::write_unaligned(bi.add(0x20) as *mut u64, flags);

    // Boot source / protocol
    core::ptr::write_unaligned(bi.add(0x40) as *mut u32, 2); // UEFI
    core::ptr::write_unaligned(bi.add(0x44) as *mut u32, 2); // UEFI_NATIVE

    // ── Framebuffer ──────────────────────────────────────────────────────
    if fb_ok {
        core::ptr::write_unaligned(bi.add(0x50) as *mut u64, fb_base as u64);
        core::ptr::write_unaligned(bi.add(0x58) as *mut u64, fb_size as u64);
        core::ptr::write_unaligned(bi.add(0x60) as *mut u32, fb_width);
        core::ptr::write_unaligned(bi.add(0x64) as *mut u32, fb_height);
        core::ptr::write_unaligned(bi.add(0x68) as *mut u32, fb_pitch);
        core::ptr::write_unaligned(bi.add(0x6C) as *mut u32, fb_bpp);
        core::ptr::write_unaligned(bi.add(0x70) as *mut u32, 2); // source=EFI_GOP
        core::ptr::write_unaligned(bi.add(0x74) as *mut u32, fb_fmt); // canonical format
        if fb_pitch > 0 && fb_bpp > 0 {
            let bpp_b = ((fb_bpp + 7) / 8).max(1);
            core::ptr::write_unaligned(bi.add(0x7C) as *mut u32, fb_pitch / bpp_b); // pixels_per_scanline
        }
        core::ptr::write_unaligned(bi.add(0x80) as *mut u32, 0); // cache_policy
    }

    // ── Kernel location ───────────────────────────────────────────────────
    core::ptr::write_unaligned(bi.add(0x98) as *mut u64, KERNEL_PHYS);
    core::ptr::write_unaligned(bi.add(0xA0) as *mut u64, kernel_size as u64);

    // ── Memory map header ─────────────────────────────────────────────────
    core::ptr::write_unaligned(bi.add(0xA8) as *mut u32, BI_MMAP_OFF);
    core::ptr::write_unaligned(bi.add(0xB0) as *mut u32, BI_MMAP_ESIZE);
    core::ptr::write_unaligned(bi.add(0xB4) as *mut u32, BI_RANGES_OFF);
    core::ptr::write_unaligned(bi.add(0xBC) as *mut u32, BI_RANGE_ESIZE);
    core::ptr::write_unaligned(bi.add(0xC0) as *mut u32, BI_FW_OFF);
    core::ptr::write_unaligned(bi.add(0xC8) as *mut u32, BI_FW_ESIZE);

    // ── Get UEFI memory map for BootInfo ─────────────────────────────────
    // First call: get required size
    let get_mem_map: FnGetMemMap = bs_call(bs, 56);
    let mut mmap_size: usize = 0;
    let mut mmap_key: usize = 0;
    let mut desc_size: usize = 0;
    let mut desc_ver: u32 = 0;
    let _ = get_mem_map(&mut mmap_size, core::ptr::null_mut(), &mut mmap_key, &mut desc_size, &mut desc_ver);

    if mmap_size > 0 && desc_size >= 32 {
        mmap_size += desc_size * 8; // extra buffer
        let allocate_pool2: FnAllocPool = bs_call(bs, 64);
        let mut mmap_buf: *mut c_void = core::ptr::null_mut();
        if allocate_pool2(EFI_LOADER_DATA, mmap_size, &mut mmap_buf) == EFI_SUCCESS && !mmap_buf.is_null() {
            let status = get_mem_map(&mut mmap_size, mmap_buf as *mut EfiMemoryDescriptor, &mut mmap_key, &mut desc_size, &mut desc_ver);
            if status == EFI_SUCCESS {
                let count = mmap_size / desc_size;
                // Convert UEFI memory map to Portix format
                let dst_off = BI_MMAP_OFF as usize;
                let dst_ptr = bi.add(dst_off) as *mut u8;
                let mut dst_idx: u32 = 0;

                // We'll write entries and track the count at offset 0xAC
                for i in 0..count.min(BI_MMAP_MAX as usize) {
                    let desc = (mmap_buf as *const u8).add(i * desc_size) as *const EfiMemoryDescriptor;
                    let desc_ref = unsafe { &*desc };
                    let base = desc_ref.physical_start;
                    let len = desc_ref.num_pages * 4096;
                    let portix_type = match desc_ref.mem_type {
                        1 | 4 | 5 | 6 | 7 => MEM_USABLE,    // loader/boot/conventional
                        2 => MEM_RESERVED,                   // ACPI reclaim -> treat as reserved for now
                        10 => MEM_ACPI_RECLAIM,
                        11 => MEM_ACPI_NVS,
                        13 | 14 => MEM_RESERVED,             // runtime services
                        17 => MEM_FRAMEBUFFER,
                        _ => MEM_RESERVED,
                    };

                    let mapped = if (base + len) <= 0x6000000 { MEM_USABLE_MAPPED } else { portix_type };
                    let actual_type = if mapped == MEM_USABLE_MAPPED { MEM_USABLE_MAPPED } else { portix_type };

                    // Skip zero-length entries
                    if len == 0 { continue; }

                    let entry_ptr = dst_ptr.add(dst_idx as usize * BI_MMAP_ESIZE as usize) as *mut u32;
                    core::ptr::write_unaligned(entry_ptr.add(0) as *mut u64, base);      // base
                    core::ptr::write_unaligned(entry_ptr.add(2) as *mut u64, len);       // length
                    core::ptr::write_unaligned(entry_ptr.add(4) as *mut u32, actual_type); // kind
                    core::ptr::write_unaligned(entry_ptr.add(5) as *mut u32, OWNER_FIRMWARE); // owner
                    core::ptr::write_unaligned(entry_ptr.add(6) as *mut u32, RECLAIM_NEVER); // reclaim
                    core::ptr::write_unaligned(entry_ptr.add(7) as *mut u32, if actual_type == MEM_USABLE_MAPPED || actual_type == MEM_USABLE { CACHE_WB } else { CACHE_UC });
                    dst_idx += 1;
                }

                core::ptr::write_unaligned(bi.add(0xAC) as *mut u32, dst_idx);
            }
            let free_pool2: FnFreePool = bs_call(bs, 72);
            let _ = free_pool2(mmap_buf);
        }
    }

    // ── Reserved ranges ──────────────────────────────────────────────────
    let ranges_off = BI_RANGES_OFF as usize;
    let rp = bi.add(ranges_off) as *mut u32;
    let mut rc = 0u32;

    // Range 0: Kernel
    let kernel_end = round_up(kernel_size as u64, 4096);
    write_range(rp.add(rc as usize * (BI_RANGE_ESIZE as usize / 4)), KERNEL_PHYS, kernel_end, MEM_KERNEL, OWNER_KERNEL, RECLAIM_NEVER, CACHE_WB);
    rc += 1;

    // Range 1: BootInfo
    write_range(rp.add(rc as usize * (BI_RANGE_ESIZE as usize / 4)), BOOTINFO_BASE, BOOTINFO_SIZE as u64, MEM_LOADER_DATA, OWNER_LOADER, RECLAIM_NEVER, CACHE_WB);
    rc += 1;

    // Range 2-3: EFI boot services code/data used by our loader
    write_range(rp.add(rc as usize * (BI_RANGE_ESIZE as usize / 4)), 0x100000, 0x2000, MEM_LOADER_DATA, OWNER_LOADER, RECLAIM_AFTER_KERNEL_INIT, CACHE_WB);
    rc += 1;

    write_range(rp.add(rc as usize * (BI_RANGE_ESIZE as usize / 4)), 0x700000, 0x100000, MEM_LOADER_DATA, OWNER_LOADER, RECLAIM_AFTER_KERNEL_INIT, CACHE_WB);
    rc += 1;

    core::ptr::write_unaligned(bi.add(0xB8) as *mut u32, rc);

    // ── Firmware tables: look for ACPI RSDP in config table ──────────────
    let configs = unsafe { (*st).configuration_table };
    let config_count = unsafe { (*st).number_of_table_entries };
    let fw_off = BI_FW_OFF as usize;
    let fp = bi.add(fw_off) as *mut u32;
    let mut fc = 0u32;

    for i in 0..config_count {
        let entry = unsafe { &*configs.add(i) };
        if entry.vendor_guid.data1 == ACPI2_GUID.data1
            && entry.vendor_guid.data2 == ACPI2_GUID.data2
            && entry.vendor_guid.data3 == ACPI2_GUID.data3
            && entry.vendor_guid.data4 == ACPI2_GUID.data4
        {
            core::ptr::write_unaligned(fp.add(0) as *mut u32, 1); // kind = ACPI_RSDP
            core::ptr::write_unaligned(fp.add(1) as *mut u32, 0); // flags
            core::ptr::write_unaligned(fp.add(2) as *mut u64, entry.vendor_table as u64); // address
            core::ptr::write_unaligned(fp.add(4) as *mut u64, 36); // length
            fc += 1;
        }
    }
    core::ptr::write_unaligned(bi.add(0xC4) as *mut u32, fc);

    // ── Calculate checksum ──────────────────────────────────────────────
    core::ptr::write_unaligned(bi.add(0x1C) as *mut u32, 0);
    let mut sum: u32 = 0;
    let total_words = BI_TOTAL_SIZE as usize / 4;
    for i in 0..total_words {
        sum = sum.wrapping_add(unsafe { core::ptr::read_unaligned(bi.add(i * 4) as *const u32) });
    }
    core::ptr::write_unaligned(bi.add(0x1C) as *mut u32, 0u32.wrapping_sub(sum) as u32);

    unsafe { serial_puts(b"EFI:exit_bs\n"); }

    // ── 6. Exit boot services ───────────────────────────────────────────
    let exit_bs: FnExitBS = bs_call(bs, 240);
    if exit_bs(image, mmap_key) != EFI_SUCCESS {
        // Retry: get key again
        let mut retry_key: usize = 0;
        let mut retry_size: usize = 0;
        let mut retry_ds: usize = 0;
        let mut retry_dv: u32 = 0;
        let _ = get_mem_map(&mut retry_size, core::ptr::null_mut(), &mut retry_key, &mut retry_ds, &mut retry_dv);
        retry_size += desc_size * 4;
        let mut rbuf: *mut c_void = core::ptr::null_mut();
        let alloc_s: FnAllocPool = bs_call(bs, 64);
        if alloc_s(EFI_LOADER_DATA, retry_size, &mut rbuf) == EFI_SUCCESS && !rbuf.is_null() {
            if get_mem_map(&mut retry_size, rbuf as *mut EfiMemoryDescriptor, &mut retry_key, &mut retry_ds, &mut retry_dv) == EFI_SUCCESS {
                if exit_bs(image, retry_key) != EFI_SUCCESS {
                    return 3;
                }
            }
        }
    }

    unsafe { serial_puts(b"EFI:jump_kernel\n"); }

    // ── 7. Jump to kernel ────────────────────────────────────────────────
    unsafe {
        core::arch::asm!(
            "cli",
            "cld",
            "mov rdi, {bi}",
            "mov rax, {kernel}",
            "jmp rax",
            bi = in(reg) BOOTINFO_BASE,
            kernel = in(reg) KERNEL_PHYS,
            options(noreturn)
        );
    }
}

#[inline(always)]
fn round_up(val: u64, align: u64) -> u64 {
    (val + align - 1) & !(align - 1)
}

fn write_range(ptr: *mut u32, base: u64, size: u64, kind: u32, owner: u32, reclaim: u32, cache: u32) {
    unsafe {
        core::ptr::write_unaligned(ptr.add(0) as *mut u64, base);
        core::ptr::write_unaligned(ptr.add(2) as *mut u64, size);
        core::ptr::write_unaligned(ptr.add(4) as *mut u32, kind);
        core::ptr::write_unaligned(ptr.add(5) as *mut u32, owner);
        core::ptr::write_unaligned(ptr.add(6) as *mut u32, reclaim);
        core::ptr::write_unaligned(ptr.add(7) as *mut u32, cache);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop { core::hint::spin_loop(); }
}
