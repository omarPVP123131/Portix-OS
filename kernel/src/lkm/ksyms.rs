// kernel/src/lkm/ksyms.rs — Kernel Symbol Table & Dynamic Extensions Registry
#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::Layout;
use crate::arch::Spinlock;
use crate::drivers::serial;

// Dynamic commands registered by modules
static DYNAMIC_CMDS: Spinlock<BTreeMap<String, extern "C" fn(*const u8)>> =
    Spinlock::new(BTreeMap::new());

// Dynamic syscall handlers (syscall numbers 1000..1031)
pub const DYNAMIC_SYSCALL_BASE: u64 = 1000;
pub const MAX_DYNAMIC_SYSCALLS: usize = 32;

static DYNAMIC_SYSCALLS: Spinlock<[Option<extern "C" fn(u64, u64, u64, u64, u64) -> i64>; MAX_DYNAMIC_SYSCALLS]> =
    Spinlock::new([None; MAX_DYNAMIC_SYSCALLS]);

// C-compatible wrapper functions exported to modules

pub unsafe extern "C" fn ksym_kprint(s: *const u8) {
    if s.is_null() { return; }
    let mut len = 0usize;
    while *s.add(len) != 0 && len < 4096 {
        len += 1;
    }
    let slice = core::slice::from_raw_parts(s, len);
    if let Ok(msg) = core::str::from_utf8(slice) {
        serial::write_str(msg);
    }
}

pub unsafe extern "C" fn ksym_kmalloc(size: usize) -> *mut u8 {
    if size == 0 { return core::ptr::null_mut(); }
    let align = 8;
    let layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return core::ptr::null_mut(),
    };
    alloc::alloc::alloc_zeroed(layout)
}

pub unsafe extern "C" fn ksym_kfree(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 { return; }
    let layout = match Layout::from_size_align(size, 8) {
        Ok(l) => l,
        Err(_) => return,
    };
    alloc::alloc::dealloc(ptr, layout);
}

pub extern "C" fn ksym_get_ticks() -> u64 {
    crate::time::pit::ticks()
}

pub extern "C" fn ksym_get_uptime_secs() -> u64 {
    crate::time::pit::ticks() / 100
}

pub unsafe extern "C" fn ksym_register_cmd(name: *const u8, handler: extern "C" fn(*const u8)) -> i32 {
    if name.is_null() { return -14; } // -EFAULT
    let mut len = 0;
    while *name.add(len) != 0 && len < 64 { len += 1; }
    let slice = core::slice::from_raw_parts(name, len);
    let Ok(cmd_str) = core::str::from_utf8(slice) else { return -22; }; // -EINVAL
    
    let mut guard = DYNAMIC_CMDS.lock();
    guard.insert(String::from(cmd_str), handler);
    serial::write_str("[LKM] Dynamic command registered: '");
    serial::write_str(cmd_str);
    serial::write_str("'\n");
    0
}

pub unsafe extern "C" fn ksym_unregister_cmd(name: *const u8) -> i32 {
    if name.is_null() { return -14; } // -EFAULT
    let mut len = 0;
    while *name.add(len) != 0 && len < 64 { len += 1; }
    let slice = core::slice::from_raw_parts(name, len);
    let Ok(cmd_str) = core::str::from_utf8(slice) else { return -22; }; // -EINVAL

    let mut guard = DYNAMIC_CMDS.lock();
    if guard.remove(cmd_str).is_some() {
        serial::write_str("[LKM] Dynamic command unregistered: '");
        serial::write_str(cmd_str);
        serial::write_str("'\n");
        0
    } else {
        -1
    }
}

pub extern "C" fn ksym_register_syscall(
    num: u64,
    handler: extern "C" fn(u64, u64, u64, u64, u64) -> i64,
) -> i32 {
    if num < DYNAMIC_SYSCALL_BASE || num >= DYNAMIC_SYSCALL_BASE + MAX_DYNAMIC_SYSCALLS as u64 {
        return -22; // -EINVAL: fuera del rango dinamico
    }
    let idx = (num - DYNAMIC_SYSCALL_BASE) as usize;
    let mut guard = DYNAMIC_SYSCALLS.lock();
    guard[idx] = Some(handler);
    serial::write_str("[LKM] Dynamic syscall registered: ");
    serial::write_usize(num as usize);
    serial::write_str("\n");
    0
}

pub extern "C" fn ksym_unregister_syscall(num: u64) -> i32 {
    if num < DYNAMIC_SYSCALL_BASE || num >= DYNAMIC_SYSCALL_BASE + MAX_DYNAMIC_SYSCALLS as u64 {
        return -22; // -EINVAL
    }
    let idx = (num - DYNAMIC_SYSCALL_BASE) as usize;
    let mut guard = DYNAMIC_SYSCALLS.lock();
    if guard[idx].is_some() {
        guard[idx] = None;
        serial::write_str("[LKM] Dynamic syscall unregistered: ");
        serial::write_usize(num as usize);
        serial::write_str("\n");
        0
    } else {
        -1
    }
}

pub fn lookup_symbol(name: &str) -> Option<usize> {
    match name {
        "kprint" | "printk" | "klog" => Some(ksym_kprint as *const () as usize),
        "kmalloc" => Some(ksym_kmalloc as *const () as usize),
        "kfree" => Some(ksym_kfree as *const () as usize),
        "get_ticks" => Some(ksym_get_ticks as *const () as usize),
        "get_uptime" => Some(ksym_get_uptime_secs as *const () as usize),
        "register_cmd" => Some(ksym_register_cmd as *const () as usize),
        "unregister_cmd" => Some(ksym_unregister_cmd as *const () as usize),
        "register_syscall" => Some(ksym_register_syscall as *const () as usize),
        "unregister_syscall" => Some(ksym_unregister_syscall as *const () as usize),
        _ => None,
    }
}

pub fn dispatch_dynamic_cmd(name: &str, args: &str) -> bool {
    let guard = DYNAMIC_CMDS.lock();
    if let Some(&handler) = guard.get(name) {
        let mut arg_buf = Vec::from(args.as_bytes());
        arg_buf.push(0);
        handler(arg_buf.as_ptr());
        true
    } else {
        false
    }
}

pub fn dispatch_dynamic_syscall(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> Option<i64> {
    if num < DYNAMIC_SYSCALL_BASE || num >= DYNAMIC_SYSCALL_BASE + MAX_DYNAMIC_SYSCALLS as u64 {
        return None;
    }
    let idx = (num - DYNAMIC_SYSCALL_BASE) as usize;
    let guard = DYNAMIC_SYSCALLS.lock();
    if let Some(handler) = guard[idx] {
        Some(handler(a1, a2, a3, a4, a5))
    } else {
        None
    }
}
