#![no_std]
#![no_main]

use portix_rt::{println, getpid, yield_now};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let pid = getpid();
    println!("==================================================");
    println!(" [INIT] PORTIX System Supervisor (PID {}) iniciado", pid);
    println!(" [INIT] Subsistemas de espacio de usuario activos ");
    println!("==================================================");

    loop {
        yield_now();
    }
}
