// ui/chrome.rs — PORTIX Kernel v0.7.4
// Chrome de la UI: cabecera, barra de tabs y status bar
//
// LAYOUT (píxeles, 1024×768):
//   [0   .. HEADER_H)   → cabecera con logo + CPU + badge
//   [HEADER_H .. HEADER_H+3)  → línea dorada divisoria
//   [TAB_Y .. TAB_Y+TAB_H)    → barra de pestañas (sin solapamiento con menú IDE)
//   [CONTENT_Y .. BOTTOM_Y)   → contenido (lo gestiona cada tab)
//   [BOTTOM_Y .. FH)          → status bar inferior
//
// NOTA: El IDE añade su propia barra de menú DENTRO de [CONTENT_Y..BOTTOM_Y].

use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::arch::hardware::HardwareInfo;
use crate::util::fmt::{fmt_u32, fmt_mib, fmt_uptime};
use crate::ui::Tab;

// ─────────────────────────────────────────────────────────────────────────────
// Paleta global del chrome
// ─────────────────────────────────────────────────────────────────────────────

pub struct ChromePal;
impl ChromePal {
    // Cabecera
    pub const HDR_BG:     Color = Color::new(0x04, 0x0B, 0x18);
    pub const HDR_EDGE:   Color = Color::new(0x08, 0x16, 0x2E);
    pub const GOLD_LINE:  Color = Color::new(0xFF, 0xD7, 0x00);
    pub const AMBER:      Color = Color::new(0xFF, 0xAA, 0x00);
    pub const CYAN_SOFT:  Color = Color::new(0x00, 0xCC, 0xEE);
    pub const BADGE_BG:   Color = Color::new(0x00, 0x28, 0x08);
    pub const BADGE_BOR:  Color = Color::new(0x00, 0x88, 0x22);
    pub const BADGE_FG:   Color = Color::new(0x00, 0xEE, 0x44);
    // Tabs
    pub const TAB_BG:     Color = Color::new(0x03, 0x08, 0x14);
    pub const TAB_ACTIVE: Color = Color::new(0x0C, 0x1E, 0x3A);
    pub const TAB_HOVER:  Color = Color::new(0x08, 0x14, 0x28);
    pub const TAB_SEP:    Color = Color::new(0x10, 0x20, 0x38);
    pub const TAB_FG_ACT: Color = Color::new(0xFF, 0xD7, 0x00);
    pub const TAB_FG_HVR: Color = Color::new(0xCC, 0xDD, 0xFF);
    pub const TAB_FG_IDL: Color = Color::new(0x50, 0x68, 0x88);
    pub const FKEY_ACT:   Color = Color::new(0xFF, 0x99, 0x00);
    pub const FKEY_IDL:   Color = Color::new(0x28, 0x38, 0x50);
    // Status bar
    pub const ST_BG:      Color = Color::new(0x04, 0x0B, 0x18);
    pub const ST_SEP:     Color = Color::new(0x18, 0x2C, 0x4A);
    pub const ST_DIM:     Color = Color::new(0x30, 0x44, 0x60);
    pub const ST_MID:     Color = Color::new(0x55, 0x77, 0xAA);
    pub const ST_BRIGHT:  Color = Color::new(0xAA, 0xBB, 0xCC);
}

// ─────────────────────────────────────────────────────────────────────────────
// Etiqueta de sección reutilizable
// ─────────────────────────────────────────────────────────────────────────────

pub fn section_label(c: &mut Console, x: usize, y: usize, title: &str, w: usize) {
    c.fill_rounded(x, y, w, 14, 2, Color::new(0x06, 0x12, 0x26));
    c.hline(x, y + 13, w, ChromePal::ST_SEP);
    c.write_at(title, x + 6, y + 3, Color::new(0x00, 0xBB, 0xAA));
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

    // ═══════════════════════════════════════════════════════════════════
    // CABECERA
    // ═══════════════════════════════════════════════════════════════════
    c.fill_rect(0, 0, fw, lay.header_h, ChromePal::HDR_BG);

    // Acento izquierdo: banda dorada + degradado
    c.fill_rect(0, 0, 6, lay.header_h, ChromePal::GOLD_LINE);
    c.fill_rect(6, 0, 3, lay.header_h, Color::new(0xB8, 0x92, 0x00));
    c.fill_rect(9, 0, 3, lay.header_h, Color::new(0x40, 0x30, 0x00));

    // Logo PORTIX (fuente tall)
    let logo_y = lay.header_h / 2 - 9;
    c.write_at_tall("PORTIX", 18, logo_y, ChromePal::GOLD_LINE);
    c.write_at("v0.7.4", 18, logo_y + 19, ChromePal::AMBER);

    // Separador vertical sutil
    c.vline(86, 8, lay.header_h - 16, Color::new(0x20, 0x34, 0x50));

    // Nombre de CPU centrado
    let brand = hw.cpu.brand_str();
    let brand = if brand.len() > 40 { &brand[..40] } else { brand };
    // Fondo pill para el CPU
    let pill_w = brand.len() * 8 + 20;
    let pill_x = fw / 2 - pill_w / 2;
    let pill_y = (lay.header_h - 16) / 2;
    c.fill_rounded(pill_x, pill_y, pill_w, 16, 3, Color::new(0x08, 0x18, 0x30));
    c.draw_rect(pill_x, pill_y, pill_w, 16, 1, Color::new(0x18, 0x38, 0x60));
    c.write_at(brand, pill_x + 10, pill_y + 4, ChromePal::CYAN_SOFT);

    // Badge BOOT OK (derecha)
    let bx = fw.saturating_sub(110);
    let by = (lay.header_h - 22) / 2;
    c.fill_rounded(bx, by, 100, 22, 4, ChromePal::BADGE_BG);
    c.draw_rect(bx, by, 100, 22, 1, ChromePal::BADGE_BOR);
    // Punto pulsante (simulado con un círculo relleno)
    c.fill_rounded(bx + 8, by + 8, 6, 6, 3, ChromePal::BADGE_FG);
    c.write_at("BOOT OK", bx + 20, by + 7, ChromePal::BADGE_FG);

    // Línea inferior de cabecera
    c.hline(0, lay.header_h - 1, fw, Color::new(0x18, 0x2C, 0x48));

    // ═══════════════════════════════════════════════════════════════════
    // LÍNEA DORADA DIVISORIA
    // ═══════════════════════════════════════════════════════════════════
    // Gradiente: dorado sólido en centro, más tenue en bordes
    c.fill_rect(0,           lay.header_h, fw / 6,       lay.gold_h, Color::new(0x88, 0x70, 0x00));
    c.fill_rect(fw / 6,      lay.header_h, fw * 4 / 6,   lay.gold_h, ChromePal::GOLD_LINE);
    c.fill_rect(fw * 5 / 6,  lay.header_h, fw / 6,       lay.gold_h, Color::new(0x88, 0x70, 0x00));

    // ═══════════════════════════════════════════════════════════════════
    // BARRA DE PESTAÑAS
    // ═══════════════════════════════════════════════════════════════════
    let ty = lay.tab_y;
    let th = lay.tab_h;
    c.fill_rect(0, ty, fw, th, ChromePal::TAB_BG);
    // Línea inferior de las tabs (límite con el contenido)
    c.hline(0, ty + th - 1, fw, ChromePal::TAB_SEP);

    // Datos de tabs: (label, fkey, variante)
    let tab_data: &[(&str, &str, Tab)] = &[
        ("SISTEMA",      "F1", Tab::System),
        ("TERMINAL",     "F2", Tab::Terminal),
        ("DISPOSITIVOS", "F3", Tab::Devices),
        ("IDE",          "F4", Tab::Ide),
        ("ARCHIVOS",     "F5", Tab::Explorer),
    ];

    // Ancho de cada tab: divide el espacio equitativamente hasta 1024px
    let total_tab_w = fw.min(1024);
    let tw = total_tab_w / tab_data.len();

    for (i, &(label, fkey, tab)) in tab_data.iter().enumerate() {
        let tx       = i * tw;
        let is_act   = tab == active;
        let hovered  = !is_act
            && (mx as usize) >= tx
            && (mx as usize) < tx + tw
            && (my as usize) >= ty
            && (my as usize) < ty + th;

        // Fondo de la pestaña
        let bg = if is_act {
            ChromePal::TAB_ACTIVE
        } else if hovered {
            ChromePal::TAB_HOVER
        } else {
            ChromePal::TAB_BG
        };
        c.fill_rect(tx, ty, tw - 1, th, bg);

        // Indicador superior: banda dorada para activa, línea sutil para hover
        if is_act {
            c.fill_rect(tx + 2, ty, tw - 3, 3, ChromePal::GOLD_LINE);
            // Ligero resplandor bajo el indicador
            c.fill_rect(tx + 2, ty + 3, tw - 3, 2, Color::new(0x40, 0x30, 0x00));
        } else if hovered {
            c.fill_rect(tx + 2, ty, tw - 3, 2, Color::new(0x44, 0x44, 0x44));
        }

        // Separador vertical derecho
        c.vline(tx + tw - 1, ty, th, ChromePal::TAB_SEP);

        // Centrar contenido: [F1] + LABEL
        let fkey_w   = fkey.len() * 8;
        let label_w  = label.len() * 8;
        let content_w = fkey_w + 4 + label_w;
        let cx = if tw > content_w + 8 { tx + (tw - content_w) / 2 } else { tx + 4 };
        let cy = ty + (th - 8) / 2;

        // F-key
        let fkey_fg = if is_act { ChromePal::FKEY_ACT } else { ChromePal::FKEY_IDL };
        c.write_at(fkey, cx, cy, fkey_fg);

        // Label
        let label_fg = if is_act {
            ChromePal::TAB_FG_ACT
        } else if hovered {
            ChromePal::TAB_FG_HVR
        } else {
            ChromePal::TAB_FG_IDL
        };
        c.write_at(label, cx + fkey_w + 4, cy, label_fg);
    }

    // ═══════════════════════════════════════════════════════════════════
    // STATUS BAR INFERIOR
    // ═══════════════════════════════════════════════════════════════════
    let sy_bar = lay.bottom_y;
    c.fill_rect(0, sy_bar, fw, 2, ChromePal::GOLD_LINE);
    let bar_h = lay.fh.saturating_sub(sy_bar + 2);
    c.fill_rect(0, sy_bar + 2, fw, bar_h, ChromePal::ST_BG);
    // Línea superior adicional (separador)
    c.hline(0, sy_bar + 1, fw, Color::new(0x80, 0x68, 0x00));

    let sy = sy_bar + 2 + bar_h / 2 - 4;

    // Sección izquierda: logo + versión + arco
    c.write_at("PORTIX",  12,  sy, ChromePal::GOLD_LINE);
    c.write_at("v0.7",    70,  sy, ChromePal::AMBER);
    sep(c, 108, sy);
    c.write_at("x86_64",  118, sy, ChromePal::ST_MID);
    sep(c, 162, sy);
    c.write_at("Listo",   172, sy, Color::new(0x00, 0xCC, 0x88));
    sep(c, 212, sy);

    // Uptime
    let mut ut = [0u8; 24];
    c.write_at("UP:", 222, sy, ChromePal::ST_DIM);
    c.write_at(fmt_uptime(&mut ut), 248, sy, ChromePal::ST_BRIGHT);

    // RAM
    let ram_x = fw.saturating_sub(150);
    let mut mr = [0u8; 24];
    sep(c, ram_x - 10, sy);
    c.write_at("RAM:", ram_x, sy, ChromePal::ST_DIM);
    c.write_at(fmt_mib(hw.ram.usable_or_default(), &mut mr), ram_x + 36, sy, ChromePal::GOLD_LINE);

    // Posición del mouse (extremo derecho, muy tenue)
    let mut bmx = [0u8; 16];
    let mut bmy = [0u8; 16];
    let mxs = fmt_u32(mx.max(0) as u32, &mut bmx);
    let mys = fmt_u32(my.max(0) as u32, &mut bmy);
    let mox = fw.saturating_sub(260);
    sep(c, mox - 10, sy);
    c.write_at("XY:", mox, sy, ChromePal::ST_DIM);
    c.write_at(mxs,  mox + 26,                        sy, Color::new(0x38, 0x52, 0x70));
    c.write_at(",",  mox + 26 + mxs.len() * 8,        sy, ChromePal::ST_DIM);
    c.write_at(mys,  mox + 26 + mxs.len() * 8 + 8,   sy, Color::new(0x38, 0x52, 0x70));
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: separador vertical en status bar
// ─────────────────────────────────────────────────────────────────────────────
#[inline(always)]
fn sep(c: &mut Console, x: usize, y: usize) {
    c.write_at("|", x, y, ChromePal::ST_SEP);
}