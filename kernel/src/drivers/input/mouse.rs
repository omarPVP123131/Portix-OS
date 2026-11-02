// kernel/src/drivers/input/mouse.rs — PORTIX PS/2 Mouse Driver v7.0
//
// Driver de ratón PS/2 optimizado para bare-metal, VirtualBox, VMware y QEMU.
// Correcciones v7.0:
//   - Eliminado filtro erróneo de 0xFA/0xFE/0xAA en feed(): en movimiento negativo,
//     dx o dy pueden valer -2 (0xFE), -6 (0xFA) o -86 (0xAA). Filtrarlos rompía
//     el paquete y congelaba el ratón ("a veces no se mueve").
//   - Conversión de movimiento canónica PS/2 (9-bit complemento a dos OSDev standard):
//     dx = if x_sign { raw - 256 } else { raw }.
//   - Timeout de resincronización de paquetes ampliado a 250ms (25 ticks a 100Hz)
//     para absorber jitter de hipervisores como VirtualBox.
//   - Inicialización canónica de 8042 con 0xF4 (Enable Streaming) como comando terminal.

#![allow(dead_code)]
use core::sync::atomic::{AtomicU32, Ordering};
use crate::time::pit;
use crate::drivers::serial;

const PS2_DATA:   u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_CMD:    u16 = 0x64;

struct InterruptGuard {
    flags: u64,
}

impl InterruptGuard {
    fn new() -> Self {
        let flags: u64;
        unsafe {
            core::arch::asm!("pushfq; pop {}", out(reg) flags, options(nostack));
            core::arch::asm!("cli", options(nostack));
        }
        Self { flags }
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.flags & 0x200 != 0 {
            unsafe {
                core::arch::asm!("sti", options(nostack));
            }
        }
    }
}

static BYTE_LOG_CNT: AtomicU32 = AtomicU32::new(0);
pub fn init_done() -> u32 { BYTE_LOG_CNT.load(Ordering::Relaxed) }

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
    let mut lim = 100_000u32;
    while inb(PS2_STATUS) & 0x02 != 0 && lim > 0 { lim -= 1; io_wait(); }
    inb(PS2_STATUS) & 0x02 == 0
}

unsafe fn wait_read() -> bool {
    let mut lim = 100_000u32;
    while inb(PS2_STATUS) & 0x01 == 0 && lim > 0 { lim -= 1; io_wait(); }
    inb(PS2_STATUS) & 0x01 != 0
}

unsafe fn drain_kbc() {
    let mut lim = 256u32;
    while inb(PS2_STATUS) & 0x01 != 0 && lim > 0 {
        let _ = inb(PS2_DATA);
        lim -= 1;
        io_wait();
    }
}

/// Poll PS/2 controller for a mouse byte (OBF=1 && AUXB=1).
/// Non-blocking — returns None if no mouse data available.
#[inline]
pub unsafe fn poll_aux() -> Option<u8> {
    let st = inb(PS2_STATUS);
    // Verificar que el buffer de salida esté lleno (OBF = bit 0)
    // y que los datos pertenezcan al dispositivo auxiliar/ratón (AUXB = bit 5)
    if st & 0x01 != 0 && st & 0x20 != 0 {
        let b = inb(PS2_DATA);
        BYTE_LOG_CNT.fetch_add(1, Ordering::Relaxed);
        Some(b)
    } else {
        None
    }
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

    pkt: [u8; 4],
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
            pkt: [0; 4], pkt_idx: 0,
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

    /// Llamar UNA VEZ al inicio de cada frame antes de procesar la cola de bytes.
    pub fn begin_frame(&mut self) {
        self.prev_buttons = self.buttons;
        self.scroll_delta = 0;
    }

    /// Procesa un byte leído del buffer PS/2.
    /// Devuelve true si el paquete se completó y generó cambio en coordenadas o botones.
    pub fn feed(&mut self, byte: u8) -> bool {
        let current_tick = pit::ticks();
        // Timeout de sincronización: si transcurren > 250ms entre bytes de un mismo
        // paquete, descartamos el estado parcial para rearmar la sincronización.
        if self.pkt_idx > 0 && current_tick.saturating_sub(self.last_tick) > 25 {
            self.pkt_idx = 0;
        }
        self.last_tick = current_tick;

        match self.pkt_idx {
            0 => {
                // Byte 0 (flags): bit 3 (0x08) DEBE ser 1 en todo paquete estándar PS/2.
                // Bits 6 y 7 representan overflow y deben ser 0 para un paquete válido.
                if (byte & 0x08) == 0 || (byte & 0xC0) != 0 {
                    // Descarte de byte inválido para Byte 0
                    return false;
                }
                self.pkt[0] = byte;
                self.pkt_idx = 1;
                false
            }
            1 => {
                // Byte 1: delta X (cualquier valor de 8-bit es válido, incluyendo 0xFA y 0xFE)
                self.pkt[1] = byte;
                self.pkt_idx = 2;
                false
            }
            2 => {
                // Byte 2: delta Y (cualquier valor de 8-bit es válido)
                self.pkt[2] = byte;
                if self.has_wheel {
                    self.pkt_idx = 3;
                    false
                } else {
                    self.pkt_idx = 0;
                    self.process()
                }
            }
            3 => {
                // Byte 3: rueda de desplazamiento (IntelliMouse)
                self.pkt[3] = byte;
                self.pkt_idx = 0;
                self.scroll_delta = byte as i8 as i32;
                self.process()
            }
            _ => {
                self.pkt_idx = 0;
                false
            }
        }
    }

    pub fn init(&mut self, sw: usize, sh: usize) -> bool {
        let _guard = InterruptGuard::new();
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
            // 1. Drenar bytes residuales
            drain_kbc();

            // 2. Habilitar puerto auxiliar PS/2 (0xA8)
            if !wait_write() {
                serial::log("MS", "FAIL: timeout habilitando puerto auxiliar");
                return false;
            }
            outb(PS2_CMD, 0xA8);
            io_wait();

            // 3. Configurar Controller Configuration Byte (CCB)
            if !wait_write() { return false; }
            outb(PS2_CMD, 0x20);
            if !wait_read() { return false; }
            let ccb = inb(PS2_DATA);

            // Activar IRQ1 (bit 0), IRQ12 (bit 1) y limpiar reloj deshabilitado (bit 4, bit 5)
            let new_ccb = (ccb | 0x03) & !0x20;
            if !wait_write() { return false; }
            outb(PS2_CMD, 0x60);
            if !wait_write() { return false; }
            outb(PS2_DATA, new_ccb);
            io_wait();

            // 4. Reset del ratón (0xFF)
            if send_mouse_cmd_wait(0xFF, 50_000) {
                let bat = Self::wait_obf_read_any(50_000);
                if bat == 0xAA {
                    let _dev_id = Self::wait_obf_read_any(50_000);
                }
            }
            drain_kbc();

            // 5. Configurar valores por defecto (0xF6)
            send_mouse_cmd_wait(0xF6, 50_000);
            drain_kbc();

            // 6. Configurar tasa de muestreo a 100 muestras/segundo (0xF3 100)
            send_mouse_cmd_with_arg_wait(0xF3, 100, 50_000);
            drain_kbc();

            // 7. Configurar resolución a 4 cuentas/mm (0xE8 0x02)
            send_mouse_cmd_with_arg_wait(0xE8, 0x02, 50_000);
            drain_kbc();

            // 8. Habilitar modo de flujo de datos continuo (0xF4) — DEBE SER EL ÚLTIMO
            if !send_mouse_cmd_wait(0xF4, 50_000) {
                serial::log("MS", "aviso: fallo 0xF4, usando fallback de lectura");
            }
            drain_kbc();
        }

        self.pkt_idx = 0;
        self.error_count = 0;
        self.resets = 0;
        self.present = true;
        serial::log("MS", "Controlador de raton PS/2 inicializado correctamente");
        true
    }

    unsafe fn wait_obf_read_any(limit: u32) -> u8 {
        for _ in 0..limit {
            let st = inb(PS2_STATUS);
            if st & 0x01 != 0 {
                return inb(PS2_DATA);
            }
            io_wait();
        }
        0xFF
    }

    pub fn intelligent_reset(&mut self) {
        let now = pit::ticks();
        if now.saturating_sub(self.last_reset_tick) < 100 { return; }
        unsafe { drain_kbc(); }
        self.resets = self.resets.saturating_add(1);
        self.last_reset_tick = now;
        self.error_count = 0;
        self.pkt_idx = 0;
    }

    fn process(&mut self) -> bool {
        let flags = self.pkt[0];

        // Verificación canónica de integridad del paquete
        if (flags & 0x08) == 0 || (flags & 0xC0) != 0 {
            return false;
        }

        let x_sign = (flags & 0x10) != 0;
        let y_sign = (flags & 0x20) != 0;
        let raw_x = self.pkt[1];
        let raw_y = self.pkt[2];

        // Decodificación de 9 bits en complemento a dos (estándar PS/2 de OSDev)
        let dx: i32 = if x_sign {
            (raw_x as i32) - 256
        } else {
            raw_x as i32
        };

        let dy: i32 = if y_sign {
            (raw_y as i32) - 256
        } else {
            raw_y as i32
        };

        self.buttons = flags & 0x07;

        // Clampeo suave anti-ruido (hasta 64 px por frame para respuesta óptima y rápida)
        let clamped_dx = dx.clamp(-64, 64);
        let clamped_dy = dy.clamp(-64, 64);

        let old_x = self.x;
        let old_y = self.y;

        // En PS/2 dy positivo es hacia arriba, en coordenadas de pantalla Y crece hacia abajo
        self.x = (self.x + clamped_dx).clamp(0, self.max_x);
        self.y = (self.y - clamped_dy).clamp(0, self.max_y);

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

pub static GLOBAL_MOUSE: crate::arch::spinlock::Spinlock<MouseState> = crate::arch::spinlock::Spinlock::new(MouseState::new());

pub fn poll_global_mouse() {
    let mut guard = GLOBAL_MOUSE.lock();
    guard.begin_frame();
    while let Some(b) = crate::arch::isr_handlers::pop_mouse_byte() {
        guard.feed(b);
    }
    unsafe {
        while let Some(b) = poll_aux() {
            guard.feed(b);
        }
    }
}
