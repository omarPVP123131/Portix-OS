// kernel/src/arch/aarch64/mmu.rs — Tablas de Traducción de Páginas MMU (4 Niveles) para ARM64 / AArch64
// Gestión de Registros de Control de Sistema: TTBR0_EL1, TTBR1_EL1, TCR_EL1 y MAIR_EL1
#![allow(dead_code)]

/// Descriptor de Paginación AArch64 (4 niveles, gránulo de 4KB, espacio virtual de 48 bits)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Arm64Pte(pub u64);

impl Arm64Pte {
    pub const VALID: u64 = 1 << 0;
    pub const TABLE_OR_PAGE: u64 = 1 << 1;
    pub const ATTR_DEVICE: u64 = 0 << 2;  // Index 0 en MAIR (Device-nGnRnE)
    pub const ATTR_NORMAL: u64 = 1 << 2;  // Index 1 en MAIR (Normal Cacheable Write-Back)
    pub const AP_RW_EL1: u64 = 0 << 6;    // Privileged Read/Write only
    pub const AP_RW_ALL: u64 = 1 << 6;    // User + Privileged Read/Write
    pub const AP_RO_ALL: u64 = 3 << 6;    // User + Privileged Read-Only
    pub const SH_INNER: u64 = 3 << 8;     // Inner Shareable
    pub const AF_ACCESSED: u64 = 1 << 10; // Access Flag
    pub const NG_NON_GLOBAL: u64 = 1 << 11; // Non-Global (para ASID en Ring-3)
    pub const PXN: u64 = 1 << 53;         // Privileged Execute Never
    pub const UXN: u64 = 1 << 54;         // Unprivileged Execute Never

    pub const fn new(paddr: u64, flags: u64) -> Self {
        Self((paddr & 0x0000_FFFF_FFFF_F000) | flags | Self::VALID | Self::TABLE_OR_PAGE | Self::AF_ACCESSED)
    }

    pub fn is_valid(&self) -> bool {
        (self.0 & Self::VALID) != 0
    }

    pub fn physical_address(&self) -> u64 {
        self.0 & 0x0000_FFFF_FFFF_F000
    }

    pub fn is_table(&self) -> bool {
        (self.0 & Self::TABLE_OR_PAGE) != 0
    }
}

/// Tabla de traducción de 512 entradas alineada a 4096 bytes (L0, L1, L2 o L3)
#[repr(C, align(4096))]
pub struct Arm64TranslationTable {
    pub entries: [Arm64Pte; 512],
}

impl Arm64TranslationTable {
    pub const fn new() -> Self {
        Self {
            entries: [Arm64Pte(0); 512],
        }
    }

    pub fn clear(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.0 = 0;
        }
    }
}

/// Memory Attribute Indirection Register (MAIR_EL1)
/// Define las características de memoria cacheable y dispositivos MMIO
pub struct MairEl1;

impl MairEl1 {
    pub const ATTR_DEVICE_NGNRNE: u64 = 0x00; // Index 0: Device non-Gathering, non-Reordering, non-Early-Write
    pub const ATTR_NORMAL_WB: u64 = 0xFF;      // Index 1: Normal Memory, Inner & Outer Write-Back Cacheable

    pub const VALUE: u64 = (Self::ATTR_DEVICE_NGNRNE << 0) | (Self::ATTR_NORMAL_WB << 8);
}

/// Translation Control Register (TCR_EL1)
/// Configura el tamaño del espacio de direcciones virtuales (48 bits = 256 TB) y gránulo de 4KB
pub struct TcrEl1;

impl TcrEl1 {
    pub const T0SZ_48BIT: u64 = 16 << 0;  // Espacio virtual de 48 bits para TTBR0 (Userland)
    pub const T1SZ_48BIT: u64 = 16 << 16; // Espacio virtual de 48 bits para TTBR1 (Kernel)
    pub const TG0_4KB: u64 = 0 << 14;     // Granularidad de 4KB para TTBR0
    pub const TG1_4KB: u64 = 2 << 30;     // Granularidad de 4KB para TTBR1
    pub const IPS_48BIT: u64 = 5 << 32;   // Espacio físico de 48 bits (256 TB)
    pub const SH0_INNER: u64 = 3 << 12;   // Shareability Inner para TTBR0
    pub const SH1_INNER: u64 = 3 << 28;   // Shareability Inner para TTBR1
    pub const ORGN0_WB: u64 = 1 << 10;    // Outer Write-Back Cacheable
    pub const IRGN0_WB: u64 = 1 << 8;     // Inner Write-Back Cacheable

    pub const VALUE: u64 = Self::T0SZ_48BIT | Self::T1SZ_48BIT | Self::TG0_4KB | Self::TG1_4KB | Self::IPS_48BIT
        | Self::SH0_INNER | Self::SH1_INNER | Self::ORGN0_WB | Self::IRGN0_WB;
}
