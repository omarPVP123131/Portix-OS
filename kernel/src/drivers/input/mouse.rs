// kernel/src/mouse.rs — PORTIX PS/2 Mouse Driver v6.3
//
// CAMBIOS vs v6.2:
//   - feed() ahora es pub — el drenado unificado de main lo llama directamente.
//   - intelligent_reset() ahora es pub — main necesita llamarlo tras el drain.
//   - begin_frame() nuevo: separa "inicio de ciclo de poll" de la lectura HW.
//     Antes poll() hacía ambas cosas; ahora el drenado unificado de main
//     llama begin_frame() + feed(byte) por cada byte de ratón que encuentra.
//   - poll() eliminado: ya no tiene sentido con el drenado unificado.
//     Si se necesita compatibilidad temporal, se puede mantener pero NO debe
//     coexistir con el drenado unificado o habrá doble lectura del buffer.

#![allow(dead_code)]
use core::sync::atomic::{AtomicU32, Ordering};
use crate::time::pit;
use crate::drivers::serial;

const PS2_DATA:   u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_CMD:    u16 = 0x64;

static BYTE_LOG_CNT: AtomicU32 = AtomicU32::new(0);
static PKT_LOG_CNT: AtomicU32 = AtomicU32::new(0);
pub fn init_done() -> u32 { BYTE_LOG_CNT.load(Ordering::Relaxed) }

const TELEPORT_THRESHOLD: i32 = 300;
const ERROR_LIMIT: u32 = 25;

#[inline(always)] unsafe fn inb(p: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", out("al") v, in("dx") p, options(nostack));
    v
}
#[inline(always)] unsafe fn outb(p: u16, v: u8) {
    core::arch::asm!("out dx, al", in("dx") p, in("al") v, options(nostack, nomem));
}
#[inline(always)] unsafe fn io_wait() {
    core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
}

unsafe fn wait_write() -> bool {
    let mut lim = 200_000u32;
    while inb(PS2_STATUS) & 0x02 != 0 && lim > 0 { lim -= 1; io_wait(); }
    inb(PS2_STATUS) & 0x02 == 0
}

unsafe fn wait_read() -> bool {
    let mut lim = 100_000u32;
    while inb(PS2_STATUS) & 0x01 == 0 && lim > 0 { lim -= 1; io_wait(); }
    inb(PS2_STATUS) & 0x01 != 0
}

unsafe fn drain_kbc() {
    let mut lim = 100u32;
    while inb(PS2_STATUS) & 0x01 != 0 && lim > 0 {
        let _ = inb(PS2_DATA); lim -= 1; io_wait();
    }
}

/// Poll PS/2 controller for a mouse byte (AUXB=1).
/// Non-blocking — returns None if no mouse data available.
/// Safe to call from main loop as fallback when IRQ12 doesn't fire.
#[inline]
pub unsafe fn poll_aux() -> Option<u8> {
    let st: u8;
    core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
    if st & 0x01 != 0 && st & 0x20 != 0 {
        let b: u8;
        core::arch::asm!("in al, dx", out("al") b, in("dx") 0x60u16, options(nostack));
        BYTE_LOG_CNT.fetch_add(1, Ordering::Relaxed);
        Some(b)
    } else {
        None
    }
}

unsafe fn wait_read_auxb() -> Option<u8> {
    let mut lim = 100_000u32;
    while lim > 0 {
        let st = inb(PS2_STATUS);
        if st & 0x01 != 0 {
            let b = inb(PS2_DATA);
            if st & 0x20 != 0 { return Some(b); }
        }
        lim -= 1;
        io_wait();
    }
    None
}


unsafe fn send_mouse_cmd_wait(cmd: u8, timeout: u32) -> bool {
    for retry in 0..3 {
        for _ in 0..16 {
            let st = inb(PS2_STATUS);
            if st & 0x01 == 0 { break; }
            let _ = inb(PS2_DATA);
            io_wait();
        }
        if !wait_write() { continue; }
        outb(PS2_CMD, 0xD4);
        if !wait_write() { continue; }
        outb(PS2_DATA, cmd);
        for _ in 0..timeout {
            let st = inb(PS2_STATUS);
            if st & 0x01 != 0 {
                let b = inb(PS2_DATA);
                if b == 0xFA { return true; }
                if b == 0xFE { break; }
            }
            io_wait();
        }
        if retry < 2 { io_wait(); }
    }
    serial::write_str("[MS] cmd ");
    serial::write_hex(cmd as usize);
    serial::write_str(" FAIL after retries\n");
    false
}

unsafe fn send_mouse_cmd_with_arg_wait(cmd: u8, arg: u8, timeout: u32) -> bool {
    if !send_mouse_cmd_wait(cmd, timeout) { return false; }
    for _ in 0..16 {
        let st = inb(PS2_STATUS);
        if st & 0x01 == 0 { break; }
        let _ = inb(PS2_DATA);
        io_wait();
    }
    if !wait_write() { return false; }
    outb(PS2_CMD, 0xD4);
    if !wait_write() { return false; }
    outb(PS2_DATA, arg);
    for _ in 0..timeout {
        let st = inb(PS2_STATUS);
        if st & 0x01 != 0 {
            let b = inb(PS2_DATA);
            if b == 0xFA { return true; }
            if b == 0xFE { break; }
        }
        io_wait();
    }
    false
}

pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub buttons: u8,
    pub prev_buttons: u8,

    pkt: [u8; 3],
    pkt_idx: u8,
    last_tick: u64,

    pub max_x: i32,
    pub max_y: i32,
    pub present: bool,
    pub has_wheel: bool,
    pub scroll_delta: i32,

    pub error_count: u32,
    pub resets: u32,
    last_reset_tick: u64,
}

impl MouseState {
    pub const fn new() -> Self {
        Self {
            x: 400, y: 300,
            buttons: 0, prev_buttons: 0,
            pkt: [0; 3], pkt_idx: 0,
            last_tick: 0,
            max_x: 1024, max_y: 768,
            present: false,
            has_wheel: false,
            scroll_delta: 0,
            error_count: 0,
            resets: 0,
            last_reset_tick: 0,
        }
    }

    /// Llamar UNA VEZ al inicio de cada frame, antes de feed().
    /// Guarda el estado de botones del frame anterior y resetea scroll_delta.
    /// Antes esto lo hacía poll() internamente; ahora el drenado unificado
    /// de main lo llama explícitamente.
    pub fn begin_frame(&mut self) {
        self.prev_buttons = self.buttons;
        self.scroll_delta = 0;
    }

    /// Procesa un byte ya leído del buffer PS/2 (AUXB=1).
    /// Devuelve true si el paquete se completó y hubo cambio de estado.
    /// El caller (main) es responsable de haber verificado AUXB antes de llamar.
pub fn feed(&mut self, byte: u8) -> bool {
    // Filtrar bytes de respuesta/comando conocidos (ACK=0xFA, BAT=0xAA, RESEND=0xFE)
    if byte == 0xFA || byte == 0xAA || byte == 0xFE {
        self.pkt_idx = 0;
        return false;
    }

    // Sincronización por timeout entre bytes del mismo paquete (reducido a 5 ticks ~50ms)
    let current_tick = pit::ticks();
    if self.pkt_idx > 0 && current_tick.saturating_sub(self.last_tick) > 5 {
        self.pkt_idx = 0;
    }
    self.last_tick = current_tick;

    match self.pkt_idx {
        0 => {
            // Byte 0 (flags): bit 3 (0x08) debe ser 1, bits 6-7 (0xC0) deben ser 0
            if (byte & 0x08) == 0 || (byte & 0xC0) != 0 {
                // Byte inválido — probable desync parcial. No contar error:
                // el timeout de sincronización ya reseteó pkt_idx, y este byte
                // es probablemente byte 1 o 2 de un paquete anterior. Reseteamos
                // en silencio y esperamos el próximo byte.
                return false;
            }
            self.pkt[0] = byte;
            self.pkt_idx = 1;
            false
        }
        1 => {
            // Byte 1: delta X (signed 8-bit, sign in flags bit 4)
            // Cualquier valor 0x00-0xFF es válido
            self.pkt[1] = byte;
            self.pkt_idx = 2;
            false
        }
        2 => {
            // Byte 2: delta Y (signed 8-bit, sign in flags bit 5)
            self.pkt[2] = byte;
            self.pkt_idx = 0;
            
            // Si tiene rueda, esperar 4to byte (Z delta)
            if self.has_wheel {
                self.pkt_idx = 3; // Estado especial: esperando 4to byte
                return false;
            }
            self.process()
        }
        3 => {
            // Byte 3: delta Z (wheel) para IntelliMouse
            self.pkt_idx = 0;
            // Guardar scroll delta para procesamiento posterior
            let dz: i32 = byte as i8 as i32;
            self.scroll_delta = dz;
            self.process()
        }
        _ => { self.pkt_idx = 0; false }
    }
}

pub fn init(&mut self, sw: usize, sh: usize) -> bool {
    self.max_x = (sw as i32).saturating_sub(1);
    self.max_y = (sh as i32).saturating_sub(1);
    self.x = self.max_x / 2;
    self.y = self.max_y / 2;
    self.has_wheel = false;
    self.present = false;
    self.error_count = 0;
    self.resets = 0;
    self.pkt_idx = 0;

    unsafe {
        let st0: u8;
        core::arch::asm!("in al, dx", out("al") st0, in("dx") 0x64u16, options(nostack));
        serial::write_str("[MS] init status=");
        serial::write_hex(st0 as usize);
        serial::write_str("\n");

        // Drain stale bytes
        for _ in 0..256 {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x01 == 0 { break; }
            let b: u8;
            core::arch::asm!("in al, dx", out("al") b, in("dx") 0x60u16, options(nostack));
                serial::write_str("[MS] drain=");
                serial::write_hex(b as usize);
            serial::write_str("\n");
        }

        // ── Inicialización robusta del 8042 + mouse ──────────────

        // 1. Habilitar puerto auxiliar (0xA8)
        if !wait_write() {
            serial::log("MS", "FAIL: timeout enabling aux port");
            return false;
        }
        outb(PS2_CMD, 0xA8);
        io_wait();
        serial::log("MS", "aux port enabled");

        // 2. Leer CCB y activar IRQ12 + aux clock (bit 1=IRQ12, bit 5=aux clock)
        if !wait_write() { serial::log("MS", "FAIL: CCB read WW"); return false; }
        outb(PS2_CMD, 0x20);
        if !wait_read() { serial::log("MS", "FAIL: CCB read timeout"); return false; }
        let ccb = inb(PS2_DATA);
        serial::write_str("[MS] CCB=");
        serial::write_hex(ccb as usize);
        serial::write_str("\n");
        // CCB bits: bit0=IRQ1, bit1=IRQ12, bit4=kbd clock disable, bit5=aux clock disable, bit6=translation
        // new: IRQ1+IRQ12 ON, keep original translation+clock settings
        let new_ccb = (ccb | 0x03) & !0x10 & !0x20;
        if !wait_write() { serial::log("MS", "FAIL: CCB write WW"); return false; }
        outb(PS2_CMD, 0x60);
        if !wait_write() { serial::log("MS", "FAIL: CCB data WW"); return false; }
        outb(PS2_DATA, new_ccb);
        serial::write_str("[MS] new_ccb=");
        serial::write_hex(new_ccb as usize);
        serial::write_str("\n");

        // 3. Try to reset mouse (0xFF) - if this fails, the mouse may still work
        //    in its current state (e.g. left by BIOS in streaming mode).
        //    We continue with present=true and rely on polling fallback in main loop.
        let reset_ok = send_mouse_cmd_wait(0xFF, 5000);
        if reset_ok {
            let bat = Self::wait_obf_read_any(5000);
            serial::write_str("[MS] post-reset BAT=");
            serial::write_hex(bat as usize);
            serial::write_str("\n");
            if bat == 0xAA {
                // Read device ID (not used for has_wheel — see step 6)
                let _dev_id = Self::wait_obf_read_any(5000);
                serial::write_str("[MS] device ID=");
                serial::write_hex(_dev_id as usize);
                serial::write_str("\n");
            } else {
                serial::write_str("[MS] BAT unexpected (0x");
                serial::write_hex(bat as usize);
                serial::write_str("), continuing without reset\n");
            }
        } else {
            serial::log("MS", "reset failed (non-fatal, will use polling)");
        }
        drain_kbc();

        // 4. Try set defaults (0xF6) — non-fatal
        if !send_mouse_cmd_wait(0xF6, 5000) {
            serial::log("MS", "set defaults failed (non-fatal)");
        }
        drain_kbc();

        // 5. Try enable streaming mode (0xF4) — non-fatal.
        // If this fails, the mouse might already be in streaming mode (BIOS)
        // or the main-loop polling fallback will pick up data.
        if !send_mouse_cmd_wait(0xF4, 5000) {
            serial::log("MS", "enable streaming failed (non-fatal, polling fallback)");
        }
        drain_kbc();

        // 6. Try to enable wheel (IntelliMouse sequence: 0xF3 200, 0xF3 100, 0xF3 80)
        // Only if ALL wheel commands succeed AND the device responds with ID 0x03/0x04.
        if send_mouse_cmd_with_arg_wait(0xF3, 200, 5000)
            && send_mouse_cmd_with_arg_wait(0xF3, 100, 5000)
            && send_mouse_cmd_with_arg_wait(0xF3, 80, 5000) {
            let wheel_id = Self::wait_obf_read_any(5000);
            if wheel_id == 0x03 || wheel_id == 0x04 {
                serial::log("MS", "IntelliMouse wheel enabled");
                self.has_wheel = true;
            } else {
                serial::write_str("[MS] wheel activation: unexpected ID=");
                serial::write_hex(wheel_id as usize);
                serial::write_str(" (ignored, 3-byte mode)\n");
            }
        }
        drain_kbc();
    }

    self.pkt_idx = 0;
    self.error_count = 0;
    self.resets = 0;
    self.present = true;
    serial::log("MS", "init OK");
    true
}



    /// Read ANY byte from output buffer (no AUXB filter), with timeout.
    /// Returns 0xFF on timeout.
    unsafe fn wait_obf_read_any(limit: u32) -> u8 {
        for _ in 0..limit {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x01 != 0 {
                let b: u8;
                core::arch::asm!("in al, dx", out("al") b, in("dx") 0x60u16, options(nostack));
                return b;
            }
            core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack));
        }
        0xFF
    }

    unsafe fn wait_obf_read_auxb() -> bool {
        for i in 0..200000 {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x01 != 0 {
                let b: u8;
                core::arch::asm!("in al, dx", out("al") b, in("dx") 0x60u16, options(nostack));
                if st & 0x20 != 0 {
                    return b == 0xFA;
                }
                if i < 5 {
            serial::write_str("[MSDBG] drain kbd byte ");
            serial::write_hex(b as usize);
            serial::write_str("\n");
                }
            }
            core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack));
            if i == 50000 {
                let st2: u8;
                core::arch::asm!("in al, dx", out("al") st2, in("dx") 0x64u16, options(nostack));
                serial::write_str("[MSDBG] wait_obf: mid status=0x");
                serial::write_hex(st2 as usize);
                serial::write_str("\n");
            }
        }
        serial::write_str("[MSDBG] wait_obf: timeout\n");
        false
    }

    pub fn intelligent_reset(&mut self) {
        let now = pit::ticks();
        if now.saturating_sub(self.last_reset_tick) < 100 { return; }

        serial::log("MS", "entered");

        // Software-only reset: do NOT send real commands (0xFF/0xF4) to the
        // mouse, because even without ACK the bytes ARE forwarded to the
        // device via 0xD4+0x60 and can physically reset it to disabled state.
        // Since init already failed, the mouse is in whatever default state
        // QEMU gives us; sending partial init sequences only makes it worse.
        unsafe { drain_kbc(); }

        self.resets = self.resets.saturating_add(1);
        self.last_reset_tick = now;
        self.error_count = 0;
        self.pkt_idx = 0;
    }

    fn process(&mut self) -> bool {
        let flags = self.pkt[0];

        if flags & 0xC0 != 0 {
            self.error_count = self.error_count.saturating_add(1);
            return false;
        }

        // Reconstrucción correcta del entero de 9 bits PS/2.
        // El bit de signo de dx está en flags bit 4 (0x10).
        // El bit de signo de dy está en flags bit 5 (0x20).
        // Tratar pkt[1] como i8 directamente es incorrecto para deltas ≥128
        // con signo positivo: el bit 7 se interpreta como negativo → teleport.
        let dx: i32 = if flags & 0x10 != 0 {
            (self.pkt[1] as i32) - 256
        } else {
            self.pkt[1] as i32
        };

        let dy: i32 = if flags & 0x20 != 0 {
            (self.pkt[2] as i32) - 256
        } else {
            self.pkt[2] as i32
        };

        if dx.abs() > TELEPORT_THRESHOLD || dy.abs() > TELEPORT_THRESHOLD {
            return false;
        }

        self.buttons = flags & 0x07;

        let sensitivity: i32 = 2;
        let old_x = self.x;
        let old_y = self.y;

        self.x = (self.x + dx * sensitivity).clamp(0, self.max_x);
        self.y = (self.y - dy * sensitivity).clamp(0, self.max_y);

        self.x != old_x || self.y != old_y || self.buttons != self.prev_buttons
    }

    #[inline] pub fn left_btn(&self)    -> bool { self.buttons & 0x01 != 0 }
    #[inline] pub fn right_btn(&self)   -> bool { self.buttons & 0x02 != 0 }
    #[inline] pub fn middle_btn(&self)  -> bool { self.buttons & 0x04 != 0 }

    #[inline] pub fn left_clicked(&self) -> bool {
        self.buttons & 0x01 != 0 && self.prev_buttons & 0x01 == 0
    }
    #[inline] pub fn right_clicked(&self) -> bool {
        self.buttons & 0x02 != 0 && self.prev_buttons & 0x02 == 0
    }
    #[inline] pub fn left_released(&self) -> bool {
        self.buttons & 0x01 == 0 && self.prev_buttons & 0x01 != 0
    }
}