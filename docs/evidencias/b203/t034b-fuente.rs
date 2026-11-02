// Reproducción externa en Rust del cierre de un grupo desde un hilo en otro CPU.
// Se compila sin modificar rustc/glibc: rustc -O -C target-feature=+crt-static.
// El anfitrión y Portix deben tener CPU 0 y CPU 1 disponibles.
use std::io::{self, Write};
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
    let code = arg as usize as i32;
    let mut cpu = u32::MAX;
    let rc = unsafe { syscall(309, &mut cpu as *mut u32, std::ptr::null_mut::<u32>(), 0usize) };
    if rc != 0 || cpu != 1 {
        eprintln!("[T034-ERROR] el hilo no arrancó en CPU 1 (cpu={cpu}, rc={rc})");
        process::exit(97);
    }
    // Deja que el líder entre en join/futex antes de terminar todo el grupo.
    thread::sleep(Duration::from_millis(150));
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

fn probar_wait(code: i32) -> bool {
    // El hijo hereda CPU 0 y no depende de migrar tras el fork.
    if !fijar_cpu(0) {
        eprintln!("[T034-ERROR] el padre no pudo fijarse a CPU 0");
        return false;
    }
    let hijo = unsafe { fork() };
    if hijo == 0 { salir_desde_ap(code); }
    if hijo < 0 { eprintln!("[T034-ERROR] fork falló"); return false; }
    let mut estado = -1;
    let esperado = (code & 255) << 8;
    let vuelto = unsafe { waitpid(hijo, &mut estado, 0) };
    let ok = vuelto == hijo && estado == esperado;
    println!("[T034-WAIT] code={code} esperado={esperado} observado={estado} pid_correcto={} {}",
             vuelto == hijo, if ok { "OK" } else { "FALLO" });
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

fn main() {
    let args: Vec<String> = env::args().collect();
    let modo = args.get(1).map(String::as_str).unwrap_or("");
    let code = args.get(2).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    match modo {
        "directo" if (0..=255).contains(&code) => salir_desde_ap(code),
        "wait" if (0..=255).contains(&code) => process::exit(if probar_wait(code) { 0 } else { 1 }),
        "sigkill" => process::exit(if probar_sigkill_real() { 0 } else { 1 }),
        _ => { eprintln!("uso: t034_exit_group directo|wait CODIGO, o sigkill"); process::exit(2); }
    }
}
