#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::{println, getuid};

#[no_mangle]
pub fn main(_argc: usize, _argv: *const *const u8) -> i32 {
    let uid = getuid();
    if uid == 0 {
        println!("root");
    } else {
        println!("user");
    }
    0
}
