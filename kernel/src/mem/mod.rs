// kernel/src/mem/mod.rs

pub mod allocator;
pub mod paging;
pub mod frames;
pub mod marcos;
pub mod vmas;
pub mod zswap;
pub mod hugepages;

use core::sync::atomic::Ordering;

/// Dirección FÍSICA del montículo del núcleo (16 MiB..80 MiB).
pub const HEAP_PHYS_START: usize = 0x0100_0000;
/// [CICLO 50 / Fase B.2] El montículo se direcciona por su alias ALTO del mapa
/// físico directo (0xFFFFFFFF80000000 + físico), nunca por la identidad plana.
/// Todo puntero de `alloc` es por tanto de la mitad superior; quien necesite la
/// física (tablas de páginas, DMA) usa `paging::virt_to_phys`.
pub const HEAP_START:   usize = paging::KERNEL_VMA_OFFSET + HEAP_PHYS_START;
pub const HEAP_SIZE:    usize = 64 * 1024 * 1024;
pub const MIN_ORDER:    usize = 4;
pub const MAX_ORDER:    usize = 22;
pub const ORDER_COUNT:  usize = MAX_ORDER - MIN_ORDER + 1;

pub fn alloc_stats_free_total() -> usize {
    let mut total = 0usize;
    for cell in &allocator::ALLOC_STATS.free_blocks {
        total += cell.load(Ordering::Relaxed);
    }
    total
}