#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;

fn number_lines(text: &str, start_line: &mut usize) {
    for l in text.split('\n') {
        if !l.is_empty() {
            println!("{:6}\t{}", *start_line, l);
            *start_line += 1;
        }
    }
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut line_num = 1usize;

    if argc < 2 || argv.is_null() {
        let mut content = Vec::new();
        let mut buf = [0u8; 512];
        loop {
            let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
            if n <= 0 { break; }
            content.extend_from_slice(&buf[..n as usize]);
        }
        if let Ok(text) = core::str::from_utf8(&content) {
            number_lines(text, &mut line_num);
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
                    let mut content = Vec::new();
                    let mut buf = [0u8; 512];
                    loop {
                        let n = portix_rt::syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
                        if n <= 0 { break; }
                        content.extend_from_slice(&buf[..n as usize]);
                    }
                    let _ = portix_rt::syscall::sys_close(fd as i32);
                    if let Ok(text) = core::str::from_utf8(&content) {
                        number_lines(text, &mut line_num);
                    }
                } else {
                    println!("nl: no se puede abrir '{}'", fname);
                }
            }
        }
    }

    0
}
