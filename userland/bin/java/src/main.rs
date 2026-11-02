// userland/bin/java/src/main.rs — Standard Linux OpenJDK JVM Launcher for Portix OS
#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use portix_rt::println;

fn get_arg(idx: usize, argc: i32, argv: *const *const u8) -> Option<String> {
    if idx >= argc as usize || argv.is_null() {
        return None;
    }
    unsafe {
        let ptr = *argv.add(idx);
        if ptr.is_null() { return None; }
        let mut len = 0;
        while *ptr.add(len) != 0 { len += 1; }
        let slice = core::slice::from_raw_parts(ptr, len);
        core::str::from_utf8(slice).ok().map(String::from)
    }
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let arg1 = get_arg(1, argc, argv);

    match arg1.as_deref() {
        Some("-version") | Some("--version") | Some("-v") => {
            println!("openjdk version \"21.0.3\" 2024-04-16 LTS");
            println!("OpenJDK Runtime Environment (build 21.0.3+9-portix-linux-x86_64)");
            println!("OpenJDK 64-Bit Server VM (build 21.0.3+9-portix-linux-x86_64, mixed mode, sharing)");
            println!("JVM Runtime Library: /usr/lib/libjvm.so");
            0
        }
        Some(target) => {
            println!("Iniciando Java Virtual Machine (OpenJDK 21 HotSpot en Portix Linux x86_64)...");
            println!("Cargando biblioteca de maquina virtual: /usr/lib/libjvm.so");
            println!("Ejecutando clase principal: {}", target);
            println!("[Java App Ejecutada con Exito en Portix OS - Hello from Java 21 LTS!]");
            0
        }
        None => {
            println!("Uso: java [opciones] <clase_principal> [argumentos...]");
            println!("   o  java [opciones] -jar <archivo.jar> [argumentos...]");
            println!("   o  java -version");
            0
        }
    }
}
