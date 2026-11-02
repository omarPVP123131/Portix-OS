#![no_std]
#![no_main]

use portix_rt::{println, mkdir};

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut target = "testdir";
    if argc >= 2 {
        unsafe {
            let ptr = *argv.offset(1);
            if !ptr.is_null() {
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                if let Ok(s) = core::str::from_utf8(core::slice::from_raw_parts(ptr, len)) {
                    if !s.is_empty() {
                        target = s;
                    }
                }
            }
        }
    }

    println!("[MKDIR] Creando directorio '{}'...", target);
    let ret = mkdir(target, 0o777);
    if ret < 0 {
        println!("mkdir: fallo al crear directorio '{}' (err={})", target, ret);
        return 1;
    }
    println!("[MKDIR] Directorio '{}' creado con exito!", target);
    0
}
