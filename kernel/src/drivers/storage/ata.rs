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
use crate::arch::Spinlock;
use crate::drivers::serial;
use crate::drivers::storage::traits::BlockDevice;

// ── Puertos ATA ────────────────────────────────────────────────────────────────

/// Offsets desde la base del canal
pub(crate) mod reg {
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

pub(crate) mod cmd {
    pub const READ_PIO:        u8 = 0x20;
    pub const READ_PIO_EXT:    u8 = 0x24;
    pub const WRITE_PIO:       u8 = 0x30;
    pub const WRITE_PIO_EXT:   u8 = 0x34;
    pub const CACHE_FLUSH:     u8 = 0xE7;
    pub const CACHE_FLUSH_EXT: u8 = 0xEA;
    pub const IDENTIFY:        u8 = 0xEC;
    pub const IDENTIFY_PACKET: u8 = 0xA1;
    pub const PACKET:          u8 = 0xA0;
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

pub(crate) struct Channel {
    pub(crate) base:    u16,
    pub(crate) control: u16,
}

impl Channel {
    pub(crate) const fn primary()   -> Self { Channel { base: 0x1F0, control: 0x3F6 } }
    pub(crate) const fn secondary() -> Self { Channel { base: 0x170, control: 0x376 } }

    #[inline] pub(crate) unsafe fn inb(&self, r: u16) -> u8 {
        let v: u8;
        core::arch::asm!("in al, dx",
            out("al") v, in("dx") self.base + r,
            options(nostack, nomem));
        v
    }
    #[inline] pub(crate) unsafe fn outb(&self, r: u16, v: u8) {
        core::arch::asm!("out dx, al",
            in("dx") self.base + r, in("al") v,
            options(nostack, nomem));
    }
    #[inline] pub(crate) unsafe fn inw(&self) -> u16 {
        let v: u16;
        core::arch::asm!("in ax, dx",
            out("ax") v, in("dx") self.base + reg::DATA,
            options(nostack, nomem));
        v
    }
    #[inline] pub(crate) unsafe fn outw(&self, v: u16) {
        core::arch::asm!("out dx, ax",
            in("dx") self.base + reg::DATA, in("ax") v,
            options(nostack, nomem));
    }
    #[inline] pub(crate) unsafe fn ctrl_inb(&self) -> u8 {
        let v: u8;
        core::arch::asm!("in al, dx",
            out("al") v, in("dx") self.control,
            options(nostack, nomem));
        v
    }
    #[inline] pub(crate) unsafe fn ctrl_outb(&self, v: u8) {
        core::arch::asm!("out dx, al",
            in("dx") self.control, in("al") v,
            options(nostack, nomem));
    }

    /// 400 ns de espera (4× lectura del registro de control alternativo).
    #[inline] pub(crate) unsafe fn delay400ns(&self) {
        for _ in 0..4 { let _ = self.ctrl_inb(); }
    }

    /// Pausa mínima entre palabras en escritura PIO (≈ "jmp $+2").
    #[inline] pub(crate) unsafe fn tiny_pause(&self) {
        let _ = self.ctrl_inb();
    }

    /// Realiza un soft-reset del canal y desactiva las IRQs (nIEN).
    /// ⚠️  COSTOSO — solo debe llamarse durante AtaBus::scan() en el arranque.
    pub(crate) unsafe fn reset_and_init(&self) {
        self.ctrl_outb(dctl::NIEN | dctl::SRST);
        for _ in 0..25 { let _ = self.ctrl_inb(); }
        self.ctrl_outb(dctl::NIEN);
        for _ in 0..100_000u32 {
            if self.ctrl_inb() & status::BSY == 0 { break; }
        }
    }

    /// [b190] Diagnóstico: qué decía el dispositivo cuando se agotó el plazo.
    pub(crate) unsafe fn volcar_estado_timeout(&self) {
        let st = self.ctrl_inb();
        crate::drivers::serial::write_str("[ATA] TIMEOUT con estado alternativo=");
        crate::drivers::serial::write_hex(st as usize);
        crate::drivers::serial::write_str(" error=");
        crate::drivers::serial::write_hex(self.inb(reg::ERROR) as usize);
        crate::drivers::serial::write_str(" base=");
        crate::drivers::serial::write_hex(self.base as usize);
        crate::drivers::serial::write_str("\n");
    }

    pub(crate) unsafe fn wait_not_busy(&self) -> AtaResult<u8> {
        let mut plazo = PlazoAta::nuevo("BSY=0");
        while plazo.sigue() {
            let st = self.ctrl_inb();
            if st & status::BSY == 0 {
                return Ok(self.inb(reg::STATUS));
            }
        }
        self.volcar_estado_timeout();
        Err(AtaError::Timeout)
    }

    pub(crate) unsafe fn wait_ready(&self) -> AtaResult<()> {
        let mut plazo = PlazoAta::nuevo("RDY");
        while plazo.sigue() {
            let st = self.ctrl_inb();
            if st & status::BSY == 0 && st & status::RDY != 0 {
                let _ = self.inb(reg::STATUS);
                return Ok(());
            }
        }
        self.volcar_estado_timeout();
        Err(AtaError::Timeout)
    }

    pub(crate) unsafe fn wait_drq(&self) -> AtaResult<()> {
        let mut plazo = PlazoAta::nuevo("DRQ");
        while plazo.sigue() {
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
        self.volcar_estado_timeout();
        Err(AtaError::Timeout)
    }

    pub(crate) unsafe fn select_drive(&self, head_val: u8) -> AtaResult<()> {
        self.wait_not_busy()?;
        self.outb(reg::DRIVE_HEAD, head_val);
        self.delay400ns();
        self.wait_ready()?;
        Ok(())
    }

    unsafe fn identify_no_reset(&self, is_slave: bool) -> Option<[u16; 256]> {
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

/// [b189] Plazo de espera del bus ATA basado en TIEMPO, no en iteraciones.
/// Antes eran 1 000 000 lecturas del registro de estado (≈1 s bajo TCG): un
/// disco lento de verdad —o una imagen en un disco del anfitrión saturado de
/// escrituras pendientes— superaba ese presupuesto y la escritura del
/// superbloque fallaba por "timeout" en el arranque (raíz ext2 sin montar, modo
/// degradado, de forma intermitente). Ahora: mínimo el presupuesto antiguo y,
/// agotado, se sigue esperando hasta ~30·10⁹ ciclos de TSC (10 s a 3 GHz).
/// Un dispositivo colgado sigue dando `Timeout`: el plazo es finito.
pub(crate) struct PlazoAta { iter: u32, tsc_ini: u64, etiqueta: &'static str }

impl PlazoAta {
    const ITER_RAPIDAS: u32 = 1_000_000;
    const CICLOS_MAX: u64 = 30_000_000_000;
    #[inline] pub(crate) fn nuevo(etiqueta: &'static str) -> Self { PlazoAta { iter: 0, tsc_ini: 0, etiqueta } }
    #[inline] pub(crate) fn sigue(&mut self) -> bool {
        if self.iter < Self::ITER_RAPIDAS { self.iter += 1; return true; }
        let ahora = unsafe { core::arch::x86_64::_rdtsc() };
        if self.tsc_ini == 0 {
            self.tsc_ini = ahora;
            crate::drivers::serial::write_str("[ATA] el dispositivo tarda mas de lo habitual: se espera hasta 10 s (");
            crate::drivers::serial::write_str(self.etiqueta);
            crate::drivers::serial::write_str(")\n");
            return true;
        }
        ahora.wrapping_sub(self.tsc_ini) < Self::CICLOS_MAX
    }
}

impl Drop for PlazoAta {
    /// Telemetría honesta: cuánto duró de verdad una espera larga (en ms,
    /// con el TSC calibrado si ya lo está; si no, en millones de ciclos).
    fn drop(&mut self) {
        if self.tsc_ini == 0 { return; }
        let ciclos = unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(self.tsc_ini);
        let hz = crate::time::pit::tsc_hz_cacheado();
        crate::drivers::serial::write_str("[ATA] espera larga terminada (");
        crate::drivers::serial::write_str(self.etiqueta);
        crate::drivers::serial::write_str("): ");
        if hz > 0 {
            crate::drivers::serial::write_usize((ciclos / (hz / 1000).max(1)) as usize);
            crate::drivers::serial::write_str(" ms");
        } else {
            crate::drivers::serial::write_usize((ciclos / 1_000_000) as usize);
            crate::drivers::serial::write_str(" Mciclos");
        }
        crate::drivers::serial::write_str(if ciclos < Self::CICLOS_MAX { " (completada)\n" } else { " (TIMEOUT)\n" });
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
        let (chan, is_slave) = resolve_channel(info.id);
        AtaDrive { info, chan, is_slave }
    }

    pub fn info(&self) -> &DriveInfo { &self.info }

    pub fn read_sectors(&mut self, lba: u64, count: usize, buf: &mut [u8]) -> AtaResult<()> {
        self.check(lba, count, buf.len())?;
        if count == 0 { return Ok(()); }
        if self.info.lba48 || lba >= (1 << 28) {
            unsafe { self.read48(lba, count, buf) }
        } else {
            unsafe { self.read28(lba, count, buf) }
        }
    }

    pub fn write_sectors(&mut self, lba: u64, count: usize, buf: &[u8]) -> AtaResult<()> {
        self.check(lba, count, buf.len())?;
        if count == 0 { return Ok(()); }
        if self.info.lba48 || lba >= (1 << 28) {
            unsafe { self.write48(lba, count, buf) }
        } else {
            unsafe { self.write28(lba, count, buf) }
        }
    }

    pub fn flush(&mut self) -> AtaResult<()> {
        unsafe {
            let c = self.chan;
            let head = if self.is_slave { 0xB0u8 } else { 0xA0u8 };
            c.select_drive(head)?;
            c.outb(reg::FEATURES, 0);
            c.outb(reg::COMMAND,
                if self.info.lba48 { cmd::CACHE_FLUSH_EXT } else { cmd::CACHE_FLUSH });
            c.delay400ns();
            let st = c.wait_not_busy()?;
            if st & status::DF != 0 {
                return Err(AtaError::DriveFault);
            }
            if st & status::ERR != 0 {
                return Err(AtaError::DeviceError(c.inb(reg::ERROR)));
            }
            Ok(())
        }
    }

    unsafe fn read28(&mut self, lba: u64, count: usize, buf: &mut [u8]) -> AtaResult<()> {
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
            crate::mem::paging::respirar(); // [T-026] un sector leído: pasan las IRQ pendientes
            let st = c.inb(reg::STATUS);
            if st & status::ERR != 0 {
                return Err(AtaError::DeviceError(c.inb(reg::ERROR)));
            }
            if st & status::DF != 0 {
                return Err(AtaError::DriveFault);
            }
        }
        Ok(())
    }

    unsafe fn write28(&mut self, lba: u64, count: usize, buf: &[u8]) -> AtaResult<()> {
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
            c.delay400ns();
            c.wait_drq()?;
            Self::pio_write_sector(c, buf, s * 512);
            crate::mem::paging::respirar(); // [T-026]
        }
        // [T-012, 89ª] Sin FLUSH CACHE por cada escritura: era un fsync del fichero
        // imagen en el anfitrión por cada bloque de 4 KiB (y por cada copia del
        // superbloque). El vaciado lo ordena quien lo necesita: `fsync`/`sync`,
        // el volcado diferido del ext2 (≤ 1 s) y la vuelta a la consola tras `run`.
        Ok(())
    }

    unsafe fn read48(&mut self, lba: u64, count: usize, buf: &mut [u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave_bit = if self.is_slave { 0x10u8 } else { 0x00u8 };
        for s in 0..count {
            let cur = lba + s as u64;
            c.select_drive(0xE0 | slave_bit)?;
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
            crate::mem::paging::respirar(); // [T-026] un sector leído: pasan las IRQ pendientes
            let st = c.inb(reg::STATUS);
            if st & status::ERR != 0 {
                return Err(AtaError::DeviceError(c.inb(reg::ERROR)));
            }
            if st & status::DF != 0 {
                return Err(AtaError::DriveFault);
            }
        }
        Ok(())
    }

    unsafe fn write48(&mut self, lba: u64, count: usize, buf: &[u8]) -> AtaResult<()> {
        let c = self.chan;
        let slave_bit = if self.is_slave { 0x10u8 } else { 0x00u8 };
        for s in 0..count {
            let cur = lba + s as u64;
            c.select_drive(0xE0 | slave_bit)?;
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
            c.delay400ns();
            c.wait_drq()?;
            Self::pio_write_sector(c, buf, s * 512);
            crate::mem::paging::respirar(); // [T-026]
        }
        // [T-012, 89ª] Sin FLUSH CACHE por cada escritura: era un fsync del fichero
        // imagen en el anfitrión por cada bloque de 4 KiB (y por cada copia del
        // superbloque). El vaciado lo ordena quien lo necesita: `fsync`/`sync`,
        // el volcado diferido del ext2 (≤ 1 s) y la vuelta a la consola tras `run`.
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

impl BlockDevice for AtaDrive {
    fn read_sectors(&mut self, lba: u64, count: usize, buf: &mut [u8]) -> Result<(), AtaError> {
        self.read_sectors(lba, count, buf)
    }

    fn write_sectors(&mut self, lba: u64, count: usize, buf: &[u8]) -> Result<(), AtaError> {
        self.write_sectors(lba, count, buf)
    }

    fn flush_cache(&mut self) -> Result<(), AtaError> {
        self.flush()
    }

    fn total_sectors(&self) -> u64 {
        self.info.total_sectors
    }

    fn device_info(&self) -> DriveInfo {
        self.info
    }
}

// ── Bus ────────────────────────────────────────────────────────────────────────

pub(crate) fn resolve_channel(id: DriveId) -> (&'static Channel, bool) {
    match id {
        DriveId::Primary0   => (&PRIMARY,   false),
        DriveId::Primary1   => (&PRIMARY,   true),
        DriveId::Secondary0 => (&SECONDARY, false),
        DriveId::Secondary1 => (&SECONDARY, true),
    }
}

pub(crate) static PRIMARY:   Channel = Channel::primary();
pub(crate) static SECONDARY: Channel = Channel::secondary();

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

        // Un soft-reset por canal (no por drive) para no matar el bus ATA
        unsafe { PRIMARY.reset_and_init(); }
        unsafe { SECONDARY.reset_and_init(); }

        for (id, chan, is_slave) in slots {
            if let Some(words) = unsafe { chan.identify_no_reset(is_slave) } {
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

static CACHED_DRIVE: Spinlock<CachedDrive> = Spinlock::new(CachedDrive {
    info:  DUMMY_INFO,
    valid: false,
});

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
    let mut guard = CACHED_DRIVE.lock();
    guard.info  = info;
    guard.valid = true;
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
    let guard = CACHED_DRIVE.lock();
    if guard.valid {
        Some(guard.info)
    } else {
        None
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
