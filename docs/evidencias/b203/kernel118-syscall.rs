extern crate alloc;
use crate::drivers::serial;
use crate::drivers::storage::vfs;
use crate::mem::paging::{self, PAGE_SIZE};
use crate::process::{self, FdEntry, FdType};
use crate::drivers::input::keyboard::{KeyboardState, Key};
use crate::arch::Spinlock;
use crate::arch::isr_handlers::{pop_ring3_scancode, set_stdin_blocked, clear_stdin_blocked};

fn alloc_page() -> Option<usize> {
    // [Fase B.4] brk/mmap: asignador de marcos físicos (alias alto), no montículo.
    paging::asignar_marco_usuario()
}

fn verify_user_buffer(buf: usize, len: usize) -> bool {
    // [CICLO 50 / T1c] `read(fd, NULL, 0)` / `write(fd, NULL, 0)` son válidos en
    // Linux (devuelven 0 sin tocar el puntero): tcc/musl los emiten. Sólo se
    // valida el puntero cuando hay bytes que transferir.
    if len == 0 { return true; }
    if buf < 0x1000 { return false; }
    if buf >= crate::process::USER_STACK_TOP { return false; }
    match buf.checked_add(len) {
        Some(e) => e <= crate::process::USER_STACK_TOP,
        None => false,
    }
}

fn verify_user_string(ptr: usize, max_len: usize) -> bool {
    if ptr < 0x1000 { return false; }
    if ptr >= crate::process::USER_STACK_TOP { return false; }
    if max_len == 0 { return false; }
    // [CICLO 49 / T1] Sólo se exige que el PRIMER byte sea de usuario: la
    // longitud real la decide el NUL (copy_cstr_from_user valida cada página).
    true
}


pub mod numbers;
pub mod types;
pub mod futex;

pub use numbers::*;
pub use types::*;


/// [ÉPICA 58] Núcleo del despachador nativo de llamadas al sistema.
/// La entrega de señales pendientes se realiza en el envoltorio público
/// `syscall_dispatch` una vez conocido el resultado del syscall.
/// [T-025 E3d] Tope de seguridad de las esperas bloqueantes (tuberías, sockets,
/// eventfd, inotify) en ticks de 10 ms. Era 10 s: con dos CPUs, un `execve` de
/// cc1 (38 MB cargados bajo el cerrojo grande) retrasaba al escritor más de
/// 10 s y el lector (`grep` de un `gcc -v … | grep | head`) devolvía un EOF
/// falso → `head` salía → cc1 moría por SIGPIPE. POSIX no tiene tope: se deja
/// uno de 10 min sólo como red contra un cuelgue total, no como semántica.
const TOPE_BLOQUEO_TICKS: u64 = 60_000;

fn syscall_dispatch_core(
    num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64,
    current_rsp: u64,
) -> SyscallResult {
    match num {
        SYS_EXIT   => sys_exit(a1 as usize),
        SYS_WRITE  => SyscallResult(sys_write(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_GETPID => SyscallResult(sys_getpid(), 0),
        SYS_YIELD  => SyscallResult(0, sys_yield_switch(current_rsp)),
        SYS_SLEEP  => SyscallResult(0, sys_sleep_switch(a1, current_rsp)),
        SYS_READ   => {
            let (res, sw) = sys_read(a1 as i32, a2 as usize, a3 as usize, current_rsp);
            SyscallResult(res as u64, sw)
        }
        SYS_OPEN   => SyscallResult(sys_open(a1 as usize, a2 as u32) as u64, 0),
        SYS_CLOSE  => SyscallResult(sys_close(a1 as i32) as u64, 0),
        SYS_BRK    => SyscallResult(sys_brk(a1 as usize) as u64, 0),
        SYS_MMAP   => { let a6 = saved_r9_from_rsp(current_rsp); SyscallResult(sys_mmap(a1 as usize, a2 as usize, a3 as u32, a4 as u32, a5 as i32, a6 as i64) as u64, 0) },
        SYS_GETDIRENTS => SyscallResult(sys_getdents(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_EXECVE => sys_execve(a1 as usize, a2 as usize, a3 as usize, current_rsp),
        SYS_DUP2   => SyscallResult(sys_dup2(a1 as i32, a2 as i32) as u64, 0),
        SYS_UPTIME => SyscallResult(sys_uptime(), 0),
        SYS_SEND   => SyscallResult(sys_send(a1, a2, a3, a4) as u64, 0),
        SYS_RECV   => {
            let (res, sw) = sys_recv(a1 as usize, a2 as usize, a3 as u32, current_rsp);
            SyscallResult(res as u64, sw)
        }
        SYS_REG_IRQ => SyscallResult(sys_reg_irq(a1, a2) as u64, 0),
        SYS_BLOCK_READ => SyscallResult(sys_block_read(a1 as i32, a2, a3, a4) as u64, 0),
        SYS_IOPORT     => SyscallResult(sys_ioport(a1 as u16, a2 as u8) as u64, 0),
        SYS_IOREAD     => SyscallResult(sys_ioread(a1 as u16) as u64, 0),
        SYS_IOWRITE    => SyscallResult(sys_iowrite(a1 as u16, a2 as u8) as u64, 0),
        SYS_MMAP_DEVICE => SyscallResult(sys_mmap_device(a1 as usize, a2 as usize) as u64, 0),
        SYS_FORK    => SyscallResult(sys_fork(current_rsp) as u64, 0),
        SYS_WAITPID => {
            let (res, sw) = sys_waitpid(a1 as i64, a2 as usize, a3 as u32, current_rsp);
            // [FRENTE C-4] Reinicio transparente: RAX vuelve a ser el numero de
            // syscall y RIP ya apunta de nuevo a `int 0x80`/`syscall`.
            if res == SYSCALL_RESTART { SyscallResult(SYS_WAITPID, sw) } else { SyscallResult(res as u64, sw) }
        }
        SYS_LSEEK   => SyscallResult(sys_lseek(a1 as i32, a2 as i64, a3 as i32) as u64, 0),
        SYS_STAT    => SyscallResult(sys_stat(a1 as usize, a2 as usize) as u64, 0),
        SYS_FSTAT   => SyscallResult(sys_fstat(a1 as i32, a2 as usize) as u64, 0),
        SYS_KILL    => SyscallResult(sys_kill(a1, a2 as i32) as u64, 0),
        SYS_GETCWD  => SyscallResult(sys_getcwd(a1 as usize, a2 as usize) as u64, 0),
        SYS_CHDIR   => SyscallResult(sys_chdir(a1 as usize) as u64, 0),
        SYS_UNLINK  => SyscallResult(sys_unlink(a1 as usize) as u64, 0),
        SYS_PIPE    => SyscallResult(sys_pipe(a1 as usize) as u64, 0),
        SYS_TIME    => SyscallResult(sys_time(a1 as usize) as u64, 0),
        SYS_MKDIR   => SyscallResult(sys_mkdir(a1 as usize, a2 as u32) as u64, 0),
        SYS_SOCKET  => SyscallResult(sys_socket(a1 as i32, a2 as i32, a3 as i32) as u64, 0),
        SYS_BIND    => SyscallResult(sys_bind(a1 as i32, a2 as usize, a3 as u32) as u64, 0),
        // [IPv6 REAL] addrlen/addrlen_ptr reales en r9 (antes 16 fijo / 0): sockaddr_in6 ocupa 28 bytes
        SYS_SENDTO  => { let a6 = saved_r9_from_rsp(current_rsp); SyscallResult(sys_sendto(a1 as i32, a2 as usize, a3 as usize, a4 as i32, a5 as usize, if a6 == 0 { 16 } else { a6 as u32 }) as u64, 0) },
        SYS_RECVFROM => { let a6 = saved_r9_from_rsp(current_rsp); SyscallResult(sys_recvfrom(a1 as i32, a2 as usize, a3 as usize, a4 as i32, a5 as usize, a6 as usize) as u64, 0) },
        SYS_INIT_MODULE   => SyscallResult(sys_init_module(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_DELETE_MODULE => SyscallResult(sys_delete_module(a1 as usize, a2 as u32) as u64, 0),
        SYS_QUERY_MODULE  => SyscallResult(sys_query_module(a1 as usize, a2 as usize) as u64, 0),
        SYS_GET_MOUSE     => SyscallResult(sys_get_mouse(a1 as usize) as u64, 0),
        SYS_POLL_KEY      => SyscallResult(sys_poll_key() as u64, 0),
        SYS_GETUID        => SyscallResult(crate::process::current_uid() as u64, 0),
        SYS_SETUID        => SyscallResult(if crate::process::set_current_uid(a1 as u32) { 0 } else { u64::MAX }, 0),
        SYS_GETGID        => SyscallResult(crate::process::current_gid() as u64, 0),
        SYS_SETGID        => SyscallResult(if crate::process::set_current_gid(a1 as u32) { 0 } else { u64::MAX }, 0),
        SYS_MUNMAP        => SyscallResult(sys_munmap(a1 as usize, a2 as usize) as u64, 0),
        SYS_DUP           => SyscallResult(sys_dup(a1 as i32) as u64, 0),
        SYS_FCNTL         => SyscallResult(sys_fcntl(a1 as i32, a2 as i32, a3 as usize) as u64, 0),
        SYS_GETPPID       => SyscallResult(sys_getppid(), 0),
        SYS_UNAME         => SyscallResult(sys_uname(a1 as usize) as u64, 0),
        SYS_FTRUNCATE     => SyscallResult(sys_ftruncate(a1 as i32, a2 as i64) as u64, 0),
        SYS_TRUNCATE      => SyscallResult(sys_truncate(a1 as usize, a2 as i64) as u64, 0),
        SYS_GETEUID       => SyscallResult(crate::process::current_euid() as u64, 0),
        SYS_GETEGID       => SyscallResult(crate::process::current_egid() as u64, 0),
        SYS_RMDIR         => SyscallResult(sys_rmdir(a1 as usize) as u64, 0),
        SYS_ACCESS        => SyscallResult(sys_access(a1 as usize, a2 as i32) as u64, 0),
        SYS_RENAME        => SyscallResult(sys_rename(a1 as usize, a2 as usize) as u64, 0),
        SYS_SETEUID       => SyscallResult(if crate::process::set_current_euid(a1 as u32) { 0 } else { u64::MAX }, 0),
        SYS_SETEGID       => SyscallResult(if crate::process::set_current_egid(a1 as u32) { 0 } else { u64::MAX }, 0),
        SYS_PIPE2         => SyscallResult(sys_pipe2(a1 as usize, a2 as i32) as u64, 0),
        SYS_NANOSLEEP     => {
            let (res, sw) = sys_nanosleep(a1 as usize, a2 as usize, current_rsp);
            SyscallResult(res as u64, sw)
        }
        SYS_GETPGRP       => SyscallResult(crate::process::current_pgid(), 0),
        SYS_SETPGID       => SyscallResult(match crate::process::set_pgid(a1, a2) { Ok(()) => 0, Err(e) => e as u64 }, 0),
        SYS_GETPGID       => SyscallResult(match crate::process::get_pgid(a1) { Some(pgid) => pgid, None => (-3i64) as u64 }, 0),
        SYS_SETSID        => SyscallResult(match crate::process::setsid() { Ok(sid) => sid, Err(e) => e as u64 }, 0),
        SYS_GETSID        => SyscallResult(match crate::process::getsid(a1) { Some(sid) => sid, None => (-3i64) as u64 }, 0),
        SYS_UMASK         => SyscallResult(crate::process::set_current_umask(a1 as u32) as u64, 0),
        SYS_POLL          => SyscallResult(sys_poll(a1 as usize, a2 as usize, a3 as i32) as u64, 0),
        SYS_CLOCK_GETTIME => SyscallResult(sys_clock_gettime(a1 as i32, a2 as usize) as u64, 0),
        SYS_CLOCK_GETRES  => SyscallResult(sys_clock_getres(a1 as i32, a2 as usize) as u64, 0),
        SYS_STATVFS       => SyscallResult(sys_statvfs(a1 as usize, a2 as usize) as u64, 0),
        SYS_SIGACTION   => SyscallResult(sys_sigaction(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_SIGPROCMASK => SyscallResult(sys_sigprocmask(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_SIGPENDING  => SyscallResult(sys_sigpending(a1 as usize) as u64, 0),
        SYS_GETRLIMIT   => SyscallResult(sys_getrlimit(a1 as i32, a2 as usize) as u64, 0),
        SYS_SETRLIMIT   => SyscallResult(sys_setrlimit(a1 as i32, a2 as usize) as u64, 0),
        SYS_GETPRIORITY => SyscallResult(match crate::process::getpriority(a1 as i32, a2) { Ok(p) => (p as i64) as u64, Err(e) => e as u64 }, 0),
        SYS_SETPRIORITY => SyscallResult(match crate::process::setpriority(a1 as i32, a2, a3 as i32) { Ok(()) => 0, Err(e) => e as u64 }, 0),
        SYS_READLINK    => SyscallResult(sys_readlink(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_SYMLINK     => SyscallResult(sys_symlink(a1 as usize, a2 as usize) as u64, 0),
        SYS_LSTAT       => SyscallResult(sys_lstat(a1 as usize, a2 as usize) as u64, 0),
        SYS_TIMES       => SyscallResult(sys_times(a1 as usize) as u64, 0),
        SYS_GETGROUPS   => SyscallResult(sys_getgroups(a1 as usize, a2 as usize) as u64, 0),
        SYS_SETGROUPS   => SyscallResult(sys_setgroups(a1 as usize, a2 as usize) as u64, 0),
        SYS_FSYNC       => SyscallResult(sys_fsync(a1 as i32) as u64, 0),
        SYS_FDATASYNC   => SyscallResult(sys_fdatasync(a1 as i32) as u64, 0),
        SYS_CHMOD       => SyscallResult(sys_chmod(a1 as usize, a2 as u32) as u64, 0),
        SYS_FCHMOD      => SyscallResult(sys_fchmod(a1 as i32, a2 as u32) as u64, 0),
        SYS_CHOWN       => SyscallResult(sys_chown(a1 as usize, a2 as u32, a3 as u32) as u64, 0),
        SYS_FCHOWN      => SyscallResult(sys_fchown(a1 as i32, a2 as u32, a3 as u32) as u64, 0),
        SYS_PAUSE       => {
            let (res, sw) = sys_pause(current_rsp);
            if res == SYSCALL_RESTART { SyscallResult(SYS_PAUSE, sw) } else { SyscallResult(res as u64, sw) }
        }
        SYS_ALARM       => SyscallResult(process::alarm(a1 as u32) as u64, 0),
        SYS_SCHED_YIELD => SyscallResult(0, sys_sched_yield(current_rsp)),
        SYS_GETRESUID   => SyscallResult(sys_getresuid(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_SETRESUID   => SyscallResult(sys_setresuid(a1 as u32, a2 as u32, a3 as u32) as u64, 0),
        SYS_GETRESGID   => SyscallResult(sys_getresgid(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_SETRESGID   => SyscallResult(sys_setresgid(a1 as u32, a2 as u32, a3 as u32) as u64, 0),
        SYS_LINK        => SyscallResult(sys_link(a1 as usize, a2 as usize) as u64, 0),
        SYS_READV       => {
            let (n, new_rsp) = sys_readv(a1 as i32, a2 as usize, a3 as i32, current_rsp);
            SyscallResult(n as u64, new_rsp)
        }
        SYS_WRITEV      => SyscallResult(sys_writev(a1 as i32, a2 as usize, a3 as i32) as u64, 0),
        SYS_PREAD64     => SyscallResult(sys_pread64(a1 as i32, a2 as usize, a3 as usize, a4 as i64) as u64, 0),
        SYS_PWRITE64    => SyscallResult(sys_pwrite64(a1 as i32, a2 as usize, a3 as usize, a4 as i64) as u64, 0),
        SYS_MPROTECT    => SyscallResult(sys_mprotect(a1 as usize, a2 as usize, a3 as i32) as u64, 0),
        SYS_SOCKETPAIR  => SyscallResult(sys_socketpair(a1 as i32, a2 as i32, a3 as i32, a4 as usize) as u64, 0),
        SYS_GETTID                 => SyscallResult(sys_gettid(), 0),
        SYS_SCHED_GETPARAM         => SyscallResult(sys_sched_getparam(a1, a2 as usize) as u64, 0),
        SYS_SCHED_SETPARAM         => SyscallResult(sys_sched_setparam(a1, a2 as usize) as u64, 0),
        SYS_SCHED_GETSCHEDULER     => SyscallResult(sys_sched_getscheduler(a1) as u64, 0),
        SYS_SCHED_SETSCHEDULER     => SyscallResult(sys_sched_setscheduler(a1, a2 as i32, a3 as usize) as u64, 0),
        SYS_PRCTL                  => SyscallResult(sys_prctl(a1 as i32, a2, a3, a4, a5) as u64, 0),
        SYS_SYNC                   => SyscallResult(sys_sync() as u64, 0),
        SYS_FSTATVFS               => SyscallResult(sys_fstatvfs(a1 as i32, a2 as usize) as u64, 0),
        SYS_EVENTFD2               => SyscallResult(sys_eventfd2(a1 as u32, a2 as i32) as u64, 0),
        SYS_DUP3                   => SyscallResult(sys_dup3(a1 as i32, a2 as i32, a3 as i32) as u64, 0),
        SYS_TGKILL                 => SyscallResult(sys_tgkill(a1 as i32, a2 as i32, a3 as i32) as u64, 0),
        SYS_SCHED_GET_PRIORITY_MAX => SyscallResult(sys_sched_get_priority_max(a1 as i32) as u64, 0),
        SYS_SCHED_GET_PRIORITY_MIN => SyscallResult(sys_sched_get_priority_min(a1 as i32) as u64, 0),
        SYS_FUTEX                  => {
            let (res, sw) = sys_futex(a1 as usize, a2 as i32, a3 as u32, a4 as usize, a5 as usize, saved_r9_from_rsp(current_rsp) as u32, current_rsp);
            if res == SYSCALL_RESTART { SyscallResult(SYS_FUTEX, sw) } else { SyscallResult(res as u64, sw) }
        }
        SYS_EPOLL_CREATE1          => SyscallResult(sys_epoll_create1(a1 as i32) as u64, 0),
        SYS_EPOLL_CTL              => SyscallResult(sys_epoll_ctl(a1 as i32, a2 as i32, a3 as i32, a4 as usize) as u64, 0),
        SYS_EPOLL_WAIT             => SyscallResult(sys_epoll_wait(a1 as i32, a2 as usize, a3 as i32, a4 as i32) as u64, 0),
        SYS_SYSINFO                => SyscallResult(sys_sysinfo(a1 as usize) as u64, 0),
        SYS_GETRANDOM              => SyscallResult(sys_getrandom(a1 as usize, a2 as usize, a3 as u32) as u64, 0),
        SYS_OPENAT                 => SyscallResult(sys_openat(a1 as i32, a2 as usize, a3 as u32) as u64, 0),
        SYS_MKDIRAT                => SyscallResult(sys_mkdirat(a1 as i32, a2 as usize) as u64, 0),
        SYS_UNLINKAT               => SyscallResult(sys_unlinkat(a1 as i32, a2 as usize, a3 as i32) as u64, 0),
        SYS_READLINKAT             => SyscallResult(sys_readlinkat(a1 as i32, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_FSTATAT                => SyscallResult(sys_fstatat(a1 as i32, a2 as usize, a3 as usize, a4 as i32) as u64, 0),
        SYS_IOCTL                  => SyscallResult(sys_ioctl(a1 as i32, a2, a3 as usize) as u64, 0),
        SYS_MEMFD_CREATE           => SyscallResult(sys_memfd_create(a1 as usize, a2 as u32) as u64, 0),
        SYS_SETSOCKOPT             => SyscallResult(sys_setsockopt(a1 as i32, a2 as i32, a3 as i32, a4 as usize, a5 as u32) as u64, 0),
        SYS_GETSOCKOPT             => SyscallResult(sys_getsockopt(a1 as i32, a2 as i32, a3 as i32, a4 as usize, a5 as usize) as u64, 0),
        SYS_SETHOSTNAME            => SyscallResult(sys_sethostname(a1 as usize, a2 as usize) as u64, 0),
        SYS_GETHOSTNAME            => SyscallResult(sys_gethostname(a1 as usize, a2 as usize) as u64, 0),
        SYS_GETTIMEOFDAY           => SyscallResult(sys_gettimeofday(a1 as usize, a2 as usize) as u64, 0),
        SYS_GETITIMER              => SyscallResult(sys_getitimer(a1 as i32, a2 as usize) as u64, 0),
        SYS_SETITIMER              => SyscallResult(sys_setitimer(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_MKNOD                  => SyscallResult(sys_mknod(a1 as usize, a2 as u32, a3) as u64, 0),
        SYS_FCHDIR                 => SyscallResult(sys_fchdir(a1 as i32) as u64, 0),
        SYS_SYMLINKAT              => SyscallResult(sys_symlinkat(a1 as usize, a2 as i32, a3 as usize) as u64, 0),
        SYS_LINKAT                 => SyscallResult(sys_linkat(a1 as i32, a2 as usize, a3 as i32, a4 as usize, a5 as i32) as u64, 0),
        SYS_RENAMEAT               => SyscallResult(sys_renameat(a1 as i32, a2 as usize, a3 as i32, a4 as usize) as u64, 0),
        SYS_FCHMODAT               => SyscallResult(sys_fchmodat(a1 as i32, a2 as usize, a3 as u32, a4 as i32) as u64, 0),
        SYS_FCHOWNAT               => SyscallResult(sys_fchownat(a1 as i32, a2 as usize, a3 as u32, a4 as u32, a5 as i32) as u64, 0),
        SYS_FACCESSAT              => SyscallResult(sys_faccessat(a1 as i32, a2 as usize, a3 as i32, a4 as i32) as u64, 0),
        SYS_UTIMENSAT              => SyscallResult(sys_utimensat(a1 as i32, a2 as usize, a3 as usize, a4 as i32) as u64, 0),
        SYS_GETRUSAGE              => SyscallResult(sys_getrusage(a1 as i32, a2 as usize) as u64, 0),
        SYS_PRLIMIT64              => SyscallResult(sys_prlimit64(a1 as i32, a2 as i32, a3 as usize, a4 as usize) as u64, 0),
        SYS_CLOCK_NANOSLEEP        => {
            let (res, sw) = sys_clock_nanosleep(a1 as i32, a2 as i32, a3 as usize, a4 as usize, current_rsp);
            SyscallResult(res as u64, sw)
        }
        SYS_TIMERFD_CREATE         => SyscallResult(sys_timerfd_create(a1 as i32, a2 as i32) as u64, 0),
        SYS_TIMERFD_SETTIME        => SyscallResult(sys_timerfd_settime(a1 as i32, a2 as i32, a3 as usize, a4 as usize) as u64, 0),
        SYS_TIMERFD_GETTIME        => SyscallResult(sys_timerfd_gettime(a1 as i32, a2 as usize) as u64, 0),
        SYS_SIGNALFD4              => SyscallResult(sys_signalfd4(a1 as i32, a2 as usize, a3 as usize, a4 as i32) as u64, 0),
        SYS_POSIX_OPENPT           => SyscallResult(sys_posix_openpt(a1 as i32) as u64, 0),
        SYS_PTSNAME_R              => SyscallResult(sys_ptsname_r(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_FADVISE64              => SyscallResult(sys_fadvise64(a1 as i32, a2 as i64, a3 as i64, a4 as i32) as u64, 0),
        SYS_FALLOCATE              => SyscallResult(sys_fallocate(a1 as i32, a2 as i32, a3 as i64, a4 as i64) as u64, 0),
        SYS_MEMBARRIER             => SyscallResult(sys_membarrier(a1 as i32, a2 as u32, a3 as i32) as u64, 0),
        SYS_COPY_FILE_RANGE        => SyscallResult(sys_copy_file_range(a1 as i32, a2 as usize, a3 as i32, a4 as usize, a5 as usize) as u64, 0),
        SYS_SPLICE                 => SyscallResult(sys_splice(a1 as i32, a2 as usize, a3 as i32, a4 as usize, a5 as usize) as u64, 0),
        SYS_TEE                    => SyscallResult(sys_tee(a1 as i32, a2 as i32, a3 as usize, a4 as u32) as u64, 0),
        SYS_SYNCFS                 => SyscallResult(sys_syncfs(a1 as i32) as u64, 0),
        SYS_STATX                  => SyscallResult(sys_statx(a1 as i32, a2 as usize, a3 as i32, a4 as u32, a5 as usize) as u64, 0),
        SYS_CLOSE_RANGE            => SyscallResult(sys_close_range(a1 as u32, a2 as u32, a3 as u32) as u64, 0),
        SYS_PIDFD_OPEN             => SyscallResult(sys_pidfd_open(a1 as i32, a2 as u32) as u64, 0),
        SYS_PIDFD_SEND_SIGNAL      => SyscallResult(sys_pidfd_send_signal(a1 as i32, a2 as i32, a3 as usize, a4 as u32) as u64, 0),
        SYS_GETCPU                 => SyscallResult(sys_getcpu(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_MREMAP                 => SyscallResult(sys_mremap(a1 as usize, a2 as usize, a3 as usize, a4 as i32, a5 as usize) as u64, 0),
        SYS_PPOLL                  => SyscallResult(sys_ppoll(a1 as usize, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_FSTATFS                => SyscallResult(sys_fstatfs(a1 as i32, a2 as usize) as u64, 0),
        SYS_READAHEAD              => SyscallResult(sys_readahead(a1 as i32, a2 as i64, a3 as usize) as u64, 0),
        SYS_EPOLL_PWAIT            => SyscallResult(sys_epoll_pwait(a1 as i32, a2 as usize, a3 as i32, a4 as i32, a5 as usize) as u64, 0),
        SYS_PREADV                 => SyscallResult(sys_preadv(a1 as i32, a2 as usize, a3 as i32, a4 as i64) as u64, 0),
        SYS_PWRITEV                => SyscallResult(sys_pwritev(a1 as i32, a2 as usize, a3 as i32, a4 as i64) as u64, 0),
        SYS_FACCESSAT2             => SyscallResult(sys_faccessat2(a1 as i32, a2 as usize, a3 as i32, a4 as i32) as u64, 0),
        SYS_MSYNC                  => SyscallResult(sys_msync(a1 as usize, a2 as usize, a3 as i32) as u64, 0),
        SYS_MINCORE                => SyscallResult(sys_mincore(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_MADVISE                => SyscallResult(sys_madvise(a1 as usize, a2 as usize, a3 as i32) as u64, 0),
        SYS_MLOCK                  => SyscallResult(sys_mlock(a1 as usize, a2 as usize) as u64, 0),
        SYS_MUNLOCK                => SyscallResult(sys_munlock(a1 as usize, a2 as usize) as u64, 0),
        SYS_MLOCKALL               => SyscallResult(sys_mlockall(a1 as i32) as u64, 0),
        SYS_MUNLOCKALL             => SyscallResult(sys_munlockall() as u64, 0),
        SYS_RENAMEAT2              => SyscallResult(sys_renameat2(a1 as i32, a2 as usize, a3 as i32, a4 as usize, a5 as u32) as u64, 0),
        SYS_MEMFD_SECRET           => SyscallResult(sys_memfd_secret(a1 as u32) as u64, 0),
        SYS_EPOLL_PWAIT2           => SyscallResult(sys_epoll_pwait2(a1 as i32, a2 as usize, a3 as i32, a4 as usize, a5 as usize) as u64, 0),
        SYS_PROCESS_MADVISE        => SyscallResult(sys_process_madvise(a1 as i32, a2 as usize, a3 as usize, a4 as i32, a5 as u32) as u64, 0),
        SYS_SCHED_YIELD_POSIX      => SyscallResult(0, sys_yield_switch(current_rsp)),
        SYS_SETTIMEOFDAY           => SyscallResult(sys_settimeofday(a1 as usize, a2 as usize) as u64, 0),
        SYS_SYNC_FILE_RANGE        => SyscallResult(sys_sync_file_range(a1 as i32, a2 as i64, a3 as i64, a4 as u32) as u64, 0),
        SYS_VMSPLICE               => SyscallResult(sys_vmsplice(a1 as i32, a2 as usize, a3 as usize, a4 as u32) as u64, 0),
        SYS_KCMP                   => SyscallResult(sys_kcmp(a1 as i64, a2 as i64, a3 as i32, a4, a5) as u64, 0),
        SYS_PERSONALITY            => SyscallResult(sys_personality(a1) as u64, 0),
        SYS_SETNS                  => SyscallResult(sys_setns(a1 as i32, a2 as i32) as u64, 0),
        SYS_UNSHARE                => SyscallResult(sys_unshare(a1 as i32) as u64, 0),
        SYS_PIVOT_ROOT             => SyscallResult(sys_pivot_root(a1 as usize, a2 as usize) as u64, 0),
        SYS_LOOKUP_DCOOKIE         => SyscallResult(sys_lookup_dcookie(a1, a2 as usize, a3 as usize) as u64, 0),
        SYS_GETRANDOM2             => SyscallResult(sys_getrandom(a1 as usize, a2 as usize, a3 as u32) as u64, 0),
        SYS_SCHED_SETAFFINITY      => SyscallResult(sys_sched_setaffinity(a1 as i64, a2 as usize, a3 as usize) as u64, 0),
        SYS_SCHED_GETAFFINITY      => SyscallResult(sys_sched_getaffinity(a1 as i64, a2 as usize, a3 as usize) as u64, 0),
        SYS_CLOCK_SETTIME          => SyscallResult(sys_clock_settime(a1 as i32, a2 as usize) as u64, 0),
        SYS_GETDENTS64             => SyscallResult(sys_getdents64(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_PIDFD_GETFD            => SyscallResult(sys_pidfd_getfd(a1 as i32, a2 as i32, a3 as u32) as u64, 0),
        SYS_OPENAT2                => SyscallResult(sys_openat2(a1 as i32, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_PROCESS_MRELEASE       => SyscallResult(sys_process_mrelease(a1 as i32, a2 as u32) as u64, 0),
        SYS_SET_MEMPOLICY          => SyscallResult(sys_set_mempolicy(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_GET_MEMPOLICY          => SyscallResult(sys_get_mempolicy(a1 as usize, a2 as usize, a3 as usize, a4 as usize, a5 as u32) as u64, 0),
        SYS_IO_PGETEVENTS          => SyscallResult(sys_io_pgetevents(a1 as usize, a2 as i64, a3 as i64, a4 as usize, a5 as usize, 0) as u64, 0),
        SYS_FSMOUNT                => SyscallResult(sys_fsmount(a1 as i32, a2 as u32, a3 as u32) as u64, 0),
        SYS_MOVE_MOUNT             => SyscallResult(sys_move_mount(a1 as i32, a2 as usize, a3 as i32, a4 as usize, a5 as u32) as u64, 0),
        SYS_CAPGET                 => SyscallResult(sys_capget(a1 as usize, a2 as usize) as u64, 0),
        SYS_CAPSET                 => SyscallResult(sys_capset(a1 as usize, a2 as usize) as u64, 0),
        SYS_SCHED_GETATTR          => SyscallResult(sys_sched_getattr(a1 as i32, a2 as usize, a3 as u32, a4 as u32) as u64, 0),
        SYS_SCHED_SETATTR          => SyscallResult(sys_sched_setattr(a1 as i32, a2 as usize, a3 as u32) as u64, 0),
        SYS_FSOPEN                 => SyscallResult(sys_fsopen(a1 as usize, a2 as u32) as u64, 0),
        SYS_FSPICK                 => SyscallResult(sys_fspick(a1 as i32, a2 as usize, a3 as u32) as u64, 0),
        SYS_FSCONFIG               => SyscallResult(sys_fsconfig(a1 as i32, a2 as u32, a3 as usize, a4 as usize, a5 as i32) as u64, 0),
        SYS_QUOTACTL               => SyscallResult(sys_quotactl(a1 as i32, a2 as usize, a3 as i32, a4 as usize) as u64, 0),
        SYS_LANDLOCK_CREATE_RULESET=> SyscallResult(sys_landlock_create_ruleset(a1 as usize, a2 as usize, a3 as u32) as u64, 0),
        SYS_LANDLOCK_ADD_RULE      => SyscallResult(sys_landlock_add_rule(a1 as i32, a2 as u32, a3 as usize, a4 as u32) as u64, 0),
        SYS_LANDLOCK_RESTRICT_SELF => SyscallResult(sys_landlock_restrict_self(a1 as i32, a2 as u32) as u64, 0),
        SYS_SECCOMP                => SyscallResult(sys_seccomp(a1 as u32, a2 as u32, a3 as usize) as u64, 0),
        SYS_STATMOUNT              => SyscallResult(sys_statmount(a1 as usize, a2 as usize, a3 as usize, a4 as u64) as u64, 0),
        SYS_LISTMOUNT              => SyscallResult(sys_listmount(a1 as usize, a2 as usize, a3 as usize, a4 as u64) as u64, 0),
        SYS_SHUTDOWN               => SyscallResult(sys_shutdown(a1 as i32, a2 as i32) as u64, 0),
        SYS_GETSOCKNAME            => SyscallResult(sys_getsockname(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_GETPEERNAME            => SyscallResult(sys_getpeername(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_SENDMSG                => SyscallResult(sys_sendmsg(a1 as i32, a2 as usize, a3 as i32) as u64, 0),
        SYS_RECVMSG                => SyscallResult(sys_recvmsg(a1 as i32, a2 as usize, a3 as i32) as u64, 0),
        SYS_SHMGET                 => SyscallResult(sys_shmget(a1 as i32, a2 as usize, a3 as i32) as u64, 0),
        SYS_SHMAT                  => SyscallResult(sys_shmat(a1 as i32, a2 as usize, a3 as i32) as u64, 0),
        SYS_SHMDT                  => SyscallResult(sys_shmdt(a1 as usize) as u64, 0),
        SYS_SHMCTL                 => SyscallResult(sys_shmctl(a1 as i32, a2 as i32, a3 as usize) as u64, 0),
        SYS_SEMGET                 => SyscallResult(sys_semget(a1 as i32, a2 as i32, a3 as i32) as u64, 0),
        SYS_SEMOP                  => SyscallResult(sys_semop(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_SEMCTL                 => SyscallResult(sys_semctl(a1 as i32, a2 as i32, a3 as i32, a4 as usize) as u64, 0),
        SYS_MSGGET                 => SyscallResult(sys_msgget(a1 as i32, a2 as i32) as u64, 0),
        SYS_MSGSND                 => SyscallResult(sys_msgsnd(a1 as i32, a2 as usize, a3 as usize, a4 as i32) as u64, 0),
        SYS_MSGRCV                 => SyscallResult(sys_msgrcv(a1 as i32, a2 as usize, a3 as usize, a4 as i64, a5 as i32) as u64, 0),
        SYS_MSGCTL                 => SyscallResult(sys_msgctl(a1 as i32, a2 as i32, a3 as usize) as u64, 0),
        SYS_TIMER_CREATE           => SyscallResult(sys_timer_create(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_TIMER_SETTIME          => SyscallResult(sys_timer_settime(a1 as i32, a2 as i32, a3 as usize, a4 as usize) as u64, 0),
        SYS_TIMER_GETTIME          => SyscallResult(sys_timer_gettime(a1 as i32, a2 as usize) as u64, 0),
        SYS_TIMER_GETOVERRUN       => SyscallResult(sys_timer_getoverrun(a1 as i32) as u64, 0),
        SYS_TIMER_DELETE           => SyscallResult(sys_timer_delete(a1 as i32) as u64, 0),
        SYS_FUTEX_WAITV            => SyscallResult(sys_futex_waitv(a1 as usize, a2 as u32, a3 as u32, a4 as usize, a5 as i32) as u64, 0),
        SYS_MLOCK2                 => SyscallResult(sys_mlock2(a1 as usize, a2 as usize, a3 as i32) as u64, 0),
        SYS_IOPRIO_GET             => SyscallResult(sys_ioprio_get(a1 as i32, a2 as i32) as u64, 0),
        SYS_IOPRIO_SET             => SyscallResult(sys_ioprio_set(a1 as i32, a2 as i32, a3 as i32) as u64, 0),
        SYS_SCHED_RR_GET_INTERVAL  => SyscallResult(sys_sched_rr_get_interval(a1 as i32, a2 as usize) as u64, 0),
        SYS_GET_ROBUST_LIST        => SyscallResult(sys_get_robust_list(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_SET_ROBUST_LIST        => SyscallResult(sys_set_robust_list(a1 as usize, a2 as usize) as u64, 0),
        SYS_RESTART_SYSCALL        => SyscallResult(sys_restart_syscall() as u64, 0),
        SYS_KEXEC_LOAD             => SyscallResult(sys_kexec_load(a1 as usize, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_KEXEC_FILE_LOAD        => SyscallResult(sys_kexec_file_load(a1 as i32, a2 as i32, a3 as usize, a4 as usize, a5 as usize) as u64, 0),
        SYS_PKEY_MPROTECT          => SyscallResult(sys_pkey_mprotect(a1 as usize, a2 as usize, a3 as i32, a4 as i32) as u64, 0),
        SYS_PKEY_ALLOC             => SyscallResult(sys_pkey_alloc(a1 as u32, a2 as u32) as u64, 0),
        SYS_PKEY_FREE              => SyscallResult(sys_pkey_free(a1 as i32) as u64, 0),
        SYS_RSEQ                   => SyscallResult(sys_rseq(a1 as usize, a2 as u32, a3 as i32, a4 as u32) as u64, 0),
        SYS_IO_URING_SETUP         => SyscallResult(sys_io_uring_setup(a1 as u32, a2 as usize) as u64, 0),
        SYS_IO_URING_ENTER         => SyscallResult(sys_io_uring_enter(a1 as i32, a2 as u32, a3 as u32, a4 as u32, a5 as usize) as u64, 0),
        SYS_IO_URING_REGISTER      => SyscallResult(sys_io_uring_register(a1 as i32, a2 as u32, a3 as usize, a4 as u32) as u64, 0),
        SYS_PREADV2                => SyscallResult(sys_preadv2(a1 as i32, a2 as usize, a3 as i32, a4 as i64, a5 as i32) as u64, 0),
        SYS_PWRITEV2               => SyscallResult(sys_pwritev2(a1 as i32, a2 as usize, a3 as i32, a4 as i64, a5 as i32) as u64, 0),
        SYS_SETDOMAINNAME          => SyscallResult(sys_setdomainname(a1 as usize, a2 as usize) as u64, 0),
        SYS_SWAPON                 => SyscallResult(sys_swapon(a1 as usize, a2 as i32) as u64, 0),
        SYS_SWAPOFF                => SyscallResult(sys_swapoff(a1 as usize) as u64, 0),
        SYS_IOPERM                 => SyscallResult(sys_ioperm(a1 as usize, a2 as usize, a3 as i32) as u64, 0),
        SYS_IOPL                   => SyscallResult(sys_iopl(a1 as i32) as u64, 0),
        SYS_USERFAULTFD            => SyscallResult(sys_userfaultfd(a1 as i32) as u64, 0),
        SYS_MQ_OPEN                => SyscallResult(sys_mq_open(a1 as usize, a2 as i32, a3 as u32, a4 as usize) as u64, 0),
        SYS_MQ_UNLINK              => SyscallResult(sys_mq_unlink(a1 as usize) as u64, 0),
        SYS_MQ_TIMEDSEND           => SyscallResult(sys_mq_timedsend(a1 as i32, a2 as usize, a3 as usize, a4 as u32, a5 as usize) as u64, 0),
        SYS_MQ_TIMEDRECEIVE        => SyscallResult(sys_mq_timedreceive(a1 as i32, a2 as usize, a3 as usize, a4 as usize, a5 as usize) as u64, 0),
        SYS_MQ_NOTIFY              => SyscallResult(sys_mq_notify(a1 as i32, a2 as usize) as u64, 0),
        SYS_MQ_GETSETATTR          => SyscallResult(sys_mq_getsetattr(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_SET_THREAD_AREA        => SyscallResult(sys_set_thread_area(a1 as usize) as u64, 0),
        SYS_GET_THREAD_AREA        => SyscallResult(sys_get_thread_area(a1 as usize) as u64, 0),
        SYS_SET_TID_ADDRESS        => SyscallResult(sys_set_tid_address(a1 as usize) as u64, 0),
        SYS_FANOTIFY_INIT          => SyscallResult(sys_fanotify_init(a1 as u32, a2 as u32) as u64, 0),
        SYS_FANOTIFY_MARK          => SyscallResult(sys_fanotify_mark(a1 as i32, a2 as u32, a3 as u64, a4 as i32, a5 as usize) as u64, 0),
        SYS_NAME_TO_HANDLE_AT      => SyscallResult(sys_name_to_handle_at(a1 as i32, a2 as usize, a3 as usize, a4 as usize, a5 as i32) as u64, 0),
        SYS_OPEN_BY_HANDLE_AT      => SyscallResult(sys_open_by_handle_at(a1 as i32, a2 as usize, a3 as i32) as u64, 0),
        SYS_FINIT_MODULE           => SyscallResult(sys_finit_module(a1 as i32, a2 as usize, a3 as i32) as u64, 0),
        SYS_REMAP_FILE_PAGES       => SyscallResult(sys_remap_file_pages(a1 as usize, a2 as usize, a3 as u32, a4 as usize, a5 as u32) as u64, 0),
        SYS_SENDFILE               => SyscallResult(sys_sendfile(a1 as i32, a2 as i32, a3 as usize, a4 as usize) as u64, 0),
        SYS_CONNECT                => SyscallResult(sys_connect(a1 as i32, a2 as usize, a3 as u32) as u64, 0),
        SYS_LISTEN                 => SyscallResult(sys_listen(a1 as i32, a2 as i32) as u64, 0),
        SYS_ACCEPT                 => SyscallResult(sys_accept(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_ACCEPT4                => SyscallResult(sys_accept4(a1 as i32, a2 as usize, a3 as usize, a4 as i32) as u64, 0),
        SYS_INOTIFY_INIT           => SyscallResult(sys_inotify_init() as u64, 0),
        SYS_INOTIFY_INIT1          => SyscallResult(sys_inotify_init1(a1 as i32) as u64, 0),
        SYS_INOTIFY_ADD_WATCH      => SyscallResult(sys_inotify_add_watch(a1 as i32, a2 as usize, a3 as u32) as u64, 0),
        SYS_INOTIFY_RM_WATCH       => SyscallResult(sys_inotify_rm_watch(a1 as i32, a2 as i32) as u64, 0),
        SYS_SETXATTR               => SyscallResult(sys_setxattr(a1 as usize, a2 as usize, a3 as usize, a4 as usize, a5 as i32) as u64, 0),
        SYS_GETXATTR               => SyscallResult(sys_getxattr(a1 as usize, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_LISTXATTR              => SyscallResult(sys_listxattr(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_REMOVEXATTR            => SyscallResult(sys_removexattr(a1 as usize, a2 as usize) as u64, 0),
        SYS_FSETXATTR              => SyscallResult(sys_fsetxattr(a1 as i32, a2 as usize, a3 as usize, a4 as usize, a5 as i32) as u64, 0),
        SYS_FGETXATTR              => SyscallResult(sys_fgetxattr(a1 as i32, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_LSETXATTR              => SyscallResult(sys_lsetxattr(a1 as usize, a2 as usize, a3 as usize, a4 as usize, a5 as i32) as u64, 0),
        SYS_LGETXATTR              => SyscallResult(sys_lgetxattr(a1 as usize, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_LLISTXATTR             => SyscallResult(sys_llistxattr(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_LREMOVEXATTR           => SyscallResult(sys_lremovexattr(a1 as usize, a2 as usize) as u64, 0),
        SYS_FLISTXATTR             => SyscallResult(sys_flistxattr(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_FREMOVEXATTR           => SyscallResult(sys_fremovexattr(a1 as i32, a2 as usize) as u64, 0),
        SYS_MKNODAT                => SyscallResult(sys_mknodat(a1 as i32, a2 as usize, a3 as u32, a4 as u64) as u64, 0),
        SYS_FUTIMESAT              => SyscallResult(sys_futimesat(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_SELECT                 => SyscallResult(sys_select(a1 as i32, a2 as usize, a3 as usize, a4 as usize, a5 as usize) as u64, 0),
        SYS_PSELECT6               => SyscallResult(sys_pselect6(a1 as i32, a2 as usize, a3 as usize, a4 as usize, a5 as usize, 0) as u64, 0),
        SYS_ADJTIMEX               => SyscallResult(sys_adjtimex(a1 as usize) as u64, 0),
        SYS_CLOCK_ADJTIME          => SyscallResult(sys_clock_adjtime(a1 as i32, a2 as usize) as u64, 0),
        SYS_MIGRATE_PAGES          => SyscallResult(sys_migrate_pages(a1 as i32, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_MOVE_PAGES             => {
            let a6 = saved_r9_from_rsp(current_rsp);
            SyscallResult(sys_move_pages(a1 as i32, a2 as usize, a3 as usize, a4 as usize, a5 as usize, a6 as i32) as u64, 0)
        },
        SYS_QUOTACTL_FD            => SyscallResult(sys_quotactl_fd(a1 as i32, a2 as u32, a3 as i32, a4 as usize) as u64, 0),
        SYS_EXECVEAT               => sys_execveat(a1 as i32, a2 as usize, a3 as usize, a4 as usize, a5 as i32, current_rsp),
        SYS_PERF_EVENT_OPEN        => SyscallResult(sys_perf_event_open(a1 as usize, a2 as i32, a3 as i32, a4 as i32, a5) as u64, 0),
        SYS_BPF                    => SyscallResult(sys_bpf(a1 as i32, a2 as usize, a3 as u32) as u64, 0),
        SYS_MOUNT_SETATTR          => SyscallResult(sys_mount_setattr(a1 as i32, a2 as usize, a3 as u32, a4 as usize, a5 as usize) as u64, 0),
        SYS_SET_MEMPOLICY_HOME_NODE => SyscallResult(sys_set_mempolicy_home_node(a1 as usize, a2 as usize, a3, a4) as u64, 0),
        SYS_CACHESTAT              => SyscallResult(sys_cachestat(a1 as i32, a2 as usize, a3 as usize, a4 as u32) as u64, 0),
        SYS_RT_SIGRETURN           => sys_rt_sigreturn(a1 as usize, current_rsp),
        SYS_FB_INFO                => SyscallResult(sys_fb_info(a1 as usize) as u64, 0),
        SYS_SIGALTSTACK            => SyscallResult(sys_sigaltstack(a1 as usize, a2 as usize) as u64, 0),
        SYS_RT_SIGSUSPEND          => {
            let (res, sw) = sys_rt_sigsuspend(a1 as usize, a2 as usize, current_rsp);
            if res == SYSCALL_RESTART { SyscallResult(SYS_RT_SIGSUSPEND, sw) } else { SyscallResult(res as u64, sw) }
        }
        SYS_RT_SIGPENDING          => SyscallResult(sys_rt_sigpending(a1 as usize, a2 as usize) as u64, 0),
        SYS_RT_SIGQUEUEINFO        => SyscallResult(sys_rt_sigqueueinfo(a1 as i32, a2 as i32, a3 as usize) as u64, 0),
        SYS_RT_SIGTIMEDWAIT        => SyscallResult(sys_rt_sigtimedwait(a1 as usize, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_ARCH_PRCTL             => SyscallResult(sys_arch_prctl(a1 as i32, a2 as usize) as u64, 0),
        SYS_FLOCK                  => SyscallResult(sys_flock(a1 as i32, a2 as i32) as u64, 0),
        SYS_LCHOWN                 => SyscallResult(sys_lchown(a1 as usize, a2 as u32, a3 as u32) as u64, 0),
        SYS_UTIMES                 => SyscallResult(sys_utimes(a1 as usize, a2 as usize) as u64, 0),
        SYS_CHROOT                 => SyscallResult(sys_chroot(a1 as usize) as u64, 0),
        SYS_GETDOMAINNAME          => SyscallResult(sys_getdomainname(a1 as usize, a2 as usize) as u64, 0),
        SYS_EVENTFD                => SyscallResult(sys_eventfd(a1 as u32) as u64, 0),
        SYS_RT_TGSIGQUEUEINFO      => SyscallResult(sys_rt_tgsigqueueinfo(a1 as i32, a2 as i32, a3 as i32, a4 as usize) as u64, 0),
        SYS_RECVMMSG               => SyscallResult(sys_recvmmsg(a1 as i32, a2 as usize, a3 as u32, a4 as u32, a5 as usize) as u64, 0),
        SYS_SENDMMSG               => SyscallResult(sys_sendmmsg(a1 as i32, a2 as usize, a3 as u32, a4 as u32) as u64, 0),
        SYS_PROCESS_VM_READV       => SyscallResult(sys_process_vm_readv(a1 as i32, a2 as usize, a3 as usize, a4 as usize, a5 as usize, saved_r9_from_rsp(current_rsp) as u64) as u64, 0),
        SYS_PROCESS_VM_WRITEV      => SyscallResult(sys_process_vm_writev(a1 as i32, a2 as usize, a3 as usize, a4 as usize, a5 as usize, saved_r9_from_rsp(current_rsp) as u64) as u64, 0),
        SYS_SEMTIMEDOP             => SyscallResult(sys_semtimedop(a1 as i32, a2 as usize, a3 as usize, a4 as usize) as u64, 0),
        SYS_ACCT                   => SyscallResult(sys_acct(a1 as usize) as u64, 0),
        SYS_USTAT                  => SyscallResult(sys_ustat(a1 as u32, a2 as usize) as u64, 0),
        SYS_SYSLOG                 => SyscallResult(sys_syslog(a1 as i32, a2 as usize, a3 as i32) as u64, 0),
        SYS_VHANGUP                => SyscallResult(sys_vhangup() as u64, 0),
        SYS_MODIFY_LDT             => SyscallResult(sys_modify_ldt(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_SETFSUID               => SyscallResult(sys_setfsuid(a1 as u32) as u64, 0),
        SYS_SETFSGID               => SyscallResult(sys_setfsgid(a1 as u32) as u64, 0),
        SYS_PTRACE                 => SyscallResult(sys_ptrace(a1 as i64, a2 as i64, a3 as usize, a4 as usize) as u64, 0),
        SYS_MOUNT                  => SyscallResult(sys_mount(a1 as usize, a2 as usize, a3 as usize, a4 as u64, a5 as usize) as u64, 0),
        SYS_UMOUNT2                => SyscallResult(sys_umount2(a1 as usize, a2 as i32) as u64, 0),
        num if num >= crate::lkm::ksyms::DYNAMIC_SYSCALL_BASE => {
            if let Some(res) = crate::lkm::ksyms::dispatch_dynamic_syscall(num, a1, a2, a3, a4, a5) {
                SyscallResult(res as u64, 0)
            } else {
                SyscallResult((-38i64) as u64, 0)
            }
        }
        _          => SyscallResult((-38i64) as u64, 0),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  [ÉPICA 58] Entrega de señales en el retorno de syscall
// ─────────────────────────────────────────────────────────────────────────────
/// Syscalls que no regresan al contexto de usuario invocado (el frame de
/// Ring 3 desaparece), por lo que no admiten entrega de señales en el retorno.
fn syscall_has_no_user_return(num: u64) -> bool {
    num == SYS_EXIT || num == SYS_EXECVE || num == SYS_EXECVEAT
}

/// [T-025 E3d] Última syscall por CPU (número | fase<<32; fase 1 = dentro, 2 = salió),
/// para el volcado de diagnóstico `^]` (`process::volcado_smp`).
static ULTIMA_SYSCALL: [core::sync::atomic::AtomicU64; crate::arch::percpu::MAX_CPUS] =
    [const { core::sync::atomic::AtomicU64::new(0) }; crate::arch::percpu::MAX_CPUS];
#[inline]
fn anotar_syscall(num: u64, fase: u64) {
    let i = crate::arch::percpu::indice_actual().min(crate::arch::percpu::MAX_CPUS - 1);
    ULTIMA_SYSCALL[i].store((num & 0xFFFF_FFFF) | (fase << 32), core::sync::atomic::Ordering::Relaxed);
}
pub fn ultima_syscall(cpu: usize) -> (u32, u32) {
    let v = ULTIMA_SYSCALL[cpu.min(crate::arch::percpu::MAX_CPUS - 1)].load(core::sync::atomic::Ordering::Relaxed);
    (v as u32, (v >> 32) as u32)
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(
    num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64,
    current_rsp: u64,
) -> SyscallResult {
    anotar_syscall(num | 0x8000_0000, 1); // bit 31: ABI nativa
    // [T-025 E3d] Cerrojo grande del núcleo durante toda la syscall (se suelta
    // en los puntos de espera; ver `arch::bkl`).
    let _cgn = crate::arch::bkl::Guard::tomar();
    // [ÉPICA 59] Trazador de llamadas al sistema con marcas de tiempo TSC.
    let tsc_start = crate::system::devsuite::trace_enter();
    let res = syscall_dispatch_core(num, a1, a2, a3, a4, a5, current_rsp);
    crate::system::devsuite::trace_exit(tsc_start, num, a1, res.0 as i64, true);
    // Sólo se despachan señales si el proceso continúa en el mismo frame
    // (new_rsp == 0) y el syscall no reemplaza el contexto de Ring 3.
    if res.1 == 0 && !syscall_has_no_user_return(num) {
        let _ = crate::process::signals::deliver_pending(current_rsp, Some(res.0));
        // [CONTROL DE TRABAJOS] Si la señal detuvo al proceso no puede volver a
        // Ring 3: se cede la CPU y, si no hay nadie más listo, se espera en
        // `hlt` hasta que SIGCONT/SIGKILL lo saque de `Stopped`.
        if crate::process::current_is_stopped() {
            let d_cgn = crate::arch::bkl::soltar_todo(); // [E3d] no retener el cerrojo detenido
            loop {
                let next = crate::process::switch_away();
                if next != 0 { return SyscallResult(res.0, next); }
                unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
                if !crate::process::current_is_stopped() { break; }
            }
            crate::arch::bkl::retomar(d_cgn);
            unsafe { let cur = crate::process::current_raw(); if !cur.is_null() { (*cur).state = crate::process::ProcessState::Running; } }
        }
    }
    anotar_syscall(num, 2);
    res
}

// ── Linux ABI x86_64 Compatibility Layer (Syscall Translation) ──────────

/// [ÉPICA 58] Núcleo del traductor de ABI Linux x86_64.
fn linux_syscall_dispatch_core(
    num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64,
    current_rsp: u64,
) -> SyscallResult {
    // [T-004] Traza de syscalls Linux (nr y args) para procesos cuyo nombre
    // empieza por el prefijo indicado: diagnóstico de arranques que rompen
    // (así se encontraron TCGETS de 60 B, RDX≠0 y time(2)). Desactivada por
    // defecto: activar cambiando STRACE_PREFIJO a Some("/usr/bin/...").
    const STRACE_PREFIJO: Option<&str> = None;
    let mut trazar = false;
    if let Some(prefijo) = STRACE_PREFIJO {
        if let Some(p) = process::current_process() {
            if p.name_str().starts_with(prefijo) {
                trazar = true;
                serial::write_str("[STRACE] nr="); serial::write_usize(num as usize);
                serial::write_str(" a1="); serial::write_hex(a1 as usize);
                serial::write_str(" a2="); serial::write_hex(a2 as usize);
                serial::write_str(" a3="); serial::write_hex(a3 as usize);
                serial::write_str("\n");
            }
        }
    }
    let resultado = linux_syscall_match(num, a1, a2, a3, a4, a5, current_rsp);
    if trazar {
        // [b190] También el valor devuelto (así se ve QUÉ syscall falla, no solo cuál se llamó).
        serial::write_str("[STRACE]   -> "); serial::write_hex(resultado.0 as usize); serial::write_str("\n");
    }
    resultado
}

#[inline(always)]
fn linux_syscall_match(
    num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64,
    current_rsp: u64,
) -> SyscallResult {
    match num {
        0 => { // sys_read(fd, buf, count)
            let (res, sw) = sys_read(a1 as i32, a2 as usize, a3 as usize, current_rsp);
            SyscallResult(res as u64, sw)
        }
        1 => { // sys_write(fd, buf, count)
            SyscallResult(sys_write(a1 as i32, a2 as usize, a3 as usize) as u64, 0)
        }
        2 => { // sys_open(filename, flags, mode)
            SyscallResult(sys_open(a1 as usize, a2 as u32) as u64, 0)
        }
        3 => { // sys_close(fd)
            SyscallResult(sys_close(a1 as i32) as u64, 0)
        }
        4 => { // sys_stat(filename, statbuf)
            SyscallResult(sys_stat(a1 as usize, a2 as usize) as u64, 0)
        }
        5 => { // sys_fstat(fd, statbuf)
            SyscallResult(sys_fstat(a1 as i32, a2 as usize) as u64, 0)
        }
        6 => { // [CICLO 49 / T1] sys_lstat(path, statbuf): lo usa `ls -l` de busybox (musl)
            SyscallResult(sys_lstat(a1 as usize, a2 as usize) as u64, 0)
        }
        332 => { // [CICLO 49 / T1] sys_statx(dirfd, path, flags, mask, buf)
            SyscallResult(sys_statx(a1 as i32, a2 as usize, a3 as i32, a4 as u32, a5 as usize) as u64, 0)
        }
        7 => { // sys_poll(fds, nfds, timeout)
            SyscallResult(sys_poll(a1 as usize, a2 as usize, a3 as i32) as u64, 0)
        }
        8 => { // sys_lseek(fd, offset, whence)
            SyscallResult(sys_lseek(a1 as i32, a2 as i64, a3 as i32) as u64, 0)
        }
        9 => { // sys_mmap(addr, len, prot, flags, fd, offset)
            // [CICLO 50 / T1b] el 6.º argumento (offset de fichero) viaja en r9.
            let a6 = saved_r9_from_rsp(current_rsp);
            SyscallResult(sys_mmap(a1 as usize, a2 as usize, a3 as u32, a4 as u32, a5 as i32, a6 as i64) as u64, 0)
        }
        10 => { // sys_mprotect
            SyscallResult(sys_mprotect(a1 as usize, a2 as usize, a3 as i32) as u64, 0)
        }
        11 => { // sys_munmap(addr, len)
            SyscallResult(sys_munmap(a1 as usize, a2 as usize) as u64, 0)
        }
        12 => { // sys_brk(brk)
            SyscallResult(sys_brk(a1 as usize) as u64, 0)
        }
        13 => { // sys_rt_sigaction(sig, act, oact, sigsetsize)
            SyscallResult(sys_sigaction(a1 as i32, a2 as usize, a3 as usize) as u64, 0)
        }
        14 => { // sys_rt_sigprocmask
            SyscallResult(sys_sigprocmask(a1 as i32, a2 as usize, a3 as usize) as u64, 0)
        }
        15 => { // sys_rt_sigreturn: restauración de contexto tras el manejador
            sys_rt_sigreturn(a1 as usize, current_rsp)
        }
        16 => { // sys_ioctl
            SyscallResult(sys_ioctl(a1 as i32, a2 as u64, a3 as usize) as u64, 0)
        }
        17 => { // sys_pread64
            SyscallResult(sys_pread64(a1 as i32, a2 as usize, a3 as usize, a4 as i64) as u64, 0)
        }
        18 => { // sys_pwrite64
            SyscallResult(sys_pwrite64(a1 as i32, a2 as usize, a3 as usize, a4 as i64) as u64, 0)
        }
        19 => { // sys_readv(fd, iov, iovcnt) [T-003] musl fread() grande (gas) lo usa
            let (n, new_rsp) = sys_readv(a1 as i32, a2 as usize, a3 as i32, current_rsp);
            SyscallResult(n as u64, new_rsp)
        }
        20 => { // sys_writev(fd, iov, iovcnt)
            SyscallResult(sys_writev(a1 as i32, a2 as usize, a3 as i32) as u64, 0)
        }
        95 => { // sys_umask(mask) [T-003] gcc al crear temporales
            SyscallResult(crate::process::set_current_umask(a1 as u32) as u64, 0)
        }
        201 => { // sys_time(tloc) [T-004] glibc sin vDSO: time(NULL)
            let seg = crate::time::rtc::now_epoch() as u64; // RTC CMOS real
            if a1 != 0 {
                if !verify_user_buffer(a1 as usize, 8) { return SyscallResult((-14i64) as u64, 0); }
                if paging::copy_to_user(a1 as usize, &seg.to_le_bytes()).is_err() { return SyscallResult((-14i64) as u64, 0); }
            }
            SyscallResult(seg, 0)
        }
        98 => { // sys_getrusage(who, usage) [T-003] gcc/collect2 lo piden al terminar
            SyscallResult(sys_getrusage(a1 as i32, a2 as usize) as u64, 0)
        }
        21 => { // sys_access(filename, mode)
            SyscallResult(sys_access(a1 as usize, a2 as i32) as u64, 0)
        }
        26 => { // sys_msync(addr, len, flags)
            SyscallResult(sys_msync(a1 as usize, a2 as usize, a3 as i32) as u64, 0)
        }
        22 => { // sys_pipe(fildes)
            SyscallResult(sys_pipe(a1 as usize) as u64, 0)
        }
        24 => { // sys_sched_yield
            SyscallResult(0, sys_yield_switch(current_rsp))
        }
        39 => { // sys_getpid
            SyscallResult(sys_getpid(), 0)
        }
        41 => { // sys_socket
            SyscallResult(sys_socket(a1 as i32, a2 as i32, a3 as i32) as u64, 0)
        }
        42 => { // sys_connect
            SyscallResult(sys_connect(a1 as i32, a2 as usize, a3 as u32) as u64, 0)
        }
        43 => { // sys_accept
            SyscallResult(sys_accept(a1 as i32, a2 as usize, a3 as usize) as u64, 0)
        }
        44 => { // sys_sendto
            SyscallResult(sys_sendto(a1 as i32, a2 as usize, a3 as usize, a4 as i32, a5 as usize, 16) as u64, 0)
        }
        45 => { // sys_recvfrom
            SyscallResult(sys_recvfrom(a1 as i32, a2 as usize, a3 as usize, a4 as i32, a5 as usize, 0) as u64, 0)
        }
        49 => { // sys_bind
            SyscallResult(sys_bind(a1 as i32, a2 as usize, a3 as u32) as u64, 0)
        }
        50 => { // sys_listen
            SyscallResult(sys_listen(a1 as i32, a2 as i32) as u64, 0)
        }
        52 => { // [T-004] sys_getpeername: bash lo llama sobre stdin para saber si es un socket
            SyscallResult(sys_getpeername(a1 as i32, a2 as usize, a3 as usize) as u64, 0)
        }
        54 => { // [b191] sys_setsockopt: la JVM (suite 71) lo pedía y recibía ENOSYS; misma firma que la nativa
            SyscallResult(sys_setsockopt(a1 as i32, a2 as i32, a3 as i32, a4 as usize, a5 as u32) as u64, 0)
        }
        55 => { // [b191] sys_getsockopt
            SyscallResult(sys_getsockopt(a1 as i32, a2 as i32, a3 as i32, a4 as usize, a5 as usize) as u64, 0)
        }
        111 => { // [T-004] sys_getpgrp: bash (control de trabajos) = getpgid(0)
            SyscallResult(match crate::process::get_pgid(0) { Some(pgid) => pgid, None => (-3i64) as u64 }, 0)
        }
        51 => { // sys_getsockname
            SyscallResult(sys_getsockname(a1 as i32, a2 as usize, a3 as usize) as u64, 0)
        }
        56 => { // sys_clone
            SyscallResult(sys_clone(a1, a2 as usize, a3 as usize, a4 as usize, a5 as usize, current_rsp) as u64, 0)
        }
        57 => { // sys_fork
            SyscallResult(sys_fork(current_rsp) as u64, 0)
        }
        59 => { // sys_execve
            sys_execve(a1 as usize, a2 as usize, a3 as usize, current_rsp)
        }
        60 => { // sys_exit: termina SOLO el hilo llamador (T-005)
            sys_exit_hilo(a1 as usize, current_rsp)
        }
        231 => { // sys_exit_group: termina todos los hilos del proceso
            sys_exit_group(a1 as usize)
        }
        61 => { // sys_wait4
            let (res, sw) = sys_waitpid(a1 as i64, a2 as usize, a3 as u32, current_rsp);
            // [FRENTE C-4] Reinicio transparente (semantica ERESTARTSYS de Linux).
            if res == SYSCALL_RESTART { SyscallResult(61, sw) } else { SyscallResult(res as u64, sw) }
        }
        234 => { // [T-005] sys_tgkill(tgid, tid, sig): tid debe pertenecer al grupo tgid
            let ok = match process::process_by_pid(a2) { Some(t) => t.tgid == a1 || t.pid == a1, None => false };
            if !ok { SyscallResult((-3i64) as u64, 0) } else { SyscallResult(sys_kill(a2, a3 as i32) as u64, 0) }
        }
        200 => { // [T-004] sys_tkill(tid, sig)
            SyscallResult(sys_kill(a1, a2 as i32) as u64, 0)
        }
        221 => { // [T-004] sys_fadvise64(fd, offset, len, advice): consejo de caché; coreutils sort/cat lo emiten
            if a1 as i32 >= 0 && process::current_process().and_then(|p| process::fd_get(p, a1 as usize)).is_some() { SyscallResult(0, 0) } else { SyscallResult((-9i64) as u64, 0) }
        }
        62 => { // sys_kill
            SyscallResult(sys_kill(a1, a2 as i32) as u64, 0)
        }
        63 => { // sys_uname
            SyscallResult(sys_uname(a1 as usize) as u64, 0)
        }
        72 => { // sys_fcntl
            SyscallResult(sys_fcntl(a1 as i32, a2 as i32, a3 as usize) as u64, 0)
        }
        79 => { // sys_getcwd
            SyscallResult(sys_getcwd(a1 as usize, a2 as usize) as u64, 0)
        }
        81 => { // [T-004] sys_fchdir: find(1) de findutils lo usa para descender
            SyscallResult(sys_fchdir(a1 as i32) as u64, 0)
        }
        137 => { // [T-004] sys_statfs(path, buf): struct statfs de 120 B con datos reales del FS
            SyscallResult(sys_statfs_linux(a1 as usize, a2 as usize) as u64, 0)
        }
        138 => { // [T-004] sys_fstatfs(fd, buf)
            SyscallResult(sys_fstatfs_linux(a1 as i32, a2 as usize) as u64, 0)
        }
        80 => { // sys_chdir
            SyscallResult(sys_chdir(a1 as usize) as u64, 0)
        }
        83 => { // sys_mkdir
            SyscallResult(sys_mkdir(a1 as usize, a2 as u32) as u64, 0)
        }
        84 => { // sys_rmdir
            SyscallResult(sys_rmdir(a1 as usize) as u64, 0)
        }
        87 => { // sys_unlink
            SyscallResult(sys_unlink(a1 as usize) as u64, 0)
        }
        280 => { // [CICLO 49 / T1] sys_utimensat(dirfd, path, times, flags): `touch` de busybox
            SyscallResult(sys_utimensat(a1 as i32, a2 as usize, a3 as usize, a4 as i32) as u64, 0)
        }
        88 => { // sys_symlink
            SyscallResult(sys_symlink(a1 as usize, a2 as usize) as u64, 0)
        }
        89 => { // sys_readlink
            SyscallResult(sys_readlink(a1 as usize, a2 as usize, a3 as usize) as u64, 0)
        }
        96 => { // sys_gettimeofday
            SyscallResult(sys_gettimeofday(a1 as usize, a2 as usize) as u64, 0)
        }
        99 => { // sys_sysinfo
            SyscallResult(sys_sysinfo(a1 as usize) as u64, 0)
        }
        101 => { // sys_ptrace
            SyscallResult(sys_ptrace(a1 as i64, a2 as i64, a3 as usize, a4 as usize) as u64, 0)
        }
        102 => { // sys_getuid
            SyscallResult(crate::process::current_uid() as u64, 0)
        }
        104 => { // sys_getgid
            SyscallResult(crate::process::current_gid() as u64, 0)
        }
        107 => { // sys_geteuid
            SyscallResult(crate::process::current_euid() as u64, 0)
        }
        108 => { // sys_getegid
            SyscallResult(crate::process::current_egid() as u64, 0)
        }
        110 => { // sys_getppid
            SyscallResult(sys_getppid(), 0)
        }
        127 => { // sys_rt_sigpending(set, sigsetsize)
            SyscallResult(sys_sigpending(a2 as usize) as u64, 0)
        }
        130 => { // sys_rt_sigsuspend(mask, sigsetsize)
            let (res, sw) = sys_rt_sigsuspend(a1 as usize, a2 as usize, current_rsp);
            if res == SYSCALL_RESTART { SyscallResult(130, sw) } else { SyscallResult(res as u64, sw) }
        }
        131 => { // sys_sigaltstack
            SyscallResult(sys_sigaltstack(a1 as usize, a2 as usize) as u64, 0)
        }
        157 => { // sys_prctl
            SyscallResult(sys_prctl(a1 as i32, a2 as u64, a3 as u64, a4 as u64, a5 as u64) as u64, 0)
        }
        158 => { // sys_arch_prctl
            SyscallResult(sys_arch_prctl(a1 as i32, a2 as usize) as u64, 0)
        }
        165 => { // sys_mount
            SyscallResult(sys_mount(a1 as usize, a2 as usize, a3 as usize, a4 as u64, a5 as usize) as u64, 0)
        }
        166 => { // sys_umount2
            SyscallResult(sys_umount2(a1 as usize, a2 as i32) as u64, 0)
        }
        186 => { // sys_gettid: el tid propio (distinto por hilo desde T-005)
            SyscallResult(sys_gettid(), 0)
        }
        28 => { // sys_madvise(addr, len, advice): glibc lo emite al liberar pilas de hilos
            SyscallResult(sys_madvise(a1 as usize, a2 as usize, a3 as i32) as u64, 0)
        }
        25 => { // sys_mremap(old, old_size, new_size, flags, new_addr) [b190]: realloc() de glibc
                // sobre trozos mmap-eados (CPython al crecer listas/dicts grandes) pasa por aqui.
            SyscallResult(sys_mremap(a1 as usize, a2 as usize, a3 as usize, a4 as i32, a5 as usize) as u64, 0)
        }
        36 => { // sys_getitimer(which, curr) [b190]
            SyscallResult(sys_getitimer(a1 as i32, a2 as usize) as u64, 0)
        }
        37 => { // sys_alarm(seconds) [b190]
            SyscallResult(process::alarm(a1 as u32) as u64, 0)
        }
        38 => { // sys_setitimer(which, new, old) [b190]: signal.setitimer de CPython (solo it_value;
                // it_interval no se rearma: limite anotado en TICKETS)
            SyscallResult(sys_setitimer(a1 as i32, a2 as usize, a3 as usize) as u64, 0)
        }
        436 => { // sys_close_range(first, last, flags) [b190]: posix_spawn de glibc 2.41 (closefrom)
                 // lo usa en el hijo; sin el, subprocess de CPython muere con rc=127.
            SyscallResult(sys_close_range(a1 as u32, a2 as u32, a3 as u32) as u64, 0)
        }
        35 => { // sys_nanosleep(req, rem) [b190]: lo usa el codigo sin libc (nasm) y libcs antiguas;
                // glibc moderna pasa por clock_nanosleep (230). Equivale a CLOCK_MONOTONIC relativo.
            let (res, sw) = sys_clock_nanosleep(1, 0, a1 as usize, a2 as usize, current_rsp);
            SyscallResult(res as u64, sw)
        }
        230 => { // sys_clock_nanosleep(clock, flags, req, rem): nanosleep() de glibc pasa por aquí
            let (res, sw) = sys_clock_nanosleep(a1 as i32, a2 as i32, a3 as usize, a4 as usize, current_rsp);
            SyscallResult(res as u64, sw)
        }
        202 => { // sys_futex(uaddr, op, val, timeout, uaddr2, val3) — val3 viaja en r9
            let (res, sw) = sys_futex(a1 as usize, a2 as i32, a3 as u32, a4 as usize, a5 as usize, saved_r9_from_rsp(current_rsp) as u32, current_rsp);
            if res == SYSCALL_RESTART { SyscallResult(202, sw) } else { SyscallResult(res as u64, sw) }
        }
        203 => { // sys_sched_setaffinity
            SyscallResult(sys_sched_setaffinity(a1 as i64, a2 as usize, a3 as usize) as u64, 0)
        }
        204 => { // sys_sched_getaffinity
            SyscallResult(sys_sched_getaffinity(a1 as i64, a2 as usize, a3 as usize) as u64, 0)
        }
        217 => { // sys_getdents64
            SyscallResult(sys_getdents64(a1 as i32, a2 as usize, a3 as usize) as u64, 0)
        }
        218 => { // sys_set_tid_address
            SyscallResult(sys_set_tid_address(a1 as usize) as u64, 0)
        }
        228 => { // sys_clock_gettime
            SyscallResult(sys_clock_gettime(a1 as i32, a2 as usize) as u64, 0)
        }
        229 => { // sys_clock_getres [b191]: el JVM la usa para decidir si hay reloj monotónico
            // utilizable; resolución real del contador: 1 ns para relojes de sistema/TSC,
            // y para los relojes de CPU de proceso/hilo (ids negativos) la del PIT (10 ms).
            let res_ns: u64 = if (a1 as i32) < 0 { 10_000_000 } else { 1 };
            if a2 != 0 {
                let ts = [0u64, res_ns];
                let bytes = unsafe { core::slice::from_raw_parts(ts.as_ptr() as *const u8, 16) };
                if paging::copy_to_user(a2 as usize, bytes).is_err() { return SyscallResult((-14i64) as u64, 0); }
            }
            SyscallResult(0, 0)
        }
        309 => { // sys_getcpu [b191]; sin esto glibc cae al vsyscall 0xFFFFFFFFFF600800 (#PF). [T-017] núcleo real.
            let cpu = crate::arch::percpu::indice_actual() as u32;
            if a1 != 0 && paging::copy_to_user(a1 as usize, &cpu.to_ne_bytes()).is_err() { return SyscallResult((-14i64) as u64, 0); }
            if a2 != 0 && paging::copy_to_user(a2 as usize, &0u32.to_ne_bytes()).is_err() { return SyscallResult((-14i64) as u64, 0); }
            SyscallResult(0, 0)
        }
        257 => { // sys_openat
            SyscallResult(sys_openat(a1 as i32, a2 as usize, a3 as u32) as u64, 0)
        }
        262 => { // sys_newfstatat
            SyscallResult(sys_fstatat(a1 as i32, a2 as usize, a3 as usize, a4 as i32) as u64, 0)
        }
        267 => { // sys_readlinkat
            SyscallResult(sys_readlinkat(a1 as i32, a2 as usize, a3 as usize, a4 as usize) as u64, 0)
        }
        269 => { // sys_faccessat
            SyscallResult(sys_faccessat(a1 as i32, a2 as usize, a3 as i32, a4 as i32) as u64, 0)
        }
        73 => { // sys_flock [b191]: misma rutina nativa (valida fd y operación)
            SyscallResult(sys_flock(a1 as i32, a2 as i32) as u64, 0)
        }
        273 => { // sys_set_robust_list
            SyscallResult(sys_set_robust_list(a1 as usize, a2 as usize) as u64, 0)
        }
        274 => { // sys_get_robust_list
            SyscallResult(sys_get_robust_list(a1 as i32, a2 as usize, a3 as usize) as u64, 0)
        }
        288 => { // sys_accept4
            SyscallResult(sys_accept4(a1 as i32, a2 as usize, a3 as usize, a4 as i32) as u64, 0)
        }
        290 => { // sys_eventfd2
            SyscallResult(sys_eventfd2(a1 as u32, a2 as i32) as u64, 0)
        }
        291 => { // sys_epoll_create1
            SyscallResult(sys_epoll_create1(a1 as i32) as u64, 0)
        }
        32 => { // [CICLO 49 / T1] sys_dup: faltaba en el traductor (ash lo usa en cada tuberia)
            SyscallResult(sys_dup(a1 as i32) as u64, 0)
        }
        34 => { // sys_pause [T-001/FIFO]: faltaba en el traductor
            let (res, sw) = sys_pause(current_rsp);
            if res == SYSCALL_RESTART { SyscallResult(34, sw) } else { SyscallResult(res as u64, sw) }
        }
        33 => { // [CICLO 49 / T1] sys_dup2: idem (redirecciones `>`, `|`)
            SyscallResult(sys_dup2(a1 as i32, a2 as i32) as u64, 0)
        }
        // [T-001] Syscalls de ficheros de la ABI Linux que faltaban en la tabla
        // (devolvian ENOSYS a busybox/musl): se traducen a las nativas, que ya
        // operan por el contrato sobre ext2/ramfs.
        74 => SyscallResult(sys_fsync(a1 as i32) as u64, 0),
        75 => SyscallResult(sys_fdatasync(a1 as i32) as u64, 0),
        76 => SyscallResult(sys_truncate(a1 as usize, a2 as i64) as u64, 0),
        77 => SyscallResult(sys_ftruncate(a1 as i32, a2 as i64) as u64, 0),
        82 => SyscallResult(sys_rename(a1 as usize, a2 as usize) as i64 as u64, 0),
        85 => SyscallResult(sys_open(a1 as usize, 0o1101) as u64, 0), // creat(2) = open(O_CREAT|O_WRONLY|O_TRUNC)
        86 => SyscallResult(sys_link(a1 as usize, a2 as usize) as u64, 0),
        90 => SyscallResult(sys_chmod(a1 as usize, a2 as u32) as u64, 0),
        91 => SyscallResult(sys_fchmod(a1 as i32, a2 as u32) as u64, 0),
        92 => SyscallResult(sys_chown(a1 as usize, a2 as u32, a3 as u32) as u64, 0),
        93 => SyscallResult(sys_fchown(a1 as i32, a2 as u32, a3 as u32) as u64, 0),
        94 => SyscallResult(sys_lchown(a1 as usize, a2 as u32, a3 as u32) as i64 as u64, 0),
        133 => SyscallResult(sys_mknod(a1 as usize, a2 as u32, a3) as u64, 0),
        162 => SyscallResult(sys_sync() as u64, 0),
        258 => SyscallResult(sys_mkdirat(a1 as i32, a2 as usize) as i64 as u64, 0),
        259 => SyscallResult(sys_mknodat(a1 as i32, a2 as usize, a3 as u32, a4) as i64 as u64, 0),
        260 => SyscallResult(sys_fchownat(a1 as i32, a2 as usize, a3 as u32, a4 as u32, a5 as i32) as i64 as u64, 0),
        263 => SyscallResult(sys_unlinkat(a1 as i32, a2 as usize, a3 as i32) as i64 as u64, 0),
        264 => SyscallResult(sys_renameat(a1 as i32, a2 as usize, a3 as i32, a4 as usize) as i64 as u64, 0),
        265 => SyscallResult(sys_linkat(a1 as i32, a2 as usize, a3 as i32, a4 as usize, a5 as i32) as i64 as u64, 0),
        266 => SyscallResult(sys_symlinkat(a1 as usize, a2 as i32, a3 as usize) as i64 as u64, 0),
        268 => SyscallResult(sys_fchmodat(a1 as i32, a2 as usize, a3 as u32, a4 as i32) as i64 as u64, 0),
        306 => SyscallResult(sys_syncfs(a1 as i32) as u64, 0),
        292 => { // sys_dup3
            SyscallResult(sys_dup3(a1 as i32, a2 as i32, a3 as i32) as u64, 0)
        }
        293 => { // sys_pipe2
            SyscallResult(sys_pipe2(a1 as usize, a2 as i32) as u64, 0)
        }
        302 => { // sys_prlimit64
            SyscallResult(sys_prlimit64(a1 as i32, a2 as i32, a3 as usize, a4 as usize) as u64, 0)
        }
        318 => { // sys_getrandom
            SyscallResult(sys_getrandom(a1 as usize, a2 as usize, a3 as u32) as u64, 0)
        }
        334 => { // sys_rseq
            SyscallResult(0, 0)
        }
        435 => { // sys_clone3
            SyscallResult(sys_clone3(a1 as usize, a2 as usize, current_rsp) as u64, 0)
        }
        439 => { // sys_faccessat2
            SyscallResult(sys_faccessat2(a1 as i32, a2 as usize, a3 as i32, a4 as i32) as u64, 0)
        }
        _ => {
            // [FRENTE C-5] Número Linux SIN traducción explícita. Antes se
            // delegaba al despachador nativo con el MISMO número, pero las
            // tablas no coinciden (Linux 0=read vs Portix 0=exit, Linux
            // 23=select vs Portix 23=waitpid...): un binario Linux podía
            // ejecutar un syscall arbitrario de Portix. Ahora se contabiliza
            // (contador + mapa de bits, visible en /proc/abi y `abicoverage`)
            // y se devuelve -ENOSYS (38), que es lo que glibc/musl esperan.
            crate::system::devsuite::note_linux_fallback(num);
            serial::write_str("[ABI] syscall Linux sin traduccion nr=");
            serial::write_usize(num as usize);
            serial::write_str(" -> ENOSYS\n");
            SyscallResult((-38i64) as u64, 0)
        }
    }
}

/// [ÉPICA 58] Envoltorio del traductor Linux: entrega señales pendientes al
/// regresar a Ring 3 (equivalente al `prepare_exit_to_usermode` de Linux).
#[no_mangle]
pub extern "C" fn linux_syscall_dispatch(
    num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64,
    current_rsp: u64,
) -> SyscallResult {
    // [ÉPICA 59] Trazador de llamadas al sistema (ABI Linux x86_64).
    anotar_syscall(num, 1);
    let _cgn = crate::arch::bkl::Guard::tomar(); // [T-025 E3d]
    crate::system::devsuite::note_linux_call();
    let tsc_start = crate::system::devsuite::trace_enter();
    let res = linux_syscall_dispatch_core(num, a1, a2, a3, a4, a5, current_rsp);
    crate::system::devsuite::trace_exit(tsc_start, num, a1, res.0 as i64, false);
    // ABI Linux x86_64: 59 execve, 60 exit, 231 exit_group, 322 execveat
    let no_return = matches!(num, 59 | 60 | 231 | 322);
    if res.1 == 0 && !no_return {
        let _ = crate::process::signals::deliver_pending(current_rsp, Some(res.0));
        // [CONTROL DE TRABAJOS] Mismo tratamiento de `Stopped` que en la ABI
        // nativa: un binario Linux detenido por SIGTSTP no vuelve a Ring 3.
        if crate::process::current_is_stopped() {
            let d_cgn = crate::arch::bkl::soltar_todo(); // [E3d] no retener el cerrojo detenido
            loop {
                let next = crate::process::switch_away();
                if next != 0 { return SyscallResult(res.0, next); }
                unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
                if !crate::process::current_is_stopped() { break; }
            }
            crate::arch::bkl::retomar(d_cgn);
            unsafe { let cur = crate::process::current_raw(); if !cur.is_null() { (*cur).state = crate::process::ProcessState::Running; } }
        }
    }
    anotar_syscall(num, 2);
    if res.0 as i64 == -14 {
        // [E3d] diagnóstico: EFAULT con PID, CPU y número de syscall Linux
        serial::write_str("[EFAULT] nr=");
        serial::write_usize(num as usize);
        serial::write_str(" pid=");
        serial::write_usize(crate::process::current_pid() as usize);
        serial::write_str(" cpu=");
        serial::write_usize(crate::arch::percpu::indice_actual());
        serial::write_str(" a1=");
        serial::write_hex(a1 as usize);
        serial::write_str(" a2=");
        serial::write_hex(a2 as usize);
        serial::write_str("\n");
    }
    res
}

// ── SYS_EXIT ─────────────────────────────────────────────────────────────

fn sys_exit(status: usize) -> ! {
    serial::write_str("[R3] SYS_EXIT called\n");
    if let Some(proc) = process::current_process() {
        proc.exit_code = status as i32;
        for i in 0..process::MAX_FDS {
            process::fd_close(proc, i);
        }
    }
    unsafe { ring3_exit_trampoline(); }
    unreachable!()
}

/// [T-005] `CLONE_CHILD_CLEARTID`/`set_tid_address`: al morir un hilo el núcleo
/// escribe 0 en `clear_child_tid` y despierta un esperador de ese futex. Es
/// exactamente lo que espera `pthread_join` (musl: `__wait(&t->tid)`;
/// glibc: `futex(&pd->tid)`). Se hace con el espacio de direcciones aún vivo.
fn limpiar_tid_y_despertar(proc: &mut process::Process) {
    let dir = proc.clear_child_tid as usize;
    proc.clear_child_tid = 0;
    if dir != 0 && (dir % 4) == 0 && verify_user_buffer(dir, 4) {
        let _ = paging::copy_to_user(dir, &0u32.to_le_bytes());
        let n = futex::despertar(proc.cr3, dir, 1, !0);
        serial::write_str("[T-005] hilo tid=");
        serial::write_usize(proc.pid as usize);
        serial::write_str(" termina: *tid=0 y futex despertado (");
        serial::write_usize(n as usize);
        serial::write_str(")\n");
    }
    futex::retirar(proc.pid);
}

/// [T-005] `exit(2)`: termina sólo el hilo llamador. En un proceso sin hilos
/// equivale a `exit_group`. Si lo llama el líder con hilos vivos, el líder
/// queda dormido (`lider_salio`) y el proceso acaba con el último hilo.
fn sys_exit_hilo(status: usize, _current_rsp: u64) -> SyscallResult {
    let cur = unsafe { process::current_raw() };
    if cur.is_null() { return sys_exit(status); }
    let (es_hilo, pid, tgid) = unsafe { ((*cur).es_hilo, (*cur).pid, (*cur).tgid) };
    if !es_hilo {
        let vivos = process::tiene_hilos_vivos(pid);
        serial::write_str("[T-005] exit(2) del lider pid=");
        serial::write_usize(pid as usize);
        serial::write_str(if vivos { " con hilos vivos\n" } else { " sin hilos vivos\n" });
        if !vivos { return sys_exit(status); }
        // Líder con hilos vivos: se duerme para siempre; el último hilo lo cierra.
        unsafe {
            limpiar_tid_y_despertar(&mut *cur);
            (*cur).exit_code = status as i32;
            (*cur).lider_salio = true;
            (*cur).state = process::ProcessState::Blocked;
            (*cur).sleep_until = 0;
        }
        serial::write_str("[T-005] lider pid=");
        serial::write_usize(pid as usize);
        serial::write_str(" salio con hilos vivos: espera al ultimo\n");
        // No guardar el frame de la syscall para reanudarlo directamente en
        // Ring 3: el líder YA salió y jamás debe ejecutar la instrucción que
        // sigue a exit(2). La espera es expropiable en Ring 0; si otro proceso
        // se ejecuta, el líder reanuda aquí y termina desde su propio contexto.
        loop {
            process::wait_hlt_preemptible();
            crate::net::poll();
            unsafe { process::wake_blocked(); }
            let (sigkill, code) = unsafe { ((*cur).sig_pending & (1u64 << process::signals::SIGKILL as u64) != 0, (*cur).exit_code) };
            if sigkill {
                // La misma resolución que en la entrega normal distingue el
                // cierre interno del SIGKILL externo y mata el grupo si toca.
                let code = process::signals::preparar_terminacion_por_senal(process::signals::SIGKILL, pid);
                sys_exit(code as usize);
            }
            if !process::tiene_hilos_vivos(pid) {
                // El último hilo salió: terminar en el núcleo, no en Ring 3.
                sys_exit(code as usize);
            }
        }
    }
    // Hilo secundario.
    let ultimo = process::hilos_del_grupo(tgid, pid) == 0;
    unsafe { limpiar_tid_y_despertar(&mut *cur); }
    serial::write_str("[T-005] exit del hilo tid=");
    serial::write_usize(pid as usize);
    serial::write_str(" (tgid ");
    serial::write_usize(tgid as usize);
    serial::write_str(")\n");
    if ultimo {
        if let Some(l) = process::process_by_pid(tgid) {
            if l.lider_salio {
                if crate::arch::percpu::pid_en_otra_cpu(tgid) {
                    // [T-025 E3d] El líder sigue EN OTRA CPU, dentro de su bucle
                    // "espera al último" (sobre este mismo CR3): cerrarlo desde
                    // aquí destruía su pila y sus tablas bajo sus pies (el BSP
                    // esperaba 2 s a que el AP dejara el CR3, el AP esperaba al
                    // BSP: #UD con RIP=3). Su propio bucle verá que ya no quedan
                    // hilos y hará `exit` él mismo.
                    serial::write_str("[T-005] lider en otra CPU: cierra el proceso el mismo\n");
                } else {
                    // Cierre del proceso entero: el líder se recoge como un exit normal.
                    let code = l.exit_code;
                    process::cerrar_lider_dormido(l, code, 0);
                }
            }
        }
    }
    unsafe { ring3_exit_trampoline(); }
    unreachable!()
}

/// [T-005] `exit_group(2)`: mata a todos los demás hilos del grupo y termina.
fn sys_exit_group(status: usize) -> ! {
    let cur = unsafe { process::current_raw() };
    if !cur.is_null() {
        let (pid, tgid, es_hilo) = unsafe { ((*cur).pid, (*cur).tgid, (*cur).es_hilo) };
        let grupo = if es_hilo { tgid } else { pid };
        let mut tids = [0u64; 64];
        let n = process::tids_del_grupo(grupo, pid, &mut tids);
        for &t in tids.iter().take(n) {
            process::terminar_hilo_del_grupo(t, status as i32); // [E3d]
        }
        if n > 0 {
            serial::write_str("[T-005] exit_group: ");
            serial::write_usize(n);
            serial::write_str(" hilos terminados junto al llamador\n");
        }
        if es_hilo {
            // El llamador es un hilo secundario: el proceso termina con su código.
            unsafe { limpiar_tid_y_despertar(&mut *cur); }
            if let Some(l) = process::process_by_pid(tgid) {
                if crate::arch::percpu::pid_en_otra_cpu(tgid) {
                    // [T-025 E3d] El líder está EN OTRA CPU (en Ring 3 o en su bucle
                    // "espera al último"): no se cierra desde aquí (se destruían su
                    // pila y su CR3 bajo sus pies). Se le deja el código, SIGKILL
                    // pendiente e IPI: su propia CPU lo termina.
                    l.lider_salio = true;
                    l.exit_code = status as i32;
                    // exit_group es salida normal, no muerte por SIGKILL: la
                    // señal interna sólo pide al líder terminar en su CPU.
                    l.term_signal = 0;
                    process::terminar_hilo_del_grupo(tgid, status as i32);
                } else {
                    process::cerrar_lider_dormido(l, status as i32, 0);
                }
            }
            unsafe { ring3_exit_trampoline(); }
            unreachable!()
        }
    }
    sys_exit(status)
}

// ── SYS_WRITE ────────────────────────────────────────────────────────────

fn sys_write(fd: i32, buf: usize, count: usize) -> i64 {
    if count == 0 { return 0; }
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; }
    if !verify_user_buffer(buf, count) {
        serial::write_str("[SYSCALL] sys_write: invalid user buffer\n");
        return -14;
    }
    // Check if this is a device or pipe FD first
    if let Some(proc) = process::current_process() {
        if let Some(entry) = process::fd_get(proc, fd as usize) {
            match entry.fd_type {
                process::FdType::Stdin | process::FdType::PipeRead(_) => {
                    serial::write_str("[SYS] WRITE: read-only fd (EBADF)\n");
                    return -9;
                }
                process::FdType::Device(ref info) => {
                    let info_clone = *info;
                    return sys_write_device(fd, buf, count, &info_clone);
                }
                process::FdType::PipeWrite(pipe_id) => {
                    // [ÉPICA 58] POSIX: escritura en tubería sin lectores ->
                    // SIGPIPE al escritor y error EPIPE.
                    if !process::pipe_has_readers(pipe_id) {
                        crate::process::signals::raise_current(crate::process::signals::SIGPIPE);
                        crate::process::signals::note_sigpipe();
                        serial::write_str("[SIG] SIGPIPE: escritura en tuberia sin lectores (EPIPE)\n");
                        return -32; // -EPIPE
                    }
                    let mut kbuf = [0u8; 128];
                    let mut total = 0;
                    let no_bloqueante = (entry.flags & 0x800) != 0; // O_NONBLOCK
                    while total < count {
                        let to_read = (count - total).min(128);
                        if paging::copy_from_user(&mut kbuf[..to_read], buf + total, to_read).is_err() {
                            break;
                        }
                        for i in 0..to_read {
                            if !process::pipe_write_byte(pipe_id, kbuf[i]) {
                                // [T-004] POSIX: write() sobre una tubería LLENA con lectores
                                // vivos BLOQUEA hasta que el lector consuma (antes devolvía
                                // EAGAIN/cuenta corta y `head -c 100000 /dev/zero | wc -c`
                                // daba 4096 = PIPE_BUF_SIZE). Tope de seguridad 10 s.
                                if no_bloqueante { return if total == 0 { -11 } else { total as i64 }; }
                                let tope = crate::time::pit::ticks().wrapping_add(TOPE_BLOQUEO_TICKS);
                                while !process::pipe_has_space(pipe_id)
                                    && process::pipe_has_readers(pipe_id)
                                    && crate::time::pit::ticks() < tope
                                    && crate::process::signals::current_sig_pending() == 0
                                {
                                    unsafe { crate::process::scheduler::wait_hlt_preemptible(); }
                                }
                                if !process::pipe_has_readers(pipe_id) {
                                    if total > 0 { return total as i64; }
                                    crate::process::signals::note_sigpipe();
                                    return -32; // EPIPE
                                }
                                if !process::pipe_write_byte(pipe_id, kbuf[i]) {
                                    return if total == 0 { -4 } else { total as i64 }; // EINTR / cuenta corta
                                }
                            }
                            total += 1;
                        }
                    }
                    return total as i64;
                }
                process::FdType::FsFile(info) => {
                    return match sys_write_ramfs(fd, buf, count, info) {
                        Ok(n) => n,
                        // [L3c] errno exacto del sistema de ficheros (EROFS en /boot,
                        // ENOSPC, EFBIG...); -EIO solo si no hubo errno.
                        Err(e) => if e > 0 { -(e as i64) } else { -5 },
                    };
                }
                process::FdType::Socket(sock_id) => {
                    let mut kbuf = alloc::vec![0u8; count];
                    if paging::copy_from_user(&mut kbuf, buf, count).is_err() {
                        return -14;
                    }
                    return match crate::net::socket::socket_write(sock_id, &kbuf) {
                        Ok(n) => n as i64,
                        Err(e) => e as i64,
                    };
                }
                process::FdType::SocketPairEnd { write_pipe, .. } => {
                    let mut kbuf = [0u8; 128];
                    let mut total = 0;
                    while total < count {
                        let to_read = (count - total).min(128);
                        if paging::copy_from_user(&mut kbuf[..to_read], buf + total, to_read).is_err() {
                            break;
                        }
                        for i in 0..to_read {
                            if !process::pipe_write_byte(write_pipe, kbuf[i]) {
                                return if total == 0 { -11 } else { total as i64 };
                            }
                            total += 1;
                        }
                    }
                    return total as i64;
                }
                process::FdType::EventFd(id) => {
                    if count < 8 { return -22; }
                    let mut kbuf = [0u8; 8];
                    if paging::copy_from_user(&mut kbuf, buf, 8).is_err() {
                        return -14;
                    }
                    let add_val = u64::from_ne_bytes(kbuf);
                    if add_val == u64::MAX {
                        return -22;
                    }
                    let mut table = process::EVENTFD_TABLE.lock();
                    let ef = match table.fds.get_mut(id as usize) {
                        Some(e) if e.in_use => e,
                        _ => return -9,
                    };
                    if u64::MAX - 1 - ef.val < add_val {
                        if (ef.flags & 0x0800) != 0 {
                            return -11;
                        }
                        return -22;
                    }
                    ef.val += add_val;
                    return 8;
                }
                process::FdType::Epoll(_) | process::FdType::TimerFd(_) | process::FdType::SignalFd(_) => {
                    return -22;
                }
                process::FdType::PtyMaster(id) => {
                    let ptab = process::PTY_TABLE.lock();
                    let slave_pipe = ptab.ptys.get(id as usize).map(|p| p.slave_pipe);
                    drop(ptab);
                    match slave_pipe {
                        Some(pipe_id) => {
                            let mut kbuf = [0u8; 128];
                            let mut total = 0;
                            while total < count {
                                let to_read = (count - total).min(128);
                                if paging::copy_from_user(&mut kbuf[..to_read], buf + total, to_read).is_err() {
                                    break;
                                }
                                for i in 0..to_read {
                                    if !process::pipe_write_byte(pipe_id, kbuf[i]) {
                                        return if total == 0 { -11 } else { total as i64 };
                                    }
                                    total += 1;
                                }
                            }
                            return total as i64;
                        }
                        None => return -9,
                    }
                }
                process::FdType::PtySlave(id) => {
                    let ptab = process::PTY_TABLE.lock();
                    let master_pipe = ptab.ptys.get(id as usize).map(|p| p.master_pipe);
                    drop(ptab);
                    match master_pipe {
                        Some(pipe_id) => {
                            let mut kbuf = [0u8; 128];
                            let mut total = 0;
                            while total < count {
                                let to_read = (count - total).min(128);
                                if paging::copy_from_user(&mut kbuf[..to_read], buf + total, to_read).is_err() {
                                    break;
                                }
                                for i in 0..to_read {
                                    if !process::pipe_write_byte(pipe_id, kbuf[i]) {
                                        return if total == 0 { -11 } else { total as i64 };
                                    }
                                    total += 1;
                                }
                            }
                            return total as i64;
                        }
                        None => return -9,
                    }
                }
                process::FdType::ProcFile(info) => {
                    // [ÉPICA 59] Los nodos de control de la suite de desarrollo
                    // (/proc/syscalls, /proc/profiler) admiten escritura.
                    let node = crate::drivers::storage::procfs::node_name(info.file_type);
                    let mut kbuf = [0u8; 32];
                    let to_copy = count.min(kbuf.len());
                    if paging::copy_from_user(&mut kbuf[..to_copy], buf, to_copy).is_err() {
                        return -14; // -EFAULT
                    }
                    let res = crate::system::devsuite::control_write(node, &kbuf[..to_copy]);
                    let _ = info;
                    return res;
                }
                _ => {}
            }
        }
    }
    if fd != 1 && fd != 2 {
        return -9;
    }
    let mut total_copied: usize = 0;
    let mut kbuf = [0u8; 256];
    while total_copied < count {
        let to_copy = (count - total_copied).min(256);
        match paging::copy_from_user(&mut kbuf[..to_copy], buf + total_copied, to_copy) {
            Ok(copied) => {
                if copied == 0 { break; }
                for &b in &kbuf[..copied] { serial::write_byte(b); }
                total_copied += copied;
            }
            Err(()) => {
                if total_copied > 0 { return total_copied as i64; }
                return -14;
            }
        }
    }
    total_copied as i64
}

// Simple ring buffer for /dev/kbd — keyboard driver writes, shell reads
const KBD_BUF_SIZE: usize = 256;

struct KbdBuffer {
    data: [u8; KBD_BUF_SIZE],
    head: usize,
    tail: usize,
}

static KBD_BUFFER: Spinlock<KbdBuffer> = Spinlock::new(KbdBuffer {
    data: [0u8; KBD_BUF_SIZE],
    head: 0,
    tail: 0,
});

fn kbd_buf_write(data: &[u8]) {
    let mut buf = KBD_BUFFER.lock();
    for &b in data {
        let next = (buf.head + 1) % KBD_BUF_SIZE;
        if next != buf.tail {
            let h = buf.head;
            buf.data[h] = b;
            buf.head = next;
        }
    }
}

fn kbd_buf_read() -> Option<u8> {
    let mut buf = KBD_BUFFER.lock();
    if buf.tail != buf.head {
        let b = buf.data[buf.tail];
        buf.tail = (buf.tail + 1) % KBD_BUF_SIZE;
        Some(b)
    } else {
        None
    }
}

fn sys_write_device(_fd: i32, buf: usize, count: usize, info: &process::DeviceInfo) -> i64 {
    match info.dev_type {
        process::DeviceType::Null | process::DeviceType::Zero | process::DeviceType::Urandom => count as i64,
        process::DeviceType::Kbd => {
            // Keyboard driver writing scancodes
            let mut kbuf = [0u8; 64];
            let to_copy = count.min(64);
            if paging::copy_from_user(&mut kbuf[..to_copy], buf, to_copy).is_err() {
                return -14;
            }
            kbd_buf_write(&kbuf[..to_copy]);
            serial::write_str("[DEV] /dev/kbd write ");
            serial::write_usize(to_copy);
            serial::write_str(" bytes\n");
            to_copy as i64
        }
        process::DeviceType::Fb => {
            // Write to framebuffer via VGA text mode (0xB8000) or direct mapping
            let mut kbuf = alloc::vec![0u8; count.min(4096)];
            let to_copy = count.min(4096);
            if paging::copy_from_user(&mut kbuf[..to_copy], buf, to_copy).is_err() {
                return -14;
            }
            // For now, just log it
            serial::write_str("[DEV] /dev/fb0 write ");
            serial::write_usize(to_copy);
            serial::write_str(" bytes\n");
            to_copy as i64
        }
        process::DeviceType::Sda => {
            serial::write_str("[DEV] write not supported on /dev/sda0\n");
            -1
        }
        process::DeviceType::Dsp => {
            crate::drivers::audio::audio_write(buf, count)
        }
    }
}

// ── SYS_GETPID ───────────────────────────────────────────────────────────

fn sys_getpid() -> u64 {
    // [T-005] todos los hilos de un proceso ven el mismo pid (el tgid)
    process::current_process().map(|p| if p.es_hilo { p.tgid } else { p.pid }).unwrap_or(0)
}

// ── Scheduler helpers ────────────────────────────────────────────────────

fn saved_r9_from_rsp(current_rsp: u64) -> u64 {
    if current_rsp != 0 {
        unsafe { (current_rsp as *const u64).add(38).read() }
    } else {
        0
    }
}

fn saved_cs_from_rsp(current_rsp: u64) -> u64 {
    // Stack layout above current_rsp:
    // 256 bytes XMM (32 u64s) + 120 bytes GPRs (15 u64s) + 8 bytes RIP (1 u64) = 384 bytes (48 u64s)
    // CS is at offset 384 from RSP
    unsafe { (current_rsp as *const u64).add(48).read() }
}

fn sys_yield_switch(current_rsp: u64) -> u64 {
    unsafe {
        let cur = process::current_raw();
        if !cur.is_null() { (*cur).ticks_used = process::TIME_SLICE; }
    }
    let cs = saved_cs_from_rsp(current_rsp);
    process::schedule_desde_syscall(current_rsp, cs)
}

fn sys_sleep_switch(ticks: u64, current_rsp: u64) -> u64 {
    serial::write_str("[R3] SYS_SLEEP ");
    serial::write_usize(ticks as usize);
    serial::write_str(" ticks\n");
    let now = crate::time::pit::ticks();
    let target = now.wrapping_add(ticks);
    if let Some(proc) = process::current_process() {
        proc.sleep_until = target;
        proc.state = process::ProcessState::Blocked;
    }
    unsafe {
        let cur = process::current_raw();
        if !cur.is_null() { (*cur).ticks_used = process::TIME_SLICE; }
    }
    let cs = saved_cs_from_rsp(current_rsp);
    let new_rsp = process::schedule_desde_syscall(current_rsp, cs);
    if new_rsp == 0 {
        while crate::time::pit::ticks() < target {
            // [CONTROL DE TRABAJOS] señal pendiente (^C/^Z/kill) → el sueño termina ya.
            unsafe {
                let cur = process::current_raw();
                if !cur.is_null() && ((*cur).sig_pending != 0 || (*cur).sleep_until == 0) { break; }
            }
            // [RED EN REPOSO] Si nadie más corre, la pila de red sigue viva: se
            // acusan datos, corren los RTO y la ventana anunciada refleja el búfer
            // real aunque el único proceso esté dormido (antes los paquetes se
            // quedaban en el anillo del e1000 hasta la siguiente llamada de red).
            crate::net::poll();
            // [T-025 E3d] Soltar el cerrojo grande mientras se duerme: antes un
            // hilo dormido en un AP retenía el CGN 200 ms y su lider, en el BSP,
            // no podia ni hacer `exit` (T-005 "pthread_exit en main" con SMP).
            process::wait_hlt_preemptible();
        }
        unsafe {
            let cur = process::current_raw();
            if !cur.is_null() {
                (*cur).state = process::ProcessState::Running;
                (*cur).sleep_until = 0;
            }
        }
        return 0;
    }
    new_rsp
}

// ── SYS_READ ─────────────────────────────────────────────────────────────

fn sys_read(fd: i32, buf: usize, count: usize, current_rsp: u64) -> (i64, u64) {
    if fd < 0 || fd as usize >= process::MAX_FDS { return (-9, 0); }
    if !verify_user_buffer(buf, count) {
        serial::write_str("[SYSCALL] sys_read: invalid user buffer buf=");
        serial::write_hex(buf);
        serial::write_str(" count=");
        serial::write_usize(count);
        serial::write_str("\n");
        return (-14, 0);
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return (-3, 0),
    };
    let entry = match process::fd_get(&proc, fd as usize) {
        Some(e) => e.clone(),
        None => return (-9, 0),
    };
    // read de 0 bytes sobre un fd válido: 0 sin tocar nada (semántica Linux).
    if count == 0 { return (0, 0); }
    match &entry.fd_type {
        FdType::Stdin              => sys_read_stdin(buf, count, current_rsp),
        FdType::Stdout | FdType::Stderr => {
            serial::write_str("[SYS] READ: fd not readable\n");
            (-9, 0)
        }
        FdType::FsFile(ref info) => {
            sys_read_ramfile(fd, buf, count, info)
        }
        FdType::Device(ref info) => {
            sys_read_device(fd, buf, count, info)
        }
        FdType::PipeRead(pipe_id) => {
            let pid = *pipe_id;
            let mut total_read = 0usize;
            let mut kbuf = [0u8; 128];
            // [CICLO 47] Semantica POSIX: read() sobre una tuberia vacia con
            // escritores vivos BLOQUEA (cede la CPU por ticks) hasta que llegue
            // un byte, se cierre el ultimo escritor, haya una senal pendiente o
            // venza un tope de seguridad de 10 s. Antes devolvia 0 (EOF falso) y
            // `cmd | cat` dependia del orden de planificacion.
            if (entry.flags & 0x800) == 0 {
                // [T-005b, defecto 29] Bloqueo POSIX de verdad: sin tope de tiempo
                // (un `read` sobre una tubería con escritores vivos espera lo que haga
                // falta; el hilo ctrl-c de rustc vive así toda la compilación) y sólo
                // lo interrumpe una señal que no esté ignorada (SIGCHLD por defecto no
                // interrumpe nada). Si sale por señal sin datos: -EINTR, nunca 0 (EOF falso).
                while !process::pipe_has_data(pid)
                    && process::pipe_has_writers(pid)
                    && !crate::process::signals::senal_interrumpe()
                {
                    unsafe { crate::process::scheduler::wait_hlt_preemptible(); }
                }
                if !process::pipe_has_data(pid) && process::pipe_has_writers(pid) {
                    return (-4, 0); // -EINTR
                }
            }
            while total_read < count {
                let to_read = (count - total_read).min(128);
                let mut chunk_bytes = 0;
                while chunk_bytes < to_read {
                    match process::pipe_read_byte(pid) {
                        Some(b) => {
                            kbuf[chunk_bytes] = b;
                            chunk_bytes += 1;
                        }
                        None => break,
                    }
                }
                if chunk_bytes == 0 {
                    break;
                }
                if paging::copy_to_user(buf + total_read, &kbuf[..chunk_bytes]).is_err() {
                    return if total_read == 0 { (-14, 0) } else { (total_read as i64, 0) };
                }
                total_read += chunk_bytes;
                if chunk_bytes < to_read {
                    break;
                }
            }
            if total_read == 0 && (entry.flags & 0x800) != 0 && process::pipe_has_writers(pid) {
                return (-11, 0); // -EAGAIN on non-blocking pipe with active writers
            }
            (total_read as i64, 0)
        }
        FdType::PipeWrite(_) => {
            serial::write_str("[SYS] READ: write end of pipe not readable\n");
            (-9, 0)
        }
        FdType::Socket(_) => {
            let n = sys_recvfrom(fd, buf, count, 0, 0, 0);
            (n, 0)
        }
        FdType::SocketPairEnd { read_pipe, .. } => {
            let pid = *read_pipe;
            let mut kbuf = [0u8; 128];
            let to_read = count.min(128);
            let mut read_bytes = 0;
            // [CICLO 47-c] Misma semantica que PipeRead: un socket (socketpair o
            // AF_UNIX aceptado) vacio con el par vivo BLOQUEA; antes devolvia 0.
            if (entry.flags & 0x800) == 0 {
                let tope = crate::time::pit::ticks().wrapping_add(TOPE_BLOQUEO_TICKS);
                while !process::pipe_has_data(pid)
                    && process::pipe_has_writers(pid)
                    && crate::time::pit::ticks() < tope
                    && crate::process::signals::current_sig_pending() == 0
                {
                    unsafe { crate::process::scheduler::wait_hlt_preemptible(); }
                }
            } else if !process::pipe_has_data(pid) && process::pipe_has_writers(pid) {
                return (-11, 0); // -EAGAIN
            }
            while read_bytes < to_read {
                match process::pipe_read_byte(pid) {
                    Some(b) => {
                        kbuf[read_bytes] = b;
                        read_bytes += 1;
                    }
                    None => break,
                }
            }
            if read_bytes > 0 {
                if paging::copy_to_user(buf, &kbuf[..read_bytes]).is_err() {
                    return (-14, 0);
                }
                (read_bytes as i64, 0)
            } else {
                (0, 0)
            }
        }
        FdType::EventFd(id) => {
            if count < 8 { return (-22, 0); }
            // [CICLO 48] Contador a cero: con EFD_NONBLOCK -EAGAIN; si no,
            // BLOQUEA cediendo la CPU hasta que alguien escriba (tope 10 s o
            // senal). Antes devolvia 0 bytes, imposible para un eventfd.
            let tope = crate::time::pit::ticks().wrapping_add(TOPE_BLOQUEO_TICKS);
            loop {
                let (vacio, nonblock) = {
                    let table = process::EVENTFD_TABLE.lock();
                    match table.fds.get(*id as usize) {
                        Some(e) if e.in_use => (e.val == 0, (e.flags & 0x0800) != 0),
                        _ => return (-9, 0),
                    }
                };
                if !vacio { break; }
                if nonblock || crate::time::pit::ticks() >= tope { return (-11, 0); }
                if crate::process::signals::current_sig_pending() != 0 { return (-4, 0); }
                unsafe { crate::process::scheduler::wait_hlt_preemptible(); }
            }
            let mut table = process::EVENTFD_TABLE.lock();
            let ef = match table.fds.get_mut(*id as usize) {
                Some(e) if e.in_use => e,
                _ => return (-9, 0),
            };
            if ef.val == 0 {
                return (-11, 0);
            }
            let ret_val = if (ef.flags & 1) != 0 {
                ef.val -= 1;
                1u64
            } else {
                let v = ef.val;
                ef.val = 0;
                v
            };
            drop(table);
            if paging::copy_to_user(buf, &ret_val.to_ne_bytes()).is_err() {
                return (-14, 0);
            }
            (8, 0)
        }
        FdType::Epoll(_) => {
            (-22, 0)
        }
        FdType::TimerFd(id) => {
            if count < 8 {
                return (-22, 0); // -EINVAL
            }
            let mut table = process::TIMERFD_TABLE.lock();
            let tf = match table.fds.get_mut(*id as usize) {
                Some(t) if t.in_use => t,
                _ => return (-9, 0),
            };
            let cur_ticks = crate::time::pit::ticks();
            let mut expirations = tf.expirations;
            if tf.expire_ticks > 0 && cur_ticks >= tf.expire_ticks {
                expirations += 1;
                if tf.interval_ticks > 0 {
                    let elapsed = cur_ticks - tf.expire_ticks;
                    let extra = elapsed / tf.interval_ticks;
                    expirations += extra;
                    tf.expire_ticks += (extra + 1) * tf.interval_ticks;
                } else {
                    tf.expire_ticks = 0;
                }
            }
            tf.expirations = 0;
            drop(table);

            if expirations == 0 {
                if (entry.flags & 0x800) != 0 {
                    (-11, 0) // -EAGAIN
                } else {
                    (0, 0)
                }
            } else {
                if paging::copy_to_user(buf, &expirations.to_ne_bytes()).is_ok() {
                    (8, 0)
                } else {
                    (-14, 0)
                }
            }
        }
        FdType::SignalFd(id) => {
            if count < core::mem::size_of::<SignalFdSigInfo>() {
                return (-22, 0); // -EINVAL
            }
            let table = process::SIGNALFD_TABLE.lock();
            let mask = match table.fds.get(*id as usize) {
                Some(s) if s.in_use => s.mask,
                _ => return (-9, 0),
            };
            drop(table);

            let pending_signo = process::current_sig_consume(mask);

            match pending_signo {
                Some(signo) => {
                    let mut info = SignalFdSigInfo::default();
                    info.ssi_signo = signo;
                    let info_bytes = unsafe {
                        core::slice::from_raw_parts(&info as *const SignalFdSigInfo as *const u8, core::mem::size_of::<SignalFdSigInfo>())
                    };
                    if paging::copy_to_user(buf, info_bytes).is_ok() {
                        (core::mem::size_of::<SignalFdSigInfo>() as i64, 0)
                    } else {
                        (-14, 0)
                    }
                }
                None => {
                    if (entry.flags & 0x800) != 0 {
                        (-11, 0) // -EAGAIN
                    } else {
                        (0, 0)
                    }
                }
            }
        }
        FdType::PtyMaster(id) => {
            let ptab = process::PTY_TABLE.lock();
            let master_pipe = ptab.ptys.get(*id as usize).map(|p| p.master_pipe);
            drop(ptab);
            match master_pipe {
                Some(pipe_id) => {
                    let mut kbuf = [0u8; 128];
                    let to_read = count.min(128);
                    let mut read_bytes = 0;
                    while read_bytes < to_read {
                        match process::pipe_read_byte(pipe_id) {
                            Some(b) => {
                                kbuf[read_bytes] = b;
                                read_bytes += 1;
                            }
                            None => break,
                        }
                    }
                    if read_bytes == 0 {
                        (0, 0)
                    } else if paging::copy_to_user(buf, &kbuf[..read_bytes]).is_ok() {
                        (read_bytes as i64, 0)
                    } else {
                        (-14, 0)
                    }
                }
                None => (-9, 0),
            }
        }
        FdType::PtySlave(id) => {
            let ptab = process::PTY_TABLE.lock();
            let slave_pipe = ptab.ptys.get(*id as usize).map(|p| p.slave_pipe);
            drop(ptab);
            match slave_pipe {
                Some(pipe_id) => {
                    let mut kbuf = [0u8; 128];
                    let to_read = count.min(128);
                    let mut read_bytes = 0;
                    while read_bytes < to_read {
                        match process::pipe_read_byte(pipe_id) {
                            Some(b) => {
                                kbuf[read_bytes] = b;
                                read_bytes += 1;
                            }
                            None => break,
                        }
                    }
                    if read_bytes == 0 {
                        (0, 0)
                    } else if paging::copy_to_user(buf, &kbuf[..read_bytes]).is_ok() {
                        (read_bytes as i64, 0)
                    } else {
                        (-14, 0)
                    }
                }
                None => (-9, 0),
            }
        }
        FdType::Inotify(id) => {
            // [CICLO 48] Lectura de eventos reales; bloquea (tope 10 s, EINTR)
            // salvo IN_NONBLOCK/O_NONBLOCK (-EAGAIN).
            let id = *id;
            let nonblock = (entry.flags & 0x800) != 0 || (process::inotify_flags(id) & 0x800) != 0;
            let tope = crate::time::pit::ticks().wrapping_add(TOPE_BLOQUEO_TICKS);
            while !process::inotify_has_events(id) {
                if nonblock || crate::time::pit::ticks() >= tope { return (-11, 0); }
                if crate::process::signals::current_sig_pending() != 0 { return (-4, 0); }
                unsafe { crate::process::scheduler::wait_hlt_preemptible(); }
            }
            let mut kbuf = [0u8; 1024];
            let n = count.min(1024);
            let r = process::inotify_read(id, &mut kbuf[..n]);
            if r < 0 { return (r, 0); }
            if paging::copy_to_user(buf, &kbuf[..r as usize]).is_err() { return (-14, 0); }
            (r, 0)
        }
        FdType::PidFd(_) | FdType::IoUring(_) | FdType::Mq(_) | FdType::Fanotify(_) => (-9, 0),
        FdType::ProcFile(info) => {
            let content = crate::drivers::storage::procfs::generate_content(info.file_type);
            if (info.pos as usize) >= content.len() {
                return (0, 0); // EOF
            }
            let remaining = &content[info.pos as usize..];
            let to_copy = remaining.len().min(count);
            if paging::copy_to_user(buf, &remaining[..to_copy]).is_err() {
                return (-14, 0);
            }
            if let Some(proc) = process::current_process() {
                if let Some(Some(ref mut fde)) = process::tabla_fds(proc).get_mut(fd as usize) {
                    if let process::FdType::ProcFile(ref mut pi) = fde.fd_type {
                        pi.pos += to_copy as u32;
                    }
                }
            }
            (to_copy as i64, 0)
        }
    }
}



fn sys_write_ramfs(fd: i32, buf: usize, count: usize, mut info: process::FsFileInfo) -> Result<i64, i32> {
    if fd < 0 || fd as usize >= process::MAX_FDS { return Err(0); }
    if count == 0 { return Ok(0); }
    // [T-002] NUNCA `vec![0; count]` con el tamaño que pida el usuario: un
    // `write(fd, buf, 9 MiB)` (APKINDEX de community) agotaba el heap del
    // núcleo y lo tiraba con "memory allocation of 9095663 bytes failed".
    // Se copia y escribe por trozos acotados; si un trozo falla tras haber
    // escrito algo, se devuelve la escritura corta (POSIX).
    const TROZO: usize = 256 * 1024;
    let path = core::str::from_utf8(&info.path[..info.path_len]).map_err(|_| 0)?;
    // [CICLO 51] escritura por el contrato `SistemaDeFicheros`
    let tam = |p: &str| crate::drivers::storage::vfs::con_fs(p, |fs| fs.metadatos(p).map(|m| m.tam).unwrap_or(0)).unwrap_or(0);
    if (info.flags & 0x0400) != 0 {
        // POSIX O_APPEND: Seek to end of file before writing
        info.pos = tam(path);
    }
    let mut ubuf = alloc::vec![0u8; count.min(TROZO)];
    let mut n = 0usize;
    while n < count {
        let k = (count - n).min(TROZO);
        if paging::copy_from_user(&mut ubuf[..k], buf + n, k).is_err() {
            if n > 0 { break; }
            return Err(0);
        }
        let pos = info.pos + n as u64;
        match crate::drivers::storage::vfs::con_fs(path, |fs| fs.escribir(path, pos, &ubuf[..k])) {
            Some(Ok(w)) => { n += w; if w < k { break; } }
            Some(Err(e)) => { if n > 0 { break; } return Err(e); }
            None => { if n > 0 { break; } return Err(0); }
        }
    }

    info.pos += n as u64;
    info.size = tam(path);

    if let Some(proc) = process::current_process() {
        if let Some(fd_entry) = process::fd_get_mut(proc, fd as usize) {
            if let FdType::FsFile(ref mut fi) = fd_entry.fd_type {
                fi.pos = info.pos;
                fi.size = info.size;
            }
        }
    }

    Ok(n as i64)
}

// ── SYS_READ stdin ───────────────────────────────────────────────────────
//
// REGLA CRÍTICA: NUNCA leer del puerto PS/2 (0x60/0x64) directamente aquí.
// IRQ1 es el único propietario de 0x60. Si sys_read_stdin roba bytes del
// hardware compite con irq1_handler, desincroniza el estado del KBD del
// kernel y genera break-codes huérfanos → teclas fantasma → #UD → #DF.
//
// El único canal legítimo es el ring buffer SCANCODE_BUF llenado por IRQ1.
// Para esperar input sin quemar CPU usamos el mecanismo de blocking:
//   1. Marcar el proceso como Blocked (set_stdin_blocked).
//   2. Forzar reschedule vía schedule_tick → el scheduler saltará a otro
//      proceso; IRQ1 llamará wake_stdin_blocked cuando llegue un scancode.
//   3. Cuando este proceso retome, el loop reintenta pop_scancode.
//
// El KBD estático local es necesario porque syscall.rs no tiene acceso al
// KeyboardState del loop principal. SOLO procesa scancodes del ring buffer
// de IRQ1 — nunca del hardware directamente.

fn sys_read_stdin(buf: usize, count: usize, current_rsp: u64) -> (i64, u64) {
    if count == 0 { return (0, 0); }
    // [CONTROL DE TRABAJOS] Un trabajo en segundo plano que lee la terminal
    // recibe SIGTTIN (y se detiene) en vez de robar las teclas del primer plano.
    if let Some(err) = crate::process::signals::tty_access_check(crate::process::signals::SIGTTIN) {
        return (err, 0);
    }

    // KBD local para decodificar scancodes del ring buffer de IRQ1.
    // No comparte estado con el KBD del kernel — correcto, son contextos
    // distintos (ring-3 stdin vs UI del kernel).
    struct StdinState {
        kbd: KeyboardState,
        buf: [u8; 256],
        head: usize,
        tail: usize,
    }

    static STDIN: Spinlock<StdinState> = Spinlock::new(StdinState {
        kbd: KeyboardState::new(),
        buf: [0u8; 256],
        head: 0,
        tail: 0,
    });

    unsafe {
        // ── Fase 1: bombear scancodes → chars hasta tener al menos uno ──
        loop {
            // Adquiere el lock UNA VEZ para drenar todos los scancodes,
            // protegiendo secuencias multi-byte (0xE0, 0xE1) contra
            // interleaving de otros procesos.
            {
                let mut guard = STDIN.lock();
                // Drain any available characters from serial COM1
                while let Some(b) = crate::process::tty_read_byte() {
                    let ch = if b == b'\r' { b'\n' } else { b };
                    let h = guard.head;
                    let next = (h + 1) % guard.buf.len();
                    if next != guard.tail {
                        guard.buf[h] = ch;
                        guard.head = next;
                    }
                }

                loop {
                    match pop_ring3_scancode() {
                        Some(sc) => {
                            if let Some(key) = guard.kbd.feed_byte(sc) {
                                let ch: u8 = match key {
                                    Key::Char(c)  => c,
                                    Key::Enter    => b'\n',
                                    Key::Backspace => 0x08,
                                    Key::Tab      => b'\t',
                                    _             => 0,
                                };
                                if ch != 0 {
                                    let h = guard.head;
                                    let next = (h + 1) % guard.buf.len();
                                    if next != guard.tail {
                                        guard.buf[h] = ch;
                                        guard.head = next;
                                    }
                                }
                            }
                        }
                        None => break,
                    }
                }

                if guard.head != guard.tail { break; }
            }

            // Sin caracteres: bloquear el proceso y forzar reschedule.
            // IRQ1 → irq1_handler_rust → wake_stdin_blocked nos despertará
            // cuando llegue el próximo scancode.
            if let Some(cur) = process::current_process() {
                set_stdin_blocked(cur.pid);
                cur.state = process::ProcessState::Blocked;
            }

            // Guardar RSP actual y hacer reschedule cooperativo.
            // schedule_tick devuelve el RSP del siguiente proceso (o 0 si
            // no hay nadie más). En ambos casos, cuando este proceso retome
            // continuará en el siguiente tick desde aquí.
            let cs = saved_cs_from_rsp(current_rsp);
            let new_rsp = process::schedule_desde_syscall(current_rsp, cs);
            if new_rsp != 0 {
                // Hay otro proceso listo: devolver (0, new_rsp) para que
                // int80_handler/syscall_entry haga el context switch.
                // El registro "bloqueado en stdin" se CONSERVA: es lo que
                // permite que IRQ1 o el bombeo serie del temporizador nos
                // despierten (antes se borraba aquí y un shell interactivo
                // con otro proceso listo dormía para siempre).
                return (0, new_rsp);
            }

            // No hay otros procesos: esperar con HLT (bajo consumo).
            // STI primero para que IRQ1 pueda despertar al proceso.
            // CLI inmediatamente después del HLT para volver a deshabilitar.
            process::wait_hlt_preemptible(); // [E3d] suelta el CGN mientras espera teclado
            clear_stdin_blocked();
            // El despertar (IRQ1 o bombeo serie) pudo dejarnos en `Ready`
            // mientras seguimos en la CPU: restaurar `Running` sobre el
            // proceso ACTUAL (por PID), no sobre "el que esté Running" —
            // antes quedaba en `Ready` y todo syscall posterior fallaba con
            // "sin proceso actual".
            let cur = process::current_raw();
            if !cur.is_null() { (*cur).state = process::ProcessState::Running; }
        }

        // ── Fase 2: copiar caracteres al buffer de usuario ───────────────
        let mut guard = STDIN.lock();
        let mut copied = 0usize;
        while guard.tail != guard.head && copied < count {
            let ch = guard.buf[guard.tail];
            guard.tail = (guard.tail + 1) % guard.buf.len();
            if paging::copy_to_user(buf + copied, &[ch]).is_err() { break; }
            copied += 1;
        }
        drop(guard);

        serial::write_str("[SYS] READ stdin chars=");
        serial::write_usize(copied);
        serial::write_str("\n");

        (copied as i64, 0)
    }
}

// ── SYS_OPEN ─────────────────────────────────────────────────────────────


/// [T-001] Registro único por arranque de que una ruta llegó a una syscall de
/// ficheros sin ningún sistema montado que la sirva (raíz ext2 ausente). La
/// FAT de arranque ya no actúa de rescate: el fallo es explícito (ENODEV).
static SIN_RAIZ_AVISADO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
pub(crate) fn sin_raiz(path: &str) {
    if !SIN_RAIZ_AVISADO.swap(true, core::sync::atomic::Ordering::Relaxed) {
        serial::write_str("[VFS] MODO DEGRADADO: ninguna raiz montada sirve '");
        serial::write_str(path);
        serial::write_str("' -> ENODEV (sin rescate FAT)\n");
    }
}

pub(crate) fn do_open_path(path: &str, flags: u32) -> i64 {
    let norm_path_str = crate::drivers::storage::vfs::normalize_path(path);
    let path = norm_path_str.as_str();
    let mut path_buf = [0u8; 256];
    let actual_len = path.len().min(255);
    path_buf[..actual_len].copy_from_slice(&path.as_bytes()[..actual_len]);

    // VFS routing: check if path belongs to devfs (/dev)
    if crate::drivers::storage::vfs::is_devfs_path(path) {
        return sys_open_devfs(path, &path_buf, actual_len);
    }

    // VFS routing: check if path belongs to procfs (/proc)
    if crate::drivers::storage::procfs::is_procfs_path(path) {
        return sys_open_procfs(path, flags);
    }

    // [CICLO 51] VFS: toda ruta servida por un sistema bajo el contrato
    // `SistemaDeFicheros` (ramfs, ext2...) pasa por la apertura genérica.
    if crate::drivers::storage::vfs::es_ruta_fs(path) {
        return sys_open_ramfs(path, &path_buf, actual_len, flags);
    }

    // [T-001] Sin raiz montada bajo el contrato no hay sistema de ficheros
    // que sirva esta ruta: ENODEV explicito (la FAT de rescate fue podada).
    sin_raiz(path);
    return -19;
}

fn sys_open(path_ptr: usize, flags: u32) -> i64 {
    if !verify_user_string(path_ptr, 256) {
        serial::write_str("[SYSCALL] sys_open: invalid user string\n");
        return -14;
    }
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_cstr_from_user(&mut path_buf, path_ptr) {
        Ok(n)   => n,
        Err(()) => return -14,
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let raw = alloc::string::String::from(core::str::from_utf8(&path_buf[..actual_len]).unwrap_or(""));
    if raw.is_empty() { return -2; }
    let n = resolver_con_cwd(&raw, &mut path_buf);
    let path = core::str::from_utf8(&path_buf[..n]).unwrap_or("");
    do_open_path(path, flags)
}

// ── RamFS helpers (Phase 10) ──────────────────────────────────────────

/// [CICLO 51] Apertura sobre cualquier `SistemaDeFicheros` (nombre histórico).
fn sys_open_ramfs(path: &str, path_buf: &[u8; 256], actual_len: usize, flags: u32) -> i64 {
    use crate::drivers::storage::vfs;
    let meta = vfs::con_fs(path, |fs| fs.metadatos(path)).unwrap_or(Err(crate::fs::errno::ENOENT));
    let is_dir = matches!(meta, Ok(m) if m.es_dir());
    if is_dir {
        if (flags & (0x0001 | 0x0002 | 0x0040 | 0x0200)) != 0 {
            serial::write_str("[RAMFS] OPEN: path is a directory (EISDIR)\n");
            return -21i64;
        }
        let proc = match process::current_process() {
            Some(p) => p,
            None => return -3i64,
        };
        let mut rpath = [0u8; 256];
        rpath[..actual_len].copy_from_slice(&path_buf[..actual_len]);
        let info = process::FsFileInfo {
            path: rpath,
            path_len: actual_len,
            size: 0,
            pos: 0,
            flags: 0,
        };
        let fd_entry = process::FdEntry::new(process::FdType::FsFile(info));
        return match process::fd_alloc(proc, fd_entry) {
            Some(fd) => fd as i64,
            None => -24i64,
        };
    }
    // [CICLO 48] FIFO con nombre: se abre como tuberia compartida, no como fichero.
    let es_fifo = matches!(meta, Ok(m) if m.modo & 0o170000 == 0o010000);
    if es_fifo {
        if (flags & 0x0040) != 0 && (flags & 0x0080) != 0 { return -17; } // O_CREAT|O_EXCL sobre FIFO existente
        let proc = match process::current_process() { Some(p) => p, None => return -9 };
        let tipo = match process::fifo_open(path, flags & 3, (flags & 0x800) != 0) { Ok(t) => t, Err(e) => return e as i64 };
        let mut entry = process::FdEntry::new(tipo);
        if (flags & 0x800) != 0 { entry.flags |= 0x800; }
        if (flags & 0x80000) != 0 { entry.flags |= 1; }
        return match process::fd_alloc(proc, entry) { Some(fd) => fd as i64, None => -24 };
    }
    let exists = meta.is_ok();
    if exists && (flags & 0x0040) != 0 && (flags & 0x0080) != 0 {
        // O_CREAT | O_EXCL specified on existing file returns -EEXIST
        serial::write_str("[RAMFS] OPEN: file exists with O_CREAT | O_EXCL (EEXIST)\n");
        return -17i64;
    }
    if !exists {
        if (flags & 0x0040) != 0 {
            // errno exacto del sistema de ficheros (ENOENT padre, ENOTDIR, ENOSPC)
            match vfs::con_fs(path, |fs| fs.crear(path, 0)) {
                Some(Ok(())) => {}
                Some(Err(e)) => return -(e as i64),
                None => return -2i64,
            }
        } else {
            // ENOENT es un resultado normal de open(2) (busquedas por PATH,
            // sondas de ficheros de configuracion, fuzzing): no se registra en
            // el puerto serie; el proceso ya recibe el errno exacto.
            return -2i64;
        }
    } else if (flags & 0x0200) != 0 && (flags & 0x0003) != 0 {
        if let Some(Err(e)) = vfs::con_fs(path, |fs| fs.truncar(path, 0)) { return -(e as i64); }
    }
    let size = vfs::con_fs(path, |fs| fs.metadatos(path).map(|m| m.tam).unwrap_or(0)).unwrap_or(0);
    let pos = if (flags & 0x0400) != 0 { size } else { 0 };
    let mut rpath = [0u8; 256];
    rpath[..actual_len].copy_from_slice(&path_buf[..actual_len]);
    let info = process::FsFileInfo {
        path: rpath,
        path_len: actual_len,
        size,
        pos,
        flags,
    };
    let fd_entry = process::FdEntry::new(process::FdType::FsFile(info));
    if let Some(proc) = process::current_process() {
        match process::fd_alloc(proc, fd_entry) {
            Some(fd) => {
                serial::write_str("[RAMFS] OPEN -> fd=");
                serial::write_usize(fd);
                serial::write_str(" path='");
                serial::write_str(path);
                serial::write_str("'\n");
                return fd as i64;
            }
            None => { serial::write_str("[RAMFS] OPEN: no free fd\n"); return -24; }
        }
    }
    -1
}

fn sys_read_ramfile(fd: i32, buf: usize, count: usize, info: &process::FsFileInfo) -> (i64, u64) {
    if fd < 0 || fd as usize >= process::MAX_FDS { return (-9, 0); }
    let path_str = core::str::from_utf8(&info.path[..info.path_len]).unwrap_or("/tmp/f");
    // [CICLO 51] lectura por el contrato `SistemaDeFicheros`
    let actual_size = crate::drivers::storage::vfs::con_fs(path_str, |fs| fs.metadatos(path_str).map(|m| m.tam).unwrap_or(info.size)).unwrap_or(info.size);
    if info.pos >= actual_size { return (0, 0); }
    // [T-002] lectura acotada a 1 MiB por llamada: el usuario reintenta
    // (lectura corta POSIX) y el heap del núcleo no se agota con `read(9 MiB)`.
    let to_read = (count as u64).min(actual_size - info.pos).min(1 << 20) as usize;
    if to_read == 0 { return (0, 0); }

    let mut kbuf = alloc::vec![0u8; to_read];

    let n = match crate::drivers::storage::vfs::con_fs(path_str, |fs| fs.leer(path_str, info.pos as u64, &mut kbuf)) {
        Some(Ok(n)) => n,
        Some(Err(e)) => return (-(e as i64), 0),
        None => return (-9, 0),
    };

    if n == 0 { return (0, 0); }
    if paging::copy_to_user(buf, &kbuf[..n]).is_err() { return (-14, 0); }

    if let Some(proc) = process::current_process() {
        if let Some(fd_entry) = process::fd_get_mut(proc, fd as usize) {
            if let process::FdType::FsFile(ref mut fi) = fd_entry.fd_type {
                fi.pos += n as u64;
                fi.size = actual_size;
            }
        }
    }

    serial::write_str("[RAMFS] READ fd=");
    serial::write_usize(fd as usize);
    serial::write_str(" bytes=");
    serial::write_usize(n);
    serial::write_str("\n");

    (n as i64, 0)
}

fn sys_getdents_ramfs(path: &str, buf: usize, count: usize) -> i64 {
    let mut entries: alloc::vec::Vec<([u8; 64], usize, bool)> = alloc::vec::Vec::new();
    // [CICLO 51] listado por el contrato `SistemaDeFicheros`
    match crate::drivers::storage::vfs::con_fs(path, |fs| fs.listar(path)) {
        Some(Ok(lista)) => for e in lista {
            let mut nb = [0u8; 64];
            let nl = e.nombre.len().min(63);
            nb[..nl].copy_from_slice(&e.nombre.as_bytes()[..nl]);
            entries.push((nb, nl, e.es_dir()));
        },
        Some(Err(e)) => return -(e as i64),
        None => return -2,
    }

    let mut total = 0usize;
    for (name_bytes, name_len, is_dir) in &entries {
        let d_reclen = 19 + name_len + 1;
        if total + d_reclen > count { break; }
        let mut hdr = [0u8; 512];
        let d_type: u8 = if *is_dir { 2u8 } else { 1u8 };
        hdr[0..8].copy_from_slice(&0u64.to_le_bytes());
        hdr[8..16].copy_from_slice(&(total as u64).to_le_bytes());
        hdr[16..18].copy_from_slice(&(d_reclen as u16).to_le_bytes());
        hdr[18] = d_type;
        hdr[19..19 + name_len].copy_from_slice(&name_bytes[..*name_len]);
        if paging::copy_to_user(buf + total, &hdr[..d_reclen.min(512)]).is_err() { break; }
        total += d_reclen;
    }

    serial::write_str("[RAMFS] GETDIRENTS path='");
    serial::write_str(path);
    serial::write_str("' entries=");
    serial::write_usize(entries.len());
    serial::write_str(" bytes=");
    serial::write_usize(total);
    serial::write_str("\n");

    total as i64
}

fn sys_getdents_devfs(path: &str, buf: usize, count: usize) -> i64 {
    let _ = path;
    let devs = crate::drivers::storage::vfs::DEVFS_ENTRIES;
    let mut total = 0usize;
    for name in devs {
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len();
        let d_reclen = 19 + name_len + 1;
        if total + d_reclen > count { break; }
        let mut hdr = [0u8; 512];
        let d_type: u8 = 1u8; // DT_FILE
        hdr[0..8].copy_from_slice(&0u64.to_le_bytes());
        hdr[8..16].copy_from_slice(&(total as u64).to_le_bytes());
        hdr[16..18].copy_from_slice(&(d_reclen as u16).to_le_bytes());
        hdr[18] = d_type;
        hdr[19..19 + name_len].copy_from_slice(name_bytes);
        if paging::copy_to_user(buf + total, &hdr[..d_reclen.min(512)]).is_err() { break; }
        total += d_reclen;
    }

    serial::write_str("[DEVFS] GETDIRENTS entries=");
    serial::write_usize(devs.len());
    serial::write_str(" bytes=");
    serial::write_usize(total);
    serial::write_str("\n");

    total as i64
}

fn sys_getdents_procfs(path: &str, buf: usize, count: usize) -> i64 {
    let entries = crate::drivers::storage::procfs::list_proc_dir(path);
    if entries.is_empty() {
        return -2; // -ENOENT
    }
    let mut total = 0usize;
    for (name, is_dir) in &entries {
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len();
        let d_reclen = 19 + name_len + 1;
        if total + d_reclen > count { break; }
        let mut hdr = [0u8; 512];
        let d_type: u8 = if *is_dir { 2u8 } else { 1u8 };
        hdr[0..8].copy_from_slice(&0u64.to_le_bytes());
        hdr[8..16].copy_from_slice(&(total as u64).to_le_bytes());
        hdr[16..18].copy_from_slice(&(d_reclen as u16).to_le_bytes());
        hdr[18] = d_type;
        hdr[19..19 + name_len].copy_from_slice(name_bytes);
        if paging::copy_to_user(buf + total, &hdr[..d_reclen.min(512)]).is_err() { break; }
        total += d_reclen;
    }

    serial::write_str("[PROCFS] GETDIRENTS path='");
    serial::write_str(path);
    serial::write_str("' entries=");
    serial::write_usize(entries.len());
    serial::write_str(" bytes=");
    serial::write_usize(total);
    serial::write_str("\n");

    total as i64
}

fn sys_open_procfs(path: &str, flags: u32) -> i64 {
    use crate::drivers::storage::procfs;
    // O_WRONLY = 0x0001, O_RDWR = 0x0002, O_CREAT = 0x0040, O_TRUNC = 0x0200
    let (ft, _is_dir) = match procfs::resolve_proc_path(path) {
        Some(x) => x,
        None => return -2, // -ENOENT
    };
    // [ÉPICA 59] Los nodos de control de la suite de desarrollo admiten
    // escritura (trazador y profiler); el resto de /proc es de solo lectura.
    let writable_node = matches!(
        ft,
        process::ProcFileType::SyscallTrace
            | process::ProcFileType::CpuProfiler
            | process::ProcFileType::DebugChannel
    );
    if !writable_node && (flags & (0x0001 | 0x0002 | 0x0040 | 0x0200)) != 0 {
        return -30; // -EROFS
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    let info = process::ProcFileInfo {
        file_type: ft,
        pos: 0,
    };
    let fd_entry = process::FdEntry::new(process::FdType::ProcFile(info));
    match process::fd_alloc(proc, fd_entry) {
        Some(fd) => {
            serial::write_str("[PROCFS] OPEN '");
            serial::write_str(path);
            serial::write_str("' -> fd=");
            serial::write_usize(fd);
            serial::write_str("\n");
            fd as i64
        }
        None => -24, // -EMFILE
    }
}

// ── DevFS helpers (Phase 12) ──────────────────────────────────────────

fn sys_open_devfs(path: &str, _path_buf: &[u8; 256], _actual_len: usize) -> i64 {
    let name = match crate::drivers::storage::vfs::resolve_devfs(path) {
        Some(n) => n,
        None => {
            serial::write_str("[DEVFS] no such device: '");
            serial::write_str(path);
            serial::write_str("'\n");
            return -2;
        }
    };

    if name == "ptmx" {
        return sys_posix_openpt(0x0002); // O_RDWR
    }
    if name == "pts" {
        let num_str = if path.starts_with("/dev/pts/") {
            &path[9..]
        } else {
            ""
        };
        let pty_id = match num_str.parse::<u32>() {
            Ok(id) if (id as usize) < process::MAX_PTYS => id,
            _ => return -2,
        };
        let ptable = process::PTY_TABLE.lock();
        if !ptable.ptys[pty_id as usize].in_use {
            return -2;
        }
        drop(ptable);
        let proc = match process::current_process() {
            Some(p) => p,
            None => return -3,
        };
        process::pty_retain(pty_id);
        let fd_entry = process::FdEntry::new(process::FdType::PtySlave(pty_id));
        return match process::fd_alloc(proc, fd_entry) {
            Some(fd) => fd as i64,
            None => {
                process::pty_close(pty_id);
                -1
            }
        };
    }

    let dev_type = match name {
        "kbd" => process::DeviceType::Kbd,
        "fb0" => process::DeviceType::Fb,
        "sda0" => process::DeviceType::Sda,
        "null" => process::DeviceType::Null,
        "zero" => process::DeviceType::Zero,
        "urandom" => process::DeviceType::Urandom,
        "dsp" | "audio" => process::DeviceType::Dsp,
        _ => {
            serial::write_str("[DEVFS] unknown device: ");
            serial::write_str(name);
            serial::write_str("\n");
            return -2;
        }
    };

    let info = process::DeviceInfo { dev_type, pos: 0 };
    let fd_entry = process::FdEntry::new(process::FdType::Device(info));
    if let Some(proc) = process::current_process() {
        match process::fd_alloc(proc, fd_entry) {
            Some(fd) => {
                serial::write_str("[DEVFS] OPEN '");
                serial::write_str(path);
                serial::write_str("' -> fd=");
                serial::write_usize(fd);
                serial::write_str("\n");
                return fd as i64;
            }
            None => {
                serial::write_str("[DEVFS] OPEN: no free fd\n");
                return -24;
            }
        }
    }
    -1
}

// ── SYS_CLOSE ────────────────────────────────────────────────────────────

fn sys_close(fd: i32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; }
    if let Some(mut proc) = process::current_process() {
        if process::tabla_fds(proc)[fd as usize].is_none() {
            serial::write_str("[SYS] CLOSE: fd not open (EBADF)\n");
            return -9;
        }
        process::fd_close(&mut proc, fd as usize);
        serial::write_str("[SYS] CLOSE fd=");
        serial::write_usize(fd as usize);
        serial::write_str("\n");
        0
    } else {
        -1
    }
}

// ── SYS_DUP2 ─────────────────────────────────────────────────────────────

fn sys_dup2(oldfd: i32, newfd: i32) -> i64 {
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    if oldfd < 0 || oldfd as usize >= process::MAX_FDS { return -9; }
    if newfd < 0 || newfd as usize >= process::MAX_FDS { return -9; }

    let mut src = match process::fd_get(proc, oldfd as usize) {
        Some(e) => process::fd_clone_entry(e),
        None => return -9, // oldfd is not an open file descriptor
    };
    src.flags &= !1; // [defecto 33] POSIX: dup2 no hereda FD_CLOEXEC

    if oldfd == newfd { return newfd as i64; }

    // Close newfd cleanly first if already open to prevent resource/pipe leaks
    process::fd_close(proc, newfd as usize);

    let _ = process::fd_alloc_at(proc, newfd as usize, src);

    serial::write_str("[SYS] DUP2 oldfd=");
    serial::write_usize(oldfd as usize);
    serial::write_str(" newfd=");
    serial::write_usize(newfd as usize);
    serial::write_str("\n");

    newfd as i64
}

fn sys_dup(oldfd: i32) -> i64 {
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    if oldfd < 0 || oldfd as usize >= process::MAX_FDS { return -9; }

    let mut src = match process::fd_get(proc, oldfd as usize) {
        Some(e) => process::fd_clone_entry(e),
        None => return -9,
    };
    src.flags &= !1; // [defecto 33] POSIX: dup no hereda FD_CLOEXEC

    match process::fd_alloc(proc, src) {
        Some(newfd) => {
            serial::write_str("[SYS] DUP oldfd=");
            serial::write_usize(oldfd as usize);
            serial::write_str(" newfd=");
            serial::write_usize(newfd);
            serial::write_str("\n");
            newfd as i64
        }
        None => -24,
    }
}

// ── SYS_FCNTL ────────────────────────────────────────────────────────────

pub const F_DUPFD: i32 = 0;
pub const F_GETFD: i32 = 1;
pub const F_SETFD: i32 = 2;
pub const F_GETFL: i32 = 3;
pub const F_SETFL: i32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Flock {
    pub l_type: i16,   // F_RDLCK=0, F_WRLCK=1, F_UNLCK=2
    pub l_whence: i16, // SEEK_SET=0, SEEK_CUR=1, SEEK_END=2
    pub l_start: i64,
    pub l_len: i64,
    pub l_pid: i32,
}

pub static FILE_LOCKS: crate::arch::Spinlock<[Option<(i32, i16, i64, i64, i32)>; 32]> = crate::arch::Spinlock::new([None; 32]);

fn sys_fcntl(fd: i32, cmd: i32, arg: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };

    match cmd {
        0 | 1030 => { // F_DUPFD / F_DUPFD_CLOEXEC: duplicar al menor fd libre >= arg
            // [CICLO 49 / T1] 1030 = F_DUPFD_CLOEXEC: lo usa ash (busybox) para
            // mover el guion a un fd >= 10 con close-on-exec. Faltaba -> EPERM.
            let min_fd = arg;
            if min_fd >= process::MAX_FDS { return -24; } // -EMFILE
            let mut entry = match process::fd_get(proc, fd as usize) {
                Some(e) => process::fd_clone_entry(e),
                None => return -9, // -EBADF
            };
            if cmd == 1030 { entry.flags |= 1; } else { entry.flags &= !1; }
            for i in min_fd..process::MAX_FDS {
                if process::tabla_fds(proc)[i].is_none() {
                    process::tabla_fds(proc)[i] = Some(entry);
                    serial::write_str("[SYS] FCNTL F_DUPFD fd=");
                    serial::write_usize(fd as usize);
                    serial::write_str(" -> newfd=");
                    serial::write_usize(i);
                    serial::write_str("\n");
                    return i as i64;
                }
            }
            -1 // EMFILE
        }
        1 => { // F_GETFD: Get descriptor flags (e.g. FD_CLOEXEC)
            match process::fd_get(proc, fd as usize) {
                Some(e) => (e.flags & 1) as i64,
                None => -9,
            }
        }
        2 => { // F_SETFD: Set descriptor flags
            match process::fd_get_mut(proc, fd as usize) {
                Some(e) => {
                    e.flags = (arg as u32) & 1;
                    0
                }
                None => -9,
            }
        }
        3 => { // F_GETFL: Get file status flags
            match process::fd_get(proc, fd as usize) {
                Some(e) => match &e.fd_type {
                    FdType::FsFile(info) => info.flags as i64,
                    FdType::PipeRead(_) | FdType::Stdin => 0, // O_RDONLY
                    FdType::PipeWrite(_) | FdType::Stdout | FdType::Stderr => 1, // O_WRONLY
                    FdType::Socket(_) | FdType::SocketPairEnd { .. } => 2, // O_RDWR
                    _ => 0,
                },
                None => -9,
            }
        }
        4 => { // F_SETFL: Set file status flags
            match process::fd_get_mut(proc, fd as usize) {
                Some(e) => match &mut e.fd_type {
                    FdType::FsFile(ref mut info) => {
                        info.flags = arg as u32;
                        0
                    }
                    _ => 0,
                },
                None => -9,
            }
        }
        5 => { // F_GETLK: Query advisory file lock
            if arg == 0 || !verify_user_buffer(arg, core::mem::size_of::<Flock>()) {
                return -14;
            }
            let mut fl = Flock::default();
            let fl_bytes = unsafe {
                core::slice::from_raw_parts_mut(&mut fl as *mut Flock as *mut u8, core::mem::size_of::<Flock>())
            };
            if paging::copy_from_user(fl_bytes, arg, core::mem::size_of::<Flock>()).is_err() {
                return -14;
            }
            let locks = FILE_LOCKS.lock();
            let mut conflict = false;
            for lock in locks.iter().flatten() {
                if lock.0 == fd && lock.4 != proc.pid as i32 {
                    if fl.l_type == 1 || lock.1 == 1 {
                        fl.l_type = lock.1;
                        fl.l_pid = lock.4;
                        conflict = true;
                        break;
                    }
                }
            }
            if !conflict {
                fl.l_type = 2; // F_UNLCK
            }
            let out_bytes = unsafe {
                core::slice::from_raw_parts(&fl as *const Flock as *const u8, core::mem::size_of::<Flock>())
            };
            let _ = paging::copy_to_user(arg, out_bytes);
            0
        }
        6 | 7 => { // F_SETLK, F_SETLKW: Set or clear advisory file lock
            if arg == 0 || !verify_user_buffer(arg, core::mem::size_of::<Flock>()) {
                return -14;
            }
            let mut fl = Flock::default();
            let fl_bytes = unsafe {
                core::slice::from_raw_parts_mut(&mut fl as *mut Flock as *mut u8, core::mem::size_of::<Flock>())
            };
            if paging::copy_from_user(fl_bytes, arg, core::mem::size_of::<Flock>()).is_err() {
                return -14;
            }
            if fl.l_type < 0 || fl.l_type > 2 {
                return -22;
            }
            let mut locks = FILE_LOCKS.lock();
            if fl.l_type == 2 { // F_UNLCK
                for lock in locks.iter_mut() {
                    if let Some(l) = lock {
                        if l.0 == fd && l.4 == proc.pid as i32 {
                            *lock = None;
                        }
                    }
                }
                return 0;
            }
            for lock in locks.iter().flatten() {
                if lock.0 == fd && lock.4 != proc.pid as i32 {
                    if fl.l_type == 1 || lock.1 == 1 {
                        return -11; // -EAGAIN
                    }
                }
            }
            for lock in locks.iter_mut() {
                if lock.is_none() {
                    *lock = Some((fd, fl.l_type, fl.l_start, fl.l_len, proc.pid as i32));
                    break;
                }
            }
            0
        }
        _ => -22, // EINVAL
    }
}

// ── SYS_GETPPID ──────────────────────────────────────────────────────────

fn sys_getppid() -> u64 {
    process::proceso_memoria().map(|p| p.ppid).unwrap_or(0)
}

// ── SYS_UNAME ────────────────────────────────────────────────────────────

#[repr(C)]
struct KernelUtsName {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

fn sys_uname(buf_ptr: usize) -> i64 {
    let size = core::mem::size_of::<KernelUtsName>();
    if !verify_user_buffer(buf_ptr, size) {
        return -14;
    }

    let mut uts = KernelUtsName {
        sysname: [0; 65],
        nodename: [0; 65],
        release: [0; 65],
        version: [0; 65],
        machine: [0; 65],
        domainname: [0; 65],
    };

    let copy_str = |dst: &mut [u8; 65], src: &str| {
        let b = src.as_bytes();
        let len = b.len().min(64);
        dst[..len].copy_from_slice(&b[..len]);
        dst[len] = 0;
    };

    copy_str(&mut uts.sysname, "Portix");
    {
        let h = HOSTNAME.lock();
        let h_len = h.iter().position(|&b| b == 0).unwrap_or(64);
        uts.nodename[..h_len].copy_from_slice(&h[..h_len]);
        uts.nodename[h_len] = 0;
    }
    copy_str(&mut uts.release, crate::version::PORTIX_VERSION);
    copy_str(&mut uts.version, "Portix x86_64 SMP (100% Rust Bare-Metal)");
    copy_str(&mut uts.machine, "x86_64");
    copy_str(&mut uts.domainname, "localdomain");

    let bytes = unsafe {
        core::slice::from_raw_parts(&uts as *const KernelUtsName as *const u8, size)
    };

    if paging::copy_to_user(buf_ptr, bytes).is_ok() {
        0
    } else {
        -1
    }
}

// ── SYS_UPTIME ───────────────────────────────────────────────────────────

fn sys_uptime() -> u64 {
    let ticks = crate::time::pit::ticks();
    serial::write_str("[SYS] UPTIME ticks=");
    serial::write_usize(ticks as usize);
    serial::write_str("\n");
    ticks
}

// ── SYS_BRK ──────────────────────────────────────────────────────────────

fn sys_brk(addr: usize) -> i64 {
    let proc = match process::proceso_memoria() {
        Some(p) => p,
        None    => return -3,
    };
    if addr == 0 { return proc.program_break as i64; }
    // Enforce bounds: heap cannot grow into device space or kernel space
    if addr < process::PROGRAM_BREAK_BASE || addr > 0x3F00_0000_0000 {
        return proc.program_break as i64;
    }

    let cr3         = proc.cr3;
    let current_end = proc.program_break_end;
    let new_brk     = addr.max(process::PROGRAM_BREAK_BASE);

    if new_brk > current_end {
        let start_page = (current_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let end_page   = (new_brk   + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let pages      = (end_page - start_page) / PAGE_SIZE;
        for i in 0..pages {
            let vaddr = start_page + i * PAGE_SIZE;
            let paddr = match alloc_page() {
                Some(p) => p,
                None    => { serial::write_str("[SYS] BRK: OOM\n"); return proc.program_break as i64; }
            };
            if paging::map_page_user(cr3, vaddr, paddr).is_err() {
                return proc.program_break as i64;
            }
        }
        proc.program_break_end = end_page;
    } else if new_brk < current_end {
        let unmap_start = (new_brk + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let unmap_end = (current_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let mut vaddr = unmap_start;
        while vaddr < unmap_end {
            let _ = paging::unmap_page(cr3, vaddr);
            vaddr += PAGE_SIZE;
        }
        proc.program_break_end = unmap_start;
    } else if new_brk < process::PROGRAM_BREAK_BASE {
        return -12;
    }

    proc.program_break = new_brk;
    // [CIERRE B-3] Sin traza serial por cada `brk`: a 115200 baudios costaba
    // más que la propia syscall y era la mayor parte de los ~45 730 ciclos de
    // `alloc+free` medidos por portix-bench. Las ampliaciones reales de la
    // arena (nuevas páginas) sí se registran para auditoría.
    if new_brk > current_end {
        serial::write_str("[SYS] BRK arena → ");
        serial::write_hex(proc.program_break_end);
        serial::write_str("\n");
    }

    new_brk as i64
}

// ── SYS_MMAP & Shared Memory Structures ───────────────────────────────────

#[derive(Clone)]
pub struct SharedMmapEntry {
    pub pid: u64,
    pub vaddr: usize,
    pub len: usize,
    pub fd: i32,
    pub flags: u32,
    pub path: [u8; 256],
    pub path_len: usize,
    pub is_ramfile: bool,
}

static SHARED_MMAPS: crate::arch::Spinlock<alloc::vec::Vec<SharedMmapEntry>> =
    crate::arch::Spinlock::new(alloc::vec::Vec::new());

/// [CICLO 50 / T1b] Lee `kbuf.len()` bytes de un descriptor de fichero regular
/// (RamFS o FAT32) a partir de `offset`, sin tocar la posición del descriptor.
/// Es el lector que usa `mmap` para los mapeos respaldados por fichero: los
/// cargadores dinámicos (ld-musl) mapean cada PT_LOAD con su propio offset.
fn leer_fd_en(entry: &process::FdEntry, offset: usize, kbuf: &mut [u8]) -> Result<usize, i64> {
    match &entry.fd_type {
        process::FdType::FsFile(info) => {
            let path_str = core::str::from_utf8(&info.path[..info.path_len]).map_err(|_| -22i64)?;
            crate::drivers::storage::vfs::leer_fs(path_str, offset as u64, kbuf)
                .map_err(|_| -5i64)
        }
        _ => Err(-19), // -ENODEV: no es un fichero mapeable
    }
}

/// `mmap(addr, len, prot, flags, fd, offset)` con semántica Linux suficiente
/// para cargadores dinámicos:
///  * `MAP_FIXED` (0x10) sustituye lo que hubiera en el rango: las páginas ya
///    presentes se conservan físicamente y se ponen a cero (contenido nuevo),
///    las ausentes se asignan. Así `ld-musl` puede mapear PT_LOAD encima de la
///    reserva PROT_NONE inicial y el .bss anónimo sobre la cola del fichero.
///  * `offset` debe ser múltiplo de página (EINVAL si no) y se respeta de
///    verdad: se copia `fichero[offset .. offset+len]` al rango mapeado.
///  * `prot` se acepta (EINVAL si tiene bits desconocidos) y SE APLICA (T1b.2):
///    las páginas se crean RW+NX para volcar el contenido y al final se ajustan
///    con `mprotect_range(prot)`: PROT_EXEC quita NX, sin PROT_WRITE se retira
///    W. Limitación conocida: PROT_NONE queda legible (no escribible, no
///    ejecutable) porque la PTE sigue presente.
/// [T-026] Perfil (ciclos TSC acumulados) del bucle de páginas de `sys_mmap`:
/// 0 pte_cruda/unmap, 1 translate+mprotect, 2 copy_to_user(cero), 3 translate+alloc_page,
/// 4 map_page_user, 5 nº páginas sustituidas, 6 nº páginas nuevas.
#[allow(clippy::declare_interior_mutable_const)]
const CERO_U64: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static MMAP_PERFIL: [core::sync::atomic::AtomicU64; 7] = [CERO_U64; 7];

fn sys_mmap(addr: usize, len: usize, prot: u32, flags: u32, fd: i32, offset: i64) -> i64 {
    const MAP_SHARED: u32 = 0x01;
    const MAP_FIXED: u32 = 0x10;
    const MAP_ANONYMOUS: u32 = 0x20;
    const MAP_POPULATE: u32 = 0x8000;
    // [b191 / T-014] Hasta 2 GiB por mapeo: las reservas anónimas son perezosas (sólo
    // tablas de páginas: 2 GiB = 8 MiB de tablas); ENOMEM por encima, como Linux sin RAM.
    if len == 0 { return -22; }
    if len > 0x8000_0000 { return -12; }
    if (prot & !0x7) != 0 { return -22; }
    if offset < 0 || (offset as usize) % PAGE_SIZE != 0 { return -22; }
    let fixed = (flags & MAP_FIXED) != 0;
    if fixed && addr == 0 { return -22; }
    let con_fichero = (flags & MAP_ANONYMOUS) == 0;
    if con_fichero && fd < 0 { return -9; }

    let proc = match process::proceso_memoria() {
        Some(p) => p,
        None    => return -3,
    };

    // El descriptor se valida ANTES de tocar el espacio de direcciones.
    let fd_entry = if con_fichero {
        if (fd as usize) >= process::MAX_FDS { return -9; }
        match process::fd_get(&proc, fd as usize) {
            Some(e) => Some(e.clone()),
            None => return -9,
        }
    } else { None };
    if let Some(e) = &fd_entry {
        match &e.fd_type {
            process::FdType::FsFile(_) => {}
            _ => return -19, // -ENODEV
        }
    }

    let pages = (len + PAGE_SIZE - 1) / PAGE_SIZE;
    let cr3 = proc.cr3;
    let mut desde_base = addr == 0;
    if !desde_base {
        let a = addr & !(PAGE_SIZE - 1);
        if fixed && a != addr { return -22; } // MAP_FIXED exige alineación exacta
        // [Fase B.4] El umbral bajo ya no es 0x20000000 (origen histórico del
        // userland Rust): el único límite inferior es la zona de arranque
        // 0–0x200000 (`ZONAS_IDENTIDAD_RESERVADAS`), como `mmap_min_addr`.
        if a < 0x20_0000 || a.checked_add(pages * PAGE_SIZE).map_or(true, |end| end >= 0x0000_8000_0000_0000) {
            if fixed { return -22; }
            desde_base = true; // pista inválida: se ignora (semántica Linux)
        } else if !paging::rango_usuario_permitido(a, a + pages * PAGE_SIZE) {
            // [CICLO 49 / Fase A] ni sobre la ventana MMIO/LFB ni sobre el back buffer
            if fixed { return -22; }
            desde_base = true;
        } else if !fixed && (0..pages).any(|i| paging::pagina_del_proceso(cr3, a + i * PAGE_SIZE)) {
            // Pista sobre un rango ya ocupado: sin MAP_FIXED, Linux elige otra
            // dirección en vez de pisar el mapeo existente.
            desde_base = true;
        }
    }
    let aligned_addr = if desde_base {
        let base = proc.mmap_base;
        if base.checked_add(pages * PAGE_SIZE).map_or(true, |end| end >= 0x0000_8000_0000_0000) {
            return -12;
        }
        proc.mmap_base += pages * PAGE_SIZE;
        base
    } else {
        addr & !(PAGE_SIZE - 1)
    };

    // [b191 / T-014] Anónimo privado sin MAP_POPULATE: reserva perezosa, sin marcos.
    // Un mapeo presente previo (MAP_FIXED) se libera; una reserva previa se sustituye.
    // [T-014b] También los mapeos PRIVADOS de fichero: el contenido se lee en el #PF
    // (`mem::vmas`). MAP_SHARED sigue siendo ansioso (tiene escritura de vuelta).
    // MAP_SHARED de sólo lectura sobre fichero (la JVM mapea así `modules`, 134 MB):
    // también perezoso, nada que escribir de vuelta.
    let compartido_ro = (flags & MAP_SHARED) != 0 && con_fichero && (prot & 2) == 0;
    let perezoso = ((flags & MAP_SHARED) == 0 || compartido_ro) && (flags & MAP_POPULATE) == 0;
    if perezoso {
        crate::mem::vmas::recortar(cr3, aligned_addr, aligned_addr + pages * PAGE_SIZE);
        if let Some(e) = &fd_entry {
            if let process::FdType::FsFile(info) = &e.fd_type {
                crate::mem::vmas::registrar(cr3, aligned_addr, aligned_addr + pages * PAGE_SIZE, &info.path[..info.path_len], offset as u64, compartido_ro);
            }
        }
        for i in 0..pages {
            let vaddr = aligned_addr + i * PAGE_SIZE;
            // [T-026] La JVM reserva su montón de golpe (cientos de miles de
            // páginas PROT_NONE): 1,1 s con IF=0 en este bucle. Sin lote de TLB
            // aquí: se liberan marcos y otro núcleo no debe conservar una
            // traducción vieja a un marco ya libre.
            if i & 0xFF == 0xFF { paging::respirar(); }
            if let Some((paddr, f)) = paging::translate(cr3, vaddr) {
                if f & paging::USER != 0 {
                    if paging::is_dynamic_addr(paddr) { paging::liberar_marco_usuario(paddr); }
                    let _ = paging::unmap_page(cr3, vaddr);
                }
            }
            if paging::mapear_perezosa(cr3, vaddr, prot & 7).is_err() { return -12; }
        }
        serial::write_str("[SYS] MMAP perezoso addr=");
        serial::write_hex(aligned_addr);
        serial::write_str(" pages=");
        serial::write_usize(pages);
        serial::write_str(" prot=");
        serial::write_usize(prot as usize);
        if con_fichero { serial::write_str(" fd="); serial::write_usize(fd as usize); serial::write_str(" off="); serial::write_hex(offset as usize); }
        serial::write_str("\n");
        return aligned_addr as i64;
    }

    let cero = [0u8; PAGE_SIZE];
    // [T-026] Un solo shootdown para todo el lote (los `return` de error de
    // este bucle terminan la syscall: `fin_lote_tlb` lo cierra en cada uno).
    paging::inicio_lote_tlb();
    for i in 0..pages {
        let vaddr = aligned_addr + i * PAGE_SIZE;
        // [T-026] Al PRINCIPIO del cuerpo, para que cubra también la rama
        // MAP_FIXED sobre páginas ya mapeadas (`continue` más abajo): la JVM
        // mapea `modules` (34 000 páginas) sobre su reserva previa y cada
        // página costaba un shootdown con acuse (1,4 s seguidos con IF=0).
        if i & 0xFF == 0xFF { paging::respirar(); }
        // [Fase B.4] Sólo cuenta como "ya mapeado" un marco de USUARIO: la
        // identidad plana del núcleo (huge pages sin bit USER) que cubre
        // 0x200000–0x5300000 no es un mapeo del proceso; `map_page` la divide
        // en la copia privada de la tabla y superpone la página del usuario.
        let t0 = unsafe { core::arch::x86_64::_rdtsc() };
        if paging::pte_cruda(cr3, vaddr).map_or(false, paging::es_perezosa) {
            // [b191] Reserva perezosa previa bajo un mapeo de fichero: se sustituye por marco.
            let _ = paging::unmap_page(cr3, vaddr);
        }
        let t1 = unsafe { core::arch::x86_64::_rdtsc() };
        MMAP_PERFIL[0].fetch_add(t1 - t0, core::sync::atomic::Ordering::Relaxed);
        if paging::translate(cr3, vaddr).map_or(false, |(_, f)| f & paging::USER != 0) {
            // Sólo posible con MAP_FIXED: el mapeo nuevo arranca a cero sobre
            // el marco existente (sustitución del contenido anterior).
            // [T1b.2] El marco puede venir de una reserva PROT_NONE/RO: se abre a
            // RW para volcar el contenido; el `prot` definitivo se aplica al final.
            if paging::mprotect_page(cr3, vaddr, 3).is_err() { paging::fin_lote_tlb(); return -12; }
            let t2 = unsafe { core::arch::x86_64::_rdtsc() };
            MMAP_PERFIL[1].fetch_add(t2 - t1, core::sync::atomic::Ordering::Relaxed);
            if paging::copy_to_user(vaddr, &cero).is_err() { paging::fin_lote_tlb(); return -14; }
            let t3 = unsafe { core::arch::x86_64::_rdtsc() };
            MMAP_PERFIL[2].fetch_add(t3 - t2, core::sync::atomic::Ordering::Relaxed);
            MMAP_PERFIL[5].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            continue;
        }
        let paddr = match alloc_page() {
            Some(p) => p,
            None    => { paging::fin_lote_tlb(); return -12; }
        };
        let t2 = unsafe { core::arch::x86_64::_rdtsc() };
        MMAP_PERFIL[3].fetch_add(t2 - t1, core::sync::atomic::Ordering::Relaxed);
        if paging::map_page_user(cr3, vaddr, paddr).is_err() { paging::fin_lote_tlb(); return -12; }
        let t3 = unsafe { core::arch::x86_64::_rdtsc() };
        MMAP_PERFIL[4].fetch_add(t3 - t2, core::sync::atomic::Ordering::Relaxed);
        MMAP_PERFIL[6].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
    paging::fin_lote_tlb();

    // Mapeo respaldado por fichero: se copia fichero[offset .. offset+len].
    if let Some(entry) = &fd_entry {
        // [T-004] Por trozos de 256 KiB: `libcrypto.so.3` de Debian mapea 6,5 MiB de
        // golpe y `vec![0u8; len]` reventaba el montículo del núcleo (KERNEL PANIC
        // "memory allocation of 6528040 bytes failed" al arrancar `sha256sum`).
        const TROZO: usize = 256 * 1024;
        let mut temp = alloc::vec![0u8; TROZO.min(len)];
        let mut hecho = 0usize;
        while hecho < len {
            let n_pedir = TROZO.min(len - hecho);
            match leer_fd_en(entry, offset as usize + hecho, &mut temp[..n_pedir]) {
                Ok(0) => break, // fin de fichero: el resto queda a cero (.bss sobre la cola del fichero)
                Ok(n) => {
                    if paging::copy_to_user(aligned_addr + hecho, &temp[..n]).is_err() { return -14; }
                    hecho += n;
                    paging::respirar(); // [T-026] 256 KiB copiados: pasan las IRQ pendientes
                    if n < n_pedir { break; }
                }
                Err(e) => return e,
            }
        }
        if (flags & MAP_SHARED) != 0 {
            if let process::FdType::FsFile(info) = &entry.fd_type {
                let sm = SharedMmapEntry {
                    pid: proc.pid,
                    vaddr: aligned_addr,
                    len,
                    fd,
                    flags,
                    path: info.path,
                    path_len: info.path_len,
                    is_ramfile: true,
                };
                SHARED_MMAPS.lock().push(sm);
            }
        }
    } else if (flags & MAP_SHARED) != 0 {
        // MAP_SHARED anónimo
        let sm = SharedMmapEntry {
            pid: proc.pid,
            vaddr: aligned_addr,
            len,
            fd: -1,
            flags,
            path: [0u8; 256],
            path_len: 0,
            is_ramfile: false,
        };
        SHARED_MMAPS.lock().push(sm);
    }

    // [T1b.2] Se aplica el `prot` pedido de verdad: PROT_EXEC quita NX; sin
    // PROT_WRITE la página queda de sólo lectura (RELRO, texto de bibliotecas).
    // Se hace DESPUÉS de volcar el contenido del fichero, que exige escritura.
    if prot != 0x3 {
        if paging::mprotect_range(cr3, aligned_addr, pages, prot as i32).is_err() { return -12; }
    }

    serial::write_str("[SYS] MMAP addr=");
    serial::write_hex(aligned_addr);
    serial::write_str(" pages=");
    serial::write_usize(pages);
    if con_fichero {
        serial::write_str(" fd=");
        serial::write_usize(fd as usize);
        serial::write_str(" off=");
        serial::write_hex(offset as usize);
    }
    if fixed { serial::write_str(" FIXED"); }
    { let (_, libres, _, _) = crate::mem::marcos::estadisticas(); serial::write_str(" libres="); serial::write_usize(libres); }
    serial::write_str("\n");

    if cr3 == paging::read_cr3() {
        paging::flush_tlb();
    }

    aligned_addr as i64
}

fn sys_munmap(addr: usize, len: usize) -> i32 {
    if len == 0 || addr == 0 || (addr & (PAGE_SIZE - 1)) != 0 {
        return -22;
    }
    // [Fase B.4] la ventana baja (0x200000–0x20000000) ya es desmapeable; la
    // imagen sigue protegida por la comprobación de ventanas de abajo.
    if addr < 0x20_0000 || addr.checked_add(len).map_or(true, |end| end > 0x0000_8000_0000_0000) {
        return -22;
    }
    let proc = match process::proceso_memoria() {
        Some(p) => p,
        None => return -3,
    };
    // [CIERRE B-3] `munmap` no puede destruir la imagen ELF (texto/datos/bss),
    // el heap de `brk` ni la pila del proceso: antes `munmap(0x2000_0000, …)`
    // desmapeaba el propio código. Los mapeos con pista (`mmap(hint, …)`) en
    // otras zonas y la región MMAP_BASE.. siguen siendo desmapeables.
    {
        let end = addr + len;
        let solapa = |a: usize, b: usize| addr < b && end > a;
        let imagen = proc.image_end > 0 && { let (ii, fi) = process::ventana_imagen(proc.image_end); solapa(ii, fi) };
        let heap = solapa(process::PROGRAM_BREAK_BASE, proc.program_break_end.max(process::PROGRAM_BREAK_BASE));
        let pila = solapa(process::USER_STACK_TOP - process::USER_STACK_SIZE, process::USER_STACK_TOP);
        if imagen || heap || pila {
            serial::write_str("[SYS] MUNMAP rechazado: rango fuera de la region mmap (texto/heap/pila protegidos)\n");
            return -22; // EINVAL
        }
    }

    // Sincronizar y desregistrar de SHARED_MMAPS
    {
        let mut guard = SHARED_MMAPS.lock();
        guard.retain(|e| {
            if e.pid == proc.pid && addr < e.vaddr + e.len && addr + len > e.vaddr {
                if (e.flags & 0x01 != 0) && e.is_ramfile {
                    let start = addr.max(e.vaddr);
                    let end = (addr + len).min(e.vaddr + e.len);
                    let sync_len = end.saturating_sub(start);
                    if sync_len > 0 {
                        let mut data = alloc::vec![0u8; sync_len];
                        if paging::copy_from_user(&mut data, start, sync_len).is_ok() {
                            let file_offset = start - e.vaddr;
                            let path_str = core::str::from_utf8(&e.path[..e.path_len]).unwrap_or("");
                            let _ = crate::drivers::storage::vfs::escribir_fs(path_str, file_offset as u64, &data);
                        }
                    }
                }
                false
            } else {
                true
            }
        });
    }

    let pages = (len + PAGE_SIZE - 1) / PAGE_SIZE;
    let cr3 = proc.cr3;
    crate::mem::vmas::recortar(cr3, addr, addr + pages * PAGE_SIZE); // [T-014b]
    for i in 0..pages {
        let vaddr = addr + i * PAGE_SIZE;
        if i & 0xFF == 0xFF { paging::respirar(); } // [T-026]
        if let Some((paddr, flags)) = paging::translate(cr3, vaddr) {
            if flags & paging::USER != 0 && paging::is_dynamic_addr(paddr) {
                paging::liberar_marco_usuario(paddr);
                let _ = paging::unmap_page(cr3, vaddr);
            }
        } else if paging::pte_cruda(cr3, vaddr).map_or(false, paging::es_perezosa) {
            let _ = paging::unmap_page(cr3, vaddr); // [b191] reserva perezosa: sólo la PTE
        }
    }
    if cr3 == paging::read_cr3() {
        paging::flush_tlb();
    }
    0
}

// ── SYS_GETDIRENTS ─────────────────────────────────────────────────────────

const DIRENT_HEADER_SIZE: usize = 19; // matches portix.h: d_ino(8)+d_off(8)+d_reclen(2)+d_type(1)
const DT_FILE: u8 = 1;
const DT_DIR: u8 = 2;


fn sys_getdents(path_ptr: usize, buf: usize, count: usize) -> i64 {
    if !verify_user_string(path_ptr, 256) || !verify_user_buffer(buf, count) {
        serial::write_str("[SYSCALL] sys_getdents: invalid user pointers\n");
        return -14;
    }
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_cstr_from_user(&mut path_buf, path_ptr) {
        Ok(n) => n,
        Err(_) => return -14,
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let raw_rel = alloc::string::String::from(core::str::from_utf8(&path_buf[..actual_len]).unwrap_or(""));
    let actual_len = if raw_rel.is_empty() { 0 } else { resolver_con_cwd(&raw_rel, &mut path_buf) };
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    if crate::drivers::storage::procfs::is_procfs_path(path) {
        return sys_getdents_procfs(path, buf, count);
    }

    // [CICLO 51 / L3c] cualquier montaje bajo el contrato (raiz ext2, ramfs,
    // /boot): antes solo ramfs, y la FAT (ya sin arbol) devolvia 0 entradas.
    if crate::drivers::storage::vfs::es_ruta_fs(path) {
        return sys_getdents_ramfs(path, buf, count);
    }

    if crate::drivers::storage::vfs::is_devfs_path(path) {
        return sys_getdents_devfs(path, buf, count);
    }

    // [T-001] Sin raiz montada bajo el contrato no hay sistema de ficheros
    // que sirva esta ruta: ENODEV explicito (la FAT de rescate fue podada).
    sin_raiz(path);
    -19
}

// ── SYS_EXECVE ─────────────────────────────────────────────────────────────

unsafe fn copy_user_str_array(arr_ptr: usize) -> alloc::vec::Vec<alloc::vec::Vec<u8>> {
    let mut result: alloc::vec::Vec<alloc::vec::Vec<u8>> = alloc::vec::Vec::new();
    if arr_ptr == 0 { return result; }
    let mut ptr = arr_ptr;
    loop {
        let mut ptr_bytes = [0u8; 8];
        if paging::copy_from_user(&mut ptr_bytes, ptr, 8).is_err() { break; }
        let s_ptr = u64::from_le_bytes(ptr_bytes) as usize;
        if s_ptr == 0 { break; }
        // [FRENTE B-3] Copia de la cadena página a página: una cadena corta al
        // final de la última página mapeada del heap (justo bajo el `brk`) hacía
        // fallar la copia de 512 bytes en bloque y el argumento llegaba VACÍO
        // (`pkg: comando desconocido ''` en el segundo fork+execve del WM).
        // [T-004] Hasta 128 KiB por cadena (MAX_ARG_STRLEN de Linux); antes 512 B,
        // que truncaba `-D`/`--greeting=` largos y PATHs reales de glibc.
        const MAX_ARG_STRLEN: usize = 128 * 1024;
        let mut buf: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        let mut copied = 0usize;
        let mut done = false;
        let mut trozo = [0u8; 4096];
        while copied < MAX_ARG_STRLEN && !done {
            let src = s_ptr + copied;
            let hasta_pagina = 0x1000 - (src & 0xFFF);
            let n = hasta_pagina.min(MAX_ARG_STRLEN - copied);
            match paging::copy_from_user(&mut trozo[..n], src, n) {
                Ok(_) => {
                    let fin = trozo[..n].iter().position(|&b| b == 0);
                    let util = fin.unwrap_or(n);
                    buf.extend_from_slice(&trozo[..util]);
                    if fin.is_some() { done = true; }
                    copied += n;
                }
                Err(_) => break,
            }
        }
        result.push(buf);
        ptr += 8;
    }
    result
}

fn sys_execve(path_ptr: usize, argv: usize, envp: usize, current_rsp: u64) -> SyscallResult {
    // 1. Copy path from user
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_cstr_from_user(&mut path_buf, path_ptr) {
        Ok(n) => n,
        Err(_) => return SyscallResult(-14i64 as u64, 0),
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let raw_rel = alloc::string::String::from(core::str::from_utf8(&path_buf[..actual_len]).unwrap_or(""));
    let actual_len = if raw_rel.is_empty() { 0 } else { resolver_con_cwd(&raw_rel, &mut path_buf) };
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    serial::write_str("[SYS] EXECVE path='");
    serial::write_str(path);
    serial::write_str("'\n");

    // 2. Leer el ELF: FAT32 o ramfs (/tmp). [CICLO 44] Antes solo FAT32, asi que
    // `sh` no podia ejecutar un binario recien compilado en /tmp por portix-cc
    // (la consola del nucleo si, via `elf::read_binary_file`).
    // [T-003] Por trozos si es grande (cc1 de gcc: 37 MiB), nunca entero al montículo.
    let Some(mut fuente) = crate::elf::FuenteElf::abrir(path) else {
        serial::write_str("[EXEC] file not found\n");
        return SyscallResult(-2i64 as u64, 0);
    };

    // 3. Validate and parse ELF
    let mut info = match crate::elf::elf_load_raw(fuente.cabecera()) {
        Ok(i) => i,
        Err(e) => { serial::write_str("[EXEC] "); serial::write_str(e); serial::write_str("\n"); return SyscallResult(-8i64 as u64, 0); }
    };

    // 4. Create new address space
    let new_cr3 = match paging::new_address_space() {
        Some(c) => c,
        None => { serial::write_str("[EXEC] OOM: new_address_space\n"); return SyscallResult(-12i64 as u64, 0); }
    };

    // 5. Load ELF segments into new CR3
    if let Err(e) = crate::elf::load_segments_fuente(new_cr3, &mut fuente, info.load_bias) {
        serial::write_str("[EXEC] segment load failed: ");
        serial::write_str(e);
        serial::write_str("\n");
        // Don't leak new_cr3 on failure, but this is best-effort during error
        return SyscallResult(-12i64 as u64, 0);
    }
    // [CICLO 50 / T1b] PT_INTERP: el RIP inicial es la entrada del cargador
    // dinámico; AT_ENTRY sigue siendo la entrada del programa.
    let rip_inicial = match crate::elf::cargar_interprete(new_cr3, &mut info) {
        Some(r) => r,
        None => { // ENOENT: liberar la imagen ya cargada en new_cr3 (ventana baja + tablas)
            { let (lo, hi) = process::ventana_baja_libre(0); paging::free_user_pages_dispersos(new_cr3, lo, hi); }
            paging::free_address_space(new_cr3);
            return SyscallResult(-2i64 as u64, 0);
        }
    };

    // 6. Copy argv/envp from old userspace (limit to prevent stack overflow)
    // [T-004] 1024 argumentos y 1024 variables (collect2/ld reciben cientos);
    // sus punteros (≤16 KiB) siempre caben en la pila inicial porque las
    // cadenas largas se desvían a la pila extendida (ver Phase 1).
    const MAX_ARGV: usize = 1024;
    const MAX_ENVP: usize = 1024;
    let mut argv_strs = unsafe { copy_user_str_array(argv) };
    let mut envp_strs = unsafe { copy_user_str_array(envp) };
    argv_strs.truncate(MAX_ARGV);
    envp_strs.truncate(MAX_ENVP);

    // 7. Allocate and set up new user stack
    let us_size = crate::process::USER_STACK_SIZE;
    let us_layout = match core::alloc::Layout::from_size_align(us_size, crate::mem::paging::PAGE_SIZE) {
        Ok(layout) => layout,
        Err(_) => {
            serial::write_str("[EXEC] CRITICAL: invalid Layout for user stack\n");
            return SyscallResult(-12i64 as u64, 0);
        }
    };
    let _ = us_layout;
    let us_base = match paging::asignar_marcos_usuario_contiguos(us_size / crate::mem::paging::PAGE_SIZE) { // [Fase B.4]
        Some(b) => b,
        None => {
            serial::write_str("[EXEC] OOM: user stack\n");
            return SyscallResult(-12i64 as u64, 0);
        }
    };
    let us_vaddr = crate::process::USER_STACK_TOP - us_size;

    // Map user stack in new CR3
    for i in 0..(us_size / crate::mem::paging::PAGE_SIZE) {
        if paging::map_page_user(new_cr3, us_vaddr + i * crate::mem::paging::PAGE_SIZE, us_base + i * crate::mem::paging::PAGE_SIZE).is_err() {
            serial::write_str("[EXEC] stack map failed\n");
            return SyscallResult(-12i64 as u64, 0);
        }
    }

    // Build user stack in kernel heap
    let stack_slice = unsafe { core::slice::from_raw_parts_mut(us_base as *mut u8, us_size) };
    let mut sp = us_size;

    macro_rules! push_u64 {
        ($val:expr) => {{
            let bytes = $val.to_le_bytes();
            sp = sp.wrapping_sub(8);
            stack_slice[sp..sp + 8].copy_from_slice(&bytes);
        }};
    }
    
    // ── Phase 1: Place all strings ──
    // [T-004] Si las cadenas no caben con holgura en los 64 KiB iniciales (Linux
    // admite hasta 2 MiB de argumentos), van a páginas de la pila extendida
    // justo debajo, mapeadas una a una (las libera `free_user_pages_dispersos`
    // en exit/execve y las copia `fork` como el resto de la pila extendida).
    const ARG_MAX_TOTAL: usize = 512 * 1024;
    let mut total_cadenas: usize = 0;
    for s in envp_strs.iter().chain(argv_strs.iter()) { total_cadenas += s.len() + 1; }
    if total_cadenas > ARG_MAX_TOTAL {
        paging::free_address_space(new_cr3);
        return SyscallResult(-7i64 as u64, 0); // E2BIG
    }
    let mut ext_paginas: alloc::vec::Vec<usize> = alloc::vec::Vec::new(); // marcos, de arriba abajo
    let mut ext_sp: usize = 0; // bytes ocupados bajo us_vaddr
    let usar_ext = total_cadenas + 24 * 1024 > us_size;
    if usar_ext {
        let np = (total_cadenas + paging::PAGE_SIZE - 1) / paging::PAGE_SIZE;
        for i in 0..np {
            let v = us_vaddr - (i + 1) * paging::PAGE_SIZE;
            let Some(m) = paging::asignar_marco_usuario() else {
                serial::write_str("[EXEC] OOM: paginas de argumentos\n");
                return SyscallResult(-12i64 as u64, 0);
            };
            unsafe { core::ptr::write_bytes(m as *mut u8, 0, paging::PAGE_SIZE); }
            if paging::map_page(new_cr3, v, m, paging::PRESENT | paging::WRITABLE | paging::USER | paging::NO_EXECUTE).is_err() {
                serial::write_str("[EXEC] mapeo de paginas de argumentos fallo\n");
                return SyscallResult(-12i64 as u64, 0);
            }
            ext_paginas.push(m);
        }
    }
    // Escribe `s` + NUL y devuelve su dirección virtual (en la pila inicial o en la extendida).
    let mut colocar = |s: &[u8], sp: &mut usize| -> Option<u64> {
        let total_len = s.len() + 1;
        if usar_ext {
            ext_sp += total_len;
            let va = us_vaddr - ext_sp;
            for (i, &b) in s.iter().enumerate() {
                let off = ext_sp - i; // distancia desde us_vaddr del byte i
                let pag = (off - 1) / paging::PAGE_SIZE;
                let dentro = paging::PAGE_SIZE - 1 - ((off - 1) % paging::PAGE_SIZE);
                unsafe { *((ext_paginas[pag] + dentro) as *mut u8) = b; }
            }
            Some(va as u64)
        } else {
            if total_len > *sp { return None; }
            *sp = sp.wrapping_sub(total_len);
            stack_slice[*sp..*sp + s.len()].copy_from_slice(s);
            stack_slice[*sp + s.len()] = 0;
            Some((us_vaddr + *sp) as u64)
        }
    };
    // Envp strings (preserve order)
    let mut envp_str_addrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    for s in &envp_strs {
        match colocar(s, &mut sp) { Some(va) => envp_str_addrs.push(va), None => break }
    }
    
    // Argv strings (preserve order)
    let mut argv_str_addrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    for s in &argv_strs {
        match colocar(s, &mut sp) { Some(va) => argv_str_addrs.push(va), None => break }
    }
    
    // [CICLO 49 / T1] 16 bytes para AT_RANDOM (musl/glibc los leen en el arranque)
    sp = sp.wrapping_sub(16);
    for i in 0..16 {
        stack_slice[sp + i] = (crate::time::pit::ticks() as u8).wrapping_mul(29).wrapping_add(i as u8).wrapping_mul(53) ^ 0x5A;
    }
    let random_va = (us_vaddr + sp) as u64;

    // [T-004] Base alineada a 16: la paridad de palabras de abajo sólo garantiza
    // RSP % 16 == 0 en la entrada si se parte de 16 (glibc hace `movaps` sobre la pila
    // y un RSP a 8 mod 16 acababa en #GP dentro de ld-linux).
    sp &= !15;

    // ── [CICLO 49 / T1] Vector auxiliar (auxv) ──
    // execve no empujaba NINGÚN auxv: los programas de Portix (crt0 propio) no
    // lo necesitan, pero musl/glibc leen auxv justo después del NULL de envp
    // (AT_PHDR para TLS, AT_RANDOM para el canario, AT_PAGESZ...). Sin él,
    // `busybox sh -c cmd` leía basura hasta el tope de la pila y moría con
    // #PF en 0x7F0000000000. Mismo contenido que la ruta de creación inicial.
    use crate::process::types::{AT_NULL, AT_PHDR, AT_PHENT, AT_PHNUM, AT_PAGESZ, AT_BASE, AT_FLAGS, AT_ENTRY, AT_UID, AT_EUID, AT_GID, AT_EGID, AT_SECURE, AT_RANDOM, AT_EXECFN, AT_CLKTCK};
    let execfn_va = argv_str_addrs.first().copied().unwrap_or(0);
    let auxv_pairs: [(u64, u64); 16] = [
        (AT_PHDR, info.auxv.phdr),
        (AT_PHENT, info.auxv.phent),
        (AT_PHNUM, info.auxv.phnum),
        (AT_PAGESZ, 4096),
        (AT_BASE, info.auxv.base),
        (AT_FLAGS, 0),
        (AT_ENTRY, info.entry),
        (AT_UID, 0), (AT_EUID, 0), (AT_GID, 0), (AT_EGID, 0),
        (AT_SECURE, 0),
        (AT_CLKTCK, 100),
        (AT_RANDOM, random_va),
        (AT_EXECFN, execfn_va),
        (AT_NULL, 0),
    ];
    // Alineación a 16 bytes del RSP final: argc + argv + NULL + envp + NULL + auxv*2
    let palabras = 1 + argv_str_addrs.len() + 1 + envp_str_addrs.len() + 1 + auxv_pairs.len() * 2;
    if palabras % 2 != 0 { push_u64!(0u64); }
    for &(tag, val) in auxv_pairs.iter().rev() {
        push_u64!(val);
        push_u64!(tag);
    }
    
    // ── Phase 2: Push envp pointers (NULL terminated) ──
    // Push NULL first (highest address), then envp[n-1]..envp[0] so that
    // envp[0] ends up at the lowest address (where crt0.s expects it).
    push_u64!(0u64); // envp NULL terminator
    for addr in envp_str_addrs.iter().rev() {
        push_u64!(*addr);
    }
    
    // ── Phase 3: Push argv pointers (NULL terminated) ──
    push_u64!(0u64); // argv NULL terminator
    for addr in argv_str_addrs.iter().rev() {
        push_u64!(*addr);
    }
    
    // ── Phase 4: Push argc ──
    let argc = argv_strs.len() as u64;
    push_u64!(argc);
    
    let user_rsp = (us_vaddr + sp) as u64;

    // [T-017] `execve` desde un hilo secundario: el llamador se convierte en
    // el líder (PID del proceso) y los demás hilos mueren antes de tocar nada.
    let era_hilo = match process::execve_desde_hilo() {
        Ok(h) => h,
        Err(e) => {
            paging::liberar_marcos_usuario_contiguos(us_base, us_size / crate::mem::paging::PAGE_SIZE);
            return SyscallResult(e as u64, 0);
        }
    };

    // 8. Free old address space and user stack
    let proc = match process::current_process() {
        Some(p) => p,
        None => { serial::write_str("[EXEC] no current process\n"); return SyscallResult(-3i64 as u64, 0); }
    };

    let old_cr3 = proc.cr3;
    let old_us_phys = proc.user_stack_phys;
    // [FRENTE C-4] Extension real del monticulo y de la zona mmap de la imagen
    // que se abandona: hay que devolver esos marcos al salir de ella.
    let old_end_break = proc.program_break_end.max(proc.program_break);
    let old_mmap_end = proc.mmap_base;
    let old_image_end = proc.image_end; // [Fase B.4] ventana baja libre de la imagen vieja

    // [T-005b, defecto 33] execve(2), ABI Linux: sólo se cierran los descriptores
    // con FD_CLOEXEC; los demás sobreviven a la nueva imagen. Antes se descartaban
    // TODOS los fd >= 3 sin pasar por `fd_close`: una tubería heredada por el hijo
    // (fork + dup2 + exec, como hace `Command::output()` de la std de Rust) no
    // restaba nunca su escritor, y el `poll` del padre esperando EOF de gcc no
    // veía POLLHUP: rustc se quedaba dormido en `poll` para siempre tras enlazar.
    for i in 0..process::MAX_FDS {
        let cerrar = match process::tabla_fds(proc)[i] { Some(ref e) => (e.flags & 1) != 0, None => false };
        if cerrar { process::fd_close(proc, i); }
    }

    // Update process struct
    proc.cr3 = new_cr3;
    proc.user_rip = rip_inicial;
    proc.user_rsp = user_rsp;
    proc.user_stack_phys = us_base;
    proc.program_break = process::PROGRAM_BREAK_BASE;
    proc.program_break_end = process::PROGRAM_BREAK_BASE;
    proc.mmap_base = process::MMAP_BASE;
    proc.image_end = info.total_size as usize;

    // Update name
    let name_bytes = path_buf[..actual_len.min(31)].to_vec(); // already have path in buffer
    proc.name_len = actual_len.min(31);
    proc.name[..proc.name_len].copy_from_slice(&name_bytes);
    {
        let ruta = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");
        proc.set_exe_path(ruta);
    }

    // [CICLO v6.x] La imagen cambió: se publica la tabla de símbolos del NUEVO
    // ejecutable para este PID, de modo que un depurador enganchado reciba la
    // parada de execve con los símbolos ya disponibles (y el profiler deje de
    // atribuir muestras a la imagen anterior).
    let pid_actual = proc.pid;
    if let Some(d) = fuente.imagen() { crate::system::symbols::register_for_pid(pid_actual, d, info.load_bias); }

    // Free old resources
    if old_us_phys != 0 {
        // [Fase B.4] vuelve al asignador de marcos (o al montículo si era respaldo)
        paging::liberar_marcos_usuario_contiguos(old_us_phys, us_size / crate::mem::paging::PAGE_SIZE);
    }
    // [FRENTE C-4] El espacio antiguo es el que esta cargado en CR3 durante
    // este syscall, asi que esta condicion NUNCA se cumplia: cada execve
    // dejaba huerfanos sus tablas de paginas, el monticulo y las referencias
    // COW a la imagen heredada del fork (1200 retenciones sin liberar en 300
    // ciclos fork+exec). Ahora se libera DESPUES de conmutar a `new_cr3`,
    // mas abajo; aqui solo queda el caso (teorico) de un CR3 distinto.
    let mut old_cr3_pendiente = old_cr3;
    if old_cr3 != 0 && old_cr3 != paging::read_cr3() {
        paging::free_user_heap_pages(old_cr3, process::PROGRAM_BREAK_BASE, old_end_break);
        paging::free_user_heap_pages(old_cr3, process::MMAP_BASE, old_mmap_end);
        { let (lo, hi) = process::ventana_baja_libre(old_image_end); paging::free_user_pages_dispersos(old_cr3, lo, hi); } // [Fase B.4]
        { let (lo, hi) = process::pila_extendida(); paging::free_user_pages_dispersos(old_cr3, lo, hi); } // [T-004]
        paging::free_address_space(old_cr3);
        old_cr3_pendiente = 0;
    }

    // 9. Validate current_rsp before modifying IRET frame
    {
        let stack_bottom = proc.kernel_stack_base;
        let stack_top = proc.kernel_stack_top;
        let rsp = current_rsp as usize;
        if rsp <= stack_bottom || rsp >= stack_top || (rsp - stack_bottom) < 256 {
            serial::write_str("[EXEC] CRITICAL: invalid current_rsp in IRET frame\n");
            return SyscallResult(-14i64 as u64, 0);
        }
    }

    // 10. Modify IRET frame on current kernel stack:
    //     PUSH_REGS guarda 256 B de XMM + 15 GPR = 376 B, y encima vienen los
    //     cinco qwords que empuja el CPU: [RIP][CS][RFLAGS][RSP][SS]. Por eso el
    //     frame vivo está en `kernel_stack_top - KERNEL_FRAME_SIZE` y sus
    //     offsets son los de `process::signals` (+376 … +408).
    //     [BUGFIX v6.x] Antes se escribían offsets de un layout antiguo (+120),
    //     que caían en el área de XMM: el reemplazo de imagen sólo funcionaba
    //     cuando el kernel relanzaba el proceso por otra vía, y un `execve`
    //     dentro de un `fork` reanudaba en el contexto equivocado.
    unsafe {
        use crate::process::signals::{FRAME_RIP, FRAME_RSP, FRAME_CS, FRAME_SS, FRAME_RFLAGS};
        let base = current_rsp as *mut u64;
        core::ptr::write_unaligned(base.add(FRAME_RIP / 8), rip_inicial);
        core::ptr::write_unaligned(base.add(FRAME_RSP / 8), user_rsp);
        core::ptr::write_unaligned(base.add(FRAME_CS / 8), 0x23u64);    // USER_CS | 3
        core::ptr::write_unaligned(base.add(FRAME_SS / 8), 0x1Bu64);    // USER_DS | 3
        core::ptr::write_unaligned(base.add(FRAME_RFLAGS / 8), 0x202u64); // IF=1
        // [T-004] ABI de entrada: GPR a cero. RDX en particular es `rtld_fini`
        // para glibc; aquí llevaba el puntero `envp` del execve antiguo y glibc
        // lo registraba con atexit (salto a basura al terminar el programa).
        // RAX lo fija el valor de retorno; RCX/R11 los usa la puerta de retorno.
        use crate::process::signals::{FRAME_RDX, FRAME_RSI, FRAME_RDI, FRAME_R8, FRAME_R9, FRAME_R10};
        for off in [FRAME_RDX, FRAME_RSI, FRAME_RDI, FRAME_R8, FRAME_R9, FRAME_R10, 328, 336, 344, 352, 360, 368] {
            core::ptr::write_unaligned(base.add(off / 8), 0u64); // rbx rbp r12 r13 r14 r15 en +328..+368
        }
    }

    // 10. Switch to new CR3
    let current_cr3 = paging::read_cr3();
    if new_cr3 != current_cr3 {
        paging::write_cr3(new_cr3);
    }
    // [T-017] Los hilos terminados en otras CPUs pueden estar aún saliendo
    // sobre el CR3 antiguo: no se libera mientras alguna lo tenga cargado.
    if era_hilo && old_cr3_pendiente != 0 && crate::arch::percpu::cr3_en_otra_cpu(old_cr3_pendiente) {
        let d_cgn = crate::arch::bkl::soltar_todo();
        let t0 = crate::time::pit::ticks();
        unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
        while crate::arch::percpu::cr3_en_otra_cpu(old_cr3_pendiente) && crate::time::pit::ticks().wrapping_sub(t0) < 200 {
            core::hint::spin_loop();
        }
        crate::arch::bkl::retomar(d_cgn);
        if crate::arch::percpu::cr3_en_otra_cpu(old_cr3_pendiente) {
            serial::write_str("[T-017] execve: otra CPU sigue sobre el CR3 antiguo tras 2 s; tablas huerfanas\n");
            old_cr3_pendiente = 0;
        }
    }
    // [FRENTE C-4] Ya sobre el espacio nuevo (la pila de kernel y el ELF en
    // memoria viven en el monticulo del nucleo, mapeado en todos los espacios),
    // se devuelve el espacio de direcciones anterior: marcos del monticulo,
    // de mmap, referencias COW de la imagen y tablas privadas.
    if old_cr3_pendiente != 0 && old_cr3_pendiente != new_cr3 {
        if old_end_break > process::PROGRAM_BREAK_BASE {
            paging::free_user_heap_pages(old_cr3_pendiente, process::PROGRAM_BREAK_BASE, old_end_break);
        }
        if old_mmap_end > process::MMAP_BASE {
            paging::free_user_heap_pages(old_cr3_pendiente, process::MMAP_BASE, old_mmap_end);
        }
        { let (lo, hi) = process::ventana_baja_libre(old_image_end); paging::free_user_pages_dispersos(old_cr3_pendiente, lo, hi); } // [Fase B.4]
        { let (lo, hi) = process::pila_extendida(); paging::free_user_pages_dispersos(old_cr3_pendiente, lo, hi); } // [T-004]
        paging::free_address_space(old_cr3_pendiente);
    }

    // [CICLO v6.x] Parada de `execve`: un proceso trazado se detiene justo
    // después de cargar la imagen nueva y antes de ejecutar su primera
    // instrucción, para que el trazador pueda inspeccionar el punto de entrada
    // e instalar puntos de parada simbólicos (misma semántica que Linux).
    if proc.ptrace_traced {
        crate::process::stop_for_tracer(proc.pid, crate::process::signals::SIGTRAP as u8, rip_inicial);
        crate::drivers::serial::write_str("[EXEC-STOP] PID=");
        crate::drivers::serial::write_usize(proc.pid as usize);
        crate::drivers::serial::write_str(" detenido en el punto de entrada tras execve\n");
        // [85ª, defecto 22] En un AP sin nadie listo `switch_away` devolvía 0 y el
        // hijo arrancaba sin esperar al trazador (el breakpoint llegaba tarde).
        let next = crate::process::esperar_mientras_detenido();
        return SyscallResult(0, next);
    }

    serial::write_str("[SYS] EXECVE done: entry=");
    serial::write_hex(rip_inicial as usize);
    serial::write_str(" rsp=");
    serial::write_hex(user_rsp as usize);
    serial::write_str(" argc=");
    serial::write_usize(argc as usize);
    serial::write_str("\n");

    // [T-017] Regla E3d: la continuación de consola (`ring3_ret_addr`) nunca
    // vive en un AP. Si el hilo que hizo `execve` corría en un AP y heredó la
    // del líder, este AP abandona el contexto (frame ya preparado, `Ready`,
    // afinidad al BSP) y es el BSP quien lo reanuda con `iretq` al programa nuevo.
    if era_hilo && proc.ring3_ret_addr != 0 && !crate::arch::smp::is_bsp() {
        serial::write_str("[T-017] lider nuevo con continuacion de consola en un AP: lo reanuda el BSP\n");
        proc.affinity_mask = 1;
        proc.kernel_rsp = current_rsp;
        proc.ticks_used = 0;
        proc.fs_base = 0;
        proc.state = process::ProcessState::Ready;
        crate::arch::smp::ap_ir_a_ocioso();
    }

    // Return (0, 0): no context switch, but modified IRET frame redirects to new program
    SyscallResult(0, 0)
}

// ── SYS_SEND ───────────────────────────────────────────────────────────────

fn sys_send(dst_pid: u64, msg_type: u64, data_ptr: u64, data_len: u64) -> i64 {
    let src_pid = match process::current_process() {
        Some(p) => p.pid,
        None => return -3,
    };

    let len = (data_len as usize).min(crate::ipc::IPC_DATA_SIZE);
    let mut kbuf = [0u8; crate::ipc::IPC_DATA_SIZE];
    if len > 0 {
        if paging::copy_from_user(&mut kbuf[..len], data_ptr as usize, len).is_err() {
            return -14;
        }
    }

    crate::ipc::send(src_pid, dst_pid, msg_type, &kbuf[..len])
}

// ── SYS_RECV ───────────────────────────────────────────────────────────────

fn sys_recv(buf: usize, len: usize, options: u32, current_rsp: u64) -> (i64, u64) {
    if !verify_user_buffer(buf, len) {
        serial::write_str("[SYSCALL] sys_recv: invalid user buffer\n");
        return (-14, 0);
    }
    let pid = match process::current_process() {
        Some(p) => p.pid,
        None => {
            serial::write_str("[SYSCALL] sys_recv: no current process\n");
            return (-3, 0);
        }
    };

    let mut kbuf = [0u8; 64];
    let res = crate::ipc::recv(pid, &mut kbuf);
    match res {
        0 => {
            let copy_len = len.min(64);
            if paging::copy_to_user(buf, &kbuf[..copy_len]).is_err() {
                return (-14, 0);
            }
            (copy_len as i64, 0)
        }
        1 => {
            // IPC_NOWAIT (bit 0 = 1): return 0 immediately without blocking
            if (options & 1) != 0 {
                return (0, 0);
            }
            if let Some(proc) = process::current_process() {
                proc.state = process::ProcessState::Blocked;
                proc.sleep_until = 0;
            }
            let cs = saved_cs_from_rsp(current_rsp);
            let new_rsp = process::schedule_desde_syscall(current_rsp, cs);
            if new_rsp != 0 {
                return (0, new_rsp);
            }
            unsafe {
                let cur = process::current_raw();
                if !cur.is_null() {
                    (*cur).state = process::ProcessState::Running;
                }
            }
            (0, 0)
        }
        _ => (-22, 0),
    }
}

// ── SYS_REG_IRQ ────────────────────────────────────────────────────────────

fn sys_reg_irq(irq: u64, pid: u64) -> i64 {
    crate::ipc::register_irq(irq as usize, pid)
}

// ── SYS_BLOCK_READ ──────────────────────────────────────────────────────────

fn sys_block_read(dev_id: i32, lba: u64, count: u64, buf: u64) -> i64 {
    if buf == 0 || count == 0 { return -22; }
    // Max 128 sectors = 64 KB (overflow-safe check antes de multiplicar)
    if count > 128 { return -22; }
    let total_bytes = (count as usize) * 512;

    let mut kbuf = alloc::vec![0u8; total_bytes];
    let n = match crate::drivers::storage::registry::with_device(dev_id as usize, |d| {
        let drive = match d {
            Some(d) => d,
            None => {
                serial::write_str("[BLK] dev not found\n");
                return None;
            }
        };
        let n = (count as usize).min(drive.total_sectors() as usize);
        let actual_bytes = n * 512;
        kbuf.resize(actual_bytes, 0);
        if drive.read_sectors(lba, n, &mut kbuf).is_err() {
            serial::write_str("[BLK] read error\n");
            return None;
        }
        Some(n)
    }) {
        Some(n) => n,
        None => return -5,
    };
    let actual_bytes = n * 512;

    if paging::copy_to_user(buf as usize, &kbuf[..actual_bytes]).is_err() {
        return -14;
    }

    serial::write_str("[BLK] ata");
    serial::write_usize(dev_id as usize);
    serial::write_str(": read LBA=");
    serial::write_usize(lba as usize);
    serial::write_str(" count=");
    serial::write_usize(n);
    serial::write_str(" -> ");
    serial::write_usize(actual_bytes);
    serial::write_str(" bytes\n");

    actual_bytes as i64
}

// ── SYS_IOPORT ───────────────────────────────────────────────────────────

/// Ports that DRIVERS are allowed to register (whitelist).
/// Sensitive kernel ports (PIC 0x20/0xA0, PIT 0x40, CMOS 0x70) are DENIED.
const DENY_PORTS: &[u16] = &[0x20, 0x21, 0xA0, 0xA1, 0x40, 0x43, 0x70, 0x71];

fn sys_ioport(port: u16, enable: u8) -> i64 {
    if DENY_PORTS.contains(&port) {
        serial::write_str("[IOPORT] DENIED port ");
        serial::write_hex(port as usize);
        serial::write_str("\n");
        return -1; // -EPERM
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    if enable != 0 {
        if proc.registered_port_count >= 16 {
            return -12;
        }
        for i in 0..proc.registered_port_count {
            if proc.registered_ports[i] == port {
                return 0; // already registered
            }
        }
        proc.registered_ports[proc.registered_port_count] = port;
        proc.registered_port_count += 1;
    } else {
        for i in 0..proc.registered_port_count {
            if proc.registered_ports[i] == port {
                proc.registered_ports[i] = proc.registered_ports[proc.registered_port_count - 1];
                proc.registered_port_count -= 1;
                break;
            }
        }
    }
    serial::write_str("[IOPORT] PID ");
    serial::write_usize(proc.pid as usize);
    serial::write_str(" port ");
    serial::write_hex(port as usize);
    serial::write_str(" enable=");
    serial::write_usize(enable as usize);
    serial::write_str("\n");
    0
}

fn port_is_registered(port: u16) -> bool {
    match process::current_process() {
        Some(proc) => {
            for i in 0..proc.registered_port_count {
                if proc.registered_ports[i] == port {
                    return true;
                }
            }
            false
        }
        None => false,
    }
}

// ── SYS_IOREAD ───────────────────────────────────────────────────────────

fn sys_ioread(port: u16) -> i64 {
    if !port_is_registered(port) {
        serial::write_str("[IOREAD] DENIED port ");
        serial::write_hex(port as usize);
        serial::write_str("\n");
        return -1; // -EPERM
    }
    let val: u8;
    unsafe {
        core::arch::asm!("in al, dx", out("al") val, in("dx") port, options(nostack, nomem));
    }
    serial::write_str("[IOREAD] port ");
    serial::write_hex(port as usize);
    serial::write_str(" -> ");
    serial::write_hex(val as usize);
    serial::write_str("\n");
    val as i64
}

// ── SYS_IOWRITE ──────────────────────────────────────────────────────────

fn sys_iowrite(port: u16, value: u8) -> i64 {
    if !port_is_registered(port) {
        serial::write_str("[IOWRITE] DENIED port ");
        serial::write_hex(port as usize);
        serial::write_str("\n");
        return -1; // -EPERM
    }
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nostack, nomem));
    }
    serial::write_str("[IOWRITE] port ");
    serial::write_hex(port as usize);
    serial::write_str(" <- ");
    serial::write_hex(value as usize);
    serial::write_str("\n");
    0
}

// ── SYS_MMAP_DEVICE ─────────────────────────────────────────────────────

fn sys_mmap_device(phys: usize, size: usize) -> i64 {
    if phys == 0 || size == 0 || size > 0x100_0000 {
        serial::write_str("[MMAP_DEV] invalid params\n");
        return -22;
    }
    // Only allow device memory regions:
    //   - VGA: 0xA0000-0xBFFFF
    //   - Framebuffer/PCI MMIO: >= 0xE000_0000
    //   - Also allow framebuffer at 0xFD00_0000 (QEMU stdvga)
    let allow = (phys >= 0xA0000 && phys < 0xC0000)
             || (phys >= 0xE000_0000)
             || (phys >= 0xFC00_0000 && phys < 0xFE00_0000);
    if !allow {
        serial::write_str("[MMAP_DEV] DENIED phys ");
        serial::write_hex(phys);
        serial::write_str(" (not device region)\n");
        return -22;
    }

    let cr3 = match process::current_process() {
        Some(p) => p.cr3,
        None => return -3,
    };

    let pages = (size + paging::PAGE_SIZE - 1) / paging::PAGE_SIZE;
    let vaddr = 0x4000_0000_0000usize; // fixed device mapping region

    for i in 0..pages {
        let vaddr_i = vaddr + i * paging::PAGE_SIZE;
        let paddr_i = (phys + i * paging::PAGE_SIZE) as u64;
        if paging::map_page(cr3, vaddr_i, paddr_i as usize,
            paging::PRESENT | paging::WRITABLE | paging::USER | paging::ACCESSED).is_err() {
            serial::write_str("[MMAP_DEV] map failed\n");
            return -12;
        }
    }

    serial::write_str("[MMAP_DEV] phys ");
    serial::write_hex(phys);
    serial::write_str(" -> vaddr ");
    serial::write_hex(vaddr);
    serial::write_str(" pages=");
    serial::write_usize(pages);
    serial::write_str("\n");

    vaddr as i64
}

// ── Device file read (for /dev/kbd, /dev/sda0) ─────────────────────────

fn sys_read_device(fd: i32, buf: usize, count: usize, info: &process::DeviceInfo) -> (i64, u64) {
    match info.dev_type {
        process::DeviceType::Null => (0, 0),
        process::DeviceType::Zero => {
            let chunk = [0u8; 128];
            let mut total = 0;
            while total < count {
                let to_copy = (count - total).min(128);
                if paging::copy_to_user(buf + total, &chunk[..to_copy]).is_err() {
                    break;
                }
                total += to_copy;
            }
            (total as i64, 0)
        }
        process::DeviceType::Urandom => {
            let res = sys_getrandom(buf, count, 0);
            (res, 0)
        }
        process::DeviceType::Kbd => {
            // Read scancodes from the /dev/kbd ring buffer
            let mut copied = 0usize;
            let mut temp = [0u8; 64];
            while copied < count.min(64) {
                match kbd_buf_read() {
                    Some(b) => {
                        temp[copied] = b;
                        copied += 1;
                    }
                    None => break,
                }
            }
            if copied > 0 {
                if paging::copy_to_user(buf, &temp[..copied]).is_err() {
                    return (-14, 0);
                }
            }
            serial::write_str("[DEV] /dev/kbd read ");
            serial::write_usize(copied);
            serial::write_str(" scancodes\n");
            (copied as i64, 0)
        }
        process::DeviceType::Sda => {
            // Block device read via the existing block device
            let pos = info.pos;
            let lba = pos / 512;
            let count_sectors = ((count + 511) / 512).min(128);
            let total_bytes = count_sectors * 512;
            let mut kbuf = alloc::vec![0u8; total_bytes];
            // Use the internal block device directly
            let offset = (pos % 512) as usize;
            let to_copy = match crate::drivers::storage::registry::with_device(0, |d| {
                let drive = d?;
                drive.read_sectors(lba as u64, count_sectors, &mut kbuf).ok()?;
                Some((total_bytes - offset).min(count))
            }) {
                Some(n) => n,
                None => return (-5, 0),
            };
            if paging::copy_to_user(buf, &kbuf[offset..offset + to_copy]).is_err() {
                return (-14, 0);
            }
            // Update position
            if let Some(proc) = process::current_process() {
                if let Some(fde) = process::fd_get_mut(proc, fd as usize) {
                    if let process::FdType::Device(ref mut di) = fde.fd_type {
                        di.pos += to_copy as u32;
                    }
                }
            }
            serial::write_str("[DEV] /dev/sda0 read LBA=");
            serial::write_usize(lba as usize);
            serial::write_str(" bytes=");
            serial::write_usize(to_copy);
            serial::write_str("\n");
            (to_copy as i64, 0)
        }
        process::DeviceType::Fb => {
            // Reading from framebuffer: return 0 bytes
            (0, 0)
        }
        process::DeviceType::Dsp => {
            (0, 0)
        }
    }
}

// ── POSIX Syscalls ────────────────────────────────────────────────────────

/// [CICLO 44] Resuelve una ruta de usuario contra el directorio de trabajo del
/// proceso: las rutas relativas (`Makefile`, `src/main.c`, `../x`) se
/// anteponen con `cwd` antes de normalizar. Hasta ahora el nucleo trataba
/// `Makefile` como `/Makefile`, y `chdir` solo servia para `getcwd`.
/// [T-004] Base para rutas relativas de las syscalls `*at` con `dirfd` real
/// (findutils/coreutils descienden con `openat(fd, "b")`, `newfstatat(fd, "b")`).
/// Hasta b187 TODAS las `*at` ignoraban `dirfd` y resolvían contra `cwd`:
/// `find /tmp/a` fallaba con ENOENT en el primer subdirectorio.
static mut DIRFD_BASE: [u8; 256] = [0; 256];
static mut DIRFD_BASE_LEN: usize = 0;

/// Ejecuta `f` resolviendo las rutas relativas contra el directorio de `dirfd`.
/// Devuelve `Err(errno)` si `dirfd` no es válido o no es un fichero de FS.
fn con_dirfd<R>(dirfd: i32, f: impl FnOnce() -> R) -> Result<R, i32> {
    if dirfd == AT_FDCWD { return Ok(f()); }
    if dirfd < 0 || dirfd as usize >= process::MAX_FDS { return Err(-9); }
    let Some(proc) = process::current_process() else { return Err(-9) };
    let (path, len) = match process::fd_get(proc, dirfd as usize) {
        Some(e) => match &e.fd_type {
            process::FdType::FsFile(info) => (info.path, info.path_len),
            _ => return Err(-20), // -ENOTDIR
        },
        None => return Err(-9),
    };
    let len = len.min(255);
    // SAFETY: el dispatch de syscalls no se reentra en el mismo núcleo; se
    // restaura siempre antes de volver al usuario.
    unsafe {
        DIRFD_BASE[..len].copy_from_slice(&path[..len]);
        DIRFD_BASE_LEN = len;
    }
    let r = f();
    unsafe { DIRFD_BASE_LEN = 0; }
    Ok(r)
}

fn resolver_con_cwd(raw: &str, buf: &mut [u8; 256]) -> usize {
    let mut tmp = alloc::string::String::new();
    if !raw.starts_with('/') {
        let base_len = unsafe { DIRFD_BASE_LEN };
        if base_len > 0 {
            if let Ok(c) = core::str::from_utf8(unsafe { &DIRFD_BASE[..base_len] }) {
                tmp.push_str(c);
                if !tmp.ends_with('/') { tmp.push('/'); }
            }
        } else if let Some(proc) = process::current_process() {
            if proc.cwd_len > 0 {
                if let Ok(c) = core::str::from_utf8(&proc.cwd[..proc.cwd_len]) {
                    tmp.push_str(c);
                    if !tmp.ends_with('/') { tmp.push('/'); }
                }
            }
        }
        if tmp.is_empty() { tmp.push('/'); }
    }
    tmp.push_str(raw);
    let norm = crate::drivers::storage::vfs::normalize_path(&tmp);
    let nb = norm.as_bytes();
    let n = nb.len().min(255);
    buf[..n].copy_from_slice(&nb[..n]);
    buf[n] = 0;
    n
}

/// Ruta de usuario SIN resolver contra `cwd` (nombres que no son rutas del
/// sistema de ficheros: colas POSIX `mq_*`, comprobaciones de forma).
fn read_user_path_raw<'a>(path_ptr: usize, buf: &'a mut [u8; 256]) -> Option<&'a str> {
    if !verify_user_string(path_ptr, 256) { return None; }
    let path_len = paging::copy_cstr_from_user(buf, path_ptr).ok()?;
    let actual_len = buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    if actual_len == 0 { return Some(""); }
    let raw = alloc::string::String::from(core::str::from_utf8(&buf[..actual_len]).ok()?);
    let norm = crate::drivers::storage::vfs::normalize_path(&raw);
    let nb = norm.as_bytes();
    let n = nb.len().min(255);
    buf[..n].copy_from_slice(&nb[..n]);
    buf[n] = 0;
    core::str::from_utf8(&buf[..n]).ok()
}

/// ¿La ruta que pasó el usuario es relativa (no empieza por '/')?
fn user_path_is_relative(path_ptr: usize) -> bool {
    let mut b = [0u8; 1];
    match paging::copy_from_user(&mut b, path_ptr, 1) { Ok(1) => b[0] != b'/', _ => false }
}

fn read_user_path<'a>(path_ptr: usize, buf: &'a mut [u8; 256]) -> Option<&'a str> {
    if !verify_user_string(path_ptr, 256) {
        return None;
    }
    let path_len = paging::copy_cstr_from_user(buf, path_ptr).ok()?;
    let actual_len = buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    if actual_len == 0 {
        return Some("");
    }
    let raw = alloc::string::String::from(core::str::from_utf8(&buf[..actual_len]).ok()?);
    let nlen = resolver_con_cwd(&raw, buf);
    core::str::from_utf8(&buf[..nlen]).ok()
}

fn sys_fork(current_rsp: u64) -> i64 {
    match process::process_fork(current_rsp) {
        Some(child_pid) => child_pid as i64,
        None => -11,
    }
}

pub const CLONE_VM: u64 = 0x00000100;
pub const CLONE_FS: u64 = 0x00000200;
pub const CLONE_FILES: u64 = 0x00000400;
pub const CLONE_SIGHAND: u64 = 0x00000800;
pub const CLONE_THREAD: u64 = 0x00010000;
pub const CLONE_SETTLS: u64 = 0x00080000;
pub const CLONE_PARENT_SETTID: u64 = 0x00100000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000;
pub const CLONE_CHILD_SETTID: u64 = 0x01000000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LinuxCloneArgs {
    pub flags: u64,
    pub pidfd: u64,
    pub child_tid: u64,
    pub parent_tid: u64,
    pub exit_signal: u64,
    pub stack: u64,
    pub stack_size: u64,
    pub tls: u64,
    pub set_tid: u64,
    pub set_tid_size: u64,
    pub cgroup: u64,
}

fn sys_clone(flags: u64, child_stack: usize, ptid_ptr: usize, ctid_ptr: usize, new_tls: usize, current_rsp: u64) -> i64 {
    // [T-005] Hilo real: CLONE_VM|CLONE_THREAD (lo que emiten pthread_create de
    // musl y glibc, y std::thread de Rust). CLONE_THREAD exige CLONE_SIGHAND, y
    // CLONE_SIGHAND exige CLONE_VM (EINVAL en Linux).
    if flags & CLONE_THREAD != 0 && flags & CLONE_SIGHAND == 0 { return -22; }
    if flags & CLONE_SIGHAND != 0 && flags & CLONE_VM == 0 { return -22; }
    let es_hilo = flags & CLONE_VM != 0 && flags & CLONE_THREAD != 0;
    if es_hilo && child_stack == 0 { return -22; }
    let child_pid = if es_hilo {
        let tls = if flags & CLONE_SETTLS != 0 { new_tls as u64 } else {
            process::current_process().map(|p| p.fs_base).unwrap_or(0)
        };
        match process::process_clone_thread(current_rsp, child_stack, tls) {
            Some(tid) => tid as i64,
            None => return -11,
        }
    } else {
        match process::process_fork_with_stack(current_rsp, child_stack) {
            Some(pid) => pid as i64,
            None => return -11, // -EAGAIN
        }
    };
    if es_hilo {
        // El hilo comparte memoria: ctid/ptid se escriben una vez y los ve todo el grupo.
        if (flags & CLONE_PARENT_SETTID) != 0 && ptid_ptr != 0 && verify_user_buffer(ptid_ptr, 4) {
            let _ = paging::copy_to_user(ptid_ptr, &(child_pid as i32).to_le_bytes());
        }
        if (flags & CLONE_CHILD_SETTID) != 0 && ctid_ptr != 0 && verify_user_buffer(ctid_ptr, 4) {
            let _ = paging::copy_to_user(ctid_ptr, &(child_pid as i32).to_le_bytes());
        }
        if (flags & CLONE_CHILD_CLEARTID) != 0 && ctid_ptr != 0 {
            if let Some(h) = process::process_by_pid(child_pid as u64) { h.clear_child_tid = ctid_ptr as u64; }
        }
        return child_pid;
    }

    if (flags & CLONE_PARENT_SETTID) != 0 && ptid_ptr != 0 && verify_user_buffer(ptid_ptr, 4) {
        let tid_bytes = (child_pid as i32).to_le_bytes();
        let _ = paging::copy_to_user(ptid_ptr, &tid_bytes);
    }

    if (flags & (CLONE_CHILD_SETTID | CLONE_CHILD_CLEARTID)) != 0 && ctid_ptr != 0 && verify_user_buffer(ctid_ptr, 4) {
        let tid_bytes = (child_pid as i32).to_le_bytes();
        let _ = paging::copy_to_user(ctid_ptr, &tid_bytes);
    }

    if (flags & CLONE_SETTLS) != 0 && new_tls != 0 {
        process::set_process_tls(child_pid as u64, new_tls as u64);
    }

    child_pid
}

fn sys_clone3(uargs: usize, size: usize, current_rsp: u64) -> i64 {
    if size < core::mem::size_of::<LinuxCloneArgs>() {
        return -22; // -EINVAL
    }
    if uargs == 0 || !verify_user_buffer(uargs, size) {
        return -14; // -EFAULT
    }

    let mut args = LinuxCloneArgs::default();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            &mut args as *mut _ as *mut u8,
            core::mem::size_of::<LinuxCloneArgs>()
        )
    };
    if paging::copy_from_user(bytes, uargs, core::mem::size_of::<LinuxCloneArgs>()).is_err() {
        return -14;
    }

    // clone3: `stack` es la dirección MÁS BAJA de la pila y `stack_size` su
    // tamaño; el RSP inicial del hijo es la suma (en clone(2) clásico el
    // argumento ya es el tope). Sin esto, el primer `push` del hilo caía
    // justo debajo de la región (#PF en CR2 = stack - 8).
    let tope = if args.stack != 0 && args.stack_size != 0 { args.stack + args.stack_size } else { args.stack };
    sys_clone(
        args.flags,
        tope as usize,
        args.parent_tid as usize,
        args.child_tid as usize,
        args.tls as usize,
        current_rsp,
    )
}

/// [FRENTE C-4] Valor centinela: el syscall debe REINICIARSE cuando el proceso
/// vuelva a ejecutarse (el despachador vuelve a poner el numero de syscall en
/// RAX y `rebobinar_syscall` ya dejo RIP sobre la instruccion de entrada).
pub const SYSCALL_RESTART: i64 = i64::MIN + 7;

/// [FRENTE C-4] Rebobina RIP del frame de Ring 3 a la instruccion que entro
/// al nucleo (`int 0x80` = CD 80 o `syscall` = 0F 05, ambas de 2 bytes) para
/// que, al reanudarse el proceso, la llamada se ejecute de nuevo completa.
/// Antes, un `waitpid` bloqueante devolvia 0 al despertar (el valor que el
/// despachador escribio en el frame antes de conmutar) y el zombi nunca se
/// recogia: tras ~14 hijos se agotaba la tabla de procesos (fork -> -1).
fn rebobinar_syscall(current_rsp: u64) -> bool {
    use crate::process::signals::FRAME_RIP;
    if current_rsp == 0 { return false; }
    let rip_ptr = (current_rsp as usize + FRAME_RIP) as *mut u64;
    let rip = unsafe { core::ptr::read_unaligned(rip_ptr) } as usize;
    if rip < 2 || !verify_user_buffer(rip - 2, 2) { return false; }
    let mut op = [0u8; 2];
    if paging::copy_from_user(&mut op, rip - 2, 2).is_err() { return false; }
    if op == [0xCD, 0x80] || op == [0x0F, 0x05] {
        unsafe { core::ptr::write_unaligned(rip_ptr, (rip - 2) as u64); }
        true
    } else {
        false
    }
}

fn sys_waitpid(target_pid: i64, status_ptr: usize, options: u32, current_rsp: u64) -> (i64, u64) {
    let (cur_pid, cur_pgid) = match process::current_process() {
        Some(p) => (p.pid, p.pgid),
        None => return (-3, 0),
    };

    // [CICLO v6.x] `waitpid` de job control completo: además de hijos
    // terminados (zombies) se reportan las PARADAS de los procesos que este
    // proceso está trazando — con o sin `WUNTRACED`, como hace un depurador
    // real — escritas en el formato POSIX `(señal << 8) | 0x7f`.
    let want_stops = (options & 2) != 0 || (options & 4) != 0;
    let (found_child, waitable) = process::find_waitable(cur_pid, target_pid, cur_pgid, want_stops);

    if let Some((wpid, status, es_parada)) = waitable {
        if status_ptr != 0 && verify_user_buffer(status_ptr, core::mem::size_of::<i32>()) {
            let code_bytes = status.to_le_bytes();
            let _ = paging::copy_to_user(status_ptr, &code_bytes);
        }
        if !es_parada {
            process::process_exit(wpid, status);
        }
        return (wpid as i64, 0);
    }

    if !found_child {
        return (-10, 0); // -ECHILD: no hay hijos que esperar
    }

    // POSIX WNOHANG = 1: if child is running and caller requested non-blocking, return 0 immediately
    if (options & 1) != 0 {
        return (0, 0);
    }

    if let Some(cur) = process::current_process() {
        cur.state = process::ProcessState::Blocked;
    }
    // [FRENTE C-4] Al despertar, el proceso repetira `waitpid` desde el
    // principio y recogera al zombi por la via rapida de arriba.
    let reiniciar = rebobinar_syscall(current_rsp);
    let cs = saved_cs_from_rsp(current_rsp);
    let new_rsp = process::schedule_desde_syscall(current_rsp, cs);
    if new_rsp == 0 {
        unsafe {
            let cur = process::current_raw();
            if !cur.is_null() {
                (*cur).state = process::ProcessState::Running;
            }
        }
    }
    (if reiniciar { SYSCALL_RESTART } else { 0 }, new_rsp)
}

fn sys_lseek(fd: i32, offset: i64, whence: i32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    let entry = match process::fd_get_mut(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match &mut entry.fd_type {
        process::FdType::FsFile(ref mut info) => {
            let path_str = core::str::from_utf8(&info.path[..info.path_len]).unwrap_or("");
            if let Some(actual_sz) = crate::drivers::storage::vfs::tam_fs(path_str) {
                info.size = actual_sz as u64;
            }
            let new_pos = match whence {
                0 => offset,
                1 => match (info.pos as i64).checked_add(offset) {
                    Some(p) => p,
                    None => return -22,
                },
                2 => match (info.size as i64).checked_add(offset) {
                    Some(p) => p,
                    None => return -22,
                },
                _ => return -22,
            };
            if new_pos < 0 { return -22; } // [T-001] 64 bits: sin tope de 4 GiB
            info.pos = new_pos as u64;
            new_pos
        }
        process::FdType::ProcFile(ref mut info) => {
            let content = crate::drivers::storage::procfs::generate_content(info.file_type);
            let sz = content.len() as i64;
            let new_pos = match whence {
                0 => offset,
                1 => match (info.pos as i64).checked_add(offset) {
                    Some(p) => p,
                    None => return -22,
                },
                2 => match sz.checked_add(offset) {
                    Some(p) => p,
                    None => return -22,
                },
                _ => return -22,
            };
            if new_pos < 0 || new_pos > u32::MAX as i64 { return -22; }
            info.pos = new_pos as u32;
            new_pos
        }
        _ => -29,
    }
}

/// [T-005b] `lstat` por ruta absoluta sin copia al usuario (metadatos del propio nodo,
/// sin seguir el enlace); fuera de los sistemas de ficheros del contrato cae en `stat`.
fn lstat_path_internal(path: &str) -> Result<Stat, i32> {
    if !path.is_empty() && vfs::es_ruta_fs(path) {
        if let Some(Ok(m)) = vfs::con_fs(path, |fs| fs.metadatos_sin_seguir(path)) {
            let mut st = Stat::default();
            st.st_mode = m.modo;
            st.st_uid = m.uid;
            st.st_gid = m.gid;
            st.st_size = m.tam as i64;
            st.st_ino = m.ino;
            st.st_nlink = m.nlink as u64;
            st.st_blksize = 4096;
            st.st_blocks = if m.bloques > 0 || m.tam == 0 { m.bloques as i64 } else { (st.st_size + 511) / 512 };
            return Ok(st);
        }
    }
    stat_path_internal(path)
}

pub(crate) fn stat_path_internal(path: &str) -> Result<Stat, i32> {
    if path.is_empty() {
        return Err(-2); // -ENOENT: empty path
    }
    let norm_path_str = crate::drivers::storage::vfs::normalize_path(path);
    let path = norm_path_str.as_str();
    if path == "/" {
        let mut st = Stat::default();
        st.st_mode = 0o040755;
        st.st_size = 4096;
        st.st_blksize = 512;
        st.st_blocks = 8;
        st.st_nlink = 2;
        return Ok(st);
    }
    if crate::drivers::storage::procfs::is_procfs_path(path) {
        return crate::drivers::storage::procfs::stat_proc(path);
    }
    // [CICLO 47] devfs: /dev es un directorio y sus nodos informan S_IFCHR o
    // S_IFBLK reales (antes stat("/dev/null") devolvia ENOENT y st_mode=0).
    if vfs::is_devfs_path(path) {
        let mut st = Stat::default();
        st.st_blksize = 512;
        st.st_nlink = 1;
        if path == "/dev" || path == "/dev/" {
            st.st_mode = 0o040755;
            st.st_size = 4096;
            st.st_nlink = 2;
            return Ok(st);
        }
        return match vfs::resolve_devfs(path) {
            Some(nombre) => {
                let es_bloque = nombre.starts_with("sda") || nombre.starts_with("loop") && nombre != "loop-control";
                st.st_mode = if es_bloque { 0o060660 } else { 0o020666 };
                Ok(st)
            }
            None => Err(-2),
        };
    }
    let mut st = Stat::default();
    if vfs::es_ruta_fs(path) {
        // [CICLO 51] metadatos por el contrato `SistemaDeFicheros`; errno exacto.
        return match vfs::con_fs(path, |fs| fs.metadatos(path)) {
            Some(Ok(m)) => {
                st.st_mode = m.modo;
                st.st_uid = m.uid;
                st.st_gid = m.gid;
                st.st_size = m.tam as i64;
                st.st_ino = m.ino;
                st.st_blksize = 4096;
                // [T-001] bloques reales asignados (ficheros dispersos); ramfs no los cuenta
                st.st_blocks = if m.bloques > 0 || m.tam == 0 { m.bloques as i64 } else { (st.st_size + 511) / 512 };
                st.st_nlink = m.nlink as u64;
                st.st_mtime = m.mtime as i64;
                Ok(st)
            }
            Some(Err(e)) => Err(-e),
            None => Err(-2),
        };
    }

    sin_raiz(path);
    Err(-19)
}

fn sys_stat(path_ptr: usize, statbuf_ptr: usize) -> i32 {
    if !verify_user_string(path_ptr, 256) || !verify_user_buffer(statbuf_ptr, core::mem::size_of::<Stat>()) {
        return -14;
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14,
    };

    match stat_path_internal(path) {
        Ok(st) => {
            let st_bytes = unsafe {
                core::slice::from_raw_parts(&st as *const Stat as *const u8, core::mem::size_of::<Stat>())
            };
            if paging::copy_to_user(statbuf_ptr, st_bytes).is_ok() { 0 } else { -14 }
        }
        // [CICLO 49 / T1] errno real (-ENOENT, ...), no -1 generico: `rm -rf` de
        // busybox distingue "no existe" (silencio) de EPERM (error).
        Err(e) => if e < 0 { e } else { -2 },
    }
}

fn sys_fstat(fd: i32, statbuf_ptr: usize) -> i32 {
    if !verify_user_buffer(statbuf_ptr, core::mem::size_of::<Stat>()) {
        return -14;
    }
    let st = match fstat_interno(fd) { Ok(s) => s, Err(e) => return e };
    let st_bytes = unsafe {
        core::slice::from_raw_parts(&st as *const Stat as *const u8, core::mem::size_of::<Stat>())
    };
    if paging::copy_to_user(statbuf_ptr, st_bytes).is_err() {
        return -14;
    }
    0
}

/// [T-005b] `fstat` sin copia al usuario: lo comparten `fstat` y `statx(fd, "", AT_EMPTY_PATH)`
/// (así consulta `File::metadata()` de la std de Rust: rustc lo hace antes de mapear cada .rlib).
fn fstat_interno(fd: i32) -> Result<Stat, i32> {
    if fd < 0 || fd as usize >= process::MAX_FDS { return Err(-9); }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return Err(-3),
    };
    let entry = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return Err(-9),
    };

    let mut st = Stat::default();
    match &entry.fd_type {
        process::FdType::Stdin | process::FdType::Stdout | process::FdType::Stderr => {
            st.st_mode = 0o020666;
        }
        process::FdType::FsFile(info) => {
            let path_str = core::str::from_utf8(&info.path[..info.path_len]).unwrap_or("");
            // [T-001] fstat por el contrato: inodo, nlink y bloques reales (no inventados).
            if let Some(Ok(m)) = crate::drivers::storage::vfs::con_fs(path_str, |fs| fs.metadatos(path_str)) {
                st.st_mode = m.modo;
                st.st_uid = m.uid;
                st.st_gid = m.gid;
                st.st_size = m.tam as i64;
                st.st_ino = m.ino;
                st.st_blksize = 4096;
                st.st_blocks = if m.bloques > 0 || m.tam == 0 { m.bloques as i64 } else { (st.st_size + 511) / 512 };
                st.st_nlink = m.nlink as u64;
            } else if let Some((mode, uid, gid, sz, is_dir, _is_symlink)) = crate::drivers::storage::vfs::meta_fs(path_str) {
                st.st_mode = mode;
                st.st_uid = uid;
                st.st_gid = gid;
                st.st_size = sz as i64;
                st.st_blksize = 512;
                st.st_blocks = (st.st_size + 511) / 512;
                st.st_nlink = if is_dir { 2 } else { 1 };
            } else {
                st.st_mode = 0o100644;
                st.st_size = info.size as i64;
                st.st_blksize = 512;
                st.st_blocks = (st.st_size + 511) / 512;
                st.st_nlink = 1;
            }
        }
        process::FdType::Device(_) => {
            st.st_mode = 0o020666;
        }
        process::FdType::PipeRead(_) | process::FdType::PipeWrite(_) => {
            st.st_mode = 0o010666;
        }
        process::FdType::Socket(_) | process::FdType::SocketPairEnd { .. } => {
            st.st_mode = 0o140777; // S_IFSOCK
            st.st_size = 0;
        }
        process::FdType::EventFd(_) => {
            st.st_mode = 0o020666; // S_IFCHR
            st.st_size = 0;
        }
        process::FdType::Epoll(_) => {
            st.st_mode = 0o020666; // S_IFCHR
            st.st_size = 0;
        }
        process::FdType::TimerFd(_) | process::FdType::SignalFd(_) => {
            st.st_mode = 0o020666; // S_IFCHR
            st.st_size = 0;
        }
        process::FdType::PtyMaster(_) | process::FdType::PtySlave(_) | process::FdType::PidFd(_) | process::FdType::IoUring(_) | process::FdType::Mq(_) | process::FdType::Fanotify(_) | process::FdType::Inotify(_) => {
            st.st_mode = 0o020666; // S_IFCHR
            st.st_size = 0;
        }
        process::FdType::ProcFile(info) => {
            let content = crate::drivers::storage::procfs::generate_content(info.file_type);
            st.st_mode = 0o100444; // S_IFREG | 0444
            st.st_size = content.len() as i64;
            st.st_blksize = 512;
            st.st_blocks = (st.st_size + 511) / 512;
            st.st_nlink = 1;
        }
    }
    Ok(st)
}

fn sys_kill(target_pid_raw: u64, sig: i32) -> i32 {
    let cur_pgid = match process::current_process() {
        Some(p) => p.pgid,
        None => 0,
    };
    match process::kill_matching(target_pid_raw as i64, sig, cur_pgid) {
        Ok(_) => 0,
        Err(_) => -3,
    }
}

fn sys_getcwd(buf_ptr: usize, size: usize) -> i64 {
    if !verify_user_buffer(buf_ptr, size) {
        return -14;
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    if size < proc.cwd_len + 1 {
        return -34;
    }
    let mut slice = alloc::vec![0u8; proc.cwd_len + 1];
    slice[..proc.cwd_len].copy_from_slice(&proc.cwd[..proc.cwd_len]);
    slice[proc.cwd_len] = 0;
    if paging::copy_to_user(buf_ptr, &slice).is_err() {
        return -14;
    }
    buf_ptr as i64
}

fn sys_chdir(path_ptr: usize) -> i32 {
    if !verify_user_string(path_ptr, 128) {
        return -14;
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14,
    };
    if path.is_empty() { return -2; }

    // POSIX validation: path must exist and must be a directory
    let is_dir = if path == "/" {
        true
    } else if crate::drivers::storage::vfs::es_dir_fs(path) {
        true
    } else {
        false
    };

    if !is_dir {
        return -20; // -ENOENT or -ENOTDIR
    }

    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    let len = path.len().min(63);
    proc.cwd[..len].copy_from_slice(&path.as_bytes()[..len]);
    proc.cwd_len = len;
    0
}

fn sys_unlink(path_ptr: usize) -> i32 {
    if !verify_user_string(path_ptr, 128) {
        return -14;
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14,
    };
    if vfs::es_ruta_fs(path) {
        // [CICLO 51] contrato `SistemaDeFicheros`: ENOENT / EISDIR exactos
        return match vfs::con_fs(path, |fs| fs.eliminar(path)) { Some(Ok(())) => 0, Some(Err(e)) => -e, None => -2 };
    }

    // [T-001] Sin raiz montada bajo el contrato no hay sistema de ficheros
    // que sirva esta ruta: ENODEV explicito (la FAT de rescate fue podada).
    sin_raiz(path);
    -19
}

fn sys_pipe(pipefd_ptr: usize) -> i32 {
    sys_pipe2(pipefd_ptr, 0)
}

fn sys_pipe2(pipefd_ptr: usize, flags: i32) -> i32 {
    const VALID_FLAGS: i32 = 0x80000 | 0x0080 | 0x0800 | 0x0004;
    if (flags & !VALID_FLAGS) != 0 {
        return -22; // -EINVAL
    }
    if !verify_user_buffer(pipefd_ptr, 8) {
        return -14; // -EFAULT
    }
    let pipe_id = match process::pipe_alloc() {
        Some(id) => id,
        None => return -24, // -EMFILE
    };
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };

    let cloexec = (flags & (0x80000 | 0x0080)) != 0;
    let mut fd_flags = if cloexec { 1 } else { 0 };
    if (flags & (0x0800 | 0x0004)) != 0 {
        fd_flags |= 0x800;
    }

    let mut entry0 = process::FdEntry::new(process::FdType::PipeRead(pipe_id));
    entry0.flags = fd_flags;
    let fd0 = match process::fd_alloc(proc, entry0) {
        Some(fd) => fd,
        None => {
            process::pipe_close_reader(pipe_id);
            process::pipe_close_writer(pipe_id);
            return -24;
        }
    };

    let mut entry1 = process::FdEntry::new(process::FdType::PipeWrite(pipe_id));
    entry1.flags = fd_flags;
    let fd1 = match process::fd_alloc(proc, entry1) {
        Some(fd) => fd,
        None => {
            process::fd_close(proc, fd0);
            process::pipe_close_writer(pipe_id);
            return -24;
        }
    };

    let fds: [i32; 2] = [fd0 as i32, fd1 as i32];
    let fds_bytes = unsafe {
        core::slice::from_raw_parts(fds.as_ptr() as *const u8, 8)
    };
    if paging::copy_to_user(pipefd_ptr, fds_bytes).is_err() {
        process::fd_close(proc, fd0);
        process::fd_close(proc, fd1);
        return -14;
    }

    serial::write_str("[SYS] PIPE2 created fd0=");
    serial::write_usize(fd0);
    serial::write_str(" fd1=");
    serial::write_usize(fd1);
    serial::write_str(" flags=");
    serial::write_usize(flags as usize);
    serial::write_str("\n");
    0
}

fn sys_access(path_ptr: usize, mode: i32) -> i32 {
    if (mode & !7) != 0 {
        return -22; // -EINVAL
    }
    if !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14, // -EFAULT
    };
    if path.is_empty() {
        return -2; // -ENOENT
    }

    if path == "/" {
        return 0;
    }

    if vfs::es_ruta_fs(path) {
        // [CICLO 51] contrato `SistemaDeFicheros`
        return match vfs::con_fs(path, |fs| fs.metadatos(path)) { Some(Ok(_)) => 0, Some(Err(e)) => -e, None => -2 };
    }

    // [T-001] Sin raiz montada bajo el contrato no hay sistema de ficheros
    // que sirva esta ruta: ENODEV explicito (la FAT de rescate fue podada).
    sin_raiz(path);
    -19
}

fn sys_rename(old_ptr: usize, new_ptr: usize) -> i32 {
    if !verify_user_string(old_ptr, 256) || !verify_user_string(new_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut old_buf = [0u8; 256];
    let mut new_buf = [0u8; 256];
    let old_path = match read_user_path(old_ptr, &mut old_buf) {
        Some(s) => s,
        None => return -14,
    };
    let new_path = match read_user_path(new_ptr, &mut new_buf) {
        Some(s) => s,
        None => return -14,
    };

    if old_path.is_empty() || new_path.is_empty() {
        return -2; // -ENOENT
    }

    // [CICLO 47] POSIX: rename entre sistemas de ficheros distintos (ramfs /tmp
    // <-> FAT32) no mueve datos: devuelve EXDEV y el usuario copia+borra.
    if vfs::es_ruta_fs(old_path) != vfs::es_ruta_fs(new_path) {
        return -18; // -EXDEV
    }
    if vfs::es_ruta_fs(old_path) && vfs::es_ruta_fs(new_path) {
        // [CICLO 51] contrato `SistemaDeFicheros`; entre montajes distintos, EXDEV
        let (tipo_old, idx_old) = vfs::resolve_fs(old_path);
        let (tipo_new, idx_new) = vfs::resolve_fs(new_path);
        if idx_old != idx_new && (tipo_old != vfs::FsType::RamFs || tipo_new != vfs::FsType::RamFs) {
            return -18; // -EXDEV: montajes distintos (ramfs comparte una sola instancia)
        }
        return match vfs::con_fs(old_path, |fs| fs.renombrar(old_path, new_path)) { Some(Ok(())) => 0, Some(Err(e)) => -e, None => -2 };
    }

    sin_raiz(old_path);
    -19
}

fn sys_nanosleep(req_ptr: usize, rem_ptr: usize, current_rsp: u64) -> (i64, u64) {
    if !verify_user_buffer(req_ptr, core::mem::size_of::<Timespec>()) {
        return (-14, 0); // -EFAULT
    }
    let mut req = Timespec::default();
    let req_bytes = unsafe {
        core::slice::from_raw_parts_mut(&mut req as *mut Timespec as *mut u8, core::mem::size_of::<Timespec>())
    };
    if paging::copy_from_user(req_bytes, req_ptr, core::mem::size_of::<Timespec>()).is_err() {
        return (-14, 0);
    }

    if req.tv_sec < 0 || req.tv_nsec < 0 || req.tv_nsec >= 1_000_000_000 {
        return (-22, 0); // -EINVAL
    }

    let ticks_from_sec = (req.tv_sec as u64).saturating_mul(100);
    let ticks_from_nsec = ((req.tv_nsec as u64) + 9_999_999) / 10_000_000;
    let mut total_ticks = ticks_from_sec.saturating_add(ticks_from_nsec);
    if total_ticks == 0 && (req.tv_sec > 0 || req.tv_nsec > 0) {
        total_ticks = 1;
    }

    if rem_ptr != 0 && verify_user_buffer(rem_ptr, core::mem::size_of::<Timespec>()) {
        let zero_rem = Timespec::default();
        let rem_bytes = unsafe {
            core::slice::from_raw_parts(&zero_rem as *const Timespec as *const u8, core::mem::size_of::<Timespec>())
        };
        let _ = paging::copy_to_user(rem_ptr, rem_bytes);
    }

    if total_ticks == 0 {
        return (0, 0);
    }

    let new_rsp = sys_sleep_switch(total_ticks, current_rsp);
    (0, new_rsp)
}

fn sys_time(tloc_ptr: usize) -> i64 {
    // Tiempo de pared real (RTC CMOS + PIT), no el uptime.
    let sec = crate::time::rtc::now_epoch();
    if tloc_ptr != 0 && verify_user_buffer(tloc_ptr, core::mem::size_of::<i64>()) {
        let sec_bytes = sec.to_le_bytes();
        let _ = paging::copy_to_user(tloc_ptr, &sec_bytes);
    }
    sec
}

fn sys_mkdir(path_ptr: usize, _mode: u32) -> i64 {
    if !verify_user_string(path_ptr, 256) { return -14; }
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_cstr_from_user(&mut path_buf, path_ptr) {
        Ok(n) => n,
        Err(()) => return -14,
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let raw_rel = alloc::string::String::from(core::str::from_utf8(&path_buf[..actual_len]).unwrap_or(""));
    let actual_len = if raw_rel.is_empty() { 0 } else { resolver_con_cwd(&raw_rel, &mut path_buf) };
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    // [CICLO 49 / T1] `mkdir -p` de busybox recorre los componentes y espera
    // -EEXIST (17) para los que ya existen (incluida "/"); -1 (EPERM) lo aborta.
    if path == "/" || stat_path_internal(path).is_ok() {
        return -17;
    }
    if vfs::es_ruta_fs(path) {
        // [CICLO 51] contrato `SistemaDeFicheros`: EEXIST / ENOENT / ENOTDIR / ENOSPC exactos
        return match vfs::con_fs(path, |fs| fs.crear_directorio(path, 0)) { Some(Ok(())) => 0, Some(Err(e)) => -(e as i64), None => -2 };
    }

    // [T-001] Sin raiz montada bajo el contrato no hay sistema de ficheros
    // que sirva esta ruta: ENODEV explicito (la FAT de rescate fue podada).
    sin_raiz(path);
    -19i64
}

fn sys_ftruncate(fd: i32, length: i64) -> i64 {
    if length < 0 { return -22; } // -EINVAL
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    let entry = match process::fd_get_mut(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };

    match &mut entry.fd_type {
        process::FdType::FsFile(ref mut info) => {
            let path = match core::str::from_utf8(&info.path[..info.path_len]) {
                Ok(s) => s,
                Err(_) => return -14,
            };
            // [CICLO 51] contrato `SistemaDeFicheros`
            match vfs::con_fs(path, |fs| fs.truncar(path, length as u64)) {
                Some(Ok(())) => { info.size = length as u64; 0 }
                Some(Err(e)) => -(e as i64),
                None => -2,
            }
        }
        _ => -22, // EINVAL: fd does not refer to a regular file
    }
}

fn sys_truncate(path_ptr: usize, length: i64) -> i64 {
    if length < 0 { return -22; } // -EINVAL
    if !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14,
    };
    if vfs::es_ruta_fs(path) {
        // [CICLO 51] contrato `SistemaDeFicheros`
        return match vfs::con_fs(path, |fs| fs.truncar(path, length as u64)) { Some(Ok(())) => 0, Some(Err(e)) => -(e as i64), None => -2 };
    }

    // [T-001] Sin raiz montada bajo el contrato no hay sistema de ficheros
    // que sirva esta ruta: ENODEV explicito (la FAT de rescate fue podada).
    sin_raiz(path);
    -19
}

fn sys_rmdir(path_ptr: usize) -> i32 {
    if !verify_user_string(path_ptr, 256) {
        return -14;
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14,
    };
    if path.is_empty() || path == "/" || path == "/tmp" {
        return -22;
    }

    if vfs::es_ruta_fs(path) {
        // [CICLO 51] contrato `SistemaDeFicheros`: ENOENT / ENOTDIR / ENOTEMPTY exactos
        return match vfs::con_fs(path, |fs| fs.eliminar_directorio(path)) { Some(Ok(())) => 0, Some(Err(e)) => -e, None => -2 };
    }

    // [T-001] Sin raiz montada bajo el contrato no hay sistema de ficheros
    // que sirva esta ruta: ENODEV explicito (la FAT de rescate fue podada).
    sin_raiz(path);
    -19
}

fn sys_socket(domain: i32, sock_type: i32, protocol: i32) -> i64 {
    // [CICLO 50 / auditoria] errno discriminante: dominio desconocido -> EAFNOSUPPORT,
    // tipo desconocido -> EINVAL, tabla llena -> ENFILE, sin fd libre -> EMFILE.
    if domain != 1 && domain != 2 && domain != 10 { return -97; } // -EAFNOSUPPORT
    if sock_type != 1 && sock_type != 2 && sock_type != 5 { return -22; } // -EINVAL
    match crate::net::socket::socket_create(domain, sock_type, protocol) {
        Some(sock_id) => {
            let fd_entry = FdEntry::new(FdType::Socket(sock_id));
            if let Some(proc) = process::current_process() {
                if let Some(fd) = process::fd_alloc(proc, fd_entry) {
                    serial::write_str("[SYS] SOCKET created fd=");
                    serial::write_usize(fd);
                    serial::write_str("\n");
                    return fd as i64;
                }
            }
            crate::net::socket::socket_close(sock_id);
            -24 // -EMFILE
        }
        None => -23, // -ENFILE
    }
}


// [CICLO 47-c] Lee `sun_path` de un sockaddr_un de usuario (family AF_UNIX=1).
fn leer_sockaddr_un(addr_ptr: usize, addrlen: u32) -> Option<alloc::vec::Vec<u8>> {
    if addrlen < 3 || addrlen as usize > 110 { return None; }
    let mut raw = [0u8; 110];
    let n = addrlen as usize;
    if paging::copy_from_user(&mut raw[..n], addr_ptr, n).is_err() { return None; }
    if u16::from_ne_bytes([raw[0], raw[1]]) != 1 { return None; }
    let mut fin = 2;
    while fin < n && raw[fin] != 0 { fin += 1; }
    if fin == 2 { return None; }
    Some(raw[2..fin].to_vec())
}

fn sys_bind(fd: i32, addr_ptr: usize, addrlen: u32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; }
    // [CICLO 47-c] AF_UNIX con nombre
    if let Some(proc) = process::current_process() {
        if let Some(entry) = process::fd_get(proc, fd as usize) {
            if let FdType::Socket(sock_id) = entry.fd_type {
                if crate::net::socket::socket_domain(sock_id) == 1 {
                    if !verify_user_buffer(addr_ptr, addrlen as usize) { return -14; }
                    return match leer_sockaddr_un(addr_ptr, addrlen) {
                        Some(ruta) => match crate::net::socket::unix_bind(sock_id, &ruta) { Ok(()) => 0, Err(e) => e as i64 },
                        None => -22,
                    };
                }
            }
        }
    }
    if addrlen < 8 { return -22; } // -EINVAL
    if !verify_user_buffer(addr_ptr, addrlen as usize) {
        return -14; // -EFAULT
    }
    let mut saddr = [0u8; 16];
    let to_copy = (addrlen as usize).min(16);
    if paging::copy_from_user(&mut saddr[..to_copy], addr_ptr, to_copy).is_err() {
        return -14;
    }
    let port = u16::from_be_bytes([saddr[2], saddr[3]]);

    if let Some(proc) = process::current_process() {
        if let Some(entry) = process::fd_get(proc, fd as usize) {
            if let FdType::Socket(sock_id) = entry.fd_type {
                if crate::net::socket::socket_bind(sock_id, port) {
                    serial::write_str("[SYS] SOCKET fd=");
                    serial::write_usize(fd as usize);
                    serial::write_str(" bound to port ");
                    serial::write_usize(port as usize);
                    serial::write_str("\n");
                    return 0;
                }
            }
        }
    }
    -1
}

fn sys_sendto(
    fd: i32,
    buf_ptr: usize,
    len: usize,
    _flags: i32,
    dest_addr_ptr: usize,
    addrlen: u32,
) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; }
    if len == 0 || !verify_user_buffer(buf_ptr, len) {
        return -14;
    }
    let mut payload = alloc::vec![0u8; len];
    if paging::copy_from_user(&mut payload, buf_ptr, len).is_err() {
        return -14;
    }

    let mut dst_ip = crate::net::IpAddr::V4([10, 0, 2, 2]);
    let mut dst_port = 80u16;

    if dest_addr_ptr != 0 && addrlen >= 8 {
        let mut saddr = [0u8; 28];
        let to_copy = (addrlen as usize).min(28);
        if paging::copy_from_user(&mut saddr[..to_copy], dest_addr_ptr, to_copy).is_ok() {
            let family = u16::from_le_bytes([saddr[0], saddr[1]]);
            dst_port = u16::from_be_bytes([saddr[2], saddr[3]]);
            if family == 10 && to_copy >= 24 {
                // [IPv6 REAL] sockaddr_in6: dirección en [8..24]
                let mut a = [0u8; 16]; a.copy_from_slice(&saddr[8..24]);
                dst_ip = crate::net::IpAddr::V6(crate::net::ipv6::Ipv6Addr(a));
            } else {
                dst_ip = crate::net::IpAddr::V4([saddr[4], saddr[5], saddr[6], saddr[7]]);
            }
        }
    }

    if let Some(proc) = process::current_process() {
        if let Some(entry) = process::fd_get(proc, fd as usize) {
            if let FdType::Socket(sock_id) = entry.fd_type {
                if dest_addr_ptr == 0 {
                    return match crate::net::socket::socket_write(sock_id, &payload) {
                        Ok(sent) => sent as i64,
                        Err(e) => e as i64,
                    };
                } else {
                    match crate::net::socket::socket_sendto(sock_id, &payload, dst_ip, dst_port) {
                        Ok(sent) => return sent as i64,
                        Err(_) => return -22,
                    }
                }
            }
        }
    }
    -1
}

fn sys_recvfrom(
    fd: i32,
    buf_ptr: usize,
    len: usize,
    flags: i32,
    src_addr_ptr: usize,
    addrlen_ptr: usize,
) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; } // -EBADF
    if len == 0 || !verify_user_buffer(buf_ptr, len) {
        return -14; // -EFAULT
    }

    if let Some(proc) = process::current_process() {
        if let Some(entry) = process::fd_get(proc, fd as usize) {
            if let FdType::Socket(sock_id) = entry.fd_type {
                crate::net::poll();

                let mut kbuf = alloc::vec![0u8; len];
                // [IPv6] Para UDP se espera aquí (hasta 5 s) con `socket_recvfrom`, que
                // conserva la dirección de origen (IPv4 o IPv6) del datagrama; antes la
                // espera caía en `socket_read` y el origen se perdía (sockaddr a ceros).
                let mut primero = crate::net::socket::socket_recvfrom(sock_id, &mut kbuf);
                if primero.is_none() && (flags & 0x40) == 0 && crate::net::socket::socket_is_dgram(sock_id) {
                    let inicio = crate::time::pit::ticks();
                    while primero.is_none() {
                        let e = crate::net::socket::socket_take_error(sock_id);
                        if e != 0 { return e as i64; }
                        if crate::time::pit::ticks().wrapping_sub(inicio) > 500 { return -11; }
                        unsafe { crate::process::scheduler::wait_hlt_preemptible(); }
                        crate::net::poll();
                        primero = crate::net::socket::socket_recvfrom(sock_id, &mut kbuf);
                    }
                }
                if let Some((n, src_ip, src_port)) = primero {
                    if paging::copy_to_user(buf_ptr, &kbuf[..n]).is_err() {
                        return -14;
                    }
                    if src_addr_ptr != 0 && addrlen_ptr != 0 {
                        match src_ip {
                            crate::net::IpAddr::V4(a4) => {
                                let mut saddr = [0u8; 16];
                                saddr[0] = 2; // AF_INET lo
                                saddr[1] = 0; // AF_INET hi
                                saddr[2] = (src_port >> 8) as u8;
                                saddr[3] = (src_port & 0xFF) as u8;
                                saddr[4..8].copy_from_slice(&a4);
                                let _ = paging::copy_to_user(src_addr_ptr, &saddr);
                                let sixteen = 16u32;
                                let _ = paging::copy_to_user(addrlen_ptr, &sixteen.to_le_bytes());
                            }
                            crate::net::IpAddr::V6(a6) => {
                                // [IPv6 REAL] sockaddr_in6 sólo si el usuario reservó 28 bytes
                                let mut cap = [0u8; 4];
                                let _ = paging::copy_from_user(&mut cap, addrlen_ptr, 4);
                                if u32::from_le_bytes(cap) >= 28 {
                                    let mut saddr = [0u8; 28];
                                    saddr[0] = 10; saddr[2] = (src_port >> 8) as u8; saddr[3] = (src_port & 0xFF) as u8;
                                    saddr[8..24].copy_from_slice(&a6.0);
                                    let _ = paging::copy_to_user(src_addr_ptr, &saddr);
                                    let _ = paging::copy_to_user(addrlen_ptr, &28u32.to_le_bytes());
                                }
                            }
                        }
                    }
                    return n as i64;
                }

                if (flags & 0x40) != 0 {
                    return -11; // -EAGAIN
                }

                // [FRENTE B] Lectura TCP bloqueante real: se procesa la red
                // hasta que haya datos, llegue el FIN (EOF = 0) o venza el
                // plazo de 5 s (-EAGAIN). Con MSG_DONTWAIT se devuelve al instante.
                let inicio = crate::time::pit::ticks();
                let mut resultado = crate::net::socket::socket_read(sock_id, &mut kbuf);
                while let Err(-11) = resultado {
                    // [IPv6] ICMPv6 destino inalcanzable → error inmediato (no 5 s de espera)
                    let e = crate::net::socket::socket_take_error(sock_id);
                    if e != 0 { return e as i64; }
                    if crate::time::pit::ticks().wrapping_sub(inicio) > 500 { break; }
                    unsafe { crate::process::scheduler::wait_hlt_preemptible(); }
                    crate::net::poll();
                    resultado = crate::net::socket::socket_read(sock_id, &mut kbuf);
                }
                match resultado {
                    Ok(n) => {
                        if paging::copy_to_user(buf_ptr, &kbuf[..n]).is_err() {
                            return -14;
                        }
                        if src_addr_ptr != 0 && addrlen_ptr != 0 {
                            let (src_ip, src_port) = crate::net::socket::socket_get_peer(sock_id).unwrap_or(([0,0,0,0], 0));
                            let mut saddr = [0u8; 16];
                            saddr[0] = 2; // AF_INET lo
                            saddr[1] = 0; // AF_INET hi
                            saddr[2] = (src_port >> 8) as u8;
                            saddr[3] = (src_port & 0xFF) as u8;
                            saddr[4..8].copy_from_slice(&src_ip);
                            let _ = paging::copy_to_user(src_addr_ptr, &saddr);
                            let sixteen = 16u32;
                            let _ = paging::copy_to_user(addrlen_ptr, &sixteen.to_le_bytes());
                        }
                        return n as i64;
                    }
                    Err(e) => {
                        if (flags & 0x40) != 0 {
                            return -11; // -EAGAIN
                        }
                        return e as i64;
                    }
                }
            }
        }
    }
    -9
}

// ── SYS_INIT_MODULE ──────────────────────────────────────────────────────

fn sys_init_module(img_ptr: usize, len: usize, param_ptr: usize) -> i64 {
    if !verify_user_buffer(img_ptr, len) || len == 0 || len > 2 * 1024 * 1024 {
        return -14;
    }
    let mut kbuf = alloc::vec![0u8; len];
    if paging::copy_from_user(&mut kbuf, img_ptr, len).is_err() {
        return -14;
    }

    let mut name_buf = [0u8; 64];
    let mut mod_name_opt = None;
    if param_ptr != 0 && verify_user_string(param_ptr, 64) {
        if let Ok(n) = paging::copy_from_user(&mut name_buf, param_ptr, 64) {
            let act_len = name_buf.iter().position(|&b| b == 0).unwrap_or(n);
            if let Ok(s) = core::str::from_utf8(&name_buf[..act_len]) {
                if !s.is_empty() {
                    mod_name_opt = Some(s);
                }
            }
        }
    }

    match crate::lkm::load_module(&kbuf, mod_name_opt) {
        Ok(_) => 0,
        Err(e) => {
            serial::write_str("[SYSCALL] sys_init_module error: ");
            serial::write_str(e);
            serial::write_str("\n");
            -1
        }
    }
}

// ── SYS_DELETE_MODULE ────────────────────────────────────────────────────

fn sys_delete_module(name_ptr: usize, _flags: u32) -> i64 {
    if !verify_user_string(name_ptr, 64) {
        return -14;
    }
    let mut name_buf = [0u8; 64];
    let n = match paging::copy_from_user(&mut name_buf, name_ptr, 64) {
        Ok(n) => n,
        Err(_) => return -14,
    };
    let act_len = name_buf.iter().position(|&b| b == 0).unwrap_or(n);
    let Ok(mod_name) = core::str::from_utf8(&name_buf[..act_len]) else {
        return -14;
    };

    match crate::lkm::unload_module(mod_name) {
        Ok(_) => 0,
        Err(e) => {
            serial::write_str("[SYSCALL] sys_delete_module error: ");
            serial::write_str(e);
            serial::write_str("\n");
            -2 // -ENOENT: modulo no cargado
        }
    }
}

// ── SYS_QUERY_MODULE ─────────────────────────────────────────────────────

fn sys_query_module(buf_ptr: usize, buf_len: usize) -> i64 {
    if !verify_user_buffer(buf_ptr, buf_len) || buf_len == 0 {
        return -14;
    }
    let text = crate::lkm::list_modules_text();
    let bytes = text.as_bytes();
    let to_copy = bytes.len().min(buf_len);
    if paging::copy_to_user(buf_ptr, &bytes[..to_copy]).is_err() {
        return -14;
    }
    to_copy as i64
}

// ── SYS_GET_MOUSE & SYS_POLL_KEY ─────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UserMouseEvent {
    pub x: i32,
    pub y: i32,
    pub buttons: u8,
    pub scroll_delta: i32,
}

fn sys_get_mouse(ptr: usize) -> i64 {
    if !verify_user_buffer(ptr, core::mem::size_of::<UserMouseEvent>()) {
        return -14;
    }
    crate::drivers::input::mouse::poll_global_mouse();
    let guard = crate::drivers::input::mouse::GLOBAL_MOUSE.lock();
    let ev = UserMouseEvent {
        x: guard.x,
        y: guard.y,
        buttons: guard.buttons,
        scroll_delta: guard.scroll_delta,
    };
    drop(guard);

    let bytes = unsafe {
        core::slice::from_raw_parts(
            &ev as *const UserMouseEvent as *const u8,
            core::mem::size_of::<UserMouseEvent>(),
        )
    };
    if paging::copy_to_user(ptr, bytes).is_err() {
        return -14;
    }
    0
}

/// SYS_FB_INFO: copia a Ring 3 el modo REAL del LFB (leído del hardware VBE/VESA).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UserFbInfo { pub phys: u64, pub width: u32, pub height: u32, pub bpp: u32, pub pitch: u32 }

fn sys_fb_info(ptr: usize) -> i64 {
    if !verify_user_buffer(ptr, core::mem::size_of::<UserFbInfo>()) { return -14; }
    let m = *crate::graphics::driver::framebuffer::FB_MODE.lock();
    if m.width == 0 { return -19; } // ENODEV: sin framebuffer lineal
    let info = UserFbInfo { phys: m.phys, width: m.width as u32, height: m.height as u32, bpp: m.bpp as u32, pitch: m.pitch as u32 };
    let bytes = unsafe { core::slice::from_raw_parts(&info as *const UserFbInfo as *const u8, core::mem::size_of::<UserFbInfo>()) };
    if paging::copy_to_user(ptr, bytes).is_err() { return -14; }
    0
}

fn sys_poll_key() -> i64 {
    // Devuelve el byte leido o -1 como centinela de "sin tecla" (no es errno; la prueba 2296 lo exige exacto).
    // 1. Check serial COM1
    if let Some(b) = crate::process::tty_read_byte() {
        let ch = if b == b'\r' { b'\n' } else { b };
        return ch as i64;
    }
    // 2. Check keyboard scancode ring buffer
    if let Some(sc) = crate::arch::isr_handlers::pop_ring3_scancode() {
        static KBD: crate::arch::spinlock::Spinlock<crate::drivers::input::keyboard::KeyboardState> =
            crate::arch::spinlock::Spinlock::new(crate::drivers::input::keyboard::KeyboardState::new());
        let mut kbd = KBD.lock();
        if let Some(key) = kbd.feed_byte(sc) {
            let ch: u8 = match key {
                crate::drivers::input::keyboard::Key::Char(c)   => c,
                crate::drivers::input::keyboard::Key::Enter     => b'\n',
                crate::drivers::input::keyboard::Key::Backspace => 0x08,
                crate::drivers::input::keyboard::Key::Tab       => b'\t',
                crate::drivers::input::keyboard::Key::Escape    => 0x1B,
                _ => 0,
            };
            if ch != 0 {
                return ch as i64;
            }
        }
    }
    -1
}

fn check_fd_poll(proc: &process::Process, fd: i32, events: i16) -> i16 {
    if fd < 0 {
        return 0; // POSIX ignores negative fd
    }
    let ufd = fd as usize;
    if ufd >= process::MAX_FDS {
        return POLLNVAL;
    }
    let entry = match &process::tabla_fds(proc)[ufd] {
        Some(e) => e,
        None => return POLLNVAL,
    };

    let mut revents = 0i16;

    match entry.fd_type {
        process::FdType::Stdin => {
            if crate::drivers::serial::rx_ready() {
                revents |= POLLIN;
            }
        }
        process::FdType::Stdout | process::FdType::Stderr => {
            revents |= POLLOUT;
        }
        process::FdType::FsFile(_) | process::FdType::ProcFile(_) => {
            revents |= POLLIN | POLLOUT;
        }
        process::FdType::Device(info) => {
            match info.dev_type {
                process::DeviceType::Kbd => {
                    if crate::drivers::serial::rx_ready() {
                        revents |= POLLIN;
                    }
                }
                process::DeviceType::Null | process::DeviceType::Fb | process::DeviceType::Sda | process::DeviceType::Zero | process::DeviceType::Urandom | process::DeviceType::Dsp => {
                    revents |= POLLIN | POLLOUT;
                }
            }
        }
        process::FdType::PipeRead(pipe_id) => {
            let ptab = process::PIPE_TABLE.lock();
            if let Some(p) = ptab.pipes.get(pipe_id as usize) {
                if !p.in_use {
                    revents |= POLLHUP;
                } else {
                    if p.count > 0 {
                        revents |= POLLIN;
                    }
                    if p.writers == 0 {
                        revents |= POLLHUP;
                    }
                }
            } else {
                revents |= POLLNVAL;
            }
        }
        process::FdType::PipeWrite(pipe_id) => {
            let ptab = process::PIPE_TABLE.lock();
            if let Some(p) = ptab.pipes.get(pipe_id as usize) {
                if !p.in_use || p.readers == 0 {
                    revents |= POLLERR;
                } else if p.count < process::PIPE_BUF_SIZE {
                    revents |= POLLOUT;
                }
            } else {
                revents |= POLLNVAL;
            }
        }
        process::FdType::Socket(sock_id) => {
            let (can_read, can_write) = crate::net::socket::socket_poll_status(sock_id);
            if can_read {
                revents |= POLLIN;
            }
            if can_write {
                revents |= POLLOUT;
            }
        }
        process::FdType::SocketPairEnd { read_pipe, write_pipe } => {
            let ptab = process::PIPE_TABLE.lock();
            if let Some(p) = ptab.pipes.get(read_pipe as usize) {
                if p.count > 0 {
                    revents |= POLLIN;
                }
                if p.writers == 0 {
                    revents |= POLLHUP;
                }
            }
            if let Some(p) = ptab.pipes.get(write_pipe as usize) {
                if p.count < process::PIPE_BUF_SIZE {
                    revents |= POLLOUT;
                }
                if p.readers == 0 {
                    revents |= POLLERR;
                }
            }
        }
        process::FdType::EventFd(id) => {
            let table = process::EVENTFD_TABLE.lock();
            if let Some(ef) = table.fds.get(id as usize) {
                if ef.val > 0 {
                    revents |= POLLIN;
                }
                if ef.val < u64::MAX - 1 {
                    revents |= POLLOUT;
                }
            }
        }
        process::FdType::Epoll(_) => {}
        process::FdType::TimerFd(id) => {
            let table = process::TIMERFD_TABLE.lock();
            if let Some(tf) = table.fds.get(id as usize) {
                let cur = crate::time::pit::ticks();
                if tf.expirations > 0 || (tf.expire_ticks > 0 && cur >= tf.expire_ticks) {
                    revents |= POLLIN;
                }
            }
        }
        process::FdType::SignalFd(id) => {
            let table = process::SIGNALFD_TABLE.lock();
            if let Some(sf) = table.fds.get(id as usize) {
                if (process::current_sig_pending() & sf.mask) != 0 {
                    revents |= POLLIN;
                }
            }
        }
        process::FdType::PtyMaster(id) => {
            let ptab = process::PTY_TABLE.lock();
            if let Some(pty) = ptab.ptys.get(id as usize) {
                let pipes = process::PIPE_TABLE.lock();
                if let Some(p) = pipes.pipes.get(pty.master_pipe as usize) {
                    if p.count > 0 { revents |= POLLIN; }
                }
                if let Some(p) = pipes.pipes.get(pty.slave_pipe as usize) {
                    if p.count < process::PIPE_BUF_SIZE { revents |= POLLOUT; }
                }
            }
        }
        process::FdType::PtySlave(id) => {
            let ptab = process::PTY_TABLE.lock();
            if let Some(pty) = ptab.ptys.get(id as usize) {
                let pipes = process::PIPE_TABLE.lock();
                if let Some(p) = pipes.pipes.get(pty.slave_pipe as usize) {
                    if p.count > 0 { revents |= POLLIN; }
                }
                if let Some(p) = pipes.pipes.get(pty.master_pipe as usize) {
                    if p.count < process::PIPE_BUF_SIZE { revents |= POLLOUT; }
                }
            }
        }
        process::FdType::PidFd(target_pid) => {
            if let Some(target) = process::process_by_pid(target_pid as u64) {
                if target.state == process::ProcessState::Dead || target.state == process::ProcessState::Zombie {
                    revents |= POLLIN;
                }
            } else {
                revents |= POLLIN | POLLHUP;
            }
        }
        process::FdType::Inotify(id) => {
            if process::inotify_has_events(id) { revents |= POLLIN; }
        }
        process::FdType::IoUring(_) | process::FdType::Mq(_) | process::FdType::Fanotify(_) => {
            revents |= POLLIN | POLLOUT;
        }
    }

    (revents & events) | (revents & (POLLERR | POLLHUP | POLLNVAL))
}

fn sys_poll(fds_ptr: usize, nfds: usize, timeout_ms: i32) -> i32 {
    if nfds == 0 {
        if timeout_ms > 0 {
            let ticks = ((timeout_ms as u64) + 9) / 10;
            let target = crate::time::pit::ticks().wrapping_add(ticks);
            while crate::time::pit::ticks() < target {
                unsafe {
                    crate::process::scheduler::wait_hlt_preemptible();
                }
            }
        }
        return 0;
    }

    if fds_ptr == 0 || !verify_user_buffer(fds_ptr, nfds * core::mem::size_of::<PollFd>()) {
        return -14; // -EFAULT
    }

    if nfds > 256 {
        return -22; // -EINVAL
    }

    let mut kfds = alloc::vec![PollFd::default(); nfds];
    unsafe {
        let src = fds_ptr as *const PollFd;
        for i in 0..nfds {
            kfds[i] = core::ptr::read_unaligned(src.add(i));
        }
    }

    let proc_ptr = unsafe { process::current_raw() };
    if proc_ptr.is_null() { return -3; }
    let proc = unsafe { &*proc_ptr };

    let start_ticks = crate::time::pit::ticks();
    let wait_ticks = if timeout_ms > 0 {
        ((timeout_ms as u64) + 9) / 10
    } else {
        0
    };
    let target_ticks = start_ticks.wrapping_add(wait_ticks);

    loop {
        let mut ready_count = 0;
        for i in 0..nfds {
            kfds[i].revents = check_fd_poll(proc, kfds[i].fd, kfds[i].events);
            if kfds[i].revents != 0 {
                ready_count += 1;
            }
        }

        if ready_count > 0 || timeout_ms == 0 {
            unsafe {
                let dst = fds_ptr as *mut PollFd;
                for i in 0..nfds {
                    core::ptr::write_unaligned(dst.add(i), kfds[i]);
                }
            }
            return ready_count;
        }

        if timeout_ms > 0 && crate::time::pit::ticks() >= target_ticks {
            unsafe {
                let dst = fds_ptr as *mut PollFd;
                for i in 0..nfds {
                    core::ptr::write_unaligned(dst.add(i), kfds[i]);
                }
            }
            return 0;
        }

        unsafe {
            crate::process::scheduler::wait_hlt_preemptible();
        }
    }
}

fn sys_clock_gettime(clk_id: i32, tp_ptr: usize) -> i32 {
    if tp_ptr == 0 || !verify_user_buffer(tp_ptr, core::mem::size_of::<Timespec>()) {
        return -14; // -EFAULT
    }

    let ticks = crate::time::pit::ticks();
    let ts = match clk_id {
        CLOCK_REALTIME => {
            Timespec {
                tv_sec: crate::time::rtc::now_epoch(), // RTC CMOS real
                tv_nsec: ((ticks % 100) * 10_000_000) as i64,
            }
        }
        CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW => {
            Timespec {
                tv_sec: (ticks / 100) as i64,
                tv_nsec: ((ticks % 100) * 10_000_000) as i64,
            }
        }
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            let used = process::current_process().map(|p| p.ticks_used).unwrap_or(0);
            Timespec {
                tv_sec: (used / 100) as i64,
                tv_nsec: ((used % 100) * 10_000_000) as i64,
            }
        }
        _ => return -22, // -EINVAL
    };

    let ts_bytes = unsafe {
        core::slice::from_raw_parts(&ts as *const Timespec as *const u8, core::mem::size_of::<Timespec>())
    };
    if paging::copy_to_user(tp_ptr, ts_bytes).is_err() {
        return -14;
    }
    0
}

fn sys_clock_getres(clk_id: i32, res_ptr: usize) -> i32 {
    if clk_id < 0 || clk_id > 4 {
        return -22; // -EINVAL
    }
    if res_ptr != 0 {
        if !verify_user_buffer(res_ptr, core::mem::size_of::<Timespec>()) {
            return -14; // -EFAULT
        }
        let res = Timespec {
            tv_sec: 0,
            tv_nsec: 10_000_000,
        };
        let res_bytes = unsafe {
            core::slice::from_raw_parts(&res as *const Timespec as *const u8, core::mem::size_of::<Timespec>())
        };
        if paging::copy_to_user(res_ptr, res_bytes).is_err() {
            return -14;
        }
    }
    0
}

fn sys_statvfs(path_ptr: usize, buf_ptr: usize) -> i32 {
    if buf_ptr == 0 || !verify_user_buffer(buf_ptr, core::mem::size_of::<Statvfs>()) {
        return -14; // -EFAULT
    }
    if !verify_user_string(path_ptr, 256) {
        return -14;
    }
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_cstr_from_user(&mut path_buf, path_ptr) {
        Ok(n) => n,
        Err(()) => return -14,
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let raw_rel = alloc::string::String::from(core::str::from_utf8(&path_buf[..actual_len]).unwrap_or(""));
    let actual_len = if raw_rel.is_empty() { 0 } else { resolver_con_cwd(&raw_rel, &mut path_buf) };
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    let exists = if path == "/" || path == "." {
        true
    } else if crate::drivers::storage::procfs::is_procfs_path(path) {
        crate::drivers::storage::procfs::resolve_proc_path(path).is_some()
    } else if vfs::es_ruta_fs(path) {
        vfs::meta_fs(path).is_some()
    } else {
        false
    };

    if !exists {
        return -2; // -ENOENT
    }

    let bsize = 4096u64;
    let st = Statvfs {
        f_bsize: bsize,
        f_frsize: bsize,
        f_blocks: 16384,
        f_bfree: 12288,
        f_bavail: 12288,
        f_files: 1024,
        f_ffree: 1000,
        f_favail: 1000,
        f_fsid: 0x505458, // 'PTX'
        f_flag: 0,
        f_namemax: 255,
    };

    let st_bytes = unsafe {
        core::slice::from_raw_parts(&st as *const Statvfs as *const u8, core::mem::size_of::<Statvfs>())
    };
    if paging::copy_to_user(buf_ptr, st_bytes).is_err() {
        return -14;
    }
    0
}

// ─────────────────────────────────────────────────────────────────────────────
//  [ÉPICA 58] rt_sigreturn — restauración del contexto tras un manejador
// ─────────────────────────────────────────────────────────────────────────────
/// Retorno desde un manejador de señales. `user_frame` es la dirección del
/// `SigFrame` en el stack de usuario (0 = deducirla del RSP guardado).
///
/// El valor devuelto en RAX es el que el propio frame de señales restaura,
/// porque el stub de ensamblador reescribe el RAX del frame con el resultado
/// del despachador: hay que devolver exactamente el RAX restaurado.
fn sys_rt_sigreturn(user_frame: usize, current_rsp: u64) -> SyscallResult {
    let mut rsp = user_frame as u64;
    if rsp == 0 && current_rsp != 0 {
        rsp = unsafe {
            core::ptr::read_unaligned((current_rsp as *const u64).add(50)) // FRAME_RSP / 8 = 50
        };
    }
    match crate::process::signals::sigreturn_restore(rsp) {
        Ok(()) => {
            crate::process::signals::note_syscall_sigreturn();
            serial::write_str("[SIG] rt_sigreturn (syscall) -> contexto restaurado\n");
            SyscallResult(crate::process::signals::frame_rax(current_rsp), 0)
        }
        Err(e) => {
            serial::write_str("[SIG] rt_sigreturn: frame invalido (EINVAL)\n");
            SyscallResult(e as u64, 0)
        }
    }
}

fn sys_sigaction(signum: i32, act_ptr: usize, oldact_ptr: usize) -> i32 {
    let act = if act_ptr != 0 {
        if !verify_user_buffer(act_ptr, core::mem::size_of::<SigAction>()) {
            return -14; // -EFAULT
        }
        let mut kact = SigAction::default();
        let kbytes = unsafe {
            core::slice::from_raw_parts_mut(&mut kact as *mut SigAction as *mut u8, core::mem::size_of::<SigAction>())
        };
        if paging::copy_from_user(kbytes, act_ptr, core::mem::size_of::<SigAction>()).is_err() {
            return -14;
        }
        Some((kact.sa_handler, kact.sa_flags, kact.sa_mask))
    } else {
        None
    };

    match crate::process::sigaction(signum, act) {
        Ok((old_h, old_f, old_m)) => {
            if oldact_ptr != 0 {
                if !verify_user_buffer(oldact_ptr, core::mem::size_of::<SigAction>()) {
                    return -14;
                }
                let oldact = SigAction {
                    sa_handler: old_h,
                    sa_flags: old_f,
                    sa_mask: old_m,
                };
                let bytes = unsafe {
                    core::slice::from_raw_parts(&oldact as *const SigAction as *const u8, core::mem::size_of::<SigAction>())
                };
                if paging::copy_to_user(oldact_ptr, bytes).is_err() {
                    return -14;
                }
            }
            0
        }
        Err(e) => e,
    }
}

fn sys_sigprocmask(how: i32, set_ptr: usize, oldset_ptr: usize) -> i32 {
    let set = if set_ptr != 0 {
        if !verify_user_buffer(set_ptr, core::mem::size_of::<u64>()) {
            return -14;
        }
        let mut kset = 0u64;
        let kbytes = unsafe {
            core::slice::from_raw_parts_mut(&mut kset as *mut u64 as *mut u8, 8)
        };
        if paging::copy_from_user(kbytes, set_ptr, 8).is_err() {
            return -14;
        }
        Some(kset)
    } else {
        None
    };

    match crate::process::sigprocmask(how, set) {
        Ok((old, _cur)) => {
            if oldset_ptr != 0 {
                if !verify_user_buffer(oldset_ptr, core::mem::size_of::<u64>()) {
                    return -14;
                }
                let old_bytes = old.to_le_bytes();
                if paging::copy_to_user(oldset_ptr, &old_bytes).is_err() {
                    return -14;
                }
            }
            0
        }
        Err(e) => e,
    }
}

fn sys_sigpending(set_ptr: usize) -> i32 {
    if set_ptr == 0 || !verify_user_buffer(set_ptr, core::mem::size_of::<u64>()) {
        return -14; // -EFAULT
    }
    match crate::process::sigpending() {
        Ok(pend) => {
            let bytes = pend.to_le_bytes();
            if paging::copy_to_user(set_ptr, &bytes).is_err() {
                return -14;
            }
            0
        }
        Err(e) => e,
    }
}

fn sys_getrlimit(resource: i32, rlim_ptr: usize) -> i32 {
    if rlim_ptr == 0 || !verify_user_buffer(rlim_ptr, core::mem::size_of::<Rlimit>()) {
        return -14;
    }
    match crate::process::getrlimit(resource) {
        Ok((soft, hard)) => {
            let rlim = Rlimit { rlim_cur: soft, rlim_max: hard };
            let bytes = unsafe {
                core::slice::from_raw_parts(&rlim as *const Rlimit as *const u8, core::mem::size_of::<Rlimit>())
            };
            if paging::copy_to_user(rlim_ptr, bytes).is_err() {
                return -14;
            }
            0
        }
        Err(e) => e,
    }
}

fn sys_setrlimit(resource: i32, rlim_ptr: usize) -> i32 {
    if rlim_ptr == 0 || !verify_user_buffer(rlim_ptr, core::mem::size_of::<Rlimit>()) {
        return -14;
    }
    let mut rlim = Rlimit::default();
    let rlim_bytes = unsafe {
        core::slice::from_raw_parts_mut(&mut rlim as *mut Rlimit as *mut u8, core::mem::size_of::<Rlimit>())
    };
    if paging::copy_from_user(rlim_bytes, rlim_ptr, core::mem::size_of::<Rlimit>()).is_err() {
        return -14;
    }
    match crate::process::setrlimit(resource, rlim.rlim_cur, rlim.rlim_max) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

fn sys_readlink(path_ptr: usize, buf_ptr: usize, bufsiz: usize) -> i64 {
    if bufsiz == 0 { return -22; }
    if buf_ptr == 0 || !verify_user_buffer(buf_ptr, bufsiz) {
        return -14;
    }
    if !verify_user_string(path_ptr, 256) {
        return -14;
    }
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_cstr_from_user(&mut path_buf, path_ptr) {
        Ok(n) => n,
        Err(()) => return -14,
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let raw_rel = alloc::string::String::from(core::str::from_utf8(&path_buf[..actual_len]).unwrap_or(""));
    let actual_len = if raw_rel.is_empty() { 0 } else { resolver_con_cwd(&raw_rel, &mut path_buf) };
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    if path == "/proc/self/exe" {
        if let Some(proc) = process::current_process() {
            // [b191] Ruta real registrada en exec; si falta (procesos integrados), `/bin/<nombre>`.
            let registrada = proc.exe_path_str();
            let exe_path = if registrada.starts_with('/') { alloc::string::String::from(registrada) } else { alloc::format!("/bin/{}", proc.name_str()) };
            let to_copy = exe_path.len().min(bufsiz);
            if paging::copy_to_user(buf_ptr, &exe_path.as_bytes()[..to_copy]).is_ok() {
                return to_copy as i64;
            } else {
                return -14;
            }
        }
    }

    if let Some(target) = vfs::readlink_universal(path) {
        let to_copy = target.len().min(bufsiz);
        if paging::copy_to_user(buf_ptr, &target.as_bytes()[..to_copy]).is_ok() {
            return to_copy as i64;
        } else {
            return -14;
        }
    }

    if vfs::es_ruta_fs(path) {
        // [CICLO 51] contrato `SistemaDeFicheros`: EINVAL si no es enlace, ENOENT si no existe
        match vfs::con_fs(path, |fs| fs.leer_enlace(path)) {
            Some(Ok(destino)) => {
                let to_copy = destino.len().min(bufsiz);
                if paging::copy_to_user(buf_ptr, &destino.as_bytes()[..to_copy]).is_ok() { to_copy as i64 } else { -14 }
            }
            Some(Err(e)) => -(e as i64),
            None => -2,
        }
    } else {
        -22 // EINVAL on non-symlink filesystem
    }
}

fn sys_symlink(target_ptr: usize, linkpath_ptr: usize) -> i32 {
    if !verify_user_string(target_ptr, 256) || !verify_user_string(linkpath_ptr, 256) {
        return -14;
    }
    let mut tbuf = [0u8; 256];
    let tlen = match paging::copy_cstr_from_user(&mut tbuf, target_ptr) {
        Ok(n) => n, Err(()) => return -14,
    };
    let actual_tlen = tbuf.iter().position(|&b| b == 0).unwrap_or(tlen);
    let target = core::str::from_utf8(&tbuf[..actual_tlen]).unwrap_or("");

    let mut lbuf = [0u8; 256];
    let llen = match paging::copy_cstr_from_user(&mut lbuf, linkpath_ptr) {
        Ok(n) => n, Err(()) => return -14,
    };
    let actual_llen = lbuf.iter().position(|&b| b == 0).unwrap_or(llen);
    let linkpath = core::str::from_utf8(&lbuf[..actual_llen]).unwrap_or("");

    if target.is_empty() || linkpath.is_empty() {
        return -2; // -ENOENT
    }

    if vfs::es_ruta_fs(linkpath) {
        // [CICLO 51] contrato `SistemaDeFicheros`: EEXIST / ENOENT / ENOTDIR exactos
        match vfs::con_fs(linkpath, |fs| fs.enlace_simbolico(target, linkpath)) { Some(Ok(())) => 0, Some(Err(e)) => -e, None => -2 }
    } else {
        if vfs::create_symlink_universal(target, linkpath) {
            0
        } else {
            -17
        }
    }
}

fn sys_lstat(path_ptr: usize, statbuf_ptr: usize) -> i64 {
    if statbuf_ptr == 0 || !verify_user_buffer(statbuf_ptr, core::mem::size_of::<Stat>()) {
        return -14;
    }
    if !verify_user_string(path_ptr, 256) {
        return -14;
    }
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_cstr_from_user(&mut path_buf, path_ptr) {
        Ok(n) => n, Err(()) => return -14,
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let raw_rel = alloc::string::String::from(core::str::from_utf8(&path_buf[..actual_len]).unwrap_or(""));
    let actual_len = if raw_rel.is_empty() { 0 } else { resolver_con_cwd(&raw_rel, &mut path_buf) };
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    if vfs::es_ruta_fs(path) {
        // [CICLO 51] contrato `SistemaDeFicheros` (lstat no sigue el enlace: metadatos del propio nodo)
        let stat_res = match vfs::con_fs(path, |fs| fs.metadatos_sin_seguir(path)) {
            Some(Ok(m)) => {
                let mut st = Stat::default();
                st.st_mode = m.modo;
                st.st_uid = m.uid;
                st.st_gid = m.gid;
                st.st_size = m.tam as i64;
                st.st_ino = m.ino;
                st.st_nlink = m.nlink as u64;
                st.st_blksize = 4096;
                st.st_blocks = if m.bloques > 0 || m.tam == 0 { m.bloques as i64 } else { (st.st_size + 511) / 512 };
                Some(st)
            }
            _ => None,
        };
        if let Some(st) = stat_res {
            let bytes = unsafe {
                core::slice::from_raw_parts(&st as *const Stat as *const u8, core::mem::size_of::<Stat>())
            };
            if paging::copy_to_user(statbuf_ptr, bytes).is_err() {
                return -14;
            }
            return 0;
        }
    }

    // Fallback to normal stat
    sys_stat(path_ptr, statbuf_ptr) as i64
}

fn sys_times(buf_ptr: usize) -> i64 {
    if buf_ptr == 0 || !verify_user_buffer(buf_ptr, core::mem::size_of::<crate::process::Tms>()) {
        return -14; // -EFAULT
    }
    let mut tms = crate::process::Tms::default();
    let ticks = match crate::process::get_times(&mut tms) {
        Ok(t) => t,
        Err(e) => return e as i64,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(&tms as *const crate::process::Tms as *const u8, core::mem::size_of::<crate::process::Tms>())
    };
    if paging::copy_to_user(buf_ptr, bytes).is_err() {
        return -14;
    }
    ticks as i64
}

fn sys_getgroups(size: usize, list_ptr: usize) -> i64 {
    if size == 0 {
        let mut dummy = [0u32; 16];
        return match crate::process::getgroups(0, &mut dummy) {
            Ok(n) => n as i64,
            Err(e) => e as i64,
        };
    }
    if list_ptr == 0 || !verify_user_buffer(list_ptr, size * 4) {
        return -14; // -EFAULT
    }
    let mut list = [0u32; 16];
    match crate::process::getgroups(size, &mut list) {
        Ok(n) => {
            let bytes = unsafe {
                core::slice::from_raw_parts(list.as_ptr() as *const u8, n * 4)
            };
            if paging::copy_to_user(list_ptr, bytes).is_err() {
                return -14;
            }
            n as i64
        }
        Err(e) => e as i64,
    }
}

fn sys_setgroups(size: usize, list_ptr: usize) -> i64 {
    if size > 16 {
        return -22; // -EINVAL
    }
    if size > 0 && (list_ptr == 0 || !verify_user_buffer(list_ptr, size * 4)) {
        return -14; // -EFAULT
    }
    let mut list = [0u32; 16];
    if size > 0 {
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(list.as_mut_ptr() as *mut u8, size * 4)
        };
        if paging::copy_from_user(bytes, list_ptr, size * 4).is_err() {
            return -14;
        }
    }
    match crate::process::setgroups(&list[..size]) {
        Ok(()) => 0,
        Err(e) => e as i64,
    }
}

fn sys_fsync(fd: i32) -> i64 {
    if fd < 0 || fd >= process::MAX_FDS as i32 {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match &entry.fd_type {
        process::FdType::PipeRead(_) | process::FdType::PipeWrite(_) | process::FdType::Socket(_) | process::FdType::SocketPairEnd { .. } | process::FdType::EventFd(_) | process::FdType::Epoll(_) => {
            -22 // -EINVAL
        }
        // [T-001] fsync real: el sistema de ficheros vuelca superbloque/GDT y
        // ordena al disco vaciar su caché de escritura (ATA FLUSH CACHE).
        process::FdType::FsFile(info) => {
            let path = match core::str::from_utf8(&info.path[..info.path_len]) { Ok(p) => p, Err(_) => return -5 };
            match vfs::con_fs(path, |fs| fs.sincronizar()) {
                Some(Ok(())) => 0,
                Some(Err(e)) => -(e as i64),
                None => sys_sync(), // devfs/procfs/FAT RO: nada propio que volcar; vaciar el disco
            }
        }
        _ => sys_sync(),
    }
}

fn sys_fdatasync(fd: i32) -> i64 {
    sys_fsync(fd)
}

fn sys_chmod(path_ptr: usize, mode: u32) -> i64 {
    if !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14,
    };

    // [L3c] todo montaje bajo el contrato (raiz ext2 persiste el modo; /boot
    // devuelve EROFS); antes solo ramfs y la raiz ext2 "aceptaba" sin hacer nada.
    if vfs::es_ruta_fs(path) {
        let cur_euid = process::current_euid();
        let res = match vfs::meta_fs(path) {
            Some((_m, uid, ..)) if cur_euid != 0 && cur_euid != uid => Err(-1), // -EPERM
            Some(_) => vfs::chmod_fs(path, mode),
            None => Err(-2), // -ENOENT
        };
        return match res {
            Ok(()) => 0,
            Err(e) => e as i64,
        };
    }

    // In FAT32/VFS fallback
    0
}

fn sys_fchmod(fd: i32, mode: u32) -> i64 {
    if fd < 0 || fd >= process::MAX_FDS as i32 {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match &entry.fd_type {
        process::FdType::PipeRead(_) | process::FdType::PipeWrite(_) | process::FdType::Socket(_) | process::FdType::SocketPairEnd { .. } | process::FdType::EventFd(_) | process::FdType::Epoll(_) => {
            -22 // -EINVAL
        }
        process::FdType::FsFile(info) => {
            let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                Ok(s) => s,
                Err(_) => return -22,
            };
            let cur_euid = process::current_euid();
            let res = match vfs::meta_fs(path_str) {
                Some((_m, uid, ..)) if cur_euid != 0 && cur_euid != uid => Err(-1), // -EPERM
                Some(_) => vfs::chmod_fs(path_str, mode),
                None => Err(-2), // -ENOENT
            };
            match res {
                Ok(()) => 0,
                Err(e) => e as i64,
            }
        }
        _ => 0,
    }
}

fn sys_chown(path_ptr: usize, owner: u32, group: u32) -> i64 {
    if !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14,
    };
    // [T-001] Regla POSIX (como Linux sin CAP_CHOWN): root puede todo; el
    // propietario solo puede "cambiar" el uid a si mismo y el gid a su egid.
    let cur_euid = process::current_euid();
    if cur_euid != 0 {
        let m = match vfs::con_fs(path, |fs| fs.metadatos(path)) {
            Some(Ok(m)) => m,
            Some(Err(e)) => return -(e as i64),
            None => return if stat_path_internal(path).is_err() { -2 } else { -30 },
        };
        if m.uid != cur_euid { return -1; } // -EPERM: no es el dueño
        if owner != u32::MAX && owner != m.uid { return -1; }
        if group != u32::MAX && group != process::current_egid() && group != m.gid { return -1; }
    }

    // [T-001] Por el contrato: ramfs y ext2. Nada de "0 sin hacer nada".
    match vfs::con_fs(path, |fs| fs.cambiar_propietario(path, owner, group)) {
        Some(Ok(())) => 0,
        Some(Err(e)) => -(e as i64),
        None => if stat_path_internal(path).is_err() { -2 } else { -30 }, // EROFS: /boot FAT, devfs
    }
}

fn sys_fchown(fd: i32, owner: u32, group: u32) -> i64 {
    if fd < 0 || fd >= process::MAX_FDS as i32 {
        return -9; // -EBADF
    }
    let cur_euid = process::current_euid();
    if cur_euid != 0 {
        return -1; // -EPERM
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match &entry.fd_type {
        process::FdType::PipeRead(_) | process::FdType::PipeWrite(_) | process::FdType::Socket(_) | process::FdType::SocketPairEnd { .. } | process::FdType::EventFd(_) | process::FdType::Epoll(_) => {
            -22 // -EINVAL
        }
        process::FdType::FsFile(info) => {
            let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                Ok(s) => s,
                Err(_) => return -22,
            };
            let res = vfs::with_ramfs(|ram| ram.chown(path_str, owner, group));
            match res {
                Ok(()) => 0,
                Err(e) => e as i64,
            }
        }
        _ => 0,
    }
}

/// [T-001/FIFO] Espera a que el proceso actual tenga una señal entregable
/// (`sig_pending & !sig_blocked`). Si hay otro proceso listo, se cede el
/// procesador con reinicio transparente de la syscall (idioma de `waitpid`);
/// si no hay nadie más, se espera en el núcleo con `hlt` preemptible (idioma
/// de `sleep`), porque un bucle de reinicio desde Ring 3 no deja correr al
/// temporizador. Devuelve `(SYSCALL_RESTART|-EINTR, rsp_siguiente)`.
fn esperar_senal(current_rsp: u64) -> (i64, u64) {
    unsafe {
        let cur = process::current_raw();
        if cur.is_null() { return (-3, 0); }
        if (*cur).sig_pending & !(*cur).sig_blocked != 0 { return (-4, 0); }
        (*cur).state = process::ProcessState::Blocked;
        let cs = saved_cs_from_rsp(current_rsp);
        let new_rsp = process::schedule_desde_syscall(current_rsp, cs);
        if new_rsp != 0 {
            let reiniciar = rebobinar_syscall(current_rsp);
            return (if reiniciar { SYSCALL_RESTART } else { -4 }, new_rsp);
        }
        (*cur).state = process::ProcessState::Running;
        loop {
            if (*cur).sig_pending & !(*cur).sig_blocked != 0 { break; }
            process::wait_hlt_preemptible();
            crate::net::poll();
            process::wake_blocked();
            // Si mientras dormíamos nos marcaron Ready/Blocked, volvemos a Running:
            // el proceso sigue en su propio contexto de núcleo.
            if (*cur).state != process::ProcessState::Running { (*cur).state = process::ProcessState::Running; }
        }
        (-4, 0)
    }
}

/// [T-001/FIFO] `pause` real: bloquea hasta que haya una señal entregable
/// (antes cedía el procesador una vez y devolvía -EINTR sin esperar).
fn sys_pause(current_rsp: u64) -> (i64, u64) {
    esperar_senal(current_rsp)
}

fn sys_sched_yield(current_rsp: u64) -> u64 {
    let cs = saved_cs_from_rsp(current_rsp);
    process::schedule_desde_syscall(current_rsp, cs)
}

fn sys_getresuid(ruid_ptr: usize, euid_ptr: usize, suid_ptr: usize) -> i64 {
    if !verify_user_buffer(ruid_ptr, 4) || !verify_user_buffer(euid_ptr, 4) || !verify_user_buffer(suid_ptr, 4) {
        return -14; // -EFAULT
    }
    if let Some((r, e, s)) = process::getresuid() {
        if paging::copy_to_user(ruid_ptr, &r.to_ne_bytes()).is_err() ||
           paging::copy_to_user(euid_ptr, &e.to_ne_bytes()).is_err() ||
           paging::copy_to_user(suid_ptr, &s.to_ne_bytes()).is_err() {
            return -14;
        }
        0
    } else {
        -1
    }
}

fn sys_setresuid(ruid: u32, euid: u32, suid: u32) -> i64 {
    match process::setresuid(ruid, euid, suid) {
        Ok(()) => 0,
        Err(e) => e as i64,
    }
}

fn sys_getresgid(rgid_ptr: usize, egid_ptr: usize, sgid_ptr: usize) -> i64 {
    if !verify_user_buffer(rgid_ptr, 4) || !verify_user_buffer(egid_ptr, 4) || !verify_user_buffer(sgid_ptr, 4) {
        return -14; // -EFAULT
    }
    if let Some((r, e, s)) = process::getresgid() {
        if paging::copy_to_user(rgid_ptr, &r.to_ne_bytes()).is_err() ||
           paging::copy_to_user(egid_ptr, &e.to_ne_bytes()).is_err() ||
           paging::copy_to_user(sgid_ptr, &s.to_ne_bytes()).is_err() {
            return -14;
        }
        0
    } else {
        -1
    }
}

fn sys_setresgid(rgid: u32, egid: u32, sgid: u32) -> i64 {
    match process::setresgid(rgid, egid, sgid) {
        Ok(()) => 0,
        Err(e) => e as i64,
    }
}

fn sys_link(oldpath_ptr: usize, newpath_ptr: usize) -> i64 {
    if !verify_user_string(oldpath_ptr, 256) || !verify_user_string(newpath_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut old_buf = [0u8; 256];
    let mut new_buf = [0u8; 256];
    let old_path = match read_user_path(oldpath_ptr, &mut old_buf) {
        Some(s) => s,
        None => return -14,
    };
    let new_path = match read_user_path(newpath_ptr, &mut new_buf) {
        Some(s) => s,
        None => return -14,
    };
    if old_path.is_empty() || new_path.is_empty() {
        return -2; // -ENOENT
    }
    // [T-001] Enlaces duros por el contrato (ramfs y ext2). Ambas rutas deben
    // caer en el MISMO sistema montado: si no, EXDEV como en Linux.
    let mismo = vfs::mismo_sistema(old_path, new_path);
    if mismo == Some(false) { return -18; } // EXDEV
    match vfs::con_fs(old_path, |fs| fs.enlazar(old_path, new_path)) {
        Some(Ok(())) => 0,
        Some(Err(e)) => -(e as i64),
        // FAT32 (/boot) y los sistemas virtuales no soportan enlaces duros:
        // EPERM, salvo que el origen no exista (ENOENT).
        None => if stat_path_internal(old_path).is_err() { -2 } else { -1 },
    }
}

fn sys_writev(fd: i32, iov_ptr: usize, iovcnt: i32) -> i64 {
    if iovcnt < 0 || iovcnt > 1024 { return -22; } // -EINVAL
    if iovcnt == 0 { return 0; }
    let cnt = iovcnt as usize;
    if !verify_user_buffer(iov_ptr, cnt * core::mem::size_of::<IoVec>()) {
        return -14; // -EFAULT
    }
    let mut iovs = alloc::vec![IoVec::default(); cnt];
    let iov_bytes = unsafe {
        core::slice::from_raw_parts_mut(iovs.as_mut_ptr() as *mut u8, cnt * core::mem::size_of::<IoVec>())
    };
    if paging::copy_from_user(iov_bytes, iov_ptr, iov_bytes.len()).is_err() {
        return -14;
    }
    let mut total: i64 = 0;
    for iov in &iovs {
        if iov.iov_len == 0 { continue; }
        let written = sys_write(fd, iov.iov_base, iov.iov_len);
        if written < 0 {
            if total > 0 { return total; }
            return written;
        }
        total += written;
        if (written as usize) < iov.iov_len {
            break;
        }
    }
    total
}

fn sys_readv(fd: i32, iov_ptr: usize, iovcnt: i32, current_rsp: u64) -> (i64, u64) {
    if iovcnt < 0 || iovcnt > 1024 { return (-22, 0); } // -EINVAL
    if iovcnt == 0 { return (0, 0); }
    let cnt = iovcnt as usize;
    if !verify_user_buffer(iov_ptr, cnt * core::mem::size_of::<IoVec>()) {
        return (-14, 0); // -EFAULT
    }
    let mut iovs = alloc::vec![IoVec::default(); cnt];
    let iov_bytes = unsafe {
        core::slice::from_raw_parts_mut(iovs.as_mut_ptr() as *mut u8, cnt * core::mem::size_of::<IoVec>())
    };
    if paging::copy_from_user(iov_bytes, iov_ptr, iov_bytes.len()).is_err() {
        return (-14, 0);
    }
    let mut total: i64 = 0;
    for iov in &iovs {
        if iov.iov_len == 0 { continue; }
        let (n, rsp) = sys_read(fd, iov.iov_base, iov.iov_len, current_rsp);
        if n < 0 {
            if total > 0 { return (total, rsp); }
            return (n, rsp);
        }
        if n == 0 {
            return (total, rsp);
        }
        total += n;
        if (n as usize) < iov.iov_len {
            return (total, rsp);
        }
    }
    (total, 0)
}

fn sys_pread64(fd: i32, buf: usize, count: usize, offset: i64) -> i64 {
    if offset < 0 { return -22; } // -EINVAL
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; } // -EBADF
    if !verify_user_buffer(buf, count) { return -14; } // -EFAULT
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(&proc, fd as usize) {
        Some(e) => e.clone(),
        None => return -9,
    };
    match &entry.fd_type {
        process::FdType::Stdin | process::FdType::Stdout | process::FdType::Stderr |
        process::FdType::PipeRead(_) | process::FdType::PipeWrite(_) |
        process::FdType::Socket(_) | process::FdType::SocketPairEnd { .. } | process::FdType::EventFd(_) | process::FdType::Epoll(_) |
        process::FdType::TimerFd(_) | process::FdType::SignalFd(_) | process::FdType::PtyMaster(_) | process::FdType::PtySlave(_) | process::FdType::PidFd(_) | process::FdType::IoUring(_) | process::FdType::Mq(_) | process::FdType::Fanotify(_) | process::FdType::Inotify(_) => {
            -29 // -ESPIPE
        }
        process::FdType::FsFile(info) => {
            let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                Ok(s) => s,
                Err(_) => return -22,
            };
            let count = count.min(1 << 20); // [T-002] lectura corta antes que agotar el heap
            let mut kbuf = alloc::vec![0u8; count];
            let res = crate::drivers::storage::vfs::leer_fs(path_str, offset as u64, &mut kbuf);
            match res {
                Ok(n) => {
                    if n > 0 && paging::copy_to_user(buf, &kbuf[..n]).is_err() {
                        -14 // -EFAULT
                    } else {
                        n as i64
                    }
                }
                Err(_) => -5,
            }
        }
        process::FdType::Device(info) => {
            match info.dev_type {
                process::DeviceType::Null => 0,
                _ => -29,
            }
        }
        process::FdType::ProcFile(info) => {
            let content = crate::drivers::storage::procfs::generate_content(info.file_type);
            if offset < 0 || (offset as usize) >= content.len() {
                return 0;
            }
            let remaining = &content[offset as usize..];
            let to_copy = remaining.len().min(count);
            if paging::copy_to_user(buf, &remaining[..to_copy]).is_err() {
                return -14;
            }
            to_copy as i64
        }
    }
}

fn sys_pwrite64(fd: i32, buf: usize, count: usize, offset: i64) -> i64 {
    if offset < 0 { return -22; } // -EINVAL
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; } // -EBADF
    if count == 0 { return 0; }
    if !verify_user_buffer(buf, count) { return -14; } // -EFAULT
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(&proc, fd as usize) {
        Some(e) => e.clone(),
        None => return -9,
    };
    match &entry.fd_type {
        process::FdType::Stdin | process::FdType::Stdout | process::FdType::Stderr |
        process::FdType::PipeRead(_) | process::FdType::PipeWrite(_) |
        process::FdType::Socket(_) | process::FdType::SocketPairEnd { .. } | process::FdType::EventFd(_) | process::FdType::Epoll(_) |
        process::FdType::TimerFd(_) | process::FdType::SignalFd(_) | process::FdType::PtyMaster(_) | process::FdType::PtySlave(_) | process::FdType::PidFd(_) | process::FdType::IoUring(_) | process::FdType::Mq(_) | process::FdType::Fanotify(_) | process::FdType::Inotify(_) => {
            -29 // -ESPIPE
        }
        process::FdType::FsFile(info) => {
            let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                Ok(s) => s,
                Err(_) => return -22,
            };
            let count = count.min(1 << 20); // [T-002] escritura corta antes que agotar el heap
            let mut kbuf = alloc::vec![0u8; count];
            if paging::copy_from_user(&mut kbuf, buf, count).is_err() {
                return -14;
            }
            let res = crate::drivers::storage::vfs::escribir_fs(path_str, offset as u64, &kbuf);
            match res {
                Ok(n) => n as i64,
                Err(_) => -5,
            }
        }
        process::FdType::Device(info) => {
            match info.dev_type {
                process::DeviceType::Null => count as i64,
                _ => -29,
            }
        }
        process::FdType::ProcFile(_) => {
            -30 // -EROFS
        }
    }
}

fn sys_mprotect(addr: usize, len: usize, prot: i32) -> i64 {
    if addr % 4096 != 0 { return -22; } // -EINVAL
    if len == 0 { return 0; }
    if (prot & !7) != 0 { return -22; } // -EINVAL
    // [CICLO 50 / T1c] Los binarios de distro no-PIE viven en 0x400000.. (por
    // debajo del userland nativo de Portix en 0x2000_0000): el límite inferior
    // es el espacio de usuario real, y ENOMEM se decide página a página (toda
    // la región debe estar mapeada para el proceso), como en Linux. Así el
    // RELRO de ld-musl (`mprotect(PROT_READ)` sobre .data.rel.ro) funciona.
    if addr < 0x1000 || addr.checked_add(len).map_or(true, |end| end > crate::process::USER_STACK_TOP) {
        return -12; // -ENOMEM
    }
    let pages = (len + 4095) / 4096;
    let proc = match process::proceso_memoria() {
        Some(p) => p,
        None => return -3, // -ESRCH
    };
    for i in 0..pages {
        if i & 0xFF == 0xFF { paging::respirar(); } // [T-026]
        if !paging::pagina_del_proceso(proc.cr3, addr + i * 4096) {
            return -12; // -ENOMEM: página no mapeada para este proceso
        }
    }
    // [T-014b] MAP_SHARED de sólo lectura sobre fichero O_RDONLY: PROT_WRITE → EACCES (Linux).
    if prot & 2 != 0 && crate::mem::vmas::hay_compartido_en(proc.cr3, addr, addr + pages * 4096) { return -13; }
    match paging::mprotect_range(proc.cr3, addr, pages, prot) {
        Ok(()) => 0,
        Err(_) => -12,
    }
}

fn sys_socketpair(domain: i32, sock_type: i32, _protocol: i32, sv_ptr: usize) -> i32 {
    if domain != 1 {
        return -97; // -EAFNOSUPPORT
    }
    if (sock_type & 0xF) != 1 && (sock_type & 0xF) != 2 {
        return -22; // -EINVAL
    }
    if sv_ptr == 0 || !verify_user_buffer(sv_ptr, 8) {
        return -14; // -EFAULT
    }
    let pipe_a = match process::pipe_alloc() {
        Some(id) => id,
        None => return -24, // -EMFILE
    };
    let pipe_b = match process::pipe_alloc() {
        Some(id) => id,
        None => {
            process::pipe_close_reader(pipe_a);
            process::pipe_close_writer(pipe_a);
            return -24; // -EMFILE
        }
    };
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    let cloexec = (sock_type & 0x80000) != 0;
    let fd_flags = if cloexec { 1 } else { 0 };

    let mut entry0 = process::FdEntry::new(process::FdType::SocketPairEnd {
        read_pipe: pipe_a,
        write_pipe: pipe_b,
    });
    entry0.flags = fd_flags;
    let fd0 = match process::fd_alloc(proc, entry0) {
        Some(f) => f,
        None => {
            process::pipe_close_reader(pipe_a);
            process::pipe_close_writer(pipe_a);
            process::pipe_close_reader(pipe_b);
            process::pipe_close_writer(pipe_b);
            return -24;
        }
    };

    let mut entry1 = process::FdEntry::new(process::FdType::SocketPairEnd {
        read_pipe: pipe_b,
        write_pipe: pipe_a,
    });
    entry1.flags = fd_flags;
    let fd1 = match process::fd_alloc(proc, entry1) {
        Some(f) => f,
        None => {
            process::fd_close(proc, fd0);
            process::pipe_close_reader(pipe_b);
            process::pipe_close_writer(pipe_b);
            return -24;
        }
    };

    let fds: [i32; 2] = [fd0 as i32, fd1 as i32];
    let fds_bytes = unsafe {
        core::slice::from_raw_parts(fds.as_ptr() as *const u8, 8)
    };
    if paging::copy_to_user(sv_ptr, fds_bytes).is_err() {
        process::fd_close(proc, fd0);
        process::fd_close(proc, fd1);
        return -14;
    }

    serial::write_str("[SYS] SOCKETPAIR fds=[");
    serial::write_usize(fd0);
    serial::write_str(", ");
    serial::write_usize(fd1);
    serial::write_str("]\n");

    0
}

fn sys_gettid() -> u64 {
    process::current_process().map(|p| p.pid).unwrap_or(0)
}

fn sys_sched_getparam(pid: u64, param_ptr: usize) -> i64 {
    if !verify_user_buffer(param_ptr, 4) {
        return -14; // -EFAULT
    }
    let cur_pid = process::current_process().map(|p| p.pid).unwrap_or(0);
    let target = if pid == 0 { cur_pid } else { pid };
    if process::process_by_pid(target).is_none() {
        return -3; // -ESRCH
    }
    let zero = 0i32;
    if paging::copy_to_user(param_ptr, &zero.to_ne_bytes()).is_err() {
        return -14;
    }
    0
}

fn sys_sched_setparam(pid: u64, param_ptr: usize) -> i64 {
    if !verify_user_buffer(param_ptr, 4) {
        return -14; // -EFAULT
    }
    let mut pbuf = [0u8; 4];
    if paging::copy_from_user(&mut pbuf, param_ptr, 4).is_err() {
        return -14;
    }
    let prio = i32::from_ne_bytes(pbuf);
    if prio < 0 || prio > 99 {
        return -22; // -EINVAL
    }
    let cur_pid = process::current_process().map(|p| p.pid).unwrap_or(0);
    let target = if pid == 0 { cur_pid } else { pid };
    if process::process_by_pid(target).is_none() {
        return -3; // -ESRCH
    }
    0
}

fn sys_sched_getscheduler(pid: u64) -> i64 {
    let cur_pid = process::current_process().map(|p| p.pid).unwrap_or(0);
    let target = if pid == 0 { cur_pid } else { pid };
    if process::process_by_pid(target).is_none() {
        return -3; // -ESRCH
    }
    0 // SCHED_OTHER
}

fn sys_sched_setscheduler(pid: u64, policy: i32, param_ptr: usize) -> i64 {
    if policy < 0 || policy > 2 {
        return -22; // -EINVAL
    }
    if param_ptr != 0 && !verify_user_buffer(param_ptr, 4) {
        return -14; // -EFAULT
    }
    let cur_pid = process::current_process().map(|p| p.pid).unwrap_or(0);
    let cur_euid = process::current_euid();
    if policy != 0 && cur_euid != 0 {
        return -1; // -EPERM
    }
    let target = if pid == 0 { cur_pid } else { pid };
    if process::process_by_pid(target).is_none() {
        return -3; // -ESRCH
    }
    0
}

fn sys_prctl(option: i32, arg2: u64, _arg3: u64, _arg4: u64, _arg5: u64) -> i64 {
    match option {
        15 => { // PR_SET_NAME
            let name_ptr = arg2 as usize;
            if !verify_user_string(name_ptr, 16) {
                return -14; // -EFAULT
            }
            let mut buf = [0u8; 16];
            let len = match paging::copy_from_user(&mut buf, name_ptr, 16) {
                Ok(n) => n,
                Err(()) => return -14,
            };
            let actual_len = buf.iter().position(|&b| b == 0).unwrap_or(len);
            if let Some(proc) = process::current_process() {
                proc.name = [0u8; 32];
                proc.name[..actual_len].copy_from_slice(&buf[..actual_len]);
                proc.name_len = actual_len;
                0
            } else {
                -1
            }
        }
        16 => { // PR_GET_NAME
            let name_ptr = arg2 as usize;
            if !verify_user_buffer(name_ptr, 16) {
                return -14; // -EFAULT
            }
            if let Some(proc) = process::current_process() {
                let mut buf = [0u8; 16];
                let len = proc.name_len.min(15);
                buf[..len].copy_from_slice(&proc.name[..len]);
                if paging::copy_to_user(name_ptr, &buf).is_ok() {
                    0
                } else {
                    -14
                }
            } else {
                -1
            }
        }
        3 => 1, // PR_GET_DUMPABLE
        4 => {  // PR_SET_DUMPABLE
            if arg2 > 1 { -22 } else { 0 }
        }
        1 => {  // PR_SET_PDEATHSIG
            if arg2 > 63 { -22 } else { 0 }
        }
        _ => -22, // -EINVAL
    }
}

fn sys_sync() -> i64 {
    // [T-012] `sync(2)` vuelca también el superbloque/GDT diferidos del ext2 raíz.
    let _ = vfs::con_fs("/", |fs| fs.sincronizar());
    let _ = crate::drivers::storage::registry::with_device(0, |d| -> Result<(), ()> {
        if let Some(drive) = d {
            let _ = drive.flush_cache();
        }
        Ok(())
    });
    0
}

fn sys_fstatvfs(fd: i32, buf: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; } // -EBADF
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    if !verify_user_buffer(buf, core::mem::size_of::<Statvfs>()) {
        return -14; // -EFAULT
    }
    let st = Statvfs {
        f_bsize: 4096,
        f_frsize: 4096,
        f_blocks: 16384,
        f_bfree: 14000,
        f_bavail: 14000,
        f_files: 1024,
        f_ffree: 1000,
        f_favail: 1000,
        f_fsid: 42,
        f_flag: 0,
        f_namemax: 255,
    };
    let st_bytes = unsafe {
        core::slice::from_raw_parts(&st as *const Statvfs as *const u8, core::mem::size_of::<Statvfs>())
    };
    if paging::copy_to_user(buf, st_bytes).is_err() {
        return -14;
    }
    0
}

fn sys_eventfd2(initval: u32, flags: i32) -> i32 {
    const VALID_FLAGS: i32 = 0x80000 | 0x0800 | 1;
    if (flags & !VALID_FLAGS) != 0 {
        return -22; // -EINVAL
    }
    let id = match process::eventfd_alloc(initval as u64, flags as u32) {
        Some(i) => i,
        None => return -24, // -EMFILE
    };
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    let cloexec = (flags & 0x80000) != 0;
    let mut entry = process::FdEntry::new(process::FdType::EventFd(id));
    entry.flags = if cloexec { 1 } else { 0 };

    match process::fd_alloc(proc, entry) {
        Some(fd) => {
            serial::write_str("[SYS] EVENTFD created fd=");
            serial::write_usize(fd);
            serial::write_str("\n");
            fd as i32
        }
        None => {
            process::eventfd_close(id);
            -24 // -EMFILE
        }
    }
}

fn sys_dup3(oldfd: i32, newfd: i32, flags: i32) -> i64 {
    if oldfd == newfd {
        return -22; // -EINVAL
    }
    if (flags & !0x80000) != 0 {
        return -22; // -EINVAL
    }
    if oldfd < 0 || oldfd >= process::MAX_FDS as i32 || newfd < 0 || newfd >= process::MAX_FDS as i32 {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    let old_entry = match process::fd_get(proc, oldfd as usize) {
        Some(e) => *e,
        None => return -9, // -EBADF
    };
    process::fd_close(proc, newfd as usize);
    let mut new_entry = process::fd_clone_entry(&old_entry);
    if (flags & 0x80000) != 0 {
        new_entry.flags |= 1;
    } else {
        new_entry.flags &= !1; // [defecto 33] sin O_CLOEXEC el nuevo fd no lo hereda
    }
    process::fd_alloc_at(proc, newfd as usize, new_entry);
    newfd as i64
}

fn sys_tgkill(tgid: i32, tid: i32, sig: i32) -> i64 {
    if sig < 0 || sig > 63 || tid <= 0 {
        return -22; // -EINVAL
    }
    let cur_pid = process::current_process().map(|p| p.pid).unwrap_or(0) as i32;
    if tgid != -1 && tgid != cur_pid && tgid != tid {
        // Target thread group check
    }
    let target_proc = match process::process_by_pid(tid as u64) {
        Some(p) => p,
        None => return -3, // -ESRCH
    };
    if sig == 0 {
        return 0;
    }
    let _ = target_proc;
    if crate::process::signals::raise_for_pid(tid as u64, sig as u32) { 0 } else { -3 }
}

fn sys_sched_get_priority_max(policy: i32) -> i64 {
    match policy {
        0 => 0, // SCHED_OTHER
        1 | 2 => 99, // SCHED_FIFO, SCHED_RR
        _ => -22, // -EINVAL
    }
}

fn sys_sched_get_priority_min(policy: i32) -> i64 {
    match policy {
        0 => 0, // SCHED_OTHER
        1 | 2 => 1, // SCHED_FIFO, SCHED_RR
        _ => -22, // -EINVAL
    }
}

/// [T-005] `futex(2)` real. Devuelve `(valor | SYSCALL_RESTART, rsp_siguiente)`.
fn sys_futex(uaddr: usize, op: i32, val: u32, timeout_ptr: usize, uaddr2: usize, val3: u32, current_rsp: u64) -> (i64, u64) {
    if uaddr == 0 || (uaddr % 4) != 0 {
        return (-22, 0); // -EINVAL
    }
    if !verify_user_buffer(uaddr, 4) {
        return (-14, 0); // -EFAULT
    }
    let opu = op as u32;
    let cmd = opu & !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);
    let cur = unsafe { process::current_raw() };
    if cur.is_null() { return (-3, 0); }
    let (pid, cr3) = unsafe { ((*cur).pid, (*cur).cr3) };
    let ahora = crate::time::pit::ticks();
    match cmd {
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            let bitset = if cmd == FUTEX_WAIT_BITSET { val3 } else { !0u32 };
            if bitset == 0 { return (-22, 0); }
            // Plazo en ticks absolutos del PIT (100 Hz). FUTEX_WAIT: relativo.
            // FUTEX_WAIT_BITSET: absoluto (CLOCK_MONOTONIC = ticks; CLOCK_REALTIME = epoch).
            let mut plazo = 0u64;
            if timeout_ptr != 0 {
                if !verify_user_buffer(timeout_ptr, 16) { return (-14, 0); }
                let mut ts = [0u8; 16];
                if paging::copy_from_user(&mut ts, timeout_ptr, 16).is_err() { return (-14, 0); }
                let sec = i64::from_ne_bytes(ts[0..8].try_into().unwrap_or([0; 8]));
                let nsec = i64::from_ne_bytes(ts[8..16].try_into().unwrap_or([0; 8]));
                if nsec < 0 || nsec >= 1_000_000_000 || sec < 0 { return (-22, 0); }
                let ticks_ts = (sec as u64) * 100 + (nsec as u64 + 9_999_999) / 10_000_000;
                plazo = if cmd == FUTEX_WAIT {
                    ahora + ticks_ts.max(1)
                } else if opu & FUTEX_CLOCK_REALTIME != 0 {
                    let epoch_ahora = crate::time::rtc::now_epoch() as u64;
                    let restante = (sec as u64).saturating_sub(epoch_ahora) * 100;
                    ahora + restante.max(1)
                } else {
                    ticks_ts.max(1)
                };
            }
            let mut v = [0u8; 4];
            if paging::copy_from_user(&mut v, uaddr, 4).is_err() { return (-14, 0); }
            let actual = u32::from_ne_bytes(v);
            let hay_senal = unsafe { (*cur).sig_pending & !(*cur).sig_blocked != 0 };
            match futex::preparar_espera(pid, cr3, uaddr, val, actual, bitset, plazo, hay_senal, ahora) {
                futex::Espera::Inmediato(r) => (r, 0),
                futex::Espera::Bloquear { plazo } => {
                    unsafe {
                        (*cur).state = process::ProcessState::Blocked;
                        // `wake_blocked` despierta por tiempo sólo con sleep_until > 0 y
                        // trata distancias > 1000 ticks como vencidas: se acota.
                        (*cur).sleep_until = if plazo != 0 { plazo.min(ahora + 900) } else { 0 };
                    }
                    let cs = saved_cs_from_rsp(current_rsp);
                    let new_rsp = process::schedule_desde_syscall(current_rsp, cs);
                    if new_rsp != 0 {
                        // Sólo se rebobina si de verdad se conmuta: si se devuelve
                        // un valor directo (abajo) con RIP rebobinado, el proceso
                        // volvería a ejecutar `syscall` con RAX = ese valor (ENOSYS).
                        let reiniciar = rebobinar_syscall(current_rsp);
                        return (if reiniciar { SYSCALL_RESTART } else { 0 }, new_rsp);
                    }
                    // Nadie más listo: esperar en el núcleo con `hlt` expropiable
                    // hasta que un WAKE (desde otro hilo que corra por IRQ0),
                    // una señal o el plazo nos saquen.
                    unsafe { (*cur).state = process::ProcessState::Running; }
                    loop {
                        if futex::fue_despertado(pid) { futex::retirar(pid); return (0, 0); }
                        unsafe {
                            if (*cur).sig_pending & !(*cur).sig_blocked != 0 { futex::retirar(pid); return (-4, 0); }
                        }
                        if plazo != 0 && crate::time::pit::ticks() >= plazo { futex::retirar(pid); return (-110, 0); }
                        process::wait_hlt_preemptible();
                        crate::net::poll();
                        unsafe { process::wake_blocked(); }
                        unsafe {
                            if (*cur).state != process::ProcessState::Running { (*cur).state = process::ProcessState::Running; }
                        }
                    }
                }
            }
        }
        FUTEX_WAKE | FUTEX_WAKE_BITSET => {
            let mascara = if cmd == FUTEX_WAKE_BITSET { val3 } else { !0u32 };
            if mascara == 0 { return (-22, 0); }
            (futex::despertar(cr3, uaddr, val, mascara), 0)
        }
        FUTEX_REQUEUE | FUTEX_CMP_REQUEUE => {
            if uaddr2 == 0 || (uaddr2 % 4) != 0 { return (-22, 0); }
            if !verify_user_buffer(uaddr2, 4) { return (-14, 0); }
            if cmd == FUTEX_CMP_REQUEUE {
                let mut v = [0u8; 4];
                if paging::copy_from_user(&mut v, uaddr, 4).is_err() { return (-14, 0); }
                if u32::from_ne_bytes(v) != val3 { return (-11, 0); }
            }
            // timeout_ptr se reinterpreta como `val2` (número a reencolar) en REQUEUE.
            (futex::reencolar(cr3, uaddr, val, uaddr2, timeout_ptr as u32), 0)
        }
        FUTEX_WAKE_OP => {
            // [T-018] wake_op: *uaddr2 = op(*uaddr2, oparg) atómico; despierta `val`
            // de uaddr y, si cmp(valor_anterior, cmparg), `val2` (timeout_ptr) de uaddr2.
            // glibc lo usa en pthread_cond_broadcast (<2.25) y en barreras/semaforos.
            if uaddr2 == 0 || (uaddr2 % 4) != 0 { return (-22, 0); }
            if !paging::is_user_range_writable(uaddr2, 4) { return (-14, 0); }
            let op = (val3 >> 28) & 0xF;
            let cmp = (val3 >> 24) & 0xF;
            let mut oparg = ((val3 >> 12) & 0xFFF) as u32;
            let cmparg = (val3 & 0xFFF) as u32;
            if op & 8 != 0 { if oparg > 31 { return (-22, 0); } oparg = 1u32 << oparg; } // FUTEX_OP_OPARG_SHIFT
            let a = unsafe { &*(uaddr2 as *const core::sync::atomic::AtomicU32) };
            use core::sync::atomic::Ordering::SeqCst;
            let anterior = match op & 7 {
                0 => a.swap(oparg, SeqCst),
                1 => a.fetch_add(oparg, SeqCst),
                2 => a.fetch_or(oparg, SeqCst),
                3 => a.fetch_and(!oparg, SeqCst),
                4 => a.fetch_xor(oparg, SeqCst),
                _ => return (-38, 0), // -ENOSYS, como Linux con un op desconocido
            };
            let cumple = match cmp {
                0 => anterior == cmparg,
                1 => anterior != cmparg,
                2 => (anterior as i32) < (cmparg as i32),
                3 => (anterior as i32) <= (cmparg as i32),
                4 => (anterior as i32) > (cmparg as i32),
                5 => (anterior as i32) >= (cmparg as i32),
                _ => return (-38, 0),
            };
            let mut n = futex::despertar(cr3, uaddr, val, !0);
            if cumple { n += futex::despertar(cr3, uaddr2, timeout_ptr as u32, !0); }
            serial::write_str("[FUTEX] WAKE_OP op="); serial::write_usize((op & 7) as usize);
            serial::write_str(" cmp="); serial::write_usize(cmp as usize);
            serial::write_str(" anterior="); serial::write_usize(anterior as usize);
            serial::write_str(if cumple { " cumple despertados=" } else { " no cumple despertados=" });
            serial::write_usize(n as usize); serial::write_str("\n");
            (n, 0)
        }
        FUTEX_LOCK_PI | FUTEX_LOCK_PI2 | FUTEX_TRYLOCK_PI => {
            // [T-018] Mutex con herencia de prioridad (PTHREAD_PRIO_INHERIT). La
            // palabra guarda el tid del dueño; FUTEX_WAITERS (bit 31) avisa de que
            // hay esperadores y el núcleo TRASPASA la propiedad al desbloquear
            // (escribe el tid del elegido). Sin herencia de prioridad efectiva (el
            // planificador no tiene prioridades que heredar): la semántica de
            // propiedad y traspaso es la de Linux, y es la que glibc necesita.
            const FUTEX_WAITERS: u32 = 0x8000_0000;
            const FUTEX_TID_MASK: u32 = 0x3FFF_FFFF;
            if !paging::is_user_range_writable(uaddr, 4) { return (-14, 0); }
            let tid = pid as u32;
            let a = unsafe { &*(uaddr as *const core::sync::atomic::AtomicU32) };
            use core::sync::atomic::Ordering::SeqCst;
            // Plazo: LOCK_PI = absoluto CLOCK_REALTIME; LOCK_PI2 = absoluto CLOCK_MONOTONIC
            // salvo FUTEX_CLOCK_REALTIME. (Sólo se lee al inscribirse; en las
            // reentradas por reinicio de syscall manda el plazo de la inscripción.)
            let mut plazo = 0u64;
            if cmd != FUTEX_TRYLOCK_PI && timeout_ptr != 0 {
                if !verify_user_buffer(timeout_ptr, 16) { return (-14, 0); }
                let mut ts = [0u8; 16];
                if paging::copy_from_user(&mut ts, timeout_ptr, 16).is_err() { return (-14, 0); }
                let sec = i64::from_ne_bytes(ts[0..8].try_into().unwrap_or([0; 8]));
                let nsec = i64::from_ne_bytes(ts[8..16].try_into().unwrap_or([0; 8]));
                if nsec < 0 || nsec >= 1_000_000_000 || sec < 0 { return (-22, 0); }
                let ticks_ts = (sec as u64) * 100 + (nsec as u64 + 9_999_999) / 10_000_000;
                plazo = if cmd == FUTEX_LOCK_PI || opu & FUTEX_CLOCK_REALTIME != 0 {
                    let epoch_ahora = crate::time::rtc::now_epoch() as u64;
                    ahora + (sec as u64).saturating_sub(epoch_ahora) * 100
                } else { ticks_ts }.max(1);
            }
            // Reentrada tras dormir (reinicio transparente de la syscall).
            if futex::inscrito(pid) {
                // Había inscripción: si nos traspasaron el mutex (la palabra lleva
                // nuestro tid) hemos terminado; si no, `preparar_espera` decide
                // abajo entre plazo, señal o volver a dormir.
                if a.load(SeqCst) & FUTEX_TID_MASK == tid { futex::retirar(pid); return (0, 0); }
            }
            let mut intentos = 0;
            loop {
                let v = a.load(SeqCst);
                if v & FUTEX_TID_MASK == 0 {
                    if a.compare_exchange(v, tid | (v & FUTEX_WAITERS), SeqCst, SeqCst).is_ok() {
                        futex::retirar(pid);
                        return (0, 0);
                    }
                    intentos += 1; if intentos > 64 { return (-11, 0); }
                    continue;
                }
                if v & FUTEX_TID_MASK == tid { return (-35, 0); } // -EDEADLK
                if cmd == FUTEX_TRYLOCK_PI { return (-11, 0); }   // -EAGAIN (EBUSY para glibc)
                if v & FUTEX_WAITERS == 0 {
                    if a.compare_exchange(v, v | FUTEX_WAITERS, SeqCst, SeqCst).is_err() { continue; }
                }
                let esperado = v | FUTEX_WAITERS;
                let actual = a.load(SeqCst);
                let sigkill = unsafe { (*cur).sig_pending & (1u64 << process::signals::SIGKILL as u64) != 0 };
                match futex::preparar_espera(pid, cr3, uaddr, esperado, actual, !0, plazo, sigkill, ahora) {
                    futex::Espera::Inmediato(0) => {
                        // Despertado por UNLOCK_PI: la palabra ya lleva nuestro tid.
                        if a.load(SeqCst) & FUTEX_TID_MASK == tid { return (0, 0); }
                        continue;
                    }
                    futex::Espera::Inmediato(-11) => { intentos += 1; if intentos > 64 { return (-11, 0); } continue; }
                    futex::Espera::Inmediato(r) => return (r, 0),
                    futex::Espera::Bloquear { plazo } => {
                        unsafe {
                            (*cur).state = process::ProcessState::Blocked;
                            (*cur).sleep_until = if plazo != 0 { plazo.min(ahora + 900) } else { 0 };
                        }
                        let cs = saved_cs_from_rsp(current_rsp);
                        let new_rsp = process::schedule_desde_syscall(current_rsp, cs);
                        if new_rsp != 0 {
                            let reiniciar = rebobinar_syscall(current_rsp);
                            return (if reiniciar { SYSCALL_RESTART } else { 0 }, new_rsp);
                        }
                        unsafe { (*cur).state = process::ProcessState::Running; }
                        loop {
                            if futex::fue_despertado(pid) { futex::retirar(pid); break; }
                            if unsafe { (*cur).sig_pending & (1u64 << process::signals::SIGKILL as u64) != 0 } { futex::retirar(pid); return (-4, 0); }
                            if plazo != 0 && crate::time::pit::ticks() >= plazo { futex::retirar(pid); return (-110, 0); }
                            process::wait_hlt_preemptible();
                            crate::net::poll();
                            unsafe { process::wake_blocked(); }
                            unsafe { if (*cur).state != process::ProcessState::Running { (*cur).state = process::ProcessState::Running; } }
                        }
                        if a.load(SeqCst) & FUTEX_TID_MASK == tid { return (0, 0); }
                        continue;
                    }
                }
            }
        }
        FUTEX_UNLOCK_PI => {
            const FUTEX_WAITERS: u32 = 0x8000_0000;
            const FUTEX_TID_MASK: u32 = 0x3FFF_FFFF;
            if !paging::is_user_range_writable(uaddr, 4) { return (-14, 0); }
            let tid = pid as u32;
            let a = unsafe { &*(uaddr as *const core::sync::atomic::AtomicU32) };
            use core::sync::atomic::Ordering::SeqCst;
            let v = a.load(SeqCst);
            if v & FUTEX_TID_MASK != tid { return (-1, 0); } // -EPERM: no es el dueño
            match futex::elegir_pi(cr3, uaddr) {
                Some((w, quedan)) => {
                    // Traspaso: la palabra pasa a llevar el tid del elegido ANTES de
                    // que pueda correr (ver `elegir_pi`).
                    let nuevo = (w as u32 & FUTEX_TID_MASK) | if quedan { FUTEX_WAITERS } else { 0 };
                    let mut cur_v = v;
                    while let Err(obs) = a.compare_exchange(cur_v, nuevo, SeqCst, SeqCst) {
                        if obs & FUTEX_TID_MASK != tid { futex::listo_pi(w); return (-1, 0); }
                        cur_v = obs;
                    }
                    futex::listo_pi(w);
                    // Traza acotada: las 8 primeras por arranque (la sonda T-018 genera
                    // ~25 000 traspasos; el total sale en `/proc/smp` como despertares).
                    static TRASPASOS_TRAZADOS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
                    if TRASPASOS_TRAZADOS.fetch_add(1, SeqCst) < 8 {
                        serial::write_str("[FUTEX] UNLOCK_PI traspaso tid=");
                        serial::write_usize(tid as usize);
                        serial::write_str(" -> tid=");
                        serial::write_usize(w as usize);
                        serial::write_str(if quedan { " (quedan esperadores)\n" } else { "\n" });
                    }
                    (0, 0)
                }
                None => {
                    let mut cur_v = v;
                    while let Err(obs) = a.compare_exchange(cur_v, 0, SeqCst, SeqCst) {
                        if obs & FUTEX_TID_MASK != tid { return (-1, 0); }
                        cur_v = obs;
                    }
                    (0, 0)
                }
            }
        }
        _ => (-22, 0), // -EINVAL
    }
}

fn sys_epoll_create1(flags: i32) -> i64 {
    const VALID_FLAGS: i32 = 0x80000; // EPOLL_CLOEXEC
    if (flags & !VALID_FLAGS) != 0 {
        return -22; // -EINVAL
    }
    let id = match process::epoll_alloc(flags as u32) {
        Some(i) => i,
        None => return -24, // -EMFILE
    };
    let proc = match process::current_process() {
        Some(p) => p,
        None => {
            process::epoll_close(id);
            return -24;
        }
    };
    let cloexec = (flags & 0x80000) != 0;
    let mut entry = process::FdEntry::new(process::FdType::Epoll(id));
    entry.flags = if cloexec { 1 } else { 0 };

    match process::fd_alloc(proc, entry) {
        Some(fd) => fd as i64,
        None => {
            process::epoll_close(id);
            -24 // -EMFILE
        }
    }
}

fn sys_epoll_ctl(epfd: i32, op: i32, fd: i32, event_ptr: usize) -> i64 {
    if epfd < 0 || epfd as usize >= process::MAX_FDS { return -9; }
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; }
    if epfd == fd { return -22; }

    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };

    let ep_id = match process::fd_get(proc, epfd as usize) {
        Some(e) => match e.fd_type {
            process::FdType::Epoll(id) => id,
            _ => return -22,
        },
        None => return -9,
    };

    if process::fd_get(proc, fd as usize).is_none() {
        return -9;
    }

    let mut event = process::EpollEvent::default();
    if op != EPOLL_CTL_DEL {
        if !verify_user_buffer(event_ptr, core::mem::size_of::<process::EpollEvent>()) {
            return -14; // -EFAULT
        }
        let ev_bytes = unsafe {
            core::slice::from_raw_parts_mut(&mut event as *mut process::EpollEvent as *mut u8, core::mem::size_of::<process::EpollEvent>())
        };
        if paging::copy_from_user(ev_bytes, event_ptr, ev_bytes.len()).is_err() {
            return -14;
        }
    }

    let mut table = process::EPOLL_TABLE.lock();
    let inst = match table.instances.get_mut(ep_id as usize) {
        Some(i) if i.in_use => i,
        _ => return -9,
    };

    match op {
        EPOLL_CTL_ADD => {
            for w in inst.watches.iter() {
                if w.in_use && w.fd == fd {
                    return -17; // -EEXIST
                }
            }
            for w in inst.watches.iter_mut() {
                if !w.in_use {
                    w.fd = fd;
                    w.event = event;
                    w.in_use = true;
                    return 0;
                }
            }
            -28 // -ENOSPC
        }
        EPOLL_CTL_DEL => {
            for w in inst.watches.iter_mut() {
                if w.in_use && w.fd == fd {
                    w.in_use = false;
                    w.fd = -1;
                    return 0;
                }
            }
            -2 // -ENOENT
        }
        EPOLL_CTL_MOD => {
            for w in inst.watches.iter_mut() {
                if w.in_use && w.fd == fd {
                    w.event = event;
                    return 0;
                }
            }
            -2 // -ENOENT
        }
        _ => -22, // -EINVAL
    }
}

fn sys_epoll_wait(epfd: i32, events_ptr: usize, maxevents: i32, timeout: i32) -> i64 {
    if maxevents <= 0 { return -22; }
    if epfd < 0 || epfd as usize >= process::MAX_FDS { return -9; }
    let required_size = (maxevents as usize).saturating_mul(core::mem::size_of::<process::EpollEvent>());
    if !verify_user_buffer(events_ptr, required_size) {
        return -14; // -EFAULT
    }

    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };

    let ep_id = match process::fd_get(proc, epfd as usize) {
        Some(e) => match e.fd_type {
            process::FdType::Epoll(id) => id,
            _ => return -22,
        },
        None => return -9,
    };

    let start_tick = crate::time::pit::ticks();
    let wait_ticks = if timeout > 0 { ((timeout as u64) + 9) / 10 } else { 0 };

    loop {
        let mut ready_count = 0usize;
        let mut ready_events = [process::EpollEvent::default(); 16];

        {
            let table = process::EPOLL_TABLE.lock();
            let inst = match table.instances.get(ep_id as usize) {
                Some(i) if i.in_use => i,
                _ => return -9,
            };

            for w in inst.watches.iter() {
                if !w.in_use { continue; }
                let p_status = check_fd_poll(proc, w.fd, w.event.events as i16);
                let revents = (p_status & (w.event.events as i16)) | (p_status & (POLLERR | POLLHUP));
                if revents != 0 {
                    if ready_count < (maxevents as usize).min(ready_events.len()) {
                        ready_events[ready_count] = process::EpollEvent {
                            events: revents as u32,
                            data: w.event.data,
                        };
                        ready_count += 1;
                    }
                }
            }
        }

        if ready_count > 0 || timeout == 0 {
            if ready_count > 0 {
                let bytes_to_copy = ready_count * core::mem::size_of::<process::EpollEvent>();
                let slice = unsafe {
                    core::slice::from_raw_parts(ready_events.as_ptr() as *const u8, bytes_to_copy)
                };
                if paging::copy_to_user(events_ptr, slice).is_err() {
                    return -14;
                }
            }
            return ready_count as i64;
        }

        if timeout > 0 && crate::time::pit::ticks().wrapping_sub(start_tick) >= wait_ticks {
            return 0;
        }

        unsafe {
            crate::process::scheduler::wait_hlt_preemptible();
        }
    }
}

fn sys_sysinfo(info_ptr: usize) -> i64 {
    if !verify_user_buffer(info_ptr, core::mem::size_of::<SysInfo>()) {
        return -14; // -EFAULT
    }
    let uptime_secs = crate::time::pit::ticks() / 100;
    let proc_count = process::active_process_count();
    let info = SysInfo {
        uptime: uptime_secs as i64,
        loads: [65536, 65536, 65536],
        // [b191 / T-011] Cifras reales: marcos de usuario del asignador físico (antes se
        // devolvía el montículo del núcleo, 64 MiB, y el JVM concluía "Too small maximum heap").
        totalram: { let (t, _, _, _) = crate::mem::marcos::estadisticas(); (t * PAGE_SIZE) as u64 },
        freeram: { let (_, l, _, _) = crate::mem::marcos::estadisticas(); (l * PAGE_SIZE) as u64 },
        sharedram: 0,
        bufferram: 0,
        totalswap: 0,
        freeswap: 0,
        procs: proc_count,
        pad: 0,
        totalhigh: 0,
        freehigh: 0,
        mem_unit: 1,
        _reserved: [],
    };
    let info_bytes = unsafe {
        core::slice::from_raw_parts(&info as *const SysInfo as *const u8, core::mem::size_of::<SysInfo>())
    };
    if paging::copy_to_user(info_ptr, info_bytes).is_err() {
        return -14;
    }
    0
}

fn sys_getrandom(buf_ptr: usize, buflen: usize, flags: u32) -> i64 {
    const VALID_FLAGS: u32 = 1 | 2; // GRND_NONBLOCK | GRND_RANDOM
    if (flags & !VALID_FLAGS) != 0 {
        return -22; // -EINVAL
    }
    if buflen == 0 {
        return 0;
    }
    if !verify_user_buffer(buf_ptr, buflen) {
        return -14; // -EFAULT
    }
    let mut kbuf = [0u8; 128];
    let mut total = 0usize;
    let mut state: u64 = unsafe { core::arch::x86_64::_rdtsc() } ^ 0x517cc1b727220a95 ^ (crate::time::pit::ticks() << 16);

    while total < buflen {
        let chunk_size = (buflen - total).min(128);
        for chunk in kbuf[..chunk_size].chunks_mut(8) {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let val = state.wrapping_mul(0x2545F4914F6CDD1D);
            let bytes = val.to_ne_bytes();
            let clen = chunk.len();
            chunk.copy_from_slice(&bytes[..clen]);
        }
        if paging::copy_to_user(buf_ptr + total, &kbuf[..chunk_size]).is_err() {
            return -14;
        }
        total += chunk_size;
    }
    total as i64
}

fn sys_openat(dirfd: i32, path_ptr: usize, flags: u32) -> i64 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    match con_dirfd(dirfd, || sys_open(path_ptr, flags)) { Ok(r) => r, Err(e) => e as i64 }
}

fn sys_mkdirat(dirfd: i32, path_ptr: usize) -> i32 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    match con_dirfd(dirfd, || sys_mkdir(path_ptr, 0o755) as i32) { Ok(r) => r, Err(e) => e }
}

fn sys_unlinkat(dirfd: i32, path_ptr: usize, flags: i32) -> i32 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    if (flags & !AT_REMOVEDIR) != 0 {
        return -22; // -EINVAL
    }
    match con_dirfd(dirfd, || if (flags & AT_REMOVEDIR) != 0 { sys_rmdir(path_ptr) } else { sys_unlink(path_ptr) }) { Ok(r) => r, Err(e) => e }
}

fn sys_readlinkat(dirfd: i32, path_ptr: usize, buf_ptr: usize, bufsiz: usize) -> i64 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    match con_dirfd(dirfd, || sys_readlink(path_ptr, buf_ptr, bufsiz)) { Ok(r) => r, Err(e) => e as i64 }
}

fn sys_fstatat(dirfd: i32, path_ptr: usize, stat_ptr: usize, flags: i32) -> i32 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    if (flags & !AT_SYMLINK_NOFOLLOW) != 0 {
        return -22; // -EINVAL
    }
    match con_dirfd(dirfd, || if (flags & AT_SYMLINK_NOFOLLOW) != 0 { sys_lstat(path_ptr, stat_ptr) as i32 } else { sys_stat(path_ptr, stat_ptr) as i32 }) { Ok(r) => r, Err(e) => e }
}

static MEMFD_COUNTER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);

fn sys_memfd_create(name_ptr: usize, flags: u32) -> i64 {
    const VALID_FLAGS: u32 = MFD_CLOEXEC | MFD_ALLOW_SEALING;
    if (flags & !VALID_FLAGS) != 0 {
        return -22; // -EINVAL
    }
    if !verify_user_string(name_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut name_buf = [0u8; 64];
    let name_len = match paging::copy_from_user(&mut name_buf, name_ptr, 64) {
        Ok(n) => n,
        Err(()) => return -14,
    };
    let actual_len = name_buf.iter().position(|&b| b == 0).unwrap_or(name_len);
    if actual_len == 0 {
        return -22; // -EINVAL
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };

    let id = MEMFD_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut path = [0u8; 64];
    let prefix = b"/tmp/.memfd_";
    path[..prefix.len()].copy_from_slice(prefix);
    let mut p_idx = prefix.len();
    let mut id_buf = [0u8; 10];
    let mut id_val = id;
    let mut id_len = 0;
    if id_val == 0 {
        id_buf[0] = b'0';
        id_len = 1;
    } else {
        while id_val > 0 {
            id_buf[id_len] = b'0' + (id_val % 10) as u8;
            id_val /= 10;
            id_len += 1;
        }
        id_buf[..id_len].reverse();
    }
    path[p_idx..p_idx + id_len].copy_from_slice(&id_buf[..id_len]);
    p_idx += id_len;

    let path_str = match core::str::from_utf8(&path[..p_idx]) {
        Ok(s) => s,
        Err(_) => return -14,
    };

    let create_res = vfs::with_ramfs(|ram| ram.create(path_str));
    if !create_res {
        return -12; // -ENOMEM
    }

    let mut info = process::FsFileInfo {
        path: [0; 256],
        path_len: p_idx,
        size: 0,
        flags: 0x0002, // O_RDWR
        pos: 0,
    };
    info.path[..p_idx].copy_from_slice(&path[..p_idx]);

    let mut entry = process::FdEntry::new(process::FdType::FsFile(info));
    if (flags & MFD_CLOEXEC) != 0 {
        entry.flags = 1;
    }
    match process::fd_alloc(proc, entry) {
        Some(fd) => fd as i64,
        None => -24, // -EMFILE
    }
}

fn sys_ioctl(fd: i32, request: u64, arg: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };

    match request {
        // [b190] FIOCLEX / FIONCLEX: poner o quitar FD_CLOEXEC por ioctl. CPython los usa
        // como via rapida de set_inheritable() al abrir cada modulo (open_code): sin ellos
        // devolvia EINVAL y el interprete moria con "Failed to import encodings module".
        0x5451 | 0x5450 => {
            let p2 = match process::current_process() { Some(p) => p, None => return -9 };
            match process::fd_get_mut(p2, fd as usize) {
                Some(e) => { e.flags = if request == 0x5451 { e.flags | 1 } else { e.flags & !1 }; 0 }
                None => -9,
            }
        }
        // [CONTROL DE TRABAJOS] grupo de procesos en primer plano de la consola.
        0x540F => { // TIOCGPGRP
            if !verify_user_buffer(arg, 4) { return -14; }
            let pg = crate::process::fg_pgid() as u32;
            if paging::copy_to_user(arg, &pg.to_le_bytes()).is_err() { return -14; }
            0
        }
        0x5410 => { // TIOCSPGRP
            if !verify_user_buffer(arg, 4) { return -14; }
            let mut b = [0u8; 4];
            if paging::copy_from_user(&mut b, arg, 4).is_err() { return -14; }
            let pg = u32::from_le_bytes(b) as u64;
            if pg == 0 { return -22; }
            // Sólo el grupo de primer plano (o quien ignora SIGTTOU) puede ceder la terminal.
            if let Some(err) = crate::process::signals::tty_access_check(crate::process::signals::SIGTTOU) {
                return err;
            }
            crate::process::set_fg_pgid(pg);
            0
        }
        TIOCGWINSZ => {
            match &entry.fd_type {
                process::FdType::Stdin | process::FdType::Stdout | process::FdType::Stderr | process::FdType::Device(_) | process::FdType::PtyMaster(_) | process::FdType::PtySlave(_) => {
                    if !verify_user_buffer(arg, core::mem::size_of::<WinSize>()) {
                        return -14; // -EFAULT
                    }
                    let ws = WinSize {
                        ws_row: 25,
                        ws_col: 80,
                        ws_xpixel: 1024,
                        ws_ypixel: 768,
                    };
                    let ws_bytes = unsafe {
                        core::slice::from_raw_parts(&ws as *const WinSize as *const u8, core::mem::size_of::<WinSize>())
                    };
                    if paging::copy_to_user(arg, ws_bytes).is_err() {
                        return -14;
                    }
                    0
                }
                _ => -25, // -ENOTTY
            }
        }
        TIOCSWINSZ => {
            match &entry.fd_type {
                process::FdType::Stdin | process::FdType::Stdout | process::FdType::Stderr | process::FdType::Device(_) | process::FdType::PtyMaster(_) | process::FdType::PtySlave(_) => {
                    if !verify_user_buffer(arg, core::mem::size_of::<WinSize>()) {
                        return -14; // -EFAULT
                    }
                    0
                }
                _ => -25, // -ENOTTY
            }
        }
        TCGETS => {
            const TERMIOS_NUCLEO: usize = 36;
            match &entry.fd_type {
                process::FdType::Stdin | process::FdType::Stdout | process::FdType::Stderr | process::FdType::Device(_) | process::FdType::PtyMaster(_) | process::FdType::PtySlave(_) => {
                    if !verify_user_buffer(arg, TERMIOS_NUCLEO) {
                        return -14; // -EFAULT
                    }
                    // [T-004] TCGETS copia el `struct termios` DEL NÚCLEO Linux: 4 banderas
                    // de 32 bits + c_line + c_cc[19] = 36 bytes, SIN velocidades (la
                    // velocidad va en CBAUD de c_cflag: 0x0F = B38400). Antes se
                    // escribían 60 bytes (la estructura de musl): glibc reserva 36 en la
                    // pila y abortaba con "*** stack smashing detected ***".
                    let mut raw = [0u8; TERMIOS_NUCLEO];
                    raw[0..4].copy_from_slice(&0x4500u32.to_le_bytes());  // c_iflag
                    raw[4..8].copy_from_slice(&0x5u32.to_le_bytes());     // c_oflag
                    raw[8..12].copy_from_slice(&0x4bfu32.to_le_bytes());  // c_cflag (B38400|CS8|CREAD|HUPCL)
                    raw[12..16].copy_from_slice(&0x8a3bu32.to_le_bytes()); // c_lflag
                    // c_cc por defecto de Linux: VINTR ^C, VQUIT ^\, VERASE DEL, VKILL ^U, VEOF ^D,
                    // VTIME 0, VMIN 1, VSTART ^Q, VSTOP ^S, VSUSP ^Z, VREPRINT ^R, VDISCARD ^O,
                    // VWERASE ^W, VLNEXT ^V
                    let cc: [u8; 19] = [3, 28, 127, 21, 4, 0, 1, 0, 17, 19, 26, 0, 18, 15, 23, 22, 0, 0, 0];
                    raw[17..36].copy_from_slice(&cc);
                    if paging::copy_to_user(arg, &raw).is_err() {
                        return -14;
                    }
                    0
                }
                _ => -25, // -ENOTTY
            }
        }
        TCSETS | TCSETSW | TCSETSF => {
            match &entry.fd_type {
                process::FdType::Stdin | process::FdType::Stdout | process::FdType::Stderr | process::FdType::Device(_) | process::FdType::PtyMaster(_) | process::FdType::PtySlave(_) => {
                    if !verify_user_buffer(arg, 36) { // [T-004] termios del núcleo Linux: 36 bytes
                        return -14;
                    }
                    0
                }
                _ => -25,
            }
        }
        0x80045430 => { // TIOCGPTN (get pty number)
            match &entry.fd_type {
                process::FdType::PtyMaster(id) => {
                    if !verify_user_buffer(arg, 4) { return -14; }
                    let id_val = *id as i32;
                    if paging::copy_to_user(arg, &id_val.to_ne_bytes()).is_err() {
                        return -14;
                    }
                    0
                }
                _ => -25, // -ENOTTY
            }
        }
        0x40045431 => { // TIOCSPTLCK (lock/unlock pty)
            match &entry.fd_type {
                process::FdType::PtyMaster(id) => {
                    if !verify_user_buffer(arg, 4) { return -14; }
                    let mut lock_val = [0u8; 4];
                    if paging::copy_from_user(&mut lock_val, arg, 4).is_err() {
                        return -14;
                    }
                    let lock = i32::from_ne_bytes(lock_val);
                    let mut ptab = process::PTY_TABLE.lock();
                    if let Some(pty) = ptab.ptys.get_mut(*id as usize) {
                        pty.locked = lock != 0;
                        0
                    } else {
                        -9
                    }
                }
                _ => -25, // -ENOTTY
            }
        }
        FIONREAD => {
            if !verify_user_buffer(arg, 4) {
                return -14; // -EFAULT
            }
            let bytes_avail: i32 = match &entry.fd_type {
                process::FdType::PipeRead(pipe_id) => {
                    let ptab = process::PIPE_TABLE.lock();
                    ptab.pipes.get(*pipe_id as usize).map(|p| p.count as i32).unwrap_or(0)
                }
                process::FdType::SocketPairEnd { read_pipe, .. } => {
                    let ptab = process::PIPE_TABLE.lock();
                    ptab.pipes.get(*read_pipe as usize).map(|p| p.count as i32).unwrap_or(0)
                }
                process::FdType::EventFd(id) => {
                    let table = process::EVENTFD_TABLE.lock();
                    if table.fds.get(*id as usize).map(|ef| ef.val > 0).unwrap_or(false) {
                        8
                    } else {
                        0
                    }
                }
                process::FdType::PtyMaster(id) => {
                    let ptab = process::PTY_TABLE.lock();
                    if let Some(pty) = ptab.ptys.get(*id as usize) {
                        let pipes = process::PIPE_TABLE.lock();
                        pipes.pipes.get(pty.master_pipe as usize).map(|p| p.count as i32).unwrap_or(0)
                    } else { 0 }
                }
                process::FdType::PtySlave(id) => {
                    let ptab = process::PTY_TABLE.lock();
                    if let Some(pty) = ptab.ptys.get(*id as usize) {
                        let pipes = process::PIPE_TABLE.lock();
                        pipes.pipes.get(pty.slave_pipe as usize).map(|p| p.count as i32).unwrap_or(0)
                    } else { 0 }
                }
                process::FdType::FsFile(info) => {
                    let path_str = core::str::from_utf8(&info.path[..info.path_len]).unwrap_or("");
                    vfs::meta_fs(path_str).map(|m| (m.3.saturating_sub(info.pos as usize)) as i32).unwrap_or(0)
                }
                _ => 0,
            };
            if paging::copy_to_user(arg, &bytes_avail.to_ne_bytes()).is_err() {
                return -14;
            }
            0
        }
        FIONBIO => {
            if !verify_user_buffer(arg, 4) {
                return -14;
            }
            0
        }
        0x4C00 => { // LOOP_SET_FD
            0
        }
        0x4C01 => { // LOOP_CLR_FD
            0
        }
        0x4C04 => { // LOOP_SET_STATUS64
            0
        }
        0x4C05 => { // LOOP_GET_STATUS64
            if arg != 0 && verify_user_buffer(arg, 64) {
                let dummy_status = [0u8; 64];
                let _ = paging::copy_to_user(arg, &dummy_status);
            }
            0
        }
        0x4C82 => { // LOOP_CTL_GET_FREE
            0
        }
        _ => -22, // -EINVAL
    }
}

fn sys_setsockopt(fd: i32, level: i32, optname: i32, optval: usize, optlen: u32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match &entry.fd_type {
        process::FdType::Socket(_) | process::FdType::SocketPairEnd { .. } => {}
        _ => return -88, // -ENOTSOCK
    }
    if level != SOL_SOCKET {
        return -92; // -ENOPROTOOPT
    }
    if optlen == 0 || !verify_user_buffer(optval, optlen as usize) {
        return -14; // -EFAULT
    }
    match optname {
        SO_REUSEADDR | SO_KEEPALIVE | SO_BROADCAST | SO_DEBUG | SO_DONTROUTE | SO_SNDBUF | SO_RCVBUF => 0,
        _ => -22, // -EINVAL
    }
}

fn sys_getsockopt(fd: i32, level: i32, optname: i32, optval: usize, optlen_ptr: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match &entry.fd_type {
        process::FdType::Socket(_) | process::FdType::SocketPairEnd { .. } => {}
        _ => return -88, // -ENOTSOCK
    }
    if level != SOL_SOCKET {
        return -92; // -ENOPROTOOPT
    }
    if !verify_user_buffer(optlen_ptr, 4) {
        return -14; // -EFAULT
    }
    let mut len_bytes = [0u8; 4];
    if paging::copy_from_user(&mut len_bytes, optlen_ptr, 4).is_err() {
        return -14;
    }
    let optlen = u32::from_ne_bytes(len_bytes) as usize;
    if optlen < 4 || !verify_user_buffer(optval, 4) {
        return -14; // -EFAULT
    }

    if optname == 17 { // SO_PEERCRED
        if optlen < 12 || !verify_user_buffer(optval, 12) {
            return -14;
        }
        #[repr(C)]
        struct Ucred { pid: i32, uid: u32, gid: u32 }
        let cred = Ucred {
            pid: proc.pid as i32,
            uid: proc.uid,
            gid: proc.gid,
        };
        let cred_bytes = unsafe {
            core::slice::from_raw_parts(&cred as *const _ as *const u8, 12)
        };
        if paging::copy_to_user(optval, cred_bytes).is_err() {
            return -14;
        }
        let out_len: u32 = 12;
        let _ = paging::copy_to_user(optlen_ptr, &out_len.to_ne_bytes());
        return 0;
    }

    let val: i32 = match optname {
        SO_TYPE => 1, // SOCK_STREAM
        SO_ERROR => 0,
        SO_REUSEADDR => 1,
        SO_SNDBUF | SO_RCVBUF => 65536,
        SO_KEEPALIVE => 0,
        _ => return -22, // -EINVAL
    };

    if paging::copy_to_user(optval, &val.to_ne_bytes()).is_err() {
        return -14;
    }
    let out_len: u32 = 4;
    let _ = paging::copy_to_user(optlen_ptr, &out_len.to_ne_bytes());
    0
}

fn sys_sethostname(name_ptr: usize, len: usize) -> i64 {
    if len == 0 || len > 64 {
        return -22; // -EINVAL
    }
    if !verify_user_buffer(name_ptr, len) {
        return -14; // -EFAULT
    }
    let mut kbuf = [0u8; 64];
    if paging::copy_from_user(&mut kbuf[..len], name_ptr, len).is_err() {
        return -14;
    }
    let mut h = HOSTNAME.lock();
    h.fill(0);
    h[..len].copy_from_slice(&kbuf[..len]);
    0
}

fn sys_gethostname(name_ptr: usize, len: usize) -> i64 {
    if len == 0 {
        return -22; // -EINVAL
    }
    if !verify_user_buffer(name_ptr, len) {
        return -14; // -EFAULT
    }
    let h = HOSTNAME.lock();
    let act_len = h.iter().position(|&b| b == 0).unwrap_or(h.len()).min(len - 1);
    let mut out = alloc::vec![0u8; act_len + 1];
    out[..act_len].copy_from_slice(&h[..act_len]);
    out[act_len] = 0;
    if paging::copy_to_user(name_ptr, &out).is_err() {
        return -14;
    }
    0
}

fn sys_gettimeofday(tv_ptr: usize, tz_ptr: usize) -> i64 {
    if tv_ptr != 0 {
        if !verify_user_buffer(tv_ptr, core::mem::size_of::<TimeVal>()) {
            return -14; // -EFAULT
        }
        let ticks = crate::time::pit::ticks();
        let sec = crate::time::rtc::now_epoch(); // RTC CMOS real + PIT
        let usec = ((ticks % 100) * 10000) as i64;
        let tv = TimeVal { tv_sec: sec, tv_usec: usec };
        let tv_bytes = unsafe {
            core::slice::from_raw_parts(&tv as *const TimeVal as *const u8, core::mem::size_of::<TimeVal>())
        };
        if paging::copy_to_user(tv_ptr, tv_bytes).is_err() {
            return -14;
        }
    }
    if tz_ptr != 0 {
        if !verify_user_buffer(tz_ptr, core::mem::size_of::<TimeZone>()) {
            return -14;
        }
        let tz = TimeZone { tz_minuteswest: 0, tz_dsttime: 0 };
        let tz_bytes = unsafe {
            core::slice::from_raw_parts(&tz as *const TimeZone as *const u8, core::mem::size_of::<TimeZone>())
        };
        if paging::copy_to_user(tz_ptr, tz_bytes).is_err() {
            return -14;
        }
    }
    0
}

fn sys_getitimer(which: i32, curr_value_ptr: usize) -> i64 {
    if which != ITIMER_REAL && which != ITIMER_VIRTUAL && which != ITIMER_PROF {
        return -22; // -EINVAL
    }
    if !verify_user_buffer(curr_value_ptr, core::mem::size_of::<ITimerVal>()) {
        return -14; // -EFAULT
    }
    let remaining_ticks = process::current_process().map(|p| {
        let cur = crate::time::pit::ticks();
        if p.alarm_ticks > cur { p.alarm_ticks - cur } else { 0 }
    }).unwrap_or(0);

    let it = ITimerVal {
        it_interval: TimeVal { tv_sec: 0, tv_usec: 0 },
        it_value: TimeVal {
            tv_sec: (remaining_ticks / 100) as i64,
            tv_usec: ((remaining_ticks % 100) * 10000) as i64,
        },
    };
    let it_bytes = unsafe {
        core::slice::from_raw_parts(&it as *const ITimerVal as *const u8, core::mem::size_of::<ITimerVal>())
    };
    if paging::copy_to_user(curr_value_ptr, it_bytes).is_err() {
        return -14;
    }
    0
}

fn sys_setitimer(which: i32, new_value_ptr: usize, old_value_ptr: usize) -> i64 {
    if which != ITIMER_REAL && which != ITIMER_VIRTUAL && which != ITIMER_PROF {
        return -22; // -EINVAL
    }
    if !verify_user_buffer(new_value_ptr, core::mem::size_of::<ITimerVal>()) {
        return -14; // -EFAULT
    }
    let mut new_it = ITimerVal::default();
    let new_bytes = unsafe {
        core::slice::from_raw_parts_mut(&mut new_it as *mut ITimerVal as *mut u8, core::mem::size_of::<ITimerVal>())
    };
    if paging::copy_from_user(new_bytes, new_value_ptr, new_bytes.len()).is_err() {
        return -14;
    }
    if new_it.it_value.tv_usec < 0 || new_it.it_value.tv_usec >= 1_000_000 || new_it.it_value.tv_sec < 0 {
        return -22; // -EINVAL
    }

    if old_value_ptr != 0 {
        let _ = sys_getitimer(which, old_value_ptr);
    }

    let ticks_to_add = (new_it.it_value.tv_sec as u64) * 100 + ((new_it.it_value.tv_usec as u64) + 9999) / 10000;
    if let Some(p) = process::current_process() {
        if ticks_to_add > 0 {
            p.alarm_ticks = crate::time::pit::ticks() + ticks_to_add;
        } else {
            p.alarm_ticks = 0;
        }
    }
    0
}

fn sys_mknod(path_ptr: usize, mode: u32, _dev: u64) -> i64 {
    if !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14,
    };
    let file_type = mode & 0o170000;
    if file_type != 0o010000 && file_type != 0o020000 && file_type != 0o100000 {
        return -22; // -EINVAL
    }
    if (file_type == 0o020000 || file_type == 0o060000) && process::current_euid() != 0 {
        return -1; // -EPERM
    }
    crear_nodo_especial(path, mode) as i64
}

/// [CICLO 48] Crea un nodo en ramfs con el tipo pedido (S_IFIFO real: `open`
/// lo tratara como tuberia con nombre). Devuelve 0, -EEXIST o -ENOENT.
fn crear_nodo_especial(raw: &str, mode: u32) -> i32 {
    let mut buf = [0u8; 256];
    let n = resolver_con_cwd(raw, &mut buf);
    let path = core::str::from_utf8(&buf[..n]).unwrap_or("");
    let umask = process::current_umask();
    if !vfs::is_ramfs_path(path) {
        // [T-001] En ext2 (y cualquier sistema del contrato) mknod de un
        // fichero regular es crear(2) y un FIFO es un inodo S_IFIFO real.
        // Dispositivos fuera de ramfs: EPERM, nunca un 0 sin nodo detrás.
        if mode & 0o170000 == 0o100000 || mode & 0o170000 == 0o010000 {
            // [T-001] fichero regular o FIFO: el contrato recibe tipo + permisos
            return match vfs::con_fs(path, |fs| fs.crear(path, (mode & 0o170000) | (mode & 0o777 & !umask))) {
                Some(Ok(())) => 0,
                Some(Err(e)) => -e,
                None => -30, // EROFS: /boot FAT, devfs
            };
        }
        return if vfs::con_fs(path, |_| ()).is_some() { -1 } else { -30 };
    }
    if vfs::with_ramfs(|ram| ram.get_metadata(path).is_some()) { return -17; } // -EEXIST
    let ok = vfs::with_ramfs(|ram| ram.create(path) && ram.set_type(path, mode & 0o170000, mode & 0o777 & !umask));
    if ok { 0 } else { -2 }
}

fn sys_fchdir(dirfd: i32) -> i32 {
    if dirfd < 0 || dirfd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let (path_buf, path_len) = match process::fd_get(proc, dirfd as usize) {
        Some(entry) => match &entry.fd_type {
            process::FdType::FsFile(info) => (info.path, info.path_len),
            _ => return -20, // -ENOTDIR
        },
        None => return -9,
    };

    let path_str = match core::str::from_utf8(&path_buf[..path_len]) {
        Ok(s) => s,
        Err(_) => return -20, // -ENOTDIR
    };
    if !crate::drivers::storage::vfs::es_dir_fs(path_str) {
        return -20; // -ENOTDIR
    }
    if path_len > proc.cwd.len() { return -36; } // -ENAMETOOLONG (cwd de 64 B; antes `copy_from_slice` habría hecho PANIC)
    proc.cwd[..path_len].copy_from_slice(&path_buf[..path_len]);
    proc.cwd_len = path_len;
    0
}

/// [T-004] `struct statfs` de Linux x86-64 (120 bytes) rellenada con los datos
/// reales del sistema de ficheros que sirve `ruta` (`espacio()` y `tipo()`).
fn statfs_linux(ruta: &str, buf_ptr: usize) -> i64 {
    if !verify_user_buffer(buf_ptr, 120) { return -14; }
    let Some((libres, totales, bloque, tipo)) = crate::drivers::storage::vfs::con_fs(ruta, |fs| { let (l, t, b) = fs.espacio(); (l, t, b, fs.tipo()) }) else { return -2; };
    let f_type: i64 = match tipo { t if t.starts_with("ext2") || t.starts_with("ext") => 0xEF53, t if t.starts_with("fat") || t.starts_with("vfat") => 0x4d44, t if t.starts_with("ramfs") || t.starts_with("tmpfs") => 0x01021994, t if t.starts_with("proc") => 0x9fa0, _ => 0x50455254 };
    let mut b = [0u8; 120];
    let mut w = |o: usize, v: u64| b[o..o + 8].copy_from_slice(&v.to_le_bytes());
    w(0, f_type as u64); w(8, bloque as u64); w(16, totales); w(24, libres); w(32, libres);
    w(40, 0); w(48, 0); // f_files/f_ffree: el contrato no los expone (0 = desconocido)
    w(56, 42); // f_fsid
    w(64, 255); w(72, bloque as u64); w(80, 0);
    if paging::copy_to_user(buf_ptr, &b).is_err() { return -14; }
    0
}

fn sys_statfs_linux(path_ptr: usize, buf_ptr: usize) -> i64 {
    let mut pb = [0u8; 256];
    let Some(ruta) = read_user_path(path_ptr, &mut pb) else { return -14 };
    if ruta != "/" && crate::drivers::storage::vfs::meta_fs(ruta).is_none() && !crate::drivers::storage::vfs::es_dir_fs(ruta) { return -2; }
    statfs_linux(ruta, buf_ptr)
}

fn sys_fstatfs_linux(fd: i32, buf_ptr: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS { return -9; }
    let Some(proc) = process::current_process() else { return -9 };
    match process::fd_get(proc, fd as usize).map(|e| e.fd_type) {
        Some(process::FdType::FsFile(info)) => {
            let Ok(ruta) = core::str::from_utf8(&info.path[..info.path_len]) else { return -22 };
            statfs_linux(ruta, buf_ptr)
        }
        Some(_) => statfs_linux("/", buf_ptr),
        None => -9,
    }
}

fn sys_symlinkat(target_ptr: usize, newdirfd: i32, linkpath_ptr: usize) -> i32 {
    if newdirfd != AT_FDCWD && (newdirfd < 0 || newdirfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    match con_dirfd(newdirfd, || sys_symlink(target_ptr, linkpath_ptr)) { Ok(r) => r, Err(e) => e }
}

fn sys_linkat(olddirfd: i32, oldpath_ptr: usize, newdirfd: i32, newpath_ptr: usize, flags: i32) -> i32 {
    if olddirfd != AT_FDCWD && (olddirfd < 0 || olddirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if newdirfd != AT_FDCWD && (newdirfd < 0 || newdirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if (flags & !(AT_SYMLINK_FOLLOW | AT_SYMLINK_NOFOLLOW)) != 0 {
        return -22; // -EINVAL
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if olddirfd >= 0 && process::fd_get(proc, olddirfd as usize).is_none() {
        return -9;
    }
    if newdirfd >= 0 && process::fd_get(proc, newdirfd as usize).is_none() {
        return -9;
    }
    sys_link(oldpath_ptr, newpath_ptr) as i32
}

fn sys_renameat(olddirfd: i32, oldpath_ptr: usize, newdirfd: i32, newpath_ptr: usize) -> i32 {
    if olddirfd != AT_FDCWD && (olddirfd < 0 || olddirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if newdirfd != AT_FDCWD && (newdirfd < 0 || newdirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if olddirfd >= 0 && process::fd_get(proc, olddirfd as usize).is_none() {
        return -9;
    }
    if newdirfd >= 0 && process::fd_get(proc, newdirfd as usize).is_none() {
        return -9;
    }
    sys_rename(oldpath_ptr, newpath_ptr)
}

fn sys_fchmodat(dirfd: i32, path_ptr: usize, mode: u32, flags: i32) -> i32 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if (flags & !AT_SYMLINK_NOFOLLOW) != 0 {
        return -22;
    }
    match con_dirfd(dirfd, || sys_chmod(path_ptr, mode) as i32) { Ok(r) => r, Err(e) => e }
}

fn sys_fchownat(dirfd: i32, path_ptr: usize, owner: u32, group: u32, flags: i32) -> i32 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if (flags & !AT_SYMLINK_NOFOLLOW) != 0 {
        return -22;
    }
    match con_dirfd(dirfd, || sys_chown(path_ptr, owner, group) as i32) { Ok(r) => r, Err(e) => e }
}

fn sys_faccessat(dirfd: i32, path_ptr: usize, mode: i32, flags: i32) -> i32 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if (flags & !(AT_EACCESS | AT_SYMLINK_NOFOLLOW)) != 0 {
        return -22;
    }
    match con_dirfd(dirfd, || sys_access(path_ptr, mode)) { Ok(r) => r, Err(e) => e }
}

fn sys_utimensat(dirfd: i32, path_ptr: usize, times_ptr: usize, flags: i32) -> i32 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if (flags & !AT_SYMLINK_NOFOLLOW) != 0 {
        return -22;
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if dirfd >= 0 && process::fd_get(proc, dirfd as usize).is_none() {
        return -9;
    }
    if times_ptr != 0 && !verify_user_buffer(times_ptr, core::mem::size_of::<TimeVal>() * 2) {
        return -14;
    }
    // utimensat(fd, NULL, ...) opera sobre el propio fd (futimens)
    if path_ptr == 0 { return if dirfd >= 0 { 0 } else { -14 }; }
    match con_dirfd(dirfd, || sys_access(path_ptr, 0)) { Ok(0) => 0, Ok(e) => e, Err(e) => e }
}

fn sys_getrusage(who: i32, usage_ptr: usize) -> i64 {
    if who != RUSAGE_SELF && who != RUSAGE_CHILDREN && who != RUSAGE_THREAD {
        return -22; // -EINVAL
    }
    if !verify_user_buffer(usage_ptr, core::mem::size_of::<RUsage>()) {
        return -14; // -EFAULT
    }
    let mut usage = RUsage::default();
    let ticks = crate::time::pit::ticks();
    usage.ru_utime.tv_sec = (ticks / 100) as i64;
    usage.ru_utime.tv_usec = ((ticks % 100) * 10000) as i64;
    usage.ru_stime.tv_sec = (ticks / 200) as i64;
    usage.ru_stime.tv_usec = ((ticks % 200) * 5000) as i64;
    usage.ru_maxrss = 4096;

    let usage_bytes = unsafe {
        core::slice::from_raw_parts(&usage as *const RUsage as *const u8, core::mem::size_of::<RUsage>())
    };
    if paging::copy_to_user(usage_ptr, usage_bytes).is_err() {
        return -14;
    }
    0
}

fn sys_prlimit64(pid: i32, resource: i32, new_limit_ptr: usize, old_limit_ptr: usize) -> i64 {
    if resource < 0 || resource > 15 {
        return -22; // -EINVAL
    }
    if pid < 0 {
        return -3; // -ESRCH
    }
    if pid > 0 {
        if process::process_by_pid(pid as u64).is_none() {
            return -3; // -ESRCH
        }
    }
    if old_limit_ptr == 0 && new_limit_ptr == 0 {
        return -14; // -EFAULT
    }
    if old_limit_ptr != 0 {
        if !verify_user_buffer(old_limit_ptr, core::mem::size_of::<RLimit64>()) {
            return -14; // -EFAULT
        }
        // [b191] Misma tabla que getrlimit nativo (antes el 9 se trataba como NPROC=128 y era
        // RLIMIT_AS: el JVM leía "128 bytes de espacio de direcciones" → "Too small maximum heap").
        let (cur_val, max_val) = match crate::process::getrlimit(resource) {
            Ok(v) => v,
            Err(_) => return -22,
        };
        let lim = RLimit64 {
            rlim_cur: cur_val,
            rlim_max: max_val,
        };
        let lim_bytes = unsafe {
            core::slice::from_raw_parts(&lim as *const RLimit64 as *const u8, core::mem::size_of::<RLimit64>())
        };
        if paging::copy_to_user(old_limit_ptr, lim_bytes).is_err() {
            return -14;
        }
    }
    if new_limit_ptr != 0 {
        if !verify_user_buffer(new_limit_ptr, core::mem::size_of::<RLimit64>()) {
            return -14; // -EFAULT
        }
        if process::current_euid() != 0 {
            return -1; // -EPERM
        }
    }
    0
}

// ── Syscalls 150-159: Advanced POSIX & Linux Extensions ───────────────────

fn sys_clock_nanosleep(clock_id: i32, flags: i32, req_ptr: usize, rem_ptr: usize, current_rsp: u64) -> (i64, u64) {
    if clock_id != 0 && clock_id != 1 { // 0=CLOCK_REALTIME, 1=CLOCK_MONOTONIC
        return (-22, 0); // -EINVAL
    }
    if (flags & !TFD_TIMER_ABSTIME) != 0 {
        return (-22, 0); // -EINVAL
    }
    if !verify_user_buffer(req_ptr, core::mem::size_of::<Timespec>()) {
        return (-14, 0); // -EFAULT
    }
    let mut req = Timespec::default();
    let req_bytes = unsafe {
        core::slice::from_raw_parts_mut(&mut req as *mut Timespec as *mut u8, core::mem::size_of::<Timespec>())
    };
    if paging::copy_from_user(req_bytes, req_ptr, core::mem::size_of::<Timespec>()).is_err() {
        return (-14, 0);
    }
    if req.tv_sec < 0 || req.tv_nsec < 0 || req.tv_nsec >= 1_000_000_000 {
        return (-22, 0); // -EINVAL
    }
    if (flags & TFD_TIMER_ABSTIME) != 0 {
        // Absolute sleep
        let cur_sec = crate::time::pit::uptime_secs() as i64;
        let cur_nsec = ((crate::time::pit::ticks() % 100) * 10_000_000) as i64;
        let delta_sec = req.tv_sec - cur_sec;
        let delta_nsec = req.tv_nsec - cur_nsec;
        if delta_sec < 0 || (delta_sec == 0 && delta_nsec <= 0) {
            return (0, 0);
        }
        let (adj_sec, adj_nsec) = if delta_nsec < 0 {
            (delta_sec - 1, delta_nsec + 1_000_000_000)
        } else {
            (delta_sec, delta_nsec)
        };
        let rel_req = Timespec { tv_sec: adj_sec, tv_nsec: adj_nsec };
        let ticks = (rel_req.tv_sec as u64 * 100) + ((rel_req.tv_nsec as u64 + 9_999_999) / 10_000_000);
        if ticks == 0 {
            return (0, 0);
        }
        // [T-005] TIMER_ABSTIME: se duerme el intervalo RELATIVO calculado
        // (antes se pasaba el puntero al absoluto y se dormía ese valor).
        // CLOCK_REALTIME absoluto: el reloj de pared difiere del uptime; se
        // convierte restando el epoch actual.
        let ticks = if clock_id == 0 {
            let epoch = crate::time::rtc::now_epoch();
            let d = req.tv_sec - epoch;
            if d < 0 { return (0, 0); }
            (d as u64 * 100).saturating_add(((req.tv_nsec as u64) + 9_999_999) / 10_000_000).max(1)
        } else { ticks };
        let _ = rem_ptr;
        let new_rsp = sys_sleep_switch(ticks, current_rsp);
        (0, new_rsp)
    } else {
        sys_nanosleep(req_ptr, rem_ptr, current_rsp)
    }
}

fn sys_timerfd_create(clockid: i32, flags: i32) -> i64 {
    if clockid != 0 && clockid != 1 { // 0=CLOCK_REALTIME, 1=CLOCK_MONOTONIC
        return -22; // -EINVAL
    }
    if (flags & !(TFD_CLOEXEC | TFD_NONBLOCK)) != 0 {
        return -22; // -EINVAL
    }
    let id = match process::timerfd_alloc(clockid, flags) {
        Some(i) => i,
        None => return -24, // -EMFILE
    };
    let proc = match process::current_process() {
        Some(p) => p,
        None => {
            process::timerfd_close(id);
            return -24;
        }
    };
    let cloexec = (flags & TFD_CLOEXEC) != 0;
    let mut entry = process::FdEntry::new(process::FdType::TimerFd(id));
    entry.flags = if cloexec { 1 } else { 0 };
    if (flags & TFD_NONBLOCK) != 0 {
        entry.flags |= 0x800;
    }
    match process::fd_alloc(proc, entry) {
        Some(fd) => fd as i64,
        None => {
            process::timerfd_close(id);
            -24 // -EMFILE
        }
    }
}

fn sys_timerfd_settime(fd: i32, flags: i32, new_value_ptr: usize, old_value_ptr: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if (flags & !TFD_TIMER_ABSTIME) != 0 {
        return -22; // -EINVAL
    }
    if !verify_user_buffer(new_value_ptr, core::mem::size_of::<ITimerSpec>()) {
        return -14; // -EFAULT
    }
    let mut new_val = ITimerSpec::default();
    let new_bytes = unsafe {
        core::slice::from_raw_parts_mut(&mut new_val as *mut ITimerSpec as *mut u8, core::mem::size_of::<ITimerSpec>())
    };
    if paging::copy_from_user(new_bytes, new_value_ptr, core::mem::size_of::<ITimerSpec>()).is_err() {
        return -14;
    }
    if new_val.it_interval.tv_sec < 0 || new_val.it_interval.tv_nsec < 0 || new_val.it_interval.tv_nsec >= 1_000_000_000 ||
       new_val.it_value.tv_sec < 0 || new_val.it_value.tv_nsec < 0 || new_val.it_value.tv_nsec >= 1_000_000_000 {
        return -22; // -EINVAL
    }

    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let fde = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    let timer_id = match fde.fd_type {
        process::FdType::TimerFd(id) => id,
        _ => return -22, // -EINVAL
    };

    let mut table = process::TIMERFD_TABLE.lock();
    let tf = match table.fds.get_mut(timer_id as usize) {
        Some(t) if t.in_use => t,
        _ => return -9,
    };

    let cur_ticks = crate::time::pit::ticks();

    if old_value_ptr != 0 {
        if !verify_user_buffer(old_value_ptr, core::mem::size_of::<ITimerSpec>()) {
            return -14;
        }
        let rem_ticks = if tf.expire_ticks > cur_ticks { tf.expire_ticks - cur_ticks } else { 0 };
        let old_val = ITimerSpec {
            it_interval: Timespec {
                tv_sec: (tf.interval_ticks / 100) as i64,
                tv_nsec: ((tf.interval_ticks % 100) * 10_000_000) as i64,
            },
            it_value: Timespec {
                tv_sec: (rem_ticks / 100) as i64,
                tv_nsec: ((rem_ticks % 100) * 10_000_000) as i64,
            },
        };
        let old_bytes = unsafe {
            core::slice::from_raw_parts(&old_val as *const ITimerSpec as *const u8, core::mem::size_of::<ITimerSpec>())
        };
        if paging::copy_to_user(old_value_ptr, old_bytes).is_err() {
            return -14;
        }
    }

    let interval_ticks = (new_val.it_interval.tv_sec as u64 * 100) + ((new_val.it_interval.tv_nsec as u64 + 9_999_999) / 10_000_000);
    tf.interval_ticks = interval_ticks;

    if new_val.it_value.tv_sec == 0 && new_val.it_value.tv_nsec == 0 {
        tf.expire_ticks = 0;
    } else if (flags & TFD_TIMER_ABSTIME) != 0 {
        let val_ticks = (new_val.it_value.tv_sec as u64 * 100) + ((new_val.it_value.tv_nsec as u64 + 9_999_999) / 10_000_000);
        tf.expire_ticks = val_ticks;
    } else {
        let val_ticks = (new_val.it_value.tv_sec as u64 * 100) + ((new_val.it_value.tv_nsec as u64 + 9_999_999) / 10_000_000);
        tf.expire_ticks = cur_ticks + val_ticks;
    }
    tf.expirations = 0;

    0
}

fn sys_timerfd_gettime(fd: i32, curr_value_ptr: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if !verify_user_buffer(curr_value_ptr, core::mem::size_of::<ITimerSpec>()) {
        return -14; // -EFAULT
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let fde = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    let timer_id = match fde.fd_type {
        process::FdType::TimerFd(id) => id,
        _ => return -22, // -EINVAL
    };

    let table = process::TIMERFD_TABLE.lock();
    let tf = match table.fds.get(timer_id as usize) {
        Some(t) if t.in_use => t,
        _ => return -9,
    };

    let cur_ticks = crate::time::pit::ticks();
    let rem_ticks = if tf.expire_ticks > cur_ticks { tf.expire_ticks - cur_ticks } else { 0 };

    let cur_val = ITimerSpec {
        it_interval: Timespec {
            tv_sec: (tf.interval_ticks / 100) as i64,
            tv_nsec: ((tf.interval_ticks % 100) * 10_000_000) as i64,
        },
        it_value: Timespec {
            tv_sec: (rem_ticks / 100) as i64,
            tv_nsec: ((rem_ticks % 100) * 10_000_000) as i64,
        },
    };

    let cur_bytes = unsafe {
        core::slice::from_raw_parts(&cur_val as *const ITimerSpec as *const u8, core::mem::size_of::<ITimerSpec>())
    };
    if paging::copy_to_user(curr_value_ptr, cur_bytes).is_err() {
        return -14;
    }
    0
}

fn sys_signalfd4(fd: i32, mask_ptr: usize, sizemask: usize, flags: i32) -> i64 {
    if sizemask != 8 && sizemask != 128 {
        return -22; // -EINVAL
    }
    if (flags & !(SFD_CLOEXEC | SFD_NONBLOCK)) != 0 {
        return -22; // -EINVAL
    }
    if !verify_user_buffer(mask_ptr, 8) {
        return -14; // -EFAULT
    }
    let mut mask_bytes = [0u8; 8];
    if paging::copy_from_user(&mut mask_bytes, mask_ptr, 8).is_err() {
        return -14;
    }
    let mask = u64::from_ne_bytes(mask_bytes);

    if fd == -1 {
        let id = match process::signalfd_alloc(mask, flags) {
            Some(i) => i,
            None => return -24, // -EMFILE
        };
        let proc = match process::current_process() {
            Some(p) => p,
            None => {
                process::signalfd_close(id);
                return -24;
            }
        };
        let cloexec = (flags & SFD_CLOEXEC) != 0;
        let mut entry = process::FdEntry::new(process::FdType::SignalFd(id));
        entry.flags = if cloexec { 1 } else { 0 };
        if (flags & SFD_NONBLOCK) != 0 {
            entry.flags |= 0x800;
        }
        match process::fd_alloc(proc, entry) {
            Some(f) => f as i64,
            None => {
                process::signalfd_close(id);
                -24 // -EMFILE
            }
        }
    } else {
        if fd < 0 || fd as usize >= process::MAX_FDS {
            return -9; // -EBADF
        }
        let proc = match process::current_process() {
            Some(p) => p,
            None => return -9,
        };
        let fde = match process::fd_get(proc, fd as usize) {
            Some(e) => e,
            None => return -9,
        };
        let sig_id = match fde.fd_type {
            process::FdType::SignalFd(id) => id,
            _ => return -22, // -EINVAL
        };
        let mut table = process::SIGNALFD_TABLE.lock();
        if let Some(sf) = table.fds.get_mut(sig_id as usize) {
            if sf.in_use {
                sf.mask = mask;
                return fd as i64;
            }
        }
        -9
    }
}

fn sys_posix_openpt(flags: i32) -> i64 {
    let id = match process::pty_alloc() {
        Some(i) => i,
        None => return -24, // -EMFILE
    };
    let proc = match process::current_process() {
        Some(p) => p,
        None => {
            process::pty_close(id);
            return -24;
        }
    };
    let cloexec = (flags & 0x80000) != 0;
    let mut entry = process::FdEntry::new(process::FdType::PtyMaster(id));
    entry.flags = if cloexec { 1 } else { 0 };
    if (flags & 0x800) != 0 {
        entry.flags |= 0x800;
    }
    match process::fd_alloc(proc, entry) {
        Some(f) => f as i64,
        None => {
            process::pty_close(id);
            -24 // -EMFILE
        }
    }
}

fn sys_ptsname_r(fd: i32, buf_ptr: usize, buflen: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if buflen < 12 {
        return -34; // -ERANGE
    }
    if !verify_user_buffer(buf_ptr, buflen) {
        return -14; // -EFAULT
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let fde = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    let pty_id = match fde.fd_type {
        process::FdType::PtyMaster(id) => id,
        _ => return -25, // -ENOTTY
    };

    let mut name_buf = [0u8; 32];
    let prefix = b"/dev/pts/";
    name_buf[..9].copy_from_slice(prefix);
    let mut num_str = [0u8; 10];
    let mut temp = pty_id;
    let idx = if temp == 0 {
        num_str[0] = b'0';
        1
    } else {
        let mut digits = [0u8; 10];
        let mut d_idx = 0;
        while temp > 0 {
            digits[d_idx] = b'0' + (temp % 10) as u8;
            temp /= 10;
            d_idx += 1;
        }
        for i in 0..d_idx {
            num_str[i] = digits[d_idx - 1 - i];
        }
        d_idx
    };
    name_buf[9..9 + idx].copy_from_slice(&num_str[..idx]);
    let total_len = 9 + idx;
    name_buf[total_len] = 0;

    if total_len + 1 > buflen {
        return -34; // -ERANGE
    }

    if paging::copy_to_user(buf_ptr, &name_buf[..total_len + 1]).is_err() {
        return -14;
    }
    0
}

fn sys_fadvise64(fd: i32, offset: i64, len: i64, advice: i32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if offset < 0 || len < 0 {
        return -22; // -EINVAL
    }
    if advice < 0 || advice > 5 {
        return -22; // -EINVAL
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    0
}

fn sys_fallocate(fd: i32, mode: i32, offset: i64, len: i64) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if offset < 0 || len <= 0 {
        return -22; // -EINVAL
    }
    if (mode & !FALLOC_FL_KEEP_SIZE) != 0 {
        return -95; // -EOPNOTSUPP
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let fde = match process::fd_get(proc, fd as usize) {
        Some(e) => e,
        None => return -9,
    };

    match &fde.fd_type {
        process::FdType::FsFile(info) => {
            let req_size = (offset + len) as usize;
            let path_str = core::str::from_utf8(&info.path[..info.path_len]).unwrap_or("");
            {
                if let Some((_, _, _, cur_sz, _, _)) = vfs::meta_fs(path_str) {
                    if req_size > cur_sz && mode == 0 {
                        let diff = req_size - cur_sz;
                        let zeroes = alloc::vec![0u8; diff];
                        let _ = vfs::escribir_fs(path_str, cur_sz as u64, &zeroes);
                    }
                }
            }
            0
        }
        _ => -22, // -EINVAL
    }
}

fn sys_membarrier(cmd: i32, flags: u32, cpu_id: i32) -> i64 {
    let _ = (flags, cpu_id);
    if cmd == MEMBARRIER_CMD_QUERY {
        return (MEMBARRIER_CMD_GLOBAL
            | MEMBARRIER_CMD_GLOBAL_EXPEDITED
            | MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
            | MEMBARRIER_CMD_PRIVATE_EXPEDITED
            | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED) as i64;
    }
    match cmd {
        MEMBARRIER_CMD_GLOBAL
        | MEMBARRIER_CMD_GLOBAL_EXPEDITED
        | MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
        | MEMBARRIER_CMD_PRIVATE_EXPEDITED
        | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED => {
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            0
        }
        _ => -22, // -EINVAL
    }
}

// ── Syscalls 160-169: Advanced High-Performance I/O, IPC & Memory ─────────

fn sys_copy_file_range(fd_in: i32, off_in_ptr: usize, fd_out: i32, off_out_ptr: usize, len: usize) -> i64 {
    if fd_in < 0 || fd_in as usize >= process::MAX_FDS || fd_out < 0 || fd_out as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd_in as usize).is_none() || process::fd_get(proc, fd_out as usize).is_none() {
        return -9; // -EBADF
    }
    if len == 0 {
        return 0;
    }

    let mut cur_in_off: i64 = if off_in_ptr != 0 {
        if !verify_user_buffer(off_in_ptr, 8) { return -14; }
        let mut b = [0u8; 8];
        if paging::copy_from_user(&mut b, off_in_ptr, 8).is_err() { return -14; }
        let o = i64::from_ne_bytes(b);
        if o < 0 { return -22; }
        o
    } else {
        match process::fd_get(proc, fd_in as usize).map(|e| &e.fd_type) {
            Some(process::FdType::FsFile(info)) => info.pos as i64,
            _ => return -22, // -EINVAL
        }
    };

    let mut cur_out_off: i64 = if off_out_ptr != 0 {
        if !verify_user_buffer(off_out_ptr, 8) { return -14; }
        let mut b = [0u8; 8];
        if paging::copy_from_user(&mut b, off_out_ptr, 8).is_err() { return -14; }
        let o = i64::from_ne_bytes(b);
        if o < 0 { return -22; }
        o
    } else {
        match process::fd_get(proc, fd_out as usize).map(|e| &e.fd_type) {
            Some(process::FdType::FsFile(info)) => info.pos as i64,
            _ => return -22, // -EINVAL
        }
    };

    let mut total_copied = 0usize;
    let mut chunk = [0u8; 512];

    while total_copied < len {
        let to_copy = (len - total_copied).min(chunk.len());
        let read_bytes = match process::fd_get(proc, fd_in as usize).map(|e| &e.fd_type) {
            Some(process::FdType::FsFile(info)) => {
                let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                    Ok(s) => s,
                    Err(_) => return -22,
                };
                match crate::drivers::storage::vfs::leer_fs(path_str, cur_in_off as u64, &mut chunk[..to_copy]) {
                    Ok(n) => n,
                    Err(_) => break,
                }
            }
            _ => return -22,
        };

        if read_bytes == 0 {
            break;
        }

        let written_bytes = match process::fd_get(proc, fd_out as usize).map(|e| &e.fd_type) {
            Some(process::FdType::FsFile(info)) => {
                let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                    Ok(s) => s,
                    Err(_) => return -22,
                };
                match crate::drivers::storage::vfs::escribir_fs(path_str, cur_out_off as u64, &chunk[..read_bytes]) {
                    Ok(n) => n,
                    Err(_) => break,
                }
            }
            _ => return -22,
        };

        cur_in_off += written_bytes as i64;
        cur_out_off += written_bytes as i64;
        total_copied += written_bytes;

        if written_bytes < read_bytes {
            break;
        }
    }

    if off_in_ptr != 0 {
        let _ = paging::copy_to_user(off_in_ptr, &cur_in_off.to_ne_bytes());
    } else if let Some(fde) = process::fd_get_mut(proc, fd_in as usize) {
        match &mut fde.fd_type {
            process::FdType::FsFile(ref mut info) => info.pos = cur_in_off as u64,
            _ => {}
        }
    }

    if off_out_ptr != 0 {
        let _ = paging::copy_to_user(off_out_ptr, &cur_out_off.to_ne_bytes());
    }
    if let Some(fde) = process::fd_get_mut(proc, fd_out as usize) {
        match &mut fde.fd_type {
            process::FdType::FsFile(ref mut info) => {
                if off_out_ptr == 0 {
                    info.pos = cur_out_off as u64;
                }
                let path_str = core::str::from_utf8(&info.path[..info.path_len]).unwrap_or("");
                let new_sz = vfs::tam_fs(path_str).unwrap_or(0) as u64;
                info.size = new_sz;
            }
            _ => {}
        }
    }

    total_copied as i64
}

fn sys_splice(fd_in: i32, off_in_ptr: usize, fd_out: i32, off_out_ptr: usize, len: usize) -> i64 {
    let _ = (off_in_ptr, off_out_ptr);
    if fd_in < 0 || fd_in as usize >= process::MAX_FDS || fd_out < 0 || fd_out as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let in_type = match process::fd_get(proc, fd_in as usize) {
        Some(e) => e.fd_type,
        None => return -9,
    };
    let out_type = match process::fd_get(proc, fd_out as usize) {
        Some(e) => e.fd_type,
        None => return -9,
    };

    let in_is_pipe = matches!(in_type, process::FdType::PipeRead(_) | process::FdType::SocketPairEnd { .. } | process::FdType::PtyMaster(_) | process::FdType::PtySlave(_));
    let out_is_pipe = matches!(out_type, process::FdType::PipeWrite(_) | process::FdType::SocketPairEnd { .. } | process::FdType::PtyMaster(_) | process::FdType::PtySlave(_));

    if !in_is_pipe && !out_is_pipe {
        return -22; // -EINVAL (at least one must be a pipe)
    }

    let mut total = 0usize;
    let mut chunk = [0u8; 128];
    while total < len {
        let to_read = (len - total).min(128);
        let n = match in_type {
            process::FdType::PipeRead(pipe_id) => {
                let mut read_cnt = 0;
                while read_cnt < to_read {
                    match process::pipe_read_byte(pipe_id) {
                        Some(b) => { chunk[read_cnt] = b; read_cnt += 1; }
                        None => break,
                    }
                }
                read_cnt
            }
            process::FdType::SocketPairEnd { read_pipe, .. } => {
                let mut read_cnt = 0;
                while read_cnt < to_read {
                    match process::pipe_read_byte(read_pipe) {
                        Some(b) => { chunk[read_cnt] = b; read_cnt += 1; }
                        None => break,
                    }
                }
                read_cnt
            }
            process::FdType::PtyMaster(id) => {
                let ptab = process::PTY_TABLE.lock();
                let master_pipe = ptab.ptys.get(id as usize).map(|p| p.master_pipe);
                drop(ptab);
                let mut read_cnt = 0;
                if let Some(pipe_id) = master_pipe {
                    while read_cnt < to_read {
                        match process::pipe_read_byte(pipe_id) {
                            Some(b) => { chunk[read_cnt] = b; read_cnt += 1; }
                            None => break,
                        }
                    }
                }
                read_cnt
            }
            process::FdType::PtySlave(id) => {
                let ptab = process::PTY_TABLE.lock();
                let slave_pipe = ptab.ptys.get(id as usize).map(|p| p.slave_pipe);
                drop(ptab);
                let mut read_cnt = 0;
                if let Some(pipe_id) = slave_pipe {
                    while read_cnt < to_read {
                        match process::pipe_read_byte(pipe_id) {
                            Some(b) => { chunk[read_cnt] = b; read_cnt += 1; }
                            None => break,
                        }
                    }
                }
                read_cnt
            }
            process::FdType::FsFile(info) => {
                let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                    Ok(s) => s,
                    Err(_) => break,
                };
                match crate::drivers::storage::vfs::leer_fs(path_str, info.pos as u64, &mut chunk[..to_read]) {
                    Ok(n) => {
                        if let Some(fde) = process::fd_get_mut(proc, fd_in as usize) {
                            if let process::FdType::FsFile(ref mut i) = fde.fd_type {
                                i.pos += n as u64;
                            }
                        }
                        n
                    }
                    Err(_) => break,
                }
            }
            _ => break,
        };

        if n == 0 {
            break;
        }

        let written = match out_type {
            process::FdType::PipeWrite(pipe_id) => {
                let mut wr = 0;
                for i in 0..n {
                    if process::pipe_write_byte(pipe_id, chunk[i]) { wr += 1; } else { break; }
                }
                wr
            }
            process::FdType::SocketPairEnd { write_pipe, .. } => {
                let mut wr = 0;
                for i in 0..n {
                    if process::pipe_write_byte(write_pipe, chunk[i]) { wr += 1; } else { break; }
                }
                wr
            }
            process::FdType::PtyMaster(id) => {
                let ptab = process::PTY_TABLE.lock();
                let slave_pipe = ptab.ptys.get(id as usize).map(|p| p.slave_pipe);
                drop(ptab);
                let mut wr = 0;
                if let Some(pipe_id) = slave_pipe {
                    for i in 0..n {
                        if process::pipe_write_byte(pipe_id, chunk[i]) { wr += 1; } else { break; }
                    }
                }
                wr
            }
            process::FdType::PtySlave(id) => {
                let ptab = process::PTY_TABLE.lock();
                let master_pipe = ptab.ptys.get(id as usize).map(|p| p.master_pipe);
                drop(ptab);
                let mut wr = 0;
                if let Some(pipe_id) = master_pipe {
                    for i in 0..n {
                        if process::pipe_write_byte(pipe_id, chunk[i]) { wr += 1; } else { break; }
                    }
                }
                wr
            }
            process::FdType::FsFile(info) => {
                let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                    Ok(s) => s,
                    Err(_) => break,
                };
                match crate::drivers::storage::vfs::escribir_fs(path_str, info.pos as u64, &chunk[..n]) {
                    Ok(w) => {
                        if let Some(fde) = process::fd_get_mut(proc, fd_out as usize) {
                            if let process::FdType::FsFile(ref mut i) = fde.fd_type {
                                i.pos += w as u64;
                            }
                        }
                        w
                    }
                    Err(_) => break,
                }
            }
            _ => break,
        };

        total += written;
        if written < n {
            break;
        }
    }

    total as i64
}

fn sys_tee(fd_in: i32, fd_out: i32, len: usize, flags: u32) -> i64 {
    let _ = flags;
    if fd_in == fd_out {
        return -22; // -EINVAL
    }
    if fd_in < 0 || fd_in as usize >= process::MAX_FDS || fd_out < 0 || fd_out as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let in_pipe_id = match process::fd_get(proc, fd_in as usize).map(|e| e.fd_type) {
        Some(process::FdType::PipeRead(id)) => id,
        Some(process::FdType::SocketPairEnd { read_pipe, .. }) => read_pipe,
        _ => return -22, // -EINVAL
    };
    let out_pipe_id = match process::fd_get(proc, fd_out as usize).map(|e| e.fd_type) {
        Some(process::FdType::PipeWrite(id)) => id,
        Some(process::FdType::SocketPairEnd { write_pipe, .. }) => write_pipe,
        _ => return -22, // -EINVAL
    };

    let mut peek_buf = [0u8; 128];
    let to_peek = len.min(128);
    let peeked = process::pipe_peek_bytes(in_pipe_id, &mut peek_buf[..to_peek]);

    let mut written = 0usize;
    for i in 0..peeked {
        if process::pipe_write_byte(out_pipe_id, peek_buf[i]) {
            written += 1;
        } else {
            break;
        }
    }
    written as i64
}

fn sys_syncfs(fd: i32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    sys_sync()
}

fn sys_statx(dirfd: i32, path_ptr: usize, flags: i32, mask: u32, statxbuf_ptr: usize) -> i64 {
    if dirfd != AT_FDCWD && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    if !verify_user_string(path_ptr, 256) || !verify_user_buffer(statxbuf_ptr, core::mem::size_of::<Statx>()) {
        return -14; // -EFAULT
    }
    let mut path_buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut path_buf) {
        Some(s) => s,
        None => return -14,
    };
    // [T-005b] Antes `statx` ignoraba `dirfd` y `flags`: `statx(fd, "", AT_EMPTY_PATH)`
    // (File::metadata() de la std de Rust) devolvía ENOENT y rustc fallaba con
    // "failed to mmap file ... No such file or directory" sobre cada .rlib.
    let st = if path.is_empty() {
        if (flags & AT_EMPTY_PATH) == 0 || dirfd == AT_FDCWD { return -2; } // -ENOENT (como Linux)
        match fstat_interno(dirfd) { Ok(s) => s, Err(e) => return e as i64 }
    } else {
        let r = con_dirfd(dirfd, || {
            let mut abs = [0u8; 256];
            let n = resolver_con_cwd(path, &mut abs);
            let ruta = core::str::from_utf8(&abs[..n]).unwrap_or("");
            if (flags & AT_SYMLINK_NOFOLLOW) != 0 { lstat_path_internal(ruta) } else { stat_path_internal(ruta) }
        });
        match r { Ok(Ok(s)) => s, Ok(Err(e)) => return e as i64, Err(e) => return e as i64 }
    };

    let mut sx = Statx::default();
    sx.stx_mask = mask & STATX_BASIC_STATS;
    sx.stx_blksize = st.st_blksize as u32;
    sx.stx_nlink = st.st_nlink as u32;
    sx.stx_uid = st.st_uid;
    sx.stx_gid = st.st_gid;
    sx.stx_mode = st.st_mode as u16;
    sx.stx_ino = st.st_ino;
    sx.stx_size = st.st_size as u64;
    sx.stx_blocks = st.st_blocks as u64;
    sx.stx_atime.tv_sec = st.st_atime;
    sx.stx_mtime.tv_sec = st.st_mtime;
    sx.stx_ctime.tv_sec = st.st_ctime;
    sx.stx_btime.tv_sec = st.st_ctime;

    let sx_bytes = unsafe {
        core::slice::from_raw_parts(&sx as *const Statx as *const u8, core::mem::size_of::<Statx>())
    };
    if paging::copy_to_user(statxbuf_ptr, sx_bytes).is_err() {
        return -14;
    }
    0
}

fn sys_close_range(first: u32, last: u32, flags: u32) -> i64 {
    if first > last {
        return -22; // -EINVAL
    }
    // [b190] CLOSE_RANGE_UNSHARE (2) se acepta y se ignora: la tabla ya es privada salvo
    // entre hilos del mismo grupo (que es justo lo que pide: no tocar la de otros).
    if (flags & !(CLOSE_RANGE_CLOEXEC | 2)) != 0 {
        return -22; // -EINVAL
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };

    let end_slot = last.min((process::MAX_FDS - 1) as u32);
    for fd in first..=end_slot {
        if (flags & CLOSE_RANGE_CLOEXEC) != 0 {
            if let Some(fde) = process::fd_get_mut(proc, fd as usize) {
                fde.flags |= 1; // FD_CLOEXEC
            }
        } else {
            process::fd_close(proc, fd as usize);
        }
    }
    0
}

fn sys_pidfd_open(pid: i32, flags: u32) -> i64 {
    if pid <= 0 {
        return -22; // -EINVAL
    }
    if (flags & !PIDFD_NONBLOCK) != 0 {
        return -22; // -EINVAL
    }
    if process::process_by_pid(pid as u64).is_none() {
        return -3; // -ESRCH
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let mut entry = process::FdEntry::new(process::FdType::PidFd(pid as u32));
    if (flags & PIDFD_NONBLOCK) != 0 {
        entry.flags |= 0x800;
    }
    match process::fd_alloc(proc, entry) {
        Some(fd) => fd as i64,
        None => -24, // -EMFILE
    }
}

fn sys_pidfd_send_signal(pidfd: i32, sig: i32, info_ptr: usize, flags: u32) -> i64 {
    let _ = info_ptr;
    if flags != 0 {
        return -22; // -EINVAL
    }
    if sig < 0 || sig > 64 {
        return -22; // -EINVAL
    }
    if pidfd < 0 || pidfd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let fde = match process::fd_get(proc, pidfd as usize) {
        Some(e) => e,
        None => return -9,
    };
    let target_pid = match fde.fd_type {
        process::FdType::PidFd(p) => p,
        _ => return -9, // -EBADF
    };

    if sig == 0 {
        if process::process_by_pid(target_pid as u64).is_some() {
            0
        } else {
            -3 // -ESRCH
        }
    } else {
        sys_kill(target_pid as u64, sig) as i64
    }
}

fn sys_getcpu(cpu_ptr: usize, node_ptr: usize, tcache_ptr: usize) -> i64 {
    let _ = tcache_ptr;
    if cpu_ptr != 0 {
        if !verify_user_buffer(cpu_ptr, 4) { return -14; }
        // [T-025 E3c] índice lógico real de la CPU que ejecuta esta syscall.
        let cpu = crate::arch::percpu::indice_actual() as u32;
        if paging::copy_to_user(cpu_ptr, &cpu.to_ne_bytes()).is_err() { return -14; }
    }
    if node_ptr != 0 {
        if !verify_user_buffer(node_ptr, 4) { return -14; }
        if paging::copy_to_user(node_ptr, &0u32.to_ne_bytes()).is_err() { return -14; }
    }
    0
}

fn sys_mremap(old_address: usize, old_size: usize, new_size: usize, flags: i32, new_address: usize) -> i64 {
    let _ = new_address;
    if (old_address & 0xfff) != 0 || old_size == 0 || new_size == 0 {
        return -22; // -EINVAL
    }
    let proc = match process::proceso_memoria() {
        Some(p) => p,
        None => return -12, // -ENOMEM
    };
    let cr3 = proc.cr3;
    let old_pages = (old_size + PAGE_SIZE - 1) / PAGE_SIZE;
    let new_pages = (new_size + PAGE_SIZE - 1) / PAGE_SIZE;

    if new_pages == old_pages {
        return old_address as i64;
    }

    if new_pages < old_pages {
        // Shrink mapping: unmap and free excess pages
        for idx in new_pages..old_pages {
            let vaddr = old_address + idx * PAGE_SIZE;
            if let Some((paddr, flags_p)) = paging::translate(cr3, vaddr) {
                if (flags_p & paging::USER != 0) && paging::is_dynamic_addr(paddr) {
                    paging::liberar_marco_usuario(paddr);
                }
                let _ = paging::unmap_page(cr3, vaddr);
            }
        }
        return old_address as i64;
    }

    // Expanding: check if adjacent virtual address space is free
    let mut contiguous = true;
    for idx in old_pages..new_pages {
        let vaddr = old_address + idx * PAGE_SIZE;
        if paging::translate(cr3, vaddr).is_some() {
            contiguous = false;
            break;
        }
    }

    if contiguous {
        for idx in old_pages..new_pages {
            let vaddr = old_address + idx * PAGE_SIZE;
            let page_frame = match alloc_page() {
                Some(p) => p,
                None => return -12, // -ENOMEM
            };
            if paging::map_page_user(cr3, vaddr, page_frame).is_err() {
                paging::free_page(page_frame);
                return -12;
            }
            unsafe {
                core::ptr::write_bytes(vaddr as *mut u8, 0, PAGE_SIZE);
            }
        }
        return old_address as i64;
    }

    if (flags & MREMAP_MAYMOVE) == 0 {
        return -12; // -ENOMEM
    }

    // Allocate new region from proc.mmap_base
    let dest_base = proc.mmap_base;
    proc.mmap_base += new_pages * PAGE_SIZE;

    for idx in 0..new_pages {
        let vaddr = dest_base + idx * PAGE_SIZE;
        let page_frame = match alloc_page() {
            Some(p) => p,
            None => return -12,
        };
        if paging::map_page_user(cr3, vaddr, page_frame).is_err() {
            paging::free_page(page_frame);
            return -12;
        }
        unsafe {
            core::ptr::write_bytes(vaddr as *mut u8, 0, PAGE_SIZE);
        }
    }

    // Copy old content into new region
    let copy_bytes = old_size.min(new_size);
    unsafe {
        core::ptr::copy_nonoverlapping(old_address as *const u8, dest_base as *mut u8, copy_bytes);
    }

    // Unmap old region
    for idx in 0..old_pages {
        let vaddr = old_address + idx * PAGE_SIZE;
        if let Some((paddr, flags_p)) = paging::translate(cr3, vaddr) {
            if (flags_p & paging::USER != 0) && paging::is_dynamic_addr(paddr) {
                paging::liberar_marco_usuario(paddr);
            }
            let _ = paging::unmap_page(cr3, vaddr);
        }
    }

    dest_base as i64
}

// ── Syscalls 170-176: Advanced High-Performance Multiplexing & Vectored I/O ─

fn sys_ppoll(fds_ptr: usize, nfds: usize, tmo_ptr: usize, sigmask_ptr: usize) -> i64 {
    let _ = sigmask_ptr;
    let timeout_ms = if tmo_ptr != 0 {
        if !verify_user_buffer(tmo_ptr, core::mem::size_of::<Timespec>()) {
            return -14; // -EFAULT
        }
        let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
        let ts_bytes = unsafe {
            core::slice::from_raw_parts_mut(&mut ts as *mut Timespec as *mut u8, core::mem::size_of::<Timespec>())
        };
        if paging::copy_from_user(ts_bytes, tmo_ptr, ts_bytes.len()).is_err() {
            return -14;
        }
        if ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 || ts.tv_sec < 0 {
            return -22; // -EINVAL
        }
        (ts.tv_sec * 1000) + ((ts.tv_nsec + 999_999) / 1_000_000)
    } else {
        -1
    };
    sys_poll(fds_ptr, nfds, timeout_ms as i32) as i64
}

fn sys_fstatfs(fd: i32, buf_ptr: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9;
    }
    sys_fstatvfs(fd, buf_ptr)
}

fn sys_readahead(fd: i32, offset: i64, count: usize) -> i64 {
    if offset < 0 {
        return -22; // -EINVAL
    }
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9;
    }
    let _ = count;
    0
}

fn sys_epoll_pwait(epfd: i32, events_ptr: usize, maxevents: i32, timeout: i32, sigmask_ptr: usize) -> i64 {
    let _ = sigmask_ptr;
    sys_epoll_wait(epfd, events_ptr, maxevents, timeout) as i64
}

fn sys_preadv(fd: i32, iov_ptr: usize, iovcnt: i32, mut offset: i64) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if offset < 0 {
        return -22; // -EINVAL
    }
    if iovcnt < 0 || iovcnt > 1024 {
        return -22; // -EINVAL
    }
    if iovcnt == 0 {
        return 0;
    }
    let cnt = iovcnt as usize;
    if !verify_user_buffer(iov_ptr, cnt * core::mem::size_of::<IoVec>()) {
        return -14; // -EFAULT
    }
    let mut iovs = alloc::vec![IoVec::default(); cnt];
    let iov_bytes = unsafe {
        core::slice::from_raw_parts_mut(iovs.as_mut_ptr() as *mut u8, cnt * core::mem::size_of::<IoVec>())
    };
    if paging::copy_from_user(iov_bytes, iov_ptr, iov_bytes.len()).is_err() {
        return -14;
    }
    let mut total: i64 = 0;
    for iov in &iovs {
        if iov.iov_len == 0 {
            continue;
        }
        let nr = sys_pread64(fd, iov.iov_base, iov.iov_len, offset);
        if nr < 0 {
            if total > 0 {
                return total;
            }
            return nr;
        }
        total += nr;
        offset += nr;
        if (nr as usize) < iov.iov_len {
            break;
        }
    }
    total
}

fn sys_pwritev(fd: i32, iov_ptr: usize, iovcnt: i32, mut offset: i64) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if offset < 0 {
        return -22; // -EINVAL
    }
    if iovcnt < 0 || iovcnt > 1024 {
        return -22; // -EINVAL
    }
    if iovcnt == 0 {
        return 0;
    }
    let cnt = iovcnt as usize;
    if !verify_user_buffer(iov_ptr, cnt * core::mem::size_of::<IoVec>()) {
        return -14; // -EFAULT
    }
    let mut iovs = alloc::vec![IoVec::default(); cnt];
    let iov_bytes = unsafe {
        core::slice::from_raw_parts_mut(iovs.as_mut_ptr() as *mut u8, cnt * core::mem::size_of::<IoVec>())
    };
    if paging::copy_from_user(iov_bytes, iov_ptr, iov_bytes.len()).is_err() {
        return -14;
    }
    let mut total: i64 = 0;
    for iov in &iovs {
        if iov.iov_len == 0 {
            continue;
        }
        let nw = sys_pwrite64(fd, iov.iov_base, iov.iov_len, offset);
        if nw < 0 {
            if total > 0 {
                return total;
            }
            return nw;
        }
        total += nw;
        offset += nw;
        if (nw as usize) < iov.iov_len {
            break;
        }
    }
    total
}

fn sys_faccessat2(dirfd: i32, path_ptr: usize, mode: i32, flags: i32) -> i32 {
    sys_faccessat(dirfd, path_ptr, mode, flags)
}

fn sys_msync(addr: usize, len: usize, flags: i32) -> i64 {
    if (addr & 0xfff) != 0 {
        return -22; // -EINVAL
    }
    if (flags & !(MS_ASYNC | MS_INVALIDATE | MS_SYNC)) != 0 {
        return -22; // -EINVAL
    }
    if (flags & MS_ASYNC != 0) && (flags & MS_SYNC != 0) {
        return -22; // -EINVAL
    }
    if len == 0 {
        return 0;
    }
    if addr.checked_add(len).is_none() {
        return -22; // -EINVAL
    }

    let proc = match process::proceso_memoria() {
        Some(p) => p,
        None => return -3,
    };

    let targets: alloc::vec::Vec<SharedMmapEntry> = {
        let guard = SHARED_MMAPS.lock();
        guard.iter()
            .filter(|e| e.pid == proc.pid && (e.flags & 0x01 != 0) && addr < e.vaddr + e.len && addr + len > e.vaddr)
            .cloned()
            .collect()
    };

    for target in targets {
        let start = addr.max(target.vaddr);
        let end = (addr + len).min(target.vaddr + target.len);
        let sync_len = end.saturating_sub(start);
        if sync_len == 0 { continue; }

        let mut data = alloc::vec![0u8; sync_len];
        if paging::copy_from_user(&mut data, start, sync_len).is_ok() {
            let file_offset = start - target.vaddr;
            if target.is_ramfile {
                let path_str = core::str::from_utf8(&target.path[..target.path_len]).unwrap_or("");
                let _ = crate::drivers::storage::vfs::escribir_fs(path_str, file_offset as u64, &data);
            }
        }
    }

    sys_sync();
    0
}

fn sys_mincore(addr: usize, len: usize, vec_ptr: usize) -> i64 {
    if (addr & 0xfff) != 0 {
        return -22; // -EINVAL
    }
    if len == 0 {
        return 0;
    }
    let pages = (len + 4095) / 4096;
    if !verify_user_buffer(vec_ptr, pages) {
        return -14; // -EFAULT
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -14,
    };
    let mut vec_bytes = alloc::vec![0u8; pages];
    let cr3 = proc.cr3;
    for (i, item) in vec_bytes.iter_mut().enumerate().take(pages) {
        let vaddr = addr + i * 4096;
        if paging::translate(cr3, vaddr).is_some() {
            *item = 1; // page is resident in RAM
        } else {
            *item = 0;
        }
    }
    if paging::copy_to_user(vec_ptr, &vec_bytes).is_err() {
        return -14;
    }
    0
}

fn sys_madvise(addr: usize, len: usize, advice: i32) -> i64 {
    if (addr & 0xfff) != 0 {
        return -22; // -EINVAL
    }
    if advice < 0 || advice > 19 {
        return -22; // -EINVAL
    }
    if len == 0 {
        return 0;
    }
    if advice == MADV_DONTNEED {
        let proc = match process::proceso_memoria() {
            Some(p) => p,
            None => return -12,
        };
        let cr3 = proc.cr3;
        let pages = (len + 4095) / 4096;
        for i in 0..pages {
            let vaddr = addr + i * 4096;
            if i & 0xFF == 0xFF { paging::respirar(); } // [T-026]
            if let Some((paddr, flags)) = paging::translate(cr3, vaddr) {
                if flags & paging::USER != 0 {
                    // [b191 / T-014] Semántica Linux: la página privada vuelve a ser una
                    // reserva perezosa (se libera el marco; el siguiente acceso lee ceros).
                    // Páginas COW o no dinámicas: se limpian en el sitio como antes.
                    if paging::is_dynamic_addr(paddr) && flags & paging::COW_FLAG == 0 && flags & paging::WRITABLE != 0 {
                        let mut prot = 1u32 | 2;
                        if flags & paging::NO_EXECUTE == 0 { prot |= 4; }
                        paging::liberar_marco_usuario(paddr);
                        let _ = paging::unmap_page(cr3, vaddr);
                        let _ = paging::mapear_perezosa(cr3, vaddr, prot);
                    } else if flags & paging::WRITABLE != 0 {
                        unsafe { core::ptr::write_bytes(vaddr as *mut u8, 0, 4096); }
                    }
                }
            }
        }
    }
    0
}

fn sys_mlock(addr: usize, len: usize) -> i64 {
    if (addr & 0xfff) != 0 {
        return -22; // -EINVAL
    }
    if len == 0 {
        return 0;
    }
    0
}

fn sys_munlock(addr: usize, len: usize) -> i64 {
    if (addr & 0xfff) != 0 {
        return -22; // -EINVAL
    }
    if len == 0 {
        return 0;
    }
    0
}

fn sys_mlockall(flags: i32) -> i64 {
    if (flags & !(MCL_CURRENT | MCL_FUTURE | MCL_ONFAULT)) != 0 || flags == 0 {
        return -22; // -EINVAL
    }
    0
}

fn sys_munlockall() -> i64 {
    0
}

fn sys_renameat2(olddirfd: i32, oldpath_ptr: usize, newdirfd: i32, newpath_ptr: usize, flags: u32) -> i32 {
    if (flags & !(RENAME_NOREPLACE | RENAME_EXCHANGE | RENAME_WHITEOUT)) != 0 {
        return -22; // -EINVAL
    }
    if (flags & RENAME_NOREPLACE != 0) && (flags & RENAME_EXCHANGE != 0) {
        return -22; // -EINVAL
    }
    if (flags & RENAME_NOREPLACE) != 0 {
        // Verify newpath does not exist
        if !verify_user_string(newpath_ptr, 256) {
            return -14;
        }
        let mut path_buf = [0u8; 256];
        if let Some(newpath) = read_user_path(newpath_ptr, &mut path_buf) {
            if stat_path_internal(newpath).is_ok() {
                return -17; // -EEXIST
            }
        }
    }
    sys_renameat(olddirfd, oldpath_ptr, newdirfd, newpath_ptr)
}

fn sys_memfd_secret(flags: u32) -> i64 {
    if (flags & !1) != 0 {
        return -22; // -EINVAL
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    static SECRET_COUNTER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let id = SECRET_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut name = [0u8; 256];
    let prefix = b"/tmp/memfd_secret_";
    name[..prefix.len()].copy_from_slice(prefix);
    let digit = b'0' + ((id % 10) as u8);
    name[prefix.len()] = digit;
    let actual_len = prefix.len() + 1;
    let name_str = match core::str::from_utf8(&name[..actual_len]) {
        Ok(s) => s,
        Err(_) => return -22,
    };

    vfs::with_ramfs(|ram| {
        let _ = ram.create(name_str);
    });

    let info = process::FsFileInfo {
        path: name,
        path_len: actual_len,
        size: 0,
        pos: 0,
        flags: 2, // O_RDWR
    };
    let mut entry = process::FdEntry::new(process::FdType::FsFile(info));
    if (flags & 1) != 0 {
        entry.flags |= 1; // FD_CLOEXEC
    }
    match process::fd_alloc(proc, entry) {
        Some(fd) => fd as i64,
        None => -24, // -EMFILE
    }
}

fn sys_epoll_pwait2(epfd: i32, events_ptr: usize, maxevents: i32, tmo_ts_ptr: usize, sigmask_ptr: usize) -> i64 {
    let _ = sigmask_ptr;
    let timeout_ms = if tmo_ts_ptr != 0 {
        if !verify_user_buffer(tmo_ts_ptr, core::mem::size_of::<Timespec>()) {
            return -14;
        }
        let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
        let ts_bytes = unsafe {
            core::slice::from_raw_parts_mut(&mut ts as *mut Timespec as *mut u8, core::mem::size_of::<Timespec>())
        };
        if paging::copy_from_user(ts_bytes, tmo_ts_ptr, ts_bytes.len()).is_err() {
            return -14;
        }
        if ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 || ts.tv_sec < 0 {
            return -22;
        }
        (ts.tv_sec * 1000) + ((ts.tv_nsec + 999_999) / 1_000_000)
    } else {
        -1
    };
    sys_epoll_wait(epfd, events_ptr, maxevents, timeout_ms as i32) as i64
}

fn sys_process_madvise(pidfd: i32, iov_ptr: usize, vlen: usize, advice: i32, flags: u32) -> i64 {
    if flags != 0 {
        return -22; // -EINVAL
    }
    if advice < 0 || advice > 19 {
        return -22; // -EINVAL
    }
    if pidfd < 0 || pidfd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let fde = match process::fd_get(proc, pidfd as usize) {
        Some(e) => e,
        None => return -9,
    };
    let target_pid = match fde.fd_type {
        process::FdType::PidFd(p) => p,
        _ => return -9, // -EBADF
    };
    if process::process_by_pid(target_pid as u64).is_none() {
        return -3; // -ESRCH
    }
    if vlen == 0 {
        return 0;
    }
    if !verify_user_buffer(iov_ptr, vlen * core::mem::size_of::<IoVec>()) {
        return -14; // -EFAULT
    }
    let mut total_bytes: i64 = 0;
    let mut iovs = alloc::vec![IoVec::default(); vlen];
    let iov_bytes = unsafe {
        core::slice::from_raw_parts_mut(iovs.as_mut_ptr() as *mut u8, vlen * core::mem::size_of::<IoVec>())
    };
    if paging::copy_from_user(iov_bytes, iov_ptr, iov_bytes.len()).is_err() {
        return -14;
    }
    for iov in &iovs {
        total_bytes += iov.iov_len as i64;
    }
    total_bytes
}

fn sys_settimeofday(tv_ptr: usize, tz_ptr: usize) -> i32 {
    if tv_ptr == 0 && tz_ptr == 0 {
        return 0;
    }
    if tv_ptr != 0 && !verify_user_buffer(tv_ptr, 16) {
        return -14; // -EFAULT
    }
    if tz_ptr != 0 && !verify_user_buffer(tz_ptr, 8) {
        return -14; // -EFAULT
    }
    if process::current_uid() != 0 {
        return -1; // -EPERM
    }
    0
}

fn sys_sync_file_range(fd: i32, offset: i64, nbytes: i64, flags: u32) -> i32 {
    if offset < 0 || nbytes < 0 {
        return -22; // -EINVAL
    }
    if (flags & !7) != 0 {
        return -22; // -EINVAL
    }
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9;
    }
    0
}

fn sys_vmsplice(fd: i32, iov_ptr: usize, nr_segs: usize, _flags: u32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if nr_segs == 0 || nr_segs > 1024 {
        return -22; // -EINVAL
    }
    if iov_ptr == 0 || !verify_user_buffer(iov_ptr, nr_segs * 16) {
        return -14; // -EFAULT
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, fd as usize) {
        Some(e) => e.clone(),
        None => return -9,
    };
    match entry.fd_type {
        process::FdType::PipeWrite(pipe_id) => {
            let mut total = 0usize;
            for seg in 0..nr_segs {
                let mut iov_buf = [0u8; 16];
                if paging::copy_from_user(&mut iov_buf, iov_ptr + seg * 16, 16).is_err() {
                    return -14;
                }
                let base = usize::from_ne_bytes(iov_buf[0..8].try_into().unwrap_or([0; 8]));
                let len = usize::from_ne_bytes(iov_buf[8..16].try_into().unwrap_or([0; 8]));
                if len == 0 { continue; }
                if !verify_user_buffer(base, len) { return -14; }
                let mut chunk = [0u8; 128];
                let mut sub_total = 0;
                while sub_total < len {
                    let to_read = (len - sub_total).min(128);
                    if paging::copy_from_user(&mut chunk[..to_read], base + sub_total, to_read).is_err() {
                        break;
                    }
                    for i in 0..to_read {
                        if !process::pipe_write_byte(pipe_id, chunk[i]) {
                            return if total == 0 { -11 } else { total as i64 };
                        }
                        total += 1;
                        sub_total += 1;
                    }
                }
            }
            total as i64
        }
        process::FdType::PipeRead(pipe_id) => {
            let mut total = 0usize;
            for seg in 0..nr_segs {
                let mut iov_buf = [0u8; 16];
                if paging::copy_from_user(&mut iov_buf, iov_ptr + seg * 16, 16).is_err() {
                    return -14;
                }
                let base = usize::from_ne_bytes(iov_buf[0..8].try_into().unwrap_or([0; 8]));
                let len = usize::from_ne_bytes(iov_buf[8..16].try_into().unwrap_or([0; 8]));
                if len == 0 { continue; }
                if !verify_user_buffer(base, len) { return -14; }
                let mut chunk = [0u8; 128];
                let mut sub_total = 0;
                while sub_total < len {
                    let to_read = (len - sub_total).min(128);
                    let mut read_bytes = 0;
                    while read_bytes < to_read {
                        match process::pipe_read_byte(pipe_id) {
                            Some(b) => {
                                chunk[read_bytes] = b;
                                read_bytes += 1;
                            }
                            None => break,
                        }
                    }
                    if read_bytes == 0 { break; }
                    if paging::copy_to_user(base + sub_total, &chunk[..read_bytes]).is_err() {
                        return -14;
                    }
                    total += read_bytes;
                    sub_total += read_bytes;
                    if read_bytes < to_read { break; }
                }
            }
            total as i64
        }
        _ => -22, // -EINVAL (not a pipe)
    }
}

fn sys_kcmp(pid1: i64, pid2: i64, ktype: i32, idx1: u64, idx2: u64) -> i32 {
    if pid1 < 0 || pid2 < 0 || ktype < 0 || ktype > 4 {
        return -22; // -EINVAL
    }
    if process::get_process_snapshot(pid1 as u64).is_none() || process::get_process_snapshot(pid2 as u64).is_none() {
        return -3; // -ESRCH
    }
    if pid1 == pid2 {
        if ktype == KCMP_FILE {
            if idx1 == idx2 { 0 } else { 1 }
        } else {
            0 // Same process shares same VM, FILES, FS, SIGHAND
        }
    } else {
        1 // Different processes
    }
}

fn sys_personality(persona: u64) -> i32 {
    if persona == 0xFFFF_FFFF {
        0 // PER_LINUX
    } else {
        0 // Returns previous persona (PER_LINUX = 0)
    }
}

fn sys_setns(fd: i32, _nstype: i32) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9;
    }
    0
}

fn sys_unshare(flags: i32) -> i32 {
    if (flags & !0x7E02_0000) != 0 && flags != 0 {
        return -22; // -EINVAL
    }
    0
}

fn sys_pivot_root(new_root_ptr: usize, put_old_ptr: usize) -> i32 {
    if !verify_user_string(new_root_ptr, 256) || !verify_user_string(put_old_ptr, 256) {
        return -14; // -EFAULT
    }
    if process::current_uid() != 0 {
        return -1; // -EPERM
    }
    0
}

fn sys_lookup_dcookie(cookie: u64, buf_ptr: usize, len: usize) -> i32 {
    let _ = cookie;
    if len == 0 {
        return -22; // -EINVAL
    }
    if !verify_user_buffer(buf_ptr, len) {
        return -14; // -EFAULT
    }
    -2 // -ENOENT
}

fn sys_sched_setaffinity(pid: i64, cpusetsize: usize, mask_ptr: usize) -> i32 {
    if cpusetsize < 8 { return -22; }
    if !verify_user_buffer(mask_ptr, cpusetsize) { return -14; }
    if pid < 0 { return -22; }
    let mut mask_bytes = [0u8; 8];
    if paging::copy_from_user(&mut mask_bytes, mask_ptr, 8).is_err() {
        return -14;
    }
    let mask = u64::from_ne_bytes(mask_bytes);
    if mask == 0 { return -22; } // Al menos un núcleo debe estar asignado
    // [CICLO 47-b] POSIX/Linux: EINVAL si la mascara no intersecta con las CPUs
    // en linea (antes se aceptaba cualquier bit, incluso de CPUs inexistentes).
    let ncpu = crate::arch::smp::cpu_count().clamp(1, 64);
    let en_linea: u64 = if ncpu >= 64 { u64::MAX } else { (1u64 << ncpu) - 1 };
    if mask & en_linea == 0 { return -22; }
    match process::set_affinity(pid as u64, mask & en_linea) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

fn sys_sched_getaffinity(pid: i64, cpusetsize: usize, mask_ptr: usize) -> i32 {
    if cpusetsize < 8 { return -22; }
    if !verify_user_buffer(mask_ptr, cpusetsize) { return -14; }
    if pid < 0 { return -22; }
    let mask = match process::get_affinity(pid as u64) {
        // [T-025 E3c] 0 = "cualquier CPU": se presenta como la máscara de las
        // CPUs planificables reales (1 bit por núcleo de `cpu_count()`).
        Some(0) => { let n = crate::arch::smp::cpu_count().min(63); (1u64 << n) - 1 }
        Some(m) => m,
        None => return -3, // -ESRCH
    };
    if paging::copy_to_user(mask_ptr, &mask.to_ne_bytes()).is_err() {
        return -14;
    }
    8
}

fn sys_clock_settime(clock_id: i32, tp_ptr: usize) -> i32 {
    if clock_id != 0 {
        return -22; // -EINVAL (only CLOCK_REALTIME can be set)
    }
    if tp_ptr == 0 || !verify_user_buffer(tp_ptr, core::mem::size_of::<Timespec>()) {
        return -14; // -EFAULT
    }
    let mut ts_bytes = [0u8; core::mem::size_of::<Timespec>()];
    let ts_len = ts_bytes.len();
    if paging::copy_from_user(&mut ts_bytes, tp_ptr, ts_len).is_err() {
        return -14;
    }
    let ts = unsafe { *(ts_bytes.as_ptr() as *const Timespec) };
    if ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return -22; // -EINVAL
    }
    0
}

fn sys_getdents64(fd: i32, buf_ptr: usize, count: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if buf_ptr == 0 || count == 0 || !verify_user_buffer(buf_ptr, count) {
        return -14; // -EFAULT
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, fd as usize) {
        Some(e) => e.clone(),
        None => return -9,
    };

    let entries: alloc::vec::Vec<(alloc::string::String, bool)> = match entry.fd_type {
        process::FdType::ProcFile(info) => {
            match info.file_type {
                process::ProcFileType::Directory(pid) => {
                    if pid == 0 {
                        crate::drivers::storage::procfs::list_proc_dir("/proc")
                    } else {
                        crate::drivers::storage::procfs::list_proc_dir(&alloc::format!("/proc/{}", pid))
                    }
                }
                _ => return -20, // -ENOTDIR
            }
        }
        process::FdType::FsFile(info) => {
            let path_str = core::str::from_utf8(&info.path[..info.path_len]).unwrap_or("");
            let mut list = alloc::vec::Vec::new();
            list.extend(crate::drivers::storage::vfs::listar_fs(path_str));
            list
        }
        _ => return -20, // -ENOTDIR
    };

    // [CICLO 49 / T1] Cursor REAL de directorio. Antes cada llamada devolvía el
    // listado completo desde el principio y nunca 0: cualquier bucle readdir()
    // de musl/glibc (busybox `ls`) giraba para siempre reservando memoria.
    // El cursor es `pos` del descriptor (índice de la siguiente entrada);
    // `lseek(fd, 0, SEEK_SET)` lo devuelve a 0 (rewinddir). Al agotarse: 0.
    let inicio = match &entry.fd_type {
        process::FdType::FsFile(i) => i.pos as usize,
        process::FdType::ProcFile(i) => i.pos as usize,
        _ => 0,
    };
    if inicio >= entries.len() { return 0; }
    let mut total = 0usize;
    let mut emitidas = 0usize;
    for (idx, (name, is_dir)) in entries.iter().enumerate().skip(inicio) {
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len();
        // d_reclen alineado a 8 como en Linux (cabecera 19 B + nombre + NUL)
        let d_reclen = (19 + name_len + 1 + 7) & !7;
        if total + d_reclen > count {
            // Si ni la primera entrada cabe: EINVAL (como Linux), no un bucle sin fin
            if emitidas == 0 { return -22; }
            break;
        }
        let mut hdr = [0u8; 512];
        let d_type: u8 = if *is_dir { 4u8 } else { 8u8 }; // DT_DIR / DT_REG
        // d_ino nunca 0 (algunas libc descartan las entradas con inodo 0)
        let mut ino: u64 = 1469598103934665603;
        for b in name_bytes { ino ^= *b as u64; ino = ino.wrapping_mul(1099511628211); }
        hdr[0..8].copy_from_slice(&(ino | 1).to_le_bytes());
        hdr[8..16].copy_from_slice(&((idx + 1) as u64).to_le_bytes()); // d_off = siguiente cursor
        hdr[16..18].copy_from_slice(&(d_reclen as u16).to_le_bytes());
        hdr[18] = d_type;
        hdr[19..19 + name_len].copy_from_slice(name_bytes);
        if paging::copy_to_user(buf_ptr + total, &hdr[..d_reclen.min(512)]).is_err() { break; }
        total += d_reclen;
        emitidas += 1;
    }
    if let Some(e) = process::fd_get_mut(proc, fd as usize) {
        let nuevo = (inicio + emitidas) as u32;
        match &mut e.fd_type {
            process::FdType::FsFile(i) => i.pos = nuevo as u64,
            process::FdType::ProcFile(i) => i.pos = nuevo,
            _ => {}
        }
    }
    total as i64
}

fn sys_pidfd_getfd(pidfd: i32, target_fd: i32, flags: u32) -> i32 {
    if flags != 0 { return -22; } // -EINVAL
    if target_fd < 0 || target_fd as usize >= process::MAX_FDS { return -9; } // -EBADF
    if pidfd < 0 || pidfd as usize >= process::MAX_FDS { return -9; }
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let fde = match process::fd_get(cur_proc, pidfd as usize) {
        Some(e) => e,
        None => return -9,
    };
    let target_pid = match fde.fd_type {
        process::FdType::PidFd(p) => p as u64,
        _ => return -9,
    };
    let cloned_entry = match process::clone_fd_from_pid(target_pid, target_fd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match process::fd_alloc(cur_proc, cloned_entry) {
        Some(new_fd) => new_fd as i32,
        None => -24, // -EMFILE
    }
}

fn sys_openat2(dirfd: i32, path_ptr: usize, how_ptr: usize, size: usize) -> i64 {
    if size != core::mem::size_of::<OpenHow>() {
        return -22; // -EINVAL
    }
    if how_ptr == 0 || !verify_user_buffer(how_ptr, size) {
        return -14; // -EFAULT
    }
    let mut how_bytes = [0u8; core::mem::size_of::<OpenHow>()];
    if paging::copy_from_user(&mut how_bytes, how_ptr, size).is_err() {
        return -14;
    }
    let how = unsafe { *(how_bytes.as_ptr() as *const OpenHow) };
    if (how.resolve & !0x3F) != 0 {
        return -22; // -EINVAL
    }
    sys_openat(dirfd, path_ptr, how.flags as u32)
}

fn sys_process_mrelease(pidfd: i32, flags: u32) -> i32 {
    if flags != 0 { return -22; } // -EINVAL
    if pidfd < 0 || pidfd as usize >= process::MAX_FDS { return -9; } // -EBADF
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let fde = match process::fd_get(cur_proc, pidfd as usize) {
        Some(e) => e,
        None => return -9,
    };
    let target_pid = match fde.fd_type {
        process::FdType::PidFd(p) => p as u64,
        _ => return -9, // -EBADF
    };
    if let Some(snap) = process::get_process_snapshot(target_pid) {
        if snap.state != process::ProcessState::Dead && snap.state != process::ProcessState::Zombie {
            return -16; // -EBUSY
        }
    } else {
        return -3; // -ESRCH
    }
    0
}

fn sys_set_mempolicy(mode: i32, _nmask_ptr: usize, _maxnode: usize) -> i32 {
    if mode < 0 || mode > 4 {
        return -22; // -EINVAL
    }
    0
}

fn sys_get_mempolicy(mode_ptr: usize, nmask_ptr: usize, _maxnode: usize, _addr: usize, _flags: u32) -> i32 {
    if mode_ptr != 0 {
        if !verify_user_buffer(mode_ptr, 4) { return -14; }
        let mode: i32 = 0; // MPOL_DEFAULT
        if paging::copy_to_user(mode_ptr, &mode.to_ne_bytes()).is_err() {
            return -14;
        }
    }
    if nmask_ptr != 0 {
        if !verify_user_buffer(nmask_ptr, 8) { return -14; }
        let nmask: u64 = 1; // Node 0
        if paging::copy_to_user(nmask_ptr, &nmask.to_ne_bytes()).is_err() {
            return -14;
        }
    }
    0
}

fn sys_io_pgetevents(_ctx_id: usize, min_nr: i64, nr: i64, _events_ptr: usize, _timeout_ptr: usize, _sigmask_ptr: usize) -> i64 {
    if min_nr < 0 || min_nr > nr {
        return -22; // -EINVAL
    }
    0
}

fn sys_fsmount(fs_fd: i32, _flags: u32, _attr_flags: u32) -> i32 {
    if fs_fd < 0 || fs_fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(cur_proc, fs_fd as usize).is_none() {
        return -9;
    }
    -22 // -EINVAL
}

fn sys_move_mount(from_dirfd: i32, from_path: usize, to_dirfd: i32, to_path: usize, _flags: u32) -> i32 {
    if from_dirfd != AT_FDCWD && (from_dirfd < 0 || from_dirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if to_dirfd != AT_FDCWD && (to_dirfd < 0 || to_dirfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if !verify_user_string(from_path, 256) || !verify_user_string(to_path, 256) {
        return -14;
    }
    if process::current_uid() != 0 {
        return -1; // -EPERM
    }
    0
}

fn sys_capget(hdr_ptr: usize, data_ptr: usize) -> i32 {
    if hdr_ptr == 0 || !verify_user_buffer(hdr_ptr, core::mem::size_of::<CapUserHeader>()) {
        return -14; // -EFAULT
    }
    let mut hdr_bytes = [0u8; core::mem::size_of::<CapUserHeader>()];
    let len = hdr_bytes.len();
    if paging::copy_from_user(&mut hdr_bytes, hdr_ptr, len).is_err() {
        return -14;
    }
    let mut hdr = unsafe { *(hdr_bytes.as_ptr() as *const CapUserHeader) };
    if hdr.pid < 0 {
        return -22; // -EINVAL
    }
    if hdr.version != LINUX_CAPABILITY_VERSION_1
        && hdr.version != LINUX_CAPABILITY_VERSION_2
        && hdr.version != LINUX_CAPABILITY_VERSION_3
    {
        hdr.version = LINUX_CAPABILITY_VERSION_3;
        let _ = paging::copy_to_user(hdr_ptr, unsafe {
            core::slice::from_raw_parts(&hdr as *const CapUserHeader as *const u8, len)
        });
        return -22; // -EINVAL
    }
    if data_ptr != 0 {
        let dlen = core::mem::size_of::<CapUserData>();
        if !verify_user_buffer(data_ptr, dlen) {
            return -14;
        }
        let is_root = process::current_uid() == 0 || process::current_euid() == 0;
        let cap_bits = if is_root { 0xffff_ffff } else { 0x0000_0000 };
        let data = CapUserData {
            effective: cap_bits,
            permitted: cap_bits,
            inheritable: cap_bits,
        };
        let _ = paging::copy_to_user(data_ptr, unsafe {
            core::slice::from_raw_parts(&data as *const CapUserData as *const u8, dlen)
        });
    }
    0
}

fn sys_capset(hdr_ptr: usize, data_ptr: usize) -> i32 {
    if hdr_ptr == 0 || !verify_user_buffer(hdr_ptr, core::mem::size_of::<CapUserHeader>()) {
        return -14;
    }
    if data_ptr == 0 || !verify_user_buffer(data_ptr, core::mem::size_of::<CapUserData>()) {
        return -14;
    }
    let mut hdr_bytes = [0u8; core::mem::size_of::<CapUserHeader>()];
    let len = hdr_bytes.len();
    if paging::copy_from_user(&mut hdr_bytes, hdr_ptr, len).is_err() {
        return -14;
    }
    let hdr = unsafe { *(hdr_bytes.as_ptr() as *const CapUserHeader) };
    if hdr.pid < 0 {
        return -22;
    }
    if hdr.version != LINUX_CAPABILITY_VERSION_1
        && hdr.version != LINUX_CAPABILITY_VERSION_2
        && hdr.version != LINUX_CAPABILITY_VERSION_3
    {
        return -22;
    }
    0
}

fn sys_sched_getattr(pid: i32, attr_ptr: usize, size: u32, flags: u32) -> i32 {
    if pid < 0 || flags != 0 {
        return -22;
    }
    let expected_size = core::mem::size_of::<SchedAttr>() as u32;
    if size < expected_size {
        return -22;
    }
    if attr_ptr == 0 || !verify_user_buffer(attr_ptr, expected_size as usize) {
        return -14;
    }
    let attr = SchedAttr {
        size: expected_size,
        sched_policy: 0, // SCHED_OTHER
        sched_flags: 0,
        sched_nice: 0,
        sched_priority: 0,
        sched_runtime: 0,
        sched_deadline: 0,
        sched_period: 0,
    };
    let _ = paging::copy_to_user(attr_ptr, unsafe {
        core::slice::from_raw_parts(&attr as *const SchedAttr as *const u8, expected_size as usize)
    });
    0
}

fn sys_sched_setattr(pid: i32, attr_ptr: usize, flags: u32) -> i32 {
    if pid < 0 || flags != 0 {
        return -22;
    }
    let expected_size = core::mem::size_of::<SchedAttr>();
    if attr_ptr == 0 || !verify_user_buffer(attr_ptr, expected_size) {
        return -14;
    }
    let mut attr_bytes = [0u8; core::mem::size_of::<SchedAttr>()];
    if paging::copy_from_user(&mut attr_bytes, attr_ptr, expected_size).is_err() {
        return -14;
    }
    let attr = unsafe { *(attr_bytes.as_ptr() as *const SchedAttr) };
    if attr.sched_policy > 6 {
        return -22;
    }
    if attr.sched_nice < -20 || attr.sched_nice > 19 {
        return -22;
    }
    0
}

fn sys_fsopen(fsname_ptr: usize, flags: u32) -> i32 {
    if fsname_ptr == 0 || !verify_user_string(fsname_ptr, 64) {
        return -14;
    }
    if flags & !FSOPEN_CLOEXEC != 0 {
        return -22;
    }
    let mut buf = [0u8; 64];
    let path_len = match paging::copy_from_user(&mut buf, fsname_ptr, 64) {
        Ok(n) => n,
        Err(()) => return -14,
    };
    let actual_len = buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let name = core::str::from_utf8(&buf[..actual_len]).unwrap_or("");
    if name != "fat32" && name != "ramfs" && name != "procfs" && name != "devfs" && name != "ext2" {
        return -19; // -ENODEV
    }
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    match process::fd_alloc(cur_proc, FdEntry::new(FdType::PipeRead(0))) {
        Some(fd) => fd as i32,
        None => -24, // -EMFILE
    }
}

fn sys_fspick(dfd: i32, path_ptr: usize, flags: u32) -> i32 {
    if dfd != AT_FDCWD && (dfd < 0 || dfd as usize >= process::MAX_FDS) {
        return -9;
    }
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14;
    }
    let allowed = FSPICK_CLOEXEC | FSPICK_SYMLINK_NOFOLLOW | FSPICK_NO_AUTOMOUNT | FSPICK_EMPTY_PATH;
    if flags & !allowed != 0 {
        return -22;
    }
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_cstr_from_user(&mut path_buf, path_ptr) {
        Ok(n) => n,
        Err(()) => return -14,
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let raw_rel = alloc::string::String::from(core::str::from_utf8(&path_buf[..actual_len]).unwrap_or(""));
    let actual_len = if raw_rel.is_empty() { 0 } else { resolver_con_cwd(&raw_rel, &mut path_buf) };
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");
    if path != "/" && path != "/proc" && path != "/tmp" && path != "/etc" && path != "/bin" {
        return -2; // -ENOENT
    }
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    match process::fd_alloc(cur_proc, FdEntry::new(FdType::PipeRead(0))) {
        Some(fd) => fd as i32,
        None => -24,
    }
}

fn sys_fsconfig(fs_fd: i32, cmd: u32, key_ptr: usize, _val_ptr: usize, _aux: i32) -> i32 {
    if fs_fd < 0 || fs_fd as usize >= process::MAX_FDS {
        return -9;
    }
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(cur_proc, fs_fd as usize).is_none() {
        return -9;
    }
    if cmd > 7 {
        return -22;
    }
    if key_ptr != 0 && !verify_user_string(key_ptr, 128) {
        return -14;
    }
    0
}

fn sys_quotactl(_cmd: i32, special_ptr: usize, id: i32, _addr_ptr: usize) -> i32 {
    if special_ptr == 0 || !verify_user_string(special_ptr, 256) {
        return -14;
    }
    if id < 0 {
        return -22;
    }
    0
}

fn sys_landlock_create_ruleset(attr_ptr: usize, size: usize, flags: u32) -> i32 {
    if flags != 0 {
        return -22;
    }
    let min_size = core::mem::size_of::<LandlockRulesetAttr>();
    if size < min_size {
        return -22;
    }
    if attr_ptr == 0 || !verify_user_buffer(attr_ptr, min_size) {
        return -14;
    }
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -3,
    };
    match process::fd_alloc(cur_proc, FdEntry::new(FdType::PipeRead(0))) {
        Some(fd) => fd as i32,
        None => -24,
    }
}

fn sys_landlock_add_rule(ruleset_fd: i32, rule_type: u32, rule_attr_ptr: usize, flags: u32) -> i32 {
    if ruleset_fd < 0 || ruleset_fd as usize >= process::MAX_FDS {
        return -9;
    }
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(cur_proc, ruleset_fd as usize).is_none() {
        return -9;
    }
    if rule_type != LANDLOCK_RULE_PATH_BENEATH && rule_type != LANDLOCK_RULE_NET_PORT {
        return -22;
    }
    if flags != 0 {
        return -22;
    }
    if rule_attr_ptr == 0 || !verify_user_buffer(rule_attr_ptr, 16) {
        return -14;
    }
    0
}

fn sys_landlock_restrict_self(ruleset_fd: i32, flags: u32) -> i32 {
    if ruleset_fd < 0 || ruleset_fd as usize >= process::MAX_FDS {
        return -9;
    }
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(cur_proc, ruleset_fd as usize).is_none() {
        return -9;
    }
    if flags != 0 {
        return -22;
    }
    0
}

fn sys_seccomp(operation: u32, flags: u32, args_ptr: usize) -> i32 {
    if operation > 3 {
        return -22;
    }
    if operation == SECCOMP_SET_MODE_STRICT {
        if flags != 0 || args_ptr != 0 {
            return -22;
        }
        return 0;
    }
    if operation == SECCOMP_GET_ACTION_AVAIL {
        if args_ptr == 0 || !verify_user_buffer(args_ptr, 4) {
            return -14;
        }
        return 0;
    }
    0
}

fn sys_statmount(req_ptr: usize, buf_ptr: usize, bufsize: usize, flags: u64) -> i64 {
    if req_ptr == 0 || !verify_user_buffer(req_ptr, core::mem::size_of::<MntIdReq>()) {
        return -14;
    }
    let sm_size = core::mem::size_of::<Statmount>();
    if buf_ptr == 0 || !verify_user_buffer(buf_ptr, bufsize) {
        return -14;
    }
    if bufsize < sm_size {
        return -22;
    }
    if flags != 0 {
        return -22;
    }
    let sm = Statmount {
        size: sm_size as u32,
        mnt_id: 1,
        mnt_parent_id: 0,
        fs_type: 0x4d44, // FAT32 / RamFS
        mnt_root: 0,
        mnt_point: 0,
        mask: 0x7f,
    };
    let _ = paging::copy_to_user(buf_ptr, unsafe {
        core::slice::from_raw_parts(&sm as *const Statmount as *const u8, sm_size)
    });
    0
}

fn sys_listmount(_req_ptr: usize, mnt_ids_ptr: usize, nr_mnt_ids: usize, flags: u64) -> i64 {
    if mnt_ids_ptr == 0 || nr_mnt_ids == 0 {
        return -14;
    }
    if !verify_user_buffer(mnt_ids_ptr, nr_mnt_ids * 8) {
        return -14;
    }
    if flags != 0 {
        return -22;
    }
    let mounts = [1u64, 2u64];
    let to_copy = core::cmp::min(mounts.len(), nr_mnt_ids);
    let _ = paging::copy_to_user(mnt_ids_ptr, unsafe {
        core::slice::from_raw_parts(mounts.as_ptr() as *const u8, to_copy * 8)
    });
    to_copy as i64
}

// ── Sockets & System V IPC Subsystems (Syscalls 225-240) ────────────────────

fn sys_shutdown(sockfd: i32, how: i32) -> i32 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    if how < 0 || how > 2 {
        return -22; // -EINVAL
    }
    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(cur_proc, sockfd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match entry.fd_type {
        FdType::Socket(id) => match crate::net::socket::socket_shutdown(id, how) {
            Ok(()) => 0,
            Err(e) => e,
        },
        FdType::SocketPairEnd { .. } => 0,
        _ => -88, // -ENOTSOCK
    }
}

fn sys_getsockname(sockfd: i32, addr_ptr: usize, addrlen_ptr: usize) -> i32 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS {
        return -9;
    }
    if addr_ptr == 0 || addrlen_ptr == 0 {
        return -14;
    }
    if !verify_user_buffer(addrlen_ptr, 4) {
        return -14;
    }
    let mut ulen_bytes = [0u8; 4];
    if paging::copy_from_user(&mut ulen_bytes, addrlen_ptr, 4).is_err() {
        return -14;
    }
    let ulen = u32::from_ne_bytes(ulen_bytes) as usize;
    if ulen < 16 {
        return -22;
    }
    if !verify_user_buffer(addr_ptr, 16) {
        return -14;
    }

    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(cur_proc, sockfd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match entry.fd_type {
        FdType::Socket(id) => {
            let port = crate::net::socket::socket_get_local(id).unwrap_or(0);
            let mut sa = [0u8; 16];
            sa[0] = 2; sa[1] = 0; // AF_INET
            sa[2..4].copy_from_slice(&port.to_be_bytes());
            let _ = paging::copy_to_user(addr_ptr, &sa);
            let out_len: u32 = 16;
            let _ = paging::copy_to_user(addrlen_ptr, &out_len.to_ne_bytes());
            0
        }
        FdType::SocketPairEnd { .. } => {
            let mut sa = [0u8; 16];
            sa[0] = 1; sa[1] = 0; // AF_UNIX
            let _ = paging::copy_to_user(addr_ptr, &sa);
            let out_len: u32 = 16;
            let _ = paging::copy_to_user(addrlen_ptr, &out_len.to_ne_bytes());
            0
        }
        _ => -88, // -ENOTSOCK
    }
}

fn sys_getpeername(sockfd: i32, addr_ptr: usize, addrlen_ptr: usize) -> i32 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS {
        return -9;
    }
    if addr_ptr == 0 || addrlen_ptr == 0 {
        return -14;
    }
    if !verify_user_buffer(addrlen_ptr, 4) {
        return -14;
    }
    let mut ulen_bytes = [0u8; 4];
    if paging::copy_from_user(&mut ulen_bytes, addrlen_ptr, 4).is_err() {
        return -14;
    }
    let ulen = u32::from_ne_bytes(ulen_bytes) as usize;
    if ulen < 16 {
        return -22;
    }
    if !verify_user_buffer(addr_ptr, 16) {
        return -14;
    }

    let cur_proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(cur_proc, sockfd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match entry.fd_type {
        FdType::Socket(id) => match crate::net::socket::socket_get_peer(id) {
            Some((ip, port)) => {
                let mut sa = [0u8; 16];
                sa[0] = 2; sa[1] = 0; // AF_INET
                sa[2..4].copy_from_slice(&port.to_be_bytes());
                sa[4..8].copy_from_slice(&ip);
                let _ = paging::copy_to_user(addr_ptr, &sa);
                let out_len: u32 = 16;
                let _ = paging::copy_to_user(addrlen_ptr, &out_len.to_ne_bytes());
                0
            }
            None => -107, // -ENOTCONN
        },
        FdType::SocketPairEnd { .. } => {
            let mut sa = [0u8; 16];
            sa[0] = 1; sa[1] = 0; // AF_UNIX
            let _ = paging::copy_to_user(addr_ptr, &sa);
            let out_len: u32 = 16;
            let _ = paging::copy_to_user(addrlen_ptr, &out_len.to_ne_bytes());
            0
        }
        _ => -88, // -ENOTSOCK
    }
}

// [CICLO 48] Descriptores en transito por SCM_RIGHTS: cola FIFO por tuberia
// de destino (la de lectura del receptor). Cada entrada ya lleva su
// referencia tomada (fd_clone_entry); si nadie la recoge se libera.
static SCM_COLA: Spinlock<alloc::vec::Vec<(u32, u64, process::FdEntry)>> = Spinlock::new(alloc::vec::Vec::new());
const SCM_MAX_FDS: usize = 16;

fn leer_msghdr(msg_ptr: usize) -> Result<MsgHdr, i64> {
    let hdr_size = core::mem::size_of::<MsgHdr>();
    if msg_ptr == 0 || !verify_user_buffer(msg_ptr, hdr_size) { return Err(-14); }
    let mut hdr_bytes = [0u8; core::mem::size_of::<MsgHdr>()];
    let len = hdr_bytes.len();
    if paging::copy_from_user(&mut hdr_bytes, msg_ptr, len).is_err() { return Err(-14); }
    let hdr = unsafe { *(hdr_bytes.as_ptr() as *const MsgHdr) };
    if hdr.msg_iovlen > 1024 { return Err(-22); }
    if hdr.msg_iovlen > 0 && (hdr.msg_iov == 0 || !verify_user_buffer(hdr.msg_iov, hdr.msg_iovlen * 16)) { return Err(-14); }
    Ok(hdr)
}

fn sys_sendmsg(sockfd: i32, msg_ptr: usize, _flags: i32) -> i64 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS { return -9; }
    let hdr = match leer_msghdr(msg_ptr) { Ok(h) => h, Err(e) => return e };
    let cur_proc = match process::current_process() { Some(p) => p, None => return -9 };
    let entry = match process::fd_get(cur_proc, sockfd as usize) { Some(e) => *e, None => return -9 };
    let destino = match entry.fd_type {
        FdType::SocketPairEnd { write_pipe, .. } => Some(write_pipe),
        FdType::Socket(_) => None,
        _ => return -88, // -ENOTSOCK
    };
    // 1) Datos auxiliares: SCM_RIGHTS duplica los descriptores AHORA (el
    //    emisor puede cerrarlos despues sin que el receptor los pierda).
    let mut clonados: alloc::vec::Vec<process::FdEntry> = alloc::vec::Vec::new();
    if hdr.msg_control != 0 && hdr.msg_controllen >= 16 {
        if !verify_user_buffer(hdr.msg_control, hdr.msg_controllen) { return -14; }
        let mut off = 0usize;
        while off + 16 <= hdr.msg_controllen {
            let mut ch = [0u8; 16];
            if paging::copy_from_user(&mut ch, hdr.msg_control + off, 16).is_err() { return -14; }
            let cmsg_len = u64::from_ne_bytes([ch[0],ch[1],ch[2],ch[3],ch[4],ch[5],ch[6],ch[7]]) as usize;
            let level = i32::from_ne_bytes([ch[8],ch[9],ch[10],ch[11]]);
            let tipo = i32::from_ne_bytes([ch[12],ch[13],ch[14],ch[15]]);
            if cmsg_len < 16 || off + cmsg_len > hdr.msg_controllen { return -22; }
            if level == 1 && tipo == 1 { // SOL_SOCKET / SCM_RIGHTS
                if destino.is_none() { return -22; }
                let n = (cmsg_len - 16) / 4;
                if n + clonados.len() > SCM_MAX_FDS { return -90; } // -EMSGSIZE
                for i in 0..n {
                    let mut fb = [0u8; 4];
                    if paging::copy_from_user(&mut fb, hdr.msg_control + off + 16 + i * 4, 4).is_err() { return -14; }
                    let fd = i32::from_ne_bytes(fb);
                    if fd < 0 || fd as usize >= process::MAX_FDS { for c in clonados { process::fd_entry_release(c); } return -9; }
                    match process::fd_get(cur_proc, fd as usize) {
                        Some(e) => { let mut c = process::fd_clone_entry(e); c.flags &= !1; clonados.push(c); }
                        None => { for c in clonados { process::fd_entry_release(c); } return -9; }
                    }
                }
            }
            off += (cmsg_len + 7) & !7;
        }
    }
    // 2) Datos: cada iovec se escribe por la ruta normal del descriptor.
    let mut total = 0i64;
    for i in 0..hdr.msg_iovlen {
        let mut iov_bytes = [0u8; 16];
        if paging::copy_from_user(&mut iov_bytes, hdr.msg_iov + i * 16, 16).is_err() { break; }
        let iov = unsafe { *(iov_bytes.as_ptr() as *const IoVec) };
        if iov.iov_len == 0 { continue; }
        let r = sys_write(sockfd, iov.iov_base, iov.iov_len);
        if r < 0 { if total == 0 { for c in clonados { process::fd_entry_release(c); } return r; } break; }
        total += r;
        if (r as usize) < iov.iov_len { break; }
    }
    if let Some(p) = destino {
        // Anclados al desplazamiento del primer byte de ESTE mensaje.
        let ancla = process::pipe_totales(p).0 - total as u64;
        let mut cola = SCM_COLA.lock();
        for c in clonados { cola.push((p, ancla, c)); }
    }
    total
}

fn sys_recvmsg(sockfd: i32, msg_ptr: usize, _flags: i32) -> i64 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS { return -9; }
    let hdr = match leer_msghdr(msg_ptr) { Ok(h) => h, Err(e) => return e };
    let cur_proc = match process::current_process() { Some(p) => p, None => return -9 };
    let entry = match process::fd_get(cur_proc, sockfd as usize) { Some(e) => *e, None => return -9 };
    let origen = match entry.fd_type {
        FdType::SocketPairEnd { read_pipe, .. } => Some(read_pipe),
        FdType::Socket(_) => None,
        _ => return -88,
    };
    // 1) Datos por la ruta normal de lectura (bloqueante segun el descriptor).
    let desde = origen.map(|p| process::pipe_totales(p).1).unwrap_or(0);
    let mut total = 0i64;
    for i in 0..hdr.msg_iovlen {
        let mut iov_bytes = [0u8; 16];
        if paging::copy_from_user(&mut iov_bytes, hdr.msg_iov + i * 16, 16).is_err() { break; }
        let iov = unsafe { *(iov_bytes.as_ptr() as *const IoVec) };
        if iov.iov_len == 0 { continue; }
        let (r, _) = sys_read(sockfd, iov.iov_base, iov.iov_len, 0);
        if r < 0 { if total == 0 { return r; } break; }
        total += r;
        if (r as usize) < iov.iov_len { break; }
    }
    // 2) Descriptores pendientes para esta tuberia -> nuevos fds del receptor.
    let mut msg_flags: i32 = 0;
    let mut controllen_usado = 0usize;
    if let Some(p) = origen {
        let recibidos: alloc::vec::Vec<process::FdEntry> = {
            let mut cola = SCM_COLA.lock();
            let mut v = alloc::vec::Vec::new();
            let mut i = 0;
            // Solo los fds anclados dentro del tramo [desde, hasta) leido ahora.
            let hasta = desde + total as u64;
            while i < cola.len() {
                let (pp, ancla, _) = cola[i];
                if pp == p && ancla >= desde && (ancla < hasta || hasta == desde) { v.push(cola.remove(i).2); } else { i += 1; }
            }
            v
        };
        if !recibidos.is_empty() {
            let necesario = 16 + recibidos.len() * 4;
            let cabe = hdr.msg_control != 0 && hdr.msg_controllen >= 16 && verify_user_buffer(hdr.msg_control, hdr.msg_controllen);
            let max_fds = if cabe { ((hdr.msg_controllen - 16) / 4).min(recibidos.len()) } else { 0 };
            if max_fds < recibidos.len() { msg_flags |= 0x8; } // MSG_CTRUNC
            let cloexec = (_flags & 0x4000_0000) != 0; // MSG_CMSG_CLOEXEC
            let mut nuevos: alloc::vec::Vec<i32> = alloc::vec::Vec::new();
            for (idx, e) in recibidos.into_iter().enumerate() {
                if idx < max_fds {
                    let mut e2 = e; if cloexec { e2.flags |= 1; }
                    match process::fd_alloc(cur_proc, e2) {
                        Some(fd) => nuevos.push(fd as i32),
                        None => { process::fd_entry_release(e2); msg_flags |= 0x8; }
                    }
                } else {
                    process::fd_entry_release(e);
                }
            }
            if cabe && !nuevos.is_empty() {
                let cmsg_len = (16 + nuevos.len() * 4) as u64;
                let mut ch = [0u8; 16];
                ch[0..8].copy_from_slice(&cmsg_len.to_ne_bytes());
                ch[8..12].copy_from_slice(&1i32.to_ne_bytes());
                ch[12..16].copy_from_slice(&1i32.to_ne_bytes());
                if paging::copy_to_user(hdr.msg_control, &ch).is_err() { return -14; }
                for (i, fd) in nuevos.iter().enumerate() {
                    if paging::copy_to_user(hdr.msg_control + 16 + i * 4, &fd.to_ne_bytes()).is_err() { return -14; }
                }
                controllen_usado = ((cmsg_len as usize) + 7) & !7;
                let _ = necesario;
            }
        }
    }
    // 3) Actualizar msg_controllen y msg_flags en el msghdr del usuario.
    let off_controllen = 40; let off_flags = 48;
    let _ = paging::copy_to_user(msg_ptr + off_controllen, &(controllen_usado as u64).to_ne_bytes());
    let _ = paging::copy_to_user(msg_ptr + off_flags, &msg_flags.to_ne_bytes());
    total
}

// ── System V IPC Tables ─────────────────────────────────────────────────────

#[derive(Copy, Clone)]
struct ShmSegment {
    key: i32,
    size: usize,
    in_use: bool,
    nattch: u64,
    creator_pid: u64,
}

static SHM_TABLE: Spinlock<[ShmSegment; 16]> = Spinlock::new([
    ShmSegment { key: 0, size: 0, in_use: false, nattch: 0, creator_pid: 0 }; 16
]);

#[derive(Copy, Clone)]
struct SemSet {
    key: i32,
    nsems: usize,
    values: [i16; 16],
    in_use: bool,
}

static SEM_TABLE: Spinlock<[SemSet; 16]> = Spinlock::new([
    SemSet { key: 0, nsems: 0, values: [0; 16], in_use: false }; 16
]);

#[derive(Copy, Clone)]
struct MsgItem {
    mtype: i64,
    len: usize,
    data: [u8; 128],
}

#[derive(Copy, Clone)]
struct MsgQueue {
    key: i32,
    messages: [MsgItem; 4],
    count: usize,
    in_use: bool,
}

static MSG_TABLE: Spinlock<[MsgQueue; 16]> = Spinlock::new([
    MsgQueue {
        key: 0,
        messages: [MsgItem { mtype: 0, len: 0, data: [0; 128] }; 4],
        count: 0,
        in_use: false,
    }; 16
]);

fn sys_shmget(key: i32, size: usize, _shmflg: i32) -> i32 {
    if size == 0 || size > 16 * 1024 * 1024 || key < 0 {
        return -22;
    }
    let mut table = SHM_TABLE.lock();
    if key != 0 {
        for (i, seg) in table.iter().enumerate() {
            if seg.in_use && seg.key == key {
                return i as i32;
            }
        }
    }
    for (i, seg) in table.iter_mut().enumerate() {
        if !seg.in_use {
            seg.in_use = true;
            seg.key = key;
            seg.size = size;
            seg.nattch = 0;
            seg.creator_pid = process::current_pid();
            return i as i32;
        }
    }
    -24 // -EMFILE
}

fn sys_shmat(shmid: i32, shmaddr: usize, _shmflg: i32) -> usize {
    if shmid < 0 || shmid >= 16 {
        return (-22i64) as usize;
    }
    let mut table = SHM_TABLE.lock();
    if !table[shmid as usize].in_use {
        return (-22i64) as usize;
    }
    if shmaddr != 0 && shmaddr % 4096 != 0 {
        return (-22i64) as usize;
    }
    let attach_addr = if shmaddr != 0 {
        shmaddr
    } else {
        0x4000_0000_0000 + (shmid as usize) * 0x1000_0000
    };
    table[shmid as usize].nattch += 1;
    attach_addr
}

fn sys_shmdt(shmaddr: usize) -> i32 {
    if shmaddr == 0 || shmaddr % 4096 != 0 {
        return -22;
    }
    let mut table = SHM_TABLE.lock();
    for seg in table.iter_mut() {
        if seg.in_use && seg.nattch > 0 {
            seg.nattch -= 1;
            return 0;
        }
    }
    -22
}

fn sys_shmctl(shmid: i32, cmd: i32, buf_ptr: usize) -> i32 {
    if shmid < 0 || shmid >= 16 {
        return -22;
    }
    let mut table = SHM_TABLE.lock();
    if !table[shmid as usize].in_use {
        return -22;
    }
    match cmd {
        IPC_STAT => {
            let ds_size = core::mem::size_of::<ShmidDs>();
            if buf_ptr == 0 || !verify_user_buffer(buf_ptr, ds_size) {
                return -14;
            }
            let ds = ShmidDs {
                shm_segsz: table[shmid as usize].size,
                shm_atime: 0,
                shm_dtime: 0,
                shm_ctime: 0,
                shm_cpid: table[shmid as usize].creator_pid as u32,
                shm_lpid: 0,
                shm_nattch: table[shmid as usize].nattch,
            };
            let _ = paging::copy_to_user(buf_ptr, unsafe {
                core::slice::from_raw_parts(&ds as *const ShmidDs as *const u8, ds_size)
            });
            0
        }
        IPC_RMID => {
            table[shmid as usize].in_use = false;
            0
        }
        _ => -22,
    }
}

fn sys_semget(key: i32, nsems: i32, _semflg: i32) -> i32 {
    if nsems < 0 || nsems > 16 || key < 0 {
        return -22;
    }
    let mut table = SEM_TABLE.lock();
    if key != 0 {
        for (i, sem) in table.iter().enumerate() {
            if sem.in_use && sem.key == key {
                return i as i32;
            }
        }
    }
    for (i, sem) in table.iter_mut().enumerate() {
        if !sem.in_use {
            sem.in_use = true;
            sem.key = key;
            sem.nsems = nsems as usize;
            sem.values = [0; 16];
            return i as i32;
        }
    }
    -24
}

fn sys_semop(semid: i32, sops_ptr: usize, nsops: usize) -> i32 {
    if semid < 0 || semid >= 16 || nsops == 0 || nsops > 16 {
        return -22;
    }
    let sop_size = core::mem::size_of::<Sembuf>();
    if sops_ptr == 0 || !verify_user_buffer(sops_ptr, nsops * sop_size) {
        return -14;
    }
    let mut table = SEM_TABLE.lock();
    if !table[semid as usize].in_use {
        return -22;
    }
    for i in 0..nsops {
        let mut sop_bytes = [0u8; core::mem::size_of::<Sembuf>()];
        let len = sop_bytes.len();
        if paging::copy_from_user(&mut sop_bytes, sops_ptr + i * sop_size, len).is_err() {
            return -14;
        }
        let sop = unsafe { *(sop_bytes.as_ptr() as *const Sembuf) };
        if (sop.sem_num as usize) >= table[semid as usize].nsems {
            return -22;
        }
        table[semid as usize].values[sop.sem_num as usize] = table[semid as usize].values[sop.sem_num as usize].saturating_add(sop.sem_op);
    }
    0
}

fn sys_semctl(semid: i32, semnum: i32, cmd: i32, arg: usize) -> i32 {
    if semid < 0 || semid >= 16 {
        return -22;
    }
    let mut table = SEM_TABLE.lock();
    if !table[semid as usize].in_use {
        return -22;
    }
    match cmd {
        SEM_GETVAL => {
            if semnum < 0 || semnum as usize >= table[semid as usize].nsems {
                return -22;
            }
            table[semid as usize].values[semnum as usize] as i32
        }
        SEM_SETVAL => {
            if semnum < 0 || semnum as usize >= table[semid as usize].nsems {
                return -22;
            }
            table[semid as usize].values[semnum as usize] = arg as i16;
            0
        }
        IPC_RMID => {
            table[semid as usize].in_use = false;
            0
        }
        _ => -22,
    }
}

fn sys_msgget(key: i32, _msgflg: i32) -> i32 {
    if key < 0 {
        return -22;
    }
    let mut table = MSG_TABLE.lock();
    if key != 0 {
        for (i, q) in table.iter().enumerate() {
            if q.in_use && q.key == key {
                return i as i32;
            }
        }
    }
    for (i, q) in table.iter_mut().enumerate() {
        if !q.in_use {
            q.in_use = true;
            q.key = key;
            q.count = 0;
            return i as i32;
        }
    }
    -24
}

fn sys_msgsnd(msqid: i32, msgp_ptr: usize, msgsz: usize, _msgflg: i32) -> i32 {
    if msqid < 0 || msqid >= 16 || msgsz > 128 {
        return -22;
    }
    if msgp_ptr == 0 || !verify_user_buffer(msgp_ptr, 8 + msgsz) {
        return -14;
    }
    let mut table = MSG_TABLE.lock();
    if !table[msqid as usize].in_use {
        return -22;
    }
    if table[msqid as usize].count >= 4 {
        return -11; // -EAGAIN
    }
    let mut mtype_bytes = [0u8; 8];
    if paging::copy_from_user(&mut mtype_bytes, msgp_ptr, 8).is_err() {
        return -14;
    }
    let mtype = i64::from_ne_bytes(mtype_bytes);
    let mut data = [0u8; 128];
    if msgsz > 0 && paging::copy_from_user(&mut data[..msgsz], msgp_ptr + 8, msgsz).is_err() {
        return -14;
    }
    let idx = table[msqid as usize].count;
    table[msqid as usize].messages[idx] = MsgItem { mtype, len: msgsz, data };
    table[msqid as usize].count += 1;
    0
}

fn sys_msgrcv(msqid: i32, msgp_ptr: usize, msgsz: usize, _msgtyp: i64, _msgflg: i32) -> i64 {
    if msqid < 0 || msqid >= 16 {
        return -22;
    }
    if msgp_ptr == 0 || !verify_user_buffer(msgp_ptr, 8 + msgsz) {
        return -14;
    }
    let mut table = MSG_TABLE.lock();
    if !table[msqid as usize].in_use {
        return -22;
    }
    if table[msqid as usize].count == 0 {
        return -11; // -EAGAIN
    }
    let msg = table[msqid as usize].messages[0];
    let to_copy = msg.len.min(msgsz);
    if paging::copy_to_user(msgp_ptr, &msg.mtype.to_ne_bytes()).is_err() {
        return -14;
    }
    if to_copy > 0 && paging::copy_to_user(msgp_ptr + 8, &msg.data[..to_copy]).is_err() {
        return -14;
    }
    // Shift messages
    for i in 0..table[msqid as usize].count - 1 {
        table[msqid as usize].messages[i] = table[msqid as usize].messages[i + 1];
    }
    table[msqid as usize].count -= 1;
    to_copy as i64
}

fn sys_msgctl(msqid: i32, cmd: i32, buf_ptr: usize) -> i32 {
    if msqid < 0 || msqid >= 16 {
        return -22;
    }
    let mut table = MSG_TABLE.lock();
    if !table[msqid as usize].in_use {
        return -22;
    }
    match cmd {
        IPC_STAT => {
            let ds_size = core::mem::size_of::<MsqidDs>();
            if buf_ptr == 0 || !verify_user_buffer(buf_ptr, ds_size) {
                return -14;
            }
            let ds = MsqidDs {
                msg_stime: 0,
                msg_rtime: 0,
                msg_ctime: 0,
                msg_qnum: table[msqid as usize].count as u64,
                msg_qbytes: 1024,
                msg_lspid: 0,
                msg_lrpid: 0,
            };
            let _ = paging::copy_to_user(buf_ptr, unsafe {
                core::slice::from_raw_parts(&ds as *const MsqidDs as *const u8, ds_size)
            });
            0
        }
        IPC_RMID => {
            table[msqid as usize].in_use = false;
            table[msqid as usize].count = 0;
            0
        }
        _ => -22,
    }
}

// ── POSIX Timers Table (Syscalls 241-245) ───────────────────────────────────

#[derive(Copy, Clone)]
struct PosixTimer {
    clockid: i32,
    interval: ITimerSpec,
    in_use: bool,
}

static POSIX_TIMERS: Spinlock<[PosixTimer; 16]> = Spinlock::new([
    PosixTimer {
        clockid: 0,
        interval: ITimerSpec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        },
        in_use: false,
    }; 16
]);

fn sys_timer_create(clockid: i32, sevp_ptr: usize, timerid_ptr: usize) -> i32 {
    if clockid != 0 && clockid != 1 {
        return -22; // -EINVAL
    }
    if timerid_ptr == 0 || !verify_user_buffer(timerid_ptr, 4) {
        return -14; // -EFAULT
    }
    if sevp_ptr != 0 && !verify_user_buffer(sevp_ptr, 32) {
        return -14;
    }
    let mut table = POSIX_TIMERS.lock();
    for (i, t) in table.iter_mut().enumerate() {
        if !t.in_use {
            t.in_use = true;
            t.clockid = clockid;
            let id = i as i32;
            let _ = paging::copy_to_user(timerid_ptr, &id.to_ne_bytes());
            return 0;
        }
    }
    -11 // -EAGAIN
}

fn sys_timer_settime(timerid: i32, _flags: i32, new_val_ptr: usize, old_val_ptr: usize) -> i32 {
    if timerid < 0 || timerid >= 16 {
        return -22;
    }
    let spec_size = core::mem::size_of::<ITimerSpec>();
    if new_val_ptr == 0 || !verify_user_buffer(new_val_ptr, spec_size) {
        return -14;
    }
    let mut table = POSIX_TIMERS.lock();
    if !table[timerid as usize].in_use {
        return -22;
    }
    if old_val_ptr != 0 {
        if !verify_user_buffer(old_val_ptr, spec_size) {
            return -14;
        }
        let _ = paging::copy_to_user(old_val_ptr, unsafe {
            core::slice::from_raw_parts(&table[timerid as usize].interval as *const ITimerSpec as *const u8, spec_size)
        });
    }
    let mut spec_bytes = [0u8; core::mem::size_of::<ITimerSpec>()];
    let len = spec_bytes.len();
    if paging::copy_from_user(&mut spec_bytes, new_val_ptr, len).is_err() {
        return -14;
    }
    let new_spec = unsafe { *(spec_bytes.as_ptr() as *const ITimerSpec) };
    table[timerid as usize].interval = new_spec;
    0
}

fn sys_timer_gettime(timerid: i32, curr_val_ptr: usize) -> i32 {
    if timerid < 0 || timerid >= 16 {
        return -22;
    }
    let spec_size = core::mem::size_of::<ITimerSpec>();
    if curr_val_ptr == 0 || !verify_user_buffer(curr_val_ptr, spec_size) {
        return -14;
    }
    let table = POSIX_TIMERS.lock();
    if !table[timerid as usize].in_use {
        return -22;
    }
    let _ = paging::copy_to_user(curr_val_ptr, unsafe {
        core::slice::from_raw_parts(&table[timerid as usize].interval as *const ITimerSpec as *const u8, spec_size)
    });
    0
}

fn sys_timer_getoverrun(timerid: i32) -> i32 {
    if timerid < 0 || timerid >= 16 {
        return -22;
    }
    let table = POSIX_TIMERS.lock();
    if !table[timerid as usize].in_use {
        return -22;
    }
    0
}

fn sys_timer_delete(timerid: i32) -> i32 {
    if timerid < 0 || timerid >= 16 {
        return -22;
    }
    let mut table = POSIX_TIMERS.lock();
    if !table[timerid as usize].in_use {
        return -22;
    }
    table[timerid as usize].in_use = false;
    0
}

// ── Vectorized Futex, Memory Page Locking & Scheduling (Syscalls 246-255) ────

fn path_exists_check(path: &str) -> bool {
    if path == "/swapfile" {
        return true;
    }
    stat_path_internal(path).is_ok()
}

fn sys_futex_waitv(waiters_ptr: usize, nr_futexes: u32, flags: u32, timeout_ptr: usize, _clockid: i32) -> i32 {
    if flags != 0 {
        return -22; // -EINVAL
    }
    if nr_futexes == 0 || nr_futexes > 128 {
        return -22; // -EINVAL
    }
    if waiters_ptr == 0 || !verify_user_buffer(waiters_ptr, nr_futexes as usize * 32) {
        return -14; // -EFAULT
    }
    if timeout_ptr != 0 {
        if !verify_user_buffer(timeout_ptr, 16) {
            return -14; // -EFAULT
        }
        let mut ts_buf = [0u8; 16];
        if paging::copy_from_user(&mut ts_buf, timeout_ptr, 16).is_err() {
            return -14;
        }
        let tv_sec = i64::from_ne_bytes(ts_buf[0..8].try_into().unwrap_or_default());
        let tv_nsec = i64::from_ne_bytes(ts_buf[8..16].try_into().unwrap_or_default());
        if tv_nsec < 0 || tv_nsec >= 1_000_000_000 {
            return -22; // -EINVAL
        }
        if tv_sec == 0 && tv_nsec == 0 {
            return -11; // -EAGAIN
        }
    }
    0
}

fn sys_mlock2(start: usize, len: usize, flags: i32) -> i32 {
    if start == 0 || (start & 0xfff) != 0 || len == 0 {
        return -22; // -EINVAL
    }
    if (flags & !1) != 0 {
        return -22; // -EINVAL
    }
    0
}

fn sys_ioprio_get(which: i32, who: i32) -> i32 {
    if which < 1 || which > 3 || who < 0 {
        return -22; // -EINVAL
    }
    // (IOPRIO_CLASS_BE << 13) | 4 = (2 << 13) | 4 = 16388
    16388
}

fn sys_ioprio_set(which: i32, who: i32, ioprio: i32) -> i32 {
    if which < 1 || which > 3 || who < 0 || ioprio < 0 || ioprio > 0x7fff {
        return -22; // -EINVAL
    }
    0
}

fn sys_sched_rr_get_interval(pid: i32, interval_ptr: usize) -> i32 {
    if pid < 0 {
        return -22; // -EINVAL
    }
    if pid > 0 && process::process_by_pid(pid as u64).is_none() {
        return -3; // -ESRCH
    }
    if interval_ptr == 0 || !verify_user_buffer(interval_ptr, 16) {
        return -14; // -EFAULT
    }
    let ts = [0i64, 10_000_000i64]; // 10ms quantum
    let bytes = unsafe { core::slice::from_raw_parts(ts.as_ptr() as *const u8, 16) };
    if paging::copy_to_user(interval_ptr, bytes).is_err() {
        return -14;
    }
    0
}

fn sys_get_robust_list(pid: i32, head_ptr_ptr: usize, len_ptr: usize) -> i32 {
    if pid < 0 {
        return -22; // -EINVAL
    }
    let (head, len) = if pid == 0 {
        match process::current_process() {
            Some(p) => (p.robust_list_head, p.robust_list_len),
            None => return -3,
        }
    } else {
        match process::process_by_pid(pid as u64) {
            Some(p) => (p.robust_list_head, p.robust_list_len),
            None => return -3, // -ESRCH
        }
    };
    if head_ptr_ptr == 0 || !verify_user_buffer(head_ptr_ptr, 8) ||
       len_ptr == 0 || !verify_user_buffer(len_ptr, 8) {
        return -14; // -EFAULT
    }
    let _ = paging::copy_to_user(head_ptr_ptr, &head.to_ne_bytes());
    let _ = paging::copy_to_user(len_ptr, &len.to_ne_bytes());
    0
}

fn sys_set_robust_list(head_ptr: usize, len: usize) -> i32 {
    if len != 24 {
        return -22; // -EINVAL
    }
    if head_ptr != 0 && !verify_user_buffer(head_ptr, 24) {
        return -14; // -EFAULT
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    proc.robust_list_head = head_ptr;
    proc.robust_list_len = len;
    0
}

fn sys_restart_syscall() -> i32 {
    0
}

fn sys_kexec_load(_entry: usize, nr_segments: usize, segments_ptr: usize, flags: usize) -> i32 {
    if nr_segments > 16 || flags != 0 {
        return -22; // -EINVAL
    }
    if nr_segments == 0 {
        return 0;
    }
    if segments_ptr == 0 || !verify_user_buffer(segments_ptr, nr_segments * 32) {
        return -14; // -EFAULT
    }
    0
}

fn sys_kexec_file_load(kernel_fd: i32, _initrd_fd: i32, cmdline_len: usize, cmdline_ptr: usize, _flags: usize) -> i32 {
    if kernel_fd < 0 || kernel_fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, kernel_fd as usize).is_none() {
        return -9; // -EBADF
    }
    if cmdline_len > 0 && (cmdline_ptr == 0 || !verify_user_buffer(cmdline_ptr, cmdline_len)) {
        return -14; // -EFAULT
    }
    0
}

fn sys_pkey_mprotect(start: usize, len: usize, _prot: i32, pkey: i32) -> i32 {
    if (start & 0xfff) != 0 || len == 0 || pkey < 0 || pkey > 15 {
        return -22; // -EINVAL
    }
    0
}

static PKEY_ALLOCATED: crate::arch::Spinlock<[bool; 16]> = crate::arch::Spinlock::new([false; 16]);

fn sys_pkey_alloc(flags: u32, access_rights: u32) -> i32 {
    if flags != 0 || access_rights > 3 {
        return -22; // -EINVAL
    }
    let mut keys = PKEY_ALLOCATED.lock();
    for i in 1..16 {
        if !keys[i] {
            keys[i] = true;
            return i as i32;
        }
    }
    -28 // -ENOSPC
}

fn sys_pkey_free(pkey: i32) -> i32 {
    if pkey <= 0 || pkey > 15 {
        return -22; // -EINVAL
    }
    let mut keys = PKEY_ALLOCATED.lock();
    if !keys[pkey as usize] {
        return -22; // -EINVAL: unallocated key
    }
    keys[pkey as usize] = false;
    0
}

fn sys_rseq(rseq_ptr: usize, rseq_len: u32, flags: i32, _sig: u32) -> i32 {
    if rseq_len != 32 || flags != 0 {
        return -22; // -EINVAL
    }
    if rseq_ptr == 0 || !verify_user_buffer(rseq_ptr, 32) {
        return -14; // -EFAULT
    }
    0
}

fn sys_io_uring_setup(entries: u32, params_ptr: usize) -> i32 {
    if entries == 0 || entries > 4096 {
        return -22; // -EINVAL
    }
    if params_ptr == 0 || !verify_user_buffer(params_ptr, 120) {
        return -14; // -EFAULT
    }
    let sq_entries = 32u32;
    let cq_entries = 64u32;
    let _ = paging::copy_to_user(params_ptr, &sq_entries.to_ne_bytes());
    let _ = paging::copy_to_user(params_ptr + 4, &cq_entries.to_ne_bytes());

    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    match process::fd_alloc(proc, process::FdEntry::new(process::FdType::Device(process::DeviceInfo { dev_type: process::DeviceType::Null, pos: 0 }))) {
        Some(fd) => fd as i32,
        None => -24, // -EMFILE
    }
}

fn sys_io_uring_enter(fd: i32, to_submit: u32, _min_complete: u32, flags: u32, _sig_ptr: usize) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    if (flags & !0x0f) != 0 {
        return -22; // -EINVAL
    }
    to_submit as i32
}

fn sys_io_uring_register(fd: i32, opcode: u32, arg_ptr: usize, nr_args: u32) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    if opcode > 32 {
        return -22; // -EINVAL
    }
    if nr_args > 0 && (arg_ptr == 0 || !verify_user_buffer(arg_ptr, nr_args as usize * 8)) {
        return -14; // -EFAULT
    }
    0
}

fn sys_preadv2(fd: i32, iov_ptr: usize, iovcnt: i32, offset: i64, flags: i32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    if (flags & !0x1f) != 0 {
        return -22; // -EINVAL
    }
    sys_preadv(fd, iov_ptr, iovcnt, offset)
}

fn sys_pwritev2(fd: i32, iov_ptr: usize, iovcnt: i32, offset: i64, flags: i32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    if (flags & !0x1f) != 0 {
        return -22; // -EINVAL
    }
    sys_pwritev(fd, iov_ptr, iovcnt, offset)
}

fn sys_setdomainname(name_ptr: usize, len: usize) -> i32 {
    if len == 0 || len > 64 {
        return -22; // -EINVAL
    }
    if name_ptr == 0 || !verify_user_buffer(name_ptr, len) {
        return -14; // -EFAULT
    }
    0
}

fn sys_swapon(path_ptr: usize, flags: i32) -> i32 {
    if flags < 0 {
        return -22; // -EINVAL
    }
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path) {
        return -2; // -ENOENT
    }
    0
}

fn sys_swapoff(path_ptr: usize) -> i32 {
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path) {
        return -2; // -ENOENT
    }
    0
}

fn sys_ioperm(from: usize, num: usize, turn_on: i32) -> i32 {
    if turn_on != 0 && turn_on != 1 {
        return -22; // -EINVAL
    }
    if from + num > 0x10000 {
        return -22; // -EINVAL
    }
    0
}

fn sys_iopl(level: i32) -> i32 {
    if level < 0 || level > 3 {
        return -22; // -EINVAL
    }
    0
}

fn sys_userfaultfd(flags: i32) -> i32 {
    if (flags & !(0x80000 | 0x800 | 0x01)) != 0 {
        return -22; // -EINVAL
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    match process::fd_alloc(proc, process::FdEntry::new(process::FdType::Device(process::DeviceInfo { dev_type: process::DeviceType::Null, pos: 0 }))) {
        Some(fd) => fd as i32,
        None => -24, // -EMFILE
    }
}

fn sys_mq_open(name_ptr: usize, oflag: i32, _mode: u32, _attr_ptr: usize) -> i32 {
    if name_ptr == 0 || !verify_user_string(name_ptr, 64) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let name = match read_user_path_raw(name_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !name.starts_with('/') || name == "/" || name.len() > 63 {
        return -22; // -EINVAL
    }

    let mut table = process::POSIX_MQ_TABLE.lock();
    let mut found_idx: Option<usize> = None;
    for (i, q) in table.queues.iter().enumerate() {
        if q.in_use && q.name_len == name.len() && &q.name[..name.len()] == name.as_bytes() {
            found_idx = Some(i);
            break;
        }
    }

    let is_create = (oflag & 0x40) != 0; // O_CREAT
    let is_excl = (oflag & 0x80) != 0;   // O_EXCL

    let target_idx = match found_idx {
        Some(_) if is_create && is_excl => return -17, // -EEXIST
        Some(idx) => idx,
        None => {
            if !is_create {
                return -2; // -ENOENT
            }
            let mut free_slot = None;
            for (i, q) in table.queues.iter().enumerate() {
                if !q.in_use {
                    free_slot = Some(i);
                    break;
                }
            }
            let slot = match free_slot {
                Some(s) => s,
                None => return -24, // -EMFILE
            };
            let q = &mut table.queues[slot];
            q.in_use = true;
            q.ref_count = 1;
            q.name_len = name.len();
            q.name[..name.len()].copy_from_slice(name.as_bytes());
            q.flags = oflag as i64;
            q.maxmsg = 10;
            q.msgsize = 128;
            q.curmsgs = 0;
            slot
        }
    };

    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    match process::fd_alloc(proc, process::FdEntry::new(process::FdType::Mq(target_idx as u32))) {
        Some(fd) => fd as i32,
        None => -24,
    }
}

fn sys_mq_unlink(name_ptr: usize) -> i32 {
    if name_ptr == 0 || !verify_user_string(name_ptr, 64) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let name = match read_user_path_raw(name_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    let mut table = process::POSIX_MQ_TABLE.lock();
    for q in table.queues.iter_mut() {
        if q.in_use && q.name_len == name.len() && &q.name[..name.len()] == name.as_bytes() {
            q.in_use = false;
            return 0;
        }
    }
    -2 // -ENOENT
}

fn sys_mq_timedsend(mqdes: i32, msg_ptr: usize, msg_len: usize, msg_prio: u32, _abs_timeout_ptr: usize) -> i32 {
    if mqdes < 0 || mqdes as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let q_id = match process::fd_get(proc, mqdes as usize) {
        Some(e) => match e.fd_type {
            process::FdType::Mq(id) => id,
            _ => return -9,
        },
        None => return -9,
    };
    let mut table = process::POSIX_MQ_TABLE.lock();
    let q = match table.queues.get_mut(q_id as usize) {
        Some(q) if q.in_use => q,
        _ => return -9,
    };
    if msg_len > q.msgsize as usize {
        return -90; // -EMSGSIZE
    }
    if msg_ptr == 0 || !verify_user_buffer(msg_ptr, msg_len) {
        return -14; // -EFAULT
    }
    if q.curmsgs >= q.maxmsg as usize {
        return -11; // -EAGAIN
    }

    let mut msg_buf = [0u8; 256];
    if paging::copy_from_user(&mut msg_buf[..msg_len], msg_ptr, msg_len).is_err() {
        return -14;
    }

    let slot = q.curmsgs;
    q.messages[slot] = process::MqMessage {
        len: msg_len,
        prio: msg_prio,
        data: msg_buf,
    };
    q.curmsgs += 1;

    for i in (1..q.curmsgs).rev() {
        if q.messages[i].prio > q.messages[i - 1].prio {
            q.messages.swap(i, i - 1);
        }
    }
    0
}

fn sys_mq_timedreceive(mqdes: i32, msg_ptr: usize, msg_len: usize, msg_prio_ptr: usize, _abs_timeout_ptr: usize) -> i32 {
    if mqdes < 0 || mqdes as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let q_id = match process::fd_get(proc, mqdes as usize) {
        Some(e) => match e.fd_type {
            process::FdType::Mq(id) => id,
            _ => return -9,
        },
        None => return -9,
    };
    let mut table = process::POSIX_MQ_TABLE.lock();
    let q = match table.queues.get_mut(q_id as usize) {
        Some(q) if q.in_use => q,
        _ => return -9,
    };
    if msg_len < q.msgsize as usize {
        return -90; // -EMSGSIZE
    }
    if msg_ptr == 0 || !verify_user_buffer(msg_ptr, msg_len) {
        return -14; // -EFAULT
    }
    if q.curmsgs == 0 {
        return -11; // -EAGAIN
    }

    let msg = q.messages[0];
    for i in 1..q.curmsgs {
        q.messages[i - 1] = q.messages[i];
    }
    q.curmsgs -= 1;

    if paging::copy_to_user(msg_ptr, &msg.data[..msg.len]).is_err() {
        return -14;
    }
    if msg_prio_ptr != 0 && verify_user_buffer(msg_prio_ptr, 4) {
        let _ = paging::copy_to_user(msg_prio_ptr, &msg.prio.to_ne_bytes());
    }
    msg.len as i32
}

fn sys_mq_notify(mqdes: i32, _sevp_ptr: usize) -> i32 {
    if mqdes < 0 || mqdes as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    match process::fd_get(proc, mqdes as usize) {
        Some(e) => match e.fd_type {
            process::FdType::Mq(_) => 0,
            _ => -9,
        },
        None => -9,
    }
}

fn sys_mq_getsetattr(mqdes: i32, newattr_ptr: usize, oldattr_ptr: usize) -> i32 {
    if mqdes < 0 || mqdes as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let q_id = match process::fd_get(proc, mqdes as usize) {
        Some(e) => match e.fd_type {
            process::FdType::Mq(id) => id,
            _ => return -9,
        },
        None => return -9,
    };
    let mut table = process::POSIX_MQ_TABLE.lock();
    let q = match table.queues.get_mut(q_id as usize) {
        Some(q) if q.in_use => q,
        _ => return -9,
    };

    if oldattr_ptr != 0 {
        if !verify_user_buffer(oldattr_ptr, 64) {
            return -14; // -EFAULT
        }
        let old = [q.flags as i64, q.maxmsg as i64, q.msgsize as i64, q.curmsgs as i64, 0i64, 0i64, 0i64, 0i64];
        let bytes = unsafe { core::slice::from_raw_parts(old.as_ptr() as *const u8, 64) };
        if paging::copy_to_user(oldattr_ptr, bytes).is_err() {
            return -14;
        }
    }
    if newattr_ptr != 0 {
        if !verify_user_buffer(newattr_ptr, 64) {
            return -14; // -EFAULT
        }
        let mut nbuf = [0u8; 64];
        if paging::copy_from_user(&mut nbuf, newattr_ptr, 64).is_err() {
            return -14;
        }
        let new_flags = i64::from_ne_bytes(nbuf[0..8].try_into().unwrap_or_default());
        q.flags = new_flags;
    }
    0
}

static THREAD_AREA_DESC: crate::arch::Spinlock<[u8; 16]> = crate::arch::Spinlock::new([0u8; 16]);

fn sys_set_thread_area(u_info_ptr: usize) -> i32 {
    if u_info_ptr == 0 || !verify_user_buffer(u_info_ptr, 16) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 16];
    if paging::copy_from_user(&mut buf, u_info_ptr, 16).is_err() {
        return -14;
    }
    let entry_num = i32::from_ne_bytes(buf[0..4].try_into().unwrap_or_default());
    let assigned = if entry_num == -1 { 12i32 } else { entry_num };
    buf[0..4].copy_from_slice(&assigned.to_ne_bytes());
    let _ = paging::copy_to_user(u_info_ptr, &buf);

    let mut stored = THREAD_AREA_DESC.lock();
    stored.copy_from_slice(&buf);
    0
}

fn sys_get_thread_area(u_info_ptr: usize) -> i32 {
    if u_info_ptr == 0 || !verify_user_buffer(u_info_ptr, 16) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 16];
    if paging::copy_from_user(&mut buf, u_info_ptr, 16).is_err() {
        return -14;
    }
    let entry_number = i32::from_ne_bytes(buf[0..4].try_into().unwrap_or_default());
    if entry_number < 12 || entry_number > 14 {
        return -22; // -EINVAL
    }
    let stored = THREAD_AREA_DESC.lock();
    let mut out = *stored;
    out[0..4].copy_from_slice(&entry_number.to_ne_bytes());
    // base_addr at offset 4..12, limit at offset 12..16
    let base: u32 = 0x20000000;
    let limit: u32 = 0xFFFFF;
    out[4..8].copy_from_slice(&base.to_ne_bytes());
    out[8..12].copy_from_slice(&limit.to_ne_bytes());
    let _ = paging::copy_to_user(u_info_ptr, &out);
    0
}

fn sys_set_tid_address(tidptr: usize) -> i32 {
    if tidptr != 0 && !verify_user_buffer(tidptr, 4) {
        return -14; // -EFAULT
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return 1,
    };
    proc.clear_child_tid = tidptr as u64; // [T-005] se limpia y despierta al morir el hilo
    proc.pid as i32
}

fn sys_fanotify_init(flags: u32, _event_f_flags: u32) -> i32 {
    if (flags & !0x0f) != 0 {
        return -22; // -EINVAL
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    match process::fd_alloc(proc, process::FdEntry::new(process::FdType::Fanotify(0))) {
        Some(fd) => fd as i32,
        None => -24,
    }
}

fn sys_fanotify_mark(fanotify_fd: i32, flags: u32, mask: u64, dfd: i32, pathname_ptr: usize) -> i32 {
    if (flags & !0x3f) != 0 || mask == 0 {
        return -22; // -EINVAL
    }
    if fanotify_fd < 0 || fanotify_fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fanotify_fd as usize).is_none() {
        return -9; // -EBADF
    }
    if dfd != -100 && (dfd < 0 || dfd as usize >= process::MAX_FDS || process::fd_get(proc, dfd as usize).is_none()) {
        return -9; // -EBADF
    }
    if pathname_ptr != 0 {
        let mut buf = [0u8; 256];
        let path = match read_user_path(pathname_ptr, &mut buf) {
            Some(s) => s,
            None => return -14,
        };
        if !path_exists_check(path) {
            return -2; // -ENOENT
        }
    }
    0
}

fn sys_name_to_handle_at(dfd: i32, name_ptr: usize, handle_ptr: usize, _mnt_id_ptr: usize, flags: i32) -> i32 {
    if dfd != -100 && (dfd < 0 || dfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    if (flags & !0x1000) != 0 {
        return -22; // -EINVAL
    }
    if name_ptr == 0 || handle_ptr == 0 || !verify_user_string(name_ptr, 256) || !verify_user_buffer(handle_ptr, 24) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path = match read_user_path(name_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path) {
        return -2; // -ENOENT
    }
    let handle_bytes: u32 = 16;
    let handle_type: i32 = 1;
    let _ = paging::copy_to_user(handle_ptr, &handle_bytes.to_ne_bytes());
    let _ = paging::copy_to_user(handle_ptr + 4, &handle_type.to_ne_bytes());
    0
}

fn sys_open_by_handle_at(mountdirfd: i32, handle_ptr: usize, _flags: i32) -> i32 {
    if mountdirfd != -100 && (mountdirfd < 0 || mountdirfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    if handle_ptr == 0 || !verify_user_buffer(handle_ptr, 8) {
        return -14; // -EFAULT
    }
    let mut hbuf = [0u8; 8];
    if paging::copy_from_user(&mut hbuf, handle_ptr, 8).is_err() {
        return -14;
    }
    let handle_bytes = u32::from_ne_bytes(hbuf[0..4].try_into().unwrap_or_default());
    if handle_bytes < 8 {
        return -22; // -EINVAL
    }
    do_open_path("/etc/hostname", 0) as i32
}

fn sys_finit_module(fd: i32, _uargs_ptr: usize, flags: i32) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    if (flags & !0x07) != 0 {
        return -22; // -EINVAL
    }
    -8 // -ENOEXEC
}

fn sys_remap_file_pages(start: usize, size: usize, _prot: u32, _pgoff: usize, flags: u32) -> i32 {
    if (start & 0xfff) != 0 || size == 0 || flags != 0 {
        return -22; // -EINVAL
    }
    if start < 0x2000_0000 || start >= 0x0000_8000_0000_0000 {
        return -12; // -ENOMEM
    }
    0
}

fn sys_sendfile(out_fd: i32, in_fd: i32, offset_ptr: usize, count: usize) -> i64 {
    if count == 0 {
        return 0;
    }
    if out_fd < 0 || in_fd < 0 || out_fd as usize >= process::MAX_FDS || in_fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, out_fd as usize).is_none() || process::fd_get(proc, in_fd as usize).is_none() {
        return -9; // -EBADF
    }
    if offset_ptr != 0 && !verify_user_buffer(offset_ptr, 8) {
        return -14; // -EFAULT
    }

    let cur_in_off: i64 = if offset_ptr != 0 {
        let mut b = [0u8; 8];
        if paging::copy_from_user(&mut b, offset_ptr, 8).is_err() {
            return -14;
        }
        let o = i64::from_ne_bytes(b);
        if o < 0 { return -22; }
        o
    } else {
        match process::fd_get(proc, in_fd as usize).map(|e| e.fd_type) {
            Some(process::FdType::FsFile(info)) => info.pos as i64,
            _ => 0,
        }
    };

    let mut total_transferred = 0usize;
    let mut chunk = [0u8; 512];

    while total_transferred < count {
        let to_read = (count - total_transferred).min(chunk.len());
        let read_bytes = match process::fd_get(proc, in_fd as usize).map(|e| e.fd_type) {
            Some(process::FdType::FsFile(info)) => {
                let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                    Ok(s) => s,
                    Err(_) => break,
                };
                match crate::drivers::storage::vfs::leer_fs(path_str, (cur_in_off as u64) + (total_transferred as u64), &mut chunk[..to_read]) {
                    Ok(n) => n,
                    Err(_) => break,
                }
            }
            Some(process::FdType::PipeRead(pipe_id)) => {
                let mut read_cnt = 0;
                while read_cnt < to_read {
                    match process::pipe_read_byte(pipe_id) {
                        Some(b) => { chunk[read_cnt] = b; read_cnt += 1; }
                        None => break,
                    }
                }
                read_cnt
            }
            // [T-004] busybox `head -c N /dev/zero` usa sendfile(2): /dev/zero y /dev/null como entrada.
            Some(process::FdType::Device(info)) if info.dev_type == process::DeviceType::Zero => {
                for b in chunk[..to_read].iter_mut() { *b = 0; }
                to_read
            }
            Some(process::FdType::Device(info)) if info.dev_type == process::DeviceType::Null => 0,
            _ => break,
        };

        if read_bytes == 0 {
            break;
        }

        let written = match process::fd_get(proc, out_fd as usize).map(|e| e.fd_type) {
            Some(process::FdType::PipeWrite(pipe_id)) => {
                let mut wr = 0;
                let no_bloqueante = process::fd_get(proc, out_fd as usize).map(|e| (e.flags & 0x800) != 0).unwrap_or(false);
                for i in 0..read_bytes {
                    if process::pipe_write_byte(pipe_id, chunk[i]) {
                        wr += 1;
                    } else {
                        // [T-004] tubería llena: bloquear como write(2) (tope 10 s)
                        if no_bloqueante { break; }
                        let tope = crate::time::pit::ticks().wrapping_add(TOPE_BLOQUEO_TICKS);
                        while !process::pipe_has_space(pipe_id) && process::pipe_has_readers(pipe_id)
                            && crate::time::pit::ticks() < tope && crate::process::signals::current_sig_pending() == 0
                        {
                            unsafe { crate::process::scheduler::wait_hlt_preemptible(); }
                        }
                        if process::pipe_write_byte(pipe_id, chunk[i]) { wr += 1; } else { break; }
                    }
                }
                wr
            }
            Some(process::FdType::FsFile(info)) => {
                let path_str = match core::str::from_utf8(&info.path[..info.path_len]) {
                    Ok(s) => s,
                    Err(_) => break,
                };
                match crate::drivers::storage::vfs::escribir_fs(path_str, info.pos as u64, &chunk[..read_bytes]) {
                    Ok(n) => {
                        if let Some(fde) = process::fd_get_mut(proc, out_fd as usize) {
                            if let process::FdType::FsFile(ref mut i) = fde.fd_type {
                                i.pos += n as u64;
                            }
                        }
                        n
                    }
                    Err(_) => break,
                }
            }
            Some(process::FdType::Stdout) | Some(process::FdType::Stderr) => {
                for i in 0..read_bytes {
                    serial::write_byte(chunk[i]);
                }
                read_bytes
            }
            _ => break,
        };

        total_transferred += written;
        if written < read_bytes {
            break;
        }
    }

    if offset_ptr != 0 {
        let new_off = cur_in_off + (total_transferred as i64);
        let _ = paging::copy_to_user(offset_ptr, &new_off.to_ne_bytes());
    } else {
        if let Some(fde) = process::fd_get_mut(proc, in_fd as usize) {
            match fde.fd_type {
                process::FdType::FsFile(ref mut info) => {
                    info.pos = (cur_in_off + (total_transferred as i64)) as u64;
                }
                _ => {}
            }
        }
    }

    total_transferred as i64
}

fn sys_connect(sockfd: i32, addr_ptr: usize, addrlen: u32) -> i32 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, sockfd as usize) {
        Some(e) => e,
        None => return -9,
    };
    if addr_ptr == 0 || !verify_user_buffer(addr_ptr, addrlen as usize) {
        return -14; // -EFAULT
    }
    // [CICLO 47-c] AF_UNIX con nombre: la conexion se materializa como un par
    // de tuberias y el descriptor pasa a ser un extremo `SocketPairEnd`.
    if let process::FdType::Socket(id) = entry.fd_type {
        if crate::net::socket::socket_domain(id) == 1 {
            let ruta = match leer_sockaddr_un(addr_ptr, addrlen) { Some(r) => r, None => return -22 };
            let (rd, wr) = match crate::net::socket::unix_connect(&ruta) { Ok(x) => x, Err(e) => return e };
            let flags = entry.flags;
            process::fd_close(proc, sockfd as usize);
            let mut nuevo = process::FdEntry::new(process::FdType::SocketPairEnd { read_pipe: rd, write_pipe: wr });
            nuevo.flags = flags;
            let _ = process::fd_alloc_at(proc, sockfd as usize, nuevo);
            return 0;
        }
    }
    if addrlen < 16 {
        return -22; // -EINVAL
    }
    let mut saddr = [0u8; 16];
    if paging::copy_from_user(&mut saddr, addr_ptr, 16).is_err() {
        return -14; // -EFAULT
    }
    let family = (saddr[0] as u16) | ((saddr[1] as u16) << 8);
    let port = ((saddr[2] as u16) << 8) | (saddr[3] as u16);
    // [IPv6 REAL] AF_INET (2): sockaddr_in; AF_INET6 (10): sockaddr_in6 (28 bytes, dirección en [8..24])
    let ip = match family {
        2 => crate::net::IpAddr::V4([saddr[4], saddr[5], saddr[6], saddr[7]]),
        10 => {
            if addrlen < 28 { return -22; }
            let mut s6 = [0u8; 28];
            if paging::copy_from_user(&mut s6, addr_ptr, 28).is_err() { return -14; }
            let mut a = [0u8; 16]; a.copy_from_slice(&s6[8..24]);
            crate::net::IpAddr::V6(crate::net::ipv6::Ipv6Addr(a))
        }
        _ => return -97, // -EAFNOSUPPORT
    };

    match entry.fd_type {
        process::FdType::Socket(id) => {
            match crate::net::socket::socket_connect(id, ip, port) {
                Ok(()) => {
                    // [FRENTE B] Handshake TCP real: esperar el SYN-ACK del
                    // servidor (hasta 3 s) antes de declarar la conexion.
                    match crate::net::socket::socket_tcp_conn(id) {
                        Some(t_id) => match crate::net::tcp::tcp_wait_established(t_id, 300) {
                            Ok(()) => 0,
                            Err(e) => {
                                crate::drivers::serial::write_str("[TCP] connect sin SYN-ACK: plazo vencido / rechazada\n");
                                e
                            }
                        },
                        None => 0, // UDP: no hay handshake
                    }
                }
                Err(e) => e,
            }
        }
        _ => -88, // -ENOTSOCK
    }
}

fn sys_listen(sockfd: i32, backlog: i32) -> i32 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, sockfd as usize) {
        Some(e) => e,
        None => return -9,
    };
    match entry.fd_type {
        process::FdType::Socket(id) if crate::net::socket::socket_domain(id) == 1 => {
            match crate::net::socket::unix_listen(id, backlog) { Ok(()) => 0, Err(e) => e }
        }
        process::FdType::Socket(id) => {
            match crate::net::socket::socket_listen(id, backlog) {
                Ok(()) => 0,
                Err(e) => e,
            }
        }
        _ => -88, // -ENOTSOCK
    }
}

fn sys_accept(sockfd: i32, addr_ptr: usize, addrlen_ptr: usize) -> i32 {
    sys_accept4(sockfd, addr_ptr, addrlen_ptr, 0)
}

fn sys_accept4(sockfd: i32, addr_ptr: usize, addrlen_ptr: usize, flags: i32) -> i32 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let entry = match process::fd_get(proc, sockfd as usize) {
        Some(e) => *e,
        None => return -9,
    };
    // [CICLO 47-c] AF_UNIX con nombre: el nuevo descriptor es un extremo de par de tuberias.
    if let process::FdType::Socket(id) = entry.fd_type {
        if crate::net::socket::socket_domain(id) == 1 {
            if !crate::net::socket::socket_is_listening(id) { return -22; }
            if (flags & !(0x80000 | 0x800)) != 0 { return -22; }
            let is_nonblock = (flags & 0x800) != 0 || (entry.flags & 0x800) != 0;
            let (rd, wr) = match crate::net::socket::unix_accept(id, !is_nonblock) { Ok(x) => x, Err(e) => return e };
            let mut nuevo = process::FdEntry::new(process::FdType::SocketPairEnd { read_pipe: rd, write_pipe: wr });
            if (flags & 0x80000) != 0 { nuevo.flags |= 1; }
            if addr_ptr != 0 && addrlen_ptr != 0 {
                let fam: [u8; 2] = 1u16.to_ne_bytes();
                let _ = paging::copy_to_user(addr_ptr, &fam);
                let _ = paging::copy_to_user(addrlen_ptr, &2u32.to_ne_bytes());
            }
            return match process::fd_alloc(proc, nuevo) { Some(fd) => fd as i32, None => -24 };
        }
    }
    let new_sock_id = match entry.fd_type {
        process::FdType::Socket(id) => {
            if !crate::net::socket::socket_is_listening(id) {
                return -22; // -EINVAL
            }
            if (flags & !(0x80000 | 0x800)) != 0 {
                return -22; // -EINVAL
            }
            let is_nonblock = (flags & 0x800) != 0 || (entry.flags & 0x800) != 0;
            match if is_nonblock { crate::net::socket::socket_accept_nowait(id) } else { crate::net::socket::socket_accept(id) } {
                Ok(new_id) => new_id,
                Err(e) => return e,
            }
        }
        _ => return -88, // -ENOTSOCK
    };

    if (flags & !(0x80000 | 0x800)) != 0 {
        crate::net::socket::socket_close(new_sock_id);
        return -22; // -EINVAL
    }

    if addr_ptr != 0 && addrlen_ptr != 0 {
        // [CIERRE RED] Dirección real del par aceptado.
        let (rip, rport) = crate::net::socket::socket_get_peer_addr(new_sock_id).unwrap_or((crate::net::IpAddr::UNSPECIFIED4, 0));
        let mut cap = [0u8; 4]; let _ = paging::copy_from_user(&mut cap, addrlen_ptr, 4);
        let cap = u32::from_ne_bytes(cap) as usize;
        match rip {
            crate::net::IpAddr::V4(a4) => {
                let bytes = [2u8, 0, (rport >> 8) as u8, (rport & 0xff) as u8, a4[0], a4[1], a4[2], a4[3], 0, 0, 0, 0, 0, 0, 0, 0];
                let _ = paging::copy_to_user(addr_ptr, &bytes[..cap.min(16)]);
                let _ = paging::copy_to_user(addrlen_ptr, &16u32.to_ne_bytes());
            }
            crate::net::IpAddr::V6(a6) => {
                // [IPv6 REAL] sockaddr_in6; como Linux, se copia lo que quepa y addrlen informa del tamaño real (28)
                let mut bytes = [0u8; 28];
                bytes[0] = 10; bytes[2] = (rport >> 8) as u8; bytes[3] = (rport & 0xff) as u8;
                bytes[8..24].copy_from_slice(&a6.0);
                let _ = paging::copy_to_user(addr_ptr, &bytes[..cap.min(28)]);
                let _ = paging::copy_to_user(addrlen_ptr, &28u32.to_ne_bytes());
            }
        }
    }

    let mut fd_entry = process::FdEntry::new(process::FdType::Socket(new_sock_id));
    if (flags & 0x800) != 0 {
        fd_entry.flags |= 0x800; // O_NONBLOCK
    }
    match process::fd_alloc(proc, fd_entry) {
        Some(fd) => fd as i32,
        None => {
            crate::net::socket::socket_close(new_sock_id);
            -24 // -EMFILE
        }
    }
}

fn sys_inotify_init() -> i32 {
    sys_inotify_init1(0)
}

fn sys_inotify_init1(flags: i32) -> i32 {
    if (flags & !(0x80000 | 0x800)) != 0 {
        return -22; // -EINVAL
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    // [CICLO 48] Instancia real con cola de eventos.
    let id = match process::inotify_alloc(flags) { Some(i) => i, None => return -24 };
    let mut entry = process::FdEntry::new(process::FdType::Inotify(id));
    if (flags & 0x800) != 0 { entry.flags |= 0x800; }
    if (flags & 0x80000) != 0 { entry.flags |= 1; }
    match process::fd_alloc(proc, entry) {
        Some(fd) => fd as i32,
        None => { process::inotify_close(id); -24 }
    }
}

/// Identificador de instancia inotify de un fd, o -EINVAL si no lo es.
fn inotify_id_de(proc: &mut process::Process, fd: i32) -> Result<u32, i32> {
    if fd < 0 || fd as usize >= process::MAX_FDS { return Err(-9); }
    match process::fd_get(proc, fd as usize) {
        Some(e) => match e.fd_type { process::FdType::Inotify(id) => Ok(id), _ => Err(-22) },
        None => Err(-9),
    }
}

fn sys_inotify_add_watch(fd: i32, path_ptr: usize, mask: u32) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let id = match inotify_id_de(proc, fd) { Ok(i) => i, Err(e) => return e };
    if mask == 0 {
        return -22; // -EINVAL
    }
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let raw = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    let mut abs = [0u8; 256];
    let n = resolver_con_cwd(raw, &mut abs);
    let path = core::str::from_utf8(&abs[..n]).unwrap_or("");
    if !path_exists_check(path) {
        return -2; // -ENOENT
    }
    process::inotify_add_watch(id, path, mask)
}

fn sys_inotify_rm_watch(fd: i32, wd: i32) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let id = match inotify_id_de(proc, fd) { Ok(i) => i, Err(e) => return e };
    process::inotify_rm_watch(id, wd)
}

fn xattr_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn sys_setxattr(path_ptr: usize, name_ptr: usize, val_ptr: usize, size: usize, flags: i32) -> i32 {
    if path_ptr == 0 || name_ptr == 0 || !verify_user_string(path_ptr, 256) || !verify_user_string(name_ptr, 64) {
        return -14; // -EFAULT
    }
    if size > 128 {
        return -22; // -EINVAL
    }
    if size > 0 && (val_ptr == 0 || !verify_user_buffer(val_ptr, size)) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path) {
        return -2; // -ENOENT
    }
    let mut nbuf = [0u8; 256];
    let name = match read_user_path(name_ptr, &mut nbuf) {
        Some(s) => s,
        None => return -14,
    };

    let p_hash = xattr_hash(path);
    let mut table = process::XATTR_TABLE.lock();

    let existing_idx = table.entries.iter().position(|e| e.in_use && e.path_hash == p_hash && e.name_len == name.len() && &e.name[..name.len()] == name.as_bytes());

    if flags == 1 && existing_idx.is_some() { // XATTR_CREATE
        return -17; // -EEXIST
    }
    if flags == 2 && existing_idx.is_none() { // XATTR_REPLACE
        return -61; // -ENODATA
    }

    let target_idx = match existing_idx {
        Some(idx) => idx,
        None => match table.entries.iter().position(|e| !e.in_use) {
            Some(free_idx) => free_idx,
            None => return -28, // -ENOSPC
        },
    };

    let entry = &mut table.entries[target_idx];
    entry.in_use = true;
    entry.path_hash = p_hash;
    entry.name_len = name.len();
    entry.name[..name.len()].copy_from_slice(name.as_bytes());
    entry.value_len = size;
    if size > 0 {
        let _ = paging::copy_from_user(&mut entry.value[..size], val_ptr, size);
    }
    0
}

fn sys_getxattr(path_ptr: usize, name_ptr: usize, val_ptr: usize, size: usize) -> i64 {
    if path_ptr == 0 || name_ptr == 0 || !verify_user_string(path_ptr, 256) || !verify_user_string(name_ptr, 64) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path) {
        return -2; // -ENOENT
    }
    let mut nbuf = [0u8; 256];
    let name = match read_user_path(name_ptr, &mut nbuf) {
        Some(s) => s,
        None => return -14,
    };

    let p_hash = xattr_hash(path);
    let table = process::XATTR_TABLE.lock();
    let entry = match table.entries.iter().find(|e| e.in_use && e.path_hash == p_hash && e.name_len == name.len() && &e.name[..name.len()] == name.as_bytes()) {
        Some(e) => e,
        None => return -61, // -ENODATA
    };

    if size == 0 {
        return entry.value_len as i64;
    }
    if size < entry.value_len {
        return -34; // -ERANGE
    }
    if val_ptr == 0 || !verify_user_buffer(val_ptr, entry.value_len) {
        return -14;
    }
    if paging::copy_to_user(val_ptr, &entry.value[..entry.value_len]).is_err() {
        return -14;
    }
    entry.value_len as i64
}

fn sys_listxattr(path_ptr: usize, list_ptr: usize, size: usize) -> i64 {
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path) {
        return -2; // -ENOENT
    }

    let p_hash = xattr_hash(path);
    let table = process::XATTR_TABLE.lock();
    let mut total_len = 0;
    for e in table.entries.iter() {
        if e.in_use && e.path_hash == p_hash {
            total_len += e.name_len + 1; // name + null byte
        }
    }

    if size == 0 {
        return total_len as i64;
    }
    if size < total_len {
        return -34; // -ERANGE
    }
    if list_ptr == 0 || !verify_user_buffer(list_ptr, total_len) {
        return -14;
    }

    let mut offset = 0;
    for e in table.entries.iter() {
        if e.in_use && e.path_hash == p_hash {
            let _ = paging::copy_to_user(list_ptr + offset, &e.name[..e.name_len]);
            offset += e.name_len;
            let _ = paging::copy_to_user(list_ptr + offset, &[0u8]);
            offset += 1;
        }
    }
    total_len as i64
}

fn sys_removexattr(path_ptr: usize, name_ptr: usize) -> i32 {
    if path_ptr == 0 || name_ptr == 0 || !verify_user_string(path_ptr, 256) || !verify_user_string(name_ptr, 64) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path) {
        return -2; // -ENOENT
    }
    let mut nbuf = [0u8; 256];
    let name = match read_user_path(name_ptr, &mut nbuf) {
        Some(s) => s,
        None => return -14,
    };

    let p_hash = xattr_hash(path);
    let mut table = process::XATTR_TABLE.lock();
    for e in table.entries.iter_mut() {
        if e.in_use && e.path_hash == p_hash && e.name_len == name.len() && &e.name[..name.len()] == name.as_bytes() {
            e.in_use = false;
            return 0;
        }
    }
    -61 // -ENODATA
}

fn sys_fsetxattr(fd: i32, name_ptr: usize, val_ptr: usize, size: usize, flags: i32) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }

    let p_hash = 0x8000_0000_0000_0000 | (fd as u64);
    if name_ptr == 0 || !verify_user_string(name_ptr, 64) {
        return -14;
    }
    if size > 128 {
        return -22;
    }
    if size > 0 && (val_ptr == 0 || !verify_user_buffer(val_ptr, size)) {
        return -14;
    }
    let mut nbuf = [0u8; 256];
    let name = match read_user_path(name_ptr, &mut nbuf) {
        Some(s) => s,
        None => return -14,
    };

    let mut table = process::XATTR_TABLE.lock();
    let existing_idx = table.entries.iter().position(|e| e.in_use && e.path_hash == p_hash && e.name_len == name.len() && &e.name[..name.len()] == name.as_bytes());

    if flags == 1 && existing_idx.is_some() { return -17; }
    if flags == 2 && existing_idx.is_none() { return -61; }

    let target_idx = match existing_idx {
        Some(idx) => idx,
        None => match table.entries.iter().position(|e| !e.in_use) {
            Some(free_idx) => free_idx,
            None => return -28,
        },
    };

    let entry = &mut table.entries[target_idx];
    entry.in_use = true;
    entry.path_hash = p_hash;
    entry.name_len = name.len();
    entry.name[..name.len()].copy_from_slice(name.as_bytes());
    entry.value_len = size;
    if size > 0 {
        let _ = paging::copy_from_user(&mut entry.value[..size], val_ptr, size);
    }
    0
}

fn sys_fgetxattr(fd: i32, name_ptr: usize, val_ptr: usize, size: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    let p_hash = 0x8000_0000_0000_0000 | (fd as u64);
    if name_ptr == 0 || !verify_user_string(name_ptr, 64) {
        return -14;
    }
    let mut nbuf = [0u8; 256];
    let name = match read_user_path(name_ptr, &mut nbuf) {
        Some(s) => s,
        None => return -14,
    };

    let table = process::XATTR_TABLE.lock();
    let entry = match table.entries.iter().find(|e| e.in_use && e.path_hash == p_hash && e.name_len == name.len() && &e.name[..name.len()] == name.as_bytes()) {
        Some(e) => e,
        None => return -61,
    };

    if size == 0 {
        return entry.value_len as i64;
    }
    if size < entry.value_len {
        return -34;
    }
    if val_ptr == 0 || !verify_user_buffer(val_ptr, entry.value_len) {
        return -14;
    }
    if paging::copy_to_user(val_ptr, &entry.value[..entry.value_len]).is_err() {
        return -14;
    }
    entry.value_len as i64
}

fn sys_lsetxattr(path_ptr: usize, name_ptr: usize, val_ptr: usize, size: usize, flags: i32) -> i32 {
    sys_setxattr(path_ptr, name_ptr, val_ptr, size, flags)
}

fn sys_lgetxattr(path_ptr: usize, name_ptr: usize, val_ptr: usize, size: usize) -> i64 {
    sys_getxattr(path_ptr, name_ptr, val_ptr, size)
}

fn sys_llistxattr(path_ptr: usize, list_ptr: usize, size: usize) -> i64 {
    sys_listxattr(path_ptr, list_ptr, size)
}

fn sys_lremovexattr(path_ptr: usize, name_ptr: usize) -> i32 {
    sys_removexattr(path_ptr, name_ptr)
}

fn sys_flistxattr(fd: i32, list_ptr: usize, size: usize) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    let p_hash = 0x8000_0000_0000_0000 | (fd as u64);
    let table = process::XATTR_TABLE.lock();
    let mut total_len = 0;
    for e in table.entries.iter() {
        if e.in_use && e.path_hash == p_hash {
            total_len += e.name_len + 1;
        }
    }
    if size == 0 {
        return total_len as i64;
    }
    if size < total_len {
        return -34;
    }
    if list_ptr == 0 || !verify_user_buffer(list_ptr, total_len) {
        return -14;
    }
    let mut offset = 0;
    for e in table.entries.iter() {
        if e.in_use && e.path_hash == p_hash {
            let _ = paging::copy_to_user(list_ptr + offset, &e.name[..e.name_len]);
            offset += e.name_len;
            let _ = paging::copy_to_user(list_ptr + offset, &[0u8]);
            offset += 1;
        }
    }
    total_len as i64
}

fn sys_fremovexattr(fd: i32, name_ptr: usize) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    let p_hash = 0x8000_0000_0000_0000 | (fd as u64);
    if name_ptr == 0 || !verify_user_string(name_ptr, 64) {
        return -14;
    }
    let mut nbuf = [0u8; 256];
    let name = match read_user_path(name_ptr, &mut nbuf) {
        Some(s) => s,
        None => return -14,
    };
    let mut table = process::XATTR_TABLE.lock();
    for e in table.entries.iter_mut() {
        if e.in_use && e.path_hash == p_hash && e.name_len == name.len() && &e.name[..name.len()] == name.as_bytes() {
            e.in_use = false;
            return 0;
        }
    }
    -61 // -ENODATA
}

fn sys_mknodat(dfd: i32, path_ptr: usize, mode: u32, _dev: u64) -> i32 {
    if dfd != -100 && (dfd < 0 || dfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14;
    }
    let mut buf = [0u8; 256];
    let path = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    let ftype = mode & 0o170000;
    if ftype != 0o100000 && ftype != 0o020000 && ftype != 0o060000 && ftype != 0o010000 && ftype != 0o140000 {
        return -22; // -EINVAL: invalid file type
    }
    if path_exists_check(path) {
        return -17; // -EEXIST
    }
    if (ftype == 0o020000 || ftype == 0o060000) && process::current_euid() != 0 {
        return -1; // -EPERM
    }
    crear_nodo_especial(path, mode)
}

fn sys_futimesat(dfd: i32, path_ptr: usize, _times_ptr: usize) -> i32 {
    if dfd != -100 && (dfd < 0 || dfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    if path_ptr != 0 {
        let mut buf = [0u8; 256];
        let path = match read_user_path(path_ptr, &mut buf) {
            Some(s) => s,
            None => return -14,
        };
        if !path_exists_check(path) {
            return -2; // -ENOENT
        }
    }
    0
}

fn sys_select(nfds: i32, _readfds_ptr: usize, _writefds_ptr: usize, _exceptfds_ptr: usize, timeout_ptr: usize) -> i32 {
    if nfds < 0 || nfds > 1024 {
        return -22; // -EINVAL
    }
    if timeout_ptr != 0 {
        if !verify_user_buffer(timeout_ptr, 16) {
            return -14;
        }
        let mut buf = [0u8; 16];
        if paging::copy_from_user(&mut buf, timeout_ptr, 16).is_err() {
            return -14;
        }
        let usec = i64::from_ne_bytes(buf[8..16].try_into().unwrap_or_default());
        if usec < 0 || usec >= 1_000_000 {
            return -22;
        }
    }
    0
}

fn sys_pselect6(nfds: i32, _readfds_ptr: usize, _writefds_ptr: usize, _exceptfds_ptr: usize, timeout_ptr: usize, _sigmask_ptr: usize) -> i32 {
    if nfds < 0 || nfds > 1024 {
        return -22; // -EINVAL
    }
    if timeout_ptr != 0 {
        if !verify_user_buffer(timeout_ptr, 16) {
            return -14;
        }
        let mut buf = [0u8; 16];
        if paging::copy_from_user(&mut buf, timeout_ptr, 16).is_err() {
            return -14;
        }
        let nsec = i64::from_ne_bytes(buf[8..16].try_into().unwrap_or_default());
        if nsec < 0 || nsec >= 1_000_000_000 {
            return -22;
        }
    }
    0
}

fn sys_adjtimex(buf_ptr: usize) -> i32 {
    if buf_ptr == 0 || !verify_user_buffer(buf_ptr, 208) {
        return -14;
    }
    0
}

fn sys_clock_adjtime(clk_id: i32, buf_ptr: usize) -> i32 {
    if clk_id != 0 && clk_id != 1 {
        return -22;
    }
    if buf_ptr == 0 || !verify_user_buffer(buf_ptr, 208) {
        return -14;
    }
    0
}

fn sys_migrate_pages(pid: i32, maxnode: usize, old_nodes_ptr: usize, new_nodes_ptr: usize) -> i64 {
    if pid <= 0 || process::process_by_pid(pid as u64).is_none() {
        return -3; // -ESRCH
    }
    if maxnode == 0 || maxnode > 1024 {
        return -22; // -EINVAL
    }
    if old_nodes_ptr == 0 || new_nodes_ptr == 0 {
        return -14; // -EFAULT
    }
    0
}

fn sys_move_pages(pid: i32, count: usize, pages_ptr: usize, _nodes_ptr: usize, status_ptr: usize, flags: i32) -> i64 {
    if (flags & !0x07) != 0 {
        return -22; // -EINVAL
    }
    if pid <= 0 || process::process_by_pid(pid as u64).is_none() {
        return -3; // -ESRCH
    }
    if count == 0 {
        return 0;
    }
    if pages_ptr == 0 || status_ptr == 0 {
        return -14; // -EFAULT
    }
    0
}

fn sys_quotactl_fd(fd: i32, cmd: u32, _id: i32, _addr: usize) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    if cmd == 0 || cmd > 8 {
        return -22; // -EINVAL
    }
    0
}

fn sys_execveat(dirfd: i32, pathname_ptr: usize, argv: usize, envp: usize, flags: i32, current_rsp: u64) -> SyscallResult {
    if (flags & !(0x100 | 0x1000)) != 0 {
        return SyscallResult(-22i64 as u64, 0); // -EINVAL
    }
    if pathname_ptr == 0 {
        return SyscallResult(-14i64 as u64, 0); // -EFAULT
    }
    if !verify_user_string(pathname_ptr, 256) {
        return SyscallResult(-14i64 as u64, 0); // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path_str = match read_user_path(pathname_ptr, &mut buf) {
        Some(s) => s,
        None => return SyscallResult(-14i64 as u64, 0),
    };
    if path_str.is_empty() {
        if (flags & 0x1000) != 0 {
            if dirfd != -100 && (dirfd < 0 || dirfd as usize >= process::MAX_FDS) {
                return SyscallResult(-9i64 as u64, 0); // -EBADF
            }
        }
        return SyscallResult(-2i64 as u64, 0); // -ENOENT
    }
    if user_path_is_relative(pathname_ptr) && dirfd != -100 {
        if dirfd < 0 || dirfd as usize >= process::MAX_FDS {
            return SyscallResult(-9i64 as u64, 0); // -EBADF
        }
    }
    if !path_exists_check(path_str) {
        return SyscallResult(-2i64 as u64, 0); // -ENOENT
    }
    sys_execve(pathname_ptr, argv, envp, current_rsp)
}

fn sys_perf_event_open(attr_ptr: usize, pid: i32, _cpu: i32, group_fd: i32, flags: u64) -> i64 {
    if attr_ptr == 0 || !verify_user_buffer(attr_ptr, 112) {
        return -14; // -EFAULT
    }
    if (flags & !0x0f) != 0 {
        return -22; // -EINVAL
    }
    if pid > 0 {
        let exists = process::process_by_pid(pid as u64).is_some();
        if !exists {
            return -3; // -ESRCH
        }
    }
    if group_fd != -1 && (group_fd < 0 || group_fd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    match process::fd_alloc(proc, process::FdEntry::new(process::FdType::Device(process::DeviceInfo { dev_type: process::DeviceType::Null, pos: 0 }))) {
        Some(fd) => fd as i64,
        None => -24, // -EMFILE
    }
}

fn sys_bpf(cmd: i32, uattr_ptr: usize, size: u32) -> i64 {
    if size < 8 || size > 4096 {
        return -22; // -EINVAL
    }
    if uattr_ptr == 0 || !verify_user_buffer(uattr_ptr, size as usize) {
        return -14; // -EFAULT
    }
    if cmd < 0 || cmd > 36 {
        return -22; // -EINVAL
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    match process::fd_alloc(proc, process::FdEntry::new(process::FdType::Device(process::DeviceInfo { dev_type: process::DeviceType::Null, pos: 0 }))) {
        Some(fd) => fd as i64,
        None => -24, // -EMFILE
    }
}

fn sys_mount_setattr(dfd: i32, path_ptr: usize, flags: u32, uattr_ptr: usize, usize_val: usize) -> i32 {
    if dfd != -100 && (dfd < 0 || dfd as usize >= process::MAX_FDS) {
        return -9; // -EBADF
    }
    if (flags & !0x3F) != 0 {
        return -22; // -EINVAL
    }
    if usize_val < 8 {
        return -22; // -EINVAL
    }
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    if uattr_ptr == 0 || !verify_user_buffer(uattr_ptr, usize_val) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path_str = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path_str) {
        return -2; // -ENOENT
    }
    0
}

fn sys_set_mempolicy_home_node(start: usize, len: usize, home_node: u64, flags: u64) -> i32 {
    if (start & 0xfff) != 0 || len == 0 || flags != 0 || home_node > 1024 {
        return -22; // -EINVAL
    }
    0
}

fn sys_cachestat(fd: i32, cstat_range_ptr: usize, cstat_ptr: usize, flags: u32) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9;
    }
    if cstat_range_ptr == 0 || !verify_user_buffer(cstat_range_ptr, 16) ||
       cstat_ptr == 0 || !verify_user_buffer(cstat_ptr, 40) {
        return -14; // -EFAULT
    }
    if flags != 0 {
        return -22; // -EINVAL
    }
    let cstat_data = [1u64, 0u64, 0u64, 0u64, 0u64];
    let bytes = unsafe { core::slice::from_raw_parts(cstat_data.as_ptr() as *const u8, 40) };
    if paging::copy_to_user(cstat_ptr, bytes).is_err() {
        return -14;
    }
    0
}

#[derive(Clone, Copy)]
struct KernelAltstack {
    sp: usize,
    flags: i32,
    size: usize,
}

/// [b187] Pila alternativa de señales POR PROCESO (tabla pid -> altstack).
/// Antes era un único global: `sort` de coreutils (c-stack) la fijaba y
/// `test_posix` (otro proceso) leía esa pila en vez de SS_DISABLE (TEST 923).
/// Las señales todavía no se ENTREGAN sobre esta pila (deuda anotada).
const ALTSTACK_RANURAS: usize = 32;
static CURRENT_ALTSTACK: crate::arch::Spinlock<[(u64, KernelAltstack); ALTSTACK_RANURAS]> =
    crate::arch::Spinlock::new([(0, KernelAltstack { sp: 0, flags: 2, size: 0 }); ALTSTACK_RANURAS]);

fn sys_sigaltstack(uss_ptr: usize, uoss_ptr: usize) -> i32 {
    let pid = process::current_pid();
    let mut tabla = CURRENT_ALTSTACK.lock();
    let idx = match tabla.iter().position(|(p, _)| *p == pid) {
        Some(i) => i,
        None => {
            // ranura libre o, si no hay, la de un pid ya muerto; si tampoco, la 0
            let libre = tabla.iter().position(|(p, _)| *p == 0)
                .or_else(|| tabla.iter().position(|(p, _)| process::with_process(*p, |_| ()).is_none()))
                .unwrap_or(0);
            tabla[libre] = (pid, KernelAltstack { sp: 0, flags: 2, size: 0 });
            libre
        }
    };
    let cur = &mut tabla[idx].1;
    if uoss_ptr != 0 {
        if !verify_user_buffer(uoss_ptr, 24) {
            return -14; // -EFAULT
        }
        let old_st = [cur.sp, cur.flags as usize, cur.size];
        let bytes = unsafe { core::slice::from_raw_parts(old_st.as_ptr() as *const u8, 24) };
        if paging::copy_to_user(uoss_ptr, bytes).is_err() {
            return -14;
        }
    }
    if uss_ptr != 0 {
        if !verify_user_buffer(uss_ptr, 24) {
            return -14; // -EFAULT
        }
        let mut buf = [0u8; 24];
        if paging::copy_from_user(&mut buf, uss_ptr, 24).is_err() {
            return -14;
        }
        let ss_sp = usize::from_ne_bytes(buf[0..8].try_into().unwrap_or_default());
        let ss_flags = i32::from_ne_bytes(buf[8..12].try_into().unwrap_or_default());
        let ss_size = usize::from_ne_bytes(buf[16..24].try_into().unwrap_or_default());

        if (ss_flags & !2) != 0 {
            return -22; // -EINVAL
        }
        if (ss_flags & 2) == 0 {
            if ss_size < 2048 {
                return -12; // -ENOMEM: MINSIGSTKSZ
            }
            cur.sp = ss_sp;
            cur.flags = ss_flags;
            cur.size = ss_size;
        } else {
            cur.flags = 2; // SS_DISABLE
        }
    }
    0
}

/// [T-001/FIFO] `rt_sigsuspend` REAL. Antes devolvía -EINTR en el acto sin
/// esperar ni tocar la máscara: `busybox sh` (`wait`, y cualquier guion con
/// `&`) giraba para siempre porque su manejador de SIGCHLD nunca corría.
/// Semántica POSIX: la máscara pasa a `newset` de forma atómica, el proceso se
/// bloquea hasta que haya una señal entregable, ésta se entrega al volver a
/// Ring 3 y la máscara previa se restaura al salir del manejador (o aquí si
/// la señal no tiene manejador). Devuelve siempre -EINTR.
fn sys_rt_sigsuspend(unewset: usize, sigsetsize: usize, current_rsp: u64) -> (i64, u64) {
    if sigsetsize != 8 { return (-22, 0); }
    if unewset == 0 || !verify_user_buffer(unewset, 8) { return (-14, 0); }
    let mut b = [0u8; 8];
    if paging::copy_from_user(&mut b, unewset, 8).is_err() { return (-14, 0); }
    let newset = u64::from_ne_bytes(b) & !((1u64 << 9) | (1u64 << 19)); // SIGKILL/SIGSTOP inmutables
    let proc = match process::current_process() { Some(p) => p, None => return (-3, 0) };
    if !proc.sig_suspend_activo {
        proc.sig_suspend_prev = proc.sig_blocked;
        proc.sig_suspend_activo = true;
    }
    proc.sig_blocked = newset;
    let entregable = proc.sig_pending & !proc.sig_blocked;
    if entregable != 0 {
        let sig = entregable.trailing_zeros();
        let disp = process::signals::current_disposition(sig);
        if disp == 1 || (disp == 0 && matches!(process::signals::default_action(sig), process::signals::DefaultAction::Ignore)) {
            // Señal ignorada: se descarta y se sigue esperando (POSIX).
            let _ = process::signals::current_sig_consume(1u64 << sig);
            return esperar_senal(current_rsp);
        }
        if disp == 0 {
            // Acción por defecto (terminar/parar): no habrá manejador que restaure
            // la máscara; se restaura aquí antes de la entrega.
            proc.sig_blocked = proc.sig_suspend_prev;
            proc.sig_suspend_activo = false;
        }
        return (-4, 0);
    }
    esperar_senal(current_rsp)
}

fn sys_rt_sigpending(uset_ptr: usize, sigsetsize: usize) -> i32 {
    if sigsetsize != 8 {
        return -22; // -EINVAL
    }
    if uset_ptr == 0 || !verify_user_buffer(uset_ptr, 8) {
        return -14; // -EFAULT
    }
    let zero_mask = 0u64.to_ne_bytes();
    if paging::copy_to_user(uset_ptr, &zero_mask).is_err() {
        return -14;
    }
    0
}

fn sys_rt_sigqueueinfo(pid: i32, sig: i32, uinfo_ptr: usize) -> i32 {
    if sig < 0 || sig > 64 {
        return -22; // -EINVAL
    }
    if uinfo_ptr == 0 || !verify_user_buffer(uinfo_ptr, 128) {
        return -14; // -EFAULT
    }
    if pid <= 0 {
        return -3; // -ESRCH
    }
    if process::process_by_pid(pid as u64).is_none() {
        return -3; // -ESRCH
    }
    // [CICLO 48] si_code (desplazamiento 8) y si_value (24) viajan hasta el
    // manejador SA_SIGINFO del destinatario. Un proceso no puede falsificar
    // codigos del nucleo (>= 0) hacia otros (EPERM), como en Linux.
    let mut raw = [0u8; 32];
    if paging::copy_from_user(&mut raw, uinfo_ptr, 32).is_err() { return -14; }
    let code = i32::from_ne_bytes([raw[8], raw[9], raw[10], raw[11]]);
    let value = u64::from_ne_bytes([raw[24], raw[25], raw[26], raw[27], raw[28], raw[29], raw[30], raw[31]]);
    let yo = process::current_process().map(|p| p.pid).unwrap_or(0);
    if code >= 0 && pid as u64 != yo { return -1; } // -EPERM
    if sig == 0 { return 0; }
    crate::process::signals::raise_for_pid_res(pid as u64, sig as u32, code, value)
}

fn sys_rt_sigtimedwait(uthese_ptr: usize, uinfo_ptr: usize, uts_ptr: usize, sigsetsize: usize) -> i32 {
    if sigsetsize != 8 {
        return -22; // -EINVAL
    }
    if uthese_ptr == 0 || !verify_user_buffer(uthese_ptr, 8) {
        return -14; // -EFAULT
    }
    let mut espera_ticks: Option<u64> = None;
    if uts_ptr != 0 {
        if !verify_user_buffer(uts_ptr, 16) {
            return -14; // -EFAULT
        }
        let mut ts_buf = [0u8; 16];
        if paging::copy_from_user(&mut ts_buf, uts_ptr, 16).is_err() {
            return -14;
        }
        let tv_sec = i64::from_ne_bytes(ts_buf[0..8].try_into().unwrap_or_default());
        let tv_nsec = i64::from_ne_bytes(ts_buf[8..16].try_into().unwrap_or_default());
        if tv_nsec < 0 || tv_nsec >= 1_000_000_000 || tv_sec < 0 {
            return -22; // -EINVAL
        }
        espera_ticks = Some((tv_sec as u64) * 100 + (tv_nsec as u64 + 9_999_999) / 10_000_000);
    }
    let mut set_b = [0u8; 8];
    if paging::copy_from_user(&mut set_b, uthese_ptr, 8).is_err() { return -14; }
    let set = u64::from_ne_bytes(set_b);
    if uinfo_ptr != 0 && !verify_user_buffer(uinfo_ptr, 128) { return -14; }
    // [CICLO 48] Espera REAL sobre las senales pendientes (bitmap + cola RT):
    // consume la de menor numero del conjunto y entrega su siginfo (origen,
    // si_code, si_value). Sin plazo: tope de seguridad de 10 s (-EAGAIN).
    let inicio = crate::time::pit::ticks();
    let tope = inicio.wrapping_add(espera_ticks.unwrap_or(1000).min(1000));
    loop {
        if let Some((sig, origen)) = crate::process::signals::tomar_pendiente(set) {
            if uinfo_ptr != 0 {
                let mut info = [0u8; 128];
                info[0..4].copy_from_slice(&(sig as i32).to_ne_bytes());
                info[8..12].copy_from_slice(&origen.code.to_ne_bytes());
                info[16..20].copy_from_slice(&origen.pid.to_ne_bytes());
                info[20..24].copy_from_slice(&origen.uid.to_ne_bytes());
                info[24..32].copy_from_slice(&origen.value.to_ne_bytes());
                if paging::copy_to_user(uinfo_ptr, &info).is_err() { return -14; }
            }
            return sig as i32;
        }
        if espera_ticks == Some(0) || crate::time::pit::ticks() >= tope { return -11; } // -EAGAIN
        // Una senal pendiente fuera del conjunto y no bloqueada interrumpe la espera.
        let (pend, bloq) = crate::process::signals::pendientes_y_bloqueadas();
        if (pend & !set & !bloq) != 0 { return -4; } // -EINTR
        unsafe { crate::process::scheduler::wait_hlt_preemptible(); }
    }
}

fn sys_arch_prctl(code: i32, addr: usize) -> i32 {
    match code {
        0x1001 => { // ARCH_SET_GS
            if addr >= 0x0000_8000_0000_0000 {
                return -14; // -EFAULT
            }
            unsafe {
                let low = addr as u32;
                let high = (addr >> 32) as u32;
                core::arch::asm!("wrmsr", in("ecx") 0xC0000102u32, in("eax") low, in("edx") high, options(nostack, preserves_flags));
            }
            0
        }
        0x1002 => { // ARCH_SET_FS
            if addr >= 0x0000_8000_0000_0000 {
                return -14; // -EFAULT
            }
            if let Some(proc) = process::current_process() {
                proc.fs_base = addr as u64;
            }
            unsafe {
                let low = addr as u32;
                let high = (addr >> 32) as u32;
                core::arch::asm!("wrmsr", in("ecx") 0xC0000100u32, in("eax") low, in("edx") high, options(nostack, preserves_flags));
            }
            0
        }
        0x1003 => { // ARCH_GET_FS
            if addr == 0 || !verify_user_buffer(addr, 8) {
                return -14; // -EFAULT
            }
            let val: u64 = unsafe {
                let low: u32;
                let high: u32;
                core::arch::asm!("rdmsr", in("ecx") 0xC0000100u32, out("eax") low, out("edx") high, options(nostack, preserves_flags));
                ((high as u64) << 32) | (low as u64)
            };
            if paging::copy_to_user(addr, &val.to_ne_bytes()).is_err() {
                return -14;
            }
            0
        }
        0x1004 => { // ARCH_GET_GS
            if addr == 0 || !verify_user_buffer(addr, 8) {
                return -14; // -EFAULT
            }
            let val: u64 = unsafe {
                let low: u32;
                let high: u32;
                core::arch::asm!("rdmsr", in("ecx") 0xC0000102u32, out("eax") low, out("edx") high, options(nostack, preserves_flags));
                ((high as u64) << 32) | (low as u64)
            };
            if paging::copy_to_user(addr, &val.to_ne_bytes()).is_err() {
                return -14;
            }
            0
        }
        _ => -22, // -EINVAL
    }
}

fn sys_flock(fd: i32, operation: i32) -> i32 {
    if fd < 0 || fd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, fd as usize).is_none() {
        return -9; // -EBADF
    }
    let op = operation & !4; // Strip LOCK_NB
    if (op & !0x0b) != 0 || (op & 0x0b) == 0 {
        return -22; // -EINVAL
    }
    0
}

fn sys_lchown(path_ptr: usize, _owner: u32, _group: u32) -> i32 {
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path_str = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path_str) {
        return -2; // -ENOENT
    }
    // [T-001] Si la ruta es un enlace simbolico, cambiar el dueño del propio
    // enlace no esta soportado: EOPNOTSUPP (nunca un 0 sin efecto). Si no,
    // es chown(2).
    if let Some(Ok(m)) = vfs::con_fs(path_str, |fs| fs.metadatos(path_str)) { if m.es_enlace() { return -95; } }
    sys_chown(path_ptr, _owner, _group) as i32
}

fn sys_utimes(path_ptr: usize, times_ptr: usize) -> i32 {
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path_str = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path_str) {
        return -2; // -ENOENT
    }
    if times_ptr != 0 {
        if !verify_user_buffer(times_ptr, 32) {
            return -14; // -EFAULT
        }
        let mut tv_buf = [0u8; 32];
        if paging::copy_from_user(&mut tv_buf, times_ptr, 32).is_err() {
            return -14;
        }
        let usec1 = i64::from_ne_bytes(tv_buf[8..16].try_into().unwrap_or_default());
        let usec2 = i64::from_ne_bytes(tv_buf[24..32].try_into().unwrap_or_default());
        if usec1 < 0 || usec1 >= 1_000_000 || usec2 < 0 || usec2 >= 1_000_000 {
            return -22; // -EINVAL
        }
    }
    0
}

// ── Syscalls 331 - 345: Modern POSIX Primitives & Process Virtualization ─────

fn sys_chroot(path_ptr: usize) -> i32 {
    if path_ptr == 0 || !verify_user_string(path_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path_str = match read_user_path(path_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path_str) {
        return -2; // -ENOENT
    }
    0
}

fn sys_getdomainname(name_ptr: usize, len: usize) -> i32 {
    if name_ptr == 0 || !verify_user_buffer(name_ptr, len) {
        return -14; // -EFAULT
    }
    if len < 8 {
        return -22; // -EINVAL
    }
    let domain = b"portix.local\0";
    let copy_len = domain.len().min(len);
    if paging::copy_to_user(name_ptr, &domain[..copy_len]).is_err() {
        return -14;
    }
    0
}

fn sys_eventfd(initval: u32) -> i64 {
    sys_eventfd2(initval, 0) as i64
}

fn sys_rt_tgsigqueueinfo(tgid: i32, tid: i32, sig: i32, uinfo_ptr: usize) -> i32 {
    if sig < 0 || sig > 64 {
        return -22; // -EINVAL
    }
    if uinfo_ptr == 0 || !verify_user_buffer(uinfo_ptr, 128) {
        return -14; // -EFAULT
    }
    if tgid <= 0 || tid <= 0 {
        return -3; // -ESRCH
    }
    if process::process_by_pid(tgid as u64).is_none() {
        return -3; // -ESRCH
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    match process::kill_matching(tgid as i64, sig, proc.pgid) {
        Ok(_) => 0,
        Err(_) => -3,
    }
}

fn sys_recvmmsg(sockfd: i32, msgvec_ptr: usize, vlen: u32, flags: u32, timeout_ptr: usize) -> i32 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, sockfd as usize).is_none() {
        return -9; // -EBADF
    }
    if (flags & !0x4000) != 0 {
        return -22; // -EINVAL
    }
    if vlen == 0 || vlen > 1024 {
        return -22; // -EINVAL
    }
    if msgvec_ptr == 0 || !verify_user_buffer(msgvec_ptr, vlen as usize * 32) {
        return -14; // -EFAULT
    }
    if timeout_ptr != 0 {
        if !verify_user_buffer(timeout_ptr, 16) {
            return -14; // -EFAULT
        }
        let mut ts_buf = [0u8; 16];
        if paging::copy_from_user(&mut ts_buf, timeout_ptr, 16).is_err() {
            return -14;
        }
        let tv_nsec = i64::from_ne_bytes(ts_buf[8..16].try_into().unwrap_or_default());
        if tv_nsec < 0 || tv_nsec >= 1_000_000_000 {
            return -22; // -EINVAL
        }
    }
    0
}

fn sys_sendmmsg(sockfd: i32, msgvec_ptr: usize, vlen: u32, flags: u32) -> i32 {
    if sockfd < 0 || sockfd as usize >= process::MAX_FDS {
        return -9; // -EBADF
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    if process::fd_get(proc, sockfd as usize).is_none() {
        return -9; // -EBADF
    }
    if (flags & !0x4000) != 0 {
        return -22; // -EINVAL
    }
    if vlen == 0 || vlen > 1024 {
        return -22; // -EINVAL
    }
    if msgvec_ptr == 0 || !verify_user_buffer(msgvec_ptr, vlen as usize * 32) {
        return -14; // -EFAULT
    }
    0
}

fn sys_process_vm_readv(pid: i32, lvec_ptr: usize, liovcnt: usize, rvec_ptr: usize, riovcnt: usize, flags: u64) -> i64 {
    if flags != 0 {
        return -22; // -EINVAL
    }
    if pid <= 0 {
        return -3; // -ESRCH
    }
    if process::process_by_pid(pid as u64).is_none() {
        return -3; // -ESRCH
    }
    if liovcnt == 0 || riovcnt == 0 || liovcnt > 1024 || riovcnt > 1024 {
        return -22; // -EINVAL
    }
    if lvec_ptr == 0 || !verify_user_buffer(lvec_ptr, liovcnt * 16) ||
       rvec_ptr == 0 || !verify_user_buffer(rvec_ptr, riovcnt * 16) {
        return -14; // -EFAULT
    }
    0
}

fn sys_process_vm_writev(pid: i32, lvec_ptr: usize, liovcnt: usize, rvec_ptr: usize, riovcnt: usize, flags: u64) -> i64 {
    if flags != 0 {
        return -22; // -EINVAL
    }
    if pid <= 0 {
        return -3; // -ESRCH
    }
    if process::process_by_pid(pid as u64).is_none() {
        return -3; // -ESRCH
    }
    if liovcnt == 0 || riovcnt == 0 || liovcnt > 1024 || riovcnt > 1024 {
        return -22; // -EINVAL
    }
    if lvec_ptr == 0 || !verify_user_buffer(lvec_ptr, liovcnt * 16) ||
       rvec_ptr == 0 || !verify_user_buffer(rvec_ptr, riovcnt * 16) {
        return -14; // -EFAULT
    }
    0
}

fn sys_semtimedop(semid: i32, sops_ptr: usize, nsops: usize, timeout_ptr: usize) -> i32 {
    if semid < 0 {
        return -22; // -EINVAL
    }
    if nsops == 0 || nsops > 256 {
        return -22; // -EINVAL
    }
    if sops_ptr == 0 || !verify_user_buffer(sops_ptr, nsops * 6) {
        return -14; // -EFAULT
    }
    if timeout_ptr != 0 {
        if !verify_user_buffer(timeout_ptr, 16) {
            return -14; // -EFAULT
        }
        let mut ts_buf = [0u8; 16];
        if paging::copy_from_user(&mut ts_buf, timeout_ptr, 16).is_err() {
            return -14;
        }
        let tv_nsec = i64::from_ne_bytes(ts_buf[8..16].try_into().unwrap_or_default());
        if tv_nsec < 0 || tv_nsec >= 1_000_000_000 {
            return -22; // -EINVAL
        }
    }
    0
}

fn sys_acct(filename_ptr: usize) -> i32 {
    if filename_ptr == 0 {
        return 0; // Disable accounting
    }
    if !verify_user_string(filename_ptr, 256) {
        return -14; // -EFAULT
    }
    let mut buf = [0u8; 256];
    let path_str = match read_user_path(filename_ptr, &mut buf) {
        Some(s) => s,
        None => return -14,
    };
    if !path_exists_check(path_str) {
        return -2; // -ENOENT
    }
    0
}

fn sys_ustat(dev: u32, ubuf_ptr: usize) -> i32 {
    if dev > 255 {
        return -22; // -EINVAL
    }
    if ubuf_ptr == 0 || !verify_user_buffer(ubuf_ptr, 32) {
        return -14; // -EFAULT
    }
    let ustat_data = [65536i64, 4096i64, 0i64, 0i64];
    let bytes = unsafe { core::slice::from_raw_parts(ustat_data.as_ptr() as *const u8, 32) };
    if paging::copy_to_user(ubuf_ptr, bytes).is_err() {
        return -14;
    }
    0
}

fn sys_syslog(action_type: i32, bufp: usize, len: i32) -> i32 {
    if action_type < 0 || action_type > 10 {
        return -22; // -EINVAL
    }
    if action_type == 2 || action_type == 3 || action_type == 4 {
        if len < 0 {
            return -22; // -EINVAL
        }
        if bufp == 0 || !verify_user_buffer(bufp, len as usize) {
            return -14; // -EFAULT
        }
    }
    0
}

fn sys_vhangup() -> i32 {
    0
}

fn sys_modify_ldt(func: i32, ptr: usize, bytecount: usize) -> i32 {
    if func != 0 && func != 1 && func != 2 {
        return -22; // -EINVAL
    }
    if ptr == 0 || !verify_user_buffer(ptr, bytecount) {
        return -14; // -EFAULT
    }
    0
}

fn sys_setfsuid(fsuid: u32) -> i32 {
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let old_uid = proc.uid;
    proc.uid = fsuid;
    old_uid as i32
}

fn sys_setfsgid(fsgid: u32) -> i32 {
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -9,
    };
    let old_gid = proc.gid;
    proc.gid = fsgid;
    old_gid as i32
}

pub const PTRACE_TRACEME: i64 = 0;
pub const PTRACE_PEEKTEXT: i64 = 1;
pub const PTRACE_PEEKDATA: i64 = 2;
pub const PTRACE_POKETEXT: i64 = 4;
pub const PTRACE_POKEDATA: i64 = 5;
pub const PTRACE_CONT: i64 = 7;
pub const PTRACE_KILL: i64 = 8;
pub const PTRACE_SINGLESTEP: i64 = 9;
pub const PTRACE_GETREGS: i64 = 12;
pub const PTRACE_SETREGS: i64 = 13;
pub const PTRACE_ATTACH: i64 = 16;
pub const PTRACE_DETACH: i64 = 17;
// [CICLO v6.x] Extensiones de Portix: puntos de vigilancia por hardware.
// data = código de longitud DR7 (bits 0-7) | código de acceso (bits 8-15).
pub const PTRACE_SETWATCHPOINT: i64 = 0x50;
pub const PTRACE_CLEARWATCHPOINTS: i64 = 0x51;
pub const PTRACE_GETWATCHPOINTS: i64 = 0x52;
// [CICLO v6.x] Breakpoints condicionales: `addr` es la dirección del punto de
// parada y `data` un puntero a la especificación (registro, operador, valor).
pub const PTRACE_SETBPCOND: i64 = 0x53;
pub const PTRACE_GETBPCOND: i64 = 0x54;
pub const PTRACE_CLEARBPCOND: i64 = 0x55;
/// `ignore N` sobre una condición ya definida (`addr` = punto de parada, `data` = N).
pub const PTRACE_SETBPCIGNORE: i64 = 0x56;
/// Condición de DOS cláusulas (conjunción). La especificación son 40 bytes:
/// `n` (u32, 1..2) + dos cláusulas de 16 bytes (`reg` u32, `op` u32, `value` u64).
pub const PTRACE_SETBPCOND2: i64 = 0x57;
/// Lenguaje completo de condiciones: hasta cuatro cláusulas en dos grupos
/// (Y dentro del grupo, O entre grupos). Especificación de 72 bytes: `n` (u32),
/// `g2` (u32), y cuatro cláusulas de 16 bytes (`reg` u32, `op` u32, `value` u64).
pub const PTRACE_SETBPCONDN: i64 = 0x58;
/// [FRENTE C] Información de la última parada por #DB: 40 bytes
/// (DR6 u64, ranura u8 + 7 de relleno, dirección vigilada u64, código de
/// longitud u8 + 7 de relleno, RIP de la parada u64).
pub const PTRACE_GETTRAPINFO: i64 = 0x59;

fn sys_ptrace(request: i64, pid: i64, addr: usize, data: usize) -> i64 {
    let cur_pid = process::current_pid();
    // [CICLO v6.x] Cualquier operación de depuración dirigida a OTRO proceso
    // convierte al llamador en trazador activo: desde ese momento el objetivo
    // se detiene en los traps y la parada se entrega por `waitpid`. Sin esta
    // marca, `PTRACE_TRACEME` por sí solo dejaría colgado a cualquier proceso
    // que sólo verifique el ABI (es lo que hacía la suite POSIX: el proceso se
    // declaraba trazable, disparaba un trap y nadie lo reanudaba).
    if request != PTRACE_TRACEME && pid > 0 && pid as u64 != cur_pid {
        process::mark_tracer_engaged(pid as u64);
    }
    match request {
        PTRACE_TRACEME => {
            if let Some(proc) = process::current_process() {
                proc.ptrace_traced = true;
                proc.ptrace_tracer = proc.ppid;
                0
            } else {
                -1 // -EPERM
            }
        }
        PTRACE_ATTACH => {
            if pid <= 0 || pid as u64 == cur_pid {
                return -22; // -EINVAL
            }
            if process::set_process_traced(pid as u64, cur_pid) {
                0
            } else {
                -3 // -ESRCH
            }
        }
        PTRACE_DETACH => {
            if pid <= 0 { return -22; }
            if process::set_process_traced(pid as u64, 0) {
                // Un proceso detenido no puede quedarse colgado al soltarlo:
                // se limpia TF y se reanuda, como exige la semántica de ptrace.
                let _ = process::cont_process(pid as u64, false);
                0
            } else {
                -3 // -ESRCH
            }
        }
        // [CICLO v6.x] PEEK/POKE operan sobre el espacio de direcciones del
        // PROCESO OBJETIVO (antes leían el del llamador, lo que impedía
        // depurar de verdad). La escritura atraviesa el bit de sólo lectura
        // del PTE: es la semántica que necesita un depurador para instalar
        // puntos de parada en la página de código.
        PTRACE_PEEKTEXT | PTRACE_PEEKDATA => {
            if addr == 0 { return -14; }
            let target_cr3 = if pid == 0 {
                process::current_process().map(|p| p.cr3)
            } else {
                process::get_process_cr3(pid as u64)
            };
            let Some(cr3) = target_cr3 else { return -3; }; // -ESRCH
            let mut bytes = [0u8; 8];
            if paging::read_remote(cr3, addr, &mut bytes) {
                if data != 0 {
                    // Convención depurador: con puntero de salida, el valor
                    // viaja por `data` y el retorno sólo indica éxito (0) o
                    // error, evitando ambigüedad con valores negativos.
                    if paging::copy_to_user(data, &bytes).is_ok() { 0 } else { -14 }
                } else {
                    i64::from_ne_bytes(bytes)
                }
            } else {
                -14 // -EFAULT
            }
        }
        PTRACE_POKETEXT | PTRACE_POKEDATA => {
            if addr == 0 { return -14; }
            let target_cr3 = if pid == 0 {
                process::current_process().map(|p| p.cr3)
            } else {
                process::get_process_cr3(pid as u64)
            };
            let Some(cr3) = target_cr3 else { return -3; };
            let bytes = (data as u64).to_ne_bytes();
            if paging::write_remote(cr3, addr, &bytes) { 0 } else { -14 }
        }
        // [CICLO v6.x] Vigilancia por hardware (DR0-DR3): el proceso objetivo
        // queda armado y el kernel carga los registros en cada cambio de
        // contexto. `pid == 0` designa al propio invocador.
        PTRACE_SETBPCOND => {
            // Especificación de la condición: reg (u32) | op (u32) | valor (u64)
            let mut spec = [0u8; 16];
            if data == 0 || paging::copy_from_user(&mut spec, data, 16).is_err() {
                return -14; // -EFAULT
            }
            let reg = u32::from_le_bytes([spec[0], spec[1], spec[2], spec[3]]) as u8;
            let op = u32::from_le_bytes([spec[4], spec[5], spec[6], spec[7]]) as u8;
            let valor = u64::from_le_bytes([spec[8], spec[9], spec[10], spec[11], spec[12], spec[13], spec[14], spec[15]]);
            process::set_bp_cond(pid as u64, addr as u64, reg, op, valor)
        }
        PTRACE_SETBPCOND2 => {
            let mut spec = [0u8; 40];
            if data == 0 || paging::copy_from_user(&mut spec, data, 40).is_err() {
                return -14; // -EFAULT
            }
            let u32at = |o: usize| u32::from_le_bytes([spec[o], spec[o + 1], spec[o + 2], spec[o + 3]]) as u8;
            let u64at = |o: usize| u64::from_le_bytes([spec[o], spec[o + 1], spec[o + 2], spec[o + 3], spec[o + 4], spec[o + 5], spec[o + 6], spec[o + 7]]);
            // El layout debe coincidir byte a byte con `portix_rt::BpCond2`
            // (repr(C)): n(0), _pad(4), reg(8), op(12), value(16), reg2(24), op2(28),
            // value2(32). Un desajuste aquí produce condiciones con registros y
            // valores equivocados, que es un fallo silencioso: se evaluaba `rax`
            // con una constante basura en lugar de `rdi` con la del programa.
            process::set_bp_cond2(
                pid as u64, addr as u64,
                u32at(8), u32at(12), u64at(16),
                u32at(24), u32at(28), u64at(32),
            )
        }
        PTRACE_SETBPCONDN => {
            let mut spec = [0u8; 72];
            if data == 0 || paging::copy_from_user(&mut spec, data, 72).is_err() {
                return -14; // -EFAULT
            }
            let u32at = |o: usize| u32::from_le_bytes([spec[o], spec[o + 1], spec[o + 2], spec[o + 3]]) as u8;
            let u64at = |o: usize| u64::from_le_bytes([spec[o], spec[o + 1], spec[o + 2], spec[o + 3], spec[o + 4], spec[o + 5], spec[o + 6], spec[o + 7]]);
            let n = u32at(0);
            let g2 = u32at(4);
            let mut cl = [(0u8, 0u8, 0u64); 4];
            for c in 0..4usize {
                let base = 8 + c * 16;
                cl[c] = (u32at(base), u32at(base + 4), u64at(base + 8));
            }
            process::set_bp_condn(pid as u64, addr as u64, n, g2, &cl)
        }
        PTRACE_SETBPCIGNORE => {
            let n = (data & 0xFFFF_FFFF) as u32;
            process::set_bp_cond_ignore(pid as u64, addr as u64, n)
        }
        PTRACE_GETBPCOND => {
            let (hits, skips, ignore, n, g2, cl) = match process::bp_cond_stats(pid as u64, addr as u64) {
                Some(v) => v,
                None => return -2, // -ENOENT
            };
            // Estado de 80 bytes: contadores, forma de la condición y sus cuatro
            // cláusulas. El layout coincide con `portix_rt::BpCondStat` (repr(C)).
            let mut out = [0u8; 80];
            out[0..4].copy_from_slice(&hits.to_le_bytes());
            out[4..8].copy_from_slice(&skips.to_le_bytes());
            out[8..12].copy_from_slice(&ignore.to_le_bytes());
            out[12] = n;
            out[13] = g2;
            for c in 0..4usize {
                let base = 16 + c * 16;
                out[base..base + 4].copy_from_slice(&(cl[c].0 as u32).to_le_bytes());
                out[base + 4..base + 8].copy_from_slice(&(cl[c].1 as u32).to_le_bytes());
                out[base + 8..base + 16].copy_from_slice(&cl[c].2.to_le_bytes());
            }
            if paging::copy_to_user(data, &out).is_err() {
                return -14;
            }
            0
        }
        PTRACE_CLEARBPCOND => process::clear_bp_cond(pid as u64, addr as u64),
        PTRACE_SETWATCHPOINT => {
            let target = if pid <= 0 { cur_pid } else { pid as u64 };
            let len_code = (data & 0xFF) as u8;
            let rw = ((data >> 8) & 0xFF) as u8;
            process::set_watchpoint(target, addr as u64, len_code, rw)
        }
        PTRACE_GETTRAPINFO => {
            let target = if pid <= 0 { cur_pid } else { pid as u64 };
            if data == 0 || !verify_user_buffer(data, 40) { return -14; }
            let Some((dr6, slot, vaddr, lcode, rip)) = process::trap_info(target) else { return -3 };
            let mut buf = alloc::vec::Vec::with_capacity(40);
            buf.extend_from_slice(&dr6.to_ne_bytes());
            buf.extend_from_slice(&(slot as u64).to_ne_bytes());
            buf.extend_from_slice(&vaddr.to_ne_bytes());
            buf.extend_from_slice(&(lcode as u64).to_ne_bytes());
            buf.extend_from_slice(&rip.to_ne_bytes());
            let _ = paging::copy_to_user(data, &buf);
            if slot == 0xFF { 0 } else { 1 }
        }
        PTRACE_CLEARWATCHPOINTS => {
            let target = if pid <= 0 { cur_pid } else { pid as u64 };
            if process::clear_watchpoints(target) { 0 } else { -3 }
        }
        PTRACE_GETWATCHPOINTS => {
            let target = if pid <= 0 { cur_pid } else { pid as u64 };
            let (mut n, mut addrs, mut lens, mut rws) = process::get_watchpoints(target);
            // Si el objetivo es el proceso que está en el CPU, la respuesta se
            // lee de los registros de depuración REALES (DR0-DR3/DR7), no de las
            // estructuras del kernel: el depurador comprueba así el hardware.
            if target == cur_pid {
                let snap = crate::arch::debug_regs::snapshot();
                let d = [snap.0, snap.1, snap.2, snap.3];
                let dr7 = snap.5;
                let (mut cnt, mut a2, mut l2, mut r2) = (0usize, [0u64; 4], [0u8; 4], [0u8; 4]);
                for i in 0..4 {
                    if (dr7 >> (2 * i)) & 1 == 1 {
                        a2[cnt] = d[i];
                        r2[cnt] = ((dr7 >> (16 + 4 * i)) & 3) as u8;
                        l2[cnt] = ((dr7 >> (18 + 4 * i)) & 3) as u8;
                        cnt += 1;
                    }
                }
                n = cnt; addrs = a2; lens = l2; rws = r2;
            }
            if data == 0 || !verify_user_buffer(data, 1 + 4 * 8 + 4 + 4) { return -14; }
            let mut buf = alloc::vec::Vec::new();
            buf.push(n as u8);
            for i in 0..4 { buf.extend_from_slice(&addrs[i].to_ne_bytes()); }
            for i in 0..4 { buf.push(lens[i]); }
            for i in 0..4 { buf.push(rws[i]); }
            let _ = paging::copy_to_user(data, &buf);
            n as i64
        }
        // [CICLO v6.x] Reanudación real: el proceso detenido vuelve a ser
        // planificable y su contexto (guardado en el kernel stack) se restaura
        // en el siguiente cambio de contexto.
        PTRACE_CONT => {
            if pid <= 0 { return -22; }
            if process::cont_process(pid as u64, false) { 0 } else { -3 }
        }
        PTRACE_KILL => {
            if pid <= 0 { return -22; }
            sys_kill(pid as u64, 9) as i64
        }
        // [CICLO v6.x] Paso a paso real: la bandera TF del frame de retorno
        // hace que el procesador entregue #DB tras UNA instrucción, lo que
        // detiene otra vez al objetivo y notifica al trazador.
        PTRACE_SINGLESTEP => {
            if pid <= 0 { return -22; }
            if process::cont_process(pid as u64, true) { 0 } else { -3 }
        }
        PTRACE_GETREGS => {
            if data == 0 || !verify_user_buffer(data, core::mem::size_of::<process::UserRegs>()) {
                return -14;
            }
            let target_pid = if pid == 0 { cur_pid } else { pid as u64 };
            if let Some(regs) = process::get_process_regs(target_pid) {
                let bytes = unsafe {
                    core::slice::from_raw_parts(&regs as *const process::UserRegs as *const u8, core::mem::size_of::<process::UserRegs>())
                };
                if paging::copy_to_user(data, bytes).is_ok() {
                    0
                } else {
                    -14
                }
            } else {
                -3 // -ESRCH
            }
        }
        PTRACE_SETREGS => {
            if data == 0 || !verify_user_buffer(data, core::mem::size_of::<process::UserRegs>()) {
                return -14;
            }
            let target_pid = if pid == 0 { cur_pid } else { pid as u64 };
            let mut regs = process::UserRegs::default();
            let bytes = unsafe {
                core::slice::from_raw_parts_mut(&mut regs as *mut process::UserRegs as *mut u8, core::mem::size_of::<process::UserRegs>())
            };
            if paging::copy_from_user(bytes, data, core::mem::size_of::<process::UserRegs>()).is_ok() {
                if process::set_process_regs(target_pid, &regs) {
                    0
                } else {
                    -3
                }
            } else {
                -14
            }
        }
        _ => -22, // -EINVAL
    }
}

fn sys_mount(
    _dev_name_ptr: usize,
    dir_name_ptr: usize,
    type_ptr: usize,
    _flags: u64,
    _data_ptr: usize,
) -> i32 {
    let mut dir_buf = [0u8; 128];
    let mut type_buf = [0u8; 64];

    if dir_name_ptr == 0 || paging::copy_from_user(&mut dir_buf, dir_name_ptr, 128).is_err() {
        return -14;
    }
    if type_ptr != 0 {
        let _ = paging::copy_from_user(&mut type_buf, type_ptr, 64);
    }

    let dir_len = dir_buf.iter().position(|&b| b == 0).unwrap_or(128);
    let dir = core::str::from_utf8(&dir_buf[..dir_len]).unwrap_or("");
    let type_len = type_buf.iter().position(|&b| b == 0).unwrap_or(64);
    let fstype_str = core::str::from_utf8(&type_buf[..type_len]).unwrap_or("ramfs");

    if dir.is_empty() { return -2; /* -ENOENT */ }

    let fs_type = match fstype_str {
        "ramfs" | "tmpfs" => vfs::FsType::RamFs,
        "fat32" | "vfat"  => vfs::FsType::Fat32,
        "ext2"  | "ext4"  => vfs::FsType::Ext2,
        _ => vfs::FsType::RamFs,
    };

    if vfs::mount(dir, fs_type) {
        0
    } else {
        -16 // -EBUSY
    }
}

fn sys_umount2(target_ptr: usize, _flags: i32) -> i32 {
    if target_ptr == 0 { return -14; }
    let mut tgt_buf = [0u8; 128];
    if paging::copy_from_user(&mut tgt_buf, target_ptr, 128).is_err() {
        return -14;
    }
    let tgt_len = tgt_buf.iter().position(|&b| b == 0).unwrap_or(128);
    let target = core::str::from_utf8(&tgt_buf[..tgt_len]).unwrap_or("");
    if target.is_empty() || target == "/" {
        return -16; // -EBUSY: cannot unmount root
    }

    if vfs::unmount(target) {
        0
    } else {
        -22 // -EINVAL: not mounted
    }
}
