// kernel/src/drivers/storage/tmpfs.rs — Sistema de Archivos Tmpfs Dinámico en Memoria con Cuotas
// Implementa almacenamiento en RAM, límites duros de cuota (bytes e inodos),
// contabilidad de bloques de 4KB y reporte de telemetría StatVFS.
#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::String;
use crate::arch::spinlock::Spinlock;

pub const TMPFS_DEFAULT_MAX_BYTES: usize = 16 * 1024 * 1024; // 16 MB de cuota
pub const TMPFS_DEFAULT_MAX_INODES: usize = 1024;
pub const TMPFS_BLOCK_SIZE: usize = 4096;

#[derive(Clone)]
pub struct TmpNode {
    pub ino: u64,
    pub name: String,
    pub is_dir: bool,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: usize,
    pub data: Vec<u8>,
}

pub struct TmpFs {
    pub max_bytes: usize,
    pub used_bytes: usize,
    pub max_inodes: usize,
    pub used_inodes: usize,
    pub next_ino: u64,
    pub nodes: Vec<TmpNode>,
}

impl TmpFs {
    pub const fn empty() -> Self {
        Self {
            max_bytes: TMPFS_DEFAULT_MAX_BYTES,
            used_bytes: 0,
            max_inodes: TMPFS_DEFAULT_MAX_INODES,
            used_inodes: 0,
            next_ino: 1,
            nodes: Vec::new(),
        }
    }

    pub fn new(max_bytes: usize, max_inodes: usize) -> Self {
        let mut fs = Self {
            max_bytes,
            used_bytes: 0,
            max_inodes,
            used_inodes: 1, // Raíz /
            next_ino: 2,
            nodes: Vec::new(),
        };
        fs.nodes.push(TmpNode {
            ino: 1,
            name: String::from("/"),
            is_dir: true,
            mode: 0o040777,
            uid: 0,
            gid: 0,
            size: 4096,
            data: Vec::new(),
        });
        fs
    }

    pub fn ensure_root(&mut self) {
        if self.nodes.is_empty() {
            self.nodes.push(TmpNode {
                ino: 1,
                name: String::from("/"),
                is_dir: true,
                mode: 0o040777,
                uid: 0,
                gid: 0,
                size: 4096,
                data: Vec::new(),
            });
            self.used_inodes = 1;
            self.next_ino = 2;
        }
    }

    pub fn find(&self, path: &str) -> Option<usize> {
        let clean = path.trim_end_matches('/');
        let target = if clean.is_empty() { "/" } else { clean };
        self.nodes.iter().position(|n| n.name == target)
    }

    pub fn mkdir(&mut self, path: &str, mode: u32) -> Result<u64, i32> {
        self.ensure_root();
        if self.used_inodes >= self.max_inodes {
            return Err(-28); // -ENOSPC (inodos agotados)
        }
        let clean = path.trim_end_matches('/');
        if clean.is_empty() || self.find(clean).is_some() {
            return Err(-17); // -EEXIST
        }
        let ino = self.next_ino;
        self.next_ino += 1;
        self.nodes.push(TmpNode {
            ino,
            name: String::from(clean),
            is_dir: true,
            mode: 0o040000 | (mode & 0o777),
            uid: 0,
            gid: 0,
            size: 4096,
            data: Vec::new(),
        });
        self.used_inodes += 1;
        Ok(ino)
    }

    pub fn create_file(&mut self, path: &str, mode: u32) -> Result<u64, i32> {
        self.ensure_root();
        if self.used_inodes >= self.max_inodes {
            return Err(-28); // -ENOSPC
        }
        let clean = path.trim_end_matches('/');
        if clean.is_empty() || self.find(clean).is_some() {
            return Err(-17); // -EEXIST
        }
        let ino = self.next_ino;
        self.next_ino += 1;
        self.nodes.push(TmpNode {
            ino,
            name: String::from(clean),
            is_dir: false,
            mode: 0o100000 | (mode & 0o777),
            uid: 0,
            gid: 0,
            size: 0,
            data: Vec::new(),
        });
        self.used_inodes += 1;
        Ok(ino)
    }

    pub fn write(&mut self, path: &str, data: &[u8], offset: usize) -> Result<usize, i32> {
        self.ensure_root();
        let idx = self.find(path).ok_or(-2)?; // -ENOENT
        if self.nodes[idx].is_dir {
            return Err(-21); // -EISDIR
        }
        let end = offset.checked_add(data.len()).ok_or(-28)?;
        let cur_size = self.nodes[idx].data.len();
        let bytes_needed = if end > cur_size { end - cur_size } else { 0 };

        if self.used_bytes + bytes_needed > self.max_bytes {
            return Err(-28); // -ENOSPC (cuota de memoria RAM excedida)
        }

        if end > cur_size {
            self.nodes[idx].data.resize(end, 0);
            self.used_bytes += bytes_needed;
            self.nodes[idx].size = end;
        }

        self.nodes[idx].data[offset..end].copy_from_slice(data);
        Ok(data.len())
    }

    pub fn read(&self, path: &str, buf: &mut [u8], offset: usize) -> Result<usize, i32> {
        let idx = self.find(path).ok_or(-2)?;
        if self.nodes[idx].is_dir {
            return Err(-21);
        }
        let file_data = &self.nodes[idx].data;
        if offset >= file_data.len() {
            return Ok(0);
        }
        let n = buf.len().min(file_data.len() - offset);
        buf[..n].copy_from_slice(&file_data[offset..offset + n]);
        Ok(n)
    }

    pub fn unlink(&mut self, path: &str) -> Result<(), i32> {
        self.ensure_root();
        let idx = self.find(path).ok_or(-2)?;
        if self.nodes[idx].is_dir {
            return Err(-1); // -EPERM
        }
        let file_size = self.nodes[idx].data.len();
        self.used_bytes = self.used_bytes.saturating_sub(file_size);
        self.used_inodes = self.used_inodes.saturating_sub(1);
        self.nodes.remove(idx);
        Ok(())
    }

    pub fn statvfs(&mut self) -> (usize, usize, usize, usize) {
        self.ensure_root();
        let total_blocks = self.max_bytes / TMPFS_BLOCK_SIZE;
        let used_blocks = (self.used_bytes + TMPFS_BLOCK_SIZE - 1) / TMPFS_BLOCK_SIZE;
        let free_blocks = total_blocks.saturating_sub(used_blocks);
        let free_inodes = self.max_inodes.saturating_sub(self.used_inodes);
        (total_blocks, free_blocks, self.max_inodes, free_inodes)
    }
}

pub static GLOBAL_TMPFS: Spinlock<TmpFs> = Spinlock::new(TmpFs::empty());

/// Auto-test del sistema de archivos TmpFs
pub fn run_tmpfs_self_test() -> bool {
    let mut fs = TmpFs::new(65536, 16); // 64 KB de cuota, 16 inodos

    // 1. Crear archivo y verificar inodo
    let ino = match fs.create_file("/test.dat", 0o644) {
        Ok(i) => i,
        Err(_) => return false,
    };
    if ino < 2 || fs.used_inodes != 2 {
        return false;
    }

    // 2. Escritura y verificación de datos
    let payload = b"Portix OS Tmpfs Dynamic Storage 2026";
    if fs.write("/test.dat", payload, 0) != Ok(payload.len()) {
        return false;
    }
    let mut read_buf = [0u8; 64];
    if fs.read("/test.dat", &mut read_buf, 0) != Ok(payload.len()) {
        return false;
    }
    if &read_buf[..payload.len()] != payload {
        return false;
    }

    // 3. Probar rechazo de escritura al superar cuota (-ENOSPC)
    let huge_data = [0xAAu8; 70000];
    if fs.write("/test.dat", &huge_data, 0) != Err(-28) {
        return false; // Debió fallar con -ENOSPC
    }

    // 4. Verificación de StatVFS
    let (tot_b, free_b, tot_ino, free_ino) = fs.statvfs();
    if tot_b == 0 || free_b > tot_b || tot_ino != 16 || free_ino != 14 {
        return false;
    }

    // 5. Unlink y liberación de cuota
    if fs.unlink("/test.dat").is_err() {
        return false;
    }
    if fs.used_bytes != 0 || fs.used_inodes != 1 {
        return false;
    }

    true
}
