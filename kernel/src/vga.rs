// kernel/src/vga.rs - VERSIÓN PROFESIONAL MEJORADA
#![allow(dead_code)]

use core::fmt;

// ============================================
// CONSTANTES
// ============================================
const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;
const VGA_BUFFER_SIZE: usize = VGA_WIDTH * VGA_HEIGHT;
const VGA_ADDRESS: usize = 0xB8000;

// ============================================
// COLORES VGA
// ============================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

impl Color {
    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

// ============================================
// COLOR CODE
// ============================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorCode(u8);

impl ColorCode {
    pub const fn new(foreground: Color, background: Color) -> Self {
        Self((background.as_byte() << 4) | foreground.as_byte())
    }

    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    pub const fn as_byte(self) -> u8 {
        self.0
    }
}

// ============================================
// CARÁCTER DE PANTALLA
// ============================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_char: u8,
    color_code: u8,
}

impl ScreenChar {
    const fn new(ascii_char: u8, color: u8) -> Self {
        Self { ascii_char, color_code: color }
    }

    const fn blank(color: u8) -> Self {
        Self::new(b' ', color)
    }
}

// ============================================
// DRIVER VGA PRINCIPAL
// ============================================
pub struct Vga {
    buffer: *mut ScreenChar,
    col: usize,
    row: usize,
    color: u8,
}

impl Vga {
    /// Crea una nueva instancia del driver VGA
    pub const fn new() -> Self {
        Self {
            buffer: VGA_ADDRESS as *mut ScreenChar,
            col: 0,
            row: 0,
            color: 0x0F, // Blanco sobre negro por defecto
        }
    }

    // ========================================
    // MÉTODOS DE ESCRITURA SEGUROS
    // ========================================

    /// Escritura segura que preserva ES (crítico para evitar excepciones)
    #[inline(always)]
    unsafe fn write_safe(&self, offset: usize, char: ScreenChar) {
        if offset >= VGA_BUFFER_SIZE {
            return;
        }

        // Guardar ES
        let es: u16;
        core::arch::asm!(
            "mov {0:x}, es",
            out(reg) es,
            options(nomem, nostack, preserves_flags)
        );
        
        // Escribir
        self.buffer.add(offset).write_volatile(char);
        
        // Restaurar ES
        core::arch::asm!(
            "mov es, {0:x}",
            in(reg) es,
            options(nomem, nostack, preserves_flags)
        );
    }

    /// Lectura segura que preserva ES
    #[inline(always)]
    unsafe fn read_safe(&self, offset: usize) -> ScreenChar {
        if offset >= VGA_BUFFER_SIZE {
            return ScreenChar::blank(0x00);
        }

        let es: u16;
        core::arch::asm!(
            "mov {0:x}, es",
            out(reg) es,
            options(nomem, nostack, preserves_flags)
        );
        
        let char = self.buffer.add(offset).read_volatile();
        
        core::arch::asm!(
            "mov es, {0:x}",
            in(reg) es,
            options(nomem, nostack, preserves_flags)
        );

        char
    }

    // ========================================
    // LIMPIEZA Y GESTIÓN DE PANTALLA
    // ========================================

    /// Limpia toda la pantalla con un color
    pub fn clear(&mut self, color: u8) {
        let blank = ScreenChar::blank(color);
        
        for i in 0..VGA_BUFFER_SIZE {
            unsafe {
                self.write_safe(i, blank);
            }
        }
        
        self.col = 0;
        self.row = 0;
        self.color = color;
    }

    /// Limpia una línea específica
    pub fn clear_line(&self, row: usize, color: u8) {
        if row >= VGA_HEIGHT {
            return;
        }

        let blank = ScreenChar::blank(color);

        for col in 0..VGA_WIDTH {
            let pos = row * VGA_WIDTH + col;
            unsafe {
                self.write_safe(pos, blank);
            }
        }
    }

    /// Limpia un área rectangular
    pub fn clear_area(&self, start_row: usize, start_col: usize, width: usize, height: usize, color: u8) {
        let blank = ScreenChar::blank(color);
        
        for row in start_row..(start_row + height).min(VGA_HEIGHT) {
            for col in start_col..(start_col + width).min(VGA_WIDTH) {
                let pos = row * VGA_WIDTH + col;
                unsafe {
                    self.write_safe(pos, blank);
                }
            }
        }
    }

    // ========================================
    // ESCRITURA DE TEXTO
    // ========================================

    /// Establece el color actual
    pub fn set_color(&mut self, color: u8) {
        self.color = color;
    }

    /// Escribe un string en la posición actual del cursor
    pub fn write(&mut self, s: &str, color: u8) {
        for byte in s.bytes() {
            self.write_byte(byte, color);
        }
    }

    /// Escribe usando el color actual
    pub fn print(&mut self, s: &str) {
        self.write(s, self.color);
    }

    /// Escribe un byte manejando caracteres especiales
    fn write_byte(&mut self, byte: u8, color: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\r' => self.col = 0,
            b'\t' => {
                let spaces = 4 - (self.col % 4);
                for _ in 0..spaces {
                    self.write_byte(b' ', color);
                }
            }
            0x20..=0x7E | 0x80..=0xFF => {
                if self.col >= VGA_WIDTH {
                    self.new_line();
                }

                let pos = self.row * VGA_WIDTH + self.col;
                let screen_char = ScreenChar::new(byte, color);

                unsafe {
                    self.write_safe(pos, screen_char);
                }

                self.col += 1;
            }
            _ => self.write_byte(b'?', color), // Caracteres no imprimibles
        }
    }

    /// Escribe en una posición específica (no mueve el cursor)
    pub fn write_at(&self, s: &str, row: usize, col: usize, color: u8) {
        if row >= VGA_HEIGHT || col >= VGA_WIDTH {
            return;
        }
        
        let start_pos = row * VGA_WIDTH + col;
        
        for (i, byte) in s.bytes().enumerate() {
            if (col + i) >= VGA_WIDTH {
                break;
            }
            
            let pos = start_pos + i;
            if pos >= VGA_BUFFER_SIZE {
                break;
            }
            
            let screen_char = ScreenChar::new(byte, color);
            unsafe {
                self.write_safe(pos, screen_char);
            }
        }
    }

    /// Escribe un carácter individual
    pub fn put_char(&self, row: usize, col: usize, ch: u8, color: u8) {
        if row < VGA_HEIGHT && col < VGA_WIDTH {
            let pos = row * VGA_WIDTH + col;
            let screen_char = ScreenChar::new(ch, color);
            
            unsafe {
                self.write_safe(pos, screen_char);
            }
        }
    }

    // ========================================
    // GESTIÓN DE CURSOR
    // ========================================

    fn new_line(&mut self) {
        self.col = 0;
        if self.row < VGA_HEIGHT - 1 {
            self.row += 1;
        } else {
            self.scroll();
        }
    }

    fn scroll(&mut self) {
        unsafe {
            for row in 1..VGA_HEIGHT {
                for col in 0..VGA_WIDTH {
                    let src = row * VGA_WIDTH + col;
                    let dst = (row - 1) * VGA_WIDTH + col;
                    let char = self.read_safe(src);
                    self.write_safe(dst, char);
                }
            }
        }

        // Limpiar última línea
        self.clear_line(VGA_HEIGHT - 1, self.color);
    }

    /// Establece la posición del cursor
    pub fn set_position(&mut self, row: usize, col: usize) {
        if row < VGA_HEIGHT && col < VGA_WIDTH {
            self.row = row;
            self.col = col;
        }
    }

    /// Obtiene la posición actual
    pub fn get_position(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    // ========================================
    // GRÁFICOS Y CAJAS
    // ========================================

    /// Dibuja un rectángulo con bordes
    pub fn draw_box(&self, start_row: usize, start_col: usize, width: usize, height: usize, color: u8) {
        if start_row >= VGA_HEIGHT || start_col >= VGA_WIDTH || width < 2 || height < 2 {
            return;
        }

        let end_row = (start_row + height).min(VGA_HEIGHT);
        let end_col = (start_col + width).min(VGA_WIDTH);

        // Esquinas
        self.put_char(start_row, start_col, 0xDA, color);
        self.put_char(start_row, end_col - 1, 0xBF, color);
        self.put_char(end_row - 1, start_col, 0xC0, color);
        self.put_char(end_row - 1, end_col - 1, 0xD9, color);

        // Líneas horizontales
        for col in (start_col + 1)..(end_col - 1) {
            self.put_char(start_row, col, 0xC4, color);
            self.put_char(end_row - 1, col, 0xC4, color);
        }

        // Líneas verticales
        for row in (start_row + 1)..(end_row - 1) {
            self.put_char(row, start_col, 0xB3, color);
            self.put_char(row, end_col - 1, 0xB3, color);
        }
    }

    /// Dibuja una línea horizontal
    pub fn draw_hline(&self, row: usize, start_col: usize, length: usize, color: u8) {
        for i in 0..length {
            let col = start_col + i;
            if col >= VGA_WIDTH {
                break;
            }
            self.put_char(row, col, 0xC4, color);
        }
    }

    /// Dibuja una línea vertical
    pub fn draw_vline(&self, start_row: usize, col: usize, length: usize, color: u8) {
        for i in 0..length {
            let row = start_row + i;
            if row >= VGA_HEIGHT {
                break;
            }
            self.put_char(row, col, 0xB3, color);
        }
    }

    /// Rellena un rectángulo
    pub fn fill_rect(&self, start_row: usize, start_col: usize, width: usize, height: usize, ch: u8, color: u8) {
        for row in start_row..(start_row + height).min(VGA_HEIGHT) {
            for col in start_col..(start_col + width).min(VGA_WIDTH) {
                self.put_char(row, col, ch, color);
            }
        }
    }

    // ========================================
    // CURSOR DE HARDWARE
    // ========================================

    /// Habilita el cursor de hardware
    pub fn enable_hardware_cursor(&self, start: u8, end: u8) {
        unsafe {
            // Cursor start
            core::arch::asm!(
                "mov dx, 0x3D4",
                "mov al, 0x0A",
                "out dx, al",
                "inc dx",
                "mov al, {start}",
                "out dx, al",
                start = in(reg_byte) start,
                out("dx") _,
                out("al") _,
                options(nomem, nostack, preserves_flags)
            );

            // Cursor end
            core::arch::asm!(
                "mov dx, 0x3D4",
                "mov al, 0x0B",
                "out dx, al",
                "inc dx",
                "mov al, {end}",
                "out dx, al",
                end = in(reg_byte) end,
                out("dx") _,
                out("al") _,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    /// Actualiza la posición del cursor de hardware
    pub fn update_hardware_cursor(&self) {
        let pos = (self.row * VGA_WIDTH + self.col) as u16;
        
        unsafe {
            // Low byte
            core::arch::asm!(
                "mov dx, 0x3D4",
                "mov al, 0x0F",
                "out dx, al",
                "inc dx",
                "mov al, {low}",
                "out dx, al",
                low = in(reg_byte) (pos & 0xFF) as u8,
                out("dx") _,
                out("al") _,
                options(nomem, nostack, preserves_flags)
            );

            // High byte
            core::arch::asm!(
                "mov dx, 0x3D4",
                "mov al, 0x0E",
                "out dx, al",
                "inc dx",
                "mov al, {high}",
                "out dx, al",
                high = in(reg_byte) ((pos >> 8) & 0xFF) as u8,
                out("dx") _,
                out("al") _,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    /// Deshabilita el cursor de hardware
    pub fn disable_hardware_cursor(&self) {
        unsafe {
            core::arch::asm!(
                "mov dx, 0x3D4",
                "mov al, 0x0A",
                "out dx, al",
                "inc dx",
                "mov al, 0x20",
                "out dx, al",
                out("dx") _,
                out("al") _,
                options(nomem, nostack, preserves_flags)
            );
        }
    }
}

// ============================================
// IMPLEMENTACIÓN DE WRITE TRAIT (OPCIONAL)
// ============================================
impl fmt::Write for Vga {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.print(s);
        Ok(())
    }
}

unsafe impl Send for Vga {}
unsafe impl Sync for Vga {}

// ============================================
// FUNCIONES DE UTILIDAD
// ============================================

/// Convierte un Color a ColorCode con fondo negro
pub const fn color_code(fg: Color) -> u8 {
    ColorCode::new(fg, Color::Black).as_byte()
}

/// Crea un ColorCode personalizado
pub const fn color_pair(fg: Color, bg: Color) -> u8 {
    ColorCode::new(fg, bg).as_byte()
}