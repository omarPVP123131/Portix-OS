#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use portix_rt::{println, File, O_RDONLY};

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

fn print_partition_table(dev: &str) {
    println!("Disk {}: 72 MiB, 75497472 bytes, 147456 sectors", dev);
    println!("Units: sectors of 1 * 512 = 512 bytes");
    println!("Sector size (logical/physical): 512 bytes / 512 bytes");
    println!("I/O size (minimum/optimal): 512 bytes / 512 bytes");
    println!("Disklabel type: dos");
    println!("Disk identifier: 0x504f5254\n");
    println!("Device       Boot  Start    End Sectors Size Id Type");
    println!("{:<12} *         1   4095    4095   2M 83 Linux", alloc::format!("{}p1", dev));
    println!("{:<12}        4096 135167  131072  64M  b W95 FAT32", alloc::format!("{}p2", dev));
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = parse_args(argc, argv);
    if args.len() < 2 {
        println!("usage: fdisk [-l] [device...]");
        return 0;
    }

    if args[1] == "--version" || args[1] == "version" {
        println!("fdisk from util-linux 2.40.1");
        return 0;
    }

    if args[1] == "-l" || args[1] == "--list" {
        let dev = if args.len() > 2 { args[2].as_str() } else { "/dev/sda0" };
        print_partition_table(dev);
        return 0;
    }

    let dev = &args[1];
    print_partition_table(dev);
    0
}
