// [T-026 / T-043] Reproducción mínima del bucle caro de `sys_mmap` perezoso.
//
// El JVM reserva una zona, la materializa y después la vuelve a mapear con
// MAP_FIXED. En esa rama el núcleo recorría las páginas una a una invalidando la
// TLB con espera de acuse en cada una (dos veces por página: `unmap_page` y
// `mapear_perezosa`). Medido en QEMU/TCG: 213 ms seguidos con las interrupciones
// cerradas para 1 MB, y por tanto pérdida de ticks del PIT (T-026).
//
// Este binario hace exactamente eso con 8 MB y mide lo que tarda la segunda
// llamada. Es un ELF Linux normal (ABI `syscall`), así que la ventana con IF=0
// es la de verdad.
//
// Antes del arreglo: ~1,7 s de pared y un tramo continuo de cientos de ms.
// Después: unos pocos ms y ningún tramo largo.
//
// Compilar:  rustc -O -C target-feature=+crt-static scripts/sondas/t026_mmapfixed.rs \
//                -o build/packages/t026_mmapfixed
use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> i32;
    fn sched_setaffinity(pid: i32, len: usize, mascara: *const u64) -> i32;
    fn clock_gettime(reloj: i32, ts: *mut Timespec) -> i32;
}

#[repr(C)]
struct Timespec { s: i64, ns: i64 }
const CLOCK_MONOTONIC: i32 = 1;

/// Microsegundos de reloj monótono del núcleo. Se usa `clock_gettime` del
/// sistema y NO `std::time::Instant`, que en Portix no tiene vDSO y devuelve 0.
fn ahora_us() -> i64 {
    let mut t = Timespec { s: 0, ns: 0 };
    unsafe { clock_gettime(CLOCK_MONOTONIC, &mut t) };
    t.s * 1_000_000 + t.ns / 1_000
}

fn a_cpu(cpu: usize) {
    let m = 1u64 << cpu;
    unsafe { sched_setaffinity(0, 8, &m) };
}

static PARAR: AtomicBool = AtomicBool::new(false);

const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const MAP_PRIVATE: i32 = 0x02;
const MAP_FIXED: i32 = 0x10;
const MAP_ANONYMOUS: i32 = 0x20;
const MAP_FAILED: isize = -1;

fn main() {
    let paginas = 2048usize;              // 8 MB
    let len = paginas * 4096;
    // [CLAVE] Un hilo vivo en el OTRO CPU con el mismo espacio de direcciones es
    // lo que hace que `flush_tlb_page` tenga que invalidar en remoto: sin él el
    // núcleo detecta `cr3_en_otra_cpu == false` y el bucle sale barato. La JVM
    // tiene hilos en los dos CPUs; el fixture también los necesita para medir.
    a_cpu(0);
    let vigilante = std::thread::spawn(|| {
        a_cpu(1);
        let mut x = 0u64;
        while !PARAR.load(Ordering::Relaxed) { x = x.wrapping_add(1); std::hint::black_box(x); }
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    unsafe {
        let base = mmap(std::ptr::null_mut(), len, PROT_READ | PROT_WRITE,
                        MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if base as isize == MAP_FAILED {
            println!("[T026-ERROR] primer mmap falló");
            std::process::exit(90);
        }
        // Materializar: una escritura por página provoca el #PF perezoso.
        let p = base as *mut u8;
        for i in 0..paginas { std::ptr::write_volatile(p.add(i * 4096), (i & 0xFF) as u8); }
        // Y ahora MAP_FIXED encima: es la rama que recorría el rango entero.
        let t0 = ahora_us();
        let otra = mmap(base, len, PROT_READ | PROT_WRITE,
                        MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
        let us = ahora_us() - t0;
        if otra as isize == MAP_FAILED {
            println!("[T026-ERROR] MAP_FIXED falló");
            std::process::exit(91);
        }
        println!("[T026] MAP_FIXED sobre {} paginas materializadas: {} us", paginas, us);
        // Control: la zona nueva se puede volver a tocar sin fallar.
        for i in (0..paginas).step_by(64) { std::ptr::write_volatile(p.add(i * 4096), 0xA5); }
        let mut suma = 0u32;
        for i in (0..paginas).step_by(64) { suma += std::ptr::read_volatile(p.add(i * 4096)) as u32; }
        println!("[T026] zona remapeada legible: suma={}", suma);
        // [T-043 / munmap] El mismo bucle caro estaba en `munmap`, y además con
        // el orden peligroso (marco liberado ANTES de quitar la PTE). Se mide
        // con el hilo vigilante vivo, que es lo que obliga a invalidar en remoto.
        let t0 = ahora_us();
        let rc = munmap(base, len);
        let us_m = ahora_us() - t0;
        if rc != 0 {
            println!("[T026-ERROR] munmap devolvió {}", rc);
            std::process::exit(92);
        }
        println!("[T026] munmap de {} paginas materializadas: {} us", paginas, us_m);
    }
    PARAR.store(true, Ordering::Relaxed);
    let _ = vigilante.join();
    println!("[T026-OK] fin");
}
