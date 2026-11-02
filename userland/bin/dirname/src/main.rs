#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        println!("Uso: dirname <ruta>");
        return 1;
    }

    let arg_ptr = unsafe { *argv.add(1) };
    if arg_ptr.is_null() {
        return 1;
    }
    let mut len = 0;
    while unsafe { *arg_ptr.add(len) } != 0 {
        len += 1;
    }
    let path = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }) {
        Ok(s) => s,
        Err(_) => return 1,
    };

    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        println!("/");
        return 0;
    }

    match trimmed.rfind('/') {
        Some(0) => println!("/"),
        Some(idx) => println!("{}", &trimmed[..idx]),
        None => println!("."),
    }

    0
}
