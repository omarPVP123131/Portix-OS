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

    let text = match core::str::from_utf8(&content) {
        Ok(t) => t,
        Err(_) => return None,
    };

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
    if argc < 3 || argv.is_null() {
        println!("Uso: comm <archivo1> <archivo2>");
        return 2;
    }

    let p1 = unsafe { *argv.add(1) };
    let p2 = unsafe { *argv.add(2) };
    if p1.is_null() || p2.is_null() {
        return 2;
    }

    let mut l1 = 0; while unsafe { *p1.add(l1) } != 0 { l1 += 1; }
    let mut l2 = 0; while unsafe { *p2.add(l2) } != 0 { l2 += 1; }
    let s1 = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(p1, l1) }) { Ok(s) => s, Err(_) => return 2 };
    let s2 = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(p2, l2) }) { Ok(s) => s, Err(_) => return 2 };

    let lines1 = match read_lines(s1) {
        Some(l) => l,
        None => {
            println!("comm: no se puede leer '{}'", s1);
            return 2;
        }
    };
    let lines2 = match read_lines(s2) {
        Some(l) => l,
        None => {
            println!("comm: no se puede leer '{}'", s2);
            return 2;
        }
    };

    let mut i = 0;
    let mut j = 0;

    while i < lines1.len() && j < lines2.len() {
        if lines1[i] == lines2[j] {
            println!("\t\t{}", lines1[i]);
            i += 1;
            j += 1;
        } else if lines1[i] < lines2[j] {
            println!("{}", lines1[i]);
            i += 1;
        } else {
            println!("\t{}", lines2[j]);
            j += 1;
        }
    }

    while i < lines1.len() {
        println!("{}", lines1[i]);
        i += 1;
    }

    while j < lines2.len() {
        println!("\t{}", lines2[j]);
        j += 1;
    }

    0
}
