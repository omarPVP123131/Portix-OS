#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use portix_rt::println;

fn fold_data(bytes: &[u8], width: usize) {
    let mut col = 0;
    for &b in bytes {
        if b == b'\n' {
            let _ = portix_rt::print!("\n");
            col = 0;
        } else {
            if col >= width {
                let _ = portix_rt::print!("\n");
                col = 0;
            }
            let s = [b];
            if let Ok(c) = core::str::from_utf8(&s) {
                let _ = portix_rt::print!("{}", c);
            }
            col += 1;
        }
    }
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut width = 80usize;
    let mut file_idx = 1;

    if argc > 2 && !argv.is_null() {
        let p1 = unsafe { *argv.add(1) };
        if !p1.is_null() {
            let mut l1 = 0; while unsafe { *p1.add(l1) } != 0 { l1 += 1; }
            if let Ok(s) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p1, l1) }) {
                if s == "-w" {
                    let p2 = unsafe { *argv.add(2) };
                    if !p2.is_null() {
                        let mut l2 = 0; while unsafe { *p2.add(l2) } != 0 { l2 += 1; }
                        if let Ok(w_str) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p2, l2) }) {
                            if let Ok(w) = w_str.parse::<usize>() {
                                width = if w > 0 { w } else { 1 };
                                file_idx = 3;
                            }
                        }
                    }
                }
            }
        }
    }

    if file_idx >= argc {
        // Read from stdin
        let mut buf = [0u8; 512];
        loop {
            let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
            if n <= 0 { break; }
            fold_data(&buf[..n as usize], width);
        }
        return 0;
    }

    while file_idx < argc {
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
                        fold_data(&buf[..n as usize], width);
                    }
                    let _ = portix_rt::syscall::sys_close(fd as i32);
                } else {
                    println!("fold: no se puede abrir '{}'", fname);
                }
            }
        }
        file_idx += 1;
    }

    0
}
