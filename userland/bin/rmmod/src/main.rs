#![no_std]
#![no_main]

use portix_rt::{println, delete_module};

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut name = "demo_mod.ko";
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
                        name = s;
                    }
                }
            }
        }
    }

    println!("[RMMOD] Descargando modulo '{}' del kernel...", name);
    let ret = delete_module(name, 0);
    if ret < 0 {
        println!("rmmod: fallo al descargar modulo '{}' (err={})", name, ret);
        return 1;
    }

    println!("[RMMOD] Modulo '{}' descargado con exito del Ring 0!", name);
    0
}
