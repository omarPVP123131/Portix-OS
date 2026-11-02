//! Reloj de tiempo real CMOS (MC146818) — fecha/hora reales del hardware.
//!
//! [FRENTE B-2] Antes `gettimeofday` devolvía `1726000000 + uptime` (fecha
//! inventada). Ahora se lee el RTC por los puertos 0x70/0x71 al arrancar y el
//! tiempo de pared es `epoch_arranque + ticks/PIT_HZ`. Necesario para validar
//! la vigencia de certificados X.509 en el cliente TLS 1.3.

use core::sync::atomic::{AtomicI64, Ordering};

static BOOT_EPOCH: AtomicI64 = AtomicI64::new(0);

#[inline]
unsafe fn cmos_read(reg: u8) -> u8 {
    let v: u8;
    // Bit 7 = NMI deshabilitado durante la lectura.
    core::arch::asm!("out dx, al", in("dx") 0x70u16, in("al") (reg | 0x80), options(nomem, nostack));
    core::arch::asm!("in al, dx", in("dx") 0x71u16, out("al") v, options(nomem, nostack));
    v
}

fn bcd(v: u8) -> u8 { (v & 0x0F) + ((v >> 4) * 10) }

/// Días desde 1970-01-01 hasta la fecha dada (algoritmo civil de H. Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Lee una instantánea coherente del RTC (dos lecturas iguales, sin UIP).
fn read_raw() -> (u8, u8, u8, u8, u8, u16) {
    unsafe {
        loop {
            while cmos_read(0x0A) & 0x80 != 0 {}
            let a = (cmos_read(0x00), cmos_read(0x02), cmos_read(0x04), cmos_read(0x07), cmos_read(0x08), cmos_read(0x09), cmos_read(0x32));
            while cmos_read(0x0A) & 0x80 != 0 {}
            let b = (cmos_read(0x00), cmos_read(0x02), cmos_read(0x04), cmos_read(0x07), cmos_read(0x08), cmos_read(0x09), cmos_read(0x32));
            if a == b {
                let status_b = cmos_read(0x0B);
                let is_bcd = status_b & 0x04 == 0;
                let is_12h = status_b & 0x02 == 0;
                let conv = |v: u8| if is_bcd { bcd(v) } else { v };
                let (mut sec, mut min, hour_raw, mut day, mut mon, mut year, century) =
                    (conv(a.0), conv(a.1), a.2, conv(a.3), conv(a.4), conv(a.5) as u16, a.6);
                let pm = is_12h && (hour_raw & 0x80) != 0;
                let mut hour = conv(hour_raw & 0x7F);
                if is_12h { if pm { if hour != 12 { hour += 12; } } else if hour == 12 { hour = 0; } }
                let cent = if century != 0 { conv(century) as u16 } else { 20 };
                year += cent * 100;
                if sec > 59 { sec = 59; } if min > 59 { min = 59; }
                if day == 0 { day = 1; } if mon == 0 { mon = 1; }
                return (sec, min, hour, day, mon, year);
            }
        }
    }
}

/// Segundos Unix según el RTC (UTC asumido, como en QEMU `-rtc base=utc`).
pub fn read_epoch() -> i64 {
    let (s, mi, h, d, mo, y) = read_raw();
    days_from_civil(y as i64, mo as i64, d as i64) * 86400 + (h as i64) * 3600 + (mi as i64) * 60 + s as i64
}

/// Fija el epoch de arranque (llamar una vez tras inicializar el PIT).
pub fn init() {
    let e = read_epoch();
    BOOT_EPOCH.store(e, Ordering::SeqCst);
    let (s, mi, h, d, mo, y) = read_raw();
    crate::drivers::serial::write_str("[RTC] CMOS: ");
    crate::drivers::serial::write_usize(y as usize); crate::drivers::serial::write_str("-");
    crate::drivers::serial::write_usize(mo as usize); crate::drivers::serial::write_str("-");
    crate::drivers::serial::write_usize(d as usize); crate::drivers::serial::write_str(" ");
    crate::drivers::serial::write_usize(h as usize); crate::drivers::serial::write_str(":");
    crate::drivers::serial::write_usize(mi as usize); crate::drivers::serial::write_str(":");
    crate::drivers::serial::write_usize(s as usize); crate::drivers::serial::write_str(" UTC epoch=");
    crate::drivers::serial::write_usize(e as usize); crate::drivers::serial::write_str("\n");
}

/// Tiempo de pared actual: epoch de arranque + segundos del PIT.
pub fn now_epoch() -> i64 {
    let base = BOOT_EPOCH.load(Ordering::Relaxed);
    if base == 0 { return read_epoch(); }
    base + (crate::time::pit::ticks() / crate::time::pit::PIT_HZ as u64) as i64
}

/// Descompone un epoch Unix en (año, mes, día, hora, min, seg) UTC.
pub fn civil_from_epoch(e: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = e.div_euclid(86400);
    let rem = e.rem_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}
