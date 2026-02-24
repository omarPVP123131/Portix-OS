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
use crate::pit;

const PS2_DATA:   u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_CMD:    u16 = 0x64;

const TELEPORT_THRESHOLD: i32 = 120;
const ERROR_LIMIT: u32 = 25;

#[inline(always)] unsafe fn inb(p: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", out("al") v, in("dx") p, options(nostack, nomem));
    v
}
#[inline(always)] unsafe fn outb(p: u16, v: u8) {
    core::arch::asm!("out dx, al", in("dx") p, in("al") v, options(nostack, nomem));
}
#[inline(always)] unsafe fn io_wait() {
    core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
}

unsafe fn wait_write() {
    let mut lim = 100_000u32;
    while inb(PS2_STATUS) & 0x02 != 0 && lim > 0 { lim -= 1; io_wait(); }
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

unsafe fn mouse_cmd(cmd: u8) -> bool {
    wait_write(); outb(PS2_CMD, 0xD4);
    wait_write(); outb(PS2_DATA, cmd);
    if !wait_read() { return false; }
    inb(PS2_DATA) == 0xFA
}

unsafe fn mouse_cmd_arg(cmd: u8, arg: u8) -> bool {
    if !mouse_cmd(cmd) { return false; }
    wait_write(); outb(PS2_CMD, 0xD4);
    wait_write(); outb(PS2_DATA, arg);
    if !wait_read() { return false; }
    inb(PS2_DATA) == 0xFA
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
        // Sincronización por timeout entre bytes del mismo paquete
        let current_tick = pit::ticks();
        if self.pkt_idx > 0 && current_tick.saturating_sub(self.last_tick) > 5 {
            self.pkt_idx = 0;
        }
        self.last_tick = current_tick;

        match self.pkt_idx {
            0 => {
                // Bit 3 siempre a 1 en el byte de flags; si no, estamos desalineados
                if byte & 0x08 == 0 { return false; }
                self.pkt[0] = byte;
                self.pkt_idx = 1;
                false
            }
            1 => {
                self.pkt[1] = byte;
                self.pkt_idx = 2;
                false
            }
            2 => {
                self.pkt[2] = byte;
                self.pkt_idx = 0;
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

        unsafe {
            drain_kbc();
            wait_write(); outb(PS2_CMD, 0xA8);

            wait_write(); outb(PS2_CMD, 0x20);
            if !wait_read() { return false; }
            let cfg = inb(PS2_DATA);
            wait_write(); outb(PS2_CMD, 0x60);
            wait_write(); outb(PS2_DATA, (cfg | 0x02) & !0x20);

            mouse_cmd(0xF6);
            mouse_cmd_arg(0xF3, 100);
            if !mouse_cmd(0xF4) { return false; }

            drain_kbc();
            self.present = true;
            true
        }
    }

    pub fn intelligent_reset(&mut self) {
        let now = pit::ticks();
        if now.saturating_sub(self.last_reset_tick) < 100 { return; }

        unsafe {
            drain_kbc();
            mouse_cmd(0xF6);
            mouse_cmd(0xF4);
            drain_kbc();
        }

        self.resets += 1;
        self.error_count = 0;
        self.pkt_idx = 0;
        self.last_reset_tick = now;
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
            self.error_count = self.error_count.saturating_add(1);
            return false;
        }

        self.buttons = flags & 0x07;

        let sensitivity: i32 = 2;
        let old_x = self.x;
        let old_y = self.y;

        self.x = (self.x + dx * sensitivity).clamp(0, self.max_x);
        self.y = (self.y - dy * sensitivity).clamp(0, self.max_y);

        if self.error_count > 0 { self.error_count -= 1; }

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