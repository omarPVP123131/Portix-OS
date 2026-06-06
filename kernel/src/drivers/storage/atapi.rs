// drivers/storage/atapi.rs — PORTIX Kernel v0.8.1
//
// FIXES vs v0.8.0:
//
//   [FIX-LBA-BYTES-SWAP]  send_packet() tenía LBA_MID y LBA_HI invertidos.
//                         La especificación ATAPI (ATA/ATAPI-6, sección 9.12)
//                         define que LBA_MID (reg 4) recibe el BYTE BAJO del
//                         byte count límite y LBA_HI (reg 5) recibe el BYTE
//                         ALTO. El código anterior los pasaba al revés.
//
//                         Efecto: el drive interpretaba un byte count de
//                         (CD_SECTOR_SIZE >> 8) = 8 en lugar de 2048.
//                         Muchos drives ATAPI reales (y VirtualBox) aceptan
//                         el primer sector con este bug porque truncan el
//                         valor, pero transfers multi-sector o drives más
//                         estrictos fallaban silenciosamente.
//
//   [FIX-ATAPI-NO-READY]  Eliminado wait_ready() entre drive_select y
//                         el envío del PACKET command. Los drives ATAPI
//                         no afirman DRDY inmediatamente tras un select
//                         (al contrario que ATA). Usar wait_ready() causaba
//                         timeout en hardware real y en VirtualBox con discos
//                         virtuales montados como ATAPI.
//
//   [FIX-BCL-ORDER]       El Byte Count Limit se escribe en dos registros:
//                           LBA_MID (reg 4) = BCL & 0xFF   (byte bajo)
//                           LBA_HI  (reg 5) = BCL >> 8     (byte alto)
//                         Ahora correcto y con constantes nombradas para
//                         evitar la confusión futura.
//
//   [FIX-MULTI-SECTOR]    El loop de lectura ahora usa batch de hasta
//                         MAX_ATAPI_BATCH sectores por comando, no 1.
//                         Reduce drásticamente la latencia en lecturas
//                         grandes (kernel loading, ISO9660 tree walks).
//
//   [FIX-STATUS-CHECK]    Después de wait_drq() se comprueba ERR/DF antes
//                         de leer los datos para propagar errores del drive
//                         en lugar de leer basura silenciosamente.

#![allow(dead_code)]

use crate::drivers::storage::ata::{
    AtaError, AtaResult, Channel, DriveInfo,
    cmd, reg,
    resolve_channel,
};
use crate::drivers::storage::traits::BlockDevice;

// ── Constantes ────────────────────────────────────────────────────────────────

pub const CD_SECTOR_SIZE: usize = 2048;

// Máximo de sectores ATAPI por comando READ(10).
// La especificación permite hasta 65535 sectores en el campo Transfer Length
// de un READ(10), pero muchos BIOSes y emuladores limitan la transferencia
// a 16 sectores (32 KiB) por comando. 16 es un valor conservador y seguro.
const MAX_ATAPI_BATCH: usize = 16;

// ── AtapiDrive ────────────────────────────────────────────────────────────────

pub struct AtapiDrive {
    info:     DriveInfo,
    chan:     &'static Channel,
    is_slave: bool,
}

impl AtapiDrive {
    pub fn new(info: DriveInfo) -> Self {
        let (chan, is_slave) = resolve_channel(info.id);
        AtapiDrive { info, chan, is_slave }
    }

    // ── send_packet ───────────────────────────────────────────────────────────
    //
    // Prepara el canal ATAPI y envía un CDB de 12 bytes.
    // El caller debe hacer wait_drq() + leer/escribir datos tras retornar.
    //
    // Registro de byte count límite (BCL):
    //   LBA_MID (reg offset 4) = BCL & 0xFF        ← byte BAJO
    //   LBA_HI  (reg offset 5) = (BCL >> 8) & 0xFF ← byte ALTO
    //
    // Referencia: ATA/ATAPI-6 (T13/1410D), Table 18 — Packet command protocol.
    unsafe fn send_packet(&self, cdb: &[u8; 12], bcl: usize) -> AtaResult<()> {
        let c    = self.chan;
        let head = if self.is_slave { 0xB0u8 } else { 0xA0u8 };

        c.wait_not_busy()?;
        c.outb(reg::DRIVE_HEAD, head);
        c.delay400ns();
        // ATAPI: NO esperar DRDY — los CD-ROM no afirman RDY inmediatamente.

        let bcl_lo = (bcl & 0xFF) as u8;
        let bcl_hi = ((bcl >> 8) & 0xFF) as u8;

        c.outb(reg::FEATURES, 0x00);   // DMA=0, OVL=0 → PIO mode
        c.outb(reg::LBA_MID,  bcl_lo); // Byte Count Limit, byte bajo
        c.outb(reg::LBA_HI,   bcl_hi); // Byte Count Limit, byte alto
        c.outb(reg::COMMAND,  cmd::PACKET);
        c.delay400ns();

        // Esperar DRQ para enviar el CDB
        c.wait_drq()?;

        // Enviar CDB de 12 bytes como 6 palabras de 16 bits (little-endian)
        for i in 0..6usize {
            let word = (cdb[i * 2] as u16) | ((cdb[i * 2 + 1] as u16) << 8);
            c.outw(word);
            c.tiny_pause();
        }
        c.delay400ns();

        Ok(())
    }

    // ── build_read10_cdb ──────────────────────────────────────────────────────
    //
    // Construye un CDB READ(10) para leer `count` sectores a partir de `lba`.
    // `lba` y `count` son sectores ATAPI de 2048 bytes.
    fn build_read10_cdb(lba: u32, count: u16) -> [u8; 12] {
        let lba_bytes   = lba.to_be_bytes();
        let cnt_bytes   = count.to_be_bytes();
        [
            0x28,           // READ(10) opcode
            0x00,           // flags (FUA=0, DPO=0)
            lba_bytes[0], lba_bytes[1], lba_bytes[2], lba_bytes[3],
            0x00,           // grupo (reservado)
            cnt_bytes[0], cnt_bytes[1],
            0x00,           // control
            0x00, 0x00,     // relleno para 12 bytes
        ]
    }
}

// ── BlockDevice impl ──────────────────────────────────────────────────────────

impl BlockDevice for AtapiDrive {
    fn read_sectors(&mut self, lba: u64, count: usize, buf: &mut [u8]) -> Result<(), AtaError> {
        if buf.len() != count * CD_SECTOR_SIZE {
            return Err(AtaError::BadBuffer);
        }
        if count == 0 {
            return Ok(());
        }

        let c             = self.chan;
        let mut remaining = count;
        let mut cur_lba   = lba as u32;
        let mut offset    = 0usize;

        while remaining > 0 {
            let batch = remaining.min(MAX_ATAPI_BATCH) as u16;
            let bcl   = batch as usize * CD_SECTOR_SIZE;
            let cdb   = Self::build_read10_cdb(cur_lba, batch);

            unsafe {
                // Enviar comando
                self.send_packet(&cdb, bcl)?;

                // Leer los sectores del batch
                for _s in 0..batch as usize {
                    // Esperar DRQ para el siguiente sector
                    c.wait_drq()?;

                    // Leer un sector (2048 bytes = 1024 palabras de 16 bits)
                    let words = CD_SECTOR_SIZE / 2;
                    for i in 0..words {
                        let w         = c.inw();
                        let byte_off  = offset + i * 2;
                        buf[byte_off]     = w as u8;
                        buf[byte_off + 1] = (w >> 8) as u8;
                    }
                    c.delay400ns();
                    offset += CD_SECTOR_SIZE;
                }

                // Esperar BSY=0 al final del comando antes del siguiente batch
                c.wait_not_busy()?;
            }

            remaining  -= batch as usize;
            cur_lba    += batch as u32;
        }

        Ok(())
    }

    fn write_sectors(&mut self, _lba: u64, _count: usize, _buf: &[u8]) -> Result<(), AtaError> {
        // Los CD-ROM (ATAPI) son de solo lectura en esta implementación.
        // Para grabación se necesitaría WRITE(10)/WRITE AND VERIFY(10).
        Err(AtaError::DriveFault)
    }

    fn flush_cache(&mut self) -> Result<(), AtaError> {
        // Los CD-ROM no tienen caché de escritura que vaciar.
        Ok(())
    }

    fn total_sectors(&self) -> u64 {
        // ATAPI: READ CAPACITY devuelve el LBA del último sector + 1.
        // Por ahora retornamos 0; iso9660.rs consulta el PVD directamente.
        // TODO: implementar READ CAPACITY (opcode 0x25) para reportar el tamaño real.
        0
    }

    fn device_info(&self) -> DriveInfo {
        self.info
    }
}