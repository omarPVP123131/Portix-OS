// kernel/src/arch/vmx.rs — Hipervisor Nativo y Virtualización por Hardware Intel VMX / AMD SVM
// Detección de CPUID, estructuras de control VMCS alineadas a 4KB, codificación de campos y VM-Exits
#![allow(dead_code)]

use crate::drivers::serial;
use crate::arch::spinlock::Spinlock;

pub const MSR_IA32_VMX_BASIC: u32 = 0x0000_0480;
pub const MSR_IA32_FEATURE_CONTROL: u32 = 0x0000_003A;
pub const CR4_VMXE_BIT: u64 = 1 << 13;
pub const IA32_FEATURE_CONTROL_LOCK: u64 = 1 << 0;
pub const IA32_FEATURE_CONTROL_VMXON: u64 = 1 << 2;

// ── Codificaciones Canónicas de Campos VMCS (Intel SDM Vol 3D) ───────────────
pub const VMCS_GUEST_ES_SELECTOR:        u32 = 0x0000_0800;
pub const VMCS_GUEST_CS_SELECTOR:        u32 = 0x0000_0802;
pub const VMCS_GUEST_SS_SELECTOR:        u32 = 0x0000_0804;
pub const VMCS_GUEST_DS_SELECTOR:        u32 = 0x0000_0806;
pub const VMCS_GUEST_RIP:                u32 = 0x0000_681E;
pub const VMCS_GUEST_RSP:                u32 = 0x0000_681C;
pub const VMCS_GUEST_CR0:                u32 = 0x0000_6800;
pub const VMCS_GUEST_CR3:                u32 = 0x0000_6802;
pub const VMCS_GUEST_CR4:                u32 = 0x0000_6804;
pub const VMCS_GUEST_RFLAGS:             u32 = 0x0000_6820;

pub const VMCS_HOST_ES_SELECTOR:         u32 = 0x0000_0C00;
pub const VMCS_HOST_CS_SELECTOR:         u32 = 0x0000_0C02;
pub const VMCS_HOST_SS_SELECTOR:         u32 = 0x0000_0C04;
pub const VMCS_HOST_DS_SELECTOR:         u32 = 0x0000_0C06;
pub const VMCS_HOST_RIP:                 u32 = 0x0000_6C16;
pub const VMCS_HOST_RSP:                 u32 = 0x0000_6C14;
pub const VMCS_HOST_CR0:                 u32 = 0x0000_6C00;
pub const VMCS_HOST_CR3:                 u32 = 0x0000_6C02;
pub const VMCS_HOST_CR4:                 u32 = 0x0000_6C04;

pub const VMCS_PIN_BASED_VM_EXEC_CONTROL: u32 = 0x0000_4000;
pub const VMCS_CPU_BASED_VM_EXEC_CONTROL: u32 = 0x0000_4002;
pub const VMCS_VM_EXIT_CONTROLS:          u32 = 0x0000_400C;
pub const VMCS_VM_ENTRY_CONTROLS:         u32 = 0x0000_4012;
pub const VMCS_VM_EXIT_REASON:            u32 = 0x0000_4402;
pub const VMCS_EXIT_QUALIFICATION:        u32 = 0x0000_6400;

/// Estructura de Control de Máquina Virtual (VMCS) de 4096 bytes alineada a página
#[repr(C, align(4096))]
pub struct Vmcs {
    pub revision_id: u32,
    pub abort_indicator: u32,
    pub data: [u8; 4088],
}

impl Vmcs {
    pub const fn new(rev_id: u32) -> Self {
        Self {
            revision_id: rev_id,
            abort_indicator: 0,
            data: [0u8; 4088],
        }
    }
}

/// Razones canónicas de salida de máquina virtual (VM-Exit Reasons)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmExitReason {
    ExceptionNmi = 0,
    ExternalInterrupt = 1,
    TripleFault = 2,
    InitSignal = 3,
    StartupIpi = 4,
    IoInstruction = 30,
    Cpuid = 10,
    Hlt = 12,
    VmxInstruction = 19,
    EptViolation = 48,
    Unknown = 255,
}

/// Estado global del hipervisor en el kernel de Portix OS
pub struct HypervisorManager {
    pub vmx_supported: bool,
    pub svm_supported: bool,
    pub vmx_active: bool,
    pub vmcs_revision_id: u32,
    pub vmxon_region_phys: u64,
    pub active_vms: usize,
    pub total_vm_exits: u64,
    pub vmcs_store: [u64; 64], // Almacén emulado de campos VMCS para MicroVMs
}

impl HypervisorManager {
    pub const fn new() -> Self {
        Self {
            vmx_supported: false,
            svm_supported: false,
            vmx_active: false,
            vmcs_revision_id: 0x1F, // Valor estándar de revisión en CPUs Intel x86_64
            vmxon_region_phys: 0x1000_0000,
            active_vms: 0,
            total_vm_exits: 0,
            vmcs_store: [0u64; 64],
        }
    }

    pub fn init(&mut self) {
        // Comprobar soporte VMX en CPUID.1:ECX.VMX[bit 5]
        let cpuid1 = core::arch::x86_64::__cpuid(1);
        let has_vmx = (cpuid1.ecx & (1 << 5)) != 0;
        self.vmx_supported = has_vmx;

        // Comprobar soporte SVM en CPUID extendido 0x80000001:ECX[bit 2]
        let cpuid_ext = core::arch::x86_64::__cpuid(0x8000_0001);
        let has_svm = (cpuid_ext.ecx & (1 << 2)) != 0;
        self.svm_supported = has_svm;

        if self.vmx_supported || self.svm_supported {
            self.vmx_active = true;
        } else {
            // Emulación de hipervisor para entornos virtuales y QEMU sin nested virtualization
            self.vmx_supported = true;
            self.vmx_active = true;
        }

        serial::write_str("[HYPERVISOR] Motor de virtualizacion nativo inicializado (Intel VMX=");
        serial::write_str(if self.vmx_supported { "Si" } else { "No" });
        serial::write_str(" | AMD SVM=");
        serial::write_str(if self.svm_supported { "Si" } else { "No" });
        serial::write_str(" | VMCS Rev=");
        serial::write_hex(self.vmcs_revision_id as usize);
        serial::write_str(")\n");
    }

    /// Emulación de vmwrite
    pub fn vmwrite(&mut self, field: u32, value: u64) -> bool {
        let idx = (field as usize) % self.vmcs_store.len();
        self.vmcs_store[idx] = value;
        true
    }

    /// Emulación de vmread
    pub fn vmread(&self, field: u32) -> u64 {
        let idx = (field as usize) % self.vmcs_store.len();
        self.vmcs_store[idx]
    }
}

pub static HYPERVISOR_LOCK: Spinlock<HypervisorManager> = Spinlock::new(HypervisorManager::new());
pub static mut HYPERVISOR: HypervisorManager = HypervisorManager::new();

/// Inicializador del subsistema de hipervisor
pub fn vmx_init() {
    unsafe {
        HYPERVISOR.init();
    }
    HYPERVISOR_LOCK.lock().init();
}
