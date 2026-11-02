#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 3 || argv.is_null() {
        println!("Uso: cmp <archivo1> <archivo2>");
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

    let mut cstr1 = alloc::string::String::from(s1); cstr1.push('\0');
    let mut cstr2 = alloc::string::String::from(s2); cstr2.push('\0');

    let fd1 = portix_rt::syscall::sys_open(cstr1.as_ptr(), 0);
    if fd1 < 0 {
        println!("cmp: no se puede abrir '{}'", s1);
        return 2;
    }
    let fd2 = portix_rt::syscall::sys_open(cstr2.as_ptr(), 0);
    if fd2 < 0 {
        let _ = portix_rt::syscall::sys_close(fd1 as i32);
        println!("cmp: no se puede abrir '{}'", s2);
        return 2;
    }

    let mut buf1 = [0u8; 512];
    let mut buf2 = [0u8; 512];
    let mut byte_idx = 1usize;
    let mut line_idx = 1usize;

    loop {
        let n1 = portix_rt::syscall::sys_read(fd1 as i32, buf1.as_mut_ptr(), buf1.len());
        let n2 = portix_rt::syscall::sys_read(fd2 as i32, buf2.as_mut_ptr(), buf2.len());

        if n1 <= 0 && n2 <= 0 {
            break;
        }

        let common = if n1 > 0 && n2 > 0 { (n1 as usize).min(n2 as usize) } else { 0 };

        for i in 0..common {
            if buf1[i] != buf2[i] {
                println!("{} {} difieren: byte {}, linea {}", s1, s2, byte_idx, line_idx);
                let _ = portix_rt::syscall::sys_close(fd1 as i32);
                let _ = portix_rt::syscall::sys_close(fd2 as i32);
                return 1;
            }
            if buf1[i] == b'\n' {
                line_idx += 1;
            }
            byte_idx += 1;
        }

        if n1 != n2 {
            println!("cmp: EOF alcanzado en uno de los archivos");
            let _ = portix_rt::syscall::sys_close(fd1 as i32);
            let _ = portix_rt::syscall::sys_close(fd2 as i32);
            return 1;
        }
    }

    let _ = portix_rt::syscall::sys_close(fd1 as i32);
    let _ = portix_rt::syscall::sys_close(fd2 as i32);
    0
}
