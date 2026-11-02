#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

fn parse_octal(s: &str) -> Option<u32> {
    let mut val: u32 = 0;
    if s.is_empty() { return None; }
    for b in s.bytes() {
        if b < b'0' || b > b'7' { return None; }
        val = (val << 3) | ((b - b'0') as u32);
    }
    Some(val)
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 3 || argv.is_null() {
        println!("chmod: missing operand");
        return 1;
    }

    let mode_ptr = unsafe { *argv.add(1) };
    if mode_ptr.is_null() { return 1; }
    let mut len = 0;
    while unsafe { *mode_ptr.add(len) } != 0 { len += 1; }
    let mode_str = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(mode_ptr, len) }) {
        Ok(s) => s,
        Err(_) => { println!("chmod: invalid mode"); return 1; }
    };

    let mode = match parse_octal(mode_str) {
        Some(m) => m,
        None => {
            if mode_str == "+x" || mode_str == "a+x" {
                0o755
            } else if mode_str == "-x" {
                0o644
            } else if mode_str == "+w" {
                0o666
            } else if mode_str == "-w" {
                0o444
            } else {
                println!("chmod: invalid mode: '{}'", mode_str);
                return 1;
            }
        }
    };

    let mut exit_code = 0;
    for i in 2..argc {
        let file_ptr = unsafe { *argv.add(i) };
        if file_ptr.is_null() { continue; }
        let ret = portix_rt::syscall::sys_chmod(file_ptr, mode);
        if ret < 0 {
            let mut flen = 0;
            while unsafe { *file_ptr.add(flen) } != 0 { flen += 1; }
            let fname = core::str::from_utf8(unsafe { core::slice::from_raw_parts(file_ptr, flen) }).unwrap_or("file");
            println!("chmod: cannot access '{}'", fname);
            exit_code = 1;
        }
    }

    exit_code
}
