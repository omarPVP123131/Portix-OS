#![no_std]
#![no_main]

use portix_rt::{println, process};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    println!("Hello from Rust ring-3 on PORTIX!");
    let pid = process::getpid();
    println!("PID = {}", pid);
    0
}
