// userland/bin/portix-bench/src/main.rs — Mediciones TSC reales desde Ring 3 (syscall, asignador, yield, carga CPU nombrada, estres fork/exec)
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use portix_rt::println;
use portix_rt::syscall::{sys_getpid, sys_sched_yield, sys_fork, sys_execve, sys_waitpid, sys_exit};

#[inline(always)]
fn read_tsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  [FRENTE C] Carga de CPU en Ring 3 con funciones NOMBRADAS, para que el
//  profiler del kernel (/proc/profiler) tenga algo real que atribuir: dos
//  funciones no inlineables con un reparto de trabajo conocido (3:1).
// ─────────────────────────────────────────────────────────────────────────────
#[inline(never)]
#[no_mangle]
pub extern "C" fn quemar_fnv(iteraciones: u64, semilla: u64) -> u64 {
    let mut h = 0xcbf29ce484222325u64 ^ semilla;
    let mut i = 0u64;
    while i < iteraciones {
        h ^= i & 0xFF;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    core::hint::black_box(h)
}

#[inline(never)]
#[no_mangle]
pub extern "C" fn quemar_collatz(iteraciones: u64, semilla: u64) -> u64 {
    let mut n = (semilla | 1).max(3);
    let mut acumulado = 0u64;
    let mut i = 0u64;
    while i < iteraciones {
        n = if n & 1 == 0 { n >> 1 } else { n.wrapping_mul(3).wrapping_add(1) };
        if n <= 1 { n = semilla.wrapping_add(i) | 1; }
        // El resultado depende de la órbita entera: el compilador no puede
        // sustituir el bucle por una fórmula cerrada.
        acumulado = acumulado.wrapping_add(n);
        i += 1;
    }
    core::hint::black_box(acumulado)
}

fn monotonic_ms() -> u64 {
    let mut ts = portix_rt::syscall::Timespec { tv_sec: 0, tv_nsec: 0 };
    let _ = portix_rt::syscall::sys_clock_gettime(1, &mut ts as *mut _);
    (ts.tv_sec as u64) * 1000 + (ts.tv_nsec as u64) / 1_000_000
}

fn carga_cpu(segundos: u64) -> i32 {
    println!("[CPU-BENCH] carga de CPU en Ring 3 durante {} s: quemar_fnv (3 llamadas por ronda) + quemar_collatz (1 llamada por ronda)", segundos);
    let inicio = monotonic_ms();
    let fin = inicio + segundos * 1000;
    let mut rondas = 0u64;
    let mut acc = 0u64;
    let (mut tsc_fnv, mut tsc_collatz) = (0u64, 0u64);
    let t0 = read_tsc();
    loop {
        let a = read_tsc();
        acc ^= quemar_fnv(200_000, rondas);
        acc ^= quemar_fnv(200_000, rondas + 1);
        acc ^= quemar_fnv(200_000, rondas + 2);
        let b = read_tsc();
        acc ^= quemar_collatz(200_000, rondas);
        let c = read_tsc();
        tsc_fnv += b.saturating_sub(a);
        tsc_collatz += c.saturating_sub(b);
        rondas += 1;
        if rondas % 4 == 0 && monotonic_ms() >= fin { break; }
    }
    let ciclos = read_tsc().saturating_sub(t0);
    let total = (tsc_fnv + tsc_collatz).max(1);
    // Reparto REAL medido con el TSC: es contra esto contra lo que se debe
    // comparar el reparto de muestras que publique /proc/profiler.
    println!("[CPU-BENCH-SHARE] quemar_fnv={}% quemar_collatz={}% (medido con TSC por funcion)",
             tsc_fnv * 100 / total, tsc_collatz * 100 / total);
    println!("[CPU-BENCH-DONE] rondas={} ciclos_tsc={} resultado={:#x}", rondas, ciclos, core::hint::black_box(acc));
    0
}

// ─────────────────────────────────────────────────────────────────────────────
//  [FRENTE C-4] Estrés real de fork/exec/wait desde Ring 3: N ciclos
//  fork() -> execve("/bin/true") -> waitpid(). Cada ciclo crea y destruye un
//  espacio de direcciones completo (COW en fork, sustitución en exec,
//  liberación en exit). Lo que se mide es lo que ocurre; los marcos los
//  verifica después /proc/frames (marcos rastreados debe volver al valor
//  inicial: ninguna fuga).
// ─────────────────────────────────────────────────────────────────────────────
fn estres_forkexec(ciclos: u64) -> i32 {
    let ruta = b"/bin/true\0";
    let argv0 = b"true\0";
    let argv: [*const u8; 2] = [argv0.as_ptr(), core::ptr::null()];
    println!("[FORKEXEC] {} ciclos fork -> execve(/bin/true) -> waitpid desde Ring 3", ciclos);
    let (mut fallos_fork, mut fallos_wait, mut salidas_raras) = (0u64, 0u64, 0u64);
    let (mut tsc_min, mut tsc_max, mut tsc_total) = (u64::MAX, 0u64, 0u64);
    let mut i = 0u64;
    while i < ciclos {
        let t0 = read_tsc();
        let pid = sys_fork();
        if pid == 0 {
            // Hijo: el exec sustituye esta imagen; si falla, salir con 127.
            sys_execve(ruta.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
            sys_exit(127);
        } else if pid < 0 {
            fallos_fork += 1;
        } else {
            let mut status: i32 = 0;
            let w = sys_waitpid(pid as i32, &mut status as *mut i32, 0);
            if w != pid {
                fallos_wait += 1;
            } else if (status >> 8) & 0xff != 0 {
                salidas_raras += 1;
            }
        }
        let dt = read_tsc().saturating_sub(t0);
        tsc_min = tsc_min.min(dt); tsc_max = tsc_max.max(dt); tsc_total += dt;
        i += 1;
    }
    let media = tsc_total / ciclos.max(1);
    println!("[FORKEXEC] latencia por ciclo (TSC): min={} media={} max={}", tsc_min, media, tsc_max);
    if fallos_fork == 0 && fallos_wait == 0 && salidas_raras == 0 {
        println!("[FORKEXEC-PASS] ciclos={} fallos_fork=0 fallos_wait=0 salidas_anomalas=0", ciclos);
        0
    } else {
        println!("[FORKEXEC-FAIL] ciclos={} fallos_fork={} fallos_wait={} salidas_anomalas={}",
                 ciclos, fallos_fork, fallos_wait, salidas_raras);
        1
    }
}

fn arg_str(argv: *const *const u8, i: usize) -> &'static str {
    unsafe {
        let p = *argv.add(i);
        if p.is_null() { return ""; }
        let mut n = 0usize;
        while *p.add(n) != 0 { n += 1; }
        core::str::from_utf8(core::slice::from_raw_parts(p, n)).unwrap_or("")
    }
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    if _argc >= 2 && !_argv.is_null() && arg_str(_argv, 1) == "cpu" {
        let segundos = if _argc >= 3 { arg_str(_argv, 2).parse::<u64>().unwrap_or(2) } else { 2 };
        return carga_cpu(segundos.clamp(1, 30));
    }
    if _argc >= 2 && !_argv.is_null() && arg_str(_argv, 1) == "forkexec" {
        let ciclos = if _argc >= 3 { arg_str(_argv, 2).parse::<u64>().unwrap_or(50) } else { 50 };
        return estres_forkexec(ciclos.clamp(1, 2000));
    }
    // ─────────────────────────────────────────────────────────────────────
    //  [FRENTE C] Sólo mediciones REALES en ciclos TSC. Se purgaron las
    //  conversiones a nanosegundos "asumiendo 2,5 GHz", los Mops/s y la
    //  matriz comparativa con Linux/Windows: ninguno de esos números se
    //  medía aquí. Para convertir a tiempo, el lector usa su propia
    //  frecuencia de TSC; aquí sólo se publica lo que el contador midió.
    // ─────────────────────────────────────────────────────────────────────
    println!("======================================================================");
    println!("  PORTIX-BENCH — mediciones TSC desde Ring 3 (sin estimaciones)");
    println!("  modos: portix-bench [cpu [s]] [forkexec [N]]");
    println!("======================================================================\n");

    // 1. Ida y vuelta de syscall (100 000 getpid)
    let iterations = 100_000u64;
    let t0 = read_tsc();
    for _ in 0..iterations {
        let _ = sys_getpid();
    }
    let syscall_cycles = read_tsc().saturating_sub(t0);
    println!("[BENCH 1/3] syscall getpid x{}: total={} ciclos, media={} ciclos/llamada",
             iterations, syscall_cycles, syscall_cycles / iterations);

    // 2. Asignador de usuario (50 000 Vec::with_capacity + push + drop)
    let alloc_iters = 50_000u64;
    let a0 = read_tsc();
    for i in 0..alloc_iters {
        let size = ((i % 256) + 16) as usize;
        let mut v: Vec<u8> = Vec::with_capacity(size);
        v.push((i & 0xFF) as u8);
        core::hint::black_box(&v);
    }
    let alloc_cycles = read_tsc().saturating_sub(a0);
    println!("[BENCH 2/3] alloc+free x{}: total={} ciclos, media={} ciclos/operacion",
             alloc_iters, alloc_cycles, alloc_cycles / alloc_iters);

    // 3. sched_yield (10 000 cesiones de CPU al planificador)
    let yield_iters = 10_000u64;
    let y0 = read_tsc();
    for _ in 0..yield_iters {
        let _ = sys_sched_yield();
    }
    let yield_cycles = read_tsc().saturating_sub(y0);
    println!("[BENCH 3/3] sched_yield x{}: total={} ciclos, media={} ciclos/cesion",
             yield_iters, yield_cycles, yield_cycles / yield_iters);

    if syscall_cycles > 0 && alloc_cycles > 0 && yield_cycles > 0 {
        println!("\n[PORTIX-BENCH-PASS] tres mediciones TSC reales completadas (syscall={} alloc={} yield={} ciclos/op)",
                 syscall_cycles / iterations, alloc_cycles / alloc_iters, yield_cycles / yield_iters);
        0
    } else {
        println!("\n[PORTIX-BENCH-FAIL] el TSC no avanzo durante alguna medicion");
        1
    }
}
