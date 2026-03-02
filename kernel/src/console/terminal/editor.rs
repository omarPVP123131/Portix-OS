// console/terminal/editor.rs — PORTIX Kernel v0.7.4
//
// Editor hexadecimal interactivo para sectores de disco ATA.
// Se activa con el comando `diskedit [lba] [drive]` desde el terminal.
//
// Controles:
//   Flechas                    → mover cursor byte a byte / fila a fila
//   RePag / AvPag              → saltar 16 filas arriba/abajo
//   Inicio / Fin               → ir al principio/final de la fila actual
//   0-9 / A-F                  → editar nibble activo (alto → bajo → avanza)
//   S                          → guardar sector en disco
//   Esc                        → pide confirmación si hay cambios; 2º Esc sale

#![allow(dead_code)]

use crate::drivers::input::keyboard::Key;
use crate::drivers::storage::ata::{AtaDrive, AtaError, DriveId, DriveInfo};
use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::util::fmt as kfmt;

// ── Paleta ────────────────────────────────────────────────────────────────────

pub struct EdPalette;
impl EdPalette {
    pub const HEADER:     Color = Color::new(0x10, 0x80, 0xFF);
    pub const HEADER_TXT: Color = Color::WHITE;
    pub const ROW_EVEN:   Color = Color::new(0x12, 0x12, 0x1A);
    pub const ROW_ODD:    Color = Color::new(0x0D, 0x0D, 0x14);
    pub const CURSOR_BG:  Color = Color::new(0xFF, 0xB0, 0x00);
    pub const CURSOR_FG:  Color = Color::new(0x00, 0x00, 0x00);
    pub const OFFSET_FG:  Color = Color::new(0x60, 0x60, 0x80);
    pub const MSG_WARN:   Color = Color::new(0xFF, 0xCC, 0x00);
    pub const MSG_ERR:    Color = Color::new(0xFF, 0x44, 0x44);
    pub const MSG_OK:     Color = Color::new(0x44, 0xFF, 0x88);
    pub const BORDER:     Color = Color::new(0x30, 0x30, 0x50);
    pub const WHITE:      Color = Color::WHITE;
}

// ── Tipo de mensaje ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum MsgKind { Normal, Warn, Error, Ok }

// ── Constantes de geometría ───────────────────────────────────────────────────

const VISIBLE_ROWS: usize = 16; // filas visibles simultáneamente
const TOTAL_ROWS:   usize = 32; // sector de 512 bytes = 32 filas × 16 bytes

// ── Estado del editor ─────────────────────────────────────────────────────────

pub struct EditorState {
    /// Buffer del sector (512 bytes)
    pub buf:          [u8; 512],
    /// Índice de byte del cursor (0-511)
    pub cursor:       usize,
    /// LBA del sector cargado
    pub lba:          u64,
    /// Info del drive (para reconstruir AtaDrive sin re-escanear)
    pub drive_info:   DriveInfo,
    /// Hay cambios sin guardar
    pub dirty:        bool,
    /// Editando nibble alto (true) o bajo (false)
    pub hi_nibble:    bool,
    /// Primera fila lógica visible en la ventana
    pub scroll:       usize,
    /// Primera pulsación de Esc con dirty: espera confirmación
    pub confirm_exit: bool,
    /// Se ha pedido salir definitivamente
    pub exit:         bool,
    /// Mensaje de estado
    pub msg:          [u8; 80],
    pub msg_len:      usize,
    pub msg_kind:     MsgKind,
}

impl EditorState {
    pub fn new(buf: [u8; 512], lba: u64, drive_info: DriveInfo) -> Self {
        let mut ed = EditorState {
            buf,
            lba,
            drive_info,
            cursor:       0,
            dirty:        false,
            hi_nibble:    true,
            scroll:       0,
            confirm_exit: false,
            exit:         false,
            msg:          [0u8; 80],
            msg_len:      0,
            msg_kind:     MsgKind::Normal,
        };
        ed.set_msg(
            b"[S]=Guardar  [Esc]=Salir  [Flechas]=Mover  [0-9/A-F]=Editar nibble",
            MsgKind::Normal,
        );
        ed
    }

    // ── Mensajes ─────────────────────────────────────────────────────────────

    pub fn set_msg(&mut self, m: &[u8], kind: MsgKind) {
        let l = m.len().min(80);
        self.msg[..l].copy_from_slice(&m[..l]);
        for b in &mut self.msg[l..] { *b = 0; }
        self.msg_len  = l;
        self.msg_kind = kind;
    }

    // ── Cursor ────────────────────────────────────────────────────────────────

    fn cur_row(&self) -> usize { self.cursor / 16 }

    fn ensure_visible(&mut self) {
        let row = self.cur_row();
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + VISIBLE_ROWS {
            self.scroll = row + 1 - VISIBLE_ROWS;
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        let new = (self.cursor as isize + delta).max(0).min(511) as usize;
        self.cursor    = new;
        self.hi_nibble = true;
        self.ensure_visible();
    }

    // ── Procesado de teclas ───────────────────────────────────────────────────

    /// Devuelve `true` si la pantalla necesita redibujar
    pub fn handle_key(&mut self, key: Key) -> bool {
        match key {
            // ── Salida ────────────────────────────────────────────────────────
            Key::Escape => {
                if self.dirty && !self.confirm_exit {
                    self.confirm_exit = true;
                    self.set_msg(
                        b"Cambios sin guardar! Pulsa Esc de nuevo para salir o S para guardar.",
                        MsgKind::Warn,
                    );
                } else {
                    self.exit = true;
                }
            }

            // ── Guardar ───────────────────────────────────────────────────────
            Key::Char(b's') | Key::Char(b'S') => {
                self.confirm_exit = false;
                self.do_save();
            }

            // ── Navegación ────────────────────────────────────────────────────
            Key::Left     => { self.move_cursor(-1); }
            Key::Right    => { self.move_cursor( 1); }
            Key::Up       => { self.move_cursor(-16); }
            Key::Down     => { self.move_cursor( 16); }
            Key::Home     => {
                self.cursor    = self.cur_row() * 16;
                self.hi_nibble = true;
                self.ensure_visible();
            }
            Key::End      => {
                self.cursor    = self.cur_row() * 16 + 15;
                self.hi_nibble = true;
                self.ensure_visible();
            }
            Key::PageUp   => { self.move_cursor(-((VISIBLE_ROWS * 16) as isize)); }
            Key::PageDown => { self.move_cursor( (VISIBLE_ROWS * 16) as isize); }

            // ── Edición de nibble ─────────────────────────────────────────────
            Key::Char(c) => {
                if let Some(nibble) = hex_nibble(c) {
                    self.confirm_exit = false;
                    let byte = &mut self.buf[self.cursor];
                    if self.hi_nibble {
                        *byte          = (*byte & 0x0F) | (nibble << 4);
                        self.hi_nibble = false;
                    } else {
                        *byte          = (*byte & 0xF0) | nibble;
                        self.hi_nibble = true;
                        if self.cursor < 511 {
                            self.cursor += 1;
                            self.ensure_visible();
                        }
                    }
                    self.dirty = true;
                    self.set_msg(b"Modificado. [S] para guardar en disco.", MsgKind::Warn);
                }
            }

            _ => return false,
        }
        true
    }

    // ── Guardar en disco ──────────────────────────────────────────────────────

    fn do_save(&mut self) {
        let drive = AtaDrive::from_info(self.drive_info);
        match drive.write_sectors(self.lba, 1, &self.buf) {
            Ok(()) => {
                self.dirty = false;
                self.set_msg(b"[OK] Sector escrito en disco correctamente.", MsgKind::Ok);
            }
            Err(e) => {
                let mut m = [0u8; 80]; let mut p = 0;
                let prefix = b"[ERROR] No se pudo guardar: ";
                m[..prefix.len()].copy_from_slice(prefix); p += prefix.len();
                let es: &[u8] = match e {
                    AtaError::Timeout        => b"timeout",
                    AtaError::DriveFault     => b"fallo de drive",
                    AtaError::OutOfRange     => b"fuera de rango",
                    AtaError::DeviceError(_) => b"error de dispositivo",
                    _                        => b"error desconocido",
                };
                let el = es.len().min(80 - p);
                m[p..p + el].copy_from_slice(&es[..el]); p += el;
                self.set_msg(&m[..p], MsgKind::Error);
            }
        }
    }
}

// ── Helpers internos ──────────────────────────────────────────────────────────

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// ── Renderizado ───────────────────────────────────────────────────────────────

/// Renderiza el editor hexadecimal en el framebuffer.
/// Llama desde el bloque de render de main.rs cuando `term.editor.is_some()`.
pub fn draw_editor_tab(c: &mut Console, lay: &Layout, ed: &EditorState) {
    const HEX: &[u8] = b"0123456789ABCDEF";

    // Usar font_w / font_h de Layout (campos reales: font_w, font_h, line_h)
    let fw  = lay.fw;
    let ch  = lay.font_h;   // altura de carácter  (era lay.char_h)
    let cw  = lay.font_w;   // anchura de carácter (era lay.char_w)
    let x0: usize = 8;
    // y0 arranca justo debajo de la barra de tabs (tab_y + tab_h + 2 = content_y)
    let y0: usize = lay.content_y;

    // ── Cabecera ──────────────────────────────────────────────────────────────
    c.fill_rect(x0, y0, fw - x0, ch + 4, EdPalette::HEADER);
    {
        let mut hbuf = [0u8; 120]; let mut hp = 0;
        let p1 = b" EDITOR DE DISCO  LBA: ";
        hbuf[..p1.len()].copy_from_slice(p1); hp += p1.len();
        let mut tmp = [0u8; 20];
        let ls = kfmt::fmt_u64(ed.lba, &mut tmp);
        for b in ls.bytes() { if hp < 120 { hbuf[hp] = b; hp += 1; } }
        let p2 = b"  Drive: ";
        for b in p2 { if hp < 120 { hbuf[hp] = *b; hp += 1; } }
        let dl: &[u8] = match ed.drive_info.id {
            DriveId::Primary0   => b"ATA0-Master",
            DriveId::Primary1   => b"ATA0-Slave ",
            DriveId::Secondary0 => b"ATA1-Master",
            DriveId::Secondary1 => b"ATA1-Slave ",
        };
        for b in dl { if hp < 120 { hbuf[hp] = *b; hp += 1; } }
        if ed.dirty {
            for b in b"  [MODIFICADO]" { if hp < 120 { hbuf[hp] = *b; hp += 1; } }
        }
        // draw_text_line no existe en Console → usar write_at_bg
        c.write_at_bg(
            core::str::from_utf8(&hbuf[..hp]).unwrap_or(""),
            x0 + 4, y0 + 2,
            EdPalette::HEADER_TXT, EdPalette::HEADER,
        );
    }

    // ── Encabezado de columnas ────────────────────────────────────────────────
    let y_col = y0 + ch + 6;
    c.fill_rect(x0, y_col, fw - x0, ch + 2, EdPalette::BORDER);
    c.write_at_bg(
        " Offset   00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F   ASCII",
        x0 + 4, y_col + 1,
        EdPalette::OFFSET_FG, EdPalette::BORDER,
    );

    // ── Filas del sector ──────────────────────────────────────────────────────
    let y_rows = y_col + ch + 4;
    let row_h  = ch + 2;

    for vis in 0..VISIBLE_ROWS {
        let row    = ed.scroll + vis;
        let byte0  = row * 16;
        let y_row  = y_rows + vis * row_h;
        let row_bg = if row % 2 == 0 { EdPalette::ROW_EVEN } else { EdPalette::ROW_ODD };

        c.fill_rect(x0, y_row, fw - x0, row_h, row_bg);
        if row >= TOTAL_ROWS { continue; }

        let mut line = [0u8; 128]; let mut lp = 0;

        // Offset 4 dígitos hex
        line[lp] = b' '; lp += 1;
        let off = byte0 as u16;
        line[lp] = HEX[((off >> 12) & 0xF) as usize]; lp += 1;
        line[lp] = HEX[((off >>  8) & 0xF) as usize]; lp += 1;
        line[lp] = HEX[((off >>  4) & 0xF) as usize]; lp += 1;
        line[lp] = HEX[(off & 0xF)  as usize];         lp += 1;
        line[lp] = b' '; lp += 1;
        line[lp] = b' '; lp += 1;
        line[lp] = b' '; lp += 1;

        // 16 bytes en hex
        for col in 0..16usize {
            if col == 8 { line[lp] = b' '; lp += 1; }
            let byte = ed.buf[byte0 + col];
            line[lp] = HEX[(byte >> 4) as usize]; lp += 1;
            line[lp] = HEX[(byte & 0xF) as usize]; lp += 1;
            line[lp] = b' '; lp += 1;
        }

        // Columna ASCII
        line[lp] = b' '; lp += 1;
        line[lp] = b' '; lp += 1;
        for col in 0..16usize {
            let b = ed.buf[byte0 + col];
            line[lp] = if b >= 0x20 && b < 0x7F { b } else { b'.' };
            lp += 1;
        }

        c.write_at_bg(
            core::str::from_utf8(&line[..lp]).unwrap_or(""),
            x0 + 2, y_row + 1,
            EdPalette::WHITE, row_bg,
        );

        // ── Resaltado del cursor ──────────────────────────────────────────────
        if ed.cursor >= byte0 && ed.cursor < byte0 + 16 {
            let col = ed.cursor - byte0;

            // Posición X del primer nibble del byte en la zona hex:
            // prefijo = " XXXX   " = 8 chars; cada byte = "XX " = 3 chars; gap en col 8
            let hex_char_x = 8 + col * 3 + if col >= 8 { 1 } else { 0 };
            let hex_px     = x0 + 2 + hex_char_x * cw;
            let nibble_px  = if ed.hi_nibble { hex_px } else { hex_px + cw };

            c.fill_rect(nibble_px, y_row, cw, row_h, EdPalette::CURSOR_BG);
            let byte = ed.buf[ed.cursor];
            let nib  = if ed.hi_nibble { HEX[(byte >> 4) as usize] } else { HEX[(byte & 0xF) as usize] };
            let ns   = [nib];
            c.write_at_bg(
                core::str::from_utf8(&ns).unwrap_or("."),
                nibble_px, y_row + 1,
                EdPalette::CURSOR_FG, EdPalette::CURSOR_BG,
            );

            // ASCII: prefijo(8) + 16*3 hex(48) + gap(1) + "  "(2) = 59
            let ascii_col_x = 8 + 16 * 3 + 1 + 2 + col;
            let ascii_px    = x0 + 2 + ascii_col_x * cw;
            c.fill_rect(ascii_px, y_row, cw, row_h, EdPalette::CURSOR_BG);
            let ac = { let b = ed.buf[ed.cursor]; if b >= 0x20 && b < 0x7F { b } else { b'.' } };
            let sa = [ac];
            c.write_at_bg(
                core::str::from_utf8(&sa).unwrap_or("."),
                ascii_px, y_row + 1,
                EdPalette::CURSOR_FG, EdPalette::CURSOR_BG,
            );
        }
    }

    // ── Barra de scroll lateral ───────────────────────────────────────────────
    {
        let sb_x    = fw - 10;
        let total_h = VISIBLE_ROWS * row_h;
        c.fill_rect(sb_x, y_rows, 8, total_h, EdPalette::BORDER);
        let thumb_h = (total_h * VISIBLE_ROWS / TOTAL_ROWS).max(8);
        let thumb_y = y_rows + (ed.scroll * total_h) / TOTAL_ROWS;
        let thumb_y_clamped = thumb_y.max(y_rows);
        let thumb_h_clamped = thumb_h.min(y_rows + total_h - thumb_y_clamped);
        c.fill_rect(sb_x + 1, thumb_y_clamped, 6, thumb_h_clamped, EdPalette::HEADER);
    }

    // ── Barra de estado ───────────────────────────────────────────────────────
    let y_status = y_rows + VISIBLE_ROWS * row_h + 6;
    let msg_color = match ed.msg_kind {
        MsgKind::Ok     => EdPalette::MSG_OK,
        MsgKind::Warn   => EdPalette::MSG_WARN,
        MsgKind::Error  => EdPalette::MSG_ERR,
        MsgKind::Normal => EdPalette::WHITE,
    };
    c.fill_rect(x0, y_status, fw - x0, ch + 4, EdPalette::BORDER);
    c.write_at_bg(
        core::str::from_utf8(&ed.msg[..ed.msg_len]).unwrap_or(""),
        x0 + 4, y_status + 2,
        msg_color, EdPalette::BORDER,
    );

    // Posición del cursor (esquina derecha de la barra de estado)
    {
        let mut info = [0u8; 32]; let mut ip = 0;
        for b in b"Byte:0x" { if ip < 32 { info[ip] = *b; ip += 1; } }
        let mut tmp = [0u8; 16];
        let os = kfmt::fmt_u16(ed.cursor as u16, &mut tmp);
        for b in os.bytes() { if ip < 32 { info[ip] = b; ip += 1; } }
        for b in b" Val:0x" { if ip < 32 { info[ip] = *b; ip += 1; } }
        let byte = ed.buf[ed.cursor];
        if ip + 2 <= 32 {
            info[ip] = HEX[(byte >> 4) as usize]; ip += 1;
            info[ip] = HEX[(byte & 0xF) as usize]; ip += 1;
        }
        let info_x = (fw).saturating_sub(ip * cw + 12).max(x0);
        c.write_at_bg(
            core::str::from_utf8(&info[..ip]).unwrap_or(""),
            info_x, y_status + 2,
            EdPalette::OFFSET_FG, EdPalette::BORDER,
        );
    }
}