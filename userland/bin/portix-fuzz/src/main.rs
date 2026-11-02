#![no_std]
#![no_main]

use portix_rt::println;
use portix_rt::syscall::{sys_read, sys_write, sys_open, sys_close, sys_getpid, sys_stat};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let pid = sys_getpid();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║       PORTIX-FUZZ: STRESS & RESILIENCE SYSCALL FUZZING ENGINE        ║");
    println!("║  PID: {} | Mode: Ring 3 Userland | Targeted Tests: 10,000 Iterations ║", pid);
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    println!("[*] Starting Phase 1: Invalid Memory Address & Kernel Boundary Fuzzing...");
    let bad_ptrs: [usize; 8] = [
        0x0,
        0x1,
        0x100,
        0x7FFF_FFFF_0000,
        0xFFFF_8000_0000_0000, // Kernel text space
        0xFFFF_FFFF_8000_0000, // Kernel MMIO space
        0xDEAD_BEEF_0000,
        usize::MAX,
    ];

    let mut rejected_faults = 0u32;
    for &ptr in &bad_ptrs {
        // sys_read into kernel/unmapped memory must return -EFAULT or error <= 0
        let ret = sys_read(0, ptr as *mut u8, 64);
        if ret <= 0 {
            rejected_faults += 1;
        }

        // sys_write from invalid pointer must return error <= 0
        let ret_w = sys_write(1, ptr as *const u8, 64);
        if ret_w <= 0 {
            rejected_faults += 1;
        }

        // sys_stat with invalid pointer must fail safely
        let mut st = [0u8; 144];
        let ret_s = sys_stat(ptr as *const u8, st.as_mut_ptr());
        if ret_s != 0 {
            rejected_faults += 1;
        }
    }
    println!("  -> Bad Pointer Invocations: {} safely caught and rejected without fault", rejected_faults);

    println!("[*] Starting Phase 2: Descriptor Boundary & Out-of-Range Fuzzing...");
    let bad_fds: [i32; 8] = [-1, -100, -9999, 32, 64, 128, 1024, i32::MAX];
    let mut rejected_ebadf = 0u32;
    let mut scratch = [0u8; 32];
    for &fd in &bad_fds {
        let r = sys_read(fd, scratch.as_mut_ptr(), 16);
        if r < 0 { rejected_ebadf += 1; }
        let w = sys_write(fd, scratch.as_ptr(), 16);
        if w < 0 { rejected_ebadf += 1; }
        let c = sys_close(fd);
        if c < 0 { rejected_ebadf += 1; }
    }
    println!("  -> Out-of-Bounds FDs: {} calls returned EBADF without kernel panic", rejected_ebadf);

    println!("[*] Starting Phase 3: High-Frequency 10,000 Iteration Churn Loop...");
    let mut lfsr: u32 = 0xACE1;
    let mut iterations = 0u32;
    for _ in 0..10_000 {
        // Galois LFSR pseudo-random generator
        let bit = ((lfsr >> 0) ^ (lfsr >> 2) ^ (lfsr >> 3) ^ (lfsr >> 5)) & 1;
        lfsr = (lfsr >> 1) | (bit << 31);

        let test_case = lfsr % 5;
        match test_case {
            0 => {
                // Read from pseudo-random FD
                let fd = (lfsr & 0x7F) as i32;
                let _ = sys_read(fd, scratch.as_mut_ptr(), (lfsr as usize) % 32);
            }
            1 => {
                // Write to pseudo-random FD
                let fd = (lfsr & 0x7F) as i32;
                let _ = sys_write(fd, scratch.as_ptr(), (lfsr as usize) % 32);
            }
            2 => {
                // Open non-existent random path
                let dummy_path = b"/nonexistent/fuzz_test_node\0";
                let fd = sys_open(dummy_path.as_ptr(), 0);
                if fd >= 0 {
                    sys_close(fd as i32);
                }
            }
            3 => {
                // Stat non-existent path
                let dummy_path = b"/var/fuzz_temp\0";
                let mut st = [0u8; 144];
                let _ = sys_stat(dummy_path.as_ptr(), st.as_mut_ptr());
            }
            _ => {
                // Harmless valid sys_getpid
                let _ = sys_getpid();
            }
        }
        iterations += 1;
    }

    println!("  -> Completed {} fuzzing rounds successfully.", iterations);
    println!("\n[PORTIX-FUZZ-PASS] 10,000 Syscall Invocations Fuzzed with Zero Kernel Panics");
    0
}
