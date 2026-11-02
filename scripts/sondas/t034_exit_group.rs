// Reproducción externa en Rust del cierre de un grupo desde un hilo en otro CPU.
// Se compila sin modificar rustc/glibc: rustc -O -C target-feature=+crt-static.
// El anfitrión y Portix deben tener CPU 0 y CPU 1 disponibles.
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};
use std::{env, process, thread};

extern "C" {
    fn sched_setaffinity(pid: i32, size: usize, mask: *const u64) -> i32;
    fn syscall(num: i64, ...) -> i64;
    fn fork() -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn pipe(fds: *mut i32) -> i32;
    fn read(fd: i32, buf: *mut std::ffi::c_void, n: usize) -> isize;
    fn write(fd: i32, buf: *const std::ffi::c_void, n: usize) -> isize;
    fn close(fd: i32) -> i32;
    fn pthread_attr_init(attr: *mut PthreadAttr) -> i32;
    fn pthread_attr_destroy(attr: *mut PthreadAttr) -> i32;
    fn pthread_attr_setaffinity_np(attr: *mut PthreadAttr, size: usize, mask: *const u64) -> i32;
    fn pthread_create(tid: *mut usize, attr: *const PthreadAttr,
                      inicio: extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void,
                      arg: *mut std::ffi::c_void) -> i32;
    fn pthread_join(tid: usize, valor: *mut *mut std::ffi::c_void) -> i32;
}

// ABI glibc x86-64: pthread_attr_t ocupa 56 bytes, alineados a 8.
#[repr(C, align(8))]
struct PthreadAttr([u8; 56]);

fn fijar_cpu(cpu: u32) -> bool {
    let mask = 1u64 << cpu;
    if unsafe { sched_setaffinity(0, 8, &mask) } != 0 { return false; }
    let fin = Instant::now() + Duration::from_secs(5);
    while Instant::now() < fin {
        let mut actual = u32::MAX;
        let rc = unsafe { syscall(309, &mut actual as *mut u32, std::ptr::null_mut::<u32>(), 0usize) };
        if rc == 0 && actual == cpu { return true; }
        thread::yield_now();
    }
    false
}

extern "C" fn hilo_exit(arg: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    let parametros = arg as usize;
    let code = (parametros & 255) as i32;
    let dormido = (parametros & 0x10000) != 0;
    let mut cpu = u32::MAX;
    let rc = unsafe { syscall(309, &mut cpu as *mut u32, std::ptr::null_mut::<u32>(), 0usize) };
    if rc != 0 || cpu != 1 {
        eprintln!("[T034-ERROR] el hilo no arrancó en CPU 1 (cpu={cpu}, rc={rc})");
        process::exit(97);
    }
    // Deja que el líder entre en join/futex antes de terminar todo el grupo.
    thread::sleep(Duration::from_millis(if dormido { 800 } else { 150 }));
    let mut cpu2 = u32::MAX;
    let rc2 = unsafe { syscall(309, &mut cpu2 as *mut u32, std::ptr::null_mut::<u32>(), 0usize) };
    if rc2 != 0 || cpu2 != 1 {
        eprintln!("[T034-ERROR] el hilo no siguió en CPU 1 (cpu={cpu2}, rc={rc2})");
        process::exit(97);
    }
    println!("[T034-AP] cpu=1 exit_group={code}");
    let _ = io::stdout().flush();
    process::exit(code);
}

fn salir_desde_ap(code: i32) -> ! {
    if !fijar_cpu(0) {
        eprintln!("[T034-ERROR] el líder no pudo fijarse a CPU 0");
        process::exit(96);
    }
    // pthread_create establece la afinidad ANTES de ejecutar el hilo. Fijarla
    // desde el propio hilo y confiar en migración inmediata sería otra prueba.
    let mut attr = PthreadAttr([0; 56]);
    let mask = 2u64;
    let mut tid = 0usize;
    let init = unsafe { pthread_attr_init(&mut attr) };
    let afin = if init == 0 { unsafe { pthread_attr_setaffinity_np(&mut attr, 8, &mask) } } else { init };
    let creado = if afin == 0 { unsafe { pthread_create(&mut tid, &attr, hilo_exit, code as usize as *mut _) } } else { afin };
    if init == 0 { unsafe { pthread_attr_destroy(&mut attr); } }
    if creado != 0 {
        eprintln!("[T034-ERROR] no se pudo crear hilo con afinidad CPU 1: {creado}");
        process::exit(97);
    }
    unsafe { pthread_join(tid, std::ptr::null_mut()); }
    eprintln!("[T034-ERROR] exit_group devolvió control al líder");
    process::exit(98);
}

/// Líder dormido en un futex SIN plazo mientras un hilo llama a exit_group(7)
/// desde la otra CPU. Salida esperada: el grupo entero muere con código 7 y la
/// consola recupera el control (si el líder no se cierra, la consola espera
/// para siempre: eso es el defecto).
fn lider_en_futex() -> ! {
    if !fijar_cpu(0) {
        eprintln!("[T034-ERROR] el líder no pudo fijarse a CPU 0");
        process::exit(96);
    }
    // Como la JVM: varios hilos de apoyo además del que mata el grupo. Algunos
    // duermen en futex y otros en nanosleep, para que el cierre del grupo tenga
    // que vérselas con hilos en esperas de distinto tipo.
    let mut apoyos = [0usize; 10];
    for (i, t) in apoyos.iter_mut().enumerate() {
        let mut a = PthreadAttr([0; 56]);
        let init_a = unsafe { pthread_attr_init(&mut a) };
        let creado_a = if init_a == 0 {
            unsafe { pthread_create(t, &a, if i % 2 == 0 { hilo_duerme_futex } else { hilo_duerme_reloj }, i as *mut _) }
        } else { init_a };
        if init_a == 0 { unsafe { pthread_attr_destroy(&mut a); } }
        if creado_a != 0 {
            eprintln!("[T034-ERROR] no se pudo crear el hilo de apoyo {i}: {creado_a}");
            process::exit(95);
        }
    }
    let mut attr = PthreadAttr([0; 56]);
    let mask = 2u64;
    let mut tid = 0usize;
    let init = unsafe { pthread_attr_init(&mut attr) };
    let afin = if init == 0 { unsafe { pthread_attr_setaffinity_np(&mut attr, 8, &mask) } } else { init };
    let creado = if afin == 0 {
        unsafe { pthread_create(&mut tid, &attr, hilo_mata_grupo_tarde, 7usize as *mut _) }
    } else { afin };
    if init == 0 { unsafe { pthread_attr_destroy(&mut attr); } }
    if creado != 0 {
        eprintln!("[T034-ERROR] no se pudo crear hilo con afinidad CPU 1: {creado}");
        process::exit(97);
    }
    // Da tiempo al hilo a llegar a CPU 1 y a estar realmente en su exit_group.
    let fin = Instant::now() + Duration::from_millis(300);
    while Instant::now() < fin { thread::yield_now(); }
    println!("[T034] lider en futex (espera infinita, palabra={})", PALABRA.load(Ordering::SeqCst));
    let _ = io::stdout().flush();
    // FUTEX_WAIT(202, uaddr, val=0, timeout=NULL): se duerme hasta que alguien
    // escriba la palabra o despierte; nadie lo hará nunca.
    unsafe { syscall(202, &PALABRA as *const AtomicU32 as usize, 0usize, 0usize, 0usize, 0usize, 0usize) };
    // Si el kernel devuelve el control al líder, exit_group NO cerró el grupo.
    println!("[T034-ERROR] el lider volvio de futex sin que el grupo muriera");
    let _ = io::stdout().flush();
    process::exit(99);
}

static PALABRA: AtomicU32 = AtomicU32::new(0);
static PALABRA2: AtomicU32 = AtomicU32::new(0);

extern "C" fn hilo_duerme_futex(_: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    unsafe { syscall(202, &PALABRA2 as *const AtomicU32 as usize, 0usize, 0usize, 0usize, 0usize, 0usize) };
    std::ptr::null_mut()
}

extern "C" fn hilo_duerme_reloj(_: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    let ts = [600i64, 0i64]; // 600 s: sigue dormido cuando el grupo muera
    unsafe { syscall(35, 0usize, &ts as *const i64 as usize, 0usize) }; // nanosleep
    std::ptr::null_mut()
}

/// [T-042] Variante que espera a que el líder esté DENTRO de su futex antes de
/// matar el grupo. Con `hilo_mata_grupo` (inmediato) el grupo muere antes de que
/// el líder llegue a dormir, así que no se ejercita el cierre diferido del líder
/// de consola, que es justo lo que dejó muda la consola en la JVM.
extern "C" fn hilo_mata_grupo_tarde(code: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    let codigo = code as usize;
    std::thread::sleep(std::time::Duration::from_millis(1200));
    println!("[T034] hilo en CPU 1 -> exit_group({codigo}) (tras 1200 ms con el lider dormido)");
    let _ = io::stdout().flush();
    unsafe { syscall(231, codigo) };
    std::ptr::null_mut()
}

extern "C" fn hilo_mata_grupo(code: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    let codigo = code as usize;
    println!("[T034] hilo en CPU 1 -> exit_group({codigo})");
    let _ = io::stdout().flush();
    unsafe { syscall(231, codigo) };
    std::ptr::null_mut()
}

static APOYO_LISTO: AtomicBool = AtomicBool::new(false);

extern "C" fn hilo_apoyo(_: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    APOYO_LISTO.store(true, Ordering::SeqCst);
    // Un hilo Ready en el BSP permite al líder ceder su contexto al salir.
    loop { std::hint::spin_loop(); }
}

fn crear_con_afinidad(cpu: u32, inicio: extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void,
                      arg: usize) -> Option<usize> {
    let mut attr = PthreadAttr([0; 56]);
    let mask = 1u64 << cpu;
    let mut tid = 0usize;
    if unsafe { pthread_attr_init(&mut attr) } != 0 { return None; }
    let afin = unsafe { pthread_attr_setaffinity_np(&mut attr, 8, &mask) };
    let creado = if afin == 0 { unsafe { pthread_create(&mut tid, &attr, inicio, arg as *mut _) } } else { afin };
    unsafe { pthread_attr_destroy(&mut attr); }
    if creado == 0 { Some(tid) } else { None }
}

fn lider_dormido(code: i32) -> ! {
    if !fijar_cpu(0) || crear_con_afinidad(0, hilo_apoyo, 0).is_none() {
        eprintln!("[T034-ERROR] no se preparó el hilo de apoyo en CPU 0");
        process::exit(96);
    }
    let fin = Instant::now() + Duration::from_secs(5);
    while !APOYO_LISTO.load(Ordering::SeqCst) && Instant::now() < fin { thread::yield_now(); }
    if !APOYO_LISTO.load(Ordering::SeqCst) || crear_con_afinidad(1, hilo_exit, (code as usize) | 0x10000).is_none() {
        eprintln!("[T034-ERROR] no se prepararon los dos hilos");
        process::exit(97);
    }
    println!("[T034-LIDER] exit(2) antes del cierre del grupo");
    let _ = io::stdout().flush();
    // syscall 60 termina sólo el hilo actual, sin desenrollar marcos Rust.
    unsafe { syscall(60, 0); }
    eprintln!("[T034-ERROR] exit(2) volvió");
    process::exit(98);
}

fn probar_wait(code: i32, dormido: bool) -> bool {
    // El hijo hereda CPU 0 y no depende de migrar tras el fork.
    if !fijar_cpu(0) {
        eprintln!("[T034-ERROR] el padre no pudo fijarse a CPU 0");
        return false;
    }
    let hijo = unsafe { fork() };
    if hijo == 0 {
        if dormido { lider_dormido(code); } else { salir_desde_ap(code); }
    }
    if hijo < 0 { eprintln!("[T034-ERROR] fork falló"); return false; }
    let mut estado = -1;
    let esperado = (code & 255) << 8;
    let vuelto = unsafe { waitpid(hijo, &mut estado, 0) };
    let ok = vuelto == hijo && estado == esperado;
    if dormido {
        println!("[T034-WAIT-DORMIDO] hijo={hijo} code={code} esperado={esperado} observado={estado} pid_correcto={} {}",
                 vuelto == hijo, if ok { "OK" } else { "FALLO" });
    } else {
        println!("[T034-WAIT] code={code} esperado={esperado} observado={estado} pid_correcto={} {}",
                 vuelto == hijo, if ok { "OK" } else { "FALLO" });
    }
    ok
}

fn probar_sigkill_real() -> bool {
    let mut fds = [-1; 2];
    if unsafe { pipe(fds.as_mut_ptr()) } != 0 { return false; }
    let hijo = unsafe { fork() };
    if hijo == 0 {
        unsafe { close(fds[0]); }
        let b = [b'R'];
        if unsafe { write(fds[1], b.as_ptr().cast(), 1) } != 1 { process::exit(95); }
        loop { thread::sleep(Duration::from_secs(1)); }
    }
    unsafe { close(fds[1]); }
    if hijo < 0 { unsafe { close(fds[0]); } return false; }
    let mut b = [0u8; 1];
    let listo = unsafe { read(fds[0], b.as_mut_ptr().cast(), 1) } == 1 && b[0] == b'R';
    unsafe { close(fds[0]); }
    let envio = unsafe { kill(hijo, 9) } == 0;
    let mut estado = -1;
    let vuelto = unsafe { waitpid(hijo, &mut estado, 0) };
    let ok = listo && envio && vuelto == hijo && estado == 9;
    println!("[T034-SIGKILL] esperado=9 observado={estado} pid_correcto={} {}",
             vuelto == hijo, if ok { "OK" } else { "FALLO" });
    ok
}

static VICTIMA_AP_LISTA: AtomicBool = AtomicBool::new(false);

extern "C" fn victima_ap(_: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    let mut cpu = u32::MAX;
    if unsafe { syscall(309, &mut cpu as *mut u32, std::ptr::null_mut::<u32>(), 0usize) } != 0 || cpu != 1 {
        eprintln!("[T034-ERROR] la víctima no ejecuta en CPU 1: {cpu}");
        process::exit(97);
    }
    println!("[T034-VICTIMA] hilo activo en CPU 1");
    let _ = io::stdout().flush();
    VICTIMA_AP_LISTA.store(true, Ordering::SeqCst);
    loop { thread::sleep(Duration::from_secs(1)); }
}

fn preparar_victima(fd: i32) -> ! {
    if !fijar_cpu(0) || crear_con_afinidad(0, hilo_apoyo, 0).is_none()
        || crear_con_afinidad(1, victima_ap, 0).is_none() {
        eprintln!("[T034-ERROR] no se preparó la víctima multihilo");
        process::exit(96);
    }
    let fin = Instant::now() + Duration::from_secs(5);
    while !(APOYO_LISTO.load(Ordering::SeqCst) && VICTIMA_AP_LISTA.load(Ordering::SeqCst)) && Instant::now() < fin {
        thread::yield_now();
    }
    if !(APOYO_LISTO.load(Ordering::SeqCst) && VICTIMA_AP_LISTA.load(Ordering::SeqCst)) {
        eprintln!("[T034-ERROR] los hilos de la víctima no arrancaron");
        process::exit(96);
    }
    let b = [b'R'];
    if unsafe { write(fd, b.as_ptr().cast(), 1) } != 1 {
        eprintln!("[T034-ERROR] señal de preparación falló");
        process::exit(95);
    }
    unsafe { close(fd); }
    println!("[T034-VICTIMA] líder llama exit(2) con hilos vivos");
    let _ = io::stdout().flush();
    unsafe { syscall(60, 0); }
    eprintln!("[T034-ERROR] exit(2) de víctima volvió");
    process::exit(98);
}

fn probar_sigkill_hilos() -> bool {
    // La afinidad se hereda en fork: así el hijo empieza ya en CPU 0 y no
    // depende de una migración inmediata después de sched_setaffinity.
    if !fijar_cpu(0) {
        eprintln!("[T034-ERROR] el padre de la víctima no pudo fijarse a CPU 0");
        return false;
    }
    let mut fds = [-1; 2];
    if unsafe { pipe(fds.as_mut_ptr()) } != 0 { return false; }
    let hijo = unsafe { fork() };
    if hijo == 0 {
        unsafe { close(fds[0]); }
        preparar_victima(fds[1]);
    }
    unsafe { close(fds[1]); }
    if hijo < 0 { unsafe { close(fds[0]); } return false; }
    let mut b = [0u8; 1];
    let listo = unsafe { read(fds[0], b.as_mut_ptr().cast(), 1) } == 1 && b[0] == b'R';
    unsafe { close(fds[0]); }
    // El verificador comprueba también la traza del núcleo que confirma que
    // el líder había salido antes de que se recoja su estado.
    thread::sleep(Duration::from_millis(500));
    println!("[T034-PADRE] SIGKILL a hijo={hijo}");
    let _ = io::stdout().flush();
    let envio = unsafe { kill(hijo, 9) } == 0;
    let mut estado = -1;
    let vuelto = unsafe { waitpid(hijo, &mut estado, 0) };
    let rc_existencia = unsafe { kill(hijo, 0) };
    let sin_proceso = rc_existencia == -1 && io::Error::last_os_error().raw_os_error() == Some(3);
    let ok = listo && envio && vuelto == hijo && estado == 9 && sin_proceso;
    println!("[T034-SIGKILL-HILOS] hijo={hijo} listo={listo} envio={envio} esperado=9 observado={estado} pid_correcto={} sin_proceso={sin_proceso} {}",
             vuelto == hijo, if ok { "OK" } else { "FALLO" });
    ok
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let modo = args.get(1).map(String::as_str).unwrap_or("");
    let code = args.get(2).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    match modo {
        "directo" if (0..=255).contains(&code) => salir_desde_ap(code),
        "dormido" if (0..=255).contains(&code) => lider_dormido(code),
        "wait" if (0..=255).contains(&code) => process::exit(if probar_wait(code, false) { 0 } else { 1 }),
        "wait-dormido" if (0..=255).contains(&code) => process::exit(if probar_wait(code, true) { 0 } else { 1 }),
        "sigkill" => process::exit(if probar_sigkill_real() { 0 } else { 1 }),
        "sigkill-hilos" => process::exit(if probar_sigkill_hilos() { 0 } else { 1 }),
        // [T-026/b203] Líder BLOQUEADO EN UN FUTEX (no en su bucle de exit) y el
        // grupo termina desde un hilo en la otra CPU: el caso que dejó viva a la
        // JVM y sorda la consola tras la sección Java.
        "futex-lider" => lider_en_futex(),
        _ => { eprintln!("uso: t034_exit_group directo|dormido|wait|wait-dormido CODIGO, sigkill|sigkill-hilos|futex-lider"); process::exit(2); }
    }
}
