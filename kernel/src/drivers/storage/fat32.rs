// drivers/storage/fat32.rs — PORTIX Kernel v0.7.4
// Driver FAT32 sobre ATA PIO.

#![allow(dead_code)]

use crate::drivers::storage::ata::{AtaDrive, AtaError};

// ── Errores ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FatError {
    Ata(AtaError),
    NotFat32,
    NotFound,
    NoSpace,
    IsDir,
    IsFile,
    NameTooLong,
    InvalidPath,
    Corrupt,
}

impl From<AtaError> for FatError {
    fn from(e: AtaError) -> Self { FatError::Ata(e) }
}

pub type FatResult<T> = Result<T, FatError>;

// ── Constantes FAT ────────────────────────────────────────────────────────────

const FAT_EOC:        u32  = 0x0FFF_FFF8;
const FAT_FREE:       u32  = 0x0000_0000;
const ATTR_DIR:       u8   = 0x10;
const ATTR_ARCH:      u8   = 0x20;
const ATTR_LFN:       u8   = 0x0F;
const DIR_ENTRY_SIZE: usize = 32;

// ── BPB ───────────────────────────────────────────────────────────────────────

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Bpb {
    _jump:          [u8; 3],
    _oem:           [u8; 8],
    bytes_per_sec:  u16,
    sec_per_clus:   u8,
    reserved_secs:  u16,
    num_fats:       u8,
    _root_entries:  u16,
    _total16:       u16,
    _media:         u8,
    _fat16:         u16,
    _sec_per_track: u16,
    _num_heads:     u16,
    _hidden:        u32,
    _total32:       u32,
    // FAT32 extended
    fat_size32:     u32,
    _ext_flags:     u16,
    _fs_ver:        u16,
    root_clus:      u32,
    _fs_info:       u16,
    _backup:        u16,
    _reserved:      [u8; 12],
    _drive_num:     u8,
    _reserved1:     u8,
    _boot_sig:      u8,
    _vol_id:        u32,
    _vol_label:     [u8; 11],
    fs_type:        [u8; 8],
}

// ── DirEntry 8.3 ──────────────────────────────────────────────────────────────

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct DirEntry83 {
    name:      [u8; 8],
    ext:       [u8; 3],
    attr:      u8,
    _nt:       u8,
    _crt_ms:   u8,
    _crt_time: u16,
    _crt_date: u16,
    _acc_date: u16,
    clus_hi:   u16,
    _wrt_time: u16,
    _wrt_date: u16,
    clus_lo:   u16,
    file_size: u32,
}

impl DirEntry83 {
    /// FIXED: acceso seguro a campos packed — usar read_unaligned vía copy
    fn cluster(&self) -> u32 {
        let hi: u16 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(self.clus_hi)) };
        let lo: u16 = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(self.clus_lo)) };
        ((hi as u32) << 16) | (lo as u32)
    }
    fn file_size(&self) -> u32 {
        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(self.file_size)) }
    }
    fn is_free(&self) -> bool { self.name[0] == 0xE5 || self.name[0] == 0x00 }
    fn is_end(&self)  -> bool { self.name[0] == 0x00 }
    fn is_lfn(&self)  -> bool { self.attr == ATTR_LFN }
    fn is_dir(&self)  -> bool { self.attr & ATTR_DIR != 0 }
}

// ── LFN ───────────────────────────────────────────────────────────────────────

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct LfnEntry {
    order:    u8,
    name1:    [u16; 5],
    attr:     u8,
    _type:    u8,
    checksum: u8,
    name2:    [u16; 6],
    _clus:    u16,
    name3:    [u16; 2],
}

// ── Entrada pública ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct DirEntryInfo {
    pub name:       [u8; 256],
    pub name_len:   usize,
    pub is_dir:     bool,
    pub size:       u32,
    pub cluster:    u32,
    pub dir_sector: u64,
    pub dir_offset: usize,
}

impl DirEntryInfo {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

// ── Volumen ───────────────────────────────────────────────────────────────────

pub struct Fat32Volume {
    drive:         AtaDrive,
    part_lba:      u64,
    bytes_per_sec: u16,
    sec_per_clus:  u32,
    reserved_secs: u32,
    num_fats:      u32,
    fat_size:      u32,
    root_clus:     u32,
    data_start:    u64,
    clus_count:    u32,
}

impl Fat32Volume {
    pub fn mount(drive: AtaDrive) -> FatResult<Self> {
        let mut mbr = [0u8; 512];
        drive.read_sectors(0, 1, &mut mbr).map_err(FatError::Ata)?;
        let part_lba = Self::find_fat32_partition(&mbr)?;

        let mut vbr = [0u8; 512];
        drive.read_sectors(part_lba, 1, &mut vbr).map_err(FatError::Ata)?;

        if vbr[510] != 0x55 || vbr[511] != 0xAA { return Err(FatError::NotFat32); }

        // Leer campos del BPB manualmente (evitar referencias a packed fields)
        let bytes_per_sec = u16::from_le_bytes([vbr[11], vbr[12]]);
        let sec_per_clus  = vbr[13] as u32;
        let reserved_secs = u16::from_le_bytes([vbr[14], vbr[15]]) as u32;
        let num_fats      = vbr[16] as u32;
        let fat_size      = u32::from_le_bytes([vbr[36], vbr[37], vbr[38], vbr[39]]);
        let root_clus     = u32::from_le_bytes([vbr[44], vbr[45], vbr[46], vbr[47]]);
        let fs_type       = &vbr[82..90];

        if fs_type != b"FAT32   " { return Err(FatError::NotFat32); }

        let data_start = part_lba
            + reserved_secs as u64
            + num_fats as u64 * fat_size as u64;

        let total32 = u32::from_le_bytes([vbr[32], vbr[33], vbr[34], vbr[35]]);
        let clus_count = total32
            .saturating_sub(reserved_secs + num_fats * fat_size)
            / sec_per_clus.max(1);

        Ok(Fat32Volume {
            drive, part_lba, bytes_per_sec, sec_per_clus,
            reserved_secs, num_fats, fat_size, root_clus,
            data_start, clus_count,
        })
    }

    fn find_fat32_partition(mbr: &[u8; 512]) -> FatResult<u64> {
        for i in 0..4usize {
            let off   = 0x1BE + i * 16;
            let ptype = mbr[off + 4];
            if ptype == 0x0B || ptype == 0x0C || ptype == 0x0E {
                let lba = u32::from_le_bytes([mbr[off+8], mbr[off+9], mbr[off+10], mbr[off+11]]) as u64;
                if lba > 0 { return Ok(lba); }
            }
        }
        if &mbr[82..90] == b"FAT32   " { return Ok(0); }
        Err(FatError::NotFat32)
    }

    // ── FAT I/O ───────────────────────────────────────────────────────────────

    fn fat_lba(&self, cluster: u32) -> u64 {
        self.part_lba + self.reserved_secs as u64
            + (cluster as u64 * 4) / self.bytes_per_sec as u64
    }
    fn fat_offset(&self, cluster: u32) -> usize {
        ((cluster as u64 * 4) % self.bytes_per_sec as u64) as usize
    }
    fn cluster_lba(&self, cluster: u32) -> u64 {
        self.data_start + (cluster as u64 - 2) * self.sec_per_clus as u64
    }
    fn bpc(&self) -> usize { self.bytes_per_sec as usize * self.sec_per_clus as usize }
    fn is_eoc(&self, c: u32) -> bool { c >= FAT_EOC }

    fn read_fat(&self, cluster: u32) -> FatResult<u32> {
        let lba = self.fat_lba(cluster);
        let off = self.fat_offset(cluster);
        let mut sec = [0u8; 512];
        self.drive.read_sectors(lba, 1, &mut sec)?;
        Ok(u32::from_le_bytes([sec[off], sec[off+1], sec[off+2], sec[off+3]]) & 0x0FFF_FFFF)
    }

    fn write_fat(&self, cluster: u32, value: u32) -> FatResult<()> {
        let lba = self.fat_lba(cluster);
        let off = self.fat_offset(cluster);
        let mut sec = [0u8; 512];
        self.drive.read_sectors(lba, 1, &mut sec)?;
        let old = u32::from_le_bytes([sec[off], sec[off+1], sec[off+2], sec[off+3]]);
        let new = (old & 0xF000_0000) | (value & 0x0FFF_FFFF);
        sec[off..off+4].copy_from_slice(&new.to_le_bytes());
        self.drive.write_sectors(lba, 1, &sec)?;
        for f in 1..self.num_fats {
            let lba2 = lba + f as u64 * self.fat_size as u64;
            self.drive.write_sectors(lba2, 1, &sec)?;
        }
        Ok(())
    }

    fn alloc_cluster(&self) -> FatResult<u32> {
        for c in 2..self.clus_count + 2 {
            if self.read_fat(c)? == FAT_FREE {
                self.write_fat(c, 0x0FFF_FFFF)?;
                return Ok(c);
            }
        }
        Err(FatError::NoSpace)
    }

    fn free_chain(&self, start: u32) -> FatResult<()> {
        let mut cur = start;
        while !self.is_eoc(cur) && cur >= 2 {
            let next = self.read_fat(cur)?;
            self.write_fat(cur, FAT_FREE)?;
            cur = next;
        }
        Ok(())
    }

    fn read_cluster(&self, cluster: u32, buf: &mut ClusterBuf) -> FatResult<()> {
        self.drive.read_sectors(self.cluster_lba(cluster), self.sec_per_clus as usize, &mut buf.data[..buf.len])?;
        Ok(())
    }

    fn write_cluster(&self, cluster: u32, buf: &ClusterBuf) -> FatResult<()> {
        self.drive.write_sectors(self.cluster_lba(cluster), self.sec_per_clus as usize, &buf.data[..buf.len])?;
        Ok(())
    }

    // ── API pública ────────────────────────────────────────────────────────────

    pub fn root_cluster(&self) -> u32 { self.root_clus }

    pub fn list_dir<F>(&self, dir_cluster: u32, mut cb: F) -> FatResult<()>
    where F: FnMut(&DirEntryInfo)
    {
        let mut clus = dir_cluster;
        let bpc = self.bpc();
        let mut lfn_buf = [0u16; 256];
        let mut lfn_len = 0usize;

        while !self.is_eoc(clus) && clus >= 2 {
            let mut buf = ClusterBuf::new(bpc);
            self.read_cluster(clus, &mut buf)?;
            let entries = bpc / DIR_ENTRY_SIZE;

            for i in 0..entries {
                let off = i * DIR_ENTRY_SIZE;
                let raw: DirEntry83 = unsafe {
                    core::ptr::read_unaligned(buf.data[off..].as_ptr() as *const DirEntry83)
                };
                if raw.is_end() { return Ok(()); }
                if raw.name[0] == 0xE5 { lfn_len = 0; continue; }
                if raw.is_lfn() {
                    let lfn: LfnEntry = unsafe {
                        core::ptr::read_unaligned(buf.data[off..].as_ptr() as *const LfnEntry)
                    };
                    accumulate_lfn(&lfn, &mut lfn_buf, &mut lfn_len);
                    continue;
                }
                if raw.attr & 0x08 != 0 { lfn_len = 0; continue; }

                let entry_lba = self.cluster_lba(clus) + (off / 512) as u64;
                let entry_off = off % 512;
                let info = build_entry(&raw, &lfn_buf, lfn_len, entry_lba, entry_off);
                cb(&info);
                lfn_len = 0;
            }
            clus = self.read_fat(clus)?;
        }
        Ok(())
    }

    pub fn find_entry(&self, dir_cluster: u32, name: &str) -> FatResult<DirEntryInfo> {
        let mut found: Option<DirEntryInfo> = None;
        self.list_dir(dir_cluster, |e| {
            if found.is_none() && names_eq(e.name_str(), name) {
                found = Some(e.clone());
            }
        })?;
        found.ok_or(FatError::NotFound)
    }

    pub fn read_file(&self, entry: &DirEntryInfo, buf: &mut [u8]) -> FatResult<usize> {
        if entry.is_dir { return Err(FatError::IsDir); }
        let to_read = buf.len().min(entry.size as usize);
        let bpc = self.bpc();
        let mut clus = entry.cluster;
        let mut done = 0usize;
        while done < to_read && !self.is_eoc(clus) && clus >= 2 {
            let mut cb = ClusterBuf::new(bpc);
            self.read_cluster(clus, &mut cb)?;
            let chunk = (to_read - done).min(bpc);
            buf[done..done + chunk].copy_from_slice(&cb.data[..chunk]);
            done += chunk;
            clus = self.read_fat(clus)?;
        }
        Ok(done)
    }

    pub fn write_file(&self, entry: &mut DirEntryInfo, data: &[u8]) -> FatResult<()> {
        if entry.is_dir { return Err(FatError::IsDir); }
        let bpc = self.bpc();
        if entry.cluster != 0 { self.free_chain(entry.cluster)?; }
        let first = self.alloc_cluster()?;
        entry.cluster = first;
        self.update_cluster_field(entry, first)?;

        let mut written = 0usize;
        let mut prev = first;
        while written < data.len() {
            let end = (written + bpc).min(data.len());
            let mut cb = ClusterBuf::new(bpc);
            let chunk = end - written;
            cb.data[..chunk].copy_from_slice(&data[written..end]);
            self.write_cluster(prev, &cb)?;
            written = end;
            if written < data.len() {
                let next = self.alloc_cluster()?;
                self.write_fat(prev, next)?;
                prev = next;
            }
        }
        entry.size = data.len() as u32;
        self.update_size_field(entry, data.len() as u32)?;
        Ok(())
    }

    pub fn create_file(&self, dir_cluster: u32, name: &str) -> FatResult<DirEntryInfo> {
        self.create_entry(dir_cluster, name, false)
    }

    pub fn create_dir(&self, dir_cluster: u32, name: &str) -> FatResult<DirEntryInfo> {
        self.create_entry(dir_cluster, name, true)
    }

    fn create_entry(&self, dir_cluster: u32, name: &str, is_dir: bool) -> FatResult<DirEntryInfo> {
        if name.len() > 255 { return Err(FatError::NameTooLong); }
        let clus = if is_dir {
            let c = self.alloc_cluster()?;
            let cb = ClusterBuf::new(self.bpc());
            self.write_cluster(c, &cb)?;
            c
        } else { 0u32 };

        let (name83, ext83) = make_83(name);
        let attr = if is_dir { ATTR_DIR } else { ATTR_ARCH };
        let raw = DirEntry83 {
            name: name83, ext: ext83, attr,
            clus_hi: (clus >> 16) as u16,
            clus_lo: clus as u16,
            ..DirEntry83::default()
        };
        let (dir_sector, dir_offset) = self.write_dir_entry(dir_cluster, &raw)?;
        let mut nb = [0u8; 256];
        let nl = name.len().min(255);
        nb[..nl].copy_from_slice(name.as_bytes());
        Ok(DirEntryInfo { name: nb, name_len: nl, is_dir, size: 0, cluster: clus, dir_sector, dir_offset })
    }

    pub fn delete_entry(&self, entry: &DirEntryInfo) -> FatResult<()> {
        if entry.cluster != 0 { self.free_chain(entry.cluster)?; }
        let mut sec = [0u8; 512];
        self.drive.read_sectors(entry.dir_sector, 1, &mut sec)?;
        sec[entry.dir_offset] = 0xE5;
        self.drive.write_sectors(entry.dir_sector, 1, &sec)?;
        Ok(())
    }

    fn write_dir_entry(&self, dir_cluster: u32, entry: &DirEntry83) -> FatResult<(u64, usize)> {
        let bpc = self.bpc();
        let mut clus = dir_cluster;
        while !self.is_eoc(clus) && clus >= 2 {
            let mut buf = ClusterBuf::new(bpc);
            self.read_cluster(clus, &mut buf)?;
            for i in 0..bpc / DIR_ENTRY_SIZE {
                let off = i * DIR_ENTRY_SIZE;
                if buf.data[off] == 0x00 || buf.data[off] == 0xE5 {
                    let raw = entry as *const DirEntry83 as *const u8;
                    let bytes = unsafe { core::slice::from_raw_parts(raw, DIR_ENTRY_SIZE) };
                    buf.data[off..off + DIR_ENTRY_SIZE].copy_from_slice(bytes);
                    self.write_cluster(clus, &buf)?;
                    let sector = self.cluster_lba(clus) + (off / 512) as u64;
                    return Ok((sector, off % 512));
                }
            }
            let next = self.read_fat(clus)?;
            if self.is_eoc(next) {
                let nc = self.alloc_cluster()?;
                self.write_fat(clus, nc)?;
                let cb = ClusterBuf::new(bpc);
                self.write_cluster(nc, &cb)?;
                clus = nc;
            } else {
                clus = next;
            }
        }
        Err(FatError::NoSpace)
    }

    fn update_cluster_field(&self, entry: &DirEntryInfo, cluster: u32) -> FatResult<()> {
        let mut sec = [0u8; 512];
        self.drive.read_sectors(entry.dir_sector, 1, &mut sec)?;
        let off = entry.dir_offset;
        // clus_hi at +20, clus_lo at +26
        sec[off + 20] = cluster as u8;        // lo byte of hi word
        sec[off + 21] = (cluster >> 8) as u8; // hi byte of hi word  -- wait, hi word is bits [31:16]
        // Correct layout: clus_hi = cluster[31:16], clus_lo = cluster[15:0]
        let hi = (cluster >> 16) as u16;
        let lo = cluster as u16;
        sec[off + 20..off + 22].copy_from_slice(&hi.to_le_bytes());
        sec[off + 26..off + 28].copy_from_slice(&lo.to_le_bytes());
        self.drive.write_sectors(entry.dir_sector, 1, &sec)?;
        Ok(())
    }

    fn update_size_field(&self, entry: &DirEntryInfo, size: u32) -> FatResult<()> {
        let mut sec = [0u8; 512];
        self.drive.read_sectors(entry.dir_sector, 1, &mut sec)?;
        let off = entry.dir_offset;
        sec[off + 28..off + 32].copy_from_slice(&size.to_le_bytes());
        self.drive.write_sectors(entry.dir_sector, 1, &sec)?;
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn accumulate_lfn(lfn: &LfnEntry, buf: &mut [u16; 256], len: &mut usize) {
    let order = (lfn.order & 0x1F) as usize;
    if order == 0 || order > 20 { return; }
    let base = (order - 1) * 13;
    let mut pos = base;
    // Read LFN name fields safely (packed struct)
macro_rules! push {
    ($offset:expr, $count:expr) => {
        for k in 0..$count {
            let w: u16 = unsafe {
                core::ptr::read_unaligned(
                    (lfn as *const LfnEntry as *const u8).add($offset + k * 2) as *const u16
                )
            };
            if pos < 256 {
                buf[pos] = w;
                pos += 1;
            }
        }
    };
}

push!(1, 5);
push!(14, 6);
push!(28, 2);
    // name1: 5 u16, name2: 6 u16, name3: 2 u16
    for k in 0..5usize {
        let w: u16 = unsafe { core::ptr::read_unaligned(
            (lfn as *const LfnEntry as *const u8).add(1 + k * 2) as *const u16
        )};
        if pos < 256 { buf[pos] = w; pos += 1; }
    }
    for k in 0..6usize {
        let w: u16 = unsafe { core::ptr::read_unaligned(
            (lfn as *const LfnEntry as *const u8).add(14 + k * 2) as *const u16
        )};
        if pos < 256 { buf[pos] = w; pos += 1; }
    }
    for k in 0..2usize {
        let w: u16 = unsafe { core::ptr::read_unaligned(
            (lfn as *const LfnEntry as *const u8).add(28 + k * 2) as *const u16
        )};
        if pos < 256 { buf[pos] = w; pos += 1; }
    }
    if pos > *len { *len = pos; }
}

fn build_entry(raw: &DirEntry83, lfn: &[u16; 256], lfn_len: usize, dir_sector: u64, dir_offset: usize) -> DirEntryInfo {
    let mut name = [0u8; 256];
    let name_len;
    if lfn_len > 0 {
        let mut nl = 0;
        for i in 0..lfn_len {
            let w = lfn[i];
            if w == 0 { break; }
            if nl < 255 { name[nl] = if w < 0x80 { w as u8 } else { b'?' }; nl += 1; }
        }
        name_len = nl;
    } else {
        let mut nl = 0;
        for &b in raw.name.iter() { if b == b' ' { break; } if nl < 255 { name[nl] = b; nl += 1; } }
        if raw.ext[0] != b' ' {
            if nl < 255 { name[nl] = b'.'; nl += 1; }
            for &b in raw.ext.iter() { if b == b' ' { break; } if nl < 255 { name[nl] = b; nl += 1; } }
        }
        name_len = nl;
    }
    DirEntryInfo {
        name, name_len,
        is_dir:     raw.is_dir(),
        size:       raw.file_size(),
        cluster:    raw.cluster(),
        dir_sector,
        dir_offset,
    }
}

fn names_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() &&
    a.bytes().zip(b.bytes()).all(|(x,y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

fn make_83(name: &str) -> ([u8; 8], [u8; 3]) {
    let mut n8 = [b' '; 8]; let mut e3 = [b' '; 3];
    let dot = name.rfind('.');
    let (base, ext) = if let Some(d) = dot { (&name[..d], &name[d+1..]) } else { (name, "") };
    for (i, b) in base.bytes().take(8).enumerate() { n8[i] = b.to_ascii_uppercase(); }
    for (i, b) in ext.bytes().take(3).enumerate() { e3[i] = b.to_ascii_uppercase(); }
    (n8, e3)
}

// ── ClusterBuf ────────────────────────────────────────────────────────────────

const MAX_BPC: usize = 512 * 128; // 64 KiB

struct ClusterBuf {
    data: [u8; MAX_BPC],
    len:  usize,
}

impl ClusterBuf {
    fn new(len: usize) -> Self {
        ClusterBuf { data: [0u8; MAX_BPC], len: len.min(MAX_BPC) }
    }
}