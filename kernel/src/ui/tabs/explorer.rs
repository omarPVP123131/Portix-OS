// ui/tabs/explorer.rs — PORTIX Kernel v0.7.4

#![allow(dead_code)]

use crate::drivers::input::keyboard::Key;
use crate::drivers::storage::fat32::{DirEntryInfo, Fat32Volume};
use crate::graphics::driver::framebuffer::{Color, Console, Layout};

// ── Paleta ────────────────────────────────────────────────────────────────────

pub struct ExpPal;
impl ExpPal {
    pub const BG:           Color = Color::new(0x08, 0x0C, 0x14);
    pub const PANEL_BG:     Color = Color::new(0x0A, 0x0F, 0x1C);
    pub const HEADER_BG:    Color = Color::new(0x05, 0x10, 0x28);
    pub const SELECTED_BG:  Color = Color::new(0x1A, 0x40, 0x80);
    pub const BORDER:       Color = Color::new(0x18, 0x28, 0x45);
    pub const SEP:          Color = Color::new(0x22, 0x38, 0x60);
    pub const TEXT_DIM:     Color = Color::new(0x60, 0x70, 0x90);
    pub const DIR_FG:       Color = Color::new(0xFF, 0xCC, 0x44);
    pub const FILE_FG:      Color = Color::new(0xA0, 0xC8, 0xFF);
    pub const SIZE_FG:      Color = Color::new(0x50, 0x80, 0xA0);
    pub const STATUS_BG:    Color = Color::new(0x10, 0x80, 0xFF);
    pub const PREVIEW_BG:   Color = Color::new(0x07, 0x0B, 0x12);
    pub const PREVIEW_FG:   Color = Color::new(0x88, 0xAA, 0xCC);
}

// ── Constantes ────────────────────────────────────────────────────────────────

const MAX_ENTRIES:    usize = 256;
const MAX_PATH_DEPTH: usize = 32;
const MAX_PATH_LEN:   usize = 512;
const PREVIEW_LINES:  usize = 6;
const PREVIEW_BYTES:  usize = 2048;

// ── Nodo de ruta ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PathNode {
    pub name:     [u8; 256],
    pub name_len: usize,
    pub cluster:  u32,
}

impl PathNode {
    pub const fn root(cluster: u32) -> Self {
        let mut name = [0u8; 256];
        name[0] = b'/';
        PathNode { name, name_len: 1, cluster }
    }
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

// ── Estado ───────────────────────────────────────────────────────────────────

pub struct ExplorerState {
    pub path_stack:    [PathNode; MAX_PATH_DEPTH],
    pub path_depth:    usize,
    pub entries:       [Option<DirEntryInfo>; MAX_ENTRIES],
    pub entry_count:   usize,
    pub selected:      usize,
    pub scroll:        usize,
    pub preview:       [u8; PREVIEW_BYTES],
    pub preview_len:   usize,
    pub preview_name:  [u8; 256],
    pub preview_nlen:  usize,
    pub status:        [u8; 80],
    pub status_len:    usize,
    pub status_ok:     bool,
    // Señal de apertura para el IDE
    pub open_request:  bool,
    pub open_cluster:  u32,
    pub open_name:     [u8; 256],
    pub open_name_len: usize,
    pub open_size:     u32,
    pub needs_refresh: bool,
}

impl ExplorerState {
    pub fn new(root_cluster: u32) -> Self {
        // Inicializar con const default para los arrays grandes
        const NONE_ENTRY: Option<DirEntryInfo> = None;
        const ROOT_NODE: PathNode = PathNode::root(0);
        let mut s = ExplorerState {
            path_stack:    [ROOT_NODE; MAX_PATH_DEPTH],
            path_depth:    1,
            entries:       [NONE_ENTRY; MAX_ENTRIES],
            entry_count:   0,
            selected:      0,
            scroll:        0,
            preview:       [0u8; PREVIEW_BYTES],
            preview_len:   0,
            preview_name:  [0u8; 256],
            preview_nlen:  0,
            status:        [0u8; 80],
            status_len:    0,
            status_ok:     true,
            open_request:  false,
            open_cluster:  0,
            open_name:     [0u8; 256],
            open_name_len: 0,
            open_size:     0,
            needs_refresh: true,
        };
        s.path_stack[0] = PathNode::root(root_cluster);
        s
    }

    pub fn current_cluster(&self) -> u32 {
        self.path_stack[self.path_depth.saturating_sub(1)].cluster
    }

    pub fn set_status(&mut self, msg: &str, ok: bool) {
        let n = msg.len().min(80);
        self.status[..n].copy_from_slice(&msg.as_bytes()[..n]);
        self.status_len = n;
        self.status_ok  = ok;
    }

    pub fn refresh(&mut self, vol: &Fat32Volume) {
        self.entry_count = 0;
        const NONE_ENTRY: Option<DirEntryInfo> = None;
        self.entries = [NONE_ENTRY; MAX_ENTRIES];
        let dir_clus = self.current_cluster();
        let mut count = 0usize;
        let entries_ref = &mut self.entries;
        let _ = vol.list_dir(dir_clus, |e| {
            let name = e.name_str();
            if name == "." || name == ".." { return; }
            if count < MAX_ENTRIES {
                entries_ref[count] = Some(e.clone());
                count += 1;
            }
        });
        self.entry_count = count;
        sort_entries(&mut self.entries, count);
        if self.selected >= count && count > 0 { self.selected = count - 1; }
        self.needs_refresh = false;
        self.set_status("Directorio cargado.", true);
    }

    pub fn selected_entry(&self) -> Option<&DirEntryInfo> {
        if self.selected < self.entry_count { self.entries[self.selected].as_ref() } else { None }
    }

    /// Entra al directorio seleccionado. Retorna true si se navegó.
    fn try_enter_dir(&mut self) -> bool {
        // Extraer datos sin mantener borrow de self
        let (is_dir, cluster, name_len, name) = if let Some(e) = self.selected_entry() {
            let mut n = [0u8; 256];
            n[..e.name_len].copy_from_slice(&e.name[..e.name_len]);
            (e.is_dir, e.cluster, e.name_len, n)
        } else {
            return false;
        };

        if is_dir && self.path_depth < MAX_PATH_DEPTH {
            self.path_stack[self.path_depth] = PathNode { name, name_len, cluster };
            self.path_depth  += 1;
            self.selected     = 0;
            self.scroll       = 0;
            self.needs_refresh = true;
            true
        } else {
            false
        }
    }

    /// Señal de apertura de archivo — extraer datos sin borrow mutable activo.
    fn try_open_file(&mut self) -> bool {
        let (cluster, size, name_len, name) = if let Some(e) = self.selected_entry() {
            if e.is_dir { return false; }
            let mut n = [0u8; 256];
            n[..e.name_len].copy_from_slice(&e.name[..e.name_len]);
            (e.cluster, e.size, e.name_len, n)
        } else {
            return false;
        };
        self.open_request  = true;
        self.open_cluster  = cluster;
        self.open_size     = size;
        self.open_name     = name;
        self.open_name_len = name_len;
        true
    }

    pub fn go_up(&mut self) {
        if self.path_depth > 1 {
            self.path_depth   -= 1;
            self.selected      = 0;
            self.scroll        = 0;
            self.needs_refresh = true;
        }
    }

    pub fn build_path(&self, out: &mut [u8]) -> usize {
        let mut p = 0usize;
        for i in 0..self.path_depth {
            let node = &self.path_stack[i];
            if i == 0 {
                if p < out.len() { out[p] = b'/'; p += 1; }
            } else {
                for b in node.name_str().bytes() { if p < out.len() { out[p] = b; p += 1; } }
                if p < out.len() { out[p] = b'/'; p += 1; }
            }
        }
        p
    }

    pub fn handle_key(&mut self, key: Key) -> bool {
        match key {
            Key::Up => {
                if self.selected > 0 { self.selected -= 1; }
                self.clamp_scroll();
                true
            }
            Key::Down => {
                if self.selected + 1 < self.entry_count { self.selected += 1; }
                self.clamp_scroll();
                true
            }
            Key::PageUp => {
                self.selected = self.selected.saturating_sub(10);
                self.clamp_scroll();
                true
            }
            Key::PageDown => {
                self.selected = (self.selected + 10).min(self.entry_count.saturating_sub(1));
                self.clamp_scroll();
                true
            }
            Key::Enter => {
                // Intentar entrar al directorio primero; si no, señalizar apertura
                if !self.try_enter_dir() {
                    self.try_open_file();
                }
                true
            }
            Key::Backspace => { self.go_up(); true }
            Key::F5 => { self.needs_refresh = true; true }
            _ => false,
        }
    }

    fn clamp_scroll(&mut self) {
        if self.selected < self.scroll { self.scroll = self.selected; }
    }
}

// ── Ordenación ────────────────────────────────────────────────────────────────

fn sort_entries(entries: &mut [Option<DirEntryInfo>; MAX_ENTRIES], count: usize) {
    for i in 0..count {
        for j in i + 1..count {
            let swap = match (&entries[i], &entries[j]) {
                (Some(a), Some(b)) => {
                    if a.is_dir && !b.is_dir { false }
                    else if !a.is_dir && b.is_dir { true }
                    else { name_gt(a, b) }
                }
                _ => false,
            };
            if swap { entries.swap(i, j); }
        }
    }
}

fn name_gt(a: &DirEntryInfo, b: &DirEntryInfo) -> bool {
    let la = a.name_len.min(16);
    let lb = b.name_len.min(16);
    for i in 0..la.min(lb) {
        let ca = a.name[i].to_ascii_lowercase();
        let cb = b.name[i].to_ascii_lowercase();
        if ca != cb { return ca > cb; }
    }
    la > lb
}

// ── Renderizado ───────────────────────────────────────────────────────────────

pub fn draw_explorer_tab(c: &mut Console, lay: &Layout, exp: &ExplorerState) {
    let fw  = lay.fw;
    let fh  = lay.fh;
    let cw  = lay.font_w;
    let ch  = lay.font_h;
    let lh  = ch + 3;
    let y0  = lay.content_y;

    c.fill_rect(0, y0, fw, fh - y0, ExpPal::BG);

    // Barra de ruta
    let header_h = ch + 8;
    c.fill_rect(0, y0, fw, header_h, ExpPal::HEADER_BG);
    c.hline(0, y0 + header_h - 1, fw, ExpPal::BORDER);

    let mut path_buf = [0u8; MAX_PATH_LEN];
    let path_len = exp.build_path(&mut path_buf);
    let path_str = core::str::from_utf8(&path_buf[..path_len]).unwrap_or("/");
    c.write_at(path_str, 6 + 2 * cw + 4, y0 + 4, Color::WHITE);
    c.write_at(
        "Enter=Abrir  Bksp=Subir  F5=Refresh",
        fw.saturating_sub(38 * cw), y0 + 4,
        Color::new(0x30, 0x50, 0x80),
    );

    let content_y  = y0 + header_h;
    let status_h   = ch + 6;
    let preview_h  = PREVIEW_LINES * lh + 4;
    let list_h     = fh.saturating_sub(content_y + status_h + preview_h);
    let visible    = (list_h / lh).max(1);

    // Cabecera columnas
    c.fill_rect(0, content_y, fw, lh, Color::new(0x0C, 0x14, 0x24));
    let size_col_w = 10 * cw;
    let name_col_w = fw.saturating_sub(size_col_w + 4);
    c.write_at("Nombre", 10, content_y + 2, Color::new(0x60, 0x80, 0xC0));
    c.write_at("Tamano", name_col_w + 4, content_y + 2, Color::new(0x60, 0x80, 0xC0));
    c.hline(0, content_y + lh - 1, fw, ExpPal::BORDER);

    let list_start_y = content_y + lh;

    // Scroll ajustado
    let scroll = if exp.selected < exp.scroll {
        exp.selected
    } else if exp.selected >= exp.scroll + visible {
        exp.selected + 1 - visible
    } else {
        exp.scroll
    };

    for vis in 0..visible {
        let idx = scroll + vis;
        if idx >= exp.entry_count { break; }
        let py  = list_start_y + vis * lh;
        let is_sel = idx == exp.selected;
        let bg  = if is_sel { ExpPal::SELECTED_BG }
                  else if vis % 2 == 0 { ExpPal::PANEL_BG }
                  else { ExpPal::BG };
        c.fill_rect(0, py, fw, lh, bg);

        if let Some(entry) = &exp.entries[idx] {
            let (icon, fg) = if entry.is_dir { ("[D]", ExpPal::DIR_FG) } else { ("[F]", ExpPal::FILE_FG) };
            c.write_at(icon, 4, py + 2, fg);

            let name = entry.name_str();
            let max_chars = (name_col_w.saturating_sub(40)) / cw;
            let display = if name.len() > max_chars { &name[..max_chars] } else { name };
            c.write_at(display, 4 + 4 * cw, py + 2, if is_sel { Color::WHITE } else { fg });

            if !entry.is_dir {
                let mut sb = [0u8; 16];
                let ss = fmt_size(entry.size, &mut sb);
                c.write_at(ss, name_col_w + 4, py + 2, ExpPal::SIZE_FG);
            } else {
                c.write_at("<DIR>", name_col_w + 4, py + 2, ExpPal::DIR_FG);
            }
        }
    }

    // Vista previa
    let preview_y = list_start_y + visible * lh;
    c.hline(0, preview_y, fw, ExpPal::SEP);
    let prev_y = preview_y + 2;
    c.fill_rect(0, prev_y, fw, preview_h, ExpPal::PREVIEW_BG);

    let prev_name = core::str::from_utf8(&exp.preview_name[..exp.preview_nlen]).unwrap_or("");
    if exp.preview_len > 0 {
        c.write_at("Vista previa: ", 6, prev_y + 2, ExpPal::TEXT_DIM);
        c.write_at(prev_name, 6 + 15 * cw, prev_y + 2, Color::WHITE);
        let mut ls = 0usize; let mut ln = 0usize;
        let data = &exp.preview[..exp.preview_len];
        for i in 0..=data.len() {
            if (i == data.len() || data[i] == b'\n') && ln < PREVIEW_LINES {
                let bytes = &data[ls..i];
                let mc = (fw.saturating_sub(12)) / cw;
                let disp = &bytes[..bytes.len().min(mc)];
                if let Ok(s) = core::str::from_utf8(disp) {
                    c.write_at(s, 6, prev_y + (ln + 1) * lh + 2, ExpPal::PREVIEW_FG);
                }
                ln += 1; ls = i + 1;
            }
        }
    } else {
        c.write_at("Selecciona un archivo para vista previa.", 6, prev_y + lh, ExpPal::TEXT_DIM);
    }

    // Status bar
    let sy = fh.saturating_sub(status_h);
    let st_bg = if exp.status_ok { ExpPal::STATUS_BG } else { Color::new(0x80, 0x10, 0x10) };
    c.fill_rect(0, sy, fw, status_h, st_bg);
    let status = core::str::from_utf8(&exp.status[..exp.status_len]).unwrap_or("");
    c.write_at(status, 8, sy + 3, Color::WHITE);

    // Contador — construir el string en un buffer separado para evitar borrow conflict
    {
        let mut cnt_buf = [0u8; 32];
        let mut cp = 0usize;
        let mut tmp = [0u8; 8];
        let ns = fmt_usize_local(exp.entry_count, &mut tmp);
        for b in ns.bytes() { if cp < 20 { cnt_buf[cp] = b; cp += 1; } }
        for b in b" elementos" { if cp < 32 { cnt_buf[cp] = *b; cp += 1; } }
        // cp ya no toca cnt_buf después de aquí
        let cnt_str = core::str::from_utf8(&cnt_buf[..cp]).unwrap_or("");
        c.write_at(cnt_str, fw.saturating_sub(18 * cw), sy + 3, Color::WHITE);
    }

    // Scrollbar
    if exp.entry_count > visible {
        let sb_x = fw - 8;
        c.fill_rect(sb_x, list_start_y, 6, list_h, ExpPal::BORDER);
        let thumb_h = (list_h * visible / exp.entry_count).max(6);
        let thumb_y = list_start_y + (scroll * list_h) / exp.entry_count;
        c.fill_rect(sb_x + 1, thumb_y, 4, thumb_h, Color::new(0x20, 0x60, 0xC0));
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn fmt_size(bytes: u32, buf: &mut [u8; 16]) -> &str {
    let mut p = 0usize;
    let mut tmp = [0u8; 8];
    if bytes < 1024 {
        let s = fmt_usize_local(bytes as usize, &mut tmp);
        for b in s.bytes() { if p < 12 { buf[p] = b; p += 1; } }
        for b in b" B" { if p < 14 { buf[p] = *b; p += 1; } }
    } else if bytes < 1024 * 1024 {
        let s = fmt_usize_local((bytes / 1024) as usize, &mut tmp);
        for b in s.bytes() { buf[p] = b; p += 1; }
        for b in b" KiB" { buf[p] = *b; p += 1; }
    } else {
        let s = fmt_usize_local((bytes / (1024 * 1024)) as usize, &mut tmp);
        for b in s.bytes() { buf[p] = b; p += 1; }
        for b in b" MiB" { buf[p] = *b; p += 1; }
    }
    core::str::from_utf8(&buf[..p]).unwrap_or("?")
}

fn fmt_usize_local(mut n: usize, buf: &mut [u8]) -> &str {
    let mut i = buf.len();
    if i == 0 { return ""; }
    if n == 0 { buf[i-1] = b'0'; return core::str::from_utf8(&buf[i-1..]).unwrap_or("0"); }
    while n > 0 && i > 0 { i -= 1; buf[i] = b'0' + (n % 10) as u8; n /= 10; }
    core::str::from_utf8(&buf[i..]).unwrap_or("?")
}