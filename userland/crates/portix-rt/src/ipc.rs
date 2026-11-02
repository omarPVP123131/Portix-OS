use crate::syscall::{self, KernelMessage};
use crate::process::yield_now;

pub const IPC_NOWAIT: u32 = 1;

pub fn send_message(dst_pid: u64, msg_type: u64, data: &[u8]) -> Result<(), i64> {
    let ret = syscall::sys_send(dst_pid, msg_type, data.as_ptr(), data.len() as u64);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn recv_message() -> Result<KernelMessage, i64> {
    let mut msg = KernelMessage {
        src_pid: 0,
        dst_pid: 0,
        msg_type: 0,
        data: [0u8; 40],
    };
    let ptr = &mut msg as *mut KernelMessage as *mut u8;
    loop {
        let ret = syscall::sys_recv(ptr, core::mem::size_of::<KernelMessage>(), 0);
        if ret > 0 {
            return Ok(msg);
        } else if ret < 0 {
            return Err(ret);
        }
        yield_now();
    }
}

pub fn try_recv_message() -> Result<Option<KernelMessage>, i64> {
    let mut msg = KernelMessage {
        src_pid: 0,
        dst_pid: 0,
        msg_type: 0,
        data: [0u8; 40],
    };
    let ptr = &mut msg as *mut KernelMessage as *mut u8;
    let ret = syscall::sys_recv(ptr, core::mem::size_of::<KernelMessage>(), IPC_NOWAIT);
    if ret > 0 {
        Ok(Some(msg))
    } else if ret == 0 {
        Ok(None)
    } else {
        Err(ret)
    }
}
