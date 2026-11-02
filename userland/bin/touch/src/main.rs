#![no_std]
#![no_main]

use portix_rt::println;
use portix_rt::syscall::{sys_open, sys_close};

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    if argc <= 1 {
        println!("Uso: touch <archivo>...");
        return 1;
    }

    for i in 1..argc {
        unsafe {
            let path_ptr = *argv.offset(i as isize);
            if !path_ptr.is_null() {
                // O_WRONLY = 0x0001, O_CREAT = 0x0040
                let fd = sys_open(path_ptr, 0x0041);
                if fd >= 0 {
                    sys_close(fd as i32);
                } else {
                    println!("touch: no se pudo crear el archivo");
                    return 1;
                }
            }
        }
    }

    0
}
