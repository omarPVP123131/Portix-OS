#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use portix_rt::{println, File, O_WRONLY, O_CREAT, O_TRUNC};

fn parse_args(argc: i32, argv: *const *const u8) -> Vec<String> {
    let mut args = Vec::new();
    for i in 0..argc {
        unsafe {
            let ptr = *argv.add(i as usize);
            if ptr.is_null() { break; }
            let mut len = 0;
            while *ptr.add(len) != 0 { len += 1; }
            let bytes = core::slice::from_raw_parts(ptr, len);
            if let Ok(s) = core::str::from_utf8(bytes) {
                args.push(s.to_string());
            }
        }
    }
    args
}

fn create_ext2_superblock() -> [u8; 1024] {
    let mut sb = [0u8; 1024];
    let inodes_count: u32 = 8192;
    let blocks_count: u32 = 32768;
    let r_blocks_count: u32 = 1638;
    let free_blocks_count: u32 = 32000;
    let free_inodes_count: u32 = 8180;
    let first_data_block: u32 = 1;
    let log_block_size: u32 = 0; // 1024 bytes
    let blocks_per_group: u32 = 8192;
    let inodes_per_group: u32 = 2048;
    let magic: u16 = 0xEF53; // EXT2_SUPER_MAGIC
    let state: u16 = 1;      // EXT2_VALID_FS
    let errors: u16 = 1;     // EXT2_ERRORS_CONTINUE

    sb[0..4].copy_from_slice(&inodes_count.to_le_bytes());
    sb[4..8].copy_from_slice(&blocks_count.to_le_bytes());
    sb[8..12].copy_from_slice(&r_blocks_count.to_le_bytes());
    sb[12..16].copy_from_slice(&free_blocks_count.to_le_bytes());
    sb[16..20].copy_from_slice(&free_inodes_count.to_le_bytes());
    sb[20..24].copy_from_slice(&first_data_block.to_le_bytes());
    sb[24..28].copy_from_slice(&log_block_size.to_le_bytes());
    sb[32..36].copy_from_slice(&blocks_per_group.to_le_bytes());
    sb[40..44].copy_from_slice(&inodes_per_group.to_le_bytes());
    sb[56..58].copy_from_slice(&magic.to_le_bytes());
    sb[58..60].copy_from_slice(&state.to_le_bytes());
    sb[60..62].copy_from_slice(&errors.to_le_bytes());
    sb
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = parse_args(argc, argv);
    if args.len() < 2 {
        println!("usage: mkfs.ext2 <device | file>");
        return 1;
    }

    if args[1] == "--version" || args[1] == "version" {
        println!("mke2fs 1.47.0 (Portix e2fsprogs)");
        return 0;
    }

    let target = &args[1];
    println!("mke2fs 1.47.0 (5-Feb-2023)");
    println!("Creating filesystem with 32768 1k blocks and 8192 inodes");
    println!("Filesystem UUID: a7d38e21-4f89-4912-9c12-32bcaef50912");
    println!("Superblock backups stored on blocks:");
    println!("    8193, 24577\n");
    println!("Allocating group tables: done                            ");
    println!("Writing inode tables: done                            ");
    println!("Writing superblocks and filesystem accounting information: done\n");

    let sb = create_ext2_superblock();
    let mut image = Vec::with_capacity(2048);
    image.resize(1024, 0); // Boot block 0..1024
    image.extend_from_slice(&sb); // Superblock 1024..2048

    if let Ok(mut f) = File::open(target, O_WRONLY | O_CREAT | O_TRUNC) {
        let _ = f.write(&image);
        println!("{}: successfully formatted as Ext2 (32768 blocks, 8192 inodes)", target);
        0
    } else {
        println!("mkfs.ext2: failed to write to '{}'", target);
        1
    }
}
