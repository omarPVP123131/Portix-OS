#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use portix_rt::println;

fn expand_data(bytes: &[u8], tab_stop: usize) {
    let mut col = 0;
    for &b in bytes {
        if b == b'\t' {
            let num_spaces = tab_stop - (col % tab_stop);
            for _ in 0..num_spaces {
                let _ = portix_rt::print!(" ");
            }
            col += num_spaces;
        } else if b == b'\n' {
            let _ = portix_rt::print!("\n");
            col = 0;
        } else {
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
    let tab_stop = 8usize;
    if argc < 2 || argv.is_null() {
        let mut buf = [0u8; 512];
        loop {
            let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
            if n <= 0 { break; }
            expand_data(&buf[..n as usize], tab_stop);
        }
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
                    let mut buf = [0u8; 512];
                    loop {
                        let n = portix_rt::syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
                        if n <= 0 { break; }
                        expand_data(&buf[..n as usize], tab_stop);
                    }
                    let _ = portix_rt::syscall::sys_close(fd as i32);
                } else {
                    println!("expand: no se puede abrir '{}'", fname);
                }
            }
        }
    }
    0
}
