// kernel/src/pit.rs — PORTIX PIT Timer (IRQ0, 100 Hz)
#![allow(dead_code)]

const PIT_CHANNEL0: u16 = 0x40;
const PIT_CMD:      u16 = 0x43;

// 1_193_182 Hz / 100 = 11931 → ~100 Hz
pub const PIT_HZ: u32 = 100;
const PIT_DIVISOR: u16 = 11931;

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nostack, nomem));
}

/// Global tick counter, incremented each IRQ0.  100 ticks = 1 second.
pub static mut TICKS: u64 = 0;

/// Called from the IRQ0 stub in isr.asm.
#[no_mangle]
pub extern "C" fn pit_tick() {
    unsafe {
        let v = core::ptr::read_volatile(&raw const TICKS);
        core::ptr::write_volatile(&raw mut TICKS, v.wrapping_add(1));
    }
}

/// Atomically read current tick count.
#[inline(always)]
pub fn ticks() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const TICKS) }
}

/// Uptime in full seconds.
#[inline(always)]
pub fn uptime_secs() -> u64 { ticks() / PIT_HZ as u64 }

/// Uptime decomposed for display.
pub fn uptime_hms() -> (u32, u32, u32) {
    let s = uptime_secs() as u32;
    (s / 3600, (s % 3600) / 60, s % 60)
}

/// Program channel 0, mode 3 (square wave), 100 Hz.
pub fn init() {
    unsafe {
        // Channel 0 | access lo/hi | mode 3 | binary
        outb(PIT_CMD, 0x36);
        outb(PIT_CHANNEL0, (PIT_DIVISOR & 0xFF) as u8);
        outb(PIT_CHANNEL0, (PIT_DIVISOR >> 8) as u8);
    }
}