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
    if !tx_ready() { return; }
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
    if n == 0 {
        write_byte(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 0usize;
    let mut m = n;
    while m > 0 {
        buf[i] = b'0' + (m % 10) as u8;
        m /= 10;
        i += 1;
    }
    buf[..i].reverse();
    write_bytes_raw(&buf[..i]);
}

/// Imprime un `usize` como `0xDEADBEEF` (64-bit completo en x86_64).
pub fn write_hex(n: usize) {
    const HEX: &[u8] = b"0123456789ABCDEF";
    write_str("0x");
    for shift in (0..16).rev() {
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

/// kprintln! — Log con nivel, tag, y archivo:línea automáticos.
/// Uso: kprintln!(Info, "TAG", "mensaje {}", arg);
///      kprintln!(Error, "TAG", "fallo en x={}", val);
#[macro_export]
macro_rules! kprintln {
    ($lvl:ident, $tag:expr, $msg:expr $(, $arg:expr)* $(,)?) => {{
        $crate::drivers::serial::log_level(
            $crate::drivers::serial::Level::$lvl,
            $tag,
            core::concat!(
                $msg,
                " [",
                core::file!(),
                ":",
                core::stringify!(core::line!()),
                "]"
            ),
        );
        $( $crate::drivers::serial::write_str("  arg=");
           $crate::drivers::serial::write_hex($arg as usize);
           $crate::drivers::serial::write_byte(b'\n'); )*
    }};
}

/// kassert! — Assert que logea ubicación antes de panic.
/// Uso: kassert!(condición, "mensaje opcional");
#[macro_export]
macro_rules! kassert {
    ($cond:expr $(, $msg:expr)? $(,)?) => {
        if !$cond {
            $crate::kprintln!(Error, "ASSERT",
                core::concat!("FAIL: ", core::stringify!($cond) $(, ": ", $msg)?));
            panic!(core::concat!("ASSERT FAIL: ", core::stringify!($cond)
                $(, ": ", $msg)?));
        }
    };
}

/// kassert_eq! — Assert con igualdad que logea ubicación.
#[macro_export]
macro_rules! kassert_eq {
    ($left:expr, $right:expr $(,)?) => {
        if $left != $right {
            $crate::kprintln!(Error, "ASSERT_EQ",
                core::concat!(
                    "FAIL: ", core::stringify!($left), " != ", core::stringify!($right)
                ));
            panic!(core::concat!(
                "ASSERT_EQ FAIL: ", core::stringify!($left), " != ", core::stringify!($right)
            ));
        }
    };
}

/// hexdump — Imprime `len` bytes de `data` en formato hex+ASCII por serial.
pub fn hexdump(tag: &str, data: &[u8], len: usize) {
    let n = len.min(data.len());
    let mut off = 0usize;
    while off < n {
        let row_end = (off + 16).min(n);
        write_str(tag);
        write_str("  ");
        write_hex(off);
        write_str("  ");
        for i in off..row_end {
            let b = data[i];
            const H: &[u8] = b"0123456789ABCDEF";
            write_byte(H[(b >> 4) as usize]);
            write_byte(H[(b & 0xF) as usize]);
            write_byte(b' ');
        }
        for _ in row_end..off + 16 { write_str("   "); }
        write_str(" |");
        for i in off..row_end {
            let b = data[i];
            write_byte(if b >= 0x20 && b < 0x7F { b } else { b'.' });
        }
        write_str("|\n");
        off = row_end;
    }
}

/// dump_regs — Imprime registros en formato grid para debugging.
pub fn dump_regs(tag: &str, rip: u64, rsp: u64, cr3: u64, gpr: &[u64; 15]) {
    write_str("[ ");
    write_str(tag);
    write_str(" ] ");
    write_str("RIP=");
    write_hex(rip as usize);
    write_str("  RSP=");
    write_hex(rsp as usize);
    write_str("  CR3=");
    write_hex(cr3 as usize);
    write_byte(b'\n');
    write_str("  RAX="); write_hex(gpr[0] as usize);
    write_str("  RBX="); write_hex(gpr[1] as usize);
    write_str("  RCX="); write_hex(gpr[2] as usize);
    write_str("  RDX="); write_hex(gpr[3] as usize);
    write_byte(b'\n');
    write_str("  RSI="); write_hex(gpr[4] as usize);
    write_str("  RDI="); write_hex(gpr[5] as usize);
    write_str("  RBP="); write_hex(gpr[6] as usize);
    write_str("  R08="); write_hex(gpr[7] as usize);
    write_byte(b'\n');
    write_str("  R09="); write_hex(gpr[8] as usize);
    write_str("  R10="); write_hex(gpr[9] as usize);
    write_str("  R11="); write_hex(gpr[10] as usize);
    write_str("  R12="); write_hex(gpr[11] as usize);
    write_byte(b'\n');
    write_str("  R13="); write_hex(gpr[12] as usize);
    write_str("  R14="); write_hex(gpr[13] as usize);
    write_str("  R15="); write_hex(gpr[14] as usize);
    write_byte(b'\n');
}