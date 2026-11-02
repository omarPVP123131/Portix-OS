// T-035: ciclo de vida de páginas anónimas tras PROT_NONE. Sólo ABI Linux x86-64.
// Compilar: rustc -O -C target-feature=+crt-static -C strip=symbols
//           scripts/sondas/t035_memoria.rs -o build/packages/t035_memoria
// Los modos control-* omiten deliberadamente una operación y DEBEN salir 1.
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::{env, process};

// El manejador acepta sólo el RIP y la dirección de la carga de prueba.
core::arch::global_asm!(
    ".text",
    ".global t035_cargar",
    "t035_cargar:",
    ".global t035_instruccion",
    "t035_instruccion:",
    "movzx eax, byte ptr [rdi]",
    "ret",
    ".global t035_continuacion",
    "t035_continuacion:",
    "mov eax, 256",
    "ret",
    ".global t035_guardar",
    "t035_guardar:",
    ".global t035_instruccion_escritura",
    "t035_instruccion_escritura:",
    "mov byte ptr [rdi], sil",
    "ret",
    ".global t035_restaurador",
    "t035_restaurador:",
    "mov rax, 15",
    "syscall",
    "ud2",
);

extern "C" {
    fn t035_cargar(p: *const u8) -> u32;
    fn t035_guardar(p: *mut u8, v: u32);
    static t035_instruccion: u8;
    static t035_instruccion_escritura: u8;
    static t035_continuacion: u8;
    static t035_restaurador: u8;
}

#[repr(C)]
struct AccionKernel { manejador: usize, banderas: u64, restaurador: usize, mascara: u64 }
static ACTIVO: AtomicBool = AtomicBool::new(false);
static DIRECCION_ESPERADA: AtomicUsize = AtomicUsize::new(0);
static CODIGO_SENAL: AtomicI32 = AtomicI32::new(0);
static FALTAS_SENAL: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn recibir_segv(sig: i32, info: *mut c_void, contexto: *mut c_void) {
    if sig != 11 || info.is_null() || contexto.is_null() || !ACTIVO.load(Ordering::SeqCst) { _exit(90); }
    let si = info as *const u8;
    let direccion = (si.add(16) as *const usize).read_volatile();
    let rip = (contexto as *mut u8).add(168) as *mut usize;
    let instruccion = rip.read_volatile();
    if direccion != DIRECCION_ESPERADA.load(Ordering::SeqCst)
        || (instruccion != std::ptr::addr_of!(t035_instruccion) as usize
            && instruccion != std::ptr::addr_of!(t035_instruccion_escritura) as usize) { _exit(91); }
    CODIGO_SENAL.store((si.add(8) as *const i32).read_volatile(), Ordering::SeqCst);
    FALTAS_SENAL.fetch_add(1, Ordering::SeqCst);
    rip.write_volatile(std::ptr::addr_of!(t035_continuacion) as usize);
}

fn instalar_segv() -> bool {
    let a = AccionKernel { manejador: recibir_segv as *const () as usize, banderas: 0x0400_0004,
        restaurador: std::ptr::addr_of!(t035_restaurador) as usize, mascara: 0 };
    unsafe { syscall(13, 11i32, &a as *const AccionKernel, std::ptr::null_mut::<c_void>(), 8usize) == 0 }
}

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

fn descartar(modo: &str, control: bool, previo: i32) -> bool {
    let p = match reservar(modo) { Some(p) => p, None => return false };
    if !proteger(modo, p, previo) { limpiar(p); return false; }
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

// La página nunca se materializa antes de la lectura. Compara una reserva
// PROT_NONE contra una página válida y otra realmente desmapeada.
fn reserva_perezosa(modo: &str, control: u8, pila_fija: bool) -> bool {
    if !instalar_segv() { return error(modo, "sigaction", "rc:0", &format!("errno:{}", errno())); }
    let prot = if control == 1 { PROT_RW } else { PROT_NONE };
    // En Portix: comienzo del área potencial de crecimiento de pila, más una
    // página. MAP_FIXED afecta sólo a este proceso de prueba independiente.
    let direccion = if pila_fija { (0x7f00_0000_0000usize - 8 * 1024 * 1024 + PAGE) as *mut c_void } else { std::ptr::null_mut() };
    let flags = MAP_PRIVATE | MAP_ANONYMOUS | if pila_fija { MAP_FIXED } else { 0 };
    let p = unsafe { mmap(direccion, PAGE, prot, flags, -1, 0) };
    if p.is_null() || p as usize == usize::MAX { return error(modo, "mmap", "direccion-valida", &format!("errno:{}", errno())); }
    if pila_fija && p != direccion { limpiar(p); return error(modo, "mmap", "direccion-fija", &format!("direccion:{:x}", p as usize)); }
    if control == 2 && unsafe { munmap(p, PAGE) } != 0 { return fallo_operacion(modo, "munmap"); }
    DIRECCION_ESPERADA.store(p as usize, Ordering::SeqCst);
    CODIGO_SENAL.store(0, Ordering::SeqCst);
    FALTAS_SENAL.store(0, Ordering::SeqCst);
    ACTIVO.store(true, Ordering::SeqCst);
    let valor = unsafe { t035_cargar(p as *const u8) };
    ACTIVO.store(false, Ordering::SeqCst);
    let codigo = CODIGO_SENAL.load(Ordering::SeqCst);
    let faltas = FALTAS_SENAL.load(Ordering::SeqCst);
    if valor != 256 || codigo != 2 || faltas != 1 {
        if control != 2 { limpiar(p); }
        return error(modo, "lectura", "senal:11,codigo:2,faltas:1", &format!("valor:{valor},codigo:{codigo},faltas:{faltas}"));
    }
    println!("[T035] modo={modo} fase=lectura senal=11 codigo=2 faltas=1 direccion=correcta");
    if !proteger(modo, p, PROT_RW) { limpiar(p); return false; }
    let inicial = byte(p);
    if inicial != 0 { limpiar(p); return error(modo, "restauracion", "valor:0", &format!("valor:{inicial}")); }
    unsafe { (p as *mut u8).write_volatile(77); }
    println!("[T035] modo={modo} fase=restauracion inicial=0 nueva=77");
    limpiar(p);
    true
}

// Comprobación específica del mecanismo de pila de Portix: no sustituye una
// comparación con Linux, cuyo mapa y política de crecimiento son diferentes.
fn pila_creciente(modo: &str) -> bool {
    if !instalar_segv() { return error(modo, "sigaction", "rc:0", &format!("errno:{}", errno())); }
    let p = (0x7f00_0000_0000usize - 8 * 1024 * 1024 + PAGE) as *mut c_void;
    limpiar_errno();
    let rc = unsafe { mprotect(p, PAGE, PROT_READ) };
    let e = if rc == -1 { errno() } else { 0 };
    if rc != -1 || e != ENOMEM { return error(modo, "precondicion", "rc:-1,errno:12", &format!("rc:{rc},errno:{e}")); }
    DIRECCION_ESPERADA.store(p as usize, Ordering::SeqCst);
    CODIGO_SENAL.store(0, Ordering::SeqCst);
    FALTAS_SENAL.store(0, Ordering::SeqCst);
    ACTIVO.store(true, Ordering::SeqCst);
    let valor = unsafe { t035_cargar(p as *const u8) };
    ACTIVO.store(false, Ordering::SeqCst);
    let codigo = CODIGO_SENAL.load(Ordering::SeqCst);
    let faltas = FALTAS_SENAL.load(Ordering::SeqCst);
    if valor != 0 || codigo != 0 || faltas != 0 {
        return error(modo, "crecimiento", "valor:0,codigo:0,faltas:0", &format!("valor:{valor},codigo:{codigo},faltas:{faltas}"));
    }
    unsafe { (p as *mut u8).write_volatile(77); }
    println!("[T035] modo={modo} fase=crecimiento inicial=0 nueva=77");
    if unsafe { munmap(p, PAGE) } != 0 { return fallo_operacion(modo, "munmap"); }
    true
}

// Página ya materializada (contenido 77) y después protegida: en x86-64 una
// página PRESENTE siempre se puede LEER aunque no sea escribible, así que una
// lectura exitosa tras PROT_NONE significa que la protección no existe.
fn protegida_materializada(modo: &str) -> bool {
    if !instalar_segv() { return error(modo, "sigaction", "rc:0", &format!("errno:{}", errno())); }
    let p = match reservar(modo) { Some(p) => p, None => return false };
    if byte(p) != 77 { limpiar(p); return error(modo, "materializada", "valor:77", &format!("valor:{}", byte(p))); }
    println!("[T035] modo={modo} fase=materializada valor=77");
    if !proteger(modo, p, PROT_NONE) { limpiar(p); return false; }
    if let Some(fallo) = acceso(modo, p, false, "lectura-prohibida") { limpiar(p); return fallo; }
    if !proteger(modo, p, PROT_READ) { limpiar(p); return false; }
    let conservado = byte(p);
    if conservado != 77 { limpiar(p); return error(modo, "lectura-permitida", "valor:77", &format!("valor:{conservado}")); }
    println!("[T035] modo={modo} fase=lectura-permitida valor=77");
    if let Some(fallo) = acceso(modo, p, true, "escritura-prohibida") { limpiar(p); return fallo; }
    if !proteger(modo, p, PROT_RW) { limpiar(p); return false; }
    if byte(p) != 77 { limpiar(p); return error(modo, "restauracion", "inicial:77", &format!("inicial:{}", byte(p))); }
    unsafe { t035_guardar(p as *mut u8, 88); }
    if byte(p) != 88 { limpiar(p); return error(modo, "escritura-permitida", "nueva:88", &format!("nueva:{}", byte(p))); }
    println!("[T035] modo={modo} fase=restauracion inicial=77 nueva=88");
    limpiar(p);
    true
}

// Devuelve Some(false-con-mensaje) si el acceso NO produjo la señal esperada.
fn acceso(modo: &str, p: *mut c_void, escritura: bool, fase: &str) -> Option<bool> {
    DIRECCION_ESPERADA.store(p as usize, Ordering::SeqCst);
    CODIGO_SENAL.store(0, Ordering::SeqCst);
    FALTAS_SENAL.store(0, Ordering::SeqCst);
    ACTIVO.store(true, Ordering::SeqCst);
    let valor = if escritura { unsafe { t035_guardar(p as *mut u8, 88) }; 256 } else { unsafe { t035_cargar(p as *const u8) } };
    ACTIVO.store(false, Ordering::SeqCst);
    let codigo = CODIGO_SENAL.load(Ordering::SeqCst);
    let faltas = FALTAS_SENAL.load(Ordering::SeqCst);
    if valor != 256 || codigo != 2 || faltas != 1 {
        return Some(error(modo, fase, "senal:11,codigo:2,faltas:1", &format!("valor:{valor},codigo:{codigo},faltas:{faltas}")));
    }
    println!("[T035] modo={modo} fase={fase} senal=11 codigo=2 faltas=1 direccion=correcta");
    None
}

fn escritura_invalida(modo: &str, control: bool, socket: bool, parcial: bool) -> bool {
    let p = unsafe { mmap(std::ptr::null_mut(), PAGE * 2, PROT_RW, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0) };
    if p as usize == usize::MAX || p.is_null() { return error(modo, "mmap", "direccion-valida", &format!("errno:{}", errno())); }
    unsafe { std::ptr::write_bytes(p as *mut u8, 90, PAGE); }
    let segunda = unsafe { (p as *mut u8).add(PAGE) };
    if !control && unsafe { munmap(segunda as *mut c_void, PAGE) } != 0 {
        limpiar(p); return fallo_operacion(modo, "munmap");
    }
    let mut fds = [-1i32; 2];
    let creado = if socket { unsafe { syscall(53, 1i32, 1i32, 0i32, fds.as_mut_ptr()) } }
                 else { unsafe { syscall(22, fds.as_mut_ptr()) } };
    if creado != 0 {
        limpiar(p); if control { unsafe { munmap(segunda as *mut c_void, PAGE); } }
        return fallo_operacion(modo, if socket { "socketpair" } else { "pipe" });
    }
    let (lectura, escritura) = if socket { (fds[1], fds[0]) } else { (fds[0], fds[1]) };
    let origen = if parcial { unsafe { segunda.sub(128) } } else { segunda };
    let longitud = if parcial { 256usize } else { 1usize };
    limpiar_errno();
    let rc = unsafe { syscall(1, escritura, origen, longitud) };
    let e = if rc == -1 { errno() } else { 0 };
    if rc != -1 || e != EFAULT {
        unsafe { syscall(3, fds[0]); syscall(3, fds[1]); }
        limpiar(p); if control { unsafe { munmap(segunda as *mut c_void, PAGE); } }
        return error(modo, "write-invalida", "rc:-1,errno:14", &format!("rc:{rc},errno:{e}"));
    }
    // Un fallo del primer bloque no debe insertar datos en el receptor.
    if unsafe { syscall(72, lectura, 4i32, 0x800i32) } != 0 {
        unsafe { syscall(3, fds[0]); syscall(3, fds[1]); }
        limpiar(p); return fallo_operacion(modo, "fcntl");
    }
    let mut valor = 0u8;
    limpiar_errno();
    let recibido = unsafe { syscall(0, lectura, &mut valor as *mut u8, 1usize) };
    let er = if recibido == -1 { errno() } else { 0 };
    unsafe { syscall(3, fds[0]); syscall(3, fds[1]); }
    limpiar(p); if control { unsafe { munmap(segunda as *mut c_void, PAGE); } }
    if recibido != -1 || er != 11 {
        return error(modo, "receptor-vacio", "rc:-1,errno:11", &format!("rc:{recibido},errno:{er}"));
    }
    println!("[T035] modo={modo} fase=write-invalida rc=-1 errno=14 receptor-vacio=si");
    true
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let modo = args.first().map(String::as_str).unwrap_or("");
    if args.len() != 1 || !matches!(modo, "control-munmap" | "munmap" | "control-fixed" | "fixed" |
        "control-fork" | "fork" | "control-madvise" | "madvise" | "control-madvise-ro" | "madvise-ro" |
        "control-madvise-rw" | "madvise-rw" | "control-usuario" | "usuario" | "usuario-lectura" |
        "control-pipe" | "pipe-invalida" | "control-socket" | "socket-invalido" |
        "control-pipe-parcial" | "pipe-parcial" | "control-socket-parcial" | "socket-parcial" |
        "perezosa" | "control-perezosa-accesible" | "control-perezosa-desmapeada" |
        "pila-protegida" | "control-pila-accesible" | "pila-creciente" | "protegida-materializada") {
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
        "control-madvise" => descartar(modo, true, PROT_NONE),
        "madvise" => descartar(modo, false, PROT_NONE),
        "control-madvise-ro" => descartar(modo, true, PROT_READ),
        "madvise-ro" => descartar(modo, false, PROT_READ),
        "control-madvise-rw" => descartar(modo, true, PROT_RW),
        "madvise-rw" => descartar(modo, false, PROT_RW),
        "control-usuario" => bufer_usuario(modo, true, false),
        "usuario" => bufer_usuario(modo, false, false),
        "usuario-lectura" => bufer_usuario(modo, false, true),
        "control-pipe" => escritura_invalida(modo, true, false, false),
        "pipe-invalida" => escritura_invalida(modo, false, false, false),
        "control-socket" => escritura_invalida(modo, true, true, false),
        "socket-invalido" => escritura_invalida(modo, false, true, false),
        "control-pipe-parcial" => escritura_invalida(modo, true, false, true),
        "pipe-parcial" => escritura_invalida(modo, false, false, true),
        "control-socket-parcial" => escritura_invalida(modo, true, true, true),
        "socket-parcial" => escritura_invalida(modo, false, true, true),
        "perezosa" => reserva_perezosa(modo, 0, false),
        "control-perezosa-accesible" => reserva_perezosa(modo, 1, false),
        "control-perezosa-desmapeada" => reserva_perezosa(modo, 2, false),
        "pila-protegida" => reserva_perezosa(modo, 0, true),
        "control-pila-accesible" => reserva_perezosa(modo, 1, true),
        "pila-creciente" => pila_creciente(modo),
        "protegida-materializada" => protegida_materializada(modo),
        _ => false,
    };
    println!("[T035] modo={modo} fin");
    if !ok { process::exit(1); }
    println!("[T035] PASS modo={modo}");
}
