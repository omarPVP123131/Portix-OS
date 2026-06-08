extern crate alloc;
use crate::drivers::serial;
use crate::drivers::storage::fat32::{Fat32Volume, DirEntryInfo};
use crate::drivers::storage::registry;
use crate::drivers::storage::vfs;
use crate::mem::paging::{self, PAGE_SIZE};
use crate::process::{self, FdEntry, FdType, OpenFileInfo};

fn alloc_page() -> Option<usize> {
    let layout = core::alloc::Layout::from_size_align(PAGE_SIZE, PAGE_SIZE).ok()?;
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() { None } else { Some(ptr as usize) }
}

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

extern "C" {
    fn ring3_exit_trampoline();
}

#[repr(C)]
pub struct SyscallResult(pub u64, pub u64);

#[no_mangle]
extern "C" fn syscall_dispatch(
    num: u64, a1: u64, a2: u64, a3: u64, _a4: u64, _a5: u64,
    current_rsp: u64,
) -> SyscallResult {
    let result = match num {
        SYS_EXIT => sys_exit(a1 as usize),
        SYS_WRITE => SyscallResult(sys_write(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_GETPID => SyscallResult(sys_getpid(), 0),
        SYS_YIELD => SyscallResult(0, sys_yield_switch(current_rsp)),
        SYS_SLEEP => SyscallResult(0, sys_sleep_switch(a1, current_rsp)),
        SYS_READ => SyscallResult(sys_read(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_OPEN => SyscallResult(sys_open(a1 as usize, a2 as u32) as u64, 0),
        SYS_CLOSE => SyscallResult(sys_close(a1 as i32) as u64, 0),
        SYS_BRK => SyscallResult(sys_brk(a1 as usize) as u64, 0),
        SYS_MMAP => SyscallResult(sys_mmap(a1 as usize, a2 as usize, a3 as u32, _a4 as u32) as u64, 0),
        _ => SyscallResult(0, 0),
    };
    result
}

// ── SYS_EXIT ─────────────────────────────────────────────────────────────

fn sys_exit(_status: usize) -> ! {
    serial::write_str("[R3] SYS_EXIT called\n");
    unsafe { ring3_exit_trampoline(); }
    unreachable!()
}

// ── SYS_WRITE ────────────────────────────────────────────────────────────

fn sys_write(fd: i32, buf: usize, count: usize) -> i64 {
    if fd != 1 && fd != 2 {
        return -1;
    }

    let mut kbuf = [0u8; 256];
    let to_copy = if count > 256 { 256 } else { count };

    match paging::copy_from_user(&mut kbuf[..to_copy], buf, to_copy) {
        Ok(copied) => {
            for &b in &kbuf[..copied] {
                serial::write_byte(b);
            }
            copied as i64
        }
        Err(()) => -1,
    }
}

// ── SYS_GETPID ───────────────────────────────────────────────────────────

fn sys_getpid() -> u64 {
    process::current_process()
        .map(|p| p.pid)
        .unwrap_or(0)
}

// ── Scheduler helpers ────────────────────────────────────────────────────

fn saved_cs_from_rsp(current_rsp: u64) -> u64 {
    unsafe { (current_rsp as *const u64).add(16).read() }
}

fn sys_yield_switch(current_rsp: u64) -> u64 {
    serial::write_str("[R3] SYS_YIELD\n");
    unsafe {
        let cur = process::current_raw();
        if !cur.is_null() {
            (*cur).ticks_used = process::TIME_SLICE;
        }
    }
    let cs = saved_cs_from_rsp(current_rsp);
    process::schedule_tick(current_rsp, cs)
}

fn sys_sleep_switch(ticks: u64, current_rsp: u64) -> u64 {
    serial::write_str("[R3] SYS_SLEEP ");
    serial::write_usize(ticks as usize);
    serial::write_str(" ticks\n");
    if let Some(proc) = process::current_process() {
        let now = crate::time::pit::ticks();
        proc.sleep_until = now.wrapping_add(ticks);
        proc.state = process::ProcessState::Blocked;
    }
    unsafe {
        let cur = process::current_raw();
        if !cur.is_null() {
            (*cur).ticks_used = process::TIME_SLICE;
        }
    }
    let cs = saved_cs_from_rsp(current_rsp);
    process::schedule_tick(current_rsp, cs)
}

// ── SYS_READ ─────────────────────────────────────────────────────────────

fn sys_read(fd: i32, buf: usize, count: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -1;
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -1,
    };
    let entry = match process::fd_get(&proc, fd as usize) {
        Some(e) => e.clone(),
        None => return -1,
    };
    // proc borrow ends here (entry is a clone)

    match &entry.fd_type {
        FdType::Stdin => {
            // No keyboard buffer for ring-3 yet — return 0 (non-blocking)
            serial::write_str("[SYS] READ stdin → 0 (no data)\n");
            Ok(0)
        }
        FdType::Stdout | FdType::Stderr => {
            serial::write_str("[SYS] READ: fd not readable\n");
            Err(())
        }
        FdType::File(info) => {
            let info_clone = info.clone();
            sys_read_file(fd, buf, count, info_clone)
        }
    }.unwrap_or(-1)
}

fn sys_read_file(fd: i32, buf: usize, count: usize, info: OpenFileInfo) -> Result<i64, ()> {
    if info.pos >= info.size {
        return Ok(0);
    }
    let to_read = (count as u32).min(info.size - info.pos) as usize;
    if to_read == 0 {
        return Ok(0);
    }

    let mut kbuf = alloc::vec![0u8; to_read];

    let drive = registry::get_device(0).ok_or(())?;
    let mut vol = Fat32Volume::mount(drive).map_err(|_| ())?;

    // Build a DirEntryInfo from our stored info
    let entry = DirEntryInfo {
        name: info.name,
        name_len: info.name_len,
        is_dir: false,
        size: info.size,
        cluster: info.cluster,
        dir_sector: 0,
        dir_offset: 0,
    };

    let full_data = {
        let mut tmp = alloc::vec![0u8; info.size as usize];
        vol.read_file(&entry, &mut tmp).map_err(|_| ())?;
        tmp
    };
    drop(vol);

    let start = info.pos as usize;
    let end = (start + to_read).min(full_data.len());
    let n = end - start;
    kbuf[..n].copy_from_slice(&full_data[start..end]);

    if paging::copy_to_user(buf, &kbuf[..n]).is_err() {
        return Err(());
    }

    // Update position
    if let Some(proc) = process::current_process() {
        if let Some(fd_entry) = process::fd_get_mut(proc, fd as usize) {
            if let FdType::File(ref mut fi) = fd_entry.fd_type {
                fi.pos = info.pos + n as u32;
            }
        }
    }

    serial::write_str("[SYS] READ file fd=");
    serial::write_usize(fd as usize);
    serial::write_str(" bytes=");
    serial::write_usize(n);
    serial::write_str("\n");

    Ok(n as i64)
}

// ── SYS_OPEN ─────────────────────────────────────────────────────────────

fn sys_open(path_ptr: usize, _flags: u32) -> i64 {
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_from_user(&mut path_buf, path_ptr, 256) {
        Ok(n) => n,
        Err(()) => return -1,
    };
    // Null-terminated from userspace
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    serial::write_str("[SYS] OPEN path='");
    serial::write_str(path);
    serial::write_str("'\n");

    let drive = match registry::get_device(0) {
        Some(d) => d,
        None => {
            serial::write_str("[SYS] OPEN: no device\n");
            return -1;
        }
    };

    let mut vol = match Fat32Volume::mount(drive) {
        Ok(v) => v,
        Err(_) => {
            serial::write_str("[SYS] OPEN: mount failed\n");
            return -1;
        }
    };

    let root = vol.root_cluster();

    // Parse path and find entry
    let mut bufs = [[0u8; 64]; 16];
    let mut lens = [0usize; 16];
    let n = vfs::path_split(path, &mut bufs, &mut lens);
    if n == 0 {
        serial::write_str("[SYS] OPEN: empty path\n");
        return -1;
    }

    let mut cur = root;

    for i in 0..n {
        let comp = vfs::component_str(&bufs, &lens, i);
        if i == n - 1 {
            let entry = match vol.find_entry(cur, comp) {
                Ok(e) => e,
                Err(_) => {
                    serial::write_str("[SYS] OPEN: not found\n");
                    return -1;
                }
            };
            if entry.is_dir {
                serial::write_str("[SYS] OPEN: is a directory\n");
                return -1;
            }

            let info = OpenFileInfo {
                dir_cluster: cur,
                cluster: entry.cluster,
                size: entry.size,
                pos: 0,
                name: entry.name,
                name_len: entry.name_len,
            };

            drop(vol);

            let fd_entry = FdEntry { fd_type: FdType::File(info) };
            if let Some(proc) = process::current_process() {
                match process::fd_alloc(proc, fd_entry) {
                    Some(fd) => {
                        serial::write_str("[SYS] OPEN → fd=");
                        serial::write_usize(fd);
                        serial::write_str("\n");
                        return fd as i64;
                    }
                    None => {
                        serial::write_str("[SYS] OPEN: no free fd\n");
                        return -1;
                    }
                }
            }
            return -1;
        } else {
            match vol.find_entry(cur, comp) {
                Ok(e) if e.is_dir => cur = e.cluster,
                _ => {
                    serial::write_str("[SYS] OPEN: path component not found\n");
                    return -1;
                }
            }
        }
    }
    -1
}

// ── SYS_CLOSE ────────────────────────────────────────────────────────────

fn sys_close(fd: i32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -1;
    }
    if let Some(mut proc) = process::current_process() {
        process::fd_close(&mut proc, fd as usize);
        serial::write_str("[SYS] CLOSE fd=");
        serial::write_usize(fd as usize);
        serial::write_str("\n");
        0
    } else {
        -1
    }
}

// ── SYS_BRK ──────────────────────────────────────────────────────────────

fn sys_brk(addr: usize) -> i64 {
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -1,
    };

    if addr == 0 {
        // Query current break
        return proc.program_break as i64;
    }

    let cr3 = proc.cr3;
    let current_end = proc.program_break_end;
    let new_brk = addr.max(process::PROGRAM_BREAK_BASE);

    if new_brk > current_end {
        // Expand: map new pages
        let start_page = (current_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let end_page = (new_brk + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let pages = (end_page - start_page) / PAGE_SIZE;

        for i in 0..pages {
            let vaddr = start_page + i * PAGE_SIZE;
            let paddr = match alloc_page() {
                Some(p) => p,
                None => {
                    serial::write_str("[SYS] BRK: OOM\n");
                    return proc.program_break as i64;
                }
            };
            if paging::map_page_user(cr3, vaddr, paddr).is_err() {
                return proc.program_break as i64;
            }
        }
    } else if new_brk < process::PROGRAM_BREAK_BASE {
        // Can't shrink below base
        return -1;
    }
    // For shrink: we could unmap pages here, but keeping them is fine (lazy)

    proc.program_break = new_brk;
    if new_brk > proc.program_break_end {
        proc.program_break_end = new_brk;
    }

    serial::write_str("[SYS] BRK → ");
    serial::write_hex(new_brk);
    serial::write_str("\n");

    new_brk as i64
}

// ── SYS_MMAP ─────────────────────────────────────────────────────────────

fn sys_mmap(addr: usize, len: usize, _prot: u32, _flags: u32) -> i64 {
    if len == 0 { return -1; }

    let aligned_addr = if addr == 0 {
        // Let kernel choose — use a heuristic: end of brk + 1MB
        process::current_process()
            .map(|p| (p.program_break_end + 0x100000 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
            .unwrap_or(0x3000_0000_0000)
    } else {
        addr & !(PAGE_SIZE - 1)
    };

    let pages = (len + PAGE_SIZE - 1) / PAGE_SIZE;

    let cr3 = match process::current_process() {
        Some(p) => p.cr3,
        None => return -1,
    };

    let mapped_addr = aligned_addr;
    for i in 0..pages {
        let vaddr = aligned_addr + i * PAGE_SIZE;
        let paddr = match alloc_page() {
            Some(p) => p,
            None => return -1,
        };
        if paging::map_page_user(cr3, vaddr, paddr).is_err() {
            return -1;
        }
    }

    if let Some(proc) = process::current_process() {
        let end = aligned_addr + pages * PAGE_SIZE;
        if end > proc.program_break_end {
            proc.program_break_end = end;
        }
    }

    serial::write_str("[SYS] MMAP addr=0x");
    serial::write_hex(mapped_addr);
    serial::write_str(" pages=");
    serial::write_usize(pages);
    serial::write_str("\n");

    mapped_addr as i64
}
