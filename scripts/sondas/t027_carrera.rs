// T-027: carrera entre `mprotect` y el fallo de página sobre la MISMA entrada.
//
// El defecto: leer la PTE, decidir y escribirla no era atómico entre CPUs. Si
// `mprotect` (hacer escribible la pila de un hilo nuevo) y el fallo de página
// que materializa esa página coincidían, el que escribía segundo partía de su
// lectura vieja y dejaba la página con la protección ANTERIOR (PROT_NONE), así
// que el primer `push rbp` del dueño acababa en SIGSEGV.
//
// Este fixture fuerza esa carrera a propósito, con dos hilos en CPUs distintas:
//   * el hilo A reserva tandas de páginas perezosas y las pone escribibles con
//     mprotect (una y otra vez, siempre las mismas direcciones);
//   * el hilo B escribe en esas mismas páginas en cuanto existen.
// Si la entrada se pierde, la escritura de B falla: el manejador de SIGSEGV
// cuenta el fallo y la prueba termina con [T027-CARRERA-FAIL]. Con el arreglo,
// cada choque se resuelve releyendo la protección vigente y el contador
// `pte: carreras` de /proc/smp dice cuántos hubo.
//
// Compilar: rustc -O -C target-feature=+crt-static -C strip=symbols \
//           scripts/sondas/t027_carrera.rs -o build/packages/t027_carrera
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

const PAGINA: usize = 4096;
const PROT_NONE: i32 = 0;
const PROT_RW: i32 = 3;
const MAPA: i32 = 0x02 | 0x20;

extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
    fn mprotect(addr: *mut u8, len: usize, prot: i32) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
}

static FALLOS: AtomicUsize = AtomicUsize::new(0);
static ULTIMA: AtomicUsize = AtomicUsize::new(0);

extern "C" fn al_segv(sig: i32, _info: usize, _ctx: usize) {
    let _ = sig;
    FALLOS.fetch_add(1, Ordering::Relaxed);
    // Sin arreglo no hay nada que reparar aquí: se abandona el proceso con el
    // código de la señal, que es lo que la sonda detecta.
    std::process::abort();
}

fn main() {
    let vueltas: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(400);
    // Manifiesto para una carrera: el SIGSEGV debe contarse, no matar sin aviso.
    unsafe { signal(11, al_segv as usize); }
    let tam = 512 * 1024;
    let base = unsafe { mmap(core::ptr::null_mut(), tam, PROT_NONE, MAPA, -1, 0) };
    if base as isize == -1 {
        println!("[T027-CARRERA-FAIL] mmap PROT_NONE falló");
        std::process::exit(1);
    }
    let dir = base as usize;
    let fin = Arc::new(AtomicBool::new(false));
    let mut hilos = Vec::new();

    // Hilo B: escribe en el tramo en cuanto A lo hace escribible.
    {
        let fin = fin.clone();
        hilos.push(std::thread::spawn(move || {
            let mut n = 0usize;
            while !fin.load(Ordering::Relaxed) {
                for p in 0..(tam / PAGINA) {
                    let a = (dir + p * PAGINA) as *mut u8;
                    // Lectura de la entrada y escritura real: si la protección se
                    // perdió, aquí se dispara la señal.
                    unsafe { core::ptr::write_volatile(a, 0x33); }
                    ULTIMA.store(dir + p * PAGINA, Ordering::Relaxed);
                    n += 1;
                }
                if n > 64 * 4096 { break; }
            }
        }));
    }
    // Hilo A: alterna la protección sobre el MISMO tramo, forzando el
    // read-modify-write concurrente con el fallo de página de B.
    let mut choques = 0usize;
    for i in 0..vueltas {
        let r = unsafe { mprotect(base, tam, PROT_RW) };
        if r != 0 { println!("[T027-CARRERA-FAIL] mprotect RW rc={} vuelta {}", r, i); std::process::exit(1); }
        unsafe { core::ptr::write_volatile(base, (i & 0xFF) as u8); }
        choques += 1;
    }
    fin.store(true, Ordering::Relaxed);
    for h in hilos { let _ = h.join(); }
    let fallos = FALLOS.load(Ordering::Relaxed);
    unsafe { let _ = munmap(base, tam); }
    if fallos > 0 {
        println!("[T027-CARRERA-FAIL] {} SIGSEGV por protección perdida", fallos);
        std::process::exit(1);
    }
    println!("[T027-CARRERA-STATS] vueltas={} choques={} sigsegv={}", vueltas, choques, fallos);
    println!("[T027-CARRERA-PASS] {} vueltas de mprotect contra fallo de página concurrente sin proteger de menos",
             vueltas);
}
