// T-035: ciclo de vida de páginas anónimas tras PROT_NONE. Sólo ABI Linux x86-64.
// Compilar: rustc -O -C target-feature=+crt-static -C strip=symbols
//           scripts/sondas/t035_memoria.rs -o build/packages/t035_memoria
// Los modos control-* omiten deliberadamente una operación y DEBEN salir 1.
use std::ffi::c_void;
use std::{env, process};

const PAGE: usize = 4096;
const PROT_NONE: i32 = 0;
const PROT_READ: i32 = 1;
const PROT_RW: i32 = 3;
const MAP_PRIVATE: i32 = 2;
const MAP_FIXED: i32 = 16;
const MAP_ANONYMOUS: i32 = 32;
const MADV_DONTNEED: i32 = 4;
const EFAULT: i32 = 14;
const ENOMEM: i32 = 12;

extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> i32;
    fn mprotect(addr: *mut c_void, len: usize, prot: i32) -> i32;
    fn madvise(addr: *mut c_void, len: usize, advice: i32) -> i32;
    fn syscall(num: i64, ...) -> i64;
    fn __errno_location() -> *mut i32;
    fn _exit(code: i32) -> !;
}

fn errno() -> i32 { unsafe { *__errno_location() } }
fn limpiar_errno() { unsafe { *__errno_location() = 0; } }
fn error(modo: &str, fase: &str, esperado: &str, observado: &str) -> bool {
    println!("[T035-ERROR] modo={modo} fase={fase} esperado={esperado} observado={observado}");
    false
}
fn fallo_operacion(modo: &str, fase: &str) -> bool {
    error(modo, fase, "rc:0", &format!("rc:-1,errno:{}", errno()))
}
fn reservar(modo: &str) -> Option<*mut c_void> {
    let p = unsafe { mmap(std::ptr::null_mut(), PAGE, PROT_RW, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0) };
    if p as usize == usize::MAX || p.is_null() {
        error(modo, "mmap", "direccion-valida", &format!("errno:{}", errno()));
        return None;
    }
    unsafe { (p as *mut u8).write_volatile(77); }
    Some(p)
}
fn proteger(modo: &str, p: *mut c_void, prot: i32) -> bool {
    if unsafe { mprotect(p, PAGE, prot) } != 0 { return fallo_operacion(modo, "mprotect"); }
    true
}
fn limpiar(p: *mut c_void) { unsafe { munmap(p, PAGE); } }
fn byte(p: *mut c_void) -> u8 { unsafe { (p as *const u8).read_volatile() } }

fn desmapear(modo: &str, control: bool) -> bool {
    let p = match reservar(modo) { Some(p) => p, None => return false };
    if !proteger(modo, p, PROT_NONE) { limpiar(p); return false; }
    if !control && unsafe { munmap(p, PAGE) } != 0 { return fallo_operacion(modo, "munmap"); }
    limpiar_errno();
    let rc = unsafe { mprotect(p, PAGE, PROT_READ) };
    let e = if rc == -1 { errno() } else { 0 };
    if rc != -1 || e != ENOMEM {
        limpiar(p);
        return error(modo, "desmapeada", "rc:-1,errno:12", &format!("rc:{rc},errno:{e}"));
    }
    println!("[T035] modo={modo} fase=desmapeada rc=-1 errno=12");
    let nuevo = unsafe { mmap(p, PAGE, PROT_RW, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0) };
    if nuevo != p { return error(modo, "remapeo", "misma-direccion", &format!("direccion:{:x}", nuevo as usize)); }
    let inicial = byte(p);
    if inicial != 0 { limpiar(p); return error(modo, "remapeo", "valor:0", &format!("valor:{inicial}")); }
    unsafe { (p as *mut u8).write_volatile(42); }
    println!("[T035] modo={modo} fase=remapeada inicial=0 nueva=42");
    limpiar(p);
    true
}

fn sustituir(modo: &str, control: bool) -> bool {
    let p = match reservar(modo) { Some(p) => p, None => return false };
    if !proteger(modo, p, PROT_NONE) { limpiar(p); return false; }
    if !control {
        let nuevo = unsafe { mmap(p, PAGE, PROT_RW, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0) };
        if nuevo != p { return error(modo, "map-fixed", "misma-direccion", &format!("direccion:{:x}", nuevo as usize)); }
    } else if !proteger(modo, p, PROT_READ) {
        limpiar(p); return false;
    }
    let inicial = byte(p);
    if inicial != 0 { limpiar(p); return error(modo, "map-fixed", "valor:0", &format!("valor:{inicial}")); }
    unsafe { (p as *mut u8).write_volatile(99); }
    println!("[T035] modo={modo} fase=map-fixed inicial=0 nueva=99");
    if unsafe { munmap(p, PAGE) } != 0 { return fallo_operacion(modo, "munmap"); }
    limpiar_errno();
    let rc = unsafe { mprotect(p, PAGE, PROT_READ) };
    let e = if rc == -1 { errno() } else { 0 };
    if rc != -1 || e != ENOMEM { return error(modo, "limpieza", "rc:-1,errno:12", &format!("rc:{rc},errno:{e}")); }
    println!("[T035] modo={modo} fase=limpieza rc=-1 errno=12");
    true
}

fn bifurcar(modo: &str, control: bool) -> bool {
    let p = match reservar(modo) { Some(p) => p, None => return false };
    if !proteger(modo, p, PROT_NONE) { limpiar(p); return false; }
    if control {
        if !proteger(modo, p, PROT_RW) { limpiar(p); return false; }
        unsafe { (p as *mut u8).write_volatile(99); }
    } else {
        let hijo = unsafe { syscall(57) }; // fork real
        if hijo == 0 {
            let ok = unsafe { mprotect(p, PAGE, PROT_RW) } == 0 && byte(p) == 77;
            if !ok { unsafe { _exit(71); } }
            unsafe { (p as *mut u8).write_volatile(99); _exit(0); }
        }
        if hijo < 0 { limpiar(p); return error(modo, "fork", "pid-positivo", &format!("errno:{}", errno())); }
        let mut estado: i32 = -1;
        let recibido = unsafe { syscall(61, hijo, &mut estado as *mut i32, 0i32, 0usize) }; // wait4
        if recibido != hijo || estado != 0 {
            limpiar(p);
            return error(modo, "hijo", "pid-correcto,estado:0", &format!("pid:{recibido},esperado:{hijo},estado:{estado}"));
        }
        println!("[T035] modo={modo} fase=hijo estado=0");
    }
    if !proteger(modo, p, PROT_READ) { limpiar(p); return false; }
    let padre = byte(p);
    if padre != 77 { limpiar(p); return error(modo, "aislamiento", "valor:77", &format!("valor:{padre}")); }
    println!("[T035] modo={modo} fase=aislamiento padre=77 hijo=99");
    limpiar(p);
    true
}

fn descartar(modo: &str, control: bool) -> bool {
    let p = match reservar(modo) { Some(p) => p, None => return false };
    if !proteger(modo, p, PROT_NONE) { limpiar(p); return false; }
    if !control && unsafe { madvise(p, PAGE, MADV_DONTNEED) } != 0 { limpiar(p); return fallo_operacion(modo, "madvise"); }
    if !proteger(modo, p, PROT_READ) { limpiar(p); return false; }
    let valor = byte(p);
    if valor != 0 { limpiar(p); return error(modo, "descarte", "valor:0", &format!("valor:{valor}")); }
    println!("[T035] modo={modo} fase=descarte valor=0");
    limpiar(p);
    true
}

fn bufer_usuario(modo: &str, control: bool, lectura: bool) -> bool {
    let p = match reservar(modo) { Some(p) => p, None => return false };
    if !control && !proteger(modo, p, if lectura { PROT_READ } else { PROT_NONE }) { limpiar(p); return false; }
    let mut fds = [-1i32; 2];
    if unsafe { syscall(22, fds.as_mut_ptr()) } != 0 { limpiar(p); return fallo_operacion(modo, "pipe"); }
    limpiar_errno();
    let rc = unsafe { syscall(1, fds[1], p, 1usize) }; // write lee la dirección de usuario
    let e = if rc == -1 { errno() } else { 0 };
    let valido = if lectura { rc == 1 } else { rc == -1 && e == EFAULT };
    if !valido {
        unsafe { syscall(3, fds[0]); syscall(3, fds[1]); }
        limpiar(p);
        return error(modo, "write", if lectura { "rc:1,errno:0" } else { "rc:-1,errno:14" }, &format!("rc:{rc},errno:{e}"));
    }
    if lectura {
        let mut valor = 0u8;
        let n = unsafe { syscall(0, fds[0], &mut valor as *mut u8, 1usize) };
        if n != 1 || valor != 77 {
            unsafe { syscall(3, fds[0]); syscall(3, fds[1]); }
            limpiar(p);
            return error(modo, "lectura-pipe", "rc:1,valor:77", &format!("rc:{n},valor:{valor}"));
        }
        println!("[T035] modo={modo} fase=write rc=1 valor=77");
    } else {
        println!("[T035] modo={modo} fase=write rc=-1 errno=14");
    }
    unsafe { syscall(3, fds[0]); syscall(3, fds[1]); }
    limpiar(p);
    true
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let modo = args.first().map(String::as_str).unwrap_or("");
    if args.len() != 1 || !matches!(modo, "control-munmap" | "munmap" | "control-fixed" | "fixed" |
        "control-fork" | "fork" | "control-madvise" | "madvise" | "control-usuario" | "usuario" | "usuario-lectura") {
        eprintln!("[T035-ERROR] modo inválido"); process::exit(2);
    }
    println!("[T035] modo={modo} inicio");
    let ok = match modo {
        "control-munmap" => desmapear(modo, true),
        "munmap" => desmapear(modo, false),
        "control-fixed" => sustituir(modo, true),
        "fixed" => sustituir(modo, false),
        "control-fork" => bifurcar(modo, true),
        "fork" => bifurcar(modo, false),
        "control-madvise" => descartar(modo, true),
        "madvise" => descartar(modo, false),
        "control-usuario" => bufer_usuario(modo, true, false),
        "usuario" => bufer_usuario(modo, false, false),
        "usuario-lectura" => bufer_usuario(modo, false, true),
        _ => false,
    };
    println!("[T035] modo={modo} fin");
    if !ok { process::exit(1); }
    println!("[T035] PASS modo={modo}");
}
