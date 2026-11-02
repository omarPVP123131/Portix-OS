// kernel/src/arch/aarch64/vectors.rs — Vectores de Excepción y Despacho de Interrupciones en AArch64
// Alineación canónica a 2048 bytes para VBAR_EL1 y decodificación de ESR_EL1 para llamadas al sistema SVC
#![allow(dead_code)]

use super::Arm64Registers;

/// Registro de Síndrome de Excepción (ESR_EL1) para decodificar SVC, fallos de página y trampas
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct EsrEl1(pub u64);

impl EsrEl1 {
    pub const EC_UNKNOWN: u32 = 0x00;
    pub const EC_WFI_WFE: u32 = 0x01;
    pub const EC_CP15_32: u32 = 0x03;
    pub const EC_CP15_64: u32 = 0x04;
    pub const EC_CP14_MRRC: u32 = 0x06;
    pub const EC_ILLEGAL_EXEC: u32 = 0x0E;
    pub const EC_SVC_AARCH32: u32 = 0x11;
    pub const EC_SVC_AARCH64: u32 = 0x15; // Syscall desde AArch64 en EL0
    pub const EC_MSR_MRS: u32 = 0x18;
    pub const EC_INSTR_ABORT_LOWER: u32 = 0x20;
    pub const EC_INSTR_ABORT_SAME: u32 = 0x21;
    pub const EC_DATA_ABORT_LOWER: u32 = 0x24;
    pub const EC_DATA_ABORT_SAME: u32 = 0x25;
    pub const EC_SP_ALIGN: u32 = 0x26;
    pub const EC_BRK_AARCH64: u32 = 0x3C;

    #[inline]
    pub fn exception_class(&self) -> u32 {
        ((self.0 >> 26) & 0x3F) as u32
    }

    #[inline]
    pub fn is_syscall(&self) -> bool {
        self.exception_class() == Self::EC_SVC_AARCH64
    }

    #[inline]
    pub fn is_data_abort(&self) -> bool {
        let ec = self.exception_class();
        ec == Self::EC_DATA_ABORT_LOWER || ec == Self::EC_DATA_ABORT_SAME
    }

    #[inline]
    pub fn imm16(&self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }
}

/// Tabla canónica de vectores de excepción AArch64 alineada a 2048 bytes (VBAR_EL1)
/// Contiene 16 entradas distribuidas en 4 grupos:
/// 1. Mismo EL con SP0
/// 2. Mismo EL con SPx
/// 3. Menor EL ejecutando en AArch64
/// 4. Menor EL ejecutando en AArch32
#[repr(C, align(2048))]
pub struct Arm64VectorTable {
    pub entries: [u32; 128], // 16 vectores x 32 bytes (8 instrucciones por vector)
}

impl Arm64VectorTable {
    pub const fn new() -> Self {
        Self {
            entries: [0xD503201Fu32; 128], // Relleno con instrucciones NOP (0xD503201F)
        }
    }
}

/// Manejador de trampas de excepción en AArch64
pub fn arm64_trap_dispatch(regs: &mut Arm64Registers, esr: EsrEl1) {
    if esr.is_syscall() {
        // En AArch64 ABI, X8 almacena el número de syscall y X0-X5 los parámetros
        let syscall_nr = regs.x[8];
        match syscall_nr {
            93 => { // __NR_exit
                let _exit_code = regs.x[0];
                regs.x[0] = 0; // Código de éxito
            }
            64 => { // __NR_write (fd, buf, count)
                // Retornar número de bytes procesados
                regs.x[0] = regs.x[2];
            }
            _ => {
                // Syscall no implementada o delegada al subsistema común
                regs.x[0] = 0;
            }
        }
    }
}
