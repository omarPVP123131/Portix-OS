#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::{println, File, O_RDONLY};

#[no_mangle]
pub fn main(_argc: usize, _argv: *const *const u8) -> i32 {
    if let Ok(f) = File::open("/etc/passwd", O_RDONLY) {
        let mut buf = [0u8; 512];
        if let Ok(n) = f.read(&mut buf) {
            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                for line in s.lines() {
                    if let Some(user) = line.split(':').next() {
                        if !user.is_empty() {
                            println!("{:<8} tty1         2026-09-20 00:00 (:0)", user);
                            return 0;
                        }
                    }
                }
            }
        }
    }

    println!("root     tty1         2026-09-20 00:00 (:0)");
    0
}
