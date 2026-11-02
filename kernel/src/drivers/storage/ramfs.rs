use alloc::vec::Vec;
use crate::drivers::serial;

const MAX_FILES: usize = 128;
const MAX_NAME: usize = 64;

/// [defecto 31] Tamaño máximo de un fichero del ramfs: un bloque del montículo (orden 22 = 4 MiB).
pub const RAMFS_MAX_FILE: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub struct RamFile {
    pub name: [u8; MAX_NAME],
    pub name_len: usize,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub data: Vec<u8>,
    /// [CICLO 51 / L3] época UNIX de la última modificación (RTC real).
    pub mtime: u32,
}

fn ahora_epoch() -> u32 {
    let t = crate::time::rtc::now_epoch();
    if t < 0 { 0 } else if t > u32::MAX as i64 { u32::MAX } else { t as u32 }
}

pub struct RamFs {
    pub files: Vec<RamFile>,
    /// [CICLO 51] prefijo de montaje para el contrato `SistemaDeFicheros`.
    prefijo: [u8; 64],
    prefijo_len: usize,
}

impl RamFs {
    pub fn prefijo_montaje(&self) -> &str {
        core::str::from_utf8(&self.prefijo[..self.prefijo_len]).unwrap_or("/tmp")
    }
    pub fn fijar_prefijo_montaje(&mut self, p: &str) {
        let n = p.len().min(64);
        self.prefijo[..n].copy_from_slice(&p.as_bytes()[..n]);
        self.prefijo_len = n;
    }
    pub fn new() -> Self {
        let mut fs = RamFs { files: Vec::new(), prefijo: [0u8; 64], prefijo_len: 0 };
        fs.fijar_prefijo_montaje("/tmp");
        let mut root_name = [0u8; MAX_NAME];
        root_name[0] = b'/';
        fs.files.push(RamFile {
            name: root_name,
            name_len: 1,
            is_dir: true,
            is_symlink: false,
            mode: 0o040755,
            uid: 0,
            gid: 0,
            data: Vec::new(),
            mtime: ahora_epoch(),
        });
        serial::write_str("[RAMFS] created (root /)\n");
        fs
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        let clean = name.trim_end_matches('/');
        let match_target = if clean.is_empty() { "/" } else { clean };
        let target_bytes = match_target.as_bytes();

        for (i, f) in self.files.iter().enumerate() {
            let fname_raw = core::str::from_utf8(&f.name[..f.name_len]).unwrap_or("");
            let fname_clean = fname_raw.trim_end_matches('/');
            let f_clean = if fname_clean.is_empty() { "/" } else { fname_clean };
            if f_clean.as_bytes() == target_bytes {
                return Some(i);
            }
        }
        None
    }

    pub fn mkdir(&mut self, path: &str) -> bool {
        if self.files.len() >= MAX_FILES { return false; }
        let clean = path.trim_end_matches('/');
        if clean.is_empty() { return false; }
        if self.find(clean).is_some() { return false; }
        let name_bytes = clean.as_bytes();
        let name_len = name_bytes.len().min(MAX_NAME);
        let umask = crate::process::current_umask();
        let euid = crate::process::current_euid();
        let egid = crate::process::current_egid();
        self.files.push(RamFile {
            name: {
                let mut n = [0u8; MAX_NAME];
                n[..name_len].copy_from_slice(&name_bytes[..name_len]);
                n
            },
            name_len,
            is_dir: true,
            is_symlink: false,
            mode: 0o040000 | (0o777 & !umask),
            uid: euid,
            gid: egid,
            data: Vec::new(),
            mtime: ahora_epoch(),
        });
        serial::write_str("[RAMFS] mkdir '");
        serial::write_str(clean);
        serial::write_str("'\n");
        true
    }

    pub fn create(&mut self, path: &str) -> bool {
        if self.files.len() >= MAX_FILES { return false; }
        let clean = path.trim_end_matches('/');
        if clean.is_empty() { return false; }
        if self.find(clean).is_some() { return false; }

        // Automatically ensure parent directory exists
        if let Some(slash_idx) = clean.rfind('/') {
            if slash_idx > 0 {
                let parent = &clean[..slash_idx];
                if self.find(parent).is_none() {
                    self.mkdir(parent);
                }
            }
        }

        let name_bytes = clean.as_bytes();
        let name_len = name_bytes.len().min(MAX_NAME);
        let umask = crate::process::current_umask();
        let euid = crate::process::current_euid();
        let egid = crate::process::current_egid();
        self.files.push(RamFile {
            name: {
                let mut n = [0u8; MAX_NAME];
                n[..name_len].copy_from_slice(&name_bytes[..name_len]);
                n
            },
            name_len,
            is_dir: false,
            is_symlink: false,
            mode: 0o100000 | (0o666 & !umask),
            uid: euid,
            gid: egid,
            data: Vec::new(),
            mtime: ahora_epoch(),
        });
        serial::write_str("[RAMFS] create '");
        serial::write_str(clean);
        serial::write_str("'\n");
        crate::process::inotify_notify(clean, crate::process::IN_CREATE);
        true
    }

    pub fn unlink(&mut self, path: &str) -> bool {
        if let Some(idx) = self.find(path) {
            if !self.files[idx].is_dir {
                self.files.remove(idx);
                serial::write_str("[RAMFS] unlink '");
                serial::write_str(path);
                serial::write_str("'\n");
                crate::process::inotify_notify(path, crate::process::IN_DELETE);
                return true;
            }
        }
        false
    }

    pub fn read(&mut self, path: &str, buf: &mut [u8], offset: u64) -> Result<usize, ()> {
        let idx = self.find(path).ok_or(())?;
        if self.files[idx].is_dir { return Err(()); }
        let data = &self.files[idx].data;
        let start = offset as usize;
        if start >= data.len() { return Ok(0); }
        let n = (buf.len()).min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    pub fn write(&mut self, path: &str, data: &[u8], offset: u64) -> Result<usize, ()> {
        let idx = self.find(path).ok_or(())?;
        if self.files[idx].is_dir { return Err(()); }
        let file = &mut self.files[idx];
        let start = offset as usize;
        let end = start.checked_add(data.len()).ok_or(())?;
        if end > file.data.len() {
            // [T-005b, defecto 31] El ramfs vive en el montículo del núcleo (bloques de
            // 4 MiB como máximo): un fichero no puede pasar de RAMFS_MAX_FILE y la
            // capacidad se reserva EXACTA (sin duplicación), de modo que el Vec nunca
            // pide un bloque mayor que el orden máximo del asignador. `ld` escribiendo
            // un ejecutable de 4,5 MB en /tmp corrompía el montículo (#UD/#GP en las
            // dos CPUs). Por encima del tope: ENOSPC, como un tmpfs lleno.
            if end > RAMFS_MAX_FILE { return Err(()); }
            if end > file.data.capacity() {
                let extra = end - file.data.len();
                if file.data.try_reserve_exact(extra).is_err() { return Err(()); }
            }
            file.data.resize(end, 0);
        }
        file.data[start..end].copy_from_slice(data);
        file.mtime = ahora_epoch();
        crate::process::inotify_notify(path, crate::process::IN_MODIFY);
        Ok(data.len())
    }

    pub fn truncate(&mut self, path: &str) -> bool {
        self.truncate_to(path, 0)
    }

    pub fn truncate_to(&mut self, path: &str, length: usize) -> bool {
        if let Some(idx) = self.find(path) {
            if !self.files[idx].is_dir {
                if length > RAMFS_MAX_FILE { return false; }
                if length > self.files[idx].data.capacity() {
                    let extra = length - self.files[idx].data.len();
                    if self.files[idx].data.try_reserve_exact(extra).is_err() { return false; }
                }
                self.files[idx].data.resize(length, 0);
                self.files[idx].mtime = ahora_epoch();
                return true;
            }
        }
        false
    }

    pub fn rmdir(&mut self, path: &str) -> bool {
        let clean = path.trim_end_matches('/');
        if clean.is_empty() || clean == "/tmp" {
            return false;
        }
        let idx = match self.find(clean) {
            Some(i) => i,
            None => return false,
        };
        if !self.files[idx].is_dir {
            return false;
        }

        let prefix = alloc::format!("{}/", clean);
        let prefix_bytes = prefix.as_bytes();
        for (i, f) in self.files.iter().enumerate() {
            if i == idx { continue; }
            let fname = core::str::from_utf8(&f.name[..f.name_len]).unwrap_or("");
            if fname.as_bytes().starts_with(prefix_bytes) {
                return false;
            }
        }

        self.files.remove(idx);
        serial::write_str("[RAMFS] rmdir '");
        serial::write_str(clean);
        serial::write_str("'\n");
        true
    }

    pub fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), i32> {
        let clean_old = old_path.trim_end_matches('/');
        let clean_new = new_path.trim_end_matches('/');
        if clean_old.is_empty() || clean_new.is_empty() { return Err(-2); /* ENOENT */ }
        if clean_old == "/tmp" || clean_new == "/tmp" { return Err(-16); /* EBUSY */ }
        if clean_old == clean_new { return Ok(()); /* Identity no-op */ }

        let old_idx = self.find(clean_old).ok_or(-2)?;
        let old_is_dir = self.files[old_idx].is_dir;

        // Check if destination exists
        if let Some(target_idx) = self.find(clean_new) {
            let target_is_dir = self.files[target_idx].is_dir;
            if old_is_dir && !target_is_dir {
                return Err(-20); // ENOTDIR
            }
            if !old_is_dir && target_is_dir {
                return Err(-21); // EISDIR
            }
            if target_is_dir {
                let prefix = alloc::format!("{}/", clean_new);
                let prefix_bytes = prefix.as_bytes();
                for (i, f) in self.files.iter().enumerate() {
                    if i == target_idx { continue; }
                    let fname = core::str::from_utf8(&f.name[..f.name_len]).unwrap_or("");
                    if fname.as_bytes().starts_with(prefix_bytes) {
                        return Err(-39); // ENOTEMPTY
                    }
                }
            }
            self.files.remove(target_idx);
        }

        // Re-find old_idx because remove may have shifted elements
        let old_idx = self.find(clean_old).ok_or(-2)?;
        let name_bytes = clean_new.as_bytes();
        let name_len = name_bytes.len().min(MAX_NAME);
        let mut n = [0u8; MAX_NAME];
        n[..name_len].copy_from_slice(&name_bytes[..name_len]);
        self.files[old_idx].name = n;
        self.files[old_idx].name_len = name_len;

        if old_is_dir {
            let old_prefix = alloc::format!("{}/", clean_old);
            let new_prefix = alloc::format!("{}/", clean_new);
            for f in self.files.iter_mut() {
                let fname = core::str::from_utf8(&f.name[..f.name_len]).unwrap_or("");
                if fname.starts_with(&old_prefix) {
                    let suffix = &fname[old_prefix.len()..];
                    let updated = alloc::format!("{}{}", new_prefix, suffix);
                    let ubytes = updated.as_bytes();
                    let ulen = ubytes.len().min(MAX_NAME);
                    let mut un = [0u8; MAX_NAME];
                    un[..ulen].copy_from_slice(&ubytes[..ulen]);
                    f.name = un;
                    f.name_len = ulen;
                }
            }
        }
        serial::write_str("[RAMFS] rename '");
        serial::write_str(clean_old);
        serial::write_str("' -> '");
        serial::write_str(clean_new);
        serial::write_str("'\n");
        Ok(())
    }

    pub fn list_dir(&self, path: &str, cb: &mut dyn FnMut(&str, bool)) -> Result<(), ()> {
        self.list_dir_with_size(path, |name, is_dir, _size| cb(name, is_dir))
    }

    pub fn list_dir_with_size<F>(&self, path: &str, mut cb: F) -> Result<(), ()>
    where
        F: FnMut(&str, bool, usize),
    {
        let clean = path.trim_end_matches('/');
        let pref = if clean.is_empty() {
            alloc::string::String::from("/")
        } else {
            alloc::format!("{}/", clean)
        };
        let pref_bytes = pref.as_bytes();

        let mut seen_dirs: Vec<&str> = Vec::new();

        for f in &self.files {
            let fname = match core::str::from_utf8(&f.name[..f.name_len]) {
                Ok(s) => s.trim_end_matches('/'),
                Err(_) => continue,
            };
            if fname.len() >= pref_bytes.len() && &fname.as_bytes()[..pref_bytes.len()] == pref_bytes {
                let rest = &fname[pref_bytes.len()..];
                if let Some(slash) = rest.find('/') {
                    let dir_name = &rest[..slash];
                    if !seen_dirs.iter().any(|&d| d == dir_name) {
                        seen_dirs.push(dir_name);
                        cb(dir_name, true, 0);
                    }
                } else if !rest.is_empty() {
                    cb(rest, f.is_dir, f.data.len());
                }
            }
        }
        Ok(())
    }

    pub fn is_dir(&self, path: &str) -> bool {
        let clean = path.trim_end_matches('/');
        if clean.is_empty() || clean == "/tmp" {
            return true;
        }
        self.find(clean).map(|i| self.files[i].is_dir).unwrap_or(false)
    }

    pub fn file_size(&self, path: &str) -> Option<usize> {
        self.find(path).map(|i| self.files[i].data.len())
    }

    pub fn link(&mut self, old_path: &str, new_path: &str) -> Result<(), i32> {
        let clean_old = old_path.trim_end_matches('/');
        let clean_new = new_path.trim_end_matches('/');
        if clean_old.is_empty() || clean_new.is_empty() { return Err(-2); } // -ENOENT
        let old_idx = self.find(clean_old).ok_or(-2)?; // -ENOENT
        if self.files[old_idx].is_dir {
            return Err(-1); // -EPERM: cannot hard link directory
        }
        if self.find(clean_new).is_some() {
            return Err(-17); // -EEXIST
        }
        if self.files.len() >= MAX_FILES {
            return Err(-28); // -ENOSPC
        }
        let old_file = self.files[old_idx].clone();
        let name_bytes = clean_new.as_bytes();
        let name_len = name_bytes.len().min(MAX_NAME);
        let mut new_name = [0u8; MAX_NAME];
        new_name[..name_len].copy_from_slice(&name_bytes[..name_len]);
        self.files.push(RamFile {
            name: new_name,
            name_len,
            is_dir: false,
            is_symlink: false,
            mode: old_file.mode,
            uid: old_file.uid,
            gid: old_file.gid,
            data: old_file.data.clone(),
            mtime: ahora_epoch(),
        });
        serial::write_str("[RAMFS] link '");
        serial::write_str(clean_new);
        serial::write_str("' -> '");
        serial::write_str(clean_old);
        serial::write_str("'\n");
        Ok(())
    }

    pub fn create_symlink(&mut self, target: &str, linkpath: &str) -> bool {
        let clean = linkpath.trim_end_matches('/');
        if clean.is_empty() { return false; }
        if self.find(clean).is_some() { return false; }
        let name_bytes = clean.as_bytes();
        let name_len = name_bytes.len().min(MAX_NAME);
        let euid = crate::process::current_euid();
        let egid = crate::process::current_egid();
        self.files.push(RamFile {
            name: {
                let mut n = [0u8; MAX_NAME];
                n[..name_len].copy_from_slice(&name_bytes[..name_len]);
                n
            },
            name_len,
            is_dir: false,
            is_symlink: true,
            mode: 0o120777,
            uid: euid,
            gid: egid,
            data: target.as_bytes().to_vec(),
            mtime: ahora_epoch(),
        });
        serial::write_str("[RAMFS] symlink '");
        serial::write_str(clean);
        serial::write_str("' -> '");
        serial::write_str(target);
        serial::write_str("'\n");
        true
    }

    /// [CICLO 48] Fija el tipo de nodo (S_IFIFO, S_IFSOCK, ...) conservando los permisos.
    pub fn set_type(&mut self, path: &str, ftype: u32, perms: u32) -> bool {
        match self.find(path) {
            Some(idx) => { self.files[idx].mode = (ftype & 0o170000) | (perms & 0o7777); true }
            None => false,
        }
    }

    pub fn chmod(&mut self, path: &str, mode: u32) -> Result<(), i32> {
        let idx = self.find(path).ok_or(-2)?; // -ENOENT
        let cur_mode = self.files[idx].mode;
        self.files[idx].mode = (cur_mode & 0o170000) | (mode & 0o7777);
        Ok(())
    }

    pub fn chown(&mut self, path: &str, uid: u32, gid: u32) -> Result<(), i32> {
        let idx = self.find(path).ok_or(-2)?; // -ENOENT
        if uid != 0xFFFF_FFFF {
            self.files[idx].uid = uid;
        }
        if gid != 0xFFFF_FFFF {
            self.files[idx].gid = gid;
        }
        Ok(())
    }

    pub fn get_metadata(&self, path: &str) -> Option<(u32, u32, u32, usize, bool, bool)> {
        let idx = self.find(path)?;
        let f = &self.files[idx];
        let sz = if f.is_dir {
            4096
        } else if f.is_symlink {
            f.data.len()
        } else {
            f.data.len()
        };
        Some((f.mode, f.uid, f.gid, sz, f.is_dir, f.is_symlink))
    }

    pub fn readlink(&self, name: &str) -> Option<&[u8]> {
        let idx = self.find(name)?;
        if self.files[idx].is_symlink {
            Some(&self.files[idx].data)
        } else {
            None
        }
    }

    pub fn is_symlink(&self, name: &str) -> bool {
        if let Some(idx) = self.find(name) {
            self.files[idx].is_symlink
        } else {
            false
        }
    }
}
