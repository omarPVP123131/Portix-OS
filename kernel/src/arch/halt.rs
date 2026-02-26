// kernel/src/halt.rs
use core::arch::asm;

pub fn halt_loop() -> ! {
    loop {
        unsafe { asm!("cli; hlt", options(nostack, nomem)); }
    }
}   