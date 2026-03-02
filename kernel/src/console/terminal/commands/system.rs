// console/terminal/commands/system.rs
// Comandos: help, ver, motd, ascii, uname, whoami, hostname,
//           info, cpu, mem, disks, pci, neofetch, uptime, date/fecha

use crate::console::terminal::{Terminal, LineColor, TERM_COLS};
use crate::console::terminal::fmt::*;

// ── help ──────────────────────────────────────────────────────────────────────

pub fn cmd_help(t: &mut Terminal) {
    t.write_empty();
    t.write_line("  +=========================================================+", LineColor::Header);
    t.write_line("  |       PORTIX v0.7  -  Referencia de Comandos            |", LineColor::Header);
    t.write_line("  +=========================================================+", LineColor::Header);
    t.write_empty();

    t.write_line("  INFORMACION DEL SISTEMA:", LineColor::Info);
    t.write_line("    neofetch      Vista general del sistema con logo ASCII",    LineColor::Normal);
    t.write_line("    cpu           Detalles del procesador y extensiones ISA",   LineColor::Normal);
    t.write_line("    mem           Mapa de memoria RAM (E820)",                  LineColor::Normal);
    t.write_line("    disks         Dispositivos de almacenamiento ATA",          LineColor::Normal);
    t.write_line("    pci           Enumeracion del bus PCI",                     LineColor::Normal);
    t.write_line("    uname / ver   Version del sistema operativo",              LineColor::Normal);
    t.write_line("    uptime        Tiempo en linea y ticks del PIT",             LineColor::Normal);
    t.write_line("    date          Fecha/hora desde el arranque",                LineColor::Normal);
    t.write_empty();

    t.write_line("  DISCO ATA:", LineColor::Info);
    t.write_line("    diskinfo                  Listar drives ATA detectados",    LineColor::Normal);
    t.write_line("    diskread [lba] [drive]    Hexdump de sector (sin editar)",  LineColor::Normal);
    t.write_line("    diskedit [lba] [drive]    Editor hexadecimal interactivo",  LineColor::Normal);
    t.write_line("    diskwrite <lba> <0xPAT>   Rellenar sector (QEMU/debug)",    LineColor::Normal);
    t.write_line("    drive: 0=ATA0-M 1=ATA0-S 2=ATA1-M 3=ATA1-S",              LineColor::Normal);
    t.write_empty();

    t.write_line("  HARDWARE Y DEPURACION:", LineColor::Info);
    t.write_line("    hexdump <dir> [bytes]  Volcado hexadecimal de memoria",     LineColor::Normal);
    t.write_line("    peek <dir>             Leer 8 bytes en direccion fisica",   LineColor::Normal);
    t.write_line("    poke <dir> <val>       Escribir byte en direccion fisica",  LineColor::Normal);
    t.write_line("    cpuid [hoja]           Ejecutar instruccion CPUID",         LineColor::Normal);
    t.write_line("    pic                    Estado de mascaras del PIC/IRQ",     LineColor::Normal);
    t.write_line("    gdt                    Volcado de la tabla GDT",            LineColor::Normal);
    t.write_line("    memtest [dir] [tam]    Prueba de lectura/escritura de RAM", LineColor::Normal);
    t.write_line("    inb <puerto>           Leer byte de puerto de E/S",         LineColor::Normal);
    t.write_line("    outb <puerto> <val>    Escribir byte en puerto de E/S",     LineColor::Normal);
    t.write_empty();

    t.write_line("  CALCULO Y CONVERSION:", LineColor::Info);
    t.write_line("    calc / = <expr>   Aritmetica: + - * /",                    LineColor::Normal);
    t.write_line("    hex <decimal>     Decimal a hexadecimal",                  LineColor::Normal);
    t.write_line("    dec <0xHEX>       Hexadecimal a decimal",                  LineColor::Normal);
    t.write_line("    bin <decimal>     Decimal a binario",                      LineColor::Normal);
    t.write_line("    rgb <r> <g> <b>   Componentes RGB a 0xRRGGBB",             LineColor::Normal);
    t.write_empty();

    t.write_line("  TERMINAL:", LineColor::Info);
    t.write_line("    echo <texto>   Imprimir texto en pantalla",                 LineColor::Normal);
    t.write_line("    history        Historial de comandos (ultimos 16)",         LineColor::Normal);
    t.write_line("    clear          Limpiar la pantalla del terminal",           LineColor::Normal);
    t.write_line("    scrolltest     Generar 50 lineas para probar scroll",       LineColor::Normal);
    t.write_empty();

    t.write_line("  NAVEGACION:", LineColor::Info);
    t.write_line("    RePag / AvPag  Desplazarse 10 lineas arriba/abajo",        LineColor::Normal);
    t.write_line("    Inicio / Fin   Saltar al principio / final",               LineColor::Normal);
    t.write_line("    Mouse          Arrastrar la barra lateral para navegar",   LineColor::Normal);
    t.write_empty();

    t.write_line("  AUDIO Y EFECTOS:", LineColor::Info);
    t.write_line("    beep [hz]     Pitido por el altavoz interno del PC",        LineColor::Normal);
    t.write_line("    matrix        Animacion ASCII estilo Matrix",               LineColor::Normal);
    t.write_line("    colors        Demostracion de paleta de colores",           LineColor::Normal);
    t.write_line("    ascii         Logo ASCII de PORTIX",                        LineColor::Normal);
    t.write_line("    banner <txt>  Mostrar texto en formato de pancarta",        LineColor::Normal);
    t.write_empty();

    t.write_line("  ENERGIA:", LineColor::Warning);
    t.write_line("    reboot        Reiniciar el sistema",                        LineColor::Normal);
    t.write_line("    poweroff      Apagar el sistema (ACPI S5)",                 LineColor::Normal);
    t.write_empty();
}

// ── ver / uname / whoami / hostname ──────────────────────────────────────────

pub fn cmd_ver(t: &mut Terminal) {
    t.separador("VERSION DEL SISTEMA");
    t.write_line("  PORTIX Kernel v0.7  -  x86_64 bare-metal",                   LineColor::Success);
    t.write_line("  Compilacion: 2026 / Rust nightly (no_std) + NASM",           LineColor::Normal);
    t.write_line("  Subsistemas: PIT  Teclado PS/2  Raton PS/2  ATA  VESA  DblBuf", LineColor::Info);
    t.write_line("               PCI  ACPI  Serial COM1  E820  IDT",             LineColor::Info);
    t.write_empty();
}

pub fn cmd_motd(t: &mut Terminal) {
    t.write_empty();
    t.write_line("   ██████╗  ██████╗ ██████╗ ████████╗██╗██╗  ██╗", LineColor::Header);
    t.write_line("   ██╔══██╗██╔═══██╗██╔══██╗╚══██╔══╝██║╚██╗██╔╝", LineColor::Header);
    t.write_line("   ██████╔╝██║   ██║██████╔╝   ██║   ██║ ╚███╔╝ ", LineColor::Header);
    t.write_line("   ██╔═══╝ ██║   ██║██╔══██╗   ██║   ██║ ██╔██╗ ", LineColor::Header);
    t.write_line("   ██║     ╚██████╔╝██║  ██║   ██║   ██║██╔╝ ██╗", LineColor::Header);
    t.write_line("   ╚═╝      ╚═════╝ ╚═╝  ╚═╝   ╚═╝   ╚═╝╚═╝  ╚═╝", LineColor::Header);
    t.write_empty();
    t.write_line("   Sistema Operativo Bare-Metal | x86_64 | Rust + NASM | Doble Buffer", LineColor::Info);
    t.write_line("   Sin stdlib. Sin runtime. Sin piedad. Solo metal.",           LineColor::Normal);
    t.write_empty();
}

// ── Tiempo ────────────────────────────────────────────────────────────────────

pub fn cmd_uptime(t: &mut Terminal) {
    let (h, m, s) = crate::time::pit::uptime_hms();
    let tick = crate::time::pit::ticks();
    let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
    append_str(&mut buf, &mut pos, b"  Tiempo en linea: ");
    append_u32(&mut buf, &mut pos, h); append_str(&mut buf, &mut pos, b"h ");
    append_u32(&mut buf, &mut pos, m); append_str(&mut buf, &mut pos, b"m ");
    append_u32(&mut buf, &mut pos, s); append_str(&mut buf, &mut pos, b"s");
    append_str(&mut buf, &mut pos, b"   |  Ticks: ");
    append_u32(&mut buf, &mut pos, (tick & 0xFFFF_FFFF) as u32);
    append_str(&mut buf, &mut pos, b" @ 100 Hz");
    t.write_bytes(&buf[..pos], LineColor::Success);
}

pub fn cmd_fecha(t: &mut Terminal) {
    let (h, m, s) = crate::time::pit::uptime_hms();
    let tick = crate::time::pit::ticks();
    let mut buf = [0u8; 80]; let mut pos = 0;
    append_str(&mut buf, &mut pos, b"  Tiempo desde arranque: ");
    append_u32(&mut buf, &mut pos, h); append_str(&mut buf, &mut pos, b"h ");
    if m < 10 { append_str(&mut buf, &mut pos, b"0"); }
    append_u32(&mut buf, &mut pos, m); append_str(&mut buf, &mut pos, b"m ");
    if s < 10 { append_str(&mut buf, &mut pos, b"0"); }
    append_u32(&mut buf, &mut pos, s); append_str(&mut buf, &mut pos, b"s");
    append_str(&mut buf, &mut pos, b"  (tick #");
    append_u32(&mut buf, &mut pos, (tick & 0xFFFF_FFFF) as u32);
    append_str(&mut buf, &mut pos, b")");
    t.write_bytes(&buf[..pos], LineColor::Info);
    t.write_line("  Nota: sin driver RTC -- tiempo mostrado es desde el arranque.", LineColor::Normal);
}

pub fn cmd_ticks(t: &mut Terminal) {
    let tick = crate::time::pit::ticks();
    let mut buf = [0u8; 80]; let mut pos = 0;
    append_str(&mut buf, &mut pos, b"  Ticks del PIT: ");
    append_u32(&mut buf, &mut pos, (tick & 0xFFFF_FFFF) as u32);
    append_str(&mut buf, &mut pos, b"  (alto 32 bits: ");
    append_u32(&mut buf, &mut pos, (tick >> 32) as u32);
    append_str(&mut buf, &mut pos, b")");
    t.write_bytes(&buf[..pos], LineColor::Info);
}

// ── Historial ─────────────────────────────────────────────────────────────────

pub fn cmd_history(t: &mut Terminal) {
    if t.hist_count == 0 {
        t.write_line("  (sin historial de comandos)", LineColor::Normal); return;
    }
    t.separador("HISTORIAL DE COMANDOS");
    let total = t.hist_count.min(16);
    let start = if t.hist_count > 16 { t.hist_count - 16 } else { 0 };
    for i in 0..total {
        let slot = (start + i) % 16;
        let len  = t.hist_lens[slot]; if len == 0 { continue; }
        let mut line = [0u8; 80]; let mut lp = 0;
        append_str(&mut line, &mut lp, b"  ");
        append_u32(&mut line, &mut lp, (start + i + 1) as u32);
        append_str(&mut line, &mut lp, b"  ");
        let l = len.min(70);
        line[lp..lp + l].copy_from_slice(&t.hist_cmds[slot][..l]); lp += l;
        t.write_bytes(&line[..lp], LineColor::Normal);
    }
    t.write_empty();
}

// ── Hardware: cpu, mem, disks, pci ───────────────────────────────────────────

pub fn cmd_info(t: &mut Terminal, hw: &crate::arch::hardware::HardwareInfo) {
    cmd_cpu(t, hw); t.write_empty(); cmd_mem(t, hw);
}

pub fn cmd_cpu(t: &mut Terminal, hw: &crate::arch::hardware::HardwareInfo) {
    t.separador("PROCESADOR (CPU)");
    {
        let mut lb = [0u8; TERM_COLS]; let bl = b"  Modelo     : ";
        lb[..bl.len()].copy_from_slice(bl);
        let n  = hw.cpu.brand_str().as_bytes();
        let nl = n.len().min(TERM_COLS - bl.len());
        lb[bl.len()..bl.len() + nl].copy_from_slice(&n[..nl]);
        t.write_bytes(&lb[..bl.len() + nl], LineColor::Normal);
    }
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Nucleos    : ");
        append_u32(&mut buf, &mut pos, hw.cpu.physical_cores as u32);
        append_str(&mut buf, &mut pos, b" fisicos / ");
        append_u32(&mut buf, &mut pos, hw.cpu.logical_cores as u32);
        append_str(&mut buf, &mut pos, b" logicos");
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Frecuencia : max ");
        append_mhz(&mut buf, &mut pos, hw.cpu.max_mhz);
        if hw.cpu.base_mhz > 0 && hw.cpu.base_mhz != hw.cpu.max_mhz {
            append_str(&mut buf, &mut pos, b"  base ");
            append_mhz(&mut buf, &mut pos, hw.cpu.base_mhz);
        }
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Extensiones:");
        if hw.cpu.has_sse2 { append_str(&mut buf, &mut pos, b" SSE2"); }
        if hw.cpu.has_sse4 { append_str(&mut buf, &mut pos, b" SSE4"); }
        if hw.cpu.has_avx  { append_str(&mut buf, &mut pos, b" AVX");  }
        if hw.cpu.has_avx2 { append_str(&mut buf, &mut pos, b" AVX2"); }
        if hw.cpu.has_aes  { append_str(&mut buf, &mut pos, b" AES");  }
        t.write_bytes(&buf[..pos], LineColor::Success);
    }
    t.write_empty();
}

pub fn cmd_mem(t: &mut Terminal, hw: &crate::arch::hardware::HardwareInfo) {
    t.separador("MEMORIA RAM (E820)");
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Utilizable : ");
        append_mib(&mut buf, &mut pos, hw.ram.usable_or_default());
        append_str(&mut buf, &mut pos, b"   Entradas E820: ");
        append_u32(&mut buf, &mut pos, hw.ram.entry_count as u32);
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    t.write_empty();
    t.write_line("  #  Base                   Longitud      Tipo", LineColor::Info);
    t.write_line("  -  --------------------   -----------   ----------", LineColor::Normal);
    unsafe {
        for i in 0..(hw.ram.entry_count.min(16) as usize) {
            let p    = (0x9102usize + i * 20) as *const u8;
            let base = core::ptr::read_unaligned(p           as *const u64);
            let len  = core::ptr::read_unaligned(p.add(8)    as *const u64);
            let kind = core::ptr::read_unaligned(p.add(16)   as *const u32);
            let ts: &[u8] = match kind {
                1 => b"Utilizable", 2 => b"Reservada",
                3 => b"ACPI Reclam", 4 => b"ACPI NVS",
                5 => b"RAM Mala",    _ => b"Desconocido",
            };
            let mut eb = [0u8; TERM_COLS]; let mut ep = 0;
            append_str(&mut eb, &mut ep, b"  ");
            append_u32(&mut eb, &mut ep, i as u32);
            append_str(&mut eb, &mut ep, b"  0x");
            append_hex64_full(&mut eb, &mut ep, base);
            append_str(&mut eb, &mut ep, b"  ");
            append_mib(&mut eb, &mut ep, len / (1024 * 1024));
            append_str(&mut eb, &mut ep, b"   ");
            eb[ep..ep + ts.len()].copy_from_slice(ts); ep += ts.len();
            t.write_bytes(&eb[..ep], if kind == 1 { LineColor::Success } else { LineColor::Normal });
        }
    }
    t.write_empty();
}

pub fn cmd_disks(t: &mut Terminal, hw: &crate::arch::hardware::HardwareInfo) {
    t.separador("ALMACENAMIENTO (ATA)");
    if hw.disks.count == 0 {
        t.write_line("  No se detectaron unidades ATA.", LineColor::Warning);
        t.write_empty(); return;
    }
    for i in 0..hw.disks.count {
        let d   = &hw.disks.drives[i];
        let bus = if d.bus   == 0 { b"ATA0" as &[u8] } else { b"ATA1" };
        let drv = if d.drive == 0 { b"Maestro" as &[u8] } else { b"Esclavo" };
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  [");
        buf[pos..pos + bus.len()].copy_from_slice(bus); pos += bus.len();
        append_str(&mut buf, &mut pos, b"-");
        buf[pos..pos + drv.len()].copy_from_slice(drv); pos += drv.len();
        append_str(&mut buf, &mut pos, if d.is_atapi { b"]  OPTICO  " } else { b"]  HDD    " });
        let m = d.model_str().as_bytes(); let ml = m.len().min(28);
        buf[pos..pos + ml].copy_from_slice(&m[..ml]); pos += ml;
        if !d.is_atapi {
            append_str(&mut buf, &mut pos, b"  ");
            append_mib(&mut buf, &mut pos, d.size_mb);
            if d.lba48 { append_str(&mut buf, &mut pos, b"  [LBA48]"); }
        }
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    t.write_empty();
}

pub fn cmd_pci(t: &mut Terminal, pci: &crate::drivers::bus::pci::PciBus) {
    t.separador("BUS PCI");
    if pci.count == 0 {
        t.write_line("  No se encontraron dispositivos PCI.", LineColor::Warning);
        t.write_empty(); return;
    }
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Se encontraron ");
        append_u32(&mut buf, &mut pos, pci.count as u32);
        append_str(&mut buf, &mut pos, b" dispositivo(s):");
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    t.write_empty();
    t.write_line("  [B:D.F]  VendorID:DevID  Fabricante           Clase", LineColor::Info);
    t.write_line("  -------  --------------  -------------------  ---------", LineColor::Normal);
    for i in 0..pci.count.min(32) {
        let d = &pci.devices[i];
        let mut lb = [0u8; TERM_COLS]; let mut lp = 0;
        append_str(&mut lb, &mut lp, b"  [");
        append_u32(&mut lb, &mut lp, d.bus as u32);
        append_str(&mut lb, &mut lp, b":");
        append_hex8_byte(&mut lb, &mut lp, d.device);
        append_str(&mut lb, &mut lp, b".");
        append_u32(&mut lb, &mut lp, d.function as u32);
        append_str(&mut lb, &mut lp, b"]  ");
        append_hex16(&mut lb, &mut lp, d.vendor_id);
        append_str(&mut lb, &mut lp, b":");
        append_hex16(&mut lb, &mut lp, d.device_id);
        append_str(&mut lb, &mut lp, b"  ");
        let vn = d.vendor_name().as_bytes();
        lb[lp..lp + vn.len()].copy_from_slice(vn); lp += vn.len();
        while lp < 56 { lb[lp] = b' '; lp += 1; }
        let cn = d.class_name().as_bytes(); let cl = cn.len().min(20);
        lb[lp..lp + cl].copy_from_slice(&cn[..cl]); lp += cl;
        t.write_bytes(&lb[..lp], LineColor::Info);
    }
    t.write_empty();
}

// ── neofetch ──────────────────────────────────────────────────────────────────

pub fn cmd_neofetch(
    t:   &mut Terminal,
    hw:  &crate::arch::hardware::HardwareInfo,
    pci: &crate::drivers::bus::pci::PciBus,
) {
    t.write_empty();
    let brand  = hw.cpu.brand_str();
    let brand  = if brand.len() > 36 { &brand[..36] } else { brand };
    let usable = hw.ram.usable_or_default();

    const LOGO: &[&str] = &[
        "     ____   ___  ____  _____ _____  __  __",
        "    |  _ \\ / _ \\|  _ \\|_   _|_   _| \\ \\/ /",
        "    | |_) | | | | |_) | | |   | |    \\  / ",
        "    |  __/| |_| |  _ <  | |   | |    /  \\ ",
        "    |_|    \\___/|_| \\_\\ |_|   |_|   /_/\\_\\",
        "                                            ",
    ];

    let mut il:  [[u8; 80]; 14] = [[0u8; 80]; 14];
    let mut ils: [usize; 14]    = [0; 14];
    let mut n = 0usize;

    macro_rules! iline {
        ($k:literal, $v:expr) => {{
            let mut buf = [0u8; 80]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  ");
            append_str(&mut buf, &mut pos, $k);
            append_str(&mut buf, &mut pos, b": ");
            let vb = $v.as_bytes(); let vl = vb.len().min(50);
            buf[pos..pos + vl].copy_from_slice(&vb[..vl]); pos += vl;
            il[n] = buf; ils[n] = pos; n += 1;
        }}
    }

    iline!(b"SO       ", "PORTIX v0.7 bare-metal");
    iline!(b"Arq      ", "x86_64");
    iline!(b"Kernel   ", "Rust nightly (no_std) + NASM");
    iline!(b"Video    ", "VESA LFB (doble buffer @ 0x600000)");

    { let mut buf = [0u8; 80]; let mut pos = 0;
      append_str(&mut buf, &mut pos, b"  CPU     : ");
      let bb = brand.as_bytes(); let bl = bb.len().min(50);
      buf[pos..pos + bl].copy_from_slice(&bb[..bl]); pos += bl;
      il[n] = buf; ils[n] = pos; n += 1; }

    { let mut buf = [0u8; 80]; let mut pos = 0;
      append_str(&mut buf, &mut pos, b"  Nucleos : ");
      append_u32(&mut buf, &mut pos, hw.cpu.physical_cores as u32);
      append_str(&mut buf, &mut pos, b"C / ");
      append_u32(&mut buf, &mut pos, hw.cpu.logical_cores as u32);
      append_str(&mut buf, &mut pos, b"T  @");
      append_mhz(&mut buf, &mut pos, hw.cpu.max_mhz);
      il[n] = buf; ils[n] = pos; n += 1; }

    { let mut buf = [0u8; 80]; let mut pos = 0;
      append_str(&mut buf, &mut pos, b"  RAM     : ");
      append_mib(&mut buf, &mut pos, usable);
      il[n] = buf; ils[n] = pos; n += 1; }

    { let mut buf = [0u8; 80]; let mut pos = 0;
      append_str(&mut buf, &mut pos, b"  Pantalla: ");
      append_u32(&mut buf, &mut pos, hw.display.width  as u32);
      append_str(&mut buf, &mut pos, b"x");
      append_u32(&mut buf, &mut pos, hw.display.height as u32);
      append_str(&mut buf, &mut pos, b" @ ");
      append_u32(&mut buf, &mut pos, hw.display.bpp    as u32);
      append_str(&mut buf, &mut pos, b"bpp");
      il[n] = buf; ils[n] = pos; n += 1; }

    { let mut buf = [0u8; 80]; let mut pos = 0;
      append_str(&mut buf, &mut pos, b"  PCI     : ");
      append_u32(&mut buf, &mut pos, pci.count as u32);
      append_str(&mut buf, &mut pos, b" disp.  Discos: ");
      append_u32(&mut buf, &mut pos, hw.disks.count as u32);
      il[n] = buf; ils[n] = pos; n += 1; }

    { let (h, m, s) = crate::time::pit::uptime_hms();
      let mut buf = [0u8; 80]; let mut pos = 0;
      append_str(&mut buf, &mut pos, b"  Uptime  : ");
      append_u32(&mut buf, &mut pos, h); append_str(&mut buf, &mut pos, b"h ");
      append_u32(&mut buf, &mut pos, m); append_str(&mut buf, &mut pos, b"m ");
      append_u32(&mut buf, &mut pos, s); append_str(&mut buf, &mut pos, b"s");
      il[n] = buf; ils[n] = pos; n += 1; }

    let rows = LOGO.len().max(n);
    for row in 0..rows {
        let mut combined = [0u8; TERM_COLS];
        if row < LOGO.len() {
            let lb = LOGO[row].as_bytes(); let ll = lb.len().min(44);
            combined[..ll].copy_from_slice(&lb[..ll]);
        }
        let mut cp = 46;
        if row < n {
            let l = ils[row].min(TERM_COLS.saturating_sub(cp));
            combined[cp..cp + l].copy_from_slice(&il[row][..l]);
            cp += l;
        }
        let col = if row < LOGO.len() { LineColor::Header } else { LineColor::Normal };
        if cp > 0 { t.write_bytes(&combined[..cp], col); }
    }
    t.write_empty();
}