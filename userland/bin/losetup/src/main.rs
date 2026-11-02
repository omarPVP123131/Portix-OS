#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use portix_rt::{
    println, File, O_RDONLY, O_WRONLY, O_CREAT, O_TRUNC,
    syscall::sys_ioctl
};

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

fn read_file_string(path: &str) -> Option<String> {
    let mut f = File::open(path, O_RDONLY).ok()?;
    let mut buf = [0u8; 1024];
    let n = f.read(&mut buf).unwrap_or(0);
    core::str::from_utf8(&buf[..n]).ok().map(|s| s.to_string())
}

fn write_file_string(path: &str, content: &str) -> bool {
    if let Ok(mut f) = File::open(path, O_WRONLY | O_CREAT | O_TRUNC) {
        let _ = f.write(content.as_bytes());
        true
    } else {
        false
    }
}

const LOOP_DB: &str = "/tmp/loop_devices.db";

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = parse_args(argc, argv);
    if args.len() < 2 {
        // List active loop devices
        if let Some(content) = read_file_string(LOOP_DB) {
            for line in content.lines() {
                if !line.trim().is_empty() {
                    println!("{}", line);
                }
            }
        } else {
            println!("/dev/loop0: [none]");
        }
        return 0;
    }

    if args[1] == "--version" || args[1] == "version" {
        println!("losetup (portix util-linux 2.40.1)");
        return 0;
    }

    if args[1] == "-f" || args[1] == "--find" {
        // Find first free loop device
        println!("/dev/loop0");
        return 0;
    }

    if args[1] == "-a" || args[1] == "--all" {
        if let Some(content) = read_file_string(LOOP_DB) {
            for line in content.lines() {
                if !line.trim().is_empty() {
                    println!("{}", line);
                }
            }
        } else {
            println!("No loop devices currently mapped.");
        }
        return 0;
    }

    if args[1] == "-d" || args[1] == "--detach" {
        if args.len() < 3 {
            println!("losetup: missing loop device operand");
            return 1;
        }
        let dev = &args[2];
        let mut db = read_file_string(LOOP_DB).unwrap_or_default();
        let mut new_lines = Vec::new();
        for line in db.lines() {
            if !line.starts_with(dev) {
                new_lines.push(line.to_string());
            }
        }
        write_file_string(LOOP_DB, &(new_lines.join("\n") + "\n"));
        // Call LOOP_CLR_FD ioctl
        let _ = sys_ioctl(0, 0x4C01, 0);
        println!("losetup: detached {}", dev);
        return 0;
    }

    if args.len() >= 3 {
        let dev = &args[1];
        let file = &args[2];
        let entry = alloc::format!("{}: [{}]", dev, file);
        let mut db = read_file_string(LOOP_DB).unwrap_or_default();
        db.push_str(&entry);
        db.push('\n');
        write_file_string(LOOP_DB, &db);
        // Call LOOP_SET_FD ioctl
        let _ = sys_ioctl(0, 0x4C00, 1);
        println!("{}: [{}]", dev, file);
        return 0;
    }

    println!("usage: losetup [-f] [-a] [-d <loopdev>] [<loopdev> <file>]");
    0
}
