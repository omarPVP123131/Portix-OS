#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;

fn dump_strings(data: &[u8], min_len: usize) {
    let mut cur: Vec<u8> = Vec::new();
    for &b in data {
        if (0x20..=0x7E).contains(&b) {
            cur.push(b);
        } else {
            if cur.len() >= min_len {
                if let Ok(s) = core::str::from_utf8(&cur) {
                    println!("{}", s);
                }
            }
            cur.clear();
        }
    }
    if cur.len() >= min_len {
        if let Ok(s) = core::str::from_utf8(&cur) {
            println!("{}", s);
        }
    }
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let min_len = 4usize;
    if argc < 2 || argv.is_null() {
        let mut content = Vec::new();
        let mut buf = [0u8; 512];
        loop {
            let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
            if n <= 0 { break; }
            content.extend_from_slice(&buf[..n as usize]);
        }
        dump_strings(&content, min_len);
        return 0;
    }

    for i in 1..argc {
        let p = unsafe { *argv.add(i) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(fname) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                let mut cstr = String::from(fname);
                cstr.push('\0');
                let fd = portix_rt::syscall::sys_open(cstr.as_ptr(), 0);
                if fd >= 0 {
                    let mut content = Vec::new();
                    let mut buf = [0u8; 512];
                    loop {
                        let n = portix_rt::syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
                        if n <= 0 { break; }
                        content.extend_from_slice(&buf[..n as usize]);
                    }
                    let _ = portix_rt::syscall::sys_close(fd as i32);
                    dump_strings(&content, min_len);
                } else {
                    println!("strings: no se puede abrir '{}'", fname);
                }
            }
        }
    }
    0
}
