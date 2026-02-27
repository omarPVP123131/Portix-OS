// kernel/src/framebuffer.rs — PORTIX v7 — Double Buffer
// FIX: Agregado soporte 24bpp en present() → pantalla negra resuelta.
// El modo VESA 0x118 (1024x768) es 24bpp en QEMU.
// El double buffer interno siempre es 32bpp; present() convierte según bpp real del LFB.
#![allow(dead_code)]

const LFB_PTR_ADDR: *const u32 = 0x9004 as *const u32;
const WIDTH_ADDR:   *const u16 = 0x9008 as *const u16;
const HEIGHT_ADDR:  *const u16 = 0x900A as *const u16;
const PITCH_ADDR:   *const u16 = 0x900C as *const u16;
const BPP_ADDR:     *const u8  = 0x900E as *const u8;

// Back buffer en RAM física: 6 MB, identity-mapped por stage2.
// Kernel en 0x10000 (~120 KB), stack en 0x7FF00 → seguro.
// 0x600000 + 3 MB (1024×768×4) = 0x900000 → dentro del mapa de 128 MB.
const BACKBUF_ADDR: u64 = 0x0060_0000;

// ── Color ─────────────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const BLACK:        Color = Color(0x000000);
    pub const WHITE:        Color = Color(0xFFFFFF);
    pub const RED:          Color = Color(0xEE2222);
    pub const GREEN:        Color = Color(0x00CC44);
    pub const BLUE:         Color = Color(0x0055FF);
    pub const YELLOW:       Color = Color(0xFFFF00);
    pub const PORTIX_BG:    Color = Color(0x01080F);
    pub const PORTIX_PANEL: Color = Color(0x030C18);
    pub const PORTIX_GOLD:  Color = Color(0xFFD700);
    pub const PORTIX_AMBER: Color = Color(0xFFAA00);
    pub const HEADER_BG:    Color = Color(0x060F1E);
    pub const TAB_ACTIVE:   Color = Color(0x0E2240);
    pub const TAB_INACTIVE: Color = Color(0x030912);
    pub const SEPARATOR:    Color = Color(0x0E1828);
    pub const SEP_BRIGHT:   Color = Color(0x1A3050);
    pub const GRAY:         Color = Color(0x506070);
    pub const LIGHT_GRAY:   Color = Color(0xAABBCC);
    pub const MID_GRAY:     Color = Color(0x2A3848);
    pub const DARK_GRAY:    Color = Color(0x080E16);
    pub const CYAN:         Color = Color(0x00CCEE);
    pub const TEAL:         Color = Color(0x00998B);
    pub const ORANGE:       Color = Color(0xFF6600);
    pub const TERM_BG:      Color = Color(0x000509);
    pub const CARD_BG:      Color = Color(0x040E1C);
    pub const ACCENT_BLUE:  Color = Color(0x1060C0);
    pub const NEON_GREEN:   Color = Color(0x00FF88);
    pub const MAGENTA:      Color = Color(0xFF00CC);
    pub const PINK:         Color = Color(0xFF88CC);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self(((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }
    pub const fn r(self) -> u8 { ((self.0 >> 16) & 0xFF) as u8 }
    pub const fn g(self) -> u8 { ((self.0 >>  8) & 0xFF) as u8 }
    pub const fn b(self) -> u8 { ( self.0        & 0xFF) as u8 }
    pub const fn dim(self, a: u8) -> Self {
        let r = (self.r() as u32) * a as u32 / 255;
        let g = (self.g() as u32) * a as u32 / 255;
        let b = (self.b() as u32) * a as u32 / 255;
        Color((r << 16) | (g << 8) | b)
    }
    pub const fn blend(self, other: Color, a: u8) -> Self {
        let ia = 255 - a as u32;
        let a  = a as u32;
        let r = (self.r() as u32 * a + other.r() as u32 * ia) / 255;
        let g = (self.g() as u32 * a + other.g() as u32 * ia) / 255;
        let b = (self.b() as u32 * a + other.b() as u32 * ia) / 255;
        Color((r << 16) | (g << 8) | b)
    }
}

// ── Layout ────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy)]
pub struct Layout {
    pub fw:        usize,
    pub fh:        usize,
    pub header_h:  usize,
    pub gold_h:    usize,
    pub tab_h:     usize,
    pub tab_y:     usize,
    pub tab_w:     usize,
    pub content_y: usize,
    pub bottom_y:  usize,
    pub pad:       usize,
    pub col_div:   usize,
    pub right_x:   usize,
    pub line_h:    usize,
    pub font_w:    usize,
    pub font_h:    usize,
}

impl Layout {
    pub fn new(fw: usize, fh: usize) -> Self {
        let header_h  = clamp(fh / 14, 44, 60);
        let gold_h    = 3;
        let tab_h     = clamp(fh / 32, 20, 28);
        let tab_y     = header_h + gold_h;
        let content_y = tab_y + tab_h + 2;
        let bottom_y  = fh.saturating_sub(clamp(fh / 34, 18, 24));
        let pad       = clamp(fw / 56, 10, 30);
        let col_div   = fw * 5 / 12;
        let tab_w     = 152;
        Layout {
            fw, fh, header_h, gold_h, tab_h, tab_y, tab_w,
            content_y, bottom_y, pad,
            col_div, right_x: col_div + pad + 4,
            line_h: 15, font_w: 8, font_h: 8,
        }
    }
    pub fn left_w(&self)    -> usize { self.col_div.saturating_sub(self.pad * 2) }
    pub fn right_w(&self)   -> usize { self.fw.saturating_sub(self.right_x + self.pad) }
    pub fn content_h(&self) -> usize { self.bottom_y.saturating_sub(self.content_y) }
    pub fn content_lines(&self) -> usize { self.content_h() / self.line_h }

    pub fn tab_hit(&self, mx: i32, my: i32) -> i32 {
        let x = mx as usize;
        let y = my as usize;
        if y < self.tab_y || y >= self.tab_y + self.tab_h + 2 { return -1; }
        let idx = x / self.tab_w;
        if idx < 3 { idx as i32 } else { -1 }
    }
}

fn clamp(v: usize, lo: usize, hi: usize) -> usize { v.max(lo).min(hi) }

// ── Framebuffer con doble buffer ──────────────────────────────────────────────
pub struct Framebuffer {
    lfb:        u64,
    backbuf:    u64,
    pub width:  usize,
    pub height: usize,
    lfb_pitch:  usize,
    bpp:        u8,
    back_pitch: usize, // siempre width * 4 (32bpp interno)
}

impl Framebuffer {
    pub fn new() -> Self {
        unsafe {
            let bpp_raw = core::ptr::read_volatile(BPP_ADDR);
            let w_raw   = core::ptr::read_volatile(WIDTH_ADDR)  as usize;
            let h_raw   = core::ptr::read_volatile(HEIGHT_ADDR) as usize;
            let p_raw   = core::ptr::read_volatile(PITCH_ADDR)  as usize;
            let lfb     = core::ptr::read_volatile(LFB_PTR_ADDR) as u64;

            let (w, h, lfb_pitch, bpp) = if w_raw == 0 || h_raw == 0 {
                (1024, 768, 3072, 24u8) // fallback: 1024x768x24 (QEMU default)
            } else {
                let bpp = if bpp_raw < 15 { 24 } else { bpp_raw }; // min razonable
                let bytes_per_px = (bpp as usize + 7) / 8;
                let p = if p_raw == 0 { w_raw * bytes_per_px } else { p_raw };
                (w_raw, h_raw, p, bpp)
            };

            let back_pitch = w * 4; // back buffer siempre 32bpp

            // Limpiar back buffer
            let total = w * h;
            let bp = BACKBUF_ADDR as *mut u32;
            for i in 0..total {
                core::ptr::write_volatile(bp.add(i), 0);
            }

            Self { lfb, backbuf: BACKBUF_ADDR, width: w, height: h,
                   lfb_pitch, bpp, back_pitch }
        }
    }

    pub fn lfb_addr(&self) -> u64  { self.lfb }
    pub fn is_valid(&self) -> bool  { self.lfb != 0 }
    pub fn bpp(&self)      -> u8    { self.bpp }

    // ── draw_pixel → back buffer (RAM, siempre 32bpp) ─────────────────────────
    #[inline(always)]
    pub unsafe fn draw_pixel(&self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height || self.backbuf == 0 { return; }
        let off = y * self.back_pitch + x * 4;
        core::ptr::write_volatile((self.backbuf + off as u64) as *mut u32, color.0);
    }

    #[inline(always)]
    pub unsafe fn read_back_pixel(&self, x: usize, y: usize) -> Color {
        if x >= self.width || y >= self.height || self.backbuf == 0 { return Color::BLACK; }
        let off = y * self.back_pitch + x * 4;
        Color(core::ptr::read_volatile((self.backbuf + off as u64) as *const u32))
    }

    // ── present() — blit back buffer → LFB real ───────────────────────────────
    // FIX: ahora soporta 24bpp (modo VESA 0x118 en QEMU = 1024x768x24).
    pub fn present(&self) {
        if self.lfb == 0 || self.backbuf == 0 { return; }
        unsafe {
            match self.bpp {
                32 => {
                    if self.lfb_pitch == self.back_pitch {
                        // Mismo pitch: copia plana directa
                        let total = self.width * self.height;
                        let src   = self.backbuf as *const u32;
                        let dst   = self.lfb    as *mut   u32;
                        for i in 0..total {
                            core::ptr::write_volatile(dst.add(i),
                                core::ptr::read(src.add(i)));
                        }
                    } else {
                        // Pitch diferente: fila por fila
                        for y in 0..self.height {
                            let s = (self.backbuf + (y * self.back_pitch) as u64) as *const u32;
                            let d = (self.lfb     + (y * self.lfb_pitch ) as u64) as *mut   u32;
                            for x in 0..self.width {
                                core::ptr::write_volatile(d.add(x), core::ptr::read(s.add(x)));
                            }
                        }
                    }
                }

                // ── FIX PRINCIPAL: soporte 24bpp ──────────────────────────────
                // El back buffer tiene píxeles 0x00RRGGBB en u32.
                // El LFB 24bpp espera bytes en orden B, G, R (little-endian BGR).
                24 => {
                    for y in 0..self.height {
                        let src_row = (self.backbuf + (y * self.back_pitch) as u64) as *const u32;
                        let dst_row = (self.lfb     + (y * self.lfb_pitch ) as u64) as *mut u8;
                        for x in 0..self.width {
                            let px = core::ptr::read(src_row.add(x));
                            // px = 0x00RRGGBB → escribir B, G, R al LFB
                            let b = ( px        & 0xFF) as u8;
                            let g = ((px >>  8) & 0xFF) as u8;
                            let r = ((px >> 16) & 0xFF) as u8;
                            let base = x * 3;
                            core::ptr::write_volatile(dst_row.add(base),     b);
                            core::ptr::write_volatile(dst_row.add(base + 1), g);
                            core::ptr::write_volatile(dst_row.add(base + 2), r);
                        }
                    }
                }

                16 => {
                    for y in 0..self.height {
                        let s = (self.backbuf + (y * self.back_pitch) as u64) as *const u32;
                        let d = (self.lfb     + (y * self.lfb_pitch ) as u64) as *mut   u16;
                        for x in 0..self.width {
                            let px = core::ptr::read(s.add(x));
                            let r  = ((px >> 16) & 0xFF) as u16;
                            let g  = ((px >>  8) & 0xFF) as u16;
                            let b  = ( px        & 0xFF) as u16;
                            let v  = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3);
                            core::ptr::write_volatile(d.add(x), v);
                        }
                    }
                }

                _ => {
                    // BPP desconocido: intentar como 24 (mejor que nada)
                    for y in 0..self.height {
                        let src_row = (self.backbuf + (y * self.back_pitch) as u64) as *const u32;
                        let dst_row = (self.lfb     + (y * self.lfb_pitch ) as u64) as *mut u8;
                        let bpp_bytes = (self.bpp as usize + 7) / 8;
                        for x in 0..self.width {
                            let px = core::ptr::read(src_row.add(x));
                            let b = ( px        & 0xFF) as u8;
                            let g = ((px >>  8) & 0xFF) as u8;
                            let r = ((px >> 16) & 0xFF) as u8;
                            let base = x * bpp_bytes;
                            core::ptr::write_volatile(dst_row.add(base),     b);
                            if bpp_bytes > 1 { core::ptr::write_volatile(dst_row.add(base + 1), g); }
                            if bpp_bytes > 2 { core::ptr::write_volatile(dst_row.add(base + 2), r); }
                        }
                    }
                }
            }
        }
    }

    // ── Primitivas ────────────────────────────────────────────────────────────
    pub fn clear(&self, color: Color) {
        let val = color.0;
        let total = self.width * self.height;
        unsafe {
            let p = self.backbuf as *mut u32;
            for i in 0..total {
                core::ptr::write_volatile(p.add(i), val);
            }
        }
    }

    pub fn fill_rect(&self, sx: usize, sy: usize, w: usize, h: usize, c: Color) {
        if self.backbuf == 0 || w == 0 || h == 0 { return; }
        let ex = sx.saturating_add(w).min(self.width);
        let ey = sy.saturating_add(h).min(self.height);
        if sx >= ex || sy >= ey { return; }
        let val = c.0;
        unsafe {
            for y in sy..ey {
                let row = (self.backbuf + (y * self.back_pitch + sx * 4) as u64) as *mut u32;
                for x in 0..(ex - sx) {
                    core::ptr::write_volatile(row.add(x), val);
                }
            }
        }
    }

    pub fn hline(&self, x: usize, y: usize, l: usize, c: Color) { self.fill_rect(x,y,l,1,c); }
    pub fn vline(&self, x: usize, y: usize, l: usize, c: Color) { self.fill_rect(x,y,1,l,c); }

    pub fn draw_rect_border(&self, sx: usize, sy: usize, w: usize, h: usize, t: usize, c: Color) {
        self.fill_rect(sx, sy, w, t, c);
        self.fill_rect(sx, sy + h.saturating_sub(t), w, t, c);
        self.fill_rect(sx, sy, t, h, c);
        self.fill_rect(sx + w.saturating_sub(t), sy, t, h, c);
    }

    pub fn fill_rounded(&self, sx: usize, sy: usize, w: usize, h: usize, r: usize, c: Color) {
        if w == 0 || h == 0 { return; }
        let r = r.min(w / 2).min(h / 2);
        self.fill_rect(sx, sy + r, w, h.saturating_sub(r * 2), c);
        self.fill_rect(sx + r, sy, w.saturating_sub(r * 2), r, c);
        self.fill_rect(sx + r, sy + h.saturating_sub(r), w.saturating_sub(r * 2), r, c);
        for dy in 0..r {
            let trim = r - dy;
            self.fill_rect(sx + trim, sy + dy,                  w.saturating_sub(trim*2), 1, c);
            self.fill_rect(sx + trim, sy + h.saturating_sub(dy+1), w.saturating_sub(trim*2), 1, c);
        }
    }

    pub fn draw_progress_bar(&self, x: usize, y: usize, w: usize, h: usize,
                             pct: u32, fg: Color, bg: Color, border: Color) {
        self.fill_rect(x, y, w, h, bg);
        self.draw_rect_border(x, y, w, h, 1, border);
        let filled = (w.saturating_sub(2)) as u64 * pct.min(100) as u64 / 100;
        if filled > 0 { self.fill_rect(x+1, y+1, filled as usize, h.saturating_sub(2), fg); }
    }

    pub fn draw_gradient_bar(&self, x: usize, y: usize, w: usize, h: usize,
                              pct: u32, fg: Color, bg: Color) {
        self.fill_rect(x, y, w, h, bg);
        let filled = w as u64 * pct.min(100) as u64 / 100;
        for i in 0..filled as usize {
            let alpha = 255u8.saturating_sub((i * 80 / filled.max(1) as usize) as u8);
            self.fill_rect(x + i, y, 1, h, fg.dim(alpha));
        }
    }

    // ── Cursor del mouse ──────────────────────────────────────────────────────
    const CURSOR_W: usize = 12;
    const CURSOR_H: usize = 12;
    const ARROW: &'static [u16] = &[
        0b1000_0000_0000_0000,
        0b1100_0000_0000_0000,
        0b1110_0000_0000_0000,
        0b1111_0000_0000_0000,
        0b1111_1000_0000_0000,
        0b1111_1100_0000_0000,
        0b1111_1110_0000_0000,
        0b1111_0000_0000_0000,
        0b1101_1000_0000_0000,
        0b1000_1100_0000_0000,
        0b0000_0110_0000_0000,
        0b0000_0000_0000_0000,
    ];

    pub fn draw_cursor(&self, mx: i32, my: i32) {
        let cx = mx.max(0) as usize;
        let cy = my.max(0) as usize;
        for (row, &mask) in Self::ARROW.iter().enumerate() {
            for col in 0..Self::CURSOR_W {
                if (mask >> (15 - col)) & 1 != 0 {
                    unsafe {
                        self.draw_pixel(
                            (cx + col + 1).min(self.width.saturating_sub(1)),
                            (cy + row + 1).min(self.height.saturating_sub(1)),
                            Color::new(0, 0, 0),
                        );
                    }
                }
            }
        }
        for (row, &mask) in Self::ARROW.iter().enumerate() {
            for col in 0..Self::CURSOR_W {
                if (mask >> (15 - col)) & 1 != 0 {
                    let c = if col == 0 || row == 0 {
                        Color::new(20, 20, 20)
                    } else {
                        Color::WHITE
                    };
                    unsafe { self.draw_pixel(cx + col, cy + row, c); }
                }
            }
        }
        unsafe { self.draw_pixel(cx, cy, Color::new(10, 10, 10)); }
    }
}

// ── Console ───────────────────────────────────────────────────────────────────
pub struct Console {
    fb:           Framebuffer,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub margin_x: usize,
    pub fg_color: Color,
    pub bg_color: Color,
    font_w:       usize,
    font_h:       usize,
}

impl Console {
    pub fn new() -> Self {
        Self {
            fb: Framebuffer::new(),
            cursor_x: 0, cursor_y: 0, margin_x: 0,
            fg_color: Color::WHITE,
            bg_color: Color::PORTIX_BG,
            font_w: 8, font_h: 8,
        }
    }

    pub fn fb(&self)     -> &Framebuffer { &self.fb }
    pub fn width(&self)  -> usize        { self.fb.width  }
    pub fn height(&self) -> usize        { self.fb.height }

    pub fn present(&self) { self.fb.present(); }
    pub fn draw_cursor(&self, mx: i32, my: i32) { self.fb.draw_cursor(mx, my); }

    pub fn clear(&mut self, color: Color) {
        self.bg_color = color;
        self.fb.clear(color);
        self.cursor_x = 0; self.cursor_y = 0; self.margin_x = 0;
    }

    pub fn set_position(&mut self, x: usize, y: usize) {
        self.cursor_x = x; self.cursor_y = y; self.margin_x = x;
    }
    pub fn set_margin(&mut self, mx: usize) { self.margin_x = mx; }

    pub fn fill_rect(&self, x: usize, y: usize, w: usize, h: usize, c: Color)                       { self.fb.fill_rect(x,y,w,h,c); }
    pub fn fill_rounded(&self, x: usize, y: usize, w: usize, h: usize, r: usize, c: Color)          { self.fb.fill_rounded(x,y,w,h,r,c); }
    pub fn draw_rect(&self, x: usize, y: usize, w: usize, h: usize, t: usize, c: Color)             { self.fb.draw_rect_border(x,y,w,h,t,c); }
    pub fn hline(&self, x: usize, y: usize, l: usize, c: Color)                                     { self.fb.hline(x,y,l,c); }
    pub fn vline(&self, x: usize, y: usize, l: usize, c: Color)                                     { self.fb.fill_rect(x,y,1,l,c); }
    pub fn progress_bar(&self, x: usize, y: usize, w: usize, h: usize,
                        pct: u32, fg: Color, bg: Color, br: Color)                                   { self.fb.draw_progress_bar(x,y,w,h,pct,fg,bg,br); }
    pub fn gradient_bar(&self, x: usize, y: usize, w: usize, h: usize,
                        pct: u32, fg: Color, bg: Color)                                              { self.fb.draw_gradient_bar(x,y,w,h,pct,fg,bg); }

    fn draw_char(&self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
        let a = ch as usize;
        if a < 32 || a > 127 { return; }
        let glyph = crate::graphics::render::font::FONT_8X8[a - 32];
        for (row, &byte) in glyph.iter().enumerate() {
            for col in 0..8usize {
                let on = (byte & (1u8 << col)) != 0;
                let px = x + col;
                let py = y + row;
                if px < self.fb.width && py < self.fb.height {
                    unsafe { self.fb.draw_pixel(px, py, if on { fg } else { bg }); }
                }
            }
        }
    }

    pub fn draw_char_tall(&self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
        let a = ch as usize;
        if a < 32 || a > 127 { return; }
        let glyph = crate::graphics::render::font::FONT_8X8[a - 32];
        for (row, &byte) in glyph.iter().enumerate() {
            for col in 0..8usize {
                let on = (byte & (1u8 << col)) != 0;
                let px = x + col;
                for dy in 0..2usize {
                    let py = y + row * 2 + dy;
                    if px < self.fb.width && py < self.fb.height {
                        unsafe { self.fb.draw_pixel(px, py, if on { fg } else { bg }); }
                    }
                }
            }
        }
    }

    pub fn write(&mut self, s: &str, color: Color) {
        self.fg_color = color;
        for ch in s.chars() {
            match ch {
                '\n' => { self.cursor_x = self.margin_x; self.cursor_y += self.font_h + 5; }
                '\r' => { self.cursor_x = self.margin_x; }
                '\t' => {
                    let tw = (self.font_w + 1) * 4;
                    self.cursor_x = (self.cursor_x / tw + 1) * tw;
                }
                _ => {
                    self.draw_char(self.cursor_x, self.cursor_y, ch, self.fg_color, self.bg_color);
                    self.cursor_x += self.font_w + 1;
                }
            }
            if self.cursor_x + self.font_w + 1 >= self.fb.width {
                self.cursor_x  = self.margin_x;
                self.cursor_y += self.font_h + 5;
            }
            if self.cursor_y + self.font_h >= self.fb.height {
                self.cursor_y = 60;
            }
        }
    }

    pub fn write_at(&mut self, s: &str, x: usize, y: usize, color: Color) {
        let (ox, oy, om) = (self.cursor_x, self.cursor_y, self.margin_x);
        self.cursor_x = x; self.cursor_y = y; self.margin_x = x;
        self.write(s, color);
        self.cursor_x = ox; self.cursor_y = oy; self.margin_x = om;
    }

    pub fn write_at_tall(&mut self, s: &str, x: usize, y: usize, color: Color) {
        let bg = self.bg_color;
        let mut cx = x;
        for ch in s.chars() {
            self.draw_char_tall(cx, y, ch, color, bg);
            cx += 9;
        }
    }

    pub fn write_at_bg(&mut self, s: &str, x: usize, y: usize, fg: Color, bg: Color) {
        let old = self.bg_color;
        self.bg_color = bg;
        self.write_at(s, x, y, fg);
        self.bg_color = old;
    }
}