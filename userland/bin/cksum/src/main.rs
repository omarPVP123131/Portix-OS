#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;

fn compute_crctab() -> [u32; 256] {
    let mut table = [0u32; 256];
    for i in 0..256 {
        let mut c = (i as u32) << 24;
        for _ in 0..8 {
            if (c & 0x8000_0000) != 0 {
                c = (c << 1) ^ 0x04C1_1DB7;
            } else {
                c <<= 1;
            }
        }
        table[i] = c;
    }
    table
}

fn cksum_data(data: &[u8]) -> (u32, usize) {
    let crctab = compute_crctab();
    let mut crc: u32 = 0;
    for &b in data {
        crc = (crc << 8) ^ crctab[(((crc >> 24) ^ (b as u32)) & 0xFF) as usize];
    }
    let mut len = data.len();
    while len > 0 {
        let b = (len & 0xFF) as u32;
        crc = (crc << 8) ^ crctab[(((crc >> 24) ^ b) & 0xFF) as usize];
        len >>= 8;
    }
    (!crc, data.len())
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        let mut content = Vec::new();
        let mut buf = [0u8; 512];
        loop {
            let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
            if n <= 0 { break; }
            content.extend_from_slice(&buf[..n as usize]);
        }
        let (crc, len) = cksum_data(&content);
        println!("{} {}", crc, len);
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
                    let (crc, len) = cksum_data(&content);
                    println!("{} {} {}", crc, len, fname);
                } else {
                    println!("cksum: {}: No existe el fichero o el directorio", fname);
                }
            }
        }
    }
    0
}
