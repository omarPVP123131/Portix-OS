// T-033: observaciones reales desde un hilo fijado a la segunda CPU.
// La prueba no afirma haber identificado marcos físicos ni haber agotado las carreras SMP.
// Compilación: rustc -O -C target-feature=+crt-static -C strip=symbols
//              scripts/sondas/t033_smp.rs -o build/packages/t033_smp
// Los controles omiten DELIBERADAMENTE munmap/mprotect y deben salir con 1.
use std::ffi::c_void;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use std::{env, process, thread};

const PAGINA: usize = 4096;
const PRIVADO: i32 = 0x02;
const FIJO: i32 = 0x10;
const ANONIMO: i32 = 0x20;
const LECTURA_ESCRITURA: i32 = 3;
const SIN_ACCESO: i32 = 0;
const SIGSEGV: i32 = 11;
const MAPERR: i32 = 1;
const ACCERR: i32 = 2;
const FALLO: u32 = 256;

// La instrucción de lectura y su continuación tienen símbolos inequívocos.
// El manejador sólo recupera exactamente esa lectura, y rechaza cualquier otra
// excepción: una SIGSEGV ajena jamás se puede convertir en un éxito de prueba.
core::arch::global_asm!(
    ".text",
    ".global t033_smp_cargar",
    "t033_smp_cargar:",
    ".global t033_smp_instruccion",
    "t033_smp_instruccion:",
    "movzx eax, byte ptr [rdi]",
    "ret",
    ".global t033_smp_continuacion",
    "t033_smp_continuacion:",
    "mov eax, 256",
    "ret",
    ".global t033_smp_restaurador",
    "t033_smp_restaurador:",
    "mov rax, 15",
    "syscall",
    "ud2",
);

extern "C" {
    fn t033_smp_cargar(dir: *const u8) -> u32;
    static t033_smp_instruccion: u8;
    static t033_smp_continuacion: u8;
    static t033_smp_restaurador: u8;
    fn syscall(num: i64, ...) -> i64;
    fn _exit(code: i32) -> !;
    fn sched_setaffinity(pid: i32, size: usize, mask: *const u64) -> i32;
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> i32;
    fn mprotect(addr: *mut c_void, len: usize, prot: i32) -> i32;
    fn pthread_attr_init(attr: *mut PthreadAttr) -> i32;
    fn pthread_attr_destroy(attr: *mut PthreadAttr) -> i32;
    fn pthread_attr_setaffinity_np(attr: *mut PthreadAttr, size: usize, mask: *const u64) -> i32;
    fn pthread_create(tid: *mut usize, attr: *const PthreadAttr,
                      inicio: extern "C" fn(*mut c_void) -> *mut c_void, arg: *mut c_void) -> i32;
    fn pthread_join(tid: usize, valor: *mut *mut c_void) -> i32;
}

#[repr(C, align(8))]
struct PthreadAttr([u8; 56]);

// ABI rt_sigaction de Linux x86-64. Portix usa SA_SIGINFO y un trampolín
// propio; Linux usa el restaurador indicado. No se instala SIG_IGN.
#[repr(C)]
struct AccionKernel {
    manejador: usize,
    banderas: u64,
    restaurador: usize,
    mascara: u64,
}

static LISTO: AtomicBool = AtomicBool::new(false);
static PARAR: AtomicBool = AtomicBool::new(false);
static CPU_AP_INICIAL: AtomicU32 = AtomicU32::new(u32::MAX);
static CPU_AP_FINAL: AtomicU32 = AtomicU32::new(u32::MAX);
static ORDEN: AtomicU32 = AtomicU32::new(0);
static ACUSE: AtomicU32 = AtomicU32::new(0);
static DIRECCION: AtomicUsize = AtomicUsize::new(0);
static DIRECCION_ESPERADA: AtomicUsize = AtomicUsize::new(0);
static ACCESO_ACTIVO: AtomicBool = AtomicBool::new(false);
static FALTAS: AtomicU32 = AtomicU32::new(0);
static CODIGO_FALLO: AtomicI32 = AtomicI32::new(0);
static VALOR: AtomicU32 = AtomicU32::new(0);
static DELTA_FALTAS: AtomicU32 = AtomicU32::new(0);
static CODIGO_OBSERVADO: AtomicI32 = AtomicI32::new(0);

// offsets ABI x86-64: si_code=8, si_addr=16, uc_mcontext.gregs[REG_RIP]=168.
// Las operaciones compartidas son atómicas nativas y no se usa ninguna función
// no segura para manejadores de señales. _exit rechaza una excepción inesperada.
unsafe extern "C" fn fallo_segv(sig: i32, info: *mut c_void, contexto: *mut c_void) {
    if sig != SIGSEGV || info.is_null() || contexto.is_null() || !ACCESO_ACTIVO.load(Ordering::SeqCst) {
        _exit(90);
    }
    let si = info as *const u8;
    let codigo = (si.add(8) as *const i32).read_volatile();
    let direccion = (si.add(16) as *const usize).read_volatile();
    let rip = (contexto as *mut u8).add(168) as *mut usize;
    if direccion != DIRECCION_ESPERADA.load(Ordering::SeqCst)
        || rip.read_volatile() != std::ptr::addr_of!(t033_smp_instruccion) as usize {
        _exit(91);
    }
    CODIGO_FALLO.store(codigo, Ordering::SeqCst);
    FALTAS.fetch_add(1, Ordering::SeqCst);
    rip.write_volatile(std::ptr::addr_of!(t033_smp_continuacion) as usize);
}

fn instalar_manejador() -> bool {
    const SA_SIGINFO: u64 = 0x0000_0004;
    const SA_RESTORER: u64 = 0x0400_0000;
    let accion = AccionKernel {
        manejador: fallo_segv as *const () as usize,
        banderas: SA_SIGINFO | SA_RESTORER,
        restaurador: std::ptr::addr_of!(t033_smp_restaurador) as usize,
        mascara: 0,
    };
    unsafe { syscall(13, SIGSEGV, &accion as *const AccionKernel, std::ptr::null_mut::<c_void>(), 8usize) == 0 }
}

fn cpu_actual() -> Option<u32> {
    let mut cpu = u32::MAX;
    let rc = unsafe { syscall(309, &mut cpu as *mut u32, std::ptr::null_mut::<u32>(), 0usize) };
    if rc == 0 { Some(cpu) } else { None }
}

fn fijar_lider() -> bool {
    let mascara = 1u64;
    if unsafe { sched_setaffinity(0, 8, &mascara) } != 0 { return false; }
    let plazo = Instant::now() + Duration::from_secs(5);
    while Instant::now() < plazo {
        if cpu_actual() == Some(0) { return true; }
        thread::yield_now();
    }
    false
}

extern "C" fn trabajador(_: *mut c_void) -> *mut c_void {
    CPU_AP_INICIAL.store(cpu_actual().unwrap_or(u32::MAX), Ordering::SeqCst);
    LISTO.store(true, Ordering::SeqCst);
    let mut anterior = 0;
    loop {
        if PARAR.load(Ordering::SeqCst) { break; }
        let orden = ORDEN.load(Ordering::SeqCst);
        if orden == anterior { std::hint::spin_loop(); continue; }
        let direccion = DIRECCION.load(Ordering::SeqCst);
        DIRECCION_ESPERADA.store(direccion, Ordering::SeqCst);
        CODIGO_FALLO.store(0, Ordering::SeqCst);
        let antes = FALTAS.load(Ordering::SeqCst);
        ACCESO_ACTIVO.store(true, Ordering::SeqCst);
        let valor = unsafe { t033_smp_cargar(direccion as *const u8) };
        ACCESO_ACTIVO.store(false, Ordering::SeqCst);
        let despues = FALTAS.load(Ordering::SeqCst);
        VALOR.store(valor, Ordering::SeqCst);
        DELTA_FALTAS.store(despues.wrapping_sub(antes), Ordering::SeqCst);
        CODIGO_OBSERVADO.store(CODIGO_FALLO.load(Ordering::SeqCst), Ordering::SeqCst);
        anterior = orden;
        ACUSE.store(orden, Ordering::SeqCst);
    }
    CPU_AP_FINAL.store(cpu_actual().unwrap_or(u32::MAX), Ordering::SeqCst);
    std::ptr::null_mut()
}

fn crear_trabajador() -> Option<usize> {
    let mut attr = PthreadAttr([0; 56]);
    let mut tid = 0usize;
    let mascara = 2u64;
    if unsafe { pthread_attr_init(&mut attr) } != 0 { return None; }
    let afin = unsafe { pthread_attr_setaffinity_np(&mut attr, 8, &mascara) };
    let rc = if afin == 0 { unsafe { pthread_create(&mut tid, &attr, trabajador, std::ptr::null_mut()) } } else { afin };
    unsafe { pthread_attr_destroy(&mut attr); }
    if rc != 0 { return None; }
    let plazo = Instant::now() + Duration::from_secs(5);
    while !LISTO.load(Ordering::SeqCst) && Instant::now() < plazo { thread::yield_now(); }
    if !LISTO.load(Ordering::SeqCst) || CPU_AP_INICIAL.load(Ordering::SeqCst) != 1 {
        PARAR.store(true, Ordering::SeqCst);
        unsafe { pthread_join(tid, std::ptr::null_mut()); }
        return None;
    }
    Some(tid)
}

#[derive(Clone, Copy)]
struct Lectura { valor: u32, codigo: i32, faltas: u32 }

fn leer_ap(dir: *mut c_void) -> Result<Lectura, &'static str> {
    let anterior = ORDEN.load(Ordering::SeqCst);
    if anterior == u32::MAX { return Err("demasiadas órdenes"); }
    DIRECCION.store(dir as usize, Ordering::SeqCst);
    let nueva = anterior + 1;
    ORDEN.store(nueva, Ordering::SeqCst);
    let plazo = Instant::now() + Duration::from_secs(5);
    while ACUSE.load(Ordering::SeqCst) != nueva {
        if Instant::now() >= plazo { return Err("trabajador sin respuesta"); }
        std::hint::spin_loop();
    }
    Ok(Lectura {
        valor: VALOR.load(Ordering::SeqCst),
        codigo: CODIGO_OBSERVADO.load(Ordering::SeqCst),
        faltas: DELTA_FALTAS.load(Ordering::SeqCst),
    })
}

fn valor_ap(modo: &str, fase: &str, dir: *mut c_void, esperado: u32) -> bool {
    match leer_ap(dir) {
        Ok(r) if r.valor == esperado && r.codigo == 0 && r.faltas == 0 => true,
        Ok(r) => {
            eprintln!("[T033-SMP-ERROR] modo={modo} fase={fase} esperado=valor:{esperado} observado=valor:{},codigo:{},faltas:{}", r.valor, r.codigo, r.faltas);
            false
        }
        Err(e) => { eprintln!("[T033-SMP-ERROR] modo={modo} fase={fase} {e}"); false }
    }
}

fn fallo_ap(modo: &str, fase: &str, dir: *mut c_void, esperado: i32) -> bool {
    match leer_ap(dir) {
        Ok(r) if r.valor == FALLO && r.codigo == esperado && r.faltas == 1 => true,
        Ok(r) => {
            eprintln!("[T033-SMP-ERROR] modo={modo} fase={fase} esperado=senal:11,codigo:{esperado} observado=valor:{},codigo:{},faltas:{}", r.valor, r.codigo, r.faltas);
            false
        }
        Err(e) => { eprintln!("[T033-SMP-ERROR] modo={modo} fase={fase} {e}"); false }
    }
}

fn reservar() -> Option<*mut c_void> {
    let p = unsafe { mmap(std::ptr::null_mut(), PAGINA, LECTURA_ESCRITURA, PRIVADO | ANONIMO, -1, 0) };
    if p as isize == -1 || p.is_null() { None } else { Some(p) }
}

fn remapear(dir: *mut c_void) -> bool {
    unsafe { mmap(dir, PAGINA, LECTURA_ESCRITURA, PRIVADO | ANONIMO | FIJO, -1, 0) == dir }
}

unsafe fn poner(dir: *mut c_void, valor: u8) { (dir as *mut u8).write_volatile(valor); }
unsafe fn obtener(dir: *mut c_void) -> u8 { (dir as *const u8).read_volatile() }

fn probar_munmap(modo: &str, control: bool) -> bool {
    let (a, b, c) = match (reservar(), reservar(), reservar()) {
        (Some(a), Some(b), Some(c)) if a != b && a != c && b != c => (a, b, c),
        _ => { eprintln!("[T033-SMP-ERROR] modo={modo} reservas distintas fallaron"); return false; }
    };
    unsafe { poner(a, 49); }
    if !valor_ap(modo, "presente", a, 49) { return false; }
    println!("[T033-SMP] modo={modo} fase=presente valor=49");
    let _ = io::stdout().flush();
    if !control && unsafe { munmap(a, PAGINA) } != 0 {
        eprintln!("[T033-SMP-ERROR] modo={modo} munmap inicial falló"); return false;
    }
    // B y C ya estaban reservadas en direcciones distintas pero sin tocar.
    // Al tocar B tras munmap existe una oportunidad de reutilizar el marco;
    // no se presupone ni se afirma que el asignador haya elegido ese marco.
    unsafe { poner(b, 183); }
    if !fallo_ap(modo, "desmapeada", a, MAPERR) { return false; }
    println!("[T033-SMP] modo={modo} fase=desmapeada senal=11 codigo=1 direccion=correcta faltas=1");
    if !remapear(a) { eprintln!("[T033-SMP-ERROR] modo={modo} MAP_FIXED inicial falló"); return false; }
    if !valor_ap(modo, "remapeada-cero", a, 0) { return false; }
    unsafe { poner(a, 90); }
    if !valor_ap(modo, "remapeada-nueva", a, 90) || unsafe { obtener(b) } != 183 {
        eprintln!("[T033-SMP-ERROR] modo={modo} aislamiento de B falló"); return false;
    }
    println!("[T033-SMP] modo={modo} fase=remapeada inicial=0 nueva=90 candidata=183 aislada=si");
    let _ = io::stdout().flush();
    // La última lectura de A calienta de nuevo su traducción en CPU 1. Ahora
    // CPU 0 desmapea y vuelve a mapear ANTES de que CPU 1 acceda a A: no se
    // intercala el sondeo de SIGSEGV, que podría por sí mismo alterar la TLB.
    if unsafe { munmap(a, PAGINA) } != 0 { eprintln!("[T033-SMP-ERROR] modo={modo} segundo munmap falló"); return false; }
    unsafe { poner(c, 195); }
    if !remapear(a) { eprintln!("[T033-SMP-ERROR] modo={modo} MAP_FIXED directo falló"); return false; }
    if !valor_ap(modo, "directa-cero", a, 0) { return false; }
    unsafe { poner(a, 109); }
    if !valor_ap(modo, "directa-nueva", a, 109) || unsafe { obtener(b) } != 183 || unsafe { obtener(c) } != 195 {
        eprintln!("[T033-SMP-ERROR] modo={modo} aislamiento de B/C falló"); return false;
    }
    println!("[T033-SMP] modo={modo} fase=remapeo-directo inicial=0 nueva=109 candidatas=183,195 aisladas=si");
    if unsafe { munmap(a, PAGINA) } != 0 || unsafe { munmap(b, PAGINA) } != 0 || unsafe { munmap(c, PAGINA) } != 0 {
        eprintln!("[T033-SMP-ERROR] modo={modo} limpieza falló"); return false;
    }
    true
}

fn probar_proteccion(modo: &str, control: bool) -> bool {
    let a = match reservar() { Some(p) => p, None => { eprintln!("[T033-SMP-ERROR] modo={modo} reserva falló"); return false; } };
    unsafe { poner(a, 77); }
    if !valor_ap(modo, "presente", a, 77) { return false; }
    println!("[T033-SMP] modo={modo} fase=presente valor=77");
    let _ = io::stdout().flush();
    if !control && unsafe { mprotect(a, PAGINA, SIN_ACCESO) } != 0 {
        eprintln!("[T033-SMP-ERROR] modo={modo} mprotect PROT_NONE falló"); return false;
    }
    if !fallo_ap(modo, "protegida", a, ACCERR) { return false; }
    println!("[T033-SMP] modo={modo} fase=protegida senal=11 codigo=2 direccion=correcta faltas=1");
    if unsafe { mprotect(a, PAGINA, LECTURA_ESCRITURA) } != 0 || !valor_ap(modo, "restaurada", a, 77) {
        eprintln!("[T033-SMP-ERROR] modo={modo} restauración falló"); return false;
    }
    println!("[T033-SMP] modo={modo} fase=restaurada valor=77");
    if unsafe { munmap(a, PAGINA) } != 0 { eprintln!("[T033-SMP-ERROR] modo={modo} limpieza falló"); return false; }
    true
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let modo = args.first().map(String::as_str).unwrap_or("");
    if args.len() != 1 || !matches!(modo, "munmap" | "control-munmap" | "proteccion" | "control-proteccion") {
        eprintln!("[T033-SMP-ERROR] uso: t033_smp munmap|control-munmap|proteccion|control-proteccion"); process::exit(2);
    }
    if !instalar_manejador() || !fijar_lider() { eprintln!("[T033-SMP-ERROR] modo={modo} manejador o CPU líder falló"); process::exit(3); }
    let tid = match crear_trabajador() { Some(t) => t, None => { eprintln!("[T033-SMP-ERROR] modo={modo} trabajador CPU 1 falló"); process::exit(4); } };
    println!("[T033-SMP] modo={modo} inicio cpu_lider=0 cpu_ap=1 paginas=1");
    let _ = io::stdout().flush();
    let ok = match modo {
        "munmap" => probar_munmap(modo, false),
        "control-munmap" => probar_munmap(modo, true),
        "proteccion" => probar_proteccion(modo, false),
        "control-proteccion" => probar_proteccion(modo, true),
        _ => false,
    };
    PARAR.store(true, Ordering::SeqCst);
    if unsafe { pthread_join(tid, std::ptr::null_mut()) } != 0 {
        eprintln!("[T033-SMP-ERROR] modo={modo} pthread_join falló"); process::exit(5);
    }
    if cpu_actual() != Some(0) || CPU_AP_FINAL.load(Ordering::SeqCst) != 1 {
        eprintln!("[T033-SMP-ERROR] modo={modo} CPU final distinta"); process::exit(6);
    }
    println!("[T033-SMP] modo={modo} fin cpu_lider=0 cpu_ap=1");
    if !ok { process::exit(1); }
    println!("[T033-SMP] PASS modo={modo}");
}
