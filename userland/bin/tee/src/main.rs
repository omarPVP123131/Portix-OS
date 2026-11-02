#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut out_fds = Vec::new();
    let mut append = false;

    if argc > 1 && !argv.is_null() {
        let mut idx = 1;
        while idx < argc {
            let p = unsafe { *argv.add(idx) };
            if !p.is_null() {
                let mut len = 0;
                while unsafe { *p.add(len) } != 0 { len += 1; }
                if let Ok(s) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, len) }) {
                    if s == "-a" {
                        append = true;
                    } else {
                        let mut path_cstr = alloc::string::String::from(s);
                        path_cstr.push('\0');
                        let flags = if append { 0x0441 } else { 0x0241 }; // O_WRONLY | O_CREAT | (O_APPEND or O_TRUNC)
                        let fd = portix_rt::syscall::sys_open(path_cstr.as_ptr(), flags);
                        if fd >= 0 {
                            out_fds.push(fd as i32);
                        } else {
                            println!("tee: no se puede abrir '{}'", s);
                        }
                    }
                }
            }
            idx += 1;
        }
    }

    let mut buf = [0u8; 1024];
    loop {
        let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
        if n <= 0 {
            break;
        }
        let count = n as usize;
        // Always write to stdout
        let _ = portix_rt::syscall::sys_write(1, buf.as_ptr(), count);
        // Write to all target files
        for &fd in &out_fds {
            let _ = portix_rt::syscall::sys_write(fd, buf.as_ptr(), count);
        }
    }

    for fd in out_fds {
        let _ = portix_rt::syscall::sys_close(fd);
    }

    0
}
