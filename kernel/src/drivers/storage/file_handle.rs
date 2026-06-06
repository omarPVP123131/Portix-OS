use alloc::vec;
use crate::drivers::storage::fat32::{Fat32Volume, FatError, DirEntryInfo};
use crate::drivers::storage::vfs::{VfsMount, path_split};

pub struct FileHandle<'a> {
    vol: &'a mut Fat32Volume<'a>,
    entry: DirEntryInfo,
    pos: u32,
}

impl<'a> FileHandle<'a> {
    pub fn open(vol: &'a mut Fat32Volume<'a>, mnt: &VfsMount, path: &str) -> Result<Self, FatError> {
        let entry = resolve_path_to_entry(vol, mnt, path)?;
        Ok(FileHandle { vol, entry, pos: 0 })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FatError> {
        if self.eof() { return Ok(0); }
        let to_read = buf.len().min((self.entry.size - self.pos) as usize);
        if to_read == 0 { return Ok(0); }
        let mut tmp = vec![0u8; self.entry.size as usize];
        self.vol.read_file(&self.entry, &mut tmp)?;
        let start = self.pos as usize;
        buf[..to_read].copy_from_slice(&tmp[start..start + to_read]);
        self.pos += to_read as u32;
        Ok(to_read)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<(), FatError> {
        if buf.is_empty() { return Ok(()); }
        let write_end = self.pos as usize + buf.len();
        let new_size = write_end.max(self.entry.size as usize);
        let mut content = if self.entry.size > 0 {
            let mut tmp = vec![0u8; self.entry.size as usize];
            self.vol.read_file(&self.entry, &mut tmp)?;
            tmp.resize(new_size, 0);
            tmp
        } else {
            vec![0u8; new_size]
        };
        let start = self.pos as usize;
        content[start..start + buf.len()].copy_from_slice(buf);
        let mut entry = self.entry.clone();
        self.vol.write_file(&mut entry, &content)?;
        self.entry = entry;
        self.pos += buf.len() as u32;
        Ok(())
    }

    pub fn close(self) {}

    pub fn size(&self) -> u32 { self.entry.size }
    pub fn pos(&self) -> u32 { self.pos }
    pub fn seek(&mut self, pos: u32) { self.pos = pos.min(self.entry.size); }
    pub fn eof(&self) -> bool { self.pos >= self.entry.size }
}

fn resolve_path_to_entry(vol: &mut Fat32Volume, mnt: &VfsMount, path: &str) -> Result<DirEntryInfo, FatError> {
    let path = path.trim();
    if path.is_empty() || path == "/" {
        return Err(FatError::IsDir);
    }
    if mnt.resolve(path).is_some() {
        return Err(FatError::IsDir);
    }
    let mut bufs = [[0u8; 64]; 16];
    let mut lens = [0usize; 16];
    let n = path_split(path, &mut bufs, &mut lens);
    if n == 0 { return Err(FatError::NotFound); }
    let mut cur = mnt.root_cluster();
    for i in 0..n - 1 {
        let comp = core::str::from_utf8(&bufs[i][..lens[i]]).map_err(|_| FatError::InvalidPath)?;
        let entry = vol.find_entry(cur, comp)?;
        if !entry.is_dir { return Err(FatError::IsFile); }
        cur = entry.cluster;
    }
    let last = core::str::from_utf8(&bufs[n - 1][..lens[n - 1]]).map_err(|_| FatError::InvalidPath)?;
    let entry = vol.find_entry(cur, last)?;
    if entry.is_dir { return Err(FatError::IsDir); }
    Ok(entry)
}
