// console/terminal/commands/debug.rs
// Comandos: hexdump, peek, poke, cpuid, pic, gdt, memtest, inb, outb

use crate::console::terminal::{Terminal, LineColor, TERM_COLS};
use crate::console::terminal::fmt::*;

// ── hexdump ───────────────────────────────────────────────────────────────────

pub fn cmd_hexdump(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() {
        t.write_line("  Uso: hexdump <0xDIR> [bytes]  (predeterminado: 64)", LineColor::Warning);
        return;
    }
    let (addr_part, count_part) = if let Some(sp) = args.iter().position(|&b| b == b' ') {
        (&args[..sp], trim(&args[sp + 1..]))
    } else {
        (args, &b""[..])
    };
    let addr = match parse_hex(addr_part) {
        Some(a) => a,
        None => { t.write_line("  Error: direccion invalida (usa prefijo 0x)", LineColor::Error); return; }
    };
    let count = if count_part.is_empty() { 64 }
                else { match parse_u64(count_part) { Some(n) => n.min(256) as usize, None => 64 } };
    {
        let mut hdr = [0u8; 80]; let mut hp = 0;
        append_str(&mut hdr, &mut hp, b"  Volcado 0x");
        append_hex64_short(&mut hdr, &mut hp, addr);
        append_str(&mut hdr, &mut hp, b" (");
        append_u32(&mut hdr, &mut hp, count as u32);
        append_str(&mut hdr, &mut hp, b" bytes):");
        t.write_bytes(&hdr[..hp], LineColor::Info);
    }
    t.write_line("  Offset    00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  ASCII", LineColor::Header);
    for row in 0..((count + 15) / 16) {
        let base = addr + (row * 16) as u64;
        let mut line = [0u8; TERM_COLS]; let mut lp = 0;
        append_str(&mut line, &mut lp, b"  ");
        append_hex64_short(&mut line, &mut lp, base);
        append_str(&mut line, &mut lp, b"  ");
        let mut ascii_buf = [b'.'; 16];
        for col in 0..16usize {
            let idx = row * 16 + col;
            if col == 8 { append_str(&mut line, &mut lp, b" "); }
            if idx < count {
                let byte = unsafe { core::ptr::read_volatile((base + col as u64) as *const u8) };
                const H: &[u8] = b"0123456789ABCDEF";
                if lp < TERM_COLS - 1 { line[lp] = H[(byte >> 4) as usize]; lp += 1; }
                if lp < TERM_COLS - 1 { line[lp] = H[(byte & 0xF) as usize]; lp += 1; }
                if lp < TERM_COLS - 1 { line[lp] = b' '; lp += 1; }
                ascii_buf[col] = if byte >= 32 && byte < 127 { byte } else { b'.' };
            } else {
                append_str(&mut line, &mut lp, b"   ");
            }
        }
        append_str(&mut line, &mut lp, b" ");
        let acnt = 16.min(count.saturating_sub(row * 16));
        for &ac in &ascii_buf[..acnt] {
            if lp < TERM_COLS - 1 { line[lp] = ac; lp += 1; }
        }
        t.write_bytes(&line[..lp], LineColor::Normal);
    }
}

// ── peek / poke ───────────────────────────────────────────────────────────────

pub fn cmd_peek(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() { t.write_line("  Uso: peek <0xDIR>", LineColor::Warning); return; }
    let addr = match parse_hex(args) {
        Some(a) => a,
        None => { t.write_line("  Error: direccion invalida", LineColor::Error); return; }
    };
    let val = unsafe { core::ptr::read_volatile(addr as *const u64) };
    let mut buf = [0u8; 80]; let mut pos = 0;
    append_str(&mut buf, &mut pos, b"  [0x"); append_hex64_short(&mut buf, &mut pos, addr);
    append_str(&mut buf, &mut pos, b"] = 0x"); append_hex64_full(&mut buf, &mut pos, val);
    append_str(&mut buf, &mut pos, b" ("); append_u32(&mut buf, &mut pos, (val & 0xFFFF_FFFF) as u32);
    append_str(&mut buf, &mut pos, b")");
    t.write_bytes(&buf[..pos], LineColor::Success);
}

pub fn cmd_poke(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    let sp = match args.iter().position(|&b| b == b' ') {
        Some(i) => i,
        None => { t.write_line("  Uso: poke <0xDIR> <valor>", LineColor::Warning); return; }
    };
    let addr = match parse_hex(&args[..sp]) {
        Some(a) => a,
        None => { t.write_line("  Error: direccion invalida", LineColor::Error); return; }
    };
    let val = match parse_u64(trim(&args[sp + 1..])) {
        Some(v) => v as u8,
        None => { t.write_line("  Error: valor invalido", LineColor::Error); return; }
    };
    unsafe { core::ptr::write_volatile(addr as *mut u8, val); }
    let mut buf = [0u8; 80]; let mut pos = 0;
    append_str(&mut buf, &mut pos, b"  Escrito 0x");
    const H: &[u8] = b"0123456789ABCDEF";
    buf[pos] = H[(val >> 4) as usize]; pos += 1;
    buf[pos] = H[(val & 0xF) as usize]; pos += 1;
    append_str(&mut buf, &mut pos, b" en 0x"); append_hex64_short(&mut buf, &mut pos, addr);
    t.write_bytes(&buf[..pos], LineColor::Success);
}

// ── cpuid ─────────────────────────────────────────────────────────────────────

pub fn cmd_cpuid(t: &mut Terminal, args: &[u8]) {
    let leaf = if args.is_empty() { 0 }
               else { match parse_u64(trim(args)) { Some(n) => n as u32, None => 0 } };
    let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
    unsafe {
        core::arch::asm!(
            "push rbx", "cpuid", "mov {ebx_out:e}, ebx", "pop rbx",
            inout("eax") leaf => eax,
            ebx_out = out(reg) ebx,
            out("ecx") ecx,
            out("edx") edx,
            options(nostack, nomem)
        );
    }
    { let mut buf = [0u8; 80]; let mut pos = 0;
      append_str(&mut buf, &mut pos, b"  CPUID hoja 0x");
      append_hex64_short(&mut buf, &mut pos, leaf as u64);
      t.write_bytes(&buf[..pos], LineColor::Info); }

    macro_rules! reg_line {
        ($name:literal, $val:expr) => {{
            let mut b = [0u8; 80]; let mut p = 0;
            append_str(&mut b, &mut p, b"    ");
            append_str(&mut b, &mut p, $name);
            append_str(&mut b, &mut p, b" = 0x");
            append_hex64_full(&mut b, &mut p, $val as u64);
            append_str(&mut b, &mut p, b" (");
            append_u32(&mut b, &mut p, $val);
            append_str(&mut b, &mut p, b")");
            t.write_bytes(&b[..p], LineColor::Normal);
        }}
    }
    reg_line!(b"EAX", eax); reg_line!(b"EBX", ebx);
    reg_line!(b"ECX", ecx); reg_line!(b"EDX", edx);

    // Hoja 0: string del fabricante
    if leaf == 0 {
        let mut vs = [0u8; 12];
        vs[0..4].copy_from_slice(&ebx.to_le_bytes());
        vs[4..8].copy_from_slice(&edx.to_le_bytes());
        vs[8..12].copy_from_slice(&ecx.to_le_bytes());
        if let Ok(s) = core::str::from_utf8(&vs) {
            let mut buf = [0u8; 80]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"    Fabricante: ");
            let sl = s.as_bytes(); let ll = sl.len().min(60);
            buf[pos..pos + ll].copy_from_slice(&sl[..ll]); pos += ll;
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
    }
}

// ── PIC / IRQ ─────────────────────────────────────────────────────────────────

pub fn cmd_pic(t: &mut Terminal) {
    let (mask1, mask2): (u8, u8);
    unsafe {
        core::arch::asm!("in al, 0x21", out("al") mask1, options(nostack, nomem));
        core::arch::asm!("in al, 0xA1", out("al") mask2, options(nostack, nomem));
    }
    t.separador("ESTADO DE INTERRUPCIONES (PIC)");
    t.write_line("  IRQ  Chip  Enmascarado  Nombre", LineColor::Header);

    const NOMBRES: &[&str] = &[
        "Temporizador PIT", "Teclado",         "Cascada (PIC2)",  "COM2",
        "COM1",             "LPT2",             "Floppy",          "LPT1/Espuria",
        "CMOS/RTC",         "Libre",            "Libre",           "Libre",
        "Raton PS/2",       "FPU",              "ATA Primario",    "ATA Secundario",
    ];

    for irq in 0u8..16 {
        let masked = if irq < 8 { mask1 & (1 << irq) != 0 } else { mask2 & (1 << (irq - 8)) != 0 };
        let chip   = if irq < 8 { "PIC1" } else { "PIC2" };
        let nombre = if (irq as usize) < NOMBRES.len() { NOMBRES[irq as usize] } else { "?" };
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"   ");
        if irq < 10 { buf[pos] = b' '; pos += 1; }
        append_u32(&mut buf, &mut pos, irq as u32);
        append_str(&mut buf, &mut pos, b"   ");
        append_str(&mut buf, &mut pos, chip.as_bytes());
        append_str(&mut buf, &mut pos, b"     ");
        append_str(&mut buf, &mut pos, if masked { b"  SI    " } else { b"  NO    " });
        let nl = nombre.as_bytes(); let ll = nl.len().min(30);
        buf[pos..pos + ll].copy_from_slice(&nl[..ll]); pos += ll;
        t.write_bytes(&buf[..pos], if masked { LineColor::Normal } else { LineColor::Success });
    }
    t.write_empty();
}

// ── GDT ──────────────────────────────────────────────────────────────────────

pub fn cmd_gdt(t: &mut Terminal) {
    let mut gdtr = [0u8; 10];
    unsafe { core::arch::asm!("sgdt [{}]", in(reg) gdtr.as_mut_ptr(), options(nostack)); }
    let limit = u16::from_le_bytes([gdtr[0], gdtr[1]]);
    let base  = u64::from_le_bytes([gdtr[2], gdtr[3], gdtr[4], gdtr[5], gdtr[6], gdtr[7], 0, 0]);
    t.separador("TABLA DE DESCRIPTORES GLOBALES (GDT)");
    {
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Base: 0x"); append_hex64_short(&mut buf, &mut pos, base);
        append_str(&mut buf, &mut pos, b"   Limite: "); append_u32(&mut buf, &mut pos, limit as u32);
        t.write_bytes(&buf[..pos], LineColor::Info);
    }
    t.write_line("  Idx  Selector  Base       Limite    Tipo", LineColor::Header);
    let count = ((limit as usize + 1) / 8).min(8);
    for i in 0..count {
        let raw   = unsafe { core::ptr::read_volatile((base + (i * 8) as u64) as *const u64) };
        let bl    = (raw & 0xFFFF) as u32;
        let bm    = ((raw >> 16) & 0xFF) as u32;
        let bh    = ((raw >> 56) & 0xFF) as u32;
        let sb    = bl | (bm << 16) | (bh << 24);
        let sl    = ((raw >> 32) & 0xFFFF) as u32 | (((raw >> 48) & 0xF) as u32) << 16;
        let access = ((raw >> 40) & 0xFF) as u8;
        let sys   = if access & 0x10 != 0 { b"Cod/Dat" as &[u8] } else { b"Sistema" };
        let dpl   = (access >> 5) & 3;
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  "); append_u32(&mut buf, &mut pos, i as u32);
        append_str(&mut buf, &mut pos, b"    0x"); append_hex64_short(&mut buf, &mut pos, (i * 8) as u64);
        append_str(&mut buf, &mut pos, b"   0x"); append_hex64_short(&mut buf, &mut pos, sb as u64);
        append_str(&mut buf, &mut pos, b"   0x"); append_hex64_short(&mut buf, &mut pos, sl as u64);
        append_str(&mut buf, &mut pos, b"  "); buf[pos..pos + sys.len()].copy_from_slice(sys); pos += sys.len();
        append_str(&mut buf, &mut pos, b" DPL"); append_u32(&mut buf, &mut pos, dpl as u32);
        t.write_bytes(&buf[..pos], if i == 0 { LineColor::Normal } else { LineColor::Info });
    }
    t.write_empty();
}

// ── memtest ───────────────────────────────────────────────────────────────────

pub fn cmd_memtest(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    let (addr, size) = if args.is_empty() {
        (0x10_0000u64, 4096usize)
    } else {
        let sp  = args.iter().position(|&b| b == b' ');
        let ap  = if let Some(i) = sp { &args[..i] } else { args };
        let sp2 = if let Some(i) = sp { trim(&args[i + 1..]) } else { &b""[..] };
        (parse_hex(ap).unwrap_or(0x10_0000), parse_u64(sp2).unwrap_or(4096).min(65536) as usize)
    };
    t.separador("PRUEBA DE MEMORIA");
    {
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Direccion: 0x"); append_hex64_short(&mut buf, &mut pos, addr);
        append_str(&mut buf, &mut pos, b"   Tamano: "); append_u32(&mut buf, &mut pos, size as u32);
        append_str(&mut buf, &mut pos, b" bytes  (4 patrones)");
        t.write_bytes(&buf[..pos], LineColor::Info);
    }
    const PATTERNS: &[u8] = &[0xAA, 0x55, 0x00, 0xFF];
    let mut errors = 0u32;
    for &pat in PATTERNS {
        for i in 0..size { unsafe { core::ptr::write_volatile((addr + i as u64) as *mut u8, pat); } }
        for i in 0..size {
            let r = unsafe { core::ptr::read_volatile((addr + i as u64) as *const u8) };
            if r != pat { errors += 1; }
        }
        for i in 0..size { unsafe { core::ptr::write_volatile((addr + i as u64) as *mut u8, 0); } }
    }
    if errors == 0 {
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  [OK] APROBADO: "); append_u32(&mut buf, &mut pos, size as u32);
        append_str(&mut buf, &mut pos, b" bytes sin errores");
        t.write_bytes(&buf[..pos], LineColor::Success);
    } else {
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  [!!] FALLO: "); append_u32(&mut buf, &mut pos, errors);
        append_str(&mut buf, &mut pos, b" errores encontrados");
        t.write_bytes(&buf[..pos], LineColor::Error);
    }
    t.write_empty();
}

// ── inb / outb ────────────────────────────────────────────────────────────────

pub fn cmd_inb(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() { t.write_line("  Uso: inb <0xPUERTO>", LineColor::Warning); return; }
    let port = match parse_hex(args) {
        Some(p) => p as u16,
        None => { t.write_line("  Error: puerto invalido", LineColor::Error); return; }
    };
    let val: u8;
    unsafe { core::arch::asm!("in al, dx", out("al") val, in("dx") port, options(nostack, nomem)); }
    let mut buf = [0u8; 80]; let mut pos = 0;
    append_str(&mut buf, &mut pos, b"  inb(0x"); append_hex64_short(&mut buf, &mut pos, port as u64);
    append_str(&mut buf, &mut pos, b") = 0x");
    const H: &[u8] = b"0123456789ABCDEF";
    buf[pos] = H[(val >> 4) as usize]; pos += 1;
    buf[pos] = H[(val & 0xF) as usize]; pos += 1;
    append_str(&mut buf, &mut pos, b" ("); append_u32(&mut buf, &mut pos, val as u32);
    append_str(&mut buf, &mut pos, b")");
    t.write_bytes(&buf[..pos], LineColor::Success);
}

pub fn cmd_outb(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    let sp = match args.iter().position(|&b| b == b' ') {
        Some(i) => i,
        None => { t.write_line("  Uso: outb <0xPUERTO> <valor>", LineColor::Warning); return; }
    };
    let port = match parse_hex(&args[..sp]) {
        Some(p) => p as u16,
        None => { t.write_line("  Error: puerto invalido", LineColor::Error); return; }
    };
    let val = match parse_u64(trim(&args[sp + 1..])) {
        Some(v) => v as u8,
        None => { t.write_line("  Error: valor invalido", LineColor::Error); return; }
    };
    unsafe { core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nostack, nomem)); }
    let mut buf = [0u8; 80]; let mut pos = 0;
    append_str(&mut buf, &mut pos, b"  outb(0x"); append_hex64_short(&mut buf, &mut pos, port as u64);
    append_str(&mut buf, &mut pos, b", 0x");
    const H: &[u8] = b"0123456789ABCDEF";
    buf[pos] = H[(val >> 4) as usize]; pos += 1;
    buf[pos] = H[(val & 0xF) as usize]; pos += 1;
    append_str(&mut buf, &mut pos, b") completado");
    t.write_bytes(&buf[..pos], LineColor::Success);
}
