// kernel/src/main.rs — PORTIX Kernel v0.7.2
//
// CORRECCIONES vs v0.7.1:
//   - Tabs: solo se cambian con CLIC (left_clicked), NO con hover.
//     El hover ahora es solo un efecto visual muy sutil (texto más brillante).
//   - Scrollbar: ahora es arrastrable con el mouse.
//     Ancho aumentado a 12px para ser clickable cómodamente.
//     Cálculo de max_scroll corregido usando term.max_scroll() que respeta
//     el ring-buffer (no excede TERM_ROWS líneas disponibles).
//   - Scroll: SOLO ocurre desde rueda del mouse (scroll_delta) o teclado.
//     Nunca desde movimiento en Y del cursor.
//   - visible_range: ahora usa term.line_at(start + i) correctamente.
//   - Anti-flicker: present() limitado a RENDER_HZ (30 Hz) como antes.
#![no_std]
#![no_main]
#![allow(dead_code)]

mod acpi;
mod font;
mod framebuffer;
mod halt;
mod hardware;
mod idt;
mod keyboard;
mod mouse;
mod pit;
mod pci;
mod serial;
mod terminal;

use core::arch::global_asm;
use core::panic::PanicInfo;
use framebuffer::{Color, Console, Layout};
use halt::halt_loop;
use keyboard::Key;
use terminal::LineColor;

extern "C" {
    static __bss_start: u8;
    static __bss_end:   u8;
}

global_asm!(
    ".section .text._start, \"ax\"",
    ".global _start",
    ".code64",
    "_start:",
    "    cli",
    "    cld",
    "    mov rsp, 0x7FF00",
    "    xor rbp, rbp",
    "    lea rdi, [rip + {BSS_START}]",
    "    lea rcx, [rip + {BSS_END}]",
    "    sub rcx, rdi",
    "    jz 1f",
    "    test rcx, rcx",
    "    js  1f",
    "    xor eax, eax",
    "    rep stosb",
    "1:",
    "    call {RUST_MAIN}",
    "2:  hlt",
    "    jmp 2b",
    BSS_START = sym __bss_start,
    BSS_END   = sym __bss_end,
    RUST_MAIN = sym rust_main,
);

// ── Configuración de render ───────────────────────────────────────────────────
const RENDER_HZ:       u64 = 30;
const RENDER_INTERVAL: u64 = 100 / RENDER_HZ; // ticks entre presents al LFB

// ── Scrollbar (terminal) ──────────────────────────────────────────────────────
/// Ancho de la barra lateral de scroll en píxeles
const SCROLLBAR_W: usize = 12;

// ── Tabs ──────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab { System = 0, Terminal = 1, Devices = 2 }

// ── Formato de números ────────────────────────────────────────────────────────
fn fmt_u32<'a>(mut n: u32, buf: &'a mut [u8; 16]) -> &'a str {
    if n == 0 { buf[0] = b'0'; return core::str::from_utf8(&buf[..1]).unwrap_or("0"); }
    let mut i = 0usize;
    while n > 0 && i < 16 { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    buf[..i].reverse();
    core::str::from_utf8(&buf[..i]).unwrap_or("?")
}
fn fmt_u64<'a>(mut n: u64, buf: &'a mut [u8; 20]) -> &'a str {
    if n == 0 { buf[0] = b'0'; return core::str::from_utf8(&buf[..1]).unwrap_or("0"); }
    let mut i = 0usize;
    while n > 0 && i < 20 { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    buf[..i].reverse();
    core::str::from_utf8(&buf[..i]).unwrap_or("?")
}
fn fmt_hex<'a>(mut v: u64, buf: &'a mut [u8; 18]) -> &'a str {
    buf[0] = b'0'; buf[1] = b'x';
    const H: &[u8] = b"0123456789ABCDEF";
    for i in 0..16 { buf[17 - i] = H[(v & 0xF) as usize]; v >>= 4; }
    core::str::from_utf8(buf).unwrap_or("0x????????????????")
}
fn fmt_mhz<'a>(mhz: u32, buf: &'a mut [u8; 24]) -> &'a str {
    if mhz == 0 { buf[..3].copy_from_slice(b"N/A"); return core::str::from_utf8(&buf[..3]).unwrap_or("N/A"); }
    let mut pos = 0usize;
    if mhz >= 1000 {
        let gi = mhz / 1000; let gf = (mhz % 1000) / 10;
        let mut t = [0u8; 16]; let s = fmt_u32(gi, &mut t);
        for b in s.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        if pos < 24 { buf[pos] = b'.'; pos += 1; }
        if gf < 10 && pos < 24 { buf[pos] = b'0'; pos += 1; }
        let mut t2 = [0u8; 16]; let sf = fmt_u32(gf, &mut t2);
        for b in sf.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        for b in b" GHz" { if pos < 24 { buf[pos] = *b; pos += 1; } }
    } else {
        let mut t = [0u8; 16]; let s = fmt_u32(mhz, &mut t);
        for b in s.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        for b in b" MHz" { if pos < 24 { buf[pos] = *b; pos += 1; } }
    }
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}
fn fmt_mib<'a>(mb: u64, buf: &'a mut [u8; 24]) -> &'a str {
    if mb == 0 { buf[0] = b'0'; buf[1] = b'B'; return core::str::from_utf8(&buf[..2]).unwrap_or("0"); }
    let mut pos = 0usize;
    if mb >= 1024 {
        let gi = mb / 1024; let gf = (mb % 1024) * 10 / 1024;
        let mut t = [0u8; 20]; let s = fmt_u64(gi, &mut t);
        for b in s.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        if pos < 24 { buf[pos] = b'.'; pos += 1; }
        if pos < 24 { buf[pos] = b'0' + gf as u8; pos += 1; }
        for b in b" GB" { if pos < 24 { buf[pos] = *b; pos += 1; } }
    } else {
        let mut t = [0u8; 20]; let s = fmt_u64(mb, &mut t);
        for b in s.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        for b in b" MB" { if pos < 24 { buf[pos] = *b; pos += 1; } }
    }
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}
fn fmt_uptime<'a>(buf: &'a mut [u8; 24]) -> &'a str {
    let (h, m, s) = pit::uptime_hms();
    let mut pos = 0usize;
    macro_rules! push2 { ($n:expr) => {{
        if $n < 10 { buf[pos] = b'0'; pos += 1; }
        let mut t = [0u8; 16]; let st = fmt_u32($n, &mut t);
        for b in st.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
    }}}
    push2!(h); if pos < 24 { buf[pos] = b':'; pos += 1; }
    push2!(m); if pos < 24 { buf[pos] = b':'; pos += 1; }
    push2!(s);
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}

// ── Section label ─────────────────────────────────────────────────────────────
fn section_label(c: &mut Console, x: usize, y: usize, title: &str, w: usize) {
    c.fill_rounded(x, y, w, 14, 2, Color::new(4, 14, 30));
    c.hline(x, y + 13, w, Color::SEP_BRIGHT);
    c.write_at(title, x + 6, y + 3, Color::TEAL);
}

// ── Chrome ────────────────────────────────────────────────────────────────────
fn draw_chrome(c: &mut Console, lay: &Layout, hw: &hardware::HardwareInfo,
               active: Tab, mx: i32, my: i32) {
    let fw = lay.fw;

    // Cabecera
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
    c.write_at("● BOOT OK", bx + 8, by + 7, Color::GREEN);

    // Línea dorada separadora
    c.fill_rect(0, lay.header_h, fw, lay.gold_h, Color::PORTIX_GOLD);

    // ── Barra de tabs ─────────────────────────────────────────────────────────
    //
    // CORRECCIÓN: el hover solo cambia el COLOR del texto, NUNCA cambia la tab
    // activa. El cambio de tab requiere un clic izquierdo (manejado en main).
    let ty = lay.tab_y;
    c.fill_rect(0, ty, fw, lay.tab_h + 2, Color::TAB_INACTIVE);

    let tw = lay.tab_w;
    let tab_data: &[(&str, Tab)] = &[
        (" F1  SISTEMA  ", Tab::System),
        (" F2  TERMINAL ", Tab::Terminal),
        (" F3  DISPOSITIVOS", Tab::Devices),
    ];

    for (i, &(label, tab)) in tab_data.iter().enumerate() {
        let tx = i * tw;
        let is_active = tab == active;

        // Hover: solo comprobamos posición para efecto visual; NO cambia tab
        let hovered = !is_active
            && (mx as usize) >= tx && (mx as usize) < tx + tw
            && (my as usize) >= ty && (my as usize) < ty + lay.tab_h + 2;

        if is_active {
            // Tab activa: barra dorada + fondo oscuro azul
            c.fill_rect(tx, ty, tw - 1, 2, Color::PORTIX_GOLD);
            c.fill_rect(tx, ty + 2, tw - 1, lay.tab_h, Color::TAB_ACTIVE);
        } else {
            // Tab inactiva: fondo uniforme, SIN cambio de fondo en hover
            // (evita la confusión visual de que "se seleccionó")
            c.fill_rect(tx, ty, tw - 1, lay.tab_h + 2, Color::TAB_INACTIVE);
        }

        // Separador vertical entre tabs
        c.fill_rect(tx + tw - 1, ty, 1, lay.tab_h + 2, Color::SEPARATOR);

        let fy = ty + 2 + lay.tab_h / 2 - 4;
        // Activa → dorado, hover → gris claro, inactiva → gris oscuro
        let fg = if is_active { Color::PORTIX_GOLD }
                 else if hovered { Color::LIGHT_GRAY }
                 else { Color::GRAY };
        c.write_at(label, tx + 4, fy, fg);
    }

    // Pista de ayuda de teclado
    let hx = tab_data.len() * tw + 14;
    let hy = ty + lay.tab_h / 2 - 4 + 2;
    if hx + 320 < fw {
        c.write_at("CLIC=cambiar tab  ESC=limpiar  Rueda/RePag=scroll", hx, hy,
                   Color::new(28, 40, 56));
    }

    // Barra de estado inferior
    let sy_bar = lay.bottom_y;
    c.fill_rect(0, sy_bar, fw, 2, Color::PORTIX_GOLD);
    let bar_h = lay.fh.saturating_sub(sy_bar + 2);
    c.fill_rect(0, sy_bar + 2, fw, bar_h, Color::HEADER_BG);
    let sy = sy_bar + 2 + bar_h / 2 - 4;

    c.write_at("PORTIX", 12, sy, Color::PORTIX_GOLD);
    c.write_at("v0.7", 66, sy, Color::PORTIX_AMBER);
    c.write_at("|", 102, sy, Color::SEP_BRIGHT);
    c.write_at("x86_64", 112, sy, Color::GRAY);
    c.write_at("|", 160, sy, Color::SEP_BRIGHT);
    c.write_at("●", 170, sy, Color::NEON_GREEN);
    c.write_at("Listo", 183, sy, Color::TEAL);
    c.write_at("|", 228, sy, Color::SEP_BRIGHT);
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
    c.write_at("XY:", mox, sy, Color::new(30, 42, 58));
    c.write_at(mxs, mox + 28, sy, Color::new(44, 60, 80));
    c.write_at(",", mox + 28 + mxs.len() * 9, sy, Color::new(30, 42, 58));
    c.write_at(mys, mox + 28 + mxs.len() * 9 + 9, sy, Color::new(44, 60, 80));
}

// ── SYSTEM tab ────────────────────────────────────────────────────────────────
fn draw_system_tab(c: &mut Console, lay: &Layout, hw: &hardware::HardwareInfo,
                   boot_lines: &[(&str, &str, Color)]) {
    let cy  = lay.content_y;
    let ch  = lay.bottom_y.saturating_sub(cy);
    let fw  = lay.fw;
    let pad = lay.pad;
    c.fill_rect(0, cy, fw, ch, Color::PORTIX_BG);
    for y in (cy + 8..lay.bottom_y - 8).step_by(4) {
        c.fill_rect(lay.col_div, y, 1, 2, Color::SEP_BRIGHT);
    }
    let sec_w = lay.col_div - pad - 6;
    section_label(c, pad, cy + 6, " LOG DE ARRANQUE", sec_w);
    let mut ly = cy + 25;
    for &(tag, msg, col) in boot_lines {
        if ly + lay.line_h > lay.bottom_y.saturating_sub(6) { break; }
        c.fill_rounded(pad, ly - 1, 52, 13, 3, Color::new(0, 35, 10));
        c.write_at(tag, pad + 2, ly, col);
        c.write_at(msg, pad + 64, ly, Color::LIGHT_GRAY);
        ly += lay.line_h + 3;
    }
    let rx = lay.right_x;
    let rw = fw.saturating_sub(rx + pad);
    let mut ry = cy + 6;
    section_label(c, rx, ry, " PROCESADOR", rw); ry += 20;
    let brand = hw.cpu.brand_str();
    let brand = if brand.len() > 34 { &brand[..34] } else { brand };
    c.write_at(brand, rx + 6, ry, Color::WHITE); ry += lay.line_h + 2;
    {
        let mut bc = [0u8; 16]; let mut bl = [0u8; 16]; let mut bf = [0u8; 24];
        let pc = fmt_u32(hw.cpu.physical_cores as u32, &mut bc);
        let lc = fmt_u32(hw.cpu.logical_cores  as u32, &mut bl);
        c.write_at(pc, rx+6, ry, Color::PORTIX_GOLD);
        c.write_at("C /", rx+6+pc.len()*9, ry, Color::GRAY);
        c.write_at(lc, rx+6+pc.len()*9+28, ry, Color::PORTIX_GOLD);
        c.write_at("T", rx+6+pc.len()*9+28+lc.len()*9, ry, Color::GRAY);
        let freq = fmt_mhz(hw.cpu.max_mhz, &mut bf);
        c.fill_rounded(rx+rw-freq.len()*9-18, ry-2, freq.len()*9+14, 14, 3, Color::new(0,25,50));
        c.write_at(freq, rx+rw-freq.len()*9-11, ry, Color::CYAN);
        ry += lay.line_h + 4;
    }
    {
        macro_rules! badge { ($label:expr, $on:expr, $bx:expr) => {{
            let (bg, fg, br) = if $on {
                (Color::new(0,30,10), Color::NEON_GREEN, Color::new(0,70,25))
            } else {
                (Color::new(6,8,12), Color::new(40,48,56), Color::new(14,20,26))
            };
            c.fill_rounded($bx, ry, 42, 14, 3, bg);
            c.draw_rect($bx, ry, 42, 14, 1, br);
            c.write_at($label, $bx+5, ry+3, fg);
        }}}
        let fx = rx + 6;
        badge!("SSE2", hw.cpu.has_sse2, fx);
        badge!("SSE4", hw.cpu.has_sse4, fx+48);
        badge!("AVX",  hw.cpu.has_avx,  fx+96);
        badge!("AVX2", hw.cpu.has_avx2, fx+144);
        badge!("AES",  hw.cpu.has_aes,  fx+192);
        ry += 22;
    }
    section_label(c, rx, ry, " MEMORIA", rw); ry += 20;
    {
        let usable = hw.ram.usable_or_default();
        let mut bu = [0u8; 24];
        c.write_at(fmt_mib(usable, &mut bu), rx+6, ry, Color::WHITE);
        c.write_at("RAM utilizable", rx+88, ry, Color::GRAY);
        ry += lay.line_h;
        c.gradient_bar(rx+6, ry, rw-16, 8, 100, Color::TEAL, Color::new(3,12,24));
        ry += 12;
        let mut be = [0u8; 16];
        c.write_at("E820:", rx+6, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.ram.entry_count as u32, &mut be), rx+50, ry, Color::LIGHT_GRAY);
        c.write_at("entradas", rx+50+5*9, ry, Color::GRAY);
        ry += lay.line_h + 4;
    }
    section_label(c, rx, ry, " ALMACENAMIENTO", rw); ry += 20;
    for i in 0..hw.disks.count.min(3) {
        if ry + lay.line_h > lay.bottom_y.saturating_sub(50) { break; }
        let d = &hw.disks.drives[i];
        c.fill_rounded(rx+6, ry-1, 50, 13, 2, Color::new(4,16,36));
        c.write_at(if d.bus==0 { "ATA0" } else { "ATA1" }, rx+8, ry+1, Color::TEAL);
        c.write_at("-", rx+40, ry+1, Color::GRAY);
        c.write_at(if d.drive==0 { "M" } else { "S" }, rx+48, ry+1, Color::TEAL);
        c.write_at(if d.is_atapi { "OPT" } else { "HDD" }, rx+64, ry, Color::PORTIX_AMBER);
        let m = d.model_str(); let m = if m.len()>22 { &m[..22] } else { m };
        c.write_at(m, rx+94, ry, Color::WHITE);
        ry += lay.line_h - 1;
        if !d.is_atapi {
            let mut sb = [0u8; 24];
            c.write_at(fmt_mib(d.size_mb, &mut sb), rx+20, ry, Color::PORTIX_GOLD);
            if d.lba48 {
                c.fill_rounded(rx+100, ry-1, 46, 12, 2, Color::new(0,30,8));
                c.write_at("LBA48", rx+104, ry, Color::GREEN);
            }
        } else {
            c.write_at("Optico / ATAPI", rx+20, ry, Color::GRAY);
        }
        ry += lay.line_h;
    }
    if ry + 32 < lay.bottom_y {
        ry += 2;
        section_label(c, rx, ry, " PANTALLA", rw); ry += 20;
        let mut bw=[0u8;16]; let mut bh=[0u8;16]; let mut bb=[0u8;16];
        let ws = fmt_u32(hw.display.width  as u32, &mut bw);
        let hs = fmt_u32(hw.display.height as u32, &mut bh);
        let bs = fmt_u32(hw.display.bpp    as u32, &mut bb);
        c.write_at(ws, rx+6, ry, Color::WHITE);
        c.write_at("x", rx+6+ws.len()*9, ry, Color::GRAY);
        c.write_at(hs, rx+60, ry, Color::WHITE);
        c.write_at("@", rx+108, ry, Color::GRAY);
        c.write_at(bs, rx+122, ry, Color::WHITE);
        c.write_at("bpp", rx+140, ry, Color::GRAY);
        let _ = ry;
    }
}

// ── TERMINAL tab — scrollbar arrastrable ─────────────────────────────────────
//
// La scrollbar es un rectángulo de SCROLLBAR_W píxeles de ancho en el borde
// derecho del área de historial. El usuario puede:
//   - Usar la rueda del mouse (scroll_delta)
//   - Usar RePag/AvPag en el teclado
//   - Hacer clic y arrastrar el thumb de la scrollbar
//
// CORRECCIÓN: el scroll NUNCA es causado por movimiento en Y del cursor.

fn terminal_hist_geometry(lay: &Layout) -> (usize, usize, usize, usize) {
    let input_h  = 24usize;
    let input_y  = lay.bottom_y.saturating_sub(input_h + 4);
    let hist_top = lay.content_y + 22;
    let hist_h   = input_y.saturating_sub(hist_top + 2);
    let max_lines = hist_h / lay.line_h;
    (hist_top, hist_h, input_y, max_lines)
}

fn draw_terminal_tab(c: &mut Console, lay: &Layout,
                     term: &terminal::Terminal,
                     sb_dragging: bool) {
    let cy  = lay.content_y;
    let ch  = lay.bottom_y.saturating_sub(cy);
    let fw  = lay.fw;
    let pad = lay.pad;

    c.fill_rect(0, cy, fw, ch, Color::TERM_BG);

    // Barra de título del terminal
    c.fill_rect(0, cy, fw, 18, Color::new(2, 8, 18));
    c.hline(0, cy + 17, fw, Color::new(16, 32, 60));
    c.fill_rect(pad, cy + 4, 8, 8, Color::GREEN);
    c.fill_rect(pad + 14, cy + 4, 8, 8, Color::PORTIX_AMBER);
    c.fill_rect(pad + 28, cy + 4, 8, 8, Color::RED);
    c.write_at("PORTIX TERMINAL v0.7", pad + 46, cy + 5, Color::PORTIX_AMBER);
    c.write_at("Rueda/RePag=scroll  ESC=limpiar",
               fw.saturating_sub(280), cy + 5, Color::new(32, 48, 68));

    let (hist_top, hist_h, input_y, max_lines) = terminal_hist_geometry(lay);

    // Gutter izquierdo decorativo
    for y in (hist_top..input_y).step_by(2) {
        c.fill_rect(0, y, 3, 1, Color::new(0, 5, 10));
    }

    // ── Scrollbar ─────────────────────────────────────────────────────────────
    let sb_x = fw.saturating_sub(SCROLLBAR_W);

    if term.line_count > max_lines {
        // Fondo de la barra
        c.fill_rect(sb_x, hist_top, SCROLLBAR_W, hist_h, Color::new(4, 10, 20));

        let max_scroll = term.max_scroll(max_lines);

        // Tamaño proporcional del thumb
        let available = term.line_count
            .saturating_sub(if term.line_count > terminal::TERM_ROWS {
                term.line_count - terminal::TERM_ROWS
            } else { 0 });
        let thumb_h = if available == 0 {
            hist_h
        } else {
            (hist_h * max_lines / available).max(10).min(hist_h)
        };

        // Posición del thumb:
        //   scroll_offset=0       → thumb al fondo
        //   scroll_offset=max     → thumb arriba
        let travel    = hist_h.saturating_sub(thumb_h);
        let thumb_top = if max_scroll == 0 {
            hist_top + travel  // siempre al fondo si no hay scroll disponible
        } else {
            let offset_clamped = term.scroll_offset.min(max_scroll);
            // Mapear offset → posición: 0→abajo, max→arriba
            hist_top + travel - (travel * offset_clamped / max_scroll)
        };

        // Color del thumb: dorado si arrastrando, cyan si en reposo al fondo
        let thumb_col = if sb_dragging { Color::PORTIX_GOLD }
                        else if term.at_bottom() { Color::TEAL }
                        else { Color::PORTIX_AMBER };
        c.fill_rect(sb_x,     thumb_top, 2,           thumb_h, Color::new(8, 20, 40));
        c.fill_rect(sb_x + 2, thumb_top, SCROLLBAR_W - 4, thumb_h, thumb_col);
        c.fill_rect(sb_x + SCROLLBAR_W - 2, thumb_top, 2, thumb_h, Color::new(8, 20, 40));

        // Badge "[↑ SCROLL]" cuando no estamos al fondo
        if !term.at_bottom() {
            let bx = sb_x.saturating_sub(82);
            c.fill_rounded(bx, hist_top + 4, 78, 14, 3, Color::new(20, 40, 0));
            c.write_at("arrib SCROLL", bx + 4, hist_top + 6, Color::PORTIX_GOLD);
        }
    } else {
        // No hay contenido suficiente para scroll: barra atenuada
        c.fill_rect(sb_x, hist_top, SCROLLBAR_W, hist_h, Color::new(2, 6, 12));
    }

    // ── Historial — usando line_at(start + i) para índice lógico correcto ────
    let (start, count) = term.visible_range(max_lines);
    let text_area_w = sb_x.saturating_sub(pad + 4);
    for i in 0..count {
        let line = term.line_at(start + i);
        if line.len == 0 { continue; }
        let ly = hist_top + i * lay.line_h;
        if ly + lay.line_h > input_y { break; }
        let col = match line.color {
            LineColor::Success => Color::NEON_GREEN,
            LineColor::Warning => Color::PORTIX_AMBER,
            LineColor::Error   => Color::RED,
            LineColor::Info    => Color::CYAN,
            LineColor::Prompt  => Color::PORTIX_GOLD,
            LineColor::Header  => Color::WHITE,
            LineColor::Normal  => Color::LIGHT_GRAY,
        };
        let s = core::str::from_utf8(&line.buf[..line.len.min(text_area_w / 9 + 1)])
            .unwrap_or("");
        if line.color == LineColor::Prompt {
            c.fill_rect(0, ly - 1, fw, lay.line_h + 1, Color::new(5, 12, 22));
        }
        c.write_at(s, pad + 4, ly, col);
    }

    // ── Área de input ─────────────────────────────────────────────────────────
    c.fill_rect(0, input_y - 2, fw, 2, Color::new(12, 28, 52));
    c.fill_rect(0, input_y, fw, 24, Color::new(2, 10, 22));

    let prompt = "PORTIX> ";
    c.write_at(prompt, pad, input_y + 8, Color::PORTIX_GOLD);
    let ix = pad + prompt.len() * 9;
    let input_str = core::str::from_utf8(&term.input[..term.input_len]).unwrap_or("");
    c.write_at(input_str, ix, input_y + 8, Color::WHITE);

    // Cursor parpadeante
    let cur_x = ix + term.input_len * 9;
    if term.cursor_vis && cur_x + 7 < sb_x {
        c.fill_rect(cur_x, input_y + 6, 7, 13, Color::PORTIX_GOLD);
    }
}

// ── DEVICES tab ───────────────────────────────────────────────────────────────
fn draw_devices_tab(c: &mut Console, lay: &Layout, hw: &hardware::HardwareInfo,
                    pci: &pci::PciBus) {
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

    // ── Columna 1: CPU + Pantalla ──────────────────────────────────────────
    let c1x = pad;
    let c1w = col_w - pad * 2;
    let mut ry = ry_start;
    section_label(c, c1x, ry, " PROCESADOR", c1w); ry += 20;
    {
        c.write_at("Fabricante:", c1x+4, ry, Color::GRAY);
        c.write_at(hw.cpu.vendor_short(), c1x+100, ry, Color::WHITE); ry += lay.line_h;
        let mut bc=[0u8;16]; let mut bl=[0u8;16];
        c.write_at("Nucleos fis.:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.cpu.physical_cores as u32,&mut bc), c1x+116, ry, Color::WHITE); ry += lay.line_h;
        c.write_at("Hilos log.:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.cpu.logical_cores as u32,&mut bl), c1x+100, ry, Color::WHITE); ry += lay.line_h;
        let mut bf=[0u8;24]; let mut bb=[0u8;24];
        c.write_at("Turbo:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_mhz(hw.cpu.max_mhz,&mut bf), c1x+56, ry, Color::CYAN); ry += lay.line_h;
        if hw.cpu.base_mhz > 0 && hw.cpu.base_mhz != hw.cpu.max_mhz {
            c.write_at("Base:", c1x+4, ry, Color::GRAY);
            c.write_at(fmt_mhz(hw.cpu.base_mhz,&mut bb), c1x+50, ry, Color::LIGHT_GRAY); ry += lay.line_h;
        }
        let mut be=[0u8;18]; let mut be2=[0u8;18];
        c.write_at("CPUID max:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_hex(hw.cpu.max_leaf as u64,&mut be), c1x+90, ry, Color::TEAL); ry += lay.line_h;
        c.write_at("Ext max:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_hex(hw.cpu.max_ext_leaf as u64,&mut be2), c1x+74, ry, Color::TEAL); ry += lay.line_h + 4;
    }
    section_label(c, c1x, ry, " PANTALLA", c1w); ry += 20;
    {
        let mut bw=[0u8;16]; let mut bh=[0u8;16]; let mut bb=[0u8;16]; let mut bp=[0u8;16];
        c.write_at("Resolucion:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.display.width as u32,&mut bw), c1x+100, ry, Color::WHITE);
        c.write_at("x", c1x+132, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.display.height as u32,&mut bh), c1x+142, ry, Color::WHITE); ry += lay.line_h;
        c.write_at("BPP:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.display.bpp as u32,&mut bb), c1x+40, ry, Color::WHITE); ry += lay.line_h;
        c.write_at("Pitch:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.display.pitch as u32,&mut bp), c1x+56, ry, Color::WHITE);
        c.write_at("(DblBuf@0x600000)", c1x+4, ry+lay.line_h, Color::new(28,40,56));
        let _ = ry;
    }

    // ── Columna 2: Almacenamiento + Entrada ───────────────────────────────
    let c2x = col_w;
    let mut c2y = ry_start;
    section_label(c, c2x, c2y, " ALMACENAMIENTO", col_w-8); c2y += 20;
    for i in 0..hw.disks.count.min(4) {
        if c2y + lay.line_h*2 > lay.bottom_y { break; }
        let d = &hw.disks.drives[i];
        c.fill_rounded(c2x+4, c2y-1, 56, 13, 2, Color::new(3,14,30));
        c.write_at(if d.bus==0 { "ATA0" } else { "ATA1" }, c2x+6, c2y, Color::TEAL);
        c.write_at(if d.drive==0 { "-M" } else { "-E" }, c2x+42, c2y, Color::GRAY);
        c.write_at(if d.is_atapi { "ATAPI" } else { "ATA" }, c2x+64, c2y, Color::PORTIX_AMBER);
        c2y += lay.line_h - 2;
        let m = d.model_str(); let m = if m.len()>26 { &m[..26] } else { m };
        c.write_at(m, c2x+8, c2y, Color::WHITE); c2y += lay.line_h - 2;
        if !d.is_atapi {
            let mut sb=[0u8;24];
            c.write_at(fmt_mib(d.size_mb,&mut sb), c2x+8, c2y, Color::PORTIX_GOLD);
            if d.lba48 {
                c.fill_rounded(c2x+100, c2y-1, 46, 12, 2, Color::new(0,28,8));
                c.write_at("LBA48", c2x+104, c2y, Color::GREEN);
            }
        } else { c.write_at("Optico / extraible", c2x+8, c2y, Color::GRAY); }
        c2y += lay.line_h;
    }
    c2y += 4;
    section_label(c, c2x, c2y, " DISPOSITIVOS DE ENTRADA", col_w-8); c2y += 20;
    c.write_at("Teclado PS/2:", c2x+4, c2y, Color::GRAY);
    c.fill_rounded(c2x+116, c2y-2, 50, 13, 3, Color::new(0,30,8));
    c.write_at("● Activo", c2x+120, c2y, Color::NEON_GREEN); c2y += lay.line_h;
    c.write_at("Raton PS/2:", c2x+4, c2y, Color::GRAY);
    c.fill_rounded(c2x+100, c2y-2, 50, 13, 3, Color::new(0,30,8));
    c.write_at("● Activo", c2x+104, c2y, Color::NEON_GREEN); c2y += lay.line_h;
    c.write_at("Rueda scroll:", c2x+4, c2y, Color::GRAY);
    c.fill_rounded(c2x+116, c2y-2, 76, 13, 3, Color::new(0,30,8));
    c.write_at("● IntelliMouse", c2x+120, c2y, Color::NEON_GREEN);

    // ── Columna 3: PCI ─────────────────────────────────────────────────────
    let c3x = col_w * 2;
    let c3w = fw.saturating_sub(c3x + pad);
    let mut c3y = ry_start;
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
        let vhex: [u8; 4] = [H[((d.vendor_id>>12)&0xF) as usize],H[((d.vendor_id>>8)&0xF) as usize],
                              H[((d.vendor_id>>4) &0xF) as usize],H[(d.vendor_id&0xF) as usize]];
        let dhex: [u8; 4] = [H[((d.device_id>>12)&0xF) as usize],H[((d.device_id>>8)&0xF) as usize],
                              H[((d.device_id>>4) &0xF) as usize],H[(d.device_id&0xF) as usize]];
        c.write_at(core::str::from_utf8(&vhex).unwrap_or("????"), c3x+4, c3y, Color::TEAL);
        c.write_at(":", c3x+40, c3y, Color::GRAY);
        c.write_at(core::str::from_utf8(&dhex).unwrap_or("????"), c3x+50, c3y, Color::TEAL);
        let cn = d.class_name(); let cn = if cn.len()>18 { &cn[..18] } else { cn };
        c.write_at(cn, c3x+98, c3y, Color::LIGHT_GRAY);
        c3y += lay.line_h - 1;
    }
}

// ── Pantalla de excepción ─────────────────────────────────────────────────────
fn draw_exception(c: &mut Console, title: &str, info: &str) {
    let w = c.width(); let h = c.height();
    c.fill_rect(0, 0, w, h, Color::new(0, 0, 60));
    c.fill_rect(0, 0, w, 4, Color::RED);
    c.fill_rect(0, h.saturating_sub(4), w, 4, Color::RED);
    let pw=520; let ph=130;
    let px=(w-pw)/2; let py=(h-ph)/2;
    c.fill_rounded(px, py, pw, ph, 6, Color::new(20, 0, 0));
    c.draw_rect(px, py, pw, ph, 1, Color::RED);
    c.write_at("!!! EXCEPCION DE KERNEL !!!", px+pw/2-120, py+12, Color::RED);
    c.hline(px+10, py+30, pw-20, Color::new(80,20,20));
    c.write_at(title, px+14, py+42, Color::WHITE);
    c.write_at(info,  px+14, py+60, Color::LIGHT_GRAY);
    c.write_at("Sistema detenido. Por favor reinicia.", px+14, py+88, Color::GRAY);
    c.present();
}

// ── Main ──────────────────────────────────────────────────────────────────────
#[no_mangle]
extern "C" fn rust_main() -> ! {
    unsafe { idt::init_idt(); }

    serial::init();
    serial::log("PORTIX", "kernel v0.7.2 iniciando");

    pit::init();
    serial::log("PIT", "temporizador 100 Hz inicializado");

    let hw  = hardware::HardwareInfo::detect_all();
    serial::log("HW", hw.cpu.brand_str());

    let pci = pci::PciBus::scan();
    {
        let mut t = [0u8; 16];
        let s = fmt_u32(pci.count as u32, &mut t);
        serial::write_str("PCI: ");
        serial::write_str(s);
        serial::write_str(" dispositivos\n");
    }

    let mut kbd = keyboard::KeyboardState::new();
    let mut ms  = mouse::MouseState::new();
    let mut c   = Console::new();
    let lay     = Layout::new(c.width(), c.height());

    ms.init(lay.fw.max(1), lay.fh.max(1));
    if ms.present {
        let wstr = if ms.has_wheel { "raton PS/2 + rueda (IntelliMouse)" }
                   else            { "raton PS/2 (sin rueda)" };
        serial::log("MOUSE", wstr);
    }

    let mut term = terminal::Terminal::new();
    term.write_line("PORTIX v0.7.2  Kernel Bare-Metal  [Doble Buffer + Scroll Corregido]", LineColor::Header);
    term.write_line("Escribe 'ayuda' para ver comandos. Rueda=scroll. Clic en tabs para cambiar.", LineColor::Info);
    if ms.has_wheel {
        term.write_line("  Rueda de scroll detectada (IntelliMouse).", LineColor::Success);
    } else {
        term.write_line("  Sin rueda. Usa RePag/AvPag para desplazarte.", LineColor::Warning);
    }
    term.write_empty();

    let mut tab = Tab::System;

    // ── Estado del arrastre de scrollbar ─────────────────────────────────────
    let mut sb_dragging:    bool  = false;
    let mut sb_drag_y:      i32   = 0;   // Y donde empezó el drag
    let mut sb_drag_offset: usize = 0;   // scroll_offset al inicio del drag

    // ── Timers ────────────────────────────────────────────────────────────────
    let mut last_blink_tick  = 0u64;
    let mut last_render_tick = 0u64;
    let mut needs_draw    = true;
    let mut needs_present = true;

    let boot_lines: &[(&str, &str, Color)] = &[
        ("  OK  ", "Modo largo (64-bit) activo",             Color::GREEN),
        ("  OK  ", "GDT + TSS cargados",                     Color::GREEN),
        ("  OK  ", "IDT configurada (0-19 + IRQ)",           Color::GREEN),
        ("  OK  ", "PIC remapeado, IRQ0 habilitado",         Color::GREEN),
        ("  OK  ", "PIT @ 100 Hz",                           Color::GREEN),
        ("  OK  ", "Teclado PS/2 inicializado",              Color::GREEN),
        ("  OK  ", "Raton PS/2 inicializado",                Color::GREEN),
        ("  OK  ", "Escaneo de discos ATA completo",         Color::GREEN),
        ("  OK  ", "Framebuffer VESA activo",                Color::GREEN),
        ("  OK  ", "Doble buffer @ 0x600000",                Color::GREEN),
        ("  OK  ", "Bus PCI escaneado",                      Color::GREEN),
        ("  OK  ", "Serial COM1 @ 38400 baud",               Color::GREEN),
    ];

    c.clear(Color::PORTIX_BG);

    loop {
        let now = pit::ticks();

        // ── Teclado (primero, antes del mouse) ────────────────────────────────
        if let Some(key) = kbd.poll() {
            needs_draw = true;
            match key {
                Key::F1  => tab = Tab::System,
                Key::F2  => tab = Tab::Terminal,
                Key::F3  => tab = Tab::Devices,
                Key::Tab => {
                    tab = match tab {
                        Tab::System   => Tab::Terminal,
                        Tab::Terminal => Tab::Devices,
                        Tab::Devices  => Tab::System,
                    };
                }
                Key::PageUp if tab == Tab::Terminal => {
                    let (_, _, _, max_lines) = terminal_hist_geometry(&lay);
                    term.scroll_up(10, max_lines);
                }
                Key::PageDown if tab == Tab::Terminal => {
                    term.scroll_down(10);
                }
                Key::Home if tab == Tab::Terminal => {
                    let (_, _, _, max_lines) = terminal_hist_geometry(&lay);
                    term.scroll_up(usize::MAX / 2, max_lines);
                }
                Key::End if tab == Tab::Terminal => {
                    term.scroll_to_bottom();
                }
                Key::Char(ch) if tab == Tab::Terminal => {
                    term.type_char(ch);
                    serial::write_byte(ch);
                }
                Key::Backspace if tab == Tab::Terminal => term.backspace(),
                Key::Enter if tab == Tab::Terminal => {
                    serial::write_byte(b'\n');
                    term.enter(&hw, &pci);
                }
                Key::Escape => {
                    if tab == Tab::Terminal {
                        term.clear_history();
                        term.clear_input();
                    }
                    sb_dragging = false;
                }
                _ => {}
            }
        }

        // ── Mouse (después del teclado) ───────────────────────────────────────
        let mouse_changed = ms.present && ms.poll();
        if mouse_changed { needs_draw = true; }

        let fw = lay.fw;
        let sb_x = fw.saturating_sub(SCROLLBAR_W) as i32;

        // ── Fin de drag si se soltó el botón ─────────────────────────────────
        if sb_dragging && (ms.left_released() || !ms.left_btn()) {
            sb_dragging = false;
            needs_draw  = true;
        }

        // ── Drag activo de scrollbar ──────────────────────────────────────────
        if sb_dragging && ms.left_btn() && tab == Tab::Terminal {
            let (hist_top, hist_h, _, max_lines) = terminal_hist_geometry(&lay);
            let max_scroll = term.max_scroll(max_lines);

            if max_scroll > 0 {
                let available = term.line_count
                    .saturating_sub(if term.line_count > terminal::TERM_ROWS {
                        term.line_count - terminal::TERM_ROWS
                    } else { 0 });
                let thumb_h = if available == 0 {
                    hist_h
                } else {
                    (hist_h * max_lines / available).max(10).min(hist_h)
                };
                let travel = hist_h.saturating_sub(thumb_h) as i32;

                if travel > 0 {
                    // Mover thumb: dy negativo (arriba) → offset crece (scroll up)
                    let dy = ms.y - sb_drag_y;
                    // Cuando dy < 0 (arriba), offset debe aumentar → -dy * max / travel
                    let new_offset = sb_drag_offset as i32 - (dy * max_scroll as i32) / travel;
                    term.scroll_offset = new_offset.max(0).min(max_scroll as i32) as usize;
                }
            }
            needs_draw = true;
        }

        // ── Clic izquierdo ────────────────────────────────────────────────────
        if mouse_changed && ms.left_clicked() {
            // 1. ¿Clic en la scrollbar? → iniciar drag
            if tab == Tab::Terminal && ms.x >= sb_x {
                sb_dragging    = true;
                sb_drag_y      = ms.y;
                sb_drag_offset = term.scroll_offset;
                needs_draw     = true;
            }
            // 2. ¿Clic en una tab? → cambiar tab (SOLO aquí, no en hover)
            else {
                let hit = lay.tab_hit(ms.x, ms.y);
                match hit {
                    0 => { tab = Tab::System;   needs_draw = true; }
                    1 => { tab = Tab::Terminal; needs_draw = true; }
                    2 => { tab = Tab::Devices;  needs_draw = true; }
                    _ => {}
                }
            }
        }

        // ── Scroll con rueda — SOLO desde scroll_delta, NUNCA desde Y ─────────
        //
        // CORRECCIÓN: este bloque se activa ÚNICAMENTE cuando scroll_delta != 0,
        // que a su vez solo se activa cuando la rueda fue girada (Z == ±1).
        // El movimiento en Y del cursor NO afecta scroll_delta gracias al mouse.rs
        // corregido, por lo que este bloque NUNCA se ejecuta por movimiento Y.
        if mouse_changed && ms.scroll_delta != 0 && tab == Tab::Terminal && !sb_dragging {
            let (_, _, _, max_lines) = terminal_hist_geometry(&lay);
            if ms.scroll_delta > 0 {
                term.scroll_up(terminal::SCROLL_STEP, max_lines);
            } else {
                term.scroll_down(terminal::SCROLL_STEP);
            }
            needs_draw = true;
        }

        // ── Cursor parpadeante @ 500ms ────────────────────────────────────────
        if now.wrapping_sub(last_blink_tick) >= 50 {
            last_blink_tick = now;
            term.cursor_vis = !term.cursor_vis;
            if tab == Tab::Terminal { needs_draw = true; }
        }

        // ── Render ────────────────────────────────────────────────────────────
        if needs_draw {
            draw_chrome(&mut c, &lay, &hw, tab, ms.x, ms.y);
            match tab {
                Tab::System   => draw_system_tab(&mut c, &lay, &hw, boot_lines),
                Tab::Terminal => draw_terminal_tab(&mut c, &lay, &term, sb_dragging),
                Tab::Devices  => draw_devices_tab(&mut c, &lay, &hw, &pci),
            }
            if ms.present { c.draw_cursor(ms.x, ms.y); }
            needs_draw    = false;
            needs_present = true;
        }

        // Blit al LFB limitado a RENDER_HZ para evitar flicker
        if needs_present && now.wrapping_sub(last_render_tick) >= RENDER_INTERVAL {
            c.present();
            last_render_tick = now;
            needs_present    = false;
        }

        unsafe { core::arch::asm!("pause", options(nostack, nomem)); }
    }
}

// ── ISRs ─────────────────────────────────────────────────────────────────────
#[no_mangle] extern "C" fn isr_divide_by_zero() {
    let mut c = Console::new();
    draw_exception(&mut c, "#DE  DIVISION POR CERO", "Division entre cero o desbordamiento DIV/IDIV.");
    halt_loop()
}
#[no_mangle] extern "C" fn isr_bound_range() {
    let mut c = Console::new();
    draw_exception(&mut c, "#BR  RANGO EXCEDIDO", "Indice fuera de rango.");
    halt_loop()
}
#[no_mangle] extern "C" fn isr_ud_handler() {
    let mut c = Console::new();
    draw_exception(&mut c, "#UD  OPCODE INVALIDO", "Se intento ejecutar una instruccion no definida.");
    halt_loop()
}
#[no_mangle] extern "C" fn isr_double_fault() {
    unsafe {
        let v = 0xB8000usize as *mut u16;
        for i in 0..80 { core::ptr::write_volatile(v.add(i), 0x4F20); }
        for (i, &b) in b"#DF DOBLE FALLO -- SISTEMA DETENIDO".iter().enumerate() {
            core::ptr::write_volatile(v.add(i), 0x4F00 | b as u16);
        }
    }
    halt_loop()
}
#[no_mangle] extern "C" fn isr_gp_handler(ec: u64) {
    let mut c = Console::new();
    let w=c.width(); let h=c.height();
    c.fill_rect(0,0,w,h,Color::new(0,0,60));
    c.fill_rect(0,0,w,4,Color::RED);
    c.fill_rect(0,h-4,w,4,Color::RED);
    c.write_at("#GP  FALLO DE PROTECCION GENERAL", 60, 64, Color::WHITE);
    let mut buf=[0u8;18];
    c.write_at("Codigo de error:", 60, 84, Color::GRAY);
    c.write_at(fmt_hex(ec,&mut buf), 200, 84, Color::YELLOW);
    c.present(); halt_loop()
}
#[no_mangle] extern "C" fn isr_page_fault(ec: u64) {
    let cr2: u64;
    unsafe { core::arch::asm!("mov {r}, cr2", r=out(reg) cr2, options(nostack, preserves_flags)); }
    let mut c = Console::new();
    let w=c.width(); let h=c.height();
    c.fill_rect(0,0,w,h,Color::new(0,0,60));
    c.fill_rect(0,0,w,4,Color::RED);
    c.fill_rect(0,h-4,w,4,Color::RED);
    c.write_at("#PF  FALLO DE PAGINA", 60, 64, Color::WHITE);
    let mut ba=[0u8;18]; let mut be=[0u8;18];
    c.write_at("CR2:", 60, 84, Color::GRAY); c.write_at(fmt_hex(cr2,&mut ba), 100, 84, Color::YELLOW);
    c.write_at("Cod:", 60, 104, Color::GRAY); c.write_at(fmt_hex(ec,&mut be), 96, 104, Color::YELLOW);
    c.present(); halt_loop()
}
#[no_mangle] extern "C" fn isr_generic_handler() {
    let mut c = Console::new();
    draw_exception(&mut c, "FALLO DE CPU", "Excepcion de CPU no manejada.");
    halt_loop()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut c = Console::new();
    let w=c.width(); let h=c.height();
    c.fill_rect(0,0,w,h,Color::new(50,0,0));
    c.fill_rect(0,0,w,4,Color::RED);
    c.fill_rect(0,h-4,w,4,Color::RED);
    c.write_at("*** PANIC DE KERNEL ***", w/2-110, 16, Color::RED);
    if let Some(loc) = info.location() {
        c.write_at("Archivo:", 60, 64, Color::GRAY);
        c.write_at(loc.file(), 130, 64, Color::YELLOW);
        let mut lb=[0u8;16];
        c.write_at("Linea:", 60, 84, Color::GRAY);
        c.write_at(fmt_u32(loc.line(),&mut lb), 110, 84, Color::YELLOW);
    }
    c.write_at("Error irrecuperable — sistema detenido.", 60, 120, Color::WHITE);
    c.present(); halt_loop()
}

// ── Stubs de libc ─────────────────────────────────────────────────────────────
#[no_mangle] pub unsafe extern "C" fn memset(s: *mut u8, cv: i32, n: usize) -> *mut u8 {
    for i in 0..n { core::ptr::write_volatile(s.add(i), cv as u8); } s
}
#[no_mangle] pub unsafe extern "C" fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    for i in 0..n { core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i))); } d
}
#[no_mangle] pub unsafe extern "C" fn memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    if (d as usize) <= (s as usize) { memcpy(d, s, n) }
    else { let mut i=n; while i>0 { i-=1; core::ptr::write_volatile(d.add(i),core::ptr::read_volatile(s.add(i))); } d }
}
#[no_mangle] pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n { let d=*a.add(i) as i32 - *b.add(i) as i32; if d!=0 { return d; } } 0
}