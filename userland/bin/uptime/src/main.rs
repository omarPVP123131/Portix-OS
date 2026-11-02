#![no_std]
#![no_main]

use portix_rt::{println, uptime};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let t = uptime();
    let secs = t / 100;
    let mins = secs / 60;
    let hrs = mins / 60;
    println!("PORTIX Uptime: {} hrs, {} min, {} sec (ticks: {})", hrs, mins % 60, secs % 60, t);
    0
}
