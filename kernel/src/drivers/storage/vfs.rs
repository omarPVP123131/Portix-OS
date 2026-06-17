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
    VfsEntry::sys("/etc",       "Config",       "[C]"),
    VfsEntry::usr("/home",      "Usuario",      "[H]"),
    VfsEntry::usr("/home/user", "Mis archivos", "[~]"),
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
}

pub struct MountEntry {
    pub path:    [u8; 64],
    pub path_len: usize,
    pub fs_type: FsType,
}

const MAX_MOUNTS: usize = 8;

struct VfsState {
    table: [MountEntry; MAX_MOUNTS],
    count: usize,
    ramfs: Option<RamFs>,
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
            ],
            count: 0,
            ramfs: None,
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

        if fs_type == FsType::RamFs {
            self.ramfs = Some(RamFs::new());
        }

        let type_name = if fs_type == FsType::RamFs { "ramfs" } else { "fat32" };
        serial::write_str("[VFS] mount ");
        serial::write_str(type_name);
        serial::write_str(" -> ");
        serial::write_str(path);
        serial::write_str("\n");
        true
    }

    fn resolve_fs(&self, path: &str) -> (FsType, usize) {
        let path_bytes = path.as_bytes();
        let path_len = path_bytes.len();
        for i in (0..self.count).rev() {
            let mp_len = self.table[i].path_len;
            if mp_len == 0 { continue; }
            if path_len >= mp_len && &self.table[i].path[..mp_len] == &path_bytes[..mp_len] {
                return (self.table[i].fs_type, i);
            }
        }
        (FsType::Fat32, usize::MAX)
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

pub fn mount(path: &str, fs_type: FsType) -> bool {
    VFS.lock().mount(path, fs_type)
}

pub fn resolve_fs(path: &str) -> (FsType, usize) {
    VFS.lock().resolve_fs(path)
}

pub fn is_ramfs_path(path: &str) -> bool {
    resolve_fs(path).0 == FsType::RamFs
}

pub fn is_devfs_path(path: &str) -> bool {
    path == "/dev" || path.starts_with("/dev/")
}

/// Known device names under /dev/
pub const DEVFS_ENTRIES: &[&str] = &["kbd", "fb0", "sda0", "null"];

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