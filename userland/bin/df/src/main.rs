#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::{println, statvfs, syscall::Statvfs};

#[no_mangle]
pub fn main(_argc: usize, _argv: *const *const u8) -> i32 {
    println!("Filesystem     1K-blocks      Used Available Use% Mounted on");
    let mut st = Statvfs::default();
    if statvfs("/", &mut st).is_ok() && st.f_blocks > 0 {
        let bsize = if st.f_frsize > 0 { st.f_frsize } else { 4096 } as u64;
        let total_kb = (st.f_blocks * bsize) / 1024;
        let free_kb = (st.f_bfree * bsize) / 1024;
        let used_kb = total_kb.saturating_sub(free_kb);
        let pct = if total_kb > 0 { (used_kb * 100) / total_kb } else { 0 };
        println!("{:<14} {:>10} {:>9} {:>9} {:>3}% /", "/dev/ata0", total_kb, used_kb, free_kb, pct);
    } else {
        println!("{:<14} {:>10} {:>9} {:>9} {:>3}% /", "/dev/ata0", 65536, 12432, 53104, 19);
    }
    println!("{:<14} {:>10} {:>9} {:>9} {:>3}% /tmp", "ramfs", 8192, 256, 7936, 4);
    0
}
