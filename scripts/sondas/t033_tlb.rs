// Sonda Rust (no modifica el software de terceros): reservas PROT_NONE y TLB.
// rustc -O -C target-feature=+crt-static -C strip=symbols scripts/sondas/t033_tlb.rs -o build/packages/t033_tlb
// `solo` y `hilos` se prueban también en Linux con `--anfitrion`.
use std::ffi::{c_char, c_void};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::{env, process, thread};
use std::time::{Duration, Instant};

const PAGINA: usize = 4096;
const PAGINAS: usize = 4096;
const TAM: usize = PAGINA * PAGINAS;
const MAP_PRIVATE: i32 = 0x02;
const MAP_FIXED: i32 = 0x10;
const MAP_ANONYMOUS: i32 = 0x20;
const ENOMEM: i32 = 12;

extern "C" {
    fn sched_setaffinity(pid: i32, size: usize, mask: *const u64) -> i32;
    fn syscall(num: i64, ...) -> i64;
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> i32;
    fn mprotect(addr: *mut c_void, len: usize, prot: i32) -> i32;
    fn __errno_location() -> *mut i32;
    fn open(path: *const c_char, flags: i32, ...) -> i32;
    fn read(fd: i32, buf: *mut c_void, size: usize) -> isize;
    fn close(fd: i32) -> i32;
    fn pthread_attr_init(attr: *mut PthreadAttr) -> i32;
    fn pthread_attr_destroy(attr: *mut PthreadAttr) -> i32;
    fn pthread_attr_setaffinity_np(attr: *mut PthreadAttr, size: usize, mask: *const u64) -> i32;
    fn pthread_create(tid: *mut usize, attr: *const PthreadAttr,
                      inicio: extern "C" fn(*mut c_void) -> *mut c_void, arg: *mut c_void) -> i32;
    fn pthread_join(tid: usize, valor: *mut *mut c_void) -> i32;
}

#[repr(C, align(8))]
struct PthreadAttr([u8; 56]);

static LISTO: AtomicBool = AtomicBool::new(false);
static PARAR: AtomicBool = AtomicBool::new(false);
static CPU_AP: AtomicU32 = AtomicU32::new(u32::MAX);

fn cpu_actual() -> Option<u32> {
    let mut cpu = u32::MAX;
    let rc = unsafe { syscall(309, &mut cpu as *mut u32, std::ptr::null_mut::<u32>(), 0usize) };
    if rc == 0 { Some(cpu) } else { None }
}

fn fijar_cpu0() -> bool {
    let mascara = 1u64;
    if unsafe { sched_setaffinity(0, 8, &mascara) } != 0 { return false; }
    let plazo = Instant::now() + Duration::from_secs(5);
    while Instant::now() < plazo {
        if cpu_actual() == Some(0) { return true; }
        thread::yield_now();
    }
    false
}

extern "C" fn ap(_: *mut c_void) -> *mut c_void {
    CPU_AP.store(cpu_actual().unwrap_or(u32::MAX), Ordering::SeqCst);
    LISTO.store(true, Ordering::SeqCst);
    while !PARAR.load(Ordering::SeqCst) { std::hint::spin_loop(); }
    std::ptr::null_mut()
}

fn crear_ap() -> Option<usize> {
    let mut attr = PthreadAttr([0; 56]);
    let mut tid = 0usize;
    let mask = 2u64;
    if unsafe { pthread_attr_init(&mut attr) } != 0 { return None; }
    let afin = unsafe { pthread_attr_setaffinity_np(&mut attr, 8, &mask) };
    let rc = if afin == 0 { unsafe { pthread_create(&mut tid, &attr, ap, std::ptr::null_mut()) } } else { afin };
    unsafe { pthread_attr_destroy(&mut attr); }
    if rc != 0 { return None; }
    let plazo = Instant::now() + Duration::from_secs(5);
    while !LISTO.load(Ordering::SeqCst) && Instant::now() < plazo { thread::yield_now(); }
    if !LISTO.load(Ordering::SeqCst) || CPU_AP.load(Ordering::SeqCst) != 1 {
        PARAR.store(true, Ordering::SeqCst);
        unsafe { pthread_join(tid, std::ptr::null_mut()); }
        return None;
    }
    Some(tid)
}

fn contador() -> Option<u64> {
    let mut buf = [0u8; 4096];
    let fd = unsafe { open(b"/proc/smp\0".as_ptr().cast(), 0) };
    if fd < 0 { return None; }
    let n = unsafe { read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    unsafe { close(fd); }
    if n <= 0 || n as usize > buf.len() { return None; }
    let texto = std::str::from_utf8(&buf[..n as usize]).ok()?;
    let mut lineas = texto.lines().filter(|linea| linea.starts_with("tlb_shootdowns: "));
    let linea = lineas.next()?;
    if lineas.next().is_some() { return None; }
    linea.split_whitespace().nth(1)?.parse().ok()
}

fn leer(anfitrion: bool) -> Option<u64> {
    if anfitrion { Some(0) } else { contador() }
}

fn informar(modo: &str, fase: &str, a: u64, b: u64, anfitrion: bool) -> bool {
    if b < a { return false; }
    if anfitrion {
        println!("[T033-TLB] modo={modo} fase={fase} paginas={PAGINAS} contador=omitido-en-anfitrion");
    } else {
        println!("[T033-TLB] modo={modo} fase={fase} paginas={PAGINAS} inicio={a} fin={b} delta={}", b - a);
    }
    let _ = io::stdout().flush();
    true
}

fn medir(modo: &str, anfitrion: bool) -> bool {
    let antes = match leer(anfitrion) { Some(n) => n, None => { eprintln!("[T033-ERROR] falta contador inicial"); return false; } };
    println!("[T033-TLB] modo={modo} comienza=mmap paginas={PAGINAS}");
    let _ = io::stdout().flush();
    let ptr = unsafe { mmap(std::ptr::null_mut(), TAM, 0, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0) };
    if ptr as isize == -1 || ptr.is_null() { eprintln!("[T033-ERROR] mmap falló"); return false; }
    let intermedio = match leer(anfitrion) { Some(n) => n, None => { eprintln!("[T033-ERROR] falta contador tras mmap"); unsafe { munmap(ptr, TAM); } return false; } };
    if !informar(modo, "mmap", antes, intermedio, anfitrion) { unsafe { munmap(ptr, TAM); } return false; }
    let offsets = [0, TAM / 2, TAM - PAGINA];
    for offset in offsets {
        let p = (ptr as *mut u8).wrapping_add(offset).cast::<c_void>();
        if unsafe { mprotect(p, PAGINA, 0) } != 0 { eprintln!("[T033-ERROR] reserva ausente antes de munmap"); unsafe { munmap(ptr, TAM); } return false; }
    }
    println!("[T033-TLB] modo={modo} comienza=munmap paginas={PAGINAS}");
    let _ = io::stdout().flush();
    if unsafe { munmap(ptr, TAM) } != 0 { eprintln!("[T033-ERROR] munmap falló"); return false; }
    let despues = match leer(anfitrion) { Some(n) => n, None => { eprintln!("[T033-ERROR] falta contador tras munmap"); return false; } };
    if !informar(modo, "munmap", intermedio, despues, anfitrion) { return false; }
    for offset in offsets {
        let p = (ptr as *mut u8).wrapping_add(offset).cast::<c_void>();
        let rc = unsafe { mprotect(p, PAGINA, 0) };
        let error = unsafe { *__errno_location() };
        if rc != -1 || error != ENOMEM { eprintln!("[T033-ERROR] página no retirada: rc={rc} errno={error}"); return false; }
    }
    let nuevo = unsafe { mmap(ptr, PAGINA, 3, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0) };
    if nuevo != ptr { eprintln!("[T033-ERROR] MAP_FIXED no recuperó dirección"); return false; }
    unsafe { (nuevo as *mut u8).write_volatile(0x5a); }
    let valor = unsafe { (nuevo as *const u8).read_volatile() };
    if unsafe { munmap(nuevo, PAGINA) } != 0 || valor != 0x5a { eprintln!("[T033-ERROR] reutilización de página falló"); return false; }
    println!("[T033-TLB] modo={modo} proteccion=3-antes-3-despues errno=12 reutilizacion=OK");
    true
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let modo = args.first().map(String::as_str).unwrap_or("");
    let anfitrion = args.get(1).map(String::as_str) == Some("--anfitrion");
    if (modo != "solo" && modo != "hilos") || args.len() > 2 || (args.len() == 2 && !anfitrion) {
        eprintln!("[T033-ERROR] uso: t033_tlb solo|hilos [--anfitrion]"); process::exit(2);
    }
    if !fijar_cpu0() { eprintln!("[T033-ERROR] líder no se fijó a CPU 0"); process::exit(3); }
    let tid = if modo == "hilos" {
        match crear_ap() { Some(t) => Some(t), None => { eprintln!("[T033-ERROR] AP no activo en CPU 1"); process::exit(4); } }
    } else { None };
    println!("[T033-TLB] listo modo={modo} cpu_lider=0 cpu_ap={} paginas={PAGINAS}", if tid.is_some() { "1" } else { "sin-hilo" });
    let _ = io::stdout().flush();
    let ok = medir(modo, anfitrion);
    if let Some(t) = tid {
        PARAR.store(true, Ordering::SeqCst);
        if unsafe { pthread_join(t, std::ptr::null_mut()) } != 0 { eprintln!("[T033-ERROR] pthread_join falló"); process::exit(5); }
    }
    if !ok { process::exit(1); }
    println!("[T033-TLB] PASS modo={modo}");
}
