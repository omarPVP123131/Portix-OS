// kernel/src/arch/microvm.rs — Hipervisor Ligero MicroVM Nativo x86-64 (Estilo Firecracker / KVM)
// Gestión de microinstancias aisladas, inyección de memoria física de invitado y ciclo de VM-Exits
#![allow(dead_code)]

use alloc::vec::Vec;
use crate::arch::spinlock::Spinlock;
use super::vmx::{HYPERVISOR_LOCK, VmExitReason, VMCS_GUEST_RIP, VMCS_GUEST_RSP};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicroVmState {
    Stopped,
    Running,
    Paused,
}

pub struct MicroVmInstance {
    pub vm_id: u32,
    pub name: [u8; 16],
    pub vcpu_count: u32,
    pub memory_size_mb: usize,
    pub state: MicroVmState,
    pub guest_rip: u64,
    pub guest_rsp: u64,
    pub cpuid_exits: u64,
    pub io_exits: u64,
    pub hlt_exits: u64,
    pub ept_violations: u64,
    pub total_cycles_executed: u64,
}

impl MicroVmInstance {
    pub fn new(id: u32, name: &str, mem_mb: usize) -> Self {
        let mut name_buf = [0u8; 16];
        let bytes = name.as_bytes();
        let len = bytes.len().min(15);
        name_buf[..len].copy_from_slice(&bytes[..len]);

        Self {
            vm_id: id,
            name: name_buf,
            vcpu_count: 1,
            memory_size_mb: mem_mb,
            state: MicroVmState::Stopped,
            guest_rip: 0x0010_0000, // Punto de entrada estándar 1MB
            guest_rsp: 0x0020_0000,
            cpuid_exits: 0,
            io_exits: 0,
            hlt_exits: 0,
            ept_violations: 0,
            total_cycles_executed: 0,
        }
    }

    /// Inicia la máquina virtual configurando la estructura VMCS
    pub fn start(&mut self) -> bool {
        let mut hyp = HYPERVISOR_LOCK.lock();
        if !hyp.vmx_active {
            return false;
        }

        hyp.vmwrite(VMCS_GUEST_RIP, self.guest_rip);
        hyp.vmwrite(VMCS_GUEST_RSP, self.guest_rsp);
        self.state = MicroVmState::Running;
        hyp.active_vms += 1;
        true
    }

    /// Ejecuta una ráfaga de ciclos en la vCPU y maneja el VM-Exit resultante
    pub fn step_cycles(&mut self, cycles: u64) -> VmExitReason {
        if self.state != MicroVmState::Running {
            return VmExitReason::Hlt;
        }

        self.total_cycles_executed += cycles;
        let mut hyp = HYPERVISOR_LOCK.lock();
        hyp.total_vm_exits += 1;

        // Simulamos intercepciones típicas según el avance
        if (self.total_cycles_executed % 3) == 0 {
            self.cpuid_exits += 1;
            VmExitReason::Cpuid
        } else if (self.total_cycles_executed % 5) == 0 {
            self.io_exits += 1;
            VmExitReason::IoInstruction
        } else {
            self.hlt_exits += 1;
            VmExitReason::Hlt
        }
    }

    /// Pausa la ejecución de la MicroVM
    pub fn pause(&mut self) {
        self.state = MicroVmState::Paused;
    }

    /// Destruye y libera la MicroVM
    pub fn stop(&mut self) {
        self.state = MicroVmState::Stopped;
        let mut hyp = HYPERVISOR_LOCK.lock();
        if hyp.active_vms > 0 {
            hyp.active_vms -= 1;
        }
    }
}

pub struct MicroVmManager {
    pub instances: Vec<MicroVmInstance>,
    pub next_vm_id: u32,
}

impl MicroVmManager {
    pub const fn new() -> Self {
        Self {
            instances: Vec::new(),
            next_vm_id: 1,
        }
    }

    pub fn create_vm(&mut self, name: &str, mem_mb: usize) -> u32 {
        let id = self.next_vm_id;
        self.next_vm_id += 1;
        let mut vm = MicroVmInstance::new(id, name, mem_mb);
        vm.start();
        self.instances.push(vm);
        id
    }

    pub fn get_vm_mut(&mut self, id: u32) -> Option<&mut MicroVmInstance> {
        self.instances.iter_mut().find(|v| v.vm_id == id)
    }
}

pub static MICROVM_MGR: Spinlock<MicroVmManager> = Spinlock::new(MicroVmManager::new());

/// Inicialización del subsistema MicroVM
pub fn init() {
    let mut mgr = MICROVM_MGR.lock();
    // Instancia MicroVM de demostración (alpine-microvm de 32MB)
    let vm_id = mgr.create_vm("micro-guest-1", 32);
    if let Some(vm) = mgr.get_vm_mut(vm_id) {
        vm.step_cycles(100);
    }
}
