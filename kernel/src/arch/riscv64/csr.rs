// kernel/src/arch/riscv64/csr.rs — Registros de Control y Estado (CSR) de RISC-V 64
// Épica 50 (Fases 3086 - 3170) — Portix OS v6.0 'Chronos'

#![allow(dead_code)]

/// Registros de Estado de Supervisor (S-Mode)
pub const SSTATUS_UIE: usize = 1 << 0;
pub const SSTATUS_SIE: usize = 1 << 1;
pub const SSTATUS_UPIE: usize = 1 << 4;
pub const SSTATUS_SPIE: usize = 1 << 5;
pub const SSTATUS_SPP: usize = 1 << 8;
pub const SSTATUS_FS: usize = 3 << 13;
pub const SSTATUS_VS: usize = 3 << 9; // Extensión Vectorial (RVV 1.0)
pub const SSTATUS_SUM: usize = 1 << 18; // Permitir acceso a memoria de usuario en supervisor

/// Registros de Traducción de Direcciones (SATP)
pub const SATP_MODE_BARE: usize = 0;
pub const SATP_MODE_SV39: usize = 8 << 60;
pub const SATP_MODE_SV48: usize = 9 << 60; // 4 niveles (48-bit VA)
pub const SATP_MODE_SV57: usize = 10 << 60; // 5 niveles (57-bit VA)

#[derive(Clone, Copy, Debug, Default)]
pub struct Riscv64CsrState {
    pub sstatus: usize,
    pub sie: usize,
    pub sip: usize,
    pub satp: usize,
    pub stvec: usize,
    pub sepc: usize,
    pub scause: usize,
    pub stval: usize,
}

impl Riscv64CsrState {
    pub fn new_supervisor_mode() -> Self {
        Riscv64CsrState {
            sstatus: SSTATUS_SPIE | SSTATUS_SPP | SSTATUS_FS | SSTATUS_VS | SSTATUS_SUM,
            sie: 0x222, // SSIP, STIP, SEIP
            sip: 0,
            satp: SATP_MODE_SV48,
            stvec: 0x80200000, // Vector de excepciones S-Mode
            sepc: 0x80200000,
            scause: 0,
            stval: 0,
        }
    }
}
