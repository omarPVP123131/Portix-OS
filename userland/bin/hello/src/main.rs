#![no_std]
#![no_main]

use portix_rt::{println, getpid};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    println!("Hello from PORTIX ring-3! (100% pure Rust)");
    let pid = getpid();
    println!("PID actual en ejecucion = {}", pid);
    0
}
