#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;

fn canonicalize(path: &str) -> String {
    let mut abs = String::new();
    if !path.starts_with('/') {
        let mut cwd_buf = [0u8; 256];
        let ret = portix_rt::syscall::sys_getcwd(cwd_buf.as_mut_ptr(), cwd_buf.len());
        if ret >= 0 {
            let l = cwd_buf.iter().position(|&b| b == 0).unwrap_or(cwd_buf.len());
            if let Ok(s) = core::str::from_utf8(&cwd_buf[..l]) {
                abs.push_str(s);
            }
        }
        if !abs.ends_with('/') {
            abs.push('/');
        }
        abs.push_str(path);
    } else {
        abs.push_str(path);
    }

    let mut parts: Vec<&str> = Vec::new();
    for seg in abs.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        } else if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }

    if parts.is_empty() {
        return String::from("/");
    }

    let mut res = String::new();
    for part in parts {
        res.push('/');
        res.push_str(part);
    }
    res
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        println!("{}", canonicalize("."));
        return 0;
    }

    for i in 1..argc {
        let p = unsafe { *argv.add(i) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(arg) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                println!("{}", canonicalize(arg));
            }
        }
    }
    0
}
