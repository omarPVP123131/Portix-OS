use crate::syscall;

pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR:   u32 = 2;
pub const O_CREAT:  u32 = 64;
pub const O_TRUNC:  u32 = 512;
pub const O_APPEND: u32 = 1024;

#[repr(C)]
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub __pad0: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    pub __unused: [i64; 3],
}

pub struct File {
    pub fd: i32,
}

impl File {
    pub fn open(path: &str, flags: u32) -> Result<Self, i64> {
        let mut buf = [0u8; 256];
        let bytes = path.as_bytes();
        let len = bytes.len().min(255);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf[len] = 0;

        let fd = syscall::sys_open(buf.as_ptr(), flags);
        if fd < 0 {
            Err(fd)
        } else {
            Ok(Self { fd: fd as i32 })
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, i64> {
        let ret = syscall::sys_read(self.fd, buf.as_mut_ptr(), buf.len());
        if ret < 0 {
            Err(ret)
        } else {
            Ok(ret as usize)
        }
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, i64> {
        let ret = syscall::sys_write(self.fd, buf.as_ptr(), buf.len());
        if ret < 0 {
            Err(ret)
        } else {
            Ok(ret as usize)
        }
    }

    pub fn lseek(&self, offset: i64, whence: i32) -> i64 {
        syscall::sys_lseek(self.fd, offset, whence)
    }

    pub fn fstat(&self) -> Result<Stat, i64> {
        let mut st = Stat::default();
        let ret = syscall::sys_fstat(self.fd, &mut st as *mut Stat as *mut u8);
        if ret < 0 {
            Err(ret)
        } else {
            Ok(st)
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        if self.fd >= 0 {
            syscall::sys_close(self.fd);
        }
    }
}

pub fn mkdir(path: &str, mode: u32) -> i64 {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    syscall::sys_mkdir(buf.as_ptr(), mode)
}

pub fn stat(path: &str) -> Result<Stat, i64> {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;

    let mut st = Stat::default();
    let ret = syscall::sys_stat(buf.as_ptr(), &mut st as *mut Stat as *mut u8);
    if ret < 0 {
        Err(ret)
    } else {
        Ok(st)
    }
}

pub fn fstat(fd: i32, st: &mut Stat) -> Result<(), i64> {
    let ret = syscall::sys_fstat(fd, st as *mut Stat as *mut u8);
    if ret < 0 {
        Err(ret)
    } else {
        Ok(())
    }
}

pub fn chdir(path: &str) -> i64 {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    syscall::sys_chdir(buf.as_ptr())
}

pub fn getcwd(buf: &mut [u8]) -> Result<usize, i64> {
    let ret = syscall::sys_getcwd(buf.as_mut_ptr(), buf.len());
    if ret < 0 {
        Err(ret)
    } else {
        // find null terminator
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(len)
    }
}

pub fn pipe() -> Result<(i32, i32), i64> {
    let mut fds = [0i32; 2];
    let ret = syscall::sys_pipe(fds.as_mut_ptr());
    if ret < 0 {
        Err(ret)
    } else {
        Ok((fds[0], fds[1]))
    }
}

pub fn unlink(path: &str) -> i32 {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    syscall::sys_unlink(buf.as_ptr()) as i32
}

pub fn dup(oldfd: i32) -> Result<i32, i64> {
    let ret = syscall::sys_dup(oldfd);
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as i32)
    }
}

pub const F_DUPFD: i32 = 0;
pub const F_GETFD: i32 = 1;
pub const F_SETFD: i32 = 2;
pub const F_GETFL: i32 = 3;
pub const F_SETFL: i32 = 4;
pub const FD_CLOEXEC: u32 = 1;

pub fn fcntl(fd: i32, cmd: i32, arg: usize) -> Result<i64, i64> {
    let ret = syscall::sys_fcntl(fd, cmd, arg);
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret)
    }
}

pub fn ftruncate(fd: i32, length: i64) -> Result<i64, i64> {
    let ret = syscall::sys_ftruncate(fd, length);
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret)
    }
}

pub fn truncate(path: &str, length: i64) -> Result<i64, i64> {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    let ret = syscall::sys_truncate(buf.as_ptr(), length);
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret)
    }
}

pub fn rmdir(path: &str) -> Result<i32, i32> {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    let ret = syscall::sys_rmdir(buf.as_ptr()) as i32;
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret)
    }
}

pub fn isatty(fd: i32) -> bool {
    let mut st = Stat::default();
    if fstat(fd, &mut st).is_ok() {
        (st.st_mode & 0o170000) == 0o020000 // S_IFCHR
    } else {
        false
    }
}

pub const F_OK: i32 = 0;
pub const X_OK: i32 = 1;
pub const W_OK: i32 = 2;
pub const R_OK: i32 = 4;

pub const O_CLOEXEC: i32 = 0x80000;
pub const O_NONBLOCK: i32 = 0x0800;

pub fn access(path: &str, mode: i32) -> Result<(), i64> {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    let ret = syscall::sys_access(buf.as_ptr(), mode);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn rename(oldpath: &str, newpath: &str) -> Result<(), i64> {
    let mut buf_old = [0u8; 256];
    let mut buf_new = [0u8; 256];
    let b_old = oldpath.as_bytes();
    let b_new = newpath.as_bytes();
    let l_old = b_old.len().min(255);
    let l_new = b_new.len().min(255);
    buf_old[..l_old].copy_from_slice(&b_old[..l_old]);
    buf_old[l_old] = 0;
    buf_new[..l_new].copy_from_slice(&b_new[..l_new]);
    buf_new[l_new] = 0;
    let ret = syscall::sys_rename(buf_old.as_ptr(), buf_new.as_ptr());
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn pipe2(flags: i32) -> Result<(i32, i32), i64> {
    let mut fds = [0i32; 2];
    let ret = syscall::sys_pipe2(fds.as_mut_ptr(), flags);
    if ret == 0 {
        Ok((fds[0], fds[1]))
    } else {
        Err(ret)
    }
}

pub fn umask(mask: u32) -> u32 {
    syscall::sys_umask(mask)
}

pub fn statvfs(path: &str, st: &mut syscall::Statvfs) -> Result<(), i64> {
    let mut buf = [0u8; 256];
    let b = path.as_bytes();
    let l = b.len().min(255);
    buf[..l].copy_from_slice(&b[..l]);
    buf[l] = 0;
    let ret = syscall::sys_statvfs(buf.as_ptr(), st as *mut syscall::Statvfs);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn symlink(target: &str, linkpath: &str) -> Result<(), i64> {
    let mut tbuf = [0u8; 256];
    let mut lbuf = [0u8; 256];
    let tb = target.as_bytes();
    let lb = linkpath.as_bytes();
    let tl = tb.len().min(255);
    let ll = lb.len().min(255);
    tbuf[..tl].copy_from_slice(&tb[..tl]);
    tbuf[tl] = 0;
    lbuf[..ll].copy_from_slice(&lb[..ll]);
    lbuf[ll] = 0;
    let ret = syscall::sys_symlink(tbuf.as_ptr(), lbuf.as_ptr());
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn readlink(path: &str, buf: &mut [u8]) -> Result<usize, i64> {
    let mut pbuf = [0u8; 256];
    let pb = path.as_bytes();
    let pl = pb.len().min(255);
    pbuf[..pl].copy_from_slice(&pb[..pl]);
    pbuf[pl] = 0;
    let ret = syscall::sys_readlink(pbuf.as_ptr(), buf.as_mut_ptr(), buf.len());
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret)
    }
}

pub fn lstat(path: &str, st: &mut Stat) -> Result<(), i64> {
    let mut pbuf = [0u8; 256];
    let pb = path.as_bytes();
    let pl = pb.len().min(255);
    pbuf[..pl].copy_from_slice(&pb[..pl]);
    pbuf[pl] = 0;
    let ret = syscall::sys_lstat(pbuf.as_ptr(), st as *mut Stat);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fsync(fd: i32) -> Result<(), i64> {
    let ret = syscall::sys_fsync(fd);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fdatasync(fd: i32) -> Result<(), i64> {
    let ret = syscall::sys_fdatasync(fd);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn chmod(path: &str, mode: u32) -> Result<(), i64> {
    let mut pbuf = [0u8; 256];
    let pb = path.as_bytes();
    let pl = pb.len().min(255);
    pbuf[..pl].copy_from_slice(&pb[..pl]);
    pbuf[pl] = 0;
    let ret = syscall::sys_chmod(pbuf.as_ptr(), mode);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fchmod(fd: i32, mode: u32) -> Result<(), i64> {
    let ret = syscall::sys_fchmod(fd, mode);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn chown(path: &str, owner: u32, group: u32) -> Result<(), i64> {
    let mut pbuf = [0u8; 256];
    let pb = path.as_bytes();
    let pl = pb.len().min(255);
    pbuf[..pl].copy_from_slice(&pb[..pl]);
    pbuf[pl] = 0;
    let ret = syscall::sys_chown(pbuf.as_ptr(), owner, group);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fchown(fd: i32, owner: u32, group: u32) -> Result<(), i64> {
    let ret = syscall::sys_fchown(fd, owner, group);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn link(oldpath: &str, newpath: &str) -> Result<(), i64> {
    let mut obuf = [0u8; 256];
    let ob = oldpath.as_bytes();
    let ol = ob.len().min(255);
    obuf[..ol].copy_from_slice(&ob[..ol]);
    obuf[ol] = 0;

    let mut nbuf = [0u8; 256];
    let nb = newpath.as_bytes();
    let nl = nb.len().min(255);
    nbuf[..nl].copy_from_slice(&nb[..nl]);
    nbuf[nl] = 0;

    let ret = syscall::sys_link(obuf.as_ptr(), nbuf.as_ptr());
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn readv(fd: i32, iov: &mut [syscall::IoVec]) -> Result<usize, i64> {
    let ret = syscall::sys_readv(fd, iov.as_mut_ptr(), iov.len() as i32);
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret)
    }
}

pub fn writev(fd: i32, iov: &[syscall::IoVec]) -> Result<usize, i64> {
    let ret = syscall::sys_writev(fd, iov.as_ptr(), iov.len() as i32);
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret)
    }
}

pub fn pread(fd: i32, buf: &mut [u8], offset: i64) -> Result<usize, i64> {
    let ret = syscall::sys_pread64(fd, buf.as_mut_ptr(), buf.len(), offset);
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret)
    }
}

pub fn pwrite(fd: i32, buf: &[u8], offset: i64) -> Result<usize, i64> {
    let ret = syscall::sys_pwrite64(fd, buf.as_ptr(), buf.len(), offset);
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret)
    }
}

pub fn socketpair(domain: i32, sock_type: i32, protocol: i32) -> Result<(i32, i32), i64> {
    let mut sv = [0i32; 2];
    let ret = syscall::sys_socketpair(domain, sock_type, protocol, sv.as_mut_ptr());
    if ret == 0 {
        Ok((sv[0], sv[1]))
    } else {
        Err(ret as i64)
    }
}

pub fn sync() -> Result<(), i64> {
    let ret = syscall::sys_sync();
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fstatvfs(fd: i32) -> Result<syscall::Statvfs, i64> {
    let mut st = syscall::Statvfs::default();
    let ret = syscall::sys_fstatvfs(fd, &mut st);
    if ret == 0 {
        Ok(st)
    } else {
        Err(ret)
    }
}

pub fn eventfd(initval: u32, flags: i32) -> Result<i32, i64> {
    let ret = syscall::sys_eventfd2(initval, flags);
    if ret >= 0 {
        Ok(ret)
    } else {
        Err(ret as i64)
    }
}

pub fn dup3(oldfd: i32, newfd: i32, flags: i32) -> Result<i32, i64> {
    let ret = syscall::sys_dup3(oldfd, newfd, flags);
    if ret >= 0 {
        Ok(ret as i32)
    } else {
        Err(ret)
    }
}

pub fn epoll_create1(flags: i32) -> Result<i32, i64> {
    let ret = syscall::sys_epoll_create1(flags);
    if ret >= 0 {
        Ok(ret as i32)
    } else {
        Err(ret)
    }
}

pub fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: Option<&syscall::EpollEvent>) -> Result<(), i64> {
    let ev_ptr = match event {
        Some(ev) => ev as *const syscall::EpollEvent,
        None => core::ptr::null(),
    };
    let ret = syscall::sys_epoll_ctl(epfd, op, fd, ev_ptr);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn epoll_wait(epfd: i32, events: &mut [syscall::EpollEvent], timeout: i32) -> Result<usize, i64> {
    let ret = syscall::sys_epoll_wait(epfd, events.as_mut_ptr(), events.len() as i32, timeout);
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret)
    }
}

pub fn openat(dirfd: i32, path: &str, flags: u32) -> Result<i32, i64> {
    let mut buf = [0u8; 256];
    if path.len() >= buf.len() { return Err(-36); }
    buf[..path.len()].copy_from_slice(path.as_bytes());
    buf[path.len()] = 0;
    let ret = syscall::sys_openat(dirfd, buf.as_ptr(), flags);
    if ret >= 0 {
        Ok(ret as i32)
    } else {
        Err(ret)
    }
}

pub fn mkdirat(dirfd: i32, path: &str) -> Result<(), i64> {
    let mut buf = [0u8; 256];
    if path.len() >= buf.len() { return Err(-36); }
    buf[..path.len()].copy_from_slice(path.as_bytes());
    buf[path.len()] = 0;
    let ret = syscall::sys_mkdirat(dirfd, buf.as_ptr());
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn unlinkat(dirfd: i32, path: &str, flags: i32) -> Result<(), i64> {
    let mut buf = [0u8; 256];
    if path.len() >= buf.len() { return Err(-36); }
    buf[..path.len()].copy_from_slice(path.as_bytes());
    buf[path.len()] = 0;
    let ret = syscall::sys_unlinkat(dirfd, buf.as_ptr(), flags);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn readlinkat(dirfd: i32, path: &str, buf: &mut [u8]) -> Result<usize, i64> {
    let mut pbuf = [0u8; 256];
    if path.len() >= pbuf.len() { return Err(-36); }
    pbuf[..path.len()].copy_from_slice(path.as_bytes());
    pbuf[path.len()] = 0;
    let ret = syscall::sys_readlinkat(dirfd, pbuf.as_ptr(), buf.as_mut_ptr(), buf.len());
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(ret)
    }
}

pub fn fstatat(dirfd: i32, path: &str, stat_buf: &mut Stat, flags: i32) -> Result<(), i64> {
    let mut pbuf = [0u8; 256];
    if path.len() >= pbuf.len() { return Err(-36); }
    pbuf[..path.len()].copy_from_slice(path.as_bytes());
    pbuf[path.len()] = 0;
    let ret = syscall::sys_fstatat(dirfd, pbuf.as_ptr(), stat_buf, flags);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret as i64)
    }
}

pub fn ioctl(fd: i32, request: u64, arg: usize) -> Result<i64, i64> {
    let ret = syscall::sys_ioctl(fd, request, arg);
    if ret >= 0 {
        Ok(ret)
    } else {
        Err(ret)
    }
}

pub fn memfd_create(name: &str, flags: u32) -> Result<i32, i64> {
    let mut buf = [0u8; 64];
    if name.len() >= buf.len() { return Err(-36); }
    buf[..name.len()].copy_from_slice(name.as_bytes());
    buf[name.len()] = 0;
    let ret = syscall::sys_memfd_create(buf.as_ptr(), flags);
    if ret >= 0 {
        Ok(ret as i32)
    } else {
        Err(ret)
    }
}

pub fn mknod(path: &str, mode: u32, dev: u64) -> Result<(), i64> {
    let mut pbuf = [0u8; 256];
    if path.len() >= pbuf.len() { return Err(-36); }
    pbuf[..path.len()].copy_from_slice(path.as_bytes());
    pbuf[path.len()] = 0;
    let ret = syscall::sys_mknod(pbuf.as_ptr(), mode, dev);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fchdir(dirfd: i32) -> Result<(), i64> {
    let ret = syscall::sys_fchdir(dirfd);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn symlinkat(target: &str, newdirfd: i32, linkpath: &str) -> Result<(), i64> {
    let mut tbuf = [0u8; 256];
    let mut lbuf = [0u8; 256];
    if target.len() >= tbuf.len() || linkpath.len() >= lbuf.len() { return Err(-36); }
    tbuf[..target.len()].copy_from_slice(target.as_bytes());
    tbuf[target.len()] = 0;
    lbuf[..linkpath.len()].copy_from_slice(linkpath.as_bytes());
    lbuf[linkpath.len()] = 0;
    let ret = syscall::sys_symlinkat(tbuf.as_ptr(), newdirfd, lbuf.as_ptr());
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn linkat(olddirfd: i32, oldpath: &str, newdirfd: i32, newpath: &str, flags: i32) -> Result<(), i64> {
    let mut obuf = [0u8; 256];
    let mut nbuf = [0u8; 256];
    if oldpath.len() >= obuf.len() || newpath.len() >= nbuf.len() { return Err(-36); }
    obuf[..oldpath.len()].copy_from_slice(oldpath.as_bytes());
    obuf[oldpath.len()] = 0;
    nbuf[..newpath.len()].copy_from_slice(newpath.as_bytes());
    nbuf[newpath.len()] = 0;
    let ret = syscall::sys_linkat(olddirfd, obuf.as_ptr(), newdirfd, nbuf.as_ptr(), flags);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn renameat(olddirfd: i32, oldpath: &str, newdirfd: i32, newpath: &str) -> Result<(), i64> {
    let mut obuf = [0u8; 256];
    let mut nbuf = [0u8; 256];
    if oldpath.len() >= obuf.len() || newpath.len() >= nbuf.len() { return Err(-36); }
    obuf[..oldpath.len()].copy_from_slice(oldpath.as_bytes());
    obuf[oldpath.len()] = 0;
    nbuf[..newpath.len()].copy_from_slice(newpath.as_bytes());
    nbuf[newpath.len()] = 0;
    let ret = syscall::sys_renameat(olddirfd, obuf.as_ptr(), newdirfd, nbuf.as_ptr());
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fchmodat(dirfd: i32, path: &str, mode: u32, flags: i32) -> Result<(), i64> {
    let mut pbuf = [0u8; 256];
    if path.len() >= pbuf.len() { return Err(-36); }
    pbuf[..path.len()].copy_from_slice(path.as_bytes());
    pbuf[path.len()] = 0;
    let ret = syscall::sys_fchmodat(dirfd, pbuf.as_ptr(), mode, flags);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn fchownat(dirfd: i32, path: &str, owner: u32, group: u32, flags: i32) -> Result<(), i64> {
    let mut pbuf = [0u8; 256];
    if path.len() >= pbuf.len() { return Err(-36); }
    pbuf[..path.len()].copy_from_slice(path.as_bytes());
    pbuf[path.len()] = 0;
    let ret = syscall::sys_fchownat(dirfd, pbuf.as_ptr(), owner, group, flags);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn faccessat(dirfd: i32, path: &str, mode: i32, flags: i32) -> Result<(), i64> {
    let mut pbuf = [0u8; 256];
    if path.len() >= pbuf.len() { return Err(-36); }
    pbuf[..path.len()].copy_from_slice(path.as_bytes());
    pbuf[path.len()] = 0;
    let ret = syscall::sys_faccessat(dirfd, pbuf.as_ptr(), mode, flags);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn utimensat(dirfd: i32, path: &str, times: Option<&[syscall::TimeVal; 2]>, flags: i32) -> Result<(), i64> {
    let mut pbuf = [0u8; 256];
    if path.len() >= pbuf.len() { return Err(-36); }
    pbuf[..path.len()].copy_from_slice(path.as_bytes());
    pbuf[path.len()] = 0;
    let tptr = match times {
        Some(t) => t.as_ptr(),
        None => core::ptr::null(),
    };
    let ret = syscall::sys_utimensat(dirfd, pbuf.as_ptr(), tptr, flags);
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}

pub fn read_to_string(path: &str) -> Result<alloc::string::String, i64> {
    let file = File::open(path, O_RDONLY)?;
    let mut buf = [0u8; 1024];
    let mut res = alloc::string::String::new();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { break; }
        if let Ok(s) = core::str::from_utf8(&buf[..n]) {
            res.push_str(s);
        }
    }
    Ok(res)
}

pub fn read(path: &str) -> Result<alloc::vec::Vec<u8>, i64> {
    let file = File::open(path, O_RDONLY)?;
    // [T-002] trozos de 64 KiB: un indice APKINDEX de 9 MiB en trozos de 1 KiB
    // eran 9000 syscalls con resolucion de ruta cada una.
    let mut buf = alloc::vec![0u8; 64 * 1024];
    let mut res = alloc::vec::Vec::new();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { break; }
        res.extend_from_slice(&buf[..n]);
    }
    Ok(res)
}

pub fn write(path: &str, data: &[u8]) -> Result<(), i64> {
    let file = File::open(path, O_WRONLY | O_CREAT | O_TRUNC)?;
    // [T-002] escrituras cortas POSIX: reintentar hasta completar.
    let mut hecho = 0usize;
    while hecho < data.len() {
        let n = file.write(&data[hecho..])?;
        if n == 0 { return Err(-5); }
        hecho += n;
    }
    Ok(())
}

// ── POSIX Shared Memory (shm_open, shm_unlink) ───────────────────────────────

pub fn shm_open(name: &str, oflag: u32, _mode: u32) -> Result<i32, i64> {
    let clean = name.trim_start_matches('/');
    let mut path = alloc::string::String::from("/dev/shm/");
    path.push_str(clean);
    path.push('\0');
    let fd = syscall::sys_open(path.as_ptr(), oflag);
    if fd >= 0 {
        Ok(fd as i32)
    } else {
        Err(fd)
    }
}

pub fn shm_unlink(name: &str) -> i32 {
    let clean = name.trim_start_matches('/');
    let mut path = alloc::string::String::from("/dev/shm/");
    path.push_str(clean);
    unlink(&path)
}

// ── POSIX Record Locking (fcntl F_SETLK, F_SETLKW, F_GETLK) ───────────────────

pub const F_RDLCK: i16 = 0;
pub const F_WRLCK: i16 = 1;
pub const F_UNLCK: i16 = 2;

pub const F_GETLK: i32  = 5;
pub const F_SETLK: i32  = 6;
pub const F_SETLKW: i32 = 7;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Flock {
    pub l_type: i16,
    pub l_whence: i16,
    pub l_start: i64,
    pub l_len: i64,
    pub l_pid: i32,
}

// ── POSIX Named Semaphores (sem_open, sem_post, sem_wait) ───────────────────

pub struct Sem {
    pub fd: i32,
    pub value: u32,
}

pub fn sem_open(name: &str, oflag: u32, _mode: u32, value: u32) -> Result<Sem, i64> {
    let clean = name.trim_start_matches('/');
    let mut path = alloc::string::String::from("/dev/shm/sem.");
    path.push_str(clean);
    path.push('\0');
    let fd = syscall::sys_open(path.as_ptr(), oflag | O_CREAT);
    if fd >= 0 {
        let s = Sem { fd: fd as i32, value };
        let bytes = value.to_ne_bytes();
        let _ = syscall::sys_write(fd as i32, bytes.as_ptr(), 4);
        Ok(s)
    } else {
        Err(fd)
    }
}

pub fn sem_post(sem: &mut Sem) -> i32 {
    sem.value = sem.value.saturating_add(1);
    let bytes = sem.value.to_ne_bytes();
    let _ = syscall::sys_lseek(sem.fd, 0, 0);
    let _ = syscall::sys_write(sem.fd, bytes.as_ptr(), 4);
    0
}

pub fn sem_wait(sem: &mut Sem) -> i32 {
    if sem.value > 0 {
        sem.value -= 1;
        let bytes = sem.value.to_ne_bytes();
        let _ = syscall::sys_lseek(sem.fd, 0, 0);
        let _ = syscall::sys_write(sem.fd, bytes.as_ptr(), 4);
        0
    } else {
        0
    }
}

pub fn sem_close(sem: Sem) -> i32 {
    syscall::sys_close(sem.fd) as i32
}

pub fn sem_unlink(name: &str) -> i32 {
    let clean = name.trim_start_matches('/');
    let mut path = alloc::string::String::from("/dev/shm/sem.");
    path.push_str(clean);
    unlink(&path)
}



