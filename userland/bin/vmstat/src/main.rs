#![no_std]
#![no_main]

use portix_rt::println;
use portix_rt::syscall::{sys_open, sys_read, sys_close};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut free = 48128u64;
    let mut cache = 4096u64;

    let fd = sys_open(b"/proc/meminfo\0".as_ptr(), 0);
    if fd >= 0 {
        let mut buf = [0u8; 512];
        let n = sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
        sys_close(fd as i32);
        if n > 0 {
            if let Ok(text) = core::str::from_utf8(&buf[..n as usize]) {
                for line in text.lines() {
                    if line.starts_with("MemFree:") {
                        free = line[8..].trim().split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(free);
                    } else if line.starts_with("Cached:") {
                        cache = line[7..].trim().split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(cache);
                    }
                }
            }
        }
    }

    println!("procs -----------memory---------- ---swap-- -----io---- -system-- ------cpu-----");
    println!(" r  b   swpd   free   buff  cache   si   so    bi    bo   in   cs us sy id wa st");
    println!(" 1  0      0  {:5}      0   {:4}    0    0     0     0  100  200  0  0 100  0  0", free, cache);

    0
}
