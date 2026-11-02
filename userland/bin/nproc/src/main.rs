#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(_argc: usize, _argv: *const *const u8) -> i32 {
    println!("1");
    0
}
