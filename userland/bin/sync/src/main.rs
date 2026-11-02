#![no_std]
#![no_main]

extern crate alloc;

#[no_mangle]
pub fn main(_argc: usize, _argv: *const *const u8) -> i32 {
    let _ = portix_rt::syscall::sys_sync();
    0
}
