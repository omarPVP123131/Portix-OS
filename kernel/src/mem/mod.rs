// kernel/src/mem/mod.rs

pub mod allocator;  // ← pub, no solo mod

use core::sync::atomic::Ordering;

pub const HEAP_START:   usize = 0x0100_0000;
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