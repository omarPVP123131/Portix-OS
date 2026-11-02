#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut max_lines = 10usize;
    let mut file_path: Option<&str> = None;

    if argc > 1 && !argv.is_null() {
        let mut idx = 1;
        while idx < argc {
            let arg_ptr = unsafe { *argv.add(idx) };
            if !arg_ptr.is_null() {
                let mut len = 0;
                while unsafe { *arg_ptr.add(len) } != 0 {
                    len += 1;
                }
                if let Ok(arg_str) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }) {
                    if arg_str == "-n" && idx + 1 < argc {
                        idx += 1;
                        let val_ptr = unsafe { *argv.add(idx) };
                        if !val_ptr.is_null() {
                            let mut vlen = 0;
                            while unsafe { *val_ptr.add(vlen) } != 0 { vlen += 1; }
                            if let Ok(vs) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(val_ptr, vlen) }) {
                                if let Ok(parsed) = vs.parse::<usize>() {
                                    max_lines = parsed;
                                }
                            }
                        }
                    } else if !arg_str.starts_with('-') {
                        file_path = Some(arg_str);
                    }
                }
            }
            idx += 1;
        }
    }

    let path = match file_path {
        Some(p) => p,
        None => {
            println!("Uso: head [-n lineas] <archivo>");
            return 1;
        }
    };

    let mut path_cstr = alloc::string::String::from(path);
    path_cstr.push('\0');

    let fd = portix_rt::syscall::sys_open(path_cstr.as_ptr(), 0);
    if fd < 0 {
        println!("head: no se puede abrir '{}'", path);
        return 1;
    }

    let mut buf = [0u8; 1024];
    let mut lines_printed = 0;

    'outer: loop {
        let n = portix_rt::syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
        if n <= 0 {
            break;
        }
        for &b in &buf[..n as usize] {
            let ch = [b];
            let _ = portix_rt::syscall::sys_write(1, ch.as_ptr(), 1);
            if b == b'\n' {
                lines_printed += 1;
                if lines_printed >= max_lines {
                    break 'outer;
                }
            }
        }
    }

    let _ = portix_rt::syscall::sys_close(fd as i32);
    0
}
