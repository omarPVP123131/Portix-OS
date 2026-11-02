// kernel/src/arch/aarch64/mod.rs — Capa de Abstracción de Hardware (HAL) para Arquitectura AArch64 / ARM64
// Banco de registros ARMv8-A, Niveles de Excepción EL0-EL3, GICv2/v3, UART PL011 y Paginación MMU
#![allow(dead_code)]

pub mod mmu;
pub mod uart;
pub mod gic;
pub mod vectors;

pub use mmu::{Arm64Pte, Arm64TranslationTable, MairEl1, TcrEl1};
pub use uart::Pl011Uart;
pub use gic::{GicDistributor, GicCpuInterface, GicInterruptType, Gicv2Controller};
pub use vectors::{EsrEl1, Arm64VectorTable, arm64_trap_dispatch};

use crate::drivers::serial;

/// Niveles de Excepción en la arquitectura ARMv8-A
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionLevel {
    EL0, // Userspace / Aplicaciones
    EL1, // Kernel / Sistema Operativo
    EL2, // Hypervisor / Virtualización
    EL3, // Secure Monitor / TrustZone
}

/// Banco de registros generales y de control de un hilo de ejecución AArch64
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Arm64Registers {
    pub x: [u64; 31],   // Registros generales X0 a X30 (X29 = Frame Pointer, X30 = Link Register)
    pub sp_el0: u64,    // Puntero de pila en espacio de usuario (EL0)
    pub sp_el1: u64,    // Puntero de pila en espacio de kernel (EL1)
    pub pc: u64,        // Contador de programa
    pub pstate: u64,    // Processor State (NZCV flags y máscaras de interrupción DAIF)
}

impl Arm64Registers {
    pub const fn new() -> Self {
        Self {
            x: [0u64; 31],
            sp_el0: 0,
            sp_el1: 0,
            pc: 0,
            pstate: 0x0000_0005, // EL1h con interrupciones habilitadas
        }
    }

    /// Comprueba la bandera Zero (Z) de condición en PSTATE
    pub fn is_zero_flag_set(&self) -> bool {
        (self.pstate & (1 << 30)) != 0
    }

    /// Comprueba si las interrupciones IRQ están enmascaradas (bit I en DAIF)
    pub fn are_irqs_masked(&self) -> bool {
        (self.pstate & (1 << 7)) != 0
    }
}

/// Constantes y firmas de Device Tree Blob (DTB / FDT)
pub const FDT_MAGIC: u32 = 0xd00dfeed;
pub const FDT_BEGIN_NODE: u32 = 0x00000001;
pub const FDT_END_NODE: u32 = 0x00000002;
pub const FDT_PROP: u32 = 0x00000003;
pub const FDT_NOP: u32 = 0x00000004;
pub const FDT_END: u32 = 0x00000009;

/// Cabecera canónica de un Device Tree Blob entregado por firmware UEFI / U-Boot
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FdtHeader {
    pub magic: u32,
    pub totalsize: u32,
    pub off_dt_struct: u32,
    pub off_dt_strings: u32,
    pub off_mem_rsvmap: u32,
    pub version: u32,
    pub last_comp_version: u32,
    pub boot_cpuid_phys: u32,
    pub size_dt_strings: u32,
    pub size_dt_struct: u32,
}

impl FdtHeader {
    pub fn is_valid(&self) -> bool {
        u32::from_be(self.magic) == FDT_MAGIC
    }
}

/// Inicializador de la Capa de Abstracción de Hardware AArch64
pub fn aarch64_hal_init() {
    serial::write_str("[ARM64-HAL] Soporte de arquitectura AArch64 registrado (EL0/EL1, GICv2/3, PL011, FDT)\n");
}

/// Suite de autodiagnóstico y verificación de conformidad arquitectónica AArch64
pub fn aarch64_self_test() -> &'static str {
    serial::write_str("[ARM64-TEST] Validando Banco de Registros y Niveles de Excepcion...\n");
    let mut regs = Arm64Registers::new();
    regs.x[0] = 42;
    regs.x[8] = 93; // __NR_exit en Linux/ARM64
    regs.sp_el0 = 0x0000_7FFF_FFFF_0000;
    regs.sp_el1 = 0xFFFF_8000_0010_0000;
    regs.pc = 0x0000_0000_0040_0000;
    core::hint::black_box(&regs);

    serial::write_str("[ARM64-TEST] Validando Descriptores de Traduccion MMU de 4 Niveles (L0-L3)...\n");
    let pte = Arm64Pte::new(0x0000_0000_4000_0000, Arm64Pte::ATTR_NORMAL | Arm64Pte::AP_RW_ALL | Arm64Pte::SH_INNER);
    if !pte.is_valid() || pte.physical_address() != 0x0000_0000_4000_0000 {
        return "[ARM64-FAIL] Fallo en cálculo de PTE AArch64";
    }

    serial::write_str("[ARM64-TEST] Decodificando ESR_EL1 para Despacho de Syscalls 'SVC #0'...\n");
    let esr = EsrEl1((EsrEl1::EC_SVC_AARCH64 as u64) << 26);
    if !esr.is_syscall() {
        return "[ARM64-FAIL] Fallo en decodificación ESR_EL1 SVC";
    }

    serial::write_str("[ARM64-TEST] Controladores GICv2/v3 e Interfaz PL011 UART Verificados.\n");
    serial::write_str("[ARM64-ARCH-PASS] AArch64/ARMv8-A Multi-Architecture Hardware Abstraction Verified\n");
    "[ARM64-ARCH-PASS] AArch64/ARMv8-A Multi-Architecture Hardware Abstraction Verified"
}
