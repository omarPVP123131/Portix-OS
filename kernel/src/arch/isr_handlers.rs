// arch/isr_handlers.rs — PORTIX OS v3.1
//
// [FIX-REG-EXHAUSTION]  inline_capture_frame() ya no usa outputs separados por
//                       registro. En su lugar escribe directamente a crash_frame
//                       usando un único puntero de entrada (in(reg) ptr).
//                       Esto evita "inline assembly requires more registers than
//                       available" — el compilador solo necesita asignar 1 reg.
//
// [FIX-RBP-OFFSET]      CrashFrame: rbp en +144, valid en +152.
//                       IMPORTANTE: actualizar isr.asm línea:
//                         mov byte [crash_frame + 144], 1
//                       a:
//                         mov byte [crash_frame + 152], 1
//
// Mejoras visuales vs v2:
//   • Grid GPR 3 columnas — usa toda la pantalla
//   • Mensaje del panic en write_at_tall (grande, legible)
//   • Hints dinámicos según mensaje de panic
//   • RIP real en esquina superior derecha

#![allow(dead_code)]

use core::panic::PanicInfo;
use crate::graphics::driver::framebuffer::{Color, Console};
use crate::arch::halt::halt_loop;
use crate::util::fmt::{fmt_u32, fmt_hex};

// ═══════════════════════════════════════════════════════════════════════════════
//  CRASH FRAME
// ═══════════════════════════════════════════════════════════════════════════════

#[repr(C)]
pub struct CrashFrame {
    pub rip:    u64,  // +0
    pub rsp:    u64,  // +8
    pub rflags: u64,  // +16
    pub cr3:    u64,  // +24
    pub rax:    u64,  // +32
    pub rbx:    u64,  // +40
    pub rcx:    u64,  // +48
    pub rdx:    u64,  // +56
    pub rsi:    u64,  // +64
    pub rdi:    u64,  // +72
    pub r8:     u64,  // +80
    pub r9:     u64,  // +88
    pub r10:    u64,  // +96
    pub r11:    u64,  // +104
    pub r12:    u64,  // +112
    pub r13:    u64,  // +120
    pub r14:    u64,  // +128
    pub r15:    u64,  // +136
    pub rbp:    u64,  // +144
    pub valid:  u8,   // +152
}

#[no_mangle]
pub static mut crash_frame: CrashFrame = CrashFrame {
    rip: 0, rsp: 0, rflags: 0, cr3: 0,
    rax: 0, rbx: 0, rcx: 0, rdx: 0,
    rsi: 0, rdi: 0,
    r8: 0, r9: 0, r10: 0, r11: 0,
    r12: 0, r13: 0, r14: 0, r15: 0,
    rbp: 0, valid: 0,
};

fn frame() -> &'static CrashFrame { unsafe { &crash_frame } }

/// [FIX-REG-EXHAUSTION] Captura registros usando un único puntero de entrada.
/// Todo el trabajo se hace dentro del asm con MOVs directos a memoria.
/// El compilador solo necesita asignar UN registro para `ptr`.
#[inline(never)]
unsafe fn inline_capture_frame() {
    let ptr = core::ptr::addr_of_mut!(crash_frame) as u64;
    core::arch::asm!(
        "mov [{p} + 32],  rax",
        "mov [{p} + 40],  rbx",
        "mov [{p} + 48],  rcx",
        "mov [{p} + 56],  rdx",
        "mov [{p} + 64],  rsi",
        "mov [{p} + 72],  rdi",
        "mov [{p} + 80],  r8",
        "mov [{p} + 88],  r9",
        "mov [{p} + 96],  r10",
        "mov [{p} + 104], r11",
        "mov [{p} + 112], r12",
        "mov [{p} + 120], r13",
        "mov [{p} + 128], r14",
        "mov [{p} + 136], r15",
        "mov [{p} + 144], rbp",
        // RSP actual
        "mov rax, rsp",
        "mov [{p} + 8], rax",
        // RFLAGS
        "pushfq",
        "pop rax",
        "mov [{p} + 16], rax",
        // CR3
        "mov rax, cr3",
        "mov [{p} + 24], rax",
        // RIP via LEA relativa
        "lea rax, [rip]",
        "mov [{p} + 0], rax",
        // valid = 1
        "mov byte ptr [{p} + 152], 1",
        p = in(reg) ptr,
        out("rax") _,   // rax es el único scratch usado internamente
        options(nostack),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
//  PALETAS
// ═══════════════════════════════════════════════════════════════════════════════

mod pal {
    use crate::graphics::driver::framebuffer::Color;
    pub const PANIC_BG:      Color = Color::new(0x0A, 0x00, 0x00);
    pub const PANIC_BG2:     Color = Color::new(0x18, 0x00, 0x02);
    pub const PANIC_RED:     Color = Color::new(0xBB, 0x00, 0x10);
    pub const PANIC_CRIMSON: Color = Color::new(0xFF, 0x20, 0x20);
    pub const PANIC_ORANGE:  Color = Color::new(0xFF, 0x88, 0x00);
    pub const PANIC_DIM:     Color = Color::new(0x28, 0x00, 0x05);
    pub const PANIC_PANEL:   Color = Color::new(0x14, 0x00, 0x02);
    pub const PANIC_HINT:    Color = Color::new(0xFF, 0xCC, 0x88);
    pub const PF_BG_L:       Color = Color::new(0x00, 0x04, 0x16);
    pub const PF_BG_R:       Color = Color::new(0x02, 0x07, 0x1E);
    pub const PF_GOLD:       Color = Color::new(0xFF, 0xD7, 0x00);
    pub const PF_GOLD_DIM:   Color = Color::new(0x60, 0x50, 0x00);
    pub const PF_OUTLINE:    Color = Color::new(0x06, 0x1A, 0x55);
    pub const PF_GRID:       Color = Color::new(0x04, 0x0E, 0x30);
    pub const PF_GREEN:      Color = Color::new(0x00, 0xFF, 0x88);
    pub const PF_GRAY:       Color = Color::new(0x22, 0x30, 0x40);
    pub const PF_BLUE:       Color = Color::new(0x44, 0xAA, 0xFF);
    pub const GP_BG:         Color = Color::new(0x08, 0x00, 0x14);
    pub const GP_BG2:        Color = Color::new(0x10, 0x00, 0x22);
    pub const GP_VIOLET:     Color = Color::new(0xAA, 0x00, 0xFF);
    pub const GP_MAGENTA:    Color = Color::new(0xFF, 0x00, 0xCC);
    pub const GP_PINK:       Color = Color::new(0xFF, 0x88, 0xEE);
    pub const GP_DIM:        Color = Color::new(0x22, 0x00, 0x38);
    pub const GP_YELLOW:     Color = Color::new(0xFF, 0xEE, 0x00);
    pub const DF_BG:         Color = Color::new(0x00, 0x06, 0x01);
    pub const DF_GREEN:      Color = Color::new(0x00, 0xFF, 0x44);
    pub const WHITE:         Color = Color::WHITE;
    pub const LIGHT:         Color = Color::new(0xCC, 0xDD, 0xEE);
    pub const MID:           Color = Color::new(0x66, 0x77, 0x88);
    pub const AMBER:         Color = Color::new(0xFF, 0xAA, 0x00);
    pub const REG_BG:        Color = Color::new(0x08, 0x08, 0x12);
    pub const REG_BORDER:    Color = Color::new(0x22, 0x22, 0x36);
}

// ═══════════════════════════════════════════════════════════════════════════════
//  PRIMITIVAS GRÁFICAS
// ═══════════════════════════════════════════════════════════════════════════════

fn grad_v(c: &mut Console, top: Color, bot: Color) {
    let (w, h) = (c.width(), c.height());
    for y in 0..h {
        let t = (y as u32 * 255 / h.max(1) as u32) as u8;
        c.fill_rect(0, y, w, 1, top.blend(bot, 255 - t));
    }
}

fn grad_h_bar(c: &mut Console, x: usize, y: usize, w: usize, h: usize, l: Color, r: Color) {
    for i in 0..w {
        let t = (i as u32 * 255 / w.max(1) as u32) as u8;
        c.fill_rect(x + i, y, 1, h, l.blend(r, 255 - t));
    }
}

fn dot_grid(c: &mut Console, sx: usize, sy: usize, ex: usize, ey: usize, step: usize, col: Color) {
    let mut y = sy;
    while y < ey {
        let mut x = sx;
        while x < ex { c.fill_rect(x, y, 1, 1, col); x += step; }
        y += step;
    }
}

fn watermark(c: &mut Console, s: &str, col: Color) {
    let (w, h) = (c.width(), c.height());
    let sw = s.len() * 9 + 40;
    let mut row = 0usize;
    let mut base_y = 0i32;
    while base_y < h as i32 {
        let ox = if row % 2 == 0 { 0 } else { (sw / 2) as i32 };
        let mut x = ox;
        while x < w as i32 {
            if x >= 0 { c.write_at(s, x as usize, base_y as usize, col); }
            x += sw as i32;
        }
        base_y += 32; row += 1;
    }
}

fn panel(c: &mut Console, x: usize, y: usize, w: usize, h: usize, bg: Color, border: Color) {
    c.fill_rect(x, y, w, h, bg);
    c.fill_rect(x, y, w, 1, border);
    c.fill_rect(x, y + h - 1, w, 1, border);
    c.fill_rect(x, y, 1, h, border);
    c.fill_rect(x + w - 1, y, 1, h, border);
}

fn accent_bar(c: &mut Console, x: usize, y: usize, h: usize, col: Color) {
    c.fill_rect(x, y, 2, h, col);
    c.fill_rect(x + 2, y, 1, h, col.dim(80));
}

fn badge(c: &mut Console, s: &str, x: usize, y: usize, fg: Color, bg: Color) {
    let bw = s.len() * 9 + 14;
    c.fill_rounded(x, y, bw, 16, 3, bg);
    c.write_at(s, x + 7, y + 4, fg);
}

fn write_glow(c: &mut Console, s: &str, x: usize, y: usize, fg: Color, glow: Color) {
    for dy in 0usize..=2 { for dx in 0usize..=2 {
        if dx == 1 && dy == 1 { continue; }
        c.write_at(s, x.saturating_add(dx).saturating_sub(1),
                      y.saturating_add(dy).saturating_sub(1), glow);
    }}
    c.write_at(s, x, y, fg);
}

fn write_glow_tall(c: &mut Console, s: &str, x: usize, y: usize, fg: Color, glow: Color) {
    for dy in 0usize..=4 { for dx in 0usize..=4 {
        if dx == 2 && dy == 2 { continue; }
        c.write_at_tall(s, x.saturating_add(dx).saturating_sub(2),
                           y.saturating_add(dy).saturating_sub(2), glow);
    }}
    c.write_at_tall(s, x, y, fg);
}

fn diamond_sep(c: &mut Console, y: usize, col: Color) {
    let w = c.width();
    c.fill_rect(0, y, w, 1, col.dim(40));
    let mut x = 20usize;
    while x + 5 < w {
        c.fill_rect(x+2, y.saturating_sub(2), 1, 1, col.dim(180));
        c.fill_rect(x+1, y.saturating_sub(1), 3, 1, col.dim(180));
        c.fill_rect(x,   y,                   5, 1, col);
        c.fill_rect(x+1, y+1,                 3, 1, col.dim(180));
        c.fill_rect(x+2, y+2,                 1, 1, col.dim(180));
        x += 48;
    }
}

fn energy_bars(c: &mut Console, x: usize, y: usize, col: Color) {
    let widths = [220usize, 160, 100, 55, 24];
    for (i, &w) in widths.iter().enumerate() {
        let by = y + i * 7;
        c.fill_rect(x, by, w, 4, col.dim(20));
        for px in 0..w {
            let a = 230u8.saturating_sub((px as u32 * 190 / w.max(1) as u32) as u8);
            c.fill_rect(x + px, by, 1, 4, col.dim(a));
        }
    }
}

fn section_title(c: &mut Console, s: &str, x: usize, y: usize, col: Color) {
    c.write_at(s, x, y, col);
    c.fill_rect(x, y + 11, s.len() * 9 + 16, 1, col.dim(55));
}

fn reg_row_w(c: &mut Console, reg: &str, val: u64,
             x: usize, y: usize, col_w: usize, ph: usize,
             accent: Color, label_col: Color, val_col: Color) {
    c.fill_rect(x, y, col_w, ph, pal::REG_BG);
    c.fill_rect(x, y, col_w, 1, pal::REG_BORDER);
    c.fill_rect(x, y + ph - 1, col_w, 1, pal::REG_BORDER);
    accent_bar(c, x, y, ph, accent);
    c.write_at(reg, x + 6, y + 2, label_col);
    let mut buf = [0u8; 18];
    c.write_at(fmt_hex(val, &mut buf), x + 46, y + 2, val_col);
}

fn reg_grid_ncol(c: &mut Console, regs: &[(&str, u64)],
                 x: usize, y: usize, cols: usize,
                 col_w: usize, row_h: usize, accent: Color) {
    let ph = row_h - 2;
    for (i, (name, val)) in regs.iter().enumerate() {
        reg_row_w(c, name, *val,
                  x + (i % cols) * (col_w + 4), y + (i / cols) * row_h,
                  col_w, ph, accent, pal::MID, pal::WHITE);
    }
}

fn draw_top_bar(c: &mut Console, l: Color, r: Color) {
    let w = c.width();
    grad_h_bar(c, 0, 0, w, 6, l, r);
    grad_h_bar(c, 0, 6, w, 2, l.dim(100), Color::new(0,0,0));
    grad_h_bar(c, 0, 8, w, 1, l.dim(35),  Color::new(0,0,0));
}

fn draw_bottom_bar(c: &mut Console, l: Color, r: Color, label: &str) {
    let (w, h) = (c.width(), c.height());
    let by = h.saturating_sub(20);
    c.fill_rect(0, by, w, 20, Color::new(0x04, 0x04, 0x07));
    c.fill_rect(0, by, w, 1, l.dim(60));
    c.fill_rect(8,  by + 6, 7, 7, l);
    c.fill_rect(20, by + 6, 7, 7, r.dim(160));
    c.fill_rect(32, by + 6, 7, 7, pal::MID);
    c.write_at(label, 48, by + 6, pal::MID);
    c.write_at("PORTIX-OS  v0.7.4", w.saturating_sub(155), by + 6, pal::MID.dim(90));
}

fn draw_corner_rip(c: &mut Console, rip: u64, valid: u8) {
    let w = c.width();
    let col = if valid != 0 { pal::PANIC_CRIMSON } else { pal::MID };
    c.write_at("RIP", w.saturating_sub(155), 12, col.dim(140));
    let mut buf = [0u8; 18];
    c.write_at(fmt_hex(rip, &mut buf), w.saturating_sub(120), 12, col);
}

// ═══════════════════════════════════════════════════════════════════════════════
//  KERNEL PANIC
// ═══════════════════════════════════════════════════════════════════════════════

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe { inline_capture_frame(); }
    let f = frame();
    let mut c = Console::new();
    let (w, h) = (c.width(), c.height());

    grad_v(&mut c, pal::PANIC_BG, pal::PANIC_BG2);
    watermark(&mut c, "PORTIX  PANIC", Color::new(0x1C, 0x00, 0x03));
    dot_grid(&mut c, 0, 0, w, h, 22, Color::new(0x22, 0x00, 0x05));
    draw_top_bar(&mut c, pal::PANIC_CRIMSON, pal::PANIC_RED);
    draw_corner_rip(&mut c, f.rip, f.valid);

    // Triángulo de alerta
    let ix = 44usize; let iy = 36usize;
    for row in 0..22usize {
        let half = (row / 2 + 1).min(11);
        let cx = ix + 11;
        c.fill_rect(cx.saturating_sub(half), iy + row, half * 2, 1,
                   pal::PANIC_ORANGE.dim((70 + row as u32 * 8).min(255) as u8));
    }
    c.fill_rect(ix + 10, iy + 5,  2, 10, Color::new(0x10, 0x00, 0x00));
    c.fill_rect(ix + 10, iy + 16, 2, 3,  Color::new(0x10, 0x00, 0x00));

    let tx = 80usize; let ty = 34usize;
    write_glow_tall(&mut c, "KERNEL  PANIC", tx, ty, pal::PANIC_CRIMSON, pal::PANIC_RED.dim(35));
    badge(&mut c, "EXCEPCION NO RECUPERABLE", tx, ty + 30, pal::WHITE, pal::PANIC_DIM);
    diamond_sep(&mut c, ty + 52, pal::PANIC_RED);

    // ── Mensaje ───────────────────────────────────────────────────────────────
    let msg_y = ty + 62;
    section_title(&mut c, "MENSAJE DEL PANIC", 44, msg_y, pal::PANIC_ORANGE.dim(190));

    let mut msg_buf = [0u8; 192];
    let mut msg_len = 0usize;
    {
        struct BufWriter<'a> { buf: &'a mut [u8], pos: &'a mut usize }
        impl<'a> core::fmt::Write for BufWriter<'a> {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                for b in s.bytes() {
                    if *self.pos < self.buf.len() {
                        self.buf[*self.pos] = if b > 31 && b < 128 { b } else { b'?' };
                        *self.pos += 1;
                    }
                }
                Ok(())
            }
        }
        use core::fmt::Write;
        let mut bw = BufWriter { buf: &mut msg_buf, pos: &mut msg_len };
        let _ = write!(bw, "{}", info.message());
    }
    let msg_str = core::str::from_utf8(&msg_buf[..msg_len]).unwrap_or("(no disponible)");
    let line1 = if msg_str.len() > 90 { &msg_str[..90] } else { msg_str };
    let line2 = if msg_str.len() > 90 { &msg_str[90..msg_str.len().min(180)] } else { "" };

    let mp_y = msg_y + 16;
    let mp_h = if line2.is_empty() { 22usize } else { 38 };
    c.fill_rect(44, mp_y, w.saturating_sub(90), mp_h, Color::new(0x18, 0x00, 0x03));
    accent_bar(&mut c, 44, mp_y, mp_h, pal::PANIC_CRIMSON);
    c.write_at_tall(line1, 54, mp_y + 2, pal::WHITE);
    if !line2.is_empty() { c.write_at(line2, 54, mp_y + 24, pal::LIGHT); }

    // ── Paneles: ubicación + CPU ───────────────────────────────────────────────
    let panels_y = mp_y + mp_h + 12;
    let panel_h  = 92usize;
    let lw = (w / 2).saturating_sub(54);
    let rx = 44 + lw + 14;
    let rw = w.saturating_sub(rx + 20);

    panel(&mut c, 44, panels_y, lw, panel_h, pal::PANIC_PANEL, pal::PANIC_RED.dim(70));
    accent_bar(&mut c, 44, panels_y, panel_h, pal::PANIC_ORANGE);
    section_title(&mut c, "UBICACION DEL PANIC", 54, panels_y + 8, pal::PANIC_ORANGE.dim(200));

    if let Some(loc) = info.location() {
        let file = loc.file();
        let fd = if file.len() > 46 { &file[file.len()-46..] } else { file };
        c.write_at("ARCHIVO:", 54, panels_y + 28, pal::MID);
        c.write_at(fd,        130, panels_y + 28, pal::PANIC_ORANGE);
        let mut lb = [0u8;16]; let mut cb = [0u8;16];
        c.write_at("LINEA:", 54, panels_y + 46, pal::MID);
        c.write_at(fmt_u32(loc.line(), &mut lb),   110, panels_y + 46, pal::PANIC_CRIMSON);
        c.write_at("COLUMNA:",   54, panels_y + 62, pal::MID);
        c.write_at(fmt_u32(loc.column(), &mut cb), 130, panels_y + 62, pal::PANIC_CRIMSON);
    } else {
        c.write_at("(ubicacion no disponible)", 54, panels_y + 38, pal::MID);
    }

    panel(&mut c, rx, panels_y, rw, panel_h, pal::PANIC_PANEL, pal::PANIC_RED.dim(70));
    accent_bar(&mut c, rx, panels_y, panel_h, pal::PANIC_CRIMSON);
    section_title(&mut c, "CPU AL MOMENTO DEL PANIC", rx + 10, panels_y + 8, pal::PANIC_ORANGE.dim(200));

    let cw2 = (rw.saturating_sub(30)) / 2;
    let crit: &[(&str, u64)] = &[
        ("RIP ", f.rip),   ("RSP ", f.rsp),
        ("RFLG", f.rflags),("CR3 ", f.cr3),
    ];
    for (i, (name, val)) in crit.iter().enumerate() {
        reg_row_w(&mut c, name, *val,
                  rx + 10 + (i % 2) * (cw2 + 4), panels_y + 26 + (i / 2) * 18,
                  cw2, 15, pal::PANIC_RED, pal::MID, pal::WHITE);
    }

    // ── GPR 3 columnas ────────────────────────────────────────────────────────
    let gpr_y = panels_y + panel_h + 14;
    section_title(&mut c, "REGISTROS DE PROPOSITO GENERAL", 44, gpr_y, pal::MID.dim(180));

    let all_regs: &[(&str, u64)] = &[
        ("RAX", f.rax), ("RBX", f.rbx), ("RCX", f.rcx),
        ("RDX", f.rdx), ("RSI", f.rsi), ("RDI", f.rdi),
        ("RBP", f.rbp), ("R8 ", f.r8),  ("R9 ", f.r9),
        ("R10", f.r10), ("R11", f.r11), ("R12", f.r12),
        ("R13", f.r13), ("R14", f.r14), ("R15", f.r15),
    ];
    let col_w3 = (w.saturating_sub(88 + 16)) / 3;
    reg_grid_ncol(&mut c, all_regs, 44, gpr_y + 16, 3, col_w3, 16, pal::PANIC_RED.dim(120));

    // ── Hints dinámicos ───────────────────────────────────────────────────────
    let rows_3 = (all_regs.len() + 2) / 3;
    let hint_y = gpr_y + 16 + rows_3 * 16 + 10;
    if hint_y + 38 < h.saturating_sub(26) {
        section_title(&mut c, "POSIBLES CAUSAS", 44, hint_y, pal::PANIC_HINT.dim(150));
        let (h1, h2) = if msg_str.contains("zero") {
            ("►  Division por cero: el divisor era 0 en DIV/IDIV o en Rust /",
             "►  Verificar denominadores antes de dividir (if divisor != 0)")
        } else if msg_str.contains("index") || msg_str.contains("out of bounds") {
            ("►  Indice fuera de rango: acceso a slice/array mas alla de su longitud",
             "►  Verificar .len() antes de indexar, o usar .get() con Option")
        } else if msg_str.contains("unwrap") || msg_str.contains("None") {
            ("►  unwrap() sobre None: la Option estaba vacia inesperadamente",
             "►  Usar if let / match en lugar de unwrap(); revisar flujo de datos")
        } else if msg_str.contains("overflow") || msg_str.contains("arithmetic") {
            ("►  Desbordamiento aritmetico (debug): operacion supero el tipo",
             "►  Usar wrapping_add/sub/mul o checked_* para evitar panics")
        } else {
            ("►  unwrap()/expect() sobre None/Err    ►  assert!() fallido",
             "►  Desbordamiento aritmetico (debug)   ►  Indice fuera de rango")
        };
        c.write_at(h1, 44, hint_y + 16, pal::PANIC_HINT.dim(120));
        c.write_at(h2, 44, hint_y + 28, pal::PANIC_HINT.dim(100));
    }

    energy_bars(&mut c, 44, h.saturating_sub(56), pal::PANIC_RED);
    draw_bottom_bar(&mut c, pal::PANIC_CRIMSON, pal::PANIC_RED,
                   "KERNEL PANIC  |  INTERRUPTS DISABLED  |  SISTEMA DETENIDO");
    c.present();
    halt_loop()
}

// ═══════════════════════════════════════════════════════════════════════════════
//  PAGE FAULT  #PF
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
extern "C" fn isr_page_fault(ec: u64) {
    let cr2: u64;
    unsafe { core::arch::asm!("mov {r}, cr2", r = out(reg) cr2, options(nostack, preserves_flags)); }
    let f = frame();
    let mut c = Console::new();
    let (w, h) = (c.width(), c.height());
    let split = w * 2 / 5;

    for y in 0..h {
        let t = (y as u32 * 50 / h.max(1) as u32) as u8;
        c.fill_rect(0,     y, split,     1, pal::PF_BG_L.blend(Color::new(0x00, 0x07, 0x28), t));
        c.fill_rect(split, y, w - split, 1, pal::PF_BG_R.blend(Color::new(0x02, 0x06, 0x1C), t));
    }
    dot_grid(&mut c, 0, 0, split, h, 22, pal::PF_GRID);
    for dx in 0..4usize {
        c.fill_rect(split + dx, 0, 1, h, pal::PF_GOLD.dim([255,140,60,20][dx]));
    }
    draw_top_bar(&mut c, pal::PF_GOLD, pal::PF_BLUE);
    draw_corner_rip(&mut c, f.rip, f.valid);

    let lp = 32usize;
    c.write_at_tall("#PF", lp+3, 46, pal::PF_OUTLINE);
    c.write_at_tall("#PF", lp+1, 44, pal::PF_GOLD_DIM);
    c.write_at_tall("#PF", lp,   43, pal::PF_GOLD);
    write_glow(&mut c, "PAGE  FAULT", lp, 69, pal::WHITE, pal::PF_OUTLINE);
    c.write_at("Acceso a memoria no mapeada o protegida.", lp, 86, pal::LIGHT);
    c.fill_rect(lp, 100, split - lp*2, 1, pal::PF_GOLD.dim(45));

    let bw = (split - lp*2).min(280);
    c.fill_rect(lp, 106, bw, 44, Color::new(0x14,0x10,0x00));
    c.fill_rect(lp, 106, bw, 2,  pal::PF_GOLD.dim(180));
    c.fill_rect(lp, 148, bw, 1,  pal::PF_GOLD.dim(55));
    c.write_at("DIRECCION FAULTING (CR2)", lp+8, 111, pal::MID);
    { let mut buf=[0u8;18]; write_glow(&mut c, fmt_hex(cr2,&mut buf), lp+8, 124, pal::PF_GOLD, pal::PF_GOLD_DIM.dim(55)); }

    let cause = if cr2 < 0x1000 { "Null pointer / acceso a pagina 0" }
                else if cr2 > 0xFFFF_8000_0000_0000 { "Acceso a espacio de kernel desde usuario" }
                else if cr2 == f.rip { "Instruccion no ejecutable (NX bit activo)" }
                else { "Pagina no presente o sin permisos de acceso" };
    c.write_at("CAUSA PROBABLE", lp, 158, pal::MID);
    c.write_at(cause, lp, 172, pal::PF_BLUE);
    c.write_at("VECTOR   0x0E  (#PF)", lp, 192, pal::MID);
    { let mut buf=[0u8;18]; c.write_at("RIP", lp, 208, pal::MID); c.write_at(fmt_hex(f.rip,&mut buf), lp+36, 208, pal::PF_BLUE); }

    let rx = split + 18;
    let rw = w.saturating_sub(rx + 14);
    section_title(&mut c, "ERROR CODE", rx, 28, pal::PF_GOLD.dim(200));
    { let mut buf=[0u8;18]; c.write_at(fmt_hex(ec,&mut buf), rx+110, 28, pal::PF_GOLD); }

    let bits: &[(&str, &str, u64)] = &[
        ("P",  "Presente en tabla",    1<<0), ("W",   "Operacion escritura",   1<<1),
        ("U",  "Modo usuario (CPL=3)", 1<<2), ("R",   "Reserved write",        1<<3),
        ("I",  "Inst. fetch (NX/XD)", 1<<4),  ("PK",  "Prot. key (MPK)",       1<<5),
        ("SS", "Shadow stack (CET)",  1<<6),  ("SGX", "SGX access control",   1<<15),
    ];
    for (i, (name, desc, mask)) in bits.iter().enumerate() {
        let by = 46 + i * 20;
        let set = (ec & mask) != 0;
        let ic = if set { pal::PF_GREEN } else { pal::PF_GRAY };
        c.fill_rect(rx, by+1, 12, 12, ic.dim(if set{200}else{50}));
        if set { c.fill_rect(rx+3, by+4, 6, 6, ic); }
        c.write_at(name, rx+16, by+2, ic);
        c.write_at(desc, rx+44, by+2, if set{pal::WHITE}else{pal::MID});
    }

    let sep_y = 46 + bits.len() * 20 + 8;
    c.fill_rect(rx, sep_y, rw, 1, pal::PF_GOLD.dim(35));
    section_title(&mut c, "CONTEXTO DE CPU", rx, sep_y+10, pal::PF_GOLD.dim(200));
    let regs: &[(&str,u64)] = &[
        ("CR2 ",cr2),("EC  ",ec),("RIP ",f.rip),("RSP ",f.rsp),
        ("RFLG",f.rflags),("CR3 ",f.cr3),("RAX ",f.rax),("RBX ",f.rbx),
        ("RCX ",f.rcx),("RDX ",f.rdx),
    ];
    reg_grid_ncol(&mut c, regs, rx, sep_y+24, 2, (rw.saturating_sub(12))/2, 16, pal::PF_GOLD.dim(110));

    draw_bottom_bar(&mut c, pal::PF_GOLD, pal::PF_BLUE, "#PF PAGE FAULT  |  SISTEMA DETENIDO");
    c.present(); halt_loop()
}

// ═══════════════════════════════════════════════════════════════════════════════
//  GENERAL PROTECTION FAULT  #GP
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
extern "C" fn isr_gp_handler(ec: u64) {
    let f = frame();
    let mut c = Console::new();
    let (w, h) = (c.width(), c.height());

    grad_v(&mut c, pal::GP_BG, pal::GP_BG2);
    let mut seed: u64 = 0xDEAD_BEEF_C0FFEE ^ ec;
    for _ in 0..220usize {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let px = ((seed>>33) as usize) % w; let py = ((seed>>17) as usize) % h;
        let sz = if seed&7==0{2}else{1};    let a = (25+(seed&0x45)) as u8;
        c.fill_rect(px, py, sz, sz, if seed&1==0{pal::GP_VIOLET.dim(a)}else{pal::GP_MAGENTA.dim(a)});
    }
    let mut y=0usize; while y<h { c.fill_rect(0,y,w,1,pal::GP_VIOLET.dim(8)); y+=8; }

    draw_top_bar(&mut c, pal::GP_VIOLET, pal::GP_MAGENTA);
    badge(&mut c, "INT  0x0D  (#GP)", w.saturating_sub(148), 12, pal::GP_YELLOW, pal::GP_DIM);
    draw_corner_rip(&mut c, f.rip, f.valid);

    let tx=50usize; let ty=38usize;
    for r in (2usize..=7).rev() { let a=(10+(7-r)*14) as u8; c.write_at_tall("#GP",tx.saturating_sub(r),ty.saturating_sub(r/2),pal::GP_VIOLET.dim(a)); }
    c.write_at_tall("#GP", tx, ty, pal::GP_MAGENTA);
    write_glow(&mut c, "GENERAL  PROTECTION  FAULT", tx, ty+30, pal::GP_PINK, pal::GP_VIOLET.dim(38));
    c.write_at("Violacion de proteccion de segmento o instruccion privilegiada.", tx, ty+44, pal::LIGHT);
    diamond_sep(&mut c, ty+60, pal::GP_VIOLET);

    let col_y = ty+70; let col2_x = w/2+20;
    section_title(&mut c, "ERROR CODE  /  SELECTOR", tx, col_y, pal::GP_VIOLET.dim(180));
    { let mut buf=[0u8;18]; c.write_at(fmt_hex(ec,&mut buf), tx+210, col_y, pal::GP_YELLOW); }

    let is_ext=(ec&1)!=0; let is_idt=(ec&2)!=0; let is_ti=(ec&4)!=0; let index=(ec>>3)&0x1FFF;
    let fields: &[(&str,&str,bool,Color)] = &[
        ("EXT  bit 0","Origen externo al procesador",          is_ext, pal::GP_PINK),
        ("IDT  bit 1","Referencia a la IDT",                   is_idt, pal::GP_PINK),
        ("TI   bit 2",if is_ti{"LDT"}else{"GDT"},              is_ti,  pal::GP_VIOLET),
    ];
    for (i,(bit,desc,set,accent)) in fields.iter().enumerate() {
        let fy = col_y+18+i*20;
        let ind = if *set{*accent}else{pal::MID};
        c.fill_rect(tx,fy+1,12,12,ind.dim(if *set{200}else{45}));
        if *set{c.fill_rect(tx+3,fy+4,6,6,ind);}
        c.write_at(bit, tx+16,fy+2,ind.dim(200));
        c.write_at(desc,tx+90,fy+2,if *set{pal::WHITE}else{pal::MID});
    }
    {
        let idx_y = col_y+18+fields.len()*20+6;
        let mut buf=[0u8;18];
        c.write_at("INDEX  bits[15:3]",tx,idx_y,pal::MID);
        c.write_at(fmt_hex(index,&mut buf),tx+168,idx_y,pal::GP_YELLOW);
        let known=match index{0=>"(NULL)",1=>"(kcode 0x08)",2=>"(kdata 0x10)",3=>"(ucode 0x18)",4=>"(udata 0x20)",_=>""};
        if !known.is_empty(){c.write_at(known,tx+240,idx_y,pal::GP_VIOLET.dim(180));}
        let cause = if ec==0{"Instruccion privilegiada / acceso a I/O en modo usuario"}
                    else if is_idt{"Excepcion dentro de handler (IDT corrompida)"}
                    else if index==0{"Selector NULL como destino de far call/jump"}
                    else{"Descriptor invalido o sin permisos suficientes"};
        section_title(&mut c,"CAUSA PROBABLE",tx,idx_y+18,pal::GP_PINK.dim(180));
        c.write_at(cause,tx,idx_y+34,pal::LIGHT);
    }

    section_title(&mut c,"CONTEXTO DE CPU",col2_x,col_y,pal::GP_VIOLET.dim(180));
    let cpu_regs: &[(&str,u64)] = &[
        ("RIP ",f.rip),("RSP ",f.rsp),("RFLG",f.rflags),("CR3 ",f.cr3),
        ("RAX ",f.rax),("RBX ",f.rbx),("RCX ",f.rcx),("RDX ",f.rdx),
        ("RSI ",f.rsi),("RDI ",f.rdi),
    ];
    let cw2 = (w.saturating_sub(col2_x+20))/2;
    reg_grid_ncol(&mut c,cpu_regs,col2_x,col_y+16,2,cw2.saturating_sub(4),16,pal::GP_MAGENTA.dim(110));

    draw_bottom_bar(&mut c,pal::GP_VIOLET,pal::GP_MAGENTA,"#GP GENERAL PROTECTION FAULT  |  SISTEMA DETENIDO");
    c.present(); halt_loop()
}

// ═══════════════════════════════════════════════════════════════════════════════
//  DOUBLE FAULT  #DF
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
extern "C" fn isr_double_fault() {
    unsafe {
        let v = 0xB8000usize as *mut u16;
        for i in 0..160usize { core::ptr::write_volatile(v.add(i), 0x4F20); }
        let msg  = b"  PORTIX-OS  #DF  DOUBLE FAULT  |  SISTEMA DETENIDO  ";
        let msg2 = b"  Excepcion doble -- Stack o IDT corrompidos. IST1 activo.";
        for (i,&b) in msg.iter().enumerate()  { core::ptr::write_volatile(v.add(i),   0x4F00|b as u16); }
        for (i,&b) in msg2.iter().enumerate() { if i<80{core::ptr::write_volatile(v.add(80+i), 0x4E00|b as u16);} }
    }

    let mut c = Console::new();
    let (w, h) = (c.width(), c.height());
    grad_v(&mut c, pal::DF_BG, Color::new(0,2,0));

    let crt_w=w.min(720); let crt_h=h.min(360);
    let crt_x=(w-crt_w)/2; let crt_y=(h-crt_h)/2;
    c.fill_rect(crt_x.saturating_sub(12),crt_y.saturating_sub(12),crt_w+24,crt_h+24,Color::new(0x10,0x10,0x10));
    c.fill_rounded(crt_x.saturating_sub(6),crt_y.saturating_sub(6),crt_w+12,crt_h+12,10,Color::new(0x1A,0x1A,0x1A));
    c.fill_rect(crt_x,crt_y,crt_w,crt_h,Color::new(0xAA,0,0));

    let cw=8usize; let ch=14usize;
    let wy=|r:usize| crt_y+r*ch+ch;
    fn crt_cx(crt_x:usize,crt_w:usize,cw:usize,s:&str)->usize{ crt_x+(crt_w.saturating_sub(s.len()*cw))/2 }

    c.fill_rect(crt_x,crt_y+ch,crt_w,ch+4,Color::new(0xFF,0xFF,0xFF));
    let t="  PORTIX  #DF  DOUBLE FAULT  |  SISTEMA DETENIDO  ";
    c.write_at(t,crt_cx(crt_x,crt_w,cw,t),crt_y+ch+2,Color::new(0xAA,0,0));
    c.fill_rect(crt_x+10,wy(2),crt_w-20,1,Color::new(0xFF,0x88,0x88));

    let lines: &[(&str,Color)] = &[
        ("Excepcion doble -- #DF ocurre cuando un handler falla.", Color::new(0xFF,0xEE,0xEE)),
        ("Causas comunes:", Color::new(0xFF,0xDD,0xDD)),
        ("  - Stack overflow (kernel stack agotado)",  Color::new(0xFF,0xCC,0xCC)),
        ("  - IDT corrompida o descriptor invalido",   Color::new(0xFF,0xCC,0xCC)),
        ("  - Doble excepcion en el primer handler",   Color::new(0xFF,0xCC,0xCC)),
        ("",Color::new(0,0,0)),
        ("Este handler usa IST1 (stack dedicado 16KB).",Color::new(0xFF,0xBB,0x88)),
        ("Reinicia el sistema. PORTIX-OS termina aqui.",Color::new(0xFF,0xBB,0x88)),
    ];
    for (i,(t,col)) in lines.iter().enumerate() {
        if t.is_empty(){continue;} c.write_at(t,crt_cx(crt_x,crt_w,cw,t),wy(3+i),*col);
    }
    c.fill_rect(crt_x+10,wy(3+lines.len())+ch-3,cw,2,pal::WHITE);
    let mut sy=crt_y; while sy<crt_y+crt_h{c.fill_rect(crt_x,sy,crt_w,1,Color::new(0x44,0,0));sy+=2;}
    for i in 0..16usize {
        let d=(16-i) as u8*14;
        c.fill_rect(crt_x+i,crt_y,1,crt_h,Color::new(0,0,0).dim(d));
        c.fill_rect(crt_x+crt_w-1-i,crt_y,1,crt_h,Color::new(0,0,0).dim(d));
        c.fill_rect(crt_x,crt_y+i,crt_w,1,Color::new(0,0,0).dim(d));
        c.fill_rect(crt_x,crt_y+crt_h-1-i,crt_w,1,Color::new(0,0,0).dim(d));
    }
    c.write_at("PORTIX CRT TERMINAL  80 x 25",crt_x+(crt_w-25*9)/2,crt_y+crt_h+12,Color::new(0x28,0x28,0x28));
    badge(&mut c,"IST1  16KB  DEDICATED STACK",w/2-134,crt_y+crt_h+28,pal::DF_GREEN.dim(180),Color::new(0,0x12,4));
    draw_bottom_bar(&mut c,pal::DF_GREEN,Color::new(0,0,0),"#DF DOUBLE FAULT  |  IST1  |  SISTEMA DETENIDO");
    c.present(); halt_loop()
}

// ═══════════════════════════════════════════════════════════════════════════════
//  DIVIDE BY ZERO  #DE
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
extern "C" fn isr_divide_by_zero() {
    let f = frame();
    let mut c = Console::new();
    let (w, h) = (c.width(), c.height());

    grad_v(&mut c, Color::new(0x0E,0x05,0), Color::new(0x18,0x08,0));
    dot_grid(&mut c, 0, 0, w, h, 20, Color::new(0x26,0x0C,0));
    draw_top_bar(&mut c, pal::PANIC_ORANGE, pal::PANIC_RED);
    draw_corner_rip(&mut c, f.rip, f.valid);

    let ix=46usize; let iy=40usize;
    c.fill_rounded(ix+6,iy,12,12,6,pal::PANIC_ORANGE);
    c.fill_rect(ix,iy+16,24,6,pal::PANIC_ORANGE);
    c.fill_rounded(ix+6,iy+26,12,12,6,pal::PANIC_ORANGE);

    let tx=80usize; let ty=40usize;
    write_glow_tall(&mut c,"#DE  DIVIDE  BY  ZERO",tx,ty,pal::PANIC_ORANGE,Color::new(0x2E,0x0C,0));
    c.fill_rect(tx,ty+30,w-tx-44,1,pal::PANIC_ORANGE.dim(45));
    c.write_at("Division entre cero o desbordamiento en instruccion DIV/IDIV.",tx,ty+38,pal::LIGHT);

    let d_y=ty+58;
    section_title(&mut c,"DIAGNOSTICO",tx,d_y,pal::PANIC_ORANGE.dim(180));
    c.write_at("►  Divisor (RCX/RBX/otro) vale 0 en el momento del fallo",tx,d_y+16,pal::AMBER.dim(180));
    c.write_at("►  IDIV con resultado fuera de rango del registro destino",tx,d_y+28,pal::AMBER.dim(180));

    if f.valid != 0 {
        let reg_y=d_y+48;
        section_title(&mut c,"REGISTROS AL MOMENTO DEL FALLO",tx,reg_y,pal::MID);
        let regs: &[(&str,u64)] = &[
            ("RIP ",f.rip),("RAX ",f.rax),("RDX ",f.rdx),
            ("RCX ",f.rcx),("RBX ",f.rbx),("RSP ",f.rsp),
        ];
        reg_grid_ncol(&mut c,regs,tx,reg_y+16,3,(w.saturating_sub(tx+44))/3,16,pal::PANIC_ORANGE.dim(130));
    }
    draw_bottom_bar(&mut c,pal::PANIC_ORANGE,pal::PANIC_RED,"#DE DIVIDE BY ZERO  |  SISTEMA DETENIDO");
    c.present(); halt_loop()
}

// ═══════════════════════════════════════════════════════════════════════════════
//  BOUND RANGE  #BR
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
extern "C" fn isr_bound_range() {
    let f = frame();
    let mut c = Console::new();
    let (w, h) = (c.width(), c.height());

    grad_v(&mut c,Color::new(0,8,0x10),Color::new(0,4,0x0C));
    dot_grid(&mut c,0,0,w,h,20,Color::new(0,0x14,0x20));
    draw_top_bar(&mut c,pal::PF_BLUE,Color::new(0,0x44,0x88));
    draw_corner_rip(&mut c,f.rip,f.valid);

    let tx=52usize; let ty=40usize;
    write_glow_tall(&mut c,"#BR  BOUND  RANGE",tx,ty,pal::PF_BLUE,Color::new(0,0x14,0x2A));
    c.fill_rect(tx,ty+30,w-tx-44,1,pal::PF_BLUE.dim(45));
    c.write_at("Indice fuera del rango definido por la instruccion BOUND.",tx,ty+38,pal::LIGHT);
    c.write_at("VECTOR  0x05  (#BR)",tx,ty+52,pal::MID);

    if f.valid != 0 {
        let d_y=ty+70;
        section_title(&mut c,"REGISTROS",tx,d_y,pal::MID);
        let regs: &[(&str,u64)] = &[("RIP ",f.rip),("RAX ",f.rax),("RCX ",f.rcx),("RSP ",f.rsp)];
        reg_grid_ncol(&mut c,regs,tx,d_y+16,2,200,16,pal::PF_BLUE.dim(130));
    }
    draw_bottom_bar(&mut c,pal::PF_BLUE,Color::new(0,0x44,0x88),"#BR BOUND RANGE  |  SISTEMA DETENIDO");
    c.present(); halt_loop()
}

// ═══════════════════════════════════════════════════════════════════════════════
//  INVALID OPCODE  #UD
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
extern "C" fn isr_ud_handler() {
    let f = frame();
    let mut c = Console::new();
    let (w, h) = (c.width(), c.height());

    grad_v(&mut c,Color::new(7,0,0x12),Color::new(3,0,0x0A));
    dot_grid(&mut c,0,0,w,h,20,Color::new(0x12,0,0x20));
    draw_top_bar(&mut c,pal::GP_VIOLET,pal::GP_MAGENTA);
    draw_corner_rip(&mut c,f.rip,f.valid);

    let tx=52usize; let ty=40usize;
    write_glow_tall(&mut c,"#UD  INVALID  OPCODE",tx,ty,pal::GP_VIOLET,Color::new(0x18,0,0x2E));
    c.fill_rect(tx,ty+30,w-tx-44,1,pal::GP_VIOLET.dim(45));
    c.write_at("La CPU encontro una instruccion no definida, UD2, o LOCK invalido.",tx,ty+38,pal::LIGHT);
    c.write_at("VECTOR  0x06  (#UD)",tx,ty+52,pal::MID);

    let d_y=ty+68;
    section_title(&mut c,"CAUSAS COMUNES",tx,d_y,pal::GP_VIOLET.dim(180));
    c.write_at("►  UD2 ejecutada intencionalmente (assert de CPU)",tx,d_y+16,pal::GP_PINK.dim(180));
    c.write_at("►  Binario para ISA superior (SSE4/AVX en CPU sin soporte)",tx,d_y+28,pal::GP_PINK.dim(180));
    c.write_at("►  Puntero de funcion invalido / salto a datos corrompidos",tx,d_y+40,pal::GP_PINK.dim(180));

    if f.valid != 0 {
        let reg_y=d_y+58;
        section_title(&mut c,"CONTEXTO AL MOMENTO DEL FALLO",tx,reg_y,pal::MID);
        let regs: &[(&str,u64)] = &[("RIP ",f.rip),("RSP ",f.rsp),("RAX ",f.rax),("RBX ",f.rbx)];
        reg_grid_ncol(&mut c,regs,tx,reg_y+16,2,200,16,pal::GP_VIOLET.dim(130));
    }
    draw_bottom_bar(&mut c,pal::GP_VIOLET,pal::GP_MAGENTA,"#UD INVALID OPCODE  |  SISTEMA DETENIDO");
    c.present(); halt_loop()
}

// ═══════════════════════════════════════════════════════════════════════════════
//  HANDLER GENÉRICO
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
extern "C" fn isr_generic_handler() {
    let f = frame();
    let mut c = Console::new();
    let (w, h) = (c.width(), c.height());

    grad_v(&mut c,Color::new(5,6,8),Color::new(2,3,5));
    dot_grid(&mut c,0,0,w,h,16,Color::new(0x0E,0x10,0x14));
    draw_top_bar(&mut c,pal::AMBER,pal::MID);
    draw_corner_rip(&mut c,f.rip,f.valid);

    let tx=52usize; let ty=40usize;
    write_glow_tall(&mut c,"CPU  EXCEPTION",tx,ty,pal::AMBER,Color::new(0x22,0x16,0));
    c.fill_rect(tx,ty+30,w-tx-44,1,pal::AMBER.dim(45));
    c.write_at("Excepcion de CPU no manejada especificamente por este kernel.",tx,ty+38,pal::LIGHT);

    if f.valid != 0 {
        let reg_y=ty+56;
        section_title(&mut c,"CONTEXTO COMPLETO DE CPU",tx,reg_y,pal::MID);
        let regs: &[(&str,u64)] = &[
            ("RIP ",f.rip),("RSP ",f.rsp),("RFLG",f.rflags),("CR3 ",f.cr3),
            ("RAX ",f.rax),("RBX ",f.rbx),("RCX ",f.rcx),("RDX ",f.rdx),
            ("RSI ",f.rsi),("RDI ",f.rdi),("R8  ",f.r8),("R9  ",f.r9),
            ("R10 ",f.r10),("R11 ",f.r11),("R12 ",f.r12),("R13 ",f.r13),
            ("R14 ",f.r14),("R15 ",f.r15),
        ];
        reg_grid_ncol(&mut c,regs,tx,reg_y+16,3,(w.saturating_sub(tx+44))/3,16,pal::AMBER.dim(130));
    }
    draw_bottom_bar(&mut c,pal::AMBER,pal::MID,"CPU EXCEPTION  |  SISTEMA DETENIDO");
    c.present(); halt_loop()
}

// ═══════════════════════════════════════════════════════════════════════════════
//  INTRÍNSECOS DE MEMORIA
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut u8, cv: i32, n: usize) -> *mut u8 {
    for i in 0..n { core::ptr::write_volatile(s.add(i), cv as u8); } s
}
#[no_mangle]
pub unsafe extern "C" fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    for i in 0..n { core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i))); } d
}
#[no_mangle]
pub unsafe extern "C" fn memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    if (d as usize) <= (s as usize) { memcpy(d, s, n) }
    else { let mut i=n; while i>0 { i-=1; core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i))); } d }
}
#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n { let d=*a.add(i) as i32 - *b.add(i) as i32; if d!=0{return d;} } 0
}