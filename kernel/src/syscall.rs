pub const SYS_EXIT:   u64 = 0;
pub const SYS_WRITE:  u64 = 1;
pub const SYS_GETPID: u64 = 2;

extern "C" {
    fn ring3_exit_trampoline();
}

#[no_mangle]
extern "C" fn syscall_dispatch(num: u64, a1: u64, a2: u64, a3: u64, _a4: u64, _a5: u64) -> u64 {
    match num {
        SYS_EXIT => sys_exit(a1 as usize),
        SYS_WRITE => sys_write(a1 as i32, a2 as usize, a3 as usize) as u64,
        SYS_GETPID => 0,
        _ => 0,
    }
}

fn sys_exit(_status: usize) -> ! {
    crate::drivers::serial::write_str("[R3] SYS_EXIT called\n");
    unsafe { ring3_exit_trampoline(); }
    unreachable!()
}

fn sys_write(fd: i32, buf: usize, count: usize) -> i64 {
    if fd != 1 && fd != 2 {
        return -1;
    }

    let mut kbuf = [0u8; 256];
    let to_copy = if count > 256 { 256 } else { count };

    match crate::mem::paging::copy_from_user(&mut kbuf[..to_copy], buf, to_copy) {
        Ok(copied) => {
            for &b in &kbuf[..copied] {
                crate::drivers::serial::write_byte(b);
            }
            copied as i64
        }
        Err(()) => -1,
    }
}
