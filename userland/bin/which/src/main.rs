#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::{println, File, O_RDONLY};

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        return 1;
    }

    let mut exit_code = 0;
    for i in 1..argc {
        let arg_ptr = unsafe { *argv.add(i) };
        if arg_ptr.is_null() {
            continue;
        }
        let mut len = 0;
        while unsafe { *arg_ptr.add(len) } != 0 {
            len += 1;
        }
        let name = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }) {
            Ok(s) => s,
            Err(_) => {
                exit_code = 1;
                continue;
            }
        };

        if name.starts_with('/') {
            if File::open(name, O_RDONLY).is_ok() {
                println!("{}", name);
            } else {
                exit_code = 1;
            }
        } else {
            let mut found = false;
            let dirs = ["/bin", "/usr/bin", "/sbin"];
            for d in dirs {
                let mut buf = [0u8; 128];
                let d_bytes = d.as_bytes();
                let n_bytes = name.as_bytes();
                if d_bytes.len() + 1 + n_bytes.len() < buf.len() {
                    buf[..d_bytes.len()].copy_from_slice(d_bytes);
                    buf[d_bytes.len()] = b'/';
                    buf[d_bytes.len() + 1..d_bytes.len() + 1 + n_bytes.len()].copy_from_slice(n_bytes);
                    if let Ok(full_path) = core::str::from_utf8(&buf[..d_bytes.len() + 1 + n_bytes.len()]) {
                        if File::open(full_path, O_RDONLY).is_ok() {
                            println!("{}", full_path);
                            found = true;
                            break;
                        }
                    }
                }
            }
            if !found {
                exit_code = 1;
            }
        }
    }

    exit_code
}
