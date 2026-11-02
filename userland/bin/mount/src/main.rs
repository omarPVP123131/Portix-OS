#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use portix_rt::{println, syscall::sys_mount};

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
    if args.len() == 1 {
        // List mounts
        println!("/dev/ata0 on / type fat32 (rw,relatime)");
        println!("ramfs on /tmp type ramfs (rw,relatime)");
        println!("devfs on /dev type devfs (rw,nosuid)");
        println!("procfs on /proc type procfs (rw,nosuid,nodev)");
        println!("shm on /dev/shm type tmpfs (rw,nosuid,nodev)");
        return 0;
    }

    if args[1] == "--version" || args[1] == "version" {
        println!("mount (portix-mount 2.40.1)");
        return 0;
    }

    let mut fs_type = "ramfs";
    let mut dev = "none";
    let mut dir = "";

    let mut i = 1;
    while i < args.len() {
        if args[i] == "-t" && i + 1 < args.len() {
            fs_type = &args[i + 1];
            i += 2;
        } else if dev == "none" && i + 1 < args.len() {
            dev = &args[i];
            dir = &args[i + 1];
            i += 2;
        } else if dir.is_empty() {
            dir = &args[i];
            i += 1;
        } else {
            i += 1;
        }
    }

    if dir.is_empty() {
        println!("usage: mount [-t type] device dir");
        return 1;
    }

    let mut dev_c = Vec::from(dev.as_bytes());
    dev_c.push(0);
    let mut dir_c = Vec::from(dir.as_bytes());
    dir_c.push(0);
    let mut type_c = Vec::from(fs_type.as_bytes());
    type_c.push(0);

    let ret = sys_mount(dev_c.as_ptr(), dir_c.as_ptr(), type_c.as_ptr(), 0, 0);
    if ret == 0 {
        println!("mounted {} on {} type {}", dev, dir, fs_type);
        0
    } else {
        println!("mount: mounting {} on {} failed: error {}", dev, dir, ret);
        1
    }
}
