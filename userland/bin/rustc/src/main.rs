//! `rustc` en Portix OS — aviso honesto.
//!
//! Portix OS todavia NO tiene un compilador de Rust nativo (rustc es un
//! programa de cientos de miles de lineas que depende de LLVM). Hasta el ciclo
//! 44 este binario fingia compilar: escribia un ELF fijo que imprimia "[Rust
//! Binario Oficial Estandar Ejecutado Exitosamente]". Esa simulacion se
//! elimino. Ahora `rustc` explica la situacion real y devuelve error, para
//! que ningun guion crea que ha compilado algo.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use portix_rt::println;

fn get_arg(idx: usize, argc: i32, argv: *const *const u8) -> Option<String> {
    if idx >= argc as usize || argv.is_null() { return None; }
    unsafe {
        let ptr = *argv.add(idx);
        if ptr.is_null() { return None; }
        let mut len = 0;
        while *ptr.add(len) != 0 { len += 1; }
        core::str::from_utf8(core::slice::from_raw_parts(ptr, len)).ok().map(String::from)
    }
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let a1 = get_arg(1, argc, argv);
    if matches!(a1.as_deref(), Some("-V") | Some("-v") | Some("--version")) {
        println!("rustc (Portix OS): compilador de Rust nativo NO DISPONIBLE");
        println!("Este binario solo informa; no compila. Portix no incluye rustc ni LLVM todavia.");
        return 1;
    }
    println!("rustc: error: Portix OS no tiene compilador de Rust nativo todavia.");
    println!("  - Para programas Rust destinados a Portix: compile en el anfitrion con");
    println!("    `cargo +nightly build --release --target x86_64-portix.json` y copie el ELF.");
    println!("  - Portix ejecuta directamente binarios Linux x86-64 estaticos (ABI compatible).");
    println!("  - Para C existe un compilador real dentro de Portix: `portix-cc fuente.c -o salida` (o `gcc`).");
    if let Some(src) = a1 {
        if !src.starts_with('-') { println!("  (no se ha generado ningun binario a partir de '{}')", src); }
    }
    1
}
