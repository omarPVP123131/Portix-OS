// ui/tabs/devices.rs — Pestaña DISPOSITIVOS: CPU, pantalla, disco, E/S, bus PCI

use crate::graphics::driver::framebuffer::{Color, Console, Layout};
use crate::arch::hardware::HardwareInfo;
use crate::drivers::bus::pci::PciBus;
use crate::util::fmt::{fmt_u32, fmt_mhz, fmt_mib, fmt_hex};
use crate::ui::chrome::section_label;

pub fn draw_devices_tab(
    c: &mut Console,
    lay: &Layout,
    hw: &HardwareInfo,
    pci: &PciBus,
) {
    let cy  = lay.content_y;
    let ch  = lay.bottom_y.saturating_sub(cy);
    let fw  = lay.fw;
    let pad = lay.pad;

    c.fill_rect(0, cy, fw, ch, Color::PORTIX_BG);
    c.fill_rect(0, cy, fw, 18, Color::new(2, 8, 18));
    c.hline(0, cy + 17, fw, Color::SEP_BRIGHT);
    c.write_at(" DISPOSITIVOS Y HARDWARE", pad, cy + 5, Color::PORTIX_AMBER);

    let col_w    = fw / 3;
    let ry_start = cy + 24;

    // ── Columna 1: CPU + Pantalla ─────────────────────────────────────────
    let c1x = pad;
    let c1w = col_w - pad * 2;
    let mut ry = ry_start;

    section_label(c, c1x, ry, " PROCESADOR", c1w); ry += 20;
    {
        c.write_at("Fabricante:", c1x + 4,  ry, Color::GRAY);
        c.write_at(hw.cpu.vendor_short(), c1x + 100, ry, Color::WHITE); ry += lay.line_h;

        let mut bc = [0u8; 16]; let mut bl = [0u8; 16];
        c.write_at("Nucleos fis.:", c1x + 4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.cpu.physical_cores as u32, &mut bc), c1x + 116, ry, Color::WHITE); ry += lay.line_h;

        c.write_at("Hilos log.:", c1x + 4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.cpu.logical_cores  as u32, &mut bl), c1x + 100, ry, Color::WHITE); ry += lay.line_h;

        let mut bf = [0u8; 24]; let mut bb = [0u8; 24];
        c.write_at("Turbo:", c1x + 4, ry, Color::GRAY);
        c.write_at(fmt_mhz(hw.cpu.max_mhz,  &mut bf), c1x + 56, ry, Color::CYAN); ry += lay.line_h;

        if hw.cpu.base_mhz > 0 && hw.cpu.base_mhz != hw.cpu.max_mhz {
            c.write_at("Base:", c1x + 4, ry, Color::GRAY);
            c.write_at(fmt_mhz(hw.cpu.base_mhz, &mut bb), c1x + 50, ry, Color::LIGHT_GRAY); ry += lay.line_h;
        }

        let mut be  = [0u8; 18]; let mut be2 = [0u8; 18];
        c.write_at("CPUID max:", c1x + 4, ry, Color::GRAY);
        c.write_at(fmt_hex(hw.cpu.max_leaf     as u64, &mut be),  c1x + 90, ry, Color::TEAL); ry += lay.line_h;
        c.write_at("Ext max:",   c1x + 4, ry, Color::GRAY);
        c.write_at(fmt_hex(hw.cpu.max_ext_leaf as u64, &mut be2), c1x + 74, ry, Color::TEAL); ry += lay.line_h + 4;
    }

    section_label(c, c1x, ry, " PANTALLA", c1w); ry += 20;
    {
        let mut bw = [0u8; 16]; let mut bh = [0u8; 16]; let mut bb = [0u8; 16]; let mut bp = [0u8; 16];
        c.write_at("Resolucion:", c1x + 4,   ry, Color::GRAY);
        c.write_at(fmt_u32(hw.display.width  as u32, &mut bw), c1x + 100, ry, Color::WHITE);
        c.write_at("x",          c1x + 132,  ry, Color::GRAY);
        c.write_at(fmt_u32(hw.display.height as u32, &mut bh), c1x + 142, ry, Color::WHITE); ry += lay.line_h;
        c.write_at("BPP:",   c1x + 4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.display.bpp   as u32, &mut bb), c1x + 40, ry, Color::WHITE); ry += lay.line_h;
        c.write_at("Pitch:", c1x + 4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.display.pitch as u32, &mut bp), c1x + 56, ry, Color::WHITE);
        c.write_at("(DblBuf@0x600000)", c1x + 4, ry + lay.line_h, Color::new(28, 40, 56));
    }

    // ── Columna 2: Almacenamiento + Dispositivos de entrada ───────────────
    let c2x = col_w;
    let mut c2y = ry_start;

    section_label(c, c2x, c2y, " ALMACENAMIENTO", col_w - 8); c2y += 20;
    for i in 0..hw.disks.count.min(4) {
        if c2y + lay.line_h * 2 > lay.bottom_y { break; }
        let d = &hw.disks.drives[i];
        c.fill_rounded(c2x + 4, c2y - 1, 56, 13, 2, Color::new(3, 14, 30));
        c.write_at(if d.bus == 0 { "ATA0" } else { "ATA1" },    c2x + 6,  c2y, Color::TEAL);
        c.write_at(if d.drive == 0 { "-M" } else { "-E" },       c2x + 42, c2y, Color::GRAY);
        c.write_at(if d.is_atapi { "ATAPI" } else { "ATA" },      c2x + 64, c2y, Color::PORTIX_AMBER);
        c2y += lay.line_h - 2;
        let m = d.model_str(); let m = if m.len() > 26 { &m[..26] } else { m };
        c.write_at(m, c2x + 8, c2y, Color::WHITE); c2y += lay.line_h - 2;
        if !d.is_atapi {
            let mut sb = [0u8; 24];
            c.write_at(fmt_mib(d.size_mb, &mut sb), c2x + 8, c2y, Color::PORTIX_GOLD);
            if d.lba48 {
                c.fill_rounded(c2x + 100, c2y - 1, 46, 12, 2, Color::new(0, 28, 8));
                c.write_at("LBA48", c2x + 104, c2y, Color::GREEN);
            }
        } else {
            c.write_at("Optico / extraible", c2x + 8, c2y, Color::GRAY);
        }
        c2y += lay.line_h;
    }

    c2y += 4;
    section_label(c, c2x, c2y, " DISPOSITIVOS DE ENTRADA", col_w - 8); c2y += 20;
    c.write_at("Teclado PS/2:", c2x + 4, c2y, Color::GRAY);
    c.fill_rounded(c2x + 116, c2y - 2, 50, 13, 3, Color::new(0, 30, 8));
    c.write_at("● Activo", c2x + 120, c2y, Color::NEON_GREEN); c2y += lay.line_h;

    c.write_at("Raton PS/2:", c2x + 4, c2y, Color::GRAY);
    c.fill_rounded(c2x + 100, c2y - 2, 50, 13, 3, Color::new(0, 30, 8));
    c.write_at("● Activo", c2x + 104, c2y, Color::NEON_GREEN); c2y += lay.line_h;

    c.write_at("Rueda scroll:", c2x + 4, c2y, Color::GRAY);
    c.fill_rounded(c2x + 116, c2y - 2, 76, 13, 3, Color::new(0, 30, 8));
    c.write_at("● IntelliMouse", c2x + 120, c2y, Color::NEON_GREEN);

    // ── Columna 3: Bus PCI ────────────────────────────────────────────────
    let c3x = col_w * 2;
    let c3w = fw.saturating_sub(c3x + pad);
    let mut c3y = ry_start;

    // Título dinámico con recuento de dispositivos
    {
        let mut tbuf = [0u8; 24]; let mut pos = 0usize;
        let ts = b" BUS PCI ("; tbuf[..ts.len()].copy_from_slice(ts); pos += ts.len();
        let mut cnt_buf = [0u8; 16];
        let s = fmt_u32(pci.count as u32, &mut cnt_buf);
        for b in s.bytes() { if pos < 24 { tbuf[pos] = b; pos += 1; } }
        if pos < 24 { tbuf[pos] = b')'; pos += 1; }
        let title = core::str::from_utf8(&tbuf[..pos]).unwrap_or(" BUS PCI");
        section_label(c, c3x, c3y, title, c3w); c3y += 20;
    }

    for i in 0..pci.count.min(14) {
        if c3y + lay.line_h > lay.bottom_y - 6 { break; }
        let d = &pci.devices[i];
        const H: &[u8] = b"0123456789ABCDEF";
        let vhex: [u8; 4] = [
            H[((d.vendor_id >> 12) & 0xF) as usize], H[((d.vendor_id >> 8)  & 0xF) as usize],
            H[((d.vendor_id >>  4) & 0xF) as usize], H[(d.vendor_id         & 0xF) as usize],
        ];
        let dhex: [u8; 4] = [
            H[((d.device_id >> 12) & 0xF) as usize], H[((d.device_id >> 8)  & 0xF) as usize],
            H[((d.device_id >>  4) & 0xF) as usize], H[(d.device_id         & 0xF) as usize],
        ];
        c.write_at(core::str::from_utf8(&vhex).unwrap_or("????"), c3x + 4,  c3y, Color::TEAL);
        c.write_at(":", c3x + 40, c3y, Color::GRAY);
        c.write_at(core::str::from_utf8(&dhex).unwrap_or("????"), c3x + 50, c3y, Color::TEAL);
        let cn = d.class_name(); let cn = if cn.len() > 18 { &cn[..18] } else { cn };
        c.write_at(cn, c3x + 98, c3y, Color::LIGHT_GRAY);
        c3y += lay.line_h - 1;
    }
}
