#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;

fn process_uniq(lines: &[&str], count_mode: bool) {
    if lines.is_empty() {
        return;
    }

    let mut current = lines[0];
    let mut count = 1usize;

    for &line in &lines[1..] {
        if line == current {
            count += 1;
        } else {
            if count_mode {
                println!("{:7} {}", count, current);
            } else {
                println!("{}", current);
            }
            current = line;
            count = 1;
        }
    }

    if count_mode {
        println!("{:7} {}", count, current);
    } else {
        println!("{}", current);
    }
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut count_mode = false;
    let mut file_idx = 1;

    if argc > 1 && !argv.is_null() {
        let p = unsafe { *argv.add(1) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(arg) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                if arg == "-c" {
                    count_mode = true;
                    file_idx = 2;
                }
            }
        }
    }

    let mut content = Vec::new();
    if file_idx >= argc {
        let mut buf = [0u8; 512];
        loop {
            let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
            if n <= 0 { break; }
            content.extend_from_slice(&buf[..n as usize]);
        }
    } else {
        let p = unsafe { *argv.add(file_idx) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(fname) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                let mut cstr = String::from(fname);
                cstr.push('\0');
                let fd = portix_rt::syscall::sys_open(cstr.as_ptr(), 0);
                if fd >= 0 {
                    let mut buf = [0u8; 512];
                    loop {
                        let n = portix_rt::syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
                        if n <= 0 { break; }
                        content.extend_from_slice(&buf[..n as usize]);
                    }
                    let _ = portix_rt::syscall::sys_close(fd as i32);
                } else {
                    println!("uniq: no se puede abrir '{}'", fname);
                    return 1;
                }
            }
        }
    }

    if let Ok(text) = core::str::from_utf8(&content) {
        let mut lines = Vec::new();
        for l in text.split('\n') {
            if !l.is_empty() {
                lines.push(l);
            }
        }
        process_uniq(&lines, count_mode);
    }

    0
}
