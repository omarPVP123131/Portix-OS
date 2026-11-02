// ── Types and Structures for the Process Subsystem ───────────────────────

pub const MAX_PROCS: usize = 64; // [b191] 16 ranuras no bastaban: el JVM arranca 11 hilos y la sonda Java 4 más (pthread_create EAGAIN)
pub const KERNEL_STACK_SIZE: usize = 16384;
pub const USER_STACK_SIZE: usize = 65536;
pub const USER_STACK_TOP: usize = 0x7F00_0000_0000;
pub const TIME_SLICE: u64 = 10; // 10 ticks = 100ms at 100Hz

pub const MAX_FDS: usize = 32;
pub const PROGRAM_BREAK_BASE: usize = 0x2000_0000_0000;
pub const MMAP_BASE: usize = 0x3000_0000_0000;
pub const PAGE_SIZE_USIZE: usize = 4096;

pub const MAX_PIPES: usize = 64;
pub const PIPE_BUF_SIZE: usize = 4096;
pub const MAX_EVENTFDS: usize = 16;
pub const MAX_EPOLLS: usize = 16;
pub const MAX_EPOLL_WATCHES: usize = 16;
pub const MAX_TIMERFDS: usize = 16;
pub const MAX_SIGNALFDS: usize = 16;
pub const MAX_PTYS: usize = 8;
pub const MQ_MAX_QUEUES: usize = 16;
pub const MQ_MAX_MSG_COUNT: usize = 16;
pub const MQ_MAX_MSG_SIZE: usize = 256;
pub const MAX_FANOTIFY_INSTANCES: usize = 8;
pub const MAX_FANOTIFY_MARKS: usize = 16;
pub const MAX_INOTIFY_INSTANCES: usize = 16;
pub const MAX_INOTIFY_WATCHES: usize = 32;
pub const MAX_XATTR_ENTRIES: usize = 64;
pub const MAX_XATTR_NAME_LEN: usize = 64;
pub const MAX_XATTR_VAL_LEN: usize = 128;

/// [CICLO 51] Descriptor de fichero de cualquier sistema bajo el contrato
/// `fs::SistemaDeFicheros` (ramfs hoy, ext2 en el lote 2): guarda la ruta
/// absoluta y la posición; el VFS resuelve el sistema en cada operación.
#[derive(Copy, Clone)]
pub struct FsFileInfo {
    pub path: [u8; 256],
    pub path_len: usize,
    pub size: u64,   // [T-001] 64 bits: ficheros > 4 GiB (triple indirecto)
    pub pos: u64,
    pub flags: u32,
}

#[derive(Copy, Clone, PartialEq)]
pub enum DeviceType {
    Kbd,
    Fb,
    Sda,
    Null,
    Zero,
    Urandom,
    Dsp,
}

#[derive(Copy, Clone)]
pub struct DeviceInfo {
    pub dev_type: DeviceType,
    pub pos: u32,
}

#[derive(Copy, Clone)]
pub enum FdType {
    Stdin,
    Stdout,
    Stderr,
    FsFile(FsFileInfo),
    Device(DeviceInfo),
    PipeRead(u32),
    PipeWrite(u32),
    Socket(usize),
    SocketPairEnd { read_pipe: u32, write_pipe: u32 },
    EventFd(u32),
    Epoll(u32),
    TimerFd(u32),
    SignalFd(u32),
    PtyMaster(u32),
    PtySlave(u32),
    PidFd(u32),
    IoUring(u32),
    Mq(u32),
    Fanotify(u32),
    Inotify(u32),
    ProcFile(ProcFileInfo),
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ProcFileType {
    Version,
    Uptime,
    Meminfo,
    /// [FRENTE C] /proc/frames: conteo de referencias de marcos COW.
    Frames,
    Cpuinfo,
    Smp,
    SmpEstres,
    Loadavg,
    Mounts,
    Modules,
    Stat,
    NvmeStatus,
    PipesStatus,
    SignalsStatus,
    SyscallTrace,
    AbiFreeze,
    Certification,
    ReleaseInfo,
    CpuProfiler,
    DebugChannel,
    RayTracing,
    DriverSandbox,
    Riscv64Hal,
    PqcStatus,
    EbpfProbe,
    RtAudio,
    WasiComponents,
    PortixDfs,
    ProcessStat(u64),
    ProcessStatus(u64),
    ProcessCmdline(u64),
    ProcessMaps(u64),
    ProcessSymbols(u64),
    Directory(u64),
}

#[derive(Copy, Clone)]
pub struct ProcFileInfo {
    pub file_type: ProcFileType,
    pub pos: u32,
}

#[derive(Copy, Clone)]
pub struct FdEntry {
    pub fd_type: FdType,
    pub flags: u32,
}

impl FdEntry {
    pub const fn new(fd_type: FdType) -> Self {
        FdEntry { fd_type, flags: 0 }
    }
}

pub struct Pipe {
    pub buf: [u8; PIPE_BUF_SIZE],
    pub read_pos: usize,
    pub write_pos: usize,
    pub count: usize,
    pub reader_open: bool,
    pub writer_open: bool,
    pub in_use: bool,
}

#[derive(Copy, Clone)]
pub struct EventFdEntry {
    pub val: u64,
    pub flags: u32,
    pub in_use: bool,
    pub ref_count: u32,
}

#[derive(Copy, Clone)]
pub struct EpollEvent {
    pub events: u32,
    pub data: u64,
}

#[derive(Copy, Clone)]
pub struct EpollWatch {
    pub fd: i32,
    pub event: EpollEvent,
    pub in_use: bool,
}

pub struct EpollInstance {
    pub watches: [EpollWatch; MAX_EPOLL_WATCHES],
    pub watch_count: usize,
    pub in_use: bool,
    pub ref_count: u32,
}

#[derive(Copy, Clone)]
pub struct TimerFdEntry {
    pub clock_id: i32,
    pub flags: i32,
    pub interval_ticks: u64,
    pub next_expire_ticks: u64,
    pub expirations: u64,
    pub in_use: bool,
    pub ref_count: u32,
}

#[derive(Copy, Clone)]
pub struct SignalFdEntry {
    pub mask: u64,
    pub flags: i32,
    pub in_use: bool,
    pub ref_count: u32,
}

pub struct PtyEntry {
    pub master_pipe: u32,
    pub slave_pipe: u32,
    pub in_use: bool,
    pub master_ref: u32,
    pub slave_ref: u32,
}

#[derive(Copy, Clone)]
pub struct MqMessage {
    pub data: [u8; MQ_MAX_MSG_SIZE],
    pub len: usize,
    pub prio: u32,
}

pub struct PosixMqQueue {
    pub name: [u8; 64],
    pub name_len: usize,
    pub msgs: [MqMessage; MQ_MAX_MSG_COUNT],
    pub count: usize,
    pub max_msgs: usize,
    pub msg_size: usize,
    pub in_use: bool,
    pub ref_count: u32,
}

#[derive(Copy, Clone)]
pub struct FanotifyMark {
    pub mask: u64,
    pub path_hash: u64,
    pub in_use: bool,
}

pub struct FanotifyInstance {
    pub flags: u32,
    pub event_f_flags: u32,
    pub marks: [FanotifyMark; MAX_FANOTIFY_MARKS],
    pub in_use: bool,
    pub ref_count: u32,
    pub pending_events: u32,
}

#[derive(Copy, Clone)]
pub struct InotifyWatch {
    pub wd: i32,
    pub mask: u32,
    pub path_hash: u64,
    pub in_use: bool,
}

pub struct InotifyInstance {
    pub watches: [InotifyWatch; MAX_INOTIFY_WATCHES],
    pub watch_count: usize,
    pub in_use: bool,
    pub ref_count: u32,
    pub next_wd: i32,
    pub pending_events: u32,
}

#[derive(Copy, Clone)]
pub struct XattrEntry {
    pub path_hash: u64,
    pub name: [u8; MAX_XATTR_NAME_LEN],
    pub name_len: usize,
    pub val: [u8; MAX_XATTR_VAL_LEN],
    pub val_len: usize,
    pub in_use: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ProcessState {
    Dead = 0,
    Ready = 1,
    Running = 2,
    Blocked = 3,
    Zombie = 4,
    /// [CICLO v6.x] Detenido por su depurador: el proceso conserva su contexto
    /// completo en el kernel stack pero no vuelve a ser planificado hasta que
    /// el trazador ordene continuar (`PTRACE_CONT`/`PTRACE_SINGLESTEP`).
    Stopped = 5,
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

/// [CICLO 48] Capacidad de la cola de senales de tiempo real por proceso.
pub const SIG_RT_COLA_MAX: usize = 32;

/// [CICLO 48] Origen de una senal: emisor, codigo `si_code` y valor de `sigqueue`.
#[derive(Clone, Copy, Default)]
pub struct SigOrigen {
    pub pid: u32,
    pub uid: u32,
    pub code: i32,
    pub value: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Process {
    pub pid: u64,
    pub ppid: u64,
    pub state: ProcessState,
    pub name: [u8; 32],
    pub name_len: usize,
    pub cr3: u64,
    pub kernel_rsp: u64,
    pub user_rsp: u64,
    pub user_rip: u64,
    pub kernel_stack_base: usize,
    pub kernel_stack_top: usize,
    pub user_stack_phys: usize,
    pub exit_code: i32,
    /// Señal que terminó al proceso (0 = salida normal). Permite a `waitpid`
    /// codificar el estado POSIX: `(código << 8)` o `señal` (WIFSIGNALED).
    pub term_signal: u8,
    pub ticks_used: u64,
    pub sleep_until: u64,
    pub ring3_ret_rsp: u64,
    pub ring3_ret_addr: u64,
    pub fds: [Option<FdEntry>; MAX_FDS],
    pub program_break: usize,
    pub program_break_end: usize,
    pub mmap_base: usize,
    /// [CIERRE B-3] Fin (exclusivo) de la imagen ELF cargada: `munmap` nunca
    /// puede destruir el texto/datos del programa.
    pub image_end: usize,
    pub registered_ports: [u16; 16],
    pub registered_port_count: usize,
    pub cwd: [u8; 64],
    pub cwd_len: usize,
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub pgid: u64,
    pub sid: u64,
    pub umask: u32,
    pub sig_actions: [usize; 65],
    pub sig_flags: [u32; 65],
    pub sig_masks: [u64; 65],
    /// [CICLO 48] Origen de la ultima emision de cada senal (para `siginfo_t`).
    pub sig_src: [SigOrigen; 65],
    /// [CICLO 48] Cola FIFO real de senales de tiempo real (32..=63): cada
    /// instancia conserva su origen; `sig_rt_n` es el numero de entradas.
    pub sig_rt_cola: [(u8, SigOrigen); SIG_RT_COLA_MAX],
    pub sig_rt_n: u8,
    pub sig_blocked: u64,
    pub sig_pending: u64,
    /// [T-001/FIFO] `rt_sigsuspend` en curso: máscara previa que debe quedar
    /// restaurada cuando termine el manejador (o la espera).
    pub sig_suspend_activo: bool,
    pub sig_suspend_prev: u64,
    pub niceness: i32,
    pub rlimit_nofile: u64,
    pub rlimit_nofile_max: u64,
    pub utime: u64,
    pub stime: u64,
    pub cutime: u64,
    pub cstime: u64,
    pub groups: [u32; 16],
    pub ngroups: usize,
    pub suid: u32,
    pub sgid: u32,
    pub alarm_ticks: u64,
    pub robust_list_head: usize,
    pub robust_list_len: usize,
    pub affinity_mask: u64,
    pub last_cpu: u8,
    pub fs_base: u64,
    pub gs_base: u64,
    pub ptrace_traced: bool,
    pub ptrace_tracer: u64,
    /// [CICLO v6.x] Verdadero en cuanto el trazador ejecuta una operación REAL
    /// de depuración (ATTACH, CONT, SINGLESTEP, GETREGS, PEEK/POKE...) sobre
    /// OTRO proceso. Un proceso que únicamente se declaró trazable con
    /// PTRACE_TRACEME —sin que nadie lo depure de verdad— no se detiene ante un
    /// trap: sigue recibiendo su SIGTRAP por el camino normal.
    pub ptrace_engaged: bool,
    pub ptrace_singlestep: bool,
    /// Señal que provocó la parada en curso (0 = el proceso no está detenido).
    pub stop_signal: u8,
    /// El trazador ya consumió esta parada con `waitpid` (patrón job control).
    pub stop_reported: bool,
    /// Dirección del evento que provocó la parada (RIP del breakpoint/watchpoint).
    pub stop_addr: u64,
    /// [FRENTE C] DR6 tal como lo dejó el procesador en la última parada por
    /// #DB: sus bits B0-B3 dicen QUÉ ranura (DR0-DR3) disparó. Es lo que permite
    /// al depurador atribuir un acceso a un trozo concreto de un búfer vigilado
    /// repartido entre varias ranuras.
    pub stop_dr6: u64,
    // [CICLO v6.x] Puntos de vigilancia por hardware (DR0-DR3 + DR7). Son por
    // proceso y el kernel los carga en cada cambio de contexto.
    pub watch_count: u8,
    /// [CICLO v6.x] Condiciones de parada: cuando el objetivo salta a un punto
    /// de parada cuya condición NO se cumple, el KERNEL reanuda la ejecución sin
    /// notificar al trazador (es lo que hace un depurador con `break ... if`).
    /// La evaluación ocurre en el propio manejo del trap, con los registros
    /// reales del frame, y los contadores dejan constancia de lo ocurrido.
    pub bp_cond_count: u8,
    pub bp_cond_addr: [u64; 4],
    pub bp_cond_reg: [u8; 4],
    pub bp_cond_op: [u8; 4],
    pub bp_cond_value: [u64; 4],
    pub bp_cond_hits: [u32; 4],
    pub bp_cond_skips: [u32; 4],
    /// Ranura del hardware desactivada temporalmente por un auto-continúo
    /// (0xFF = ninguna). Al llegar el trap de la instrucción ejecutada hay que
    /// volver a armarla y consumir el evento en silencio.
    pub bp_cond_rearm: u8,
    /// Contador de omisión por condición (`ignore N`): las primeras N
    /// evaluaciones verdaderas se ignoran y el objetivo sigue; el trazador sólo
    /// se entera a partir de la N+1. Es la semántica de `ignore` de gdb y la
    /// forma de decir "no me avises de los primeros N aciertos".
    pub bp_cond_ignore: [u32; 4],
    /// Lenguaje general de condiciones: cada punto de parada admite hasta CUATRO
    /// comparaciones repartidas en DOS grupos; las de un mismo grupo se unen por
    /// Y lógico y los grupos por O lógico. `bp_cond_g2` es el índice de la
    /// primera cláusula del segundo grupo (0 = no hay segundo grupo).
    ///   * un rango .................. `A <= x < B`           -> n=2, g2=0
    ///   * una disyunción ............ `x == A || x == B`     -> n=2, g2=1
    ///   * un rango con guarda ....... `A <= x < B && y != 0` -> n=3, g2=0
    ///   * rango o valor exacto ...... `(A <= x < B) || x == C` -> n=3, g2=2
    pub bp_cond_clauses: [u8; 4],
    pub bp_cond_g2: [u8; 4],
    pub bp_cond_regs: [[u8; 4]; 4],
    pub bp_cond_ops: [[u8; 4]; 4],
    pub bp_cond_vals: [[u64; 4]; 4],
    pub watch_addr: [u64; 4],
    pub watch_len: [u8; 4],   // código DR7 de longitud
    pub watch_rw: [u8; 4],    // 1 = escritura, 3 = lectura/escritura
    pub regs: UserRegs,
    /// [T-005] Grupo de hilos: `pid` del hilo líder (el proceso que ve
    /// `getpid`). En un proceso sin hilos `tgid == pid`.
    pub tgid: u64,
    /// [T-005] `true` si esta ranura es un hilo secundario creado con
    /// `clone(CLONE_VM|CLONE_THREAD)`: comparte CR3, descriptores, brk y mmap
    /// con su líder y NO posee pila de usuario propia del núcleo.
    pub es_hilo: bool,
    /// [T-005] `set_tid_address`/`CLONE_CHILD_CLEARTID`: al morir el hilo el
    /// núcleo escribe 0 en esta dirección y despierta un futex en ella (así
    /// espera `pthread_join`). 0 = no establecido.
    pub clear_child_tid: u64,
    /// [T-005] El líder llamó a `exit(2)` (no `exit_group`) con hilos vivos:
    /// queda dormido y el proceso termina cuando muera el último hilo.
    pub lider_salio: bool,
    /// [b191] Ruta completa del ejecutable (para `readlink("/proc/self/exe")`, que glibc
    /// usa para expandir `$ORIGIN` en RUNPATH: el `java` de OpenJDK carga `libjli.so`
    /// desde `$ORIGIN/../lib`). `name` sólo guarda 31 bytes.
    pub exe_path: [u8; 160],
    pub exe_len: u8,
    /// [T-025 E3d] Índice de la CPU que está FÍSICAMENTE sobre el contexto de
    /// este proceso (su kernel stack), o `SIN_CPU`. No es lo mismo que
    /// `Running`: un proceso `Blocked` dentro de una syscall (futex, sleep)
    /// sigue teniendo a su CPU ejecutando el bucle de espera sobre su pila;
    /// si otra CPU lo despierta (`Ready`) y lo elige, dos CPUs ejecutan el
    /// mismo contexto (#UD con RIP=3 en `run java`). `pick_next_ready` no
    /// elige a nadie cuya CPU no sea la propia.
    pub en_cpu: u8,
    /// [T-020] Excepciones (#PF/#GP/…) entregadas como señal a un manejador de
    /// usuario. Sólo las 3 primeras se registran línea a línea en el serie; el
    /// total se imprime al salir el proceso (la JVM genera miles por NPE).
    pub senales_excepcion: u32,
    /// [T-017] El hilo muere SOLO: su SIGKILL (puesto por `execve_desde_hilo`
    /// o por otro cierre ordenado del grupo) no debe arrastrar al grupo entero.
    pub salir_solo: bool,
}

pub const SIN_CPU: u8 = 0xFF;

impl Process {
    /// Estado POSIX para `waitpid`/`wait4`: WIFEXITED → `(code & 0xff) << 8`;
    /// WIFSIGNALED → `señal & 0x7f` (sin core dump).
    pub fn wait_status(&self) -> i32 {
        if self.term_signal != 0 { (self.term_signal as i32) & 0x7f } else { (self.exit_code & 0xff) << 8 }
    }
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
    /// [b191] Ruta completa del ejecutable; si no se registró, se reconstruye como `/bin/<nombre>`.
    pub fn exe_path_str(&self) -> &str {
        core::str::from_utf8(&self.exe_path[..(self.exe_len as usize).min(self.exe_path.len())]).unwrap_or("")
    }
    pub fn set_exe_path(&mut self, ruta: &str) {
        let n = ruta.len().min(self.exe_path.len());
        self.exe_path = [0u8; 160];
        self.exe_path[..n].copy_from_slice(&ruta.as_bytes()[..n]);
        self.exe_len = n as u8;
    }
}

pub struct ProcessSnapshot {
    pub pid: u64,
    pub ppid: u64,
    pub state: ProcessState,
    pub name: [u8; 32],
    pub name_len: usize,
    pub ticks_used: u64,
    pub exit_code: i32,
    pub uid: u32,
    pub gid: u32,
    pub vmsize_kb: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct Tms {
    pub tms_utime: i64,
    pub tms_stime: i64,
    pub tms_cutime: i64,
    pub tms_cstime: i64,
}

#[derive(Clone, Copy)]
pub struct AuxvInfo {
    pub phdr: u64,
    pub phent: u64,
    pub phnum: u64,
    pub entry: u64,
    pub base: u64,
    pub execfn: u64,
}

pub const AT_NULL: u64 = 0;
pub const AT_IGNORE: u64 = 1;
pub const AT_EXECFD: u64 = 2;
pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_BASE: u64 = 7;
pub const AT_FLAGS: u64 = 8;
pub const AT_ENTRY: u64 = 9;
pub const AT_NOTELF: u64 = 10;
pub const AT_UID: u64 = 11;
pub const AT_EUID: u64 = 12;
pub const AT_GID: u64 = 13;
pub const AT_EGID: u64 = 14;
pub const AT_PLATFORM: u64 = 15;
pub const AT_HWCAP: u64 = 16;
pub const AT_CLKTCK: u64 = 17;
pub const AT_SECURE: u64 = 23;
pub const AT_RANDOM: u64 = 25;
pub const AT_EXECFN: u64 = 31;
pub const AT_SYSINFO_EHDR: u64 = 33;
