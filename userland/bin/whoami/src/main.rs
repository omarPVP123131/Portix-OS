#![no_std]
#![no_main]

use portix_rt::{println, getuid};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let uid = getuid();
    if uid == 0 {
        println!("root");
    } else if uid == 1000 {
        println!("user");
    } else {
        println!("uid:{}", uid);
    }
    0
}
