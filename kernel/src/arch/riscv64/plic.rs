// kernel/src/arch/riscv64/plic.rs — Controlador de Interrupciones de Plataforma (PLIC / AIA)
// Épica 50 (Fases 3086 - 3170) — Portix OS v6.0 'Chronos'

#![allow(dead_code)]

pub const PLIC_BASE: usize = 0x0C00_0000;
pub const PLIC_PRIORITY_BASE: usize = PLIC_BASE;
pub const PLIC_PENDING_BASE: usize = PLIC_BASE + 0x1000;
pub const PLIC_ENABLE_BASE: usize = PLIC_BASE + 0x2000;
pub const PLIC_THRESHOLD_BASE: usize = PLIC_BASE + 0x20_0000;
pub const PLIC_CLAIM_BASE: usize = PLIC_BASE + 0x20_0004;

pub struct Riscv64Plic {
    pub enabled_irqs: u64,
    pub claimed_count: u64,
    pub irq_handlers_registered: usize,
}

impl Riscv64Plic {
    pub const fn new() -> Self {
        Riscv64Plic {
            enabled_irqs: 0,
            claimed_count: 0,
            irq_handlers_registered: 0,
        }
    }

    pub fn init(&mut self) {
        // Habilitar UART (IRQ 10), VirtIO-Net (IRQ 1), VirtIO-Blk (IRQ 2)
        self.enabled_irqs = (1 << 10) | (1 << 1) | (1 << 2);
        self.irq_handlers_registered = 3;
    }

    pub fn set_priority(&mut self, _irq: u32, _priority: u32) {
        // En silicio real: escritura en PLIC_PRIORITY_BASE + irq * 4
    }

    pub fn claim(&mut self) -> u32 {
        self.claimed_count += 1;
        10 // Simula UART IRQ activa
    }

    pub fn complete(&mut self, _irq: u32) {
        // En silicio real: escritura en PLIC_CLAIM_BASE
    }
}
