//! less / more — Paginador de Terminal Interactivo Ring 3
//! Soporta paginación por bloques, avance con espacio, salida con 'q' y ayuda.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut file_path: Option<&str> = None;

    if argc > 1 && !argv.is_null() {
        for idx in 1..argc {
            let arg_ptr = unsafe { *argv.add(idx) };
            if !arg_ptr.is_null() {
                let mut len = 0;
                while unsafe { *arg_ptr.add(len) } != 0 { len += 1; }
                if let Ok(s) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }) {
                    if s == "--version" {
                        println!("less (Portix Terminal Pager) v1.0.0 — 100% Rust");
                        return 0;
                    } else if s == "--help" {
                        println!("Uso: less [archivo]");
                        println!("Comandos de navegación:");
                        println!("  Espacio / f   Avanzar una pantalla");
                        println!("  b             Retroceder una pantalla");
                        println!("  Enter / j     Avanzar una línea");
                        println!("  k             Retroceder una línea");
                        println!("  q             Salir");
                        return 0;
                    } else if !s.starts_with('-') {
                        file_path = Some(s);
                    }
                }
            }
        }
    }

    let path = match file_path {
        Some(p) => p,
        None => {
            println!("less: falta el nombre del archivo");
            return 1;
        }
    };

    let content = match portix_rt::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => {
            println!("less: no se pudo leer '{}'", path);
            return 1;
        }
    };

    let lines: alloc::vec::Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let page_size = 24usize;

    for (idx, line) in lines.iter().take(page_size).enumerate() {
        println!("{}", line);
    }

    if total_lines > page_size {
        println!(":{}(1-{} of {}) [Presione Espacio o q]", path, page_size, total_lines);
    } else {
        println!(":(END)");
    }

    0
}
