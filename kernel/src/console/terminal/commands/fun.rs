// console/terminal/commands/fun.rs
// Comandos: beep, colors, ascii, banner, progress, matrix, scrolltest, motd

use crate::console::terminal::{Terminal, LineColor, TERM_COLS};
use crate::console::terminal::fmt::*;

pub fn cmd_beep(t: &mut Terminal, args: &[u8]) {
    let freq = if args.is_empty() { 440u32 } else {
        match parse_u64(trim(args)) { Some(f) => (f as u32).max(20).min(20000), None => 440 }
    };
    let div = 1_193_182u32 / freq;
    unsafe {
        core::arch::asm!("out 0x43, al", in("al") 0xB6u8, options(nostack, nomem));
        core::arch::asm!("out 0x42, al", in("al") (div & 0xFF) as u8, options(nostack, nomem));
        core::arch::asm!("out 0x42, al", in("al") ((div >> 8) & 0xFF) as u8, options(nostack, nomem));
        let mut p: u8;
        core::arch::asm!("in al, 0x61", out("al") p, options(nostack, nomem));
        p |= 0x03;
        core::arch::asm!("out 0x61, al", in("al") p, options(nostack, nomem));
    }
    // ~200 ms de duración
    let start = crate::time::pit::ticks();
    while crate::time::pit::ticks().wrapping_sub(start) < 20 {
        unsafe { core::arch::asm!("pause", options(nostack, nomem)); }
    }
    unsafe {
        let mut p: u8;
        core::arch::asm!("in al, 0x61", out("al") p, options(nostack, nomem));
        p &= !0x03;
        core::arch::asm!("out 0x61, al", in("al") p, options(nostack, nomem));
    }
    let mut buf = [0u8; 80]; let mut pos = 0;
    append_str(&mut buf, &mut pos, b"  Pitido a ");
    append_u32(&mut buf, &mut pos, freq);
    append_str(&mut buf, &mut pos, b" Hz (200ms)");
    t.write_bytes(&buf[..pos], LineColor::Success);
}

pub fn cmd_colors(t: &mut Terminal) {
    t.write_empty();
    t.separador("PALETA DE COLORES DEL TERMINAL");
    t.write_empty();
    t.write_line("  Normal   -- texto estandar",          LineColor::Normal);
    t.write_line("  EXITO    -- operacion correcta",      LineColor::Success);
    t.write_line("  AVISO    -- atencion no critica",     LineColor::Warning);
    t.write_line("  ERROR    -- algo fallo",              LineColor::Error);
    t.write_line("  INFO     -- informacion del sistema", LineColor::Info);
    t.write_line("  PROMPT   -- linea de comandos",       LineColor::Prompt);
    t.write_line("  CABECERA -- titulo de seccion",       LineColor::Header);
    t.write_empty();
}

pub fn cmd_ascii_art(t: &mut Terminal) {
    t.write_empty();
    t.write_line("    .--------.", LineColor::Header);
    t.write_line("    | PORTIX |   .-------------.", LineColor::Header);
    t.write_line("    |  v0.7  |   |   x86_64    |", LineColor::Header);
    t.write_line("    |        |   | bare-metal   |", LineColor::Header);
    t.write_line("    '--------'   | Doble Buffer |", LineColor::Header);
    t.write_line("        ||       '-------------'", LineColor::Header);
    t.write_line("    [========]   Rust + NASM",     LineColor::Info);
    t.write_line("    |        |   Sin stdlib.",      LineColor::Info);
    t.write_line("    [========]",                    LineColor::Info);
    t.write_empty();
}

pub fn cmd_banner(t: &mut Terminal, args: &[u8]) {
    if args.is_empty() {
        t.write_line("  Uso: banner <texto>  (max 8 chars)", LineColor::Warning); return;
    }
    let s = core::str::from_utf8(args).unwrap_or("?");
    let s = if s.len() > 8 { &s[..8] } else { s };
    t.write_empty();

    let w = (s.len() * 2 + 4).min(40);
    let mut top = [0u8; 80]; let mut tp = 0;
    top[tp] = b'+'; tp += 1;
    for _ in 0..w { if tp < 79 { top[tp] = b'='; tp += 1; } }
    top[tp] = b'+'; tp += 1;
    t.write_bytes(&top[..tp], LineColor::Header);

    let mut mid = [0u8; 80]; let mut mp = 0;
    mid[mp] = b'|'; mp += 1; mid[mp] = b' '; mp += 1;
    for ch in s.chars() {
        let u = if ch.is_ascii_lowercase() { ch as u8 - b'a' + b'A' } else { ch as u8 };
        if mp < 78 { mid[mp] = u; mp += 1; }
        if mp < 78 { mid[mp] = b' '; mp += 1; }
    }
    mid[mp] = b' '; mp += 1; mid[mp] = b'|'; mp += 1;
    t.write_bytes(&mid[..mp], LineColor::Success);

    let mut bot = [0u8; 80]; let mut bp = 0;
    bot[bp] = b'+'; bp += 1;
    for _ in 0..w { if bp < 79 { bot[bp] = b'='; bp += 1; } }
    bot[bp] = b'+'; bp += 1;
    t.write_bytes(&bot[..bp], LineColor::Header);
    t.write_empty();
}

pub fn cmd_progress(t: &mut Terminal) {
    t.write_empty();
    t.write_line("  Cargando componentes de PORTIX:", LineColor::Info);
    for pct in [20u32, 40, 60, 80, 100] {
        let mut bar = [b' '; 52];
        bar[0] = b'[';
        let filled = (pct as usize * 50) / 100;
        for i in 0..filled { bar[1 + i] = b'#'; }
        bar[51] = b']';
        let mut line = [0u8; 80]; let mut lp = 0;
        line[lp] = b' '; lp += 1; line[lp] = b' '; lp += 1;
        for &b in &bar { if lp < 79 { line[lp] = b; lp += 1; } }
        line[lp] = b' '; lp += 1;
        append_u32(&mut line, &mut lp, pct); line[lp] = b'%'; lp += 1;
        t.write_bytes(&line[..lp], if pct == 100 { LineColor::Success } else { LineColor::Info });
    }
    t.write_empty();
}

pub fn cmd_matrix(t: &mut Terminal) {
    t.write_empty();
    t.write_line("  Despierta, Neo...", LineColor::Success);
    let seed   = crate::time::pit::ticks() as u32;
    const CHARS: &[u8] = b"01ABCDEF<>{}[]!?#$@*";
    for row in 0..8u32 {
        let mut line = [b' '; TERM_COLS]; let mut lp = 2;
        for col in 0..60usize {
            let v  = (seed ^ (row * 31337 + col as u32 * 13)).wrapping_mul(0x6B43_9AA7) >> 24;
            let ch = if v < 180 { CHARS[(v as usize) % CHARS.len()] } else { b' ' };
            if lp < TERM_COLS - 1 { line[lp] = ch; lp += 1; }
            if lp < TERM_COLS - 1 { line[lp] = b' '; lp += 1; }
        }
        let col = if row % 3 == 0 { LineColor::Success }
                  else if row % 3 == 1 { LineColor::Info }
                  else { LineColor::Normal };
        t.write_bytes(&line[..lp], col);
    }
    t.write_empty();
    t.write_line("  La Matrix te tiene.", LineColor::Warning);
    t.write_empty();
}

pub fn cmd_scrolltest(t: &mut Terminal) {
    t.separador("PRUEBA DE SCROLL");
    t.write_line("  Generando 50 lineas... usa RePag/AvPag o arrastra la barra.", LineColor::Info);
    t.write_empty();
    for i in 0u32..50 {
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Linea #");
        append_u32(&mut buf, &mut pos, i + 1);
        append_str(&mut buf, &mut pos, b"  -- desplazate con RePag/AvPag o la barra lateral");
        let col = match i % 5 {
            0 => LineColor::Normal,  1 => LineColor::Info,
            2 => LineColor::Success, 3 => LineColor::Warning,
            _ => LineColor::Header,
        };
        t.write_bytes(&buf[..pos], col);
    }
    t.write_empty();
    t.write_line("  [OK] Fin de la prueba.", LineColor::Success);
}
