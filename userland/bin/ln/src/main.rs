#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use portix_rt::{println, unlink, fs::symlink};

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
        println!("usage: ln [-s] [-f] <target> <linkname>");
        return 1;
    }

    if args[1] == "--version" || args[1] == "version" {
        println!("ln (portix coreutils 9.4)");
        return 0;
    }

    let mut is_symbolic = false;
    let mut force = false;
    let mut positional = Vec::new();

    for arg in &args[1..] {
        if arg == "-s" || arg == "--symbolic" {
            is_symbolic = true;
        } else if arg == "-f" || arg == "--force" {
            force = true;
        } else if arg == "-sf" || arg == "-fs" {
            is_symbolic = true;
            force = true;
        } else {
            positional.push(arg.as_str());
        }
    }

    if positional.len() < 2 {
        println!("ln: missing destination file operand after '{}'", if positional.is_empty() { "" } else { positional[0] });
        return 1;
    }

    let target = positional[0];
    let linkname = positional[1];

    if force {
        let _ = unlink(linkname);
    }

    if is_symbolic {
        match symlink(target, linkname) {
            Ok(()) => 0,
            Err(e) => {
                println!("ln: failed to create symbolic link '{}': error {}", linkname, e);
                1
            }
        }
    } else {
        // Hard link fallback / invocation
        match symlink(target, linkname) {
            Ok(()) => 0,
            Err(e) => {
                println!("ln: failed to create link '{}': error {}", linkname, e);
                1
            }
        }
    }
}
