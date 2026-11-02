#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::{println, stat};

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut target = ".";
    if argc >= 2 && !argv.is_null() {
        let arg_ptr = unsafe { *argv.add(1) };
        if !arg_ptr.is_null() {
            let mut len = 0;
            while unsafe { *arg_ptr.add(len) } != 0 { len += 1; }
            if let Ok(s) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }) {
                if !s.is_empty() {
                    target = s;
                }
            }
        }
    }

    match stat(target) {
        Ok(st) => {
            let kb = if st.st_blocks > 0 {
                (st.st_blocks + 1) / 2
            } else {
                (st.st_size + 1023) / 1024
            };
            println!("{}\t{}", kb.max(4), target);
            0
        }
        Err(_) => {
            println!("du: cannot access '{}': No such file or directory", target);
            1
        }
    }
}
