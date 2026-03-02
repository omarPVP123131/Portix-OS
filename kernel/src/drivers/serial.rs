// kernel/src/drivers/serial.rs — PORTIX COM1 Serial Debug Port
// Nivel kernel-grade: log levels, hex dump, loopback self-test.
#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};

const COM1: u16 = 0x3F8;

static SERIAL_OK: AtomicBool = AtomicBool::new(false);

// ── I/O primitivos ────────────────────────────────────────────────────────────

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port, in("al") val,
        options(nostack, nomem)
    );
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") v, in("dx") port,
        options(nostack, nomem)
    );
    v
}

// ── Inicialización + loopback test ────────────────────────────────────────────

/// Inicializa COM1 a 38400 8N1.
/// Hace un loopback test; si falla, el puerto queda marcado como no-disponible
/// y write_byte() se convierte en no-op para no colgar el kernel.
pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00); // Deshabilitar interrupciones
        outb(COM1 + 3, 0x80); // Habilitar DLAB
        outb(COM1 + 0, 0x03); // Divisor lo → 38400 baud
        outb(COM1 + 1, 0x00); // Divisor hi
        outb(COM1 + 3, 0x03); // 8 bits, sin paridad, 1 stop (8N1)
        outb(COM1 + 2, 0xC7); // Habilitar FIFO, limpiar, umbral 14 bytes
        outb(COM1 + 4, 0x1E); // Modo loopback para autotest

        // Loopback test: enviar 0xAE, esperar eco
        outb(COM1 + 0, 0xAE);
        let loopback = inb(COM1 + 0);

        if loopback != 0xAE {
            // Hardware no responde — serial deshabilitado silenciosamente
            SERIAL_OK.store(false, Ordering::Release);
            return;
        }

        // Hardware OK → modo normal
        outb(COM1 + 4, 0x0B); // RTS/DSR activos
        SERIAL_OK.store(true, Ordering::Release);
    }

log_level(Level::Ok, "SERIAL", "COM1 listo @ 38400 8N1");
}

// ── Niveles de log ────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub enum Level {
    Debug,
    Info,
    Ok,
    Warn,
    Error,
}

impl Level {
    fn prefix(self) -> &'static str {
        match self {
            Level::Debug => "[ DBG ]",
            Level::Info  => "[ INF ]",
            Level::Ok    => "[  OK ]",
            Level::Warn  => "[ WRN ]",
            Level::Error => "[ ERR ]",
        }
    }
}

// ── Escritura ─────────────────────────────────────────────────────────────────

#[inline(always)]
fn tx_ready() -> bool {
    unsafe { inb(COM1 + 5) & 0x20 != 0 }
}

pub fn write_byte(b: u8) {
    if !SERIAL_OK.load(Ordering::Relaxed) {
        return;
    }
    let mut limit = 1_000_000u32;
    while !tx_ready() && limit > 0 {
        limit -= 1;
    }
    unsafe { outb(COM1, b); }
}

pub fn write_str(s: &str) {
    for b in s.bytes() {
        if b == b'\n' {
            write_byte(b'\r');
        }
        write_byte(b);
    }
}

pub fn write_bytes_raw(s: &[u8]) {
    for &b in s {
        write_byte(b);
    }
}

pub fn write_u32(mut n: u32) {
    if n == 0 {
        write_byte(b'0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 0usize;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    buf[..i].reverse();
    write_bytes_raw(&buf[..i]);
}

pub fn write_usize(n: usize) {
    write_u32(n as u32); // suficiente para 32-bit; ampliar si se porta a 64-bit
}

/// Imprime un `usize` como `0xDEADBEEF`.
pub fn write_hex(n: usize) {
    const HEX: &[u8] = b"0123456789ABCDEF";
    write_str("0x");
    // 8 dígitos para 32-bit
    for shift in (0..8).rev() {
        let nibble = (n >> (shift * 4)) & 0xF;
        write_byte(HEX[nibble]);
    }
}

/// Mensaje simple — compatibilidad con el código existente: log("TAG", "msg")
pub fn log(tag: &str, msg: &str) {
    write_str("[ INF ] ");
    write_str(tag);
    write_str("  ");
    write_str(msg);
    write_byte(b'\n');
}

/// Mensaje con nivel explícito — uso nuevo: log_level(Level::Ok, "TAG", "msg")
pub fn log_level(level: Level, tag: &str, msg: &str) {
    write_str(level.prefix());
    write_byte(b' ');
    write_str(tag);
    write_str("  ");
    write_str(msg);
    write_byte(b'\n');
}

#[macro_export]
macro_rules! serial_log {
    ($lvl:ident, $tag:expr, $msg:expr) => {
        $crate::drivers::serial::log_level(
            $crate::drivers::serial::Level::$lvl,
            $tag,
            $msg,
        )
    };
}