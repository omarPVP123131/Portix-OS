//! Asignador de Ring 3 de `portix-rt`.
//!
//! [CIERRE B-3] Sustituye al "bump allocator" que hacía DOS syscalls `brk`
//! por cada `alloc` y nunca liberaba (≈45 730 ciclos por `alloc+free`
//! medidos con `portix-bench`). Diseño:
//!
//! * La arena crece con `brk` en trozos de `CHUNK` (64 KiB), así la mayoría de
//!   asignaciones no entran al kernel.
//! * Clases de tamaño en potencias de dos (16 B .. 32 KiB) con listas libres
//!   intrusivas: `dealloc` devuelve el bloque a su clase y `alloc` lo reutiliza.
//! * Peticiones mayores de 32 KiB (o con alineación > 4 KiB) se sirven
//!   directamente del final de la arena y, al liberarse, se devuelven a una
//!   lista de bloques grandes por tamaño exacto para reutilizarlas.
//!
//! Un cerrojo atómico simple protege el estado (los procesos con hilos
//! `clone(CLONE_VM)` comparten el heap).

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, Ordering};
use crate::syscall;

const CHUNK: usize = 64 * 1024;
const MIN_CLASS_SHIFT: usize = 4;              // 16 B
const MAX_CLASS_SHIFT: usize = 15;             // 32 KiB
const NUM_CLASSES: usize = MAX_CLASS_SHIFT - MIN_CLASS_SHIFT + 1;
/// [T-004] Huecos grandes recordados (antes 32: con 25 paquetes `.deb` seguidos `pkg`
/// llegaba a 244 MiB de brk y agotaba la RAM física porque los huecos no se
/// reutilizaban salvo por tamaño EXACTO ni se fusionaban).
const BIG_SLOTS: usize = 256;

struct State {
    /// Siguiente byte libre dentro de la arena actual.
    cursor: usize,
    /// Fin de la arena reservada con `brk`.
    end: usize,
    /// Cabezas de las listas libres por clase (bloques intrusivos: el puntero
    /// al siguiente se guarda en los primeros 8 bytes del bloque libre).
    free: [usize; NUM_CLASSES],
    /// Bloques grandes liberados: (dirección, tamaño). Lista pequeña y lineal.
    big: [(usize, usize); BIG_SLOTS],
}

static mut STATE: State = State { cursor: 0, end: 0, free: [0; NUM_CLASSES], big: [(0, 0); BIG_SLOTS] };
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}
fn unlock() { LOCK.store(false, Ordering::Release); }

#[inline]
fn class_of(size: usize) -> Option<usize> {
    if size > (1 << MAX_CLASS_SHIFT) { return None; }
    let mut shift = MIN_CLASS_SHIFT;
    while (1usize << shift) < size { shift += 1; }
    Some(shift - MIN_CLASS_SHIFT)
}

/// Garantiza que `[cursor, cursor+need)` esté dentro de la arena, pidiendo al
/// kernel más páginas con `brk` en múltiplos de `CHUNK`.
unsafe fn ensure(st: &mut State, need: usize) -> bool {
    if st.end == 0 {
        let cur = syscall::sys_brk(0);
        if cur <= 0 { return false; }
        st.cursor = cur as usize;
        st.end = cur as usize;
    }
    if st.cursor + need <= st.end { return true; }
    let falta = st.cursor + need - st.end;
    let grow = ((falta + CHUNK - 1) / CHUNK) * CHUNK;
    let nuevo = syscall::sys_brk(st.end + grow);
    if nuevo < (st.end + grow) as i64 { return false; }
    st.end += grow;
    true
}

pub struct PortixAllocator;

unsafe impl GlobalAlloc for PortixAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(1);
        let align = layout.align();
        lock();
        let st = &mut *core::ptr::addr_of_mut!(STATE);
        let p = alloc_locked(st, size, align);
        unlock();
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() { return; }
        let size = layout.size().max(1);
        lock();
        let st = &mut *core::ptr::addr_of_mut!(STATE);
        if let Some(c) = class_of(size.max(layout.align())) {
            // Bloque de clase: lo enlazamos al frente de su lista libre.
            *(ptr as *mut usize) = st.free[c];
            st.free[c] = ptr as usize;
        } else {
            // Bloque grande. [T-003] Si es el ÚLTIMO de la arena (patrón Vec que
            // dobla: 1, 2, 4, 8… MiB) se retrocede el cursor y el hueco se
            // reutiliza de inmediato; `pkg` llegaba a 238 MiB de brk con un
            // .apk de 23 MiB y agotaba la RAM física del sistema.
            let total = size.max(layout.align());
            liberar_grande(st, ptr as usize, total);
        }
        unlock();
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // [T-003] Crecimiento in situ de un bloque grande situado al final de
        // la arena: evita copiar y duplicar el búfer en cada doblado de `Vec`.
        let old = layout.size().max(1).max(layout.align());
        if class_of(old).is_none() && new_size > old {
            lock();
            let st = &mut *core::ptr::addr_of_mut!(STATE);
            if ptr as usize + old == st.cursor {
                let extra = new_size.max(layout.align()) - old;
                if ensure(st, extra) {
                    st.cursor += extra;
                    unlock();
                    return ptr;
                }
            }
            unlock();
        }
        let nuevo_layout = match Layout::from_size_align(new_size, layout.align()) { Ok(l) => l, Err(_) => return core::ptr::null_mut() };
        let nuevo = self.alloc(nuevo_layout);
        if !nuevo.is_null() {
            core::ptr::copy_nonoverlapping(ptr, nuevo, layout.size().min(new_size));
            self.dealloc(ptr, layout);
        }
        nuevo
    }
}

unsafe fn alloc_locked(st: &mut State, size: usize, align: usize) -> *mut u8 {
    // Inicializa la arena (brk(0)) antes de calcular alineaciones sobre `cursor`.
    if st.end == 0 && !ensure(st, 0) { return core::ptr::null_mut(); }
    // 1. Clases de tamaño (la alineación natural de una clase es su tamaño,
    //    que siempre es potencia de dos ≥ 16, suficiente para align ≤ clase).
    if align <= (1 << MAX_CLASS_SHIFT) {
        if let Some(c) = class_of(size.max(align)) {
            let head = st.free[c];
            if head != 0 {
                st.free[c] = *(head as *const usize);
                return head as *mut u8;
            }
            let bsize = 1usize << (c + MIN_CLASS_SHIFT);
            // Alinear el cursor al tamaño de clase para respetar `align`.
            let aligned = (st.cursor + bsize - 1) & !(bsize - 1);
            let need = (aligned - st.cursor) + bsize;
            if !ensure(st, need) { return core::ptr::null_mut(); }
            st.cursor = aligned + bsize;
            return aligned as *mut u8;
        }
    }
    // 2. Bloques grandes: [T-004] mejor ajuste entre los huecos liberados (tamaño
    //    >= pedido, alineación respetada); el sobrante vuelve a la lista.
    let total = size.max(align);
    let mut mejor: Option<usize> = None;
    for (i, e) in st.big.iter().enumerate() {
        if e.0 == 0 { continue; }
        let ini = (e.0 + align - 1) & !(align - 1);
        if ini + total > e.0 + e.1 { continue; }
        if mejor.map(|m| e.1 < st.big[m].1).unwrap_or(true) { mejor = Some(i); }
    }
    if let Some(i) = mejor {
        let (base, tam) = st.big[i];
        st.big[i] = (0, 0);
        let ini = (base + align - 1) & !(align - 1);
        if ini > base { insertar_grande(st, base, ini - base); }
        let fin = ini + total;
        if base + tam > fin { insertar_grande(st, fin, base + tam - fin); }
        return ini as *mut u8;
    }
    let aligned = (st.cursor + align - 1) & !(align - 1);
    let need = (aligned - st.cursor) + total;
    if !ensure(st, need) { return core::ptr::null_mut(); }
    st.cursor = aligned + total;
    aligned as *mut u8
}

/// Inserta un hueco grande en la tabla (sin fusionar). Si la tabla está llena, el
/// hueco se pierde hasta que el proceso termine.
unsafe fn insertar_grande(st: &mut State, base: usize, tam: usize) {
    if tam == 0 { return; }
    if let Some(slot) = st.big.iter_mut().find(|e| e.0 == 0) { *slot = (base, tam); }
}

/// Libera un bloque grande: fusiona con huecos vecinos y, si queda al final de
/// la arena, retrocede el cursor (patrón `Vec` que dobla: 1, 2, 4, 8… MiB).
unsafe fn liberar_grande(st: &mut State, mut base: usize, mut tam: usize) {
    // fusión con vecinos (iterar hasta que no haya cambios)
    loop {
        let mut hubo = false;
        for e in st.big.iter_mut() {
            if e.0 == 0 { continue; }
            if e.0 + e.1 == base { base = e.0; tam += e.1; *e = (0, 0); hubo = true; }
            else if base + tam == e.0 { tam += e.1; *e = (0, 0); hubo = true; }
        }
        if !hubo { break; }
    }
    if base + tam == st.cursor { st.cursor = base; return; }
    insertar_grande(st, base, tam);
}

#[alloc_error_handler]
fn alloc_error_handler(_layout: Layout) -> ! {
    let msg = b"[PORTIX-RT] Error: Out of Memory (OOM)\n";
    syscall::sys_write(2, msg.as_ptr(), msg.len());
    syscall::sys_exit(1)
}
