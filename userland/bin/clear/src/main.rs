#![no_std]
#![no_main]

use portix_rt::print;

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    print!("\x1b[2J\x1b[H");
    0
}
