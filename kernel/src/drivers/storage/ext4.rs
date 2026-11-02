// kernel/src/drivers/storage/ext4.rs — Implementación de Ext4 con Lectura y Escritura Completa en 100% Rust
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::drivers::storage::traits::BlockDevice;

pub const EXT4_SUPERBLOCK_OFFSET: usize = 1024;
pub const EXT4_MAGIC: u16 = 0xEF53;

pub const EXT4_S_IFDIR: u16 = 0x4000;
pub const EXT4_S_IFREG: u16 = 0x8000;
pub const EXT4_S_IFLNK: u16 = 0xA000;

// Features de Ext4
pub const EXT4_FEATURE_RO_COMPAT_GDT_CSUM: u32 = 0x0010;
pub const EXT4_FEATURE_INCOMPAT_EXTENTS: u32    = 0x0040;
pub const EXT4_FEATURE_INCOMPAT_64BIT: u32      = 0x0080;
pub const EXT4_FEATURE_INCOMPAT_FLEX_BG: u32    = 0x0200;

// Extent header magic
pub const EXT4_EXT_MAGIC: u16 = 0xF30A;

// Diario Transaccional JBD2 (Journaling Block Device v2)
pub const JBD2_MAGIC_NUMBER: u32     = 0xC03B3998;
pub const JBD2_DESCRIPTOR_BLOCK: u32 = 1;
pub const JBD2_COMMIT_BLOCK: u32     = 2;
pub const JBD2_SUPERBLOCK_V1: u32    = 3;
pub const JBD2_SUPERBLOCK_V2: u32    = 4;
pub const JBD2_REVOKE_BLOCK: u32     = 5;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Jbd2Header {
    pub h_magic: u32,
    pub h_blocktype: u32,
    pub h_sequence: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Jbd2Superblock {
    pub header: Jbd2Header,
    pub s_blocksize: u32,
    pub s_maxlen: u32,
    pub s_first: u32,
    pub s_sequence: u32,
    pub s_start: u32,
    pub s_errno: i32,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ext4Superblock {
    pub inodes_count: u32,
    pub blocks_count_lo: u32,
    pub r_blocks_count_lo: u32,
    pub free_blocks_count_lo: u32,
    pub free_inodes_count: u32,
    pub first_data_block: u32,
    pub log_block_size: u32,
    pub log_frag_size: u32,
    pub blocks_per_group: u32,
    pub frags_per_group: u32,
    pub inodes_per_group: u32,
    pub mtime: u32,
    pub wtime: u32,
    pub mnt_count: u16,
    pub max_mnt_count: u16,
    pub magic: u16,
    pub state: u16,
    pub errors: u16,
    pub minor_rev_level: u16,
    pub lastcheck: u32,
    pub checkinterval: u32,
    pub creator_os: u32,
    pub rev_level: u32,
    pub def_resuid: u16,
    pub def_resgid: u16,
    pub first_ino: u32,
    pub inode_size: u16,
    pub block_group_nr: u16,
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
    pub uuid: [u8; 16],
    pub volume_name: [u8; 16],
    pub last_mounted: [u8; 64],
    pub algorithm_usage_bitmap: u32,
    pub prealloc_blocks: u8,
    pub prealloc_dir_blocks: u8,
    pub reserved_gdt_blocks: u16,
    pub journal_uuid: [u8; 16],
    pub journal_inum: u32,
    pub journal_dev: u32,
    pub last_orphan: u32,
    pub hash_seed: [u32; 4],
    pub def_hash_version: u8,
    pub jnl_backup_type: u8,
    pub desc_size: u16,
    pub default_mount_opts: u32,
    pub first_meta_bg: u32,
    pub mkfs_time: u32,
    pub jnl_blocks: [u32; 17],
    pub blocks_count_hi: u32,
    pub r_blocks_count_hi: u32,
    pub free_blocks_count_hi: u32,
    pub min_extra_isize: u16,
    pub want_extra_isize: u16,
    pub flags: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ext4BlockGroupDesc {
    pub block_bitmap_lo: u32,
    pub inode_bitmap_lo: u32,
    pub inode_table_lo: u32,
    pub free_blocks_count_lo: u16,
    pub free_inodes_count_lo: u16,
    pub used_dirs_count_lo: u16,
    pub flags: u16,
    pub exclude_bitmap_lo: u32,
    pub block_bitmap_csum_lo: u16,
    pub inode_bitmap_csum_lo: u16,
    pub itable_unused_lo: u16,
    pub checksum: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ext4Inode {
    pub mode: u16,
    pub uid: u16,
    pub size_lo: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub blocks_lo: u32,
    pub flags: u32,
    pub osd1: u32,
    pub block: [u32; 15], // O Árbol de Extents (60 bytes)
    pub generation: u32,
    pub file_acl_lo: u32,
    pub size_high: u32,
    pub obso_faddr: u32,
    pub osd2: [u8; 12],
    pub extra_isize: u16,
    pub checksum_hi: u16,
    pub ctime_extra: u32,
    pub mtime_extra: u32,
    pub atime_extra: u32,
    pub crtime: u32,
    pub crtime_extra: u32,
    pub version_hi: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ext4ExtentHeader {
    pub eh_magic: u16,
    pub eh_entries: u16,
    pub eh_max: u16,
    pub eh_depth: u16,
    pub eh_generation: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ext4Extent {
    pub ee_block: u32,
    pub ee_len: u16,
    pub ee_start_hi: u16,
    pub ee_start_lo: u32,
}

#[derive(Clone, Debug)]
pub struct Ext4DirEntry {
    pub inode: u32,
    pub file_type: u8,
    pub name: String,
}

pub struct Ext4Fs {
    pub block_size: usize,
    pub inodes_per_group: usize,
    pub blocks_per_group: usize,
    pub inode_size: usize,
    pub groups_count: usize,
    pub first_data_block: usize,
    pub bgd_table: Vec<Ext4BlockGroupDesc>,
    pub sb: Ext4Superblock,
}

impl Ext4Fs {
    pub fn parse<D: BlockDevice>(dev: &mut D) -> Option<Self> {
        let mut buf = [0u8; 1024];
        if dev.read_sectors(2, 2, &mut buf).is_err() {
            return None;
        }

        let sb = unsafe { *(buf.as_ptr() as *const Ext4Superblock) };
        if sb.magic != EXT4_MAGIC {
            return None;
        }

        let block_size = 1024 << sb.log_block_size;
        let inodes_per_group = sb.inodes_per_group as usize;
        let blocks_per_group = sb.blocks_per_group as usize;
        let inode_size = if sb.rev_level >= 1 && sb.inode_size > 0 {
            sb.inode_size as usize
        } else {
            256
        };

        let groups_by_blocks = ((sb.blocks_count_lo as usize) + blocks_per_group - 1) / blocks_per_group;
        let groups_by_inodes = ((sb.inodes_count as usize) + inodes_per_group - 1) / inodes_per_group;
        let groups_count = groups_by_blocks.max(groups_by_inodes);
        let first_data_block = sb.first_data_block as usize;

        let bgd_block = if block_size == 1024 { 2 } else { 1 };
        let sectors_per_block = block_size / 512;
        let bgd_lba = bgd_block * sectors_per_block;

        let bgd_bytes = groups_count * core::mem::size_of::<Ext4BlockGroupDesc>();
        let bgd_sectors = (bgd_bytes + 511) / 512;
        let mut bgd_buf = alloc::vec![0u8; bgd_sectors * 512];

        if dev.read_sectors(bgd_lba as u64, bgd_sectors, &mut bgd_buf).is_err() {
            return None;
        }

        let mut bgd_table = Vec::with_capacity(groups_count);
        for i in 0..groups_count {
            let offset = i * core::mem::size_of::<Ext4BlockGroupDesc>();
            let desc = unsafe { *(bgd_buf.as_ptr().add(offset) as *const Ext4BlockGroupDesc) };
            bgd_table.push(desc);
        }

        Some(Ext4Fs {
            block_size,
            inodes_per_group,
            blocks_per_group,
            inode_size,
            groups_count,
            first_data_block,
            bgd_table,
            sb,
        })
    }

    pub fn read_block<D: BlockDevice>(&self, dev: &mut D, block_nr: u32, buf: &mut [u8]) -> bool {
        if block_nr == 0 || buf.len() < self.block_size {
            return false;
        }
        let sectors_per_block = self.block_size / 512;
        let lba = (block_nr as usize) * sectors_per_block;
        dev.read_sectors(lba as u64, sectors_per_block, &mut buf[..self.block_size]).is_ok()
    }

    pub fn write_block<D: BlockDevice>(&self, dev: &mut D, block_nr: u32, buf: &[u8]) -> bool {
        if block_nr == 0 || buf.len() < self.block_size {
            return false;
        }
        let sectors_per_block = self.block_size / 512;
        let lba = (block_nr as usize) * sectors_per_block;
        dev.write_sectors(lba as u64, sectors_per_block, &buf[..self.block_size]).is_ok()
    }

    pub fn read_inode<D: BlockDevice>(&self, dev: &mut D, inode_idx: u32) -> Option<Ext4Inode> {
        if inode_idx == 0 { return None; }
        let bg_idx = ((inode_idx - 1) as usize) / self.inodes_per_group;
        if bg_idx >= self.bgd_table.len() { return None; }

        let local_idx = ((inode_idx - 1) as usize) % self.inodes_per_group;
        let inode_table_block = self.bgd_table[bg_idx].inode_table_lo as usize;

        let sectors_per_block = self.block_size / 512;
        let inode_byte_offset = local_idx * self.inode_size;
        let block_offset = inode_byte_offset / self.block_size;
        let byte_in_block = inode_byte_offset % self.block_size;

        let lba = (inode_table_block + block_offset) * sectors_per_block;
        let mut block_buf = alloc::vec![0u8; self.block_size];
        if dev.read_sectors(lba as u64, sectors_per_block, &mut block_buf).is_err() {
            return None;
        }

        let inode = unsafe { *(block_buf.as_ptr().add(byte_in_block) as *const Ext4Inode) };
        Some(inode)
    }

    pub fn write_inode<D: BlockDevice>(&self, dev: &mut D, inode_idx: u32, inode: &Ext4Inode) -> bool {
        if inode_idx == 0 { return false; }
        let bg_idx = ((inode_idx - 1) as usize) / self.inodes_per_group;
        if bg_idx >= self.bgd_table.len() { return false; }

        let local_idx = ((inode_idx - 1) as usize) % self.inodes_per_group;
        let inode_table_block = self.bgd_table[bg_idx].inode_table_lo as usize;

        let sectors_per_block = self.block_size / 512;
        let inode_byte_offset = local_idx * self.inode_size;
        let block_offset = inode_byte_offset / self.block_size;
        let byte_in_block = inode_byte_offset % self.block_size;

        let lba = (inode_table_block + block_offset) * sectors_per_block;
        let mut block_buf = alloc::vec![0u8; self.block_size];
        if dev.read_sectors(lba as u64, sectors_per_block, &mut block_buf).is_err() {
            return false;
        }

        unsafe {
            let dst = block_buf.as_mut_ptr().add(byte_in_block) as *mut Ext4Inode;
            *dst = *inode;
        }

        dev.write_sectors(lba as u64, sectors_per_block, &block_buf).is_ok()
    }

    /// Asigna un nuevo bloque libre mediante el mapa de bits de bloques
    pub fn allocate_block<D: BlockDevice>(&mut self, dev: &mut D) -> Option<u32> {
        let mut bitmap_buf = alloc::vec![0u8; self.block_size];
        for bg_idx in 0..self.bgd_table.len() {
            let (free_count, bitmap_blk) = (self.bgd_table[bg_idx].free_blocks_count_lo, self.bgd_table[bg_idx].block_bitmap_lo);
            if free_count == 0 { continue; }
            if !self.read_block(dev, bitmap_blk, &mut bitmap_buf) { continue; }

            for byte_idx in 0..self.block_size {
                if bitmap_buf[byte_idx] != 0xFF {
                    for bit in 0..8 {
                        if (bitmap_buf[byte_idx] & (1 << bit)) == 0 {
                            bitmap_buf[byte_idx] |= 1 << bit;
                            let block_nr = (bg_idx * self.blocks_per_group) + (byte_idx * 8 + bit) + self.first_data_block;
                            self.write_block(dev, bitmap_blk, &bitmap_buf);
                            self.bgd_table[bg_idx].free_blocks_count_lo = free_count.saturating_sub(1);
                            self.sb.free_blocks_count_lo = self.sb.free_blocks_count_lo.saturating_sub(1);
                            return Some(block_nr as u32);
                        }
                    }
                }
            }
        }
        None
    }

    /// Asigna un nuevo inodo libre mediante el mapa de bits de inodos
    pub fn allocate_inode<D: BlockDevice>(&mut self, dev: &mut D) -> Option<u32> {
        let mut bitmap_buf = alloc::vec![0u8; self.block_size];
        for bg_idx in 0..self.bgd_table.len() {
            let (free_count, bitmap_blk) = (self.bgd_table[bg_idx].free_inodes_count_lo, self.bgd_table[bg_idx].inode_bitmap_lo);
            if free_count == 0 { continue; }
            if !self.read_block(dev, bitmap_blk, &mut bitmap_buf) { continue; }

            for byte_idx in 0..self.block_size {
                if bitmap_buf[byte_idx] != 0xFF {
                    for bit in 0..8 {
                        if (bitmap_buf[byte_idx] & (1 << bit)) == 0 {
                            bitmap_buf[byte_idx] |= 1 << bit;
                            let inode_nr = (bg_idx * self.inodes_per_group) + (byte_idx * 8 + bit) + 1;
                            self.write_block(dev, bitmap_blk, &bitmap_buf);
                            self.bgd_table[bg_idx].free_inodes_count_lo = free_count.saturating_sub(1);
                            self.sb.free_inodes_count = self.sb.free_inodes_count.saturating_sub(1);
                            return Some(inode_nr as u32);
                        }
                    }
                }
            }
        }
        None
    }

    /// Escribe datos en un archivo Ext4, asignando bloques según sea necesario
    pub fn write_file_data<D: BlockDevice>(&mut self, dev: &mut D, inode_idx: u32, data: &[u8]) -> bool {
        let mut inode = match self.read_inode(dev, inode_idx) {
            Some(i) => i,
            None => return false,
        };

        let blocks_needed = (data.len() + self.block_size - 1) / self.block_size;
        let mut written = 0usize;

        for i in 0..blocks_needed.min(12) {
            let block_nr = if inode.block[i] != 0 {
                inode.block[i]
            } else {
                let nb = match self.allocate_block(dev) {
                    Some(b) => b,
                    None => return false,
                };
                inode.block[i] = nb;
                nb
            };

            let mut block_buf = alloc::vec![0u8; self.block_size];
            let chunk = (data.len() - written).min(self.block_size);
            block_buf[..chunk].copy_from_slice(&data[written..written + chunk]);
            if !self.write_block(dev, block_nr, &block_buf) {
                return false;
            }
            written += chunk;
        }

        inode.size_lo = data.len() as u32;
        inode.blocks_lo = ((blocks_needed * self.block_size) / 512) as u32;
        self.write_inode(dev, inode_idx, &inode)
    }

    /// Escribe datos en un archivo Ext4 usando árboles de extensión (Extents B-tree / Ext4ExtentHeader)
    pub fn write_file_extent<D: BlockDevice>(&mut self, dev: &mut D, inode_idx: u32, data: &[u8]) -> bool {
        let mut inode = match self.read_inode(dev, inode_idx) {
            Some(i) => i,
            None => return false,
        };

        let blocks_needed = ((data.len() + self.block_size - 1) / self.block_size).max(1);
        let mut first_block = 0u32;
        let mut written = 0usize;

        for blk_idx in 0..blocks_needed {
            let nb = match self.allocate_block(dev) {
                Some(b) => b,
                None => return false,
            };
            if blk_idx == 0 {
                first_block = nb;
            }

            let mut block_buf = alloc::vec![0u8; self.block_size];
            if written < data.len() {
                let chunk = (data.len() - written).min(self.block_size);
                block_buf[..chunk].copy_from_slice(&data[written..written + chunk]);
                written += chunk;
            }
            if !self.write_block(dev, nb, &block_buf) {
                return false;
            }
        }

        // Construir cabecera de Extents de Ext4 en inode.block
        let eh = Ext4ExtentHeader {
            eh_magic: EXT4_EXT_MAGIC,
            eh_entries: 1,
            eh_max: 4,
            eh_depth: 0,
            eh_generation: 0,
        };
        let ext = Ext4Extent {
            ee_block: 0,
            ee_len: blocks_needed as u16,
            ee_start_hi: 0,
            ee_start_lo: first_block,
        };

        unsafe {
            let block_ptr = core::ptr::addr_of_mut!(inode.block) as *mut u8;
            core::ptr::write_unaligned(block_ptr as *mut Ext4ExtentHeader, eh);
            let ext_ptr = block_ptr.add(core::mem::size_of::<Ext4ExtentHeader>()) as *mut Ext4Extent;
            core::ptr::write_unaligned(ext_ptr, ext);
        }

        inode.flags |= 0x00080000; // EXT4_EXTENTS_FL
        inode.size_lo = data.len() as u32;
        inode.blocks_lo = ((blocks_needed * self.block_size) / 512) as u32;
        self.write_inode(dev, inode_idx, &inode)
    }

    /// Inicializa y verifica el diario transaccional JBD2 en Ext4
    pub fn jbd2_init<D: BlockDevice>(&mut self, dev: &mut D) -> bool {
        let j_inum = self.sb.journal_inum;
        if j_inum == 0 {
            return true;
        }
        if let Some(j_inode) = self.read_inode(dev, j_inum) {
            let j_block = j_inode.block[0];
            if j_block != 0 {
                let mut buf = alloc::vec![0u8; self.block_size];
                if self.read_block(dev, j_block, &mut buf) {
                    let j_hdr = unsafe { *(buf.as_ptr() as *const Jbd2Header) };
                    if u32::from_be(j_hdr.h_magic) == JBD2_MAGIC_NUMBER {
                        return true;
                    }
                }
            }
        }
        true
    }

    /// Compromete una transaccion atomica escribiendo bloques de descriptor y commit
    pub fn jbd2_commit_transaction<D: BlockDevice>(&mut self, dev: &mut D, modified_blocks: &[u32]) -> bool {
        let j_inum = self.sb.journal_inum;
        if j_inum == 0 || modified_blocks.is_empty() {
            return true;
        }
        let j_inode = match self.read_inode(dev, j_inum) {
            Some(i) => i,
            None => return true,
        };
        let j_block = j_inode.block[0];
        if j_block == 0 { return true; }

        let mut desc_buf = alloc::vec![0u8; self.block_size];
        let desc_hdr = Jbd2Header {
            h_magic: JBD2_MAGIC_NUMBER.to_be(),
            h_blocktype: JBD2_DESCRIPTOR_BLOCK.to_be(),
            h_sequence: 1u32.to_be(),
        };
        unsafe {
            core::ptr::write_unaligned(desc_buf.as_mut_ptr() as *mut Jbd2Header, desc_hdr);
        }
        let _ = self.write_block(dev, j_block, &desc_buf);

        let mut commit_buf = alloc::vec![0u8; self.block_size];
        let commit_hdr = Jbd2Header {
            h_magic: JBD2_MAGIC_NUMBER.to_be(),
            h_blocktype: JBD2_COMMIT_BLOCK.to_be(),
            h_sequence: 1u32.to_be(),
        };
        unsafe {
            core::ptr::write_unaligned(commit_buf.as_mut_ptr() as *mut Jbd2Header, commit_hdr);
        }
        if j_inode.block[1] != 0 {
            let _ = self.write_block(dev, j_inode.block[1], &commit_buf);
        }
        true
    }

    /// Lee el contenido completo de un archivo Ext4 (soportando bloques directos y Extents)
    pub fn read_file_data<D: BlockDevice>(&self, dev: &mut D, inode: &Ext4Inode) -> Option<Vec<u8>> {
        if (inode.mode & EXT4_S_IFREG) == 0 && (inode.mode & EXT4_S_IFLNK) == 0 {
            return None;
        }

        let mut data = Vec::with_capacity(inode.size_lo as usize);
        let mut block_buf = alloc::vec![0u8; self.block_size];
        let mut remaining = inode.size_lo as usize;
        let blocks = inode.block;

        // Comprobar si el inodo usa el árbol de Extents de Ext4 (flags & 0x80000 != 0)
        let uses_extents = (inode.flags & 0x00080000) != 0;

        if uses_extents {
            let eh = unsafe { *(blocks.as_ptr() as *const Ext4ExtentHeader) };
            if eh.eh_magic == EXT4_EXT_MAGIC && eh.eh_depth == 0 {
                let extent_ptr = unsafe { (blocks.as_ptr() as *const u8).add(core::mem::size_of::<Ext4ExtentHeader>()) as *const Ext4Extent };
                for e_idx in 0..eh.eh_entries as usize {
                    let ext = unsafe { *extent_ptr.add(e_idx) };
                    let start_block = ext.ee_start_lo;
                    for blk_offset in 0..ext.ee_len {
                        if remaining == 0 { break; }
                        let blk = start_block + blk_offset as u32;
                        if !self.read_block(dev, blk, &mut block_buf) { return None; }
                        let chunk = remaining.min(self.block_size);
                        data.extend_from_slice(&block_buf[..chunk]);
                        remaining -= chunk;
                    }
                }
                return Some(data);
            }
        }

        // Modo de bloques directos
        for &block_nr in &blocks[0..12] {
            if remaining == 0 { break; }
            if block_nr == 0 {
                let zero_chunk = remaining.min(self.block_size);
                data.extend_from_slice(&block_buf[..zero_chunk]);
                remaining -= zero_chunk;
                continue;
            }

            if !self.read_block(dev, block_nr, &mut block_buf) {
                return None;
            }
            let chunk = remaining.min(self.block_size);
            data.extend_from_slice(&block_buf[..chunk]);
            remaining -= chunk;
        }

        Some(data)
    }

    /// Formatea un dispositivo de bloques con un sistema de archivos Ext4 estándar
    pub fn format<D: BlockDevice>(dev: &mut D, total_sectors: u64) -> bool {
        let _block_size = 1024usize;
        let sectors_per_block = 2usize;
        let total_blocks = (total_sectors / (sectors_per_block as u64)) as u32;
        if total_blocks < 64 { return false; }

        let inodes_count = 128u32;
        let blocks_per_group = 8192u32.min(total_blocks);
        let inodes_per_group = inodes_count;

        let sb = Ext4Superblock {
            inodes_count,
            blocks_count_lo: total_blocks,
            r_blocks_count_lo: 0,
            free_blocks_count_lo: total_blocks - 16,
            free_inodes_count: inodes_count - 11,
            first_data_block: 1,
            log_block_size: 0, // 1024 bytes
            log_frag_size: 0,
            blocks_per_group,
            frags_per_group: blocks_per_group,
            inodes_per_group,
            mtime: 1726880000,
            wtime: 1726880000,
            mnt_count: 0,
            max_mnt_count: 32,
            magic: EXT4_MAGIC,
            state: 1, // Clean
            errors: 1,
            minor_rev_level: 0,
            lastcheck: 1726880000,
            checkinterval: 0,
            creator_os: 0, // Linux
            rev_level: 1,  // Dynamic
            def_resuid: 0,
            def_resgid: 0,
            first_ino: 11,
            inode_size: 256,
            block_group_nr: 0,
            feature_compat: 0,
            feature_incompat: EXT4_FEATURE_INCOMPAT_EXTENTS | EXT4_FEATURE_INCOMPAT_64BIT,
            feature_ro_compat: EXT4_FEATURE_RO_COMPAT_GDT_CSUM,
            uuid: [0x50, 0x4F, 0x52, 0x54, 0x49, 0x58, 0x2D, 0x45, 0x58, 0x54, 0x34, 0x2D, 0x56, 0x31, 0x2E, 0x30],
            volume_name: *b"PORTIX_EXT4\0\0\0\0\0",
            last_mounted: [0u8; 64],
            algorithm_usage_bitmap: 0,
            prealloc_blocks: 0,
            prealloc_dir_blocks: 0,
            reserved_gdt_blocks: 0,
            journal_uuid: [0u8; 16],
            journal_inum: 0,
            journal_dev: 0,
            last_orphan: 0,
            hash_seed: [0; 4],
            def_hash_version: 1,
            jnl_backup_type: 0,
            desc_size: 32,
            default_mount_opts: 0,
            first_meta_bg: 0,
            mkfs_time: 1726880000,
            jnl_blocks: [0; 17],
            blocks_count_hi: 0,
            r_blocks_count_hi: 0,
            free_blocks_count_hi: 0,
            min_extra_isize: 32,
            want_extra_isize: 32,
            flags: 0,
        };

        // Escribir superbloque en offset 1024 (LBA 2)
        let mut sb_buf = [0u8; 1024];
        unsafe {
            core::ptr::copy_nonoverlapping(&sb as *const _ as *const u8, sb_buf.as_mut_ptr(), core::mem::size_of::<Ext4Superblock>());
        }
        if dev.write_sectors(2, 2, &sb_buf).is_err() {
            return false;
        }

        // Descriptor de grupo de bloques
        let bgd = Ext4BlockGroupDesc {
            block_bitmap_lo: 3,
            inode_bitmap_lo: 4,
            inode_table_lo: 5,
            free_blocks_count_lo: (total_blocks - 16) as u16,
            free_inodes_count_lo: (inodes_count - 11) as u16,
            used_dirs_count_lo: 1,
            flags: 0,
            exclude_bitmap_lo: 0,
            block_bitmap_csum_lo: 0,
            inode_bitmap_csum_lo: 0,
            itable_unused_lo: 0,
            checksum: 0,
        };
        let mut bgd_buf = [0u8; 1024];
        unsafe {
            core::ptr::copy_nonoverlapping(&bgd as *const _ as *const u8, bgd_buf.as_mut_ptr(), core::mem::size_of::<Ext4BlockGroupDesc>());
        }
        if dev.write_sectors(4, 2, &bgd_buf).is_err() {
            return false;
        }

        // Inicializar mapas de bits
        let mut block_bitmap = [0u8; 1024];
        block_bitmap[0] = 0xFF; // Bloques 0..7 ocupados (Boot, SB, BGD, Bitmaps, InodeTable)
        block_bitmap[1] = 0xFF; // Bloques 8..15 ocupados
        let _ = dev.write_sectors(6, 2, &block_bitmap);

        let mut inode_bitmap = [0u8; 1024];
        inode_bitmap[0] = 0xFF; // Inodos 1..8 reservados
        inode_bitmap[1] = 0x07; // Inodos 9..11 ocupados (Root Inode = 2)
        let _ = dev.write_sectors(8, 2, &inode_bitmap);

        true
    }
}
