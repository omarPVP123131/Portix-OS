// kernel/src/drivers/gpu/fence.rs — Vallas de Sincronización de Hardware GPU (Sync Fences)
// Fases 551 - 560: Sincronización asíncrona CPU-GPU sin bloqueos activos.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::spinlock::Spinlock;

#[derive(Debug, Clone)]
pub struct GpuSyncFence {
    pub fence_id: u64,
    pub context_id: u32,
    pub seqno: u64,
    pub is_signaled: bool,
    pub timestamp_created: u64,
}

pub struct FenceManager {
    pub fences: Vec<GpuSyncFence>,
    pub next_fence_id: AtomicU64,
    pub signaled_count: AtomicU64,
}

impl FenceManager {
    pub fn new() -> Self {
        Self {
            fences: Vec::new(),
            next_fence_id: AtomicU64::new(1),
            signaled_count: AtomicU64::new(0),
        }
    }

    /// Emite una nueva valla de sincronización asociada a un comando de GPU
    pub fn create_fence(&mut self, context_id: u32, seqno: u64) -> u64 {
        let id = self.next_fence_id.fetch_add(1, Ordering::SeqCst);
        let tsc: u64;
        unsafe {
            core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nostack, nomem));
        }

        self.fences.push(GpuSyncFence {
            fence_id: id,
            context_id,
            seqno,
            is_signaled: false,
            timestamp_created: tsc,
        });

        id
    }

    /// Marca una valla como completada cuando la GPU termina de procesar
    pub fn signal_fence(&mut self, fence_id: u64) -> bool {
        if let Some(fence) = self.fences.iter_mut().find(|f| f.fence_id == fence_id) {
            if !fence.is_signaled {
                fence.is_signaled = true;
                self.signaled_count.fetch_add(1, Ordering::SeqCst);
                return true;
            }
        }
        false
    }

    pub fn is_fence_signaled(&self, fence_id: u64) -> bool {
        self.fences.iter().find(|f| f.fence_id == fence_id)
            .map(|f| f.is_signaled)
            .unwrap_or(false)
    }
}

pub static FENCE_MANAGER: Spinlock<Option<FenceManager>> = Spinlock::new(None);

pub fn init_fences() {
    let mut lock = FENCE_MANAGER.lock();
    if lock.is_none() {
        *lock = Some(FenceManager::new());
    }
}
