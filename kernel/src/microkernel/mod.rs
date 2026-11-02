// kernel/src/core/mod.rs — Microkernel Core Abstraction Layer (Architecture, Memory, IPC, Scheduling, Task)
// Implements strict separation of powers for a resilient microkernel architecture:
// - Hardware Architecture & CPU HAL: GDT, IDT, ISR, Ring-3 Contexts, SMP
// - Paging & Physical Memory Management: Buddy Allocator, HugePages, Per-CPU Slabs
// - Microkernel Capability-based Fast-IPC: Rendezvous messaging, CSpaces, Zero-Copy Shared Memory
// - Preemptive Scheduler: CFS (Completely Fair Scheduler), Real-Time Prioritization, Quantum
// - Task & Execution Management: Ring-3 Processes, Security Badges, Capabilities

#![allow(dead_code)]

pub mod arch {
    pub use crate::arch::*;
}

pub mod mem {
    pub use crate::mem::*;
}

pub mod ipc {
    pub use crate::ipc::*;
    pub use crate::arch::microkernel::*;
}

pub mod sched {
    pub use crate::arch::cfs::*;
}

pub mod task {
    pub use crate::process::*;
}

pub mod ebpf;

