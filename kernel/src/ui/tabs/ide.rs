// ui/tabs/ide.rs — PORTIX Kernel v0.7.4
//
// IDE de texto con syntax highlighting.
//
// FIX CRÍTICO: PAGE_POOL ahora usa MaybeUninit para que el static vaya a .bss
// (todo ceros) en lugar de a .data (2 MB de bytes -1 en el binario).
//
// Antes:
//   static mut PAGE_POOL: [Page; 64] = [Page::empty(); 64];
//   → Page::empty() tiene next:-1, prev:-1  → no es todo-ceros
//   → Rust lo mete en .data → 2 MB DENTRO DEL BINARIO
//   → El bootloader no puede cargar el kernel completo → #UD al saltar
//
// Ahora:
//   static mut PAGE_POOL: MaybeUninit<[Page; 64]> = MaybeUninit::uninit();
//   → MaybeUninit::uninit() se representa como ceros en .bss
//   → 0 bytes en el binario, inicializado en runtime por init_page_pool()
//   → El bootloader carga el kernel sin problema

#![allow(dead_code)]

use core::mem::MaybeUninit;
use crate::drivers::input::keyboard::Key;
use crate::graphics::driver::framebuffer::{Color, Console, Layout};

// ── Paleta IDE ────────────────────────────────────────────────────────────────

pub struct IdePal;
impl IdePal {
    pub const BG:           Color = Color::new(0x0D, 0x10, 0x17);
    pub const GUTTER_BG:    Color = Color::new(0x08, 0x0A, 0x10);
    pub const HEADER_BG:    Color = Color::new(0x07, 0x0E, 0x1C);
    pub const TAB_ACTIVE:   Color = Color::new(0x0E, 0x22, 0x40);
    pub const TAB_INACTIVE: Color = Color::new(0x07, 0x0E, 0x18);
    pub const STATUS_BG:    Color = Color::new(0x10, 0x80, 0xFF);
    pub const TEXT:         Color = Color::new(0xCC, 0xD0, 0xD8);
    pub const LINE_NUM:     Color = Color::new(0x3A, 0x40, 0x55);
    pub const CURSOR_LINE:  Color = Color::new(0x12, 0x18, 0x28);
    pub const CURSOR_BG:    Color = Color::new(0xFF, 0xB0, 0x00);
    pub const CURSOR_FG:    Color = Color::new(0x00, 0x00, 0x00);
    pub const SYN_KEYWORD:  Color = Color::new(0x56, 0x9C, 0xD6);
    pub const SYN_STRING:   Color = Color::new(0xCE, 0x91, 0x78);
    pub const SYN_COMMENT:  Color = Color::new(0x6A, 0x99, 0x55);
    pub const SYN_NUMBER:   Color = Color::new(0xB5, 0xCE, 0xA8);
    pub const SYN_TYPE:     Color = Color::new(0x4E, 0xC9, 0xB0);
    pub const SYN_MACRO:    Color = Color::new(0xBD, 0x63, 0xC5);
    pub const SYN_PUNCT:    Color = Color::new(0xFF, 0xD7, 0x00);
    pub const DIRTY_DOT:    Color = Color::new(0xFF, 0x66, 0x00);
    pub const STATUS_FG:    Color = Color::WHITE;
    pub const BORDER:       Color = Color::new(0x20, 0x30, 0x48);
}

// ── Lenguaje ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum Lang { Plain, Rust, C, Asm }

impl Lang {
    pub fn from_name(name: &str) -> Self {
        if name.ends_with(".rs")                               { Lang::Rust }
        else if name.ends_with(".c") || name.ends_with(".h")  { Lang::C }
        else if name.ends_with(".asm") || name.ends_with(".s"){ Lang::Asm }
        else                                                   { Lang::Plain }
    }
    pub fn label(self) -> &'static str {
        match self { Lang::Rust=>"Rust", Lang::C=>"C", Lang::Asm=>"ASM", Lang::Plain=>"TXT" }
    }
}

// ── Buffer paginado (sin alloc) ───────────────────────────────────────────────

const MAX_LINES:          usize = 4096;
const MAX_LINE_LEN:       usize = 512;
const MAX_BUFFERS:        usize = 8;
const PAGE_LINES:         usize = 64;
const MAX_PAGES_TOTAL:    usize = 64;

// ── Line ──────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct Line {
    pub data: [u8; MAX_LINE_LEN],
    pub len:  usize,
}

impl Line {
    pub const fn empty() -> Self {
        Line { data: [0u8; MAX_LINE_LEN], len: 0 }
    }
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

// ── Page ──────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Page {
    lines: [Line; PAGE_LINES],
    used:  bool,
    next:  i32,   // índice en pool, -1 = sin enlace
    prev:  i32,
    count: usize,
}

impl Page {
    // Esta función existe para inicialización en runtime únicamente.
    // NO se usa como inicializador de un static (evitamos .data inflado).
    fn zeroed_sentinel() -> Self {
        Page {
            lines: [Line::empty(); PAGE_LINES],
            used:  false,
            next:  -1,
            prev:  -1,
            count: 0,
        }
    }
}

// ── Pool global ───────────────────────────────────────────────────────────────
//
// MaybeUninit::uninit() se representa como ceros en .bss → 0 bytes en el binario.
// Llamar a init_page_pool() ANTES de cualquier uso (en rust_main, antes de IdeState::new).
//
// Contraste con la versión anterior:
//   static mut PAGE_POOL: [Page; 64] = [Page::empty(); 64];
//   → Page contiene next:-1, prev:-1 (no-cero) → Rust lo emite en .data → ~2 MB en el ELF.
//   → El bootloader no puede cargar el ELF completo → salta a código parcial → #UD.

static mut PAGE_POOL: MaybeUninit<[Page; MAX_PAGES_TOTAL]> = MaybeUninit::uninit();

/// Inicializa el pool de páginas. Llamar UNA VEZ desde rust_main antes de IdeState::new().
/// Escribe los valores de sentinel (-1) en next/prev de cada página.
/// Como PAGE_POOL está en .bss, el _start ya lo limpió a ceros;
/// aquí solo sobreescribimos los campos next/prev con -1.
pub fn init_page_pool() {
    unsafe {
        let pool = PAGE_POOL.as_mut_ptr() as *mut [Page; MAX_PAGES_TOTAL];
        for i in 0..MAX_PAGES_TOTAL {
            let p = &mut (*pool)[i];
            p.used  = false;
            p.next  = -1;
            p.prev  = -1;
            p.count = 0;
            // lines ya son ceros (BSS), no hace falta inicializar cada byte.
            // Solo aseguramos los len=0 de cada Line (ya son ceros).
        }
    }
}

// Acceso al pool ya inicializado
#[inline(always)]
unsafe fn pool() -> &'static mut [Page; MAX_PAGES_TOTAL] {
    &mut *PAGE_POOL.as_mut_ptr()
}

unsafe fn alloc_page() -> Option<usize> {
    let pool = pool();
    for i in 0..MAX_PAGES_TOTAL {
        if !pool[i].used {
            pool[i].used  = true;
            pool[i].next  = -1;
            pool[i].prev  = -1;
            pool[i].count = 0;
            // Limpiar líneas de esta página
            for j in 0..PAGE_LINES {
                pool[i].lines[j] = Line::empty();
            }
            return Some(i);
        }
    }
    None
}

unsafe fn free_page(idx: usize) {
    if idx < MAX_PAGES_TOTAL {
        let pool = pool();
        pool[idx].used  = false;
        pool[idx].next  = -1;
        pool[idx].prev  = -1;
        pool[idx].count = 0;
    }
}

unsafe fn page_mut(idx: usize) -> &'static mut Page {
    &mut pool()[idx]
}

// ── TextBuffer ────────────────────────────────────────────────────────────────

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
}

impl TextBuffer {
    pub fn new_empty(name: &str) -> Self {
        let lang = Lang::from_name(name);
        let mut head = -1i32;
        unsafe {
            if let Some(pi) = alloc_page() {
                head = pi as i32;
            }
        }
        let mut tb = TextBuffer {
            head_page: head,
            tail_page: head,
            page_cnt:  if head >= 0 { 1 } else { 0 },
            line_cnt:  1,
            name:      [0u8; 256],
            name_len:  0,
            lang,
            dirty:     false,
            cursor_l:  0,
            cursor_c:  0,
            scroll:    0,
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
        self.line_cnt = 1;
        self.cursor_l = 0;
        self.cursor_c = 0;
        self.scroll   = 0;

        unsafe {
            let pidx = self.head_page as usize;
            let p = page_mut(pidx);
            p.count = 1;
            p.lines[0] = Line::empty();
        }

        for &b in data {
            if b == b'\n' {
                cur_line_idx = cur_line_idx.saturating_add(1);
                self.line_cnt = self.line_cnt.saturating_add(1);
                let (pidx, slot) = self.ensure_slot_for_line(cur_line_idx);
                unsafe {
                    let p = page_mut(pidx);
                    p.lines[slot] = Line::empty();
                }
            } else if b != b'\r' {
                let (pidx, slot) = self.ensure_slot_for_line(cur_line_idx);
                unsafe {
                    let p = page_mut(pidx);
                    let _ = p.lines[slot].insert(p.lines[slot].len, b);
                }
            }
        }

        self.cursor_l = 0;
        self.cursor_c = 0;
        self.dirty    = false;
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
                out[ppos] = b'\n';
                ppos += 1;
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
        if self.cursor_l < self.scroll {
            self.scroll = self.cursor_l;
        } else if self.cursor_l >= self.scroll + visible_rows {
            self.scroll = self.cursor_l + 1 - visible_rows;
        }
    }

    fn insert_char(&mut self, ch: u8) {
        let cur_c = self.cursor_c;
        let inserted = {
            if let Some(line) = self.get_line_mut(self.cursor_l) {
                line.insert(cur_c, ch)
            } else { false }
        };
        if inserted {
            self.cursor_c = self.cursor_c.saturating_add(1);
            self.dirty = true;
            return;
        }
        self.insert_newline();
        let cur_c2 = self.cursor_c;
        if let Some(line) = self.get_line_mut(self.cursor_l) {
            let _ = line.insert(cur_c2, ch);
            self.cursor_c = self.cursor_c.saturating_add(1);
            self.dirty = true;
        }
    }

    fn insert_newline(&mut self) {
        let l = self.cursor_l;
        if self.line_cnt >= MAX_LINES { return; }

        let mut cur = Line::empty();
        if let Some(existing) = self.get_line(l) { cur = *existing; }
        let split_at = self.cursor_c.min(cur.len);
        let old_len  = cur.len;

        let mut new_line = Line::empty();
        let tail_len = old_len.saturating_sub(split_at);
        if tail_len > 0 {
            new_line.data[..tail_len].copy_from_slice(&cur.data[split_at..old_len]);
            new_line.len = tail_len;
        }

        if let Some(cur_mut) = self.get_line_mut(l) { cur_mut.len = split_at; }

        self.insert_line_at(l + 1, new_line);
        self.line_cnt  = self.line_cnt.saturating_add(1);
        self.cursor_l  = self.cursor_l.saturating_add(1);
        self.cursor_c  = 0;
        self.dirty     = true;
    }

    fn backspace(&mut self) {
        let cur_c = self.cursor_c;
        if cur_c > 0 {
            let l = self.cursor_l;
            if let Some(line) = self.get_line_mut(l) {
                let _ = line.remove(cur_c.saturating_sub(1));
            }
            self.cursor_c = cur_c.saturating_sub(1);
            self.dirty    = true;
            return;
        }
        if self.cursor_l > 0 {
            let prev = self.cursor_l - 1;
            let cur  = self.cursor_l;
            let mut cur_line = Line::empty();
            if let Some(c) = self.get_line(cur) { cur_line = *c; }
            let prev_len = self.get_line(prev).map(|l| l.len).unwrap_or(0);
            let copy_len = cur_line.len.min(MAX_LINE_LEN.saturating_sub(prev_len));
            if copy_len > 0 {
                if let Some(prev_mut) = self.get_line_mut(prev) {
                    prev_mut.data[prev_len..prev_len + copy_len]
                        .copy_from_slice(&cur_line.data[..copy_len]);
                    prev_mut.len = prev_len + copy_len;
                }
            }
            self.delete_line_at(cur);
            self.line_cnt  = self.line_cnt.saturating_sub(1);
            self.cursor_l  = self.cursor_l.saturating_sub(1);
            self.cursor_c  = prev_len;
            self.dirty     = true;
        }
    }

    fn delete_forward(&mut self) {
        let l     = self.cursor_l;
        let cur_c = self.cursor_c;
        if let Some(line) = self.get_line_mut(l) {
            if cur_c < line.len { line.remove(cur_c); self.dirty = true; return; }
        }
        if l + 1 < self.line_cnt {
            let next_idx  = l + 1;
            let mut next_line = Line::empty();
            if let Some(nl) = self.get_line(next_idx) { next_line = *nl; }
            let cur_len  = self.get_line(l).map(|l| l.len).unwrap_or(0);
            let copy_len = next_line.len.min(MAX_LINE_LEN.saturating_sub(cur_len));
            if copy_len > 0 {
                if let Some(cur_mut) = self.get_line_mut(l) {
                    cur_mut.data[cur_len..cur_len + copy_len]
                        .copy_from_slice(&next_line.data[..copy_len]);
                    cur_mut.len = cur_len + copy_len;
                }
            }
            self.delete_line_at(next_idx);
            self.line_cnt = self.line_cnt.saturating_sub(1);
            self.dirty    = true;
        }
    }

    // ── Helpers de paginación ─────────────────────────────────────────────────

    fn find_page_for_line(&self, line_idx: usize) -> Option<(usize, usize)> {
        if self.head_page < 0 { return None; }
        let mut pidx = self.head_page as usize;
        let mut acc  = 0usize;
        unsafe {
            let pool = &*PAGE_POOL.as_ptr();
            loop {
                let p = &pool[pidx];
                if acc + p.count > line_idx {
                    return Some((pidx, line_idx - acc));
                }
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
        if let Some((pidx, off)) = self.find_page_for_line(line_idx) {
            return (pidx, off);
        }
        unsafe {
            if let Some(pi) = alloc_page() {
                let pi = pi as usize;
                if self.tail_page >= 0 {
                    let t = page_mut(self.tail_page as usize);
                    t.next = pi as i32;
                    page_mut(pi).prev = self.tail_page;
                    self.tail_page = pi as i32;
                } else {
                    self.head_page = pi as i32;
                    self.tail_page = pi as i32;
                }
                self.page_cnt = self.page_cnt.saturating_add(1);
                page_mut(pi).count = 1;
                page_mut(pi).lines[0] = Line::empty();
                return (pi, 0);
            } else {
                return (self.head_page as usize, 0);
            }
        }
    }

    fn append_empty_line(&mut self) {
        if self.head_page < 0 {
            unsafe {
                if let Some(pi) = alloc_page() {
                    self.head_page = pi as i32;
                    self.tail_page = pi as i32;
                    self.page_cnt  = 1;
                }
            }
        }
        unsafe {
            let tail = self.tail_page as usize;
            let p    = page_mut(tail);
            if p.count < PAGE_LINES {
                p.lines[p.count] = Line::empty();
                p.count += 1;
            } else if let Some(pi) = alloc_page() {
                let pi = pi as usize;
                page_mut(pi).count = 1;
                page_mut(pi).lines[0] = Line::empty();
                p.next = pi as i32;
                page_mut(pi).prev = self.tail_page;
                self.tail_page = pi as i32;
                self.page_cnt  = self.page_cnt.saturating_add(1);
            } else {
                let head = self.head_page as usize;
                let ph   = page_mut(head);
                ph.lines[ph.count % PAGE_LINES] = Line::empty();
                ph.count = ph.count.saturating_add(1);
            }
        }
        self.line_cnt = self.line_cnt.saturating_add(1);
    }

    fn get_line(&self, line_idx: usize) -> Option<&Line> {
        self.find_page_for_line(line_idx).map(|(pidx, off)| unsafe {
            &(*PAGE_POOL.as_ptr())[pidx].lines[off]
        })
    }

    fn get_line_mut(&mut self, line_idx: usize) -> Option<&mut Line> {
        if let Some((pidx, off)) = self.find_page_for_line(line_idx) {
            unsafe { Some(&mut (*PAGE_POOL.as_mut_ptr())[pidx].lines[off]) }
        } else {
            None
        }
    }

    fn insert_line_at(&mut self, at: usize, line: Line) {
        if at > self.line_cnt { return; }
        if at == self.line_cnt {
            self.append_empty_line();
            if let Some(dst) = self.get_line_mut(at) { *dst = line; }
            return;
        }
        self.append_empty_line();
        if self.line_cnt < 2 {
            if let Some(dst) = self.get_line_mut(at) { *dst = line; }
            return;
        }
        let mut i = self.line_cnt.saturating_sub(2);
        loop {
            if i < at { break; }
            if let Some(src) = self.get_line(i) {
                let tmp = *src;
                if let Some(dst) = self.get_line_mut(i + 1) { *dst = tmp; }
            }
            if i == 0 { break; }
            i -= 1;
        }
        if let Some(dst) = self.get_line_mut(at) { *dst = line; }
    }

    fn delete_line_at(&mut self, at: usize) {
        if at >= self.line_cnt { return; }
        let mut i = at;
        while i + 1 < self.line_cnt {
            if let Some(src) = self.get_line(i + 1) {
                let tmp = *src;
                if let Some(dst) = self.get_line_mut(i) { *dst = tmp; }
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
                        let prev = p.prev;
                        let next = p.next;
                        if prev >= 0 { page_mut(prev as usize).next = next; }
                        else         { self.head_page = if next >= 0 { next } else { -1 }; }
                        if next >= 0 { page_mut(next as usize).prev = prev; }
                        else         { self.tail_page = if prev >= 0 { prev } else { -1 }; }
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
            let next = unsafe { (*PAGE_POOL.as_ptr())[cur as usize].next };
            unsafe { free_page(cur as usize); }
            cur = next;
        }
        self.head_page = -1;
        self.tail_page = -1;
        self.page_cnt  = 0;
        self.line_cnt  = 0;
    }
}

// ── IdeState ──────────────────────────────────────────────────────────────────

pub struct IdeState {
    pub buffers:    [Option<TextBuffer>; MAX_BUFFERS],
    pub active:     usize,
    pub buf_count:  usize,
    pub status_msg: [u8; 80],
    pub status_len: usize,
    pub status_err: bool,
}

impl IdeState {
    pub fn new() -> Self {
        let mut ide = IdeState {
            buffers:    core::array::from_fn(|_| None),
            active:     0,
            buf_count:  0,
            status_msg: [0u8; 80],
            status_len: 0,
            status_err: false,
        };
        ide.open_new("untitled.txt");
        ide
    }

    pub fn open_new(&mut self, name: &str) -> bool {
        if self.buf_count >= MAX_BUFFERS { return false; }
        for i in 0..MAX_BUFFERS {
            if self.buffers[i].is_none() {
                self.buffers[i] = Some(TextBuffer::new_empty(name));
                self.active     = i;
                self.buf_count += 1;
                self.set_status("Nuevo archivo creado.", false);
                return true;
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
                self.active     = i;
                self.buf_count += 1;
                self.set_status("Archivo abierto.", false);
                return true;
            }
        }
        false
    }

    pub fn close_active(&mut self) {
        if let Some(mut buf) = self.buffers[self.active].take() {
            buf.clear_pages();
        }
        if self.buf_count > 0 { self.buf_count -= 1; }
        for i in 0..MAX_BUFFERS {
            if self.buffers[i].is_some() { self.active = i; return; }
        }
        self.active = 0;
        self.open_new("untitled.txt");
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

    fn set_status(&mut self, msg: &str, is_err: bool) {
        let n = msg.len().min(80);
        self.status_msg[..n].copy_from_slice(msg.as_bytes());
        self.status_len = n;
        self.status_err = is_err;
    }

    pub fn handle_key(&mut self, key: Key, ctrl: bool, visible_rows: usize) -> bool {
        if ctrl {
            match key {
                Key::Char(b's') | Key::Char(b'S') => {
                    if let Some(buf) = self.buffers[self.active].as_mut() { buf.dirty = false; }
                    self.set_status("Guardado.", false);
                    return true;
                }
                Key::Char(b'n') | Key::Char(b'N') => { self.open_new("untitled.txt"); return true; }
                Key::Char(b'w') | Key::Char(b'W') => { self.close_active(); return true; }
                Key::Tab | Key::Right => { self.switch_next(); return true; }
                Key::Left             => { self.switch_prev(); return true; }
                _ => {}
            }
        }

        let Some(buf) = self.buffers[self.active].as_mut() else { return false };

        match key {
            Key::Up => {
                if buf.cursor_l > 0 { buf.cursor_l -= 1; buf.clamp_col(); }
                buf.ensure_scroll(visible_rows);
            }
            Key::Down => {
                if buf.cursor_l + 1 < buf.line_cnt { buf.cursor_l += 1; buf.clamp_col(); }
                buf.ensure_scroll(visible_rows);
            }
            Key::Left => {
                if buf.cursor_c > 0 { buf.cursor_c -= 1; }
                else if buf.cursor_l > 0 {
                    buf.cursor_l -= 1;
                    buf.cursor_c = buf.get_line(buf.cursor_l).map(|l| l.len).unwrap_or(0);
                }
                buf.ensure_scroll(visible_rows);
            }
            Key::Right => {
                let ll = buf.cur_line_len();
                if buf.cursor_c < ll { buf.cursor_c += 1; }
                else if buf.cursor_l + 1 < buf.line_cnt {
                    buf.cursor_l += 1;
                    buf.cursor_c = 0;
                }
                buf.ensure_scroll(visible_rows);
            }
            Key::Home => { buf.cursor_c = 0; }
            Key::End  => { buf.cursor_c = buf.cur_line_len(); }
            Key::PageUp => {
                buf.cursor_l = buf.cursor_l.saturating_sub(visible_rows);
                buf.clamp_col();
                buf.ensure_scroll(visible_rows);
            }
            Key::PageDown => {
                buf.cursor_l = (buf.cursor_l + visible_rows).min(buf.line_cnt.saturating_sub(1));
                buf.clamp_col();
                buf.ensure_scroll(visible_rows);
            }
            Key::Enter     => { buf.insert_newline(); buf.ensure_scroll(visible_rows); }
            Key::Tab       => { for _ in 0..4 { buf.insert_char(b' '); } }
            Key::Backspace => { buf.backspace(); buf.ensure_scroll(visible_rows); }
            Key::Delete    => { buf.delete_forward(); }
            Key::Char(c) if c >= 0x20 && c < 0x7F => { buf.insert_char(c); }
            _ => return false,
        }
        true
    }

    pub fn get_save_data(&self, out: &mut [u8; 65536]) -> usize {
        if let Some(buf) = &self.buffers[self.active] { buf.serialize(out) } else { 0 }
    }
}

// ── Syntax highlighting ────────────────────────────────────────────────────────

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
            let delim = line[i];
            in_string = delim;
            let start = i;
            i += 1;
            while i < line.len() {
                if line[i] == b'\\' { i += 2; continue; }
                if line[i] == delim { i += 1; in_string = 0; break; }
                i += 1;
            }
            emit(start, i, IdePal::SYN_STRING);
            continue;
        }
        if in_string != 0 { i += 1; continue; }

        if lang == Lang::Rust && is_ident_start(line[i]) {
            let start = i;
            while i < line.len() && is_ident(line[i]) { i += 1; }
            if i < line.len() && line[i] == b'!' {
                i += 1;
                emit(start, i, IdePal::SYN_MACRO);
                continue;
            }
            let word = &line[start..i];
            if      is_keyword(word, RUST_KEYWORDS) { emit(start, i, IdePal::SYN_KEYWORD); }
            else if is_keyword(word, RUST_TYPES)    { emit(start, i, IdePal::SYN_TYPE); }
            else                                    { emit(start, i, IdePal::TEXT); }
            continue;
        }

        if lang == Lang::C && is_ident_start(line[i]) {
            let start = i;
            while i < line.len() && is_ident(line[i]) { i += 1; }
            let word = &line[start..i];
            if is_keyword(word, C_KEYWORDS) { emit(start, i, IdePal::SYN_KEYWORD); }
            else                            { emit(start, i, IdePal::TEXT); }
            continue;
        }

        if lang == Lang::Asm {
            if line[i] == b'.' {
                let start = i; i += 1;
                while i < line.len() && is_ident(line[i]) { i += 1; }
                emit(start, i, IdePal::SYN_MACRO);
                continue;
            }
            if is_ident_start(line[i]) {
                let start = i;
                while i < line.len() && is_ident(line[i]) { i += 1; }
                if i < line.len() && line[i] == b':' {
                    i += 1; emit(start, i, IdePal::SYN_TYPE);
                } else {
                    emit(start, i, IdePal::SYN_KEYWORD);
                }
                continue;
            }
        }

        if line[i].is_ascii_digit()
            || (line[i] == b'0' && i + 1 < line.len() && line[i + 1] == b'x')
        {
            let start = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') { i += 1; }
            emit(start, i, IdePal::SYN_NUMBER);
            continue;
        }

        if b"{}[]();,.<>!&|^~%+-*/=@#".contains(&line[i]) {
            emit(i, i + 1, IdePal::SYN_PUNCT);
            i += 1;
            continue;
        }

        emit(i, i + 1, IdePal::TEXT);
        i += 1;
    }
}

fn is_ident_start(b: u8) -> bool { b.is_ascii_alphabetic() || b == b'_' }
fn is_ident(b: u8) -> bool       { b.is_ascii_alphanumeric() || b == b'_' }
fn is_keyword(word: &[u8], list: &[&[u8]]) -> bool {
    list.iter().any(|&k| k == word)
}

// ── Renderizado ────────────────────────────────────────────────────────────────

const GUTTER_W: usize = 5;

pub fn draw_ide_tab(c: &mut Console, lay: &Layout, ide: &IdeState) {
    let fw  = lay.fw;
    let fh  = lay.fh;
    let cw  = lay.font_w;
    let ch  = lay.font_h;
    let lh  = ch + 2;
    let y0  = lay.content_y;

    c.fill_rect(0, y0, fw, fh - y0, IdePal::BG);

    let tab_bar_h = ch + 6;
    c.fill_rect(0, y0, fw, tab_bar_h, IdePal::HEADER_BG);
    c.hline(0, y0 + tab_bar_h - 1, fw, IdePal::BORDER);

    let mut tx = 4usize;
    for i in 0..MAX_BUFFERS {
        if let Some(buf) = &ide.buffers[i] {
            let is_active = i == ide.active;
            let label     = buf.name_str();
            let tab_w     = label.len() * cw + 20;
            if is_active {
                c.fill_rect(tx, y0, tab_w, tab_bar_h, IdePal::TAB_ACTIVE);
                c.fill_rect(tx, y0, tab_w, 2,         IdePal::CURSOR_BG);
            } else {
                c.fill_rect(tx, y0, tab_w, tab_bar_h, IdePal::TAB_INACTIVE);
            }
            c.vline(tx + tab_w - 1, y0, tab_bar_h, IdePal::BORDER);
            let fg = if is_active { Color::WHITE } else { Color::new(0x60, 0x70, 0x90) };
            c.write_at(label, tx + 8, y0 + 3, fg);
            if buf.dirty { c.fill_rect(tx + tab_w - 10, y0 + 4, 5, 5, IdePal::DIRTY_DOT); }
            tx += tab_w;
        }
    }

    let edit_y       = y0 + tab_bar_h + 1;
    let status_h     = ch + 6;
    let edit_h       = fh.saturating_sub(edit_y + status_h);
    let visible_rows = edit_h / lh;
    let gutter_px    = GUTTER_W * cw + 4;

    let Some(buf) = &ide.buffers[ide.active] else {
        c.write_at("Sin archivo. Ctrl+N = nuevo.", 20, edit_y + 20, IdePal::LINE_NUM);
        return;
    };

    c.fill_rect(0, edit_y, gutter_px, edit_h, IdePal::GUTTER_BG);
    c.vline(gutter_px, edit_y, edit_h, IdePal::BORDER);

    let mut lnbuf = [0u8; 8];
    for vis in 0..visible_rows {
        let lnum = buf.scroll + vis;
        if lnum >= buf.line_cnt { break; }
        let py        = edit_y + vis * lh;
        let is_cursor = lnum == buf.cursor_l;
        if is_cursor {
            c.fill_rect(gutter_px + 1, py, fw - gutter_px - 1, lh, IdePal::CURSOR_LINE);
        }
        let lnstr = fmt_usize(lnum + 1, &mut lnbuf);
        let lnx   = gutter_px.saturating_sub(lnstr.len() * cw + 4);
        let ln_fg = if is_cursor { Color::WHITE } else { IdePal::LINE_NUM };
        c.write_at(lnstr, lnx, py + 1, ln_fg);

        let mut line_buf = [0u8; MAX_LINE_LEN];
        let mut line_len = 0usize;
        if let Some(line) = buf.get_line(lnum) {
            line_len = line.len.min(MAX_LINE_LEN);
            line_buf[..line_len].copy_from_slice(&line.data[..line_len]);
        }

        let text_x   = gutter_px + 4;
        let max_cols = (fw.saturating_sub(text_x + 10)) / cw;
        draw_highlighted_line(c, &line_buf[..line_len], buf.lang, text_x, py + 1, cw, max_cols);

        if is_cursor {
            let cx = text_x + buf.cursor_c * cw;
            if cx + cw <= fw {
                let cur_char = if let Some(line) = buf.get_line(lnum) {
                    if buf.cursor_c < line.len { line.data[buf.cursor_c] } else { b' ' }
                } else { b' ' };
                c.fill_rect(cx, py, cw, lh, IdePal::CURSOR_BG);
                let s = [cur_char];
                c.write_at_bg(
                    core::str::from_utf8(&s).unwrap_or(" "),
                    cx, py + 1,
                    IdePal::CURSOR_FG, IdePal::CURSOR_BG,
                );
            }
        }
    }

    let sy    = fh.saturating_sub(status_h);
    let st_bg = if ide.status_err { Color::new(0x80, 0x10, 0x10) } else { IdePal::STATUS_BG };
    c.fill_rect(0, sy, fw, status_h, st_bg);

    let mut pos_buf = [0u8; 32]; let mut pp = 0;
    let mut tmp = [0u8; 8];
    for b in b"Ln "    { pos_buf[pp] = *b; pp += 1; }
    for b in fmt_usize(buf.cursor_l + 1, &mut tmp).bytes() { pos_buf[pp] = b; pp += 1; }
    for b in b", Col " { pos_buf[pp] = *b; pp += 1; }
    for b in fmt_usize(buf.cursor_c + 1, &mut tmp).bytes() { pos_buf[pp] = b; pp += 1; }
    c.write_at(core::str::from_utf8(&pos_buf[..pp]).unwrap_or(""), 8, sy + 3, IdePal::STATUS_FG);
    c.write_at(buf.lang.label(), fw / 2 - 20, sy + 3, IdePal::STATUS_FG);
    c.write_at(buf.name_str(),   fw / 2 + 20, sy + 3, IdePal::STATUS_FG);

    let msg = core::str::from_utf8(&ide.status_msg[..ide.status_len]).unwrap_or("");
    if !msg.is_empty() {
        c.write_at(msg, fw.saturating_sub(msg.len() * cw + 12), sy + 3, IdePal::STATUS_FG);
    }
    c.write_at(
        "^S Guardar  ^N Nuevo  ^W Cerrar  ^Tab Siguiente",
        8, sy + 3 + ch + 2,
        Color::new(0xA0, 0xC0, 0xFF),
    );
}

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