// ── Syscall Types, Structures and Constants ──────────────────────────────

use crate::arch::Spinlock;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct MqAttr {
    pub mq_flags: i64,
    pub mq_maxmsg: i64,
    pub mq_msgsize: i64,
    pub mq_curmsgs: i64,
    pub __pad: [i64; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct UserDesc {
    pub entry_number: u32,
    pub base_addr: u32,
    pub limit: u32,
    pub seg_32bit: u32,
    pub contents: u32,
    pub read_exec_only: u32,
    pub limit_in_pages: u32,
    pub seg_not_present: u32,
    pub useable: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct FileHandle {
    pub handle_bytes: u32,
    pub handle_type: i32,
    pub f_handle: [u8; 16],
}

pub const FAN_CLOEXEC: u32 = 0x00000001;
pub const FAN_NONBLOCK: u32 = 0x00000002;
pub const FAN_CLASS_NOTIF: u32 = 0x00000000;
pub const FAN_CLASS_CONTENT: u32 = 0x00000004;
pub const FAN_CLASS_PRE_CONTENT: u32 = 0x00000008;
pub const FAN_MARK_ADD: u32 = 0x00000001;
pub const FAN_MARK_REMOVE: u32 = 0x00000002;
pub const FAN_MARK_DONT_FOLLOW: u32 = 0x00000004;
pub const FAN_MARK_ONLYDIR: u32 = 0x00000008;
pub const FAN_MARK_MOUNT: u32 = 0x00000010;
pub const FAN_MARK_FILESYSTEM: u32 = 0x00000100;
pub const FAN_ACCESS: u64 = 0x00000001;
pub const FAN_MODIFY: u64 = 0x00000002;
pub const FAN_CLOSE_WRITE: u64 = 0x00000008;
pub const FAN_CLOSE_NOWRITE: u64 = 0x00000010;
pub const FAN_OPEN: u64 = 0x00000020;
pub const MODULE_INIT_IGNORE_MODVERSIONS: i32 = 1;
pub const MODULE_INIT_IGNORE_VERMAGIC: i32 = 2;

pub const PKEY_DISABLE_ACCESS: i32 = 0x1;
pub const PKEY_DISABLE_WRITE:  i32 = 0x2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Rseq {
    pub cpu_id_start: u32,
    pub cpu_id: u32,
    pub rseq_cs: u64,
    pub flags: u32,
    pub pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringSqringOffsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub flags: u32,
    pub dropped: u32,
    pub array: u32,
    pub resv1: u32,
    pub user_addr: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringCqringOffsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub overflow: u32,
    pub cqes: u32,
    pub flags: u32,
    pub resv1: u32,
    pub user_addr: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringParams {
    pub sq_entries: u32,
    pub cq_entries: u32,
    pub flags: u32,
    pub sq_thread_cpu: u32,
    pub sq_thread_idle: u32,
    pub features: u32,
    pub wq_fd: u32,
    pub resv: [u32; 3],
    pub sq_off: IoUringSqringOffsets,
    pub cq_off: IoUringCqringOffsets,
}

pub const UFFD_USER_MODE_ONLY: u32 = 1 << 0;

pub const SHUT_RD: i32 = 0;
pub const SHUT_WR: i32 = 1;
pub const SHUT_RDWR: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FutexWaitv {
    pub val: u64,
    pub uaddr: usize,
    pub flags: u32,
    pub __reserved: u32,
}

pub const MLOCK_ONFAULT: i32 = 1;

pub const IOPRIO_WHO_PROCESS: i32 = 1;
pub const IOPRIO_WHO_PGRP: i32 = 2;
pub const IOPRIO_WHO_USER: i32 = 3;

pub const IOPRIO_CLASS_NONE: i32 = 0;
pub const IOPRIO_CLASS_RT: i32 = 1;
pub const IOPRIO_CLASS_BE: i32 = 2;
pub const IOPRIO_CLASS_IDLE: i32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RobustListHead {
    pub list: usize,
    pub futex_offset: isize,
    pub list_op_pending: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MsgHdr {
    pub msg_name: usize,
    pub msg_namelen: u32,
    pub msg_iov: usize,
    pub msg_iovlen: usize,
    pub msg_control: usize,
    pub msg_controllen: usize,
    pub msg_flags: i32,
}

pub const IPC_CREAT: i32 = 0o1000;
pub const IPC_EXCL: i32 = 0o2000;
pub const IPC_RMID: i32 = 0;
pub const IPC_SET: i32 = 1;
pub const IPC_STAT: i32 = 2;

pub const SEM_GETVAL: i32 = 12;
pub const SEM_SETVAL: i32 = 16;
pub const SEM_GETALL: i32 = 13;
pub const SEM_SETALL: i32 = 17;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Sembuf {
    pub sem_num: u16,
    pub sem_op: i16,
    pub sem_flg: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ShmidDs {
    pub shm_segsz: usize,
    pub shm_atime: i64,
    pub shm_dtime: i64,
    pub shm_ctime: i64,
    pub shm_cpid: u32,
    pub shm_lpid: u32,
    pub shm_nattch: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MsqidDs {
    pub msg_stime: i64,
    pub msg_rtime: i64,
    pub msg_ctime: i64,
    pub msg_qnum: u64,
    pub msg_qbytes: u64,
    pub msg_lspid: u32,
    pub msg_lrpid: u32,
}

pub const LINUX_CAPABILITY_VERSION_1: u32 = 0x19980330;
pub const LINUX_CAPABILITY_VERSION_2: u32 = 0x20071026;
pub const LINUX_CAPABILITY_VERSION_3: u32 = 0x20080522;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CapUserHeader {
    pub version: u32,
    pub pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CapUserData {
    pub effective: u32,
    pub permitted: u32,
    pub inheritable: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SchedAttr {
    pub size: u32,
    pub sched_policy: u32,
    pub sched_flags: u64,
    pub sched_nice: i32,
    pub sched_priority: u32,
    pub sched_runtime: u64,
    pub sched_deadline: u64,
    pub sched_period: u64,
}

pub const FSOPEN_CLOEXEC: u32 = 0x01;
pub const FSPICK_CLOEXEC: u32 = 0x01;
pub const FSPICK_SYMLINK_NOFOLLOW: u32 = 0x02;
pub const FSPICK_NO_AUTOMOUNT: u32 = 0x04;
pub const FSPICK_EMPTY_PATH: u32 = 0x08;

pub const FSCONFIG_SET_FLAG: u32 = 0;
pub const FSCONFIG_SET_STRING: u32 = 1;
pub const FSCONFIG_SET_BINARY: u32 = 2;
pub const FSCONFIG_SET_PATH: u32 = 3;
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
pub const FSCONFIG_SET_FD: u32 = 5;
pub const FSCONFIG_CMD_CREATE: u32 = 6;
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LandlockRulesetAttr {
    pub handled_access_fs: u64,
    pub handled_access_net: u64,
}

pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

pub const SECCOMP_SET_MODE_STRICT: u32 = 0;
pub const SECCOMP_SET_MODE_FILTER: u32 = 1;
pub const SECCOMP_GET_ACTION_AVAIL: u32 = 2;
pub const SECCOMP_GET_NOTIF_SIZES: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MntIdReq {
    pub size: u32,
    pub spare: u32,
    pub mnt_id: u64,
    pub param: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Statmount {
    pub size: u32,
    pub mnt_id: u64,
    pub mnt_parent_id: u64,
    pub fs_type: u64,
    pub mnt_root: u64,
    pub mnt_point: u64,
    pub mask: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenHow {
    pub flags: u64,
    pub mode: u64,
    pub resolve: u64,
}

pub const RESOLVE_NO_XDEV: u64 = 0x01;
pub const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
pub const RESOLVE_NO_SYMLINKS: u64 = 0x04;
pub const RESOLVE_BENEATH: u64 = 0x08;
pub const RESOLVE_IN_ROOT: u64 = 0x10;
pub const RESOLVE_CACHED: u64 = 0x20;

pub const MPOL_DEFAULT: i32 = 0;
pub const MPOL_PREFERRED: i32 = 1;
pub const MPOL_BIND: i32 = 2;
pub const MPOL_INTERLEAVE: i32 = 3;
pub const MPOL_LOCAL: i32 = 4;

pub const KCMP_FILE: i32 = 0;
pub const KCMP_VM: i32 = 1;
pub const KCMP_FILES: i32 = 2;
pub const KCMP_FS: i32 = 3;
pub const KCMP_SIGHAND: i32 = 4;

pub const SYNC_FILE_RANGE_WAIT_BEFORE: u32 = 1;
pub const SYNC_FILE_RANGE_WRITE: u32 = 2;
pub const SYNC_FILE_RANGE_WAIT_AFTER: u32 = 4;

pub const MS_ASYNC: i32 = 1;
pub const MS_INVALIDATE: i32 = 2;
pub const MS_SYNC: i32 = 4;

pub const MADV_NORMAL: i32 = 0;
pub const MADV_RANDOM: i32 = 1;
pub const MADV_SEQUENTIAL: i32 = 2;
pub const MADV_WILLNEED: i32 = 3;
pub const MADV_DONTNEED: i32 = 4;
pub const MADV_FREE: i32 = 8;
pub const MADV_REMOVE: i32 = 9;
pub const MADV_DONTFORK: i32 = 10;
pub const MADV_DOFORK: i32 = 11;

pub const MCL_CURRENT: i32 = 1;
pub const MCL_FUTURE: i32 = 2;
pub const MCL_ONFAULT: i32 = 4;

pub const RENAME_NOREPLACE: u32 = 1;
pub const RENAME_EXCHANGE: u32 = 2;
pub const RENAME_WHITEOUT: u32 = 4;

pub const CLOSE_RANGE_UNSHARE: u32 = 2;
pub const CLOSE_RANGE_CLOEXEC: u32 = 4;

pub const PIDFD_NONBLOCK: u32 = 0x800;

pub const MREMAP_MAYMOVE: i32 = 1;
pub const MREMAP_FIXED: i32 = 2;

pub const STATX_ALL: u32 = 0x00000fff;
pub const STATX_BASIC_STATS: u32 = 0x000007ff;
pub const STATX_TYPE: u32 = 0x00000001;
pub const STATX_MODE: u32 = 0x00000002;
pub const STATX_NLINK: u32 = 0x00000004;
pub const STATX_UID: u32 = 0x00000008;
pub const STATX_GID: u32 = 0x00000010;
pub const STATX_ATIME: u32 = 0x00000020;
pub const STATX_MTIME: u32 = 0x00000040;
pub const STATX_CTIME: u32 = 0x00000080;
pub const STATX_INO: u32 = 0x00000100;
pub const STATX_SIZE: u32 = 0x00000200;
pub const STATX_BLOCKS: u32 = 0x00000400;
pub const STATX_BTIME: u32 = 0x00000800;

pub const TFD_CLOEXEC: i32 = 0x80000;
pub const TFD_NONBLOCK: i32 = 0x800;
pub const TFD_TIMER_ABSTIME: i32 = 1;

pub const SFD_CLOEXEC: i32 = 0x80000;
pub const SFD_NONBLOCK: i32 = 0x800;

pub const POSIX_FADV_NORMAL: i32 = 0;
pub const POSIX_FADV_RANDOM: i32 = 1;
pub const POSIX_FADV_SEQUENTIAL: i32 = 2;
pub const POSIX_FADV_WILLNEED: i32 = 3;
pub const POSIX_FADV_DONTNEED: i32 = 4;
pub const POSIX_FADV_NOREUSE: i32 = 5;

pub const FALLOC_FL_KEEP_SIZE: i32 = 0x01;

pub const MEMBARRIER_CMD_QUERY: i32 = 0;
pub const MEMBARRIER_CMD_GLOBAL: i32 = 1;
pub const MEMBARRIER_CMD_GLOBAL_EXPEDITED: i32 = 2;
pub const MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED: i32 = 4;
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED: i32 = 8;
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED: i32 = 16;

pub const TIOCGWINSZ: u64 = 0x5413;
pub const TIOCSWINSZ: u64 = 0x5414;
pub const FIONREAD:   u64 = 0x541B;
pub const FIONBIO:    u64 = 0x5421;
pub const TCGETS:     u64 = 0x5401;
pub const TCSETS:     u64 = 0x5402;
pub const TCSETSW:    u64 = 0x5403;
pub const TCSETSF:    u64 = 0x5404;

pub const MFD_CLOEXEC: u32 = 0x0001;
pub const MFD_ALLOW_SEALING: u32 = 0x0002;

pub const SOL_SOCKET: i32 = 1;
pub const SO_DEBUG: i32 = 1;
pub const SO_REUSEADDR: i32 = 2;
pub const SO_TYPE: i32 = 3;
pub const SO_ERROR: i32 = 4;
pub const SO_DONTROUTE: i32 = 5;
pub const SO_BROADCAST: i32 = 6;
pub const SO_SNDBUF: i32 = 7;
pub const SO_RCVBUF: i32 = 8;
pub const SO_KEEPALIVE: i32 = 9;

pub const ITIMER_REAL: i32 = 0;
pub const ITIMER_VIRTUAL: i32 = 1;
pub const ITIMER_PROF: i32 = 2;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 32],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TimeVal {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TimeZone {
    pub tz_minuteswest: i32,
    pub tz_dsttime: i32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ITimerVal {
    pub it_interval: TimeVal,
    pub it_value: TimeVal,
}

pub static HOSTNAME: Spinlock<[u8; 65]> = Spinlock::new(*b"portix-box\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");

pub const FUTEX_WAIT: u32 = 0;
pub const FUTEX_WAKE: u32 = 1;
pub const FUTEX_FD: u32 = 2;
pub const FUTEX_REQUEUE: u32 = 3;
pub const FUTEX_CMP_REQUEUE: u32 = 4;
pub const FUTEX_WAKE_OP: u32 = 5;
pub const FUTEX_LOCK_PI: u32 = 6;
pub const FUTEX_UNLOCK_PI: u32 = 7;
pub const FUTEX_TRYLOCK_PI: u32 = 8;
pub const FUTEX_WAIT_BITSET: u32 = 9;
pub const FUTEX_WAKE_BITSET: u32 = 10;
pub const FUTEX_LOCK_PI2: u32 = 13;
pub const FUTEX_PRIVATE_FLAG: u32 = 128;
pub const FUTEX_CLOCK_REALTIME: u32 = 256;

pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_DEL: i32 = 2;
pub const EPOLL_CTL_MOD: i32 = 3;
pub const EPOLLIN: u32 = 0x0001;
pub const EPOLLPRI: u32 = 0x0002;
pub const EPOLLOUT: u32 = 0x0004;
pub const EPOLLERR: u32 = 0x0008;
pub const EPOLLHUP: u32 = 0x0010;
pub const EPOLLRDHUP: u32 = 0x2000;
pub const EPOLLET: u32 = 1 << 31;
pub const EPOLL_CLOEXEC: i32 = 0x80000;

pub const AT_FDCWD: i32 = -100;
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
pub const AT_EMPTY_PATH: i32 = 0x1000;
pub const AT_REMOVEDIR: i32 = 0x200;
pub const AT_SYMLINK_FOLLOW: i32 = 0x400;
pub const AT_EACCESS: i32 = 0x200;

pub const RUSAGE_SELF: i32 = 0;
pub const RUSAGE_CHILDREN: i32 = -1;
pub const RUSAGE_THREAD: i32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RUsage {
    pub ru_utime: TimeVal,
    pub ru_stime: TimeVal,
    pub ru_maxrss: i64,
    pub ru_ixrss: i64,
    pub ru_idrss: i64,
    pub ru_isrss: i64,
    pub ru_minflt: i64,
    pub ru_majflt: i64,
    pub ru_nswap: i64,
    pub ru_inblock: i64,
    pub ru_oublock: i64,
    pub ru_msgsnd: i64,
    pub ru_msgrcv: i64,
    pub ru_nsignals: i64,
    pub ru_nvcsw: i64,
    pub ru_nivcsw: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RLimit64 {
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct ITimerSpec {
    pub it_interval: Timespec,
    pub it_value: Timespec,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct SignalFdSigInfo {
    pub ssi_signo: u32,
    pub ssi_errno: i32,
    pub ssi_code: i32,
    pub ssi_pid: u32,
    pub ssi_uid: u32,
    pub ssi_fd: i32,
    pub ssi_tid: u32,
    pub ssi_band: u32,
    pub ssi_overrun: u32,
    pub ssi_trapno: u32,
    pub ssi_status: i32,
    pub ssi_int: i32,
    pub ssi_ptr: u64,
    pub ssi_utime: u64,
    pub ssi_stime: u64,
    pub ssi_addr: u64,
    pub ssi_addr_lsb: u16,
    pub _pad2: u16,
    pub ssi_syscall: i32,
    pub ssi_call_addr: u64,
    pub ssi_arch: u32,
    pub _pad: [u8; 28],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct SysInfo {
    pub uptime: i64,
    pub loads: [u64; 3],
    pub totalram: u64,
    pub freeram: u64,
    pub sharedram: u64,
    pub bufferram: u64,
    pub totalswap: u64,
    pub freeswap: u64,
    pub procs: u16,
    pub pad: u16,
    pub totalhigh: u64,
    pub freehigh: u64,
    pub mem_unit: u32,
    /// [T-004] Linux x86-64: `char _f[20-2*sizeof(long)-sizeof(int)]` = 0 bytes; la
    /// estructura mide 112 B. Con 16 bytes de relleno el núcleo escribía 128 B y
    /// `sort` de coreutils (glibc) moría con *** stack smashing detected ***.
    pub _reserved: [u8; 0],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct SchedParam {
    pub sched_priority: i32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct StatxTimestamp {
    pub tv_sec: i64,
    pub tv_nsec: u32,
    pub __reserved: i32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct Statx {
    pub stx_mask: u32,
    pub stx_blksize: u32,
    pub stx_attributes: u64,
    pub stx_nlink: u32,
    pub stx_uid: u32,
    pub stx_gid: u32,
    pub stx_mode: u16,
    pub __spare0: [u16; 1],
    pub stx_ino: u64,
    pub stx_size: u64,
    pub stx_blocks: u64,
    pub stx_attributes_mask: u64,
    pub stx_atime: StatxTimestamp,
    pub stx_btime: StatxTimestamp,
    pub stx_ctime: StatxTimestamp,
    pub stx_mtime: StatxTimestamp,
    pub stx_rdev_major: u32,
    pub stx_rdev_minor: u32,
    pub stx_dev_major: u32,
    pub stx_dev_minor: u32,
    pub __spare2: [u64; 14],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct IoVec {
    pub iov_base: usize,
    pub iov_len: usize,
}

extern "C" {
    pub fn ring3_exit_trampoline();
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug)]
pub struct SigAction {
    pub sa_handler: usize,
    pub sa_flags: u32,
    pub sa_mask: u64,
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub struct Rlimit {
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

pub const RLIMIT_CPU: i32    = 0;
pub const RLIMIT_FSIZE: i32  = 1;
pub const RLIMIT_DATA: i32   = 2;
pub const RLIMIT_STACK: i32  = 3;
pub const RLIMIT_CORE: i32   = 4;
pub const RLIMIT_NOFILE: i32 = 7;
pub const RLIMIT_AS: i32     = 9;

pub const SIG_BLOCK: i32   = 0;
pub const SIG_UNBLOCK: i32 = 1;
pub const SIG_SETMASK: i32 = 2;

#[repr(C)]
#[derive(Default, Copy, Clone, Debug)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

pub const POLLIN: i16   = 0x0001;
pub const POLLPRI: i16  = 0x0002;
pub const POLLOUT: i16  = 0x0004;
pub const POLLERR: i16  = 0x0008;
pub const POLLHUP: i16  = 0x0010;
pub const POLLNVAL: i16 = 0x0020;

pub const CLOCK_REALTIME: i32           = 0;
pub const CLOCK_MONOTONIC: i32          = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
pub const CLOCK_THREAD_CPUTIME_ID: i32  = 3;
pub const CLOCK_MONOTONIC_RAW: i32      = 4;

#[repr(C)]
#[derive(Default, Copy, Clone, Debug)]
pub struct Statvfs {
    pub f_bsize: u64,
    pub f_frsize: u64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub f_favail: u64,
    pub f_fsid: u64,
    pub f_flag: u64,
    pub f_namemax: u64,
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug)]
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

#[repr(C)]
pub struct SyscallResult(pub u64, pub u64);
