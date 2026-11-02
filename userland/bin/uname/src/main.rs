#![no_std]
#![no_main]

use portix_rt::println;

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    println!("Portix 1.0.0 x86_64 GNU/Portix (100% Rust Bare-Metal)");
    0
}
