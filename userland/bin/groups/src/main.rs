#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::{println, getuid};

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut username = "";
    if argc > 1 && !argv.is_null() {
        let arg = unsafe { *argv.add(1) };
        if !arg.is_null() {
            let mut len = 0;
            while unsafe { *arg.add(len) } != 0 {
                len += 1;
            }
            let bytes = unsafe { core::slice::from_raw_parts(arg, len) };
            if let Ok(s) = core::str::from_utf8(bytes) {
                username = s;
            }
        }
    }

    if username.is_empty() {
        let uid = getuid();
        if uid == 0 {
            println!("root bin daemon sys adm disk wheel");
        } else {
            println!("user dialout audio video users");
        }
    } else if username == "root" {
        println!("root : root bin daemon sys adm disk wheel");
    } else if username == "user" {
        println!("user : user dialout audio video users");
    } else if username == "bin" {
        println!("bin : bin daemon");
    } else if username == "daemon" {
        println!("daemon : daemon bin");
    } else {
        println!("groups: '{}': no such user", username);
        return 1;
    }
    0
}
