#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use portix_rt::{println, syscall::sys_umount2};

fn parse_args(argc: i32, argv: *const *const u8) -> Vec<String> {
    let mut args = Vec::new();
    for i in 0..argc {
        unsafe {
            let ptr = *argv.add(i as usize);
            if ptr.is_null() { break; }
            let mut len = 0;
            while *ptr.add(len) != 0 { len += 1; }
            let bytes = core::slice::from_raw_parts(ptr, len);
            if let Ok(s) = core::str::from_utf8(bytes) {
                args.push(s.to_string());
            }
        }
    }
    args
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = parse_args(argc, argv);
    if args.len() < 2 {
        println!("usage: umount <directory | device>");
        return 1;
    }

    if args[1] == "--version" || args[1] == "version" {
        println!("umount (portix-mount 2.40.1)");
        return 0;
    }

    let target = &args[1];
    let mut tgt_c = Vec::from(target.as_bytes());
    tgt_c.push(0);

    let ret = sys_umount2(tgt_c.as_ptr(), 0);
    if ret == 0 {
        println!("umount: unmounted {}", target);
        0
    } else {
        println!("umount: {}: target is busy or not mounted (error {})", target, ret);
        1
    }
}
