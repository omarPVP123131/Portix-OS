#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;
use portix_rt::syscall::{sys_open, sys_read, sys_close, sys_getdents};

struct ProcessInfo {
    pid: u64,
    ppid: u64,
    name: String,
    state: char,
}

fn parse_proc_status(pid_str: &str) -> Option<ProcessInfo> {
    let mut path_buf = [0u8; 64];
    let prefix = b"/proc/";
    let suffix = b"/status";
    let mut p = 0;
    for &b in prefix { path_buf[p] = b; p += 1; }
    for &b in pid_str.as_bytes() { path_buf[p] = b; p += 1; }
    for &b in suffix { path_buf[p] = b; p += 1; }
    path_buf[p] = 0;

    let fd = sys_open(path_buf.as_ptr(), 0);
    if fd < 0 { return None; }

    let mut buf = [0u8; 512];
    let n = sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
    sys_close(fd as i32);
    if n <= 0 { return None; }

    let text = core::str::from_utf8(&buf[..n as usize]).ok()?;
    let mut name = String::new();
    let mut state = '?';
    let mut pid = 0u64;
    let mut ppid = 0u64;

    for line in text.lines() {
        if line.starts_with("Name:\t") {
            name = String::from(&line[6..]);
        } else if line.starts_with("State:\t") {
            if let Some(ch) = line[7..].chars().next() {
                state = ch;
            }
        } else if line.starts_with("Pid:\t") {
            pid = line[5..].trim().parse::<u64>().unwrap_or(0);
        } else if line.starts_with("PPid:\t") {
            ppid = line[6..].trim().parse::<u64>().unwrap_or(0);
        }
    }

    if pid > 0 {
        Some(ProcessInfo { pid, ppid, name, state })
    } else {
        None
    }
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut full = false;
    for i in 1..argc {
        unsafe {
            let ptr = *argv.offset(i as isize);
            if !ptr.is_null() {
                let mut len = 0;
                while *ptr.add(len) != 0 { len += 1; }
                if let Ok(s) = core::str::from_utf8(core::slice::from_raw_parts(ptr, len)) {
                    if s == "-ef" || s == "aux" || s == "-e" || s == "-a" {
                        full = true;
                    }
                }
            }
        }
    }

    let mut dirent_buf = [0u8; 1024];
    let n = sys_getdents(b"/proc\0".as_ptr(), dirent_buf.as_mut_ptr(), dirent_buf.len());
    if n <= 0 {
        println!("ps: fallo al leer /proc");
        return 1;
    }

    let mut procs: Vec<ProcessInfo> = Vec::new();
    let mut offset = 0usize;
    let total = n as usize;

    while offset + 19 <= total {
        let reclen = u16::from_le_bytes([dirent_buf[offset + 16], dirent_buf[offset + 17]]) as usize;
        let dtype = dirent_buf[offset + 18];
        if reclen == 0 || offset + reclen > total { break; }

        let name_start = offset + 19;
        let mut name_end = name_start;
        while name_end < offset + reclen && dirent_buf[name_end] != 0 {
            name_end += 1;
        }

        if dtype == 2 { // Directory
            if let Ok(name_str) = core::str::from_utf8(&dirent_buf[name_start..name_end]) {
                if name_str != "." && name_str != ".." && name_str != "self" {
                    if name_str.chars().all(|c| c.is_ascii_digit()) {
                        if let Some(info) = parse_proc_status(name_str) {
                            procs.push(info);
                        }
                    }
                }
            }
        }
        offset += reclen;
    }

    if full {
        println!("UID        PID  PPID STAT TTY          TIME CMD");
        for p in &procs {
            println!("root     {:5} {:5}    {} ?        00:00:00 {}", p.pid, p.ppid, p.state, p.name);
        }
    } else {
        println!("  PID TTY          TIME CMD");
        for p in &procs {
            println!("{:5} ?        00:00:00 {}", p.pid, p.name);
        }
    }

    0
}
