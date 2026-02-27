// ui/tabs/system.rs — Pestaña SISTEMA: log de arranque + info de CPU/RAM/disco/pantalla

use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::arch::hardware::HardwareInfo;
use crate::util::fmt::{fmt_u32, fmt_mhz, fmt_mib, fmt_hex};
use crate::ui::chrome::section_label;

pub fn draw_system_tab(
    c: &mut Console,
    lay: &Layout,
    hw: &HardwareInfo,
    boot_lines: &[(&str, &str, Color)],
) {
    let cy  = lay.content_y;
    let ch  = lay.bottom_y.saturating_sub(cy);
    let fw  = lay.fw;
    let pad = lay.pad;

    c.fill_rect(0, cy, fw, ch, Color::PORTIX_BG);

    // Línea divisoria vertical punteada
    for y in (cy + 8..lay.bottom_y - 8).step_by(4) {
        c.fill_rect(lay.col_div, y, 1, 2, Color::SEP_BRIGHT);
    }

    // ── Columna izquierda: log de arranque ────────────────────────────────
    let sec_w = lay.col_div - pad - 6;
    section_label(c, pad, cy + 6, " LOG DE ARRANQUE", sec_w);

    let mut ly = cy + 25;
    for &(tag, msg, col) in boot_lines {
        if ly + lay.line_h > lay.bottom_y.saturating_sub(6) { break; }
        c.fill_rounded(pad, ly - 1, 52, 13, 3, Color::new(0, 35, 10));
        c.write_at(tag,        pad + 2,  ly, col);
        c.write_at(msg,        pad + 64, ly, Color::LIGHT_GRAY);
        ly += lay.line_h + 3;
    }

    // ── Columna derecha ───────────────────────────────────────────────────
    let rx = lay.right_x;
    let rw = fw.saturating_sub(rx + pad);
    let mut ry = cy + 6;

    // Procesador
    section_label(c, rx, ry, " PROCESADOR", rw); ry += 20;
    let brand = hw.cpu.brand_str();
    let brand = if brand.len() > 34 { &brand[..34] } else { brand };
    c.write_at(brand, rx + 6, ry, Color::WHITE); ry += lay.line_h + 2;

    {
        let mut bc = [0u8; 16]; let mut bl = [0u8; 16]; let mut bf = [0u8; 24];
        let pc  = fmt_u32(hw.cpu.physical_cores as u32, &mut bc);
        let lc  = fmt_u32(hw.cpu.logical_cores  as u32, &mut bl);
        c.write_at(pc,  rx + 6,                       ry, Color::PORTIX_GOLD);
        c.write_at("C /", rx + 6 + pc.len() * 9,      ry, Color::GRAY);
        c.write_at(lc,  rx + 6 + pc.len() * 9 + 28,   ry, Color::PORTIX_GOLD);
        c.write_at("T",  rx + 6 + pc.len() * 9 + 28 + lc.len() * 9, ry, Color::GRAY);
        let freq = fmt_mhz(hw.cpu.max_mhz, &mut bf);
        c.fill_rounded(rx + rw - freq.len() * 9 - 18, ry - 2, freq.len() * 9 + 14, 14, 3, Color::new(0, 25, 50));
        c.write_at(freq, rx + rw - freq.len() * 9 - 11, ry, Color::CYAN);
        ry += lay.line_h + 4;
    }

    // Badges de extensiones
    {
        macro_rules! badge { ($label:expr, $on:expr, $bx:expr) => {{
            let (bg, fg, br) = if $on {
                (Color::new(0,30,10), Color::NEON_GREEN, Color::new(0,70,25))
            } else {
                (Color::new(6,8,12), Color::new(40,48,56), Color::new(14,20,26))
            };
            c.fill_rounded($bx, ry, 42, 14, 3, bg);
            c.draw_rect($bx, ry, 42, 14, 1, br);
            c.write_at($label, $bx + 5, ry + 3, fg);
        }}}
        let fx = rx + 6;
        badge!("SSE2", hw.cpu.has_sse2, fx);
        badge!("SSE4", hw.cpu.has_sse4, fx + 48);
        badge!("AVX",  hw.cpu.has_avx,  fx + 96);
        badge!("AVX2", hw.cpu.has_avx2, fx + 144);
        badge!("AES",  hw.cpu.has_aes,  fx + 192);
        ry += 22;
    }

    // Memoria
    section_label(c, rx, ry, " MEMORIA", rw); ry += 20;
    {
        let usable = hw.ram.usable_or_default();
        let mut bu = [0u8; 24];
        c.write_at(fmt_mib(usable, &mut bu), rx + 6, ry, Color::WHITE);
        c.write_at("RAM utilizable", rx + 88, ry, Color::GRAY);
        ry += lay.line_h;
        c.gradient_bar(rx + 6, ry, rw - 16, 8, 100, Color::TEAL, Color::new(3, 12, 24));
        ry += 12;
        let mut be = [0u8; 16];
        c.write_at("E820:",    rx + 6,  ry, Color::GRAY);
        c.write_at(fmt_u32(hw.ram.entry_count as u32, &mut be), rx + 50, ry, Color::LIGHT_GRAY);
        c.write_at("entradas", rx + 50 + 5 * 9, ry, Color::GRAY);
        ry += lay.line_h + 4;
    }

    // Almacenamiento
    section_label(c, rx, ry, " ALMACENAMIENTO", rw); ry += 20;
    for i in 0..hw.disks.count.min(3) {
        if ry + lay.line_h > lay.bottom_y.saturating_sub(50) { break; }
        let d = &hw.disks.drives[i];
        c.fill_rounded(rx + 6, ry - 1, 50, 13, 2, Color::new(4, 16, 36));
        c.write_at(if d.bus == 0 { "ATA0" } else { "ATA1" }, rx + 8,  ry + 1, Color::TEAL);
        c.write_at("-",                                        rx + 40, ry + 1, Color::GRAY);
        c.write_at(if d.drive == 0 { "M" } else { "S" },      rx + 48, ry + 1, Color::TEAL);
        c.write_at(if d.is_atapi { "OPT" } else { "HDD" },    rx + 64, ry,     Color::PORTIX_AMBER);
        let m = d.model_str(); let m = if m.len() > 22 { &m[..22] } else { m };
        c.write_at(m, rx + 94, ry, Color::WHITE);
        ry += lay.line_h - 1;
        if !d.is_atapi {
            let mut sb = [0u8; 24];
            c.write_at(fmt_mib(d.size_mb, &mut sb), rx + 20, ry, Color::PORTIX_GOLD);
            if d.lba48 {
                c.fill_rounded(rx + 100, ry - 1, 46, 12, 2, Color::new(0, 30, 8));
                c.write_at("LBA48", rx + 104, ry, Color::GREEN);
            }
        } else {
            c.write_at("Optico / ATAPI", rx + 20, ry, Color::GRAY);
        }
        ry += lay.line_h;
    }

    // Pantalla
    if ry + 32 < lay.bottom_y {
        ry += 2;
        section_label(c, rx, ry, " PANTALLA", rw); ry += 20;
        let mut bw = [0u8; 16]; let mut bh = [0u8; 16]; let mut bb = [0u8; 16];
        let ws = fmt_u32(hw.display.width  as u32, &mut bw);
        let hs = fmt_u32(hw.display.height as u32, &mut bh);
        let bs = fmt_u32(hw.display.bpp    as u32, &mut bb);
        c.write_at(ws,    rx + 6,              ry, Color::WHITE);
        c.write_at("x",   rx + 6 + ws.len() * 9, ry, Color::GRAY);
        c.write_at(hs,    rx + 60,             ry, Color::WHITE);
        c.write_at("@",   rx + 108,            ry, Color::GRAY);
        c.write_at(bs,    rx + 122,            ry, Color::WHITE);
        c.write_at("bpp", rx + 140,            ry, Color::GRAY);
        let _ = ry;
    }

    // Suprimir warning si fmt_hex no es usada en esta pestaña
    let _ = fmt_hex;
}
