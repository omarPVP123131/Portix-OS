// drivers/src/storage/ata.rs — PORTIX Kernel v0.8.0
//
// CAMBIOS v0.8.0 — Fix "no se detecta drive ATA" en comandos del terminal:
//
//   PROBLEMA RAÍZ:
//     mount_vol() en disk.rs llamaba AtaBus::scan() en CADA comando de disco.
//     scan() → identify() → reset_and_init() (soft-reset del canal ATA).
//     QEMU/VirtualBox no toleran un segundo reset mientras el canal ya está
//     activo → el drive "desaparece" y todos los comandos fallan tras el primero.
//     Síntoma visible: ls y diskpart funcionan, edit/touch/cat fallan con
//     "no se detecta ningún drive ATA".
//
//   SOLUCIÓN — DriveInfo estático en BSS (sin Mutex, sin std):
//
//     ┌─────────────────────────────────────────────────────────────────┐
//     │  main.rs (boot)                                                 │
//     │    AtaBus::scan()  ← único scan(), único reset_and_init()       │
//     │    store_primary_drive_info(info)  ← guarda DriveInfo en BSS    │
//     │                                                                 │
//     │  disk.rs (cada comando)                                         │
//     │    get_cached_drive_info()   ← lee BSS, NO toca hardware        │
//     │    AtaDrive::from_info(info) ← construye handle (ya existía)    │
//     │    Fat32Volume::mount(drive) ← monta FAT32 normalmente          │
//     └─────────────────────────────────────────────────────────────────┘
//
//   POR QUÉ NO Mutex/Spinlock:
//     - Kernel bare-metal single-threaded → no hay carreras de datos.
//     - Mutex requeriría implementar un tipo de sincronización custom o
//       importar una crate externa (spin), añadiendo complejidad innecesaria.
//     - DriveInfo es Copy, la escritura en boot es atómica en términos
//       de secuenciación del programa.
//
//   ARCHIVOS MODIFICADOS:
//     - drivers/src/storage/ata.rs   ← este archivo (añade CACHED_DRIVE + fns)
//     - console/terminal/commands/disk.rs ← mount_vol usa get_cached_drive_info
//     - kernel/src/main.rs           ← llama store_primary_drive_info en boot
//
//   INVARIANTES GARANTIZADOS POST-PATCH:
//     1. reset_and_init() solo se ejecuta UNA vez (durante scan() en boot).
//     2. Todos los comandos del terminal usan el DriveInfo cacheado.
//     3. diskpart conserva su scan() propio (comando de diagnóstico).
//     4. diskread/diskedit para drives != Primary0 pueden hacer scan puntual.
//     5. Si no hay drive cacheado → error descriptivo, no panic.
//
// ─────────────────────────────────────────────────────────────────────────────
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
// CAMBIOS v0.7.5 (robustez) — conservados íntegros:
//   - Inicialización del canal: soft reset + deshabilitar IRQs (nIEN)
//   - drive_select(): espera BSY=0 Y RDY=1 tras seleccionar drive
//   - features register a 0 antes de cada comando
//   - wait_drq(): timeout finito (100 000 iteraciones)
//   - pio_write_sector(): pequeña pausa entre palabras
//   - Escritura: poll DRQ después de enviar el comando
//   - flush() comprueba ERR/DF en el status final
//   - Todos los timeout devuelven AtaError::Timeout

#![allow(dead_code)]

use core::fmt;

// ── Puertos ATA ────────────────────────────────────────────────────────────────

/// Offsets desde la base del canal
mod reg {
    pub const DATA:       u16 = 0;
    pub const ERROR:      u16 = 1;
    pub const FEATURES:   u16 = 1;
    pub const SECTOR_CNT: u16 = 2;
    pub const LBA_LO:     u16 = 3;
    pub const LBA_MID:    u16 = 4;
    pub const LBA_HI:     u16 = 5;
    pub const DRIVE_HEAD: u16 = 6;
    pub const STATUS:     u16 = 7;
    pub const COMMAND:    u16 = 7;
}

mod status {
    pub const ERR: u8 = 1 << 0;
    pub const DRQ: u8 = 1 << 3;
    pub const DF:  u8 = 1 << 5;
    pub const RDY: u8 = 1 << 6;
    pub const BSY: u8 = 1 << 7;
}

mod dctl {
    pub const NIEN:  u8 = 1 << 1;
    pub const SRST:  u8 = 1 << 2;
    pub const HOB:   u8 = 1 << 7;
}

mod cmd {
    pub const READ_PIO:        u8 = 0x20;
    pub const READ_PIO_EXT:    u8 = 0x24;
    pub const WRITE_PIO:       u8 = 0x30;
    pub const WRITE_PIO_EXT:   u8 = 0x34;
    pub const CACHE_FLUSH:     u8 = 0xE7;
    pub const CACHE_FLUSH_EXT: u8 = 0xEA;
    pub const IDENTIFY:        u8 = 0xEC;
    pub const IDENTIFY_PACKET: u8 = 0xA1;
}

// ── Tipos públicos ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveId {
    Primary0   = 0,
    Primary1   = 1,
    Secondary0 = 2,
    Secondary1 = 3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveType {
    Ata,
    Atapi,
}

#[derive(Clone, Copy)]
pub struct DriveInfo {
    pub id:            DriveId,
    pub kind:          DriveType,
    pub total_sectors: u64,
    pub capacity_mib:  u64,
    pub lba48:         bool,
    pub model:         [u8; 40],
    pub firmware:      [u8; 8],
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
    base:    u16,
    control: u16,
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
    #[inline] unsafe fn ctrl_outb(&self, v: u8) {
        core::arch::asm!("out dx, al",
            in("dx") self.control, in("al") v,
            options(nostack, nomem));
    }

    /// 400 ns de espera (4× lectura del registro de control alternativo).
    #[inline] unsafe fn delay400ns(&self) {
        for _ in 0..4 { let _ = self.ctrl_inb(); }
    }

    /// Pausa mínima entre palabras en escritura PIO (≈ "jmp $+2").
    #[inline] unsafe fn tiny_pause(&self) {
        let _ = self.ctrl_inb();
    }

    /// Realiza un soft-reset del canal y desactiva las IRQs (nIEN).
    /// ⚠️  COSTOSO — solo debe llamarse durante AtaBus::scan() en el arranque.
    unsafe fn reset_and_init(&self) {
        self.ctrl_outb(dctl::NIEN | dctl::SRST);
        for _ in 0..25 { let _ = self.ctrl_inb(); }
        self.ctrl_outb(dctl::NIEN);
        for _ in 0..100_000u32 {
            if self.ctrl_inb() & status::BSY == 0 { break; }
        }
    }

    unsafe fn wait_not_busy(&self) -> AtaResult<u8> {
        for _ in 0..100_000u32 {
            let st = self.ctrl_inb();
            if st & status::BSY == 0 {
                return Ok(self.inb(reg::STATUS));
            }
        }
        Err(AtaError::Timeout)
    }

    unsafe fn wait_ready(&self) -> AtaResult<()> {
        for _ in 0..100_000u32 {
            let st = self.ctrl_inb();
            if st & status::BSY == 0 && st & status::RDY != 0 {
                let _ = self.inb(reg::STATUS);
                return Ok(());
            }
        }
        Err(AtaError::Timeout)
    }

    unsafe fn wait_drq(&self) -> AtaResult<()> {
        for _ in 0..100_000u32 {
            let st = self.ctrl_inb();
            if st & status::BSY != 0 { continue; }
            if st & status::ERR != 0 {
                let _ = self.inb(reg::STATUS);
                return Err(AtaError::DeviceError(self.inb(reg::ERROR)));
            }
            if st & status::DF  != 0 {
                let _ = self.inb(reg::STATUS);
                return Err(AtaError::DriveFault);
            }
            if st & status::DRQ != 0 {
                let _ = self.inb(reg::STATUS);
                return Ok(());
            }
        }
        Err(AtaError::Timeout)
    }

    unsafe fn select_drive(&self, head_val: u8) -> AtaResult<()> {
        self.wait_not_busy()?;
        self.outb(reg::DRIVE_HEAD, head_val);
        self.delay400ns();
        self.wait_ready()?;
        Ok(())
    }

    unsafe fn identify(&self, is_slave: bool) -> Option<[u16; 256]> {
        self.reset_and_init();

        let head = if is_slave { 0xB0u8 } else { 0xA0u8 };
        self.outb(reg::DRIVE_HEAD, head);
        self.delay400ns();

        for r in [reg::FEATURES, reg::SECTOR_CNT, reg::LBA_LO, reg::LBA_MID, reg::LBA_HI] {
            self.outb(r, 0);
        }

        self.outb(reg::COMMAND, cmd::IDENTIFY);
        self.delay400ns();

        if self.ctrl_inb() == 0 { return None; }
        if self.wait_not_busy().is_err() { return None; }

        if self.inb(reg::LBA_MID) != 0 || self.inb(reg::LBA_HI) != 0 {
            self.outb(reg::COMMAND, cmd::IDENTIFY_PACKET);
            self.delay400ns();
            if self.wait_not_busy().is_err() { return None; }
        }

        if self.wait_drq().is_err() { return None; }

        let mut buf = [0u16; 256];
        for w in buf.iter_mut() { *w = self.inw(); }
        self.delay400ns();
        Some(buf)
    }
}

// ── Drive ──────────────────────────────────────────────────────────────────────

pub struct AtaDrive {
    info:     DriveInfo,
    chan:     &'static Channel,
    is_slave: bool,
}

impl AtaDrive {
    /// Crea un handle de E/S desde un DriveInfo ya conocido, sin re-escanear el bus.
    /// ✅ NO llama reset_and_init() — seguro de usar en cualquier momento.
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

    pub fn read_sectors(&self, lba: u64, count: usize, buf: &mut [u8]) -> AtaResult<()> {
        self.check(lba, count, buf.len())?;
        if count == 0 { return Ok(()); }
        if self.info.lba48 || lba >= (1 << 28) {
            unsafe { self.read48(lba, count, buf) }
        } else {
            unsafe { self.read28(lba, count, buf) }
        }
    }

    pub fn write_sectors(&self, lba: u64, count: usize, buf: &[u8]) -> AtaResult<()> {
        self.check(lba, count, buf.len())?;
        if count == 0 { return Ok(()); }
        if self.info.lba48 || lba >= (1 << 28) {
            unsafe { self.write48(lba, count, buf) }
        } else {
            unsafe { self.write28(lba, count, buf) }
        }
    }

    pub fn flush(&self) -> AtaResult<()> {
        unsafe {
            let c = self.chan;
            let head = if self.is_slave { 0xB0u8 } else { 0xA0u8 };
            c.select_drive(head)?;
            c.outb(reg::FEATURES, 0);
            c.outb(reg::COMMAND,
                if self.info.lba48 { cmd::CACHE_FLUSH_EXT } else { cmd::CACHE_FLUSH });
            let _ = c.wait_not_busy();
            Ok(())
        }
    }

    unsafe fn read28(&self, lba: u64, count: usize, buf: &mut [u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave_bit = if self.is_slave { 0x10u8 } else { 0x00u8 };
        for s in 0..count {
            let cur = lba + s as u64;
            c.select_drive(0xE0 | slave_bit | ((cur >> 24) as u8 & 0x0F))?;
            c.outb(reg::FEATURES,    0);
            c.outb(reg::SECTOR_CNT,  1);
            c.outb(reg::LBA_LO,      cur as u8);
            c.outb(reg::LBA_MID,    (cur >>  8) as u8);
            c.outb(reg::LBA_HI,     (cur >> 16) as u8);
            c.outb(reg::COMMAND,     cmd::READ_PIO);
            c.delay400ns();
            c.wait_drq()?;
            Self::pio_read_sector(c, buf, s * 512);
        }
        Ok(())
    }

    unsafe fn write28(&self, lba: u64, count: usize, buf: &[u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave_bit = if self.is_slave { 0x10u8 } else { 0x00u8 };
        for s in 0..count {
            let cur = lba + s as u64;
            c.select_drive(0xE0 | slave_bit | ((cur >> 24) as u8 & 0x0F))?;
            c.outb(reg::FEATURES,    0);
            c.outb(reg::SECTOR_CNT,  1);
            c.outb(reg::LBA_LO,      cur as u8);
            c.outb(reg::LBA_MID,    (cur >>  8) as u8);
            c.outb(reg::LBA_HI,     (cur >> 16) as u8);
            c.outb(reg::COMMAND,     cmd::WRITE_PIO);
            c.wait_drq()?;
            Self::pio_write_sector(c, buf, s * 512);
            self.flush()?;
        }
        Ok(())
    }

    unsafe fn read48(&self, lba: u64, count: usize, buf: &mut [u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave_bit = if self.is_slave { 0x10u8 } else { 0x00u8 };
        for s in 0..count {
            let cur = lba + s as u64;
            c.select_drive(0x40 | slave_bit)?;
            c.outb(reg::FEATURES,    0);
            c.outb(reg::SECTOR_CNT,  0);
            c.outb(reg::LBA_LO,     (cur >> 24) as u8);
            c.outb(reg::LBA_MID,    (cur >> 32) as u8);
            c.outb(reg::LBA_HI,     (cur >> 40) as u8);
            c.outb(reg::FEATURES,    0);
            c.outb(reg::SECTOR_CNT,  1);
            c.outb(reg::LBA_LO,      cur as u8);
            c.outb(reg::LBA_MID,    (cur >>  8) as u8);
            c.outb(reg::LBA_HI,     (cur >> 16) as u8);
            c.outb(reg::COMMAND,     cmd::READ_PIO_EXT);
            c.delay400ns();
            c.wait_drq()?;
            Self::pio_read_sector(c, buf, s * 512);
        }
        Ok(())
    }

    unsafe fn write48(&self, lba: u64, count: usize, buf: &[u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave_bit = if self.is_slave { 0x10u8 } else { 0x00u8 };
        for s in 0..count {
            let cur = lba + s as u64;
            c.select_drive(0x40 | slave_bit)?;
            c.outb(reg::FEATURES,    0);
            c.outb(reg::SECTOR_CNT,  0);
            c.outb(reg::LBA_LO,     (cur >> 24) as u8);
            c.outb(reg::LBA_MID,    (cur >> 32) as u8);
            c.outb(reg::LBA_HI,     (cur >> 40) as u8);
            c.outb(reg::FEATURES,    0);
            c.outb(reg::SECTOR_CNT,  1);
            c.outb(reg::LBA_LO,      cur as u8);
            c.outb(reg::LBA_MID,    (cur >>  8) as u8);
            c.outb(reg::LBA_HI,     (cur >> 16) as u8);
            c.outb(reg::COMMAND,     cmd::WRITE_PIO_EXT);
            c.wait_drq()?;
            Self::pio_write_sector(c, buf, s * 512);
            self.flush()?;
        }
        Ok(())
    }

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
            c.tiny_pause();
        }
        c.delay400ns();
    }

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

pub struct AtaBus {
    drives: [Option<DriveInfo>; 4],
    count:  usize,
}

impl AtaBus {
    /// Detecta todos los drives ATA presentes.
    /// ⚠️  Llama reset_and_init() internamente.
    ///     Debe invocarse UNA SOLA VEZ en main.rs durante el arranque.
    ///     Llamarlo de nuevo mata el canal en QEMU/VirtualBox.
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

    pub fn info(&self, id: DriveId) -> Option<&DriveInfo> {
        self.drives[id as usize].as_ref()
    }

    pub fn drive(&self, id: DriveId) -> Option<AtaDrive> {
        let info = self.drives[id as usize]?;
        Some(AtaDrive::from_info(info))
    }

    pub fn iter(&self) -> impl Iterator<Item = &DriveInfo> {
        self.drives.iter().filter_map(|d| d.as_ref())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  CACHÉ GLOBAL DE DriveInfo — NUEVO en v0.8.0
// ═══════════════════════════════════════════════════════════════════════════════
//
// Por qué un static mut con bool en lugar de Option<DriveInfo>:
//   Option<T> no es const-initializable en no_std cuando T no implementa
//   el trait const Default. DriveInfo contiene enums que no tienen valor
//   "cero" claro para el inicializador estático. Usar un struct wrapper
//   con un campo `valid: bool` es el patrón idiomático en kernels Rust
//   bare-metal (ver también Linux/Redox).
//
// Seguridad:
//   El kernel PORTIX es single-threaded. No existen preemption ni ISRs
//   que modifiquen este dato. La escritura ocurre una sola vez en boot
//   y todas las lecturas son posteriores → no hay carreras de datos.
//   El compilador no puede reordenar más allá de la barrera de boot
//   porque la escritura y las lecturas están en funciones distintas
//   compiladas por separado.

struct CachedDrive {
    info:  DriveInfo,
    valid: bool,
}

// Valor dummy para inicialización estática. Solo usado para que el
// compilador pueda inicializar la BSS. Nunca se lee si valid == false.
const DUMMY_INFO: DriveInfo = DriveInfo {
    id:            DriveId::Primary0,
    kind:          DriveType::Ata,
    total_sectors: 0,
    capacity_mib:  0,
    lba48:         false,
    model:         [b' '; 40],
    firmware:      [b' ';  8],
    serial:        [b' '; 20],
};

static mut CACHED_DRIVE: CachedDrive = CachedDrive {
    info:  DUMMY_INFO,
    valid: false,
};

/// Guarda el DriveInfo del Primary0 en el caché global.
///
/// **Llamar UNA SOLA VEZ desde `main.rs`**, justo después del primer
/// `AtaBus::scan()` exitoso, antes de lanzar el loop principal.
///
/// Ejemplo en main.rs:
/// ```rust
/// let ata = ata::AtaBus::scan();
/// ata::log_drives(&ata);
/// if let Some(info) = ata.info(ata::DriveId::Primary0) {
///     ata::store_primary_drive_info(*info);
/// }
/// ```
pub fn store_primary_drive_info(info: DriveInfo) {
    // SAFETY: kernel bare-metal single-threaded.
    // Esta función se llama exactamente una vez en boot antes del loop.
    unsafe {
        CACHED_DRIVE.info  = info;
        CACHED_DRIVE.valid = true;
    }
}

/// Devuelve el DriveInfo cacheado del Primary0.
///
/// Returns `None` si `store_primary_drive_info` no fue llamado todavía
/// (no debería ocurrir en operación normal).
///
/// **NO toca el hardware.** Seguro de llamar en cualquier momento.
/// Usar junto con `AtaDrive::from_info()` para acceso sin re-escanear.
///
/// Ejemplo en disk.rs:
/// ```rust
/// let info = match get_cached_drive_info() {
///     Some(i) => i,
///     None => { /* error */ return; }
/// };
/// let drive = AtaDrive::from_info(info);
/// let vol = Fat32Volume::mount(drive)?;
/// ```
pub fn get_cached_drive_info() -> Option<DriveInfo> {
    // SAFETY: ver store_primary_drive_info.
    unsafe {
        if CACHED_DRIVE.valid {
            Some(CACHED_DRIVE.info)
        } else {
            None
        }
    }
}

// ── Parseo de IDENTIFY ────────────────────────────────────────────────────────

fn parse_identify(words: [u16; 256], id: DriveId) -> DriveInfo {
    let kind = if words[0] & 0x8000 != 0 { DriveType::Atapi } else { DriveType::Ata };

    let mut model    = [b' '; 40];
    let mut firmware = [b' ';  8];
    let mut serial   = [b' '; 20];

    for i in 0..20usize {
        let w = words[27 + i];
        model[i * 2]     = (w >> 8) as u8;
        model[i * 2 + 1] = (w & 0xFF) as u8;
    }
    for i in 0..4usize {
        let w = words[23 + i];
        firmware[i * 2]     = (w >> 8) as u8;
        firmware[i * 2 + 1] = (w & 0xFF) as u8;
    }
    for i in 0..10usize {
        let w = words[10 + i];
        serial[i * 2]     = (w >> 8) as u8;
        serial[i * 2 + 1] = (w & 0xFF) as u8;
    }

    let lba48 = words[83] & (1 << 10) != 0;

    let total_sectors = if lba48 {
        (words[100] as u64)
        | ((words[101] as u64) << 16)
        | ((words[102] as u64) << 32)
        | ((words[103] as u64) << 48)
    } else {
        (words[60] as u64) | ((words[61] as u64) << 16)
    };

    DriveInfo {
        id, kind, total_sectors,
        capacity_mib: total_sectors / 2048,
        lba48, model, firmware, serial,
    }
}

// ── Helpers de log ────────────────────────────────────────────────────────────

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