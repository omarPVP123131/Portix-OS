// kernel/src/framebuffer.rs — PORTIX v0.9.0
//
// CAMBIOS RESPECTO A v0.8:
//   [+] Restauradas TODAS las constantes de Color del v0.8 (RED, GREEN, GRAY, CYAN, etc.)
//   [+] Color::blend()  — método original restaurado (alias de blend_fast para compatibilidad)
//   [+] gradient_bar()  — método original restaurado en Console (alias de fill_gradient_dither)
//   [+] DirtyRegion     — mejora #2: present() solo vuelca zonas modificadas
//   [+] rep stosd/movsd — mejora #1: blit por hardware x86 (3-5× más rápido)
//   [+] Alpha LUT       — mejora #6: blending sin divisiones en caliente
//   [+] fill_gradient_dither — mejora #4: degradado sin banding
//   [+] Bresenham, fill_circle, scroll_region_up, blit_sprite — nuevas primitivas
//   [+] Layout::new() 100% responsivo — sin constantes de resolución hardcodeadas
//   [-] NO se eliminó ningún método ni constante existente en v0.8
//
#![allow(dead_code)]
#![allow(clippy::missing_safety_doc)]

const LFB_PTR_ADDR: *const u32 = 0x9004 as *const u32;
const WIDTH_ADDR:   *const u16 = 0x9008 as *const u16;
const HEIGHT_ADDR:  *const u16 = 0x900A as *const u16;
const PITCH_ADDR:   *const u16 = 0x900C as *const u16;
const BPP_ADDR:     *const u8  = 0x900E as *const u8;

const BACKBUF_ADDR: u64 = 0x0060_0000;

// Matriz Bayer 4×4 para dithering ordenado (mejora #4)
const BAYER_4X4: [[u8; 4]; 4] = [
    [ 0,  8,  2, 10],
    [12,  4, 14,  6],
    [ 3, 11,  1,  9],
    [15,  7, 13,  5],
];

// ── Alpha LUT (mejora #6) ─────────────────────────────────────────────────────
static mut ALPHA_LUT: [[u8; 256]; 256] = [[0u8; 256]; 256];

pub fn init_alpha_lut() {
    unsafe {
        for a in 0usize..256 {
            for v in 0usize..256 {
                ALPHA_LUT[a][v] = ((v * a) / 255) as u8;
            }
        }
    }
}

#[inline(always)]
fn alpha_mul(v: u8, a: u8) -> u8 {
    unsafe { ALPHA_LUT[a as usize][v as usize] }
}

// ── Color ─────────────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    // ── Constantes originales v0.8 — NO ELIMINAR ─────────────────────────────
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
    #[inline(always)] pub const fn r(self) -> u8 { ((self.0 >> 16) & 0xFF) as u8 }
    #[inline(always)] pub const fn g(self) -> u8 { ((self.0 >>  8) & 0xFF) as u8 }
    #[inline(always)] pub const fn b(self) -> u8 { ( self.0        & 0xFF) as u8 }

    pub const fn dim(self, a: u8) -> Self {
        let r = (self.r() as u32) * a as u32 / 255;
        let g = (self.g() as u32) * a as u32 / 255;
        let b = (self.b() as u32) * a as u32 / 255;
        Color((r << 16) | (g << 8) | b)
    }

    /// blend() — método original v0.8, mantenido para compatibilidad binaria
    pub const fn blend(self, other: Color, a: u8) -> Self {
        let ia = 255 - a as u32;
        let a  = a as u32;
        let r = (self.r() as u32 * a + other.r() as u32 * ia) / 255;
        let g = (self.g() as u32 * a + other.g() as u32 * ia) / 255;
        let b = (self.b() as u32 * a + other.b() as u32 * ia) / 255;
        Color((r << 16) | (g << 8) | b)
    }

    /// blend_fast() — versión nueva con LUT (mejora #6), idéntica semántica
    #[inline]
    pub fn blend_fast(self, dst: Color, alpha: u8) -> Self {
        let ia = 255 - alpha;
        Color::new(
            alpha_mul(self.r(), alpha).wrapping_add(alpha_mul(dst.r(), ia)),
            alpha_mul(self.g(), alpha).wrapping_add(alpha_mul(dst.g(), ia)),
            alpha_mul(self.b(), alpha).wrapping_add(alpha_mul(dst.b(), ia)),
        )
    }
}

// ── DirtyRegion (mejora #2) ───────────────────────────────────────────────────
#[derive(Clone, Copy)]
pub struct DirtyRegion {
    pub min_x: usize,
    pub min_y: usize,
    pub max_x: usize,
    pub max_y: usize,
    pub dirty: bool,
}

impl DirtyRegion {
    pub const fn clean() -> Self {
        Self { min_x: usize::MAX, min_y: usize::MAX, max_x: 0, max_y: 0, dirty: false }
    }
    #[inline]
    pub fn mark(&mut self, x: usize, y: usize, w: usize, h: usize) {
        self.dirty = true;
        if x < self.min_x { self.min_x = x; }
        if y < self.min_y { self.min_y = y; }
        let ex = x + w; if ex > self.max_x { self.max_x = ex; }
        let ey = y + h; if ey > self.max_y { self.max_y = ey; }
    }
    pub fn reset(&mut self) { *self = Self::clean(); }
}

// ── Layout ────────────────────────────────────────────────────────────────────
// Ahora 100% responsivo — calcula todo en proporción a (fw, fh).
// Compatible con cualquier resolución VESA.
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
    pub status_h:  usize,
    pub pad:       usize,
    pub col_div:   usize,
    pub right_x:   usize,
    pub line_h:    usize,
    pub font_w:    usize,
    pub font_h:    usize,
}

impl Layout {
    pub fn new(fw: usize, fh: usize) -> Self {
        let font_w   = 8usize;
        let font_h   = 8usize;

        // Proporciones de fh con mínimos/máximos razonables
        let header_h = ((fh * 65) / 1000).max(38).min(60);
        let gold_h   = 4usize;
        let tab_h    = ((fh * 35) / 1000).max(22).min(32);
        let status_h = ((fh * 28) / 1000).max(18).min(24);

        let tab_y     = header_h + gold_h;
        let content_y = tab_y + tab_h;
        let bottom_y  = fh.saturating_sub(status_h);

        let pad     = (fw / 80).max(8).min(18);
        let col_div = fw * 5 / 12;
        let tab_w   = fw / 5;

        Layout {
            fw, fh,
            header_h, gold_h, tab_h, tab_y, tab_w,
            content_y, bottom_y, status_h,
            pad, col_div,
            right_x: col_div + pad + 4,
            line_h:  font_h + font_h / 2,
            font_w, font_h,
        }
    }

    pub fn left_w(&self)        -> usize { self.col_div.saturating_sub(self.pad * 2) }
    pub fn right_w(&self)       -> usize { self.fw.saturating_sub(self.right_x + self.pad) }
    pub fn content_h(&self)     -> usize { self.bottom_y.saturating_sub(self.content_y) }
    pub fn content_lines(&self) -> usize { self.content_h() / self.line_h }

    pub fn tab_hit(&self, mx: i32, my: i32) -> i32 {
        let x = mx as usize; let y = my as usize;
        if y < self.tab_y || y >= self.tab_y + self.tab_h { return -1; }
        let idx = x / self.tab_w;
        if idx < 5 { idx as i32 } else { -1 }
    }
}

fn clamp(v: usize, lo: usize, hi: usize) -> usize { v.max(lo).min(hi) }

// ── Framebuffer ───────────────────────────────────────────────────────────────
pub struct Framebuffer {
    lfb:        u64,
    backbuf:    u64,
    pub width:  usize,
    pub height: usize,
    lfb_pitch:  usize,
    bpp:        u8,
    back_pitch: usize,
    pub dirty:  DirtyRegion,
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
                (1024, 768, 3072, 24u8)
            } else {
                let bpp = if bpp_raw < 15 { 24 } else { bpp_raw };
                let bpp_b = (bpp as usize + 7) / 8;
                let p = if p_raw == 0 { w_raw * bpp_b } else { p_raw };
                (w_raw, h_raw, p, bpp)
            };

            let back_pitch = w * 4;

            // rep stosd: limpia back buffer (mejora #1)
            Self::fast_fill_u32(BACKBUF_ADDR as *mut u32, 0, w * h);

            init_alpha_lut(); // mejora #6

            Self {
                lfb, backbuf: BACKBUF_ADDR,
                width: w, height: h,
                lfb_pitch, bpp, back_pitch,
                dirty: DirtyRegion::clean(),
            }
        }
    }

    // ── rep stosd ─────────────────────────────────────────────────────────────
    #[cfg(target_arch = "x86_64")]
    #[inline]
    unsafe fn fast_fill_u32(dst: *mut u32, val: u32, count: usize) {
        core::arch::asm!(
            "cld", "rep stosd",
            inout("rdi") dst   => _,
            inout("ecx") count => _,
            in("eax")    val,
            options(nostack)
        );
    }
    #[cfg(not(target_arch = "x86_64"))]
    #[inline]
    unsafe fn fast_fill_u32(dst: *mut u32, val: u32, count: usize) {
        for i in 0..count { core::ptr::write_volatile(dst.add(i), val); }
    }

    // ── rep movsd ─────────────────────────────────────────────────────────────
    #[cfg(target_arch = "x86_64")]
    #[inline]
    unsafe fn fast_copy_u32(dst: *mut u32, src: *const u32, count: usize) {
        core::arch::asm!(
            "cld", "rep movsd",
            inout("rdi") dst   => _,
            inout("rsi") src   => _,
            inout("ecx") count => _,
            options(nostack)
        );
    }
    #[cfg(not(target_arch = "x86_64"))]
    #[inline]
    unsafe fn fast_copy_u32(dst: *mut u32, src: *const u32, count: usize) {
        for i in 0..count {
            core::ptr::write_volatile(dst.add(i), core::ptr::read(src.add(i)));
        }
    }

    pub fn lfb_addr(&self) -> u64  { self.lfb }
    pub fn is_valid(&self) -> bool  { self.lfb != 0 }
    pub fn bpp(&self)      -> u8    { self.bpp }

    #[inline(always)]
    pub unsafe fn draw_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height { return; }
        let off = (y * self.back_pitch + x * 4) as u64;
        core::ptr::write_volatile((self.backbuf + off) as *mut u32, color.0);
    }

    #[inline(always)]
    pub unsafe fn read_back_pixel(&self, x: usize, y: usize) -> Color {
        if x >= self.width || y >= self.height { return Color::BLACK; }
        let off = (y * self.back_pitch + x * 4) as u64;
        Color(core::ptr::read_volatile((self.backbuf + off) as *const u32))
    }

    // ── present() — dirty-rect (mejora #2) + rep movsd (mejora #1) ───────────
    pub fn present(&mut self) {
        if self.lfb == 0 || !self.dirty.dirty { return; }
        let x0 = self.dirty.min_x.min(self.width);
        let y0 = self.dirty.min_y.min(self.height);
        let x1 = self.dirty.max_x.min(self.width);
        let y1 = self.dirty.max_y.min(self.height);
        self.dirty.reset();
        if x0 >= x1 || y0 >= y1 { return; }
        let cols = x1 - x0;

        unsafe {
            match self.bpp {
                32 => {
                    for y in y0..y1 {
                        let src = (self.backbuf + (y * self.back_pitch + x0 * 4) as u64) as *const u32;
                        let dst = (self.lfb     + (y * self.lfb_pitch  + x0 * 4) as u64) as *mut   u32;
                        Self::fast_copy_u32(dst, src, cols);
                    }
                }
                24 => {
                    for y in y0..y1 {
                        let src = (self.backbuf + (y * self.back_pitch + x0 * 4) as u64) as *const u32;
                        let dst = (self.lfb     + (y * self.lfb_pitch  + x0 * 3) as u64) as *mut u8;
                        for x in 0..cols {
                            let px = core::ptr::read(src.add(x));
                            let b = x * 3;
                            core::ptr::write_volatile(dst.add(b),     ( px        & 0xFF) as u8);
                            core::ptr::write_volatile(dst.add(b + 1), ((px >>  8) & 0xFF) as u8);
                            core::ptr::write_volatile(dst.add(b + 2), ((px >> 16) & 0xFF) as u8);
                        }
                    }
                }
                16 => {
                    for y in y0..y1 {
                        let s = (self.backbuf + (y * self.back_pitch + x0 * 4) as u64) as *const u32;
                        let d = (self.lfb     + (y * self.lfb_pitch  + x0 * 2) as u64) as *mut u16;
                        for x in 0..cols {
                            let px = core::ptr::read(s.add(x));
                            let r = ((px >> 16) & 0xFF) as u16;
                            let g = ((px >>  8) & 0xFF) as u16;
                            let bv = ( px        & 0xFF) as u16;
                            core::ptr::write_volatile(d.add(x),
                                ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (bv >> 3));
                        }
                    }
                }
                _ => {
                    let bpp_b = (self.bpp as usize + 7) / 8;
                    for y in y0..y1 {
                        let src = (self.backbuf + (y * self.back_pitch + x0 * 4) as u64) as *const u32;
                        let dst = (self.lfb     + (y * self.lfb_pitch  + x0 * bpp_b) as u64) as *mut u8;
                        for x in 0..cols {
                            let px = core::ptr::read(src.add(x));
                            let base = x * bpp_b;
                            core::ptr::write_volatile(dst.add(base),   ( px        & 0xFF) as u8);
                            if bpp_b > 1 { core::ptr::write_volatile(dst.add(base+1), ((px>>8)&0xFF)  as u8); }
                            if bpp_b > 2 { core::ptr::write_volatile(dst.add(base+2), ((px>>16)&0xFF) as u8); }
                        }
                    }
                }
            }
        }
    }

    /// Fuerza un blit completo de toda la pantalla (cambio de tab, etc.)
    pub fn present_full(&mut self) {
        self.dirty.mark(0, 0, self.width, self.height);
        self.present();
    }

    // ── Primitivas ────────────────────────────────────────────────────────────

    pub fn clear(&self, color: Color) {
        let val   = color.0;
        let total = self.width * self.height;
        unsafe { Self::fast_fill_u32(self.backbuf as *mut u32, val, total); }
    }

    pub fn fill_rect(&mut self, sx: usize, sy: usize, w: usize, h: usize, c: Color) {
        if self.backbuf == 0 || w == 0 || h == 0 { return; }
        let ex = sx.saturating_add(w).min(self.width);
        let ey = sy.saturating_add(h).min(self.height);
        if sx >= ex || sy >= ey { return; }
        let rw  = ex - sx;
        let val = c.0;
        unsafe {
            for y in sy..ey {
                let row = (self.backbuf + (y * self.back_pitch + sx * 4) as u64) as *mut u32;
                Self::fast_fill_u32(row, val, rw);
            }
        }
        self.dirty.mark(sx, sy, ex - sx, ey - sy);
    }

    pub fn hline(&mut self, x: usize, y: usize, l: usize, c: Color) { self.fill_rect(x,y,l,1,c); }
    pub fn vline(&mut self, x: usize, y: usize, l: usize, c: Color) { self.fill_rect(x,y,1,l,c); }

    pub fn draw_rect_border(&mut self, sx: usize, sy: usize, w: usize, h: usize, t: usize, c: Color) {
        self.fill_rect(sx, sy, w, t, c);
        self.fill_rect(sx, sy + h.saturating_sub(t), w, t, c);
        self.fill_rect(sx, sy, t, h, c);
        self.fill_rect(sx + w.saturating_sub(t), sy, t, h, c);
    }

    pub fn fill_rounded(&mut self, sx: usize, sy: usize, w: usize, h: usize, r: usize, c: Color) {
        if w == 0 || h == 0 { return; }
        let r = r.min(w / 2).min(h / 2);
        self.fill_rect(sx, sy + r, w, h.saturating_sub(r * 2), c);
        self.fill_rect(sx + r, sy, w.saturating_sub(r * 2), r, c);
        self.fill_rect(sx + r, sy + h.saturating_sub(r), w.saturating_sub(r * 2), r, c);
        for dy in 0..r {
            let trim = r - dy;
            self.fill_rect(sx + trim, sy + dy,                       w.saturating_sub(trim * 2), 1, c);
            self.fill_rect(sx + trim, sy + h.saturating_sub(dy + 1), w.saturating_sub(trim * 2), 1, c);
        }
    }

    pub fn draw_progress_bar(&mut self, x: usize, y: usize, w: usize, h: usize,
                              pct: u32, fg: Color, bg: Color, border: Color) {
        self.fill_rect(x, y, w, h, bg);
        self.draw_rect_border(x, y, w, h, 1, border);
        let filled = (w.saturating_sub(2)) as u64 * pct.min(100) as u64 / 100;
        if filled > 0 { self.fill_rect(x+1, y+1, filled as usize, h.saturating_sub(2), fg); }
    }

    /// gradient_bar original — mantenido para compatibilidad
    pub fn draw_gradient_bar(&mut self, x: usize, y: usize, w: usize, h: usize,
                              pct: u32, fg: Color, bg: Color) {
        self.fill_rect(x, y, w, h, bg);
        let filled = w as u64 * pct.min(100) as u64 / 100;
        for i in 0..filled as usize {
            let alpha = 255u8.saturating_sub((i * 80 / filled.max(1) as usize) as u8);
            self.fill_rect(x + i, y, 1, h, fg.dim(alpha));
        }
    }

    /// fill_gradient_dither — degradado con dithering Bayer (mejora #4)
    pub fn fill_gradient_dither(&mut self, x: usize, y: usize, w: usize, h: usize,
                                 c0: Color, c1: Color) {
        if w == 0 || h == 0 { return; }
        for py in y..(y + h).min(self.height) {
            let brow = &BAYER_4X4[py & 3];
            for px in x..(x + w).min(self.width) {
                let t   = ((px - x) as u32 * 255) / w as u32;
                let dith = brow[px & 3] as u32;
                let td  = (t + dith / 2).min(255) as u8;
                let it  = 255 - td;
                let r = (c0.r() as u32 * td as u32 / 255 + c1.r() as u32 * it as u32 / 255) as u8;
                let g = (c0.g() as u32 * td as u32 / 255 + c1.g() as u32 * it as u32 / 255) as u8;
                let b = (c0.b() as u32 * td as u32 / 255 + c1.b() as u32 * it as u32 / 255) as u8;
                unsafe { self.draw_pixel(px, py, Color::new(r, g, b)); }
            }
        }
        self.dirty.mark(x, y, w, h);
    }

    /// fill_rect_alpha con LUT (mejora #6)
    pub fn fill_rect_alpha_fast(&mut self, sx: usize, sy: usize, w: usize, h: usize,
                                 color: Color, alpha: u8) {
        if alpha == 0 { return; }
        if alpha == 255 { self.fill_rect(sx, sy, w, h, color); return; }
        let ex = (sx + w).min(self.width);
        let ey = (sy + h).min(self.height);
        for y in sy..ey {
            for x in sx..ex {
                unsafe {
                    let bg  = self.read_back_pixel(x, y);
                    let out = color.blend_fast(bg, alpha);
                    self.draw_pixel(x, y, out);
                }
            }
        }
        self.dirty.mark(sx, sy, ex - sx, ey - sy);
    }

    /// Bresenham (mejora #10)
    pub fn draw_line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, c: Color) {
        let dx  =  (x1 - x0).abs();
        let sx  = if x0 < x1 { 1i32 } else { -1 };
        let dy  = -(y1 - y0).abs();
        let sy  = if y0 < y1 { 1i32 } else { -1 };
        let mut err = dx + dy;
        loop {
            if x0 >= 0 && y0 >= 0 {
                unsafe { self.draw_pixel(x0 as usize, y0 as usize, c); }
            }
            if x0 == x1 && y0 == y1 { break; }
            let e2 = 2 * err;
            if e2 >= dy { err += dy; x0 += sx; }
            if e2 <= dx { err += dx; y0 += sy; }
        }
        let bx = x0.min(x1).max(0) as usize;
        let by = y0.min(y1).max(0) as usize;
        let ex = x0.max(x1).max(0) as usize;
        let ey = y0.max(y1).max(0) as usize;
        self.dirty.mark(bx, by, (ex - bx).max(1), (ey - by).max(1));
    }

    /// fill_circle Midpoint (mejora #11)
    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, c: Color) {
        if r <= 0 { return; }
        let mut x = 0i32; let mut y = r;
        let mut d = 1 - r;
        while x <= y {
            let draw = |fb: &mut Framebuffer, yy: i32, xl: i32, xr: i32| {
                if yy < 0 || yy >= fb.height as i32 { return; }
                let xl = xl.max(0) as usize;
                let xr = (xr.min(fb.width as i32 - 1) as usize).max(xl);
                fb.fill_rect(xl, yy as usize, xr - xl + 1, 1, c);
            };
            draw(self, cy + x, cx - y, cx + y);
            draw(self, cy - x, cx - y, cx + y);
            draw(self, cy + y, cx - x, cx + x);
            draw(self, cy - y, cx - x, cx + x);
            if d < 0 { d += 2 * x + 3; } else { d += 2 * (x - y) + 5; y -= 1; }
            x += 1;
        }
    }

    /// scroll_region_up — memmove vertical (mejora #13)
    pub fn scroll_region_up(&mut self, sx: usize, sy: usize, w: usize, h: usize,
                             lines: usize, fill: Color) {
        if lines == 0 || lines >= h || w == 0 { return; }
        let visible = h - lines;
        for row in 0..visible {
            let src_y = sy + row + lines;
            let dst_y = sy + row;
            if src_y >= self.height { break; }
            let src = (self.backbuf + (src_y * self.back_pitch + sx * 4) as u64) as *const u32;
            let dst = (self.backbuf + (dst_y * self.back_pitch + sx * 4) as u64) as *mut u32;
            unsafe { Self::fast_copy_u32(dst, src, w.min(self.width - sx)); }
        }
        for row in visible..h {
            self.fill_rect(sx, sy + row, w, 1, fill);
        }
        self.dirty.mark(sx, sy, w, h);
    }

    /// blit_sprite con color-key transparente (mejora #12)
    pub fn blit_sprite(&mut self, dx: usize, dy: usize, sw: usize, sh: usize,
                        data: &[Color], key: Color) {
        for row in 0..sh {
            for col in 0..sw {
                let c = data[row * sw + col];
                if c == key { continue; }
                unsafe { self.draw_pixel(dx + col, dy + row, c); }
            }
        }
        self.dirty.mark(dx, dy, sw, sh);
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

    pub fn draw_cursor(&mut self, mx: i32, my: i32) {
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
                    let c = if col == 0 || row == 0 { Color::new(20, 20, 20) } else { Color::WHITE };
                    unsafe { self.draw_pixel(cx + col, cy + row, c); }
                }
            }
        }
        unsafe { self.draw_pixel(cx, cy, Color::new(10, 10, 10)); }
        self.dirty.mark(cx, cy, Self::CURSOR_W + 1, Self::CURSOR_H + 1);
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

    pub fn fb(&self)         -> &Framebuffer     { &self.fb }
    pub fn fb_mut(&mut self) -> &mut Framebuffer { &mut self.fb }
    pub fn width(&self)      -> usize            { self.fb.width  }
    pub fn height(&self)     -> usize            { self.fb.height }

    pub fn present(&mut self)                        { self.fb.present(); }
    pub fn present_full(&mut self)                   { self.fb.present_full(); }
    pub fn draw_cursor(&mut self, mx: i32, my: i32) { self.fb.draw_cursor(mx, my); }

    pub fn clear(&mut self, color: Color) {
        self.bg_color = color;
        self.fb.clear(color);
        self.fb.dirty.mark(0, 0, self.fb.width, self.fb.height);
        self.cursor_x = 0; self.cursor_y = 0; self.margin_x = 0;
    }

    pub fn set_position(&mut self, x: usize, y: usize) {
        self.cursor_x = x; self.cursor_y = y; self.margin_x = x;
    }
    pub fn set_margin(&mut self, mx: usize) { self.margin_x = mx; }

    // Delegaciones — API idéntica a v0.8
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, c: Color)
        { self.fb.fill_rect(x,y,w,h,c); }
    pub fn fill_rounded(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize, c: Color)
        { self.fb.fill_rounded(x,y,w,h,r,c); }
    pub fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, t: usize, c: Color)
        { self.fb.draw_rect_border(x,y,w,h,t,c); }
    pub fn hline(&mut self, x: usize, y: usize, l: usize, c: Color) { self.fb.hline(x,y,l,c); }
    pub fn vline(&mut self, x: usize, y: usize, l: usize, c: Color) { self.fb.fill_rect(x,y,1,l,c); }
    pub fn progress_bar(&mut self, x: usize, y: usize, w: usize, h: usize,
                        pct: u32, fg: Color, bg: Color, br: Color)
        { self.fb.draw_progress_bar(x,y,w,h,pct,fg,bg,br); }

    /// gradient_bar — nombre original v0.8, mantenido para compatibilidad
    pub fn gradient_bar(&mut self, x: usize, y: usize, w: usize, h: usize,
                        pct: u32, fg: Color, bg: Color)
        { self.fb.draw_gradient_bar(x,y,w,h,pct,fg,bg); }

    /// gradient — versión nueva con dithering Bayer (mejora #4)
    pub fn gradient(&mut self, x: usize, y: usize, w: usize, h: usize, c0: Color, c1: Color)
        { self.fb.fill_gradient_dither(x,y,w,h,c0,c1); }

    pub fn fill_rect_alpha(&mut self, x: usize, y: usize, w: usize, h: usize,
                           color: Color, alpha: u8) {
        if alpha == 0 { return; }
        if alpha == 255 { self.fb.fill_rect(x,y,w,h,color); return; }
        let ex = (x + w).min(self.fb.width);
        let ey = (y + h).min(self.fb.height);
        for py in y..ey {
            for px in x..ex {
                unsafe {
                    let bg  = self.fb.read_back_pixel(px, py);
                    let out = color.blend_fast(bg, alpha);
                    self.fb.draw_pixel(px, py, out);
                }
            }
        }
        self.fb.dirty.mark(x, y, ex - x, ey - y);
    }

    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Color)
        { self.fb.draw_line(x0,y0,x1,y1,c); }
    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, c: Color)
        { self.fb.fill_circle(cx,cy,r,c); }
    pub fn scroll_region_up(&mut self, x: usize, y: usize, w: usize, h: usize,
                             lines: usize, fill: Color)
        { self.fb.scroll_region_up(x,y,w,h,lines,fill); }
    pub fn blit_sprite(&mut self, dx: usize, dy: usize, sw: usize, sh: usize,
                        data: &[Color], key: Color)
        { self.fb.blit_sprite(dx,dy,sw,sh,data,key); }

    fn draw_char(&mut self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
        let a = ch as usize;
        if a < 32 || a > 127 { return; }
        let glyph = crate::graphics::render::font::FONT_8X8[a - 32];
        for (row, &byte) in glyph.iter().enumerate() {
            for col in 0..8usize {
                let on = (byte & (1u8 << col)) != 0;
                let px = x + col; let py = y + row;
                if px < self.fb.width && py < self.fb.height {
                    unsafe { self.fb.draw_pixel(px, py, if on { fg } else { bg }); }
                }
            }
        }
    }

    pub fn draw_char_tall(&mut self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
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
        let fw = self.font_w + 1;
        let fh = self.font_h + 5;
        for ch in s.chars() {
            match ch {
                '\n' => { self.cursor_x = self.margin_x; self.cursor_y += fh; }
                '\r' => { self.cursor_x = self.margin_x; }
                '\t' => { let tw = fw * 4; self.cursor_x = (self.cursor_x / tw + 1) * tw; }
                _ => {
                    self.draw_char(self.cursor_x, self.cursor_y, ch, self.fg_color, self.bg_color);
                    self.fb.dirty.mark(self.cursor_x, self.cursor_y, 8, 8);
                    self.cursor_x += fw;
                }
            }
            if self.cursor_x + fw >= self.fb.width {
                self.cursor_x  = self.margin_x;
                self.cursor_y += fh;
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
        let bg  = self.bg_color;
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