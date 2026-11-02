#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

static DEFAULT_ENV: &[(&str, &str)] = &[
    ("PATH", "/bin:/usr/bin"),
    ("HOME", "/home/user"),
    ("USER", "root"),
    ("SHELL", "/bin/sh"),
    ("TERM", "xterm-256color"),
    ("OS", "portix"),
    ("ARCH", "x86_64"),
];

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        for &(k, v) in DEFAULT_ENV {
            println!("{}={}", k, v);
        }
        return 0;
    }

    for i in 1..argc {
        let p = unsafe { *argv.add(i) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(key) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                let mut found = false;
                for &(k, v) in DEFAULT_ENV {
                    if k == key {
                        println!("{}", v);
                        found = true;
                        break;
                    }
                }
                if !found {
                    return 1;
                }
            }
        }
    }
    0
}
