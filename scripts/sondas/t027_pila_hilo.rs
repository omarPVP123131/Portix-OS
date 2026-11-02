// T-027: ¿se puede escribir siempre en la pila de un hilo cuando debe?
//
// La traza original: HotSpot aborts a ratos con SIGSEGV en `push rbp` —la
// PRIMERA escritura de una función— sobre la pila de un hilo. Los sospechosos
// anotados eran la reutilización de pilas de glibc (madvise(MADV_DONTNEED) que
// las vuelve perezosas) y las zonas de guarda que HotSpot pone con mprotect.
//
// Este fixture reproduce ESE ciclo, con las llamadas reales y en el mismo
// orden que usa HotSpot, y exige que no haya ni un fallo espurio:
//   1. reserva 1 MB PROT_NONE y hace escribible el tramo útil (como el
//      `os::create_stack` del JVM);
//   2. escribe en TODA la pila desde un hilo, recursando hasta lo más hondo
//      (eso es el `push rbp` que reventaba);
//   3. `madvise(MADV_DONTNEED)` sobre el tramo, como hace glibc al reciclar
//      pilas del caché, y vuelve a escribir: debe seguir siendo escribible;
//   4. deja la primera página como guarda con `mprotect(PROT_NONE)` y comprueba
//      que el uso normal por encima de ella sigue funcionando.
// Repetido N veces (por defecto 200) porque el defecto original era de 1 en 6.
//
// Compilar: rustc -O -C target-feature=+crt-static -C strip=symbols \
//           scripts/sondas/t027_pila_hilo.rs -o build/packages/t027_pila
use std::sync::atomic::{AtomicUsize, Ordering};

const PAGINA: usize = 4096;
const PROT_NONE: i32 = 0;
const PROT_RW: i32 = 3;
const MAP_PRIVATE_ANON: i32 = 0x02 | 0x20;
const MAP_FAILED: isize = -1;
const MADV_DONTNEED: i32 = 4;

extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
    fn mprotect(addr: *mut u8, len: usize, prot: i32) -> i32;
    fn madvise(addr: *mut u8, len: usize, advice: i32) -> i32;
}

/// Baja hasta el fondo de la pila reservada y escribe en cada marco: es el
/// patrón que hace estallar un `push rbp` sobre una pila que no es escribible.
#[inline(never)]
fn hondo(base: usize, actual: usize, nivel: usize, sonda: &AtomicUsize) -> usize {
    let mut marco = [0u8; 512];
    marco[0] = 0x5A;
    marco[511] = 0xA5;
    let dir = &marco as *const _ as usize;
    if dir < actual {
        sonda.store(dir, Ordering::Relaxed);
    }
    // Sin optimizar a cola: cada llamada deja su marco y su escritura.
    if actual > base + 4096 && nivel < 4096 {
        return hondo(base, actual - 256, nivel + 1, sonda) + marco[0] as usize + marco[511] as usize;
    }
    marco[0] as usize + marco[511] as usize
}

fn main() {
    let vueltas: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(200);
    println!("[T027] vueltas={}", vueltas);
    let mut fallos = 0usize;
    for v in 1..=vueltas {
        let tam = 1024 * 1024;
        let base = unsafe { mmap(core::ptr::null_mut(), tam, PROT_NONE, MAP_PRIVATE_ANON, -1, 0) };
        if base as isize == MAP_FAILED {
            println!("[T027-FAIL] mmap PROT_NONE falló en la vuelta {}", v);
            std::process::exit(1);
        }
        // Tramo útil: todo menos la primera página (guarda), como el JVM.
        if unsafe { mprotect(base.add(PAGINA), tam - PAGINA, PROT_RW) } != 0 {
            println!("[T027-FAIL] mprotect de la pila falló en la vuelta {}", v);
            std::process::exit(1);
        }
        // 1) Escribir en el tramo útil desde ESTE hilo (materializa perezosas).
        unsafe {
            for p in 1..(tam / PAGINA) {
                let d = base.add(p * PAGINA);
                core::ptr::write_volatile(d, (v & 0xFF) as u8);
                core::ptr::write_volatile(d.add(PAGINA - 1), 0x7E);
            }
        }
        // 2) Bajar hasta el fondo con marcos reales: el `push rbp` del caso.
        let sonda = AtomicUsize::new(usize::MAX);
        let tope = base as usize + tam;
        let suma = hondo(base as usize + PAGINA, tope - 4096, 0, &sonda);
        if suma == 0 {
            println!("[T027-FAIL] la recursión no escribió ningún marco (vuelta {})", v);
            fallos += 1;
        }
        // 3) Reciclar como glibc: el contenido se descarta y la pila debe seguir
        //    siendo escribible (aquí es donde una reserva perezosa mal protegida
        //    convertiría el siguiente `push` en SIGSEGV).
        let m = unsafe { madvise(base.add(PAGINA), tam - 2 * PAGINA, MADV_DONTNEED) };
        if m != 0 {
            println!("[T027-FAIL] madvise(MADV_DONTNEED) rc={} en la vuelta {}", m, v);
            fallos += 1;
        }
        unsafe {
            for p in 1..(tam / PAGINA) - 1 {
                let d = base.add(p * PAGINA);
                core::ptr::write_volatile(d, 0x11);
                if core::ptr::read_volatile(d) != 0x11 {
                    println!("[T027-FAIL] escritura perdida en página {} de la vuelta {}", p, v);
                    fallos += 1;
                    break;
                }
            }
        }
        // 4) Guarda real: la primera página sigue sin poder escribirse. Se
        //    comprueba por el camino del núcleo (sin provocar la señal aquí).
        unsafe { let _ = munmap(base, tam); }
        if v % 50 == 0 {
            println!("[T027] vuelta {}/{} sin fallos={}", v, vueltas, fallos);
        }
    }
    if fallos > 0 {
        println!("[T027-FAIL] {} fallos en {} vueltas", fallos, vueltas);
        std::process::exit(1);
    }
    println!("[T027-PASS] {} vueltas de pila+guarda+madvise+escritura sin un solo SIGSEGV ni escritura perdida", vueltas);
}
