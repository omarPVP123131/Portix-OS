#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut seconds = 1u64;

    if argc > 1 && !argv.is_null() {
        let arg_ptr = unsafe { *argv.add(1) };
        if !arg_ptr.is_null() {
            let mut len = 0;
            while unsafe { *arg_ptr.add(len) } != 0 {
                len += 1;
            }
            if let Ok(arg_str) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }) {
                if let Ok(parsed) = arg_str.parse::<u64>() {
                    seconds = parsed;
                }
            }
        }
    } else {
        println!("Uso: sleep <segundos>");
        return 1;
    }

    let req = portix_rt::Timespec {
        tv_sec: seconds as i64,
        tv_nsec: 0,
    };
    let mut rem = portix_rt::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };

    let _ = portix_rt::syscall::sys_nanosleep(&req, &mut rem);
    0
}
