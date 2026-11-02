#![no_std]
#![no_main]
//! paralelo — [T-025 E3c] medición REAL del paralelismo entre núcleos.
//!
//! Lanza N hijos (fork) que hacen el mismo trabajo de CPU (sin syscalls en el
//! bucle) y mide con CLOCK_MONOTONIC cuánto tardan todos en acabar; antes mide
//! el mismo trabajo total hecho por UN solo hijo (serie). Imprime la
//! aceleración = serie / paralelo y en qué núcleo (getcpu) terminó cada hijo.
//! Con la planificación SMP apagada la aceleración debe rondar 1,0 (control
//! negativo: la medida no se inventa); con 2 núcleos debe superar 1,3.
//!
//! Uso: paralelo [hijos=2] [millones_de_iteraciones=40]

use portix_rt::println;
use portix_rt::syscall::{sys_fork, sys_waitpid, sys_exit, sys_clock_gettime, sys_getcpu, Timespec};

const CLOCK_MONOTONIC: i32 = 1;

fn ahora_ms() -> u64 {
    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    sys_clock_gettime(CLOCK_MONOTONIC, &mut ts as *mut Timespec);
    (ts.tv_sec as u64) * 1000 + (ts.tv_nsec as u64) / 1_000_000
}

fn cpu_actual() -> u32 {
    let mut c: u32 = 0xFFFF;
    sys_getcpu(&mut c as *mut u32, core::ptr::null_mut(), core::ptr::null_mut());
    c
}

/// Trabajo de CPU puro: mezcla xorshift sobre `iter` iteraciones (black_box
/// impide que el compilador lo elimine). Devuelve un resumen de 8 bits para el
/// código de salida (así el padre comprueba que el hijo trabajó de verdad).
fn trabajo(iter: u64, semilla: u64) -> u8 {
    let mut x = core::hint::black_box(semilla | 1);
    let mut i = 0u64;
    while i < iter {
        x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        i += 1;
    }
    (core::hint::black_box(x) & 0x7F) as u8
}

fn esperado(iter: u64, semilla: u64) -> u8 { trabajo(iter, semilla) }

/// [E3d] Como `trabajo`, pero muestrea `getcpu` 64 veces a lo largo del bucle y
/// devuelve además la máscara de CPUs en las que corrió: un hijo puede migrar
/// al BSP justo antes de terminar, y mirar sólo la última CPU mentía.
fn trabajo_muestreado(iter: u64, semilla: u64) -> (u8, u64) {
    let mut x = core::hint::black_box(semilla | 1);
    let tramo = (iter / 64).max(1);
    let mut vistas = 0u64;
    let mut hecho = 0u64;
    while hecho < iter {
        let fin = (hecho + tramo).min(iter);
        while hecho < fin {
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            hecho += 1;
        }
        vistas |= 1u64 << (cpu_actual() as u64 & 63);
    }
    ((core::hint::black_box(x) & 0x7F) as u8, vistas)
}

fn parse(p: *const u8) -> Option<u64> {
    if p.is_null() { return None; }
    let mut v = 0u64; let mut n = 0; let mut q = p;
    unsafe {
        while *q != 0 { if *q < b'0' || *q > b'9' { return None; } v = v * 10 + (*q - b'0') as u64; q = q.add(1); n += 1; }
    }
    if n == 0 { None } else { Some(v) }
}

/// Lanza `n` hijos con `iter` iteraciones cada uno y espera a todos.
/// Devuelve (ms, hijos correctos, máscara de CPUs en las que terminaron).
fn ronda(n: u64, iter: u64) -> (u64, u64, u64) {
    // Los resultados esperados se calculan ANTES de arrancar el reloj (el
    // padre no debe sumar su propio trabajo de verificación al tiempo medido).
    let n = n.min(16);
    let mut esperados = [0u8; 16];
    for k in 0..n { esperados[k as usize] = esperado(iter, 0x9E37_79B9_7F4A_7C15 ^ (k + 1)); }
    let t0 = ahora_ms();
    let mut pids = [0i64; 16];
    for k in 0..n {
        let pid = sys_fork();
        if pid == 0 {
            let th0 = ahora_ms();
            let (r, vistas) = trabajo_muestreado(iter, 0x9E37_79B9_7F4A_7C15 ^ (k + 1));
            // bits 0-6: resumen. CPU y duración propia del hijo (desde que empezó a
            // correr hasta que acabó: si es ~serie/N, el núcleo le dio la CPU entera y
            // cualquier diferencia con el tiempo de pared es latencia de arranque).
            println!("[PARALELO-HIJO] k={} cpu={} cpus_vistas={:#x} ms={} inicio=+{}ms", k, cpu_actual(), vistas, ahora_ms().saturating_sub(th0), th0.saturating_sub(t0));
            sys_exit(r as i32);
        }
        pids[k as usize] = pid;
    }
    let mut bien = 0u64; let mut cpus = 0u64;
    for k in 0..n {
        if pids[k as usize] <= 0 { continue; }
        let mut st: i32 = 0;
        sys_waitpid(pids[k as usize] as i32, &mut st as *mut i32, 0);
        let code = portix_rt::wexitstatus(st) as u8;
        if portix_rt::wifexited(st) && code == esperados[k as usize] { bien += 1; }
        cpus |= 1 << (k & 63);
    }
    (ahora_ms().saturating_sub(t0), bien, cpus)
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut hijos = 2u64; let mut millones = 40u64;
    unsafe {
        if argc >= 2 { if let Some(v) = parse(*argv.add(1)) { hijos = v.clamp(1, 16); } }
        if argc >= 3 { if let Some(v) = parse(*argv.add(2)) { millones = v.clamp(1, 10_000); } }
    }
    let iter = millones * 1_000_000;
    println!("[PARALELO] hijos={} iteraciones_por_hijo={}M padre_en_cpu={}", hijos, millones, cpu_actual());
    // Serie: un solo hijo hace TODO el trabajo (hijos × iter).
    let (ms_serie, ok_serie, _) = ronda(1, iter * hijos);
    // Paralelo: `hijos` hijos a la vez.
    let (ms_par, ok_par, _) = ronda(hijos, iter);
    let acel_x100 = if ms_par > 0 { ms_serie * 100 / ms_par } else { 0 };
    println!("[PARALELO] serie={}ms paralelo={}ms aceleracion={}.{:02} hijos_correctos={}/{} serie_correcta={}",
        ms_serie, ms_par, acel_x100 / 100, acel_x100 % 100, ok_par, hijos, ok_serie == 1);
    if ok_par != hijos || ok_serie != 1 { println!("[PARALELO-FAIL] algun hijo devolvio un resultado erroneo"); return 1; }
    0
}
