// kernel/src/drivers/storage/ext2.rs — Implementación del Sistema de Archivos Ext2 en Rust Puro
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::drivers::storage::traits::BlockDevice;

pub const EXT2_SUPERBLOCK_OFFSET: usize = 1024;
pub const EXT2_MAGIC: u16 = 0xEF53;

pub const EXT2_S_IFDIR: u16 = 0x4000;
pub const EXT2_S_IFREG: u16 = 0x8000;
pub const EXT2_S_IFLNK: u16 = 0xA000;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ext2Superblock {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub r_blocks_count: u32,
    pub free_blocks_count: u32,
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
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ext2BlockGroupDesc {
    pub block_bitmap: u32,
    pub inode_bitmap: u32,
    pub inode_table: u32,
    pub free_blocks_count: u16,
    pub free_inodes_count: u16,
    pub used_dirs_count: u16,
    pub pad: u16,
    pub reserved: [u32; 3],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ext2Inode {
    pub mode: u16,
    pub uid: u16,
    pub size: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub blocks: u32,
    pub flags: u32,
    pub osd1: u32,
    pub block: [u32; 15], // 0-11 directos, 12 indirecto simple, 13 doble, 14 triple
    pub generation: u32,
    pub file_acl: u32,
    pub dir_acl: u32,
    pub faddr: u32,
    pub osd2: [u32; 3],
}

#[derive(Clone, Debug)]
pub struct Ext2DirEntry {
    pub inode: u32,
    pub file_type: u8,
    pub name: String,
}

pub struct Ext2Fs {
    pub block_size: usize,
    pub inodes_per_group: usize,
    pub blocks_per_group: usize,
    pub inode_size: usize,
    pub groups_count: usize,
    pub first_data_block: usize,
    pub bgd_table: Vec<Ext2BlockGroupDesc>,
}

impl Ext2Fs {
    pub fn parse<D: BlockDevice>(dev: &mut D) -> Option<Self> {
        // Leer el superbloque en offset 1024 (LBA 2 en bloques de 512 bytes)
        let mut buf = [0u8; 1024];
        if dev.read_sectors(2, 2, &mut buf).is_err() {
            return None;
        }

        let sb = unsafe { *(buf.as_ptr() as *const Ext2Superblock) };
        if sb.magic != EXT2_MAGIC {
            return None;
        }

        let block_size = 1024 << sb.log_block_size;
        let inodes_per_group = sb.inodes_per_group as usize;
        let blocks_per_group = sb.blocks_per_group as usize;
        let inode_size = if sb.rev_level >= 1 && sb.inode_size > 0 {
            sb.inode_size as usize
        } else {
            128
        };

        let groups_by_blocks = ((sb.blocks_count as usize) + blocks_per_group - 1) / blocks_per_group;
        let groups_by_inodes = ((sb.inodes_count as usize) + inodes_per_group - 1) / inodes_per_group;
        let groups_count = groups_by_blocks.max(groups_by_inodes);

        let first_data_block = sb.first_data_block as usize;

        // Leer la tabla de descriptores de grupos de bloques (inmediatamente tras el superbloque)
        let bgd_block = if block_size == 1024 { 2 } else { 1 };
        let sectors_per_block = block_size / 512;
        let bgd_lba = bgd_block * sectors_per_block;

        let bgd_bytes = groups_count * core::mem::size_of::<Ext2BlockGroupDesc>();
        let bgd_sectors = (bgd_bytes + 511) / 512;
        let mut bgd_buf = alloc::vec![0u8; bgd_sectors * 512];

        if dev.read_sectors(bgd_lba as u64, bgd_sectors, &mut bgd_buf).is_err() {
            return None;
        }

        let mut bgd_table = Vec::with_capacity(groups_count);
        for i in 0..groups_count {
            let offset = i * core::mem::size_of::<Ext2BlockGroupDesc>();
            let desc = unsafe { *(bgd_buf.as_ptr().add(offset) as *const Ext2BlockGroupDesc) };
            bgd_table.push(desc);
        }

        Some(Self {
            block_size,
            inodes_per_group,
            blocks_per_group,
            inode_size,
            groups_count,
            first_data_block,
            bgd_table,
        })
    }

    /// Lee un inodo dado su índice (1-based)
    pub fn read_inode<D: BlockDevice>(&self, dev: &mut D, inode_idx: u32) -> Option<Ext2Inode> {
        if inode_idx == 0 {
            return None;
        }

        let bg_idx = ((inode_idx - 1) as usize) / self.inodes_per_group;
        if bg_idx >= self.bgd_table.len() {
            return None;
        }

        let local_idx = ((inode_idx - 1) as usize) % self.inodes_per_group;
        let inode_table_block = self.bgd_table[bg_idx].inode_table as usize;

        let sectors_per_block = self.block_size / 512;
        let inode_byte_offset = local_idx * self.inode_size;
        let block_offset = inode_byte_offset / self.block_size;
        let byte_in_block = inode_byte_offset % self.block_size;

        let lba = (inode_table_block + block_offset) * sectors_per_block;
        let mut block_buf = alloc::vec![0u8; self.block_size];
        if dev.read_sectors(lba as u64, sectors_per_block, &mut block_buf).is_err() {
            return None;
        }

        let inode = unsafe { *(block_buf.as_ptr().add(byte_in_block) as *const Ext2Inode) };
        Some(inode)
    }

    /// Lee el contenido de un bloque del filesystem
    pub fn read_block<D: BlockDevice>(&self, dev: &mut D, block_nr: u32, buf: &mut [u8]) -> bool {
        if block_nr == 0 || buf.len() < self.block_size {
            return false;
        }
        let sectors_per_block = self.block_size / 512;
        let lba = (block_nr as usize) * sectors_per_block;
        dev.read_sectors(lba as u64, sectors_per_block, &mut buf[..self.block_size]).is_ok()
    }

    /// Lista las entradas de un directorio (inodo tipo DIR)
    pub fn list_directory<D: BlockDevice>(&self, dev: &mut D, inode: &Ext2Inode) -> Option<Vec<Ext2DirEntry>> {
        if (inode.mode & EXT2_S_IFDIR) == 0 {
            return None;
        }

        let mut entries = Vec::new();
        let mut block_buf = alloc::vec![0u8; self.block_size];
        let blocks = inode.block;

        for &block_nr in &blocks[0..12] {
            if block_nr == 0 {
                break;
            }
            if !self.read_block(dev, block_nr, &mut block_buf) {
                break;
            }

            let mut off = 0;
            while off + 8 <= self.block_size {
                let ino = u32::from_le_bytes([block_buf[off], block_buf[off+1], block_buf[off+2], block_buf[off+3]]);
                let rec_len = u16::from_le_bytes([block_buf[off+4], block_buf[off+5]]) as usize;
                let name_len = block_buf[off+6] as usize;
                let file_type = block_buf[off+7];

                if rec_len == 0 || off + rec_len > self.block_size {
                    break;
                }

                if ino != 0 && name_len > 0 && off + 8 + name_len <= self.block_size {
                    let name_bytes = &block_buf[off+8..off+8+name_len];
                    if let Ok(name_str) = core::str::from_utf8(name_bytes) {
                        entries.push(Ext2DirEntry {
                            inode: ino,
                            file_type,
                            name: String::from(name_str),
                        });
                    }
                }
                off += rec_len;
            }
        }

        Some(entries)
    }

    /// Lee los datos completos de un archivo regular
    pub fn read_file_data<D: BlockDevice>(&self, dev: &mut D, inode: &Ext2Inode) -> Option<Vec<u8>> {
        if (inode.mode & EXT2_S_IFREG) == 0 && (inode.mode & EXT2_S_IFLNK) == 0 {
            return None;
        }

        let mut data = Vec::with_capacity(inode.size as usize);
        let mut block_buf = alloc::vec![0u8; self.block_size];
        let mut remaining = inode.size as usize;
        let blocks = inode.block;

        for &block_nr in &blocks[0..12] {
            if remaining == 0 {
                break;
            }
            if block_nr == 0 {
                // Hueco (sparse file): rellenar con ceros
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
}
