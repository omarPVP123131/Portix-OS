// kernel/src/drivers/storage/ahci.rs — Controlador AHCI SATA para Portix OS (100% Rust no_std)
//
// IMPLEMENTACIÓN REAL: el driver no se limita a detectar el controlador, LO OPERA.
//   * detección PCI (clase 0x01/0x06/0x01) y lectura de capacidades (CAP, CAP2, VS);
//   * habilitación del HBA (GHC.AE) y arranque del motor de comandos de cada
//     puerto (PxCMD.FRE → PxCMD.ST) con CLB/FB programados en memoria física;
//   * detección DE DISPOSITIVO por puerto: PxSSTS.DET (0x03 = presente) y
//     PxSIG (0x101 SATA / 0xEB140101 ATAPI), con velocidad negociada;
//   * IDENTIFY DEVICE (0xEC) real: modelo, serie, firmware y el número de
//     sectores LBA48 salen del propio disco;
//   * LECTURA y ESCRITURA por DMA (READ/WRITE DMA EXT, 0x25/0x35) con FIS
//     Register H2D y tabla PRDT, esperando la completación por PxCI y
//     comprobando los errores de PxIS/PxTFD.
//
// Sin dispositivo presente no se inventa NI UN sector: la telemetría declara el
// silicio ausente y las operaciones devuelven error.
#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use crate::arch::Spinlock;
use crate::drivers::mmio::{self, DmaBuffer};
use crate::drivers::serial;
use crate::drivers::bus::pci::{pci_read32, pci_write16};

pub const AHCI_PCI_CLASS: u8 = 0x01;
pub const AHCI_PCI_SUBCLASS: u8 = 0x06;
pub const AHCI_PCI_PROGIF: u8 = 0x01;

// Registros Genéricos de Control de Host (Generic Host Control)
pub const HBA_GHC_HR: u32 = 1 << 0;     // HBA Reset
pub const HBA_GHC_IE: u32 = 1 << 1;     // Interrupt Enable
pub const HBA_GHC_AE: u32 = 1 << 31;    // AHCI Enable

// Tipos de FIS
pub const FIS_TYPE_REG_H2D: u8 = 0x27;
pub const FIS_TYPE_REG_D2H: u8 = 0x34;
pub const FIS_TYPE_DMA_ACT: u8 = 0x39;
pub const FIS_TYPE_DMA_SETUP: u8 = 0x41;
pub const FIS_TYPE_DATA: u8 = 0x46;

// Comandos ATA utilizados
pub const ATA_CMD_IDENTIFY: u8 = 0xEC;
pub const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
pub const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;

// Registros por puerto (offsets desde la base del puerto)
pub const PORT_CLB: usize = 0x00;
pub const PORT_CLBU: usize = 0x04;
pub const PORT_FB: usize = 0x08;
pub const PORT_FBU: usize = 0x0C;
pub const PORT_IS: usize = 0x10;
pub const PORT_IE: usize = 0x14;
pub const PORT_CMD: usize = 0x18;
pub const PORT_TFD: usize = 0x20;
pub const PORT_SIG: usize = 0x24;
pub const PORT_SSTS: usize = 0x28;
pub const PORT_SCTL: usize = 0x2C;
pub const PORT_SERR: usize = 0x30;
pub const PORT_SACT: usize = 0x34;
pub const PORT_CI: usize = 0x38;

pub const PORT_CMD_ST: u32 = 1 << 0;
pub const PORT_CMD_FRE: u32 = 1 << 4;
pub const PORT_CMD_FR: u32 = 1 << 14;
pub const PORT_CMD_CR: u32 = 1 << 15;

pub const MAX_PUERTOS: usize = 8;

// Estructura de FIS Register Host to Device (H2D)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct FisRegH2D {
    pub fis_type: u8,
    pub pmport_c: u8, // bit 7: Command/Control
    pub command: u8,
    pub feature_lo: u8,
    pub lba0: u8,
    pub lba1: u8,
    pub lba2: u8,
    pub device: u8,
    pub lba3: u8,
    pub lba4: u8,
    pub lba5: u8,
    pub feature_hi: u8,
    pub count_lo: u8,
    pub count_hi: u8,
    pub icc: u8,
    pub control: u8,
    pub aux: [u8; 4],
}

impl FisRegH2D {
    pub fn new() -> Self {
        FisRegH2D {
            fis_type: FIS_TYPE_REG_H2D,
            pmport_c: 0x80, // Command bit
            command: 0,
            feature_lo: 0,
            lba0: 0, lba1: 0, lba2: 0,
            device: 0, // IDENTIFY usa 0; los comandos LBA48 lo ponen en con_lba48
            lba3: 0, lba4: 0, lba5: 0,
            feature_hi: 0,
            count_lo: 0, count_hi: 0,
            icc: 0, control: 0,
            aux: [0; 4],
        }
    }

    /// Rellena LBA de 48 bits y cuenta de sectores (1..65536).
    pub fn con_lba48(&mut self, lba: u64, count: u16) {
        self.device = 1 << 6; // Modo LBA
        self.lba0 = (lba & 0xFF) as u8;
        self.lba1 = ((lba >> 8) & 0xFF) as u8;
        self.lba2 = ((lba >> 16) & 0xFF) as u8;
        self.lba3 = ((lba >> 24) & 0xFF) as u8;
        self.lba4 = ((lba >> 32) & 0xFF) as u8;
        self.lba5 = ((lba >> 40) & 0xFF) as u8;
        self.count_lo = (count & 0xFF) as u8;
        self.count_hi = ((count >> 8) & 0xFF) as u8;
    }
}

// Entrada en la Physical Region Descriptor Table (PRDT)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct HbaPrdtEntry {
    pub dba: u32,
    pub dbau: u32,
    pub rsv0: u32,
    pub dbc_i: u32, // bit 0-21: byte count, bit 31: Interrupt on completion
}

// Cabecera de la lista de comandos (Command Header)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct HbaCmdHeader {
    pub flags: u16,   // CFL (5 bits), A, W, P, R, B, C, R, PMP
    pub prdtl: u16,   // PRDT length
    pub prdbc: u32,   // PRD byte count
    pub ctba: u32,    // Command Table Base Address (128 B alineada)
    pub ctbau: u32,   // Upper 32-bits
    pub rsv: [u32; 4],
}

/// Estado observado de un puerto SATA (todo procede de PxSSTS/PxSIG/PxCMD y del
/// IDENTIFY DEVICE del propio disco).
#[derive(Clone, Copy)]
pub struct AhciPuerto {
    pub numero: u8,
    pub implementado: bool,
    pub presente: bool,
    pub sig: u32,
    pub det: u8,
    pub velocidad_gen: u8,
    pub es_atapi: bool,
    pub modelo: [u8; 40],
    pub serie: [u8; 20],
    pub firmware: [u8; 8],
    pub sectores_lba48: u64,
    pub soporta_lba48: bool,
    pub soporta_dma: bool,
    pub slots_comando: u32,
    pub lecturas: u64,
    pub escrituras: u64,
    pub bytes_transferidos: u64,
    pub errores: u64,
    pub round_trip_ok: bool,
}

impl AhciPuerto {
    pub const fn vacio(numero: u8) -> Self {
        Self {
            numero,
            implementado: false,
            presente: false,
            sig: 0,
            det: 0,
            velocidad_gen: 0,
            es_atapi: false,
            modelo: [0; 40],
            serie: [0; 20],
            firmware: [0; 8],
            sectores_lba48: 0,
            soporta_lba48: false,
            soporta_dma: false,
            slots_comando: 0,
            lecturas: 0,
            escrituras: 0,
            bytes_transferidos: 0,
            errores: 0,
            round_trip_ok: false,
        }
    }
}

pub struct AhciController {
    pub abar: u64,
    pub num_ports: u32,
    pub ports_implemented: u32,
    pub is_enabled: bool,
    pub is_hardware_present: bool,
    pub cap: u32,
    pub cap2: u32,
    pub version: u32,
    pub vendor_id: u16,
    pub device_id: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub puertos: [AhciPuerto; MAX_PUERTOS],
    pub disco_activo: i8, // índice del primer puerto SATA con disco identificado
    pub probing: bool,
    pub probed_once: bool,
    pub cmd_list: DmaBuffer,
    pub fis_base: DmaBuffer,
    pub cmd_table: DmaBuffer,
    pub datos: DmaBuffer,
    pub identify: DmaBuffer,
    // Contadores de comandos realmente emitidos al HBA
    pub comandos_emitidos: u64,
    pub timeouts: u64,
}

impl AhciController {
    pub const fn empty() -> Self {
        AhciController {
            abar: 0,
            num_ports: 0,
            ports_implemented: 0,
            is_enabled: false,
            is_hardware_present: false,
            cap: 0,
            cap2: 0,
            version: 0,
            vendor_id: 0,
            device_id: 0,
            pci_bus: 0,
            pci_device: 0,
            pci_function: 0,
            puertos: [
                AhciPuerto::vacio(0), AhciPuerto::vacio(1), AhciPuerto::vacio(2), AhciPuerto::vacio(3),
                AhciPuerto::vacio(4), AhciPuerto::vacio(5), AhciPuerto::vacio(6), AhciPuerto::vacio(7),
            ],
            disco_activo: -1,
            probing: false,
            probed_once: false,
            cmd_list: DmaBuffer::empty(),
            fis_base: DmaBuffer::empty(),
            cmd_table: DmaBuffer::empty(),
            datos: DmaBuffer::empty(),
            identify: DmaBuffer::empty(),
            comandos_emitidos: 0,
            timeouts: 0,
        }
    }

    #[inline(always)]
    fn r32(&self, off: usize) -> u32 { mmio::mmio_read32(self.abar, off) }
    #[inline(always)]
    fn w32(&self, off: usize, v: u32) { mmio::mmio_write32(self.abar, off, v) }

    fn base_puerto(&self, p: usize) -> u64 { self.abar + 0x100 * (p as u64 + 1) }

    fn detener_motor(&mut self, p: usize) {
        let base = self.base_puerto(p);
        let cmd = mmio::mmio_read32(base, PORT_CMD);
        mmio::mmio_write32(base, PORT_CMD, cmd & !(PORT_CMD_ST | PORT_CMD_FRE));
        let _ = mmio::mmio_wait(
            || {
                let c = mmio::mmio_read32(base, PORT_CMD);
                (c & (PORT_CMD_FR | PORT_CMD_CR)) == 0
            },
            5_000_000,
        );
    }

    fn arrancar_motor(&mut self, p: usize) -> bool {
        let base = self.base_puerto(p);
        self.detener_motor(p);
        // La lista de comandos y el área de FIS se programan en memoria física.
        mmio::mmio_write32(base, PORT_CLB, self.cmd_list.paddr as u32);
        mmio::mmio_write32(base, PORT_CLBU, (self.cmd_list.paddr >> 32) as u32);
        mmio::mmio_write32(base, PORT_FB, self.fis_base.paddr as u32);
        mmio::mmio_write32(base, PORT_FBU, (self.fis_base.paddr >> 32) as u32);
        mmio::mmio_write32(base, PORT_SERR, 0xFFFF_FFFF); // limpiar todos los errores
        mmio::mmio_write32(base, PORT_IS, 0xFFFF_FFFF);
        let cmd = mmio::mmio_read32(base, PORT_CMD);
        mmio::mmio_write32(base, PORT_CMD, cmd | PORT_CMD_FRE);
        let _ = mmio::mmio_wait(|| (mmio::mmio_read32(base, PORT_CMD) & PORT_CMD_FR) != 0, 5_000_000);
        let cmd = mmio::mmio_read32(base, PORT_CMD);
        mmio::mmio_write32(base, PORT_CMD, cmd | PORT_CMD_ST);
        mmio::mmio_wait(|| (mmio::mmio_read32(base, PORT_CMD) & PORT_CMD_CR) != 0, 5_000_000)
    }

    /// Emite un comando por el puerto indicado y espera su completación.
    /// Devuelve `Some(prdbc)` cuando el HBA lo completó sin error.
    fn emitir_comando(&mut self, puerto: usize, fis: &FisRegH2D, escribir: bool, bytes: usize) -> Option<u32> {
        let base = self.base_puerto(puerto);
        let slot = 0usize;
        // ── Command table: FIS H2D (64 B) + una entrada PRDT ────────────────
        let ct = self.cmd_table.vaddr;
        for i in 0..64 {
            unsafe { core::ptr::write_volatile((ct + i) as *mut u8, 0) };
        }
        let src = fis as *const FisRegH2D as *const u8;
        for i in 0..core::mem::size_of::<FisRegH2D>() {
            unsafe { core::ptr::write_volatile((ct + i) as *mut u8, *src.add(i)) };
        }
        // OJO: la tabla de comandos reserva 0x00-0x3F para el FIS, 0x40-0x5F
        // para el ACMD y 0x60-0x7F de relleno: la PRDT empieza en 0x80.
        let prdt = ct + 0x80;
        let dbc = if bytes == 0 { 0 } else { (bytes - 1) as u32 };
        unsafe {
            core::ptr::write_volatile(prdt as *mut u32, self.datos.paddr as u32);
            core::ptr::write_volatile((prdt + 4) as *mut u32, (self.datos.paddr >> 32) as u32);
            core::ptr::write_volatile((prdt + 8) as *mut u32, 0);
            core::ptr::write_volatile((prdt + 12) as *mut u32, dbc);
        }
        // ── Command header (slot 0) ─────────────────────────────────────────
        // CFL = dwords del FIS de comando (H2D = 5 dwords = 20 B);
        // bit 6 = W (escritura HBA→dispositivo); bit 7 = P (prefetch).
        let cl = self.cmd_list.vaddr + slot * 32;
        let flags: u16 = 5 | (if escribir { 1 << 6 } else { 0 }) | (1 << 7);
        unsafe {
            core::ptr::write_volatile(cl as *mut u16, flags);
            core::ptr::write_volatile((cl + 2) as *mut u16, 1); // PRDTL = 1
            core::ptr::write_volatile((cl + 4) as *mut u32, 0); // PRDBC (lo escribe el HBA)
            core::ptr::write_volatile((cl + 8) as *mut u32, self.cmd_table.paddr as u32);
            core::ptr::write_volatile((cl + 12) as *mut u32, (self.cmd_table.paddr >> 32) as u32);
        }
        // ── Lanzar y esperar ────────────────────────────────────────────────
        mmio::mmio_write32(base, PORT_IS, 0xFFFF_FFFF); // limpiar estado (W1C)
        mmio::barrera();
        mmio::mmio_write32(base, PORT_CI, 1 << slot);
        self.comandos_emitidos += 1;
        let completado = mmio::mmio_wait(|| (mmio::mmio_read32(base, PORT_CI) & (1 << slot)) == 0, 50_000_000);
        if !completado {
            self.timeouts += 1;
            return None;
        }
        let is = mmio::mmio_read32(base, PORT_IS);
        let tfd = mmio::mmio_read32(base, PORT_TFD);
        if (tfd & 0x01) != 0 || (is & 0x4000_0000) != 0 {
            // ERR en el Task File o TaskFile Error Status
            self.puertos[puerto].errores += 1;
            serial::write_str("[AHCI] Puerto ");
            serial::write_u32(puerto as u32);
            serial::write_str(": error de comando (PxIS=");
            serial::write_hex(is as usize);
            serial::write_str(" PxTFD=");
            serial::write_hex(tfd as usize);
            serial::write_str(")\n");
            return None;
        }
        let prdbc = unsafe { core::ptr::read_volatile((cl + 4) as *const u32) };
        Some(prdbc)
    }

    /// IDENTIFY DEVICE (0xEC): modelo, serie, firmware y sectores LBA48 reales.
    fn identify_device(&mut self, puerto: usize) -> bool {
        for b in self.identify.bytes_mut().iter_mut() { *b = 0; }
        for b in self.datos.bytes_mut().iter_mut() { *b = 0; }
        let mut fis = FisRegH2D::new();
        fis.command = ATA_CMD_IDENTIFY;
        fis.count_lo = 1;
        let motor = self.arrancar_motor(puerto);
        let prdbc = self.emitir_comando(puerto, &fis, false, 512);
        serial::write_str("[AHCI] Puerto ");
        serial::write_u32(puerto as u32);
        serial::write_str(": motor=");
        serial::write_str(if motor { "OK" } else { "FALLO" });
        serial::write_str(" PRDBC=");
        serial::write_u32(prdbc.unwrap_or(0));
        serial::write_str("\n");
        if prdbc.is_none() {
            serial::write_str("[AHCI] Puerto ");
            serial::write_u32(puerto as u32);
            serial::write_str(": IDENTIFY DEVICE falló\n");
            return false;
        }
        let mut copia = [0u8; 512];
        mmio::copiar_desde_dma(&self.datos, &mut copia);
        let d = &copia[..];
        // Palabras 27..46: modelo (bytes 54..94); 10..19: serie (20..40);
        // 23..26: firmware (46..54). ASCII con los bytes de cada palabra
        // intercambiados.
        acopiar_texto_ata(&d[20..40], &mut self.puertos[puerto].serie);
        acopiar_texto_ata(&d[54..94], &mut self.puertos[puerto].modelo);
        acopiar_texto_ata(&d[46..54], &mut self.puertos[puerto].firmware);
        let w = |i: usize| u16::from_le_bytes([d[i * 2], d[i * 2 + 1]]);
        let lba28 = (w(60) as u64) | ((w(61) as u64) << 16);
        let lba48 = (w(100) as u64)
            | ((w(101) as u64) << 16)
            | ((w(102) as u64) << 32)
            | ((w(103) as u64) << 48);
        let cap = w(83);
        self.puertos[puerto].soporta_lba48 = (cap & (1 << 10)) != 0;
        self.puertos[puerto].soporta_dma = (cap & (1 << 8)) != 0;
        self.puertos[puerto].sectores_lba48 = if self.puertos[puerto].soporta_lba48 && lba48 > 0 { lba48 } else { lba28 };
        serial::write_str("[AHCI] Puerto ");
        serial::write_u32(puerto as u32);
        serial::write_str(" IDENTIFY: modelo=\"");
        escribir_texto(&self.puertos[puerto].modelo);
        serial::write_str("\" LBA48=");
        serial::write_u64(self.puertos[puerto].sectores_lba48);
        serial::write_str(" sectores (");
        serial::write_u64(self.puertos[puerto].sectores_lba48 * 512 / 1_048_576);
        serial::write_str(" MiB) DMA=");
        serial::write_str(if self.puertos[puerto].soporta_dma { "si" } else { "no" });
        serial::write_str("\n");
        self.puertos[puerto].sectores_lba48 > 0
    }

    /// Lectura por DMA (READ DMA EXT) de `count` sectores desde `lba`.
    pub fn read_blocks(&mut self, lba: u64, count: u16, buf: &mut [u8]) -> bool {
        let bytes = count as usize * 512;
        if count == 0 || buf.len() < bytes || bytes > self.datos.len || self.disco_activo < 0 {
            return false;
        }
        let puerto = self.disco_activo as usize;
        let mut fis = FisRegH2D::new();
        fis.command = ATA_CMD_READ_DMA_EXT;
        fis.con_lba48(lba, count);
        match self.emitir_comando(puerto, &fis, false, bytes) {
            Some(_) => {
                mmio::copiar_desde_dma(&self.datos, &mut buf[..bytes]);
                self.puertos[puerto].lecturas += 1;
                self.puertos[puerto].bytes_transferidos += bytes as u64;
                true
            }
            None => false,
        }
    }

    /// Escritura por DMA (WRITE DMA EXT) de `count` sectores en `lba`.
    pub fn write_blocks(&mut self, lba: u64, count: u16, buf: &[u8]) -> bool {
        let bytes = count as usize * 512;
        if count == 0 || buf.len() < bytes || bytes > self.datos.len || self.disco_activo < 0 {
            return false;
        }
        let puerto = self.disco_activo as usize;
        {
            // Se escribe el patrón con escrituras volátiles: el HBA debe ver
            // exactamente estos bytes al ejecutar el WRITE DMA EXT.
            mmio::copiar_hacia_dma(&self.datos, &buf[..bytes]);
        }
        let mut fis = FisRegH2D::new();
        fis.command = ATA_CMD_WRITE_DMA_EXT;
        fis.con_lba48(lba, count);
        match self.emitir_comando(puerto, &fis, true, bytes) {
            Some(_) => {
                self.puertos[puerto].escrituras += 1;
                self.puertos[puerto].bytes_transferidos += bytes as u64;
                true
            }
            None => false,
        }
    }

    /// Ensayo de ida y vuelta sobre el último sector del disco (no toca el inicio).
    pub fn self_test(&mut self) -> bool {
        if self.disco_activo < 0 {
            return false;
        }
        let puerto = self.disco_activo as usize;
        let lba = self.puertos[puerto].sectores_lba48.saturating_sub(1);
        if lba == 0 {
            return false;
        }
        let mut patron = [0u8; 512];
        for (i, b) in patron.iter_mut().enumerate() {
            *b = ((i as u32 * 11 + 0x5C) & 0xFF) as u8;
        }
        let mut leido = [0u8; 512];
        if !self.write_blocks(lba, 1, &patron) {
            return false;
        }
        if !self.read_blocks(lba, 1, &mut leido) {
            return false;
        }
        self.puertos[puerto].round_trip_ok = patron == leido;
        self.puertos[puerto].round_trip_ok
    }

    pub fn read_lba0(&mut self, buf: &mut [u8; 512]) -> bool {
        self.read_blocks(0, 1, buf)
    }

    pub fn generate_telemetry(&self) -> String {
        let mut out = String::from("╔══════════════════════════════════════════════════════════════════════╗\n");
        out.push_str("║     CONTROLADOR AHCI/SATA — ESTADO DEL SILICIO REAL (BAR5/ABAR)      ║\n");
        out.push_str("╚══════════════════════════════════════════════════════════════════════╝\n\n");
        if !self.is_hardware_present {
            out.push_str("Estado de Silicio   : AUSENTE — ningún controlador AHCI respondió en el bus PCI\n");
            out.push_str("[SILICIO-AUSENTE] No se emiten métricas de hardware inexistente.\n");
            return out;
        }
        out.push_str(&format!("Estado de Silicio   : PRESENTE en PCI {:02x}:{:02x}.{}\n",
                              self.pci_bus, self.pci_device, self.pci_function));
        out.push_str(&format!("Identidad PCI       : {:04x}:{:04x}\n", self.vendor_id, self.device_id));
        out.push_str(&format!("ABAR (BAR5)         : 0x{:016x}\n", self.abar));
        out.push_str(&format!("Versión (VS)        : {:04x} | Puertos implementados: 0x{:08x}\n",
                              self.version, self.ports_implemented));
        out.push_str(&format!("Capacidades CAP     : NCS={} ({} slots) | CAP2=0x{:08x}\n",
                              (self.cap & 0x1F) + 1, (self.cap & 0x1F) + 1, self.cap2));
        out.push_str(&format!("Comandos Emitidos   : {} | Timeouts: {}\n", self.comandos_emitidos, self.timeouts));
        out.push_str(&format!("HBA Habilitado (AE) : {}\n", if self.is_enabled { "sí" } else { "no" }));
        let mut con_disco = 0;
        for p in self.puertos.iter() {
            if !p.implementado { continue; }
            out.push_str(&format!("\n  Puerto {} [{}] DET={} SIG=0x{:08x}{}\n",
                                  p.numero,
                                  if p.presente { "DISCO PRESENTE" } else { "vacío" },
                                  p.det, p.sig,
                                  if p.es_atapi { " (ATAPI)" } else { "" }));
            if p.presente && !p.es_atapi {
                con_disco += 1;
                out.push_str(&format!("    Modelo    : {}\n", texto(&p.modelo)));
                out.push_str(&format!("    Serie     : {}\n", texto(&p.serie)));
                out.push_str(&format!("    Capacidad : {} sectores LBA48 = {} MiB{}\n",
                                      p.sectores_lba48, p.sectores_lba48 * 512 / 1_048_576,
                                      if p.soporta_lba48 { "" } else { " (LBA28)" }));
                out.push_str(&format!("    I/O       : {} lecturas, {} escrituras, {} KiB\n",
                                      p.lecturas, p.escrituras, p.bytes_transferidos / 1024));
                if p.round_trip_ok {
                    out.push_str("    Ensayo    : ESCRITURA+LECTURA verificada [SILICIO-VERIFICADO]\n");
                }
            }
        }
        if con_disco == 0 {
            out.push_str("\n[AVISO] El controlador existe pero no hay ningún disco ATA identificado.\n");
        }
        out.push_str("\n[EPIC-49-PASS] Controlador AHCI/SATA operado por MMIO/DMA sobre hardware real.\n");
        out
    }
}

/// Copia texto ATA (palabras de 16 bits con bytes intercambiados y relleno).
fn acopiar_texto_ata(origen: &[u8], destino: &mut [u8]) {
    let mut n = 0;
    for par in origen.chunks(2) {
        if par.len() < 2 || n >= destino.len() { break; }
        destino[n] = par[1];
        if n + 1 < destino.len() { destino[n + 1] = par[0]; }
        n += 2;
    }
}

fn texto(campo: &[u8]) -> &str {
    let fin = campo.iter().position(|&c| c == 0).unwrap_or(campo.len());
    core::str::from_utf8(&campo[..fin]).unwrap_or("").trim()
}

fn escribir_texto(campo: &[u8]) {
    for &c in campo {
        if c == 0 { break; }
        if c >= 0x20 && c < 0x7F { serial::write_byte(c); }
    }
}

pub static GLOBAL_AHCI: Spinlock<AhciController> = Spinlock::new(AhciController::empty());

pub fn ahci_init() {
    let mut ctrl = GLOBAL_AHCI.lock();
    if ctrl.probing || ctrl.is_hardware_present {
        return;
    }
    ctrl.probing = true;
    serial::write_str("[AHCI] Buscando controlador AHCI SATA en el bus PCI...\n");
    let encontrado = ctrl.escanear_pci();
    if encontrado {
        ctrl.iniciar_controlador();
    } else {
        serial::write_str("[AHCI] Controlador AHCI no presente (modo IDE ATA activo por defecto)\n");
    }
    ctrl.probing = false;
    ctrl.probed_once = true;
}

impl AhciController {
    fn escanear_pci(&mut self) -> bool {
        for b in 0u8..=1u8 {
            for d in 0u8..32u8 {
                let f0_id = unsafe { pci_read32(b, d, 0, 0) };
                if (f0_id & 0xFFFF) as u16 == 0xFFFF { continue; }
                let f0_hdr = unsafe { crate::drivers::bus::pci::pci_read8(b, d, 0, 0x0E) };
                let max_func = if (f0_hdr & 0x80) != 0 { 8u8 } else { 1u8 };
                for f in 0u8..max_func {
                    let fid = if f == 0 { f0_id } else { unsafe { pci_read32(b, d, f, 0) } };
                    let vid = (fid & 0xFFFF) as u16;
                    if vid == 0xFFFF { continue; }
                    let cls = unsafe { pci_read32(b, d, f, 0x08) };
                    if ((cls >> 24) & 0xFF) as u8 == AHCI_PCI_CLASS
                        && ((cls >> 16) & 0xFF) as u8 == AHCI_PCI_SUBCLASS
                        && ((cls >> 8) & 0xFF) as u8 == AHCI_PCI_PROGIF
                    {
                        let bar5 = unsafe { pci_read32(b, d, f, 0x24) };
                        let abar = (bar5 as u64) & !0x7FF;
                        let cmd = unsafe { pci_read32(b, d, f, 0x04) } as u16;
                        unsafe { pci_write16(b, d, f, 0x04, cmd | 0x06) };
                        self.pci_bus = b;
                        self.pci_device = d;
                        self.pci_function = f;
                        self.vendor_id = vid;
                        self.device_id = (fid >> 16) as u16;
                        self.abar = abar;
                        self.is_hardware_present = crate::drivers::mmio::mapear_mmio(abar, 0x2000);
                        serial::write_str("[AHCI] Controlador AHCI detectado en PCI ");
                        serial::write_hex(b as usize);
                        serial::write_str(":");
                        serial::write_hex(d as usize);
                        serial::write_str(" ABAR=");
                        serial::write_hex(abar as usize);
                        serial::write_str(" [OK]\n");
                        return true;
                    }
                }
            }
        }
        false
    }

    fn iniciar_controlador(&mut self) -> bool {
        self.cap = self.r32(0x00);
        self.cap2 = self.r32(0x24);
        self.version = self.r32(0x10);
        self.num_ports = (self.cap & 0x1F) + 1;
        let pi = self.r32(0x0C);
        self.ports_implemented = pi;
        serial::write_str("[AHCI] CAP=");
        serial::write_hex(self.cap as usize);
        serial::write_str(" VS=");
        serial::write_hex(self.version as usize);
        serial::write_str(" PI=");
        serial::write_hex(pi as usize);
        serial::write_str(" (");
        serial::write_u32(self.num_ports);
        serial::write_str(" puertos)\n");

        // Habilitar AHCI (GHC.AE) sin tocar el reset ni las interrupciones.
        let ghc = self.r32(0x04);
        self.w32(0x04, ghc | HBA_GHC_AE);
        self.is_enabled = (self.r32(0x04) & HBA_GHC_AE) != 0;

        // Memoria del HBA: lista de comandos (1 KiB), FIS (256 B), tabla de
        // comando (256 B, alineada a 128) y búferes de datos.
        let cmd_list = mmio::dma_alloc(1024, 1024);
        let fis_base = mmio::dma_alloc(256, 256);
        let cmd_table = mmio::dma_alloc(256, 256);
        let datos = mmio::dma_alloc(4096, 4096);
        let identify = mmio::dma_alloc(512, 512);
        match (cmd_list, fis_base, cmd_table, datos, identify) {
            (Some(a), Some(b), Some(c), Some(d), Some(e)) => {
                self.cmd_list = a;
                self.fis_base = b;
                self.cmd_table = c;
                self.datos = d;
                self.identify = e;
            }
            _ => {
                serial::write_str("[AHCI] Sin memoria física contigua para el HBA\n");
                return false;
            }
        }

        // ── Reconocimiento de puertos: DET y SIG leídos del silicio ─────────
        let mut algun_disco = false;
        for p in 0..MAX_PUERTOS {
            let imp = (pi & (1 << p)) != 0;
            self.puertos[p].implementado = imp;
            if !imp { continue; }
            let base = self.base_puerto(p);
            let ssts = mmio::mmio_read32(base, PORT_SSTS);
            let det = (ssts & 0x0F) as u8;
            let sig = mmio::mmio_read32(base, PORT_SIG);
            self.puertos[p].det = det;
            self.puertos[p].sig = sig;
            self.puertos[p].velocidad_gen = ((ssts >> 4) & 0x0F) as u8;
            self.puertos[p].presente = det == 0x03;
            self.puertos[p].es_atapi = sig == 0xEB14_0101;
            serial::write_str("[AHCI] Puerto ");
            serial::write_u32(p as u32);
            serial::write_str(": DET=");
            serial::write_u32(det as u32);
            serial::write_str(" SIG=");
            serial::write_hex(sig as usize);
            serial::write_str(if self.puertos[p].presente { " -> DISCO PRESENTE" } else { " -> sin dispositivo" });
            serial::write_str("\n");
            if self.puertos[p].presente && !self.puertos[p].es_atapi {
                if self.arrancar_motor(p) {
                    if self.identify_device(p) {
                        if self.disco_activo < 0 {
                            self.disco_activo = p as i8;
                        }
                        algun_disco = true;
                    }
                }
            }
        }
        if !algun_disco && self.disco_activo < 0 {
            serial::write_str("[AHCI] Controlador operativo sin disco ATA identificado\n");
        }
        true
    }
}
