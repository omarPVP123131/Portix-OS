// ui/chrome.rs — PORTIX Kernel v0.9.0
//
// CORRECCIONES:
//   - Bug línea ~200: usize::<MAX> era inválido. Ahora usa usize::MAX correctamente.
//   - Footer: 3 zonas fijas (L/C/R), sin colisión posible.
//
// DISEÑO: Cyberpunk neon — amarillo eléctrico + cian + verde neon.

use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::arch::hardware::HardwareInfo;
use crate::util::fmt::{fmt_u32, fmt_mib, fmt_uptime};
use crate::ui::Tab;

// ─────────────────────────────────────────────────────────────────────────────
// Paleta cyberpunk — usa las constantes del Color propio del proyecto
// donde existen, y Color::new() para las nuevas.
// ─────────────────────────────────────────────────────────────────────────────

pub struct Pal;
impl Pal {
    // Fondos
    pub const VOID:       Color = Color::new(0x06, 0x06, 0x08);
    pub const PANEL:      Color = Color::new(0x0C, 0x0C, 0x10);
    pub const RAISED:     Color = Color::new(0x13, 0x12, 0x1A);

    // Neons
    pub const YELLOW:     Color = Color::new(0xFF, 0xE0, 0x00); // amarillo eléctrico
    pub const GOLD:       Color = Color::new(0xFF, 0xAA, 0x00); // ámbar neon
    pub const CYAN:       Color = Color::new(0x00, 0xF0, 0xFF); // cian frío
    pub const GREEN_NEO:  Color = Color::new(0x00, 0xFF, 0x88); // verde neon OK

    // Variantes dim (fondos de badge/resplandor)
    pub const YELLOW_DIM: Color = Color::new(0x28, 0x1C, 0x00);
    pub const CYAN_DIM:   Color = Color::new(0x00, 0x18, 0x20);
    pub const GREEN_DIM:  Color = Color::new(0x00, 0x1A, 0x0C);

    // Bordes
    pub const BOR_WARM:   Color = Color::new(0x50, 0x38, 0x00);
    pub const BOR_COLD:   Color = Color::new(0x1C, 0x1A, 0x28);
    pub const BOR_SEP:    Color = Color::new(0x22, 0x20, 0x30);

    // Tipografía
    pub const TXT_BRIGHT: Color = Color::new(0xEE, 0xEE, 0xFF);
    pub const TXT_MID:    Color = Color::new(0x88, 0x88, 0xAA);
    pub const TXT_DIM:    Color = Color::new(0x44, 0x44, 0x66);

    // Tabs
    pub const TAB_BG:     Color = Color::new(0x09, 0x09, 0x0E);
    pub const TAB_ACT:    Color = Color::new(0x10, 0x0F, 0x1A);
    pub const TAB_HOV:    Color = Color::new(0x13, 0x12, 0x1E);
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers internos
// ─────────────────────────────────────────────────────────────────────────────

/// Glow de 1px: escribe offset ±1 en glow, luego fg encima.
#[inline]
fn write_glow(c: &mut Console, s: &str, x: usize, y: usize, fg: Color, glow: Color) {
    if x > 0 { c.write_at(s, x - 1, y, glow); }
    c.write_at(s, x + 1, y, glow);
    c.write_at(s, x, y, fg);
}

/// Badge redondeado con borde neon. Devuelve el ancho total.
fn neon_badge(
    c:    &mut Console,
    x:    usize, y: usize,
    text: &str,
    fg: Color, bg: Color, bor: Color,
    cw: usize, ch: usize,
) -> usize {
    let pw = 8usize;
    let ph = 4usize;
    let w  = text.len() * cw + pw * 2;
    let h  = ch + ph * 2;
    c.fill_rounded(x, y, w, h, 2, bg);
    c.draw_rect(x, y, w, h, 1, bor);
    c.write_at(text, x + pw, y + ph, fg);
    w
}

/// Línea de acento con halo: 1px halo + 2px neon + 1px halo.
fn accent_bar(c: &mut Console, x: usize, y: usize, w: usize, main: Color, halo: Color) {
    c.fill_rect(x, y,     w, 1, halo);
    c.fill_rect(x, y + 1, w, 2, main);
    c.fill_rect(x, y + 3, w, 1, halo);
}

// ─────────────────────────────────────────────────────────────────────────────
// section_label reutilizable
// ─────────────────────────────────────────────────────────────────────────────

pub fn section_label(c: &mut Console, x: usize, y: usize, title: &str, w: usize) {
    c.fill_rect(x, y, w, 14, Color::new(0x0F, 0x0E, 0x16));
    c.fill_rect(x, y, 2, 14, Pal::YELLOW);
    c.hline(x, y + 13, w, Pal::BOR_SEP);
    c.write_at(title, x + 7, y + 3, Pal::YELLOW);
}

// ─────────────────────────────────────────────────────────────────────────────
// draw_chrome
// ─────────────────────────────────────────────────────────────────────────────

pub fn draw_chrome(
    c:      &mut Console,
    lay:    &Layout,
    hw:     &HardwareInfo,
    active: Tab,
    mx:     i32,
    my:     i32,
) {
    let fw = lay.fw;
    let cw = lay.font_w;
    let ch = lay.font_h;
    let hh = lay.header_h;

    // ═══════════════════════════════════════════════════════════════════════
    // CABECERA
    // ═══════════════════════════════════════════════════════════════════════
    c.fill_rect(0, 0, fw, hh, Pal::VOID);

    // Banda izquierda 4px amarilla + 2px halo
    c.fill_rect(0, 0, 4, hh, Pal::YELLOW);
    c.fill_rect(4, 0, 2, hh, Pal::YELLOW_DIM);

    // Logo PORTIX con glow
    let logo_y = (hh.saturating_sub(18)) / 2;
    write_glow(c, "PORTIX", 13, logo_y, Pal::YELLOW, Color::new(0x44, 0x28, 0x00));
    c.write_at("v0.9.0", 13, logo_y + 13, Pal::TXT_DIM);

    // Separador vertical
    c.vline(82, 8, hh - 16, Pal::BOR_SEP);

    // CPU pill centrado
    let brand  = hw.cpu.brand_str();
    let brand  = if brand.len() > 38 { &brand[..38] } else { brand };
    let pill_w = brand.len() * cw + 20;
    let pill_x = fw / 2 - pill_w / 2;
    let pill_y = (hh.saturating_sub(20)) / 2;
    c.fill_rounded(pill_x, pill_y, pill_w, 20, 3, Pal::RAISED);
    c.draw_rect(pill_x, pill_y, pill_w, 20, 1, Pal::BOR_COLD);
    c.write_at(brand, pill_x + 10, pill_y + (20 - ch) / 2, Pal::TXT_MID);

    // Badges derecha — calculados desde la derecha para evitar overflow
    let badge_y  = (hh.saturating_sub(ch + 8)) / 2;
    let gap      = 4usize;
    let margin_r = 10usize;
    let bw_boot  = "BOOT OK".len() * cw + 16;
    let bw_arch  = "x86_64".len()  * cw + 16;
    let bx_boot  = fw.saturating_sub(margin_r + bw_boot);
    let bx_arch  = bx_boot.saturating_sub(gap + bw_arch);

    neon_badge(c, bx_arch, badge_y, "x86_64",
               Pal::CYAN, Pal::CYAN_DIM, Color::new(0x00, 0x55, 0x77), cw, ch);
    neon_badge(c, bx_boot, badge_y, "BOOT OK",
               Pal::GREEN_NEO, Pal::GREEN_DIM, Color::new(0x00, 0x77, 0x44), cw, ch);

    c.hline(0, hh - 1, fw, Pal::BOR_SEP);

    // ═══════════════════════════════════════════════════════════════════════
    // LÍNEA DE ACENTO NEON
    // ═══════════════════════════════════════════════════════════════════════
    accent_bar(c, 0, lay.header_h, fw, Pal::YELLOW, Pal::YELLOW_DIM);

    // ═══════════════════════════════════════════════════════════════════════
    // BARRA DE PESTAÑAS
    // ═══════════════════════════════════════════════════════════════════════
    let ty = lay.tab_y;
    let th = lay.tab_h;
    c.fill_rect(0, ty, fw, th, Pal::TAB_BG);
    c.hline(0, ty + th - 1, fw, Pal::BOR_SEP);

    let tabs: &[(&str, &str, Tab)] = &[
        ("F1", "SISTEMA",      Tab::System),
        ("F2", "TERMINAL",     Tab::Terminal),
        ("F3", "DISPOSITIVOS", Tab::Devices),
        ("F4", "IDE",          Tab::Ide),
        ("F5", "ARCHIVOS",     Tab::Explorer),
    ];

    let tw = fw / tabs.len();

    for (i, &(fkey, label, tab)) in tabs.iter().enumerate() {
        let tx     = i * tw;
        let is_act = tab == active;
        let hov    = !is_act
            && (mx as usize) >= tx && (mx as usize) < tx + tw
            && (my as usize) >= ty && (my as usize) < ty + th;

        let bg = if is_act { Pal::TAB_ACT } else if hov { Pal::TAB_HOV } else { Pal::TAB_BG };
        c.fill_rect(tx, ty, tw - 1, th, bg);

        if is_act {
            c.fill_rect(tx, ty + th - 3, tw - 1, 3, Pal::YELLOW);
            c.fill_rect(tx + 2, ty + th - 4, tw.saturating_sub(5), 1, Pal::YELLOW_DIM);
            c.hline(tx, ty, tw - 1, Pal::BOR_WARM);
        } else if hov {
            c.fill_rect(tx, ty + th - 1, tw - 1, 1, Pal::BOR_WARM);
        }

        c.vline(tx + tw - 1, ty + 2, th - 4, Pal::BOR_SEP);

        let fkey_w    = fkey.len() * cw;
        let label_w   = label.len() * cw;
        let content_w = fkey_w + 5 + label_w;
        let cx = if tw > content_w + 8 { tx + (tw - content_w) / 2 } else { tx + 4 };
        let cy = ty + (th - ch) / 2;

        let fkey_fg  = if is_act { Pal::YELLOW } else if hov { Pal::GOLD } else { Pal::BOR_WARM };
        let label_fg = if is_act { Pal::TXT_BRIGHT } else if hov { Pal::TXT_MID } else { Pal::TXT_DIM };
        c.write_at(fkey, cx, cy, fkey_fg);
        c.write_at(label, cx + fkey_w + 5, cy, label_fg);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // STATUS BAR — 3 zonas fijas: L (izq) | C (flex) | R (der)
    //
    // Arquitectura garantizada sin colisión:
    //   1. Zona R se calcula PRIMERO (de derecha a izquierda).
    //   2. Zona L se dibuja de izquierda a derecha.
    //   3. Zona C solo ocupa el espacio que queda entre L y R.
    //      Si no cabe, se oculta graciosamente (sin pisar nada).
    // ═══════════════════════════════════════════════════════════════════════
    let sy_bar = lay.bottom_y;
    accent_bar(c, 0, sy_bar, fw, Pal::YELLOW, Pal::YELLOW_DIM);

    let bar_top = sy_bar + 4;
    let bar_h   = lay.fh.saturating_sub(bar_top);
    c.fill_rect(0, bar_top, fw, bar_h, Pal::VOID);

    let ty_txt = bar_top + bar_h.saturating_sub(ch) / 2;

    // ── ZONA R (calcular primero) ─────────────────────────────────────────
    let mut bmx = [0u8; 16];
    let mut bmy = [0u8; 16];
    let mxs = fmt_u32(mx.max(0) as u32, &mut bmx);
    let mys = fmt_u32(my.max(0) as u32, &mut bmy);
    // Ancho texto: "XY:" + mxs + "," + mys
    let xy_chars  = 3 + mxs.len() + 1 + mys.len();
    let zone_r_w  = xy_chars * cw + 18; // 9px padding por lado
    let zone_r_x  = fw.saturating_sub(zone_r_w);

    // Cápsula zona R
    c.fill_rect(zone_r_x, bar_top, zone_r_w, bar_h, Pal::RAISED);
    c.vline(zone_r_x, bar_top, bar_h, Pal::BOR_SEP);

    // Texto XY
    let xy_x = zone_r_x + 9;
    c.write_at("XY:", xy_x, ty_txt, Pal::TXT_DIM);
    c.write_at(mxs, xy_x + 3 * cw, ty_txt, Pal::TXT_MID);
    c.write_at(",",  xy_x + (3 + mxs.len()) * cw, ty_txt, Pal::BOR_WARM);
    c.write_at(mys, xy_x + (4 + mxs.len()) * cw, ty_txt, Pal::TXT_MID);

    // ── ZONA L (izquierda fija) ───────────────────────────────────────────
    let mut lx = 10usize;

    c.write_at("PORTIX", lx, ty_txt, Pal::YELLOW);
    lx += "PORTIX".len() * cw + 4;
    c.write_at(">", lx, ty_txt, Pal::BOR_WARM);
    lx += cw + 4;

    c.write_at("x86_64", lx, ty_txt, Pal::CYAN);
    lx += "x86_64".len() * cw + 4;
    c.write_at(">", lx, ty_txt, Pal::BOR_WARM);
    lx += cw + 4;

    c.write_at("LISTO", lx, ty_txt, Pal::GREEN_NEO);
    lx += "LISTO".len() * cw + 10; // +10 holgura

    let zone_l_end = lx;

    // ── ZONA C (flexible — aparece solo si hay espacio) ───────────────────
    let avail = zone_r_x.saturating_sub(zone_l_end);

    let mut mr  = [0u8; 24];
    let ram_str = fmt_mib(hw.ram.usable_or_default(), &mut mr);
    let ram_w   = (4 + ram_str.len()) * cw + 20;

    let mut ut    = [0u8; 24];
    let up_str    = fmt_uptime(&mut ut);
    let up_label  = "UPTIME:";
    let up_w      = (up_label.len() + 1 + up_str.len()) * cw + 20;

    let both_fit  = avail >= ram_w + up_w;
    let ram_fit   = avail >= ram_w;

    if both_fit {
        let mut cx = zone_l_end + 4;
        c.write_at(">", cx, ty_txt, Pal::BOR_WARM); cx += cw + 6;
        c.write_at("RAM:", cx, ty_txt, Pal::TXT_DIM);
        c.write_at(ram_str, cx + 4 * cw, ty_txt, Pal::GOLD);
        cx += (4 + ram_str.len()) * cw + 10;
        c.write_at(">", cx, ty_txt, Pal::BOR_WARM); cx += cw + 6;
        c.write_at(up_label, cx, ty_txt, Pal::TXT_DIM);
        c.write_at(up_str, cx + (up_label.len() + 1) * cw, ty_txt, Pal::TXT_BRIGHT);
    } else if ram_fit {
        let mut cx = zone_l_end + 4;
        c.write_at(">", cx, ty_txt, Pal::BOR_WARM); cx += cw + 6;
        c.write_at("RAM:", cx, ty_txt, Pal::TXT_DIM);
        c.write_at(ram_str, cx + 4 * cw, ty_txt, Pal::GOLD);
    }
    // Si no cabe nada en C → zona vacía, sin colisión.
}