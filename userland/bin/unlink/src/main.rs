#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        println!("unlink: missing operand");
        return 1;
    }
    if argc > 2 {
        println!("unlink: extra operand");
        return 1;
    }

    let p = unsafe { *argv.add(1) };
    if p.is_null() {
        return 1;
    }

    let ret = portix_rt::syscall::sys_unlink(p);
    if ret < 0 {
        println!("unlink: cannot unlink");
        return 1;
    }
    0
}
