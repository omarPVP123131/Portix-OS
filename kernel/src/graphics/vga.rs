// kernel/src/graphics/vga.rs
//
// Driver VGA Modo Texto (80x25) - Portix Kernel
// -----------------------------------------------
// Este módulo actúa como RESPALDO cuando el framebuffer VESA no está disponible
// o cuando ocurre un fallo crítico antes de que el doble buffer esté listo.
// No depende de ningún otro módulo del kernel excepto de tipos primitivos.
//
// Uso:
//   let mut vga = VgaWriter::new();
//   vga.clear();
//   vga.write_str("Hola desde VGA!", VgaColor::new(VgaColorCode::White, VgaColorCode::Blue));

use core::fmt;

// ─────────────────────────────────────────────
//  Constantes del hardware VGA modo texto
// ─────────────────────────────────────────────

const VGA_BUFFER_ADDR: usize = 0xB8000;
const VGA_WIDTH:  usize = 80;
const VGA_HEIGHT: usize = 25;

// ─────────────────────────────────────────────
//  Colores VGA (4-bit palette estándar)
// ─────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VgaColorCode {
    Black        = 0,
    Blue         = 1,
    Green        = 2,
    Cyan         = 3,
    Red          = 4,
    Magenta      = 5,
    Brown        = 6,
    LightGray    = 7,
    DarkGray     = 8,
    LightBlue    = 9,
    LightGreen   = 10,
    LightCyan    = 11,
    LightRed     = 12,
    Pink         = 13,
    Yellow       = 14,
    White        = 15,
}

/// Un color VGA empaquetado: nibble alto = fondo, nibble bajo = frente
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct VgaColor(u8);

impl VgaColor {
    #[inline]
    pub const fn new(fg: VgaColorCode, bg: VgaColorCode) -> Self {
        Self((bg as u8) << 4 | (fg as u8))
    }

    /// Color por defecto del kernel: texto blanco sobre fondo azul oscuro
    pub const DEFAULT: Self = Self::new(VgaColorCode::White, VgaColorCode::Blue);
    /// Color de error: texto blanco sobre rojo
    pub const PANIC:   Self = Self::new(VgaColorCode::White, VgaColorCode::Red);
    /// Color de advertencia
    pub const WARN:    Self = Self::new(VgaColorCode::Yellow, VgaColorCode::Black);
    /// Color OK / éxito
    pub const OK:      Self = Self::new(VgaColorCode::LightGreen, VgaColorCode::Black);
}

// ─────────────────────────────────────────────
//  Celda VGA (carácter + atributo de color)
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct VgaCell {
    ascii: u8,
    color: VgaColor,
}

impl VgaCell {
    const fn blank(color: VgaColor) -> Self {
        Self { ascii: b' ', color }
    }
}

// ─────────────────────────────────────────────
//  Buffer VGA (mapeado directamente en memoria)
// ─────────────────────────────────────────────

/// Buffer VGA mapeado en 0xB8000. Volatile write para evitar que el
/// compilador optimice las escrituras como "muertas".
struct VgaBuffer {
    base: *mut VgaCell,
}

impl VgaBuffer {
    /// # Safety: Sólo debe construirse una instancia apuntando a 0xB8000.
    unsafe fn new() -> Self {
        Self { base: VGA_BUFFER_ADDR as *mut VgaCell }
    }

    #[inline]
    fn write(&mut self, row: usize, col: usize, cell: VgaCell) {
        debug_assert!(row < VGA_HEIGHT && col < VGA_WIDTH);
        unsafe {
            // Escritura volátil: garantiza que el HW VGA siempre vea el cambio
            core::ptr::write_volatile(
                self.base.add(row * VGA_WIDTH + col),
                cell,
            );
        }
    }

    #[inline]
    fn read(&self, row: usize, col: usize) -> VgaCell {
        unsafe {
            core::ptr::read_volatile(self.base.add(row * VGA_WIDTH + col))
        }
    }
}

// ─────────────────────────────────────────────
//  Cursor por software (VGA modo texto lo
//  soporta por hardware, pero lo mantenemos
//  como estado interno para scroll limpio)
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct Cursor {
    row: usize,
    col: usize,
}

impl Cursor {
    const fn origin() -> Self { Self { row: 0, col: 0 } }
}

// ─────────────────────────────────────────────
//  VgaWriter — API pública
// ─────────────────────────────────────────────

/// Escritor de texto sobre el buffer VGA 80×25.
/// Instanciar con `VgaWriter::new()`. Es seguro crear múltiples instancias
/// en contextos de pánico/ISR porque cada una escribe directamente en el HW.
pub struct VgaWriter {
    buffer:  VgaBuffer,
    cursor:  Cursor,
    color:   VgaColor,
}

impl VgaWriter {
    /// Crea un escritor VGA con el color por defecto del kernel.
    pub fn new() -> Self {
        Self {
            buffer: unsafe { VgaBuffer::new() },
            cursor: Cursor::origin(),
            color:  VgaColor::DEFAULT,
        }
    }

    /// Crea un escritor VGA con un color personalizado (útil en ISRs de pánico).
    pub fn with_color(color: VgaColor) -> Self {
        let mut w = Self::new();
        w.color = color;
        w
    }

    // ── Color ──────────────────────────────────

    /// Cambia el color activo para las siguientes escrituras.
    #[inline]
    pub fn set_color(&mut self, color: VgaColor) {
        self.color = color;
    }

    // ── Limpieza ───────────────────────────────

    /// Limpia toda la pantalla con el color activo.
    pub fn clear(&mut self) {
        let blank = VgaCell::blank(self.color);
        for row in 0..VGA_HEIGHT {
            for col in 0..VGA_WIDTH {
                self.buffer.write(row, col, blank);
            }
        }
        self.cursor = Cursor::origin();
    }

    /// Limpia la pantalla con un color específico sin cambiar `self.color`.
    pub fn clear_with(&mut self, color: VgaColor) {
        let prev = self.color;
        self.color = color;
        self.clear();
        self.color = prev;
    }

    // ── Posición ───────────────────────────────

    /// Mueve el cursor a una posición (col, row) arbitraria.
    pub fn set_position(&mut self, col: usize, row: usize) {
        self.cursor.col = col.min(VGA_WIDTH  - 1);
        self.cursor.row = row.min(VGA_HEIGHT - 1);
    }

    // ── Escritura básica ───────────────────────

    /// Escribe un único byte ASCII en la posición actual con el color activo.
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.cursor.col = 0,
            byte  => {
                if self.cursor.col >= VGA_WIDTH {
                    self.newline();
                }
                self.buffer.write(
                    self.cursor.row,
                    self.cursor.col,
                    VgaCell { ascii: byte, color: self.color },
                );
                self.cursor.col += 1;
            }
        }
    }

    /// Escribe una cadena en la posición actual con el color activo.
    pub fn write_str(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // Carácter ASCII imprimible o salto de línea/retorno
                0x20..=0x7E | b'\n' | b'\r' => self.write_byte(byte),
                // Carácter no representable → placeholder
                _ => self.write_byte(0xFE),
            }
        }
    }

    /// Escribe una cadena en una posición absoluta (col, row) con color propio.
    /// No modifica el cursor ni el color activo del escritor.
    pub fn write_at(&mut self, s: &str, col: usize, row: usize, color: VgaColor) {
        let prev_cursor = self.cursor;
        let prev_color  = self.color;

        self.cursor.col = col.min(VGA_WIDTH  - 1);
        self.cursor.row = row.min(VGA_HEIGHT - 1);
        self.color      = color;

        self.write_str(s);

        self.cursor = prev_cursor;
        self.color  = prev_color;
    }

    // ── Utilidades de layout ───────────────────

    /// Dibuja una línea horizontal de un carácter dado (por defecto '─' ASCII '-')
    pub fn draw_hline(&mut self, row: usize, fill: u8) {
        for col in 0..VGA_WIDTH {
            self.buffer.write(row, col, VgaCell { ascii: fill, color: self.color });
        }
    }

    /// Rellena una región rectangular con un carácter y color dados.
    pub fn fill_rect(
        &mut self,
        col: usize, row: usize,
        width: usize, height: usize,
        fill: u8, color: VgaColor,
    ) {
        for r in row..(row + height).min(VGA_HEIGHT) {
            for c in col..(col + width).min(VGA_WIDTH) {
                self.buffer.write(r, c, VgaCell { ascii: fill, color });
            }
        }
    }

    // ── Helpers de estado del sistema ──────────

    /// Escribe una línea con prefijo coloreado estilo syslog:
    /// `[ OK ]`, `[WARN]`, `[FAIL]`, `[INFO]`
    pub fn write_status(&mut self, level: StatusLevel, msg: &str) {
        let (tag, tag_color, msg_color) = match level {
            StatusLevel::Ok   => ("[ OK ] ", VgaColor::OK,      VgaColor::DEFAULT),
            StatusLevel::Warn => ("[WARN] ", VgaColor::WARN,     VgaColor::DEFAULT),
            StatusLevel::Fail => ("[FAIL] ", VgaColor::PANIC,    VgaColor::DEFAULT),
            StatusLevel::Info => ("[INFO] ", VgaColor::new(VgaColorCode::Cyan, VgaColorCode::Black), VgaColor::DEFAULT),
        };

        let saved = self.color;

        self.color = tag_color;
        self.write_str(tag);

        self.color = msg_color;
        self.write_str(msg);
        self.write_byte(b'\n');

        self.color = saved;
    }

    // ── Scroll ─────────────────────────────────

    fn newline(&mut self) {
        if self.cursor.row + 1 >= VGA_HEIGHT {
            self.scroll_up();
        } else {
            self.cursor.row += 1;
        }
        self.cursor.col = 0;
    }

    fn scroll_up(&mut self) {
        // Copia cada fila hacia la fila anterior
        for row in 1..VGA_HEIGHT {
            for col in 0..VGA_WIDTH {
                let cell = self.buffer.read(row, col);
                self.buffer.write(row - 1, col, cell);
            }
        }
        // Limpia la última fila
        let blank = VgaCell::blank(self.color);
        for col in 0..VGA_WIDTH {
            self.buffer.write(VGA_HEIGHT - 1, col, blank);
        }
        // El cursor ya está en la última fila
        self.cursor.row = VGA_HEIGHT - 1;
    }
}

// ─────────────────────────────────────────────
//  Implementación de core::fmt::Write
//  → permite usar write!() / writeln!() con VgaWriter
// ─────────────────────────────────────────────

impl fmt::Write for VgaWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Self::write_str(self, s);
        Ok(())
    }
}

// ─────────────────────────────────────────────
//  StatusLevel — niveles de log
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Ok,
    Warn,
    Fail,
    Info,
}

// ─────────────────────────────────────────────
//  Macros de conveniencia (opcionales)
//  Requieren que exista un `VgaWriter` accesible
//  como `$writer` en el ámbito de la llamada.
// ─────────────────────────────────────────────

/// Escribe con formato en un `VgaWriter`:
///   `vga_print!(writer, "valor={}", x);`
#[macro_export]
macro_rules! vga_print {
    ($writer:expr, $($arg:tt)*) => {
        { use core::fmt::Write; let _ = write!($writer, $($arg)*); }
    };
}

/// Escribe con formato + newline en un `VgaWriter`.
#[macro_export]
macro_rules! vga_println {
    ($writer:expr, $($arg:tt)*) => {
        { use core::fmt::Write; let _ = writeln!($writer, $($arg)*); }
    };
}