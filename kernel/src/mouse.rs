// kernel/src/mouse.rs — PORTIX PS/2 Mouse Driver v6.1 (Intelligent Edition)
//
// CAMBIOS:
//   - Añadido campo 'resets' para telemetría.
//   - Constructor 'new()' corregido y completo.
//   - Filtro de "Bit 3" + Timeout de ráfaga (2 ticks).
//   - Intelligent Reset: cura el driver sin colgar el kernel.
//   - Movimiento clamp y detección de saltos físicos imposibles.

#![allow(dead_code)]
use crate::pit;

// --- Puertos I/O del Controlador 8042 ---
const PS2_DATA:   u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_CMD:    u16 = 0x64;

// --- Parámetros del Filtro Inteligente ---
const TELEPORT_THRESHOLD: i32 = 120; // Píxeles máximos permitidos por paquete
const ERROR_LIMIT: u32 = 25;         // Errores acumulados antes de resetear hardware

// --- Utilidades de bajo nivel ---
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

// --- Estado del Ratón ---
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

    // Monitor de salud y telemetría
    pub error_count: u32,
    pub resets: u32,
    last_reset_tick: u64,
}

impl MouseState {
    /// Crea una nueva instancia del estado del ratón (necesaria para el kernel)
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

    /// Inicializa el hardware PS/2 en modo estándar (3 bytes)
    pub fn init(&mut self, sw: usize, sh: usize) -> bool {
        self.max_x = (sw as i32).saturating_sub(1);
        self.max_y = (sh as i32).saturating_sub(1);
        self.x = self.max_x / 2;
        self.y = self.max_y / 2;
        self.has_wheel = false; 

        unsafe {
            drain_kbc();
            wait_write(); outb(PS2_CMD, 0xA8); // Activar puerto auxiliar
            
            // Habilitar IRQ12 en el Command Byte
            wait_write(); outb(PS2_CMD, 0x20);
            if !wait_read() { return false; }
            let cfg = inb(PS2_DATA);
            wait_write(); outb(PS2_CMD, 0x60);
            wait_write(); outb(PS2_DATA, (cfg | 0x02) & !0x20);

            mouse_cmd(0xF6); // Set Defaults
            mouse_cmd_arg(0xF3, 100); // Sample Rate 100Hz
            if !mouse_cmd(0xF4) { return false; } // Enable Streaming

            drain_kbc();
            self.present = true;
            true
        }
    }

    /// Limpia el controlador y re-habilita el ratón tras un desync grave
    fn intelligent_reset(&mut self) {
        let now = pit::ticks();
        // Protección contra bucles de reset (máximo 1 cada segundo)
        if now.saturating_sub(self.last_reset_tick) < 100 { return; }

        unsafe {
            drain_kbc();
            mouse_cmd(0xF6); // Reset a defaults
            mouse_cmd(0xF4); // Re-habilitar
            drain_kbc();
        }

        self.resets += 1;
        self.error_count = 0;
        self.pkt_idx = 0;
        self.last_reset_tick = now;
    }

    /// Función principal de lectura de datos
    pub fn poll(&mut self) -> bool {
        self.scroll_delta = 0;
        self.prev_buttons = self.buttons;
        let mut changed = false;
        let current_tick = pit::ticks();

        unsafe {
            loop {
                let st = inb(PS2_STATUS);
                if st & 0x01 == 0 { break; } // No hay más datos
                if st & 0x20 == 0 { // Los datos son del teclado, no del mouse
                    // Podríamos redirigirlos al driver de teclado aquí si fuera necesario
                    break; 
                }
                
                let byte = inb(PS2_DATA);

                // --- Mecanismo de Timeout ---
                // Si el último byte fue hace mucho, el paquete actual está roto.
                if self.pkt_idx > 0 && current_tick.saturating_sub(self.last_tick) > 2 {
                    self.pkt_idx = 0;
                    self.error_count += 1;
                }
                self.last_tick = current_tick;

                if self.feed(byte) { changed = true; }
            }
        }

        // Si hay demasiados errores acumulados, resetear hardware
        if self.error_count >= ERROR_LIMIT {
            self.intelligent_reset();
        }

        changed
    }

    /// Alimentador del buffer de paquetes
    fn feed(&mut self, byte: u8) -> bool {
        match self.pkt_idx {
            0 => {
                // Validación del Bit 3: El byte 0 de un paquete PS/2 SIEMPRE tiene el bit 3 en 1.
                if (byte & 0x08) == 0 {
                    self.error_count += 1;
                    return false; 
                }
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
                
                if self.process() {
                    // Paquete válido: reducimos el contador de sospecha lentamente
                    if self.error_count > 0 { self.error_count -= 1; }
                    true
                } else {
                    false
                }
            }
            _ => { self.pkt_idx = 0; false }
        }
    }

    /// Procesa el paquete final y aplica el movimiento
    fn process(&mut self) -> bool {
        let flags = self.pkt[0];
        let mut dx = self.pkt[1] as i32;
        let mut dy = self.pkt[2] as i32;

        // Signos (Bits 4 y 5)
        if flags & 0x10 != 0 { dx -= 256; }
        if flags & 0x20 != 0 { dy -= 256; }

        // --- Filtro de Cordura (Teletransporte) ---
        if dx.abs() > TELEPORT_THRESHOLD || dy.abs() > TELEPORT_THRESHOLD {
            self.error_count += 5; // Gran penalización por saltos imposibles
            return false;
        }

        // --- Detección de Esquinas Pegajosas ---
        // Si el mouse empuja contra los bordes con valores muy altos, es síntoma de desync.
        if (self.x >= self.max_x && dx > 60) || (self.x <= 0 && dx < -60) {
            self.error_count += 1;
        }

        let old_x = self.x;
        let old_y = self.y;

        self.buttons = flags & 0x07;
        
        // Aplicar movimiento y clamp a los límites de la pantalla
        // Nota: dy se resta porque en PS/2 el eje Y es positivo hacia arriba.
        self.x = (self.x + dx).clamp(0, self.max_x);
        self.y = (self.y - dy).clamp(0, self.max_y);

        self.x != old_x || self.y != old_y || self.buttons != self.prev_buttons
    }

    // --- Helpers de Estado para el Kernel ---
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