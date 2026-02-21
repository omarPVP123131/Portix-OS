// kernel/src/framebuffer.rs — PORTIX v6 — cursor bg-save, modern UI primitives
#![allow(dead_code)]

const LFB_PTR_ADDR: *const u32 = 0x9004 as *const u32;
const WIDTH_ADDR:   *const u16 = 0x9008 as *const u16;
const HEIGHT_ADDR:  *const u16 = 0x900A as *const u16;
const PITCH_ADDR:   *const u16 = 0x900C as *const u16;
const BPP_ADDR:     *const u8  = 0x900E as *const u8;

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

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self(((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }
    pub const fn dim(self, a: u8) -> Self {
        let r = ((self.0 >> 16) & 0xFF) * a as u32 / 255;
        let g = ((self.0 >>  8) & 0xFF) * a as u32 / 255;
        let b = ( self.0        & 0xFF) * a as u32 / 255;
        Color((r << 16) | (g << 8) | b)
    }
    /// Blend self with other by alpha 0..=255 (255 = 100% self)
    pub const fn blend(self, other: Color, a: u8) -> Self {
        let ia = 255 - a as u32;
        let a  = a as u32;
        let r = (((self.0 >> 16) & 0xFF) * a + ((other.0 >> 16) & 0xFF) * ia) / 255;
        let g = (((self.0 >>  8) & 0xFF) * a + ((other.0 >>  8) & 0xFF) * ia) / 255;
        let b = (( self.0        & 0xFF) * a + ( other.0        & 0xFF) * ia) / 255;
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
        let content_y = header_h + gold_h + tab_h + 2;
        let bottom_y  = fh.saturating_sub(clamp(fh / 34, 18, 24));
        let pad       = clamp(fw / 56, 10, 30);
        let col_div   = fw * 5 / 12;  // 5:7 split (left narrower)
        Layout {
            fw, fh, header_h, gold_h, tab_h,
            content_y, bottom_y, pad,
            col_div, right_x: col_div + pad + 4,
            line_h: 15, font_w: 8, font_h: 8,
        }
    }
    pub fn left_w(&self)    -> usize { self.col_div.saturating_sub(self.pad * 2) }
    pub fn right_w(&self)   -> usize { self.fw.saturating_sub(self.right_x + self.pad) }
    pub fn content_h(&self) -> usize { self.bottom_y.saturating_sub(self.content_y) }
    pub fn content_lines(&self) -> usize { self.content_h() / self.line_h }
}

fn clamp(v: usize, lo: usize, hi: usize) -> usize { v.max(lo).min(hi) }

// ── Mouse cursor background save (16×16 max) ─────────────────────────────────
const CUR_W: usize = 12;
const CUR_H: usize = 12;
static mut CUR_BG:    [[u32; CUR_W]; CUR_H] = [[0; CUR_W]; CUR_H];
static mut CUR_BG_X:  i32 = -200;
static mut CUR_BG_Y:  i32 = -200;
static mut CUR_BG_OK: bool = false;

// ── Framebuffer ───────────────────────────────────────────────────────────────
pub struct Framebuffer {
    buffer: u64,
    pub width:  usize,
    pub height: usize,
    pitch:  usize,
    bpp:    u8,
}

impl Framebuffer {
    pub fn new() -> Self {
        unsafe {
            let bpp_raw = core::ptr::read_volatile(BPP_ADDR);
            let w_raw   = core::ptr::read_volatile(WIDTH_ADDR)  as usize;
            let h_raw   = core::ptr::read_volatile(HEIGHT_ADDR) as usize;
            let p_raw   = core::ptr::read_volatile(PITCH_ADDR)  as usize;
            let lfb     = core::ptr::read_volatile(LFB_PTR_ADDR) as u64;
            let (w, h, pitch, bpp) = if w_raw == 0 || h_raw == 0 {
                (1024, 768, 4096, 32u8)
            } else {
                let bpp = bpp_raw.max(16);
                let p   = if p_raw == 0 { w_raw * bpp as usize / 8 } else { p_raw };
                (w_raw, h_raw, p, bpp)
            };
            Self { buffer: lfb, width: w, height: h, pitch, bpp }
        }
    }

    pub fn lfb_addr(&self) -> u64  { self.buffer }
    pub fn is_valid(&self) -> bool  { self.buffer != 0 }
    pub fn bpp(&self)      -> u8    { self.bpp }

    #[inline(always)]
    pub unsafe fn draw_pixel(&self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height || self.buffer == 0 { return; }
        let off = y as u64 * self.pitch as u64 + x as u64 * (self.bpp as u64 / 8);
        let ptr = (self.buffer + off) as *mut u8;
        match self.bpp {
            32 => core::ptr::write_volatile(ptr as *mut u32, color.0),
            24 => {
                core::ptr::write_volatile(ptr,           color.0 as u8);
                core::ptr::write_volatile(ptr.add(1), (color.0 >>  8) as u8);
                core::ptr::write_volatile(ptr.add(2), (color.0 >> 16) as u8);
            }
            16 => {
                let r = ((color.0 >> 16) & 0xFF) as u16;
                let g = ((color.0 >>  8) & 0xFF) as u16;
                let b = ( color.0        & 0xFF) as u16;
                let v = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3);
                core::ptr::write_volatile(ptr as *mut u16, v);
            }
            _ => core::ptr::write_volatile(ptr, color.0 as u8),
        }
    }

    #[inline(always)]
    unsafe fn read_pixel32(&self, x: usize, y: usize) -> u32 {
        if x >= self.width || y >= self.height || self.buffer == 0 { return 0; }
        if self.bpp != 32 { return 0; }
        let off = y as u64 * self.pitch as u64 + x as u64 * 4;
        core::ptr::read_volatile((self.buffer + off) as *const u32)
    }

    pub fn clear(&self, color: Color) {
        if self.buffer == 0 { return; }
        for y in 0..self.height {
            for x in 0..self.width {
                unsafe { self.draw_pixel(x, y, color); }
            }
        }
    }

    pub fn fill_rect(&self, sx: usize, sy: usize, w: usize, h: usize, c: Color) {
        if self.buffer == 0 || w == 0 || h == 0 { return; }
        let ex = sx.saturating_add(w).min(self.width);
        let ey = sy.saturating_add(h).min(self.height);
        if sx >= ex || sy >= ey { return; }
        for y in sy..ey {
            for x in sx..ex {
                unsafe { self.draw_pixel(x, y, c); }
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

    /// Rounded-corner rectangle (r = corner radius, 1-based via simple bevel).
    pub fn fill_rounded(&self, sx: usize, sy: usize, w: usize, h: usize, r: usize, c: Color) {
        if w == 0 || h == 0 { return; }
        let r = r.min(w / 2).min(h / 2);
        // Body
        self.fill_rect(sx,     sy + r,  w,      h - r * 2, c);
        self.fill_rect(sx + r, sy,      w - r * 2, r,       c);
        self.fill_rect(sx + r, sy + h.saturating_sub(r), w - r * 2, r, c);
        // Approximate corners with small squares
        for dy in 0..r {
            let trim = r - dy;
            self.fill_rect(sx + trim,             sy + dy,          w - trim * 2, 1, c);
            self.fill_rect(sx + trim,             sy + h - dy - 1, w - trim * 2, 1, c);
        }
    }

    pub fn draw_progress_bar(&self, x: usize, y: usize, w: usize, h: usize,
                             pct: u32, fg: Color, bg: Color, border: Color) {
        self.fill_rect(x, y, w, h, bg);
        self.draw_rect_border(x, y, w, h, 1, border);
        let filled = (w.saturating_sub(2)) as u64 * pct.min(100) as u64 / 100;
        if filled > 0 {
            self.fill_rect(x+1, y+1, filled as usize, h.saturating_sub(2), fg);
        }
    }

    /// Gradient progress bar (left = bright, right = dim).
    pub fn draw_gradient_bar(&self, x: usize, y: usize, w: usize, h: usize,
                              pct: u32, fg: Color, bg: Color) {
        self.fill_rect(x, y, w, h, bg);
        let filled = w as u64 * pct.min(100) as u64 / 100;
        for i in 0..filled as usize {
            let alpha = 255 - (i * 80 / (filled.max(1) as usize)) as u8;
            let c = fg.dim(alpha);
            self.fill_rect(x + i, y, 1, h, c);
        }
    }

    // ── Cursor with background save ───────────────────────────────────────────
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

    /// Save pixels under cursor, then draw it.
    pub fn draw_cursor_save(&self, mx: i32, my: i32) {
        if self.buffer == 0 || self.bpp != 32 { return; }
        let cx = mx.max(0) as usize;
        let cy = my.max(0) as usize;
        unsafe {
            // Save background
            for row in 0..CUR_H {
                for col in 0..CUR_W {
                    CUR_BG[row][col] = self.read_pixel32(cx + col, cy + row);
                }
            }
            CUR_BG_X  = mx;
            CUR_BG_Y  = my;
            CUR_BG_OK = true;
        }
        self.draw_cursor_pixels(cx, cy);
    }

    /// Restore saved background, then draw cursor at new position.
    pub fn move_cursor(&self, old_mx: i32, old_my: i32, new_mx: i32, new_my: i32) {
        unsafe {
            if CUR_BG_OK && old_mx == CUR_BG_X && old_my == CUR_BG_Y {
                let ox = old_mx.max(0) as usize;
                let oy = old_my.max(0) as usize;
                for row in 0..CUR_H {
                    for col in 0..CUR_W {
                        self.draw_pixel(ox + col, oy + row, Color(CUR_BG[row][col]));
                    }
                }
            }
        }
        self.draw_cursor_save(new_mx, new_my);
    }

    /// Invalidate saved background (call before full redraws).
    pub fn invalidate_cursor_bg() { unsafe { CUR_BG_OK = false; } }

    fn draw_cursor_pixels(&self, cx: usize, cy: usize) {
        for (row, &mask) in Self::ARROW.iter().enumerate() {
            for col in 0..CUR_W {
                if (mask >> (15 - col)) & 1 != 0 {
                    // White pixel with 1px dark outline effect
                    let c = if col == 0 || row == 0 {
                        Color::new(10, 10, 10)
                    } else {
                        Color::WHITE
                    };
                    unsafe { self.draw_pixel(cx + col, cy + row, c); }
                }
            }
        }
        unsafe { self.draw_pixel(cx, cy, Color::new(8, 8, 8)); } // tip
    }

    /// Legacy (used for full-redraw paths).
    pub fn draw_mouse_cursor(&self, mx: i32, my: i32) {
        self.draw_cursor_save(mx, my);
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

    pub fn clear(&mut self, color: Color) {
        self.bg_color = color;
        Framebuffer::invalidate_cursor_bg();
        self.fb.clear(color);
        self.cursor_x = 0; self.cursor_y = 0; self.margin_x = 0;
    }

    pub fn set_position(&mut self, x: usize, y: usize) {
        self.cursor_x = x; self.cursor_y = y; self.margin_x = x;
    }
    pub fn set_margin(&mut self, mx: usize) { self.margin_x = mx; }

    pub fn fill_rect(&self, x: usize, y: usize, w: usize, h: usize, c: Color) { self.fb.fill_rect(x,y,w,h,c); }
    pub fn fill_rounded(&self, x: usize, y: usize, w: usize, h: usize, r: usize, c: Color) { self.fb.fill_rounded(x,y,w,h,r,c); }
    pub fn draw_rect(&self, x: usize, y: usize, w: usize, h: usize, t: usize, c: Color) { self.fb.draw_rect_border(x,y,w,h,t,c); }
    pub fn hline(&self, x: usize, y: usize, l: usize, c: Color) { self.fb.hline(x,y,l,c); }
    pub fn vline(&self, x: usize, y: usize, l: usize, c: Color) { self.fb.fill_rect(x,y,1,l,c); }
    pub fn progress_bar(&self, x: usize, y: usize, w: usize, h: usize, pct: u32, fg: Color, bg: Color, br: Color) { self.fb.draw_progress_bar(x,y,w,h,pct,fg,bg,br); }
    pub fn gradient_bar(&self, x: usize, y: usize, w: usize, h: usize, pct: u32, fg: Color, bg: Color) { self.fb.draw_gradient_bar(x,y,w,h,pct,fg,bg); }
    pub fn draw_mouse(&self, mx: i32, my: i32) { self.fb.draw_mouse_cursor(mx, my); }
    pub fn move_mouse(&self, omx: i32, omy: i32, nmx: i32, nmy: i32) { self.fb.move_cursor(omx, omy, nmx, nmy); }

    fn draw_char(&self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
        let a = ch as usize;
        if a < 32 || a > 127 { return; }
        let glyph = crate::font::FONT_8X8[a - 32];
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

    /// Draw char at 2× vertical scale (8×16 effective).
    pub fn draw_char_tall(&self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
        let a = ch as usize;
        if a < 32 || a > 127 { return; }
        let glyph = crate::font::FONT_8X8[a - 32];
        for (row, &byte) in glyph.iter().enumerate() {
            for col in 0..8usize {
                let on = (byte & (1u8 << col)) != 0;
                let px = x + col;
                // Each row drawn twice for 2× height
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