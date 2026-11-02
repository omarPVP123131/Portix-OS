//! `gcc` / `cc` en Portix OS: controlador (driver) del compilador real `portix-cc`.
//!
//! Historia: hasta el ciclo 44 este binario era un "binario de identidad" que
//! no compilaba nada — extraía las cadenas de `printf` del fuente y fabricaba
//! un ELF que sólo las imprimía. Eso era una simulación y se eliminó. Ahora
//! `gcc` traduce la línea de órdenes al estilo GNU (`gcc -O2 -Wall -o salida
//! fuente.c`) a la de PCC y lo ejecuta con `execve`, de modo que `make`,
//! guiones y usuarios acostumbrados a `gcc` usan el compilador real de Portix
//! sin cambiar sus hábitos. No se pretende ser GCC: `-v` lo dice claramente.
//!
//! Lo que NO existe se dice: C++ (`g++`/`c++`), `-c` (objetos separados),
//! `-l`/`-L` (bibliotecas) y varios fuentes en una sola invocación devuelven un
//! error explícito con código 1 en vez de fingir éxito.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::{println, syscall};

const PCC: &str = "/bin/portix-cc";
const VERSION_DRIVER: &str = "1.0.0";

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

fn existe(path: &str) -> bool {
    let mut p = Vec::with_capacity(path.len() + 1);
    p.extend_from_slice(path.as_bytes());
    p.push(0);
    let fd = syscall::sys_open(p.as_ptr(), 0);
    if fd < 0 { return false; }
    syscall::sys_close(fd as i32);
    true
}

fn nombre_programa(argc: i32, argv: *const *const u8) -> String {
    let prog = get_arg(0, argc, argv).unwrap_or(String::from("gcc"));
    match prog.rsplit('/').next() {
        Some(n) if !n.is_empty() => String::from(n),
        _ => prog,
    }
}

fn version(prog: &str) {
    println!("{} (Portix OS) controlador {} -> compilador real: portix-cc", prog, VERSION_DRIVER);
    println!("Objetivo: x86_64-portix-elf (ELF64 estatico, sin libc dinamica)");
    println!("No es GNU GCC: acepta la sintaxis de linea de ordenes de gcc y delega en /bin/portix-cc.");
    println!("Soportado: fuente.c unico, -o, -O*, -W*, -g, -std=* (ignorados), -E/-S no; C++ no.");
}

fn ayuda(prog: &str) {
    println!("Uso: {} [opciones] fuente.c [-o salida]", prog);
    println!("  -o <archivo>     binario ELF64 de salida (por defecto /tmp/a.out)");
    println!("  -v, --version    identidad del controlador y del compilador real");
    println!("  -O*, -W*, -g, -std=*, -D*  aceptadas e ignoradas (PCC no las necesita)");
    println!("  -c, -S, -E, -l*, -L*, -I*  NO soportadas: error explicito");
}

/// Ejecuta `portix-cc fuente -o salida` sustituyendo este proceso (`execve`).
fn ejecutar_pcc(src: &str, out: &str) -> i32 {
    let mut a0 = Vec::from(PCC.as_bytes()); a0.push(0);
    let mut a1 = Vec::from(src.as_bytes()); a1.push(0);
    let a2 = b"-o\0";
    let mut a3 = Vec::from(out.as_bytes()); a3.push(0);
    let argv: [*const u8; 5] = [a0.as_ptr(), a1.as_ptr(), a2.as_ptr(), a3.as_ptr(), core::ptr::null()];
    syscall::sys_execve(a0.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
    println!("gcc: no se pudo ejecutar {}", PCC);
    127
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let prog = nombre_programa(argc, argv);
    let es_cpp = prog.contains("g++") || prog.contains("c++");

    let mut out_file = String::from("/tmp/a.out");
    let mut fuentes: Vec<String> = Vec::new();
    let mut no_soportadas: Vec<String> = Vec::new();
    let mut pedir_version = false;
    let mut pedir_ayuda = false;

    let mut i = 1;
    while i < argc as usize {
        let Some(arg) = get_arg(i, argc, argv) else { i += 1; continue; };
        if arg == "-v" || arg == "--version" || arg == "-dumpversion" {
            pedir_version = true;
        } else if arg == "--help" || arg == "-h" {
            pedir_ayuda = true;
        } else if arg == "-o" {
            i += 1;
            match get_arg(i, argc, argv) {
                Some(o) => out_file = o,
                None => { println!("{}: la opcion -o requiere un argumento", prog); return 1; }
            }
        } else if arg == "-c" || arg == "-S" || arg == "-E" || arg == "-shared" || arg == "-static-pie"
            || arg.starts_with("-l") || arg.starts_with("-L") || arg.starts_with("-I") {
            no_soportadas.push(arg);
        } else if arg.starts_with('-') {
            // -O2, -Wall, -g, -std=c99, -DFOO, -fno-..., -m64, -static, -pedantic…: inofensivas.
        } else {
            fuentes.push(arg);
        }
        i += 1;
    }

    if pedir_version { version(&prog); return 0; }
    if pedir_ayuda || (fuentes.is_empty() && no_soportadas.is_empty()) {
        ayuda(&prog);
        return if pedir_ayuda { 0 } else { 1 };
    }
    if es_cpp {
        println!("{}: error: Portix OS no tiene compilador de C++; portix-cc solo compila C.", prog);
        println!("{}: (no se genera ningun binario; antes este comando fingia compilar)", prog);
        return 1;
    }
    if !no_soportadas.is_empty() {
        println!("{}: error: opciones no soportadas por portix-cc: {}", prog, no_soportadas.join(" "));
        println!("{}: PCC compila y enlaza un fuente C completo en un paso (sin objetos ni bibliotecas externas)", prog);
        return 1;
    }
    if fuentes.len() > 1 {
        println!("{}: error: portix-cc acepta un unico fichero fuente por invocacion ({} recibidos: {})", prog, fuentes.len(), fuentes.join(" "));
        return 1;
    }
    let src = &fuentes[0];
    if !src.ends_with(".c") {
        println!("{}: error: '{}' no es un fuente C (.c); ensamblador, objetos y C++ no estan soportados", prog, src);
        return 1;
    }
    if !existe(PCC) {
        println!("{}: error: falta {} (instale el paquete gcc-toolchain con `pkg install gcc-toolchain`)", prog, PCC);
        return 1;
    }
    ejecutar_pcc(src, &out_file)
}
