// console/terminal/commands/disk.rs — PORTIX Kernel v0.8.0
//
// CAMBIOS v0.8.0 — ÚNICO cambio funcional: eliminar AtaBus::scan() de los
// comandos del terminal. Todos los demás comportamientos son idénticos a v0.7.5.
//
// ANTES (v0.7.5) — mount_vol():
//   let bus  = AtaBus::scan();        ← reset_and_init() → mata el canal
//   let info = bus.info(Primary0)?;
//
// AHORA (v0.8.0) — mount_vol():
//   let info = get_cached_drive_info()?;  ← lee BSS, NO toca hardware
//   let drive = AtaDrive::from_info(info);
//
// COMANDO cmd_edit() — ANTES:
//   let bus  = AtaBus::scan();            ← segundo scan innecesario
//   let info = bus.info(DriveId::Primary0).map(|i| *i);
//
// COMANDO cmd_edit() — AHORA:
//   let drive_info = get_cached_drive_info();  ← del caché
//
// COMANDO cmd_diskpart() — SIN CAMBIOS:
//   Conserva su AtaBus::scan() propio porque es un comando de diagnóstico
//   que debe reflejar el estado REAL del bus (si el drive desapareciera del
//   hardware, diskpart debe informarlo). Es el único scan() permitido post-boot.
//
// COMANDOS cmd_diskread / cmd_diskedit:
//   Para Primary0 usan get_cached_drive_info().
//   Para otros drives (drv_idx > 0) hacen un scan puntual porque son
//   herramientas de diagnóstico (mismo razonamiento que diskpart).
//
// COMANDO cmd_diskwrite:
//   Usa get_cached_drive_info() — solo opera sobre Primary0.
//
// ─────────────────────────────────────────────────────────────────────────────
//
// Herramientas de disco y sistema de archivos para PORTIX.
//
// ┌─ Navegación ──────────────────────────────────────────────────────────────┐
// │  ls   [ruta]          Listar directorio (actual si se omite)              │
// │  cd   <ruta>          Cambiar directorio                                  │
// │  pwd                  Mostrar ruta actual                                 │
// │  tree [ruta]          Árbol de directorios recursivo                      │
// └───────────────────────────────────────────────────────────────────────────┘
// ┌─ Archivos ─────────────────────────────────────────────────────────────────┐
// │  cat  <archivo>       Ver contenido de un archivo de texto                │
// │  touch <archivo>      Crear archivo vacío                                 │
// │  write <arch> <texto> Crear/sobreescribir archivo con texto               │
// │  rm   <ruta>          Eliminar archivo o directorio vacío                 │
// │  mv   <src> <dst>     Renombrar archivo o directorio                      │
// │  stat <ruta>          Información detallada de entrada                    │
// │  edit <archivo>       Abrir editor de texto tipo nano                     │
// └───────────────────────────────────────────────────────────────────────────┘
// ┌─ Directorios ──────────────────────────────────────────────────────────────┐
// │  mkdir <ruta>         Crear directorio                                    │
// └───────────────────────────────────────────────────────────────────────────┘
// ┌─ Disco ────────────────────────────────────────────────────────────────────┐
// │  diskinfo             Drives ATA detectados                               │
// │  diskread [lba] [drv] Hexdump de sector (solo lectura)                    │
// │  diskedit [lba] [drv] Editor hexadecimal de sector raw                    │
// │  diskwrite <lba> <0x> Rellenar sector con patrón (solo debug)             │
// │  diskpart             Panel tipo diskpart con layout del disco             │
// └───────────────────────────────────────────────────────────────────────────┘

#![allow(dead_code)]

use crate::console::terminal::{Terminal, LineColor, TERM_COLS};
use crate::console::terminal::fmt::*;
use crate::console::terminal::editor::EditorState;
use crate::drivers::storage::ata::{
    AtaBus, AtaError, AtaDrive, DriveId, DriveType,
    get_cached_drive_info,  // v0.8.0: caché global — no re-escanea el bus
};
use crate::drivers::storage::fat32::{Fat32Volume, FatError};
use crate::drivers::storage::vfs::{VfsMount, path_split, path_join, basename, parent_copy};

// ── Helpers privados ──────────────────────────────────────────────────────────

fn drive_id(idx: usize) -> DriveId {
    match idx {
        1 => DriveId::Primary1,
        2 => DriveId::Secondary0,
        3 => DriveId::Secondary1,
        _ => DriveId::Primary0,
    }
}

fn parse_lba_drive(args: &[u8]) -> (u64, usize) {
    let a  = trim(args);
    let sp = a.iter().position(|&b| b == b' ');
    let lba = if a.is_empty() { 0 } else {
        let part = if let Some(i) = sp { &a[..i] } else { a };
        parse_u64(part).unwrap_or(0)
    };
    let drv = if let Some(i) = sp {
        parse_u64(trim(&a[i + 1..])).unwrap_or(0) as usize
    } else { 0 };
    (lba, drv.min(3))
}

fn ata_err_msg(e: AtaError) -> &'static [u8] {
    match e {
        AtaError::Timeout         => "timeout del dispositivo".as_bytes(),
        AtaError::DriveFault      => "fallo de hardware del drive".as_bytes(),
        AtaError::OutOfRange      => "sector fuera de rango".as_bytes(),
        AtaError::DeviceError(_)  => "error del dispositivo ATA".as_bytes(),
        AtaError::BadBuffer       => "buffer de tamaño incorrecto".as_bytes(),
        AtaError::NoDrive         => "drive no detectado".as_bytes(),
    }
}

fn fat_err_msg(e: FatError) -> &'static [u8] {
    match e {
        FatError::NotFound    => b"ruta no encontrada",
        FatError::NoSpace     => b"sin espacio en disco",
        FatError::IsDir       => b"es un directorio, no un archivo",
        FatError::IsFile      => b"es un archivo, no un directorio",
        FatError::NameTooLong => b"nombre demasiado largo (max 255 chars)",
        FatError::InvalidPath => b"ruta no valida",
        FatError::Corrupt     => b"sistema de archivos corrupto",
        FatError::NotFat32    => b"volumen no FAT32 o no montado",
        FatError::Ata(e)      => ata_err_msg(e),
    }
}

// ── mount_vol — v0.8.0 ────────────────────────────────────────────────────────
//
// Monta el volumen FAT32 usando el DriveInfo cacheado en boot.
//
// CAMBIO v0.8.0: eliminado AtaBus::scan() y bus.info(Primary0).
// En su lugar: get_cached_drive_info() + AtaDrive::from_info().
//
// Por qué Fat32Volume::mount() consume el AtaDrive:
//   mount() toma ownership del drive para realizar lecturas en el volumen.
//   Esto significa que necesitamos crear un AtaDrive nuevo en cada llamada,
//   pero usando from_info() (que NO toca el hardware), no scan() (que sí).

fn mount_vol(t: &mut Terminal) -> Option<(Fat32Volume, VfsMount)> {
    // v0.8.0: usar caché en lugar de scan()
    let info = match get_cached_drive_info() {
        Some(i) => i,
        None => {
            t.write_line(
                "  Error: no se detecta ningún drive ATA.",
                LineColor::Error,
            );
            return None;
        }
    };

    let drive = AtaDrive::from_info(info);

    match Fat32Volume::mount(drive) {
        Ok(vol) => {
            let mut mnt = VfsMount::new();
            let root = vol.root_cluster();
            mnt.register("/", root);
            let _ = resolve_and_register(&vol, root, &mut mnt);
            Some((vol, mnt))
        }
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  Error: no se pudo montar el volumen FAT32: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            buf[pos] = b'.'; pos += 1;
            t.write_bytes(&buf[..pos], LineColor::Error);
            t.write_line(
                "  Sugerencia: ejecuta 'diskpart' para verificar el estado del disco.",
                LineColor::Normal,
            );
            None
        }
    }
}

/// Puebla el VfsMount con las rutas conocidas recorriendo el árbol.
fn resolve_and_register(vol: &Fat32Volume, root: u32, mnt: &mut VfsMount) -> Result<(), FatError> {
    let first_level = ["bin", "etc", "home", "tmp", "usr", "var"];
    for name in first_level {
        if let Ok(e) = vol.find_entry(root, name) {
            let mut path = [0u8; 64]; let mut plen = 0;
            path[0] = b'/'; plen += 1;
            let nl = name.len().min(63 - plen);
            path[plen..plen + nl].copy_from_slice(&name.as_bytes()[..nl]); plen += nl;
            if let Ok(s) = core::str::from_utf8(&path[..plen]) {
                mnt.register(s, e.cluster);
            }
            if name == "home" {
                if let Ok(u) = vol.find_entry(e.cluster, "user") {
                    mnt.register("/home/user", u.cluster);
                }
            }
        }
    }
    Ok(())
}

fn resolve_path(
    vol:  &Fat32Volume,
    mnt:  &VfsMount,
    cwd:  &[u8],
    cwd_len: usize,
    path: &[u8],
) -> Result<u32, FatError> {
    let path = trim(path);
    if path.is_empty() || path == b"/" {
        return Ok(mnt.root_cluster());
    }

    let mut abs = [0u8; 512];
    let abs_len;
    if path[0] == b'/' {
        let l = path.len().min(512);
        abs[..l].copy_from_slice(&path[..l]);
        abs_len = l;
    } else {
        abs_len = path_join(
            core::str::from_utf8(&cwd[..cwd_len]).unwrap_or("/"),
            core::str::from_utf8(path).unwrap_or(""),
            &mut abs,
        );
    }

    if let Some(c) = mnt.resolve(core::str::from_utf8(&abs[..abs_len]).unwrap_or("")) {
        return Ok(c);
    }

    let mut bufs = [[0u8; 64]; 16];
    let mut lens = [0usize; 16];
    let n = path_split(
        core::str::from_utf8(&abs[..abs_len]).unwrap_or("/"),
        &mut bufs,
        &mut lens,
    );

    let mut cur = mnt.root_cluster();
    for i in 0..n {
        let comp = core::str::from_utf8(&bufs[i][..lens[i]]).unwrap_or("");
        if comp == "." || comp.is_empty() { continue; }
        if comp == ".." {
            cur = mnt.root_cluster();
            for j in 0..i.saturating_sub(1) {
                let c2 = core::str::from_utf8(&bufs[j][..lens[j]]).unwrap_or("");
                if c2.is_empty() || c2 == "." { continue; }
                match vol.find_entry(cur, c2) {
                    Ok(e) => cur = e.cluster,
                    Err(e) => return Err(e),
                }
            }
            continue;
        }
        match vol.find_entry(cur, comp) {
            Ok(e) => cur = e.cluster,
            Err(e) => return Err(e),
        }
    }
    Ok(cur)
}

fn make_abs_path(cwd: &[u8], cwd_len: usize, input: &[u8], out: &mut [u8]) -> usize {
    let input = trim(input);
    if input.is_empty() {
        let l = cwd_len.min(out.len());
        out[..l].copy_from_slice(&cwd[..l]);
        return l;
    }
    if input[0] == b'/' {
        let l = input.len().min(out.len());
        out[..l].copy_from_slice(&input[..l]);
        return l;
    }
    path_join(
        core::str::from_utf8(&cwd[..cwd_len]).unwrap_or("/"),
        core::str::from_utf8(input).unwrap_or(""),
        out,
    )
}

fn fmt_size(buf: &mut [u8], pos: &mut usize, bytes: u32) {
    if bytes >= 1024 * 1024 {
        append_u32(buf, pos, bytes / (1024 * 1024));
        append_str(buf, pos, b" MiB");
    } else if bytes >= 1024 {
        append_u32(buf, pos, bytes / 1024);
        append_str(buf, pos, b" KiB");
    } else {
        append_u32(buf, pos, bytes);
        append_str(buf, pos, b" B  ");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMANDOS DE NAVEGACIÓN — idénticos a v0.7.5 excepto que usan nuevo mount_vol
// ═══════════════════════════════════════════════════════════════════════════════

pub fn cmd_ls(t: &mut Terminal, args: &[u8]) {
    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };
    let cwd     = t.cwd;
    let cwd_len = t.cwd_len;

    let cluster = if trim(args).is_empty() {
        mnt.resolve(core::str::from_utf8(&cwd[..cwd_len]).unwrap_or("/"))
           .unwrap_or(mnt.root_cluster())
    } else {
        match resolve_path(&vol, &mnt, &cwd, cwd_len, trim(args)) {
            Ok(c) => c,
            Err(e) => {
                let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                append_str(&mut buf, &mut pos, b"  ls: ");
                let em = fat_err_msg(e);
                buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
                t.write_bytes(&buf[..pos], LineColor::Error);
                return;
            }
        }
    };

    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Directorio: ");
        if trim(args).is_empty() {
            let l = cwd_len.min(TERM_COLS - pos);
            buf[pos..pos + l].copy_from_slice(&cwd[..l]); pos += l;
        } else {
            let a = trim(args);
            let l = a.len().min(TERM_COLS - pos);
            buf[pos..pos + l].copy_from_slice(&a[..l]); pos += l;
        }
        t.write_bytes(&buf[..pos], LineColor::Info);
    }
    t.write_line("  Tipo  Tamaño      Nombre", LineColor::Header);
    t.write_line("  ----  ----------  ------", LineColor::Normal);

    let mut count_files = 0u32;
    let mut count_dirs  = 0u32;
    let mut total_bytes = 0u64;

    let result = vol.list_dir(cluster, |e| {
        let name = e.name_str();
        if name == "." || name == ".." { return; }

        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  ");

        if e.is_dir {
            append_str(&mut buf, &mut pos, b"[DIR]  ");
            append_str(&mut buf, &mut pos, b"           ");
            count_dirs += 1;
        } else {
            append_str(&mut buf, &mut pos, b"[ARC]  ");
            fmt_size(&mut buf, &mut pos, e.size);
            while pos < 20 { buf[pos] = b' '; pos += 1; }
            count_files += 1;
            total_bytes += e.size as u64;
        }

        let nb = name.as_bytes();
        let nl = nb.len().min(TERM_COLS - pos);
        buf[pos..pos + nl].copy_from_slice(&nb[..nl]); pos += nl;

        let color = if e.is_dir { LineColor::Info } else { LineColor::Normal };
        t.write_bytes(&buf[..pos], color);
    });

    if let Err(e) = result {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Error al leer directorio: ");
        let em = fat_err_msg(e);
        buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
        t.write_bytes(&buf[..pos], LineColor::Error);
        return;
    }

    t.write_line("  ----  ----------  ------", LineColor::Normal);
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  ");
        append_u32(&mut buf, &mut pos, count_dirs);
        append_str(&mut buf, &mut pos, b" directorio(s)   ");
        append_u32(&mut buf, &mut pos, count_files);
        append_str(&mut buf, &mut pos, b" archivo(s)   total: ");
        if total_bytes >= 1024 {
            append_u32(&mut buf, &mut pos, (total_bytes / 1024) as u32);
            append_str(&mut buf, &mut pos, b" KiB");
        } else {
            append_u32(&mut buf, &mut pos, total_bytes as u32);
            append_str(&mut buf, &mut pos, b" bytes");
        }
        t.write_bytes(&buf[..pos], LineColor::Success);
    }
    t.write_empty();
}

pub fn cmd_pwd(t: &mut Terminal) {
    let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
    append_str(&mut buf, &mut pos, b"  ");
    let l = t.cwd_len.min(TERM_COLS - 2);
    buf[pos..pos + l].copy_from_slice(&t.cwd[..l]); pos += l;
    t.write_bytes(&buf[..pos], LineColor::Normal);
}

pub fn cmd_cd(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() {
        t.cwd[0] = b'/'; t.cwd[1] = b'h'; t.cwd[2] = b'o';
        t.cwd[3] = b'm'; t.cwd[4] = b'e'; t.cwd[5] = b'/';
        t.cwd[6] = b'u'; t.cwd[7] = b's'; t.cwd[8] = b'e';
        t.cwd[9] = b'r'; t.cwd_len = 10;
        return;
    }

    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };

    let cluster = match resolve_path(&vol, &mnt, &t.cwd, t.cwd_len, args) {
        Ok(c) => c,
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  cd: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    let mut new_path = [0u8; 256];
    let new_len = make_abs_path(&t.cwd, t.cwd_len, args, &mut new_path);
    let _ = cluster;

    let l = new_len.min(255);
    t.cwd[..l].copy_from_slice(&new_path[..l]);
    t.cwd_len = l;
}

pub fn cmd_tree(t: &mut Terminal, args: &[u8]) {
    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };

    let cluster = if trim(args).is_empty() {
        mnt.resolve(core::str::from_utf8(&t.cwd[..t.cwd_len]).unwrap_or("/"))
           .unwrap_or(mnt.root_cluster())
    } else {
        match resolve_path(&vol, &mnt, &t.cwd, t.cwd_len, trim(args)) {
            Ok(c) => c,
            Err(e) => {
                let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                append_str(&mut buf, &mut pos, b"  tree: ");
                let em = fat_err_msg(e);
                buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
                t.write_bytes(&buf[..pos], LineColor::Error);
                return;
            }
        }
    };

    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  ");
        let l = t.cwd_len.min(TERM_COLS - 2);
        buf[pos..pos + l].copy_from_slice(&t.cwd[..l]); pos += l;
        t.write_bytes(&buf[..pos], LineColor::Info);
    }

    draw_tree(t, &vol, cluster, 0, 4);
    t.write_empty();
}

fn draw_tree(t: &mut Terminal, vol: &Fat32Volume, cluster: u32, depth: usize, max_depth: usize) {
    if depth >= max_depth { return; }
    let _ = vol.list_dir(cluster, |e| {
        let name = e.name_str();
        if name == "." || name == ".." { return; }

        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  ");
        for _ in 0..depth { append_str(&mut buf, &mut pos, b"   "); }
        if e.is_dir {
            append_str(&mut buf, &mut pos, b"+-[");
        } else {
            append_str(&mut buf, &mut pos, b"+-[F] ");
        }
        let nb = name.as_bytes();
        let nl = nb.len().min(TERM_COLS - pos - 10);
        buf[pos..pos + nl].copy_from_slice(&nb[..nl]); pos += nl;
        if e.is_dir { buf[pos] = b']'; pos += 1; }
        let color = if e.is_dir { LineColor::Info } else { LineColor::Normal };
        t.write_bytes(&buf[..pos], color);
    });
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMANDOS DE ARCHIVOS — idénticos a v0.7.5
// ═══════════════════════════════════════════════════════════════════════════════

pub fn cmd_cat(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() {
        t.write_line("  Uso: cat <archivo>", LineColor::Warning);
        return;
    }

    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };

    let name = basename(core::str::from_utf8(args).unwrap_or(""));
    let mut par = [0u8; 256]; let par_len;
    let mut abs = [0u8; 256];
    let abs_len = make_abs_path(&t.cwd, t.cwd_len, args, &mut abs);
    {
        par_len = parent_copy(core::str::from_utf8(&abs[..abs_len]).unwrap_or("/"), &mut par);
    }

    let dir_cluster = match resolve_path(&vol, &mnt, &par, par_len, b".") {
        Ok(c) => c,
        Err(_) => {
            mnt.resolve(core::str::from_utf8(&t.cwd[..t.cwd_len]).unwrap_or("/"))
               .unwrap_or(mnt.root_cluster())
        }
    };

    let entry = match vol.find_entry(dir_cluster, name) {
        Ok(e) => e,
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  cat: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    if entry.is_dir {
        t.write_line("  cat: es un directorio. Usa 'ls' para listar su contenido.", LineColor::Warning);
        return;
    }

    const MAX_READ: usize = 8192;
    let mut content = [0u8; MAX_READ];
    let bytes = match vol.read_file(&entry, &mut content) {
        Ok(n) => n,
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  cat: error leyendo: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    {
        let mut hdr = [0u8; TERM_COLS]; let mut hp = 0;
        append_str(&mut hdr, &mut hp, b"  --- ");
        let nb = args.len().min(50);
        hdr[hp..hp + nb].copy_from_slice(&args[..nb]); hp += nb;
        append_str(&mut hdr, &mut hp, b" (");
        append_u32(&mut hdr, &mut hp, bytes as u32);
        append_str(&mut hdr, &mut hp, b" bytes) ---");
        t.write_bytes(&hdr[..hp], LineColor::Header);
    }

    let mut start = 0usize;
    let mut line_n = 1u32;
    for i in 0..bytes {
        let is_nl = content[i] == b'\n' || content[i] == b'\r';
        let is_end = i + 1 == bytes;
        if is_nl || is_end {
            let end = if is_end && !is_nl { i + 1 } else { i };
            if end > start {
                let chunk = &content[start..end];
                let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                append_str(&mut buf, &mut pos, b"  ");
                append_u32(&mut buf, &mut pos, line_n);
                append_str(&mut buf, &mut pos, b"  ");
                if pos < 6 { buf[pos] = b' '; pos += 1; }
                for &b in chunk {
                    if pos >= TERM_COLS - 1 { break; }
                    buf[pos] = if b >= 0x20 && b < 0x7F { b } else if b == b'\t' { b' ' } else { b'.' };
                    pos += 1;
                }
                t.write_bytes(&buf[..pos], LineColor::Normal);
                line_n += 1;
            }
            if content[i] == b'\r' && i + 1 < bytes && content[i + 1] == b'\n' {
                start = i + 2;
            } else {
                start = i + 1;
            }
        }
    }
    t.write_empty();
}

pub fn cmd_touch(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() {
        t.write_line("  Uso: touch <nombre_archivo>", LineColor::Warning);
        return;
    }
    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };

    let cluster = mnt.resolve(core::str::from_utf8(&t.cwd[..t.cwd_len]).unwrap_or("/"))
                     .unwrap_or(mnt.root_cluster());
    let name = core::str::from_utf8(args).unwrap_or("archivo");

    match vol.create_file(cluster, name) {
        Ok(_) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  [OK] Archivo creado: ");
            let nl = args.len().min(TERM_COLS - pos);
            buf[pos..pos + nl].copy_from_slice(&args[..nl]); pos += nl;
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  touch: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
        }
    }
}

pub fn cmd_write(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() {
        t.write_line("  Uso: write <archivo> <contenido>", LineColor::Warning);
        t.write_line("  Ejemplo: write saludo.txt Hola Mundo", LineColor::Normal);
        return;
    }

    let sp = match args.iter().position(|&b| b == b' ') {
        Some(i) => i,
        None => {
            t.write_line("  write: falta el contenido. Uso: write <archivo> <texto>", LineColor::Warning);
            return;
        }
    };

    let name_bytes = trim(&args[..sp]);
    let content    = trim(&args[sp + 1..]);
    let name = core::str::from_utf8(name_bytes).unwrap_or("archivo");

    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };
    let cluster = mnt.resolve(core::str::from_utf8(&t.cwd[..t.cwd_len]).unwrap_or("/"))
                     .unwrap_or(mnt.root_cluster());

    let mut entry = match vol.find_entry(cluster, name) {
        Ok(e) => e,
        Err(FatError::NotFound) => {
            match vol.create_file(cluster, name) {
                Ok(e)  => e,
                Err(e) => {
                    let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                    append_str(&mut buf, &mut pos, b"  write: no se pudo crear el archivo: ");
                    let em = fat_err_msg(e);
                    buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
                    t.write_bytes(&buf[..pos], LineColor::Error);
                    return;
                }
            }
        }
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  write: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    match vol.write_file(&mut entry, content) {
        Ok(()) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  [OK] Escrito: ");
            let nl = name_bytes.len().min(40);
            buf[pos..pos + nl].copy_from_slice(&name_bytes[..nl]); pos += nl;
            append_str(&mut buf, &mut pos, b" (");
            append_u32(&mut buf, &mut pos, content.len() as u32);
            append_str(&mut buf, &mut pos, b" bytes)");
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  write: error al escribir: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
        }
    }
}

pub fn cmd_mkdir(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() {
        t.write_line("  Uso: mkdir <nombre>", LineColor::Warning);
        return;
    }

    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };
    let cluster = mnt.resolve(core::str::from_utf8(&t.cwd[..t.cwd_len]).unwrap_or("/"))
                     .unwrap_or(mnt.root_cluster());
    let name = core::str::from_utf8(args).unwrap_or("directorio");

    match vol.create_dir(cluster, name) {
        Ok(_) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  [OK] Directorio creado: ");
            let nl = args.len().min(TERM_COLS - pos);
            buf[pos..pos + nl].copy_from_slice(&args[..nl]); pos += nl;
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  mkdir: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
        }
    }
}

pub fn cmd_rm(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() {
        t.write_line("  Uso: rm <archivo_o_directorio>", LineColor::Warning);
        return;
    }

    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };
    let name = basename(core::str::from_utf8(args).unwrap_or(""));
    let cluster = mnt.resolve(core::str::from_utf8(&t.cwd[..t.cwd_len]).unwrap_or("/"))
                     .unwrap_or(mnt.root_cluster());

    let entry = match vol.find_entry(cluster, name) {
        Ok(e) => e,
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  rm: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    match vol.delete_entry(&entry) {
        Ok(()) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  [OK] Eliminado: ");
            let nl = args.len().min(TERM_COLS - pos);
            buf[pos..pos + nl].copy_from_slice(&args[..nl]); pos += nl;
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  rm: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
        }
    }
}

pub fn cmd_stat(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() {
        t.write_line("  Uso: stat <archivo_o_directorio>", LineColor::Warning);
        return;
    }

    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };
    let name = basename(core::str::from_utf8(args).unwrap_or(""));
    let cluster = mnt.resolve(core::str::from_utf8(&t.cwd[..t.cwd_len]).unwrap_or("/"))
                     .unwrap_or(mnt.root_cluster());

    let entry = match vol.find_entry(cluster, name) {
        Ok(e) => e,
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  stat: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    t.separador("INFORMACIÓN DE ENTRADA");
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Nombre   : ");
        let nb = entry.name_str().as_bytes();
        let nl = nb.len().min(TERM_COLS - pos);
        buf[pos..pos + nl].copy_from_slice(&nb[..nl]); pos += nl;
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Tipo     : ");
        if entry.is_dir { append_str(&mut buf, &mut pos, b"Directorio"); }
        else            { append_str(&mut buf, &mut pos, b"Archivo"); }
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Tama\xF1o     : ");
        append_u32(&mut buf, &mut pos, entry.size);
        append_str(&mut buf, &mut pos, b" bytes  (");
        append_u32(&mut buf, &mut pos, entry.size / 1024 + 1);
        append_str(&mut buf, &mut pos, b" KiB aprox)");
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Cluster  : ");
        append_u32(&mut buf, &mut pos, entry.cluster);
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Sector   : LBA ");
        append_u32(&mut buf, &mut pos, entry.dir_sector as u32);
        append_str(&mut buf, &mut pos, b"  offset ");
        append_u32(&mut buf, &mut pos, entry.dir_offset as u32);
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    t.write_empty();
}

pub fn cmd_mv(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    let sp = match args.iter().position(|&b| b == b' ') {
        Some(i) => i,
        None => {
            t.write_line("  Uso: mv <origen> <destino>", LineColor::Warning);
            return;
        }
    };

    let src_bytes = trim(&args[..sp]);
    let dst_bytes = trim(&args[sp + 1..]);
    let src_name = core::str::from_utf8(src_bytes).unwrap_or("");
    let dst_name = core::str::from_utf8(dst_bytes).unwrap_or("");

    if src_name.is_empty() || dst_name.is_empty() {
        t.write_line("  mv: origen o destino vacío.", LineColor::Warning);
        return;
    }

    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };
    let cluster = mnt.resolve(core::str::from_utf8(&t.cwd[..t.cwd_len]).unwrap_or("/"))
                     .unwrap_or(mnt.root_cluster());

    let src_entry = match vol.find_entry(cluster, src_name) {
        Ok(e) => e,
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  mv: origen no encontrado: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    let new_entry_result = if src_entry.is_dir {
        vol.create_dir(cluster, dst_name)
    } else {
        vol.create_file(cluster, dst_name)
    };

    let mut new_entry = match new_entry_result {
        Ok(e) => e,
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  mv: no se pudo crear destino: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    if !src_entry.is_dir && src_entry.size > 0 {
        const MAX: usize = 65536;
        let mut buf_data = [0u8; MAX];
        let read_len = src_entry.size.min(MAX as u32) as usize;
        let real = match vol.read_file(&src_entry, &mut buf_data[..read_len]) {
            Ok(n) => n,
            Err(_) => 0,
        };
        if real > 0 {
            let _ = vol.write_file(&mut new_entry, &buf_data[..real]);
        }
    }

    match vol.delete_entry(&src_entry) {
        Ok(()) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  [OK] ");
            let sl = src_bytes.len().min(30);
            buf[pos..pos + sl].copy_from_slice(&src_bytes[..sl]); pos += sl;
            append_str(&mut buf, &mut pos, b" -> ");
            let dl = dst_bytes.len().min(30);
            buf[pos..pos + dl].copy_from_slice(&dst_bytes[..dl]); pos += dl;
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  mv: no se pudo eliminar origen: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
        }
    }
}

/// `edit <archivo>` — Abre el editor de texto tipo nano.
///
/// CAMBIO v0.8.0: eliminado AtaBus::scan() para obtener el DriveInfo.
/// En su lugar se usa get_cached_drive_info() del caché global.
pub fn cmd_edit(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.is_empty() {
        t.write_line("  Uso: edit <archivo>", LineColor::Warning);
        return;
    }

    let (vol, mnt) = match mount_vol(t) { Some(x) => x, None => return };
    let name = core::str::from_utf8(args).unwrap_or("archivo");
    let cluster = mnt.resolve(core::str::from_utf8(&t.cwd[..t.cwd_len]).unwrap_or("/"))
                     .unwrap_or(mnt.root_cluster());

    let entry = match vol.find_entry(cluster, name) {
        Ok(e) => {
            if e.is_dir {
                t.write_line("  edit: es un directorio.", LineColor::Error);
                return;
            }
            e
        }
        Err(FatError::NotFound) => {
            match vol.create_file(cluster, name) {
                Ok(e) => {
                    let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                    append_str(&mut buf, &mut pos, b"  Archivo nuevo: ");
                    let nl = args.len().min(40);
                    buf[pos..pos + nl].copy_from_slice(&args[..nl]); pos += nl;
                    t.write_bytes(&buf[..pos], LineColor::Info);
                    e
                }
                Err(e) => {
                    let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                    append_str(&mut buf, &mut pos, b"  edit: no se pudo crear: ");
                    let em = fat_err_msg(e);
                    buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
                    t.write_bytes(&buf[..pos], LineColor::Error);
                    return;
                }
            }
        }
        Err(e) => {
            let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  edit: ");
            let em = fat_err_msg(e);
            buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
            t.write_bytes(&buf[..pos], LineColor::Error);
            return;
        }
    };

    let mut content = [0u8; crate::console::terminal::editor::EDITOR_MAX_BYTES];
    let content_len = if entry.size > 0 {
        vol.read_file(&entry, &mut content).unwrap_or(0)
    } else {
        0
    };

    let mut full_path = [0u8; 256];
    let fpl = make_abs_path(&t.cwd, t.cwd_len, args, &mut full_path);

    // v0.8.0: usar caché en lugar de AtaBus::scan()
    // ANTES: let bus = AtaBus::scan(); let info = bus.info(DriveId::Primary0).map(|i| *i);
    // AHORA:
    if let Some(drive_info) = get_cached_drive_info() {
        t.editor = Some(EditorState::new_text(
            content, content_len,
            entry, drive_info,
            &full_path[..fpl],
        ));
        let mut msg = [0u8; TERM_COLS]; let mut mp = 0;
        append_str(&mut msg, &mut mp, b"  Abriendo editor de texto: ");
        let nl = args.len().min(40);
        msg[mp..mp + nl].copy_from_slice(&args[..nl]); mp += nl;
        t.write_bytes(&msg[..mp], LineColor::Info);
    } else {
        t.write_line("  edit: drive no disponible.", LineColor::Error);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DISKPART — Panel de información del disco
// Conserva AtaBus::scan() propio: es un comando de diagnóstico.
// ═══════════════════════════════════════════════════════════════════════════════

pub fn cmd_diskpart(t: &mut Terminal) {
    t.separador("DISKPART — GESTIÓN DE DISCO");

    // scan() aquí es INTENCIONAL: diskpart debe mostrar el estado real del bus
    // en el momento de la ejecución. Si el drive desapareciera físicamente,
    // diskpart debe informarlo correctamente.
    let bus = AtaBus::scan();
    if bus.count() == 0 {
        t.write_line("  No se detectaron unidades ATA.", LineColor::Warning);
        t.write_empty(); return;
    }

    t.write_line("  UNIDADES DETECTADAS:", LineColor::Header);
    t.write_line("  #   Canal         Tipo   Modo   Capacidad      Modelo", LineColor::Info);
    t.write_line("  --- -----------   -----  -----  -------------  -----", LineColor::Normal);

    for info in bus.iter() {
        let idx: usize = info.id as usize;
        let canal: &[u8] = match info.id {
            DriveId::Primary0   => b"ATA0-Master ",
            DriveId::Primary1   => b"ATA0-Slave  ",
            DriveId::Secondary0 => b"ATA1-Master ",
            DriveId::Secondary1 => b"ATA1-Slave  ",
        };
        let tipo: &[u8]  = if info.kind == DriveType::Atapi { b"ATAPI" } else { b"ATA  " };
        let modo: &[u8]  = if info.lba48 { b"LBA48" } else { b"LBA28" };

        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  ");
        append_u32(&mut buf, &mut pos, idx as u32);
        append_str(&mut buf, &mut pos, b"   ");
        buf[pos..pos + canal.len()].copy_from_slice(canal); pos += canal.len();
        append_str(&mut buf, &mut pos, b"  ");
        buf[pos..pos + tipo.len()].copy_from_slice(tipo); pos += tipo.len();
        append_str(&mut buf, &mut pos, b"  ");
        buf[pos..pos + modo.len()].copy_from_slice(modo); pos += modo.len();
        append_str(&mut buf, &mut pos, b"  ");
        append_mib(&mut buf, &mut pos, info.capacity_mib);
        append_str(&mut buf, &mut pos, b"       ");
        let m  = info.model_str().as_bytes();
        let ml = m.len().min(TERM_COLS - pos);
        buf[pos..pos + ml].copy_from_slice(&m[..ml]); pos += ml;
        t.write_bytes(&buf[..pos], LineColor::Normal);
    }
    t.write_empty();

    t.write_line("  VOLUMEN FAT32 (ATA0-Master):", LineColor::Header);

    // Segundo scan del diskpart: también intencional para leer el MBR en vivo
    let bus2 = AtaBus::scan();
    if let Some(info) = bus2.info(DriveId::Primary0) {
        let info = *info;
        let drive = AtaDrive::from_info(info);

        let mut mbr = [0u8; 512];
        match drive.read_sectors(0, 1, &mut mbr) {
            Ok(()) => {
                let sig_ok = mbr[510] == 0x55 && mbr[511] == 0xAA;
                {
                    let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                    append_str(&mut buf, &mut pos, b"  Firma MBR (0x55AA): ");
                    if sig_ok {
                        append_str(&mut buf, &mut pos, b"VALIDA");
                        t.write_bytes(&buf[..pos], LineColor::Success);
                    } else {
                        append_str(&mut buf, &mut pos, b"NO ENCONTRADA");
                        t.write_bytes(&buf[..pos], LineColor::Warning);
                    }
                }

                t.write_line("  Tabla de particiones:", LineColor::Info);
                t.write_line("  #  Estado   Tipo    LBA inicio   Tamaño (sectores)", LineColor::Normal);
                for i in 0..4usize {
                    let off    = 0x1BE + i * 16;
                    let status = mbr[off];
                    let ptype  = mbr[off + 4];
                    let lba    = u32::from_le_bytes([mbr[off+8], mbr[off+9], mbr[off+10], mbr[off+11]]);
                    let size   = u32::from_le_bytes([mbr[off+12], mbr[off+13], mbr[off+14], mbr[off+15]]);
                    if ptype == 0 { continue; }

                    let tipo_s: &[u8] = match ptype {
                        0x0B | 0x0C => b"FAT32   ",
                        0x0E        => b"FAT16   ",
                        0x83        => b"Linux   ",
                        0x07        => b"NTFS    ",
                        _           => b"Otro    ",
                    };

                    let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                    append_str(&mut buf, &mut pos, b"  ");
                    append_u32(&mut buf, &mut pos, (i + 1) as u32);
                    append_str(&mut buf, &mut pos, b"  ");
                    if status == 0x80 {
                        append_str(&mut buf, &mut pos, b"Activa   ");
                    } else {
                        append_str(&mut buf, &mut pos, b"Inactiva ");
                    }
                    buf[pos..pos + tipo_s.len()].copy_from_slice(tipo_s); pos += tipo_s.len();
                    append_str(&mut buf, &mut pos, b"  ");
                    append_u32(&mut buf, &mut pos, lba);
                    append_str(&mut buf, &mut pos, b"          ");
                    append_u32(&mut buf, &mut pos, size);
                    t.write_bytes(&buf[..pos], LineColor::Normal);
                }
            }
            Err(e) => {
                let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                append_str(&mut buf, &mut pos, b"  Error leyendo MBR: ");
                let em = ata_err_msg(e);
                buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
                t.write_bytes(&buf[..pos], LineColor::Error);
            }
        }

        t.write_empty();
        let drive2 = AtaDrive::from_info(info);
        match Fat32Volume::mount(drive2) {
            Ok(vol) => {
                let root = vol.root_cluster();
                t.write_line("  Estado FAT32: MONTADO", LineColor::Success);
                {
                    let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                    append_str(&mut buf, &mut pos, b"  Cluster ra\xEDz : ");
                    append_u32(&mut buf, &mut pos, root);
                    t.write_bytes(&buf[..pos], LineColor::Normal);
                }
                let mut file_count = 0u32; let mut dir_count = 0u32;
                let _ = vol.list_dir(root, |e| {
                    if e.is_dir { dir_count += 1; } else { file_count += 1; }
                });
                {
                    let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                    append_str(&mut buf, &mut pos, b"  Entradas ra\xEDz: ");
                    append_u32(&mut buf, &mut pos, dir_count);
                    append_str(&mut buf, &mut pos, b" directorios, ");
                    append_u32(&mut buf, &mut pos, file_count);
                    append_str(&mut buf, &mut pos, b" archivos");
                    t.write_bytes(&buf[..pos], LineColor::Normal);
                }
            }
            Err(_) => {
                t.write_line("  Estado FAT32: NO MONTADO — ejecuta 'mkfs' para formatear", LineColor::Warning);
            }
        }
    }

    t.write_empty();
    t.write_line("  COMANDOS DISPONIBLES:", LineColor::Header);
    t.write_line("    ls   [ruta]          Listar directorio", LineColor::Normal);
    t.write_line("    cd   <ruta>          Cambiar directorio", LineColor::Normal);
    t.write_line("    pwd                  Directorio actual", LineColor::Normal);
    t.write_line("    cat  <archivo>       Ver contenido", LineColor::Normal);
    t.write_line("    edit <archivo>       Editar texto (tipo nano)", LineColor::Normal);
    t.write_line("    touch <archivo>      Crear archivo vacío", LineColor::Normal);
    t.write_line("    write <arch> <texto> Escribir en archivo", LineColor::Normal);
    t.write_line("    mkdir <nombre>       Crear directorio", LineColor::Normal);
    t.write_line("    rm    <ruta>         Eliminar", LineColor::Normal);
    t.write_line("    mv    <src> <dst>    Renombrar", LineColor::Normal);
    t.write_line("    stat  <ruta>         Información detallada", LineColor::Normal);
    t.write_line("    tree  [ruta]         Árbol de directorios", LineColor::Normal);
    t.write_empty();
}

pub fn cmd_diskinfo(t: &mut Terminal) {
    cmd_diskpart(t);
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMANDOS ATA RAW — v0.8.0: Primary0 usa caché, otros drives scan puntual
// ═══════════════════════════════════════════════════════════════════════════════

pub fn cmd_diskread(t: &mut Terminal, args: &[u8]) {
    let (lba, drv_idx) = parse_lba_drive(args);
    let id             = drive_id(drv_idx);

    // v0.8.0: Primary0 usa caché; otros drives usan scan puntual (diagnóstico)
    let info = if drv_idx == 0 {
        match get_cached_drive_info() {
            Some(i) => i,
            None => { t.write_line("  Error: drive no inicializado.", LineColor::Error); return; }
        }
    } else {
        let bus = AtaBus::scan();
        match bus.info(id) {
            Some(i) => *i,
            None => { t.write_line("  Error: drive no detectado.", LineColor::Error); return; }
        }
    };

    if lba >= info.total_sectors {
        t.write_line("  Error: LBA fuera de rango.", LineColor::Error); return;
    }

    let drive      = AtaDrive::from_info(info);
    let mut sector = [0u8; 512];
    if let Err(e) = drive.read_sectors(lba, 1, &mut sector) {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Error leyendo LBA ");
        append_u32(&mut buf, &mut pos, lba as u32);
        append_str(&mut buf, &mut pos, b": ");
        let em = ata_err_msg(e);
        buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
        t.write_bytes(&buf[..pos], LineColor::Error);
        return;
    }

    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Sector LBA=");
        append_u32(&mut buf, &mut pos, lba as u32);
        append_str(&mut buf, &mut pos, b"  drive=");
        append_u32(&mut buf, &mut pos, drv_idx as u32);
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

    if lba == 0 {
        if sector[510] == 0x55 && sector[511] == 0xAA {
            t.write_line("  [MBR] Firma 0x55AA valida.", LineColor::Success);
        } else {
            t.write_line("  [MBR] Sin firma 0x55AA.", LineColor::Warning);
        }
    }
    t.write_empty();
}

pub fn cmd_diskedit(t: &mut Terminal, args: &[u8]) {
    let (lba, drv_idx) = parse_lba_drive(args);
    let id             = drive_id(drv_idx);

    // v0.8.0: Primary0 usa caché; otros drives usan scan puntual
    let info = if drv_idx == 0 {
        match get_cached_drive_info() {
            Some(i) => i,
            None => { t.write_line("  Error: drive no inicializado.", LineColor::Error); return; }
        }
    } else {
        let bus = AtaBus::scan();
        match bus.info(id) {
            Some(i) => *i,
            None => {
                let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
                append_str(&mut buf, &mut pos, b"  Error: drive ");
                append_u32(&mut buf, &mut pos, drv_idx as u32);
                append_str(&mut buf, &mut pos, b" no detectado.");
                t.write_bytes(&buf[..pos], LineColor::Error);
                return;
            }
        }
    };

    if lba >= info.total_sectors {
        t.write_line("  Error: LBA fuera de rango.", LineColor::Error); return;
    }

    let drive      = AtaDrive::from_info(info);
    let mut sector = [0u8; 512];
    if let Err(e) = drive.read_sectors(lba, 1, &mut sector) {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Error leyendo sector: ");
        let em = ata_err_msg(e);
        buf[pos..pos + em.len()].copy_from_slice(em); pos += em.len();
        t.write_bytes(&buf[..pos], LineColor::Error);
        return;
    }

    {
        let mut buf = [0u8; TERM_COLS]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  Abriendo editor hex: LBA=");
        append_u32(&mut buf, &mut pos, lba as u32);
        append_str(&mut buf, &mut pos, b" drive=");
        append_u32(&mut buf, &mut pos, drv_idx as u32);
        t.write_bytes(&buf[..pos], LineColor::Info);
    }

    t.editor = Some(EditorState::new_hex(sector, lba, info));
}

pub fn cmd_diskwrite(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    let sp = match args.iter().position(|&b| b == b' ') {
        Some(i) => i,
        None => {
            t.write_line("  Uso: diskwrite <lba> <0xPATRON>", LineColor::Warning);
            return;
        }
    };

    let lba = match parse_u64(&args[..sp]) {
        Some(n) => n,
        None => { t.write_line("  Error: LBA inválido.", LineColor::Error); return; }
    };
    let pat = match parse_hex(trim(&args[sp + 1..])) {
        Some(n) => (n & 0xFF) as u8,
        None => { t.write_line("  Error: patrón inválido (usa 0xNN).", LineColor::Error); return; }
    };

    // v0.8.0: usar caché
    let info = match get_cached_drive_info() {
        Some(i) => i,
        None => { t.write_line("  Error: drive 0 no disponible.", LineColor::Error); return; }
    };
    if lba >= info.total_sectors {
        t.write_line("  Error: LBA fuera de rango.", LineColor::Error); return;
    }

    let drive = AtaDrive::from_info(info);
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
        Err(_) => { t.write_line("  Error: fallo al escribir sector.", LineColor::Error); }
    }
}