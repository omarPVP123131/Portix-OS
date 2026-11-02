#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

fn factorize(mut n: u64) {
    let orig = n;
    let _ = portix_rt::print!("{}:", orig);

    if n <= 1 {
        println!(" {}", n);
        return;
    }

    while n % 2 == 0 {
        let _ = portix_rt::print!(" 2");
        n /= 2;
    }

    let mut d = 3u64;
    while d * d <= n {
        while n % d == 0 {
            let _ = portix_rt::print!(" {}", d);
            n /= d;
        }
        d += 2;
    }

    if n > 1 {
        let _ = portix_rt::print!(" {}", n);
    }
    println!();
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        println!("Uso: factor <numero> [numero...]");
        return 1;
    }

    let mut idx = 1;
    while idx < argc {
        let p = unsafe { *argv.add(idx) };
        if !p.is_null() {
            let mut len = 0;
            while unsafe { *p.add(len) } != 0 { len += 1; }
            if let Ok(s) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, len) }) {
                if let Ok(num) = s.parse::<u64>() {
                    factorize(num);
                } else {
                    println!("factor: '{}' no es un numero valido", s);
                }
            }
        }
        idx += 1;
    }

    0
}
