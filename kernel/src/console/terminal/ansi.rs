// kernel/src/console/terminal/ansi.rs — Emulador y Parser ANSI / VT100
// Soporta colores estándar, alta intensidad, 256 colores y TrueColor RGB (24-bit),
// posicionamiento de cursor, borrado de pantalla y formateo enriquecido.
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

/// Color RGB de 32 bits empaquetado (0x00RRGGBB)
pub type AnsiColor = u32;

pub const COLOR_BLACK:   AnsiColor = 0x001E1E2E;
pub const COLOR_RED:     AnsiColor = 0x00F38BA8;
pub const COLOR_GREEN:   AnsiColor = 0x00A6E3A1;
pub const COLOR_YELLOW:  AnsiColor = 0x00F9E2AF;
pub const COLOR_BLUE:    AnsiColor = 0x0089B4FA;
pub const COLOR_MAGENTA: AnsiColor = 0x00CBA6F7;
pub const COLOR_CYAN:    AnsiColor = 0x0089DCEB;
pub const COLOR_WHITE:   AnsiColor = 0x00CDD6F4;

pub const COLOR_BRIGHT_BLACK:   AnsiColor = 0x00585B70;
pub const COLOR_BRIGHT_RED:     AnsiColor = 0x00FF6E6E;
pub const COLOR_BRIGHT_GREEN:   AnsiColor = 0x0069FF94;
pub const COLOR_BRIGHT_YELLOW:  AnsiColor = 0x00FFFFA5;
pub const COLOR_BRIGHT_BLUE:    AnsiColor = 0x00D6ACFF;
pub const COLOR_BRIGHT_MAGENTA: AnsiColor = 0x00FF92DF;
pub const COLOR_BRIGHT_CYAN:    AnsiColor = 0x00A4FFFF;
pub const COLOR_BRIGHT_WHITE:   AnsiColor = 0x00FFFFFF;

/// Tramo de texto con atributos visuales ANSI asociados
#[derive(Debug, Clone)]
pub struct AnsiSpan {
    pub text: String,
    pub fg: AnsiColor,
    pub bg: AnsiColor,
    pub bold: bool,
    pub underline: bool,
}

/// Estado actual del motor de emulación ANSI/VT100
#[derive(Debug, Clone)]
pub struct AnsiState {
    pub fg: AnsiColor,
    pub bg: AnsiColor,
    pub bold: bool,
    pub underline: bool,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub screen_cleared: bool,
}

impl AnsiState {
    pub fn new(default_fg: AnsiColor, default_bg: AnsiColor) -> Self {
        Self {
            fg: default_fg,
            bg: default_bg,
            bold: false,
            underline: false,
            cursor_row: 0,
            cursor_col: 0,
            screen_cleared: false,
        }
    }

    pub fn reset_attributes(&mut self, default_fg: AnsiColor, default_bg: AnsiColor) {
        self.fg = default_fg;
        self.bg = default_bg;
        self.bold = false;
        self.underline = false;
    }
}

/// Mapea un índice de la paleta estándar de 256 colores a un valor RGB de 24 bits
pub fn ansi_256_to_rgb(idx: u8) -> AnsiColor {
    match idx {
        0 => COLOR_BLACK,
        1 => COLOR_RED,
        2 => COLOR_GREEN,
        3 => COLOR_YELLOW,
        4 => COLOR_BLUE,
        5 => COLOR_MAGENTA,
        6 => COLOR_CYAN,
        7 => COLOR_WHITE,
        8 => COLOR_BRIGHT_BLACK,
        9 => COLOR_BRIGHT_RED,
        10 => COLOR_BRIGHT_GREEN,
        11 => COLOR_BRIGHT_YELLOW,
        12 => COLOR_BRIGHT_BLUE,
        13 => COLOR_BRIGHT_MAGENTA,
        14 => COLOR_BRIGHT_CYAN,
        15 => COLOR_BRIGHT_WHITE,
        16..=231 => {
            // Cubo de color 6x6x6
            let n = idx - 16;
            let b = (n % 6) * 51;
            let g = ((n / 6) % 6) * 51;
            let r = (n / 36) * 51;
            ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
        }
        232..=255 => {
            // Escala de grises (24 niveles)
            let gray = (idx - 232) * 10 + 8;
            ((gray as u32) << 16) | ((gray as u32) << 8) | (gray as u32)
        }
    }
}

/// Elimina todas las secuencias de escape ANSI devolviendo sólo texto plano imprimible
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Saltarse la secuencia CSI hasta un caracter terminal ('m', 'H', 'J', etc.)
            i += 2;
            while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // saltar el byte terminador
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Parsea un texto que contiene secuencias de escape ANSI CSI y genera una lista de spans coloreados
pub fn parse_ansi_spans(s: &str, default_fg: AnsiColor, default_bg: AnsiColor) -> Vec<AnsiSpan> {
    let mut spans = Vec::new();
    let mut state = AnsiState::new(default_fg, default_bg);
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut cur_text = String::new();

    while i < bytes.len() {
        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Volcar texto acumulado antes de procesar el escape
            if !cur_text.is_empty() {
                spans.push(AnsiSpan {
                    text: cur_text.clone(),
                    fg: state.fg,
                    bg: state.bg,
                    bold: state.bold,
                    underline: state.underline,
                });
                cur_text.clear();
            }

            i += 2; // saltar \x1b[
            let start_params = i;
            while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < bytes.len() {
                let cmd = bytes[i];
                let params_str = core::str::from_utf8(&bytes[start_params..i]).unwrap_or("");
                i += 1; // consumir terminador

                match cmd {
                    b'm' => {
                        // Select Graphic Rendition (SGR)
                        if params_str.is_empty() || params_str == "0" {
                            state.reset_attributes(default_fg, default_bg);
                        } else {
                            let parts: Vec<&str> = params_str.split(';').collect();
                            let mut p_idx = 0;
                            while p_idx < parts.len() {
                                let code: u32 = parts[p_idx].parse().unwrap_or(0);
                                match code {
                                    0 => state.reset_attributes(default_fg, default_bg),
                                    1 => state.bold = true,
                                    4 => state.underline = true,
                                    22 => state.bold = false,
                                    24 => state.underline = false,
                                    30 => state.fg = COLOR_BLACK,
                                    31 => state.fg = COLOR_RED,
                                    32 => state.fg = COLOR_GREEN,
                                    33 => state.fg = COLOR_YELLOW,
                                    34 => state.fg = COLOR_BLUE,
                                    35 => state.fg = COLOR_MAGENTA,
                                    36 => state.fg = COLOR_CYAN,
                                    37 => state.fg = COLOR_WHITE,
                                    39 => state.fg = default_fg,
                                    40 => state.bg = COLOR_BLACK,
                                    41 => state.bg = COLOR_RED,
                                    42 => state.bg = COLOR_GREEN,
                                    43 => state.bg = COLOR_YELLOW,
                                    44 => state.bg = COLOR_BLUE,
                                    45 => state.bg = COLOR_MAGENTA,
                                    46 => state.bg = COLOR_CYAN,
                                    47 => state.bg = COLOR_WHITE,
                                    49 => state.bg = default_bg,
                                    90 => state.fg = COLOR_BRIGHT_BLACK,
                                    91 => state.fg = COLOR_BRIGHT_RED,
                                    92 => state.fg = COLOR_BRIGHT_GREEN,
                                    93 => state.fg = COLOR_BRIGHT_YELLOW,
                                    94 => state.fg = COLOR_BRIGHT_BLUE,
                                    95 => state.fg = COLOR_BRIGHT_MAGENTA,
                                    96 => state.fg = COLOR_BRIGHT_CYAN,
                                    97 => state.fg = COLOR_BRIGHT_WHITE,
                                    100 => state.bg = COLOR_BRIGHT_BLACK,
                                    101 => state.bg = COLOR_BRIGHT_RED,
                                    102 => state.bg = COLOR_BRIGHT_GREEN,
                                    103 => state.bg = COLOR_BRIGHT_YELLOW,
                                    104 => state.bg = COLOR_BRIGHT_BLUE,
                                    105 => state.bg = COLOR_BRIGHT_MAGENTA,
                                    106 => state.bg = COLOR_BRIGHT_CYAN,
                                    107 => state.bg = COLOR_BRIGHT_WHITE,
                                    38 => {
                                        // 38;5;N o 38;2;R;G;B
                                        if p_idx + 2 < parts.len() && parts[p_idx + 1] == "5" {
                                            let color_idx: u8 = parts[p_idx + 2].parse().unwrap_or(0);
                                            state.fg = ansi_256_to_rgb(color_idx);
                                            p_idx += 2;
                                        } else if p_idx + 4 < parts.len() && parts[p_idx + 1] == "2" {
                                            let r: u32 = parts[p_idx + 2].parse().unwrap_or(0);
                                            let g: u32 = parts[p_idx + 3].parse().unwrap_or(0);
                                            let b: u32 = parts[p_idx + 4].parse().unwrap_or(0);
                                            state.fg = (r << 16) | (g << 8) | b;
                                            p_idx += 4;
                                        }
                                    }
                                    48 => {
                                        // 48;5;N o 48;2;R;G;B
                                        if p_idx + 2 < parts.len() && parts[p_idx + 1] == "5" {
                                            let color_idx: u8 = parts[p_idx + 2].parse().unwrap_or(0);
                                            state.bg = ansi_256_to_rgb(color_idx);
                                            p_idx += 2;
                                        } else if p_idx + 4 < parts.len() && parts[p_idx + 1] == "2" {
                                            let r: u32 = parts[p_idx + 2].parse().unwrap_or(0);
                                            let g: u32 = parts[p_idx + 3].parse().unwrap_or(0);
                                            let b: u32 = parts[p_idx + 4].parse().unwrap_or(0);
                                            state.bg = (r << 16) | (g << 8) | b;
                                            p_idx += 4;
                                        }
                                    }
                                    _ => {}
                                }
                                p_idx += 1;
                            }
                        }
                    }
                    b'H' | b'f' => {
                        // Posicionamiento de cursor: \x1b[row;colH
                        let parts: Vec<&str> = params_str.split(';').collect();
                        if parts.len() == 2 {
                            state.cursor_row = parts[0].parse::<usize>().unwrap_or(1).saturating_sub(1);
                            state.cursor_col = parts[1].parse::<usize>().unwrap_or(1).saturating_sub(1);
                        } else {
                            state.cursor_row = 0;
                            state.cursor_col = 0;
                        }
                    }
                    b'J' => {
                        // 2J = Borrado de pantalla
                        if params_str == "2" {
                            state.screen_cleared = true;
                        }
                    }
                    _ => {}
                }
            }
        } else {
            cur_text.push(bytes[i] as char);
            i += 1;
        }
    }

    if !cur_text.is_empty() {
        spans.push(AnsiSpan {
            text: cur_text,
            fg: state.fg,
            bg: state.bg,
            bold: state.bold,
            underline: state.underline,
        });
    }

    spans
}

/// Auto-test de validación para el motor de secuencias de escape ANSI/VT100
pub fn run_ansi_self_test() -> bool {
    // 1. Validar strip_ansi
    let colored = "\x1B[1;32mTexto Verde\x1B[0m";
    if strip_ansi(colored) != "Texto Verde" {
        return false;
    }

    // 2. Validar parse de colores estándar
    let spans = parse_ansi_spans("\x1B[31mRojo\x1B[34mAzul\x1B[0m", COLOR_WHITE, COLOR_BLACK);
    if spans.len() != 2 || spans[0].fg != COLOR_RED || spans[1].fg != COLOR_BLUE {
        return false;
    }

    // 3. Validar RGB TrueColor de 24 bits (\x1b[38;2;255;128;0m)
    let rgb_sample = "\x1B[38;2;255;128;0mNaranja\x1B[0m";
    let rgb_spans = parse_ansi_spans(rgb_sample, COLOR_WHITE, COLOR_BLACK);
    if rgb_spans.len() != 1 || rgb_spans[0].fg != 0x00FF8000 {
        return false;
    }

    // 4. Validar paleta de 256 colores
    let c256_sample = "\x1B[38;5;196mRojo256\x1B[0m";
    let c256_spans = parse_ansi_spans(c256_sample, COLOR_WHITE, COLOR_BLACK);
    if c256_spans.is_empty() {
        return false;
    }

    true
}
