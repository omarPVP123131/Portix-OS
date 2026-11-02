#![no_std]
#![no_main]

use portix_rt::{println, File, O_RDONLY, init_module};
use core::alloc::GlobalAlloc;

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut path = "/lib/modules/demo_mod.ko";
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

    println!("[INSMOD] Abriendo archivo de modulo '{}'...", path);
    let f = match File::open(path, O_RDONLY) {
        Ok(file) => file,
        Err(e) => {
            println!("insmod: no se pudo abrir '{}' (err={})", path, e);
            return 1;
        }
    };

    let st = match f.fstat() {
        Ok(s) => s,
        Err(e) => {
            println!("insmod: fstat fallo (err={})", e);
            return 1;
        }
    };

    let sz = st.st_size as usize;
    if sz == 0 || sz > 1024 * 1024 {
        println!("insmod: tamano de modulo invalido ({} bytes)", sz);
        return 1;
    }

    // Allocate memory for module
    let buf = unsafe {
        let ptr = portix_rt::allocator::PortixAllocator.alloc(core::alloc::Layout::from_size_align_unchecked(sz, 16));
        if ptr.is_null() {
            println!("insmod: out of memory");
            return 1;
        }
        core::slice::from_raw_parts_mut(ptr, sz)
    };

    let n = match f.read(buf) {
        Ok(bytes) => bytes,
        Err(e) => {
            println!("insmod: error de lectura (err={})", e);
            return 1;
        }
    };

    let filename = if let Some(pos) = path.rfind('/') {
        &path[pos + 1..]
    } else {
        path
    };
    let mod_name = if let Some(pos) = filename.rfind(".ko") {
        &filename[..pos]
    } else {
        filename
    };

    println!("[INSMOD] Leidos {} bytes. Inyectando modulo en Ring 0...", n);
    let ret = init_module(&buf[..n], mod_name);
    if ret < 0 {
        println!("insmod: error al cargar modulo (err={})", ret);
        return 1;
    }

    println!("[INSMOD] Modulo '{}' cargado con exito en Ring 0!", path);
    0
}
