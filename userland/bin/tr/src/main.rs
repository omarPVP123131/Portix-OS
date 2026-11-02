#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        println!("Uso: tr [-d] SET1 [SET2]");
        return 1;
    }

    let mut delete_mode = false;
    let set1;
    let mut set2 = "";

    let p1 = unsafe { *argv.add(1) };
    if p1.is_null() { return 1; }
    let mut l1 = 0; while unsafe { *p1.add(l1) } != 0 { l1 += 1; }
    let a1 = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(p1, l1) }) { Ok(s) => s, Err(_) => return 1 };

    if a1 == "-d" {
        delete_mode = true;
        if argc < 3 {
            println!("tr: falta SET1 tras -d");
            return 1;
        }
        let p2 = unsafe { *argv.add(2) };
        if p2.is_null() { return 1; }
        let mut l2 = 0; while unsafe { *p2.add(l2) } != 0 { l2 += 1; }
        set1 = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(p2, l2) }) { Ok(s) => s, Err(_) => return 1 };
    } else {
        set1 = a1;
        if argc > 2 {
            let p2 = unsafe { *argv.add(2) };
            if !p2.is_null() {
                let mut l2 = 0; while unsafe { *p2.add(l2) } != 0 { l2 += 1; }
                if let Ok(s) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p2, l2) }) {
                    set2 = s;
                }
            }
        }
    }

    let set1_bytes = set1.as_bytes();
    let set2_bytes = set2.as_bytes();

    let file_arg = if delete_mode && argc > 3 {
        let pf = unsafe { *argv.add(3) };
        if !pf.is_null() {
            let mut lf = 0; while unsafe { *pf.add(lf) } != 0 { lf += 1; }
            core::str::from_utf8(unsafe { core::slice::from_raw_parts(pf, lf) }).ok()
        } else { None }
    } else if !delete_mode && argc > 3 {
        let pf = unsafe { *argv.add(3) };
        if !pf.is_null() {
            let mut lf = 0; while unsafe { *pf.add(lf) } != 0 { lf += 1; }
            core::str::from_utf8(unsafe { core::slice::from_raw_parts(pf, lf) }).ok()
        } else { None }
    } else {
        None
    };

    let fd = match file_arg {
        Some(fname) => {
            let mut cstr = alloc::string::String::from(fname);
            cstr.push('\0');
            let opened = portix_rt::syscall::sys_open(cstr.as_ptr(), 0);
            if opened < 0 {
                println!("tr: no se puede abrir '{}'", fname);
                return 1;
            }
            opened as i32
        }
        None => 0,
    };

    let mut buf = [0u8; 512];
    loop {
        let n = portix_rt::syscall::sys_read(fd, buf.as_mut_ptr(), buf.len());
        if n <= 0 {
            break;
        }
        let mut out = Vec::new();
        for &b in &buf[..n as usize] {
            if delete_mode {
                if !set1_bytes.contains(&b) {
                    out.push(b);
                }
            } else {
                if let Some(pos) = set1_bytes.iter().position(|&x| x == b) {
                    if pos < set2_bytes.len() {
                        out.push(set2_bytes[pos]);
                    } else if !set2_bytes.is_empty() {
                        out.push(*set2_bytes.last().unwrap());
                    } else {
                        out.push(b);
                    }
                } else {
                    out.push(b);
                }
            }
        }
        if !out.is_empty() {
            let _ = portix_rt::syscall::sys_write(1, out.as_ptr(), out.len());
        }
    }

    if fd > 0 {
        let _ = portix_rt::syscall::sys_close(fd);
    }

    0
}
