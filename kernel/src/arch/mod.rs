// kernel/src/arch/mod.rs — CPU Architecture & Low-Level Hardware Abstraction Layer (HAL)
// Contains strictly CPU-specific architecture primitives:
// - Hardware Interrupt Descriptor Table (IDT) and Interrupt Service Routines (ISR)
// - Hardware ports, registers (CR0-CR4, MSRs), CPUID, and Long Mode setup
// - Symmetric Multiprocessing (SMP) and Per-CPU Local APIC
// - Hardware spinlocks and synchronizations
// - Ring-3 transitions, context switching, and CPU halt/power primitives
// - Hardware Virtualization (VMX, SVM, MicroVM)

pub mod halt;
pub mod debug_regs;
pub mod idt;
pub mod percpu;
pub mod hardware;
pub mod isr_handlers;
pub mod ring3;
pub mod spinlock;
pub mod bkl;
pub mod smp;
pub mod aarch64;
pub mod vmx;
pub mod debugger;
pub mod microvm;
pub mod cfs;
pub mod microkernel;
pub mod io_uring;
pub mod power;
pub mod container;
pub mod acpi_sleep;
pub mod vmx_hypervisor;
pub mod backlight;
pub mod post_quantum;
pub mod sovereign_boot;
pub mod tpm_secure;
pub mod deployment_profile;
pub mod laptop_power;
pub mod riscv64;

// Re-export spinlock for convenient access
pub use spinlock::Spinlock;
