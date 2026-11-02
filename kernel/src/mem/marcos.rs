// kernel/src/mem/marcos.rs — [CICLO 50 / Fase B.4] Asignador de marcos físicos
//
// Hasta B.4 TODOS los marcos de usuario (imagen ELF, pila, brk, mmap, copias
// COW) salían del montículo del núcleo (`alloc_zeroed`), que vive en los 64 MiB
// físicos 0x1000000–0x5000000. Consecuencias reales:
//   • el espacio de usuario de toda la máquina cabía en lo que sobrase del
//     montículo (compartido con tablas de páginas, buffers de disco, red, etc.);
//   • el resto de la RAM utilizable que anuncia el firmware (E820/UEFI) quedaba
//     sin usar: con `-m 512M` se desperdiciaban ~430 MiB;
//   • el rango virtual 0x1000000–0x5000000 se mantenía vedado a los procesos
//     (`ZONAS_IDENTIDAD_RESERVADAS`) por precaución.
//
// Este módulo es un asignador de marcos de 4 KiB independiente del montículo:
//   • mapa de bits estático (sin asignaciones dinámicas dentro del gestor de
//     memoria), protegido por cerrojo de giro; bit a 1 = marco LIBRE;
//   • cubre la RAM utilizable que el mapa de memoria del arranque (bootinfo,
//     BIOS o UEFI) declara entre el fin del back buffer (0x5300000) y el límite
//     del mapa físico directo del núcleo (1 GiB), de modo que todo marco es
//     accesible por `phys_to_virt` sin tocar la identidad plana;
//   • `asignar` devuelve el marco a cero; `asignar_contiguos` sirve la pila de
//     usuario (16 marcos seguidos) con primer ajuste;
//   • si el firmware no entregó mapa (o se agotan los marcos) el llamador cae al
//     montículo y se CONTABILIZA (`RESPALDO_HEAP`): la prueba exige 0.
//
// El montículo (`allocator.rs`, zona protegida) no se toca: sigue sirviendo
// tablas de páginas, pilas de núcleo y estructuras internas.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::spinlock::Spinlock;
use crate::drivers::serial;
use crate::mem::paging::{BACKBUF_PHYS, BACKBUF_LEN, PAGE_SIZE};

/// Primer marco administrable: justo tras el back buffer del compositor.
pub const MARCOS_BASE: usize = BACKBUF_PHYS + BACKBUF_LEN; // 0x0530_0000
/// Límite: el mapa físico directo del núcleo cubre 1 GiB (HH_PD, 512 × 2 MiB).
pub const MARCOS_LIMITE: usize = 0x4000_0000;
const TOTAL_POSIBLES: usize = (MARCOS_LIMITE - MARCOS_BASE) / PAGE_SIZE; // 241 408
const PALABRAS: usize = (TOTAL_POSIBLES + 63) / 64;

struct Estado {
    libre: [u64; PALABRAS],
    /// [T-041] Marco que el firmware declaró RAM utilizable. `liberar` sólo
    /// acepta estos: un físico del hueco PCI/MMIO está dentro de
    /// [MARCOS_BASE, MARCOS_LIMITE) pero no es memoria (escribir ahí se pierde
    /// y leer devuelve 0), y marcarlo libre lo entregaba al espacio de usuario.
    existe: [u64; PALABRAS],
    total: usize,
    libres: usize,
    pico_usados: usize,
    siguiente: usize, // pista de búsqueda (palabra)
    iniciado: bool,
}

static ESTADO: Spinlock<Estado> = Spinlock::new(Estado {
    libre: [0; PALABRAS],
    existe: [0; PALABRAS],
    total: 0,
    libres: 0,
    pico_usados: 0,
    siguiente: 0,
    iniciado: false,
});

/// Veces que un llamador tuvo que recurrir al montículo por falta de marcos.
pub static RESPALDO_HEAP: AtomicU64 = AtomicU64::new(0);
pub static ASIGNADOS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static LIBERADOS_TOTAL: AtomicU64 = AtomicU64::new(0);
/// [T-041] Liberaciones de físicos que el firmware nunca declaró RAM (huecos). Se
/// ignoran: si se aceptaran, `asignar` las entregaría como marcos de usuario.
pub static LIBERADOS_IGNORADOS: AtomicU64 = AtomicU64::new(0);
/// [T-003] Peticiones rechazadas por falta de marcos Y de margen en el montículo.
pub static RESPALDO_DENEGADO: AtomicU64 = AtomicU64::new(0);

#[inline]
fn indice(paddr: usize) -> usize { (paddr - MARCOS_BASE) / PAGE_SIZE }

/// `true` si `paddr` (física o alias alto) pertenece al rango administrado.
/// No implica que esté asignado: sólo que su dueño es este asignador.
pub fn es_marco(paddr: usize) -> bool {
    let p = crate::mem::paging::virt_to_phys(paddr);
    if p < MARCOS_BASE || p >= MARCOS_LIMITE { return false; }
    let i = indice(p);
    let e = ESTADO.lock();
    e.iniciado && e.existe[i / 64] & (1u64 << (i % 64)) != 0
}

/// Construye el mapa de bits a partir del mapa de memoria del arranque.
/// Devuelve el número de marcos utilizables registrados.
pub fn init() -> usize {
    let mut e = ESTADO.lock();
    if e.iniciado { return e.total; }
    let mut registrados = 0usize;
    if let Some(boot) = crate::bootinfo::get() {
        for r in crate::bootinfo::memory_regions(boot) {
            if r.kind != crate::bootinfo::MEM_USABLE_MAPPED && r.kind != crate::bootinfo::MEM_USABLE_UNMAPPED {
                continue;
            }
            let ini = (r.base as usize).max(MARCOS_BASE);
            let fin_raw = match (r.base as usize).checked_add(r.length as usize) { Some(f) => f, None => usize::MAX };
            let fin = fin_raw.min(MARCOS_LIMITE);
            // Redondeo hacia dentro: sólo marcos COMPLETOS dentro de la región.
            let ini_al = (ini + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            let fin_al = fin & !(PAGE_SIZE - 1);
            if fin_al <= ini_al { continue; }
            let mut p = ini_al;
            while p < fin_al {
                let i = indice(p);
                e.libre[i / 64] |= 1u64 << (i % 64);
                e.existe[i / 64] |= 1u64 << (i % 64);
                registrados += 1;
                p += PAGE_SIZE;
            }
        }
    }
    e.total = registrados;
    e.libres = registrados;
    e.iniciado = true;
    drop(e);
    serial::write_str("[MARCOS] asignador de marcos fisicos: ");
    serial::write_usize(registrados);
    serial::write_str(" marcos (");
    serial::write_usize(registrados * PAGE_SIZE / (1024 * 1024));
    serial::write_str(" MiB) utilizables en ");
    serial::write_hex(MARCOS_BASE);
    serial::write_str("-");
    serial::write_hex(MARCOS_LIMITE);
    serial::write_str(" segun el mapa del firmware; montículo del núcleo intacto\n");
    registrados
}

fn limpiar(paddr: usize) {
    let v = crate::mem::paging::phys_to_virt(paddr) as *mut u8;
    unsafe { core::ptr::write_bytes(v, 0, PAGE_SIZE); }
}

/// Un marco de 4 KiB a cero. Devuelve la dirección FÍSICA.
/// [T-026] Perfil de `asignar` (ciclos TSC acumulados): 0 espera del cerrojo,
/// 1 recorrido del mapa, 2 `limpiar`, 3 palabras recorridas.
#[allow(clippy::declare_interior_mutable_const)]
const CERO_U64: AtomicU64 = AtomicU64::new(0);
pub static PERFIL: [AtomicU64; 4] = [CERO_U64; 4];

pub fn asignar() -> Option<usize> {
    let t0 = unsafe { core::arch::x86_64::_rdtsc() };
    let mut e = ESTADO.lock();
    let t1 = unsafe { core::arch::x86_64::_rdtsc() };
    PERFIL[0].fetch_add(t1 - t0, Ordering::Relaxed);
    if !e.iniciado || e.libres == 0 { return None; }
    let n = PALABRAS;
    let inicio = e.siguiente.min(n - 1);
    for paso in 0..n {
        let w = (inicio + paso) % n;
        let palabra = e.libre[w];
        if palabra == 0 { continue; }
        let bit = palabra.trailing_zeros() as usize;
        let i = w * 64 + bit;
        if i >= TOTAL_POSIBLES { continue; }
        e.libre[w] &= !(1u64 << bit);
        e.libres -= 1;
        e.siguiente = w;
        let usados = e.total - e.libres;
        if usados > e.pico_usados { e.pico_usados = usados; }
        drop(e);
        let t2 = unsafe { core::arch::x86_64::_rdtsc() };
        PERFIL[1].fetch_add(t2 - t1, Ordering::Relaxed);
        PERFIL[3].fetch_add(paso as u64 + 1, Ordering::Relaxed);
        let paddr = MARCOS_BASE + i * PAGE_SIZE;
        limpiar(paddr);
        PERFIL[2].fetch_add(unsafe { core::arch::x86_64::_rdtsc() } - t2, Ordering::Relaxed);
        ASIGNADOS_TOTAL.fetch_add(1, Ordering::Relaxed);
        return Some(paddr);
    }
    None
}

/// `n` marcos físicamente contiguos a cero (pila de usuario). Primer ajuste.
pub fn asignar_contiguos(n: usize) -> Option<usize> {
    if n == 0 { return None; }
    if n == 1 { return asignar(); }
    let mut e = ESTADO.lock();
    if !e.iniciado || e.libres < n { return None; }
    let mut racha = 0usize;
    let mut i = 0usize;
    while i < TOTAL_POSIBLES {
        let w = i / 64;
        if e.libre[w] == 0 { racha = 0; i = (w + 1) * 64; continue; }
        if e.libre[w] & (1u64 << (i % 64)) != 0 {
            racha += 1;
            if racha == n {
                let primero = i + 1 - n;
                for k in primero..=i {
                    e.libre[k / 64] &= !(1u64 << (k % 64));
                }
                e.libres -= n;
                let usados = e.total - e.libres;
                if usados > e.pico_usados { e.pico_usados = usados; }
                drop(e);
                let paddr = MARCOS_BASE + primero * PAGE_SIZE;
                for k in 0..n { limpiar(paddr + k * PAGE_SIZE); }
                ASIGNADOS_TOTAL.fetch_add(n as u64, Ordering::Relaxed);
                return Some(paddr);
            }
        } else {
            racha = 0;
        }
        i += 1;
    }
    None
}

/// Devuelve un marco. Ignora (y cuenta) direcciones fuera de rango o dobles
/// liberaciones: nunca corrompe el mapa.
pub fn liberar(paddr: usize) {
    let p = crate::mem::paging::virt_to_phys(paddr) & !(PAGE_SIZE - 1);
    if p < MARCOS_BASE || p >= MARCOS_LIMITE { return; }
    let i = indice(p);
    let mut e = ESTADO.lock();
    if !e.iniciado { return; }
    let w = i / 64; let b = 1u64 << (i % 64);
    if e.existe[w] & b == 0 {
        // [T-041] No es RAM declarada por el firmware: el hueco PCI/MMIO
        // cae dentro del rango del mapa de bits. Marcarlo libre lo hacía
        // asignable y el espacio de usuario acababa mapeado a memoria
        // inexistente (lecturas 0 -> ejecución de ceros -> #PF CR2=0).
        LIBERADOS_IGNORADOS.fetch_add(1, Ordering::Relaxed);
        return;
    }
    if e.libre[w] & b != 0 {
        serial::write_str("[MARCOS] AVISO: doble liberacion de 0x");
        serial::write_hex(p);
        serial::write_str("\n");
        return;
    }
    e.libre[w] |= b;
    e.libres += 1;
    if w < e.siguiente { e.siguiente = w; }
    LIBERADOS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn liberar_contiguos(paddr: usize, n: usize) {
    for k in 0..n { liberar(paddr + k * PAGE_SIZE); }
}

/// (total, libres, pico_usados, respaldo_heap)
pub fn estadisticas() -> (usize, usize, usize, u64) {
    let e = ESTADO.lock();
    (e.total, e.libres, e.pico_usados, RESPALDO_HEAP.load(Ordering::Relaxed))
}
