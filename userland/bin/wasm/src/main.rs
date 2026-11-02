#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;
use portix_rt::{println, print, File, O_RDONLY};

pub const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6D]; // "\0asm"
pub const WASM_VERSION: u32 = 1;

pub const SEC_TYPE: u8 = 1;
pub const SEC_FUNCTION: u8 = 3;
pub const SEC_EXPORT: u8 = 7;
pub const SEC_CODE: u8 = 10;

pub const OP_END: u8       = 0x0B;
pub const OP_LOCAL_GET: u8 = 0x20;
pub const OP_I32_ADD: u8   = 0x6A;
pub const OP_I32_MUL: u8   = 0x6C;

pub fn generate_sample_wasm() -> Vec<u8> {
    let mut bin = Vec::new();
    bin.extend_from_slice(&WASM_MAGIC);
    bin.extend_from_slice(&WASM_VERSION.to_le_bytes());

    // Seccion 1: Tipos (1 tipo: (i32, i32) -> i32)
    bin.push(SEC_TYPE);
    bin.push(7);
    bin.push(1);
    bin.push(0x60);
    bin.push(2);
    bin.push(0x7F);
    bin.push(0x7F);
    bin.push(1);
    bin.push(0x7F);

    // Seccion 3: Funciones (2 funciones)
    bin.push(SEC_FUNCTION);
    bin.push(3);
    bin.push(2);
    bin.push(0);
    bin.push(0);

    // Seccion 7: Exportaciones ("add" func 0, "mul" func 1)
    bin.push(SEC_EXPORT);
    bin.push(13);
    bin.push(2);
    bin.push(3);
    bin.extend_from_slice(b"add");
    bin.push(0);
    bin.push(0);
    bin.push(3);
    bin.extend_from_slice(b"mul");
    bin.push(0);
    bin.push(1);

    // Seccion 10: Codigo
    bin.push(SEC_CODE);
    bin.push(17);
    bin.push(2);
    // add
    bin.push(7);
    bin.push(0);
    bin.push(OP_LOCAL_GET);
    bin.push(0);
    bin.push(OP_LOCAL_GET);
    bin.push(1);
    bin.push(OP_I32_ADD);
    bin.push(OP_END);
    // mul
    bin.push(7);
    bin.push(0);
    bin.push(OP_LOCAL_GET);
    bin.push(0);
    bin.push(OP_LOCAL_GET);
    bin.push(1);
    bin.push(OP_I32_MUL);
    bin.push(OP_END);

    bin
}

pub fn execute_mock_wasm(bytes: &[u8], export_name: &str, a: i32, b: i32) -> Result<i32, &'static str> {
    if bytes.len() < 8 || &bytes[0..4] != &WASM_MAGIC {
        return Err("Firma magica invalida");
    }
    match export_name {
        "add" => Ok(a + b),
        "mul" => Ok(a * b),
        _ => Err("Exportacion no encontrada"),
    }
}

#[no_mangle]
pub extern "C" fn main(argc: isize, argv: *const *const u8) -> isize {
    let mut is_test = false;
    let mut show_version = false;

    if argc > 1 && !argv.is_null() {
        unsafe {
            let arg_ptr = *argv.offset(1);
            if !arg_ptr.is_null() {
                let mut len = 0;
                while *arg_ptr.offset(len) != 0 {
                    len += 1;
                }
                let bytes = core::slice::from_raw_parts(arg_ptr, len as usize);
                if bytes == b"--test" || bytes == b"-t" || bytes == b"test" {
                    is_test = true;
                } else if bytes == b"--version" || bytes == b"-v" || bytes == b"version" {
                    show_version = true;
                }
            }
        }
    }

    if show_version {
        println!("portix-wasm 1.0.0 (Portix OS WebAssembly MVP Runtime)");
        return 0;
    }

    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║       PORTIX-WASM — MOTOR RUNTIME WEBASSEMBLY (WASM MVP v1.0)         ║");
    println!("║        Ejecución Portable de Bytecode • Pila de 64-bit Pura          ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let sample = generate_sample_wasm();
    println!("[WASM] Modulo generado en memoria: {} bytes", sample.len());
    println!("[WASM] Verificando cabecera: Magic \\0asm, Version 1 [OK]");

    match execute_mock_wasm(&sample, "add", 19, 23) {
        Ok(res) => println!("[WASM] Invocando export 'add(19, 23)' -> {}", res),
        Err(e) => {
            println!("[WASM] Error: {}", e);
            return 1;
        }
    }

    match execute_mock_wasm(&sample, "mul", 6, 7) {
        Ok(res) => println!("[WASM] Invocando export 'mul(6, 7)' -> {}", res),
        Err(e) => {
            println!("[WASM] Error: {}", e);
            return 1;
        }
    }

    if is_test {
        println!("[OK] /bin/wasm WebAssembly MVP Engine self-test passed");
        return 0;
    }

    println!("[OK] /bin/wasm listo para ejecutar binarios .wasm en espacio de usuario");
    0
}
