// kernel/src/drivers/gpu/gem_ttm.rs — Asignador de Memoria de Vídeo GEM / TTM
// Fases 551 - 560: Buffer Objects (BOs), Placements en VRAM/GTT/RAM, y Paginación de VRAM.

use alloc::vec::Vec;
use alloc::string::String;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use crate::arch::spinlock::Spinlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryDomain {
    DedicatedVram,
    GttMapped,
    SystemRam,
}

#[derive(Debug, Clone)]
pub struct BufferObject {
    pub id: u32,
    pub size_bytes: usize,
    pub domain: MemoryDomain,
    pub is_pinned: bool,
    pub physical_address: usize,
    pub virtual_address: usize,
    pub is_dirty: bool,
}

pub struct TtmManager {
    pub buffer_objects: Vec<BufferObject>,
    pub next_bo_id: u32,
    pub total_vram_bytes: usize,
    pub used_vram_bytes: AtomicUsize,
    pub total_gtt_bytes: usize,
    pub used_gtt_bytes: AtomicUsize,
    pub evictions_count: AtomicU64,
}

impl TtmManager {
    pub fn new() -> Self {
        Self {
            buffer_objects: Vec::new(),
            next_bo_id: 1,
            total_vram_bytes: 8 * 1024 * 1024 * 1024, // 8 GB VRAM simulada/gestionada
            used_vram_bytes: AtomicUsize::new(0),
            total_gtt_bytes: 4 * 1024 * 1024 * 1024,  // 4 GB GTT
            used_gtt_bytes: AtomicUsize::new(0),
            evictions_count: AtomicU64::new(0),
        }
    }

    /// Asigna un Buffer Object (GEM BO) en VRAM o GTT
    pub fn allocate_bo(&mut self, size_bytes: usize, domain: MemoryDomain, pinned: bool) -> Result<u32, &'static str> {
        let aligned_size = (size_bytes + 4095) & !4095;
        let id = self.next_bo_id;
        self.next_bo_id += 1;

        match domain {
            MemoryDomain::DedicatedVram => {
                let current_used = self.used_vram_bytes.load(Ordering::Relaxed);
                if current_used + aligned_size > self.total_vram_bytes {
                    return Err("VRAM agotada (requiere desalojo TTM a GTT)");
                }
                self.used_vram_bytes.fetch_add(aligned_size, Ordering::SeqCst);
            }
            MemoryDomain::GttMapped | MemoryDomain::SystemRam => {
                self.used_gtt_bytes.fetch_add(aligned_size, Ordering::SeqCst);
            }
        }

        let paddr = 0xD000_0000 + (id as usize * 0x0020_0000);
        let vaddr = 0xFFFF_9000_0000_0000 + (id as usize * 0x0020_0000);

        self.buffer_objects.push(BufferObject {
            id,
            size_bytes: aligned_size,
            domain,
            is_pinned: pinned,
            physical_address: paddr,
            virtual_address: vaddr,
            is_dirty: false,
        });

        Ok(id)
    }

    /// Migra un Buffer Object entre VRAM y memoria del sistema (TTM Migration)
    pub fn migrate_bo(&mut self, bo_id: u32, target_domain: MemoryDomain) -> Result<(), &'static str> {
        let bo = self.buffer_objects.iter_mut().find(|b| b.id == bo_id)
            .ok_or("Buffer Object no encontrado")?;

        if bo.is_pinned {
            return Err("No se puede migrar un BO anclado (pinned)");
        }

        if bo.domain != target_domain {
            if target_domain == MemoryDomain::GttMapped && bo.domain == MemoryDomain::DedicatedVram {
                self.used_vram_bytes.fetch_sub(bo.size_bytes, Ordering::SeqCst);
                self.used_gtt_bytes.fetch_add(bo.size_bytes, Ordering::SeqCst);
                self.evictions_count.fetch_add(1, Ordering::Relaxed);
            } else if target_domain == MemoryDomain::DedicatedVram {
                self.used_gtt_bytes.fetch_sub(bo.size_bytes, Ordering::SeqCst);
                self.used_vram_bytes.fetch_add(bo.size_bytes, Ordering::SeqCst);
            }
            bo.domain = target_domain;
        }

        Ok(())
    }

    pub fn get_bo(&self, bo_id: u32) -> Option<&BufferObject> {
        self.buffer_objects.iter().find(|b| b.id == bo_id)
    }
}

pub static TTM_MANAGER: Spinlock<Option<TtmManager>> = Spinlock::new(None);

pub fn init_ttm() {
    let mut lock = TTM_MANAGER.lock();
    if lock.is_none() {
        let mut mgr = TtmManager::new();
        // Asignamos el Framebuffer primario (1920x1080x4 bytes = ~8.3 MB)
        let _ = mgr.allocate_bo(1920 * 1080 * 4, MemoryDomain::DedicatedVram, true);
        // Asignamos un búfer de Cursor (64x64x4 bytes = 16 KB)
        let _ = mgr.allocate_bo(64 * 64 * 4, MemoryDomain::DedicatedVram, true);
        *lock = Some(mgr);
    }
}

pub fn get_ttm_summary() -> String {
    let lock = TTM_MANAGER.lock();
    match &*lock {
        Some(mgr) => {
            let vram_mb = mgr.used_vram_bytes.load(Ordering::Relaxed) / (1024 * 1024);
            let gtt_mb = mgr.used_gtt_bytes.load(Ordering::Relaxed) / (1024 * 1024);
            let bo_count = mgr.buffer_objects.len();
            let evictions = mgr.evictions_count.load(Ordering::Relaxed);
            alloc::format!(
                "TTM/GEM: {} BOs | VRAM Usada: {} MB / 8192 MB | GTT: {} MB | Migraciones: {}",
                bo_count, vram_mb, gtt_mb, evictions
            )
        }
        None => String::from("TTM/GEM: No inicializado"),
    }
}
