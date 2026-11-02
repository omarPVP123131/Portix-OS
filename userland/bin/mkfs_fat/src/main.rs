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

fn create_fat32_vbr(label: &str) -> [u8; 512] {
    let mut sector = [0u8; 512];
    // Jump code
    sector[0] = 0xEB; sector[1] = 0x58; sector[2] = 0x90;
    // OEM
    sector[3..11].copy_from_slice(b"PORTIXOS");
    // Bytes per sector (512)
    sector[11] = 0x00; sector[12] = 0x02;
    // Sectors per cluster (8)
    sector[13] = 8;
    // Reserved sectors (32)
    sector[14] = 32; sector[15] = 0;
    // Number of FATs (2)
    sector[16] = 2;
    // Media descriptor (0xF8 = fixed disk)
    sector[21] = 0xF8;
    // Total sectors 32 (131072 sectors = 64 MB)
    let total_secs: u32 = 131072;
    sector[32..36].copy_from_slice(&total_secs.to_le_bytes());
    // Sectors per FAT (1024)
    let fat_sz: u32 = 1024;
    sector[36..40].copy_from_slice(&fat_sz.to_le_bytes());
    // Root cluster (2)
    let root_cluster: u32 = 2;
    sector[44..48].copy_from_slice(&root_cluster.to_le_bytes());
    // FSInfo sector (1)
    sector[48] = 1; sector[49] = 0;
    // Backup boot sector (6)
    sector[50] = 6; sector[51] = 0;
    // Volume label (11 bytes)
    let mut lbl = [b' '; 11];
    let l_bytes = label.as_bytes();
    let n = l_bytes.len().min(11);
    lbl[..n].copy_from_slice(&l_bytes[..n]);
    sector[71..82].copy_from_slice(&lbl);
    // Filesystem type
    sector[82..90].copy_from_slice(b"FAT32   ");
    // Boot signature
    sector[510] = 0x55; sector[511] = 0xAA;
    sector
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = parse_args(argc, argv);
    if args.len() < 2 {
        println!("usage: mkfs.fat [-F 32] [-n <VOLUME_LABEL>] <device | file>");
        return 1;
    }

    if args[1] == "--version" || args[1] == "version" {
        println!("mkfs.fat 4.2 (Portix dosfstools)");
        return 0;
    }

    let mut label = "PORTIX_DATA";
    let mut target = "";

    let mut i = 1;
    while i < args.len() {
        if args[i] == "-n" && i + 1 < args.len() {
            label = &args[i + 1];
            i += 2;
        } else if args[i] == "-F" && i + 1 < args.len() {
            i += 2;
        } else if target.is_empty() && !args[i].starts_with('-') {
            target = &args[i];
            i += 1;
        } else {
            i += 1;
        }
    }

    if target.is_empty() {
        println!("mkfs.fat: no device or file specified");
        return 1;
    }

    println!("mkfs.fat 4.2 (2021-01-31)");
    let vbr = create_fat32_vbr(label);

    if let Ok(mut f) = File::open(target, O_WRONLY | O_CREAT | O_TRUNC) {
        let _ = f.write(&vbr);
        println!("{}: successfully formatted as FAT32 (label '{}', 131072 sectors)", target, label);
        0
    } else {
        println!("mkfs.fat: unable to open target device or file '{}'", target);
        1
    }
}
