#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;

fn read_lines(filename: &str) -> Option<Vec<String>> {
    let mut cstr = String::from(filename);
    cstr.push('\0');
    let fd = portix_rt::syscall::sys_open(cstr.as_ptr(), 0);
    if fd < 0 {
        return None;
    }
    let mut content = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let n = portix_rt::syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
        if n <= 0 {
            break;
        }
        content.extend_from_slice(&buf[..n as usize]);
    }
    let _ = portix_rt::syscall::sys_close(fd as i32);
    let text = core::str::from_utf8(&content).ok()?;
    let mut lines = Vec::new();
    for l in text.split('\n') {
        if !l.is_empty() {
            lines.push(String::from(l));
        }
    }
    Some(lines)
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        println!("Uso: paste <archivo1> [archivo2...]");
        return 1;
    }

    let mut files_lines: Vec<Vec<String>> = Vec::new();
    for i in 1..argc {
        let p = unsafe { *argv.add(i) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(fname) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                match read_lines(fname) {
                    Some(lines) => files_lines.push(lines),
                    None => {
                        println!("paste: no se puede abrir '{}'", fname);
                        return 1;
                    }
                }
            }
        }
    }

    if files_lines.is_empty() {
        return 0;
    }

    let mut max_lines = 0;
    for f in &files_lines {
        if f.len() > max_lines {
            max_lines = f.len();
        }
    }

    for row in 0..max_lines {
        for (col, f) in files_lines.iter().enumerate() {
            if col > 0 {
                let _ = portix_rt::print!("\t");
            }
            if row < f.len() {
                let _ = portix_rt::print!("{}", f[row]);
            }
        }
        println!();
    }

    0
}
