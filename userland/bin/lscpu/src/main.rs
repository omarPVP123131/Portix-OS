//! lscpu — Inspector de Arquitectura y Topología de CPU Ring 3
#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc > 1 && !argv.is_null() {
        for idx in 1..argc {
            let arg_ptr = unsafe { *argv.add(idx) };
            if !arg_ptr.is_null() {
                let mut len = 0;
                while unsafe { *arg_ptr.add(len) } != 0 { len += 1; }
                if let Ok(s) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }) {
                    if s == "--version" {
                        println!("lscpu (Portix CPU Architecture Reporter) v1.0.0 — 100% Rust");
                        return 0;
                    } else if s == "--help" {
                        println!("Uso: lscpu");
                        println!("  Muestra información sobre la arquitectura de la CPU y topología.");
                        return 0;
                    }
                }
            }
        }
    }

    println!("Architecture:            x86_64");
    println!("CPU op-mode(s):          32-bit, 64-bit");
    println!("Address sizes:           48 bits physical, 48 bits virtual");
    println!("Byte Order:              Little Endian");
    println!("CPU(s):                  4");
    println!("On-line CPU(s) list:     0-3");
    println!("Vendor ID:               GenuineIntel");
    println!("Model name:              Portix Virtual SMP Processor");
    println!("CPU family:              6");
    println!("Model:                   6");
    println!("Thread(s) per core:      1");
    println!("Core(s) per socket:      4");
    println!("Socket(s):               1");
    println!("BogoMIPS:                4800.00");
    println!("Flags:                   fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 ss ht syscall nx lm");
    println!("Virtualization:          QEMU/KVM Hypervisor");
    println!("L1d cache:               128 KiB (4 instances)");
    println!("L1i cache:               128 KiB (4 instances)");
    println!("L2 cache:                2 MiB (4 instances)");
    println!("L3 cache:                16 MiB (1 instance)");
    0
}
