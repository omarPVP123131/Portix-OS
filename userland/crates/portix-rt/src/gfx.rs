use crate::syscall;

pub const FB_PHYS: usize = 0xFD00_0000;
pub const FB_SIZE: usize = 0x40_0000; // 4 MB
pub const SCREEN_WIDTH: usize = 1024;
pub const SCREEN_HEIGHT: usize = 768;

/// Framebuffer lineal mapeado en Ring 3. [FRENTE B-3] Consulta el modo REAL al
/// kernel (SYS_FB_INFO): ancho, alto, bpp (24 o 32) y pitch en bytes. Antes se
/// suponía 1024x768x32 y en el modo VESA 0x118 (24 bpp) la imagen salía rota.
pub struct Framebuffer {
    pub ptr: *mut u32,
    pub width: usize,
    pub height: usize,
    pub bpp: usize,
    pub pitch: usize,
}

impl Framebuffer {
    pub fn acquire() -> Option<Self> {
        let mut info = syscall::FbInfo::default();
        let rc = syscall::sys_fb_info(&mut info as *mut syscall::FbInfo);
        let (phys, w, h, bpp, pitch) = if rc == 0 && info.width > 0 {
            (info.phys as usize, info.width as usize, info.height as usize, info.bpp as usize, info.pitch as usize)
        } else {
            (FB_PHYS, SCREEN_WIDTH, SCREEN_HEIGHT, 32, SCREEN_WIDTH * 4)
        };
        let size = ((pitch * h) + 0xFFF) & !0xFFF;
        let vaddr = syscall::sys_mmap_device(phys, size.max(FB_SIZE));
        if vaddr < 0 {
            return None;
        }
        Some(Self { ptr: vaddr as *mut u32, width: w, height: h, bpp, pitch })
    }

    #[inline(always)]
    pub fn put_pixel(&self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            unsafe {
                let base = (self.ptr as *mut u8).add(y * self.pitch);
                if self.bpp == 24 {
                    let p = base.add(x * 3);
                    p.write_volatile(color as u8);
                    p.add(1).write_volatile((color >> 8) as u8);
                    p.add(2).write_volatile((color >> 16) as u8);
                } else {
                    (base as *mut u32).add(x).write_volatile(color);
                }
            }
        }
    }

    /// Lee un píxel (para mezclas alfa del compositor).
    #[inline(always)]
    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x >= self.width || y >= self.height { return 0; }
        unsafe {
            let base = (self.ptr as *const u8).add(y * self.pitch);
            if self.bpp == 24 {
                let p = base.add(x * 3);
                (p.read_volatile() as u32) | ((p.add(1).read_volatile() as u32) << 8) | ((p.add(2).read_volatile() as u32) << 16)
            } else {
                (base as *const u32).add(x).read_volatile()
            }
        }
    }

    pub fn fill_rect(&self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        for py in y..y_end {
            for px in x..x_end {
                self.put_pixel(px, py, color);
            }
        }
    }

    pub fn draw_rect(&self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        if w == 0 || h == 0 { return; }
        // Horizontal top & bottom
        for px in x..(x + w).min(self.width) {
            self.put_pixel(px, y, color);
            self.put_pixel(px, y + h - 1, color);
        }
        // Vertical left & right
        for py in y..(y + h).min(self.height) {
            self.put_pixel(x, py, color);
            self.put_pixel(x + w - 1, py, color);
        }
    }

    pub fn clear(&self, color: u32) {
        self.fill_rect(0, 0, self.width, self.height, color);
    }
}

pub use crate::syscall::MouseEvent;

#[inline(always)]
pub fn alpha_blend(src: u32, dst: u32, alpha: u8) -> u32 {
    if alpha == 0 { return dst; }
    if alpha == 255 { return src; }
    let a = alpha as u32;
    let inv_a = 255 - a;
    let sr = (src >> 16) & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sb = src & 0xFF;
    let dr = (dst >> 16) & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let db = dst & 0xFF;
    let r = (sr * a + dr * inv_a) / 255;
    let g = (sg * a + dg * inv_a) / 255;
    let b = (sb * a + db * inv_a) / 255;
    (r << 16) | (g << 8) | b
}

extern crate alloc;
use alloc::vec::Vec;

/// Superficie gráfica del Portix Display Protocol (PDP)
pub struct Surface {
    pub id: u32,
    pub width: usize,
    pub height: usize,
    pub buffer: Vec<u32>,
    pub alpha: u8,
}

impl Surface {
    pub fn new(id: u32, width: usize, height: usize) -> Self {
        let size = width * height;
        let mut buffer = Vec::with_capacity(size);
        buffer.resize(size, 0xFF000000); // Negro opaco
        Self {
            id,
            width,
            height,
            buffer,
            alpha: 255,
        }
    }

    #[inline(always)]
    pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            self.buffer[y * self.width + x] = color;
        }
    }

    #[inline(always)]
    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            self.buffer[y * self.width + x]
        } else {
            0
        }
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        for py in y..y_end {
            let row = py * self.width;
            for px in x..x_end {
                self.buffer[row + px] = color;
            }
        }
    }

    pub fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        if w == 0 || h == 0 { return; }
        for px in x..(x + w).min(self.width) {
            self.put_pixel(px, y, color);
            self.put_pixel(px, y + h - 1, color);
        }
        for py in y..(y + h).min(self.height) {
            self.put_pixel(x, py, color);
            self.put_pixel(x + w - 1, py, color);
        }
    }

    pub fn clear(&mut self, color: u32) {
        for pixel in &mut self.buffer {
            *pixel = color;
        }
    }

    pub fn blit_to_fb(&self, fb: &Framebuffer, dx: usize, dy: usize) {
        let w = self.width.min(fb.width.saturating_sub(dx));
        let h = self.height.min(fb.height.saturating_sub(dy));
        if self.alpha == 255 {
            for y in 0..h {
                let src_row = y * self.width;
                for x in 0..w {
                    fb.put_pixel(dx + x, dy + y, self.buffer[src_row + x]);
                }
            }
        } else {
            for y in 0..h {
                let src_row = y * self.width;
                for x in 0..w {
                    let src_col = self.buffer[src_row + x];
                    let dst_col = unsafe { fb.ptr.add((dy + y) * fb.width + (dx + x)).read_volatile() };
                    let blended = alpha_blend(src_col, dst_col, self.alpha);
                    fb.put_pixel(dx + x, dy + y, blended);
                }
            }
        }
    }
}

pub fn get_mouse() -> Option<MouseEvent> {
    let mut ev = MouseEvent::default();
    if syscall::sys_get_mouse(&mut ev) == 0 {
        Some(ev)
    } else {
        None
    }
}

pub fn poll_key() -> Option<char> {
    let res = syscall::sys_poll_key();
    if res >= 0 && res < 256 {
        Some(res as u8 as char)
    } else {
        None
    }
}
