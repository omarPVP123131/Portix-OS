// drivers/storage/vfs.rs — PORTIX Virtual Filesystem v1.1
//
// CAPA: drivers/storage  (no ui/)
//
// El VFS gestiona paths del sistema y su resolución a clusters FAT32.
// Es responsabilidad del subsistema de almacenamiento, igual que fat32.rs.
// La UI lo consume; nunca debe contener lógica de paths.
//
// Árbol del sistema:
//   /             Raíz FAT32
//   ├── bin/      Ejecutables del kernel
//   ├── etc/      Configuración
//   ├── home/user Archivos del usuario
//   ├── tmp/      Temporal
//   ├── usr/      Herramientas
//   └── var/      Logs y datos

#![allow(dead_code)]

// ─────────────────────────────────────────────────────────────────────────────
// Árbol VFS predefinido
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
pub fn path_join(dir: &str, name: &str, out: &mut [u8]) -> usize {
    let mut p = 0usize;
    for &b in dir.as_bytes()  { if p < out.len() { out[p] = b; p += 1; } }
    if p > 0 && out.get(p - 1) != Some(&b'/') {
        if p < out.len() { out[p] = b'/'; p += 1; }
    }
    for &b in name.as_bytes() { if p < out.len() { out[p] = b; p += 1; } }
    p
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