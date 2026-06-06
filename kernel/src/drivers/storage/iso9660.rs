// drivers/storage/iso9660.rs — ISO 9660 filesystem reader
// Para CD-ROM (ATAPI), sectores de 2048 bytes.

use crate::drivers::storage::ata::AtaError;
use crate::drivers::storage::traits::BlockDevice;

const ISO_SECTOR: usize = 2048;
const PVD_LBA: u64 = 16;

#[derive(Clone, Copy)]
pub struct FileInfo {
    pub lba: u64,
    pub size: u32,
    pub is_dir: bool,
}

pub struct Iso9660Reader<'a> {
    drive: &'a mut dyn BlockDevice,
    buf: [u8; ISO_SECTOR],
}

impl<'a> Iso9660Reader<'a> {
    pub fn new(drive: &'a mut dyn BlockDevice) -> Self {
        Iso9660Reader { drive, buf: [0u8; ISO_SECTOR] }
    }

    pub fn find_file(&mut self, path: &[u8]) -> Result<FileInfo, AtaError> {
        let p = if path.starts_with(b"/") { &path[1..] } else { path };
        if p.is_empty() { return Err(AtaError::DriveFault); }

        self.drive.read_sectors(PVD_LBA, 1, &mut self.buf)?;
        if &self.buf[1..6] != b"CD001" { return Err(AtaError::DriveFault); }

        let root_lba  = u32::from_le_bytes([
            self.buf[158], self.buf[159], self.buf[160], self.buf[161],
        ]) as u64;
        let root_size = u32::from_le_bytes([
            self.buf[166], self.buf[167], self.buf[168], self.buf[169],
        ]);

        self.find_recursive(root_lba, root_size, p)
    }

    fn find_recursive(
        &mut self, dir_lba: u64, dir_size: u32, path: &[u8],
    ) -> Result<FileInfo, AtaError> {
        if let Some(slash) = path.iter().position(|&b| b == b'/') {
            let comp = &path[..slash];
            let rest = &path[slash + 1..];
            match self.find_in_dir(dir_lba, dir_size, comp)? {
                Some(sub) if sub.is_dir => self.find_recursive(sub.lba, sub.size, rest),
                Some(_) => Err(AtaError::DriveFault),
                None => Err(AtaError::DriveFault),
            }
        } else {
            match self.find_in_dir(dir_lba, dir_size, path)? {
                Some(f) => Ok(f),
                None => Err(AtaError::DriveFault),
            }
        }
    }

    fn find_in_dir(
        &mut self, dir_lba: u64, dir_size: u32, name: &[u8],
    ) -> Result<Option<FileInfo>, AtaError> {
        let mut remaining = dir_size as usize;
        let mut lba = dir_lba;

        while remaining > 0 {
            self.drive.read_sectors(lba, 1, &mut self.buf)?;
            let mut off = 0usize;

            while off < ISO_SECTOR && remaining > 0 {
                let len_dr = self.buf[off] as usize;
                if len_dr == 0 {
                    remaining = remaining.saturating_sub(ISO_SECTOR - off);
                    break;
                }
                if off + len_dr > ISO_SECTOR {
                    remaining = remaining.saturating_sub(ISO_SECTOR - off);
                    break;
                }

                let name_len = self.buf[off + 32] as usize;
                let name_start = off + 33;

                if name_len > 0 && name_start + name_len <= ISO_SECTOR {
                    let raw_name = &self.buf[name_start..name_start + name_len];
                    let ver_sep = raw_name.iter().position(|&b| b == b';');
                    let trimmed = match ver_sep {
                        Some(p) => &raw_name[..p],
                        None => raw_name,
                    };

                    if trimmed.eq_ignore_ascii_case(name) {
                        let f_lba = u32::from_le_bytes([
                            self.buf[off + 2], self.buf[off + 3],
                            self.buf[off + 4], self.buf[off + 5],
                        ]) as u64;
                        let f_size = u32::from_le_bytes([
                            self.buf[off + 10], self.buf[off + 11],
                            self.buf[off + 12], self.buf[off + 13],
                        ]);
                        let flags = self.buf[off + 25];
                        return Ok(Some(FileInfo {
                            lba: f_lba,
                            size: f_size,
                            is_dir: flags & 2 != 0,
                        }));
                    }
                }

                remaining = remaining.saturating_sub(len_dr);
                off += len_dr;
            }

            lba += 1;
        }

        Ok(None)
    }

    pub fn read_file(&mut self, info: &FileInfo, buf: &mut [u8]) -> Result<usize, AtaError> {
        let to_read = buf.len().min(info.size as usize);
        let mut done = 0usize;
        let mut lba = info.lba;
        while done < to_read {
            let chunk = (to_read - done).min(ISO_SECTOR);
            self.drive.read_sectors(lba, 1, &mut self.buf)?;
            buf[done..done + chunk].copy_from_slice(&self.buf[..chunk]);
            done += chunk;
            lba += 1;
        }
        Ok(done)
    }
}
