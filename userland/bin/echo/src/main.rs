#![no_std]
#![no_main]

use portix_rt::println;

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    if argc <= 1 {
        println!();
        return 0;
    }
    // [CICLO 47-b] POSIX: argumentos separados por UN espacio, sin espacio
    // final (antes se emitia "hola \n", lo que corrompia base64/cksum/paste).
    // Se admite `-n` (sin salto de linea final).
    let mut inicio = 1;
    let mut salto = true;
    unsafe {
        let p0 = *argv.offset(1);
        if !p0.is_null() && *p0 == b'-' && *p0.add(1) == b'n' && *p0.add(2) == 0 { salto = false; inicio = 2; }
    }
    for i in inicio..argc {
        unsafe {
            let ptr = *argv.offset(i as isize);
            if !ptr.is_null() {
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                if let Ok(s) = core::str::from_utf8(core::slice::from_raw_parts(ptr, len)) {
                    if i > inicio { portix_rt::print!(" "); }
                    portix_rt::print!("{}", s);
                }
            }
        }
    }
    if salto { println!(); }
    0
}
