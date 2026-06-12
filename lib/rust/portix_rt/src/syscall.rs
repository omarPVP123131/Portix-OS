use core::arch::asm;

pub const SYS_EXIT:   u64 = 0;
pub const SYS_WRITE:  u64 = 1;
pub const SYS_GETPID: u64 = 2;
pub const SYS_YIELD:  u64 = 3;
pub const SYS_SLEEP:  u64 = 4;
pub const SYS_READ:   u64 = 5;
pub const SYS_OPEN:   u64 = 6;
pub const SYS_CLOSE:  u64 = 7;
pub const SYS_BRK:    u64 = 8;
pub const SYS_MMAP:   u64 = 9;
pub const SYS_GETDIRENTS: u64 = 10;
pub const SYS_EXECVE: u64 = 11;
pub const SYS_DUP2:   u64 = 12;
pub const SYS_UPTIME: u64 = 13;

macro_rules! syscall {
    ($nr:expr) => {{
        let r: u64;
        asm!("int $0x80", in("rax") $nr, out("rax") r, lateout("rcx") _, lateout("r11") _, options(nostack));
        r
    }};
    ($nr:expr, $a1:expr) => {{
        let r: u64;
        asm!("int $0x80", in("rax") $nr, in("rdi") $a1, out("rax") r, lateout("rcx") _, lateout("r11") _, options(nostack));
        r
    }};
    ($nr:expr, $a1:expr, $a2:expr) => {{
        let r: u64;
        asm!("int $0x80", in("rax") $nr, in("rdi") $a1, in("rsi") $a2, out("rax") r, lateout("rcx") _, lateout("r11") _, options(nostack));
        r
    }};
    ($nr:expr, $a1:expr, $a2:expr, $a3:expr) => {{
        let r: u64;
        asm!("int $0x80", in("rax") $nr, in("rdi") $a1, in("rsi") $a2, in("rdx") $a3, out("rax") r, lateout("rcx") _, lateout("r11") _, options(nostack));
        r
    }};
    ($nr:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr) => {{
        let r: u64;
        asm!("int $0x80", in("rax") $nr, in("rdi") $a1, in("rsi") $a2, in("rdx") $a3, in("r10") $a4, out("rax") r, lateout("rcx") _, lateout("r11") _, options(nostack));
        r
    }};
}

pub use syscall;

pub fn sys_exit(code: i32) -> ! {
    unsafe { syscall!(SYS_EXIT, code as u64); }
    loop { unsafe { asm!("hlt", options(nostack)); } }
}

pub fn sys_write(fd: i32, buf: *const u8, len: usize) -> i64 {
    unsafe { syscall!(SYS_WRITE, fd as u64, buf as u64, len as u64) as i64 }
}

pub fn sys_read(fd: i32, buf: *mut u8, len: usize) -> i64 {
    unsafe { syscall!(SYS_READ, fd as u64, buf as u64, len as u64) as i64 }
}

pub fn sys_open(path: *const u8, flags: u32) -> i64 {
    unsafe { syscall!(SYS_OPEN, path as u64, flags as u64) as i64 }
}

pub fn sys_close(fd: i32) -> i64 {
    unsafe { syscall!(SYS_CLOSE, fd as u64) as i64 }
}

pub fn sys_getpid() -> u64 {
    unsafe { syscall!(SYS_GETPID) }
}

pub fn sys_yield() {
    unsafe { syscall!(SYS_YIELD); }
}

pub fn sys_sleep(ticks: u64) {
    unsafe { syscall!(SYS_SLEEP, ticks); }
}

pub fn sys_brk(addr: usize) -> i64 {
    unsafe { syscall!(SYS_BRK, addr as u64) as i64 }
}

pub fn sys_mmap(addr: usize, len: usize, prot: u32, flags: u32) -> i64 {
    unsafe { syscall!(SYS_MMAP, addr as u64, len as u64, prot as u64, flags as u64) as i64 }
}

pub fn sys_getdents(path: *const u8, buf: *mut u8, count: usize) -> i64 {
    unsafe { syscall!(SYS_GETDIRENTS, path as u64, buf as u64, count as u64) as i64 }
}

pub fn sys_execve(path: *const u8, argv: *const u8, envp: *const u8) -> i64 {
    unsafe { syscall!(SYS_EXECVE, path as u64, argv as u64, envp as u64) as i64 }
}

pub fn sys_dup2(oldfd: i32, newfd: i32) -> i64 {
    unsafe { syscall!(SYS_DUP2, oldfd as u64, newfd as u64) as i64 }
}

pub fn sys_uptime() -> u64 {
    unsafe { syscall!(SYS_UPTIME) }
}
