// kernel/src/drivers/storage/fuse.rs — Subsistema FUSE (Filesystem in Userspace) para el Kernel
// Proporciona el protocolo de comunicación estructurada vía /dev/fuse,
// serialización de peticiones, colas de intercambio y soporte para sistemas de archivos de usuario.
#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::String;
use crate::arch::spinlock::Spinlock;

// ── Constantes y Opcodes del Protocolo FUSE (Linux ABI v7.31) ────────────────
pub const FUSE_KERNEL_VERSION: u32 = 7;
pub const FUSE_KERNEL_MINOR_VERSION: u32 = 31;

pub const FUSE_LOOKUP:   u32 = 1;
pub const FUSE_FORGET:   u32 = 2;
pub const FUSE_GETATTR:  u32 = 3;
pub const FUSE_SETATTR:  u32 = 4;
pub const FUSE_MKDIR:    u32 = 9;
pub const FUSE_UNLINK:   u32 = 10;
pub const FUSE_RMDIR:    u32 = 11;
pub const FUSE_RENAME:   u32 = 12;
pub const FUSE_LINK:     u32 = 13;
pub const FUSE_OPEN:     u32 = 14;
pub const FUSE_READ:     u32 = 15;
pub const FUSE_WRITE:    u32 = 16;
pub const FUSE_STATFS:   u32 = 17;
pub const FUSE_RELEASE:  u32 = 18;
pub const FUSE_FSYNC:    u32 = 20;
pub const FUSE_INIT:     u32 = 26;
pub const FUSE_OPENDIR:  u32 = 27;
pub const FUSE_READDIR:  u32 = 28;

pub const FUSE_ROOT_ID:  u64 = 1;

// ── Cabeceras de Paquetes FUSE ────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FuseInHeader {
    pub len: u32,
    pub opcode: u32,
    pub unique: u64,
    pub nodeid: u64,
    pub uid: u32,
    pub gid: u32,
    pub pid: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FuseOutHeader {
    pub len: u32,
    pub error: i32,
    pub unique: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FuseInitIn {
    pub major: u32,
    pub minor: u32,
    pub max_readahead: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FuseInitOut {
    pub major: u32,
    pub minor: u32,
    pub max_readahead: u32,
    pub flags: u32,
    pub max_background: u16,
    pub congestion_threshold: u16,
    pub max_write: u32,
    pub time_gran: u32,
    pub unused: [u32; 9],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FuseAttr {
    pub ino: u64,
    pub size: u64,
    pub blocks: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub atimensec: u32,
    pub mtimensec: u32,
    pub ctimensec: u32,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u32,
    pub blksize: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FuseEntryOut {
    pub nodeid: u64,
    pub generation: u64,
    pub entry_valid: u64,
    pub attr_valid: u64,
    pub entry_valid_nsec: u32,
    pub attr_valid_nsec: u32,
    pub attr: FuseAttr,
}

// ── Canal FUSE (/dev/fuse) ───────────────────────────────────────────────────

pub struct FuseChannel {
    pub is_registered: bool,
    pub is_mounted: bool,
    pub mount_point: String,
    pub next_unique: u64,
    pub pending_requests: Vec<Vec<u8>>,
    pub completed_responses: Vec<Vec<u8>>,
    pub total_ops_dispatched: u64,
}

impl FuseChannel {
    pub const fn new() -> Self {
        Self {
            is_registered: true,
            is_mounted: false,
            mount_point: String::new(),
            next_unique: 1,
            pending_requests: Vec::new(),
            completed_responses: Vec::new(),
            total_ops_dispatched: 0,
        }
    }

    pub fn alloc_unique(&mut self) -> u64 {
        let u = self.next_unique;
        self.next_unique = self.next_unique.wrapping_add(1);
        u
    }

    pub fn mount(&mut self, target: &str) -> bool {
        self.mount_point = String::from(target);
        self.is_mounted = true;
        true
    }

    pub fn unmount(&mut self) {
        self.mount_point.clear();
        self.is_mounted = false;
        self.pending_requests.clear();
        self.completed_responses.clear();
    }

    pub fn send_request(&mut self, opcode: u32, nodeid: u64, payload: &[u8]) -> u64 {
        let unique = self.alloc_unique();
        let total_len = (core::mem::size_of::<FuseInHeader>() + payload.len()) as u32;
        let hdr = FuseInHeader {
            len: total_len,
            opcode,
            unique,
            nodeid,
            uid: 0,
            gid: 0,
            pid: 1,
            padding: 0,
        };
        let mut buf = Vec::with_capacity(total_len as usize);
        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(&hdr as *const FuseInHeader as *const u8, core::mem::size_of::<FuseInHeader>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(payload);
        self.pending_requests.push(buf);
        self.total_ops_dispatched += 1;
        unique
    }

    pub fn receive_response(&mut self, unique: u64, error: i32, payload: &[u8]) {
        let total_len = (core::mem::size_of::<FuseOutHeader>() + payload.len()) as u32;
        let hdr = FuseOutHeader {
            len: total_len,
            error,
            unique,
        };
        let mut buf = Vec::with_capacity(total_len as usize);
        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(&hdr as *const FuseOutHeader as *const u8, core::mem::size_of::<FuseOutHeader>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(payload);
        self.completed_responses.push(buf);
    }
}

pub static FUSE_CHANNEL: Spinlock<FuseChannel> = Spinlock::new(FuseChannel::new());

/// Auto-test del subsistema FUSE
pub fn run_fuse_self_test() -> bool {
    let mut ch = FUSE_CHANNEL.lock();
    if !ch.is_registered {
        return false;
    }

    // 1. Simular montaje en /mnt/fuse
    ch.mount("/mnt/fuse");
    if !ch.is_mounted || ch.mount_point != "/mnt/fuse" {
        return false;
    }

    // 2. Simular handshake FUSE_INIT
    let init_in = FuseInitIn {
        major: FUSE_KERNEL_VERSION,
        minor: FUSE_KERNEL_MINOR_VERSION,
        max_readahead: 131072,
        flags: 0x3F,
    };
    let init_bytes = unsafe {
        core::slice::from_raw_parts(&init_in as *const FuseInitIn as *const u8, core::mem::size_of::<FuseInitIn>())
    };
    let req_id = ch.send_request(FUSE_INIT, FUSE_ROOT_ID, init_bytes);

    let init_out = FuseInitOut {
        major: FUSE_KERNEL_VERSION,
        minor: FUSE_KERNEL_MINOR_VERSION,
        max_readahead: 131072,
        flags: 0x3F,
        max_background: 16,
        congestion_threshold: 12,
        max_write: 131072,
        time_gran: 1,
        unused: [0; 9],
    };
    let out_bytes = unsafe {
        core::slice::from_raw_parts(&init_out as *const FuseInitOut as *const u8, core::mem::size_of::<FuseInitOut>())
    };
    ch.receive_response(req_id, 0, out_bytes);

    if ch.total_ops_dispatched < 1 || ch.completed_responses.is_empty() {
        return false;
    }

    // 3. Simular FUSE_LOOKUP de archivo virtual "readme.txt"
    let lookup_id = ch.send_request(FUSE_LOOKUP, FUSE_ROOT_ID, b"readme.txt\0");
    let attr = FuseAttr {
        ino: 2,
        size: 1024,
        blocks: 2,
        atime: 1700000000,
        mtime: 1700000000,
        ctime: 1700000000,
        atimensec: 0,
        mtimensec: 0,
        ctimensec: 0,
        mode: 0o100644,
        nlink: 1,
        uid: 1000,
        gid: 1000,
        rdev: 0,
        blksize: 4096,
        padding: 0,
    };
    let entry_out = FuseEntryOut {
        nodeid: 2,
        generation: 1,
        entry_valid: 1,
        attr_valid: 1,
        entry_valid_nsec: 0,
        attr_valid_nsec: 0,
        attr,
    };
    let entry_bytes = unsafe {
        core::slice::from_raw_parts(&entry_out as *const FuseEntryOut as *const u8, core::mem::size_of::<FuseEntryOut>())
    };
    ch.receive_response(lookup_id, 0, entry_bytes);

    true
}
