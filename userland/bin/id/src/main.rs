#![no_std]
#![no_main]

use portix_rt::{println, getuid, getgid};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let uid = getuid();
    let gid = getgid();
    let u_name = if uid == 0 { "root" } else if uid == 1000 { "user" } else { "unknown" };
    let g_name = if gid == 0 { "root" } else if gid == 1000 { "user" } else { "unknown" };

    println!("uid={}({}) gid={}({}) groups={}({})", uid, u_name, gid, g_name, gid, g_name);
    0
}
