#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(_argc: usize, _argv: *const *const u8) -> i32 {
    let fd = portix_rt::syscall::sys_open(b"/etc/hostname\0".as_ptr(), 0) as i32;
    let mut h: u32 = 0x007f0101;
    if fd >= 0 {
        let mut buf = [0u8; 64];
        let n = portix_rt::syscall::sys_read(fd, buf.as_mut_ptr(), buf.len());
        portix_rt::syscall::sys_close(fd);
        if n > 0 {
            let mut acc: u32 = 0;
            for &b in &buf[..n as usize] {
                if b == b'\n' || b == b'\r' || b == 0 { break; }
                acc = acc.wrapping_mul(31).wrapping_add(b as u32);
            }
            if acc != 0 {
                h = acc;
            }
        }
    }
    println!("{:08x}", h);
    0
}
