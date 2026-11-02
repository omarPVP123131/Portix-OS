#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

fn parse_size(s: &str) -> Option<i64> {
    let mut num: i64 = 0;
    let mut multiplier: i64 = 1;
    let bytes = s.as_bytes();
    let mut end = bytes.len();

    if end > 0 {
        match bytes[end - 1] {
            b'k' | b'K' => { multiplier = 1024; end -= 1; }
            b'm' | b'M' => { multiplier = 1024 * 1024; end -= 1; }
            b'g' | b'G' => { multiplier = 1024 * 1024 * 1024; end -= 1; }
            _ => {}
        }
    }

    for &b in &bytes[..end] {
        if b >= b'0' && b <= b'9' {
            num = num.checked_mul(10)?.checked_add((b - b'0') as i64)?;
        } else {
            return None;
        }
    }

    num.checked_mul(multiplier)
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 3 || argv.is_null() {
        println!("truncate: missing operand");
        return 1;
    }

    let mut size: i64 = 0;
    let mut target_arg: Option<*const u8> = None;
    let mut i = 1;

    while i < argc {
        let p = unsafe { *argv.add(i) };
        if p.is_null() { i += 1; continue; }
        let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
        if let Ok(arg) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
            if arg == "-s" || arg == "--size" {
                i += 1;
                if i < argc {
                    let sp = unsafe { *argv.add(i) };
                    let mut sl = 0; while unsafe { *sp.add(sl) } != 0 { sl += 1; }
                    if let Ok(sstr) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(sp, sl) }) {
                        if let Some(sz) = parse_size(sstr) {
                            size = sz;
                        } else {
                            println!("truncate: invalid size");
                            return 1;
                        }
                    }
                }
            } else {
                target_arg = Some(p);
            }
        }
        i += 1;
    }

    if let Some(target) = target_arg {
        // Semántica GNU: si el fichero no existe (ENOENT), se crea (como
        // open(O_WRONLY|O_CREAT)) y después se ajusta el tamaño.
        let mut ret = portix_rt::syscall::sys_truncate(target, size);
        if ret == -2 {
            let fd = portix_rt::syscall::sys_open(target, 0o101); // O_WRONLY|O_CREAT
            if fd >= 0 {
                portix_rt::syscall::sys_close(fd as i32);
                ret = portix_rt::syscall::sys_truncate(target, size);
            }
        }
        if ret < 0 {
            println!("truncate: cannot truncate file");
            return 1;
        }
        0
    } else {
        println!("truncate: no file specified");
        1
    }
}
