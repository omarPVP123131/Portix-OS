use alloc::vec::Vec;
use crate::drivers::serial;

const MAX_FILES: usize = 64;
const MAX_NAME: usize = 64;

#[derive(Clone)]
pub struct RamFile {
    pub name: [u8; MAX_NAME],
    pub name_len: usize,
    pub is_dir: bool,
    pub data: Vec<u8>,
}

pub struct RamFs {
    pub files: Vec<RamFile>,
}

impl RamFs {
    pub fn new() -> Self {
        let mut fs = RamFs { files: Vec::new() };
        let mut root_name = [0u8; MAX_NAME];
        root_name[0] = b'/';
        fs.files.push(RamFile {
            name: root_name,
            name_len: 1,
            is_dir: true,
            data: Vec::new(),
        });
        serial::write_str("[RAMFS] created (root /)\n");
        fs
    }

    fn find(&self, name: &str) -> Option<usize> {
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len();
        for (i, f) in self.files.iter().enumerate() {
            if f.name_len == name_len && &f.name[..name_len] == name_bytes {
                return Some(i);
            }
        }
        None
    }

    pub fn create(&mut self, path: &str) -> bool {
        if self.files.len() >= MAX_FILES { return false; }
        let name_bytes = path.as_bytes();
        let name_len = name_bytes.len().min(MAX_NAME);
        if self.find(path).is_some() { return false; }
        self.files.push(RamFile {
            name: {
                let mut n = [0u8; MAX_NAME];
                n[..name_len].copy_from_slice(&name_bytes[..name_len]);
                n
            },
            name_len,
            is_dir: false,
            data: Vec::new(),
        });
        serial::write_str("[RAMFS] create '");
        serial::write_str(path);
        serial::write_str("'\n");
        true
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
        let end = start + data.len();
        if end > file.data.len() {
            file.data.resize(end, 0);
        }
        file.data[start..end].copy_from_slice(data);
        Ok(data.len())
    }

    pub fn list_dir(&self, path: &str, cb: &mut dyn FnMut(&str, bool)) -> Result<(), ()> {
        let mut prefix_buf = [0u8; MAX_NAME];
        let prefix = {
            let dir_bytes = path.as_bytes();
            let dir_len = dir_bytes.len();
            if dir_len == 1 && dir_bytes[0] == b'/' {
                None
            } else {
                let trailing = if dir_bytes[dir_len - 1] == b'/' { 0 } else { 1 };
                let plen = dir_len + trailing;
                if plen > MAX_NAME { return Err(()); }
                prefix_buf[..dir_len].copy_from_slice(dir_bytes);
                if trailing > 0 { prefix_buf[dir_len] = b'/'; }
                Some(&prefix_buf[..plen])
            }
        };

        for f in &self.files {
            let fname = core::str::from_utf8(&f.name[..f.name_len]).unwrap_or("?");
            match prefix {
                None => {
                    if f.name_len > 1 || f.name[0] != b'/' {
                        let trimmed = if f.name[0] == b'/' {
                            core::str::from_utf8(&f.name[1..f.name_len]).unwrap_or("?")
                        } else {
                            fname
                        };
                        if !trimmed.contains('/') {
                            cb(trimmed, f.is_dir);
                        }
                    }
                }
                Some(pref) => {
                    if f.name_len > pref.len() && f.name[..pref.len()] == *pref {
                        let rest = core::str::from_utf8(&f.name[pref.len()..f.name_len]).unwrap_or("?");
                        if !rest.contains('/') {
                            cb(rest, f.is_dir);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn is_dir(&self, path: &str) -> bool {
        self.find(path).map(|i| self.files[i].is_dir).unwrap_or(false)
    }

    pub fn file_size(&self, path: &str) -> Option<usize> {
        self.find(path).map(|i| self.files[i].data.len())
    }
}