// kernel/src/drivers/mmio.rs — Acceso a MMIO y DMA físico contiguo (100% Rust no_std)
//
// Este módulo es la pieza que convierte los drivers bare-metal en drivers
// verificables: para hablar con un controlador real hacen falta dos cosas que el
// kernel no tenía centralizadas.
//
//   1. LECTURA/ESCRITURA MMIO de verdad: registros mapeados en memoria con
//      `read_volatile`/`write_volatile` (sin caché del compilador), con la misma
//      disciplina en todos los drivers y sin punteros sueltos por el árbol.
//   2. MEMORIA FÍSICA CONTIGUA para DMA: los controladores (NVMe, AHCI, xHCI)
//      programan direcciones FÍSICAS en sus registros. El único camino honesto
//      para obtenerlas es preguntarle a las tablas de página del propio kernel
//      (`paging::translate`) en lugar de suponer que la memoria está mapeada en
//      identidad: si algún día el layout cambia, el driver sigue siendo correcto.
//
// Todo el almacenamiento DMA se reserva con el asignador del kernel, alineado a
// página y puesto a cero, y se libera explícitamente cuando el driver deja de
// necesitarlo.
#![allow(dead_code)]

use crate::mem::paging;

/// Descriptor de un búfer de DMA: su dirección virtual (para el CPU) y su
/// dirección física (para el dispositivo). Ambas describen LA MISMA memoria.
#[derive(Clone, Copy, Debug)]
pub struct DmaBuffer {
    pub vaddr: usize,
    pub paddr: u64,
    pub len: usize,
}

impl DmaBuffer {
    pub const fn empty() -> Self {
        DmaBuffer { vaddr: 0, paddr: 0, len: 0 }
    }

    pub fn is_valid(&self) -> bool {
        self.vaddr != 0 && self.paddr != 0 && self.len != 0
    }

    /// Vista de bytes del búfer (para construir comandos, leer identificación…).
    pub fn bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.vaddr as *const u8, self.len) }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.vaddr as *mut u8, self.len) }
    }

    pub fn as_u32(&self, index: usize) -> u32 {
        if index >= self.len / 4 {
            return 0;
        }
        unsafe { core::ptr::read_volatile((self.vaddr as *const u32).add(index)) }
    }
}

/// Copia DESDE un búfer DMA hacia memoria normal, con lecturas volátiles.
///
/// Es obligatorio tras un DMA: el controlador escribió en memoria física y el
/// compilador no lo sabe. Sin lecturas volátiles puede "recordar" los ceros que
/// escribimos antes de lanzar el comando (propagación de almacenamiento a carga)
/// y devolver un búfer vacío aunque el hardware haya transferido de verdad —
/// un fallo silencioso que se ve como "el disco responde pero sin datos".
pub fn copiar_desde_dma(buf: &DmaBuffer, destino: &mut [u8]) {
    let n = destino.len().min(buf.len);
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    for i in 0..n {
        destino[i] = unsafe { core::ptr::read_volatile((buf.vaddr + i) as *const u8) };
    }
}

/// Copia HACIA un búfer DMA con escrituras volátiles, para que el controlador
/// vea exactamente lo que el CPU escribió antes de tocar el timbre.
pub fn copiar_hacia_dma(buf: &DmaBuffer, origen: &[u8]) {
    let n = origen.len().min(buf.len);
    for i in 0..n {
        unsafe { core::ptr::write_volatile((buf.vaddr + i) as *mut u8, origen[i]) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Copia HACIA un búfer DMA en un desplazamiento dado (para limpiar por tramos).
pub fn copiar_hacia_dma_offset(buf: &DmaBuffer, off: usize, origen: &[u8]) {
    copiar_hacia_dma_slice(buf, off, origen);
}

fn copiar_hacia_dma_slice(buf: &DmaBuffer, off: usize, origen: &[u8]) {
    let n = origen.len().min(buf.len.saturating_sub(off));
    for i in 0..n {
        unsafe { core::ptr::write_volatile((buf.vaddr + off + i) as *mut u8, origen[i]) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Reserva un búfer DMA puesto a cero, alineado (mínimo a página).
///
/// Devuelve `None` si no hay memoria o si la dirección física no es alcanzable
/// por un dispositivo (memoria alta): en ese caso el llamador debe degradar
/// explícitamente en lugar de programar un puntero inválido en el hardware.
pub fn dma_alloc(len: usize, align: usize) -> Option<DmaBuffer> {
    let align = align.max(paging::PAGE_SIZE);
    let len_alineado = (len + align - 1) & !(align - 1);
    let layout = core::alloc::Layout::from_size_align(len_alineado, align).ok()?;
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        return None;
    }
    let vaddr = ptr as usize;
    // La dirección física se obtiene de las tablas de página reales.
    let cr3 = paging::read_cr3();
    let paddr = match paging::translate(cr3, vaddr) {
        Some((p, _flags)) => p as u64,
        None => {
            unsafe { alloc::alloc::dealloc(ptr, layout) };
            return None;
        }
    };
    // Los controladores de PC hablan con memoria de 32 bits (o con buses que
    // sólo direccionan los primeros 4 GiB). Fuera de ese rango, no es utilizable.
    if paddr == 0 || paddr + len_alineado as u64 > 0x1_0000_0000 {
        unsafe { alloc::alloc::dealloc(ptr, layout) };
        return None;
    }
    Some(DmaBuffer { vaddr, paddr, len: len_alineado })
}

/// Libera un búfer reservado con `dma_alloc`. Idempotente con búferes vacíos.
pub fn dma_free(buf: &mut DmaBuffer) {
    if buf.vaddr == 0 {
        return;
    }
    if let Ok(layout) = core::alloc::Layout::from_size_align(buf.len, paging::PAGE_SIZE) {
        unsafe { alloc::alloc::dealloc(buf.vaddr as *mut u8, layout) };
    }
    *buf = DmaBuffer::empty();
}

/// Mapea en el espacio del kernel las páginas de un BAR de MMIO.
///
/// El mapa de identidad del cargador cubre únicamente los primeros 96 MiB de
/// memoria (y el framebuffer), de modo que los BAR de los controladores PCI
/// —que el firmware sitúa por encima de 0xF000_0000— NO serían accesibles: la
/// primera lectura provocaría un fallo de página. Esta función mapea en
/// identidad el rango físico del BAR con CACHÉ DESHABILITADA, que es como debe
/// accederse a registros de dispositivo (una lectura cacheada devolvería valores
/// viejos y las escrituras podrían no llegar nunca al hardware).
///
/// Devuelve `true` si el rango queda accesible.
pub fn mapear_mmio(paddr: u64, bytes: usize) -> bool {
    if paddr == 0 || bytes == 0 {
        return false;
    }
    let inicio = (paddr as usize) & !(paging::PAGE_SIZE - 1);
    let fin = ((paddr as usize) + bytes + paging::PAGE_SIZE - 1) & !(paging::PAGE_SIZE - 1);
    let cr3 = paging::read_cr3();
    // Si ya está mapeado (por ejemplo, el framebuffer del cargador) no se toca.
    if let Some(_) = paging::translate(cr3, inicio) {
        return true;
    }
    let paginas = (fin - inicio) / paging::PAGE_SIZE;
    // PRESENT | WRITABLE | PCD (caché deshabilitada)
    const FLAGS_MMIO: u64 = 0x1 | 0x2 | 0x10;
    let mut ok = true;
    for i in 0..paginas {
        let dir = inicio + i * paging::PAGE_SIZE;
        if paging::map_page(cr3, dir, dir, FLAGS_MMIO).is_err() {
            ok = false;
            break;
        }
        paging::flush_tlb_page(dir);
    }
    if ok {
        crate::drivers::serial::write_str("[MMIO] Rango de registros mapeado en identidad: ");
        crate::drivers::serial::write_hex(inicio);
        crate::drivers::serial::write_str(" - ");
        crate::drivers::serial::write_hex(fin);
        crate::drivers::serial::write_str(" (");
        crate::drivers::serial::write_usize(paginas);
        crate::drivers::serial::write_str(" paginas, cache deshabilitada)\n");
    } else {
        crate::drivers::serial::write_str("[MMIO] NO se pudo mapear el rango de registros ");
        crate::drivers::serial::write_hex(inicio);
        crate::drivers::serial::write_str("\n");
    }
    ok && paging::translate(cr3, inicio).is_some()
}

// ── Accesos MMIO ─────────────────────────────────────────────────────────────
// Volátiles y con barrera de compilación: el compilador no puede reordenar ni
// eliminar accesos a registros de dispositivo.

#[inline(always)]
pub fn mmio_read8(base: u64, off: usize) -> u8 {
    unsafe { core::ptr::read_volatile((base as usize + off) as *const u8) }
}

#[inline(always)]
pub fn mmio_read16(base: u64, off: usize) -> u16 {
    unsafe { core::ptr::read_volatile((base as usize + off) as *const u16) }
}

#[inline(always)]
pub fn mmio_read32(base: u64, off: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base as usize + off) as *const u32) }
}

#[inline(always)]
pub fn mmio_read64(base: u64, off: usize) -> u64 {
    unsafe { core::ptr::read_volatile((base as usize + off) as *const u64) }
}

#[inline(always)]
pub fn mmio_write8(base: u64, off: usize, val: u8) {
    unsafe { core::ptr::write_volatile((base as usize + off) as *mut u8, val) };
}

#[inline(always)]
pub fn mmio_write16(base: u64, off: usize, val: u16) {
    unsafe { core::ptr::write_volatile((base as usize + off) as *mut u16, val) };
}

#[inline(always)]
pub fn mmio_write32(base: u64, off: usize, val: u32) {
    unsafe { core::ptr::write_volatile((base as usize + off) as *mut u32, val) };
}

#[inline(always)]
pub fn mmio_write64(base: u64, off: usize, val: u64) {
    unsafe { core::ptr::write_volatile((base as usize + off) as *mut u64, val) };
}

/// Espera a que una condición de un registro MMIO se cumpla, con límite de
/// iteraciones. Devuelve `false` si el dispositivo no responde (jamás se cuelga
/// el kernel esperando a hardware que no existe).
pub fn mmio_wait(mut cond: impl FnMut() -> bool, limite: u32) -> bool {
    for _ in 0..limite {
        if cond() {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

/// Barrera de memoria para ordenar accesos MMIO frente a escrituras de datos
/// (por ejemplo: escribir el comandos en la cola ANTES de tocar el doorbell).
#[inline(always)]
pub fn barrera() {
    unsafe { core::arch::asm!("mfence", options(nostack, preserves_flags)) };
}
