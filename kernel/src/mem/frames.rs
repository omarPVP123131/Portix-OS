// kernel/src/mem/frames.rs — Conteo de referencias de marcos físicos (frente C)
//
// Antes de este módulo los marcos de la imagen ELF (texto, datos, BSS) se
// asignaban del montículo del núcleo y NUNCA se devolvían: `free_address_space`
// liberaba sólo las tablas de páginas porque, con copy-on-write tras `fork`, no
// había forma de saber si otro proceso seguía mapeando el mismo marco. Cada
// `fork`+`exec` filtraba la imagen completa.
//
// Ahora cada marco de imagen lleva un contador de referencias:
//   • `retain` al asignarlo en el cargador ELF (cuenta = 1);
//   • `retain_if_tracked` por cada proceso que lo comparte en `fork`;
//   • en un fallo COW, si la cuenta es 1 el escritor es el ÚNICO dueño y la
//     página se vuelve escribible SIN copiar (copia evitada, medible);
//     si es > 1 se copia, se `release` el original y se `retain` la copia;
//   • `release` al destruir un espacio de direcciones (exit/exec): el marco se
//     devuelve al asignador sólo cuando la cuenta llega a 0.
//
// Tabla hash de direccionamiento abierto, estática (sin asignaciones dinámicas
// dentro del propio gestor de memoria) y protegida por un cerrojo de giro.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::spinlock::Spinlock;

const SLOTS: usize = 16384; // 16384 marcos = 64 MiB de imágenes rastreables
const VACIO: u64 = 0;
const TUMBA: u64 = u64::MAX;

struct Tabla {
    clave: [u64; SLOTS],
    cuenta: [u32; SLOTS],
    rastreados: usize,
}

static TABLA: Spinlock<Tabla> = Spinlock::new(Tabla {
    clave: [VACIO; SLOTS],
    cuenta: [0; SLOTS],
    rastreados: 0,
});

pub static COPIAS_EVITADAS: AtomicU64 = AtomicU64::new(0);
pub static COPIAS_REALIZADAS: AtomicU64 = AtomicU64::new(0);
pub static LIBERADOS: AtomicU64 = AtomicU64::new(0);
pub static RETENCIONES_FORK: AtomicU64 = AtomicU64::new(0);
pub static PICO_RASTREADOS: AtomicU64 = AtomicU64::new(0);
pub static TABLA_LLENA: AtomicU64 = AtomicU64::new(0);

#[inline]
fn hash(paddr: u64) -> usize {
    // Mezcla multiplicativa sobre el número de marco (los 12 bits bajos son 0).
    let x = (paddr >> 12).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((x >> 20) as usize) & (SLOTS - 1)
}

impl Tabla {
    fn buscar(&self, paddr: u64) -> Option<usize> {
        let mut i = hash(paddr);
        for _ in 0..SLOTS {
            let k = self.clave[i];
            if k == VACIO { return None; }
            if k == paddr { return Some(i); }
            i = (i + 1) & (SLOTS - 1);
        }
        None
    }

    fn insertar(&mut self, paddr: u64) -> Option<usize> {
        let mut i = hash(paddr);
        let mut tumba: Option<usize> = None;
        for _ in 0..SLOTS {
            let k = self.clave[i];
            if k == paddr { return Some(i); }
            if k == TUMBA && tumba.is_none() { tumba = Some(i); }
            if k == VACIO {
                let dest = tumba.unwrap_or(i);
                self.clave[dest] = paddr;
                self.cuenta[dest] = 0;
                return Some(dest);
            }
            i = (i + 1) & (SLOTS - 1);
        }
        if let Some(t) = tumba {
            self.clave[t] = paddr;
            self.cuenta[t] = 0;
            return Some(t);
        }
        None
    }
}

#[inline]
fn valido(paddr: usize) -> bool {
    paddr != 0 && (paddr & 0xFFF) == 0 && crate::mem::paging::is_dynamic_addr(paddr)
}

/// Registra el marco (o incrementa su cuenta). Devuelve la cuenta resultante.
pub fn retain(paddr: usize) -> u32 {
    let paddr = crate::mem::paging::virt_to_phys(paddr); // [Fase B.2] clave siempre física
    if !valido(paddr) { return 0; }
    let mut t = TABLA.lock();
    let Some(i) = t.insertar(paddr as u64) else {
        TABLA_LLENA.fetch_add(1, Ordering::Relaxed);
        return 0;
    };
    if t.cuenta[i] == 0 {
        t.rastreados += 1;
        let r = t.rastreados as u64;
        if r > PICO_RASTREADOS.load(Ordering::Relaxed) { PICO_RASTREADOS.store(r, Ordering::Relaxed); }
    }
    t.cuenta[i] = t.cuenta[i].saturating_add(1);
    t.cuenta[i]
}

/// Incrementa la cuenta SÓLO si el marco ya está rastreado (compartición en
/// `fork`): un marco desconocido no se adopta a medias, porque liberarlo
/// desde un único dueño dejaría colgado al otro.
pub fn retain_if_tracked(paddr: usize) -> Option<u32> {
    let paddr = crate::mem::paging::virt_to_phys(paddr); // [Fase B.2] clave siempre física
    if !valido(paddr) { return None; }
    let mut t = TABLA.lock();
    let i = t.buscar(paddr as u64)?;
    if t.cuenta[i] == 0 { return None; }
    t.cuenta[i] = t.cuenta[i].saturating_add(1);
    RETENCIONES_FORK.fetch_add(1, Ordering::Relaxed);
    Some(t.cuenta[i])
}

/// Decrementa la cuenta. `Some(0)` = el llamador era el último dueño y DEBE
/// liberar el marco; `None` = marco no rastreado (no tocar).
pub fn release(paddr: usize) -> Option<u32> {
    let paddr = crate::mem::paging::virt_to_phys(paddr); // [Fase B.2] clave siempre física
    if !valido(paddr) { return None; }
    let mut t = TABLA.lock();
    let i = t.buscar(paddr as u64)?;
    if t.cuenta[i] == 0 { return None; }
    t.cuenta[i] -= 1;
    let c = t.cuenta[i];
    if c == 0 {
        t.clave[i] = TUMBA;
        t.rastreados = t.rastreados.saturating_sub(1);
        LIBERADOS.fetch_add(1, Ordering::Relaxed);
    }
    Some(c)
}

/// Cuenta actual del marco (0 = no rastreado).
pub fn count(paddr: usize) -> u32 {
    let paddr = crate::mem::paging::virt_to_phys(paddr); // [Fase B.2] clave siempre física
    if !valido(paddr) { return 0; }
    let t = TABLA.lock();
    match t.buscar(paddr as u64) { Some(i) => t.cuenta[i], None => 0 }
}

/// Instantánea: (rastreados, compartidos ≥2, suma de referencias).
pub fn snapshot() -> (usize, usize, u64) {
    let t = TABLA.lock();
    let mut compartidos = 0usize;
    let mut refs = 0u64;
    for i in 0..SLOTS {
        let k = t.clave[i];
        if k == VACIO || k == TUMBA { continue; }
        let c = t.cuenta[i];
        if c >= 2 { compartidos += 1; }
        refs += c as u64;
    }
    (t.rastreados, compartidos, refs)
}

/// Informe textual para `/proc/frames` y el comando `cow`.
pub fn informe() -> alloc::string::String {
    use core::fmt::Write;
    let (rastreados, compartidos, refs) = snapshot();
    let mut s = alloc::string::String::new();
    let _ = writeln!(s, "[COW-REFCOUNT] marcos rastreados={} compartidos={} referencias={} pico={}",
                     rastreados, compartidos, refs, PICO_RASTREADOS.load(Ordering::Relaxed));
    let _ = writeln!(s, "[COW-REFCOUNT] copias evitadas (ultimo dueno)={} copias realizadas={} liberados al llegar a 0={} retenciones en fork={} tabla llena={}",
                     COPIAS_EVITADAS.load(Ordering::Relaxed), COPIAS_REALIZADAS.load(Ordering::Relaxed),
                     LIBERADOS.load(Ordering::Relaxed), RETENCIONES_FORK.load(Ordering::Relaxed),
                     TABLA_LLENA.load(Ordering::Relaxed));
    s
}
