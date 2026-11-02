#![no_std]
#![no_main]

extern crate portix_rt;

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    1
}
