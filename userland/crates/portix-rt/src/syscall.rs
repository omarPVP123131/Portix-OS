use core::arch::asm;

pub const SYS_EXIT:   u64 = 0;
pub const SYS_WRITE:  u64 = 1;
pub const SYS_GETPID: u64 = 2;
pub const SYS_YIELD:  u64 = 3;
pub const SYS_SLEEP:  u64 = 4;
pub const SYS_READ:   u64 = 5;
pub const SYS_OPEN:   u64 = 6;
pub const SYS_CLOSE:  u64 = 7;
pub const SYS_BRK:    u64 = 8;
pub const SYS_MMAP:   u64 = 9;
pub const SYS_GETDIRENTS: u64 = 10;
pub const SYS_EXECVE: u64 = 11;
pub const SYS_DUP2:   u64 = 12;
pub const SYS_UPTIME: u64 = 13;
pub const SYS_SEND:   u64 = 14;
pub const SYS_RECV:   u64 = 15;
pub const SYS_REG_IRQ: u64 = 16;
pub const SYS_BLOCK_READ: u64 = 17;
pub const SYS_IOPORT: u64 = 18;
pub const SYS_IOREAD: u64 = 19;
pub const SYS_IOWRITE: u64 = 20;
pub const SYS_MMAP_DEVICE: u64 = 21;
pub const SYS_FORK:   u64 = 22;
pub const SYS_WAITPID: u64 = 23;
pub const SYS_LSEEK:  u64 = 24;
pub const SYS_STAT:   u64 = 25;
pub const SYS_FSTAT:  u64 = 26;
pub const SYS_KILL:   u64 = 27;
pub const SYS_GETCWD: u64 = 28;
pub const SYS_CHDIR:  u64 = 29;
pub const SYS_UNLINK: u64 = 30;
pub const SYS_PIPE:   u64 = 31;
pub const SYS_TIME:   u64 = 32;
pub const SYS_MKDIR:  u64 = 33;
pub const SYS_SOCKET: u64 = 34;
pub const SYS_BIND:   u64 = 35;
pub const SYS_SENDTO: u64 = 36;
pub const SYS_RECVFROM: u64 = 37;
pub const SYS_INIT_MODULE: u64 = 38;
pub const SYS_DELETE_MODULE: u64 = 39;
pub const SYS_QUERY_MODULE: u64 = 40;
pub const SYS_GET_MOUSE: u64 = 41;
pub const SYS_POLL_KEY:  u64 = 42;
pub const SYS_FB_INFO:   u64 = 353;
pub const SYS_GETUID:    u64 = 43;
pub const SYS_SETUID:    u64 = 44;
pub const SYS_GETGID:    u64 = 45;
pub const SYS_SETGID:    u64 = 46;
pub const SYS_MUNMAP:    u64 = 47;
pub const SYS_DUP:       u64 = 48;
pub const SYS_FCNTL:     u64 = 49;
pub const SYS_GETPPID:   u64 = 50;
pub const SYS_UNAME:     u64 = 51;
pub const SYS_FTRUNCATE: u64 = 52;
pub const SYS_TRUNCATE:  u64 = 53;
pub const SYS_GETEUID:   u64 = 54;
pub const SYS_GETEGID:   u64 = 55;
pub const SYS_RMDIR:     u64 = 56;
pub const SYS_ACCESS:    u64 = 57;
pub const SYS_RENAME:    u64 = 58;
pub const SYS_SETEUID:   u64 = 59;
pub const SYS_SETEGID:   u64 = 60;
pub const SYS_PIPE2:     u64 = 61;
pub const SYS_NANOSLEEP: u64 = 62;
pub const SYS_GETPGRP:   u64 = 63;
pub const SYS_SETPGID:   u64 = 64;
pub const SYS_GETPGID:   u64 = 65;
pub const SYS_SETSID:    u64 = 66;
pub const SYS_GETSID:    u64 = 67;
pub const SYS_UMASK:     u64 = 68;
pub const SYS_POLL:      u64 = 69;
pub const SYS_CLOCK_GETTIME: u64 = 70;
pub const SYS_CLOCK_GETRES:  u64 = 71;
pub const SYS_STATVFS:   u64 = 72;
pub const SYS_SIGACTION: u64 = 73;
pub const SYS_RT_SIGRETURN: u64 = 352; // [Épica 58] restauración de contexto
pub const SYS_SIGPROCMASK: u64 = 74;
pub const SYS_SIGPENDING: u64 = 75;
pub const SYS_GETRLIMIT: u64 = 76;
pub const SYS_SETRLIMIT: u64 = 77;
pub const SYS_GETPRIORITY: u64 = 78;
pub const SYS_SETPRIORITY: u64 = 79;
pub const SYS_READLINK:  u64 = 80;
pub const SYS_SYMLINK:   u64 = 81;
pub const SYS_LSTAT:     u64 = 82;
pub const SYS_TIMES:     u64 = 83;
pub const SYS_GETGROUPS: u64 = 84;
pub const SYS_SETGROUPS: u64 = 85;
pub const SYS_FSYNC:     u64 = 86;
pub const SYS_FDATASYNC:   u64 = 87;
pub const SYS_CHMOD:       u64 = 88;
pub const SYS_FCHMOD:      u64 = 89;
pub const SYS_CHOWN:       u64 = 90;
pub const SYS_FCHOWN:      u64 = 91;
pub const SYS_PAUSE:       u64 = 92;
pub const SYS_ALARM:       u64 = 93;
pub const SYS_SCHED_YIELD: u64 = 94;
pub const SYS_GETRESUID:   u64 = 95;
pub const SYS_SETRESUID:   u64 = 96;
pub const SYS_GETRESGID:   u64 = 97;
pub const SYS_SETRESGID:   u64 = 98;
pub const SYS_LINK:        u64 = 99;
pub const SYS_READV:       u64 = 100;
pub const SYS_WRITEV:      u64 = 101;
pub const SYS_PREAD64:     u64 = 102;
pub const SYS_PWRITE64:    u64 = 103;
pub const SYS_MPROTECT:    u64 = 104;
pub const SYS_SOCKETPAIR:  u64 = 105;
pub const SYS_GETTID:                 u64 = 106;
pub const SYS_SCHED_GETPARAM:         u64 = 107;
pub const SYS_SCHED_SETPARAM:         u64 = 108;
pub const SYS_SCHED_GETSCHEDULER:     u64 = 109;
pub const SYS_SCHED_SETSCHEDULER:     u64 = 110;
pub const SYS_PRCTL:                  u64 = 111;
pub const SYS_SYNC:                   u64 = 112;
pub const SYS_FSTATVFS:               u64 = 113;
pub const SYS_EVENTFD2:               u64 = 114;
pub const SYS_DUP3:                   u64 = 115;
pub const SYS_TGKILL:                 u64 = 116;
pub const SYS_SCHED_GET_PRIORITY_MAX: u64 = 117;
pub const SYS_SCHED_GET_PRIORITY_MIN: u64 = 118;
pub const SYS_FUTEX:                    u64 = 119;
pub const SYS_EPOLL_CREATE1:            u64 = 120;
pub const SYS_EPOLL_CTL:                u64 = 121;
pub const SYS_EPOLL_WAIT:               u64 = 122;
pub const SYS_SYSINFO:                  u64 = 123;
pub const SYS_GETRANDOM:                u64 = 124;
pub const SYS_OPENAT:                   u64 = 125;
pub const SYS_MKDIRAT:                  u64 = 126;
pub const SYS_UNLINKAT:                 u64 = 127;
pub const SYS_READLINKAT:               u64 = 128;
pub const SYS_FSTATAT:                  u64 = 129;
pub const SYS_IOCTL:                    u64 = 130;
pub const SYS_MEMFD_CREATE:             u64 = 131;
pub const SYS_SETSOCKOPT:               u64 = 132;
pub const SYS_GETSOCKOPT:               u64 = 133;
pub const SYS_SETHOSTNAME:              u64 = 134;
pub const SYS_GETHOSTNAME:              u64 = 135;
pub const SYS_GETTIMEOFDAY:             u64 = 136;
pub const SYS_GETITIMER:                u64 = 137;
pub const SYS_SETITIMER:                u64 = 138;
pub const SYS_MKNOD:                    u64 = 139;
pub const SYS_FCHDIR:                   u64 = 140;
pub const SYS_SYMLINKAT:                u64 = 141;
pub const SYS_LINKAT:                   u64 = 142;
pub const SYS_RENAMEAT:                 u64 = 143;
pub const SYS_FCHMODAT:                 u64 = 144;
pub const SYS_FCHOWNAT:                 u64 = 145;
pub const SYS_FACCESSAT:                u64 = 146;
pub const SYS_UTIMENSAT:                u64 = 147;
pub const SYS_GETRUSAGE:                u64 = 148;
pub const SYS_PRLIMIT64:                u64 = 149;
pub const SYS_CLOCK_NANOSLEEP:          u64 = 150;
pub const SYS_TIMERFD_CREATE:           u64 = 151;
pub const SYS_TIMERFD_SETTIME:          u64 = 152;
pub const SYS_TIMERFD_GETTIME:          u64 = 153;
pub const SYS_SIGNALFD4:                u64 = 154;
pub const SYS_POSIX_OPENPT:             u64 = 155;
pub const SYS_PTSNAME_R:                u64 = 156;
pub const SYS_FADVISE64:                u64 = 157;
pub const SYS_FALLOCATE:                u64 = 158;
pub const SYS_MEMBARRIER:               u64 = 159;
pub const SYS_COPY_FILE_RANGE:          u64 = 160;
pub const SYS_SPLICE:                   u64 = 161;
pub const SYS_TEE:                      u64 = 162;
pub const SYS_SYNCFS:                   u64 = 163;
pub const SYS_STATX:                    u64 = 164;
pub const SYS_CLOSE_RANGE:              u64 = 165;
pub const SYS_PIDFD_OPEN:               u64 = 166;
pub const SYS_PIDFD_SEND_SIGNAL:        u64 = 167;
pub const SYS_GETCPU:                   u64 = 168;
pub const SYS_MREMAP:                   u64 = 169;
pub const SYS_PPOLL:                    u64 = 170;
pub const SYS_FSTATFS:                  u64 = 171;
pub const SYS_READAHEAD:                u64 = 172;
pub const SYS_EPOLL_PWAIT:              u64 = 173;
pub const SYS_PREADV:                   u64 = 174;
pub const SYS_PWRITEV:                  u64 = 175;
pub const SYS_FACCESSAT2:               u64 = 176;
pub const SYS_MSYNC:                    u64 = 177;
pub const SYS_MINCORE:                  u64 = 178;
pub const SYS_MADVISE:                  u64 = 179;
pub const SYS_MLOCK:                    u64 = 180;
pub const SYS_MUNLOCK:                  u64 = 181;
pub const SYS_MLOCKALL:                 u64 = 182;
pub const SYS_MUNLOCKALL:               u64 = 183;
pub const SYS_RENAMEAT2:                u64 = 184;
pub const SYS_MEMFD_SECRET:             u64 = 185;
pub const SYS_EPOLL_PWAIT2:             u64 = 186;
pub const SYS_PROCESS_MADVISE:          u64 = 187;
pub const SYS_SCHED_YIELD_POSIX:        u64 = 188;
pub const SYS_SETTIMEOFDAY:             u64 = 189;
pub const SYS_SYNC_FILE_RANGE:          u64 = 190;
pub const SYS_VMSPLICE:                 u64 = 191;
pub const SYS_KCMP:                     u64 = 192;
pub const SYS_PERSONALITY:              u64 = 193;
pub const SYS_SETNS:                    u64 = 194;
pub const SYS_UNSHARE:                  u64 = 195;
pub const SYS_PIVOT_ROOT:               u64 = 196;
pub const SYS_LOOKUP_DCOOKIE:           u64 = 197;
pub const SYS_GETRANDOM2:               u64 = 198;
pub const SYS_SCHED_SETAFFINITY:        u64 = 199;
pub const SYS_SCHED_GETAFFINITY:        u64 = 200;
pub const SYS_CLOCK_SETTIME:            u64 = 201;
pub const SYS_GETDENTS64:               u64 = 202;
pub const SYS_PIDFD_GETFD:              u64 = 203;
pub const SYS_OPENAT2:                  u64 = 204;
pub const SYS_PROCESS_MRELEASE:         u64 = 205;
pub const SYS_SET_MEMPOLICY:            u64 = 206;
pub const SYS_GET_MEMPOLICY:            u64 = 207;
pub const SYS_IO_PGETEVENTS:            u64 = 208;
pub const SYS_FSMOUNT:                  u64 = 209;
pub const SYS_MOVE_MOUNT:               u64 = 210;
pub const SYS_CAPGET:                   u64 = 211;
pub const SYS_CAPSET:                   u64 = 212;
pub const SYS_SCHED_GETATTR:            u64 = 213;
pub const SYS_SCHED_SETATTR:            u64 = 214;
pub const SYS_FSOPEN:                   u64 = 215;
pub const SYS_FSPICK:                   u64 = 216;
pub const SYS_FSCONFIG:                 u64 = 217;
pub const SYS_QUOTACTL:                 u64 = 218;
pub const SYS_LANDLOCK_CREATE_RULESET:  u64 = 219;
pub const SYS_LANDLOCK_ADD_RULE:        u64 = 220;
pub const SYS_LANDLOCK_RESTRICT_SELF:   u64 = 221;
pub const SYS_SECCOMP:                  u64 = 222;
pub const SYS_STATMOUNT:                u64 = 223;
pub const SYS_LISTMOUNT:                u64 = 224;
pub const SYS_SHUTDOWN:                 u64 = 225;
pub const SYS_GETSOCKNAME:              u64 = 226;
pub const SYS_GETPEERNAME:              u64 = 227;
pub const SYS_SENDMSG:                  u64 = 228;
pub const SYS_RECVMSG:                  u64 = 229;
pub const SYS_SHMGET:                   u64 = 230;
pub const SYS_SHMAT:                    u64 = 231;
pub const SYS_SHMDT:                    u64 = 232;
pub const SYS_SHMCTL:                   u64 = 233;
pub const SYS_SEMGET:                   u64 = 234;
pub const SYS_SEMOP:                    u64 = 235;
pub const SYS_SEMCTL:                   u64 = 236;
pub const SYS_MSGGET:                   u64 = 237;
pub const SYS_MSGSND:                   u64 = 238;
pub const SYS_MSGRCV:                   u64 = 239;
pub const SYS_MSGCTL:                   u64 = 240;
pub const SYS_TIMER_CREATE:             u64 = 241;
pub const SYS_TIMER_SETTIME:            u64 = 242;
pub const SYS_TIMER_GETTIME:            u64 = 243;
pub const SYS_TIMER_GETOVERRUN:         u64 = 244;
pub const SYS_TIMER_DELETE:             u64 = 245;
pub const SYS_FUTEX_WAITV:              u64 = 246;
pub const SYS_MLOCK2:                   u64 = 247;
pub const SYS_IOPRIO_GET:               u64 = 248;
pub const SYS_IOPRIO_SET:               u64 = 249;
pub const SYS_SCHED_RR_GET_INTERVAL:    u64 = 250;
pub const SYS_GET_ROBUST_LIST:          u64 = 251;
pub const SYS_SET_ROBUST_LIST:          u64 = 252;
pub const SYS_RESTART_SYSCALL:          u64 = 253;
pub const SYS_KEXEC_LOAD:               u64 = 254;
pub const SYS_KEXEC_FILE_LOAD:          u64 = 255;
pub const SYS_PKEY_MPROTECT:            u64 = 256;
pub const SYS_PKEY_ALLOC:               u64 = 257;
pub const SYS_PKEY_FREE:                u64 = 258;
pub const SYS_RSEQ:                     u64 = 259;
pub const SYS_IO_URING_SETUP:           u64 = 260;
pub const SYS_IO_URING_ENTER:           u64 = 261;
pub const SYS_IO_URING_REGISTER:        u64 = 262;
pub const SYS_PREADV2:                  u64 = 263;
pub const SYS_PWRITEV2:                 u64 = 264;
pub const SYS_SETDOMAINNAME:            u64 = 265;
pub const SYS_SWAPON:                   u64 = 266;
pub const SYS_SWAPOFF:                  u64 = 267;
pub const SYS_IOPERM:                   u64 = 268;
pub const SYS_IOPL:                     u64 = 269;
pub const SYS_USERFAULTFD:              u64 = 270;
pub const SYS_MQ_OPEN:                  u64 = 271;
pub const SYS_MQ_UNLINK:                u64 = 272;
pub const SYS_MQ_TIMEDSEND:             u64 = 273;
pub const SYS_MQ_TIMEDRECEIVE:          u64 = 274;
pub const SYS_MQ_NOTIFY:                u64 = 275;
pub const SYS_MQ_GETSETATTR:            u64 = 276;
pub const SYS_SET_THREAD_AREA:          u64 = 277;
pub const SYS_GET_THREAD_AREA:          u64 = 278;
pub const SYS_SET_TID_ADDRESS:          u64 = 279;
pub const SYS_FANOTIFY_INIT:            u64 = 280;
pub const SYS_FANOTIFY_MARK:            u64 = 281;
pub const SYS_NAME_TO_HANDLE_AT:        u64 = 282;
pub const SYS_OPEN_BY_HANDLE_AT:        u64 = 283;
pub const SYS_FINIT_MODULE:             u64 = 284;
pub const SYS_REMAP_FILE_PAGES:         u64 = 285;
pub const SYS_SENDFILE:                 u64 = 286;
pub const SYS_CONNECT:                  u64 = 287;
pub const SYS_LISTEN:                   u64 = 288;
pub const SYS_ACCEPT:                   u64 = 289;
pub const SYS_ACCEPT4:                  u64 = 290;
pub const SYS_INOTIFY_INIT:             u64 = 291;
pub const SYS_INOTIFY_INIT1:            u64 = 292;
pub const SYS_INOTIFY_ADD_WATCH:        u64 = 293;
pub const SYS_INOTIFY_RM_WATCH:         u64 = 294;
pub const SYS_SETXATTR:                 u64 = 295;
pub const SYS_GETXATTR:                 u64 = 296;
pub const SYS_LISTXATTR:                u64 = 297;
pub const SYS_REMOVEXATTR:              u64 = 298;
pub const SYS_FSETXATTR:                u64 = 299;
pub const SYS_FGETXATTR:                u64 = 300;
pub const SYS_LSETXATTR:                u64 = 301;
pub const SYS_LGETXATTR:                u64 = 302;
pub const SYS_LLISTXATTR:               u64 = 303;
pub const SYS_LREMOVEXATTR:             u64 = 304;
pub const SYS_FLISTXATTR:               u64 = 305;
pub const SYS_FREMOVEXATTR:             u64 = 306;
pub const SYS_MKNODAT:                  u64 = 307;
pub const SYS_FUTIMESAT:                u64 = 308;
pub const SYS_SELECT:                   u64 = 309;
pub const SYS_PSELECT6:                 u64 = 310;
pub const SYS_ADJTIMEX:                 u64 = 311;
pub const SYS_CLOCK_ADJTIME:            u64 = 312;
pub const SYS_MIGRATE_PAGES:            u64 = 313;
pub const SYS_MOVE_PAGES:               u64 = 314;
pub const SYS_QUOTACTL_FD:              u64 = 315;
pub const SYS_EXECVEAT:                 u64 = 316;
pub const SYS_PERF_EVENT_OPEN:          u64 = 317;
pub const SYS_BPF:                      u64 = 318;
pub const SYS_MOUNT_SETATTR:            u64 = 319;
pub const SYS_SET_MEMPOLICY_HOME_NODE:  u64 = 320;
pub const SYS_CACHESTAT:                u64 = 321;
pub const SYS_SIGALTSTACK:              u64 = 322;
pub const SYS_RT_SIGSUSPEND:            u64 = 323;
pub const SYS_RT_SIGPENDING:            u64 = 324;
pub const SYS_RT_SIGQUEUEINFO:          u64 = 325;
pub const SYS_RT_SIGTIMEDWAIT:          u64 = 326;
pub const SYS_ARCH_PRCTL:               u64 = 327;
pub const SYS_FLOCK:                    u64 = 328;
pub const SYS_LCHOWN:                   u64 = 329;
pub const SYS_UTIMES:                   u64 = 330;
pub const SYS_CHROOT:                   u64 = 331;
pub const SYS_GETDOMAINNAME:            u64 = 332;
pub const SYS_EVENTFD:                  u64 = 333;
pub const SYS_RT_TGSIGQUEUEINFO:        u64 = 334;
pub const SYS_RECVMMSG:                 u64 = 335;
pub const SYS_SENDMMSG:                 u64 = 336;
pub const SYS_PROCESS_VM_READV:         u64 = 337;
pub const SYS_PROCESS_VM_WRITEV:        u64 = 338;
pub const SYS_SEMTIMEDOP:               u64 = 339;
pub const SYS_ACCT:                     u64 = 340;
pub const SYS_USTAT:                    u64 = 341;
pub const SYS_SYSLOG:                   u64 = 342;
pub const SYS_VHANGUP:                  u64 = 343;
pub const SYS_MODIFY_LDT:               u64 = 344;
pub const SYS_SETFSUID:                 u64 = 345;
pub const SYS_SETFSGID:                 u64 = 346;
pub const SYS_PTRACE:                   u64 = 347;
pub const SYS_MOUNT:                    u64 = 348;
pub const SYS_UMOUNT2:                  u64 = 349;
pub const SYS_SYMLINK_FULL:             u64 = 350;
pub const SYS_READLINK_FULL:            u64 = 351;
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
pub const AT_REMOVEDIR: i32 = 0x200;
pub const AT_SYMLINK_FOLLOW: i32 = 0x400;
pub const AT_EACCESS: i32 = 0x200;

pub const RUSAGE_SELF: i32 = 0;
pub const RUSAGE_CHILDREN: i32 = -1;
pub const RUSAGE_THREAD: i32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
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
#[derive(Clone, Copy, Debug, Default)]
pub struct RLimit64 {
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ITimerSpec {
    pub it_interval: Timespec,
    pub it_value: Timespec,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
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
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct EpollEvent {
    pub events: u32,
    pub data: u64,
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
    pub _reserved: [u8; 0], // [T-004] 112 B como Linux x86-64
}

pub const PR_SET_PDEATHSIG: i32 = 1;
pub const PR_GET_DUMPABLE:  i32 = 3;
pub const PR_SET_DUMPABLE:  i32 = 4;
pub const PR_SET_NAME:      i32 = 15;
pub const PR_GET_NAME:      i32 = 16;

pub const EFD_SEMAPHORE: i32 = 1;
pub const EFD_NONBLOCK:  i32 = 0x0800;
pub const EFD_CLOEXEC:   i32 = 0x80000;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct SchedParam {
    pub sched_priority: i32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct IoVec {
    pub iov_base: *mut u8,
    pub iov_len: usize,
}

#[macro_export]
macro_rules! syscall {
    ($nr:expr) => {{
        let r: u64;
        core::arch::asm!(
            "int $0x80",
            inout("rax") $nr as u64 => r,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
        r
    }};
    ($nr:expr, $a1:expr) => {{
        let r: u64;
        core::arch::asm!(
            "int $0x80",
            inout("rax") $nr as u64 => r,
            in("rdi") $a1 as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
        r
    }};
    ($nr:expr, $a1:expr, $a2:expr) => {{
        let r: u64;
        core::arch::asm!(
            "int $0x80",
            inout("rax") $nr as u64 => r,
            in("rdi") $a1 as u64,
            in("rsi") $a2 as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
        r
    }};
    ($nr:expr, $a1:expr, $a2:expr, $a3:expr) => {{
        let r: u64;
        core::arch::asm!(
            "int $0x80",
            inout("rax") $nr as u64 => r,
            in("rdi") $a1 as u64,
            in("rsi") $a2 as u64,
            in("rdx") $a3 as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
        r
    }};
    ($nr:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr) => {{
        let r: u64;
        core::arch::asm!(
            "int $0x80",
            inout("rax") $nr as u64 => r,
            in("rdi") $a1 as u64,
            in("rsi") $a2 as u64,
            in("rdx") $a3 as u64,
            in("r10") $a4 as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
        r
    }};
    ($nr:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {{
        let r: u64;
        core::arch::asm!(
            "int $0x80",
            inout("rax") $nr as u64 => r,
            in("rdi") $a1 as u64,
            in("rsi") $a2 as u64,
            in("rdx") $a3 as u64,
            in("r10") $a4 as u64,
            in("r8") $a5 as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
        r
    }};
    ($nr:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr, $a6:expr) => {{
        let r: u64;
        core::arch::asm!(
            "int $0x80",
            inout("rax") $nr as u64 => r,
            in("rdi") $a1 as u64,
            in("rsi") $a2 as u64,
            in("rdx") $a3 as u64,
            in("r10") $a4 as u64,
            in("r8") $a5 as u64,
            in("r9") $a6 as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
        r
    }};
}

pub fn sys_exit(code: i32) -> ! {
    unsafe { syscall!(SYS_EXIT, code as u64); }
    loop { unsafe { asm!("hlt", options(nostack)); } }
}

pub fn sys_write(fd: i32, buf: *const u8, len: usize) -> i64 {
    unsafe { syscall!(SYS_WRITE, fd as u64, buf as u64, len as u64) as i64 }
}

pub fn sys_read(fd: i32, buf: *mut u8, len: usize) -> i64 {
    unsafe { syscall!(SYS_READ, fd as u64, buf as u64, len as u64) as i64 }
}

pub fn sys_open(path: *const u8, flags: u32) -> i64 {
    unsafe { syscall!(SYS_OPEN, path as u64, flags as u64) as i64 }
}

pub fn sys_close(fd: i32) -> i64 {
    unsafe { syscall!(SYS_CLOSE, fd as u64) as i64 }
}

pub fn sys_getpid() -> u64 {
    unsafe { syscall!(SYS_GETPID) }
}

pub fn sys_yield() {
    unsafe { syscall!(SYS_YIELD); }
}

pub fn sys_sleep(ticks: u64) {
    unsafe { syscall!(SYS_SLEEP, ticks); }
}

pub fn sys_brk(addr: usize) -> i64 {
    unsafe { syscall!(SYS_BRK, addr as u64) as i64 }
}

// [CICLO 50 / T1b] mmap siempre viaja con los 6 argumentos de la ABI
// (fd=-1 y offset=0 para los anónimos): r8/r9 nunca quedan con basura.
pub fn sys_mmap(addr: usize, len: usize, prot: u32, flags: u32) -> i64 {
    unsafe { syscall!(SYS_MMAP, addr as u64, len as u64, prot as u64, flags as u64, (-1i64) as u64, 0u64) as i64 }
}

pub fn sys_mmap_file(addr: usize, len: usize, prot: u32, flags: u32, fd: i32) -> i64 {
    unsafe { syscall!(SYS_MMAP, addr as u64, len as u64, prot as u64, flags as u64, fd as i64 as u64, 0u64) as i64 }
}

/// mmap con offset de fichero (múltiplo de página), como usan los cargadores ELF.
pub fn sys_mmap_off(addr: usize, len: usize, prot: u32, flags: u32, fd: i32, offset: i64) -> i64 {
    unsafe { syscall!(SYS_MMAP, addr as u64, len as u64, prot as u64, flags as u64, fd as i64 as u64, offset as u64) as i64 }
}

pub fn sys_munmap(addr: usize, len: usize) -> i32 {
    unsafe { syscall!(SYS_MUNMAP, addr as u64, len as u64) as i32 }
}

pub fn sys_getdents(path: *const u8, buf: *mut u8, count: usize) -> i64 {
    unsafe { syscall!(SYS_GETDIRENTS, path as u64, buf as u64, count as u64) as i64 }
}

pub fn sys_execve(path: *const u8, argv: *const u8, envp: *const u8) -> i64 {
    unsafe { syscall!(SYS_EXECVE, path as u64, argv as u64, envp as u64) as i64 }
}

pub fn sys_dup2(oldfd: i32, newfd: i32) -> i64 {
    unsafe { syscall!(SYS_DUP2, oldfd as u64, newfd as u64) as i64 }
}

pub fn sys_dup(oldfd: i32) -> i64 {
    unsafe { syscall!(SYS_DUP, oldfd as u64) as i64 }
}

pub fn sys_uptime() -> u64 {
    unsafe { syscall!(SYS_UPTIME) }
}

pub fn sys_fork() -> i64 {
    unsafe { syscall!(SYS_FORK) as i64 }
}

pub fn sys_waitpid(pid: i32, status: *mut i32, options: i32) -> i64 {
    unsafe { syscall!(SYS_WAITPID, pid as u64, status as u64, options as u64) as i64 }
}

pub fn sys_lseek(fd: i32, offset: i64, whence: i32) -> i64 {
    unsafe { syscall!(SYS_LSEEK, fd as u64, offset as u64, whence as u64) as i64 }
}

pub fn sys_stat(path: *const u8, statbuf: *mut u8) -> i64 {
    unsafe { syscall!(SYS_STAT, path as u64, statbuf as u64) as i64 }
}

pub fn sys_fstat(fd: i32, statbuf: *mut u8) -> i64 {
    unsafe { syscall!(SYS_FSTAT, fd as u64, statbuf as u64) as i64 }
}

pub fn sys_kill(pid: i32, sig: i32) -> i64 {
    unsafe { syscall!(SYS_KILL, pid as u64, sig as u64) as i64 }
}

pub fn sys_getcwd(buf: *mut u8, size: usize) -> i64 {
    unsafe { syscall!(SYS_GETCWD, buf as u64, size as u64) as i64 }
}

pub fn sys_chdir(path: *const u8) -> i64 {
    unsafe { syscall!(SYS_CHDIR, path as u64) as i64 }
}

pub fn sys_unlink(path: *const u8) -> i64 {
    unsafe { syscall!(SYS_UNLINK, path as u64) as i64 }
}

pub fn sys_pipe(pipefd: *mut i32) -> i64 {
    unsafe { syscall!(SYS_PIPE, pipefd as u64) as i64 }
}

pub fn sys_time(tloc: *mut i64) -> i64 {
    unsafe { syscall!(SYS_TIME, tloc as u64) as i64 }
}

pub fn sys_mkdir(path: *const u8, mode: u32) -> i64 {
    unsafe { syscall!(SYS_MKDIR, path as u64, mode as u64) as i64 }
}

pub fn sys_socket(domain: i32, sock_type: i32, protocol: i32) -> i64 {
    unsafe { syscall!(SYS_SOCKET, domain as u64, sock_type as u64, protocol as u64) as i64 }
}

pub fn sys_bind(sockfd: i32, addr: *const u8, addrlen: u32) -> i64 {
    unsafe { syscall!(SYS_BIND, sockfd as u64, addr as u64, addrlen as u64) as i64 }
}

pub fn sys_sendto(sockfd: i32, buf: *const u8, len: usize, flags: i32, dest_addr: *const u8, addrlen: u32) -> i64 {
    unsafe { syscall!(SYS_SENDTO, sockfd as u64, buf as u64, len as u64, flags as u64, dest_addr as u64, addrlen as u64) as i64 }
}

pub fn sys_recvfrom(sockfd: i32, buf: *mut u8, len: usize, flags: i32, src_addr: *mut u8, addrlen: *mut u32) -> i64 {
    unsafe { syscall!(SYS_RECVFROM, sockfd as u64, buf as u64, len as u64, flags as u64, src_addr as u64, addrlen as u64) as i64 }
}

pub fn sys_init_module(image: *const u8, len: usize, params: *const u8) -> i64 {
    unsafe { syscall!(SYS_INIT_MODULE, image as u64, len as u64, params as u64) as i64 }
}

pub fn sys_delete_module(name: *const u8, flags: i32) -> i64 {
    unsafe { syscall!(SYS_DELETE_MODULE, name as u64, flags as u64) as i64 }
}

pub fn sys_query_module(buf: *mut u8, len: usize) -> i64 {
    unsafe { syscall!(SYS_QUERY_MODULE, buf as u64, len as u64) as i64 }
}

pub fn sys_mmap_device(phys: usize, size: usize) -> i64 {
    unsafe { syscall!(SYS_MMAP_DEVICE, phys as u64, size as u64) as i64 }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct MouseEvent {
    pub x: i32,
    pub y: i32,
    pub buttons: u8,
    pub scroll_delta: i32,
}

pub fn sys_get_mouse(ev: &mut MouseEvent) -> i64 {
    unsafe { syscall!(SYS_GET_MOUSE, ev as *mut MouseEvent as u64) as i64 }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FbInfo { pub phys: u64, pub width: u32, pub height: u32, pub bpp: u32, pub pitch: u32 }

/// Modo real del framebuffer lineal (SYS_FB_INFO). Devuelve 0 o -errno.
pub fn sys_fb_info(info: *mut FbInfo) -> i64 {
    unsafe { syscall!(SYS_FB_INFO, info as u64) as i64 }
}

pub fn sys_poll_key() -> i64 {
    unsafe { syscall!(SYS_POLL_KEY) as i64 }
}

pub fn sys_fcntl(fd: i32, cmd: i32, arg: usize) -> i64 {
    unsafe { syscall!(SYS_FCNTL, fd as u64, cmd as u64, arg as u64) as i64 }
}

pub fn sys_getppid() -> u64 {
    unsafe { syscall!(SYS_GETPPID) as u64 }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UtsName {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}

impl Default for UtsName {
    fn default() -> Self {
        Self {
            sysname: [0; 65],
            nodename: [0; 65],
            release: [0; 65],
            version: [0; 65],
            machine: [0; 65],
            domainname: [0; 65],
        }
    }
}

pub fn sys_uname(buf: *mut UtsName) -> i64 {
    unsafe { syscall!(SYS_UNAME, buf as u64) as i64 }
}

pub fn sys_ftruncate(fd: i32, length: i64) -> i64 {
    unsafe { syscall!(SYS_FTRUNCATE, fd as u64, length as u64) as i64 }
}

pub fn sys_truncate(path: *const u8, length: i64) -> i64 {
    unsafe { syscall!(SYS_TRUNCATE, path as u64, length as u64) as i64 }
}

pub fn sys_getuid() -> u64 {
    unsafe { syscall!(SYS_GETUID) as u64 }
}

pub fn sys_getgid() -> u64 {
    unsafe { syscall!(SYS_GETGID) as u64 }
}

pub fn sys_geteuid() -> u64 {
    unsafe { syscall!(SYS_GETEUID) as u64 }
}

pub fn sys_getegid() -> u64 {
    unsafe { syscall!(SYS_GETEGID) as u64 }
}

pub fn sys_rmdir(path: *const u8) -> i64 {
    unsafe { syscall!(SYS_RMDIR, path as u64) as i64 }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelMessage {
    pub src_pid: u64,
    pub dst_pid: u64,
    pub msg_type: u64,
    pub data: [u8; 40],
}

pub fn sys_send(dst_pid: u64, msg_type: u64, data: *const u8, data_len: u64) -> i64 {
    unsafe { syscall!(SYS_SEND, dst_pid, msg_type, data as u64, data_len) as i64 }
}

pub fn sys_recv(buf: *mut u8, len: usize, options: u32) -> i64 {
    unsafe { syscall!(SYS_RECV, buf as u64, len as u64, options as u64) as i64 }
}

pub fn sys_access(path: *const u8, mode: i32) -> i64 {
    unsafe { syscall!(SYS_ACCESS, path as u64, mode as u64) as i64 }
}

pub fn sys_rename(oldpath: *const u8, newpath: *const u8) -> i64 {
    unsafe { syscall!(SYS_RENAME, oldpath as u64, newpath as u64) as i64 }
}

pub fn sys_seteuid(euid: u32) -> i64 {
    unsafe { syscall!(SYS_SETEUID, euid as u64) as i64 }
}

pub fn sys_setegid(egid: u32) -> i64 {
    unsafe { syscall!(SYS_SETEGID, egid as u64) as i64 }
}

pub fn sys_pipe2(pipefd: *mut i32, flags: i32) -> i64 {
    unsafe { syscall!(SYS_PIPE2, pipefd as u64, flags as u64) as i64 }
}

pub fn sys_nanosleep(req: *const Timespec, rem: *mut Timespec) -> i64 {
    unsafe { syscall!(SYS_NANOSLEEP, req as u64, rem as u64) as i64 }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
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

pub fn sys_getpgrp() -> u64 {
    unsafe { syscall!(SYS_GETPGRP) }
}

pub fn sys_setpgid(pid: u64, pgid: u64) -> i64 {
    unsafe { syscall!(SYS_SETPGID, pid, pgid) as i64 }
}

pub fn sys_getpgid(pid: u64) -> i64 {
    unsafe { syscall!(SYS_GETPGID, pid) as i64 }
}

pub fn sys_setsid() -> i64 {
    unsafe { syscall!(SYS_SETSID) as i64 }
}

pub fn sys_getsid(pid: u64) -> i64 {
    unsafe { syscall!(SYS_GETSID, pid) as i64 }
}

pub fn sys_umask(mask: u32) -> u32 {
    unsafe { syscall!(SYS_UMASK, mask as u64) as u32 }
}

pub fn sys_poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32 {
    unsafe { syscall!(SYS_POLL, fds as u64, nfds as u64, timeout as u64) as i32 }
}

pub fn sys_clock_gettime(clk_id: i32, tp: *mut Timespec) -> i32 {
    unsafe { syscall!(SYS_CLOCK_GETTIME, clk_id as u64, tp as u64) as i32 }
}

pub fn sys_clock_getres(clk_id: i32, res: *mut Timespec) -> i32 {
    unsafe { syscall!(SYS_CLOCK_GETRES, clk_id as u64, res as u64) as i32 }
}

pub fn sys_statvfs(path: *const u8, buf: *mut Statvfs) -> i32 {
    unsafe { syscall!(SYS_STATVFS, path as u64, buf as u64) as i32 }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SigAction {
    pub sa_handler: usize,
    pub sa_flags: u32,
    pub sa_mask: u64,
}

pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

// Números canónicos de señal (IEEE Std 1003.1)
pub const SIGHUP: i32    = 1;
pub const SIGINT: i32    = 2;
pub const SIGQUIT: i32   = 3;
pub const SIGILL: i32    = 4;
pub const SIGTRAP: i32   = 5;
pub const SIGABRT: i32   = 6;
pub const SIGBUS: i32    = 7;
pub const SIGFPE: i32    = 8;
pub const SIGKILL: i32   = 9;
pub const SIGUSR1: i32   = 10;
pub const SIGSEGV: i32   = 11;
pub const SIGUSR2: i32   = 12;
pub const SIGPIPE: i32   = 13;
pub const SIGALRM: i32   = 14;
pub const SIGTERM: i32   = 15;
pub const SIGCHLD: i32   = 17;
pub const SIGCONT: i32   = 18;
pub const SIGSTOP: i32   = 19;

/// Dirección del trampolín de retorno de manejadores de señales.
/// El kernel coloca esta dirección como dirección de retorno del manejador:
/// al ejecutar `ret`, el programa salta aquí y el #PF resultante se traduce
/// en `rt_sigreturn`, restaurando el contexto interrumpido.
pub const SIGRETURN_TRAMPOLINE: usize = 0x0000_0000_7FF0_0000;

/// rt_sigreturn explícita: restaura el contexto guardado en `frame_ptr`.
/// El frame debe ser el apuntado por RSP dentro del manejador de señales.
#[inline]
pub fn sys_rt_sigreturn(frame_ptr: usize) -> i64 {
    unsafe { syscall!(SYS_RT_SIGRETURN, frame_ptr as u64) as i64 }
}

/// Devuelve el puntero al `SigFrame` del manejador en ejecución (RSP actual).
#[inline]
pub fn sigframe_ptr() -> usize {
    let rsp: usize;
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags)); }
    rsp
}

pub const SIG_BLOCK: i32   = 0;
pub const SIG_UNBLOCK: i32 = 1;
pub const SIG_SETMASK: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
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

pub fn sys_sigaction(signum: i32, act: *const SigAction, oldact: *mut SigAction) -> i32 {
    unsafe { syscall!(SYS_SIGACTION, signum as u64, act as u64, oldact as u64) as i32 }
}

pub fn sys_sigprocmask(how: i32, set: *const u64, oldset: *mut u64) -> i32 {
    unsafe { syscall!(SYS_SIGPROCMASK, how as u64, set as u64, oldset as u64) as i32 }
}

pub fn sys_sigpending(set: *mut u64) -> i32 {
    unsafe { syscall!(SYS_SIGPENDING, set as u64) as i32 }
}

pub fn sys_getrlimit(resource: i32, rlim: *mut Rlimit) -> i32 {
    unsafe { syscall!(SYS_GETRLIMIT, resource as u64, rlim as u64) as i32 }
}

pub fn sys_setrlimit(resource: i32, rlim: *const Rlimit) -> i32 {
    unsafe { syscall!(SYS_SETRLIMIT, resource as u64, rlim as u64) as i32 }
}

pub fn sys_getpriority(which: i32, who: u64) -> i32 {
    unsafe { syscall!(SYS_GETPRIORITY, which as u64, who as u64) as i32 }
}

pub fn sys_setpriority(which: i32, who: u64, prio: i32) -> i32 {
    unsafe { syscall!(SYS_SETPRIORITY, which as u64, who as u64, prio as u64) as i32 }
}

pub fn sys_readlink(path: *const u8, buf: *mut u8, bufsiz: usize) -> i64 {
    unsafe { syscall!(SYS_READLINK, path as u64, buf as u64, bufsiz as u64) as i64 }
}

pub fn sys_symlink(target: *const u8, linkpath: *const u8) -> i32 {
    unsafe { syscall!(SYS_SYMLINK, target as u64, linkpath as u64) as i32 }
}

pub fn sys_lstat(path: *const u8, buf: *mut crate::fs::Stat) -> i64 {
    unsafe { syscall!(SYS_LSTAT, path as u64, buf as u64) as i64 }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Tms {
    pub tms_utime: i64,
    pub tms_stime: i64,
    pub tms_cutime: i64,
    pub tms_cstime: i64,
}

pub fn sys_times(buf: *mut Tms) -> i64 {
    unsafe { syscall!(SYS_TIMES, buf as u64) as i64 }
}

pub fn sys_getgroups(size: usize, list: *mut u32) -> i64 {
    unsafe { syscall!(SYS_GETGROUPS, size as u64, list as u64) as i64 }
}

pub fn sys_setgroups(size: usize, list: *const u32) -> i64 {
    unsafe { syscall!(SYS_SETGROUPS, size as u64, list as u64) as i64 }
}

pub fn sys_fsync(fd: i32) -> i64 {
    unsafe { syscall!(SYS_FSYNC, fd as u64) as i64 }
}

pub fn sys_fdatasync(fd: i32) -> i64 {
    unsafe { syscall!(SYS_FDATASYNC, fd as u64) as i64 }
}

pub fn sys_chmod(path: *const u8, mode: u32) -> i64 {
    unsafe { syscall!(SYS_CHMOD, path as u64, mode as u64) as i64 }
}

pub fn sys_fchmod(fd: i32, mode: u32) -> i64 {
    unsafe { syscall!(SYS_FCHMOD, fd as u64, mode as u64) as i64 }
}

pub fn sys_chown(path: *const u8, owner: u32, group: u32) -> i64 {
    unsafe { syscall!(SYS_CHOWN, path as u64, owner as u64, group as u64) as i64 }
}

pub fn sys_fchown(fd: i32, owner: u32, group: u32) -> i64 {
    unsafe { syscall!(SYS_FCHOWN, fd as u64, owner as u64, group as u64) as i64 }
}

pub fn sys_pause() -> i64 {
    unsafe { syscall!(SYS_PAUSE) as i64 }
}

pub fn sys_alarm(seconds: u32) -> u32 {
    unsafe { syscall!(SYS_ALARM, seconds as u64) as u32 }
}

pub fn sys_sched_yield() -> i64 {
    unsafe { syscall!(SYS_SCHED_YIELD) as i64 }
}

pub fn sys_getresuid(ruid: *mut u32, euid: *mut u32, suid: *mut u32) -> i64 {
    unsafe { syscall!(SYS_GETRESUID, ruid as u64, euid as u64, suid as u64) as i64 }
}

pub fn sys_setresuid(ruid: u32, euid: u32, suid: u32) -> i64 {
    unsafe { syscall!(SYS_SETRESUID, ruid as u64, euid as u64, suid as u64) as i64 }
}

pub fn sys_getresgid(rgid: *mut u32, egid: *mut u32, sgid: *mut u32) -> i64 {
    unsafe { syscall!(SYS_GETRESGID, rgid as u64, egid as u64, sgid as u64) as i64 }
}

pub fn sys_setresgid(rgid: u32, egid: u32, sgid: u32) -> i64 {
    unsafe { syscall!(SYS_SETRESGID, rgid as u64, egid as u64, sgid as u64) as i64 }
}

pub fn sys_link(oldpath: *const u8, newpath: *const u8) -> i64 {
    unsafe { syscall!(SYS_LINK, oldpath as u64, newpath as u64) as i64 }
}

pub fn sys_readv(fd: i32, iov: *mut IoVec, iovcnt: i32) -> i64 {
    unsafe { syscall!(SYS_READV, fd as u64, iov as u64, iovcnt as u64) as i64 }
}

pub fn sys_writev(fd: i32, iov: *const IoVec, iovcnt: i32) -> i64 {
    unsafe { syscall!(SYS_WRITEV, fd as u64, iov as u64, iovcnt as u64) as i64 }
}

pub fn sys_pread64(fd: i32, buf: *mut u8, count: usize, offset: i64) -> i64 {
    unsafe { syscall!(SYS_PREAD64, fd as u64, buf as u64, count as u64, offset as u64) as i64 }
}

pub fn sys_pwrite64(fd: i32, buf: *const u8, count: usize, offset: i64) -> i64 {
    unsafe { syscall!(SYS_PWRITE64, fd as u64, buf as u64, count as u64, offset as u64) as i64 }
}

pub fn sys_mprotect(addr: *mut u8, len: usize, prot: i32) -> i64 {
    unsafe { syscall!(SYS_MPROTECT, addr as u64, len as u64, prot as u64) as i64 }
}

pub fn sys_socketpair(domain: i32, sock_type: i32, protocol: i32, sv: *mut i32) -> i64 {
    unsafe { syscall!(SYS_SOCKETPAIR, domain as u64, sock_type as u64, protocol as u64, sv as u64) as i64 }
}

pub fn sys_gettid() -> u64 {
    unsafe { syscall!(SYS_GETTID) }
}

pub fn sys_sched_getparam(pid: u64, param: *mut SchedParam) -> i64 {
    unsafe { syscall!(SYS_SCHED_GETPARAM, pid, param as u64) as i64 }
}

pub fn sys_sched_setparam(pid: u64, param: *const SchedParam) -> i64 {
    unsafe { syscall!(SYS_SCHED_SETPARAM, pid, param as u64) as i64 }
}

pub fn sys_sched_getscheduler(pid: u64) -> i64 {
    unsafe { syscall!(SYS_SCHED_GETSCHEDULER, pid) as i64 }
}

pub fn sys_sched_setscheduler(pid: u64, policy: i32, param: *const SchedParam) -> i64 {
    unsafe { syscall!(SYS_SCHED_SETSCHEDULER, pid, policy as u64, param as u64) as i64 }
}

pub fn sys_prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i64 {
    unsafe { syscall!(SYS_PRCTL, option as u64, arg2, arg3, arg4, arg5) as i64 }
}

pub fn sys_sync() -> i64 {
    unsafe { syscall!(SYS_SYNC) as i64 }
}

pub fn sys_fstatvfs(fd: i32, buf: *mut Statvfs) -> i64 {
    unsafe { syscall!(SYS_FSTATVFS, fd as u64, buf as u64) as i64 }
}

pub fn sys_eventfd2(initval: u32, flags: i32) -> i32 {
    unsafe { syscall!(SYS_EVENTFD2, initval as u64, flags as u64) as i32 }
}

pub fn sys_dup3(oldfd: i32, newfd: i32, flags: i32) -> i64 {
    unsafe { syscall!(SYS_DUP3, oldfd as u64, newfd as u64, flags as u64) as i64 }
}

pub fn sys_tgkill(tgid: i32, tid: i32, sig: i32) -> i64 {
    unsafe { syscall!(SYS_TGKILL, tgid as u64, tid as u64, sig as u64) as i64 }
}

pub fn sys_sched_get_priority_max(policy: i32) -> i64 {
    unsafe { syscall!(SYS_SCHED_GET_PRIORITY_MAX, policy as u64) as i64 }
}

pub fn sys_sched_get_priority_min(policy: i32) -> i64 {
    unsafe { syscall!(SYS_SCHED_GET_PRIORITY_MIN, policy as u64) as i64 }
}

pub fn sys_futex(uaddr: *mut u32, op: i32, val: u32, timeout: *const Timespec, uaddr2: *mut u32, _val3: u32) -> i64 {
    unsafe { syscall!(SYS_FUTEX, uaddr as u64, op as u64, val as u64, timeout as u64, uaddr2 as u64) as i64 }
}

pub fn sys_epoll_create1(flags: i32) -> i64 {
    unsafe { syscall!(SYS_EPOLL_CREATE1, flags as u64) as i64 }
}

pub fn sys_epoll_ctl(epfd: i32, op: i32, fd: i32, event: *const EpollEvent) -> i64 {
    unsafe { syscall!(SYS_EPOLL_CTL, epfd as u64, op as u64, fd as u64, event as u64) as i64 }
}

pub fn sys_epoll_wait(epfd: i32, events: *mut EpollEvent, maxevents: i32, timeout: i32) -> i64 {
    unsafe { syscall!(SYS_EPOLL_WAIT, epfd as u64, events as u64, maxevents as u64, timeout as u64) as i64 }
}

pub fn sys_sysinfo(info: *mut SysInfo) -> i64 {
    unsafe { syscall!(SYS_SYSINFO, info as u64) as i64 }
}

pub fn sys_getrandom(buf: *mut u8, buflen: usize, flags: u32) -> i64 {
    unsafe { syscall!(SYS_GETRANDOM, buf as u64, buflen as u64, flags as u64) as i64 }
}

pub fn sys_openat(dirfd: i32, path: *const u8, flags: u32) -> i64 {
    unsafe { syscall!(SYS_OPENAT, dirfd as u64, path as u64, flags as u64) as i64 }
}

pub fn sys_mkdirat(dirfd: i32, path: *const u8) -> i32 {
    unsafe { syscall!(SYS_MKDIRAT, dirfd as u64, path as u64) as i32 }
}

pub fn sys_unlinkat(dirfd: i32, path: *const u8, flags: i32) -> i32 {
    unsafe { syscall!(SYS_UNLINKAT, dirfd as u64, path as u64, flags as u64) as i32 }
}

pub fn sys_readlinkat(dirfd: i32, path: *const u8, buf: *mut u8, bufsiz: usize) -> i64 {
    unsafe { syscall!(SYS_READLINKAT, dirfd as u64, path as u64, buf as u64, bufsiz as u64) as i64 }
}

pub fn sys_fstatat(dirfd: i32, path: *const u8, stat_buf: *mut crate::fs::Stat, flags: i32) -> i32 {
    unsafe { syscall!(SYS_FSTATAT, dirfd as u64, path as u64, stat_buf as u64, flags as u64) as i32 }
}

pub fn sys_ioctl(fd: i32, request: u64, arg: usize) -> i64 {
    unsafe { syscall!(SYS_IOCTL, fd as u64, request, arg as u64) as i64 }
}

pub fn sys_memfd_create(name: *const u8, flags: u32) -> i64 {
    unsafe { syscall!(SYS_MEMFD_CREATE, name as u64, flags as u64) as i64 }
}

pub fn sys_setsockopt(fd: i32, level: i32, optname: i32, optval: *const u8, optlen: u32) -> i64 {
    unsafe { syscall!(SYS_SETSOCKOPT, fd as u64, level as u64, optname as u64, optval as u64, optlen as u64) as i64 }
}

pub fn sys_getsockopt(fd: i32, level: i32, optname: i32, optval: *mut u8, optlen: *mut u32) -> i64 {
    unsafe { syscall!(SYS_GETSOCKOPT, fd as u64, level as u64, optname as u64, optval as u64, optlen as u64) as i64 }
}

pub fn sys_sethostname(name: *const u8, len: usize) -> i64 {
    unsafe { syscall!(SYS_SETHOSTNAME, name as u64, len as u64) as i64 }
}

pub fn sys_gethostname(buf: *mut u8, len: usize) -> i64 {
    unsafe { syscall!(SYS_GETHOSTNAME, buf as u64, len as u64) as i64 }
}

pub fn sys_gettimeofday(tv: *mut TimeVal, tz: *mut TimeZone) -> i64 {
    unsafe { syscall!(SYS_GETTIMEOFDAY, tv as u64, tz as u64) as i64 }
}

pub fn sys_getitimer(which: i32, curr_value: *mut ITimerVal) -> i64 {
    unsafe { syscall!(SYS_GETITIMER, which as u64, curr_value as u64) as i64 }
}

pub fn sys_setitimer(which: i32, new_value: *const ITimerVal, old_value: *mut ITimerVal) -> i64 {
    unsafe { syscall!(SYS_SETITIMER, which as u64, new_value as u64, old_value as u64) as i64 }
}

pub fn sys_mknod(path: *const u8, mode: u32, dev: u64) -> i64 {
    unsafe { syscall!(SYS_MKNOD, path as u64, mode as u64, dev) as i64 }
}

pub fn sys_fchdir(dirfd: i32) -> i64 {
    unsafe { syscall!(SYS_FCHDIR, dirfd as u64) as i64 }
}

pub fn sys_symlinkat(target: *const u8, newdirfd: i32, linkpath: *const u8) -> i64 {
    unsafe { syscall!(SYS_SYMLINKAT, target as u64, newdirfd as u64, linkpath as u64) as i64 }
}

pub fn sys_linkat(olddirfd: i32, oldpath: *const u8, newdirfd: i32, newpath: *const u8, flags: i32) -> i64 {
    unsafe { syscall!(SYS_LINKAT, olddirfd as u64, oldpath as u64, newdirfd as u64, newpath as u64, flags as u64) as i64 }
}

pub fn sys_renameat(olddirfd: i32, oldpath: *const u8, newdirfd: i32, newpath: *const u8) -> i64 {
    unsafe { syscall!(SYS_RENAMEAT, olddirfd as u64, oldpath as u64, newdirfd as u64, newpath as u64) as i64 }
}

pub fn sys_fchmodat(dirfd: i32, path: *const u8, mode: u32, flags: i32) -> i64 {
    unsafe { syscall!(SYS_FCHMODAT, dirfd as u64, path as u64, mode as u64, flags as u64) as i64 }
}

pub fn sys_fchownat(dirfd: i32, path: *const u8, owner: u32, group: u32, flags: i32) -> i64 {
    unsafe { syscall!(SYS_FCHOWNAT, dirfd as u64, path as u64, owner as u64, group as u64, flags as u64) as i64 }
}

pub fn sys_faccessat(dirfd: i32, path: *const u8, mode: i32, flags: i32) -> i64 {
    unsafe { syscall!(SYS_FACCESSAT, dirfd as u64, path as u64, mode as u64, flags as u64) as i64 }
}

pub fn sys_utimensat(dirfd: i32, path: *const u8, times: *const TimeVal, flags: i32) -> i64 {
    unsafe { syscall!(SYS_UTIMENSAT, dirfd as u64, path as u64, times as u64, flags as u64) as i64 }
}

pub fn sys_getrusage(who: i32, usage: *mut RUsage) -> i64 {
    unsafe { syscall!(SYS_GETRUSAGE, who as u64, usage as u64) as i64 }
}

pub fn sys_prlimit64(pid: i32, resource: i32, new_limit: *const RLimit64, old_limit: *mut RLimit64) -> i64 {
    unsafe { syscall!(SYS_PRLIMIT64, pid as u64, resource as u64, new_limit as u64, old_limit as u64) as i64 }
}

pub fn sys_clock_nanosleep(clock_id: i32, flags: i32, req: *const Timespec, rem: *mut Timespec) -> i64 {
    unsafe { syscall!(SYS_CLOCK_NANOSLEEP, clock_id as u64, flags as u64, req as u64, rem as u64) as i64 }
}

pub fn sys_timerfd_create(clockid: i32, flags: i32) -> i64 {
    unsafe { syscall!(SYS_TIMERFD_CREATE, clockid as u64, flags as u64) as i64 }
}

pub fn sys_timerfd_settime(fd: i32, flags: i32, new_value: *const ITimerSpec, old_value: *mut ITimerSpec) -> i64 {
    unsafe { syscall!(SYS_TIMERFD_SETTIME, fd as u64, flags as u64, new_value as u64, old_value as u64) as i64 }
}

pub fn sys_timerfd_gettime(fd: i32, curr_value: *mut ITimerSpec) -> i64 {
    unsafe { syscall!(SYS_TIMERFD_GETTIME, fd as u64, curr_value as u64) as i64 }
}

pub fn sys_signalfd4(fd: i32, mask: *const u64, sizemask: usize, flags: i32) -> i64 {
    unsafe { syscall!(SYS_SIGNALFD4, fd as u64, mask as u64, sizemask as u64, flags as u64) as i64 }
}

pub fn sys_posix_openpt(flags: i32) -> i64 {
    unsafe { syscall!(SYS_POSIX_OPENPT, flags as u64) as i64 }
}

pub fn sys_ptsname_r(fd: i32, buf: *mut u8, buflen: usize) -> i64 {
    unsafe { syscall!(SYS_PTSNAME_R, fd as u64, buf as u64, buflen as u64) as i64 }
}

pub fn sys_fadvise64(fd: i32, offset: i64, len: i64, advice: i32) -> i64 {
    unsafe { syscall!(SYS_FADVISE64, fd as u64, offset as u64, len as u64, advice as u64) as i64 }
}

pub fn sys_fallocate(fd: i32, mode: i32, offset: i64, len: i64) -> i64 {
    unsafe { syscall!(SYS_FALLOCATE, fd as u64, mode as u64, offset as u64, len as u64) as i64 }
}

pub fn sys_membarrier(cmd: i32, flags: u32, cpu_id: i32) -> i64 {
    unsafe { syscall!(SYS_MEMBARRIER, cmd as u64, flags as u64, cpu_id as u64) as i64 }
}

pub fn sys_copy_file_range(fd_in: i32, off_in: *mut i64, fd_out: i32, off_out: *mut i64, len: usize) -> i64 {
    unsafe { syscall!(SYS_COPY_FILE_RANGE, fd_in as u64, off_in as u64, fd_out as u64, off_out as u64, len as u64) as i64 }
}

pub fn sys_splice(fd_in: i32, off_in: *mut i64, fd_out: i32, off_out: *mut i64, len: usize) -> i64 {
    unsafe { syscall!(SYS_SPLICE, fd_in as u64, off_in as u64, fd_out as u64, off_out as u64, len as u64) as i64 }
}

pub fn sys_tee(fd_in: i32, fd_out: i32, len: usize, flags: u32) -> i64 {
    unsafe { syscall!(SYS_TEE, fd_in as u64, fd_out as u64, len as u64, flags as u64) as i64 }
}

pub fn sys_syncfs(fd: i32) -> i64 {
    unsafe { syscall!(SYS_SYNCFS, fd as u64) as i64 }
}

pub fn sys_statx(dirfd: i32, pathname: *const u8, flags: i32, mask: u32, statxbuf: *mut Statx) -> i64 {
    unsafe { syscall!(SYS_STATX, dirfd as u64, pathname as u64, flags as u64, mask as u64, statxbuf as u64) as i64 }
}

pub fn sys_close_range(first: u32, last: u32, flags: u32) -> i64 {
    unsafe { syscall!(SYS_CLOSE_RANGE, first as u64, last as u64, flags as u64) as i64 }
}

pub fn sys_pidfd_open(pid: i32, flags: u32) -> i64 {
    unsafe { syscall!(SYS_PIDFD_OPEN, pid as u64, flags as u64) as i64 }
}

pub fn sys_pidfd_send_signal(pidfd: i32, sig: i32, info: *const usize, flags: u32) -> i64 {
    unsafe { syscall!(SYS_PIDFD_SEND_SIGNAL, pidfd as u64, sig as u64, info as u64, flags as u64) as i64 }
}

pub fn sys_getcpu(cpu: *mut u32, node: *mut u32, tcache: *mut usize) -> i64 {
    unsafe { syscall!(SYS_GETCPU, cpu as u64, node as u64, tcache as u64) as i64 }
}

pub fn sys_mremap(old_address: usize, old_size: usize, new_size: usize, flags: i32, new_address: usize) -> i64 {
    unsafe { syscall!(SYS_MREMAP, old_address as u64, old_size as u64, new_size as u64, flags as u64, new_address as u64) as i64 }
}

pub fn sys_ppoll(fds: *mut PollFd, nfds: usize, tmo_p: *const Timespec, sigmask: *const u64) -> i64 {
    unsafe { syscall!(SYS_PPOLL, fds as u64, nfds as u64, tmo_p as u64, sigmask as u64) as i64 }
}

pub fn sys_fstatfs(fd: i32, buf: *mut Statvfs) -> i64 {
    unsafe { syscall!(SYS_FSTATFS, fd as u64, buf as u64) as i64 }
}

pub fn sys_readahead(fd: i32, offset: i64, count: usize) -> i64 {
    unsafe { syscall!(SYS_READAHEAD, fd as u64, offset as u64, count as u64) as i64 }
}

pub fn sys_epoll_pwait(epfd: i32, events: *mut EpollEvent, maxevents: i32, timeout: i32, sigmask: *const u64) -> i64 {
    unsafe { syscall!(SYS_EPOLL_PWAIT, epfd as u64, events as u64, maxevents as u64, timeout as u64, sigmask as u64) as i64 }
}

pub fn sys_preadv(fd: i32, iov: *const IoVec, iovcnt: i32, offset: i64) -> i64 {
    unsafe { syscall!(SYS_PREADV, fd as u64, iov as u64, iovcnt as u64, offset as u64) as i64 }
}

pub fn sys_pwritev(fd: i32, iov: *const IoVec, iovcnt: i32, offset: i64) -> i64 {
    unsafe { syscall!(SYS_PWRITEV, fd as u64, iov as u64, iovcnt as u64, offset as u64) as i64 }
}

pub fn sys_faccessat2(dirfd: i32, pathname: *const u8, mode: i32, flags: i32) -> i32 {
    unsafe { syscall!(SYS_FACCESSAT2, dirfd as u64, pathname as u64, mode as u64, flags as u64) as i32 }
}

pub fn sys_msync(addr: usize, len: usize, flags: i32) -> i64 {
    unsafe { syscall!(SYS_MSYNC, addr as u64, len as u64, flags as u64) as i64 }
}

pub fn sys_mincore(addr: usize, len: usize, vec: *mut u8) -> i64 {
    unsafe { syscall!(SYS_MINCORE, addr as u64, len as u64, vec as u64) as i64 }
}

pub fn sys_madvise(addr: usize, len: usize, advice: i32) -> i64 {
    unsafe { syscall!(SYS_MADVISE, addr as u64, len as u64, advice as u64) as i64 }
}

pub fn sys_mlock(addr: usize, len: usize) -> i64 {
    unsafe { syscall!(SYS_MLOCK, addr as u64, len as u64) as i64 }
}

pub fn sys_munlock(addr: usize, len: usize) -> i64 {
    unsafe { syscall!(SYS_MUNLOCK, addr as u64, len as u64) as i64 }
}

pub fn sys_mlockall(flags: i32) -> i64 {
    unsafe { syscall!(SYS_MLOCKALL, flags as u64) as i64 }
}

pub fn sys_munlockall() -> i64 {
    unsafe { syscall!(SYS_MUNLOCKALL) as i64 }
}

pub fn sys_renameat2(olddirfd: i32, oldpath: *const u8, newdirfd: i32, newpath: *const u8, flags: u32) -> i32 {
    unsafe { syscall!(SYS_RENAMEAT2, olddirfd as u64, oldpath as u64, newdirfd as u64, newpath as u64, flags as u64) as i32 }
}

pub fn sys_memfd_secret(flags: u32) -> i64 {
    unsafe { syscall!(SYS_MEMFD_SECRET, flags as u64) as i64 }
}

pub fn sys_epoll_pwait2(epfd: i32, events: *mut EpollEvent, maxevents: i32, tmo_ts: *const Timespec, sigmask: *const u64) -> i64 {
    unsafe { syscall!(SYS_EPOLL_PWAIT2, epfd as u64, events as u64, maxevents as u64, tmo_ts as u64, sigmask as u64) as i64 }
}

pub fn sys_process_madvise(pidfd: i32, iov: *const IoVec, vlen: usize, advice: i32, flags: u32) -> i64 {
    unsafe { syscall!(SYS_PROCESS_MADVISE, pidfd as u64, iov as u64, vlen as u64, advice as u64, flags as u64) as i64 }
}

pub fn sys_sched_yield_posix() -> i64 {
    unsafe { syscall!(SYS_SCHED_YIELD_POSIX) as i64 }
}

pub fn sys_settimeofday(tv: *const TimeVal, tz: *const TimeZone) -> i32 {
    unsafe { syscall!(SYS_SETTIMEOFDAY, tv as u64, tz as u64) as i32 }
}

pub fn sys_sync_file_range(fd: i32, offset: i64, nbytes: i64, flags: u32) -> i32 {
    unsafe { syscall!(SYS_SYNC_FILE_RANGE, fd as u64, offset as u64, nbytes as u64, flags as u64) as i32 }
}

pub fn sys_vmsplice(fd: i32, iov: *const IoVec, nr_segs: usize, flags: u32) -> i64 {
    unsafe { syscall!(SYS_VMSPLICE, fd as u64, iov as u64, nr_segs as u64, flags as u64) as i64 }
}

pub fn sys_kcmp(pid1: i64, pid2: i64, ktype: i32, idx1: u64, idx2: u64) -> i32 {
    unsafe { syscall!(SYS_KCMP, pid1 as u64, pid2 as u64, ktype as u64, idx1, idx2) as i32 }
}

pub fn sys_personality(persona: u64) -> i32 {
    unsafe { syscall!(SYS_PERSONALITY, persona) as i32 }
}

pub fn sys_setns(fd: i32, nstype: i32) -> i32 {
    unsafe { syscall!(SYS_SETNS, fd as u64, nstype as u64) as i32 }
}

pub fn sys_unshare(flags: i32) -> i32 {
    unsafe { syscall!(SYS_UNSHARE, flags as u64) as i32 }
}

pub fn sys_pivot_root(new_root: *const u8, put_old: *const u8) -> i32 {
    unsafe { syscall!(SYS_PIVOT_ROOT, new_root as u64, put_old as u64) as i32 }
}

pub fn sys_lookup_dcookie(cookie: u64, buf: *mut u8, len: usize) -> i32 {
    unsafe { syscall!(SYS_LOOKUP_DCOOKIE, cookie, buf as u64, len as u64) as i32 }
}

pub fn sys_getrandom2(buf: *mut u8, count: usize, flags: u32) -> i64 {
    unsafe { syscall!(SYS_GETRANDOM2, buf as u64, count as u64, flags as u64) as i64 }
}

pub fn sys_sched_setaffinity(pid: i64, cpusetsize: usize, mask: *const u64) -> i32 {
    unsafe { syscall!(SYS_SCHED_SETAFFINITY, pid as u64, cpusetsize as u64, mask as u64) as i32 }
}

pub fn sys_sched_getaffinity(pid: i64, cpusetsize: usize, mask: *mut u64) -> i32 {
    unsafe { syscall!(SYS_SCHED_GETAFFINITY, pid as u64, cpusetsize as u64, mask as u64) as i32 }
}

pub fn sys_clock_settime(clock_id: i32, tp: *const Timespec) -> i32 {
    unsafe { syscall!(SYS_CLOCK_SETTIME, clock_id as u64, tp as u64) as i32 }
}

pub fn sys_getdents64(fd: i32, buf: *mut u8, count: usize) -> i64 {
    unsafe { syscall!(SYS_GETDENTS64, fd as u64, buf as u64, count as u64) as i64 }
}

pub fn sys_pidfd_getfd(pidfd: i32, target_fd: i32, flags: u32) -> i32 {
    unsafe { syscall!(SYS_PIDFD_GETFD, pidfd as u64, target_fd as u64, flags as u64) as i32 }
}

pub fn sys_openat2(dirfd: i32, path: *const u8, how: *const OpenHow, size: usize) -> i64 {
    unsafe { syscall!(SYS_OPENAT2, dirfd as u64, path as u64, how as u64, size as u64) as i64 }
}

pub fn sys_process_mrelease(pidfd: i32, flags: u32) -> i32 {
    unsafe { syscall!(SYS_PROCESS_MRELEASE, pidfd as u64, flags as u64) as i32 }
}

pub fn sys_set_mempolicy(mode: i32, nmask: *const u64, maxnode: usize) -> i32 {
    unsafe { syscall!(SYS_SET_MEMPOLICY, mode as u64, nmask as u64, maxnode as u64) as i32 }
}

pub fn sys_get_mempolicy(mode: *mut i32, nmask: *mut u64, maxnode: usize, addr: usize, flags: u32) -> i32 {
    unsafe { syscall!(SYS_GET_MEMPOLICY, mode as u64, nmask as u64, maxnode as u64, addr as u64, flags as u64) as i32 }
}

pub fn sys_io_pgetevents(ctx_id: usize, min_nr: i64, nr: i64, events: usize, timeout: usize) -> i64 {
    unsafe { syscall!(SYS_IO_PGETEVENTS, ctx_id as u64, min_nr as u64, nr as u64, events as u64, timeout as u64) as i64 }
}

pub fn sys_fsmount(fs_fd: i32, flags: u32, attr_flags: u32) -> i32 {
    unsafe { syscall!(SYS_FSMOUNT, fs_fd as u64, flags as u64, attr_flags as u64) as i32 }
}

pub fn sys_move_mount(from_dirfd: i32, from_path: *const u8, to_dirfd: i32, to_path: *const u8, flags: u32) -> i32 {
    unsafe { syscall!(SYS_MOVE_MOUNT, from_dirfd as u64, from_path as u64, to_dirfd as u64, to_path as u64, flags as u64) as i32 }
}

pub fn sys_capget(hdr: *mut CapUserHeader, data: *mut CapUserData) -> i32 {
    unsafe { syscall!(SYS_CAPGET, hdr as u64, data as u64) as i32 }
}

pub fn sys_capset(hdr: *const CapUserHeader, data: *const CapUserData) -> i32 {
    unsafe { syscall!(SYS_CAPSET, hdr as u64, data as u64) as i32 }
}

pub fn sys_sched_getattr(pid: i32, attr: *mut SchedAttr, size: u32, flags: u32) -> i32 {
    unsafe { syscall!(SYS_SCHED_GETATTR, pid as u64, attr as u64, size as u64, flags as u64) as i32 }
}

pub fn sys_sched_setattr(pid: i32, attr: *const SchedAttr, flags: u32) -> i32 {
    unsafe { syscall!(SYS_SCHED_SETATTR, pid as u64, attr as u64, flags as u64) as i32 }
}

pub fn sys_fsopen(fsname: *const u8, flags: u32) -> i32 {
    unsafe { syscall!(SYS_FSOPEN, fsname as u64, flags as u64) as i32 }
}

pub fn sys_fspick(dfd: i32, path: *const u8, flags: u32) -> i32 {
    unsafe { syscall!(SYS_FSPICK, dfd as u64, path as u64, flags as u64) as i32 }
}

pub fn sys_fsconfig(fs_fd: i32, cmd: u32, key: *const u8, value: *const u8, aux: i32) -> i32 {
    unsafe { syscall!(SYS_FSCONFIG, fs_fd as u64, cmd as u64, key as u64, value as u64, aux as u64) as i32 }
}

pub fn sys_quotactl(cmd: i32, special: *const u8, id: i32, addr: *mut u8) -> i32 {
    unsafe { syscall!(SYS_QUOTACTL, cmd as u64, special as u64, id as u64, addr as u64) as i32 }
}

pub fn sys_landlock_create_ruleset(attr: *const LandlockRulesetAttr, size: usize, flags: u32) -> i32 {
    unsafe { syscall!(SYS_LANDLOCK_CREATE_RULESET, attr as u64, size as u64, flags as u64) as i32 }
}

pub fn sys_landlock_add_rule(ruleset_fd: i32, rule_type: u32, rule_attr: *const u8, flags: u32) -> i32 {
    unsafe { syscall!(SYS_LANDLOCK_ADD_RULE, ruleset_fd as u64, rule_type as u64, rule_attr as u64, flags as u64) as i32 }
}

pub fn sys_landlock_restrict_self(ruleset_fd: i32, flags: u32) -> i32 {
    unsafe { syscall!(SYS_LANDLOCK_RESTRICT_SELF, ruleset_fd as u64, flags as u64) as i32 }
}

pub fn sys_seccomp(operation: u32, flags: u32, args: *const u8) -> i32 {
    unsafe { syscall!(SYS_SECCOMP, operation as u64, flags as u64, args as u64) as i32 }
}

pub fn sys_statmount(req: *const MntIdReq, buf: *mut Statmount, bufsize: usize, flags: u64) -> i64 {
    unsafe { syscall!(SYS_STATMOUNT, req as u64, buf as u64, bufsize as u64, flags) as i64 }
}

pub fn sys_listmount(req: *const MntIdReq, mnt_ids: *mut u64, nr_mnt_ids: usize, flags: u64) -> i64 {
    unsafe { syscall!(SYS_LISTMOUNT, req as u64, mnt_ids as u64, nr_mnt_ids as u64, flags) as i64 }
}

pub fn sys_shutdown(sockfd: i32, how: i32) -> i32 {
    unsafe { syscall!(SYS_SHUTDOWN, sockfd as u64, how as u64) as i32 }
}

pub fn sys_getsockname(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32 {
    unsafe { syscall!(SYS_GETSOCKNAME, sockfd as u64, addr as u64, addrlen as u64) as i32 }
}

pub fn sys_getpeername(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32 {
    unsafe { syscall!(SYS_GETPEERNAME, sockfd as u64, addr as u64, addrlen as u64) as i32 }
}

pub fn sys_sendmsg(sockfd: i32, msg: *const MsgHdr, flags: i32) -> i64 {
    unsafe { syscall!(SYS_SENDMSG, sockfd as u64, msg as u64, flags as u64) as i64 }
}

pub fn sys_recvmsg(sockfd: i32, msg: *mut MsgHdr, flags: i32) -> i64 {
    unsafe { syscall!(SYS_RECVMSG, sockfd as u64, msg as u64, flags as u64) as i64 }
}

pub fn sys_shmget(key: i32, size: usize, shmflg: i32) -> i32 {
    unsafe { syscall!(SYS_SHMGET, key as u64, size as u64, shmflg as u64) as i32 }
}

pub fn sys_shmat(shmid: i32, shmaddr: usize, shmflg: i32) -> usize {
    unsafe { syscall!(SYS_SHMAT, shmid as u64, shmaddr as u64, shmflg as u64) as usize }
}

pub fn sys_shmdt(shmaddr: usize) -> i32 {
    unsafe { syscall!(SYS_SHMDT, shmaddr as u64) as i32 }
}

pub fn sys_shmctl(shmid: i32, cmd: i32, buf: *mut ShmidDs) -> i32 {
    unsafe { syscall!(SYS_SHMCTL, shmid as u64, cmd as u64, buf as u64) as i32 }
}

pub fn sys_semget(key: i32, nsems: i32, semflg: i32) -> i32 {
    unsafe { syscall!(SYS_SEMGET, key as u64, nsems as u64, semflg as u64) as i32 }
}

pub fn sys_semop(semid: i32, sops: *const Sembuf, nsops: usize) -> i32 {
    unsafe { syscall!(SYS_SEMOP, semid as u64, sops as u64, nsops as u64) as i32 }
}

pub fn sys_semctl(semid: i32, semnum: i32, cmd: i32, arg: usize) -> i32 {
    unsafe { syscall!(SYS_SEMCTL, semid as u64, semnum as u64, cmd as u64, arg as u64) as i32 }
}

pub fn sys_msgget(key: i32, msgflg: i32) -> i32 {
    unsafe { syscall!(SYS_MSGGET, key as u64, msgflg as u64) as i32 }
}

pub fn sys_msgsnd(msqid: i32, msgp: *const u8, msgsz: usize, msgflg: i32) -> i32 {
    unsafe { syscall!(SYS_MSGSND, msqid as u64, msgp as u64, msgsz as u64, msgflg as u64) as i32 }
}

pub fn sys_msgrcv(msqid: i32, msgp: *mut u8, msgsz: usize, msgtyp: i64, msgflg: i32) -> i64 {
    unsafe { syscall!(SYS_MSGRCV, msqid as u64, msgp as u64, msgsz as u64, msgtyp as u64, msgflg as u64) as i64 }
}

pub fn sys_msgctl(msqid: i32, cmd: i32, buf: *mut MsqidDs) -> i32 {
    unsafe { syscall!(SYS_MSGCTL, msqid as u64, cmd as u64, buf as u64) as i32 }
}

pub fn sys_timer_create(clockid: i32, sevp: *const u8, timerid: *mut i32) -> i32 {
    unsafe { syscall!(SYS_TIMER_CREATE, clockid as u64, sevp as u64, timerid as u64) as i32 }
}

pub fn sys_timer_settime(timerid: i32, flags: i32, new_value: *const ITimerSpec, old_value: *mut ITimerSpec) -> i32 {
    unsafe { syscall!(SYS_TIMER_SETTIME, timerid as u64, flags as u64, new_value as u64, old_value as u64) as i32 }
}

pub fn sys_timer_gettime(timerid: i32, curr_value: *mut ITimerSpec) -> i32 {
    unsafe { syscall!(SYS_TIMER_GETTIME, timerid as u64, curr_value as u64) as i32 }
}

pub fn sys_timer_getoverrun(timerid: i32) -> i32 {
    unsafe { syscall!(SYS_TIMER_GETOVERRUN, timerid as u64) as i32 }
}

pub fn sys_timer_delete(timerid: i32) -> i32 {
    unsafe { syscall!(SYS_TIMER_DELETE, timerid as u64) as i32 }
}



pub fn sys_futex_waitv(waiters: *const FutexWaitv, nr_futexes: u32, flags: u32, timeout: *const Timespec, clockid: i32) -> i32 {
    unsafe { syscall!(SYS_FUTEX_WAITV, waiters as u64, nr_futexes as u64, flags as u64, timeout as u64, clockid as u64) as i32 }
}

pub fn sys_mlock2(start: usize, len: usize, flags: i32) -> i32 {
    unsafe { syscall!(SYS_MLOCK2, start as u64, len as u64, flags as u64) as i32 }
}

pub fn sys_ioprio_get(which: i32, who: i32) -> i32 {
    unsafe { syscall!(SYS_IOPRIO_GET, which as u64, who as u64) as i32 }
}

pub fn sys_ioprio_set(which: i32, who: i32, ioprio: i32) -> i32 {
    unsafe { syscall!(SYS_IOPRIO_SET, which as u64, who as u64, ioprio as u64) as i32 }
}

pub fn sys_sched_rr_get_interval(pid: i32, interval: *mut Timespec) -> i32 {
    unsafe { syscall!(SYS_SCHED_RR_GET_INTERVAL, pid as u64, interval as u64) as i32 }
}

pub fn sys_get_robust_list(pid: i32, head_ptr: *mut usize, len_ptr: *mut usize) -> i32 {
    unsafe { syscall!(SYS_GET_ROBUST_LIST, pid as u64, head_ptr as u64, len_ptr as u64) as i32 }
}

pub fn sys_set_robust_list(head_ptr: usize, len: usize) -> i32 {
    unsafe { syscall!(SYS_SET_ROBUST_LIST, head_ptr as u64, len as u64) as i32 }
}

pub fn sys_restart_syscall() -> i32 {
    unsafe { syscall!(SYS_RESTART_SYSCALL) as i32 }
}

pub fn sys_kexec_load(entry: usize, nr_segments: usize, segments: *const u8, flags: usize) -> i32 {
    unsafe { syscall!(SYS_KEXEC_LOAD, entry as u64, nr_segments as u64, segments as u64, flags as u64) as i32 }
}

pub fn sys_kexec_file_load(kernel_fd: i32, initrd_fd: i32, cmdline_len: usize, cmdline: *const u8, flags: usize) -> i32 {
    unsafe { syscall!(SYS_KEXEC_FILE_LOAD, kernel_fd as u64, initrd_fd as u64, cmdline_len as u64, cmdline as u64, flags as u64) as i32 }
}

pub fn sys_pkey_mprotect(start: usize, len: usize, prot: i32, pkey: i32) -> i32 {
    unsafe { syscall!(SYS_PKEY_MPROTECT, start as u64, len as u64, prot as u64, pkey as u64) as i32 }
}

pub fn sys_pkey_alloc(flags: u32, access_rights: u32) -> i32 {
    unsafe { syscall!(SYS_PKEY_ALLOC, flags as u64, access_rights as u64) as i32 }
}

pub fn sys_pkey_free(pkey: i32) -> i32 {
    unsafe { syscall!(SYS_PKEY_FREE, pkey as u64) as i32 }
}

pub fn sys_rseq(rseq: *mut Rseq, rseq_len: u32, flags: i32, sig: u32) -> i32 {
    unsafe { syscall!(SYS_RSEQ, rseq as u64, rseq_len as u64, flags as u64, sig as u64) as i32 }
}

pub fn sys_io_uring_setup(entries: u32, params: *mut IoUringParams) -> i32 {
    unsafe { syscall!(SYS_IO_URING_SETUP, entries as u64, params as u64) as i32 }
}

pub fn sys_io_uring_enter(fd: i32, to_submit: u32, min_complete: u32, flags: u32, sig: *const u8) -> i32 {
    unsafe { syscall!(SYS_IO_URING_ENTER, fd as u64, to_submit as u64, min_complete as u64, flags as u64, sig as u64) as i32 }
}

pub fn sys_io_uring_register(fd: i32, opcode: u32, arg: *const u8, nr_args: u32) -> i32 {
    unsafe { syscall!(SYS_IO_URING_REGISTER, fd as u64, opcode as u64, arg as u64, nr_args as u64) as i32 }
}

pub fn sys_preadv2(fd: i32, iov: *const IoVec, iovcnt: i32, offset: i64, flags: i32) -> i64 {
    unsafe { syscall!(SYS_PREADV2, fd as u64, iov as u64, iovcnt as u64, offset as u64, flags as u64) as i64 }
}

pub fn sys_pwritev2(fd: i32, iov: *const IoVec, iovcnt: i32, offset: i64, flags: i32) -> i64 {
    unsafe { syscall!(SYS_PWRITEV2, fd as u64, iov as u64, iovcnt as u64, offset as u64, flags as u64) as i64 }
}

pub fn sys_setdomainname(name: *const u8, len: usize) -> i32 {
    unsafe { syscall!(SYS_SETDOMAINNAME, name as u64, len as u64) as i32 }
}

pub fn sys_swapon(path: *const u8, flags: i32) -> i32 {
    unsafe { syscall!(SYS_SWAPON, path as u64, flags as u64) as i32 }
}

pub fn sys_swapoff(path: *const u8) -> i32 {
    unsafe { syscall!(SYS_SWAPOFF, path as u64) as i32 }
}

pub fn sys_ioperm(from: usize, num: usize, turn_on: i32) -> i32 {
    unsafe { syscall!(SYS_IOPERM, from as u64, num as u64, turn_on as u64) as i32 }
}

pub fn sys_iopl(level: i32) -> i32 {
    unsafe { syscall!(SYS_IOPL, level as u64) as i32 }
}

pub fn sys_userfaultfd(flags: i32) -> i32 {
    unsafe { syscall!(SYS_USERFAULTFD, flags as u64) as i32 }
}

pub fn sys_mq_open(name: *const u8, oflag: i32, mode: u32, attr: *const MqAttr) -> i32 {
    unsafe { syscall!(SYS_MQ_OPEN, name as u64, oflag as u64, mode as u64, attr as u64) as i32 }
}

pub fn sys_mq_unlink(name: *const u8) -> i32 {
    unsafe { syscall!(SYS_MQ_UNLINK, name as u64) as i32 }
}

pub fn sys_mq_timedsend(mqdes: i32, msg_ptr: *const u8, msg_len: usize, msg_prio: u32, abs_timeout: *const Timespec) -> i32 {
    unsafe { syscall!(SYS_MQ_TIMEDSEND, mqdes as u64, msg_ptr as u64, msg_len as u64, msg_prio as u64, abs_timeout as u64) as i32 }
}

pub fn sys_mq_timedreceive(mqdes: i32, msg_ptr: *mut u8, msg_len: usize, msg_prio: *mut u32, abs_timeout: *const Timespec) -> i32 {
    unsafe { syscall!(SYS_MQ_TIMEDRECEIVE, mqdes as u64, msg_ptr as u64, msg_len as u64, msg_prio as u64, abs_timeout as u64) as i32 }
}

pub fn sys_mq_notify(mqdes: i32, sevp: *const u8) -> i32 {
    unsafe { syscall!(SYS_MQ_NOTIFY, mqdes as u64, sevp as u64) as i32 }
}

pub fn sys_mq_getsetattr(mqdes: i32, newattr: *const MqAttr, oldattr: *mut MqAttr) -> i32 {
    unsafe { syscall!(SYS_MQ_GETSETATTR, mqdes as u64, newattr as u64, oldattr as u64) as i32 }
}

pub fn sys_set_thread_area(u_info: *mut UserDesc) -> i32 {
    unsafe { syscall!(SYS_SET_THREAD_AREA, u_info as u64) as i32 }
}

pub fn sys_get_thread_area(u_info: *mut UserDesc) -> i32 {
    unsafe { syscall!(SYS_GET_THREAD_AREA, u_info as u64) as i32 }
}

pub fn sys_set_tid_address(tidptr: *mut i32) -> i32 {
    unsafe { syscall!(SYS_SET_TID_ADDRESS, tidptr as u64) as i32 }
}

pub fn sys_fanotify_init(flags: u32, event_f_flags: u32) -> i32 {
    unsafe { syscall!(SYS_FANOTIFY_INIT, flags as u64, event_f_flags as u64) as i32 }
}

pub fn sys_fanotify_mark(fanotify_fd: i32, flags: u32, mask: u64, dfd: i32, pathname: *const u8) -> i32 {
    unsafe { syscall!(SYS_FANOTIFY_MARK, fanotify_fd as u64, flags as u64, mask, dfd as u64, pathname as u64) as i32 }
}

pub fn sys_name_to_handle_at(dfd: i32, name: *const u8, handle: *mut FileHandle, mnt_id: *mut i32, flags: i32) -> i32 {
    unsafe { syscall!(SYS_NAME_TO_HANDLE_AT, dfd as u64, name as u64, handle as u64, mnt_id as u64, flags as u64) as i32 }
}

pub fn sys_open_by_handle_at(mountdirfd: i32, handle: *const FileHandle, flags: i32) -> i32 {
    unsafe { syscall!(SYS_OPEN_BY_HANDLE_AT, mountdirfd as u64, handle as u64, flags as u64) as i32 }
}

pub fn sys_finit_module(fd: i32, uargs: *const u8, flags: i32) -> i32 {
    unsafe { syscall!(SYS_FINIT_MODULE, fd as u64, uargs as u64, flags as u64) as i32 }
}

pub fn sys_remap_file_pages(start: usize, size: usize, prot: u32, pgoff: usize, flags: u32) -> i32 {
    unsafe { syscall!(SYS_REMAP_FILE_PAGES, start as u64, size as u64, prot as u64, pgoff as u64, flags as u64) as i32 }
}

pub fn sys_sendfile(out_fd: i32, in_fd: i32, offset: *mut usize, count: usize) -> i64 {
    unsafe { syscall!(SYS_SENDFILE, out_fd as u64, in_fd as u64, offset as u64, count as u64) as i64 }
}

pub fn sys_connect(sockfd: i32, addr: *const u8, addrlen: u32) -> i32 {
    unsafe { syscall!(SYS_CONNECT, sockfd as u64, addr as u64, addrlen as u64) as i32 }
}

pub fn sys_listen(sockfd: i32, backlog: i32) -> i32 {
    unsafe { syscall!(SYS_LISTEN, sockfd as u64, backlog as u64) as i32 }
}

pub fn sys_accept(sockfd: i32, addr: *mut u8, addrlen: *mut u32) -> i32 {
    unsafe { syscall!(SYS_ACCEPT, sockfd as u64, addr as u64, addrlen as u64) as i32 }
}

pub fn sys_accept4(sockfd: i32, addr: *mut u8, addrlen: *mut u32, flags: i32) -> i32 {
    unsafe { syscall!(SYS_ACCEPT4, sockfd as u64, addr as u64, addrlen as u64, flags as u64) as i32 }
}

pub fn sys_inotify_init() -> i32 {
    unsafe { syscall!(SYS_INOTIFY_INIT) as i32 }
}

pub fn sys_inotify_init1(flags: i32) -> i32 {
    unsafe { syscall!(SYS_INOTIFY_INIT1, flags as u64) as i32 }
}

pub fn sys_inotify_add_watch(fd: i32, pathname: *const u8, mask: u32) -> i32 {
    unsafe { syscall!(SYS_INOTIFY_ADD_WATCH, fd as u64, pathname as u64, mask as u64) as i32 }
}

pub fn sys_inotify_rm_watch(fd: i32, wd: i32) -> i32 {
    unsafe { syscall!(SYS_INOTIFY_RM_WATCH, fd as u64, wd as u64) as i32 }
}

pub fn sys_setxattr(path: *const u8, name: *const u8, val: *const u8, size: usize, flags: i32) -> i32 {
    unsafe { syscall!(SYS_SETXATTR, path as u64, name as u64, val as u64, size as u64, flags as u64) as i32 }
}

pub fn sys_getxattr(path: *const u8, name: *const u8, val: *mut u8, size: usize) -> i64 {
    unsafe { syscall!(SYS_GETXATTR, path as u64, name as u64, val as u64, size as u64) as i64 }
}

pub fn sys_listxattr(path: *const u8, list: *mut u8, size: usize) -> i64 {
    unsafe { syscall!(SYS_LISTXATTR, path as u64, list as u64, size as u64) as i64 }
}

pub fn sys_removexattr(path: *const u8, name: *const u8) -> i32 {
    unsafe { syscall!(SYS_REMOVEXATTR, path as u64, name as u64) as i32 }
}

pub fn sys_fsetxattr(fd: i32, name: *const u8, val: *const u8, size: usize, flags: i32) -> i32 {
    unsafe { syscall!(SYS_FSETXATTR, fd as u64, name as u64, val as u64, size as u64, flags as u64) as i32 }
}

pub fn sys_fgetxattr(fd: i32, name: *const u8, val: *mut u8, size: usize) -> i64 {
    unsafe { syscall!(SYS_FGETXATTR, fd as u64, name as u64, val as u64, size as u64) as i64 }
}

pub fn sys_flistxattr(fd: i32, list: *mut u8, size: usize) -> i64 {
    unsafe { syscall!(SYS_FLISTXATTR, fd as u64, list as u64, size as u64) as i64 }
}

pub fn sys_fremovexattr(fd: i32, name: *const u8) -> i32 {
    unsafe { syscall!(SYS_FREMOVEXATTR, fd as u64, name as u64) as i32 }
}

pub fn sys_lsetxattr(path: *const u8, name: *const u8, val: *const u8, size: usize, flags: i32) -> i32 {
    unsafe { syscall!(SYS_LSETXATTR, path as u64, name as u64, val as u64, size as u64, flags as u64) as i32 }
}

pub fn sys_lgetxattr(path: *const u8, name: *const u8, val: *mut u8, size: usize) -> i64 {
    unsafe { syscall!(SYS_LGETXATTR, path as u64, name as u64, val as u64, size as u64) as i64 }
}

pub fn sys_llistxattr(path: *const u8, list: *mut u8, size: usize) -> i64 {
    unsafe { syscall!(SYS_LLISTXATTR, path as u64, list as u64, size as u64) as i64 }
}

pub fn sys_lremovexattr(path: *const u8, name: *const u8) -> i32 {
    unsafe { syscall!(SYS_LREMOVEXATTR, path as u64, name as u64) as i32 }
}

pub fn sys_mknodat(dfd: i32, filename: *const u8, mode: u32, dev: u64) -> i32 {
    unsafe { syscall!(SYS_MKNODAT, dfd as u64, filename as u64, mode as u64, dev) as i32 }
}

pub fn sys_futimesat(dfd: i32, filename: *const u8, times: *const u8) -> i32 {
    unsafe { syscall!(SYS_FUTIMESAT, dfd as u64, filename as u64, times as u64) as i32 }
}

pub fn sys_select(nfds: i32, readfds: usize, writefds: usize, exceptfds: usize, timeout: usize) -> i32 {
    unsafe { syscall!(SYS_SELECT, nfds as u64, readfds as u64, writefds as u64, exceptfds as u64, timeout as u64) as i32 }
}

pub fn sys_pselect6(nfds: i32, readfds: usize, writefds: usize, exceptfds: usize, timeout: usize, sigmask: usize) -> i32 {
    unsafe { syscall!(SYS_PSELECT6, nfds as u64, readfds as u64, writefds as u64, exceptfds as u64, timeout as u64, sigmask as u64) as i32 }
}

pub fn sys_adjtimex(buf: *mut u8) -> i32 {
    unsafe { syscall!(SYS_ADJTIMEX, buf as u64) as i32 }
}

pub fn sys_clock_adjtime(clk_id: i32, buf: *mut u8) -> i32 {
    unsafe { syscall!(SYS_CLOCK_ADJTIME, clk_id as u64, buf as u64) as i32 }
}

pub fn sys_migrate_pages(pid: i32, maxnode: usize, old_nodes: *const u8, new_nodes: *const u8) -> i64 {
    unsafe { syscall!(SYS_MIGRATE_PAGES, pid as u64, maxnode as u64, old_nodes as u64, new_nodes as u64) as i64 }
}

pub fn sys_move_pages(pid: i32, count: usize, pages: *const usize, nodes: *const i32, status: *mut i32, flags: i32) -> i64 {
    unsafe { syscall!(SYS_MOVE_PAGES, pid as u64, count as u64, pages as u64, nodes as u64, status as u64, flags as u64) as i64 }
}

pub fn sys_quotactl_fd(fd: i32, cmd: u32, id: i32, addr: usize) -> i32 {
    unsafe { syscall!(SYS_QUOTACTL_FD, fd as u64, cmd as u64, id as u64, addr as u64) as i32 }
}

pub fn sys_execveat(dirfd: i32, pathname: *const u8, argv: *const *const u8, envp: *const *const u8, flags: i32) -> i32 {
    unsafe { syscall!(SYS_EXECVEAT, dirfd as u64, pathname as u64, argv as u64, envp as u64, flags as u64) as i32 }
}

pub fn sys_perf_event_open(attr: usize, pid: i32, cpu: i32, group_fd: i32, flags: u64) -> i64 {
    unsafe { syscall!(SYS_PERF_EVENT_OPEN, attr as u64, pid as u64, cpu as u64, group_fd as u64, flags) as i64 }
}

pub fn sys_bpf(cmd: i32, uattr: usize, size: u32) -> i64 {
    unsafe { syscall!(SYS_BPF, cmd as u64, uattr as u64, size as u64) as i64 }
}

pub fn sys_mount_setattr(dfd: i32, path: *const u8, flags: u32, uattr: usize, usize_val: usize) -> i32 {
    unsafe { syscall!(SYS_MOUNT_SETATTR, dfd as u64, path as u64, flags as u64, uattr as u64, usize_val as u64) as i32 }
}

pub fn sys_set_mempolicy_home_node(start: usize, len: usize, home_node: u64, flags: u64) -> i32 {
    unsafe { syscall!(SYS_SET_MEMPOLICY_HOME_NODE, start as u64, len as u64, home_node, flags) as i32 }
}

pub fn sys_cachestat(fd: i32, cstat_range: usize, cstat: usize, flags: u32) -> i32 {
    unsafe { syscall!(SYS_CACHESTAT, fd as u64, cstat_range as u64, cstat as u64, flags as u64) as i32 }
}

pub fn sys_sigaltstack(uss: usize, uoss: usize) -> i32 {
    unsafe { syscall!(SYS_SIGALTSTACK, uss as u64, uoss as u64) as i32 }
}

pub fn sys_rt_sigsuspend(unewset: usize, sigsetsize: usize) -> i32 {
    unsafe { syscall!(SYS_RT_SIGSUSPEND, unewset as u64, sigsetsize as u64) as i32 }
}

pub fn sys_rt_sigpending(uset: usize, sigsetsize: usize) -> i32 {
    unsafe { syscall!(SYS_RT_SIGPENDING, uset as u64, sigsetsize as u64) as i32 }
}

pub fn sys_rt_sigqueueinfo(pid: i32, sig: i32, uinfo: usize) -> i32 {
    unsafe { syscall!(SYS_RT_SIGQUEUEINFO, pid as u64, sig as u64, uinfo as u64) as i32 }
}

pub fn sys_rt_sigtimedwait(uthese: usize, uinfo: usize, uts: usize, sigsetsize: usize) -> i32 {
    unsafe { syscall!(SYS_RT_SIGTIMEDWAIT, uthese as u64, uinfo as u64, uts as u64, sigsetsize as u64) as i32 }
}

pub fn sys_arch_prctl(code: i32, addr: usize) -> i32 {
    unsafe { syscall!(SYS_ARCH_PRCTL, code as u64, addr as u64) as i32 }
}

pub fn sys_flock(fd: i32, operation: i32) -> i32 {
    unsafe { syscall!(SYS_FLOCK, fd as u64, operation as u64) as i32 }
}

pub fn sys_lchown(path: *const u8, owner: u32, group: u32) -> i32 {
    unsafe { syscall!(SYS_LCHOWN, path as u64, owner as u64, group as u64) as i32 }
}

pub fn sys_utimes(path: *const u8, times: *const Timespec) -> i32 {
    unsafe { syscall!(SYS_UTIMES, path as u64, times as u64) as i32 }
}

pub fn sys_chroot(path: *const u8) -> i32 {
    unsafe { syscall!(SYS_CHROOT, path as u64) as i32 }
}

pub fn sys_getdomainname(name: *mut u8, len: usize) -> i32 {
    unsafe { syscall!(SYS_GETDOMAINNAME, name as u64, len as u64) as i32 }
}

pub fn sys_eventfd(initval: u32) -> i64 {
    unsafe { syscall!(SYS_EVENTFD, initval as u64) as i64 }
}

pub fn sys_rt_tgsigqueueinfo(tgid: i32, tid: i32, sig: i32, uinfo: usize) -> i32 {
    unsafe { syscall!(SYS_RT_TGSIGQUEUEINFO, tgid as u64, tid as u64, sig as u64, uinfo as u64) as i32 }
}

pub fn sys_recvmmsg(sockfd: i32, msgvec: usize, vlen: u32, flags: u32, timeout: usize) -> i32 {
    unsafe { syscall!(SYS_RECVMMSG, sockfd as u64, msgvec as u64, vlen as u64, flags as u64, timeout as u64) as i32 }
}

pub fn sys_sendmmsg(sockfd: i32, msgvec: usize, vlen: u32, flags: u32) -> i32 {
    unsafe { syscall!(SYS_SENDMMSG, sockfd as u64, msgvec as u64, vlen as u64, flags as u64) as i32 }
}

pub fn sys_process_vm_readv(pid: i32, lvec: usize, liovcnt: usize, rvec: usize, riovcnt: usize, flags: u64) -> i64 {
    unsafe { syscall!(SYS_PROCESS_VM_READV, pid as u64, lvec as u64, liovcnt as u64, rvec as u64, riovcnt as u64, flags) as i64 }
}

pub fn sys_process_vm_writev(pid: i32, lvec: usize, liovcnt: usize, rvec: usize, riovcnt: usize, flags: u64) -> i64 {
    unsafe { syscall!(SYS_PROCESS_VM_WRITEV, pid as u64, lvec as u64, liovcnt as u64, rvec as u64, riovcnt as u64, flags) as i64 }
}

pub fn sys_semtimedop(semid: i32, sops: usize, nsops: usize, timeout: usize) -> i32 {
    unsafe { syscall!(SYS_SEMTIMEDOP, semid as u64, sops as u64, nsops as u64, timeout as u64) as i32 }
}

pub fn sys_acct(filename: *const u8) -> i32 {
    unsafe { syscall!(SYS_ACCT, filename as u64) as i32 }
}

pub fn sys_ustat(dev: u32, ubuf: usize) -> i32 {
    unsafe { syscall!(SYS_USTAT, dev as u64, ubuf as u64) as i32 }
}

pub fn sys_syslog(action_type: i32, bufp: usize, len: i32) -> i32 {
    unsafe { syscall!(SYS_SYSLOG, action_type as u64, bufp as u64, len as u64) as i32 }
}

pub fn sys_vhangup() -> i32 {
    unsafe { syscall!(SYS_VHANGUP) as i32 }
}

pub fn sys_modify_ldt(func: i32, ptr: usize, bytecount: usize) -> i32 {
    unsafe { syscall!(SYS_MODIFY_LDT, func as u64, ptr as u64, bytecount as u64) as i32 }
}

pub fn sys_setfsuid(fsuid: u32) -> i32 {
    unsafe { syscall!(SYS_SETFSUID, fsuid as u64) as i32 }
}

pub fn sys_setfsgid(fsgid: u32) -> i32 {
    unsafe { syscall!(SYS_SETFSGID, fsgid as u64) as i32 }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct UserRegs {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9:  u64,
    pub r8:  u64,
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub orig_rax: u64,
    pub rip: u64,
    pub cs:  u64,
    pub eflags: u64,
    pub rsp: u64,
    pub ss:  u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub ds:  u64,
    pub es:  u64,
    pub fs:  u64,
    pub gs:  u64,
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
// [CICLO v6.x] Extensiones de Portix: vigilancia por hardware (DR0-DR3).
// `data` para SETWATCHPOINT = código de longitud DR7 | (código de acceso << 8)
pub const PTRACE_SETWATCHPOINT: i64 = 0x50;
pub const PTRACE_CLEARWATCHPOINTS: i64 = 0x51;
pub const PTRACE_GETWATCHPOINTS: i64 = 0x52;
// [CICLO v6.x] Breakpoints condicionales: la condición se evalúa en el KERNEL
// al disparar el trap y, si no se cumple, el objetivo reanuda sin despertar al
// trazador (semántica de `break ... if`).
pub const PTRACE_SETBPCOND: i64 = 0x53;
pub const PTRACE_GETBPCOND: i64 = 0x54;
pub const PTRACE_CLEARBPCOND: i64 = 0x55;
pub const PTRACE_SETBPCIGNORE: i64 = 0x56;
pub const PTRACE_SETBPCOND2: i64 = 0x57;
pub const PTRACE_SETBPCONDN: i64 = 0x58;
/// [FRENTE C] Última parada por #DB: DR6, ranura que disparó, dirección vigilada,
/// código de longitud y RIP (40 bytes, ver `TrapInfo`).
pub const PTRACE_GETTRAPINFO: i64 = 0x59;

/// Información de la última parada por #DB publicada por `PTRACE_GETTRAPINFO`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct TrapInfo {
    pub dr6: u64,
    /// 0-3 = ranura DRn que disparó; 0xFF = no fue un punto de vigilancia.
    pub slot: u64,
    pub watch_addr: u64,
    pub len_code: u64,
    pub rip: u64,
}

/// Reparte el intervalo [addr, addr+len) en trozos alineados de 8/4/2/1 bytes,
/// que es lo único que el hardware x86-64 sabe vigilar (DR7.LENn exige que la
/// dirección esté alineada a la longitud). Devuelve `None` si hacen falta más
/// de `max_slots` ranuras.
pub fn repartir_vigilancia(addr: u64, len: u64, max_slots: usize) -> Option<alloc::vec::Vec<(u64, u64)>> {
    let mut out = alloc::vec::Vec::new();
    let (mut a, fin) = (addr, addr.checked_add(len)?);
    while a < fin {
        let resto = fin - a;
        let mut tam = 8u64;
        while tam > 1 && (a % tam != 0 || tam > resto) { tam /= 2; }
        out.push((a, tam));
        if out.len() > max_slots { return None; }
        a += tam;
    }
    Some(out)
}

/// Registros direccionables por una condición (mismo orden que el kernel).
pub const CREG_RAX: u32 = 0;
pub const CREG_RBX: u32 = 1;
pub const CREG_RCX: u32 = 2;
pub const CREG_RDX: u32 = 3;
pub const CREG_RSI: u32 = 4;
pub const CREG_RDI: u32 = 5;
pub const CREG_RBP: u32 = 6;
pub const CREG_RSP: u32 = 7;
pub const CREG_R8: u32 = 8;
pub const CREG_R9: u32 = 9;
pub const CREG_R10: u32 = 10;
pub const CREG_R11: u32 = 11;
pub const CREG_R12: u32 = 12;
pub const CREG_R13: u32 = 13;
pub const CREG_R14: u32 = 14;
pub const CREG_R15: u32 = 15;
pub const CREG_RIP: u32 = 16;
pub const CREG_RFLAGS: u32 = 17;
/// Pseudo-registro: la dirección que disparó el trap (para rangos sobre lo vigilado).
pub const CREG_ADDR: u32 = 18;

pub const COP_EQ: u32 = 0;
pub const COP_NE: u32 = 1;
pub const COP_LT: u32 = 2;
pub const COP_LE: u32 = 3;
pub const COP_GT: u32 = 4;
pub const COP_GE: u32 = 5;

/// Especificación de una condición de parada (16 bytes, layout del kernel).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BpCond {
    pub reg: u32,
    pub op: u32,
    pub value: u64,
}

/// Forma heredada de dos cláusulas unidas por Y lógico (40 bytes): es el layout
/// que viaja en `PTRACE_SETBPCOND2` (`--rangetest` del depurador). El kernel lo
/// envuelve sobre el lenguaje general de cuatro cláusulas en dos grupos.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BpCond2 {
    pub n: u32,
    pub _pad: u32,
    pub reg: u32,
    pub op: u32,
    pub value: u64,
    pub reg2: u32,
    pub op2: u32,
    pub value2: u64,
}

/// Cláusula de una condición: comparación `reg op value`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CondClause {
    pub reg: u32,
    pub op: u32,
    pub value: u64,
}

/// Especificación general del lenguaje de condiciones (72 bytes): hasta cuatro
/// cláusulas repartidas en dos grupos; Y dentro de un grupo, O entre grupos.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BpCondN {
    pub n: u32,
    pub g2: u32,
    pub clauses: [CondClause; 4],
}

/// Estado observado de una condición (80 bytes, layout idéntico al del kernel):
/// contadores del kernel, forma de la condición y sus cuatro cláusulas.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BpCondStat {
    pub hits: u32,
    pub skips: u32,
    pub ignore: u32,
    pub n: u8,
    pub g2: u8,
    pub _pad: [u8; 2],
    pub clauses: [CondClause; 4],
}

pub const fn dr7_len_code(len: u64) -> u64 {
    match len { 1 => 0, 2 => 1, 4 => 3, _ => 2 }
}
/// Código DR7 de acceso: 1 = escritura, 3 = lectura/escritura
pub const DR7_RW_EXEC: u64 = 0;
pub const DR7_RW_WRITE: u64 = 1;
pub const DR7_RW_RW: u64 = 3;

/// Empaqueta la configuración de PTRACE_SETWATCHPOINT: código de longitud en el
/// byte bajo y código de acceso en el siguiente (formato que consume el kernel).
pub const fn watch_cfg(len: u64, rw: u64) -> usize {
    (dr7_len_code(len) | (rw << 8)) as usize
}

pub fn sys_ptrace(request: i64, pid: i64, addr: usize, data: usize) -> i64 {
    unsafe { syscall!(SYS_PTRACE, request as u64, pid as u64, addr as u64, data as u64) as i64 }
}

pub fn sys_mount(dev_name: *const u8, dir_name: *const u8, fstype: *const u8, flags: u64, data: usize) -> i32 {
    unsafe { syscall!(SYS_MOUNT, dev_name as u64, dir_name as u64, fstype as u64, flags, data as u64) as i32 }
}

pub fn sys_umount2(target: *const u8, flags: i32) -> i32 {
    unsafe { syscall!(SYS_UMOUNT2, target as u64, flags as u64) as i32 }
}
