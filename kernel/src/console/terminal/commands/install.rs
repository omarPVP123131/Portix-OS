// console/terminal/commands/install.rs -- PORTIX v0.8.1
//
// FIXES vs v0.8.0:
//
//   [FIX-KERNEL-ALIGN]  get_kernel_info() alinea kernel_size al multiplo de
//                       512 superior. Sin esto el ultimo sector parcial era
//                       truncado y el kernel en disco quedaba incompleto.
//
//   [FIX-SECTOR-WRITE]  write_sectors_safe() acepta &[u8] de cualquier
//                       tamano y maneja el ultimo sector parcial con un
//                       buffer intermedio de 512 bytes con relleno de ceros.
//
//   [FIX-PART-ALIGN]    part_lba se alinea a multiplo de 2048 sectores
//                       (1 MiB). Mejora compatibilidad con fdisk/parted y
//                       rendimiento en drives modernos.
//
//   [FIX-STAGE2-SIZE]   La comprobacion de stage2.bin acepta cualquier
//                       multiplo de 512 hasta STAGE2_SECTORS*512, no solo
//                       el tamano exacto.
//
//   [FIX-CANARY]        Sector de verificacion post-escritura con magic
//                       conocido para detectar errores silenciosos del ATA.
//
//   [FIX-TYPES]         STAGE2_SECTORS como usize para evitar casts u64->u32
//                       en append_u32(). Eliminado kernel_aligned sin uso.

use crate::console::terminal::{Terminal, LineColor, TERM_COLS};
use crate::console::terminal::fmt::*;
use crate::drivers::storage::ata::DriveType;
use crate::drivers::storage::fat32::Fat32Volume;
use crate::drivers::storage::mkfs;
use crate::drivers::storage::registry;
use alloc::boxed::Box;
use crate::drivers::storage::traits::BlockDevice;

// -- Layout del area de arranque ----------------------------------------------
//
//   LBA 0          : MBR (boot.bin, 512 bytes)
//   LBA 1..64      : stage2.bin (64 sectores x 512 B = 32 KiB)
//   LBA 68..67+N   : kernel.bin (N sectores x 512 B, alineado a 512)
//   LBA part_lba.. : Particion FAT32 (alineada a 2048 sectores = 1 MiB)
//
// KERNEL_LBA_START debe coincidir con %define KERNEL_LBA en stage2.asm.
// STAGE2_SECTORS   debe coincidir con STAGE2_SECTORS en boot.asm/stage2.asm.

// usize para evitar casts innecesarios en append_u32 y aritmetica de indices.
const STAGE2_SECTORS:   usize = 64;
const BOOT_BIN_SIZE:    usize = 512;
const KERNEL_LBA_START: u64   = 68;
const KERNEL_PHYS_ADDR: usize = 0x0020_0000;

// Alineacion de la particion FAT32: 2048 sectores = 1 MiB con sectores 512 B.
const PART_ALIGN_SECS: u64 = 2048;

// Magic de verificacion post-escritura: "PORTIXOK" en little-endian ASCII.
const WRITE_CANARY_MAGIC: u64 = 0x4B4F5849_54524F50;

static BOOT_BIN_DATA:   &[u8] = include_bytes!("../../../../../build/boot.bin");
static STAGE2_BIN_DATA: &[u8] = include_bytes!("../../../../../build/stage2.bin");

extern "C" {
    static __kernel_end: u8;
}

// -- get_kernel_info ----------------------------------------------------------
//
// Devuelve (puntero, tamano_raw_bytes, tamano_alineado_a_512, sectores).
// El puntero apunta al kernel en RAM fisica. El tamano alineado es el que
// se debe usar para calcular el layout del disco y los sectores a escribir.
fn get_kernel_info() -> (*const u8, usize, usize) {
    let kernel_start    = KERNEL_PHYS_ADDR;
    let kernel_end_ptr  = core::ptr::addr_of!(__kernel_end) as usize;
    let raw_size        = kernel_end_ptr.saturating_sub(kernel_start);

    // Alinear al multiplo de 512 superior (o igual si ya es multiplo).
    // Ejemplo: raw=33000 -> aligned=33280 (65 sectores).
    let aligned_size    = (raw_size + 511) & !511;

    let ptr = kernel_start as *const u8;
    (ptr, raw_size, aligned_size)
}

// -- write_sectors_safe -------------------------------------------------------
//
// Escribe `data` al disco a partir de `lba`. Acepta cualquier longitud:
//   - Sectores completos: escritos directamente desde `data`.
//   - Ultimo sector parcial: copiado a un buffer de 512 B con relleno de
//     ceros y escrito como sector completo.
//
// Retorna el numero de sectores escritos, o Err(()) si fallo el disco.
fn write_sectors_safe(drive: &mut dyn BlockDevice, lba: u64, data: &[u8]) -> Result<usize, ()> {
    if data.is_empty() {
        return Ok(0);
    }

    let full_sectors    = data.len() / 512;
    let remainder       = data.len() % 512;
    let mut written     = 0usize;

    if full_sectors > 0 {
        drive
            .write_sectors(lba, full_sectors, &data[..full_sectors * 512])
            .map_err(|_| ())?;
        written += full_sectors;
    }

    if remainder > 0 {
        let mut tail = [0u8; 512];
        tail[..remainder].copy_from_slice(&data[full_sectors * 512..]);
        drive
            .write_sectors(lba + full_sectors as u64, 1, &tail)
            .map_err(|_| ())?;
        written += 1;
    }

    Ok(written)
}

// -- verify_write_canary ------------------------------------------------------
//
// Escribe un sector con un magic conocido en `lba` y lo relee para
// confirmar que el driver ATA funciona correctamente antes de comprometer
// datos reales. Devuelve false si la escritura o la verificacion fallan.
fn verify_write_canary(drive: &mut dyn BlockDevice, lba: u64) -> bool {
    let mut canary = [0u8; 512];
    canary[0..8].copy_from_slice(&WRITE_CANARY_MAGIC.to_le_bytes());
    canary[504..512].copy_from_slice(&WRITE_CANARY_MAGIC.to_le_bytes());

    if drive.write_sectors(lba, 1, &canary).is_err() {
        return false;
    }

    let mut verify = [0u8; 512];
    if drive.read_sectors(lba, 1, &mut verify).is_err() {
        return false;
    }

    verify[0..8]     == canary[0..8] &&
    verify[504..512] == canary[504..512]
}

// -- find_hdd_target ----------------------------------------------------------
fn find_hdd_target(requested: Option<usize>) -> Result<usize, &'static str> {
    if let Some(idx) = requested {
        let valid = registry::with_device(idx, |d| {
            let dev = d.ok_or("El dispositivo especificado no existe.")?;
            if dev.device_info().kind == DriveType::Atapi {
                return Err("El dispositivo es un CD-ROM (ATAPI). El destino debe ser ATA.");
            }
            if dev.total_sectors() < 8192 {
                return Err("El dispositivo tiene menos de 4 MiB de capacidad.");
            }
            Ok(())
        });
        match valid {
            Ok(()) => return Ok(idx),
            Err(e) => return Err(e),
        }
    }

    let count = registry::device_count();
    for id in 0..count {
        let valid = registry::with_device(id, |d| {
            match d {
                Some(dev) if dev.device_info().kind != DriveType::Atapi && dev.total_sectors() > 8192 => true,
                _ => false,
            }
        });
        if valid { return Ok(id); }
    }
    Err("No se encontro un disco duro ATA valido para instalar.")
}

// -- show_devices -------------------------------------------------------------
fn show_devices(t: &mut Terminal) {
    t.write_line("  Dispositivos disponibles:", LineColor::Info);
    let count = registry::device_count();
    for i in 0..count {
        let info = registry::with_device(i, |d| d.map(|d| d.device_info()));
        if let Some(info) = info {
            let kind_s = if info.kind == DriveType::Atapi { b"CD-ROM " } else { b"DISCO  " };
            let mut buf = [0u8; TERM_COLS];
            let mut pos = 0;
            append_str(&mut buf, &mut pos, b"    ");
            append_u32(&mut buf, &mut pos, i as u32);
            append_str(&mut buf, &mut pos, b"  ");
            buf[pos..pos + kind_s.len()].copy_from_slice(kind_s);
            pos += kind_s.len();
            append_mib(&mut buf, &mut pos, info.capacity_mib);
            append_str(&mut buf, &mut pos, b"  ");
            let m  = info.model_str().as_bytes();
            let ml = m.len().min(TERM_COLS - pos);
            buf[pos..pos + ml].copy_from_slice(&m[..ml]);
            pos += ml;
            t.write_bytes(&buf[..pos], LineColor::Normal);
        }
    }
}

// -- show_size ----------------------------------------------------------------
fn show_size(t: &mut Terminal, label: &str, bytes: usize) {
    let mut buf = [0u8; TERM_COLS];
    let mut p   = 0;
    append_str(&mut buf, &mut p, label.as_bytes());
    append_str(&mut buf, &mut p, b": ");
    append_u32(&mut buf, &mut p, bytes as u32);
    append_str(&mut buf, &mut p, b" bytes (");
    append_u32(&mut buf, &mut p, ((bytes + 511) / 512) as u32);
    append_str(&mut buf, &mut p, b" sectores)");
    t.write_bytes(&buf[..p], LineColor::Normal);
}

// -- cmd_install --------------------------------------------------------------
pub fn cmd_install(t: &mut Terminal, args: &[u8]) {
    t.separador("INSTALACION - PORTIX EN DISCO DURO");

    // Parsear argumento opcional: indice del dispositivo destino
    let requested_hdd = {
        let a = trim(args);
        if a.is_empty() {
            None
        } else {
            match parse_u64(a) {
                Some(n) => Some(n as usize),
                None => {
                    t.write_line(
                        "  Error: argumento invalido. Uso: install [device]",
                        LineColor::Error,
                    );
                    t.write_line("    install       Primer disco ATA disponible", LineColor::Normal);
                    t.write_line("    install 1     Dispositivo 1", LineColor::Normal);
                    show_devices(t);
                    t.write_empty();
                    return;
                }
            }
        }
    };

    // [1/6] Seleccionar disco destino
    t.write_line("  [1/6] Buscando disco de destino...", LineColor::Info);
    let hdd_id = match find_hdd_target(requested_hdd) {
        Ok(id) => id,
        Err(msg) => {
            t.write_line("  Error:", LineColor::Error);
            t.write_line(msg, LineColor::Error);
            t.write_empty();
            show_devices(t);
            t.write_empty();
            return;
        }
    };

    let Some(boxed_hdd) = registry::take_device(hdd_id) else {
        t.write_line("  Error: no se pudo acceder al disco de destino.", LineColor::Error);
        return;
    };
    let hdd: &'static mut dyn BlockDevice = Box::leak(boxed_hdd);
    let total_secs = hdd.total_sectors();

    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Destino: device ");
        append_u32(&mut buf, &mut pos, hdd_id as u32);
        append_str(&mut buf, &mut pos, b" - ");
        append_u32(&mut buf, &mut pos, (total_secs / 2048) as u32);
        append_str(&mut buf, &mut pos, b" MiB (");
        append_u32(&mut buf, &mut pos, total_secs as u32);
        append_str(&mut buf, &mut pos, b" sectores)");
        t.write_bytes(&buf[..pos], LineColor::Success);
    }

    // [2/6] Validar componentes embebidos
    t.write_line("  [2/6] Validando componentes de arranque...", LineColor::Info);

    let boot_bin              = BOOT_BIN_DATA;
    let stage2                = STAGE2_BIN_DATA;
    let (kernel_ptr, kernel_raw, kernel_aligned) = get_kernel_info();
    let kernel_secs           = kernel_aligned / 512;

    // boot.bin: exactamente 512 bytes con firma 0x55AA
    if boot_bin.len() != BOOT_BIN_SIZE {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"  Error: boot.bin tiene ");
        append_u32(&mut b, &mut p, boot_bin.len() as u32);
        append_str(&mut b, &mut p, b" bytes, se esperan 512.");
        t.write_bytes(&b[..p], LineColor::Error);
        return;
    }
    if boot_bin[510] != 0x55 || boot_bin[511] != 0xAA {
        t.write_line("  Error: boot.bin no tiene firma 0x55AA.", LineColor::Error);
        return;
    }

    // stage2.bin: multiplo de 512, entre 1 y STAGE2_SECTORS sectores
    let stage2_max = STAGE2_SECTORS * 512;
    if stage2.is_empty() || stage2.len() % 512 != 0 || stage2.len() > stage2_max {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"  Error: stage2.bin tamano invalido (");
        append_u32(&mut b, &mut p, stage2.len() as u32);
        append_str(&mut b, &mut p, b" bytes, max ");
        append_u32(&mut b, &mut p, stage2_max as u32);
        append_str(&mut b, &mut p, b").");
        t.write_bytes(&b[..p], LineColor::Error);
        return;
    }

    if kernel_raw == 0 {
        t.write_line("  Error: kernel_size es cero.", LineColor::Error);
        return;
    }

    t.write_line("  Componentes:", LineColor::Info);
    show_size(t, "    boot.bin",   BOOT_BIN_SIZE);
    show_size(t, "    stage2.bin", stage2.len());
    show_size(t, "    kernel.bin", kernel_raw);
    {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"    kernel en disco: ");
        append_u32(&mut b, &mut p, kernel_aligned as u32);
        append_str(&mut b, &mut p, b" bytes (");
        append_u32(&mut b, &mut p, kernel_secs as u32);
        append_str(&mut b, &mut p, b" sectores)");
        t.write_bytes(&b[..p], LineColor::Normal);
    }

    // Calcular layout del disco:
    //   boot_end  = primer sector libre tras el kernel
    //   part_lba  = boot_end redondeado al proximo multiplo de PART_ALIGN_SECS
    //   part_secs = sectores disponibles para FAT32
    let boot_end   = KERNEL_LBA_START + kernel_secs as u64;
    let part_lba   = ((boot_end + PART_ALIGN_SECS - 1) / PART_ALIGN_SECS) * PART_ALIGN_SECS;
    let part_secs  = total_secs.saturating_sub(part_lba);

    if part_secs < 8192 {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"  Error: espacio FAT32 insuficiente (");
        append_u32(&mut b, &mut p, part_secs as u32);
        append_str(&mut b, &mut p, b" sectores, 8192 minimo).");
        t.write_bytes(&b[..p], LineColor::Error);
        return;
    }

    {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"  Layout: boot LBA 0-");
        append_u32(&mut b, &mut p, (boot_end - 1) as u32);
        append_str(&mut b, &mut p, b"  FAT32 LBA ");
        append_u32(&mut b, &mut p, part_lba as u32);
        append_str(&mut b, &mut p, b" (");
        append_u32(&mut b, &mut p, part_secs as u32);
        append_str(&mut b, &mut p, b" sectores)");
        t.write_bytes(&b[..p], LineColor::Normal);
    }

    // [3/6] Verificar escritura con canary
    t.write_line("  [3/6] Verificando escritura en disco...", LineColor::Info);

    // Usar boot_end como LBA temporal de canary (sera sobreescrito por FAT32).
    if !verify_write_canary(hdd, boot_end) {
        t.write_line(
            "  Error: verificacion de escritura fallo. Revisa el disco.",
            LineColor::Error,
        );
        return;
    }
    t.write_line("    Escritura verificada OK.", LineColor::Normal);

    // [4/6] Escribir area de arranque
    t.write_line("  [4/6] Escribiendo bootloader...", LineColor::Info);

    // MBR: boot.bin con la tabla de particiones parcheada.
    // boot.bin ocupa bytes 0x000-0x1BD; la tabla va en 0x1BE-0x1FD.
    let mut patched_mbr = [0u8; 512];
    patched_mbr[..BOOT_BIN_SIZE].copy_from_slice(boot_bin);

    let poff  = 0x1BE;
    let ps32  = part_secs as u32;
    let pl32  = part_lba  as u32;
    patched_mbr[poff]                 = 0x80; // booteable
    patched_mbr[poff + 1]             = 0x00;
    patched_mbr[poff + 2]             = 0x02;
    patched_mbr[poff + 3]             = 0x00;
    patched_mbr[poff + 4]             = 0x0C; // tipo: FAT32 con LBA
    patched_mbr[poff + 5]             = 0xFE;
    patched_mbr[poff + 6]             = 0xFF;
    patched_mbr[poff + 7]             = 0xFF;
    patched_mbr[poff +  8..poff + 12].copy_from_slice(&pl32.to_le_bytes());
    patched_mbr[poff + 12..poff + 16].copy_from_slice(&ps32.to_le_bytes());

    if hdd.write_sectors(0, 1, &patched_mbr).is_err() {
        t.write_line("    Error escribiendo MBR.", LineColor::Error);
        return;
    }
    t.write_line("    MBR escrito en LBA 0.", LineColor::Normal);

    // stage2: siempre ocupa exactamente STAGE2_SECTORS en disco.
    // Si stage2.bin es menor, se rellenan los sectores restantes con ceros
    // para no dejar basura de instalaciones previas.
    {
        let real_secs = stage2.len() / 512;
        if hdd.write_sectors(1, real_secs, stage2).is_err() {
            t.write_line("    Error escribiendo stage2.", LineColor::Error);
            return;
        }
        let pad_secs = STAGE2_SECTORS - real_secs;
        if pad_secs > 0 {
            let zero = [0u8; 512];
            for s in 0..pad_secs {
                let lba = 1 + real_secs as u64 + s as u64;
                if hdd.write_sectors(lba, 1, &zero).is_err() {
                    t.write_line("    Error borrando relleno de stage2.", LineColor::Error);
                    return;
                }
            }
        }
    }
    {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"    stage2.bin escrito en LBA 1-");
        append_u32(&mut b, &mut p, STAGE2_SECTORS as u32);
        t.write_bytes(&b[..p], LineColor::Normal);
    }

    // Kernel: write_sectors_safe maneja alineacion del ultimo sector parcial.
    {
        let kernel_data = unsafe {
            core::slice::from_raw_parts(kernel_ptr, kernel_raw)
        };
        match write_sectors_safe(hdd, KERNEL_LBA_START, kernel_data) {
            Ok(secs_written) => {
                let mut b = [0u8; TERM_COLS]; let mut p = 0;
                append_str(&mut b, &mut p, b"    kernel escrito en LBA ");
                append_u32(&mut b, &mut p, KERNEL_LBA_START as u32);
                append_str(&mut b, &mut p, b"-");
                append_u32(&mut b, &mut p, (KERNEL_LBA_START + secs_written as u64 - 1) as u32);
                t.write_bytes(&b[..p], LineColor::Normal);
            }
            Err(_) => {
                t.write_line("    Error escribiendo kernel.", LineColor::Error);
                return;
            }
        }
    }

    t.write_line("  Bootloader + kernel escritos.", LineColor::Success);

    // [5/6] Formatear particion FAT32
    t.write_line("  [5/6] Formateando particion FAT32...", LineColor::Info);

    if mkfs::format_partition(hdd, part_lba, part_secs as u32).is_none() {
        t.write_line("    Error formateando FAT32.", LineColor::Error);
        return;
    }
    t.write_line("    Particion FAT32 formateada.", LineColor::Success);

    // [6/6] Montar, crear arbol de directorios y copiar kernel a FAT32
    t.write_line("  [6/6] Montando sistema de archivos...", LineColor::Info);

    let mut vol = match Fat32Volume::mount(hdd) {
        Ok(v) => v,
        Err(_) => {
            t.write_line("    Error montando FAT32.", LineColor::Error);
            return;
        }
    };
    let root = vol.root_cluster();

    let dirs: &[&str] = &["bin", "etc", "home", "tmp", "usr", "var", "dev", "proc"];
    for name in dirs {
        let _ = vol.create_dir(root, name);
    }
    if let Ok(home) = vol.find_entry(root, "home") {
        let _ = vol.create_dir(home.cluster, "user");
    }

    // /bin/kernel: copia de referencia del kernel en el sistema de archivos
    let kernel_data = unsafe {
        core::slice::from_raw_parts(kernel_ptr, kernel_raw)
    };
    let bin_dir = match vol.find_entry(root, "bin") {
        Ok(e) if e.is_dir => e,
        _ => {
            t.write_line("    Error: no se encontro /bin.", LineColor::Error);
            return;
        }
    };
    if let Ok(old) = vol.find_entry(bin_dir.cluster, "kernel") {
        let _ = vol.delete_entry(&old);
    }
    let mut kernel_entry = match vol.create_file(bin_dir.cluster, "kernel") {
        Ok(e) => e,
        Err(_) => {
            t.write_line("    Error creando /bin/kernel.", LineColor::Error);
            return;
        }
    };
    if vol.write_file(&mut kernel_entry, kernel_data).is_err() {
        t.write_line("    Error escribiendo /bin/kernel.", LineColor::Error);
        return;
    }

    if let Ok(home) = vol.find_entry(root, "home") {
        if let Ok(user) = vol.find_entry(home.cluster, "user") {
            if let Ok(mut f) = vol.create_file(user.cluster, "README.TXT") {
                let msg = b"Bienvenido a PORTIX\r\nSistema instalado en disco duro.\r\n";
                let _ = vol.write_file(&mut f, msg);
            }
        }
    }

    // Resumen final
    t.write_empty();
    t.write_line("  INSTALACION COMPLETADA", LineColor::Success);
    {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"  Dispositivo: device ");
        append_u32(&mut b, &mut p, hdd_id as u32);
        t.write_bytes(&b[..p], LineColor::Normal);
    }
    {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"  Boot area: LBA 0-");
        append_u32(&mut b, &mut p, (boot_end - 1) as u32);
        t.write_bytes(&b[..p], LineColor::Normal);
    }
    {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"  FAT32: LBA ");
        append_u32(&mut b, &mut p, part_lba as u32);
        append_str(&mut b, &mut p, b" (");
        append_u32(&mut b, &mut p, part_secs as u32);
        append_str(&mut b, &mut p, b" sectores)");
        t.write_bytes(&b[..p], LineColor::Normal);
    }
    {
        let mut b = [0u8; TERM_COLS]; let mut p = 0;
        append_str(&mut b, &mut p, b"  Kernel: ");
        append_u32(&mut b, &mut p, kernel_raw as u32);
        append_str(&mut b, &mut p, b" bytes en LBA ");
        append_u32(&mut b, &mut p, KERNEL_LBA_START as u32);
        t.write_bytes(&b[..p], LineColor::Normal);
    }
    t.write_line(
        "  Accion requerida: mueve el VDI a Primary Master en VirtualBox.",
        LineColor::Info,
    );
    t.write_line(
        "  Luego apaga la VM, retira la ISO y arranca desde el HDD.",
        LineColor::Info,
    );

    // Invalidar caché FAT32 para que los comandos vean el nuevo sistema
    super::disk::invalidate_vol_cache();
}