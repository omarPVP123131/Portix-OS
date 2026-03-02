// ui/tabs/ide.rs — PORTIX Kernel v0.7.4
//
// IDE de texto con syntax highlighting + barra de menú tipo IDE real.
//
// LAYOUT INTERNO (dentro del área content_y..bottom_y):
//
//   [MENU_BAR_H = 20px]  → Archivo | Editar | Ver | Ayuda
//   [FILE_TABS_H = 22px] → pestañas de buffers abiertos
//   [edit area]          → editor con gutter
//   [STATUS_H = 18px]    → Ln/Col · Lang · nombre · hint de teclas
//
// FIX CRÍTICO: PAGE_POOL usa MaybeUninit → .bss (ver comentario original).

#![allow(dead_code)]

use core::mem::MaybeUninit;
use crate::drivers::input::keyboard::Key;
use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::ui::input::{InputBox, InputMode, draw_input_overlay, INPUT_BG};

// ─────────────────────────────────────────────────────────────────────────────
// Paleta IDE
// ─────────────────────────────────────────────────────────────────────────────

pub struct IdePal;
impl IdePal {
    // Fondos
    pub const BG:             Color = Color::new(0x0D, 0x11, 0x1A);
    pub const GUTTER_BG:      Color = Color::new(0x08, 0x0B, 0x12);
    pub const MENUBAR_BG:     Color = Color::new(0x05, 0x0E, 0x1E);
    pub const MENUBAR_BORDER: Color = Color::new(0x18, 0x2C, 0x4A);
    pub const FILETAB_BG:     Color = Color::new(0x08, 0x10, 0x1E);
    pub const FILETAB_ACT:    Color = Color::new(0x10, 0x24, 0x42);
    pub const FILETAB_HOV:    Color = Color::new(0x0C, 0x1C, 0x32);
    pub const DROPDOWN_BG:    Color = Color::new(0x0A, 0x16, 0x2E);
    pub const DROPDOWN_BOR:   Color = Color::new(0x28, 0x44, 0x70);
    pub const DROPDOWN_HOV:   Color = Color::new(0x16, 0x30, 0x5A);
    pub const DROPDOWN_SEP:   Color = Color::new(0x18, 0x28, 0x44);
    pub const STATUS_BG:      Color = Color::new(0x0C, 0x78, 0xE8);
    pub const STATUS_ERR:     Color = Color::new(0x88, 0x10, 0x10);
    // Texto
    pub const TEXT:           Color = Color::new(0xCC, 0xD4, 0xE0);
    pub const LINE_NUM:       Color = Color::new(0x36, 0x3E, 0x54);
    pub const MENU_FG:        Color = Color::new(0xB8, 0xC8, 0xE0);
    pub const MENU_FG_ACT:    Color = Color::new(0xFF, 0xD7, 0x00);
    pub const MENU_ACCEL:     Color = Color::new(0x88, 0xAA, 0xFF);
    pub const MENU_SHORTCUT:  Color = Color::new(0x60, 0x80, 0xAA);
    pub const FILETAB_FG:     Color = Color::new(0x70, 0x88, 0xAA);
    pub const FILETAB_FG_ACT: Color = Color::new(0xFF, 0xFF, 0xFF);
    // Editor
    pub const CURSOR_LINE:    Color = Color::new(0x14, 0x1E, 0x32);
    pub const CURSOR_BG:      Color = Color::new(0xFF, 0xB0, 0x00);
    pub const CURSOR_FG:      Color = Color::new(0x00, 0x00, 0x00);
    pub const DIRTY_DOT:      Color = Color::new(0xFF, 0x55, 0x00);
    pub const BORDER:         Color = Color::new(0x1C, 0x2E, 0x48);
    pub const GUTTER_BORDER:  Color = Color::new(0x22, 0x38, 0x60);
    // Highlight
    pub const SYN_KEYWORD:    Color = Color::new(0x56, 0x9C, 0xD6);
    pub const SYN_STRING:     Color = Color::new(0xCE, 0x91, 0x78);
    pub const SYN_COMMENT:    Color = Color::new(0x6A, 0x99, 0x55);
    pub const SYN_NUMBER:     Color = Color::new(0xB5, 0xCE, 0xA8);
    pub const SYN_TYPE:       Color = Color::new(0x4E, 0xC9, 0xB0);
    pub const SYN_MACRO:      Color = Color::new(0xBD, 0x63, 0xC5);
    pub const SYN_PUNCT:      Color = Color::new(0xFF, 0xD7, 0x00);
}

// ─────────────────────────────────────────────────────────────────────────────
// Submenú — items que muestra cada menú desplegable
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum MenuAction {
    None,
    // Archivo
    FileNew,
    FileOpen,
    FileSave,
    FileSaveAs,
    FileClose,
    // Editar
    EditUndo,
    EditSelectAll,
    EditGoToLine,
    // Ver
    ViewLineNumbers,
    ViewWordWrap,
    // Ayuda
    HelpAbout,
    // Separador (no ejecuta nada)
    Separator,
}

#[derive(Clone, Copy)]
pub struct MenuItem {
    pub label:    &'static str,
    pub shortcut: &'static str,
    pub action:   MenuAction,
}

impl MenuItem {
    const fn new(label: &'static str, shortcut: &'static str, action: MenuAction) -> Self {
        MenuItem { label, shortcut, action }
    }
    const fn sep() -> Self {
        MenuItem { label: "─────────────────", shortcut: "", action: MenuAction::Separator }
    }
}

const MENU_ARCHIVO: &[MenuItem] = &[
    MenuItem::new("Nuevo",           "Ctrl+N", MenuAction::FileNew),
    MenuItem::new("Abrir...",        "Ctrl+O", MenuAction::FileOpen),
    MenuItem::sep(),
    MenuItem::new("Guardar",         "Ctrl+S", MenuAction::FileSave),
    MenuItem::new("Guardar como...", "",       MenuAction::FileSaveAs),
    MenuItem::sep(),
    MenuItem::new("Cerrar",          "Ctrl+W", MenuAction::FileClose),
];

const MENU_EDITAR: &[MenuItem] = &[
    MenuItem::new("Deshacer",        "Ctrl+Z", MenuAction::EditUndo),
    MenuItem::sep(),
    MenuItem::new("Selec. todo",     "Ctrl+A", MenuAction::EditSelectAll),
    MenuItem::new("Ir a línea...",   "Ctrl+G", MenuAction::EditGoToLine),
];

const MENU_VER: &[MenuItem] = &[
    MenuItem::new("Núm. de línea",   "",       MenuAction::ViewLineNumbers),
    MenuItem::new("Ajuste de línea", "",       MenuAction::ViewWordWrap),
];

const MENU_AYUDA: &[MenuItem] = &[
    MenuItem::new("Acerca de PORTIX","",       MenuAction::HelpAbout),
];

#[derive(Clone, Copy)]
pub struct MenuDef {
    pub title: &'static str,
    pub items: &'static [MenuItem],
}

pub const MENUS: &[MenuDef] = &[
    MenuDef { title: "Archivo", items: MENU_ARCHIVO },
    MenuDef { title: "Editar",  items: MENU_EDITAR  },
    MenuDef { title: "Ver",     items: MENU_VER     },
    MenuDef { title: "Ayuda",   items: MENU_AYUDA   },
];

// ─────────────────────────────────────────────────────────────────────────────
// Estado del menú
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum MenuState {
    Closed,
    Open(usize),   // índice del menú abierto
}

// ─────────────────────────────────────────────────────────────────────────────
// Lenguaje
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum Lang { Plain, Rust, C, Asm }

impl Lang {
    pub fn from_name(name: &str) -> Self {
        if name.ends_with(".rs")                                { Lang::Rust }
        else if name.ends_with(".c") || name.ends_with(".h")   { Lang::C }
        else if name.ends_with(".asm") || name.ends_with(".s") { Lang::Asm }
        else                                                    { Lang::Plain }
    }
    pub fn label(self) -> &'static str {
        match self { Lang::Rust=>"Rust", Lang::C=>"C", Lang::Asm=>"ASM", Lang::Plain=>"TXT" }
    }
    pub fn icon(self) -> &'static str {
        match self { Lang::Rust=>"[rs]", Lang::C=>"[ c]", Lang::Asm=>"[as]", Lang::Plain=>"[tx]" }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Buffer paginado (sin alloc) — idéntico al original
// ─────────────────────────────────────────────────────────────────────────────

const MAX_LINES:       usize = 4096;
const MAX_LINE_LEN:    usize = 512;
const MAX_BUFFERS:     usize = 8;
const PAGE_LINES:      usize = 64;
const MAX_PAGES_TOTAL: usize = 64;

#[derive(Clone, Copy)]
pub struct Line {
    pub data: [u8; MAX_LINE_LEN],
    pub len:  usize,
}

impl Line {
    pub const fn empty() -> Self { Line { data: [0u8; MAX_LINE_LEN], len: 0 } }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("")
    }
    pub fn insert(&mut self, col: usize, byte: u8) -> bool {
        if self.len >= MAX_LINE_LEN { return false; }
        let col = col.min(self.len);
        self.data.copy_within(col..self.len, col + 1);
        self.data[col] = byte;
        self.len += 1;
        true
    }
    pub fn remove(&mut self, col: usize) -> bool {
        if col >= self.len { return false; }
        self.data.copy_within(col + 1..self.len, col);
        self.len -= 1;
        true
    }
}

#[derive(Clone, Copy)]
struct Page {
    lines: [Line; PAGE_LINES],
    used:  bool,
    next:  i32,
    prev:  i32,
    count: usize,
}

// SAFETY: kernel bare-metal, single-threaded. No existe concurrencia.
// Usamos raw pointers en lugar de &mut static para satisfacer Rust 2024.
static mut PAGE_POOL: MaybeUninit<[Page; MAX_PAGES_TOTAL]> = MaybeUninit::uninit();

/// Puntero mutable al primer Page del pool.
#[inline(always)]
unsafe fn pool_raw() -> *mut Page {
    core::ptr::addr_of_mut!(PAGE_POOL) as *mut Page
}

/// Puntero constante al primer Page del pool.
#[inline(always)]
unsafe fn pool_raw_const() -> *const Page {
    core::ptr::addr_of!(PAGE_POOL) as *const Page
}

pub fn init_page_pool() {
    unsafe {
        for i in 0..MAX_PAGES_TOTAL {
            let p = &mut *pool_raw().add(i);
            p.used  = false;
            p.next  = -1;
            p.prev  = -1;
            p.count = 0;
        }
    }
}

unsafe fn alloc_page() -> Option<usize> {
    for i in 0..MAX_PAGES_TOTAL {
        let p = &mut *pool_raw().add(i);
        if !p.used {
            p.used  = true;
            p.next  = -1;
            p.prev  = -1;
            p.count = 0;
            for j in 0..PAGE_LINES { p.lines[j] = Line::empty(); }
            return Some(i);
        }
    }
    None
}

unsafe fn free_page(idx: usize) {
    if idx < MAX_PAGES_TOTAL {
        let p = &mut *pool_raw().add(idx);
        p.used  = false;
        p.next  = -1;
        p.prev  = -1;
        p.count = 0;
    }
}

#[inline(always)]
unsafe fn page_mut(idx: usize) -> &'static mut Page { &mut *pool_raw().add(idx) }

#[inline(always)]
unsafe fn page_ref(idx: usize) -> &'static Page { &*pool_raw_const().add(idx) }

// ─────────────────────────────────────────────────────────────────────────────
// TextBuffer (sin cambios respecto al original salvo nombres)
// ─────────────────────────────────────────────────────────────────────────────

pub struct TextBuffer {
    pub head_page: i32,
    pub tail_page: i32,
    pub page_cnt:  usize,
    pub line_cnt:  usize,
    pub name:      [u8; 256],
    pub name_len:  usize,
    pub lang:      Lang,
    pub dirty:     bool,
    pub cursor_l:  usize,
    pub cursor_c:  usize,
    pub scroll:    usize,
    pub show_ln:   bool,  // NUEVO: mostrar números de línea
}

impl TextBuffer {
    pub fn new_empty(name: &str) -> Self {
        let lang = Lang::from_name(name);
        let mut head = -1i32;
        unsafe {
            if let Some(pi) = alloc_page() { head = pi as i32; }
        }
        let mut tb = TextBuffer {
            head_page: head, tail_page: head,
            page_cnt:  if head >= 0 { 1 } else { 0 },
            line_cnt:  1,
            name:      [0u8; 256], name_len: 0,
            lang,
            dirty:     false,
            cursor_l:  0, cursor_c: 0, scroll: 0,
            show_ln:   true,
        };
        let n = name.len().min(255);
        tb.name[..n].copy_from_slice(name.as_bytes());
        tb.name_len = n;
        if tb.head_page >= 0 {
            unsafe {
                let p = page_mut(tb.head_page as usize);
                p.count = 1;
                p.lines[0] = Line::empty();
            }
        }
        tb
    }

    pub fn load_text(&mut self, data: &[u8]) {
        self.clear_pages();
        if self.head_page < 0 {
            unsafe {
                if let Some(pi) = alloc_page() {
                    self.head_page = pi as i32;
                    self.tail_page = pi as i32;
                    self.page_cnt  = 1;
                }
            }
        }
        let mut cur_line_idx: usize = 0;
        self.line_cnt = 1; self.cursor_l = 0; self.cursor_c = 0; self.scroll = 0;
        unsafe {
            let p = page_mut(self.head_page as usize);
            p.count = 1; p.lines[0] = Line::empty();
        }
        for &b in data {
            if b == b'\n' {
                cur_line_idx = cur_line_idx.saturating_add(1);
                self.line_cnt = self.line_cnt.saturating_add(1);
                let (pidx, slot) = self.ensure_slot_for_line(cur_line_idx);
                unsafe { page_mut(pidx).lines[slot] = Line::empty(); }
            } else if b != b'\r' {
                let (pidx, slot) = self.ensure_slot_for_line(cur_line_idx);
                unsafe {
                    let p = page_mut(pidx);
                    let _ = p.lines[slot].insert(p.lines[slot].len, b);
                }
            }
        }
        self.cursor_l = 0; self.cursor_c = 0; self.dirty = false;
    }

    pub fn serialize(&self, out: &mut [u8]) -> usize {
        let mut ppos = 0usize;
        for li in 0..self.line_cnt {
            if let Some(line) = self.get_line(li) {
                let n = line.len.min(out.len().saturating_sub(ppos));
                out[ppos..ppos + n].copy_from_slice(&line.data[..n]);
                ppos += n;
            }
            if li + 1 < self.line_cnt && ppos < out.len() {
                out[ppos] = b'\n'; ppos += 1;
            }
        }
        ppos
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("untitled")
    }

    fn cur_line_len(&self) -> usize {
        self.get_line(self.cursor_l).map(|l| l.len).unwrap_or(0)
    }
    fn clamp_col(&mut self) {
        let max = self.get_line(self.cursor_l).map(|l| l.len).unwrap_or(0);
        if self.cursor_c > max { self.cursor_c = max; }
    }
    fn ensure_scroll(&mut self, visible_rows: usize) {
        if self.cursor_l < self.scroll { self.scroll = self.cursor_l; }
        else if self.cursor_l >= self.scroll + visible_rows {
            self.scroll = self.cursor_l + 1 - visible_rows;
        }
    }

    fn insert_char(&mut self, ch: u8) {
        let cur_c = self.cursor_c;
        let inserted = if let Some(line) = self.get_line_mut(self.cursor_l) {
            line.insert(cur_c, ch)
        } else { false };
        if inserted { self.cursor_c = self.cursor_c.saturating_add(1); self.dirty = true; return; }
        self.insert_newline();
        let cur_c2 = self.cursor_c;
        if let Some(line) = self.get_line_mut(self.cursor_l) {
            let _ = line.insert(cur_c2, ch);
            self.cursor_c = self.cursor_c.saturating_add(1); self.dirty = true;
        }
    }

    fn insert_newline(&mut self) {
        let l = self.cursor_l;
        if self.line_cnt >= MAX_LINES { return; }
        let mut cur = Line::empty();
        if let Some(e) = self.get_line(l) { cur = *e; }
        let split_at = self.cursor_c.min(cur.len);
        let old_len  = cur.len;
        let mut new_line = Line::empty();
        let tail_len = old_len.saturating_sub(split_at);
        if tail_len > 0 {
            new_line.data[..tail_len].copy_from_slice(&cur.data[split_at..old_len]);
            new_line.len = tail_len;
        }
        if let Some(cm) = self.get_line_mut(l) { cm.len = split_at; }
        self.insert_line_at(l + 1, new_line);
        self.line_cnt  = self.line_cnt.saturating_add(1);
        self.cursor_l  = self.cursor_l.saturating_add(1);
        self.cursor_c  = 0; self.dirty = true;
    }

    fn backspace(&mut self) {
        let cur_c = self.cursor_c;
        if cur_c > 0 {
            let l = self.cursor_l;
            if let Some(line) = self.get_line_mut(l) { let _ = line.remove(cur_c - 1); }
            self.cursor_c = cur_c - 1; self.dirty = true; return;
        }
        if self.cursor_l > 0 {
            let prev = self.cursor_l - 1;
            let cur  = self.cursor_l;
            let mut cur_line = Line::empty();
            if let Some(c) = self.get_line(cur) { cur_line = *c; }
            let prev_len = self.get_line(prev).map(|l| l.len).unwrap_or(0);
            let copy_len = cur_line.len.min(MAX_LINE_LEN.saturating_sub(prev_len));
            if copy_len > 0 {
                if let Some(pm) = self.get_line_mut(prev) {
                    pm.data[prev_len..prev_len + copy_len].copy_from_slice(&cur_line.data[..copy_len]);
                    pm.len = prev_len + copy_len;
                }
            }
            self.delete_line_at(cur);
            self.line_cnt  = self.line_cnt.saturating_sub(1);
            self.cursor_l  = self.cursor_l.saturating_sub(1);
            self.cursor_c  = prev_len; self.dirty = true;
        }
    }

    fn delete_forward(&mut self) {
        let l = self.cursor_l; let cur_c = self.cursor_c;
        if let Some(line) = self.get_line_mut(l) {
            if cur_c < line.len { line.remove(cur_c); self.dirty = true; return; }
        }
        if l + 1 < self.line_cnt {
            let next_idx = l + 1;
            let mut next_line = Line::empty();
            if let Some(nl) = self.get_line(next_idx) { next_line = *nl; }
            let cur_len  = self.get_line(l).map(|l| l.len).unwrap_or(0);
            let copy_len = next_line.len.min(MAX_LINE_LEN.saturating_sub(cur_len));
            if copy_len > 0 {
                if let Some(cm) = self.get_line_mut(l) {
                    cm.data[cur_len..cur_len + copy_len].copy_from_slice(&next_line.data[..copy_len]);
                    cm.len = cur_len + copy_len;
                }
            }
            self.delete_line_at(next_idx);
            self.line_cnt = self.line_cnt.saturating_sub(1); self.dirty = true;
        }
    }

    // ── Helpers de paginación (idénticos al original) ─────────────────────────

    fn find_page_for_line(&self, line_idx: usize) -> Option<(usize, usize)> {
        if self.head_page < 0 { return None; }
        let mut pidx = self.head_page as usize;
        let mut acc  = 0usize;
        unsafe {
            loop {
                let p = page_ref(pidx);
                if acc + p.count > line_idx { return Some((pidx, line_idx - acc)); }
                acc += p.count;
                if p.next < 0 { break; }
                pidx = p.next as usize;
            }
        }
        None
    }

    fn ensure_slot_for_line(&mut self, line_idx: usize) -> (usize, usize) {
        if line_idx >= self.line_cnt {
            while self.line_cnt <= line_idx { self.append_empty_line(); }
        }
        if let Some((pidx, off)) = self.find_page_for_line(line_idx) { return (pidx, off); }
        unsafe {
            if let Some(pi) = alloc_page() {
                let pi = pi as usize;
                if self.tail_page >= 0 {
                    let t = page_mut(self.tail_page as usize);
                    t.next = pi as i32;
                    page_mut(pi).prev = self.tail_page;
                    self.tail_page = pi as i32;
                } else {
                    self.head_page = pi as i32; self.tail_page = pi as i32;
                }
                self.page_cnt = self.page_cnt.saturating_add(1);
                page_mut(pi).count = 1; page_mut(pi).lines[0] = Line::empty();
                return (pi, 0);
            } else { return (self.head_page as usize, 0); }
        }
    }

    fn append_empty_line(&mut self) {
        if self.head_page < 0 {
            unsafe {
                if let Some(pi) = alloc_page() {
                    self.head_page = pi as i32; self.tail_page = pi as i32; self.page_cnt = 1;
                }
            }
        }
        unsafe {
            let tail = self.tail_page as usize;
            let p = page_mut(tail);
            if p.count < PAGE_LINES {
                p.lines[p.count] = Line::empty(); p.count += 1;
            } else if let Some(pi) = alloc_page() {
                let pi = pi as usize;
                page_mut(pi).count = 1; page_mut(pi).lines[0] = Line::empty();
                p.next = pi as i32; page_mut(pi).prev = self.tail_page;
                self.tail_page = pi as i32;
                self.page_cnt = self.page_cnt.saturating_add(1);
            }
        }
        self.line_cnt = self.line_cnt.saturating_add(1);
    }

    fn get_line(&self, line_idx: usize) -> Option<&Line> {
        self.find_page_for_line(line_idx).map(|(pidx, off)| unsafe {
            &page_ref(pidx).lines[off]
        })
    }
    fn get_line_mut(&mut self, line_idx: usize) -> Option<&mut Line> {
        if let Some((pidx, off)) = self.find_page_for_line(line_idx) {
            unsafe { Some(&mut page_mut(pidx).lines[off]) }
        } else { None }
    }

    fn insert_line_at(&mut self, at: usize, line: Line) {
        if at > self.line_cnt { return; }
        if at == self.line_cnt { self.append_empty_line(); if let Some(d) = self.get_line_mut(at) { *d = line; } return; }
        self.append_empty_line();
        if self.line_cnt < 2 { if let Some(d) = self.get_line_mut(at) { *d = line; } return; }
        let mut i = self.line_cnt.saturating_sub(2);
        loop {
            if i < at { break; }
            if let Some(src) = self.get_line(i) {
                let tmp = *src;
                if let Some(d) = self.get_line_mut(i + 1) { *d = tmp; }
            }
            if i == 0 { break; } i -= 1;
        }
        if let Some(d) = self.get_line_mut(at) { *d = line; }
    }

    fn delete_line_at(&mut self, at: usize) {
        if at >= self.line_cnt { return; }
        let mut i = at;
        while i + 1 < self.line_cnt {
            if let Some(src) = self.get_line(i + 1) {
                let tmp = *src;
                if let Some(d) = self.get_line_mut(i) { *d = tmp; }
            }
            i += 1;
        }
        self.remove_last_line_slot();
    }

    fn remove_last_line_slot(&mut self) {
        if self.line_cnt == 0 { return; }
        let last_idx = self.line_cnt - 1;
        if let Some((pidx, off)) = self.find_page_for_line(last_idx) {
            unsafe {
                let p = page_mut(pidx);
                if p.count > 0 {
                    p.lines[off] = Line::empty();
                    p.count = p.count.saturating_sub(1);
                    if p.count == 0 {
                        let prev = p.prev; let next = p.next;
                        if prev >= 0 { page_mut(prev as usize).next = next; }
                        else { self.head_page = if next >= 0 { next } else { -1 }; }
                        if next >= 0 { page_mut(next as usize).prev = prev; }
                        else { self.tail_page = if prev >= 0 { prev } else { -1 }; }
                        free_page(pidx);
                        self.page_cnt = self.page_cnt.saturating_sub(1);
                    }
                }
            }
        }
        self.line_cnt = self.line_cnt.saturating_sub(1);
    }

    pub fn clear_pages(&mut self) {
        if self.head_page < 0 { return; }
        let mut cur = self.head_page as i32;
        while cur >= 0 {
            let next = unsafe { page_ref(cur as usize).next };
            unsafe { free_page(cur as usize); }
            cur = next;
        }
        self.head_page = -1; self.tail_page = -1; self.page_cnt = 0; self.line_cnt = 0;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// IdeState — ahora con MenuState y show_line_numbers
// ─────────────────────────────────────────────────────────────────────────────

pub struct IdeState {
    pub buffers:       [Option<TextBuffer>; MAX_BUFFERS],
    pub active:        usize,
    pub buf_count:     usize,
    pub status_msg:    [u8; 80],
    pub status_len:    usize,
    pub status_err:    bool,
    pub menu:          MenuState,
    pub show_ln:       bool,
    // Input inline (Guardar como, Abrir, Ir a línea...)
    pub input:         InputBox,
    // Ruta del archivo activo
    pub save_path:     [u8; 256],
    pub save_plen:     usize,
}

impl IdeState {
    pub fn new() -> Self {
        let mut ide = IdeState {
            buffers:    core::array::from_fn(|_| None),
            active:     0, buf_count: 0,
            status_msg: [0u8; 80], status_len: 0, status_err: false,
            menu:       MenuState::Closed,
            show_ln:    true,
            input:      InputBox::new(),
            save_path:  [0u8; 256],
            save_plen:  0,
        };
        ide.open_new("untitled.txt");
        ide
    }

    pub fn open_new(&mut self, name: &str) -> bool {
        if self.buf_count >= MAX_BUFFERS { return false; }
        for i in 0..MAX_BUFFERS {
            if self.buffers[i].is_none() {
                self.buffers[i] = Some(TextBuffer::new_empty(name));
                self.active     = i; self.buf_count += 1;
                self.set_status("Nuevo archivo creado.", false); return true;
            }
        }
        false
    }

    pub fn open_with_data(&mut self, name: &str, data: &[u8]) -> bool {
        if self.buf_count >= MAX_BUFFERS { return false; }
        for i in 0..MAX_BUFFERS {
            if self.buffers[i].is_none() {
                let mut buf = TextBuffer::new_empty(name);
                buf.load_text(data);
                self.buffers[i] = Some(buf);
                self.active     = i; self.buf_count += 1;
                self.set_status("Archivo abierto.", false); return true;
            }
        }
        false
    }

    pub fn close_active(&mut self) {
        if let Some(mut buf) = self.buffers[self.active].take() { buf.clear_pages(); }
        if self.buf_count > 0 { self.buf_count -= 1; }
        for i in 0..MAX_BUFFERS { if self.buffers[i].is_some() { self.active = i; return; } }
        self.active = 0; self.open_new("untitled.txt");
    }

    pub fn switch_next(&mut self) {
        let mut i = (self.active + 1) % MAX_BUFFERS;
        for _ in 0..MAX_BUFFERS {
            if self.buffers[i].is_some() { self.active = i; return; }
            i = (i + 1) % MAX_BUFFERS;
        }
    }

    pub fn switch_prev(&mut self) {
        let mut i = if self.active == 0 { MAX_BUFFERS - 1 } else { self.active - 1 };
        for _ in 0..MAX_BUFFERS {
            if self.buffers[i].is_some() { self.active = i; return; }
            i = if i == 0 { MAX_BUFFERS - 1 } else { i - 1 };
        }
    }

    pub fn set_status(&mut self, msg: &str, is_err: bool) {
        let n = msg.len().min(80);
        self.status_msg[..n].copy_from_slice(msg.as_bytes());
        self.status_len = n; self.status_err = is_err;
    }

    /// Ejecuta una acción de menú. Devuelve true si consumió el evento.
    pub fn execute_menu(&mut self, action: MenuAction) -> bool {
        self.menu = MenuState::Closed;
        match action {
            MenuAction::FileNew => {
                self.open_new("untitled.txt");
                self.set_status("Nuevo archivo creado.", false);
            }
            MenuAction::FileOpen => {
                // Activar input para escribir nombre de archivo a abrir
                self.input.start(InputMode::SaveAs, "");
                self.set_status("Nombre del archivo a abrir (Enter=OK, Esc=Cancelar):", false);
                // Nota: necesitas navegar el explorer para abrir; esto es acceso directo por nombre
            }
            MenuAction::FileSave => {
                if self.save_plen == 0 {
                    // Sin ruta — pedir nombre
                    let name = if let Some(buf) = &self.buffers[self.active] { buf.name_str() } else { "untitled.txt" };
                    let prefill = name;
                    self.input.start(InputMode::SaveAs, prefill);
                    self.set_status("Nombre del archivo (Enter=OK, Esc=Cancelar):", false);
                } else {
                    // Ya tiene ruta — guardar silenciosamente
                    if let Some(buf) = self.buffers[self.active].as_mut() { buf.dirty = false; }
                    self.set_status("Guardado. (Escribe en FAT32 via vol.write_file)", false);
                }
            }
            MenuAction::FileSaveAs => {
                let name = if let Some(buf) = &self.buffers[self.active] { buf.name_str() } else { "untitled.txt" };
                self.input.start(InputMode::SaveAs, name);
                self.set_status("Guardar como... (Enter=OK, Esc=Cancelar):", false);
            }
            MenuAction::FileClose => {
                self.close_active();
                self.set_status("Archivo cerrado.", false);
            }
            MenuAction::EditUndo => {
                self.set_status("Deshacer: no implementado aún.", true);
            }
            MenuAction::EditSelectAll => {
                self.set_status("Selec. todo: no implementado aún.", true);
            }
            MenuAction::EditGoToLine => {
                self.input.start(InputMode::SaveAs, "");
                self.set_status("Ir a línea... (número + Enter):", false);
            }
            MenuAction::ViewLineNumbers => {
                self.show_ln = !self.show_ln;
                self.set_status(if self.show_ln { "Números de línea: ON" } else { "Números de línea: OFF" }, false);
            }
            MenuAction::ViewWordWrap => {
                self.set_status("Ajuste de línea: no implementado aún.", true);
            }
            MenuAction::HelpAbout => {
                self.set_status("PORTIX IDE v0.7.4 — Kernel Bare-Metal x86_64", false);
            }
            MenuAction::Separator => {}
            MenuAction::None => {}
        }
        true
    }

    /// Confirma el InputBox (llamado desde main cuando Enter en input activo).
    /// Devuelve true si se hizo algo.
    pub fn confirm_input(&mut self) -> bool {
        let mode = self.input.mode;
        let text_bytes = &self.input.buf[..self.input.len];
        match mode {
            InputMode::SaveAs => {
                // Actualizar nombre del buffer activo
                if self.input.len > 0 {
                    if let Some(buf) = self.buffers[self.active].as_mut() {
                        let n = self.input.len.min(256);
                        buf.name[..n].copy_from_slice(&text_bytes[..n]);
                        buf.name_len = n;
                        buf.dirty = false;
                        // Guardar la ruta
                        let pn = self.input.len.min(256);
                        self.save_path[..pn].copy_from_slice(&text_bytes[..pn]);
                        self.save_plen = pn;
                    }
                    self.set_status("Nombre actualizado. Conecta FAT32 para escribir.", false);
                }
                self.input.close();
                true
            }
            _ => {
                self.input.close();
                false
            }
        }
    }

    pub fn handle_key(&mut self, key: Key, ctrl: bool, visible_rows: usize) -> bool {
        // ── Input box activo — consume todos los keypresses ──────────────
        use crate::ui::input::InputMode;
        if self.input.mode != InputMode::None {
            if let Some(confirmed) = self.input.feed(key) {
                if confirmed { self.confirm_input(); }
                else         { self.set_status("Cancelado.", false); }
            }
            return true;
        }

        // Cerrar menú con Escape
        if key == Key::Escape && self.menu != MenuState::Closed {
            self.menu = MenuState::Closed; return true;
        }

        if ctrl {
            match key {
                Key::Char(b's') | Key::Char(b'S') => return self.execute_menu(MenuAction::FileSave),
                Key::Char(b'n') | Key::Char(b'N') => return self.execute_menu(MenuAction::FileNew),
                Key::Char(b'w') | Key::Char(b'W') => return self.execute_menu(MenuAction::FileClose),
                Key::Tab | Key::Right => { self.switch_next(); return true; }
                Key::Left             => { self.switch_prev(); return true; }
                _ => {}
            }
        }

        let Some(buf) = self.buffers[self.active].as_mut() else { return false };

        match key {
            Key::Up       => { if buf.cursor_l > 0 { buf.cursor_l -= 1; buf.clamp_col(); } buf.ensure_scroll(visible_rows); }
            Key::Down     => { if buf.cursor_l + 1 < buf.line_cnt { buf.cursor_l += 1; buf.clamp_col(); } buf.ensure_scroll(visible_rows); }
            Key::Left     => {
                if buf.cursor_c > 0 { buf.cursor_c -= 1; }
                else if buf.cursor_l > 0 {
                    buf.cursor_l -= 1;
                    buf.cursor_c = buf.get_line(buf.cursor_l).map(|l| l.len).unwrap_or(0);
                }
                buf.ensure_scroll(visible_rows);
            }
            Key::Right    => {
                let ll = buf.cur_line_len();
                if buf.cursor_c < ll { buf.cursor_c += 1; }
                else if buf.cursor_l + 1 < buf.line_cnt { buf.cursor_l += 1; buf.cursor_c = 0; }
                buf.ensure_scroll(visible_rows);
            }
            Key::Home     => { buf.cursor_c = 0; }
            Key::End      => { buf.cursor_c = buf.cur_line_len(); }
            Key::PageUp   => { buf.cursor_l = buf.cursor_l.saturating_sub(visible_rows); buf.clamp_col(); buf.ensure_scroll(visible_rows); }
            Key::PageDown => { buf.cursor_l = (buf.cursor_l + visible_rows).min(buf.line_cnt.saturating_sub(1)); buf.clamp_col(); buf.ensure_scroll(visible_rows); }
            Key::Enter    => { buf.insert_newline(); buf.ensure_scroll(visible_rows); }
            Key::Tab      => { for _ in 0..4 { buf.insert_char(b' '); } }
            Key::Backspace => { buf.backspace(); buf.ensure_scroll(visible_rows); }
            Key::Delete   => { buf.delete_forward(); }
            Key::Char(c) if c >= 0x20 && c < 0x7F => { buf.insert_char(c); }
            _ => return false,
        }
        true
    }

    pub fn get_save_data(&self, out: &mut [u8; 65536]) -> usize {
        if let Some(buf) = &self.buffers[self.active] { buf.serialize(out) } else { 0 }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Syntax highlighting (idéntico al original)
// ─────────────────────────────────────────────────────────────────────────────

const RUST_KEYWORDS: &[&[u8]] = &[
    b"fn", b"let", b"mut", b"pub", b"use", b"mod", b"struct", b"enum",
    b"impl", b"trait", b"for", b"while", b"if", b"else", b"match",
    b"return", b"self", b"Self", b"super", b"crate", b"const", b"static",
    b"type", b"where", b"async", b"await", b"unsafe", b"extern",
    b"true", b"false", b"in", b"loop", b"break", b"continue",
    b"ref", b"move", b"dyn", b"box",
];
const RUST_TYPES: &[&[u8]] = &[
    b"u8", b"u16", b"u32", b"u64", b"u128", b"usize",
    b"i8", b"i16", b"i32", b"i64", b"i128", b"isize",
    b"f32", b"f64", b"bool", b"char", b"str", b"String",
    b"Option", b"Result", b"Vec", b"Box", b"Arc", b"Rc",
];
const C_KEYWORDS: &[&[u8]] = &[
    b"int", b"char", b"void", b"long", b"short", b"unsigned", b"signed",
    b"float", b"double", b"struct", b"union", b"enum", b"typedef",
    b"if", b"else", b"for", b"while", b"do", b"return", b"break",
    b"continue", b"switch", b"case", b"default", b"static", b"extern",
    b"const", b"volatile", b"sizeof", b"NULL", b"true", b"false",
];

pub fn highlight_line<F>(line: &[u8], lang: Lang, mut emit: F)
where F: FnMut(usize, usize, Color)
{
    if lang == Lang::Plain { emit(0, line.len(), IdePal::TEXT); return; }
    let mut i = 0usize;
    let mut in_string: u8 = 0;
    while i < line.len() {
        if in_string == 0 {
            let rem = &line[i..];
            if (lang == Lang::Rust || lang == Lang::C) && rem.starts_with(b"//") {
                emit(i, line.len(), IdePal::SYN_COMMENT); return;
            }
            if lang == Lang::Asm && (line[i] == b';' || line[i] == b'#') {
                emit(i, line.len(), IdePal::SYN_COMMENT); return;
            }
        }
        if in_string == 0 && (line[i] == b'"' || line[i] == b'\'') {
            let delim = line[i]; in_string = delim;
            let start = i; i += 1;
            while i < line.len() {
                if line[i] == b'\\' { i += 2; continue; }
                if line[i] == delim { i += 1; in_string = 0; break; }
                i += 1;
            }
            emit(start, i, IdePal::SYN_STRING); continue;
        }
        if in_string != 0 { i += 1; continue; }

        if (lang == Lang::Rust || lang == Lang::C) && is_ident_start(line[i]) {
            let start = i;
            while i < line.len() && is_ident(line[i]) { i += 1; }
            if lang == Lang::Rust && i < line.len() && line[i] == b'!' {
                i += 1; emit(start, i, IdePal::SYN_MACRO); continue;
            }
            let word = &line[start..i];
            if is_keyword(word, RUST_KEYWORDS)   { emit(start, i, IdePal::SYN_KEYWORD); }
            else if is_keyword(word, RUST_TYPES)  { emit(start, i, IdePal::SYN_TYPE); }
            else if is_keyword(word, C_KEYWORDS)  { emit(start, i, IdePal::SYN_KEYWORD); }
            else                                  { emit(start, i, IdePal::TEXT); }
            continue;
        }

        if lang == Lang::Asm {
            if line[i] == b'.' {
                let start = i; i += 1;
                while i < line.len() && is_ident(line[i]) { i += 1; }
                emit(start, i, IdePal::SYN_MACRO); continue;
            }
            if is_ident_start(line[i]) {
                let start = i;
                while i < line.len() && is_ident(line[i]) { i += 1; }
                if i < line.len() && line[i] == b':' {
                    i += 1; emit(start, i, IdePal::SYN_TYPE);
                } else { emit(start, i, IdePal::SYN_KEYWORD); }
                continue;
            }
        }

        if line[i].is_ascii_digit() {
            let start = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') { i += 1; }
            emit(start, i, IdePal::SYN_NUMBER); continue;
        }
        if b"{}[]();,.<>!&|^~%+-*/=@#".contains(&line[i]) {
            emit(i, i + 1, IdePal::SYN_PUNCT); i += 1; continue;
        }
        emit(i, i + 1, IdePal::TEXT); i += 1;
    }
}

fn is_ident_start(b: u8) -> bool { b.is_ascii_alphabetic() || b == b'_' }
fn is_ident(b: u8) -> bool       { b.is_ascii_alphanumeric() || b == b'_' }
fn is_keyword(word: &[u8], list: &[&[u8]]) -> bool { list.iter().any(|&k| k == word) }

// ─────────────────────────────────────────────────────────────────────────────
// Constantes de renderizado
// ─────────────────────────────────────────────────────────────────────────────

const MENUBAR_H:  usize = 20;  // barra de menú
const FILETABS_H: usize = 22;  // pestañas de archivos
const STATUS_H:   usize = 18;  // status bar inferior del IDE
const GUTTER_W:   usize = 5;   // columnas de número de línea
const DROPDOWN_ITEM_H: usize = 16; // altura de cada item de dropdown

// ─────────────────────────────────────────────────────────────────────────────
// draw_ide_tab
// ─────────────────────────────────────────────────────────────────────────────

pub fn draw_ide_tab(c: &mut Console, lay: &Layout, ide: &IdeState) {
    let fw  = lay.fw;
    let _fh = lay.fh;  // reservado para uso futuro
    let cw  = lay.font_w;       // 8
    let ch  = lay.font_h;       // 8
    let lh  = ch + 3;           // 11px por fila de código
    let y0  = lay.content_y;    // donde comienza el contenido

    // Fondo completo
    c.fill_rect(0, y0, fw, lay.bottom_y.saturating_sub(y0), IdePal::BG);

    // ═══════════════════════════════════════════════════════════════════
    // MENÚ BAR  — misma paleta oscura que el chrome
    // ═══════════════════════════════════════════════════════════════════
    let menu_y = y0;
    // Fondo idéntico al HDR_BG del chrome
    c.fill_rect(0, menu_y, fw, MENUBAR_H, Color::new(0x04, 0x0B, 0x18));
    // Línea inferior: separador sutil azul
    c.hline(0, menu_y + MENUBAR_H - 1, fw, Color::new(0x18, 0x2C, 0x4A));

    let mut mx_pos = 6usize;
    for (mi, menu) in MENUS.iter().enumerate() {
        let is_open = ide.menu == MenuState::Open(mi);
        let label_w = menu.title.len() * cw + 16;

        if is_open {
            c.fill_rect(mx_pos, menu_y, label_w, MENUBAR_H, IdePal::DROPDOWN_HOV);
            c.fill_rect(mx_pos, menu_y, label_w, 2, Color::new(0xFF, 0xD7, 0x00));
        }
        let fg = if is_open { IdePal::MENU_FG_ACT } else { IdePal::MENU_FG };
        c.write_at(menu.title, mx_pos + 8, menu_y + (MENUBAR_H - ch) / 2, fg);
        mx_pos += label_w + 2;
    }

    // Atajo rápido en el extremo derecho de la menubar
    c.write_at("Ctrl+S Guardar  Ctrl+N Nuevo  Ctrl+W Cerrar",
        fw.saturating_sub(46 * cw), menu_y + (MENUBAR_H - ch) / 2,
        Color::new(0x38, 0x58, 0x88));

    // ═══════════════════════════════════════════════════════════════════
    // PESTAÑAS DE ARCHIVOS
    // ═══════════════════════════════════════════════════════════════════
    let ftab_y = y0 + MENUBAR_H;
    c.fill_rect(0, ftab_y, fw, FILETABS_H, IdePal::FILETAB_BG);
    c.hline(0, ftab_y + FILETABS_H - 1, fw, IdePal::BORDER);

    let mut tx = 2usize;
    for i in 0..MAX_BUFFERS {
        if let Some(buf) = &ide.buffers[i] {
            let is_active = i == ide.active;
            let icon      = buf.lang.icon();
            let name      = buf.name_str();
            // Truncar el nombre si es muy largo
            let max_name  = 18usize;
            let name_disp = if name.len() > max_name { &name[..max_name] } else { name };
            let tab_w     = (icon.len() + 1 + name_disp.len() + if buf.dirty { 3 } else { 2 }) * cw + 12;

            if is_active {
                c.fill_rect(tx, ftab_y, tab_w, FILETABS_H, IdePal::FILETAB_ACT);
                c.fill_rect(tx, ftab_y, tab_w, 2, Color::new(0xFF, 0xD7, 0x00));
            }
            c.vline(tx + tab_w - 1, ftab_y, FILETABS_H, IdePal::BORDER);

            let icon_fg = match buf.lang {
                Lang::Rust  => Color::new(0xDE, 0x6A, 0x40),
                Lang::C     => Color::new(0x44, 0x99, 0xFF),
                Lang::Asm   => Color::new(0xCC, 0xAA, 0x00),
                Lang::Plain => Color::new(0x88, 0xAA, 0xCC),
            };
            let name_fg = if is_active { IdePal::FILETAB_FG_ACT } else { IdePal::FILETAB_FG };
            let ty_text = ftab_y + (FILETABS_H - ch) / 2;
            c.write_at(icon, tx + 4, ty_text, icon_fg);
            c.write_at(name_disp, tx + 4 + (icon.len() + 1) * cw, ty_text, name_fg);
            if buf.dirty {
                let dot_x = tx + 4 + (icon.len() + 1 + name_disp.len() + 1) * cw;
                c.fill_rounded(dot_x, ty_text + 2, 5, 5, 2, IdePal::DIRTY_DOT);
            }
            tx += tab_w;
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // ÁREA DE EDICIÓN
    // ═══════════════════════════════════════════════════════════════════
    let edit_y       = ftab_y + FILETABS_H;
    let avail_h      = lay.bottom_y.saturating_sub(edit_y);
    let edit_h       = avail_h.saturating_sub(STATUS_H);
    let visible_rows = edit_h / lh;
    let gutter_px    = if ide.show_ln { GUTTER_W * cw + 8 } else { 4 };

    let Some(buf) = &ide.buffers[ide.active] else {
        c.write_at("Sin archivo — Ctrl+N para nuevo.", 20, edit_y + 20, IdePal::MENU_FG);
        return;
    };

    // Gutter
    if ide.show_ln {
        c.fill_rect(0, edit_y, gutter_px, edit_h, IdePal::GUTTER_BG);
        c.vline(gutter_px, edit_y, edit_h, IdePal::GUTTER_BORDER);
    }

    let mut lnbuf = [0u8; 8];
    for vis in 0..visible_rows {
        let lnum = buf.scroll + vis;
        if lnum >= buf.line_cnt { break; }
        let py        = edit_y + vis * lh;
        let is_cursor = lnum == buf.cursor_l;

        // Línea del cursor
        if is_cursor {
            c.fill_rect(if ide.show_ln { gutter_px + 1 } else { 0 }, py,
                fw.saturating_sub(if ide.show_ln { gutter_px + 1 } else { 0 }), lh,
                IdePal::CURSOR_LINE);
        }

        // Número de línea
        if ide.show_ln {
            let lnstr = fmt_usize(lnum + 1, &mut lnbuf);
            let lnx   = gutter_px.saturating_sub(lnstr.len() * cw + 4);
            let ln_fg = if is_cursor { Color::new(0xFF, 0xD7, 0x00) } else { IdePal::LINE_NUM };
            c.write_at(lnstr, lnx, py + 1, ln_fg);
        }

        // Contenido de la línea con highlighting
        let mut line_buf = [0u8; MAX_LINE_LEN];
        let mut line_len = 0usize;
        if let Some(line) = buf.get_line(lnum) {
            line_len = line.len.min(MAX_LINE_LEN);
            line_buf[..line_len].copy_from_slice(&line.data[..line_len]);
        }
        let text_x   = gutter_px + 4;
        let max_cols = fw.saturating_sub(text_x + 8) / cw;
        draw_highlighted_line(c, &line_buf[..line_len], buf.lang, text_x, py + 1, cw, max_cols);

        // Cursor (bloque)
        if is_cursor {
            let cx = text_x + buf.cursor_c * cw;
            if cx + cw <= fw {
                let cur_char = buf.get_line(lnum)
                    .map(|l| if buf.cursor_c < l.len { l.data[buf.cursor_c] } else { b' ' })
                    .unwrap_or(b' ');
                c.fill_rect(cx, py, cw, lh, IdePal::CURSOR_BG);
                let s = [cur_char];
                c.write_at_bg(
                    core::str::from_utf8(&s).unwrap_or(" "),
                    cx, py + 1, IdePal::CURSOR_FG, IdePal::CURSOR_BG,
                );
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // STATUS BAR DEL IDE / INPUT BOX INLINE
    // ═══════════════════════════════════════════════════════════════════
    let sy       = lay.bottom_y.saturating_sub(STATUS_H);
    let in_input = ide.input.is_active();
    let st_bg    = if in_input        { INPUT_BG }
                   else if ide.status_err { IdePal::STATUS_ERR }
                   else               { IdePal::STATUS_BG };
    c.fill_rect(0, sy, fw, STATUS_H, st_bg);
    c.hline(0, sy, fw, Color::new(0x00, 0x55, 0xBB));

    let sy_text = sy + (STATUS_H - ch) / 2;

    if in_input {
        // draw_input_overlay unificado — mismo widget que el Explorer
        draw_input_overlay(c, &ide.input, 8, sy, fw, STATUS_H, cw, ch);
    } else {
        // ── Modo normal ─────────────────────────────────────────────────
        let mut pos_buf = [0u8; 32]; let mut pp = 0;
        let mut tmp = [0u8; 8];
        for b in b"Ln " { pos_buf[pp] = *b; pp += 1; }
        for b in fmt_usize(buf.cursor_l + 1, &mut tmp).bytes() { pos_buf[pp] = b; pp += 1; }
        for b in b"  Col " { pos_buf[pp] = *b; pp += 1; }
        for b in fmt_usize(buf.cursor_c + 1, &mut tmp).bytes() { pos_buf[pp] = b; pp += 1; }
        c.write_at(core::str::from_utf8(&pos_buf[..pp]).unwrap_or(""), 8, sy_text, Color::WHITE);

        c.write_at("|", 120, sy_text, Color::new(0x00, 0x66, 0xCC));
        c.write_at(buf.lang.icon(),  130,        sy_text, Color::new(0xDD, 0xEE, 0xFF));
        c.write_at(buf.lang.label(), 130 + 40,   sy_text, Color::WHITE);

        // Nombre centrado
        let name_disp = buf.name_str();
        let name_x    = fw / 2 - name_disp.len() * cw / 2;
        c.write_at(name_disp, name_x, sy_text, Color::WHITE);
        if buf.dirty {
            c.write_at("[*]", name_x + name_disp.len() * cw + 4, sy_text, IdePal::DIRTY_DOT);
        }

        // Ruta (si tiene) — a la derecha del nombre
        if ide.save_plen > 0 {
            let path_str = core::str::from_utf8(&ide.save_path[..ide.save_plen]).unwrap_or("");
            let px = name_x + name_disp.len() * cw + (if buf.dirty { 4 + 3 * cw } else { 0 }) + 8;
            c.write_at(path_str, px.min(fw.saturating_sub(path_str.len() * cw + 8)), sy_text,
                Color::new(0x60, 0x90, 0xD0));
        }

        // Mensaje de status (extremo derecho)
        let msg = core::str::from_utf8(&ide.status_msg[..ide.status_len]).unwrap_or("");
        if !msg.is_empty() {
            c.write_at(msg, fw.saturating_sub(msg.len() * cw + 8), sy_text, Color::WHITE);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // DROPDOWN DE MENÚ (se dibuja encima de todo lo demás)
    // ═══════════════════════════════════════════════════════════════════
    if let MenuState::Open(open_idx) = ide.menu {
        draw_dropdown(c, lay, open_idx, y0);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_dropdown — dibuja el menú desplegable sobre el contenido
// ─────────────────────────────────────────────────────────────────────────────

fn draw_dropdown(c: &mut Console, lay: &Layout, menu_idx: usize, y0: usize) {
    if menu_idx >= MENUS.len() { return; }
    let cw = lay.font_w;
    let ch = lay.font_h;
    let menu = &MENUS[menu_idx];

    // Calcular posición X del menú (mismo lugar que el título en la menubar)
    let mut mx_pos = 6usize;
    for i in 0..menu_idx {
        let label_w = MENUS[i].title.len() * cw + 16;
        mx_pos += label_w + 2;
    }

    let max_label = menu.items.iter().map(|it| it.label.len()).max().unwrap_or(10);
    let max_short = menu.items.iter().map(|it| it.shortcut.len()).max().unwrap_or(0);
    let dd_w = (max_label + max_short + 6) * cw + 16;
    let dd_h = menu.items.len() * DROPDOWN_ITEM_H + 6;

    let dd_x = mx_pos;
    let dd_y = y0 + MENUBAR_H;

    // Sombra
    c.fill_rect(dd_x + 3, dd_y + 3, dd_w, dd_h, Color::new(0x00, 0x00, 0x00));
    // Fondo
    c.fill_rect(dd_x, dd_y, dd_w, dd_h, IdePal::DROPDOWN_BG);
    // Borde
    c.draw_rect(dd_x, dd_y, dd_w, dd_h, 1, IdePal::DROPDOWN_BOR);

    for (ii, item) in menu.items.iter().enumerate() {
        let iy = dd_y + 3 + ii * DROPDOWN_ITEM_H;
        let text_y = iy + (DROPDOWN_ITEM_H - ch) / 2;

        if item.action == MenuAction::Separator {
            c.hline(dd_x + 4, iy + DROPDOWN_ITEM_H / 2, dd_w - 8, IdePal::DROPDOWN_SEP);
        } else {
            c.write_at(item.label, dd_x + 10, text_y, IdePal::MENU_FG);
            if !item.shortcut.is_empty() {
                let sx = dd_x + dd_w - item.shortcut.len() * cw - 8;
                c.write_at(item.shortcut, sx, text_y, IdePal::MENU_SHORTCUT);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_highlighted_line
// ─────────────────────────────────────────────────────────────────────────────

fn draw_highlighted_line(
    c: &mut Console, line: &[u8], lang: Lang,
    x0: usize, y: usize, cw: usize, max_cols: usize,
) {
    let mut col = 0usize;
    highlight_line(line, lang, |start, end, color| {
        for i in start..end {
            if col >= max_cols || i >= line.len() { break; }
            let s = [line[i]];
            c.write_at(core::str::from_utf8(&s).unwrap_or("."), x0 + col * cw, y, color);
            col += 1;
        }
    });
}

fn fmt_usize(mut n: usize, buf: &mut [u8]) -> &str {
    let mut i = buf.len();
    if i == 0 { return ""; }
    if n == 0 { buf[i - 1] = b'0'; return core::str::from_utf8(&buf[i - 1..]).unwrap_or("0"); }
    while n > 0 && i > 0 { i -= 1; buf[i] = b'0' + (n % 10) as u8; n /= 10; }
    core::str::from_utf8(&buf[i..]).unwrap_or("?")
}