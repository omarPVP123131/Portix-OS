// kernel/src/drivers/gpu/ring.rs — Anillos de Comandos DMA de GPU (Command Submission Ring)
// Fases 551 - 560: Envío directo de paquetes de dibujo y cómputo sin intervención continua de CPU.

use alloc::string::String;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::arch::spinlock::Spinlock;

pub const RING_SIZE_DWORDS: usize = 1024;

pub const PACKET_NOP: u32 = 0x00;
pub const PACKET_SET_STATE: u32 = 0x01;
pub const PACKET_DRAW_RECT: u32 = 0x02;
pub const PACKET_BLIT: u32 = 0x03;
pub const PACKET_FENCE_EMIT: u32 = 0x04;

pub struct GpuCommandRing {
    pub ring_buffer: [u32; RING_SIZE_DWORDS],
    pub rptr: AtomicU32, // Read pointer (GPU)
    pub wptr: AtomicU32, // Write pointer (CPU)
    pub packets_processed: AtomicU64,
}

impl GpuCommandRing {
    pub const fn new() -> Self {
        Self {
            ring_buffer: [0; RING_SIZE_DWORDS],
            rptr: AtomicU32::new(0),
            wptr: AtomicU32::new(0),
            packets_processed: AtomicU64::new(0),
        }
    }

    /// Envía un comando de dibujo o configuración al anillo DMA
    pub fn emit_packet(&mut self, opcode: u32, arg0: u32, arg1: u32, arg2: u32) -> Result<(), &'static str> {
        let w = self.wptr.load(Ordering::Relaxed) as usize;
        let r = self.rptr.load(Ordering::Relaxed) as usize;

        // Comprobamos si hay espacio para 4 dwords
        let next_w = (w + 4) % RING_SIZE_DWORDS;
        if next_w == r {
            return Err("Anillo de comandos de GPU saturado");
        }

        self.ring_buffer[w] = opcode;
        self.ring_buffer[(w + 1) % RING_SIZE_DWORDS] = arg0;
        self.ring_buffer[(w + 2) % RING_SIZE_DWORDS] = arg1;
        self.ring_buffer[(w + 3) % RING_SIZE_DWORDS] = arg2;

        self.wptr.store(next_w as u32, Ordering::Release);
        self.packets_processed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Simula el avance y vaciado de la GPU consumiendo paquetes pendientes
    pub fn flush_gpu(&mut self) {
        let w = self.wptr.load(Ordering::Acquire);
        self.rptr.store(w, Ordering::Release);
    }
}

pub static GPU_COMMAND_RING: Spinlock<GpuCommandRing> = Spinlock::new(GpuCommandRing::new());

pub fn get_ring_summary() -> String {
    let ring = GPU_COMMAND_RING.lock();
    let r = ring.rptr.load(Ordering::Relaxed);
    let w = ring.wptr.load(Ordering::Relaxed);
    let total = ring.packets_processed.load(Ordering::Relaxed);
    alloc::format!(
        "GPU DMA Ring: ReadPtr={}, WritePtr={}, Paquetes Procesados={}",
        r, w, total
    )
}
