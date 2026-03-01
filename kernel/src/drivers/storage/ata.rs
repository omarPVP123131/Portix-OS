// drivers/src/storage/ata.rs — PORTIX Kernel v0.7.4
//
// Driver ATA PIO (Programmed I/O) para acceso a discos duros.
//
// Canales soportados:
//   Canal primario   → base 0x1F0, control 0x3F6
//   Canal secundario → base 0x170, control 0x376
//
// Modos:
//   LBA28  → hasta 128 GiB  (sector < 0x0FFF_FFFF)
//   LBA48  → hasta 128 PiB  (sector ≤ 0xFFFF_FFFF_FFFF)
//
// Uso mínimo:
//   let bus = AtaBus::scan();
//   if let Some(drive) = bus.drive(DriveId::Primary0) {
//       let mut buf = [0u8; 512];
//       drive.read_sectors(0, 1, &mut buf).unwrap();
//   }

#![allow(dead_code)]

use core::fmt;

// ── Puertos ATA ────────────────────────────────────────────────────────────────

/// Offsets desde la base del canal
mod reg {
    pub const DATA:       u16 = 0; // R/W  datos (16-bit)
    pub const ERROR:      u16 = 1; // R    registro de error
    pub const FEATURES:   u16 = 1; // W    características
    pub const SECTOR_CNT: u16 = 2; // R/W  contador de sectores
    pub const LBA_LO:     u16 = 3; // R/W  LBA bits  7- 0
    pub const LBA_MID:    u16 = 4; // R/W  LBA bits 15- 8
    pub const LBA_HI:     u16 = 5; // R/W  LBA bits 23-16
    pub const DRIVE_HEAD: u16 = 6; // R/W  selección drive + LBA27-24
    pub const STATUS:     u16 = 7; // R    estado
    pub const COMMAND:    u16 = 7; // W    comando
}

/// Bits del registro STATUS
mod status {
    pub const ERR: u8 = 1 << 0; // Error
    pub const DRQ: u8 = 1 << 3; // Data Request — listo para transferir
    pub const DF:  u8 = 1 << 5; // Drive Fault
    pub const RDY: u8 = 1 << 6; // Drive Ready
    pub const BSY: u8 = 1 << 7; // Busy
}

/// Comandos ATA estándar
mod cmd {
    pub const READ_PIO:        u8 = 0x20;
    pub const READ_PIO_EXT:    u8 = 0x24; // LBA48
    pub const WRITE_PIO:       u8 = 0x30;
    pub const WRITE_PIO_EXT:   u8 = 0x34; // LBA48
    pub const CACHE_FLUSH:     u8 = 0xE7;
    pub const CACHE_FLUSH_EXT: u8 = 0xEA; // LBA48
    pub const IDENTIFY:        u8 = 0xEC;
    pub const IDENTIFY_PACKET: u8 = 0xA1; // ATAPI
}

// ── Tipos públicos ─────────────────────────────────────────────────────────────

/// Identifica uno de los cuatro drives posibles
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveId {
    Primary0   = 0, // canal primario,   master
    Primary1   = 1, // canal primario,   slave
    Secondary0 = 2, // canal secundario, master
    Secondary1 = 3, // canal secundario, slave
}

/// Tipo de dispositivo
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveType {
    Ata,   // disco duro / SSD
    Atapi, // CD/DVD
}

/// Información obtenida mediante IDENTIFY DEVICE
#[derive(Clone, Copy)]
pub struct DriveInfo {
    pub id:            DriveId,
    pub kind:          DriveType,
    /// Número total de sectores LBA
    pub total_sectors: u64,
    /// Capacidad en MiB
    pub capacity_mib:  u64,
    /// Soporte de LBA48
    pub lba48:         bool,
    /// Modelo (40 bytes ASCII, padded con espacios)
    pub model:         [u8; 40],
    /// Revisión de firmware (8 bytes ASCII)
    pub firmware:      [u8; 8],
    /// Número de serie (20 bytes ASCII)
    pub serial:        [u8; 20],
}

impl DriveInfo {
    pub fn model_str(&self) -> &str {
        let end = self.model
            .iter()
            .rposition(|&b| b != b' ' && b != 0)
            .map(|i| i + 1)
            .unwrap_or(0);
        core::str::from_utf8(&self.model[..end]).unwrap_or("?")
    }
    pub fn serial_str(&self) -> &str {
        let end = self.serial
            .iter()
            .rposition(|&b| b != b' ' && b != 0)
            .map(|i| i + 1)
            .unwrap_or(0);
        core::str::from_utf8(&self.serial[..end]).unwrap_or("?")
    }
    pub fn firmware_str(&self) -> &str {
        let end = self.firmware
            .iter()
            .rposition(|&b| b != b' ' && b != 0)
            .map(|i| i + 1)
            .unwrap_or(0);
        core::str::from_utf8(&self.firmware[..end]).unwrap_or("?")
    }
}

/// Errores del driver
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtaError {
    NoDrive,
    DeviceError(u8),
    DriveFault,
    Timeout,
    OutOfRange,
    BadBuffer,
}

impl fmt::Display for AtaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtaError::NoDrive        => write!(f, "no hay drive"),
            AtaError::DeviceError(e) => write!(f, "error del dispositivo (0x{:02X})", e),
            AtaError::DriveFault     => write!(f, "fallo del drive"),
            AtaError::Timeout        => write!(f, "timeout"),
            AtaError::OutOfRange     => write!(f, "sector fuera de rango"),
            AtaError::BadBuffer      => write!(f, "buffer debe ser múltiplo de 512 bytes"),
        }
    }
}

pub type AtaResult<T> = Result<T, AtaError>;

// ── Canal ATA (privado) ────────────────────────────────────────────────────────

struct Channel {
    base:    u16, // 0x1F0 / 0x170
    control: u16, // 0x3F6 / 0x376
}

impl Channel {
    const fn primary()   -> Self { Channel { base: 0x1F0, control: 0x3F6 } }
    const fn secondary() -> Self { Channel { base: 0x170, control: 0x376 } }

    #[inline] unsafe fn inb(&self, r: u16) -> u8 {
        let v: u8;
        core::arch::asm!("in al, dx",
            out("al") v, in("dx") self.base + r,
            options(nostack, nomem));
        v
    }
    #[inline] unsafe fn outb(&self, r: u16, v: u8) {
        core::arch::asm!("out dx, al",
            in("dx") self.base + r, in("al") v,
            options(nostack, nomem));
    }
    #[inline] unsafe fn inw(&self) -> u16 {
        let v: u16;
        core::arch::asm!("in ax, dx",
            out("ax") v, in("dx") self.base + reg::DATA,
            options(nostack, nomem));
        v
    }
    #[inline] unsafe fn outw(&self, v: u16) {
        core::arch::asm!("out dx, ax",
            in("dx") self.base + reg::DATA, in("ax") v,
            options(nostack, nomem));
    }
    #[inline] unsafe fn ctrl_inb(&self) -> u8 {
        let v: u8;
        core::arch::asm!("in al, dx",
            out("al") v, in("dx") self.control,
            options(nostack, nomem));
        v
    }

    /// 400 ns de espera (4× lectura del registro de control alternativo)
    #[inline] unsafe fn delay400ns(&self) {
        for _ in 0..4 { let _ = self.ctrl_inb(); }
    }

    /// Espera hasta BSY=0; devuelve el STATUS final
    unsafe fn wait_not_busy(&self) -> AtaResult<u8> {
        for _ in 0..100_000u32 {
            let st = self.inb(reg::STATUS);
            if st & status::BSY == 0 { return Ok(st); }
        }
        Err(AtaError::Timeout)
    }

    /// Espera hasta DRQ=1 (o error)
    unsafe fn wait_drq(&self) -> AtaResult<()> {
        loop {
            let st = self.inb(reg::STATUS);
            if st & status::BSY != 0  { continue; }
            if st & status::ERR != 0  {
                return Err(AtaError::DeviceError(self.inb(reg::ERROR)));
            }
            if st & status::DF  != 0  { return Err(AtaError::DriveFault); }
            if st & status::DRQ != 0  { return Ok(()); }
        }
    }

    /// Envía IDENTIFY y devuelve los 256 words si el drive existe
    unsafe fn identify(&self, is_slave: bool) -> Option<[u16; 256]> {
        self.outb(reg::DRIVE_HEAD, if is_slave { 0xB0 } else { 0xA0 });
        self.delay400ns();

        // Poner a cero los registros de dirección
        for r in [reg::SECTOR_CNT, reg::LBA_LO, reg::LBA_MID, reg::LBA_HI] {
            self.outb(r, 0);
        }

        self.outb(reg::COMMAND, cmd::IDENTIFY);
        self.delay400ns();

        // STATUS == 0  →  no hay drive
        if self.inb(reg::STATUS) == 0 { return None; }
        if self.wait_not_busy().is_err() { return None; }

        // Si LBA_MID/LBA_HI != 0 el device es ATAPI → re-IDENTIFY
        if self.inb(reg::LBA_MID) != 0 || self.inb(reg::LBA_HI) != 0 {
            self.outb(reg::COMMAND, cmd::IDENTIFY_PACKET);
            self.delay400ns();
            if self.wait_not_busy().is_err() { return None; }
        }

        if self.wait_drq().is_err() { return None; }

        let mut buf = [0u16; 256];
        for w in buf.iter_mut() { *w = self.inw(); }
        Some(buf)
    }
}

// ── Drive ──────────────────────────────────────────────────────────────────────

/// Handle a un drive ATA listo para E/S
pub struct AtaDrive {
    info:     DriveInfo,
    chan:     &'static Channel,
    is_slave: bool,
}

impl AtaDrive {
    /// Crea un handle de E/S desde un DriveInfo ya conocido, sin re-escanear el bus.
    /// Usado por el editor hexadecimal para guardar sectores sin relanzar AtaBus::scan().
    pub fn from_info(info: DriveInfo) -> Self {
        let (chan, is_slave) = match info.id {
            DriveId::Primary0   => (&PRIMARY,   false),
            DriveId::Primary1   => (&PRIMARY,   true),
            DriveId::Secondary0 => (&SECONDARY, false),
            DriveId::Secondary1 => (&SECONDARY, true),
        };
        AtaDrive { info, chan, is_slave }
    }

    pub fn info(&self) -> &DriveInfo { &self.info }

    // ── Lectura ──────────────────────────────────────────────────────────────

    /// Lee `count` sectores a partir de `lba` en `buf` (`buf.len() == count*512`)
    pub fn read_sectors(&self, lba: u64, count: usize, buf: &mut [u8]) -> AtaResult<()> {
        self.check(lba, count, buf.len())?;
        if count == 0 { return Ok(()); }
        if self.info.lba48 || lba >= (1 << 28) {
            unsafe { self.read48(lba, count, buf) }
        } else {
            unsafe { self.read28(lba, count, buf) }
        }
    }

    // ── Escritura ─────────────────────────────────────────────────────────────

    /// Escribe `count` sectores a partir de `lba` desde `buf` (`buf.len() == count*512`)
    pub fn write_sectors(&self, lba: u64, count: usize, buf: &[u8]) -> AtaResult<()> {
        self.check(lba, count, buf.len())?;
        if count == 0 { return Ok(()); }
        if self.info.lba48 || lba >= (1 << 28) {
            unsafe { self.write48(lba, count, buf) }
        } else {
            unsafe { self.write28(lba, count, buf) }
        }
    }

    /// Envía CACHE FLUSH al drive
    pub fn flush(&self) -> AtaResult<()> {
        unsafe {
            let c = self.chan;
            c.wait_not_busy()?;
            c.outb(reg::DRIVE_HEAD, if self.is_slave { 0xB0 } else { 0xA0 });
            c.delay400ns();
            c.outb(reg::COMMAND,
                if self.info.lba48 { cmd::CACHE_FLUSH_EXT } else { cmd::CACHE_FLUSH });
            c.wait_not_busy()?;
            Ok(())
        }
    }

    // ── LBA28 ─────────────────────────────────────────────────────────────────

    unsafe fn read28(&self, lba: u64, count: usize, buf: &mut [u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave = if self.is_slave { 0x10u8 } else { 0x00 };

        for s in 0..count {
            let cur = lba + s as u64;
            c.wait_not_busy()?;
            c.outb(reg::DRIVE_HEAD, 0xE0 | slave | ((cur >> 24) as u8 & 0x0F));
            c.outb(reg::SECTOR_CNT, 1);
            c.outb(reg::LBA_LO,     cur as u8);
            c.outb(reg::LBA_MID,   (cur >>  8) as u8);
            c.outb(reg::LBA_HI,    (cur >> 16) as u8);
            c.outb(reg::COMMAND,    cmd::READ_PIO);
            c.delay400ns();
            c.wait_drq()?;
            Self::pio_read_sector(c, buf, s * 512);
        }
        Ok(())
    }

    unsafe fn write28(&self, lba: u64, count: usize, buf: &[u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave = if self.is_slave { 0x10u8 } else { 0x00 };

        for s in 0..count {
            let cur = lba + s as u64;
            c.wait_not_busy()?;
            c.outb(reg::DRIVE_HEAD, 0xE0 | slave | ((cur >> 24) as u8 & 0x0F));
            c.outb(reg::SECTOR_CNT, 1);
            c.outb(reg::LBA_LO,     cur as u8);
            c.outb(reg::LBA_MID,   (cur >>  8) as u8);
            c.outb(reg::LBA_HI,    (cur >> 16) as u8);
            c.outb(reg::COMMAND,    cmd::WRITE_PIO);
            c.delay400ns();
            c.wait_drq()?;
            Self::pio_write_sector(c, buf, s * 512);
        }
        self.flush()
    }

    // ── LBA48 ─────────────────────────────────────────────────────────────────

    unsafe fn read48(&self, lba: u64, count: usize, buf: &mut [u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave = if self.is_slave { 0x10u8 } else { 0x00 };

        for s in 0..count {
            let cur = lba + s as u64;
            c.wait_not_busy()?;
            c.outb(reg::DRIVE_HEAD, 0x40 | slave);

            // Primero los bytes altos (HOB), luego los bajos
            c.outb(reg::SECTOR_CNT, 0);                  // count  [15:8]
            c.outb(reg::LBA_LO,    (cur >> 24) as u8);   // LBA    [31:24]
            c.outb(reg::LBA_MID,   (cur >> 32) as u8);   // LBA    [39:32]
            c.outb(reg::LBA_HI,    (cur >> 40) as u8);   // LBA    [47:40]
            c.outb(reg::SECTOR_CNT, 1);                   // count  [7:0]
            c.outb(reg::LBA_LO,     cur as u8);           // LBA    [7:0]
            c.outb(reg::LBA_MID,   (cur >>  8) as u8);   // LBA    [15:8]
            c.outb(reg::LBA_HI,    (cur >> 16) as u8);   // LBA    [23:16]

            c.outb(reg::COMMAND, cmd::READ_PIO_EXT);
            c.delay400ns();
            c.wait_drq()?;
            Self::pio_read_sector(c, buf, s * 512);
        }
        Ok(())
    }

    unsafe fn write48(&self, lba: u64, count: usize, buf: &[u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave = if self.is_slave { 0x10u8 } else { 0x00 };

        for s in 0..count {
            let cur = lba + s as u64;
            c.wait_not_busy()?;
            c.outb(reg::DRIVE_HEAD, 0x40 | slave);

            c.outb(reg::SECTOR_CNT, 0);
            c.outb(reg::LBA_LO,    (cur >> 24) as u8);
            c.outb(reg::LBA_MID,   (cur >> 32) as u8);
            c.outb(reg::LBA_HI,    (cur >> 40) as u8);
            c.outb(reg::SECTOR_CNT, 1);
            c.outb(reg::LBA_LO,     cur as u8);
            c.outb(reg::LBA_MID,   (cur >>  8) as u8);
            c.outb(reg::LBA_HI,    (cur >> 16) as u8);

            c.outb(reg::COMMAND, cmd::WRITE_PIO_EXT);
            c.delay400ns();
            c.wait_drq()?;
            Self::pio_write_sector(c, buf, s * 512);
        }
        self.flush()
    }

    // ── Helpers de transferencia ──────────────────────────────────────────────

    #[inline]
    unsafe fn pio_read_sector(c: &Channel, buf: &mut [u8], offset: usize) {
        for i in 0..256usize {
            let w = c.inw();
            buf[offset + i * 2]     = w as u8;
            buf[offset + i * 2 + 1] = (w >> 8) as u8;
        }
        c.delay400ns();
    }

    #[inline]
    unsafe fn pio_write_sector(c: &Channel, buf: &[u8], offset: usize) {
        for i in 0..256usize {
            let lo = buf[offset + i * 2]     as u16;
            let hi = buf[offset + i * 2 + 1] as u16;
            c.outw(lo | (hi << 8));
        }
        c.delay400ns();
    }

    // ── Validación ────────────────────────────────────────────────────────────

    fn check(&self, lba: u64, count: usize, buf_len: usize) -> AtaResult<()> {
        if buf_len != count * 512 { return Err(AtaError::BadBuffer); }
        let end = lba.checked_add(count as u64).ok_or(AtaError::OutOfRange)?;
        if end > self.info.total_sectors { return Err(AtaError::OutOfRange); }
        Ok(())
    }
}

// ── Bus ────────────────────────────────────────────────────────────────────────

static PRIMARY:   Channel = Channel::primary();
static SECONDARY: Channel = Channel::secondary();

/// Resultado del escaneo inicial del bus ATA
pub struct AtaBus {
    drives: [Option<DriveInfo>; 4],
    count:  usize,
}

impl AtaBus {
    /// Detecta todos los drives ATA presentes (≤ 4: 2 canales × 2 drives)
    pub fn scan() -> Self {
        let slots: [(DriveId, &'static Channel, bool); 4] = [
            (DriveId::Primary0,   &PRIMARY,   false),
            (DriveId::Primary1,   &PRIMARY,   true),
            (DriveId::Secondary0, &SECONDARY, false),
            (DriveId::Secondary1, &SECONDARY, true),
        ];

        let mut drives = [None; 4];
        let mut count  = 0;

        for (id, chan, is_slave) in slots {
            if let Some(words) = unsafe { chan.identify(is_slave) } {
                drives[id as usize] = Some(parse_identify(words, id));
                count += 1;
            }
        }

        AtaBus { drives, count }
    }

    pub fn count(&self) -> usize { self.count }

    /// Información de un drive (sin abrir un handle de E/S)
    pub fn info(&self, id: DriveId) -> Option<&DriveInfo> {
        self.drives[id as usize].as_ref()
    }

    /// Devuelve un `AtaDrive` listo para leer/escribir
    pub fn drive(&self, id: DriveId) -> Option<AtaDrive> {
        let info = self.drives[id as usize]?;
        let (chan, is_slave) = match id {
            DriveId::Primary0   => (&PRIMARY,   false),
            DriveId::Primary1   => (&PRIMARY,   true),
            DriveId::Secondary0 => (&SECONDARY, false),
            DriveId::Secondary1 => (&SECONDARY, true),
        };
        Some(AtaDrive { info, chan, is_slave })
    }

    /// Itera sobre los drives presentes
    pub fn iter(&self) -> impl Iterator<Item = &DriveInfo> {
        self.drives.iter().filter_map(|d| d.as_ref())
    }
}

// ── Parseo de IDENTIFY ────────────────────────────────────────────────────────

fn parse_identify(words: [u16; 256], id: DriveId) -> DriveInfo {
    // Word 0 bit 15 = 0 → ATA,  = 1 → ATAPI
    let kind = if words[0] & 0x8000 != 0 { DriveType::Atapi } else { DriveType::Ata };

    // Strings ATA: cada word almacena 2 bytes en orden big-endian
    let mut model    = [b' '; 40];
    let mut firmware = [b' ';  8];
    let mut serial   = [b' '; 20];

    for i in 0..20usize { let w = words[27+i]; model[i*2]=(w>>8)as u8; model[i*2+1]=(w&0xFF)as u8; }
    for i in 0.. 4usize { let w = words[23+i]; firmware[i*2]=(w>>8)as u8; firmware[i*2+1]=(w&0xFF)as u8; }
    for i in 0..10usize { let w = words[10+i]; serial[i*2]=(w>>8)as u8; serial[i*2+1]=(w&0xFF)as u8; }

    // LBA48: word 83 bit 10
    let lba48 = words[83] & (1 << 10) != 0;

    // Número total de sectores
    let total_sectors = if lba48 {
        // words 100-103 (64-bit little-endian en words de 16 bits)
        (words[100] as u64)
        | ((words[101] as u64) << 16)
        | ((words[102] as u64) << 32)
        | ((words[103] as u64) << 48)
    } else {
        // words 60-61 (32-bit)
        (words[60] as u64) | ((words[61] as u64) << 16)
    };

    DriveInfo {
        id,
        kind,
        total_sectors,
        capacity_mib: total_sectors / 2048,
        lba48,
        model,
        firmware,
        serial,
    }
}

// ── Helpers de log ────────────────────────────────────────────────────────────

/// Imprime por serial la lista de drives; llama desde `rust_main` tras `AtaBus::scan()`
pub fn log_drives(bus: &AtaBus) {
    use crate::drivers::serial;

    if bus.count() == 0 {
        serial::log("ATA", "ningun drive detectado");
        return;
    }

    for info in bus.iter() {
        let label = match info.id {
            DriveId::Primary0   => "pri/master",
            DriveId::Primary1   => "pri/slave ",
            DriveId::Secondary0 => "sec/master",
            DriveId::Secondary1 => "sec/slave ",
        };
        let kind_s = if info.kind == DriveType::Atapi { "ATAPI" } else { "ATA  " };
        let lba_s  = if info.lba48 { "LBA48" } else { "LBA28" };

        let mut tmp = [0u8; 20];
        let mib_s = crate::util::fmt::fmt_u64(info.capacity_mib, &mut tmp);

        serial::write_str("ATA ["); serial::write_str(label);
        serial::write_str("] ");    serial::write_str(kind_s);
        serial::write_str(" ");     serial::write_str(lba_s);
        serial::write_str("  ");    serial::write_str(mib_s);
        serial::write_str(" MiB  ");serial::write_str(info.model_str());
        serial::write_str("\n");
    }
}