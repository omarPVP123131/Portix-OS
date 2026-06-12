use crate::syscall;

pub struct File {
    fd: i32,
}

impl File {
    pub fn open(path: &str) -> Result<File, i64> {
        let fd = syscall::sys_open(path.as_ptr(), 0);
        if fd < 0 {
            Err(fd)
        } else {
            Ok(File { fd: fd as i32 })
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, i64> {
        let n = syscall::sys_read(self.fd, buf.as_mut_ptr(), buf.len());
        if n < 0 {
            Err(n)
        } else {
            Ok(n as usize)
        }
    }

    pub fn close(&self) {
        syscall::sys_close(self.fd);
    }
}
