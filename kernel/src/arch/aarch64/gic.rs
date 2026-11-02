// kernel/src/arch/aarch64/gic.rs — Controlador Genérico de Interrupciones ARM (GICv2 y GICv3)
// Manejo de Distribuidor (GICD), Redistribuidores (GICR) e Interfaz de CPU (GICC)
#![allow(dead_code)]

/// Tipos de interrupción en la arquitectura ARM GIC
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GicInterruptType {
    SGI(u8),  // Software Generated Interrupts (0 - 15) para IPI entre núcleos
    PPI(u8),  // Private Peripheral Interrupts (16 - 31) para temporizadores de CPU
    SPI(u16), // Shared Peripheral Interrupts (32 - 1019) para UART, ATA, red
}

/// Registros operacionales del Distribuidor GICv2 (GICD)
#[repr(C)]
pub struct GicDistributor {
    pub ctlr: u32,       // 0x000: Distributor Control Register
    pub typer: u32,      // 0x004: Interrupt Controller Type Register
    pub iidr: u32,       // 0x008: Implementer Identification Register
    _reserved1: [u32; 29],
    pub isenabler: [u32; 32], // 0x100: Interrupt Set-Enable Registers
    pub icenabler: [u32; 32], // 0x180: Interrupt Clear-Enable Registers
    pub ispendr: [u32; 32],   // 0x200: Interrupt Set-Pending Registers
    pub icpendr: [u32; 32],   // 0x280: Interrupt Clear-Pending Registers
    pub isactiver: [u32; 32], // 0x300: Interrupt Set-Active Registers
    pub icactiver: [u32; 32], // 0x380: Interrupt Clear-Active Registers
    pub ipriorityr: [u32; 255],// 0x400: Interrupt Priority Registers
    _reserved2: [u32; 1],
    pub itargetsr: [u32; 255], // 0x800: Interrupt Processor Targets Registers
    _reserved3: [u32; 1],
    pub icfgr: [u32; 64],     // 0xC00: Interrupt Configuration Registers (Edge/Level)
}

/// Registros operacionales de la Interfaz de CPU GICv2 (GICC)
#[repr(C)]
pub struct GicCpuInterface {
    pub ctlr: u32,       // 0x0000: CPU Interface Control Register
    pub pmr: u32,        // 0x0004: Interrupt Priority Mask Register
    pub bpr: u32,        // 0x0008: Binary Point Register
    pub iar: u32,        // 0x000C: Interrupt Acknowledge Register
    pub eoir: u32,       // 0x0010: End of Interrupt Register
    pub rpr: u32,        // 0x0014: Running Priority Register
    pub hppir: u32,      // 0x0018: Highest Priority Pending Interrupt Register
}

/// Controlador de Interrupciones ARM GICv2
pub struct Gicv2Controller {
    pub gicd_base: usize,
    pub gicc_base: usize,
}

impl Gicv2Controller {
    pub const QEMU_VIRT_GICD: usize = 0x0800_0000;
    pub const QEMU_VIRT_GICC: usize = 0x0801_0000;

    pub const fn new(gicd_base: usize, gicc_base: usize) -> Self {
        Self { gicd_base, gicc_base }
    }

    /// Inicializa el distribuidor GICD y la interfaz de CPU GICC
    pub fn init(&self) {
        unsafe {
            let gicd = self.gicd_base as *mut GicDistributor;
            let gicc = self.gicc_base as *mut GicCpuInterface;

            // Deshabilitar GICD antes de configurar
            core::ptr::write_volatile(&mut (*gicd).ctlr, 0);

            // Deshabilitar todas las SPIs (32..1020)
            for i in 1..32 {
                core::ptr::write_volatile(&mut (*gicd).icenabler[i], 0xFFFF_FFFF);
                core::ptr::write_volatile(&mut (*gicd).icpendr[i], 0xFFFF_FFFF);
            }

            // Habilitar Grupo 0 y Grupo 1 en Distribuidor
            core::ptr::write_volatile(&mut (*gicd).ctlr, 0x03);

            // Configurar máscara de prioridad de CPU (PMR): permitir todas las prioridades (0xFF)
            core::ptr::write_volatile(&mut (*gicc).pmr, 0xFF);
            // Habilitar señalización de interrupciones en la interfaz de CPU
            core::ptr::write_volatile(&mut (*gicc).ctlr, 0x01);
        }
    }

    /// Habilita una interrupción específica en el Distribuidor
    pub fn enable_interrupt(&self, irq: u32) {
        let reg_idx = (irq / 32) as usize;
        let bit = 1 << (irq % 32);
        unsafe {
            let gicd = self.gicd_base as *mut GicDistributor;
            core::ptr::write_volatile(&mut (*gicd).isenabler[reg_idx], bit);
        }
    }

    /// Acepta la interrupción pendiente más alta (IAR) devolviendo el número de IRQ
    #[inline]
    pub fn acknowledge_interrupt(&self) -> u32 {
        unsafe {
            let gicc = self.gicc_base as *mut GicCpuInterface;
            core::ptr::read_volatile(&(*gicc).iar) & 0x3FF
        }
    }

    /// Señala el fin de la interrupción (EOIR)
    #[inline]
    pub fn end_of_interrupt(&self, irq: u32) {
        unsafe {
            let gicc = self.gicc_base as *mut GicCpuInterface;
            core::ptr::write_volatile(&mut (*gicc).eoir, irq);
        }
    }
}
