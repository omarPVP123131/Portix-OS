// ui/chrome.rs — PORTIX Kernel v0.7.4
// Chrome de la UI: cabecera, barra de tabs y status bar

use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::arch::hardware::HardwareInfo;
use crate::util::fmt::{fmt_u32, fmt_mib, fmt_uptime};
use crate::ui::Tab;

/// Etiqueta de sección con línea separadora inferior
pub fn section_label(c: &mut Console, x: usize, y: usize, title: &str, w: usize) {
    c.fill_rounded(x, y, w, 14, 2, Color::new(4, 14, 30));
    c.hline(x, y + 13, w, Color::SEP_BRIGHT);
    c.write_at(title, x + 6, y + 3, Color::TEAL);
}

/// Dibuja la cabecera, las pestañas y la barra de estado inferior.
pub fn draw_chrome(
    c: &mut Console,
    lay: &Layout,
    hw: &HardwareInfo,
    active: Tab,
    mx: i32,
    my: i32,
) {
let fw = lay.fw;
    // ── Cabecera ──────────────────────────────────────────────────────────
c.fill_rect(0, 0, fw, lay.header_h, Color::HEADER_BG);
    c.fill_rect(0, 0, 6, lay.header_h, Color::PORTIX_GOLD);
    c.fill_rect(6, 0, 2, lay.header_h, Color::new(180, 120, 0));
    c.write_at_tall("PORTIX", 16, lay.header_h / 2 - 8, Color::PORTIX_GOLD);
    c.write_at("v0.7", 16, lay.header_h / 2 + 9, Color::PORTIX_AMBER);

    let brand = hw.cpu.brand_str();
    let brand = if brand.len() > 38 { &brand[..38] } else { brand };
    let bx = fw / 2 - (brand.len() * 9) / 2;
    c.write_at(brand, bx, lay.header_h / 2 - 4, Color::CYAN);

    let bx = fw.saturating_sub(100);
    let by = (lay.header_h - 22) / 2;
    c.fill_rounded(bx, by, 92, 22, 4, Color::new(0, 40, 10));
    c.draw_rect(bx, by, 92, 22, 1, Color::new(0, 100, 30));
    c.write_at("BOOT OK", bx + 8, by + 7, Color::GREEN);

    // ── Línea dorada divisoria ────────────────────────────────────────────
    c.fill_rect(0, lay.header_h, fw, lay.gold_h, Color::PORTIX_GOLD);

    // ── Barra de tabs ─────────────────────────────────────────────────────
    let ty = lay.tab_y;
    c.fill_rect(0, ty, fw, lay.tab_h + 2, Color::TAB_INACTIVE);

    // Datos de tabs: (label, fkey_label, variante Tab)
    let tab_data: &[(&str, &str, Tab)] = &[
        ("SISTEMA",      "F1", Tab::System),
        ("TERMINAL",     "F2", Tab::Terminal),
        ("DISPOSITIVOS", "F3", Tab::Devices),
        ("IDE",          "F4", Tab::Ide),
        ("ARCHIVOS",     "F5", Tab::Explorer),
    ];

    let tw: usize = (fw.min(1000)) / tab_data.len();

    for (i, &(label, fkey, tab)) in tab_data.iter().enumerate() {
        let tx        = i * tw;
        let is_active = tab == active;
        let hovered   = !is_active
            && (mx as usize) >= tx && (mx as usize) < tx + tw
            && (my as usize) >= ty && (my as usize) < ty + lay.tab_h + 2;

        if is_active {
            c.fill_rect(tx, ty,     tw - 1, 2,         Color::PORTIX_GOLD);
            c.fill_rect(tx, ty + 2, tw - 1, lay.tab_h, Color::TAB_ACTIVE);
        } else {
            let bg = if hovered { Color::new(0x0C, 0x18, 0x30) } else { Color::TAB_INACTIVE };
            c.fill_rect(tx, ty, tw - 1, lay.tab_h + 2, bg);
        }
        c.fill_rect(tx + tw - 1, ty, 1, lay.tab_h + 2, Color::SEPARATOR);

        let fy = ty + 2 + lay.tab_h / 2 - 4;

        // F-key en tono apagado a la izquierda
        let fkey_fg = if is_active { Color::PORTIX_AMBER } else { Color::new(0x30, 0x40, 0x55) };
        c.write_at(fkey, tx + 4, fy, fkey_fg);

        // Label centrado
        let fg = if is_active    { Color::PORTIX_GOLD }
                 else if hovered { Color::LIGHT_GRAY  }
                 else            { Color::GRAY        };
        let label_px = label.len() * 9;
        let lx = if tw > label_px + 28 { tx + (tw + 28 - label_px) / 2 } else { tx + 28 };
        c.write_at(label, lx, fy, fg);
    }

    // ── Barra de estado inferior ──────────────────────────────────────────
    let sy_bar = lay.bottom_y;
    c.fill_rect(0, sy_bar, fw, 2, Color::PORTIX_GOLD);
    let bar_h = lay.fh.saturating_sub(sy_bar + 2);
    c.fill_rect(0, sy_bar + 2, fw, bar_h, Color::HEADER_BG);

    let sy = sy_bar + 2 + bar_h / 2 - 4;
    c.write_at("PORTIX",  12,  sy, Color::PORTIX_GOLD);
    c.write_at("v0.7",    66,  sy, Color::PORTIX_AMBER);
    c.write_at("|",       102, sy, Color::SEP_BRIGHT);
    c.write_at("x86_64",  112, sy, Color::GRAY);
    c.write_at("|",       160, sy, Color::SEP_BRIGHT);
    c.write_at("Listo",   183, sy, Color::TEAL);
    c.write_at("|",       228, sy, Color::SEP_BRIGHT);

    let mut ut = [0u8; 24];
    c.write_at("Tiempo:", 238, sy, Color::GRAY);
    c.write_at(fmt_uptime(&mut ut), 298, sy, Color::LIGHT_GRAY);

    let mut mr = [0u8; 24];
    c.write_at("RAM:", fw.saturating_sub(140), sy, Color::GRAY);
    c.write_at(fmt_mib(hw.ram.usable_or_default(), &mut mr), fw.saturating_sub(100), sy, Color::PORTIX_GOLD);

    let mut bmx = [0u8; 16]; let mut bmy = [0u8; 16];
    let mxs = fmt_u32(mx.max(0) as u32, &mut bmx);
    let mys = fmt_u32(my.max(0) as u32, &mut bmy);
    let mox = fw.saturating_sub(260);
    c.write_at("XY:",  mox,                              sy, Color::new(30, 42, 58));
    c.write_at(mxs,   mox + 28,                          sy, Color::new(44, 60, 80));
    c.write_at(",",   mox + 28 + mxs.len() * 9,          sy, Color::new(30, 42, 58));
    c.write_at(mys,   mox + 28 + mxs.len() * 9 + 9,      sy, Color::new(44, 60, 80));
}