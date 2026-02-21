// kernel/src/main.rs — PORTIX Kernel v0.6 — full overhaul
// Mouse fix · PIT timer · PCI · ACPI · improved UI
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

// ── Tabs ──────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab { System = 0, Terminal = 1, Devices = 2 }

// ── Format helpers ────────────────────────────────────────────────────────────
fn fmt_u32<'a>(mut n: u32, buf: &'a mut [u8; 16]) -> &'a str {
    if n == 0 { buf[0]=b'0'; return core::str::from_utf8(&buf[..1]).unwrap_or("0"); }
    let mut i=0usize;
    while n>0 && i<16 { buf[i]=b'0'+(n%10) as u8; n/=10; i+=1; }
    buf[..i].reverse();
    core::str::from_utf8(&buf[..i]).unwrap_or("?")
}
fn fmt_u64<'a>(mut n: u64, buf: &'a mut [u8; 20]) -> &'a str {
    if n == 0 { buf[0]=b'0'; return core::str::from_utf8(&buf[..1]).unwrap_or("0"); }
    let mut i=0usize;
    while n>0 && i<20 { buf[i]=b'0'+(n%10) as u8; n/=10; i+=1; }
    buf[..i].reverse();
    core::str::from_utf8(&buf[..i]).unwrap_or("?")
}
fn fmt_hex<'a>(mut v: u64, buf: &'a mut [u8; 18]) -> &'a str {
    buf[0]=b'0'; buf[1]=b'x';
    const H: &[u8]=b"0123456789ABCDEF";
    for i in 0..16 { buf[17-i]=H[(v&0xF) as usize]; v>>=4; }
    core::str::from_utf8(buf).unwrap_or("0x????????????????")
}
fn fmt_mhz<'a>(mhz: u32, buf: &'a mut [u8; 24]) -> &'a str {
    if mhz==0 { buf[..3].copy_from_slice(b"N/A"); return core::str::from_utf8(&buf[..3]).unwrap_or("N/A"); }
    let mut pos=0usize;
    if mhz>=1000 {
        let gi=mhz/1000; let gf=(mhz%1000)/10;
        let mut t=[0u8;16]; let s=fmt_u32(gi,&mut t);
        for b in s.bytes() { if pos<24 { buf[pos]=b; pos+=1; } }
        if pos<24 { buf[pos]=b'.'; pos+=1; }
        if gf<10 && pos<24 { buf[pos]=b'0'; pos+=1; }
        let mut t2=[0u8;16]; let sf=fmt_u32(gf,&mut t2);
        for b in sf.bytes() { if pos<24 { buf[pos]=b; pos+=1; } }
        for b in b" GHz" { if pos<24 { buf[pos]=*b; pos+=1; } }
    } else {
        let mut t=[0u8;16]; let s=fmt_u32(mhz,&mut t);
        for b in s.bytes() { if pos<24 { buf[pos]=b; pos+=1; } }
        for b in b" MHz" { if pos<24 { buf[pos]=*b; pos+=1; } }
    }
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}
fn fmt_mib<'a>(mb: u64, buf: &'a mut [u8; 24]) -> &'a str {
    if mb==0 { buf[0]=b'0'; buf[1]=b'B'; return core::str::from_utf8(&buf[..2]).unwrap_or("0"); }
    let mut pos=0usize;
    if mb>=1024 {
        let gi=mb/1024; let gf=(mb%1024)*10/1024;
        let mut t=[0u8;20]; let s=fmt_u64(gi,&mut t);
        for b in s.bytes() { if pos<24 { buf[pos]=b; pos+=1; } }
        if pos<24 { buf[pos]=b'.'; pos+=1; }
        if pos<24 { buf[pos]=b'0'+(gf as u8); pos+=1; }
        for b in b" GB" { if pos<24 { buf[pos]=*b; pos+=1; } }
    } else {
        let mut t=[0u8;20]; let s=fmt_u64(mb,&mut t);
        for b in s.bytes() { if pos<24 { buf[pos]=b; pos+=1; } }
        for b in b" MB" { if pos<24 { buf[pos]=*b; pos+=1; } }
    }
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}
fn fmt_uptime<'a>(buf: &'a mut [u8; 24]) -> &'a str {
    let (h, m, s) = pit::uptime_hms();
    let mut pos = 0usize;
    macro_rules! push2 { ($n:expr) => {{
        if $n < 10 { buf[pos]=b'0'; pos+=1; }
        let mut t=[0u8;16]; let st=fmt_u32($n,&mut t);
        for b in st.bytes() { if pos<24 { buf[pos]=b; pos+=1; } }
    }}}
    push2!(h); if pos<24{buf[pos]=b':';pos+=1;}
    push2!(m); if pos<24{buf[pos]=b':';pos+=1;}
    push2!(s);
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}

// ── Section label ─────────────────────────────────────────────────────────────
fn section_label(c: &mut Console, x: usize, y: usize, title: &str, w: usize) {
    c.fill_rounded(x, y, w, 14, 2, Color::new(4, 14, 30));
    c.hline(x, y+13, w, Color::SEP_BRIGHT);
    c.write_at(title, x+6, y+3, Color::TEAL);
}

// ── Chrome (header + tabs + status bar) ──────────────────────────────────────
fn draw_chrome(c: &mut Console, lay: &Layout, hw: &hardware::HardwareInfo,
               active: Tab, mx: i32, my: i32) {
    let fw = lay.fw;

    // ── Header ────────────────────────────────────────────────────────────────
    c.fill_rect(0, 0, fw, lay.header_h, Color::HEADER_BG);
    // Gold accent strip on left
    c.fill_rect(0, 0, 6, lay.header_h, Color::PORTIX_GOLD);
    c.fill_rect(6, 0, 2, lay.header_h, Color::new(180, 120, 0));
    // Logo
    c.write_at_tall("PORTIX", 16, lay.header_h/2-8, Color::PORTIX_GOLD);
    c.write_at("v0.6", 16, lay.header_h/2+9, Color::PORTIX_AMBER);

    // CPU brand centred
    let brand = hw.cpu.brand_str();
    let brand = if brand.len()>38 { &brand[..38] } else { brand };
    let bx = fw/2 - (brand.len()*9)/2;
    c.write_at(brand, bx, lay.header_h/2-4, Color::CYAN);

    // Status badge (top-right)
    let bx = fw.saturating_sub(100);
    let by = (lay.header_h-22)/2;
    c.fill_rounded(bx, by, 92, 22, 4, Color::new(0, 40, 10));
    c.draw_rect(bx, by, 92, 22, 1, Color::new(0, 100, 30));
    c.write_at("● BOOT OK", bx+8, by+7, Color::GREEN);

    // Gold separator
    c.fill_rect(0, lay.header_h, fw, lay.gold_h, Color::PORTIX_GOLD);
    c.fill_rect(0, lay.header_h + lay.gold_h - 1, fw, 1, Color::new(180, 110, 0));

    // ── Tab bar ───────────────────────────────────────────────────────────────
    let ty = lay.header_h + lay.gold_h;
    c.fill_rect(0, ty, fw, lay.tab_h + 2, Color::TAB_INACTIVE);

    let tw = 152usize;
    let tab_data: &[(&str, Tab)] = &[
        (" ⎇ F1  SYSTEM   ", Tab::System),
        (" ⌨ F2  TERMINAL ", Tab::Terminal),
        (" ⚙ F3  DEVICES  ", Tab::Devices),
    ];
    for (i, &(label, tab)) in tab_data.iter().enumerate() {
        let tx = i * tw;
        let is_active = tab == active;
        let bg = if is_active { Color::TAB_ACTIVE } else { Color::TAB_INACTIVE };
        if is_active {
            // Gold top indicator
            c.fill_rect(tx, ty, tw-1, 2, Color::PORTIX_GOLD);
            c.fill_rect(tx, ty+2, tw-1, lay.tab_h, bg);
        } else {
            c.fill_rect(tx, ty, tw-1, lay.tab_h+2, bg);
        }
        let fy = ty + 2 + lay.tab_h/2 - 4;
        let fg = if is_active { Color::PORTIX_GOLD } else { Color::GRAY };
        c.write_at(label, tx+4, fy, fg);
        // Thin right divider
        c.fill_rect(tx+tw-1, ty, 1, lay.tab_h+2, Color::SEPARATOR);
    }

    // Hint text on right side of tab bar
    let hx = tab_data.len()*tw + 14;
    let hy = ty + lay.tab_h/2 - 4 + 2;
    if hx + 250 < fw {
        c.write_at("ESC=clear  TAB=cycle  ENTER=run", hx, hy, Color::new(32, 44, 60));
    }

    // ── Status bar ────────────────────────────────────────────────────────────
    let sy_bar = lay.bottom_y;
    c.fill_rect(0, sy_bar, fw, 2, Color::PORTIX_GOLD);
    let bar_h = lay.fh.saturating_sub(sy_bar + 2);
    c.fill_rect(0, sy_bar+2, fw, bar_h, Color::HEADER_BG);
    let sy = sy_bar + 2 + bar_h/2 - 4;

    c.write_at("PORTIX", 12, sy, Color::PORTIX_GOLD);
    c.write_at("v0.6", 66, sy, Color::PORTIX_AMBER);
    c.write_at("│", 102, sy, Color::SEP_BRIGHT);
    c.write_at("x86_64", 112, sy, Color::GRAY);
    c.write_at("│", 160, sy, Color::SEP_BRIGHT);
    c.write_at("●", 170, sy, Color::NEON_GREEN);
    c.write_at("Ready", 183, sy, Color::TEAL);
    c.write_at("│", 228, sy, Color::SEP_BRIGHT);

    // Uptime
    let mut ut=[0u8;24];
    c.write_at("⏱", 238, sy, Color::GRAY);
    c.write_at(fmt_uptime(&mut ut), 252, sy, Color::LIGHT_GRAY);

    // RAM
    let mut mr=[0u8;24];
    c.write_at("RAM:", fw.saturating_sub(130), sy, Color::GRAY);
    c.write_at(fmt_mib(hw.ram.usable_or_default(), &mut mr), fw.saturating_sub(90), sy, Color::PORTIX_GOLD);

    // Mouse XY
    let mut bmx=[0u8;16]; let mut bmy=[0u8;16];
    let mxs = fmt_u32(mx.max(0) as u32, &mut bmx);
    let mys = fmt_u32(my.max(0) as u32, &mut bmy);
    let mox = fw.saturating_sub(250);
    c.write_at(mxs, mox, sy, Color::new(36,50,66));
    c.write_at(",", mox + mxs.len()*9, sy, Color::new(36,50,66));
    c.write_at(mys, mox + mxs.len()*9 + 9, sy, Color::new(36,50,66));
}

// ── SYSTEM tab ────────────────────────────────────────────────────────────────
fn draw_system_tab(c: &mut Console, lay: &Layout, hw: &hardware::HardwareInfo,
                   boot_lines: &[(&str, &str, Color)]) {
    let cy  = lay.content_y;
    let ch  = lay.bottom_y.saturating_sub(cy);
    let fw  = lay.fw;
    let pad = lay.pad;

    c.fill_rect(0, cy, fw, ch, Color::PORTIX_BG);

    // Subtle column divider
    for y in (cy+8..lay.bottom_y-8).step_by(4) {
        c.fill_rect(lay.col_div, y, 1, 2, Color::SEP_BRIGHT);
    }

    // ── Left column: BOOT LOG ─────────────────────────────────────────────────
    let sec_w = lay.col_div - pad - 6;
    section_label(c, pad, cy+6, " ✓ BOOT LOG", sec_w);
    let mut ly = cy + 25;
    for &(tag, msg, col) in boot_lines {
        if ly + lay.line_h > lay.bottom_y.saturating_sub(6) { break; }
        // Status pill
        c.fill_rounded(pad, ly-1, 52, 13, 3, Color::new(0, 35, 10));
        c.write_at(tag, pad+2, ly, col);
        c.write_at(msg, pad+64, ly, Color::LIGHT_GRAY);
        ly += lay.line_h + 3;
    }

    // ── Right column ──────────────────────────────────────────────────────────
    let rx = lay.right_x;
    let rw = fw.saturating_sub(rx + pad);
    let mut ry = cy + 6;

    // CPU card
    section_label(c, rx, ry, " CPU", rw); ry += 20;
    let brand = hw.cpu.brand_str();
    let brand = if brand.len()>34 { &brand[..34] } else { brand };
    c.write_at(brand, rx+6, ry, Color::WHITE); ry += lay.line_h + 2;
    {
        let mut bc=[0u8;16]; let mut bl=[0u8;16]; let mut bf=[0u8;24];
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

    // Feature badges
    {
        macro_rules! badge {
            ($label:expr, $on:expr, $bx:expr) => {{
                let (bg, fg, br) = if $on {
                    (Color::new(0, 30, 10), Color::NEON_GREEN, Color::new(0, 70, 25))
                } else {
                    (Color::new(6, 8, 12), Color::new(40, 48, 56), Color::new(14, 20, 26))
                };
                c.fill_rounded($bx, ry, 42, 14, 3, bg);
                c.draw_rect($bx, ry, 42, 14, 1, br);
                c.write_at($label, $bx+5, ry+3, fg);
            }};
        }
        let fx = rx+6;
        badge!("SSE2", hw.cpu.has_sse2, fx);
        badge!("SSE4", hw.cpu.has_sse4, fx+48);
        badge!("AVX",  hw.cpu.has_avx,  fx+96);
        badge!("AVX2", hw.cpu.has_avx2, fx+144);
        badge!("AES",  hw.cpu.has_aes,  fx+192);
        ry += 22;
    }

    // Memory card
    section_label(c, rx, ry, " MEMORY", rw); ry += 20;
    {
        let usable = hw.ram.usable_or_default();
        let mut bu=[0u8;24];
        c.write_at(fmt_mib(usable, &mut bu), rx+6, ry, Color::WHITE);
        c.write_at("usable RAM", rx+88, ry, Color::GRAY);
        ry += lay.line_h;
        c.gradient_bar(rx+6, ry, rw-16, 8, 100, Color::TEAL, Color::new(3,12,24));
        ry += 12;
        let mut be=[0u8;16];
        c.write_at("E820:", rx+6, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.ram.entry_count as u32, &mut be), rx+50, ry, Color::LIGHT_GRAY);
        c.write_at("entries", rx+50+5*9, ry, Color::GRAY);
        ry += lay.line_h + 4;
    }

    // Storage card
    section_label(c, rx, ry, " STORAGE", rw); ry += 20;
    for i in 0..hw.disks.count.min(3) {
        if ry + lay.line_h > lay.bottom_y.saturating_sub(50) { break; }
        let d = &hw.disks.drives[i];
        c.fill_rounded(rx+6, ry-1, 50, 13, 2, Color::new(4,16,36));
        c.write_at(if d.bus==0{"ATA0"}else{"ATA1"}, rx+8,  ry+1, Color::TEAL);
        c.write_at("-",                              rx+40, ry+1, Color::GRAY);
        c.write_at(if d.drive==0{"M"}else{"S"},      rx+48, ry+1, Color::TEAL);
        c.write_at(if d.is_atapi{"OPT"}else{"HDD"},  rx+64, ry,   Color::PORTIX_AMBER);
        let m = d.model_str(); let m = if m.len()>22{&m[..22]}else{m};
        c.write_at(m, rx+94, ry, Color::WHITE);
        ry += lay.line_h - 1;
        if !d.is_atapi {
            let mut sb=[0u8;24];
            c.write_at(fmt_mib(d.size_mb, &mut sb), rx+20, ry, Color::PORTIX_GOLD);
            if d.lba48 {
                c.fill_rounded(rx+100, ry-1, 46, 12, 2, Color::new(0,30,8));
                c.write_at("LBA48", rx+104, ry, Color::GREEN);
            }
        } else {
            c.write_at("Optical / ATAPI", rx+20, ry, Color::GRAY);
        }
        ry += lay.line_h;
    }

    // Display row (if space)
    if ry + 32 < lay.bottom_y {
        ry += 2;
        section_label(c, rx, ry, " DISPLAY", rw); ry += 20;
        let mut bw=[0u8;16]; let mut bh=[0u8;16]; let mut bb=[0u8;16];
        let ws = fmt_u32(hw.display.width  as u32, &mut bw);
        let hs = fmt_u32(hw.display.height as u32, &mut bh);
        let bs = fmt_u32(hw.display.bpp    as u32, &mut bb);
        c.write_at(ws, rx+6, ry, Color::WHITE);
        c.write_at("×", rx+6+ws.len()*9, ry, Color::GRAY);
        c.write_at(hs, rx+60, ry, Color::WHITE);
        c.write_at("@", rx+108, ry, Color::GRAY);
        c.write_at(bs, rx+122, ry, Color::WHITE);
        c.write_at("bpp", rx+140, ry, Color::GRAY);
    }
}

// ── TERMINAL tab ──────────────────────────────────────────────────────────────
fn draw_terminal_tab(c: &mut Console, lay: &Layout, term: &terminal::Terminal) {
    let cy  = lay.content_y;
    let ch  = lay.bottom_y.saturating_sub(cy);
    let fw  = lay.fw;
    let pad = lay.pad;

    c.fill_rect(0, cy, fw, ch, Color::TERM_BG);

    // Top bar
    c.fill_rect(0, cy, fw, 18, Color::new(2, 8, 18));
    c.hline(0, cy+17, fw, Color::new(16, 32, 60));
    c.fill_rect(pad, cy+4, 8, 8, Color::GREEN);
    c.fill_rect(pad+14, cy+4, 8, 8, Color::PORTIX_AMBER);
    c.fill_rect(pad+28, cy+4, 8, 8, Color::RED);
    c.write_at("PORTIX TERMINAL", pad+46, cy+5, Color::PORTIX_AMBER);
    c.write_at("type 'help' for commands", fw.saturating_sub(210), cy+5, Color::new(32, 48, 68));

    let input_h   = 24;
    let input_y   = lay.bottom_y.saturating_sub(input_h + 4);
    let hist_top  = cy + 22;
    let hist_h    = input_y.saturating_sub(hist_top + 2);
    let max_lines = hist_h / lay.line_h;

    // Subtle scanline effect on left gutter
    for y in (hist_top..input_y).step_by(2) {
        c.fill_rect(0, y, 3, 1, Color::new(0, 5, 10));
    }

    // History
    let (start, count) = term.visible_range(max_lines);
    for i in 0..count {
        let line = &term.lines[(start+i) % terminal::TERM_ROWS];
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
        let s = core::str::from_utf8(&line.buf[..line.len]).unwrap_or("");
        // Row highlight on prompt lines
        if line.color == LineColor::Prompt {
            c.fill_rect(0, ly-1, fw, lay.line_h+1, Color::new(5, 12, 22));
        }
        c.write_at(s, pad + 4, ly, col);
    }

    // Input area
    c.fill_rect(0, input_y - 2, fw, 2, Color::new(12, 28, 52));
    c.fill_rect(0, input_y, fw, input_h, Color::new(2, 10, 22));

    // Prompt
    let prompt = "PORTIX> ";
    c.write_at(prompt, pad, input_y + 8, Color::PORTIX_GOLD);
    let ix = pad + prompt.len() * 9;
    let input_str = core::str::from_utf8(&term.input[..term.input_len]).unwrap_or("");
    c.write_at(input_str, ix, input_y + 8, Color::WHITE);

    // Block cursor
    let cur_x = ix + term.input_len * 9;
    if term.cursor_vis && cur_x + 7 < fw {
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
    c.hline(0, cy+17, fw, Color::SEP_BRIGHT);
    c.write_at(" DEVICES & HARDWARE MAP", pad, cy+5, Color::PORTIX_AMBER);

    let col_w = fw / 3;
    let ry_start = cy + 24;

    // ── Col 1: CPU ────────────────────────────────────────────────────────────
    let c1x = pad;
    let c1w = col_w - pad * 2;
    let mut ry = ry_start;

    section_label(c, c1x, ry, " PROCESSOR", c1w); ry += 20;
    {
        let rows: &[(&str, fn(&hardware::HardwareInfo) -> &str)] = &[];
        let _ = rows;
        c.write_at("Vendor:", c1x+4, ry, Color::GRAY);
        c.write_at(hw.cpu.vendor_short(), c1x+68, ry, Color::WHITE);   ry += lay.line_h;
        let mut bc=[0u8;16]; let mut bl=[0u8;16];
        c.write_at("Phys cores:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.cpu.physical_cores as u32,&mut bc), c1x+100,ry,Color::WHITE); ry+=lay.line_h;
        c.write_at("Log threads:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.cpu.logical_cores  as u32,&mut bl), c1x+108,ry,Color::WHITE); ry+=lay.line_h;
        let mut bf=[0u8;24]; let mut bb=[0u8;24];
        c.write_at("Boost:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_mhz(hw.cpu.max_mhz,&mut bf), c1x+56, ry, Color::CYAN); ry+=lay.line_h;
        if hw.cpu.base_mhz>0 && hw.cpu.base_mhz!=hw.cpu.max_mhz {
            c.write_at("Base:", c1x+4, ry, Color::GRAY);
            c.write_at(fmt_mhz(hw.cpu.base_mhz,&mut bb), c1x+50,ry,Color::LIGHT_GRAY); ry+=lay.line_h;
        }
        let mut be=[0u8;18]; let mut be2=[0u8;18];
        c.write_at("CPUID max:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_hex(hw.cpu.max_leaf as u64,&mut be), c1x+90,ry,Color::TEAL); ry+=lay.line_h;
        c.write_at("Ext max:",   c1x+4, ry, Color::GRAY);
        c.write_at(fmt_hex(hw.cpu.max_ext_leaf as u64,&mut be2),c1x+74,ry,Color::TEAL); ry+=lay.line_h+4;
    }
    section_label(c, c1x, ry, " DISPLAY", c1w); ry += 20;
    {
        let mut bw=[0u8;16]; let mut bh=[0u8;16]; let mut bb=[0u8;16];
        let mut bp=[0u8;16];
        c.write_at("Resolution:", c1x+4, ry, Color::GRAY);
        c.write_at(fmt_u32(hw.display.width  as u32,&mut bw), c1x+100,ry,Color::WHITE);
        c.write_at("×", c1x+132,ry,Color::GRAY);
        c.write_at(fmt_u32(hw.display.height as u32,&mut bh), c1x+142,ry,Color::WHITE); ry+=lay.line_h;
        c.write_at("BPP:",   c1x+4,ry,Color::GRAY);
        c.write_at(fmt_u32(hw.display.bpp   as u32,&mut bb), c1x+40,ry,Color::WHITE);  ry+=lay.line_h;
        c.write_at("Pitch:", c1x+4,ry,Color::GRAY);
        c.write_at(fmt_u32(hw.display.pitch as u32,&mut bp), c1x+56,ry,Color::WHITE);
    }

    // ── Col 2: Storage + Input ────────────────────────────────────────────────
    let c2x = col_w;
    let mut c2y = ry_start;
    section_label(c, c2x, c2y, " STORAGE DEVICES", col_w-8); c2y+=20;
    for i in 0..hw.disks.count.min(4) {
        if c2y + lay.line_h*2 > lay.bottom_y { break; }
        let d = &hw.disks.drives[i];
        c.fill_rounded(c2x+4, c2y-1, 56, 13, 2, Color::new(3,14,30));
        c.write_at(if d.bus==0{"ATA0"}else{"ATA1"}, c2x+6, c2y, Color::TEAL);
        c.write_at(if d.drive==0{"-M"}else{"-S"},   c2x+42,c2y, Color::GRAY);
        c.write_at(if d.is_atapi{"ATAPI"}else{"ATA"},c2x+64,c2y,Color::PORTIX_AMBER);
        c2y += lay.line_h - 2;
        let m=d.model_str(); let m=if m.len()>26{&m[..26]}else{m};
        c.write_at(m, c2x+8, c2y, Color::WHITE); c2y += lay.line_h - 2;
        if !d.is_atapi {
            let mut sb=[0u8;24];
            c.write_at(fmt_mib(d.size_mb,&mut sb), c2x+8, c2y, Color::PORTIX_GOLD);
            if d.lba48 { c.fill_rounded(c2x+100,c2y-1,46,12,2,Color::new(0,28,8));
                         c.write_at("LBA48",c2x+104,c2y,Color::GREEN); }
        } else {
            c.write_at("Optical / removable", c2x+8, c2y, Color::GRAY);
        }
        c2y += lay.line_h;
    }
    c2y += 4;
    section_label(c, c2x, c2y, " INPUT DEVICES", col_w-8); c2y+=20;
    c.write_at("PS/2 Keyboard:", c2x+4, c2y, Color::GRAY);
    c.fill_rounded(c2x+130, c2y-2, 50, 13, 3, Color::new(0,30,8));
    c.write_at("● Active", c2x+134, c2y, Color::NEON_GREEN); c2y+=lay.line_h;
    c.write_at("PS/2 Mouse:",   c2x+4, c2y, Color::GRAY);
    c.fill_rounded(c2x+100, c2y-2, 50, 13, 3, Color::new(0,30,8));
    c.write_at("● Active", c2x+104, c2y, Color::NEON_GREEN);

    // ── Col 3: PCI ────────────────────────────────────────────────────────────
    let c3x = col_w * 2;
    let c3w = fw.saturating_sub(c3x + pad);
    let mut c3y = ry_start;
    {
        let mut tbuf=[0u8;24]; let mut pos=0usize;
        let ts = b" PCI BUS ("; let tl=ts.len();
        tbuf[..tl].copy_from_slice(ts); pos+=tl;
        let mut cnt_buf=[0u8;16];
        let s=fmt_u32(pci.count as u32, &mut cnt_buf);
        for b in s.bytes() { if pos<24{tbuf[pos]=b;pos+=1;} }
        if pos<24{tbuf[pos]=b')';pos+=1;}
        let title = core::str::from_utf8(&tbuf[..pos]).unwrap_or(" PCI BUS");
        section_label(c, c3x, c3y, title, c3w); c3y += 20;
    }
    for i in 0..pci.count.min(14) {
        if c3y + lay.line_h > lay.bottom_y - 6 { break; }
        let d = &pci.devices[i];
        // Vendor:Device in muted style
        {
            const H: &[u8]=b"0123456789ABCDEF";
            let vhex: [u8;4] = [H[((d.vendor_id>>12)&0xF) as usize],
                                  H[((d.vendor_id>>8 )&0xF) as usize],
                                  H[((d.vendor_id>>4 )&0xF) as usize],
                                  H[(d.vendor_id     &0xF) as usize]];
            let dhex: [u8;4] = [H[((d.device_id>>12)&0xF) as usize],
                                  H[((d.device_id>>8 )&0xF) as usize],
                                  H[((d.device_id>>4 )&0xF) as usize],
                                  H[(d.device_id     &0xF) as usize]];
            c.write_at(core::str::from_utf8(&vhex).unwrap_or("????"), c3x+4, c3y, Color::TEAL);
            c.write_at(":",    c3x+40, c3y, Color::GRAY);
            c.write_at(core::str::from_utf8(&dhex).unwrap_or("????"), c3x+50, c3y, Color::TEAL);
        }
        let cn = d.class_name(); let cn = if cn.len()>18{&cn[..18]}else{cn};
        c.write_at(cn, c3x+98, c3y, Color::LIGHT_GRAY);
        c3y += lay.line_h - 1;
    }
}

// ── Exception screen ──────────────────────────────────────────────────────────
fn draw_exception(c: &mut Console, title: &str, info: &str) {
    let w=c.width(); let h=c.height();
    c.fill_rect(0, 0, w, h, Color::new(0, 0, 60));
    c.fill_rect(0, 0, w, 4, Color::RED);
    c.fill_rect(0, h.saturating_sub(4), w, 4, Color::RED);
    // Center panel
    let pw=500; let ph=120;
    let px=(w-pw)/2; let py=(h-ph)/2;
    c.fill_rounded(px, py, pw, ph, 6, Color::new(20, 0, 0));
    c.draw_rect(px, py, pw, ph, 1, Color::RED);
    c.write_at("!!! KERNEL EXCEPTION !!!", px+pw/2-110, py+12, Color::RED);
    c.hline(px+10, py+30, pw-20, Color::new(80, 20, 20));
    c.write_at(title, px+14, py+40, Color::WHITE);
    c.write_at(info,  px+14, py+58, Color::LIGHT_GRAY);
    c.write_at("System halted. Please reboot.", px+14, py+84, Color::GRAY);
}

// ── Main ──────────────────────────────────────────────────────────────────────
#[no_mangle]
extern "C" fn rust_main() -> ! {
    unsafe { idt::init_idt(); }

    // Init serial first for debug output
    serial::init();
    serial::log("PORTIX", "kernel starting");

    // Init PIT (100 Hz timer)
    pit::init();
    serial::log("PIT", "100 Hz timer initialized");

    let hw  = hardware::HardwareInfo::detect_all();
    serial::log("HW", hw.cpu.brand_str());

    // Enumerate PCI
    let pci = pci::PciBus::scan();
    {
        let mut buf=[0u8;16]; let mut i=0usize;
        let n = pci.count as u32;
        let mut t=[0u8;16]; let s=fmt_u32(n,&mut t);
        for b in s.bytes() { if i<16{buf[i]=b;i+=1;} }
        serial::write_str("PCI: ");
        serial::write_bytes_raw(&buf[..i]);
        serial::write_str(" devices found\n");
    }

    let mut kbd  = keyboard::KeyboardState::new();
    let mut ms   = mouse::MouseState::new();
    ms.init(hw.display.width.max(1) as usize, hw.display.height.max(1) as usize);
    if ms.present { serial::log("MOUSE", "PS/2 mouse active"); }

    let mut term = terminal::Terminal::new();
    term.write_line("PORTIX v0.6  Bare-Metal Kernel", LineColor::Header);
    term.write_line("Type 'help' for available commands.", LineColor::Info);
    term.write_empty();

    let mut c   = Console::new();
    let lay     = Layout::new(c.width(), c.height());
    let mut tab = Tab::System;

    // Separate dirty flags
    let mut content_dirty = true;  // UI content changed
    let mut mouse_dirty   = false; // Only mouse moved

    // Previous mouse position (for cursor save/restore)
    let mut prev_mx = ms.x;
    let mut prev_my = ms.y;

    // Cursor blink via PIT ticks
    let mut last_blink_tick = 0u64;

    let boot_lines: &[(&str, &str, Color)] = &[
        ("  OK  ", "Long Mode (64-bit) active",           Color::GREEN),
        ("  OK  ", "GDT + TSS loaded",                    Color::GREEN),
        ("  OK  ", "IDT configured (vectors 0–19 + IRQ)", Color::GREEN),
        ("  OK  ", "PIC remapped, IRQ0 unmasked",         Color::GREEN),
        ("  OK  ", "PIT @ 100 Hz",                        Color::GREEN),
        ("  OK  ", "PS/2 keyboard initialized",            Color::GREEN),
        ("  OK  ", "PS/2 mouse initialized",               Color::GREEN),
        ("  OK  ", "ATA disk scan complete",               Color::GREEN),
        ("  OK  ", "VESA framebuffer active",              Color::GREEN),
        ("  OK  ", "PCI bus scanned",                      Color::GREEN),
        ("  OK  ", "Serial COM1 @ 38400 baud",            Color::GREEN),
        ("  OK  ", "Event loop started",                   Color::GREEN),
    ];

    c.clear(Color::PORTIX_BG);

    loop {
        // ── Keyboard ─────────────────────────────────────────────────────────
        if let Some(key) = kbd.poll() {
            content_dirty = true;
            match key {
                Key::F1 => tab = Tab::System,
                Key::F2 => tab = Tab::Terminal,
                Key::F3 => tab = Tab::Devices,
                Key::Tab => {
                    tab = match tab {
                        Tab::System   => Tab::Terminal,
                        Tab::Terminal => Tab::Devices,
                        Tab::Devices  => Tab::System,
                    };
                }
                Key::Char(ch) if tab == Tab::Terminal => {
                    term.type_char(ch);
                    serial::write_byte(ch);
                }
                Key::Backspace if tab == Tab::Terminal => term.backspace(),
                Key::Enter     if tab == Tab::Terminal => {
                    serial::write_byte(b'\n');
                    term.enter(&hw, &pci);
                }
                Key::Escape    if tab == Tab::Terminal => {
                    term.clear_history();
                    term.clear_input();
                }
                _ => {}
            }
        }

        // ── Mouse ─────────────────────────────────────────────────────────────
        if ms.present && ms.poll() {
            if ms.x != prev_mx || ms.y != prev_my {
                mouse_dirty = true;
            }
        }

        // ── Cursor blink via PIT ──────────────────────────────────────────────
        let now = pit::ticks();
        if now.wrapping_sub(last_blink_tick) >= 50 {  // 50 ticks = 500ms
            last_blink_tick = now;
            term.cursor_vis = !term.cursor_vis;
            if tab == Tab::Terminal { content_dirty = true; }
        }

        // ── Render ───────────────────────────────────────────────────────────
        if content_dirty {
            // Full redraw — invalidate saved cursor bg
            draw_chrome(&mut c, &lay, &hw, tab, ms.x, ms.y);
            match tab {
                Tab::System   => draw_system_tab(&mut c, &lay, &hw, boot_lines),
                Tab::Terminal => draw_terminal_tab(&mut c, &lay, &term),
                Tab::Devices  => draw_devices_tab(&mut c, &lay, &hw, &pci),
            }
            // Draw mouse ONCE, at the very end, saving background
            c.draw_mouse(ms.x, ms.y);
            prev_mx = ms.x; prev_my = ms.y;
            content_dirty = false;
            mouse_dirty   = false;
        } else if mouse_dirty {
            // Only mouse moved: restore old bg, draw cursor at new pos
            c.move_mouse(prev_mx, prev_my, ms.x, ms.y);
            // Update status bar mouse coords without full redraw
            let sy_bar = lay.bottom_y;
            let bar_h  = lay.fh.saturating_sub(sy_bar + 2);
            let sy     = sy_bar + 2 + bar_h / 2 - 4;
            let mox    = lay.fw.saturating_sub(250);
            // Erase old coords
            c.fill_rect(mox, sy - 2, 120, 14, Color::HEADER_BG);
            // Draw new coords
            let mut bmx=[0u8;16]; let mut bmy=[0u8;16];
            let mxs = fmt_u32(ms.x.max(0) as u32, &mut bmx);
            let mys = fmt_u32(ms.y.max(0) as u32, &mut bmy);
            c.write_at(mxs, mox, sy, Color::new(36,50,66));
            c.write_at(",", mox + mxs.len()*9, sy, Color::new(36,50,66));
            c.write_at(mys, mox + mxs.len()*9 + 9, sy, Color::new(36,50,66));

            prev_mx = ms.x; prev_my = ms.y;
            mouse_dirty = false;
        }

        unsafe { core::arch::asm!("pause", options(nostack, nomem)); }
    }
}

// ── ISRs ─────────────────────────────────────────────────────────────────────
#[no_mangle] extern "C" fn isr_divide_by_zero() {
    let mut c=Console::new();
    draw_exception(&mut c,"#DE  DIVIDE BY ZERO","Division by zero or DIV/IDIV overflow.");
    halt_loop()
}
#[no_mangle] extern "C" fn isr_bound_range() {
    let mut c=Console::new();
    draw_exception(&mut c,"#BR  BOUND RANGE EXCEEDED","Index out of bounds (BOUND instruction).");
    halt_loop()
}
#[no_mangle] extern "C" fn isr_ud_handler() {
    let mut c=Console::new();
    draw_exception(&mut c,"#UD  INVALID OPCODE","Attempted to execute an undefined instruction.");
    halt_loop()
}
#[no_mangle] extern "C" fn isr_double_fault() {
    unsafe {
        let v=0xB8000usize as *mut u16;
        for i in 0..80 { core::ptr::write_volatile(v.add(i),0x4F20); }
        for (i,&b) in b"#DF DOUBLE FAULT -- SYSTEM HALTED".iter().enumerate() {
            core::ptr::write_volatile(v.add(i),0x4F00|b as u16);
        }
    }
    halt_loop()
}
#[no_mangle] extern "C" fn isr_gp_handler(ec: u64) {
    let mut c=Console::new(); let w=c.width(); let h=c.height();
    c.fill_rect(0,0,w,h,Color::new(0,0,60));
    c.fill_rect(0,0,w,4,Color::RED); c.fill_rect(0,h-4,w,4,Color::RED);
    c.write_at("#GP  GENERAL PROTECTION FAULT",60,64,Color::WHITE);
    let mut buf=[0u8;18];
    c.write_at("Error code:",60,84,Color::GRAY);
    c.write_at(fmt_hex(ec,&mut buf),168,84,Color::YELLOW);
    halt_loop()
}
#[no_mangle] extern "C" fn isr_page_fault(ec: u64) {
    let cr2:u64;
    unsafe { core::arch::asm!("mov {r},cr2",r=out(reg) cr2,options(nostack,preserves_flags)); }
    let mut c=Console::new(); let w=c.width(); let h=c.height();
    c.fill_rect(0,0,w,h,Color::new(0,0,60));
    c.fill_rect(0,0,w,4,Color::RED); c.fill_rect(0,h-4,w,4,Color::RED);
    c.write_at("#PF  PAGE FAULT",60,64,Color::WHITE);
    let mut ba=[0u8;18]; let mut be=[0u8;18];
    c.write_at("CR2:",60,84,Color::GRAY);   c.write_at(fmt_hex(cr2,&mut ba),100,84,Color::YELLOW);
    c.write_at("Code:",60,104,Color::GRAY); c.write_at(fmt_hex(ec,&mut be),108,104,Color::YELLOW);
    halt_loop()
}
#[no_mangle] extern "C" fn isr_generic_handler() {
    let mut c=Console::new();
    draw_exception(&mut c,"CPU FAULT","Unhandled CPU exception."); halt_loop()
}

// ── Panic ─────────────────────────────────────────────────────────────────────
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut c=Console::new(); let w=c.width(); let h=c.height();
    c.fill_rect(0,0,w,h,Color::new(50,0,0));
    c.fill_rect(0,0,w,4,Color::RED); c.fill_rect(0,h-4,w,4,Color::RED);
    c.write_at("*** KERNEL PANIC ***",w/2-100,16,Color::RED);
    if let Some(loc)=info.location() {
        c.write_at("File:",60,64,Color::GRAY);
        c.write_at(loc.file(),114,64,Color::YELLOW);
        let mut lb=[0u8;16];
        c.write_at("Line:",60,84,Color::GRAY);
        c.write_at(fmt_u32(loc.line(),&mut lb),110,84,Color::YELLOW);
    }
    c.write_at("Unrecoverable error — system halted.",60,120,Color::WHITE);
    halt_loop()
}

// ── libc stubs ────────────────────────────────────────────────────────────────
#[no_mangle] pub unsafe extern "C" fn memset(s:*mut u8,cv:i32,n:usize)->*mut u8 {
    for i in 0..n { core::ptr::write_volatile(s.add(i),cv as u8); } s
}
#[no_mangle] pub unsafe extern "C" fn memcpy(d:*mut u8,s:*const u8,n:usize)->*mut u8 {
    for i in 0..n { core::ptr::write_volatile(d.add(i),core::ptr::read_volatile(s.add(i))); } d
}
#[no_mangle] pub unsafe extern "C" fn memmove(d:*mut u8,s:*const u8,n:usize)->*mut u8 {
    if (d as usize)<=(s as usize) { memcpy(d,s,n) }
    else { let mut i=n; while i>0{i-=1;core::ptr::write_volatile(d.add(i),core::ptr::read_volatile(s.add(i)));} d }
}
#[no_mangle] pub unsafe extern "C" fn memcmp(a:*const u8,b:*const u8,n:usize)->i32 {
    for i in 0..n { let d=*a.add(i) as i32-*b.add(i) as i32; if d!=0{return d;} } 0
}