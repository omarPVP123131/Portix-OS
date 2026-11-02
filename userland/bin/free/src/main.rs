#![no_std]
#![no_main]

use portix_rt::println;
use portix_rt::syscall::{sys_open, sys_read, sys_close};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let fd = sys_open(b"/proc/meminfo\0".as_ptr(), 0);
    if fd < 0 {
        println!("free: no se pudo abrir /proc/meminfo");
        return 1;
    }

    let mut buf = [0u8; 512];
    let n = sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
    sys_close(fd as i32);
    if n <= 0 {
        println!("free: error leyendo /proc/meminfo");
        return 1;
    }

    let text = match core::str::from_utf8(&buf[..n as usize]) {
        Ok(t) => t,
        Err(_) => return 1,
    };

    let mut total = 0u64;
    let mut free = 0u64;
    let mut avail = 0u64;
    let mut cached = 0u64;

    for line in text.lines() {
        if line.starts_with("MemTotal:") {
            total = line[9..].trim().split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("MemFree:") {
            free = line[8..].trim().split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("MemAvailable:") {
            avail = line[13..].trim().split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("Cached:") {
            cached = line[7..].trim().split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }

    let used = total.saturating_sub(free).saturating_sub(cached);

    println!("               total        used        free      shared  buff/cache   available");
    println!("Mem:       {:9}   {:9}   {:9}           0   {:9}   {:9}", total, used, free, cached, avail);
    println!("Swap:              0           0           0");

    0
}
