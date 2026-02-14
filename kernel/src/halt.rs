// halt.rs
use core::arch::asm;

pub fn halt_loop() -> ! {
    loop {
        unsafe {
            asm!("cli");
            asm!("hlt");
        }
    }
}
