#![no_std]
#![no_main]

use portix_rt::{println, print, query_module};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut buf = [0u8; 2048];
    let ret = query_module(&mut buf);
    if ret < 0 {
        println!("lsmod: fallo al consultar modulos activos (err={})", ret);
        return 1;
    }
    let len = ret as usize;
    if len == 0 {
        println!("No hay modulos de kernel cargados.");
        return 0;
    }
    if let Ok(s) = core::str::from_utf8(&buf[..len]) {
        print!("{}", s);
    }
    0
}
