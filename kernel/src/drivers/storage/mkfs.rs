// kernel/src/drivers/storage/mkfs.rs — PORTIX FAT32 formatter
//
// Formatea el disco primario con:
//   • MBR + tabla de partición tipo 0x0B (FAT32 CHS)
//   • VBR FAT32 (BPB correcta, firma 0xAA55)
//   • FAT1 + FAT2 inicializadas (cluster 2 = root EOC)
//   • Directorio raíz vacío
//   • Árbol de directorios del sistema
//
// Llamar desde rust_main() cuando Fat32Volume::mount() falla.

#![allow(dead_code)]

use crate::drivers::serial;
use crate::drivers::storage::ata::AtaDrive;
use crate::drivers::storage::fat32::Fat32Volume;

// ── Parámetros del volumen ────────────────────────────────────────────────────

const BYTES_PER_SEC:  u16 = 512;
const SEC_PER_CLUS:   u8  = 8;       // 4 KiB por cluster
const RESERVED_SECS:  u16 = 32;      // sectores reservados antes de la FAT
const NUM_FATS:       u8  = 2;
const PART_LBA_START: u32 = 2048;    // 1 MiB de alineación — mismo que build.py

// Tamaño de la FAT en sectores para un disco de ~8 MiB
// FAT entries = total_clusters + 2
// Con SEC_PER_CLUS=8 y disco de 8 MiB: ~2000 clusters → 1 sector FAT alcanza
// Usamos 32 sectores por seguridad (estándar mínimo)
const FAT_SIZE_SECS: u32 = 32;

// ── Punto de entrada ──────────────────────────────────────────────────────────

/// Formatea `drive` con FAT32 y crea el árbol de directorios del sistema.
/// Devuelve el cluster raíz si tiene éxito.
pub fn auto_format(drive: AtaDrive) -> Option<u32> {
    serial::log_level(serial::Level::Warn, "MKFS", "Disco sin FAT32 — iniciando formato...");

    // Necesitamos el total de sectores del disco
    let total_secs = drive.info().total_sectors;
    if total_secs < 8192 {
        serial::log_level(serial::Level::Error, "MKFS", "Disco demasiado pequeno (<4 MB)");
        return None;
    }

    // ── 1. Escribir MBR ──────────────────────────────────────────────────────
    if write_mbr(&drive, total_secs).is_err() {
        serial::log_level(serial::Level::Error, "MKFS", "Error escribiendo MBR");
        return None;
    }
    serial::log_level(serial::Level::Ok, "MKFS", "MBR escrito");

    // ── 2. Escribir VBR (BPB FAT32) ──────────────────────────────────────────
    let part_secs = (total_secs as u32).saturating_sub(PART_LBA_START);
    if write_vbr(&drive, part_secs).is_err() {
        serial::log_level(serial::Level::Error, "MKFS", "Error escribiendo VBR");
        return None;
    }
    serial::log_level(serial::Level::Ok, "MKFS", "VBR FAT32 escrito");

    // ── 3. Inicializar FAT1 + FAT2 ────────────────────────────────────────────
    if init_fat(&drive).is_err() {
        serial::log_level(serial::Level::Error, "MKFS", "Error inicializando FAT");
        return None;
    }
    serial::log_level(serial::Level::Ok, "MKFS", "FAT inicializada");

    // ── 4. Montar y crear árbol de directorios ────────────────────────────────
    // Re-crear AtaDrive desde el mismo DriveInfo (mount consume el drive)
    let info = *drive.info();
    let drive2 = crate::drivers::storage::ata::AtaDrive::from_info(info);

    match Fat32Volume::mount(drive2) {
        Ok(vol) => {
            let root = vol.root_cluster();
            create_dir_tree(&vol, root);
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

// ── Escritura de sectores base ────────────────────────────────────────────────

fn write_mbr(drive: &AtaDrive, total_secs: u64) -> Result<(), ()> {
    let mut mbr = [0u8; 512];

    // Código de arranque mínimo: JMP $ (loop infinito) + NOP
    mbr[0] = 0xEB; // JMP SHORT
    mbr[1] = 0xFE; // a sí mismo
    mbr[2] = 0x90; // NOP

    // Tabla de particiones — entrada 0: FAT32 CHS (tipo 0x0B)
    let part_size = (total_secs as u32).saturating_sub(PART_LBA_START);

    let off = 0x1BE;
    mbr[off]     = 0x80; // booteable
    mbr[off + 1] = 0x00; // head inicio
    mbr[off + 2] = 0x02; // sector inicio (1-based)
    mbr[off + 3] = 0x00; // cilindro inicio
    mbr[off + 4] = 0x0B; // tipo: FAT32 CHS

    // CHS fin (aproximado — suficiente para BIOS moderno que ignora CHS)
    mbr[off + 5] = 0xFE;
    mbr[off + 6] = 0xFF;
    mbr[off + 7] = 0xFF;

    // LBA inicio y tamaño (little-endian u32)
    mbr[off + 8..off + 12].copy_from_slice(&PART_LBA_START.to_le_bytes());
    mbr[off + 12..off + 16].copy_from_slice(&part_size.to_le_bytes());

    // Firma MBR
    mbr[0x1FE] = 0x55;
    mbr[0x1FF] = 0xAA;

    drive.write_sectors(0, 1, &mbr).map_err(|_| ())
}

fn write_vbr(drive: &AtaDrive, part_secs: u32) -> Result<(), ()> {
    let mut vbr = [0u8; 512];

    // JMP SHORT + NOP (BPB requerido)
    vbr[0] = 0xEB;
    vbr[1] = 0x58; // salta sobre el BPB (a offset 0x5A)
    vbr[2] = 0x90;

    // OEM Name
    vbr[3..11].copy_from_slice(b"PORTIX  ");

    // BPB — campos básicos
    vbr[11..13].copy_from_slice(&BYTES_PER_SEC.to_le_bytes());
    vbr[13] = SEC_PER_CLUS;
    vbr[14..16].copy_from_slice(&RESERVED_SECS.to_le_bytes());
    vbr[16] = NUM_FATS;
    vbr[17..19].copy_from_slice(&0u16.to_le_bytes()); // root entries = 0 (FAT32)
    vbr[19..21].copy_from_slice(&0u16.to_le_bytes()); // total16 = 0
    vbr[21] = 0xF8; // media: disco fijo
    vbr[22..24].copy_from_slice(&0u16.to_le_bytes()); // fat16 = 0
    vbr[24..26].copy_from_slice(&63u16.to_le_bytes()); // sectors per track
    vbr[26..28].copy_from_slice(&255u16.to_le_bytes()); // heads
    vbr[28..32].copy_from_slice(&PART_LBA_START.to_le_bytes()); // hidden sectors

    // total32: sectores de la partición
    vbr[32..36].copy_from_slice(&part_secs.to_le_bytes());

    // FAT32 extended BPB
    vbr[36..40].copy_from_slice(&FAT_SIZE_SECS.to_le_bytes()); // fat_size32
    vbr[40..42].copy_from_slice(&0u16.to_le_bytes());   // ext_flags
    vbr[42..44].copy_from_slice(&0u16.to_le_bytes());   // fs_version = 0.0
    vbr[44..48].copy_from_slice(&2u32.to_le_bytes());   // root_cluster = 2
    vbr[48..50].copy_from_slice(&1u16.to_le_bytes());   // fs_info sector
    vbr[50..52].copy_from_slice(&6u16.to_le_bytes());   // backup boot sector
    // bytes 52-63: reserved (cero)
    vbr[64] = 0x80; // drive number
    vbr[65] = 0x00; // reserved1
    vbr[66] = 0x29; // boot signature
    vbr[67..71].copy_from_slice(&0x50525458u32.to_le_bytes()); // volume ID "PRTX"
    vbr[71..82].copy_from_slice(b"PORTIX     "); // volume label (11 bytes)
    vbr[82..90].copy_from_slice(b"FAT32   ");    // fs type

    // Firma de arranque
    vbr[510] = 0x55;
    vbr[511] = 0xAA;

    drive.write_sectors(PART_LBA_START as u64, 1, &vbr).map_err(|_| ())
}

fn init_fat(drive: &AtaDrive) -> Result<(), ()> {
    let fat_lba = PART_LBA_START as u64 + RESERVED_SECS as u64;
    let mut sec = [0u8; 512];

    // Primeras entradas FAT:
    //   [0] = 0x0FFFFFF8 (media byte)
    //   [1] = 0x0FFFFFFF (EOC)
    //   [2] = 0x0FFFFFFF (cluster raíz = EOC)
    sec[0..4].copy_from_slice(&0x0FFFFFF8u32.to_le_bytes());
    sec[4..8].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
    sec[8..12].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());

    // FAT1
    drive.write_sectors(fat_lba, 1, &sec).map_err(|_| ())?;

    // FAT2 (copia)
    let fat2_lba = fat_lba + FAT_SIZE_SECS as u64;
    drive.write_sectors(fat2_lba, 1, &sec).map_err(|_| ())?;

    // Limpiar el directorio raíz (cluster 2)
    let data_start = fat_lba + NUM_FATS as u64 * FAT_SIZE_SECS as u64;
    let root_lba   = data_start; // cluster 2 → offset 0
    let empty = [0u8; 512];
    for s in 0..SEC_PER_CLUS as u64 {
        drive.write_sectors(root_lba + s, 1, &empty).map_err(|_| ())?;
    }

    Ok(())
}

// ── Árbol de directorios ──────────────────────────────────────────────────────

fn create_dir_tree(vol: &Fat32Volume, root: u32) {
    // Directorios de primer nivel
    let dirs: &[&str] = &["bin", "etc", "home", "tmp", "usr", "var"];

    for name in dirs {
        match vol.create_dir(root, name) {
            Ok(_)  => {
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

    // /home/user
    if let Ok(home) = vol.find_entry(root, "home") {
        match vol.create_dir(home.cluster, "user") {
            Ok(_)  => serial::log_level(serial::Level::Ok, "MKFS", "mkdir /home/user"),
            Err(_) => serial::log_level(serial::Level::Warn, "MKFS", "mkdir /home/user fallo"),
        }
    }

    // Archivo de bienvenida en /home/user
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