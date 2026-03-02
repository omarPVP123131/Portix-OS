// ui/input.rs — PORTIX InputBox widget v1.1
//
// Widget de entrada de texto inline. SOLO UI — sin lógica de paths ni FAT32.
// Usado por:
//   IDE      → Guardar como, Ir a línea
//   Explorer → Nueva carpeta, Nuevo archivo, Eliminar
//
// El rendering se hace en draw_input_overlay() para reutilizarlo en
// cualquier status bar sin duplicar código.

#![allow(dead_code)]

use crate::drivers::input::keyboard::Key;
use crate::graphics::driver::framebuffer::{Color, Console};

pub const INPUT_MAX: usize = 128;

// ─────────────────────────────────────────────────────────────────────────────
// InputMode — qué operación está activa
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    None,
    SaveAs,    // IDE: Guardar como / renombrar
    GoToLine,  // IDE: Ir a línea (número)
    NewDir,    // Explorer: Nueva carpeta
    NewFile,   // Explorer: Nuevo archivo
    Delete,    // Explorer: Confirmar eliminación
}

impl InputMode {
    /// Texto del prompt que se muestra delante del cursor.
    pub fn prompt(self) -> &'static str {
        match self {
            InputMode::SaveAs   => "Nombre: ",
            InputMode::GoToLine => "Ir a línea: ",
            InputMode::NewDir   => "Nueva carpeta: ",
            InputMode::NewFile  => "Nuevo archivo: ",
            InputMode::Delete   => "Eliminar (Enter=confirmar): ",
            InputMode::None     => "",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// InputBox — estado del cuadro de texto
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct InputBox {
    pub buf:    [u8; INPUT_MAX],
    pub len:    usize,
    pub mode:   InputMode,
    pub cursor: usize,
}

impl InputBox {
    pub const fn new() -> Self {
        InputBox { buf: [0u8; INPUT_MAX], len: 0, mode: InputMode::None, cursor: 0 }
    }

    pub fn start(&mut self, mode: InputMode, prefill: &str) {
        self.mode   = mode;
        self.len    = prefill.len().min(INPUT_MAX);
        self.buf[..self.len].copy_from_slice(&prefill.as_bytes()[..self.len]);
        self.cursor = self.len;
    }

    pub fn close(&mut self) {
        self.mode   = InputMode::None;
        self.len    = 0;
        self.cursor = 0;
        // limpiar buffer por seguridad
        self.buf    = [0u8; INPUT_MAX];
    }

    pub fn is_active(&self) -> bool { self.mode != InputMode::None }

    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    // ── Procesado de teclas ───────────────────────────────────────────────────
    //
    // FIX al borrow error en explorer.rs:
    // El caller NO debe leer self.input.mode en la misma expresión donde
    // llama a self.input.feed().  Patrón correcto:
    //
    //   if self.input.is_active() {           // is_active() = bool, no borrow &mode
    //       if let Some(ok) = self.input.feed(key) {
    //           let mode = self.input.mode;   // ya terminó feed(), borrow libre
    //           self.input.close();
    //           // usar mode aquí
    //       }
    //   }
    //
    // Devuelve:
    //   Some(true)  → confirmado con Enter
    //   Some(false) → cancelado con Escape (box ya cerrado)
    //   None        → seguir editando
    pub fn feed(&mut self, key: Key) -> Option<bool> {
        match key {
            Key::Enter   => Some(true),
            Key::Escape  => { self.close(); Some(false) }
            Key::Backspace => {
                if self.cursor > 0 {
                    let c = self.cursor - 1;
                    self.buf.copy_within(c + 1..self.len, c);
                    self.len    -= 1;
                    self.cursor -= 1;
                }
                None
            }
            Key::Delete => {
                if self.cursor < self.len {
                    self.buf.copy_within(self.cursor + 1..self.len, self.cursor);
                    self.len -= 1;
                }
                None
            }
            Key::Left  => { if self.cursor > 0        { self.cursor -= 1; } None }
            Key::Right => { if self.cursor < self.len { self.cursor += 1; } None }
            Key::Home  => { self.cursor = 0;            None }
            Key::End   => { self.cursor = self.len;     None }
            Key::Char(c) if c >= 0x20 && c < 0x7F && self.len < INPUT_MAX => {
                let pos = self.cursor;
                self.buf.copy_within(pos..self.len, pos + 1);
                self.buf[pos] = c;
                self.len    += 1;
                self.cursor += 1;
                None
            }
            _ => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_input_overlay — renderiza el InputBox sobre la status bar
//
// Llámalo DESPUÉS de haber rellenado el fondo de la status bar.
// Funciona para el IDE y el Explorer sin duplicar código.
// ─────────────────────────────────────────────────────────────────────────────

pub const INPUT_BG:         Color = Color::new(0x1A, 0x38, 0x70);
pub const INPUT_BG_DELETE:  Color = Color::new(0x60, 0x10, 0x10);
pub const INPUT_PROMPT_FG:  Color = Color::new(0xFF, 0xD7, 0x00);
pub const INPUT_TEXT_FG:    Color = Color::WHITE;
pub const INPUT_CURSOR_FG:  Color = Color::new(0x1A, 0x38, 0x70);  // texto sobre cursor
pub const INPUT_HINT_FG:    Color = Color::new(0x80, 0xA0, 0xD0);

/// Dibuja el input overlay dentro de la status bar `(x0, sy, fw, bar_h)`.
/// El fondo debe estar ya pintado con el color adecuado.
pub fn draw_input_overlay(
    c:     &mut Console,
    input: &InputBox,
    x0:    usize,  // margen izquierdo (normalmente 8)
    sy:    usize,  // Y de la barra de status
    fw:    usize,  // ancho total del área
    bar_h: usize,  // alto de la barra
    cw:    usize,  // font_w
    ch:    usize,  // font_h
) {
    if !input.is_active() { return; }

    let ty     = sy + (bar_h.saturating_sub(ch)) / 2;
    let prompt = input.mode.prompt();
    let bg     = if input.mode == InputMode::Delete { INPUT_BG_DELETE } else { INPUT_BG };

    // Prompt
    c.write_at(prompt, x0, ty, INPUT_PROMPT_FG);
    let px   = x0 + prompt.len() * cw;

    // Texto ingresado
    let text = input.text();
    c.write_at(text, px, ty, INPUT_TEXT_FG);

    // Cursor de bloque (visible, sin parpadeo en bare-metal)
    let cx = px + input.cursor * cw;
    if cx + cw <= fw {
        c.fill_rect(cx, ty.saturating_sub(1), cw, ch + 2, Color::WHITE);
        let cur = if input.cursor < input.len { input.buf[input.cursor] } else { b' ' };
        let s   = [cur];
        if let Ok(cs) = core::str::from_utf8(&s) {
            c.write_at(cs, cx, ty, bg);
        }
    }

    // Hint de teclas (extremo derecho)
    let hint = "Enter=OK  Esc=Cancelar";
    let hx   = fw.saturating_sub(hint.len() * cw + 6);
    c.write_at(hint, hx, ty, INPUT_HINT_FG);
}