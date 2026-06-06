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

// GPT constants for disks > 2 TB
const GPT_PART_LBA_START: u64 = 4096;
const GPT_SIZE_THRESHOLD: u64 = 0xFFFFFFFF;

// ── auto_format — formato completo con MBR ────────────────────────────────────

pub fn auto_format(drive: &mut dyn BlockDevice, total_secs: u64) -> Option<u32> {
    if total_secs < 8192 {
        serial::log_level(serial::Level::Error, "MKFS", "Disco demasiado pequeno (<4 MB)");
        return None;
    }

    let use_gpt = total_secs > GPT_SIZE_THRESHOLD;

    if use_gpt {
        serial::log_level(serial::Level::Warn, "MKFS", "Disco grande (>2TB) — usando tabla GPT...");

        if write_gpt(drive, total_secs).is_err() {
            serial::log_level(serial::Level::Error, "MKFS", "Error escribiendo GPT");
            return None;
        }
        serial::log_level(serial::Level::Ok, "MKFS", "GPT + MBR protector escrito");
    } else {
        serial::log_level(serial::Level::Warn, "MKFS", "Disco sin FAT32 — iniciando formato...");

        if write_mbr(drive, total_secs).is_err() {
            serial::log_level(serial::Level::Error, "MKFS", "Error escribiendo MBR");
            return None;
        }
        serial::log_level(serial::Level::Ok, "MKFS", "MBR escrito");
    }

    let part_lba = if use_gpt { GPT_PART_LBA_START } else { PART_LBA_START as u64 };
    let part_secs = if use_gpt {
        let last_usable = total_secs - 1 - 33;
        (last_usable.saturating_sub(GPT_PART_LBA_START) + 1) as u32
    } else {
        (total_secs as u32).saturating_sub(PART_LBA_START)
    };
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

// ── GPT functions ─────────────────────────────────────────────────────────────

fn write_protective_mbr(drive: &mut dyn BlockDevice) -> Result<(), ()> {
    let mut mbr = [0u8; 512];
    let has_valid = drive.read_sectors(0, 1, &mut mbr).is_ok()
        && mbr[0x1FE] == 0x55 && mbr[0x1FF] == 0xAA;
    if !has_valid {
        mbr.fill(0);
        mbr[0] = 0xEB; mbr[1] = 0xFE; mbr[2] = 0x90;
    }
    let off = 0x1BE;
    mbr[off]     = 0x00;
    mbr[off + 1] = 0x00;
    mbr[off + 2] = 0x02;
    mbr[off + 3] = 0x00;
    mbr[off + 4] = 0xEE;
    mbr[off + 5] = 0xFF;
    mbr[off + 6] = 0xFF;
    mbr[off + 7] = 0xFF;
    mbr[off + 8..off + 12].copy_from_slice(&1u32.to_le_bytes());
    mbr[off + 12..off + 16].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());
    mbr[0x1FE] = 0x55;
    mbr[0x1FF] = 0xAA;
    drive.write_sectors(0, 1, &mbr).map_err(|_| ())
}

fn write_gpt_header_and_entries(drive: &mut dyn BlockDevice, total_secs: u64) -> Result<(), ()> {
    let last_lba = total_secs - 1;
    let part_last_lba = last_lba - 33;

    let mut entries = [0u8; 512 * 32];
    let pe = &mut entries[..128];
    let fat32_guid: [u8; 16] = [0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7];
    pe[..16].copy_from_slice(&fat32_guid);
    let part_guid: [u8; 16] = [0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00];
    pe[16..32].copy_from_slice(&part_guid);
    pe[32..40].copy_from_slice(&GPT_PART_LBA_START.to_le_bytes());
    pe[40..48].copy_from_slice(&part_last_lba.to_le_bytes());
    let name_utf16: &[u16] = &[
        0x0050, 0x006F, 0x0072, 0x0074, 0x0069, 0x0078,
        0x0020, 0x0046, 0x0041, 0x0054, 0x0033, 0x0032,
    ];
    for (i, &c) in name_utf16.iter().enumerate() {
        let off = 56 + i * 2;
        pe[off..off + 2].copy_from_slice(&c.to_le_bytes());
    }

    for s in 0..32u64 {
        let offset = (s * 512) as usize;
        drive.write_sectors(2 + s, 1, &entries[offset..offset + 512]).map_err(|_| ())?;
    }

    let mut hdr = [0u8; 512];
    hdr[..8].copy_from_slice(b"EFI PART");
    hdr[8..12].copy_from_slice(&0x00010000u32.to_le_bytes());
    hdr[12..16].copy_from_slice(&92u32.to_le_bytes());
    hdr[24..32].copy_from_slice(&1u64.to_le_bytes());
    hdr[32..40].copy_from_slice(&last_lba.to_le_bytes());
    hdr[40..48].copy_from_slice(&GPT_PART_LBA_START.to_le_bytes());
    hdr[48..56].copy_from_slice(&part_last_lba.to_le_bytes());
    let disk_guid: [u8; 16] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    hdr[56..72].copy_from_slice(&disk_guid);
    hdr[72..80].copy_from_slice(&2u64.to_le_bytes());
    hdr[80..84].copy_from_slice(&128u32.to_le_bytes());
    hdr[84..88].copy_from_slice(&128u32.to_le_bytes());
    drive.write_sectors(1, 1, &hdr).map_err(|_| ())?;

    for s in 0..32u64 {
        let offset = (s * 512) as usize;
        let backup_lba = last_lba - 32 + s;
        drive.write_sectors(backup_lba, 1, &entries[offset..offset + 512]).map_err(|_| ())?;
    }

    hdr[24..32].copy_from_slice(&last_lba.to_le_bytes());
    hdr[32..40].copy_from_slice(&1u64.to_le_bytes());
    drive.write_sectors(last_lba, 1, &hdr).map_err(|_| ())?;

    Ok(())
}

fn write_gpt(drive: &mut dyn BlockDevice, total_secs: u64) -> Result<(), ()> {
    write_protective_mbr(drive)?;
    write_gpt_header_and_entries(drive, total_secs)
}

// ── create_dir_tree ───────────────────────────────────────────────────────────

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
