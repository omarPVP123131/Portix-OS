#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        println!("Uso: seq [PRIMERO [PASO]] ULTIMO");
        return 1;
    }

    let mut start = 1i64;
    let mut end = 1i64;

    if argc == 2 {
        let p = unsafe { *argv.add(1) };
        if !p.is_null() {
            let mut len = 0;
            while unsafe { *p.add(len) } != 0 { len += 1; }
            if let Ok(s) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, len) }) {
                if let Ok(v) = s.parse::<i64>() {
                    end = v;
                }
            }
        }
    } else if argc >= 3 {
        let p1 = unsafe { *argv.add(1) };
        let p2 = unsafe { *argv.add(2) };
        if !p1.is_null() && !p2.is_null() {
            let mut l1 = 0;
            let mut l2 = 0;
            while unsafe { *p1.add(l1) } != 0 { l1 += 1; }
            while unsafe { *p2.add(l2) } != 0 { l2 += 1; }
            if let (Ok(s1), Ok(s2)) = (
                core::str::from_utf8(unsafe { core::slice::from_raw_parts(p1, l1) }),
                core::str::from_utf8(unsafe { core::slice::from_raw_parts(p2, l2) }),
            ) {
                if let (Ok(v1), Ok(v2)) = (s1.parse::<i64>(), s2.parse::<i64>()) {
                    start = v1;
                    end = v2;
                }
            }
        }
    }

    // [CICLO 47-b] Forma POSIX de tres argumentos: seq PRIMERO PASO ULTIMO
    let mut step = 1i64;
    if argc >= 4 {
        let p3 = unsafe { *argv.add(3) };
        if !p3.is_null() {
            let mut l3 = 0; while unsafe { *p3.add(l3) } != 0 { l3 += 1; }
            if let Ok(s3) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p3, l3) }) {
                if let Ok(v3) = s3.parse::<i64>() { step = end; end = v3; }
            }
        }
    }
    if step == 0 { println!("seq: el paso no puede ser 0"); return 1; }
    let mut cur = start;
    if step > 0 {
        while cur <= end { println!("{}", cur); cur += step; }
    } else {
        while cur >= end { println!("{}", cur); cur += step; }
    }

    0
}
