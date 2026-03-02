// console/terminal/commands/disk.rs — PORTIX Kernel v0.7.4
//
// Comandos de gestión de disco ATA:
//   diskedit [lba] [drive]   — editor hexadecimal interactivo de un sector
//   diskread [lba] [drive]   — hexdump de un sector (solo lectura)
//   diskinfo                 — lista drives ATA detectados
//   diskwrite <lba> <0xPAT>  — rellenar sector con patrón (solo QEMU/debug)
//
// "drive": 0=ATA0-Master  1=ATA0-Slave  2=ATA1-Master  3=ATA1-Slave

#![allow(dead_code)]

use crate::console::terminal::{Terminal, LineColor, TERM_COLS};
use crate::console::terminal::fmt::*;
use crate::console::terminal::editor::EditorState;
use crate::drivers::storage::ata::{AtaBus, AtaError, DriveId, DriveType};

// ── Helpers privados ──────────────────────────────────────────────────────────

fn drive_id(idx: usize) -> DriveId {
    match idx {
        1 => DriveId::Primary1,
        2 => DriveId::Secondary0,
        3 => DriveId::Secondary1,
        _ => DriveId::Primary0,
    }
}

/// Parsea "[lba] [drive]" de los args; ambos opcionales.
fn parse_lba_drive(args: &[u8]) -> (u64, usize) {
    let a   = trim(args);
    let sp  = a.iter().position(|&b| b == b' ');
    let lba = if a.is_empty() { 0 } else {
        let part = if let Some(i) = sp { &a[..i] } else { a };
        parse_u64(part).unwrap_or(0)
    };
    let drv = if let Some(i) = sp {
        parse_u64(trim(&a[i + 1..])).unwrap_or(0) as usize
    } else { 0 };
    (lba, drv.min(3))
}

fn ata_err_str(e: AtaError) -> &'static [u8] {
    match e {
        AtaError::Timeout        => b"timeout",
        AtaError::DriveFault     => b"fallo de drive",
        AtaError::OutOfRange     => b"fuera de rango",
        AtaError::DeviceError(_) => b"error de dispositivo",
        AtaError::BadBuffer      => b"buffer incorrecto",
        AtaError::NoDrive        => b"no hay drive",
    }
}

// ── diskedit ──────────────────────────────────────────────────────────────────

/// Abre el editor hexadecimal sobre el sector indicado.
pub fn cmd_diskedit(t: &mut Terminal, args: &[u8]) {
    let (lba, drv_idx) = parse_lba_drive(args);
    let id             = drive_id(drv_idx);

    let bus  = AtaBus::scan();
    let info = match bus.info(id) {
        Some(i) => *i,
        None => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Error: drive ");
            append_u32(&mut buf, &mut pos, drv_idx as u32);
            append_str(&mut buf, &mut pos, b" no detectado. Usa 'diskinfo' para ver drives disponibles.");
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    if lba >= info.total_sectors {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Error: LBA ");
        append_u32(&mut buf, &mut pos, lba as u32);
        append_str(&mut buf, &mut pos, b" fuera de rango (total: ");
        append_u32(&mut buf, &mut pos, (info.total_sectors & 0xFFFF_FFFF) as u32);
        append_str(&mut buf, &mut pos, b" sectores).");
        t.write_bytes(&buf[..pos], LineColor::Error);
        return;
    }

    // Leer sector
    let drive      = crate::drivers::storage::ata::AtaDrive::from_info(info);
    let mut sector = [0u8; 512];
    if let Err(e) = drive.read_sectors(lba, 1, &mut sector) {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Error leyendo sector ");
        append_u32(&mut buf, &mut pos, lba as u32);
        append_str(&mut buf, &mut pos, b": ");
        let es = ata_err_str(e);
        buf[pos..pos + es.len()].copy_from_slice(es); pos += es.len();
        t.write_bytes(&buf[..pos], LineColor::Error);
        return;
    }

    // Confirmación en el terminal antes de abrir el editor
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Abriendo editor: LBA=");
        append_u32(&mut buf, &mut pos, lba as u32);
        append_str(&mut buf, &mut pos, b"  drive=");
        append_u32(&mut buf, &mut pos, drv_idx as u32);
        append_str(&mut buf, &mut pos, b"  (");
        let m  = info.model_str().as_bytes();
        let ml = m.len().min(TERM_COLS - pos - 2);
        buf[pos..pos + ml].copy_from_slice(&m[..ml]); pos += ml;
        buf[pos] = b')'; pos += 1;
        t.write_bytes(&buf[..pos], LineColor::Info);
    }

    // Activar el editor; main.rs detecta term.editor.is_some() y cambia el render
    t.editor = Some(EditorState::new(sector, lba, info));
}

// ── diskread ──────────────────────────────────────────────────────────────────

/// Muestra un hexdump de 512 bytes del sector indicado sin abrir el editor.
pub fn cmd_diskread(t: &mut Terminal, args: &[u8]) {
    let (lba, drv_idx) = parse_lba_drive(args);
    let id             = drive_id(drv_idx);

    let bus  = AtaBus::scan();
    let info = match bus.info(id) {
        Some(i) => *i,
        None => { t.write_line("  Error: drive no detectado.", LineColor::Error); return; }
    };

    if lba >= info.total_sectors {
        t.write_line("  Error: LBA fuera de rango.", LineColor::Error); return;
    }

    let drive      = crate::drivers::storage::ata::AtaDrive::from_info(info);
    let mut sector = [0u8; 512];
    if let Err(e) = drive.read_sectors(lba, 1, &mut sector) {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Error: ");
        let es = ata_err_str(e);
        buf[pos..pos + es.len()].copy_from_slice(es); pos += es.len();
        t.write_bytes(&buf[..pos], LineColor::Error);
        return;
    }

    // Cabecera
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Sector LBA=");
        append_u32(&mut buf, &mut pos, lba as u32);
        append_str(&mut buf, &mut pos, b"  drive=");
        append_u32(&mut buf, &mut pos, drv_idx as u32);
        append_str(&mut buf, &mut pos, b"  512 bytes:");
        t.write_bytes(&buf[..pos], LineColor::Info);
    }
    t.write_line("  Offset   00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  ASCII",
                 LineColor::Header);

    const H: &[u8] = b"0123456789ABCDEF";
    for row in 0..32usize {
        let base = row * 16;
        let mut line = [0u8; TERM_COLS]; let mut lp = 0;
        append_str(&mut line, &mut lp, b"  ");
        let off = base as u16;
        line[lp] = H[((off >> 12) & 0xF) as usize]; lp += 1;
        line[lp] = H[((off >>  8) & 0xF) as usize]; lp += 1;
        line[lp] = H[((off >>  4) & 0xF) as usize]; lp += 1;
        line[lp] = H[(off & 0xF)  as usize];         lp += 1;
        append_str(&mut line, &mut lp, b"   ");
        for col in 0..16usize {
            if col == 8 { append_str(&mut line, &mut lp, b" "); }
            let b = sector[base + col];
            line[lp] = H[(b >> 4) as usize]; lp += 1;
            line[lp] = H[(b & 0xF) as usize]; lp += 1;
            append_str(&mut line, &mut lp, b" ");
        }
        append_str(&mut line, &mut lp, b" ");
        for col in 0..16usize {
            let b = sector[base + col];
            line[lp] = if b >= 0x20 && b < 0x7F { b } else { b'.' };
            lp += 1;
        }
        t.write_bytes(&line[..lp], if row % 2 == 0 { LineColor::Normal } else { LineColor::Info });
    }

    // Verificar firma MBR si es sector 0
    if lba == 0 {
        if sector[510] == 0x55 && sector[511] == 0xAA {
            t.write_line("  [MBR] Firma 0x55AA valida — disco particionado.", LineColor::Success);
        } else {
            t.write_line("  [MBR] Sin firma estandar (0x55AA no encontrado).", LineColor::Warning);
        }
    }
    t.write_empty();
}

// ── diskinfo ──────────────────────────────────────────────────────────────────

/// Lista todos los drives ATA detectados.
pub fn cmd_diskinfo(t: &mut Terminal) {
    t.separador("INFORMACION DE DISCO (ATA)");

    let bus = AtaBus::scan();
    if bus.count() == 0 {
        t.write_line("  No se detectaron unidades ATA.", LineColor::Warning);
        t.write_empty(); return;
    }

    t.write_line("  #  Canal        Tipo   LBA    Capacidad     Modelo", LineColor::Header);
    t.write_line("  -  -----------  -----  -----  ------------  ------", LineColor::Normal);

    for info in bus.iter() {
        let idx: usize = info.id as usize;
        let canal: &[u8] = match info.id {
            DriveId::Primary0   => b"ATA0-Master",
            DriveId::Primary1   => b"ATA0-Slave ",
            DriveId::Secondary0 => b"ATA1-Master",
            DriveId::Secondary1 => b"ATA1-Slave ",
        };
        let tipo: &[u8] = if info.kind == DriveType::Atapi { b"ATAPI" } else { b"ATA  " };
        let lba_s: &[u8] = if info.lba48 { b"LBA48" } else { b"LBA28" };

        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  ");
        append_u32(&mut buf, &mut pos, idx as u32);
        append_str(&mut buf, &mut pos, b"  ");
        buf[pos..pos + canal.len()].copy_from_slice(canal); pos += canal.len();
        append_str(&mut buf, &mut pos, b"  ");
        buf[pos..pos + tipo.len()].copy_from_slice(tipo); pos += tipo.len();
        append_str(&mut buf, &mut pos, b"  ");
        buf[pos..pos + lba_s.len()].copy_from_slice(lba_s); pos += lba_s.len();
        append_str(&mut buf, &mut pos, b"  ");
        append_mib(&mut buf, &mut pos, info.capacity_mib);
        while pos < 48 { buf[pos] = b' '; pos += 1; }
        let m  = info.model_str().as_bytes();
        let ml = m.len().min(TERM_COLS - pos);
        buf[pos..pos + ml].copy_from_slice(&m[..ml]); pos += ml;
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    t.write_empty();
    t.write_line("  Comandos:", LineColor::Info);
    t.write_line("    diskread [lba] [drive]   Hexdump de sector (sin modificar)", LineColor::Normal);
    t.write_line("    diskedit [lba] [drive]   Editor hexadecimal interactivo",     LineColor::Normal);
    t.write_line("    diskwrite <lba> <0xPAT>  Rellenar sector con patron (QEMU)", LineColor::Normal);
    t.write_empty();
}

// ── diskwrite ─────────────────────────────────────────────────────────────────

/// Rellena un sector entero con un patrón de 1 byte (solo testing en QEMU).
/// Uso: diskwrite <lba> <0xPATRON>
pub fn cmd_diskwrite(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    let sp   = match args.iter().position(|&b| b == b' ') {
        Some(i) => i,
        None => {
            t.write_line("  Uso: diskwrite <lba> <0xPATRON>", LineColor::Warning);
            t.write_line("  Ejemplo: diskwrite 100 0xAB", LineColor::Normal);
            return;
        }
    };

    let lba = match parse_u64(&args[..sp]) {
        Some(n) => n,
        None => { t.write_line("  Error: LBA invalido.", LineColor::Error); return; }
    };
    let pat = match parse_hex(trim(&args[sp + 1..])) {
        Some(n) => (n & 0xFF) as u8,
        None => { t.write_line("  Error: patron invalido (usa 0xNN).", LineColor::Error); return; }
    };

    let bus  = AtaBus::scan();
    let info = match bus.info(DriveId::Primary0) {
        Some(i) => *i,
        None => { t.write_line("  Error: drive 0 no disponible.", LineColor::Error); return; }
    };
    if lba >= info.total_sectors {
        t.write_line("  Error: LBA fuera de rango.", LineColor::Error); return;
    }

    let drive = crate::drivers::storage::ata::AtaDrive::from_info(info);
    let buf   = [pat; 512];
    match drive.write_sectors(lba, 1, &buf) {
        Ok(()) => {
            let mut line = [0u8; TERM_COLS]; let mut lp = 0;
            append_str(&mut line, &mut lp, b"  [OK] Sector LBA=");
            append_u32(&mut line, &mut lp, lba as u32);
            append_str(&mut line, &mut lp, b" rellenado con 0x");
            const H: &[u8] = b"0123456789ABCDEF";
            line[lp] = H[(pat >> 4) as usize]; lp += 1;
            line[lp] = H[(pat & 0xF) as usize]; lp += 1;
            t.write_bytes(&line[..lp], LineColor::Success);
        }
        Err(_) => { t.write_line("  Error: fallo al escribir.", LineColor::Error); }
    }
}