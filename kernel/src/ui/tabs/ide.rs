// ui/tabs/ide.rs — PORTIX Kernel v0.8.0
//
// REDISEÑO COMPLETO:
//  - Paleta desaturada, profesional (estilo VS Code Dark+)
//  - Caret corregido: limpia fondo antes de dibujar, ancho exacto cw px
//  - Atajos ocultos → overlay de ayuda con F1 o botón [?]
//  - Barra de menú más limpia y compacta
//  - Status bar más informativa y menos recargada
//  - Sin texto de ayuda visible en la barra (solo nombre + posición)
//
// LAYOUT INTERNO (dentro de content_y..bottom_y):
//   [MENU_H  = 22px]  → Archivo | Editar | Ver | Ayuda | [?]
//   [TABS_H  = 20px]  → pestañas de buffers
//   [EDIT_H  = …   ]  → área de edición con gutter
//   [STATUS_H= 18px]  → Ln/Col · Lang · nombre · estado

#![allow(dead_code)]

use core::mem::MaybeUninit;
use crate::drivers::input::keyboard::Key;
use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::ui::input::{InputBox, InputMode, draw_input_overlay, INPUT_BG};

// ─────────────────────────────────────────────────────────────────────────────
// Paleta IDE  —  desaturada, VS Code-inspired
// ─────────────────────────────────────────────────────────────────────────────

pub struct IdePal;
impl IdePal {
    // Fondos principales
    pub const BG:           Color = Color::new(0x1E, 0x1E, 0x1E); // editor bg
    pub const GUTTER_BG:    Color = Color::new(0x18, 0x18, 0x18); // gutter
    pub const MENU_BG:      Color = Color::new(0x33, 0x33, 0x33); // menubar
    pub const MENU_BORDER:  Color = Color::new(0x45, 0x45, 0x45); // menubar bottom
    pub const TAB_BG:       Color = Color::new(0x25, 0x25, 0x25); // inactive tab
    pub const TAB_ACT:      Color = Color::new(0x1E, 0x1E, 0x1E); // active tab
    pub const DROP_BG:      Color = Color::new(0x25, 0x25, 0x26); // dropdown
    pub const DROP_BOR:     Color = Color::new(0x45, 0x45, 0x45); // dropdown border
    pub const DROP_HOV:     Color = Color::new(0x04, 0x44, 0x7A); // dropdown hover
    pub const DROP_SEP:     Color = Color::new(0x3A, 0x3A, 0x3A); // dropdown separator
    pub const STATUS_BG:    Color = Color::new(0x00, 0x7A, 0xCC); // status (azul VS Code)
    pub const STATUS_ERR:   Color = Color::new(0xA1, 0x26, 0x0C); // status error
    pub const OVERLAY_BG:   Color = Color::new(0x25, 0x25, 0x25); // help overlay
    // Texto
    pub const TEXT:         Color = Color::new(0xD4, 0xD4, 0xD4); // texto normal
    pub const TEXT_DIM:     Color = Color::new(0x66, 0x6E, 0x7A); // texto tenue
    pub const LINE_NUM:     Color = Color::new(0x42, 0x42, 0x42); // número de línea
    pub const LINE_NUM_ACT: Color = Color::new(0xC6, 0xC6, 0xC6); // línea activa
    pub const MENU_FG:      Color = Color::new(0xCC, 0xCC, 0xCC); // texto menú
    pub const MENU_FG_ACT:  Color = Color::new(0xFF, 0xFF, 0xFF); // texto menú activo
    pub const MENU_SHORT:   Color = Color::new(0x80, 0x80, 0x80); // atajo (tenue)
    pub const TAB_FG:       Color = Color::new(0x80, 0x80, 0x80); // tab inactiva
    pub const TAB_FG_ACT:   Color = Color::new(0xFF, 0xFF, 0xFF); // tab activa
    pub const ACCENT:       Color = Color::new(0x00, 0x7A, 0xCC); // acento azul
    pub const DIRTY:        Color = Color::new(0xE4, 0x74, 0x00); // punto sucio
    pub const BORDER:       Color = Color::new(0x2D, 0x2D, 0x2D); // borde sutil
    // Cursor
    pub const CURSOR_LINE:  Color = Color::new(0x28, 0x28, 0x28); // línea del cursor
    pub const CURSOR_BG:    Color = Color::new(0xAE, 0xAF, 0xAD); // cursor block
    pub const CURSOR_FG:    Color = Color::new(0x1E, 0x1E, 0x1E); // char en cursor
    // Syntax
    pub const SYN_KW:       Color = Color::new(0x56, 0x9C, 0xD6); // keyword
    pub const SYN_STR:      Color = Color::new(0xCE, 0x91, 0x78); // string
    pub const SYN_CMT:      Color = Color::new(0x6A, 0x99, 0x55); // comment
    pub const SYN_NUM:      Color = Color::new(0xB5, 0xCE, 0xA8); // number
    pub const SYN_TYP:      Color = Color::new(0x4E, 0xC9, 0xB0); // type
    pub const SYN_MAC:      Color = Color::new(0xBD, 0x63, 0xC5); // macro
    pub const SYN_PUN:      Color = Color::new(0xD4, 0xD4, 0xD4); // punctuation (neutral)
}

// ─────────────────────────────────────────────────────────────────────────────
// Menús
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum MenuAction {
    None, Separator,
    FileNew, FileOpen, FileSave, FileSaveAs, FileClose,
    EditUndo, EditSelectAll, EditGoToLine,
    ViewLineNumbers, ViewWordWrap,
    HelpAbout, HelpKeys,
}

#[derive(Clone, Copy)]
pub struct MenuItem {
    pub label:    &'static str,
    pub shortcut: &'static str,
    pub action:   MenuAction,
}
impl MenuItem {
    const fn new(l: &'static str, s: &'static str, a: MenuAction) -> Self { MenuItem { label:l, shortcut:s, action:a } }
    const fn sep() -> Self { MenuItem { label:"───────────────", shortcut:"", action:MenuAction::Separator } }
}

const MENU_ARCH: &[MenuItem] = &[
    MenuItem::new("Nuevo",           "Ctrl+N", MenuAction::FileNew),
    MenuItem::new("Abrir...",        "Ctrl+O", MenuAction::FileOpen),
    MenuItem::sep(),
    MenuItem::new("Guardar",         "Ctrl+S", MenuAction::FileSave),
    MenuItem::new("Guardar como...", "",       MenuAction::FileSaveAs),
    MenuItem::sep(),
    MenuItem::new("Cerrar",          "Ctrl+W", MenuAction::FileClose),
];
const MENU_EDIT: &[MenuItem] = &[
    MenuItem::new("Deshacer",        "Ctrl+Z", MenuAction::EditUndo),
    MenuItem::sep(),
    MenuItem::new("Selec. todo",     "Ctrl+A", MenuAction::EditSelectAll),
    MenuItem::new("Ir a línea...",   "Ctrl+G", MenuAction::EditGoToLine),
];
const MENU_VIEW: &[MenuItem] = &[
    MenuItem::new("Núm. de línea",   "",       MenuAction::ViewLineNumbers),
    MenuItem::new("Ajuste línea",    "",       MenuAction::ViewWordWrap),
];
const MENU_HELP: &[MenuItem] = &[
    MenuItem::new("Atajos (F1)",     "F1",     MenuAction::HelpKeys),
    MenuItem::sep(),
    MenuItem::new("Acerca de...",    "",       MenuAction::HelpAbout),
];

#[derive(Clone, Copy)]
pub struct MenuDef { pub title: &'static str, pub items: &'static [MenuItem] }
pub const MENUS: &[MenuDef] = &[
    MenuDef { title: "Archivo", items: MENU_ARCH },
    MenuDef { title: "Editar",  items: MENU_EDIT },
    MenuDef { title: "Ver",     items: MENU_VIEW },
    MenuDef { title: "Ayuda",   items: MENU_HELP },
];

#[derive(Clone, Copy, PartialEq)]
pub enum MenuState { Closed, Open(usize) }

// ─────────────────────────────────────────────────────────────────────────────
// Lenguaje
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum Lang { Plain, Rust, C, Asm }
impl Lang {
    pub fn from_name(n: &str) -> Self {
        if n.ends_with(".rs") { Lang::Rust }
        else if n.ends_with(".c") || n.ends_with(".h") { Lang::C }
        else if n.ends_with(".asm") || n.ends_with(".s") { Lang::Asm }
        else { Lang::Plain }
    }
    pub fn label(self) -> &'static str { match self { Lang::Rust=>"Rust", Lang::C=>"C", Lang::Asm=>"ASM", Lang::Plain=>"TXT" } }
    pub fn icon(self) -> &'static str  { match self { Lang::Rust=>"rs", Lang::C=>"c", Lang::Asm=>"asm", Lang::Plain=>"txt" } }
    pub fn icon_color(self) -> Color   { match self {
        Lang::Rust  => Color::new(0xDE, 0x6A, 0x40),
        Lang::C     => Color::new(0x44, 0x99, 0xFF),
        Lang::Asm   => Color::new(0xCC, 0xAA, 0x00),
        Lang::Plain => Color::new(0x88, 0x99, 0xAA),
    }}
}

// ─────────────────────────────────────────────────────────────────────────────
// Buffer paginado (sin cambios funcionales)
// ─────────────────────────────────────────────────────────────────────────────

const MAX_LINES:       usize = 4096;
const MAX_LINE_LEN:    usize = 512;
const MAX_BUFFERS:     usize = 8;
const PAGE_LINES:      usize = 64;
const MAX_PAGES_TOTAL: usize = 64;

#[derive(Clone, Copy)]
pub struct Line { pub data: [u8; MAX_LINE_LEN], pub len: usize }
impl Line {
    pub const fn empty() -> Self { Line { data: [0u8; MAX_LINE_LEN], len: 0 } }
    pub fn as_str(&self) -> &str { core::str::from_utf8(&self.data[..self.len]).unwrap_or("") }
    pub fn insert(&mut self, col: usize, byte: u8) -> bool {
        if self.len >= MAX_LINE_LEN { return false; }
        let col = col.min(self.len);
        self.data.copy_within(col..self.len, col + 1);
        self.data[col] = byte; self.len += 1; true
    }
    pub fn remove(&mut self, col: usize) -> bool {
        if col >= self.len { return false; }
        self.data.copy_within(col + 1..self.len, col);
        self.len -= 1; true
    }
}

#[derive(Clone, Copy)]
struct Page { lines: [Line; PAGE_LINES], used: bool, next: i32, prev: i32, count: usize }

static mut PAGE_POOL: MaybeUninit<[Page; MAX_PAGES_TOTAL]> = MaybeUninit::uninit();

#[inline(always)] unsafe fn pool_raw() -> *mut Page { core::ptr::addr_of_mut!(PAGE_POOL) as *mut Page }
#[inline(always)] unsafe fn pool_raw_const() -> *const Page { core::ptr::addr_of!(PAGE_POOL) as *const Page }

pub fn init_page_pool() {
    unsafe {
        for i in 0..MAX_PAGES_TOTAL {
            let p = &mut *pool_raw().add(i);
            p.used = false; p.next = -1; p.prev = -1; p.count = 0;
        }
    }
}

unsafe fn alloc_page() -> Option<usize> {
    for i in 0..MAX_PAGES_TOTAL {
        let p = &mut *pool_raw().add(i);
        if !p.used {
            p.used = true; p.next = -1; p.prev = -1; p.count = 0;
            for j in 0..PAGE_LINES { p.lines[j] = Line::empty(); }
            return Some(i);
        }
    }
    None
}
unsafe fn free_page(idx: usize) {
    if idx < MAX_PAGES_TOTAL {
        let p = &mut *pool_raw().add(idx);
        p.used = false; p.next = -1; p.prev = -1; p.count = 0;
    }
}
#[inline(always)] unsafe fn page_mut(idx: usize) -> &'static mut Page { &mut *pool_raw().add(idx) }
#[inline(always)] unsafe fn page_ref(idx: usize) -> &'static Page { &*pool_raw_const().add(idx) }

// ─────────────────────────────────────────────────────────────────────────────
// TextBuffer
// ─────────────────────────────────────────────────────────────────────────────

pub struct TextBuffer {
    pub head_page: i32, pub tail_page: i32,
    pub page_cnt:  usize, pub line_cnt: usize,
    pub name:      [u8; 256], pub name_len: usize,
    pub lang:      Lang,
    pub dirty:     bool,
    pub cursor_l:  usize, pub cursor_c: usize,
    pub scroll:    usize,
}

impl TextBuffer {
    pub fn new_empty(name: &str) -> Self {
        let lang = Lang::from_name(name);
        let mut head = -1i32;
        unsafe { if let Some(pi) = alloc_page() { head = pi as i32; } }
        let mut tb = TextBuffer {
            head_page: head, tail_page: head,
            page_cnt: if head >= 0 { 1 } else { 0 }, line_cnt: 1,
            name: [0u8; 256], name_len: 0, lang, dirty: false,
            cursor_l: 0, cursor_c: 0, scroll: 0,
        };
        let n = name.len().min(255);
        tb.name[..n].copy_from_slice(name.as_bytes());
        tb.name_len = n;
        if tb.head_page >= 0 {
            unsafe { let p = page_mut(tb.head_page as usize); p.count = 1; p.lines[0] = Line::empty(); }
        }
        tb
    }

    pub fn load_text(&mut self, data: &[u8]) {
        self.clear_pages();
        if self.head_page < 0 {
            unsafe {
                if let Some(pi) = alloc_page() {
                    self.head_page = pi as i32; self.tail_page = pi as i32; self.page_cnt = 1;
                }
            }
        }
        let mut cur_line_idx: usize = 0;
        self.line_cnt = 1; self.cursor_l = 0; self.cursor_c = 0; self.scroll = 0;
        unsafe { let p = page_mut(self.head_page as usize); p.count = 1; p.lines[0] = Line::empty(); }
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
        let mut p = 0usize;
        for li in 0..self.line_cnt {
            if let Some(line) = self.get_line(li) {
                let n = line.len.min(out.len().saturating_sub(p));
                out[p..p + n].copy_from_slice(&line.data[..n]);
                p += n;
            }
            if li + 1 < self.line_cnt && p < out.len() { out[p] = b'\n'; p += 1; }
        }
        p
    }

    pub fn name_str(&self) -> &str { core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("untitled") }

    fn cur_line_len(&self) -> usize { self.get_line(self.cursor_l).map(|l| l.len).unwrap_or(0) }
    fn clamp_col(&mut self) { let m = self.get_line(self.cursor_l).map(|l| l.len).unwrap_or(0); if self.cursor_c > m { self.cursor_c = m; } }
    fn ensure_scroll(&mut self, vis: usize) {
        if self.cursor_l < self.scroll { self.scroll = self.cursor_l; }
        else if self.cursor_l >= self.scroll + vis { self.scroll = self.cursor_l + 1 - vis; }
    }

    fn insert_char(&mut self, ch: u8) {
        let cc = self.cursor_c;
        let ok = if let Some(line) = self.get_line_mut(self.cursor_l) { line.insert(cc, ch) } else { false };
        if ok { self.cursor_c = self.cursor_c.saturating_add(1); self.dirty = true; return; }
        self.insert_newline();
        let cc2 = self.cursor_c;
        if let Some(line) = self.get_line_mut(self.cursor_l) {
            let _ = line.insert(cc2, ch); self.cursor_c = self.cursor_c.saturating_add(1); self.dirty = true;
        }
    }

    fn insert_newline(&mut self) {
        if self.line_cnt >= MAX_LINES { return; }
        let l = self.cursor_l;
        let mut cur = Line::empty();
        if let Some(e) = self.get_line(l) { cur = *e; }
        let split_at = self.cursor_c.min(cur.len);
        let old_len = cur.len;
        let mut new_line = Line::empty();
        let tail = old_len.saturating_sub(split_at);
        if tail > 0 { new_line.data[..tail].copy_from_slice(&cur.data[split_at..old_len]); new_line.len = tail; }
        if let Some(cm) = self.get_line_mut(l) { cm.len = split_at; }
        self.insert_line_at(l + 1, new_line);
        self.line_cnt = self.line_cnt.saturating_add(1);
        self.cursor_l = self.cursor_l.saturating_add(1);
        self.cursor_c = 0; self.dirty = true;
    }

    fn backspace(&mut self) {
        let cc = self.cursor_c;
        if cc > 0 {
            let l = self.cursor_l;
            if let Some(line) = self.get_line_mut(l) { let _ = line.remove(cc - 1); }
            self.cursor_c = cc - 1; self.dirty = true; return;
        }
        if self.cursor_l > 0 {
            let prev = self.cursor_l - 1; let cur = self.cursor_l;
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
            self.line_cnt = self.line_cnt.saturating_sub(1);
            self.cursor_l = self.cursor_l.saturating_sub(1);
            self.cursor_c = prev_len; self.dirty = true;
        }
    }

    fn delete_forward(&mut self) {
        let l = self.cursor_l; let cc = self.cursor_c;
        if let Some(line) = self.get_line_mut(l) {
            if cc < line.len { line.remove(cc); self.dirty = true; return; }
        }
        if l + 1 < self.line_cnt {
            let ni = l + 1;
            let mut nl = Line::empty();
            if let Some(n) = self.get_line(ni) { nl = *n; }
            let cl = self.get_line(l).map(|l| l.len).unwrap_or(0);
            let copy = nl.len.min(MAX_LINE_LEN.saturating_sub(cl));
            if copy > 0 {
                if let Some(cm) = self.get_line_mut(l) {
                    cm.data[cl..cl + copy].copy_from_slice(&nl.data[..copy]);
                    cm.len = cl + copy;
                }
            }
            self.delete_line_at(ni);
            self.line_cnt = self.line_cnt.saturating_sub(1); self.dirty = true;
        }
    }

    // ── Paginación ────────────────────────────────────────────────────────────

    fn find_page_for_line(&self, li: usize) -> Option<(usize, usize)> {
        if self.head_page < 0 { return None; }
        let mut pidx = self.head_page as usize; let mut acc = 0usize;
        unsafe { loop {
            let p = page_ref(pidx);
            if acc + p.count > li { return Some((pidx, li - acc)); }
            acc += p.count;
            if p.next < 0 { break; }
            pidx = p.next as usize;
        }}
        None
    }

    fn ensure_slot_for_line(&mut self, li: usize) -> (usize, usize) {
        if li >= self.line_cnt { while self.line_cnt <= li { self.append_empty_line(); } }
        if let Some((pidx, off)) = self.find_page_for_line(li) { return (pidx, off); }
        unsafe {
            if let Some(pi) = alloc_page() {
                let pi = pi as usize;
                if self.tail_page >= 0 {
                    let t = page_mut(self.tail_page as usize); t.next = pi as i32;
                    page_mut(pi).prev = self.tail_page; self.tail_page = pi as i32;
                } else { self.head_page = pi as i32; self.tail_page = pi as i32; }
                self.page_cnt = self.page_cnt.saturating_add(1);
                page_mut(pi).count = 1; page_mut(pi).lines[0] = Line::empty();
                return (pi, 0);
            } else { return (self.head_page as usize, 0); }
        }
    }

    fn append_empty_line(&mut self) {
        if self.head_page < 0 {
            unsafe {
                if let Some(pi) = alloc_page() { self.head_page = pi as i32; self.tail_page = pi as i32; self.page_cnt = 1; }
            }
        }
        unsafe {
            let tail = self.tail_page as usize; let p = page_mut(tail);
            if p.count < PAGE_LINES { p.lines[p.count] = Line::empty(); p.count += 1; }
            else if let Some(pi) = alloc_page() {
                let pi = pi as usize;
                page_mut(pi).count = 1; page_mut(pi).lines[0] = Line::empty();
                p.next = pi as i32; page_mut(pi).prev = self.tail_page;
                self.tail_page = pi as i32; self.page_cnt = self.page_cnt.saturating_add(1);
            }
        }
        self.line_cnt = self.line_cnt.saturating_add(1);
    }

    pub fn get_line(&self, li: usize) -> Option<&Line> {
        self.find_page_for_line(li).map(|(pidx, off)| unsafe { &page_ref(pidx).lines[off] })
    }
    fn get_line_mut(&mut self, li: usize) -> Option<&mut Line> {
        if let Some((pidx, off)) = self.find_page_for_line(li) { unsafe { Some(&mut page_mut(pidx).lines[off]) } } else { None }
    }

    fn insert_line_at(&mut self, at: usize, line: Line) {
        if at > self.line_cnt { return; }
        if at == self.line_cnt { self.append_empty_line(); if let Some(d) = self.get_line_mut(at) { *d = line; } return; }
        self.append_empty_line();
        if self.line_cnt < 2 { if let Some(d) = self.get_line_mut(at) { *d = line; } return; }
        let mut i = self.line_cnt.saturating_sub(2);
        loop {
            if i < at { break; }
            if let Some(src) = self.get_line(i) { let tmp = *src; if let Some(d) = self.get_line_mut(i + 1) { *d = tmp; } }
            if i == 0 { break; } i -= 1;
        }
        if let Some(d) = self.get_line_mut(at) { *d = line; }
    }

    fn delete_line_at(&mut self, at: usize) {
        if at >= self.line_cnt { return; }
        let mut i = at;
        while i + 1 < self.line_cnt {
            if let Some(src) = self.get_line(i + 1) { let tmp = *src; if let Some(d) = self.get_line_mut(i) { *d = tmp; } }
            i += 1;
        }
        self.remove_last_line_slot();
    }

    fn remove_last_line_slot(&mut self) {
        if self.line_cnt == 0 { return; }
        let last = self.line_cnt - 1;
        if let Some((pidx, off)) = self.find_page_for_line(last) {
            unsafe {
                let p = page_mut(pidx);
                if p.count > 0 {
                    p.lines[off] = Line::empty(); p.count = p.count.saturating_sub(1);
                    if p.count == 0 {
                        let prev = p.prev; let next = p.next;
                        if prev >= 0 { page_mut(prev as usize).next = next; }
                        else { self.head_page = if next >= 0 { next } else { -1 }; }
                        if next >= 0 { page_mut(next as usize).prev = prev; }
                        else { self.tail_page = if prev >= 0 { prev } else { -1 }; }
                        free_page(pidx); self.page_cnt = self.page_cnt.saturating_sub(1);
                    }
                }
            }
        }
        self.line_cnt = self.line_cnt.saturating_sub(1);
    }

    pub fn clear_pages(&mut self) {
        if self.head_page < 0 { return; }
        let mut cur = self.head_page as i32;
        while cur >= 0 { let next = unsafe { page_ref(cur as usize).next }; unsafe { free_page(cur as usize); } cur = next; }
        self.head_page = -1; self.tail_page = -1; self.page_cnt = 0; self.line_cnt = 0;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// IdeState
// ─────────────────────────────────────────────────────────────────────────────

pub struct IdeState {
    pub buffers:    [Option<TextBuffer>; MAX_BUFFERS],
    pub active:     usize,
    pub buf_count:  usize,
    pub status_msg: [u8; 80],
    pub status_len: usize,
    pub status_err: bool,
    pub menu:       MenuState,
    pub show_ln:    bool,
    pub show_help:  bool,   // F1 = overlay de atajos
    pub input:      InputBox,
    pub save_path:  [u8; 256],
    pub save_plen:  usize,
}

impl IdeState {
    pub fn new() -> Self {
        let mut ide = IdeState {
            buffers:    core::array::from_fn(|_| None),
            active:     0, buf_count: 0,
            status_msg: [0u8; 80], status_len: 0, status_err: false,
            menu:       MenuState::Closed,
            show_ln:    true,
            show_help:  false,
            input:      InputBox::new(),
            save_path:  [0u8; 256], save_plen: 0,
        };
        ide.open_new("untitled.txt");
        ide
    }

    pub fn open_new(&mut self, name: &str) -> bool {
        if self.buf_count >= MAX_BUFFERS { return false; }
        for i in 0..MAX_BUFFERS {
            if self.buffers[i].is_none() {
                self.buffers[i] = Some(TextBuffer::new_empty(name));
                self.active = i; self.buf_count += 1;
                self.set_status("Nuevo archivo", false); return true;
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
                self.active = i; self.buf_count += 1;
                self.set_status("Archivo abierto", false); return true;
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
        for _ in 0..MAX_BUFFERS { if self.buffers[i].is_some() { self.active = i; return; } i = (i + 1) % MAX_BUFFERS; }
    }
    pub fn switch_prev(&mut self) {
        let mut i = if self.active == 0 { MAX_BUFFERS - 1 } else { self.active - 1 };
        for _ in 0..MAX_BUFFERS { if self.buffers[i].is_some() { self.active = i; return; } i = if i == 0 { MAX_BUFFERS - 1 } else { i - 1 }; }
    }

    pub fn set_status(&mut self, msg: &str, is_err: bool) {
        let n = msg.len().min(80);
        self.status_msg[..n].copy_from_slice(msg.as_bytes());
        self.status_len = n; self.status_err = is_err;
    }

    pub fn execute_menu(&mut self, action: MenuAction) -> bool {
        self.menu = MenuState::Closed;
        match action {
            MenuAction::FileNew     => { self.open_new("untitled.txt"); }
            MenuAction::FileOpen    => { self.input.start(InputMode::SaveAs, ""); self.set_status("Ruta del archivo a abrir:", false); }
            MenuAction::FileSave    => {
                if self.save_plen == 0 {
                    let name = if let Some(b) = &self.buffers[self.active] { b.name_str() } else { "untitled.txt" };
                    self.input.start(InputMode::SaveAs, name);
                    self.set_status("Nombre del archivo:", false);
                } else {
                    if let Some(b) = self.buffers[self.active].as_mut() { b.dirty = false; }
                    self.set_status("Guardado", false);
                }
            }
            MenuAction::FileSaveAs  => {
                let name = if let Some(b) = &self.buffers[self.active] { b.name_str() } else { "untitled.txt" };
                self.input.start(InputMode::SaveAs, name);
                self.set_status("Guardar como:", false);
            }
            MenuAction::FileClose   => { self.close_active(); }
            MenuAction::EditUndo    => { self.set_status("Deshacer: no implementado", true); }
            MenuAction::EditSelectAll => { self.set_status("Selec. todo: no implementado", true); }
            MenuAction::EditGoToLine => { self.input.start(InputMode::SaveAs, ""); self.set_status("Ir a línea:", false); }
            MenuAction::ViewLineNumbers => {
                self.show_ln = !self.show_ln;
                self.set_status(if self.show_ln { "Núm. de línea: ON" } else { "Núm. de línea: OFF" }, false);
            }
            MenuAction::ViewWordWrap => { self.set_status("Ajuste de línea: no implementado", true); }
            MenuAction::HelpKeys    => { self.show_help = true; }
            MenuAction::HelpAbout   => { self.set_status("PORTIX IDE v0.8.0 — Kernel x86_64 Bare-Metal", false); }
            MenuAction::Separator   => {}
            MenuAction::None        => {}
        }
        true
    }

    pub fn confirm_input(&mut self) -> bool {
        let mode = self.input.mode;
        let tb = &self.input.buf[..self.input.len];
        if let InputMode::SaveAs = mode {
            if self.input.len > 0 {
                if let Some(buf) = self.buffers[self.active].as_mut() {
                    let n = self.input.len.min(256);
                    buf.name[..n].copy_from_slice(&tb[..n]);
                    buf.name_len = n; buf.dirty = false;
                    let pn = self.input.len.min(256);
                    self.save_path[..pn].copy_from_slice(&tb[..pn]);
                    self.save_plen = pn;
                }
                self.set_status("Nombre actualizado", false);
            }
        }
        self.input.close(); true
    }

    pub fn handle_key(&mut self, key: Key, ctrl: bool, vis: usize) -> bool {
        use crate::ui::input::InputMode;

        // Cerrar help overlay primero
        if self.show_help {
            self.show_help = false; return true;
        }

        // Input activo consume todo
        if self.input.mode != InputMode::None {
            if let Some(confirmed) = self.input.feed(key) {
                if confirmed { self.confirm_input(); }
                else { self.set_status("Cancelado", false); }
            }
            return true;
        }

        // Escape cierra menú
        if key == Key::Escape && self.menu != MenuState::Closed { self.menu = MenuState::Closed; return true; }
        // F1 = help
        if key == Key::F1 { self.show_help = true; return true; }

        if ctrl {
            match key {
                Key::Char(b's') | Key::Char(b'S') => return self.execute_menu(MenuAction::FileSave),
                Key::Char(b'n') | Key::Char(b'N') => return self.execute_menu(MenuAction::FileNew),
                Key::Char(b'w') | Key::Char(b'W') => return self.execute_menu(MenuAction::FileClose),
                Key::Tab | Key::Right => { self.switch_next(); return true; }
                Key::Left => { self.switch_prev(); return true; }
                _ => {}
            }
        }

        let Some(buf) = self.buffers[self.active].as_mut() else { return false };

        match key {
            Key::Up    => { if buf.cursor_l > 0 { buf.cursor_l -= 1; buf.clamp_col(); } buf.ensure_scroll(vis); }
            Key::Down  => { if buf.cursor_l + 1 < buf.line_cnt { buf.cursor_l += 1; buf.clamp_col(); } buf.ensure_scroll(vis); }
            Key::Left  => {
                if buf.cursor_c > 0 { buf.cursor_c -= 1; }
                else if buf.cursor_l > 0 { buf.cursor_l -= 1; buf.cursor_c = buf.get_line(buf.cursor_l).map(|l| l.len).unwrap_or(0); }
                buf.ensure_scroll(vis);
            }
            Key::Right => {
                let ll = buf.cur_line_len();
                if buf.cursor_c < ll { buf.cursor_c += 1; }
                else if buf.cursor_l + 1 < buf.line_cnt { buf.cursor_l += 1; buf.cursor_c = 0; }
                buf.ensure_scroll(vis);
            }
            Key::Home     => { buf.cursor_c = 0; }
            Key::End      => { buf.cursor_c = buf.cur_line_len(); }
            Key::PageUp   => { buf.cursor_l = buf.cursor_l.saturating_sub(vis); buf.clamp_col(); buf.ensure_scroll(vis); }
            Key::PageDown => { buf.cursor_l = (buf.cursor_l + vis).min(buf.line_cnt.saturating_sub(1)); buf.clamp_col(); buf.ensure_scroll(vis); }
            Key::Enter    => { buf.insert_newline(); buf.ensure_scroll(vis); }
            Key::Tab      => { for _ in 0..4 { buf.insert_char(b' '); } }
            Key::Backspace => { buf.backspace(); buf.ensure_scroll(vis); }
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
// Syntax highlighting
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
    b"u8",b"u16",b"u32",b"u64",b"u128",b"usize",
    b"i8",b"i16",b"i32",b"i64",b"i128",b"isize",
    b"f32",b"f64",b"bool",b"char",b"str",b"String",
    b"Option",b"Result",b"Vec",b"Box",b"Arc",b"Rc",
];
const C_KEYWORDS: &[&[u8]] = &[
    b"int",b"char",b"void",b"long",b"short",b"unsigned",b"signed",
    b"float",b"double",b"struct",b"union",b"enum",b"typedef",
    b"if",b"else",b"for",b"while",b"do",b"return",b"break",
    b"continue",b"switch",b"case",b"default",b"static",b"extern",
    b"const",b"volatile",b"sizeof",b"NULL",b"true",b"false",
];

pub fn highlight_line<F>(line: &[u8], lang: Lang, mut emit: F) where F: FnMut(usize, usize, Color) {
    if lang == Lang::Plain { emit(0, line.len(), IdePal::TEXT); return; }
    let mut i = 0usize; let mut in_str: u8 = 0;
    while i < line.len() {
        if in_str == 0 {
            let rem = &line[i..];
            if (lang == Lang::Rust || lang == Lang::C) && rem.starts_with(b"//") { emit(i, line.len(), IdePal::SYN_CMT); return; }
            if lang == Lang::Asm && (line[i] == b';' || line[i] == b'#') { emit(i, line.len(), IdePal::SYN_CMT); return; }
        }
        if in_str == 0 && (line[i] == b'"' || line[i] == b'\'') {
            let delim = line[i]; in_str = delim; let s = i; i += 1;
            while i < line.len() { if line[i] == b'\\' { i += 2; continue; } if line[i] == delim { i += 1; in_str = 0; break; } i += 1; }
            emit(s, i, IdePal::SYN_STR); continue;
        }
        if in_str != 0 { i += 1; continue; }
        if (lang == Lang::Rust || lang == Lang::C) && is_ident_start(line[i]) {
            let s = i;
            while i < line.len() && is_ident(line[i]) { i += 1; }
            if lang == Lang::Rust && i < line.len() && line[i] == b'!' { i += 1; emit(s, i, IdePal::SYN_MAC); continue; }
            let word = &line[s..i];
            if is_kw(word, RUST_KEYWORDS)  { emit(s, i, IdePal::SYN_KW); }
            else if is_kw(word, RUST_TYPES){ emit(s, i, IdePal::SYN_TYP); }
            else if is_kw(word, C_KEYWORDS){ emit(s, i, IdePal::SYN_KW); }
            else                           { emit(s, i, IdePal::TEXT); }
            continue;
        }
        if lang == Lang::Asm {
            if line[i] == b'.' { let s = i; i += 1; while i < line.len() && is_ident(line[i]) { i += 1; } emit(s, i, IdePal::SYN_MAC); continue; }
            if is_ident_start(line[i]) {
                let s = i;
                while i < line.len() && is_ident(line[i]) { i += 1; }
                if i < line.len() && line[i] == b':' { i += 1; emit(s, i, IdePal::SYN_TYP); } else { emit(s, i, IdePal::SYN_KW); }
                continue;
            }
        }
        if line[i].is_ascii_digit() {
            let s = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') { i += 1; }
            emit(s, i, IdePal::SYN_NUM); continue;
        }
        if b"{}[]();,.<>!&|^~%+-*/=@#".contains(&line[i]) { emit(i, i+1, IdePal::SYN_PUN); i += 1; continue; }
        emit(i, i+1, IdePal::TEXT); i += 1;
    }
}

fn is_ident_start(b: u8) -> bool { b.is_ascii_alphabetic() || b == b'_' }
fn is_ident(b: u8) -> bool       { b.is_ascii_alphanumeric() || b == b'_' }
fn is_kw(w: &[u8], list: &[&[u8]]) -> bool { list.iter().any(|&k| k == w) }

// ─────────────────────────────────────────────────────────────────────────────
// Constantes de renderizado
// ─────────────────────────────────────────────────────────────────────────────

pub const MENU_H:         usize = 22;
pub const TABS_H:         usize = 20;
pub const STATUS_H:       usize = 18;
const GUTTER_W:           usize = 5;
const DROPDOWN_ITEM_H:    usize = 16;
const HELP_OVERLAY_W:     usize = 380;
const HELP_OVERLAY_H:     usize = 280;

// ─────────────────────────────────────────────────────────────────────────────
// draw_ide_tab
// ─────────────────────────────────────────────────────────────────────────────

pub fn draw_ide_tab(c: &mut Console, lay: &Layout, ide: &IdeState) {
    let fw  = lay.fw;
    let cw  = lay.font_w;   // 8
    let ch  = lay.font_h;   // 8
    let lh  = ch + 3;       // 11px por fila
    let y0  = lay.content_y;

    // ── Fondo completo ────────────────────────────────────────────────────────
    c.fill_rect(0, y0, fw, lay.bottom_y.saturating_sub(y0), IdePal::BG);

    // ═════════════════════════════════════════════════════════════════════════
    // BARRA DE MENÚ
    // ═════════════════════════════════════════════════════════════════════════
    let my = y0;
    c.fill_rect(0, my, fw, MENU_H, IdePal::MENU_BG);
    c.hline(0, my + MENU_H - 1, fw, IdePal::MENU_BORDER);

    let mut mx_pos = 8usize;
    for (mi, menu) in MENUS.iter().enumerate() {
        let is_open = ide.menu == MenuState::Open(mi);
        let lw      = menu.title.len() * cw + 14;
        if is_open {
            c.fill_rect(mx_pos, my, lw, MENU_H - 1, IdePal::DROP_HOV);
            // Línea de acento superior
            c.fill_rect(mx_pos, my, lw, 2, IdePal::ACCENT);
        }
        let fg = if is_open { IdePal::MENU_FG_ACT } else { IdePal::MENU_FG };
        c.write_at(menu.title, mx_pos + 7, my + (MENU_H - ch) / 2, fg);
        mx_pos += lw + 2;
    }

    // Botón [?] de ayuda — extremo derecho de la menubar
    let help_x = fw.saturating_sub(cw * 3 + 12);
    let help_active = ide.show_help;
    let help_bg = if help_active { IdePal::DROP_HOV } else { IdePal::MENU_BG };
    c.fill_rect(help_x, my + 3, cw * 2 + 8, MENU_H - 6, help_bg);
    c.draw_rect(help_x, my + 3, cw * 2 + 8, MENU_H - 6, 1, IdePal::DROP_BOR);
    c.write_at("?", help_x + 4 + cw / 2, my + (MENU_H - ch) / 2, Color::new(0xCC, 0xCC, 0xCC));

    // ═════════════════════════════════════════════════════════════════════════
    // PESTAÑAS DE BUFFERS
    // ═════════════════════════════════════════════════════════════════════════
    let ty = y0 + MENU_H;
    c.fill_rect(0, ty, fw, TABS_H, Color::new(0x25, 0x25, 0x26));
    c.hline(0, ty + TABS_H - 1, fw, IdePal::BORDER);

    let mut tx = 0usize;
    for i in 0..MAX_BUFFERS {
        if let Some(buf) = &ide.buffers[i] {
            let is_act = i == ide.active;
            let name   = buf.name_str();
            let nmax   = 18usize;
            let ndisp  = if name.len() > nmax { &name[..nmax] } else { name };
            // width: icono(2ch) + espacio + nombre + sucio(2ch) + padding
            let tab_w  = (3 + 1 + ndisp.len() + if buf.dirty { 2 } else { 1 }) * cw + 12;

            let bg = if is_act { IdePal::TAB_ACT } else { Color::new(0x2D, 0x2D, 0x2D) };
            c.fill_rect(tx, ty, tab_w, TABS_H, bg);

            if is_act {
                // Línea de acento inferior = indica tab activa
                c.fill_rect(tx, ty + TABS_H - 2, tab_w, 2, IdePal::ACCENT);
            }

            // Separador derecho
            c.vline(tx + tab_w - 1, ty, TABS_H, IdePal::BORDER);

            let tty  = ty + (TABS_H - ch) / 2;
            let icon_col = buf.lang.icon_color();
            let name_col = if is_act { IdePal::TAB_FG_ACT } else { IdePal::TAB_FG };

            // Punto de color del lenguaje
            c.fill_rounded(tx + 5, tty + 2, 4, 4, 2, icon_col);
            c.write_at(ndisp, tx + 14, tty, name_col);

            if buf.dirty {
                // Punto naranja de "sin guardar"
                let dot_x = tx + 14 + ndisp.len() * cw + 3;
                c.fill_rounded(dot_x, tty + 2, 4, 4, 2, IdePal::DIRTY);
            }
            tx += tab_w;
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // ÁREA DE EDICIÓN
    // ═════════════════════════════════════════════════════════════════════════
    let edit_y     = ty + TABS_H;
    let avail_h    = lay.bottom_y.saturating_sub(edit_y);
    let edit_h     = avail_h.saturating_sub(STATUS_H);
    let vis_rows   = edit_h / lh;
    let gutter_px  = if ide.show_ln { GUTTER_W * cw + 10 } else { 4 };

    let Some(buf) = &ide.buffers[ide.active] else {
        c.write_at("Sin archivo — Ctrl+N para nuevo", 20, edit_y + 20, IdePal::TEXT_DIM);
        return;
    };

    // Gutter
    if ide.show_ln {
        c.fill_rect(0, edit_y, gutter_px, edit_h, IdePal::GUTTER_BG);
        c.vline(gutter_px, edit_y, edit_h, IdePal::BORDER);
    }

    let mut lnbuf = [0u8; 8];
    for vis in 0..vis_rows {
        let lnum = buf.scroll + vis;
        if lnum >= buf.line_cnt { break; }
        let py = edit_y + vis * lh;
        let is_cur = lnum == buf.cursor_l;

        // ── Fondo de la línea del cursor (ANTES de dibujar texto) ────────────
        if is_cur {
            let bx = if ide.show_ln { gutter_px + 1 } else { 0 };
            let bw = fw.saturating_sub(bx);
            c.fill_rect(bx, py, bw, lh, IdePal::CURSOR_LINE);
        }

        // Número de línea
        if ide.show_ln {
            let lns = fmt_usize(lnum + 1, &mut lnbuf);
            let lnx = gutter_px.saturating_sub(lns.len() * cw + 4);
            let lfg = if is_cur { IdePal::LINE_NUM_ACT } else { IdePal::LINE_NUM };
            c.write_at(lns, lnx, py + 2, lfg);
        }

        // ── Contenido con highlighting ────────────────────────────────────────
        let mut line_buf = [0u8; MAX_LINE_LEN];
        let mut line_len = 0usize;
        if let Some(line) = buf.get_line(lnum) {
            line_len = line.len.min(MAX_LINE_LEN);
            line_buf[..line_len].copy_from_slice(&line.data[..line_len]);
        }
        let text_x  = gutter_px + 6;
        let max_col = fw.saturating_sub(text_x + 8) / cw;
        draw_hl_line(c, &line_buf[..line_len], buf.lang, text_x, py + 2, cw, max_col);

        // ── CARET (cursor de edición) ─────────────────────────────────────────
        // FIX: limpia exactamente cw × lh píxeles, luego dibuja el carácter
        if is_cur {
            let cx = text_x + buf.cursor_c * cw;
            if cx + cw <= fw {
                let cur_char = buf.get_line(lnum)
                    .map(|l| if buf.cursor_c < l.len { l.data[buf.cursor_c] } else { b' ' })
                    .unwrap_or(b' ');

                // 1. Bloque de color del cursor — ancho exacto = cw, alto = lh
                c.fill_rect(cx, py, cw, lh, IdePal::CURSOR_BG);

                // 2. Carácter encima (si imprimible)
                if cur_char >= 0x20 && cur_char < 0x7F {
                    let s = [cur_char];
                    if let Ok(cs) = core::str::from_utf8(&s) {
                        // write_at_bg garantiza que no pinte fuera del bloque
                        c.write_at_bg(cs, cx, py + 2, IdePal::CURSOR_FG, IdePal::CURSOR_BG);
                    }
                }
            }
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // STATUS BAR
    // ═════════════════════════════════════════════════════════════════════════
    let sy      = lay.bottom_y.saturating_sub(STATUS_H);
    let in_inp  = ide.input.is_active();
    let st_bg   = if in_inp { INPUT_BG } else if ide.status_err { IdePal::STATUS_ERR } else { IdePal::STATUS_BG };
    c.fill_rect(0, sy, fw, STATUS_H, st_bg);

    if in_inp {
        draw_input_overlay(c, &ide.input, 8, sy, fw, STATUS_H, cw, ch);
    } else {
        let sty = sy + (STATUS_H - ch) / 2;
        // Izquierda: Ln/Col
        let mut pb = [0u8; 32]; let mut pp = 0;
        let mut tmp = [0u8; 8];
        for b in b"Ln " { pb[pp] = *b; pp += 1; }
        for b in fmt_usize(buf.cursor_l + 1, &mut tmp).bytes() { pb[pp] = b; pp += 1; }
        for b in b"  Col " { pb[pp] = *b; pp += 1; }
        for b in fmt_usize(buf.cursor_c + 1, &mut tmp).bytes() { pb[pp] = b; pp += 1; }
        c.write_at(core::str::from_utf8(&pb[..pp]).unwrap_or(""), 8, sty, Color::WHITE);

        // Separador
        c.write_at("|", 120, sty, Color::new(0x00, 0x55, 0xAA));

        // Lenguaje
        c.write_at(buf.lang.label(), 132, sty, Color::WHITE);

        // Nombre centrado
        let ndisp = buf.name_str();
        let nx    = fw / 2 - ndisp.len() * cw / 2;
        c.write_at(ndisp, nx, sty, Color::WHITE);
        if buf.dirty {
            c.write_at("●", nx + ndisp.len() * cw + 4, sty, IdePal::DIRTY);
        }

        // Mensaje de status (derecha) — solo si hay algo que decir
        let msg = core::str::from_utf8(&ide.status_msg[..ide.status_len]).unwrap_or("");
        if !msg.is_empty() {
            c.write_at(msg, fw.saturating_sub(msg.len() * cw + 8), sty, Color::WHITE);
        }

        // Hint de ayuda muy sutil (extremo derecho inferior de toda la UI)
        // No en la barra — solo el botón [?] en menubar
    }

    // ═════════════════════════════════════════════════════════════════════════
    // DROPDOWN DE MENÚ
    // ═════════════════════════════════════════════════════════════════════════
    if let MenuState::Open(oi) = ide.menu {
        draw_dropdown(c, lay, oi, y0);
    }

    // ═════════════════════════════════════════════════════════════════════════
    // OVERLAY DE AYUDA (F1)
    // ═════════════════════════════════════════════════════════════════════════
    if ide.show_help {
        draw_help_overlay(c, lay);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_dropdown
// ─────────────────────────────────────────────────────────────────────────────

fn draw_dropdown(c: &mut Console, lay: &Layout, mi: usize, y0: usize) {
    if mi >= MENUS.len() { return; }
    let cw  = lay.font_w;
    let ch  = lay.font_h;
    let menu = &MENUS[mi];

    let mut mx_pos = 8usize;
    for i in 0..mi { mx_pos += MENUS[i].title.len() * cw + 16; }

    let max_l = menu.items.iter().map(|it| it.label.len()).max().unwrap_or(10);
    let max_s = menu.items.iter().map(|it| it.shortcut.len()).max().unwrap_or(0);
    let dd_w = (max_l + max_s + 5) * cw + 20;
    let dd_h = menu.items.len() * DROPDOWN_ITEM_H + 8;
    let dd_x = mx_pos;
    let dd_y = y0 + MENU_H;

    // Sombra sutil
    c.fill_rect(dd_x + 2, dd_y + 2, dd_w, dd_h, Color::new(0x00, 0x00, 0x00));
    // Fondo
    c.fill_rect(dd_x, dd_y, dd_w, dd_h, IdePal::DROP_BG);
    // Borde
    c.draw_rect(dd_x, dd_y, dd_w, dd_h, 1, IdePal::DROP_BOR);

    for (ii, item) in menu.items.iter().enumerate() {
        let iy  = dd_y + 4 + ii * DROPDOWN_ITEM_H;
        let tty = iy + (DROPDOWN_ITEM_H - ch) / 2;
        if item.action == MenuAction::Separator {
            c.hline(dd_x + 6, iy + DROPDOWN_ITEM_H / 2, dd_w - 12, IdePal::DROP_SEP);
        } else {
            c.write_at(item.label, dd_x + 12, tty, IdePal::MENU_FG);
            if !item.shortcut.is_empty() {
                let sx = dd_x + dd_w - item.shortcut.len() * cw - 10;
                c.write_at(item.shortcut, sx, tty, IdePal::MENU_SHORT);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_help_overlay — modal semitransparente con todos los atajos
// ─────────────────────────────────────────────────────────────────────────────

fn draw_help_overlay(c: &mut Console, lay: &Layout) {
    let fw = lay.fw;
    let cw = lay.font_w;
    let ch = lay.font_h;

    // Oscurecer fondo
    c.fill_rect_alpha(0, lay.content_y, fw, lay.bottom_y.saturating_sub(lay.content_y), Color::new(0x00, 0x00, 0x00), 160);

    let ox = (fw.saturating_sub(HELP_OVERLAY_W)) / 2;
    let oy = (lay.bottom_y.saturating_sub(HELP_OVERLAY_H)) / 2;

    // Fondo del overlay
    c.fill_rect(ox, oy, HELP_OVERLAY_W, HELP_OVERLAY_H, IdePal::OVERLAY_BG);
    c.draw_rect(ox, oy, HELP_OVERLAY_W, HELP_OVERLAY_H, 1, IdePal::ACCENT);

    // Título
    let title = "Atajos de teclado — IDE";
    c.fill_rect(ox, oy, HELP_OVERLAY_W, 24, IdePal::ACCENT);
    c.write_at(title, ox + 10, oy + (24 - ch) / 2, Color::WHITE);
    c.write_at("[Cualquier tecla para cerrar]", ox + HELP_OVERLAY_W - 29 * cw - 8, oy + (24 - ch) / 2, Color::new(0xCC, 0xCC, 0xFF));

    // Lista de atajos en dos columnas
    let entries: &[(&str, &str)] = &[
        ("Ctrl+N",     "Nuevo archivo"),
        ("Ctrl+S",     "Guardar"),
        ("Ctrl+W",     "Cerrar archivo"),
        ("Ctrl+Tab",   "Siguiente buffer"),
        ("Ctrl+Left",  "Buffer anterior"),
        ("─────────────", ""),
        ("Flechas",    "Mover cursor"),
        ("PageUp/Dn",  "Scroll rápido"),
        ("Home/End",   "Inicio/fin línea"),
        ("─────────────", ""),
        ("Enter",      "Nueva línea"),
        ("Tab",        "Indentar (4 esp)"),
        ("Backspace",  "Borrar izq"),
        ("Delete",     "Borrar der"),
        ("─────────────", ""),
        ("Clic menú",  "Abrir menús"),
        ("F1 / [?]",   "Mostrar esta ayuda"),
        ("Esc",        "Cerrar menús"),
    ];

    let col_w = HELP_OVERLAY_W / 2;
    let row_h = ch + 4;
    let entries_per_col = (HELP_OVERLAY_H - 32) / row_h;

    for (idx, (key, desc)) in entries.iter().enumerate() {
        let col  = idx / entries_per_col;
        let row  = idx % entries_per_col;
        if col > 1 { break; }
        let ex = ox + col * col_w + 12;
        let ey = oy + 30 + row * row_h;
        if key.starts_with('─') {
            c.hline(ex, ey + ch / 2, col_w - 20, Color::new(0x3A, 0x3A, 0x3A));
        } else {
            c.write_at(key, ex, ey, Color::new(0x9C, 0xCB, 0xFF));
            if !desc.is_empty() {
                let kw = 12 * cw;
                c.write_at(desc, ex + kw, ey, IdePal::TEXT);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_hl_line — syntax highlighting por columna
// ─────────────────────────────────────────────────────────────────────────────

fn draw_hl_line(c: &mut Console, line: &[u8], lang: Lang, x0: usize, y: usize, cw: usize, max_cols: usize) {
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