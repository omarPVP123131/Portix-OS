// drivers/storage/vfs.rs — PORTIX Virtual Filesystem v2.0
//
// CAPA: drivers/storage
//
// VFS abstraction: mount points, path routing, filesystem dispatch.
// Supports FAT32 (default) and ramfs (/tmp).

#![allow(dead_code)]

use crate::arch::Spinlock;
use crate::drivers::serial;
use crate::drivers::storage::ramfs::RamFs;
use alloc::string::String;
use alloc::vec::Vec;

/// Canonicaliza un path POSIX: resuelve '.', '..', elimina barras dobles y normaliza el resultado.
/// Ejemplo: "/tmp/../tmp/./edge_path.txt" -> "/tmp/edge_path.txt"
pub fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        } else if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    let mut res = String::new();
    if path.starts_with('/') {
        res.push('/');
    }
    for (i, p) in parts.iter().enumerate() {
        if i > 0 && !res.ends_with('/') {
            res.push('/');
        }
        res.push_str(p);
    }
    if res.is_empty() {
        res.push('/');
    }
    res
}

// ─────────────────────────────────────────────────────────────────────────────
// Árbol VFS predefinido (for UI explorer)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct VfsEntry {
    pub path:    &'static str,
    pub label:   &'static str,
    pub icon:    &'static str,
    pub is_user: bool,
}

impl VfsEntry {
    const fn sys(path: &'static str, label: &'static str, icon: &'static str) -> Self {
        VfsEntry { path, label, icon, is_user: false }
    }
    const fn usr(path: &'static str, label: &'static str, icon: &'static str) -> Self {
        VfsEntry { path, label, icon, is_user: true }
    }
}

pub const VFS_TREE: &[VfsEntry] = &[
    VfsEntry::sys("/",          "Raíz",         "[/]"),
    VfsEntry::sys("/bin",       "Sistema",      "[S]"),
    VfsEntry::sys("/dev",       "Dispositivos", "[D]"),
    VfsEntry::sys("/etc",       "Config",       "[C]"),
    VfsEntry::usr("/home",      "Usuario",      "[H]"),
    VfsEntry::usr("/home/user", "Mis archivos", "[~]"),
    VfsEntry::sys("/proc",      "Procesos",     "[P]"),
    VfsEntry::usr("/tmp",       "Temporal",     "[T]"),
    VfsEntry::sys("/usr",       "Herramientas", "[U]"),
    VfsEntry::sys("/var",       "Logs/Datos",   "[V]"),
];

// ─────────────────────────────────────────────────────────────────────────────
// Utilidades de paths — SIN &str con lifetime ambiguo
//
// El error "lifetime may not live long enough" en la versión anterior venía de:
//   pub fn path_components(path: &str, out: &mut [&str]) -> usize
//
// El problema: `out[n] = &path[start..i]` intenta guardar una referencia a
// `path` dentro de `out`, pero Rust no puede saber que el lifetime de `out`
// es el mismo que el de `path` — necesitaría `<'a>(path: &'a str, out: &mut [&'a str])`.
//
// Solución adoptada: las funciones copian bytes a buffers propios del caller.
// Sin referencias cruzadas, sin anotaciones de lifetime, sin errores.
// ─────────────────────────────────────────────────────────────────────────────

/// Descompone un path en componentes, copiando cada uno en su buffer.
/// Devuelve el número de componentes encontrados.
///
/// "/home/user/main.rs"  →  bufs[0]="home" | bufs[1]="user" | bufs[2]="main.rs"
pub fn path_split(path: &str, bufs: &mut [[u8; 64]], lens: &mut [usize]) -> usize {
    let bytes = path.as_bytes();
    let mut n     = 0usize;
    let mut start = 0usize;
    let mut i     = 0usize;

    if i < bytes.len() && bytes[i] == b'/' { i += 1; start = i; }

    while i <= bytes.len() {
        let at_boundary = i == bytes.len() || bytes[i] == b'/';
        if at_boundary {
            if i > start && n < bufs.len() {
                let len = (i - start).min(64);
                bufs[n][..len].copy_from_slice(&bytes[start..start + len]);
                lens[n] = len;
                n += 1;
            }
            i += 1; start = i;
        } else {
            i += 1;
        }
    }
    n
}

/// Recupera el componente `idx` como &str desde los buffers de `path_split`.
#[inline]
pub fn component_str<'a>(bufs: &'a [[u8; 64]], lens: &[usize], idx: usize) -> &'a str {
    core::str::from_utf8(&bufs[idx][..lens[idx]]).unwrap_or("?")
}

/// Construye un path: dir + "/" + name en `out`, devuelve bytes escritos.
pub fn path_join(dir: &str, name: &str, out: &mut [u8]) -> Result<usize, ()> {
    let needed = dir.len() + 1 + name.len();
    if needed > out.len() { return Err(()); }
    let mut p = 0usize;
    for &b in dir.as_bytes()  { out[p] = b; p += 1; }
    if p > 0 && out.get(p - 1) != Some(&b'/') {
        out[p] = b'/'; p += 1;
    }
    for &b in name.as_bytes() { out[p] = b; p += 1; }
    Ok(p)
}

/// Nombre base: "/home/user/foo.txt" → "foo.txt"
/// Lifetime ligado a `path` (mismo str, sin problema).
pub fn basename(path: &str) -> &str {
    let bytes = path.as_bytes();
    let mut last = 0usize;
    for i in 0..bytes.len() { if bytes[i] == b'/' { last = i + 1; } }
    &path[last..]
}

/// Copia el directorio padre en `out`. Devuelve bytes escritos.
pub fn parent_copy(path: &str, out: &mut [u8]) -> usize {
    let bytes = path.as_bytes();
    let mut last = 0usize;
    for i in 0..bytes.len() { if bytes[i] == b'/' { last = i; } }
    if last == 0 { if !out.is_empty() { out[0] = b'/'; } return 1; }
    let n = last.min(out.len());
    out[..n].copy_from_slice(&bytes[..n]);
    n
}

// ─────────────────────────────────────────────────────────────────────────────
// VfsMount — tabla path → cluster FAT32
// ─────────────────────────────────────────────────────────────────────────────

const VFS_MOUNT_MAX: usize = 16;

pub struct VfsMount {
    keys:     [[u8; 64]; VFS_MOUNT_MAX],
    key_lens: [usize;    VFS_MOUNT_MAX],
    clusters: [u32;      VFS_MOUNT_MAX],
    count:    usize,
}

impl VfsMount {
    pub const fn new() -> Self {
        VfsMount {
            keys:     [[0u8; 64]; VFS_MOUNT_MAX],
            key_lens: [0usize;    VFS_MOUNT_MAX],
            clusters: [0u32;      VFS_MOUNT_MAX],
            count:    0,
        }
    }

    pub fn register(&mut self, path: &str, cluster: u32) {
        let n  = path.len().min(64);
        let pb = &path.as_bytes()[..n];
        for i in 0..self.count {
            if self.key_lens[i] == n && &self.keys[i][..n] == pb {
                self.clusters[i] = cluster; return;
            }
        }
        if self.count >= VFS_MOUNT_MAX { return; }
        self.keys[self.count][..n].copy_from_slice(pb);
        self.key_lens[self.count] = n;
        self.clusters[self.count] = cluster;
        self.count += 1;
    }

    pub fn resolve(&self, path: &str) -> Option<u32> {
        let n  = path.len().min(64);
        let pb = &path.as_bytes()[..n];
        for i in 0..self.count {
            if self.key_lens[i] == n && &self.keys[i][..n] == pb {
                return Some(self.clusters[i]);
            }
        }
        None
    }

    pub fn root_cluster(&self) -> u32 { self.resolve("/").unwrap_or(2) }
    pub fn count(&self) -> usize { self.count }
}

// ─────────────────────────────────────────────────────────────────────────────
// VFS Mount System — Phase 10: mount points + FS routing
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq)]
pub enum FsType {
    Fat32,
    RamFs,
    Ext2,
}

#[derive(Copy, Clone)]
pub struct MountEntry {
    pub path:    [u8; 64],
    pub path_len: usize,
    pub fs_type: FsType,
}

#[derive(Copy, Clone)]
pub struct UniversalSymlink {
    pub target: [u8; 128],
    pub target_len: usize,
    pub linkpath: [u8; 128],
    pub linkpath_len: usize,
}

impl UniversalSymlink {
    const fn empty() -> Self {
        UniversalSymlink {
            target: [0; 128],
            target_len: 0,
            linkpath: [0; 128],
            linkpath_len: 0,
        }
    }
}

const MAX_MOUNTS: usize = 16;
const MAX_SYMLINKS: usize = 32;

struct VfsState {
    table: [MountEntry; MAX_MOUNTS],
    count: usize,
    ramfs: Option<RamFs>,
    /// [CICLO 51] sistemas montados bajo el contrato `SistemaDeFicheros`
    /// (ext2 y sucesores), indexados igual que `table`.
    genericos: [Option<alloc::boxed::Box<dyn crate::fs::SistemaDeFicheros>>; MAX_MOUNTS],
    symlinks: [UniversalSymlink; MAX_SYMLINKS],
    symlink_count: usize,
}

impl VfsState {
    const fn new() -> Self {
        VfsState {
            table: [
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
                MountEntry { path: [0; 64], path_len: 0, fs_type: FsType::Fat32 },
            ],
            count: 0,
            ramfs: None,
            genericos: [const { None }; MAX_MOUNTS],
            symlinks: [UniversalSymlink::empty(); MAX_SYMLINKS],
            symlink_count: 0,
        }
    }

    fn mount(&mut self, path: &str, fs_type: FsType) -> bool {
        if self.count >= MAX_MOUNTS { return false; }
        let n = path.len().min(63);
        let bytes = path.as_bytes();
        self.table[self.count].path[..n].copy_from_slice(&bytes[..n]);
        self.table[self.count].path[n] = 0;
        self.table[self.count].path_len = n;
        self.table[self.count].fs_type = fs_type;
        self.count += 1;

        if fs_type == FsType::RamFs && self.ramfs.is_none() {
            self.ramfs = Some(RamFs::new());
        }

        let type_name = match fs_type {
            FsType::RamFs => "ramfs",
            FsType::Fat32 => "fat32",
            FsType::Ext2  => "ext2",
        };
        serial::write_str("[VFS] mount ");
        serial::write_str(type_name);
        serial::write_str(" -> ");
        serial::write_str(path);
        serial::write_str("\n");
        true
    }

    fn unmount(&mut self, path: &str) -> bool {
        let n = path.len().min(63);
        let bytes = path.as_bytes();
        for i in 0..self.count {
            if self.table[i].path_len == n && &self.table[i].path[..n] == &bytes[..n] {
                for j in i..(self.count - 1) {
                    self.table[j] = self.table[j + 1];
                }
                self.count -= 1;
                serial::write_str("[VFS] unmounted ");
                serial::write_str(path);
                serial::write_str("\n");
                return true;
            }
        }
        false
    }

    fn create_symlink(&mut self, target: &str, linkpath: &str) -> bool {
        if self.symlink_count >= MAX_SYMLINKS { return false; }
        let t_bytes = target.as_bytes();
        let t_len = t_bytes.len().min(127);
        let l_bytes = linkpath.as_bytes();
        let l_len = l_bytes.len().min(127);
        for i in 0..self.symlink_count {
            if self.symlinks[i].linkpath_len == l_len && &self.symlinks[i].linkpath[..l_len] == &l_bytes[..l_len] {
                return false;
            }
        }
        let mut sl = UniversalSymlink::empty();
        sl.target[..t_len].copy_from_slice(&t_bytes[..t_len]);
        sl.target_len = t_len;
        sl.linkpath[..l_len].copy_from_slice(&l_bytes[..l_len]);
        sl.linkpath_len = l_len;
        self.symlinks[self.symlink_count] = sl;
        self.symlink_count += 1;
        serial::write_str("[VFS] symlink ");
        serial::write_str(linkpath);
        serial::write_str(" -> ");
        serial::write_str(target);
        serial::write_str("\n");
        true
    }

    fn readlink(&self, linkpath: &str) -> Option<String> {
        let l_bytes = linkpath.as_bytes();
        let l_len = l_bytes.len();
        for i in 0..self.symlink_count {
            if self.symlinks[i].linkpath_len == l_len && &self.symlinks[i].linkpath[..l_len] == l_bytes {
                if let Ok(tgt) = core::str::from_utf8(&self.symlinks[i].target[..self.symlinks[i].target_len]) {
                    return Some(String::from(tgt));
                }
            }
        }
        None
    }

    /// [CICLO 51 / L3] Gana el punto de montaje MAS LARGO que sea prefijo de
    /// la ruta (como en Linux), con independencia del orden de montaje: la
    /// raiz ext2 ("/") sirve todo lo que no tenga un montaje mas especifico
    /// (/tmp, /dev/shm, ...). A igual longitud, el ultimo montado.
    fn resolve_fs(&self, path: &str) -> (FsType, usize) {
        let path_bytes = path.as_bytes();
        let path_len = path_bytes.len();
        let mut mejor: Option<(usize, usize)> = None; // (longitud, indice)
        for i in 0..self.count {
            let mp_len = self.table[i].path_len;
            if mp_len == 0 { continue; }
            let es_raiz = mp_len == 1 && self.table[i].path[0] == b'/';
            let coincide = if es_raiz {
                path_len >= 1 && path_bytes[0] == b'/'
            } else {
                path_len >= mp_len && &self.table[i].path[..mp_len] == &path_bytes[..mp_len]
                    && (path_len == mp_len || path_bytes[mp_len] == b'/' || self.table[i].path[mp_len - 1] == b'/')
            };
            if coincide {
                match mejor {
                    Some((l, _)) if l > mp_len => {}
                    _ => mejor = Some((mp_len, i)),
                }
            }
        }
        match mejor {
            Some((_, i)) => (self.table[i].fs_type, i),
            None => (FsType::Fat32, usize::MAX),
        }
    }

    fn resolve_path(&self, path: &str) -> Option<([u8; 256], usize)> {
        let (_fs_type, idx) = self.resolve_fs(path);
        if idx == usize::MAX { return None; }
        let mp_len = self.table[idx].path_len;
        let path_bytes = path.as_bytes();
        let rel_start = if mp_len > 0 && self.table[idx].path[mp_len - 1] == b'/' { mp_len - 1 } else { mp_len };
        let rel_path = if path_bytes.len() > rel_start { &path_bytes[rel_start..] } else { b"/" };
        let mut full = [0u8; 256];
        let plen = rel_path.len().min(255);
        full[..plen].copy_from_slice(&rel_path[..plen]);
        if plen < 256 { full[plen] = 0; }
        Some((full, plen))
    }
}

static VFS: Spinlock<VfsState> = Spinlock::new(VfsState::new());

/// [T-001] Modo degradado: el arranque no encontró raíz ext2 (o no tenía
/// /bin/init). No hay PID 1 ni rescate FAT; sólo la consola del núcleo.
static MODO_DEGRADADO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
pub fn marcar_modo_degradado() { MODO_DEGRADADO.store(true, core::sync::atomic::Ordering::Relaxed); }
pub fn en_modo_degradado() -> bool { MODO_DEGRADADO.load(core::sync::atomic::Ordering::Relaxed) }

pub fn mount(path: &str, fs_type: FsType) -> bool {
    VFS.lock().mount(path, fs_type)
}

pub fn unmount(path: &str) -> bool {
    VFS.lock().unmount(path)
}

pub fn create_symlink_universal(target: &str, linkpath: &str) -> bool {
    VFS.lock().create_symlink(target, linkpath)
}

pub fn readlink_universal(linkpath: &str) -> Option<String> {
    VFS.lock().readlink(linkpath)
}

pub fn list_mounts() -> Vec<(String, &'static str)> {
    let guard = VFS.lock();
    let mut list = Vec::new();
    list.push((String::from("/"), "fat32"));
    for i in 0..guard.count {
        let plen = guard.table[i].path_len;
        if let Ok(p) = core::str::from_utf8(&guard.table[i].path[..plen]) {
            let tname = match guard.table[i].fs_type {
                FsType::Fat32 => "fat32",
                FsType::RamFs => "ramfs",
                FsType::Ext2  => "ext2",
            };
            list.push((String::from(p), tname));
        }
    }
    list
}

pub fn resolve_fs(path: &str) -> (FsType, usize) {
    VFS.lock().resolve_fs(path)
}

/// [CICLO 51] Monta un sistema de ficheros que implementa el contrato
/// `SistemaDeFicheros` (ext2, ...). Devuelve false si no hay hueco.
pub fn montar_generico(path: &str, fs_type: FsType, mut fs: alloc::boxed::Box<dyn crate::fs::SistemaDeFicheros>) -> bool {
    let mut g = VFS.lock();
    if g.count >= MAX_MOUNTS { return false; }
    fs.fijar_montaje(path);
    let idx = g.count;
    if !g.mount(path, fs_type) { return false; }
    g.genericos[idx] = Some(fs);
    serial::write_str("[VFS] montaje generico (SistemaDeFicheros) -> ");
    serial::write_str(path);
    serial::write_str("\n");
    true
}

/// [CICLO 51] `true` si la ruta la sirve un sistema bajo el contrato
/// `SistemaDeFicheros` (ramfs o cualquier montaje genérico). Las rutas de
/// FAT32 (raíz), devfs y procfs devuelven false hasta su migración.
/// Rutas que nunca pertenecen a un sistema de ficheros de disco aunque la
/// raiz este montada: /dev (devfs, salvo /dev/shm que es un montaje ramfs
/// propio) y /proc (procfs).
fn es_ruta_virtual(path: &str) -> bool {
    is_devfs_path(path) || crate::drivers::storage::procfs::is_procfs_path(path)
}

pub fn es_ruta_fs(path: &str) -> bool {
    if es_ruta_virtual(path) { return false; }
    let g = VFS.lock();
    let (t, idx) = g.resolve_fs(path);
    if idx == usize::MAX { return false; }
    t == FsType::RamFs || g.genericos[idx].is_some()
}

/// [CICLO 51] Ejecuta `f` sobre el sistema de ficheros que sirve `path`
/// (como `&mut dyn SistemaDeFicheros`). `None` si la ruta no pertenece a un
/// sistema bajo el contrato (FAT32 raíz, devfs, procfs).
pub fn con_fs<R>(path: &str, f: impl FnOnce(&mut dyn crate::fs::SistemaDeFicheros) -> R) -> Option<R> {
    if es_ruta_virtual(path) { return None; }
    let mut g = VFS.lock();
    let (t, idx) = g.resolve_fs(path);
    if idx == usize::MAX { return None; }
    if g.genericos[idx].is_some() {
        let fs = g.genericos[idx].as_mut()?;
        return Some(f(fs.as_mut()));
    }
    if t == FsType::RamFs {
        // La instancia de ramfs es única y sirve varios puntos de montaje
        // (/tmp, /dev/shm, ...): se le indica cuál atiende en esta llamada
        // para que el contrato conozca la raíz del montaje.
        let mut punto = [0u8; 64];
        let n = g.table[idx].path_len.min(64);
        punto[..n].copy_from_slice(&g.table[idx].path[..n]);
        let ram = g.ramfs.as_mut()?;
        if let Ok(p) = core::str::from_utf8(&punto[..n]) { ram.fijar_prefijo_montaje(p); }
        return Some(f(ram));
    }
    None
}

/// [T-001] `Some(true)` si ambas rutas las sirve la MISMA instancia montada
/// (mismo índice de la tabla); `None` si alguna no pertenece al contrato.
pub fn mismo_sistema(a: &str, b: &str) -> Option<bool> {
    if es_ruta_virtual(a) || es_ruta_virtual(b) { return None; }
    let g = VFS.lock();
    let (ta, ia) = g.resolve_fs(a);
    let (tb, ib) = g.resolve_fs(b);
    if ia == usize::MAX || ib == usize::MAX { return None; }
    // La ramfs es una sola instancia para varios puntos de montaje: para ella
    // el enlace entre /tmp y /dev/shm es legítimo (mismo árbol en memoria).
    if ta == FsType::RamFs && tb == FsType::RamFs { return Some(true); }
    Some(ia == ib)
}

/// [CICLO 51] Verificador de invariantes de TODOS los sistemas montados bajo
/// el contrato. Devuelve (punto de montaje, tipo, informe).
pub fn verificar_todos() -> Vec<(String, &'static str, crate::fs::InformeVerificacion)> {
    let mut g = VFS.lock();
    let mut salida = Vec::new();
    let n = g.count;
    let mut ram_hecho = false;
    for i in 0..n {
        let plen = g.table[i].path_len;
        let ruta = String::from(core::str::from_utf8(&g.table[i].path[..plen]).unwrap_or("?"));
        let tipo = g.table[i].fs_type;
        if g.genericos[i].is_some() {
            if let Some(fs) = g.genericos[i].as_mut() {
                let tnombre = fs.tipo();
                if let Ok(inf) = fs.verificar() { salida.push((ruta, tnombre, inf)); }
            }
        } else if tipo == FsType::RamFs && !ram_hecho {
            ram_hecho = true;
            if let Some(ram) = g.ramfs.as_mut() {
                let inf = crate::fs::SistemaDeFicheros::verificar(ram).unwrap_or_default();
                salida.push((ruta, "ramfs", inf));
            }
        }
    }
    salida
}

pub fn is_ramfs_path(path: &str) -> bool {
    resolve_fs(path).0 == FsType::RamFs
}

pub fn is_devfs_path(path: &str) -> bool {
    (path == "/dev" || path.starts_with("/dev/")) && !path.starts_with("/dev/shm")
}

/// Known device names under /dev/
pub const DEVFS_ENTRIES: &[&str] = &[
    "kbd", "fb0", "sda0", "null", "zero", "urandom", "ptmx", "dsp", "audio",
    "loop0", "loop1", "loop2", "loop3", "loop-control"
];

/// Check if a devfs path is valid and return the device name (static)
pub fn resolve_devfs(path: &str) -> Option<&'static str> {
    if !is_devfs_path(path) {
        return None;
    }
    if path == "/dev" || path == "/dev/" {
        return None; // directory listing
    }
    let name = if path.starts_with("/dev/") {
        &path[5..]
    } else {
        path
    };
    if name.is_empty() {
        return None;
    }
    if name.starts_with("pts/") {
        return Some("pts");
    }
    for entry in DEVFS_ENTRIES {
        if *entry == name {
            return Some(entry);
        }
    }
    None
}

pub fn with_ramfs<F, R>(f: F) -> R
where F: FnOnce(&mut RamFs) -> R
{
    let mut guard = VFS.lock();
    match guard.ramfs.as_mut() {
        Some(ram) => f(ram),
        None => {
            serial::log("VFS", "CRITICAL: RAMFS not mounted - kernel panic\n");
            panic!("RAMFS not mounted - this is a fatal initialization error");
        }
    }
}
// ── [CICLO 51] Ayudas de compatibilidad sobre el contrato `SistemaDeFicheros` ──
// Permiten migrar los puntos de `syscall/mod.rs` que hablaban con ramfs por su
// API propia sin reescribirlos uno a uno; el errno exacto queda en `con_fs`.

pub fn leer_fs(path: &str, off: u64, buf: &mut [u8]) -> Result<usize, ()> {
    match con_fs(path, |fs| fs.leer(path, off, buf)) { Some(Ok(n)) => Ok(n), _ => Err(()) }
}

pub fn escribir_fs(path: &str, off: u64, datos: &[u8]) -> Result<usize, ()> {
    match con_fs(path, |fs| fs.escribir(path, off, datos)) { Some(Ok(n)) => Ok(n), _ => Err(()) }
}

/// Tamaño si existe y no es directorio (como `RamFs::file_size`).
pub fn tam_fs(path: &str) -> Option<usize> {
    match con_fs(path, |fs| fs.metadatos(path)) { Some(Ok(m)) if !m.es_dir() => Some(m.tam as usize), _ => None }
}

pub fn es_dir_fs(path: &str) -> bool {
    matches!(con_fs(path, |fs| fs.metadatos(path)), Some(Ok(m)) if m.es_dir())
}

/// (modo, uid, gid, tamaño, es_dir, es_enlace) — misma forma que `RamFs::get_metadata`.
pub fn meta_fs(path: &str) -> Option<(u32, u32, u32, usize, bool, bool)> {
    match con_fs(path, |fs| fs.metadatos(path)) {
        Some(Ok(m)) => Some((m.modo, m.uid, m.gid, m.tam as usize, m.es_dir(), m.es_enlace())),
        _ => None,
    }
}

pub fn listar_fs(path: &str) -> Vec<(String, bool)> {
    match con_fs(path, |fs| fs.listar(path)) {
        Some(Ok(l)) => l.into_iter().map(|e| (e.nombre.clone(), e.es_dir())).collect(),
        _ => Vec::new(),
    }
}

pub fn truncar_fs(path: &str, len: u64) -> bool {
    matches!(con_fs(path, |fs| fs.truncar(path, len)), Some(Ok(())))
}

pub fn chmod_fs(path: &str, modo: u32) -> Result<(), i32> {
    match con_fs(path, |fs| fs.cambiar_modo(path, modo)) { Some(Ok(())) => Ok(()), Some(Err(e)) => Err(-e), None => Err(-2) }
}

/// (nombre, es_dir, tamaño) para el `ls` del terminal del nucleo.
pub fn listar_fs_tam(path: &str) -> Result<Vec<(String, bool, u64)>, i32> {
    match con_fs(path, |fs| fs.listar(path)) {
        Some(Ok(l)) => Ok(l.into_iter().map(|e| (e.nombre.clone(), e.es_dir(), e.tam)).collect()),
        Some(Err(e)) => Err(e),
        None => Err(2),
    }
}

/// Escribe un fichero completo (crea si no existe, trunca) por el contrato.
pub fn escribir_fichero_fs(path: &str, datos: &[u8]) -> bool {
    match con_fs(path, |fs| {
        if fs.metadatos(path).is_err() { fs.crear(path, 0o644)?; }
        fs.truncar(path, 0)?;
        fs.escribir(path, 0, datos)
    }) {
        Some(Ok(n)) => n == datos.len(),
        _ => false,
    }
}
