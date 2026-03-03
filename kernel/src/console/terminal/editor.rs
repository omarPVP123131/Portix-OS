// console/terminal/editor.rs — PORTIX Kernel v0.7.5
//
// Editor dual: hex (raw sector) y texto (tipo nano).
//
// ┌─ Modo HEX ─────────────────────────────────────────────────────────────────┐
// │  Flechas / RePág / AvPág / Inicio / Fin → mover cursor                    │
// │  0-9 / A-F → editar nibble activo (alto → bajo → avanza)                  │
// │  S → guardar sector raw en disco                                           │
// │  Esc → pide confirmación si hay cambios; 2.º Esc sale                      │
// └────────────────────────────────────────────────────────────────────────────┘
// ┌─ Modo TEXTO (nano-like) ───────────────────────────────────────────────────┐
// │  Flechas → mover cursor                                                    │
// │  Ctrl+S (s)  → guardar archivo FAT32                                       │
// │  Ctrl+X (Esc)→ salir (pide confirmación si hay cambios)                    │
// │  Enter → nueva línea                                                       │
// │  Backspace → borrar carácter anterior                                      │
// │  Caracteres imprimibles → insertar                                         │
// └────────────────────────────────────────────────────────────────────────────┘

#![allow(dead_code)]

use crate::drivers::input::keyboard::Key;
use crate::drivers::storage::ata::{AtaDrive, AtaError, DriveId, DriveInfo};
use crate::drivers::storage::fat32::{Fat32Volume, DirEntryInfo};
use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::util::fmt as kfmt;

// ── Límites ───────────────────────────────────────────────────────────────────

/// Máximo de bytes editables en modo texto (32 KiB).
pub const EDITOR_MAX_BYTES: usize = 32768;
/// Máximo de líneas lógicas en el editor de texto.
const TEXT_MAX_LINES: usize = 512;
/// Máximo de bytes por línea en el editor de texto.
const TEXT_LINE_MAX: usize = 256;
/// Filas visibles en el editor hex.
const HEX_VISIBLE_ROWS: usize = 16;
/// Total de filas en un sector (512 / 16).
const HEX_TOTAL_ROWS: usize = 32;
/// Filas visibles en el editor de texto.
const TEXT_VISIBLE_ROWS: usize = 24;

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
    // Texto modo nano
    pub const TEXT_BG:    Color = Color::new(0x10, 0x10, 0x18);
    pub const TEXT_FG:    Color = Color::WHITE;
    pub const LINE_NUM:   Color = Color::new(0x44, 0x44, 0x66);
    pub const CUR_LINE:   Color = Color::new(0x1A, 0x1A, 0x28);
    pub const STATUS_BAR: Color = Color::new(0x00, 0x55, 0xAA);
    pub const SHORTCUT:   Color = Color::new(0xAA, 0xCC, 0xFF);
}

// ── Modo del editor ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum EditorMode {
    /// Editor hexadecimal de sectores raw.
    Hex,
    /// Editor de texto tipo nano para archivos FAT32.
    Text,
}

// ── Tipo de mensaje ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum MsgKind { Normal, Warn, Error, Ok }

// ── Estado del editor ─────────────────────────────────────────────────────────

pub struct EditorState {
    pub mode: EditorMode,

    // ── Modo Hex ──────────────────────────────────────────────────────────────
    /// Buffer del sector (512 bytes) en modo hex.
    pub buf:          [u8; 512],
    /// LBA del sector en modo hex.
    pub lba:          u64,
    /// Info del drive para modo hex.
    pub drive_info:   DriveInfo,
    /// Cursor en modo hex (índice de byte 0-511).
    pub cursor:       usize,
    /// Nibble alto (true) o bajo (false) en modo hex.
    pub hi_nibble:    bool,
    /// Primera fila visible en modo hex.
    pub scroll:       usize,

    // ── Modo Texto ────────────────────────────────────────────────────────────
    /// Buffer de texto (hasta EDITOR_MAX_BYTES).
    pub text_buf:     [u8; EDITOR_MAX_BYTES],
    /// Bytes válidos en text_buf.
    pub text_len:     usize,
    /// Línea actual del cursor (fila lógica 0-based).
    pub text_row:     usize,
    /// Columna actual del cursor (columna lógica 0-based).
    pub text_col:     usize,
    /// Primera línea visible (scroll).
    pub text_scroll:  usize,
    /// Entrada FAT32 del archivo en edición.
    pub fat_entry:    Option<DirEntryInfo>,
    /// Ruta del archivo (para el título).
    pub file_path:    [u8; 256],
    pub file_path_len: usize,

    // ── Común ─────────────────────────────────────────────────────────────────
    pub dirty:        bool,
    pub confirm_exit: bool,
    pub exit:         bool,
    pub msg:          [u8; 80],
    pub msg_len:      usize,
    pub msg_kind:     MsgKind,
}

impl EditorState {
    // ── Constructores ─────────────────────────────────────────────────────────

    /// Crea el estado para el editor hexadecimal (sector raw).
    pub fn new_hex(buf: [u8; 512], lba: u64, drive_info: DriveInfo) -> Self {
        let mut ed = Self::base(drive_info);
        ed.mode   = EditorMode::Hex;
        ed.buf    = buf;
        ed.lba    = lba;
        ed.set_msg(
            b"[S]=Guardar  [Esc]=Salir  [Flechas]=Mover  [0-9/A-F]=Editar nibble",
            MsgKind::Normal,
        );
        ed
    }

    /// Crea el estado para el editor de texto tipo nano.
    pub fn new_text(
        content: [u8; EDITOR_MAX_BYTES],
        content_len: usize,
        entry: DirEntryInfo,
        drive_info: DriveInfo,
        path: &[u8],
    ) -> Self {
        let mut ed = Self::base(drive_info);
        ed.mode       = EditorMode::Text;
        ed.text_buf   = content;
        ed.text_len   = content_len;
        ed.fat_entry  = Some(entry);
        let pl = path.len().min(256);
        ed.file_path[..pl].copy_from_slice(&path[..pl]);
        ed.file_path_len = pl;
        ed.set_msg(
            b"^S Guardar  ^X Salir  Flechas Mover  Enter Nueva linea",
            MsgKind::Normal,
        );
        ed
    }

    fn base(drive_info: DriveInfo) -> Self {
        EditorState {
            mode:          EditorMode::Hex,
            buf:           [0u8; 512],
            lba:           0,
            drive_info,
            cursor:        0,
            hi_nibble:     true,
            scroll:        0,
            text_buf:      [0u8; EDITOR_MAX_BYTES],
            text_len:      0,
            text_row:      0,
            text_col:      0,
            text_scroll:   0,
            fat_entry:     None,
            file_path:     [0u8; 256],
            file_path_len: 0,
            dirty:         false,
            confirm_exit:  false,
            exit:          false,
            msg:           [0u8; 80],
            msg_len:       0,
            msg_kind:      MsgKind::Normal,
        }
    }

    // Mantener compatibilidad con código antiguo que usa new() para modo hex
    pub fn new(buf: [u8; 512], lba: u64, drive_info: DriveInfo) -> Self {
        Self::new_hex(buf, lba, drive_info)
    }

    // ── Mensajes ─────────────────────────────────────────────────────────────

    pub fn set_msg(&mut self, m: &[u8], kind: MsgKind) {
        let l = m.len().min(80);
        self.msg[..l].copy_from_slice(&m[..l]);
        for b in &mut self.msg[l..] { *b = 0; }
        self.msg_len  = l;
        self.msg_kind = kind;
    }

    // ── Procesado de teclas ───────────────────────────────────────────────────

    /// Retorna `true` si la pantalla necesita redibujar.
    pub fn handle_key(&mut self, key: Key) -> bool {
        match self.mode {
            EditorMode::Hex  => self.handle_key_hex(key),
            EditorMode::Text => self.handle_key_text(key),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Teclas en modo HEX
    // ─────────────────────────────────────────────────────────────────────────

    fn handle_key_hex(&mut self, key: Key) -> bool {
        match key {
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
            Key::Char(b's') | Key::Char(b'S') => {
                self.confirm_exit = false;
                self.save_hex();
            }
            Key::Left     => { self.hex_move(-1); }
            Key::Right    => { self.hex_move( 1); }
            Key::Up       => { self.hex_move(-16); }
            Key::Down     => { self.hex_move( 16); }
            Key::Home     => {
                self.cursor    = (self.cursor / 16) * 16;
                self.hi_nibble = true;
                self.hex_ensure_visible();
            }
            Key::End      => {
                self.cursor    = (self.cursor / 16) * 16 + 15;
                self.hi_nibble = true;
                self.hex_ensure_visible();
            }
            Key::PageUp   => { self.hex_move(-((HEX_VISIBLE_ROWS * 16) as isize)); }
            Key::PageDown => { self.hex_move( (HEX_VISIBLE_ROWS * 16) as isize); }
            Key::Char(c) => {
                if let Some(nibble) = hex_nibble(c) {
                    self.confirm_exit = false;
                    let byte = &mut self.buf[self.cursor];
                    if self.hi_nibble {
                        *byte = (*byte & 0x0F) | (nibble << 4);
                        self.hi_nibble = false;
                    } else {
                        *byte = (*byte & 0xF0) | nibble;
                        self.hi_nibble = true;
                        if self.cursor < 511 {
                            self.cursor += 1;
                            self.hex_ensure_visible();
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

    fn hex_move(&mut self, delta: isize) {
        let new = (self.cursor as isize + delta).max(0).min(511) as usize;
        self.cursor    = new;
        self.hi_nibble = true;
        self.hex_ensure_visible();
    }

    fn hex_ensure_visible(&mut self) {
        let row = self.cursor / 16;
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + HEX_VISIBLE_ROWS {
            self.scroll = row + 1 - HEX_VISIBLE_ROWS;
        }
    }

    fn save_hex(&mut self) {
        let drive = AtaDrive::from_info(self.drive_info);
        match drive.write_sectors(self.lba, 1, &self.buf) {
            Ok(()) => {
                self.dirty = false;
                self.set_msg(b"[OK] Sector escrito en disco.", MsgKind::Ok);
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
                m[p..p + es.len().min(80 - p)].copy_from_slice(&es[..es.len().min(80 - p)]);
                self.set_msg(&m, MsgKind::Error);
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Teclas en modo TEXTO
    // ─────────────────────────────────────────────────────────────────────────

    fn handle_key_text(&mut self, key: Key) -> bool {
        match key {
            // Salir (Ctrl+X = Esc en nuestro keyboard driver)
            Key::Escape => {
                if self.dirty && !self.confirm_exit {
                    self.confirm_exit = true;
                    self.set_msg(
                        b"Cambios sin guardar! Pulsa Esc de nuevo para salir o ^S para guardar.",
                        MsgKind::Warn,
                    );
                } else {
                    self.exit = true;
                }
            }
            // Guardar (Ctrl+S → en keyboard driver mapea a Key::Char(0x13) o Key::Save)
            Key::Char(b's') | Key::Char(b'S') => {
                self.confirm_exit = false;
                self.save_text();
            }
            // Navegación
            Key::Up    => { self.text_move_up(); }
            Key::Down  => { self.text_move_down(); }
            Key::Left  => { self.text_move_left(); }
            Key::Right => { self.text_move_right(); }
            Key::Home  => {
                self.text_col = 0;
            }
            Key::End => {
                let (_, len) = self.get_line_bounds(self.text_row);
                self.text_col = len;
            }
            Key::PageUp => {
                for _ in 0..TEXT_VISIBLE_ROWS {
                    if self.text_row == 0 { break; }
                    self.text_row -= 1;
                }
                self.text_ensure_visible();
            }
            Key::PageDown => {
                let lines = self.count_lines();
                for _ in 0..TEXT_VISIBLE_ROWS {
                    if self.text_row + 1 >= lines { break; }
                    self.text_row += 1;
                }
                self.text_ensure_visible();
            }
            // Edición
            Key::Enter => {
                self.text_insert(b'\n');
                self.dirty        = true;
                self.confirm_exit = false;
                self.set_msg(b"Modificado. ^S para guardar.", MsgKind::Warn);
            }
            Key::Backspace => {
                self.text_backspace();
                self.dirty        = true;
                self.confirm_exit = false;
                self.set_msg(b"Modificado. ^S para guardar.", MsgKind::Warn);
            }
            Key::Char(c) => {
                if c >= 0x20 && c < 0x7F {
                    self.text_insert(c);
                    self.dirty        = true;
                    self.confirm_exit = false;
                    self.set_msg(b"Modificado. ^S para guardar.", MsgKind::Warn);
                }
            }
            _ => return false,
        }
        true
    }

    // ── Helpers de navegación de texto ────────────────────────────────────────

    /// Cuenta el número de líneas lógicas en el buffer.
    fn count_lines(&self) -> usize {
        if self.text_len == 0 { return 1; }
        let mut n = 1usize;
        for i in 0..self.text_len {
            if self.text_buf[i] == b'\n' { n += 1; }
        }
        n
    }

    /// Devuelve (offset_inicio, longitud) de la línea lógica `row`.
    fn get_line_bounds(&self, row: usize) -> (usize, usize) {
        let mut cur_row = 0usize;
        let mut start   = 0usize;
        let mut i       = 0usize;
        while i <= self.text_len {
            let at_nl  = i < self.text_len && self.text_buf[i] == b'\n';
            let at_end = i == self.text_len;
            if at_nl || at_end {
                if cur_row == row {
                    return (start, i - start);
                }
                cur_row += 1;
                start = i + 1;
            }
            i += 1;
        }
        (self.text_len, 0)
    }

    /// Offset en el buffer del cursor actual.
    fn cursor_offset(&self) -> usize {
        let (start, len) = self.get_line_bounds(self.text_row);
        start + self.text_col.min(len)
    }

    fn text_move_up(&mut self) {
        if self.text_row > 0 {
            self.text_row -= 1;
            let (_, len) = self.get_line_bounds(self.text_row);
            if self.text_col > len { self.text_col = len; }
            self.text_ensure_visible();
        }
    }
    fn text_move_down(&mut self) {
        let lines = self.count_lines();
        if self.text_row + 1 < lines {
            self.text_row += 1;
            let (_, len) = self.get_line_bounds(self.text_row);
            if self.text_col > len { self.text_col = len; }
            self.text_ensure_visible();
        }
    }
    fn text_move_left(&mut self) {
        if self.text_col > 0 {
            self.text_col -= 1;
        } else if self.text_row > 0 {
            self.text_row -= 1;
            let (_, len) = self.get_line_bounds(self.text_row);
            self.text_col = len;
            self.text_ensure_visible();
        }
    }
    fn text_move_right(&mut self) {
        let (_, len) = self.get_line_bounds(self.text_row);
        if self.text_col < len {
            self.text_col += 1;
        } else {
            let lines = self.count_lines();
            if self.text_row + 1 < lines {
                self.text_row += 1;
                self.text_col = 0;
                self.text_ensure_visible();
            }
        }
    }

    fn text_ensure_visible(&mut self) {
        if self.text_row < self.text_scroll {
            self.text_scroll = self.text_row;
        } else if self.text_row >= self.text_scroll + TEXT_VISIBLE_ROWS {
            self.text_scroll = self.text_row + 1 - TEXT_VISIBLE_ROWS;
        }
    }

    // ── Inserción y borrado ───────────────────────────────────────────────────

    fn text_insert(&mut self, c: u8) {
        let offset = self.cursor_offset();
        if self.text_len >= EDITOR_MAX_BYTES { return; }
        // Desplazar hacia adelante
        let mut i = self.text_len;
        while i > offset {
            self.text_buf[i] = self.text_buf[i - 1];
            i -= 1;
        }
        self.text_buf[offset] = c;
        self.text_len += 1;

        if c == b'\n' {
            self.text_row += 1;
            self.text_col  = 0;
            self.text_ensure_visible();
        } else {
            self.text_col += 1;
        }
    }

    fn text_backspace(&mut self) {
        let offset = self.cursor_offset();
        if offset == 0 { return; }
        // Determinar si el carácter anterior es '\n'
        let prev = self.text_buf[offset - 1];
        // Desplazar hacia atrás
        let mut i = offset - 1;
        while i < self.text_len - 1 {
            self.text_buf[i] = self.text_buf[i + 1];
            i += 1;
        }
        self.text_len -= 1;

        if prev == b'\n' {
            // Volver a la línea anterior
            if self.text_row > 0 {
                self.text_row -= 1;
                let (_, len) = self.get_line_bounds(self.text_row);
                self.text_col = len;
                self.text_ensure_visible();
            }
        } else if self.text_col > 0 {
            self.text_col -= 1;
        }
    }

    // ── Guardar en FAT32 ─────────────────────────────────────────────────────

    fn save_text(&mut self) {
        // Necesitamos montar el volumen para guardar
        let bus  = crate::drivers::storage::ata::AtaBus::scan();
        let info = match bus.info(DriveId::Primary0) {
            Some(i) => *i,
            None => {
                self.set_msg(b"[ERROR] Drive no disponible.", MsgKind::Error);
                return;
            }
        };
        let drive = AtaDrive::from_info(info);
        let vol = match Fat32Volume::mount(drive) {
            Ok(v) => v,
            Err(_) => {
                self.set_msg(b"[ERROR] No se pudo montar el volumen FAT32.", MsgKind::Error);
                return;
            }
        };

        if let Some(ref mut entry) = self.fat_entry {
            match vol.write_file(entry, &self.text_buf[..self.text_len]) {
                Ok(()) => {
                    self.dirty = false;
                    let mut m = [0u8; 80]; let mut p = 0;
                    let ok = b"[OK] Guardado: ";
                    m[..ok.len()].copy_from_slice(ok); p += ok.len();
                    let nl = self.file_path_len.min(80 - p);
                    m[p..p + nl].copy_from_slice(&self.file_path[..nl]);
                    self.set_msg(&m, MsgKind::Ok);
                }
                Err(_) => {
                    self.set_msg(b"[ERROR] No se pudo escribir el archivo.", MsgKind::Error);
                }
            }
        } else {
            self.set_msg(b"[ERROR] Sin referencia al archivo.", MsgKind::Error);
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RENDERIZADO
// ═══════════════════════════════════════════════════════════════════════════════

/// Renderiza el editor en el framebuffer.
/// Selecciona automáticamente modo hex o texto según `ed.mode`.
pub fn draw_editor_tab(c: &mut Console, lay: &Layout, ed: &EditorState) {
    match ed.mode {
        EditorMode::Hex  => draw_hex(c, lay, ed),
        EditorMode::Text => draw_text(c, lay, ed),
    }
}

// ── Renderizado HEX ───────────────────────────────────────────────────────────

fn draw_hex(c: &mut Console, lay: &Layout, ed: &EditorState) {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let fw  = lay.fw;
    let ch  = lay.font_h;
    let cw  = lay.font_w;
    let x0  = 8usize;
    let y0  = lay.content_y;

    // Cabecera
    c.fill_rect(x0, y0, fw - x0, ch + 4, EdPalette::HEADER);
    {
        let mut hbuf = [0u8; 120]; let mut hp = 0;
        for b in b" EDITOR HEX  LBA: " { if hp < 120 { hbuf[hp] = *b; hp += 1; } }
        let mut tmp = [0u8; 20];
        let ls = kfmt::fmt_u64(ed.lba, &mut tmp);
        for b in ls.bytes() { if hp < 120 { hbuf[hp] = b; hp += 1; } }
        for b in b"  Drive: " { if hp < 120 { hbuf[hp] = *b; hp += 1; } }
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
        c.write_at_bg(
            core::str::from_utf8(&hbuf[..hp]).unwrap_or(""),
            x0 + 4, y0 + 2,
            EdPalette::HEADER_TXT, EdPalette::HEADER,
        );
    }

    let y_col = y0 + ch + 6;
    c.fill_rect(x0, y_col, fw - x0, ch + 2, EdPalette::BORDER);
    c.write_at_bg(
        " Offset   00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F   ASCII",
        x0 + 4, y_col + 1,
        EdPalette::OFFSET_FG, EdPalette::BORDER,
    );

    let y_rows = y_col + ch + 4;
    let row_h  = ch + 2;

    for vis in 0..HEX_VISIBLE_ROWS {
        let row    = ed.scroll + vis;
        let byte0  = row * 16;
        let y_row  = y_rows + vis * row_h;
        let row_bg = if row % 2 == 0 { EdPalette::ROW_EVEN } else { EdPalette::ROW_ODD };

        c.fill_rect(x0, y_row, fw - x0, row_h, row_bg);
        if row >= HEX_TOTAL_ROWS { continue; }

        let mut line = [0u8; 128]; let mut lp = 0;
        line[lp] = b' '; lp += 1;
        let off = byte0 as u16;
        line[lp] = HEX[((off >> 12) & 0xF) as usize]; lp += 1;
        line[lp] = HEX[((off >>  8) & 0xF) as usize]; lp += 1;
        line[lp] = HEX[((off >>  4) & 0xF) as usize]; lp += 1;
        line[lp] = HEX[(off & 0xF)  as usize];         lp += 1;
        line[lp] = b' '; lp += 1; line[lp] = b' '; lp += 1; line[lp] = b' '; lp += 1;

        for col in 0..16usize {
            if col == 8 { line[lp] = b' '; lp += 1; }
            let byte = ed.buf[byte0 + col];
            line[lp] = HEX[(byte >> 4) as usize]; lp += 1;
            line[lp] = HEX[(byte & 0xF) as usize]; lp += 1;
            line[lp] = b' '; lp += 1;
        }

        line[lp] = b' '; lp += 1; line[lp] = b' '; lp += 1;
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

        // Cursor
        if ed.cursor >= byte0 && ed.cursor < byte0 + 16 {
            let col = ed.cursor - byte0;
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

    // Scrollbar
    {
        let sb_x    = fw - 10;
        let total_h = HEX_VISIBLE_ROWS * row_h;
        c.fill_rect(sb_x, y_rows, 8, total_h, EdPalette::BORDER);
        let thumb_h = (total_h * HEX_VISIBLE_ROWS / HEX_TOTAL_ROWS).max(8);
        let thumb_y = y_rows + (ed.scroll * total_h) / HEX_TOTAL_ROWS;
        let th_clamped = thumb_h.min(y_rows + total_h - thumb_y.max(y_rows));
        c.fill_rect(sb_x + 1, thumb_y.max(y_rows), 6, th_clamped, EdPalette::HEADER);
    }

    draw_status_bar(c, lay, ed, y_rows + HEX_VISIBLE_ROWS * row_h + 6);
}

// ── Renderizado TEXTO (nano-like) ─────────────────────────────────────────────

fn draw_text(c: &mut Console, lay: &Layout, ed: &EditorState) {
    let fw  = lay.fw;
    let ch  = lay.font_h;
    let cw  = lay.font_w;
    let x0  = 0usize;
    let y0  = lay.content_y;
    let row_h = ch + 2;

    // ── Barra de título (estilo nano) ─────────────────────────────────────────
    c.fill_rect(x0, y0, fw, ch + 4, EdPalette::STATUS_BAR);
    {
        let mut title = [0u8; 120]; let mut tp = 0;
        for b in b"  PORTIX EDITOR  " { if tp < 120 { title[tp] = *b; tp += 1; } }
        let nl = ed.file_path_len.min(60);
        for b in &ed.file_path[..nl] { if tp < 120 { title[tp] = *b; tp += 1; } }
        if ed.dirty {
            for b in b"  [sin guardar]" { if tp < 120 { title[tp] = *b; tp += 1; } }
        }
        c.write_at_bg(
            core::str::from_utf8(&title[..tp]).unwrap_or(""),
            x0 + 4, y0 + 2,
            EdPalette::WHITE, EdPalette::STATUS_BAR,
        );
    }

    // ── Área de texto ─────────────────────────────────────────────────────────
    let y_text = y0 + ch + 6;
    c.fill_rect(x0, y_text, fw, TEXT_VISIBLE_ROWS * row_h, EdPalette::TEXT_BG);

    // Recopilar líneas para renderizar
    let mut lines: [[u8; TEXT_LINE_MAX]; TEXT_VISIBLE_ROWS] = [[0u8; TEXT_LINE_MAX]; TEXT_VISIBLE_ROWS];
    let mut line_lens = [0usize; TEXT_VISIBLE_ROWS];
    let mut logical_row = 0usize;
    let mut vis_idx = 0usize;
    let mut line_buf = [0u8; TEXT_LINE_MAX]; let mut lb_len = 0usize;
    let total_lines = ed.count_lines();

    let mut i = 0usize;
    loop {
        let at_end = i >= ed.text_len;
        let at_nl  = !at_end && ed.text_buf[i] == b'\n';

        if at_end || at_nl {
            if logical_row >= ed.text_scroll && vis_idx < TEXT_VISIBLE_ROWS {
                let l = lb_len.min(TEXT_LINE_MAX);
                lines[vis_idx][..l].copy_from_slice(&line_buf[..l]);
                line_lens[vis_idx] = l;
                vis_idx += 1;
            }
            logical_row += 1;
            lb_len = 0;
            if at_end { break; }
        } else {
            if lb_len < TEXT_LINE_MAX { line_buf[lb_len] = ed.text_buf[i]; lb_len += 1; }
        }
        i += 1;
    }

    // Si no hay líneas en el buffer, añadir una vacía
    if vis_idx == 0 && ed.text_len == 0 {
        vis_idx = 1; // línea 0 vacía
    }

    for vis in 0..TEXT_VISIBLE_ROWS {
        let log_row  = ed.text_scroll + vis;
        let y_row    = y_text + vis * row_h;
        let is_cur   = log_row == ed.text_row;
        let row_bg   = if is_cur { EdPalette::CUR_LINE } else { EdPalette::TEXT_BG };

        c.fill_rect(x0, y_row, fw, row_h, row_bg);

        // Número de línea (4 caracteres)
        if log_row < total_lines || log_row == 0 {
            let mut num = [0u8; 6]; let mut np = 0;
            let ln = log_row + 1;
            if ln >= 1000 { num[np] = b'0' + (ln / 1000) as u8 % 10; np += 1; }
            if ln >= 100  { num[np] = b'0' + (ln /  100) as u8 % 10; np += 1; }
            if ln >= 10   { num[np] = b'0' + (ln /   10) as u8 % 10; np += 1; }
            num[np] = b'0' + (ln % 10) as u8; np += 1;
            c.write_at_bg(
                core::str::from_utf8(&num[..np]).unwrap_or(""),
                x0 + 2, y_row + 1,
                EdPalette::LINE_NUM, row_bg,
            );
        }

        // Contenido de la línea
        let content_x = x0 + 6 * cw;
        if vis < vis_idx && line_lens[vis] > 0 {
            let view_start = 0; // scroll horizontal no implementado en v1
            let avail = ((fw - content_x) / cw).min(TEXT_LINE_MAX);
            let end   = line_lens[vis].min(view_start + avail);
            let slice = &lines[vis][view_start..end];
            // Limpiar caracteres no imprimibles
            let mut cleaned = [0u8; TEXT_LINE_MAX];
            for (j, &b) in slice.iter().enumerate() {
                cleaned[j] = if b >= 0x20 && b < 0x7F { b } else { b'.' };
            }
            c.write_at_bg(
                core::str::from_utf8(&cleaned[..end - view_start]).unwrap_or(""),
                content_x, y_row + 1,
                EdPalette::TEXT_FG, row_bg,
            );
        }

        // Cursor de texto
        if is_cur {
            let col_x = content_x + ed.text_col * cw;
            c.fill_rect(col_x, y_row, cw.max(2), row_h, EdPalette::CURSOR_BG);
            // Mostrar carácter bajo el cursor
            let cur_char = if vis < vis_idx && ed.text_col < line_lens[vis] {
                let b = lines[vis][ed.text_col];
                if b >= 0x20 && b < 0x7F { b } else { b' ' }
            } else { b' ' };
            let cs = [cur_char];
            c.write_at_bg(
                core::str::from_utf8(&cs).unwrap_or(" "),
                col_x, y_row + 1,
                EdPalette::CURSOR_FG, EdPalette::CURSOR_BG,
            );
        }
    }

    // ── Barra inferior (atajos tipo nano) ─────────────────────────────────────
    let y_bottom = y_text + TEXT_VISIBLE_ROWS * row_h + 2;
    // Barra de estado
    draw_status_bar(c, lay, ed, y_bottom);
    // Barra de atajos (2 líneas como nano)
    let y_keys1 = y_bottom + ch + 6;
    let y_keys2 = y_keys1 + ch + 2;
    c.fill_rect(x0, y_keys1, fw, ch + 2, EdPalette::BORDER);
    c.fill_rect(x0, y_keys2, fw, ch + 2, EdPalette::BORDER);
    c.write_at_bg("  ^S Guardar    ^X Salir    Flechas Mover    Home Inicio linea    End Fin linea",
        x0 + 2, y_keys1 + 1,
        EdPalette::SHORTCUT, EdPalette::BORDER,
    );
    {
        let mut info = [0u8; 80]; let mut ip = 0;
        for b in b"  Linea: " { if ip < 80 { info[ip] = *b; ip += 1; } }
        let ln = ed.text_row + 1;
        if ln >= 100 { info[ip] = b'0' + (ln / 100) as u8 % 10; ip += 1; }
        if ln >= 10  { info[ip] = b'0' + (ln /  10) as u8 % 10; ip += 1; }
        info[ip] = b'0' + (ln % 10) as u8; ip += 1;
        for b in b"  Col: " { if ip < 80 { info[ip] = *b; ip += 1; } }
        let co = ed.text_col + 1;
        if co >= 100 { info[ip] = b'0' + (co / 100) as u8 % 10; ip += 1; }
        if co >= 10  { info[ip] = b'0' + (co /  10) as u8 % 10; ip += 1; }
        info[ip] = b'0' + (co % 10) as u8; ip += 1;
        for b in b"  Total: " { if ip < 80 { info[ip] = *b; ip += 1; } }
        let tl = ed.text_len;
        let ts: [u8; 10] = {
            let mut t = [0u8; 10]; let mut tp = 9;
            let mut n = tl; if n == 0 { t[tp] = b'0'; } else {
                while n > 0 { t[tp] = b'0' + (n % 10) as u8; n /= 10; if tp > 0 { tp -= 1; } else { break; } }
                tp += 1;
            }
            let mut out = [0u8; 10]; let mut op = 0;
            while tp < 10 { out[op] = t[tp]; op += 1; tp += 1; }
            out
        };
        for b in &ts { if *b != 0 && ip < 80 { info[ip] = *b; ip += 1; } }
        for b in b" bytes" { if ip < 80 { info[ip] = *b; ip += 1; } }
        c.write_at_bg(
            core::str::from_utf8(&info[..ip]).unwrap_or(""),
            x0 + 2, y_keys2 + 1,
            EdPalette::SHORTCUT, EdPalette::BORDER,
        );
    }
}

// ── Barra de estado común ─────────────────────────────────────────────────────

fn draw_status_bar(c: &mut Console, lay: &Layout, ed: &EditorState, y: usize) {
    let fw = lay.fw;
    let ch = lay.font_h;
    let cw = lay.font_w;
    let x0 = 8usize;

    let msg_color = match ed.msg_kind {
        MsgKind::Ok     => EdPalette::MSG_OK,
        MsgKind::Warn   => EdPalette::MSG_WARN,
        MsgKind::Error  => EdPalette::MSG_ERR,
        MsgKind::Normal => EdPalette::WHITE,
    };
    c.fill_rect(x0, y, fw - x0, ch + 4, EdPalette::BORDER);
    c.write_at_bg(
        core::str::from_utf8(&ed.msg[..ed.msg_len]).unwrap_or(""),
        x0 + 4, y + 2,
        msg_color, EdPalette::BORDER,
    );

    // Info adicional en el modo hex (posición del byte)
    if ed.mode == EditorMode::Hex {
        const H: &[u8] = b"0123456789ABCDEF";
        let mut info = [0u8; 32]; let mut ip = 0;
        for b in b"Byte:0x" { if ip < 32 { info[ip] = *b; ip += 1; } }
        let mut tmp = [0u8; 16];
        let os = kfmt::fmt_u16(ed.cursor as u16, &mut tmp);
        for b in os.bytes() { if ip < 32 { info[ip] = b; ip += 1; } }
        for b in b" Val:0x" { if ip < 32 { info[ip] = *b; ip += 1; } }
        let byte = ed.buf[ed.cursor];
        if ip + 2 <= 32 {
            info[ip] = H[(byte >> 4) as usize]; ip += 1;
            info[ip] = H[(byte & 0xF) as usize]; ip += 1;
        }
        let info_x = (fw).saturating_sub(ip * cw + 12).max(x0);
        c.write_at_bg(
            core::str::from_utf8(&info[..ip]).unwrap_or(""),
            info_x, y + 2,
            EdPalette::OFFSET_FG, EdPalette::BORDER,
        );
    }
}