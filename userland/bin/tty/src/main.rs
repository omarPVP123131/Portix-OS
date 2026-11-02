#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut silent = false;
    if argc > 1 && !argv.is_null() {
        let arg = unsafe { *argv.add(1) };
        if !arg.is_null() {
            let b0 = unsafe { *arg };
            let b1 = unsafe { *arg.add(1) };
            let b2 = unsafe { *arg.add(2) };
            if b0 == b'-' && b1 == b's' && b2 == 0 {
                silent = true;
            }
        }
    }

    if !silent {
        println!("/dev/tty0");
    }
    0
}
