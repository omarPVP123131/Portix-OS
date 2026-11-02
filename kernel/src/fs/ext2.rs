// kernel/src/fs/ext2.rs — ext2 de LECTURA/ESCRITURA bajo el contrato
// `SistemaDeFicheros` (ciclo 51, lote 2).
//
// Principios:
//   • Formato ext2 estándar rev 1 (lo que crea `mke2fs -t ext2` en Linux): el
//     volumen que escribe Portix debe pasar `e2fsck -fn` en el anfitrión y
//     poder leerse con `debugfs`. Nada de "ext2 propio".
//   • [T-001] Caché de bloques de escritura inmediata (write-through): arena
//     fija de 2 MiB, correspondencia directa (bloque -> ranura). Las lecturas
//     repetidas de inodos, directorios, mapas de bits e indirectos se sirven
//     de memoria; toda escritura va al disco en el acto (y actualiza la
//     ranura), de modo que un corte de corriente deja el volumen igual que
//     antes de la caché: `e2fsck -fn` limpio. Aciertos/fallos se cuentan y
//     `bench` los publica. El superbloque y la tabla de descriptores de grupo
//     se mantienen en memoria y se vuelcan (primario + copias de respaldo) al
//     final de cada operación que los modifica.
//   • Se rechaza montar cualquier volumen con características `incompat`
//     desconocidas (p. ej. extents de ext4) o `ro_compat` que no sepamos
//     mantener: antes "no montado" que "corrompido".
//   • [T-001] Mapeo de bloques: 12 directos + indirecto simple, doble y
//     triple (tabla genérica por niveles); más allá, EFBIG.
//   • Sin `unwrap`; los errores son errno positivos (`Resultado`).

use alloc::string::String;
use alloc::vec::Vec;
use crate::drivers::storage::registry;
use super::{errno, EntradaDir, InformeVerificacion, Metadatos, Resultado, SistemaDeFicheros};

const MAGIA: u16 = 0xEF53;
const RAIZ: u32 = 2;
const INCOMPAT_FILETYPE: u32 = 0x0002;
const ROCOMPAT_SPARSE_SUPER: u32 = 0x0001;
const ROCOMPAT_LARGE_FILE: u32 = 0x0002;
const S_IFMT: u16 = 0o170000;
const S_IFDIR: u16 = 0o040000;
const S_IFREG: u16 = 0o100000;
const S_IFLNK: u16 = 0o120000;
const S_IFIFO: u16 = 0o010000;
const TIPO_REG: u8 = 1;
const TIPO_DIR: u8 = 2;
const TIPO_LNK: u8 = 7;
const TIPO_FIFO: u8 = 5;
const NOMBRE_MAX: usize = 255;

fn rd16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
fn rd32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }
fn wr16(b: &mut [u8], o: usize, v: u16) { b[o..o + 2].copy_from_slice(&v.to_le_bytes()); }
fn wr32(b: &mut [u8], o: usize, v: u32) { b[o..o + 4].copy_from_slice(&v.to_le_bytes()); }

fn ahora() -> u32 {
    let t = crate::time::rtc::now_epoch();
    if t < 0 { 0 } else if t > u32::MAX as i64 { u32::MAX } else { t as u32 }
}

fn limpiar(ruta: &str) -> &str {
    let c = ruta.trim_end_matches('/');
    if c.is_empty() { "/" } else { c }
}

/// Inodo en memoria (los 128 bytes estándar; se conserva el resto crudo).
#[derive(Clone)]
struct Inodo {
    modo: u16,
    uid: u16,
    tam: u32,
    atime: u32,
    ctime: u32,
    mtime: u32,
    dtime: u32,
    gid: u16,
    enlaces: u16,
    bloques512: u32,
    flags: u32,
    bloque: [u32; 15],
    tam_alto: u32,
    crudo: Vec<u8>,
}

impl Inodo {
    fn de_bytes(b: &[u8]) -> Inodo {
        let mut bloque = [0u32; 15];
        for i in 0..15 { bloque[i] = rd32(b, 40 + i * 4); }
        Inodo {
            modo: rd16(b, 0), uid: rd16(b, 2), tam: rd32(b, 4), atime: rd32(b, 8), ctime: rd32(b, 12),
            mtime: rd32(b, 16), dtime: rd32(b, 20), gid: rd16(b, 24), enlaces: rd16(b, 26),
            bloques512: rd32(b, 28), flags: rd32(b, 32), bloque, tam_alto: rd32(b, 108),
            crudo: b.to_vec(),
        }
    }
    fn nuevo(modo: u16, tam_inodo: usize) -> Inodo {
        let t = ahora();
        let mut i = Inodo::de_bytes(&alloc::vec![0u8; tam_inodo.max(128)]);
        i.modo = modo; i.atime = t; i.ctime = t; i.mtime = t; i.enlaces = 1;
        // [T-001] el nodo pertenece a quien lo crea (euid/egid del proceso), como en Linux
        i.uid = crate::process::current_euid().min(0xFFFF) as u16;
        i.gid = crate::process::current_egid().min(0xFFFF) as u16;
        i
    }
    fn a_bytes(&self) -> Vec<u8> {
        let mut b = self.crudo.clone();
        if b.len() < 128 { b.resize(128, 0); }
        wr16(&mut b, 0, self.modo); wr16(&mut b, 2, self.uid); wr32(&mut b, 4, self.tam);
        wr32(&mut b, 8, self.atime); wr32(&mut b, 12, self.ctime); wr32(&mut b, 16, self.mtime);
        wr32(&mut b, 20, self.dtime); wr16(&mut b, 24, self.gid); wr16(&mut b, 26, self.enlaces);
        wr32(&mut b, 28, self.bloques512); wr32(&mut b, 32, self.flags);
        for i in 0..15 { wr32(&mut b, 40 + i * 4, self.bloque[i]); }
        wr32(&mut b, 108, self.tam_alto);
        b
    }
    fn es_dir(&self) -> bool { self.modo & S_IFMT == S_IFDIR }
    fn es_reg(&self) -> bool { self.modo & S_IFMT == S_IFREG }
    fn es_lnk(&self) -> bool { self.modo & S_IFMT == S_IFLNK }
    fn tamano(&self) -> u64 {
        if self.es_reg() { ((self.tam_alto as u64) << 32) | self.tam as u64 } else { self.tam as u64 }
    }
    /// Enlace "rápido": destino guardado en el propio campo i_block.
    fn enlace_rapido(&self) -> bool { self.es_lnk() && self.bloques512 == 0 }
}

/// [T-001] Caché de bloques: `tags[i]` = número de bloque + 1 (0 = vacía).
struct CacheBloques {
    datos: Vec<u8>,
    tags: Vec<u32>,
    ranuras: usize,
    aciertos: u64,
    fallos: u64,
}

const CACHE_BYTES: usize = 2 * 1024 * 1024;

impl CacheBloques {
    fn nueva(bs: usize) -> CacheBloques {
        let ranuras = (CACHE_BYTES / bs).max(16);
        CacheBloques { datos: alloc::vec![0u8; ranuras * bs], tags: alloc::vec![0u32; ranuras], ranuras, aciertos: 0, fallos: 0 }
    }
    #[inline] fn ranura(&self, b: u32) -> usize { (b as usize) % self.ranuras }
    fn buscar(&mut self, b: u32, bs: usize, out: &mut [u8]) -> bool {
        let r = self.ranura(b);
        if self.tags[r] == b.wrapping_add(1) {
            out[..bs].copy_from_slice(&self.datos[r * bs..(r + 1) * bs]);
            self.aciertos += 1;
            true
        } else {
            self.fallos += 1;
            false
        }
    }
    fn guardar(&mut self, b: u32, bs: usize, src: &[u8]) {
        let r = self.ranura(b);
        self.datos[r * bs..(r + 1) * bs].copy_from_slice(&src[..bs]);
        self.tags[r] = b.wrapping_add(1);
    }
    fn descartar_todo(&mut self) { for t in self.tags.iter_mut() { *t = 0; } }
    fn invalidar(&mut self, b: u32) {
        let r = self.ranura(b);
        if self.tags[r] == b.wrapping_add(1) { self.tags[r] = 0; }
    }
}

pub struct Ext2 {
    cache: crate::arch::spinlock::Spinlock<CacheBloques>,
    prefijo: String,
    lba_base: u64,
    sectores: u64,
    bs: u32,
    sb: Vec<u8>,   // 1024 bytes
    gdt: Vec<u8>,  // grupos * 32, redondeado a bloques
    bloques_total: u32,
    inodos_total: u32,
    bpg: u32,
    ipg: u32,
    primer_dato: u32,
    tam_inodo: usize,
    grupos: u32,
    primer_inodo: u32,
    con_filetype: bool,
    sparse: bool,
    sucio: bool,
    /// [T-012] tick del PIT del último volcado de superbloque/GDT (debounce).
    ultimo_volcado: u64,
}

// ─── Descubrimiento de la partición ────────────────────────────────────────

/// GUID "Linux filesystem data" (0FC63DAF-8483-4772-8E79-3D69D8477DE4) en el
/// orden mixto con que GPT lo serializa.
const GUID_LINUX_FS: [u8; 16] = [
    0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47,
    0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4,
];

fn tiene_magia_ext2(d: &mut dyn crate::drivers::storage::traits::BlockDevice, lba: u64) -> bool {
    let mut sec = [0u8; 1024];
    if d.read_sectors(lba + 2, 2, &mut sec).is_err() { return false; }
    rd16(&sec, 56) == MAGIA
}

/// Busca la partición ext2 del disco 0: MBR tipo 0x83 con magia válida, o
/// entrada GPT "Linux filesystem". Devuelve (lba_inicio, sectores).
pub fn hallar_particion() -> Option<(u64, u64)> {
    registry::with_device(0, |d| {
        let d = d?;
        let mut mbr = [0u8; 512];
        d.read_sectors(0, 1, &mut mbr).ok()?;
        if crate::drivers::storage::gpt::is_gpt_disk(&mbr) {
            let mut cab = [0u8; 512];
            d.read_sectors(1, 1, &mut cab).ok()?;
            if &cab[0..8] != b"EFI PART" { return None; }
            let lba_ent = u64::from_le_bytes(cab[72..80].try_into().ok()?);
            let n = rd32(&cab, 80).min(128) as usize;
            let tam = rd32(&cab, 84) as usize;
            if tam < 128 { return None; }
            let por_sector = 512 / tam;
            if por_sector == 0 { return None; }
            let mut buf = [0u8; 512];
            for i in 0..n {
                let s = i / por_sector;
                let o = (i % por_sector) * tam;
                if o == 0 && d.read_sectors(lba_ent + s as u64, 1, &mut buf).is_err() { return None; }
                if buf[o..o + 16] == GUID_LINUX_FS {
                    let ini = u64::from_le_bytes(buf[o + 32..o + 40].try_into().ok()?);
                    let fin = u64::from_le_bytes(buf[o + 40..o + 48].try_into().ok()?);
                    if fin >= ini && tiene_magia_ext2(d, ini) { return Some((ini, fin - ini + 1)); }
                }
            }
            return None;
        }
        for i in 0..4usize {
            let o = 0x1BE + i * 16;
            if mbr[o + 4] != 0x83 { continue; }
            let ini = rd32(&mbr, o + 8) as u64;
            let n = rd32(&mbr, o + 12) as u64;
            if ini > 0 && n > 4 && tiene_magia_ext2(d, ini) { return Some((ini, n)); }
        }
        None
    })
}

// ─── E/S de bajo nivel ─────────────────────────────────────────────────────

/// Diagnóstico de E/S fallida del disco raíz (visible en la serie).
fn diag_disco(op: &str, lba: u64, n: usize, e: crate::drivers::storage::ata::AtaError) {
    use crate::drivers::serial;
    serial::write_str("[EXT2] E/S fallida: "); serial::write_str(op);
    serial::write_str(" lba="); serial::write_usize(lba as usize);
    serial::write_str(" n="); serial::write_usize(n);
    serial::write_str(" error="); serial::write_str(match e {
        crate::drivers::storage::ata::AtaError::Timeout => "timeout",
        crate::drivers::storage::ata::AtaError::DriveFault => "drive-fault",
        crate::drivers::storage::ata::AtaError::DeviceError(_) => "device-error",
        _ => "otro",
    });
    serial::write_str("\n");
}

impl Ext2 {
    fn spb(&self) -> u64 { (self.bs / 512) as u64 }

    fn leer_bloque(&self, b: u32, buf: &mut [u8]) -> Resultado<()> {
        if b >= self.bloques_total || buf.len() < self.bs as usize {
            crate::drivers::serial::log("EXT2", "leer_bloque: bloque fuera de rango o buffer corto");
            return Err(errno::EIO);
        }
        let bs = self.bs as usize;
        if self.cache.lock().buscar(b, bs, buf) { return Ok(()); }
        let lba = self.lba_base + b as u64 * self.spb();
        let r = registry::with_device(0, |d| match d {
            Some(d) => d.read_sectors(lba, self.spb() as usize, &mut buf[..bs]).map_err(|_| errno::EIO),
            None => Err(errno::EIO),
        });
        if r.is_ok() { self.cache.lock().guardar(b, bs, buf); }
        if r.is_err() {
            crate::drivers::serial::write_str("[EXT2] fallo de lectura en bloque ");
            crate::drivers::serial::write_usize(b as usize);
            crate::drivers::serial::write_str("\n");
        }
        r
    }

    fn escribir_bloque(&self, b: u32, buf: &[u8]) -> Resultado<()> {
        if b >= self.bloques_total || b < self.primer_dato { return Err(errno::EIO); }
        let lba = self.lba_base + b as u64 * self.spb();
        let r = registry::with_device(0, |d| match d {
            Some(d) => d.write_sectors(lba, self.spb() as usize, &buf[..self.bs as usize]).map_err(|_| errno::EIO),
            None => Err(errno::EIO),
        });
        // Escritura inmediata: la ranura refleja el disco; si el disco fallo,
        // se invalida para no servir un bloque que no esta escrito.
        let mut c = self.cache.lock();
        if r.is_ok() { c.guardar(b, self.bs as usize, buf); } else { c.invalidar(b); }
        r
    }

    /// [T-001] (aciertos, fallos, ranuras, bytes) de la caché de bloques.
    pub fn estadisticas_cache(&self) -> (u64, u64, usize, usize) {
        let c = self.cache.lock();
        (c.aciertos, c.fallos, c.ranuras, c.ranuras * self.bs as usize)
    }

    /// Escritura de bytes a un desplazamiento arbitrario de la partición
    /// (sólo para el superbloque, que vive a 1024 bytes).
    fn escribir_bytes(&self, off: u64, datos: &[u8]) -> Resultado<()> {
        let lba0 = self.lba_base + off / 512;
        let n = ((off % 512) as usize + datos.len() + 511) / 512;
        {
            // Los bloques que solapan dejan de ser fiables en la caché.
            let mut c = self.cache.lock();
            let b0 = (off / self.bs as u64) as u32;
            let b1 = ((off + datos.len() as u64 - 1) / self.bs as u64) as u32;
            for b in b0..=b1 { c.invalidar(b); }
        }
        let mut buf = alloc::vec![0u8; n * 512];
        registry::with_device(0, |d| match d {
            Some(d) => {
                d.read_sectors(lba0, n, &mut buf).map_err(|e| { diag_disco("read", lba0, n, e); errno::EIO })?;
                let o = (off % 512) as usize;
                buf[o..o + datos.len()].copy_from_slice(datos);
                d.write_sectors(lba0, n, &buf).map_err(|e| { diag_disco("write", lba0, n, e); errno::EIO })
            }
            None => { diag_disco("sin-dispositivo", lba0, n, crate::drivers::storage::ata::AtaError::Timeout); Err(errno::EIO) }
        })
    }

    fn vaciar_cache_disco(&self) -> Resultado<()> {
        registry::with_device(0, |d| match d {
            Some(d) => d.flush_cache().map_err(|_| errno::EIO),
            None => Err(errno::EIO),
        })
    }

    // ─── Montaje ───────────────────────────────────────────────────────────

    pub fn montar(lba_base: u64, sectores: u64) -> Result<Ext2, &'static str> {
        let mut sb = alloc::vec![0u8; 1024];
        let ok = registry::with_device(0, |d| match d {
            Some(d) => d.read_sectors(lba_base + 2, 2, &mut sb).is_ok(),
            None => false,
        });
        if !ok { return Err("no se pudo leer el superbloque"); }
        if rd16(&sb, 56) != MAGIA { return Err("magia ext2 ausente"); }
        let rev = rd32(&sb, 76);
        let log_bs = rd32(&sb, 24);
        if log_bs > 2 { return Err("tamaño de bloque no soportado"); }
        let bs = 1024u32 << log_bs;
        let (incompat, ro_compat) = if rev >= 1 { (rd32(&sb, 96), rd32(&sb, 100)) } else { (0, 0) };
        if incompat & !INCOMPAT_FILETYPE != 0 { return Err("caracteristicas incompat desconocidas (¿ext3/ext4?)"); }
        if ro_compat & !(ROCOMPAT_SPARSE_SUPER | ROCOMPAT_LARGE_FILE) != 0 { return Err("caracteristicas ro_compat no mantenibles"); }
        let tam_inodo = if rev >= 1 { rd16(&sb, 88) as usize } else { 128 };
        if tam_inodo < 128 || tam_inodo as u32 > bs { return Err("tamaño de inodo invalido"); }
        let bloques_total = rd32(&sb, 4);
        let bpg = rd32(&sb, 32);
        let ipg = rd32(&sb, 40);
        if bpg == 0 || ipg == 0 || bloques_total == 0 { return Err("geometria invalida"); }
        if (bloques_total as u64) * (bs as u64) > sectores * 512 { return Err("el volumen excede la particion"); }
        let primer_dato = rd32(&sb, 20);
        let grupos = (bloques_total - primer_dato + bpg - 1) / bpg;
        let gdt_bloques = (grupos * 32 + bs - 1) / bs;
        let mut fs = Ext2 {
            cache: crate::arch::spinlock::Spinlock::new(CacheBloques::nueva(bs as usize)),
            prefijo: String::new(), lba_base, sectores, bs, sb, gdt: alloc::vec![0u8; (gdt_bloques * bs) as usize],
            bloques_total, inodos_total: 0, bpg, ipg, primer_dato, tam_inodo, grupos,
            primer_inodo: if rev >= 1 { 11 } else { 11 },
            con_filetype: incompat & INCOMPAT_FILETYPE != 0,
            sparse: ro_compat & ROCOMPAT_SPARSE_SUPER != 0,
            sucio: false,
            ultimo_volcado: 0,
        };
        fs.inodos_total = rd32(&fs.sb, 0);
        if rev >= 1 { fs.primer_inodo = rd32(&fs.sb, 84).max(11); }
        let gdt_ini = primer_dato + 1;
        for i in 0..gdt_bloques {
            let o = (i * bs) as usize;
            let mut tmp = alloc::vec![0u8; bs as usize];
            fs.leer_bloque(gdt_ini + i, &mut tmp).map_err(|_| "no se pudo leer la tabla de grupos")?;
            fs.gdt[o..o + bs as usize].copy_from_slice(&tmp);
        }
        // Estado: montado (como hace Linux: mnt_count+1, mtime). `state` se
        // deja en 1 (limpio) porque toda operación confirma antes de volver.
        let mnt = rd16(&fs.sb, 52).wrapping_add(1);
        wr16(&mut fs.sb, 52, mnt);
        wr32(&mut fs.sb, 44, ahora());
        fs.sucio = true;
        fs.confirmar().map_err(|_| "no se pudo escribir el superbloque (¿disco de solo lectura?)")?;
        Ok(fs)
    }

    fn tiene_super(&self, g: u32) -> bool {
        if !self.sparse || g <= 1 { return true; }
        let mut p = 3u32; while p < g { p *= 3; } if p == g { return true; }
        let mut p = 5u32; while p < g { p *= 5; } if p == g { return true; }
        let mut p = 7u32; while p < g { p *= 7; } p == g
    }

    /// Vuelca superbloque y GDT (primario y copias) y vacía la caché del disco.
    ///
    /// [T-012, 89ª] Con debounce de 1 s: antes CADA `create`/`write`/`chmod`
    /// reescribía las 5 copias del superbloque y de la GDT (lectura-modificación
    /// -escritura por PIO) y ordenaba un FLUSH CACHE (fsync del fichero imagen
    /// en el anfitrión): ~60 ms por fichero desplegado, el 90 % del tiempo de
    /// `pkg deb-add` (libc6: 286 ficheros, 19 s). Los bloques de datos, inodos,
    /// directorios y mapas de bits siguen escribiéndose en el acto (write-
    /// through); sólo los contadores del superbloque/GDT y el vaciado de la
    /// caché del disco se difieren como máximo 1 s, o hasta `fsync`/`sync`/
    /// desmontaje (`sincronizar` fuerza el volcado).
    fn confirmar(&mut self) -> Resultado<()> {
        if !self.sucio { return Ok(()); }
        let t = crate::time::pit::ticks();
        if t.wrapping_sub(self.ultimo_volcado) < 100 { return Ok(()); }
        self.volcar_ahora()
    }

    fn volcar_ahora(&mut self) -> Resultado<()> {
        self.ultimo_volcado = crate::time::pit::ticks();
        wr32(&mut self.sb, 48, ahora());
        let gdt_bloques = (self.gdt.len() as u32) / self.bs;
        for g in 0..self.grupos {
            if !self.tiene_super(g) { continue; }
            let ini = self.primer_dato + g * self.bpg;
            if g == 0 {
                self.escribir_bytes(1024, &self.sb)?;
            } else {
                let mut sbg = self.sb.clone();
                wr16(&mut sbg, 90, g as u16); // s_block_group_nr
                self.escribir_bytes(ini as u64 * self.bs as u64, &sbg)?;
            }
            for i in 0..gdt_bloques {
                let o = (i * self.bs) as usize;
                self.escribir_bloque(ini + 1 + i, &self.gdt[o..o + self.bs as usize])?;
            }
        }
        self.vaciar_cache_disco()?;
        self.sucio = false;
        Ok(())
    }

    // ─── Descriptores de grupo ─────────────────────────────────────────────

    fn gd(&self, g: u32, campo: usize) -> u32 {
        let o = (g * 32) as usize + campo;
        if campo >= 12 { rd16(&self.gdt, o) as u32 } else { rd32(&self.gdt, o) }
    }
    fn gd_set16(&mut self, g: u32, campo: usize, v: u16) { let o = (g * 32) as usize + campo; wr16(&mut self.gdt, o, v); self.sucio = true; }
    fn sb_libres_bloques(&self) -> u32 { rd32(&self.sb, 12) }
    fn sb_libres_inodos(&self) -> u32 { rd32(&self.sb, 16) }

    fn bloques_en_grupo(&self, g: u32) -> u32 {
        let ini = self.primer_dato + g * self.bpg;
        (self.bloques_total - ini).min(self.bpg)
    }

    // ─── Inodos ────────────────────────────────────────────────────────────

    fn pos_inodo(&self, num: u32) -> Resultado<(u32, usize)> {
        if num == 0 || num > self.inodos_total { return Err(errno::EIO); }
        let g = (num - 1) / self.ipg;
        let idx = ((num - 1) % self.ipg) as usize;
        let tabla = self.gd(g, 8);
        let off = idx * self.tam_inodo;
        Ok((tabla + (off / self.bs as usize) as u32, off % self.bs as usize))
    }

    fn leer_inodo(&self, num: u32) -> Resultado<Inodo> {
        let (b, o) = self.pos_inodo(num)?;
        let mut buf = alloc::vec![0u8; self.bs as usize];
        self.leer_bloque(b, &mut buf)?;
        Ok(Inodo::de_bytes(&buf[o..o + self.tam_inodo]))
    }

    fn escribir_inodo(&self, num: u32, ino: &Inodo) -> Resultado<()> {
        let (b, o) = self.pos_inodo(num)?;
        let mut buf = alloc::vec![0u8; self.bs as usize];
        self.leer_bloque(b, &mut buf)?;
        let bytes = ino.a_bytes();
        let n = bytes.len().min(self.tam_inodo);
        buf[o..o + n].copy_from_slice(&bytes[..n]);
        self.escribir_bloque(b, &buf)
    }

    fn asignar_inodo(&mut self, g_pref: u32, es_dir: bool) -> Resultado<u32> {
        for k in 0..self.grupos {
            let g = (g_pref + k) % self.grupos;
            if self.gd(g, 14) == 0 { continue; }
            let mapa_b = self.gd(g, 4);
            let mut mapa = alloc::vec![0u8; self.bs as usize];
            self.leer_bloque(mapa_b, &mut mapa)?;
            let n = self.ipg as usize;
            for i in 0..n {
                if mapa[i / 8] & (1 << (i % 8)) == 0 {
                    let num = g * self.ipg + i as u32 + 1;
                    if num < self.primer_inodo { continue; }
                    mapa[i / 8] |= 1 << (i % 8);
                    self.escribir_bloque(mapa_b, &mapa)?;
                    let li = self.gd(g, 14) as u16 - 1;
                    self.gd_set16(g, 14, li);
                    if es_dir { let d = self.gd(g, 16) as u16 + 1; self.gd_set16(g, 16, d); }
                    let sl = self.sb_libres_inodos().saturating_sub(1);
                    wr32(&mut self.sb, 16, sl);
                    self.sucio = true;
                    return Ok(num);
                }
            }
        }
        Err(errno::ENOSPC)
    }

    fn liberar_inodo(&mut self, num: u32, es_dir: bool) -> Resultado<()> {
        if num < self.primer_inodo && num != RAIZ { return Err(errno::EIO); }
        let g = (num - 1) / self.ipg;
        let i = ((num - 1) % self.ipg) as usize;
        let mapa_b = self.gd(g, 4);
        let mut mapa = alloc::vec![0u8; self.bs as usize];
        self.leer_bloque(mapa_b, &mut mapa)?;
        if mapa[i / 8] & (1 << (i % 8)) == 0 { return Err(errno::EIO); } // doble liberación: incoherencia real
        mapa[i / 8] &= !(1 << (i % 8));
        self.escribir_bloque(mapa_b, &mapa)?;
        let li = self.gd(g, 14) as u16 + 1;
        self.gd_set16(g, 14, li);
        if es_dir { let d = (self.gd(g, 16) as u16).saturating_sub(1); self.gd_set16(g, 16, d); }
        let sl = self.sb_libres_inodos() + 1;
        wr32(&mut self.sb, 16, sl);
        self.sucio = true;
        Ok(())
    }

    // ─── Bloques ───────────────────────────────────────────────────────────

    fn asignar_bloque(&mut self, g_pref: u32, poner_a_cero: bool) -> Resultado<u32> {
        for k in 0..self.grupos {
            let g = (g_pref + k) % self.grupos;
            if self.gd(g, 12) == 0 { continue; }
            let mapa_b = self.gd(g, 0);
            let mut mapa = alloc::vec![0u8; self.bs as usize];
            self.leer_bloque(mapa_b, &mut mapa)?;
            let n = self.bloques_en_grupo(g) as usize;
            for i in 0..n {
                if mapa[i / 8] & (1 << (i % 8)) == 0 {
                    mapa[i / 8] |= 1 << (i % 8);
                    self.escribir_bloque(mapa_b, &mapa)?;
                    let lb = self.gd(g, 12) as u16 - 1;
                    self.gd_set16(g, 12, lb);
                    let sl = self.sb_libres_bloques().saturating_sub(1);
                    wr32(&mut self.sb, 12, sl);
                    self.sucio = true;
                    let b = self.primer_dato + g * self.bpg + i as u32;
                    if poner_a_cero {
                        let ceros = alloc::vec![0u8; self.bs as usize];
                        self.escribir_bloque(b, &ceros)?;
                    }
                    return Ok(b);
                }
            }
        }
        Err(errno::ENOSPC)
    }

    fn liberar_bloque(&mut self, b: u32) -> Resultado<()> {
        if b < self.primer_dato || b >= self.bloques_total { return Err(errno::EIO); }
        let g = (b - self.primer_dato) / self.bpg;
        let i = ((b - self.primer_dato) % self.bpg) as usize;
        let mapa_b = self.gd(g, 0);
        let mut mapa = alloc::vec![0u8; self.bs as usize];
        self.leer_bloque(mapa_b, &mut mapa)?;
        if mapa[i / 8] & (1 << (i % 8)) == 0 { return Err(errno::EIO); }
        mapa[i / 8] &= !(1 << (i % 8));
        self.escribir_bloque(mapa_b, &mapa)?;
        let lb = self.gd(g, 12) as u16 + 1;
        self.gd_set16(g, 12, lb);
        let sl = self.sb_libres_bloques() + 1;
        wr32(&mut self.sb, 12, sl);
        self.sucio = true;
        Ok(())
    }

    fn grupo_de_inodo(&self, num: u32) -> u32 { (num.saturating_sub(1)) / self.ipg }
    fn punteros_por_bloque(&self) -> u32 { self.bs / 4 }

    /// [T-001] Geometría del bloque lógico `n`: (nivel, índices por nivel).
    /// nivel 0 = directo (índice en i_block), 1..3 = indirecto simple/doble/
    /// triple con los índices de cada salto. `EFBIG` si excede el triple.
    fn geometria(&self, n: u32) -> Resultado<(usize, [u32; 3])> {
        let ppb = self.punteros_por_bloque() as u64;
        let n = n as u64;
        if n < 12 { return Ok((0, [n as u32, 0, 0])); }
        let n1 = n - 12;
        if n1 < ppb { return Ok((1, [n1 as u32, 0, 0])); }
        let n2 = n1 - ppb;
        if n2 < ppb * ppb { return Ok((2, [(n2 / ppb) as u32, (n2 % ppb) as u32, 0])); }
        let n3 = n2 - ppb * ppb;
        if n3 < ppb * ppb * ppb {
            return Ok((3, [(n3 / (ppb * ppb)) as u32, ((n3 / ppb) % ppb) as u32, (n3 % ppb) as u32]));
        }
        Err(errno::EFBIG)
    }

    /// [L3] Como `mapear`, pero reutiliza el último bloque indirecto leído
    /// (`cache` = (número de bloque, contenido)): en una lectura secuencial de
    /// un ELF de varios MiB evita releer el mismo indirecto por cada bloque.
    fn mapear_cacheado(&self, ino: &Inodo, n: u32, cache: &mut (u32, Vec<u8>)) -> Resultado<u32> {
        let (nivel, idx) = self.geometria(n)?;
        if nivel == 0 { return Ok(ino.bloque[idx[0] as usize]); }
        let mut b = ino.bloque[11 + nivel];
        let mut buf = alloc::vec![0u8; self.bs as usize];
        for paso in 0..nivel {
            if b == 0 { return Ok(0); }
            if paso + 1 == nivel {
                // último salto: es el que se repite en lecturas secuenciales
                if cache.0 != b {
                    if cache.1.len() != self.bs as usize { cache.1 = alloc::vec![0u8; self.bs as usize]; }
                    self.leer_bloque(b, &mut cache.1)?;
                    cache.0 = b;
                }
                return Ok(rd32(&cache.1, (idx[paso] * 4) as usize));
            }
            self.leer_bloque(b, &mut buf)?;
            b = rd32(&buf, (idx[paso] * 4) as usize);
        }
        Ok(b)
    }

    /// Bloque físico del bloque lógico `n` (0 = agujero).
    fn mapear(&self, ino: &Inodo, n: u32) -> Resultado<u32> {
        let (nivel, idx) = self.geometria(n)?;
        if nivel == 0 { return Ok(ino.bloque[idx[0] as usize]); }
        let mut b = ino.bloque[11 + nivel];
        let mut buf = alloc::vec![0u8; self.bs as usize];
        for paso in 0..nivel {
            if b == 0 { return Ok(0); }
            self.leer_bloque(b, &mut buf)?;
            b = rd32(&buf, (idx[paso] * 4) as usize);
        }
        Ok(b)
    }

    /// Como `mapear`, pero asigna el bloque (y los indirectos) si no existe.
    fn mapear_o_asignar(&mut self, num: u32, ino: &mut Inodo, n: u32) -> Resultado<u32> {
        let existente = self.mapear(ino, n)?;
        if existente != 0 { return Ok(existente); }
        let g = self.grupo_de_inodo(num);
        let spb = self.bs / 512;
        let (nivel, idx) = self.geometria(n)?;
        if nivel == 0 {
            let b = self.asignar_bloque(g, false)?;
            ino.bloque[idx[0] as usize] = b; ino.bloques512 += spb;
            return Ok(b);
        }
        if ino.bloque[11 + nivel] == 0 {
            ino.bloque[11 + nivel] = self.asignar_bloque(g, true)?; ino.bloques512 += spb;
        }
        let mut cur = ino.bloque[11 + nivel];
        let mut buf = alloc::vec![0u8; self.bs as usize];
        for paso in 0..nivel {
            self.leer_bloque(cur, &mut buf)?;
            let o = (idx[paso] * 4) as usize;
            let mut hijo = rd32(&buf, o);
            if hijo == 0 {
                let es_dato = paso + 1 == nivel;
                hijo = self.asignar_bloque(g, !es_dato)?; ino.bloques512 += spb;
                wr32(&mut buf, o, hijo);
                self.escribir_bloque(cur, &buf)?;
            }
            cur = hijo;
        }
        Ok(cur)
    }

    /// [T-001] Libera, dentro del subárbol indirecto `b` de `nivel` saltos, los
    /// bloques lógicos con índice relativo >= `ini`. Devuelve si quedan
    /// bloques vivos (si no, el llamador libera `b` también).
    fn liberar_subarbol(&mut self, b: u32, nivel: usize, ini: u64, ino: &mut Inodo) -> Resultado<bool> {
        let ppb = self.punteros_por_bloque() as u64;
        let spb = self.bs / 512;
        let span: u64 = if nivel == 1 { 1 } else if nivel == 2 { ppb } else { ppb * ppb };
        let mut buf = alloc::vec![0u8; self.bs as usize];
        self.leer_bloque(b, &mut buf)?;
        let mut quedan = false;
        let mut cambiado = false;
        for j in 0..ppb {
            let o = (j * 4) as usize;
            let hijo = rd32(&buf, o);
            if hijo == 0 { continue; }
            let desde_hijo = ini.saturating_sub(j * span);
            if desde_hijo >= span { quedan = true; continue; }   // intacto
            let vivo = if nivel == 1 {
                false
            } else {
                self.liberar_subarbol(hijo, nivel - 1, desde_hijo, ino)?
            };
            if vivo { quedan = true; }
            else {
                self.liberar_bloque(hijo)?;
                wr32(&mut buf, o, 0); cambiado = true;
                ino.bloques512 = ino.bloques512.saturating_sub(spb);
            }
        }
        if quedan && cambiado { self.escribir_bloque(b, &buf)?; }
        Ok(quedan)
    }

    /// Libera todos los bloques lógicos >= `desde` (y los indirectos que queden vacíos).
    fn liberar_desde(&mut self, ino: &mut Inodo, desde: u32) -> Resultado<()> {
        let ppb = self.punteros_por_bloque() as u64;
        let spb = self.bs / 512;
        for i in (desde.min(12) as usize)..12 {
            if ino.bloque[i] != 0 { self.liberar_bloque(ino.bloque[i])?; ino.bloque[i] = 0; ino.bloques512 = ino.bloques512.saturating_sub(spb); }
        }
        let bases = [12u64, 12 + ppb, 12 + ppb + ppb * ppb];
        for nivel in 1..=3usize {
            let raiz = ino.bloque[11 + nivel];
            if raiz == 0 { continue; }
            let ini = (desde as u64).saturating_sub(bases[nivel - 1]);
            let quedan = self.liberar_subarbol(raiz, nivel, ini, ino)?;
            if !quedan {
                self.liberar_bloque(raiz)?; ino.bloque[11 + nivel] = 0;
                ino.bloques512 = ino.bloques512.saturating_sub(spb);
            }
        }
        Ok(())
    }

    // ─── Directorios ───────────────────────────────────────────────────────

    /// Recorre las entradas válidas de un directorio: f(ino, tipo, nombre).
    fn recorrer_dir(&self, dir: &Inodo, f: &mut dyn FnMut(u32, u8, &[u8]) -> bool) -> Resultado<()> {
        let nb = (dir.tam as u64 + self.bs as u64 - 1) / self.bs as u64;
        let mut buf = alloc::vec![0u8; self.bs as usize];
        for lb in 0..nb as u32 {
            let b = self.mapear(dir, lb)?;
            if b == 0 { continue; }
            self.leer_bloque(b, &mut buf)?;
            let mut o = 0usize;
            while o + 8 <= self.bs as usize {
                let ino = rd32(&buf, o);
                let rec = rd16(&buf, o + 4) as usize;
                let nl = buf[o + 6] as usize;
                if rec < 8 || o + rec > self.bs as usize {
                    crate::drivers::serial::log("EXT2", "entrada de directorio con rec_len invalido");
                    return Err(errno::EIO);
                }
                if ino != 0 && nl > 0 && o + 8 + nl <= self.bs as usize {
                    if !f(ino, buf[o + 7], &buf[o + 8..o + 8 + nl]) { return Ok(()); }
                }
                o += rec;
            }
        }
        Ok(())
    }

    fn buscar(&self, dir: &Inodo, nombre: &str) -> Resultado<Option<u32>> {
        let mut hallado = None;
        self.recorrer_dir(dir, &mut |ino, _t, n| {
            if n == nombre.as_bytes() { hallado = Some(ino); false } else { true }
        })?;
        Ok(hallado)
    }

    fn dir_vacio(&self, dir: &Inodo) -> Resultado<bool> {
        let mut vacio = true;
        self.recorrer_dir(dir, &mut |_i, _t, n| {
            if n != b"." && n != b".." { vacio = false; false } else { true }
        })?;
        Ok(vacio)
    }

    fn tipo_entrada(&self, modo: u16) -> u8 {
        if !self.con_filetype { return 0; }
        match modo & S_IFMT { S_IFDIR => TIPO_DIR, S_IFLNK => TIPO_LNK, S_IFREG => TIPO_REG, S_IFIFO => TIPO_FIFO, _ => 0 }
    }

    fn anadir_entrada(&mut self, dnum: u32, dir: &mut Inodo, nombre: &str, ino: u32, tipo: u8) -> Resultado<()> {
        let nl = nombre.len();
        if nl == 0 || nl > NOMBRE_MAX { return Err(errno::ENAMETOOLONG); }
        let necesario = (8 + nl + 3) & !3;
        let nb = (dir.tam as u64 + self.bs as u64 - 1) / self.bs as u64;
        let mut buf = alloc::vec![0u8; self.bs as usize];
        for lb in 0..nb as u32 {
            let b = self.mapear(dir, lb)?;
            if b == 0 { continue; }
            self.leer_bloque(b, &mut buf)?;
            let mut o = 0usize;
            while o + 8 <= self.bs as usize {
                let e_ino = rd32(&buf, o);
                let rec = rd16(&buf, o + 4) as usize;
                if rec < 8 || o + rec > self.bs as usize { return Err(errno::EIO); }
                let usado = if e_ino == 0 { 0 } else { (8 + buf[o + 6] as usize + 3) & !3 };
                if rec - usado >= necesario {
                    let pos = if e_ino == 0 { o } else { wr16(&mut buf, o + 4, usado as u16); o + usado };
                    let rec_nuevo = if e_ino == 0 { rec } else { rec - usado };
                    wr32(&mut buf, pos, ino);
                    wr16(&mut buf, pos + 4, rec_nuevo as u16);
                    buf[pos + 6] = nl as u8;
                    buf[pos + 7] = tipo;
                    buf[pos + 8..pos + 8 + nl].copy_from_slice(nombre.as_bytes());
                    self.escribir_bloque(b, &buf)?;
                    dir.mtime = ahora(); dir.ctime = dir.mtime;
                    return self.escribir_inodo(dnum, dir);
                }
                o += rec;
            }
        }
        // Sin hueco: bloque nuevo al final del directorio.
        let lb = nb as u32;
        let b = self.mapear_o_asignar(dnum, dir, lb)?;
        for x in buf.iter_mut() { *x = 0; }
        wr32(&mut buf, 0, ino);
        wr16(&mut buf, 4, self.bs as u16);
        buf[6] = nl as u8; buf[7] = tipo;
        buf[8..8 + nl].copy_from_slice(nombre.as_bytes());
        self.escribir_bloque(b, &buf)?;
        dir.tam += self.bs;
        dir.mtime = ahora(); dir.ctime = dir.mtime;
        self.escribir_inodo(dnum, dir)
    }

    fn quitar_entrada(&mut self, dnum: u32, dir: &mut Inodo, nombre: &str) -> Resultado<()> {
        let nb = (dir.tam as u64 + self.bs as u64 - 1) / self.bs as u64;
        let mut buf = alloc::vec![0u8; self.bs as usize];
        for lb in 0..nb as u32 {
            let b = self.mapear(dir, lb)?;
            if b == 0 { continue; }
            self.leer_bloque(b, &mut buf)?;
            let mut o = 0usize;
            let mut prev: Option<usize> = None;
            while o + 8 <= self.bs as usize {
                let e_ino = rd32(&buf, o);
                let rec = rd16(&buf, o + 4) as usize;
                let nl = buf[o + 6] as usize;
                if rec < 8 || o + rec > self.bs as usize { return Err(errno::EIO); }
                if e_ino != 0 && o + 8 + nl <= self.bs as usize && &buf[o + 8..o + 8 + nl] == nombre.as_bytes() {
                    match prev {
                        Some(p) => { let pr = rd16(&buf, p + 4) as usize; wr16(&mut buf, p + 4, (pr + rec) as u16); }
                        None => { wr32(&mut buf, o, 0); }
                    }
                    self.escribir_bloque(b, &buf)?;
                    dir.mtime = ahora(); dir.ctime = dir.mtime;
                    return self.escribir_inodo(dnum, dir);
                }
                prev = Some(o);
                o += rec;
            }
        }
        Err(errno::ENOENT)
    }

    /// Reescribe la entrada ".." de un directorio (al moverlo de padre).
    fn fijar_padre(&mut self, dir: &Inodo, nuevo_padre: u32) -> Resultado<()> {
        let b = self.mapear(dir, 0)?;
        if b == 0 { return Err(errno::EIO); }
        let mut buf = alloc::vec![0u8; self.bs as usize];
        self.leer_bloque(b, &mut buf)?;
        let rec0 = rd16(&buf, 4) as usize;
        if rec0 < 12 || rec0 + 8 > self.bs as usize || &buf[rec0 + 8..rec0 + 10] != b".." { return Err(errno::EIO); }
        wr32(&mut buf, rec0, nuevo_padre);
        self.escribir_bloque(b, &buf)
    }

    // ─── Rutas ─────────────────────────────────────────────────────────────

    /// Ruta relativa al montaje, sin barras sobrantes.
    fn relativa<'a>(&self, ruta: &'a str) -> Resultado<&'a str> {
        let c = limpiar(ruta);
        let p = self.prefijo.as_str();
        if p.is_empty() || p == "/" { return Ok(c); }
        if c == p { return Ok("/"); }
        if let Some(resto) = c.strip_prefix(p) {
            if resto.starts_with('/') { return Ok(resto); }
        }
        Err(errno::ENOENT)
    }

    /// [T-002] Destino de un enlace simbólico (rápido en `i_block` o en su bloque 0).
    fn destino_enlace(&self, ino: &Inodo) -> Resultado<String> {
        let n = (ino.tam as usize).min(self.bs as usize);
        let mut bytes = alloc::vec![0u8; n];
        if ino.enlace_rapido() {
            let mut crudo = [0u8; 60];
            for i in 0..15 { wr32(&mut crudo, i * 4, ino.bloque[i]); }
            bytes.copy_from_slice(&crudo[..n.min(60)]);
        } else {
            let b = self.mapear(ino, 0)?;
            if b == 0 { return Err(errno::EIO); }
            let mut buf = alloc::vec![0u8; self.bs as usize];
            self.leer_bloque(b, &mut buf)?;
            bytes.copy_from_slice(&buf[..n]);
        }
        String::from_utf8(bytes).map_err(|_| errno::EIO)
    }

    /// [T-002] Recorrido de ruta que SIGUE los enlaces simbólicos (los
    /// intermedios siempre; el último sólo si `seguir_ultimo`), como hace el
    /// VFS de Linux: destinos relativos se resuelven desde el directorio del
    /// enlace, absolutos desde la raíz de este montaje; más de 40 saltos →
    /// ELOOP. Antes `/usr/lib/libc.so -> ../../lib/ld-musl-x86_64.so.1` (musl-dev
    /// de Alpine) se "leía" como fichero regular y salían bloques basura.
    fn resolver_con(&self, ruta: &str, seguir_ultimo: bool) -> Resultado<(u32, Inodo)> {
        let mut rel = String::from(self.relativa(ruta)?);
        for _salto in 0..40 {
            let comps: Vec<String> = rel.split('/').filter(|c| !c.is_empty() && *c != ".").map(String::from).collect();
            let mut num = RAIZ;
            let mut ino = self.leer_inodo(num)?;
            let mut recorrido = String::new();
            let mut reinicio: Option<String> = None;
            for (i, comp) in comps.iter().enumerate() {
                if !ino.es_dir() { return Err(errno::ENOTDIR); }
                if comp.len() > NOMBRE_MAX { return Err(errno::ENAMETOOLONG); }
                num = self.buscar(&ino, comp)?.ok_or(errno::ENOENT)?;
                ino = self.leer_inodo(num)?;
                let ultimo = i + 1 == comps.len();
                if ino.es_lnk() && (!ultimo || seguir_ultimo) {
                    let destino = self.destino_enlace(&ino)?;
                    let resto = comps[i + 1..].join("/");
                    let mut nueva = if destino.starts_with('/') {
                        String::from(self.relativa(&destino)?)
                    } else {
                        alloc::format!("{}/{}", recorrido, destino)
                    };
                    if !resto.is_empty() { nueva.push('/'); nueva.push_str(&resto); }
                    reinicio = Some(nueva);
                    break;
                }
                recorrido.push('/');
                recorrido.push_str(comp);
            }
            match reinicio {
                Some(n) => rel = n,
                None => return Ok((num, ino)),
            }
        }
        Err(errno::ELOOP)
    }

    fn resolver(&self, ruta: &str) -> Resultado<(u32, Inodo)> { self.resolver_con(ruta, true) }

    /// (número del padre, inodo del padre, nombre final). ENOTDIR si algún
    /// componente intermedio no es directorio; ENOENT si no existe.
    fn resolver_padre<'a>(&self, ruta: &'a str) -> Resultado<(u32, Inodo, &'a str)> {
        let rel = self.relativa(ruta)?;
        if rel == "/" { return Err(errno::EEXIST); }
        let (padre, nombre) = match rel.rfind('/') { Some(i) => (&rel[..i], &rel[i + 1..]), None => ("/", rel) };
        if nombre.is_empty() || nombre == "." || nombre == ".." { return Err(errno::EINVAL); }
        if nombre.len() > NOMBRE_MAX { return Err(errno::ENAMETOOLONG); }
        let padre_abs = if padre.is_empty() { "/" } else { padre };
        // [T-002] el directorio padre se resuelve siguiendo enlaces simbólicos
        // (p. ej. `/usr/lib64 -> lib`), el nombre final nunca.
        let abs = if self.prefijo.is_empty() || self.prefijo == "/" { String::from(padre_abs) } else if padre_abs == "/" { self.prefijo.clone() } else { alloc::format!("{}{}", self.prefijo, padre_abs) };
        let (num, ino) = self.resolver_con(&abs, true)?;
        if !ino.es_dir() { return Err(errno::ENOTDIR); }
        Ok((num, ino, nombre))
    }

    fn metadatos_de(&self, num: u32, ino: &Inodo) -> Metadatos {
        Metadatos {
            modo: ino.modo as u32, uid: ino.uid as u32, gid: ino.gid as u32, tam: ino.tamano(),
            ino: num as u64, nlink: ino.enlaces as u32, mtime: ino.mtime as u64, bloques: ino.bloques512 as u64,
        }
    }

    /// Libera el contenido y el inodo (cuando el contador de enlaces llega a 0).
    fn destruir_inodo(&mut self, num: u32, ino: &mut Inodo) -> Resultado<()> {
        let es_dir = ino.es_dir();
        if !ino.enlace_rapido() { self.liberar_desde(ino, 0)?; }
        ino.dtime = ahora(); ino.enlaces = 0; ino.tam = 0; ino.tam_alto = 0;
        if ino.enlace_rapido() { ino.bloque = [0; 15]; }
        self.escribir_inodo(num, ino)?;
        self.liberar_inodo(num, es_dir)
    }

    fn crear_nodo(&mut self, ruta: &str, modo: u16) -> Resultado<(u32, Inodo, u32, Inodo)> {
        let (pnum, mut padre, nombre) = self.resolver_padre(ruta)?;
        if self.buscar(&padre, nombre)?.is_some() { return Err(errno::EEXIST); }
        let num = self.asignar_inodo(self.grupo_de_inodo(pnum), modo & S_IFMT == S_IFDIR)?;
        let ino = Inodo::nuevo(modo, self.tam_inodo);
        self.escribir_inodo(num, &ino)?;
        let tipo = self.tipo_entrada(modo);
        if let Err(e) = self.anadir_entrada(pnum, &mut padre, nombre, num, tipo) {
            let _ = self.liberar_inodo(num, modo & S_IFMT == S_IFDIR);
            return Err(e);
        }
        Ok((num, ino, pnum, padre))
    }

    pub fn resumen(&self) -> (u32, u32, u32, u32, u32) {
        (self.bs, self.bloques_total, self.sb_libres_bloques(), self.inodos_total, self.sb_libres_inodos())
    }
}

// ─── Contrato ──────────────────────────────────────────────────────────────

impl SistemaDeFicheros for Ext2 {
    fn tipo(&self) -> &'static str { "ext2" }
    fn montado_en(&self) -> &str { &self.prefijo }
    fn fijar_montaje(&mut self, prefijo: &str) { self.prefijo = String::from(limpiar(prefijo)); }

    fn metadatos(&self, ruta: &str) -> Resultado<Metadatos> {
        let (num, ino) = self.resolver(ruta)?;
        Ok(self.metadatos_de(num, &ino))
    }

    fn leer(&mut self, ruta: &str, desplazamiento: u64, buf: &mut [u8]) -> Resultado<usize> {
        let (_num, ino) = self.resolver(ruta)?;
        if ino.es_dir() { return Err(errno::EISDIR); }
        let tam = ino.tamano();
        if desplazamiento >= tam { return Ok(0); }
        let total = ((tam - desplazamiento) as usize).min(buf.len());
        let mut hecho = 0usize;
        let mut bloque = alloc::vec![0u8; self.bs as usize];
        let mut cache_ind: (u32, Vec<u8>) = (0, Vec::new());
        while hecho < total {
            let pos = desplazamiento + hecho as u64;
            let lb = (pos / self.bs as u64) as u32;
            let o = (pos % self.bs as u64) as usize;
            let n = (self.bs as usize - o).min(total - hecho);
            let b = self.mapear_cacheado(&ino, lb, &mut cache_ind)?;
            if b == 0 {
                for x in buf[hecho..hecho + n].iter_mut() { *x = 0; }
            } else if o == 0 && n == self.bs as usize {
                // bloque completo: directamente al destino, sin copia intermedia
                self.leer_bloque(b, &mut buf[hecho..hecho + n])?;
            } else {
                self.leer_bloque(b, &mut bloque)?;
                buf[hecho..hecho + n].copy_from_slice(&bloque[o..o + n]);
            }
            hecho += n;
        }
        Ok(hecho)
    }

    fn escribir(&mut self, ruta: &str, desplazamiento: u64, datos: &[u8]) -> Resultado<usize> {
        let (num, mut ino) = self.resolver(ruta)?;
        if ino.es_dir() { return Err(errno::EISDIR); }
        if !ino.es_reg() { return Err(errno::EINVAL); }
        let mut hecho = 0usize;
        let mut bloque = alloc::vec![0u8; self.bs as usize];
        let resultado: Resultado<()> = (|| {
            while hecho < datos.len() {
                let pos = desplazamiento + hecho as u64;
                if pos >= (u32::MAX as u64) << 1 { return Err(errno::EFBIG); }
                let lb = (pos / self.bs as u64) as u32;
                let o = (pos % self.bs as u64) as usize;
                let n = (self.bs as usize - o).min(datos.len() - hecho);
                let b = self.mapear_o_asignar(num, &mut ino, lb)?;
                if n == self.bs as usize {
                    bloque.copy_from_slice(&datos[hecho..hecho + n]);
                } else {
                    self.leer_bloque(b, &mut bloque)?;
                    bloque[o..o + n].copy_from_slice(&datos[hecho..hecho + n]);
                }
                self.escribir_bloque(b, &bloque)?;
                hecho += n;
            }
            Ok(())
        })();
        let fin = desplazamiento + hecho as u64;
        if fin > ino.tamano() { ino.tam = fin as u32; ino.tam_alto = (fin >> 32) as u32; }
        ino.mtime = ahora(); ino.ctime = ino.mtime;
        self.escribir_inodo(num, &ino)?;
        self.confirmar()?;
        match resultado { Ok(()) => Ok(hecho), Err(e) if hecho > 0 => { let _ = e; Ok(hecho) } Err(e) => Err(e) }
    }

    fn crear(&mut self, ruta: &str, modo: u32) -> Resultado<()> {
        let perm = if modo & 0o7777 == 0 { 0o644 } else { (modo & 0o7777) as u16 };
        // [T-001] El contrato admite el tipo en `modo`: sin tipo (open O_CREAT) o
        // S_IFREG -> fichero regular; S_IFIFO -> tubería con nombre persistente
        // (inodo sin bloques, d_type FIFO; `open` la enruta a la tubería).
        // Dispositivos (char/block) requieren rdev y no se admiten: EPERM.
        let tipo = match (modo & 0o170000) as u16 {
            0 | S_IFREG => S_IFREG,
            S_IFIFO => S_IFIFO,
            _ => return Err(errno::EPERM),
        };
        self.crear_nodo(ruta, tipo | perm)?;
        self.confirmar()
    }

    fn truncar(&mut self, ruta: &str, longitud: u64) -> Resultado<()> {
        let (num, mut ino) = self.resolver(ruta)?;
        if ino.es_dir() { return Err(errno::EISDIR); }
        if !ino.es_reg() { return Err(errno::EINVAL); }
        if longitud >= (u32::MAX as u64) << 1 { return Err(errno::EFBIG); }
        let actual = ino.tamano();
        if longitud < actual {
            let desde = ((longitud + self.bs as u64 - 1) / self.bs as u64) as u32;
            self.liberar_desde(&mut ino, desde)?;
            // Cero en la cola del último bloque parcial (lo que `read` devolvería).
            let o = (longitud % self.bs as u64) as usize;
            if o != 0 {
                let b = self.mapear(&ino, (longitud / self.bs as u64) as u32)?;
                if b != 0 {
                    let mut bloque = alloc::vec![0u8; self.bs as usize];
                    self.leer_bloque(b, &mut bloque)?;
                    for x in bloque[o..].iter_mut() { *x = 0; }
                    self.escribir_bloque(b, &bloque)?;
                }
            }
        }
        ino.tam = longitud as u32; ino.tam_alto = (longitud >> 32) as u32;
        ino.mtime = ahora(); ino.ctime = ino.mtime;
        self.escribir_inodo(num, &ino)?;
        self.confirmar()
    }

    fn eliminar(&mut self, ruta: &str) -> Resultado<()> {
        let (pnum, mut padre, nombre) = self.resolver_padre(ruta)?;
        let num = self.buscar(&padre, nombre)?.ok_or(errno::ENOENT)?;
        let mut ino = self.leer_inodo(num)?;
        if ino.es_dir() { return Err(errno::EISDIR); }
        self.quitar_entrada(pnum, &mut padre, nombre)?;
        ino.enlaces = ino.enlaces.saturating_sub(1);
        if ino.enlaces == 0 { self.destruir_inodo(num, &mut ino)?; } else { ino.ctime = ahora(); self.escribir_inodo(num, &ino)?; }
        self.confirmar()
    }

    fn crear_directorio(&mut self, ruta: &str, modo: u32) -> Resultado<()> {
        if self.relativa(ruta)? == "/" { return Err(errno::EEXIST); }
        let perm = if modo & 0o7777 == 0 { 0o755 } else { (modo & 0o7777) as u16 };
        let nombre = String::from(self.resolver_padre(ruta)?.2);
        let (num, mut ino, pnum, mut padre) = self.crear_nodo(ruta, S_IFDIR | perm)?;
        // Bloque inicial con "." y "..".
        let b = match self.mapear_o_asignar(num, &mut ino, 0) {
            Ok(b) => b,
            Err(e) => { let _ = self.quitar_entrada(pnum, &mut padre, &nombre); let _ = self.liberar_inodo(num, true); let _ = self.confirmar(); return Err(e); }
        };
        let mut buf = alloc::vec![0u8; self.bs as usize];
        wr32(&mut buf, 0, num); wr16(&mut buf, 4, 12); buf[6] = 1; buf[7] = if self.con_filetype { TIPO_DIR } else { 0 }; buf[8] = b'.';
        wr32(&mut buf, 12, pnum); wr16(&mut buf, 16, (self.bs - 12) as u16); buf[18] = 2; buf[19] = buf[7]; buf[20] = b'.'; buf[21] = b'.';
        self.escribir_bloque(b, &buf)?;
        ino.tam = self.bs; ino.enlaces = 2;
        self.escribir_inodo(num, &ino)?;
        padre.enlaces += 1; padre.ctime = ahora();
        self.escribir_inodo(pnum, &padre)?;
        self.confirmar()
    }

    fn eliminar_directorio(&mut self, ruta: &str) -> Resultado<()> {
        if self.relativa(ruta)? == "/" { return Err(errno::EPERM); }
        let (pnum, mut padre, nombre) = self.resolver_padre(ruta)?;
        let num = self.buscar(&padre, nombre)?.ok_or(errno::ENOENT)?;
        let mut ino = self.leer_inodo(num)?;
        if !ino.es_dir() { return Err(errno::ENOTDIR); }
        if !self.dir_vacio(&ino)? { return Err(errno::ENOTEMPTY); }
        self.quitar_entrada(pnum, &mut padre, nombre)?;
        padre.enlaces = padre.enlaces.saturating_sub(1);
        self.escribir_inodo(pnum, &padre)?;
        self.destruir_inodo(num, &mut ino)?;
        self.confirmar()
    }

    fn listar(&self, ruta: &str) -> Resultado<Vec<EntradaDir>> {
        let (_num, dir) = self.resolver(ruta)?;
        if !dir.es_dir() { return Err(errno::ENOTDIR); }
        let mut crudas: Vec<(u32, String)> = Vec::new();
        self.recorrer_dir(&dir, &mut |ino, _t, n| {
            if n != b"." && n != b".." {
                if let Ok(s) = core::str::from_utf8(n) { crudas.push((ino, String::from(s))); }
            }
            true
        })?;
        let mut salida = Vec::with_capacity(crudas.len());
        for (ino, nombre) in crudas {
            let i = self.leer_inodo(ino)?;
            salida.push(EntradaDir { nombre, modo: i.modo as u32, tam: i.tamano(), ino: ino as u64 });
        }
        Ok(salida)
    }

    fn renombrar(&mut self, origen: &str, destino: &str) -> Resultado<()> {
        let (po, mut padre_o, nombre_o) = self.resolver_padre(origen)?;
        let num = self.buscar(&padre_o, nombre_o)?.ok_or(errno::ENOENT)?;
        let ino = self.leer_inodo(num)?;
        let (pd, mut padre_d, nombre_d) = self.resolver_padre(destino)?;
        if ino.es_dir() {
            // No se puede mover un directorio dentro de sí mismo.
            let ro = self.relativa(origen)?; let rd = self.relativa(destino)?;
            if rd.starts_with(ro) && rd[ro.len()..].starts_with('/') { return Err(errno::EINVAL); }
        }
        if let Some(existente) = self.buscar(&padre_d, nombre_d)? {
            if existente == num { return Ok(()); }
            let mut ex = self.leer_inodo(existente)?;
            if ex.es_dir() {
                if !ino.es_dir() { return Err(errno::EISDIR); }
                if !self.dir_vacio(&ex)? { return Err(errno::ENOTEMPTY); }
                self.quitar_entrada(pd, &mut padre_d, nombre_d)?;
                padre_d.enlaces = padre_d.enlaces.saturating_sub(1);
                self.destruir_inodo(existente, &mut ex)?;
            } else {
                if ino.es_dir() { return Err(errno::ENOTDIR); }
                self.quitar_entrada(pd, &mut padre_d, nombre_d)?;
                ex.enlaces = ex.enlaces.saturating_sub(1);
                if ex.enlaces == 0 { self.destruir_inodo(existente, &mut ex)?; } else { self.escribir_inodo(existente, &ex)?; }
            }
        }
        let tipo = self.tipo_entrada(ino.modo);
        self.anadir_entrada(pd, &mut padre_d, nombre_d, num, tipo)?;
        if po == pd { padre_o = self.leer_inodo(po)?; }
        self.quitar_entrada(po, &mut padre_o, nombre_o)?;
        if ino.es_dir() && po != pd {
            self.fijar_padre(&ino, pd)?;
            padre_o.enlaces = padre_o.enlaces.saturating_sub(1);
            self.escribir_inodo(po, &padre_o)?;
            let mut pdn = self.leer_inodo(pd)?;
            pdn.enlaces += 1;
            self.escribir_inodo(pd, &pdn)?;
        } else if po == pd {
            // `quitar_entrada` ya escribió `padre_o`; nada más.
        } else {
            self.escribir_inodo(po, &padre_o)?;
        }
        self.confirmar()
    }

    fn enlace_simbolico(&mut self, destino: &str, ruta: &str) -> Resultado<()> {
        if destino.is_empty() || destino.len() > self.bs as usize { return Err(errno::ENAMETOOLONG); }
        let (num, mut ino, _p, _pi) = self.crear_nodo(ruta, S_IFLNK | 0o777)?;
        if destino.len() < 60 {
            let mut crudo = [0u8; 60];
            crudo[..destino.len()].copy_from_slice(destino.as_bytes());
            for i in 0..15 { ino.bloque[i] = rd32(&crudo, i * 4); }
        } else {
            let b = self.mapear_o_asignar(num, &mut ino, 0)?;
            let mut buf = alloc::vec![0u8; self.bs as usize];
            buf[..destino.len()].copy_from_slice(destino.as_bytes());
            self.escribir_bloque(b, &buf)?;
        }
        ino.tam = destino.len() as u32;
        self.escribir_inodo(num, &ino)?;
        self.confirmar()
    }

    fn leer_enlace(&self, ruta: &str) -> Resultado<String> {
        let (_num, ino) = self.resolver_con(ruta, false)?;
        if !ino.es_lnk() { return Err(errno::EINVAL); }
        self.destino_enlace(&ino)
    }

    fn metadatos_sin_seguir(&self, ruta: &str) -> Resultado<Metadatos> {
        let (num, ino) = self.resolver_con(ruta, false)?;
        Ok(self.metadatos_de(num, &ino))
    }

    fn cambiar_modo(&mut self, ruta: &str, modo: u32) -> Resultado<()> {
        let (num, mut ino) = self.resolver(ruta)?;
        ino.modo = (ino.modo & S_IFMT) | (modo as u16 & 0o7777);
        ino.ctime = ahora();
        self.escribir_inodo(num, &ino)?;
        self.confirmar()
    }

    fn cambiar_propietario(&mut self, ruta: &str, uid: u32, gid: u32) -> Resultado<()> {
        let (num, mut ino) = self.resolver(ruta)?;
        // ext2 rev1 guarda 16 bits (los altos viven en osd2; no los usamos).
        if uid != u32::MAX { if uid > 0xFFFF { return Err(errno::EINVAL); } ino.uid = uid as u16; }
        if gid != u32::MAX { if gid > 0xFFFF { return Err(errno::EINVAL); } ino.gid = gid as u16; }
        // POSIX: chown borra setuid/setgid de ficheros regulares.
        if ino.es_reg() { ino.modo &= !0o6000; }
        ino.ctime = ahora();
        self.escribir_inodo(num, &ino)?;
        self.confirmar()
    }

    fn enlazar(&mut self, existente: &str, nuevo: &str) -> Resultado<()> {
        let (num, mut ino) = self.resolver_con(existente, false)?; // link(2) no sigue el enlace final
        if ino.es_dir() { return Err(errno::EPERM); }
        if ino.enlaces == u16::MAX { return Err(errno::EMLINK); }
        let (pnum, mut padre, nombre) = self.resolver_padre(nuevo)?;
        if self.buscar(&padre, nombre)?.is_some() { return Err(errno::EEXIST); }
        let nombre = String::from(nombre);
        let tipo = if ino.es_lnk() { TIPO_LNK } else { TIPO_REG };
        self.anadir_entrada(pnum, &mut padre, &nombre, num, tipo)?;
        ino.enlaces += 1;
        ino.ctime = ahora();
        self.escribir_inodo(num, &ino)?;
        self.confirmar()
    }

    fn sincronizar(&mut self) -> Resultado<()> {
        self.sucio = true;
        self.volcar_ahora()
    }

    fn descartar_cache_bloques(&mut self) { self.cache.lock().descartar_todo(); }

    fn cache_bloques(&self) -> Option<(u64, u64, usize)> {
        let (a, f, _, bytes) = self.estadisticas_cache();
        Some((a, f, bytes))
    }

    /// Verificador real: (1) recuenta los mapas de bits de bloques e inodos y
    /// los contrasta con los contadores de cada grupo y del superbloque;
    /// (2) recorre el árbol desde la raíz y comprueba que el conjunto de inodos
    /// alcanzables coincide con el mapa de inodos, que los enlaces de cada
    /// directorio cuadran y que ningún bloque de datos está marcado libre.
    fn verificar(&mut self) -> Resultado<InformeVerificacion> {
        let mut inf = InformeVerificacion::default();
        let mut mapa_b = alloc::vec![0u8; self.bs as usize];
        let mut mapa_i = alloc::vec![0u8; self.bs as usize];
        let mut libres_b_total = 0u32;
        let mut libres_i_total = 0u32;
        let mut bits_bloques: Vec<u8> = Vec::new();
        let mut bits_inodos: Vec<u8> = Vec::new();
        for g in 0..self.grupos {
            self.leer_bloque(self.gd(g, 0), &mut mapa_b)?;
            self.leer_bloque(self.gd(g, 4), &mut mapa_i)?;
            let nb = self.bloques_en_grupo(g) as usize;
            let mut lb = 0u32;
            for i in 0..nb { if mapa_b[i / 8] & (1 << (i % 8)) == 0 { lb += 1; } }
            let mut li = 0u32;
            for i in 0..self.ipg as usize { if mapa_i[i / 8] & (1 << (i % 8)) == 0 { li += 1; } }
            if lb != self.gd(g, 12) { inf.incoherencias.push(alloc::format!("grupo {}: bloques libres mapa={} descriptor={}", g, lb, self.gd(g, 12))); }
            if li != self.gd(g, 14) { inf.incoherencias.push(alloc::format!("grupo {}: inodos libres mapa={} descriptor={}", g, li, self.gd(g, 14))); }
            libres_b_total += lb; libres_i_total += li;
            bits_bloques.extend_from_slice(&mapa_b[..(self.bpg as usize + 7) / 8]);
            bits_inodos.extend_from_slice(&mapa_i[..(self.ipg as usize + 7) / 8]);
            inf.bloques_revisados += nb as u64;
        }
        if libres_b_total != self.sb_libres_bloques() { inf.incoherencias.push(alloc::format!("superbloque: bloques libres {} frente a {} contados", self.sb_libres_bloques(), libres_b_total)); }
        if libres_i_total != self.sb_libres_inodos() { inf.incoherencias.push(alloc::format!("superbloque: inodos libres {} frente a {} contados", self.sb_libres_inodos(), libres_i_total)); }
        let bit_bloque = |b: u32| -> bool {
            let r = b - self.primer_dato; let g = r / self.bpg; let i = (r % self.bpg) as usize;
            let base = (g as usize) * ((self.bpg as usize + 7) / 8);
            bits_bloques.get(base + i / 8).map(|x| x & (1 << (i % 8)) != 0).unwrap_or(false)
        };
        let bit_inodo = |n: u32| -> bool {
            let g = (n - 1) / self.ipg; let i = ((n - 1) % self.ipg) as usize;
            let base = (g as usize) * ((self.ipg as usize + 7) / 8);
            bits_inodos.get(base + i / 8).map(|x| x & (1 << (i % 8)) != 0).unwrap_or(false)
        };
        // Recorrido del árbol.
        let mut alcanzados: Vec<u32> = Vec::new();
        let mut enlaces: Vec<(u32, u32)> = Vec::new(); // (ino, referencias vistas)
        let mut pila: Vec<u32> = alloc::vec![RAIZ];
        let cuenta = |v: &mut Vec<(u32, u32)>, n: u32| { if let Some(e) = v.iter_mut().find(|e| e.0 == n) { e.1 += 1; } else { v.push((n, 1)); } };
        cuenta(&mut enlaces, RAIZ); // ".." de la raíz apunta a sí misma: se cuenta abajo
        while let Some(d) = pila.pop() {
            if alcanzados.contains(&d) { continue; }
            alcanzados.push(d);
            let ino = self.leer_inodo(d)?;
            inf.inodos_revisados += 1;
            if !bit_inodo(d) { inf.incoherencias.push(alloc::format!("inodo {} en uso pero libre en el mapa", d)); }
            // bloques del inodo marcados en uso
            let nb = ((ino.tamano() + self.bs as u64 - 1) / self.bs as u64) as u32;
            if !ino.enlace_rapido() {
                for lb in 0..nb {
                    let b = self.mapear(&ino, lb)?;
                    if b != 0 && !bit_bloque(b) { inf.incoherencias.push(alloc::format!("inodo {}: bloque {} en uso pero libre en el mapa", d, b)); }
                }
            }
            if ino.es_dir() {
                let mut hijos: Vec<(u32, bool)> = Vec::new();
                let mut vistos_punto = 0;
                self.recorrer_dir(&ino, &mut |n, _t, nombre| {
                    if nombre == b"." { vistos_punto += 1; if n != d { hijos.push((0, false)); } }
                    else if nombre == b".." { vistos_punto += 1; hijos.push((n, true)); }
                    else { hijos.push((n, false)); }
                    true
                })?;
                if vistos_punto != 2 { inf.incoherencias.push(alloc::format!("directorio {}: faltan '.' o '..'", d)); }
                for (n, es_pp) in hijos {
                    if n == 0 { inf.incoherencias.push(alloc::format!("directorio {}: '.' no apunta a si mismo", d)); continue; }
                    if es_pp { if d != RAIZ { cuenta(&mut enlaces, n); } continue; }
                    cuenta(&mut enlaces, n);
                    let h = self.leer_inodo(n)?;
                    if h.es_dir() { pila.push(n); } else if !alcanzados.contains(&n) {
                        alcanzados.push(n);
                        inf.inodos_revisados += 1;
                        if !bit_inodo(n) { inf.incoherencias.push(alloc::format!("inodo {} en uso pero libre en el mapa", n)); }
                        if !h.enlace_rapido() {
                            let nbh = ((h.tamano() + self.bs as u64 - 1) / self.bs as u64) as u32;
                            for lb in 0..nbh.min(12 + self.punteros_por_bloque() * 2) {
                                let b = self.mapear(&h, lb)?;
                                if b != 0 && !bit_bloque(b) { inf.incoherencias.push(alloc::format!("inodo {}: bloque {} en uso pero libre en el mapa", n, b)); }
                            }
                        }
                    }
                }
            }
        }
        // Contadores de enlaces (directorios: 1 por entrada + 1 por "." + 1 por cada hijo "..").
        for (n, refs) in &enlaces {
            let i = self.leer_inodo(*n)?;
            let esperado = if i.es_dir() { refs + 1 } else { *refs };
            if esperado != i.enlaces as u32 { inf.incoherencias.push(alloc::format!("inodo {}: enlaces {} pero {} referencias", n, i.enlaces, esperado)); }
        }
        // Inodos marcados en uso que nadie alcanza (salvo los reservados < primer_inodo).
        let mut huerfanos = 0u32;
        for n in self.primer_inodo..=self.inodos_total {
            if bit_inodo(n) && !alcanzados.contains(&n) { huerfanos += 1; }
        }
        if huerfanos > 0 { inf.incoherencias.push(alloc::format!("{} inodos marcados en uso sin entrada de directorio", huerfanos)); }
        Ok(inf)
    }

    fn espacio(&self) -> (u64, u64, u32) {
        (self.sb_libres_bloques() as u64, self.bloques_total as u64, self.bs)
    }
}
