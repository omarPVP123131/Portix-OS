// kernel/src/drivers/storage/mkfs.rs — PORTIX FAT32 formatter
//
// Formatea el disco primario con:
//   • MBR + tabla de partición tipo 0x0B (FAT32 CHS)
//   • VBR FAT32 (BPB correcta, firma 0xAA55)
//   • FAT1 + FAT2 inicializadas (cluster 2 = root EOC)
//   • Directorio raíz vacío
//   • Árbol de directorios del sistema

#![allow(dead_code)]

use crate::drivers::serial;
use crate::drivers::storage::traits::BlockDevice;
use crate::drivers::storage::fat32::Fat32Volume;

// ── Parámetros del volumen ────────────────────────────────────────────────────

const BYTES_PER_SEC:  u16 = 512;
const SEC_PER_CLUS:   u8  = 8;
const RESERVED_SECS:  u16 = 32;
const NUM_FATS:       u8  = 2;
const PART_LBA_START: u32 = 2048;
const FAT_SIZE_SECS:  u32 = 32;

// ── auto_format — formato completo con MBR ────────────────────────────────────

pub fn auto_format(drive: &mut dyn BlockDevice, total_secs: u64) -> Option<u32> {
    serial::log_level(serial::Level::Warn, "MKFS", "Disco sin FAT32 — iniciando formato...");

    if total_secs < 8192 {
        serial::log_level(serial::Level::Error, "MKFS", "Disco demasiado pequeno (<4 MB)");
        return None;
    }

    if write_mbr(drive, total_secs).is_err() {
        serial::log_level(serial::Level::Error, "MKFS", "Error escribiendo MBR");
        return None;
    }
    serial::log_level(serial::Level::Ok, "MKFS", "MBR escrito");

    let part_lba = PART_LBA_START as u64;
    let part_secs = (total_secs as u32).saturating_sub(PART_LBA_START);
    if write_vbr(drive, part_lba, part_secs).is_err() {
        serial::log_level(serial::Level::Error, "MKFS", "Error escribiendo VBR");
        return None;
    }
    serial::log_level(serial::Level::Ok, "MKFS", "VBR FAT32 escrito");

    if init_fat(drive, part_lba).is_err() {
        serial::log_level(serial::Level::Error, "MKFS", "Error inicializando FAT");
        return None;
    }
    serial::log_level(serial::Level::Ok, "MKFS", "FAT inicializada");

    match Fat32Volume::mount(drive) {
        Ok(mut vol) => {
            let root = vol.root_cluster();
            create_dir_tree(&mut vol, root);
            serial::log_level(serial::Level::Ok, "MKFS", "Formato completado — volumen listo");
            Some(root)
        }
        Err(e) => {
            serial::log_level(serial::Level::Error, "MKFS", "No se pudo montar tras formato");
            let _ = e;
            None
        }
    }
}

// ── format_partition — formato en LBA arbitrario (sin MBR) ───────────────────

pub fn format_partition(drive: &mut dyn BlockDevice, part_lba: u64, part_secs: u32) -> Option<()> {
    if write_vbr(drive, part_lba, part_secs).is_err() {
        return None;
    }
    if init_fat(drive, part_lba).is_err() {
        return None;
    }
    Some(())
}

// ── MBR ──────────────────────────────────────────────────────────────────────

fn write_mbr(drive: &mut dyn BlockDevice, total_secs: u64) -> Result<(), ()> {
    let mut mbr = [0u8; 512];

    // Preserve existing boot code if the disk already has a valid MBR
    // (0x55AA signature). This is CRITICAL: boot.bin lives at LBA 0 and
    // must NOT be overwritten by auto-format, otherwise the OS becomes
    // unbootable after the first reboot.
    let has_valid_mbr = drive.read_sectors(0, 1, &mut mbr).is_ok()
        && mbr[0x1FE] == 0x55
        && mbr[0x1FF] == 0xAA;

    if !has_valid_mbr {
        // No existing boot code — write minimal stub (infinite loop)
        mbr.fill(0);
        mbr[0] = 0xEB;
        mbr[1] = 0xFE;
        mbr[2] = 0x90;
    }

    let part_size = (total_secs as u32).saturating_sub(PART_LBA_START);
    let off = 0x1BE;
    mbr[off]     = 0x80;
    mbr[off + 1] = 0x00;
    mbr[off + 2] = 0x02;
    mbr[off + 3] = 0x00;
    mbr[off + 4] = 0x0B;
    mbr[off + 5] = 0xFE;
    mbr[off + 6] = 0xFF;
    mbr[off + 7] = 0xFF;
    mbr[off + 8..off + 12].copy_from_slice(&PART_LBA_START.to_le_bytes());
    mbr[off + 12..off + 16].copy_from_slice(&part_size.to_le_bytes());

    mbr[0x1FE] = 0x55;
    mbr[0x1FF] = 0xAA;

    drive.write_sectors(0, 1, &mbr).map_err(|_| ())
}

fn write_vbr(drive: &mut dyn BlockDevice, part_lba: u64, part_secs: u32) -> Result<(), ()> {
    let mut vbr = [0u8; 512];

    vbr[0] = 0xEB;
    vbr[1] = 0x58;
    vbr[2] = 0x90;
    vbr[3..11].copy_from_slice(b"PORTIX  ");

    vbr[11..13].copy_from_slice(&BYTES_PER_SEC.to_le_bytes());
    vbr[13] = SEC_PER_CLUS;
    vbr[14..16].copy_from_slice(&RESERVED_SECS.to_le_bytes());
    vbr[16] = NUM_FATS;
    vbr[17..19].copy_from_slice(&0u16.to_le_bytes());
    vbr[19..21].copy_from_slice(&0u16.to_le_bytes());
    vbr[21] = 0xF8;
    vbr[22..24].copy_from_slice(&0u16.to_le_bytes());
    vbr[24..26].copy_from_slice(&63u16.to_le_bytes());
    vbr[26..28].copy_from_slice(&255u16.to_le_bytes());
    vbr[28..32].copy_from_slice(&(part_lba as u32).to_le_bytes());

    vbr[32..36].copy_from_slice(&part_secs.to_le_bytes());
    vbr[36..40].copy_from_slice(&FAT_SIZE_SECS.to_le_bytes());
    vbr[40..42].copy_from_slice(&0u16.to_le_bytes());
    vbr[42..44].copy_from_slice(&0u16.to_le_bytes());
    vbr[44..48].copy_from_slice(&2u32.to_le_bytes());
    vbr[48..50].copy_from_slice(&1u16.to_le_bytes());
    vbr[50..52].copy_from_slice(&6u16.to_le_bytes());
    vbr[64] = 0x80;
    vbr[65] = 0x00;
    vbr[66] = 0x29;
    vbr[67..71].copy_from_slice(&0x50525458u32.to_le_bytes());
    vbr[71..82].copy_from_slice(b"PORTIX     ");
    vbr[82..90].copy_from_slice(b"FAT32   ");

    vbr[510] = 0x55;
    vbr[511] = 0xAA;

    drive.write_sectors(part_lba, 1, &vbr).map_err(|_| ())
}

fn init_fat(drive: &mut dyn BlockDevice, part_lba: u64) -> Result<(), ()> {
    let fat_lba = part_lba + RESERVED_SECS as u64;
    let mut sec = [0u8; 512];

    sec[0..4].copy_from_slice(&0x0FFFFFF8u32.to_le_bytes());
    sec[4..8].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
    sec[8..12].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());

    drive.write_sectors(fat_lba, 1, &sec).map_err(|_| ())?;

    let fat2_lba = fat_lba + FAT_SIZE_SECS as u64;
    drive.write_sectors(fat2_lba, 1, &sec).map_err(|_| ())?;

    let data_start = fat_lba + NUM_FATS as u64 * FAT_SIZE_SECS as u64;
    let root_lba = data_start;
    let empty = [0u8; 512];
    for s in 0..SEC_PER_CLUS as u64 {
        drive.write_sectors(root_lba + s, 1, &empty).map_err(|_| ())?;
    }

    Ok(())
}

fn create_dir_tree(vol: &mut Fat32Volume, root: u32) {
    let dirs: &[&str] = &["bin", "etc", "home", "tmp", "usr", "var"];

    for name in dirs {
        match vol.create_dir(root, name) {
            Ok(_) => {
                serial::write_str("[  OK ] MKFS  mkdir /");
                serial::write_str(name);
                serial::write_byte(b'\n');
            }
            Err(e) => {
                serial::write_str("[ WRN ] MKFS  mkdir /");
                serial::write_str(name);
                serial::write_str(" fallo\n");
                let _ = e;
            }
        }
    }

    if let Ok(home) = vol.find_entry(root, "home") {
        match vol.create_dir(home.cluster, "user") {
            Ok(_) => serial::log_level(serial::Level::Ok, "MKFS", "mkdir /home/user"),
            Err(_) => serial::log_level(serial::Level::Warn, "MKFS", "mkdir /home/user fallo"),
        }
    }

    if let Ok(home) = vol.find_entry(root, "home") {
        if let Ok(user) = vol.find_entry(home.cluster, "user") {
            if let Ok(mut f) = vol.create_file(user.cluster, "README.TXT") {
                let msg = b"Bienvenido a PORTIX\r\nSistema de archivos inicializado.\r\n";
                let _ = vol.write_file(&mut f, msg);
                serial::log_level(serial::Level::Ok, "MKFS", "README.TXT creado en /home/user");
            }
        }
    }
}
