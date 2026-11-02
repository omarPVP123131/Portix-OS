#![no_std]
#![no_main]

use portix_rt::{println, print, File, O_RDONLY};

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    // [CICLO 47] Comportamiento POSIX: sin argumentos (o con "-") cat copia
    // su entrada estandar (fd 0) a la salida, lo que habilita tuberias reales.
    let mut path = "-";
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
                        path = s;
                    }
                }
            }
        }
    }

    if path == "-" {
        let mut buf = [0u8; 512];
        loop {
            let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
            if n <= 0 { break; }
            let mut off = 0usize;
            while off < n as usize {
                let w = portix_rt::syscall::sys_write(1, buf[off..].as_ptr(), n as usize - off);
                if w <= 0 { break; }
                off += w as usize;
            }
        }
        return 0;
    }

    let f = match File::open(path, O_RDONLY) {
        Ok(file) => file,
        Err(e) => {
            println!("cat: no se pudo abrir '{}' (err={})", path, e);
            return 1;
        }
    };

    let mut buf = [0u8; 512];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", s);
                }
            }
            Err(e) => {
                println!("cat: error leyendo '{}' (err={})", path, e);
                return 1;
            }
        }
    }
    0
}
