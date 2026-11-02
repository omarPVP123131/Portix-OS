#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::{println, syscall};

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

fn file_exists(path: &str) -> bool {
    let mut null_path = Vec::with_capacity(path.len() + 1);
    null_path.extend_from_slice(path.as_bytes());
    null_path.push(0);

    let fd = syscall::sys_open(null_path.as_ptr(), 0);
    if fd >= 0 {
        syscall::sys_close(fd as i32);
        true
    } else {
        false
    }
}

fn read_file_content(path: &str) -> Option<Vec<u8>> {
    let mut null_path = Vec::with_capacity(path.len() + 1);
    null_path.extend_from_slice(path.as_bytes());
    null_path.push(0);

    let fd = syscall::sys_open(null_path.as_ptr(), 0);
    if fd < 0 {
        return None;
    }

    let mut content = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let n = syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
        if n <= 0 { break; }
        content.extend_from_slice(&buf[..n as usize]);
    }
    syscall::sys_close(fd as i32);
    Some(content)
}

fn write_file_content(path: &str, data: &[u8]) -> bool {
    let mut null_path = Vec::with_capacity(path.len() + 1);
    null_path.extend_from_slice(path.as_bytes());
    null_path.push(0);

    // O_CREAT | O_WRONLY | O_TRUNC = 0x241
    let fd = syscall::sys_open(null_path.as_ptr(), 0x241);
    if fd < 0 {
        return false;
    }
    let res = syscall::sys_write(fd as i32, data.as_ptr(), data.len());
    syscall::sys_close(fd as i32);
    res == data.len() as i64
}


/// Ejecuta `/bin/curl -o <destino> <url>` (fork + execve + waitpid reales).
fn descargar(url: &str, destino: &str) -> i32 {
    use portix_rt::syscall::{sys_fork, sys_execve, sys_waitpid, sys_exit};
    let ruta = b"/bin/curl\0";
    let a0 = b"curl\0";
    let a1 = b"-o\0";
    let mut dst: Vec<u8> = destino.as_bytes().to_vec(); dst.push(0);
    let mut u: Vec<u8> = url.as_bytes().to_vec(); u.push(0);
    let argv: [*const u8; 5] = [a0.as_ptr(), a1.as_ptr(), dst.as_ptr(), u.as_ptr(), core::ptr::null()];
    let pid = sys_fork();
    if pid == 0 {
        sys_execve(ruta.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
        sys_exit(127);
    }
    if pid < 0 { return -1; }
    let mut status: i32 = 0;
    if sys_waitpid(pid as i32, &mut status as *mut i32, 0) != pid { return -1; }
    (status >> 8) & 0xff
}

fn hex32(h: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for b in h.iter() { s.push(HEX[(b >> 4) as usize] as char); s.push(HEX[(b & 15) as usize] as char); }
    s
}

/// [FRENTE B] Descarga REAL de un crate desde el repositorio HTTP del
/// anfitrion (https://10.0.2.2:8443/crates/, TLS 1.3 real). No hay datos incrustados: si la red o
/// el servidor fallan, el comando falla. La integridad se comprueba con
/// SHA-256 real contra `crates/index.txt` (tambien descargado).
fn download_crate_over_network(crate_name: &str) -> bool {
    println!("[CARGO] cargo-portix v6.1.0 - gestor de crates (HTTP/1.1 real sobre TCP)");
    println!("[CARGO] Solicitando crate: '{}'", crate_name);
    if crate_name.starts_with("https://") {
        println!("[CARGO-TLS-NO-IMPLEMENTADO] HTTPS no disponible todavia; use el repositorio http://");
        return false;
    }
    let base = "https://10.0.2.2:8443/crates/"; // TLS 1.3 real (portix_rt::tls13)
    let idx_path = "/tmp/crates-index.txt";
    let mut idx_url = String::from(base); idx_url.push_str("index.txt");
    if descargar(&idx_url, idx_path) != 0 {
        println!("[CARGO-FETCH-FAIL] no se pudo descargar el indice {}", idx_url);
        return false;
    }
    let idx = match read_file_content(idx_path) { Some(d) => d, None => { println!("[CARGO-FETCH-FAIL] indice ilegible"); return false; } };
    let idx_txt = String::from_utf8_lossy(&idx).into_owned();
    let mut esperado: Option<(usize, String)> = None;
    for line in idx_txt.lines() {
        let mut it = line.split_whitespace();
        if let (Some(n), Some(sz), Some(h)) = (it.next(), it.next(), it.next()) {
            if n == crate_name { esperado = Some((sz.parse().unwrap_or(0), String::from(h))); }
        }
    }
    let (size_esp, sha_esp) = match esperado {
        Some(e) => e,
        None => { println!("[CARGO-NOT-FOUND] '{}' no figura en el indice del repositorio", crate_name); return false; }
    };
    let mut url = String::from(base); url.push_str(crate_name); url.push_str(".crate");
    let mut tmp = String::from("/tmp/"); tmp.push_str(crate_name); tmp.push_str(".crate");
    if descargar(&url, &tmp) != 0 {
        println!("[CARGO-FETCH-FAIL] descarga fallida: {}", url);
        return false;
    }
    let data = match read_file_content(&tmp) { Some(d) => d, None => { println!("[CARGO-FETCH-FAIL] archivo descargado ilegible"); return false; } };
    println!("[CARGO] Recibidos {} bytes de {}", data.len(), url);
    let real = hex32(&portix_rt::tls::sha256(&data));
    if data.len() != size_esp || real != sha_esp {
        println!("[CARGO-SHA256-MISMATCH] '{}' rechazado: tamano={} (esperado {}) sha256={} (esperado {})", crate_name, data.len(), size_esp, real, sha_esp);
        return false;
    }
    println!("[+] SHA-256 verificado: {}", real);
    let mut lib_path_a = String::from("/usr/lib/lib"); lib_path_a.push_str(crate_name); lib_path_a.push_str(".a");
    let mut lib_path_rlib = String::from("/tmp/lib"); lib_path_rlib.push_str(crate_name); lib_path_rlib.push_str(".rlib");
    let ok1 = write_file_content(&lib_path_a, &data);
    let ok2 = write_file_content(&lib_path_rlib, &data);
    if ok1 && ok2 {
        println!("[CARGO] Crate instalado exitosamente en '{}' y '{}'", lib_path_a, lib_path_rlib);
        true
    } else {
        println!("[CARGO] Error al escribir el crate en disco");
        false
    }
}

/// `build`/`run`/`check`: no hay compilador de Rust dentro de Portix. Hasta el
/// ciclo 44 aqui se fabricaba un ELF fijo con la salida de una "calculadora" y
/// se imprimia como si se hubiera compilado `src/main.rs`; esa simulacion se
/// elimino. Se informa la situacion real y se devuelve 101 (codigo de fallo de
/// cargo) para que los guiones no crean que ha compilado nada.
fn sin_compilador(subcomando: &str) -> i32 {
    let manifiesto = if file_exists("Cargo.toml") { Some("Cargo.toml") } else { None };
    match manifiesto {
        Some(m) => println!("cargo-portix {}: manifiesto {} encontrado, pero Portix OS no tiene compilador de Rust nativo.", subcomando, m),
        None => println!("cargo-portix {}: no hay Cargo.toml en el directorio actual y Portix OS no tiene compilador de Rust nativo.", subcomando),
    }
    println!("  Compile en el anfitrion (`cargo +nightly build --release --target x86_64-portix.json`) y copie el ELF,");
    println!("  o use el compilador real de C incluido: `portix-cc fuente.c -o salida` (tambien `gcc`, `make`).");
    println!("  Lo que SI es real aqui: `cargo-portix fetch|install <crate>` (HTTP/1.1 + SHA-256 verificado).");
    101
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let cmd = get_arg(1, argc, argv);
    let arg2 = get_arg(2, argc, argv);

    match cmd.as_deref() {
        Some("run") | Some("build") | Some("release") | Some("check") | Some("clean") | Some("test") => {
            sin_compilador(cmd.as_deref().unwrap_or(""))
        }
        Some("add") | Some("install") | Some("fetch") => {
            let pkg_name = arg2.as_deref().unwrap_or("math");
            if download_crate_over_network(pkg_name) {
                0
            } else {
                1
            }
        }
        Some("list") => {
            println!("Crates descargados en /usr/lib (solo los que existen de verdad en disco):");
            let mut n = 0;
            for nombre in ["libserde.rlib", "libmath.rlib", "libmath.a", "libserde.a"] {
                let mut ruta = String::from("/usr/lib/"); ruta.push_str(nombre);
                if let Some(d) = read_file_content(&ruta) { println!("  - {} ({} B)", ruta, d.len()); n += 1; }
            }
            if n == 0 { println!("  (ninguno; use `cargo-portix install <crate>`)"); }
            0
        }
        Some("-v") | Some("--version") => {
            println!("cargo-portix 6.1.0 (Portix OS): gestor de crates por HTTP/1.1 con SHA-256 verificado");
            println!("Sin compilador de Rust nativo: build/run/check no compilan (devuelven 101).");
            0
        }
        _ => {
            println!("Uso: cargo-portix <comando> [argumentos]");
            println!("  fetch|install|add <crate>  Descarga el crate del repositorio HTTP y verifica su SHA-256 (real)");
            println!("  list                        Crates presentes en /usr/lib");
            println!("  build|run|check|clean       NO disponibles: Portix no tiene compilador de Rust nativo");
            println!("  --version                   Identidad de esta herramienta");
            1
        }
    }
}
