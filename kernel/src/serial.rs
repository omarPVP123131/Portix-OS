// kernel/src/serial.rs — PORTIX COM1 Serial Debug Port
#![allow(dead_code)]

const COM1: u16 = 0x3F8;

#[inline(always)] unsafe fn outb(p: u16, v: u8) {
    core::arch::asm!("out dx, al", in("dx") p, in("al") v, options(nostack, nomem));
}
#[inline(always)] unsafe fn inb(p: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", out("al") v, in("dx") p, options(nostack, nomem));
    v
}

pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00); // Disable interrupts
        outb(COM1 + 3, 0x80); // Enable DLAB
        outb(COM1 + 0, 0x03); // Divisor lo: 38400 baud
        outb(COM1 + 1, 0x00); // Divisor hi
        outb(COM1 + 3, 0x03); // 8N1
        outb(COM1 + 2, 0xC7); // Enable FIFO, clear, 14-byte threshold
        outb(COM1 + 4, 0x0B); // RTS/DSR set
    }
}

#[inline(always)]
fn tx_ready() -> bool { unsafe { inb(COM1 + 5) & 0x20 != 0 } }

pub fn write_byte(b: u8) {
    let mut limit = 1_000_000u32;
    while !tx_ready() && limit > 0 { limit -= 1; }
    unsafe { outb(COM1, b); }
}

pub fn write_str(s: &str) {
    for b in s.bytes() {
        if b == b'\n' { write_byte(b'\r'); }
        write_byte(b);
    }
}

pub fn write_bytes_raw(s: &[u8]) {
    for &b in s { write_byte(b); }
}

/// fmt_u32 without heap — writes decimal to serial.
pub fn write_u32(mut n: u32) {
    if n == 0 { write_byte(b'0'); return; }
    let mut buf = [0u8; 10];
    let mut i = 0usize;
    while n > 0 { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    buf[..i].reverse();
    write_bytes_raw(&buf[..i]);
}

/// Convenience: write "[TAG] message\n"
pub fn log(tag: &str, msg: &str) {
    write_str("[");
    write_str(tag);
    write_str("] ");
    write_str(msg);
    write_byte(b'\n');
}