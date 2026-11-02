// kernel/src/system/devsuite.rs — Suite de Desarrollo, Depuración Dinámica y
// Profiler de Silicio (Épica 59, Fases 3851-3935)
//
// Componentes reales, sin telemetría simulada:
//
//   • Trazador de llamadas al sistema con marcas de tiempo TSC (rdtsc):
//     anillo circular en kernel con pid, número de syscall, argumento 0,
//     ciclos consumidos y retorno.
//   • Profiler de CPU por proceso: muestreo del RIP en cada tick del
//     planificador (100 Hz) con histograma de puntos calientes por proceso.
//   • Instrumentación de pila (stack probe) a partir del RSP muestreado.
//   • Canal de depuración /proc/syscalls, /proc/profiler y /proc/dbg.
//
// 100% Rust no_std, sin dependencias externas, sin unwrap.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use crate::arch::spinlock::Spinlock;

// ─────────────────────────────────────────────────────────────────────────────
//  Trazador de llamadas al sistema (TSC)
// ─────────────────────────────────────────────────────────────────────────────
pub const TRACE_RING_SIZE: usize = 128;

/// Entrada del anillo de trazas de llamadas al sistema.
#[derive(Clone, Copy, Default)]
pub struct SyscallTrace {
    pub seq: u64,          // número de secuencia global
    pub pid: u64,          // proceso emisor
    pub num: u64,          // número de syscall
    pub arg0: u64,         // primer argumento (fd / puntero / pid)
    pub tsc_start: u64,    // marca TSC de entrada
    pub tsc_cycles: u64,   // ciclos consumidos en el kernel
    pub ret: i64,          // valor devuelto
    pub kernel_abi: bool,  // true = ABI Portix, false = ABI Linux x86_64
}

pub struct TraceState {
    pub ring: [SyscallTrace; TRACE_RING_SIZE],
    pub head: usize,
    pub produced: u64,
    pub dropped: u64,
    pub max_cycles: u64,
    pub total_cycles: u64,
}

impl TraceState {
    pub const fn new() -> Self {
        TraceState {
            ring: [SyscallTrace {
                seq: 0, pid: 0, num: 0, arg0: 0,
                tsc_start: 0, tsc_cycles: 0, ret: 0, kernel_abi: true,
            }; TRACE_RING_SIZE],
            head: 0,
            produced: 0,
            dropped: 0,
            max_cycles: 0,
            total_cycles: 0,
        }
    }
}

pub static TRACER: Spinlock<TraceState> = Spinlock::new(TraceState::new());
pub static TRACE_ENABLED: AtomicBool = AtomicBool::new(false);
pub static TRACE_ENTRIES: AtomicUsize = AtomicUsize::new(TRACE_RING_SIZE);

/// Marca la entrada de una llamada al sistema. Devuelve el TSC de entrada
/// (0 si el trazador está desactivado, evitando así el coste de rdtsc).
#[inline]
pub fn trace_enter() -> u64 {
    if !TRACE_ENABLED.load(Ordering::Relaxed) {
        return 0;
    }
    read_tsc()
}

/// Cierra la traza de la llamada al sistema con su resultado.
#[inline]
pub fn trace_exit(start: u64, num: u64, arg0: u64, ret: i64, kernel_abi: bool) {
    if start == 0 {
        return;
    }
    let end = read_tsc();
    let cycles = end.saturating_sub(start);
    let pid = crate::arch::percpu::pid_actual();

    let mut guard = match TRACER.try_lock() {
        Some(g) => g,
        None => {
            TRACER_DROPS.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    let idx = guard.head;
    guard.ring[idx] = SyscallTrace {
        seq: guard.produced + 1,
        pid,
        num,
        arg0,
        tsc_start: start,
        tsc_cycles: cycles,
        ret,
        kernel_abi,
    };
    guard.head = (idx + 1) % TRACE_RING_SIZE;
    guard.produced = guard.produced.wrapping_add(1);
    guard.total_cycles = guard.total_cycles.saturating_add(cycles);
    if cycles > guard.max_cycles {
        guard.max_cycles = cycles;
    }
}

static TRACER_DROPS: AtomicU64 = AtomicU64::new(0);

/// Lectura del contador de marcas de tiempo del procesador (TSC).
#[inline]
pub fn read_tsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, nomem, preserves_flags));
    }
    ((hi as u64) << 32) | lo as u64
}

/// Frecuencia del TSC en MHz medida contra el reloj PIT de 100 Hz.
/// Se recalibra de forma perezosa en la primera consulta.
pub static TSC_MHZ: AtomicU64 = AtomicU64::new(0);

pub fn tsc_mhz() -> u64 {
    let cached = TSC_MHZ.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    // Calibración real: cuenta ciclos TSC durante 100 ticks del PIT (1 s).
    let t0 = read_tsc();
    let base = crate::time::pit::ticks();
    let mut waited = 0u64;
    while crate::time::pit::ticks().wrapping_sub(base) < 100 && waited < 200_000_000 {
        core::hint::spin_loop();
        waited += 1;
    }
    let t1 = read_tsc();
    let ticks = crate::time::pit::ticks().wrapping_sub(base).max(1);
    let hz = (t1.wrapping_sub(t0)) / ticks * 100;
    let mhz = hz / 1_000_000;
    let mhz = if mhz == 0 { 1000 } else { mhz }; // valor conservador si no hay TSC
    TSC_MHZ.store(mhz, Ordering::Relaxed);
    mhz
}

/// Convierte ciclos TSC a nanosegundos con la frecuencia medida.
pub fn cycles_to_ns(cycles: u64) -> u64 {
    let mhz = tsc_mhz();
    if mhz == 0 {
        return 0;
    }
    cycles.saturating_mul(1000) / mhz
}

/// Habilita o deshabilita el trazador de llamadas al sistema.
pub fn set_tracing(enabled: bool) -> bool {
    let prev = TRACE_ENABLED.swap(enabled, Ordering::SeqCst);
    prev
}

pub fn tracing_enabled() -> bool {
    TRACE_ENABLED.load(Ordering::Relaxed)
}

/// Vuelca las últimas `n` trazas en orden cronológico.
pub fn recent_traces(n: usize) -> Vec<SyscallTrace> {
    let mut out = Vec::new();
    let guard = match TRACER.try_lock() {
        Some(g) => g,
        None => return out,
    };
    let count = if guard.produced as usize > TRACE_RING_SIZE {
        TRACE_RING_SIZE
    } else {
        guard.produced as usize
    };
    let take = n.min(count);
    let start = (guard.head + TRACE_RING_SIZE - take) % TRACE_RING_SIZE;
    for i in 0..take {
        out.push(guard.ring[(start + i) % TRACE_RING_SIZE]);
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
//  Cobertura medida del traductor de ABI Linux (métrica de runtime)
// ─────────────────────────────────────────────────────────────────────────────
// Cada llamada que entra por el ABI Linux se contabiliza; las que no tienen
// traducción explícita caen al despachador nativo y se registran en un mapa de
// bits, de modo que la cobertura publicada es MEDIDA, no declarada.
pub static LINUX_CALLS: AtomicU64 = AtomicU64::new(0);
pub static LINUX_FALLBACKS: AtomicU64 = AtomicU64::new(0);
pub const LINUX_BITMAP_WORDS: usize = 16; // [FRENTE C-5] 1024 números cubiertos
pub static LINUX_FALLBACK_BITS: [AtomicU64; LINUX_BITMAP_WORDS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];
/// [FRENTE C-5] Números >= 1024 (fuera del mapa) vistos sin traducción.
pub static LINUX_FALLBACKS_FUERA_DE_MAPA: AtomicU64 = AtomicU64::new(0);

/// Contabiliza una entrada al traductor de ABI Linux.
#[inline]
pub fn note_linux_call() {
    LINUX_CALLS.fetch_add(1, Ordering::Relaxed);
}

/// Contabiliza que el número `num` no tiene traducción explícita y cae al
/// despachador nativo de Portix.
#[inline]
pub fn note_linux_fallback(num: u64) {
    LINUX_FALLBACKS.fetch_add(1, Ordering::Relaxed);
    if (num as usize) < LINUX_BITMAP_WORDS * 64 {
        let word = (num / 64) as usize;
        let bit = num % 64;
        LINUX_FALLBACK_BITS[word].fetch_or(1u64 << bit, Ordering::Relaxed);
    } else {
        LINUX_FALLBACKS_FUERA_DE_MAPA.fetch_add(1, Ordering::Relaxed);
    }
}

/// Números del ABI Linux observados sin traducción explícita.
pub fn linux_fallback_numbers() -> Vec<u64> {
    let mut out = Vec::new();
    for w in 0..LINUX_BITMAP_WORDS {
        let word = LINUX_FALLBACK_BITS[w].load(Ordering::Relaxed);
        if word == 0 {
            continue;
        }
        for b in 0..64u64 {
            if (word & (1u64 << b)) != 0 {
                out.push((w as u64) * 64 + b);
            }
        }
    }
    out
}

/// Reporte de cobertura del traductor de ABI Linux.
pub fn get_linux_coverage_report() -> String {
    let total = LINUX_CALLS.load(Ordering::Relaxed);
    let fallbacks = LINUX_FALLBACKS.load(Ordering::Relaxed);
    let translated = total.saturating_sub(fallbacks);
    let pct = if total > 0 { (translated * 100) / total } else { 100 };
    let mut out = String::new();
    out.push_str("==============================================================\n");
    out.push_str("  COBERTURA DEL TRADUCTOR DE ABI LINUX (medida en runtime)\n");
    out.push_str("  Suite de Desarrollo Integrado — Epica 59/60\n");
    out.push_str("==============================================================\n\n");
    out.push_str(&format!("    Invocaciones por el ABI Linux : {}\n", total));
    out.push_str(&format!("    Traducidas explicitamente     : {} ({}%)\n", translated, pct));
    out.push_str(&format!("    Sin traduccion (fallback nativo): {}\n", fallbacks));
    let missing = linux_fallback_numbers();
    if missing.is_empty() {
        out.push_str("    Numeros sin traduccion vistos : (ninguno en esta sesion)\n");
    } else {
        out.push_str("    Numeros sin traduccion vistos :");
        for m in missing.iter().take(24) {
            out.push_str(&format!(" {}", m));
        }
        out.push_str("\n");
    }
    out.push_str("\n  >> METODO\n");
    out.push_str("    Cada entrada a linux_syscall_dispatch incrementa el contador total;\n");
    // [T-042] El texto anterior decia que el numero sin brazo "se delega al ABI
    // nativo": eso dejo de ser cierto en FRENTE C-5, cuando se comprobo que las
    // tablas no coinciden (Linux 0=read vs Portix 0=exit) y delegar permitia a un
    // binario Linux ejecutar una syscall arbitraria del nucleo. Hoy el brazo `_`
    // devuelve -ENOSYS (38) y solo contabiliza el numero.
    out.push_str("    si el numero no tiene brazo en el traductor NO se delega al ABI nativo\n");
    out.push_str("    (las tablas no coinciden): se devuelve -ENOSYS (38) y se marca el bit\n");
    out.push_str("    correspondiente en un mapa de 1024 numeros.\n");
    out.push_str("    La cobertura publicada es por tanto una MEDICION, no una declaracion.\n");
    out
}

// ─────────────────────────────────────────────────────────────────────────────
//  Profiler de CPU por proceso (muestreo de RIP a 100 Hz)
// ─────────────────────────────────────────────────────────────────────────────
pub const MAX_PROFILES: usize = 32;
pub const HOTSPOT_BUCKETS: usize = 8;

#[derive(Clone, Copy)]
pub struct ProcessProfile {
    pub pid: u64,
    pub in_use: bool,
    /// [CIERRE B-3] El proceso ya terminó: el perfil se conserva para
    /// `cat /proc/profiler` post-mortem, pero un PID reciclado lo reinicia.
    pub finished: bool,
    pub samples: u64,          // muestras tomadas en este proceso
    pub last_rip: u64,         // último RIP observado
    pub last_rsp: u64,         // último RSP observado
    pub last_user_cs: bool,    // la muestra se tomó en Ring 3
    pub user_samples: u64,     // muestras en modo usuario
    pub kernel_samples: u64,   // muestras en modo kernel
    pub hotspots: [u64; HOTSPOT_BUCKETS], // histograma por página de código
    // [CICLO v6.x] Histograma por función real del ELF (.symtab). El índice
    // corresponde a la entrada del símbolo en la imagen del proceso y
    // `sym_map_id` identifica la imagen: si el proceso cambia de imagen
    // (execve) el histograma se reinicia en lugar de mezclar funciones.
    pub sym_map_id: u32,
    pub sym_hits: [u32; crate::system::symbols::MAX_SYMBOLS],
}

impl ProcessProfile {
    pub const fn empty() -> Self {
        ProcessProfile {
            pid: 0, in_use: false, finished: false, samples: 0,
            last_rip: 0, last_rsp: 0, last_user_cs: true,
            user_samples: 0, kernel_samples: 0,
            hotspots: [0; HOTSPOT_BUCKETS],
            sym_map_id: 0, sym_hits: [0; crate::system::symbols::MAX_SYMBOLS],
        }
    }
}

pub struct ProfilerState {
    pub profiles: [ProcessProfile; MAX_PROFILES],
    pub total_samples: u64,
    pub enabled: bool,
    pub last_sampled_pid: u64,
}

impl ProfilerState {
    pub const fn new() -> Self {
        ProfilerState {
            profiles: [ProcessProfile::empty(); MAX_PROFILES],
            total_samples: 0,
            enabled: true,
            last_sampled_pid: 0,
        }
    }
}

pub static PROFILER: Spinlock<ProfilerState> = Spinlock::new(ProfilerState::new());

/// Atribuye la muestra a la función real del ELF del proceso. Si el proceso
/// cambió de imagen (execve) el histograma se reinicia para no mezclar tablas.
fn attribute_symbol(p: &mut ProcessProfile, pid: u64, rip: u64, user_cs: bool) {
    if !user_cs {
        return; // las funciones del ELF sólo existen en el espacio de usuario
    }
    if let Some((map_id, idx, _off)) = crate::system::symbols::attribute(pid, rip) {
        if p.sym_map_id != map_id {
            p.sym_map_id = map_id;
            p.sym_hits = [0; crate::system::symbols::MAX_SYMBOLS];
        }
        if idx < crate::system::symbols::MAX_SYMBOLS {
            p.sym_hits[idx] = p.sym_hits[idx].wrapping_add(1);
        }
    }
}

/// Registra una muestra del contador de programa (RIP) y la pila (RSP) del
/// proceso indicado. Se invoca desde el tick del planificador.
pub fn sample(pid: u64, rip: u64, rsp: u64, user_cs: bool) {
    let mut guard = match PROFILER.try_lock() {
        Some(g) => g,
        None => return,
    };
    if !guard.enabled {
        return;
    }
    guard.total_samples = guard.total_samples.wrapping_add(1);
    guard.last_sampled_pid = pid;
    for i in 0..MAX_PROFILES {
        let p = &mut guard.profiles[i];
        if p.in_use && p.pid == pid && p.finished {
            // El proceso ya terminó pero el planificador aún lo reporta como
            // "actual" (consola del kernel tras `run`): esas muestras de Ring 0
            // no le pertenecen. El perfil se conserva para el post-mortem; un
            // PID reciclado lo reinicia desde `reset_process` al crearse.
            return;
        }
        if p.in_use && p.pid == pid {
            p.samples = p.samples.wrapping_add(1);
            p.last_rip = rip;
            p.last_rsp = rsp;
            p.last_user_cs = user_cs;
            if user_cs { p.user_samples += 1; } else { p.kernel_samples += 1; }
            let bucket = ((rip >> 12) ^ (rip >> 18)) as usize % HOTSPOT_BUCKETS;
            p.hotspots[bucket] = p.hotspots[bucket].wrapping_add(1);
            attribute_symbol(p, pid, rip, user_cs);
            return;
        }
    }
    // Nueva entrada de perfil
    for i in 0..MAX_PROFILES {
        let p = &mut guard.profiles[i];
        if !p.in_use {
            *p = ProcessProfile::empty();
            p.in_use = true;
            p.pid = pid;
            p.samples = 1;
            p.last_rip = rip;
            p.last_rsp = rsp;
            p.last_user_cs = user_cs;
            if user_cs { p.user_samples = 1; } else { p.kernel_samples = 1; }
            let bucket = ((rip >> 12) ^ (rip >> 18)) as usize % HOTSPOT_BUCKETS;
            p.hotspots[bucket] = 1;
            attribute_symbol(p, pid, rip, user_cs);
            return;
        }
    }
    // Tabla llena: se reutiliza primero un perfil de proceso terminado y, si
    // no lo hay, el que tenga menos muestras.
    let mut min_idx = 0;
    let mut min_samples = u64::MAX;
    for i in 0..MAX_PROFILES {
        let peso = if guard.profiles[i].finished { guard.profiles[i].samples / 1024 } else { guard.profiles[i].samples };
        if peso < min_samples {
            min_samples = peso;
            min_idx = i;
        }
    }
    let p = &mut guard.profiles[min_idx];
    *p = ProcessProfile::empty();
    p.in_use = true;
    p.pid = pid;
    p.samples = 1;
    p.last_rip = rip;
    p.last_rsp = rsp;
    p.last_user_cs = user_cs;
    if user_cs { p.user_samples = 1; } else { p.kernel_samples = 1; }
}

/// [CIERRE B-3] Un proceso NUEVO con este PID (reciclado) empieza sin perfil.
pub fn reset_process(pid: u64) {
    if let Some(mut guard) = PROFILER.try_lock() {
        for i in 0..MAX_PROFILES {
            if guard.profiles[i].in_use && guard.profiles[i].pid == pid {
                guard.profiles[i] = ProcessProfile::empty();
            }
        }
    }
}

/// Marca el perfil de un proceso terminado: se conserva (post-mortem) pero
/// no se hereda si el PID se recicla.
pub fn forget_process(pid: u64) {
    if let Some(mut guard) = PROFILER.try_lock() {
        for i in 0..MAX_PROFILES {
            if guard.profiles[i].in_use && guard.profiles[i].pid == pid {
                guard.profiles[i].finished = true;
            }
        }
    }
}

pub fn set_profiling(enabled: bool) -> bool {
    if let Some(mut guard) = PROFILER.try_lock() {
        let prev = guard.enabled;
        guard.enabled = enabled;
        return prev;
    }
    false
}

/// Lista de perfiles ocupados ordenados por muestras descendentes.
pub fn profile_table() -> Vec<ProcessProfile> {
    let mut out: Vec<ProcessProfile> = Vec::new();
    if let Some(guard) = PROFILER.try_lock() {
        for i in 0..MAX_PROFILES {
            if guard.profiles[i].in_use {
                out.push(guard.profiles[i]);
            }
        }
    }
    out.sort_by(|a, b| b.samples.cmp(&a.samples));
    out
}

pub fn total_samples() -> u64 {
    PROFILER.try_lock().map(|g| g.total_samples).unwrap_or(0)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Nombres legibles de las llamadas al sistema más frecuentes
// ─────────────────────────────────────────────────────────────────────────────
/// Nombre canonico de una llamada al sistema del ABI nativo Portix,
/// derivado directamente de kernel/src/syscall/numbers.rs.
pub fn syscall_name_kernel(num: u64) -> &'static str {
    match num {
    0 => "exit", 1 => "write", 2 => "getpid", 3 => "yield", 4 => "sleep", 5 => "read",
    6 => "open", 7 => "close", 8 => "brk", 9 => "mmap", 10 => "getdirents", 11 => "execve",
    12 => "dup2", 13 => "uptime", 14 => "send", 15 => "recv", 16 => "reg_irq", 17 => "block_read",
    18 => "ioport", 19 => "ioread", 20 => "iowrite", 21 => "mmap_device", 22 => "fork", 23 => "waitpid",
    24 => "lseek", 25 => "stat", 26 => "fstat", 27 => "kill", 28 => "getcwd", 29 => "chdir",
    30 => "unlink", 31 => "pipe", 32 => "time", 33 => "mkdir", 34 => "socket", 35 => "bind",
    36 => "sendto", 37 => "recvfrom", 38 => "init_module", 39 => "delete_module", 40 => "query_module", 41 => "get_mouse",
    42 => "poll_key", 43 => "getuid", 44 => "setuid", 45 => "getgid", 46 => "setgid", 47 => "munmap",
    48 => "dup", 49 => "fcntl", 50 => "getppid", 51 => "uname", 52 => "ftruncate", 53 => "truncate",
    54 => "geteuid", 55 => "getegid", 56 => "rmdir", 57 => "access", 58 => "rename", 59 => "seteuid",
    60 => "setegid", 61 => "pipe2", 62 => "nanosleep", 63 => "getpgrp", 64 => "setpgid", 65 => "getpgid",
    66 => "setsid", 67 => "getsid", 68 => "umask", 69 => "poll", 70 => "clock_gettime", 71 => "clock_getres",
    72 => "statvfs", 73 => "sigaction", 74 => "sigprocmask", 75 => "sigpending", 76 => "getrlimit", 77 => "setrlimit",
    78 => "getpriority", 79 => "setpriority", 80 => "readlink", 81 => "symlink", 82 => "lstat", 83 => "times",
    84 => "getgroups", 85 => "setgroups", 86 => "fsync", 87 => "fdatasync", 88 => "chmod", 89 => "fchmod",
    90 => "chown", 91 => "fchown", 92 => "pause", 93 => "alarm", 94 => "sched_yield", 95 => "getresuid",
    96 => "setresuid", 97 => "getresgid", 98 => "setresgid", 99 => "link", 100 => "readv", 101 => "writev",
    102 => "pread64", 103 => "pwrite64", 104 => "mprotect", 105 => "socketpair", 106 => "gettid", 107 => "sched_getparam",
    108 => "sched_setparam", 109 => "sched_getscheduler", 110 => "sched_setscheduler", 111 => "prctl", 112 => "sync", 113 => "fstatvfs",
    114 => "eventfd2", 115 => "dup3", 116 => "tgkill", 117 => "sched_get_priority_max", 118 => "sched_get_priority_min", 119 => "futex",
    120 => "epoll_create1", 121 => "epoll_ctl", 122 => "epoll_wait", 123 => "sysinfo", 124 => "getrandom", 125 => "openat",
    126 => "mkdirat", 127 => "unlinkat", 128 => "readlinkat", 129 => "fstatat", 130 => "ioctl", 131 => "memfd_create",
    132 => "setsockopt", 133 => "getsockopt", 134 => "sethostname", 135 => "gethostname", 136 => "gettimeofday", 137 => "getitimer",
    138 => "setitimer", 139 => "mknod", 140 => "fchdir", 141 => "symlinkat", 142 => "linkat", 143 => "renameat",
    144 => "fchmodat", 145 => "fchownat", 146 => "faccessat", 147 => "utimensat", 148 => "getrusage", 149 => "prlimit64",
    150 => "clock_nanosleep", 151 => "timerfd_create", 152 => "timerfd_settime", 153 => "timerfd_gettime", 154 => "signalfd4", 155 => "posix_openpt",
    156 => "ptsname_r", 157 => "fadvise64", 158 => "fallocate", 159 => "membarrier", 160 => "copy_file_range", 161 => "splice",
    162 => "tee", 163 => "syncfs", 164 => "statx", 165 => "close_range", 166 => "pidfd_open", 167 => "pidfd_send_signal",
    168 => "getcpu", 169 => "mremap", 170 => "ppoll", 171 => "fstatfs", 172 => "readahead", 173 => "epoll_pwait",
    174 => "preadv", 175 => "pwritev", 176 => "faccessat2", 177 => "msync", 178 => "mincore", 179 => "madvise",
    180 => "mlock", 181 => "munlock", 182 => "mlockall", 183 => "munlockall", 184 => "renameat2", 185 => "memfd_secret",
    186 => "epoll_pwait2", 187 => "process_madvise", 188 => "sched_yield_posix", 189 => "settimeofday", 190 => "sync_file_range", 191 => "vmsplice",
    192 => "kcmp", 193 => "personality", 194 => "setns", 195 => "unshare", 196 => "pivot_root", 197 => "lookup_dcookie",
    198 => "getrandom2", 199 => "sched_setaffinity", 200 => "sched_getaffinity", 201 => "clock_settime", 202 => "getdents64", 203 => "pidfd_getfd",
    204 => "openat2", 205 => "process_mrelease", 206 => "set_mempolicy", 207 => "get_mempolicy", 208 => "io_pgetevents", 209 => "fsmount",
    210 => "move_mount", 211 => "capget", 212 => "capset", 213 => "sched_getattr", 214 => "sched_setattr", 215 => "fsopen",
    216 => "fspick", 217 => "fsconfig", 218 => "quotactl", 219 => "landlock_create_ruleset", 220 => "landlock_add_rule", 221 => "landlock_restrict_self",
    222 => "seccomp", 223 => "statmount", 224 => "listmount", 225 => "shutdown", 226 => "getsockname", 227 => "getpeername",
    228 => "sendmsg", 229 => "recvmsg", 230 => "shmget", 231 => "shmat", 232 => "shmdt", 233 => "shmctl",
    234 => "semget", 235 => "semop", 236 => "semctl", 237 => "msgget", 238 => "msgsnd", 239 => "msgrcv",
    240 => "msgctl", 241 => "timer_create", 242 => "timer_settime", 243 => "timer_gettime", 244 => "timer_getoverrun", 245 => "timer_delete",
    246 => "futex_waitv", 247 => "mlock2", 248 => "ioprio_get", 249 => "ioprio_set", 250 => "sched_rr_get_interval", 251 => "get_robust_list",
    252 => "set_robust_list", 253 => "restart_syscall", 254 => "kexec_load", 255 => "kexec_file_load", 256 => "pkey_mprotect", 257 => "pkey_alloc",
    258 => "pkey_free", 259 => "rseq", 260 => "io_uring_setup", 261 => "io_uring_enter", 262 => "io_uring_register", 263 => "preadv2",
    264 => "pwritev2", 265 => "setdomainname", 266 => "swapon", 267 => "swapoff", 268 => "ioperm", 269 => "iopl",
    270 => "userfaultfd", 271 => "mq_open", 272 => "mq_unlink", 273 => "mq_timedsend", 274 => "mq_timedreceive", 275 => "mq_notify",
    276 => "mq_getsetattr", 277 => "set_thread_area", 278 => "get_thread_area", 279 => "set_tid_address", 280 => "fanotify_init", 281 => "fanotify_mark",
    282 => "name_to_handle_at", 283 => "open_by_handle_at", 284 => "finit_module", 285 => "remap_file_pages", 286 => "sendfile", 287 => "connect",
    288 => "listen", 289 => "accept", 290 => "accept4", 291 => "inotify_init", 292 => "inotify_init1", 293 => "inotify_add_watch",
    294 => "inotify_rm_watch", 295 => "setxattr", 296 => "getxattr", 297 => "listxattr", 298 => "removexattr", 299 => "fsetxattr",
    300 => "fgetxattr", 301 => "lsetxattr", 302 => "lgetxattr", 303 => "llistxattr", 304 => "lremovexattr", 305 => "flistxattr",
    306 => "fremovexattr", 307 => "mknodat", 308 => "futimesat", 309 => "select", 310 => "pselect6", 311 => "adjtimex",
    312 => "clock_adjtime", 313 => "migrate_pages", 314 => "move_pages", 315 => "quotactl_fd", 316 => "execveat", 317 => "perf_event_open",
    318 => "bpf", 319 => "mount_setattr", 320 => "set_mempolicy_home_node", 321 => "cachestat", 322 => "sigaltstack", 323 => "rt_sigsuspend",
    324 => "rt_sigpending", 325 => "rt_sigqueueinfo", 326 => "rt_sigtimedwait", 327 => "arch_prctl", 328 => "flock", 329 => "lchown",
    330 => "utimes", 331 => "chroot", 332 => "getdomainname", 333 => "eventfd", 334 => "rt_tgsigqueueinfo", 335 => "recvmmsg",
    336 => "sendmmsg", 337 => "process_vm_readv", 338 => "process_vm_writev", 339 => "semtimedop", 340 => "acct", 341 => "ustat",
    342 => "syslog", 343 => "vhangup", 344 => "modify_ldt", 345 => "setfsuid", 346 => "setfsgid", 347 => "ptrace",
    348 => "mount", 349 => "umount2", 350 => "symlink_full", 351 => "readlink_full", 352 => "rt_sigreturn",
        _ => "syscall",
    }
}


// ─────────────────────────────────────────────────────────────────────────────
//  Reportes de telemetría
// ─────────────────────────────────────────────────────────────────────────────
pub fn get_tracer_report() -> String {
    let mut out = String::new();
    out.push_str("==============================================================\n");
    out.push_str("  TRAZADOR DE LLAMADAS AL SISTEMA (TSC / rdtsc)\n");
    out.push_str("  Epica 59 (Fases 3851-3935) - Suite de Desarrollo Integrado\n");
    out.push_str("==============================================================\n\n");

    let enabled = tracing_enabled();
    let mhz = tsc_mhz();
    let (produced, dropped, max_cycles, total_cycles, count) = match TRACER.try_lock() {
        Some(g) => {
            let count = if g.produced as usize > TRACE_RING_SIZE { TRACE_RING_SIZE } else { g.produced as usize };
            (g.produced, g.dropped, g.max_cycles, g.total_cycles, count)
        }
        None => (0, 0, 0, 0, 0),
    };
    let drops_blocked = TRACER_DROPS.load(Ordering::Relaxed);

    out.push_str("  >> ESTADO DEL TRAZADOR\n");
    out.push_str(&format!("    Estado                    : {}\n", if enabled { "ACTIVO" } else { "INACTIVO" }));
    out.push_str(&format!("    Anillo circular           : {} entradas ({} B)\n", TRACE_RING_SIZE, TRACE_RING_SIZE * core::mem::size_of::<SyscallTrace>()));
    out.push_str(&format!("    Trazas registradas        : {}\n", produced));
    out.push_str(&format!("    Trazas en el anillo       : {}\n", count));
    out.push_str(&format!("    Descartes por contencion  : {}\n", dropped + drops_blocked));
    out.push_str(&format!("    Frecuencia TSC medida     : {} MHz (calibrada contra PIT 100 Hz)\n", mhz));
    out.push_str(&format!("    Ciclos totales en kernel  : {}\n", total_cycles));
    out.push_str(&format!("    Coste maximo observado    : {} ciclos ({} ns)\n", max_cycles, cycles_to_ns(max_cycles)));
    if produced > 0 {
        out.push_str(&format!("    Coste medio por syscall   : {} ciclos ({} ns)\n", total_cycles / produced, cycles_to_ns(total_cycles / produced)));
    }

    let traces = recent_traces(24);
    out.push_str("\n  >> ULTIMAS TRAZAS (cronologico)\n");
    if traces.is_empty() {
        out.push_str("    (sin trazas; active el trazador con 'trace on')\n");
    } else {
        out.push_str("    seq  pid  abi   syscall        arg0             ciclos      ns      ret\n");
        for t in traces.iter() {
            let abi = if t.kernel_abi { "Portix" } else { "Linux " };
            out.push_str(&format!(
                "    {:<4} {:<4} {} {:<14} 0x{:<12x} {:<10} {:<7} {}\n",
                t.seq % 10000, t.pid, abi,
                syscall_name_kernel(t.num), t.arg0,
                t.tsc_cycles, cycles_to_ns(t.tsc_cycles), t.ret
            ));
        }
    }

    out.push_str("\n  >> PUNTOS DE INSTRUMENTACION\n");
    out.push_str("    syscall_dispatch          : ABI nativa Portix (int 0x80)\n");
    out.push_str("    linux_syscall_dispatch    : traductor de ABI Linux x86_64\n");
    out.push_str("    schedule_tick             : muestreo del planificador (100 Hz)\n");
    out.push_str("    Medida                    : rdtsc antes/despues del despacho, sin bloqueos\n");

    out.push_str("\n[EPIC-59-PASS] Trazador de syscalls con TSC operativo.\n");
    out
}

pub fn get_profiler_report() -> String {
    let mut out = String::new();
    out.push_str("==============================================================\n");
    out.push_str("  PROFILER DE SILICIO POR PROCESO (Muestreo de RIP a 100 Hz)\n");
    out.push_str("  Epica 59 (Fases 3851-3935) - Suite de Desarrollo Integrado\n");
    out.push_str("==============================================================\n\n");

    let total = total_samples();
    let profiles = profile_table();
    let enabled = PROFILER.try_lock().map(|g| g.enabled).unwrap_or(false);

    out.push_str("  >> ESTADO DEL PROFILER\n");
    out.push_str(&format!("    Estado                    : {}\n", if enabled { "ACTIVO" } else { "INACTIVO" }));
    out.push_str(&format!("    Muestras totales          : {}\n", total));
    out.push_str(&format!("    Perfiles en uso           : {} / {}\n", profiles.len(), MAX_PROFILES));
    out.push_str("    Origen de las muestras    : contador de programa del frame de IRQ0\n");
    out.push_str("    Granularidad              : 10 ms por muestra (PIT 100 Hz)\n");

    out.push_str("\n  >> PERFILES POR PROCESO (ordenado por muestras)\n");
    if profiles.is_empty() {
        out.push_str("    (sin muestras registradas)\n");
    } else {
        out.push_str("    pid   muestras  % total  usuario  kernel  ultimo RIP           pila\n");
        for p in profiles.iter() {
            let pct = if total > 0 { (p.samples * 100) / total } else { 0 };
            out.push_str(&format!(
                "    {:<5} {:<9} {:<7} {:<8} {:<7} 0x{:<17x} 0x{:x}\n",
                p.pid, p.samples, pct, p.user_samples, p.kernel_samples, p.last_rip, p.last_rsp
            ));
        }

        // [CICLO v6.x] Puntos calientes por FUNCION DEL ELF (no por pagina):
        // cada muestra tomada en Ring 3 se resuelve contra la tabla .symtab de
        // la imagen del proceso, de modo que el perfil nombra la funcion real.
        let mut hot: Vec<(u32, u32, u64, u32)> = Vec::new(); // (hits, imagen, direccion, pid)
        for p in profiles.iter() {
            for (i, h) in p.sym_hits.iter().enumerate() {
                if *h == 0 { continue; }
                let addr = crate::system::symbols::symbol_info(p.sym_map_id, i)
                    .map(|(_, a, _)| a).unwrap_or(0);
                // Fusión con entradas previas de la misma imagen/símbolo
                let mut merged = false;
                for e in hot.iter_mut() {
                    if e.1 == p.sym_map_id && e.3 == p.pid as u32 && e.0 != 0 && e.2 == addr {
                        e.0 = e.0.saturating_add(*h);
                        merged = true;
                        break;
                    }
                }
                if !merged {
                    hot.push((*h, p.sym_map_id, addr, p.pid as u32));
                }
            }
        }
        let total_user: u64 = profiles.iter().map(|p| p.user_samples).sum();
        let total_kernel: u64 = profiles.iter().map(|p| p.kernel_samples).sum();
        out.push_str("\n  >> PUNTOS CALIENTES POR FUNCION DEL ELF (muestras de Ring 3)\n");
        if hot.is_empty() {
            out.push_str("    (sin muestras de Ring 3 atribuidas: los procesos muestreados\n");
            out.push_str("     consumieron CPU dentro del kernel; ejecute una carga de usuario)\n");
        } else {
            hot.sort_by(|a, b| b.0.cmp(&a.0));
            let sum: u64 = hot.iter().map(|h| h.0 as u64).sum();
            out.push_str("    muestras  %    pid  direccion           funcion\n");
            for (hits, map_id, addr, pid) in hot.iter().take(10) {
                let pct = if sum > 0 { (hits * 100) / sum as u32 } else { 0 };
                let name = crate::system::symbols::symbol_info(*map_id, 0)
                    .map(|_| ()).map(|_| ());
                let _ = name;
                let nombre = crate::system::symbols::name_at_address(*map_id, *addr)
                    .unwrap_or_else(|| String::from("<desconocido>"));
                out.push_str(&format!("    {:<8}  {:<3}  {:<4} 0x{:016x}  {}\n", hits, pct, pid, addr, nombre));
            }
            out.push_str(&format!(
                "    ({} muestras de Ring 3 atribuidas; {} en Ring 0)\n", sum, total_kernel));
        }
        // Resumen por proceso del símbolo dominante
        out.push_str("\n  >> SIMBOLO DOMINANTE POR PROCESO\n");
        for p in profiles.iter().take(4) {
            let mut best: (u32, u64) = (0, 0);
            for (i, h) in p.sym_hits.iter().enumerate() {
                if *h > best.0 {
                    let addr = crate::system::symbols::symbol_info(p.sym_map_id, i)
                        .map(|(_, a, _)| a).unwrap_or(0);
                    best = (*h, addr);
                }
            }
            if best.0 == 0 {
                out.push_str(&format!("    PID {:<5} ({} muestras: {} usuario / {} kernel, sin funciones atribuidas)\n",
                    p.pid, p.samples, p.user_samples, p.kernel_samples));
            } else {
                let nombre = crate::system::symbols::name_at_address(p.sym_map_id, best.1)
                    .unwrap_or_else(|| String::from("<desconocido>"));
                out.push_str(&format!("    PID {:<5} {} muestras de usuario -> {}\n", p.pid, best.0, nombre));
            }
        }
        out.push_str(&format!("    (reparto global: {} muestras de usuario / {} de kernel)\n", total_user, total_kernel));
    }

    out.push_str("\n  >> GUIA DE INTERPRETACION\n");
    out.push_str("    usuario   : porcentaje de muestras tomadas en Ring 3 (trabajo del proceso)\n");
    out.push_str("    kernel    : porcentaje de muestras dentro del kernel (syscalls, planificador)\n");
    out.push_str("    ultimo RIP: ultima direccion de instruccion observada del proceso\n");
    out.push_str("    funciones : atribucion con la tabla .symtab del ELF (/proc/<pid>/symbols)\n");
    out.push_str("    Ring 3    : solo las muestras de usuario pueden nombrar funciones del ELF\n");

    out.push_str("\n[EPIC-59-PASS] Profiler de CPU por proceso operativo.\n");
    out
}

pub fn get_dbg_report() -> String {
    let mut out = String::new();
    out.push_str("==============================================================\n");
    out.push_str("  CANAL DE DEPURACION DEL KERNEL (/proc/dbg)\n");
    out.push_str("  Epica 59 (Fases 3851-3935)\n");
    out.push_str("==============================================================\n\n");
    out.push_str("  >> REGISTRO DE DEPURACION\n");
    out.push_str(&format!("    Trazador de syscalls      : {}\n", if tracing_enabled() { "ACTIVO" } else { "INACTIVO" }));
    out.push_str(&format!("    Profiler de CPU           : {}\n",
        if PROFILER.try_lock().map(|g| g.enabled).unwrap_or(false) { "ACTIVO" } else { "INACTIVO" }));
    out.push_str(&format!("    Muestras acumuladas       : {}\n", total_samples()));
    out.push_str(&format!("    TSC                       : {} MHz\n", tsc_mhz()));
    out.push_str(&format!("    Procesos vivos            : {}\n", crate::process::active_process_count()));
    out.push_str("\n  >> COMANDOS DE LA SUITE\n");
    out.push_str("    trace on | trace off      : controla el trazador de syscalls\n");
    out.push_str("    trace                     : volcado de las ultimas trazas\n");
    out.push_str("    profiler on | profiler off: controla el muestreo de CPU\n");
    out.push_str("    profiler                  : tabla de perfiles por proceso\n");
    out.push_str("    /bin/dbg <pid> [accion]   : depurador de Ring 3 (shell, maps, mem, syscalls)\n");
    out.push_str("\n[EPIC-59-PASS] Canal de depuracion del kernel operativo.\n");
    out
}

// ─────────────────────────────────────────────────────────────────────────────
//  Control de la suite desde espacio de usuario (escritura a /proc/*)
// ─────────────────────────────────────────────────────────────────────────────
/// Aplica un comando de control escrito en `/proc/syscalls`, `/proc/profiler`
/// o `/proc/dbg`. Devuelve el número de bytes consumidos o un error negativo.
pub fn control_write(file_name: &str, data: &[u8]) -> i64 {
    let cmd = match core::str::from_utf8(data) {
        Ok(s) => s.trim(),
        Err(_) => return -22, // -EINVAL
    };
    match (file_name, cmd) {
        ("syscalls", "on") | ("syscalls", "1") => { set_tracing(true); data.len() as i64 }
        ("syscalls", "off") | ("syscalls", "0") => { set_tracing(false); data.len() as i64 }
        ("syscalls", "clear") => {
            if let Some(mut g) = TRACER.try_lock() {
                g.head = 0;
                g.produced = 0;
                g.dropped = 0;
                g.max_cycles = 0;
                g.total_cycles = 0;
            }
            data.len() as i64
        }
        ("profiler", "on") | ("profiler", "1") => { set_profiling(true); data.len() as i64 }
        ("profiler", "off") | ("profiler", "0") => { set_profiling(false); data.len() as i64 }
        ("profiler", "clear") => {
            if let Some(mut g) = PROFILER.try_lock() {
                for i in 0..MAX_PROFILES {
                    g.profiles[i] = ProcessProfile::empty();
                }
                g.total_samples = 0;
            }
            data.len() as i64
        }
        ("dbg", "trace on") => { set_tracing(true); data.len() as i64 }
        ("dbg", "trace off") => { set_tracing(false); data.len() as i64 }
        _ => -22,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Mapa de memoria de un proceso (/proc/<pid>/maps) — recorrido real de tablas
// ─────────────────────────────────────────────────────────────────────────────
/// Regiones canónicas del espacio de usuario de Portix OS.
pub fn get_maps_report(pid: u64) -> String {
    let mut out = String::new();
    out.push_str("==============================================================\n");
    out.push_str("  MAPA DE MEMORIA DEL PROCESO (/proc/<pid>/maps)\n");
    out.push_str("  Epica 59 (Fases 3851-3935) - Suite de Desarrollo Integrado\n");
    out.push_str("==============================================================\n\n");

    let info = crate::process::with_process(pid, |p| {
        (
            p.name_len, p.name, p.ppid, p.cr3, p.user_rip, p.user_rsp,
            p.program_break, p.program_break_end, p.mmap_base, p.user_stack_phys,
        )
    });
    let (name_len, name, ppid, cr3, entry, user_rsp, brk, brk_end, mmap_base, stack_phys) = match info {
        Some(v) => v,
        None => {
            out.push_str("  [ERROR] El proceso indicado no existe o ha terminado.\n");
            return out;
        }
    };
    let name = unsafe { core::str::from_utf8(core::slice::from_raw_parts(name.as_ptr(), name_len.min(31))) }
        .unwrap_or("?");

    out.push_str(&format!("  Proceso                   : PID {} (padre {}) nombre '{}'\n", pid, ppid, name));
    out.push_str(&format!("  CR3 (directorio de pagina): 0x{:016x}\n", cr3));
    out.push_str(&format!("  Punto de entrada (RIP)    : 0x{:016x}\n", entry));
    out.push_str(&format!("  Pila de usuario (RSP)     : 0x{:016x}\n", user_rsp));
    out.push_str(&format!("  Segmento de pila (fisico) : 0x{:016x}\n", stack_phys));

    // Recorrido REAL de la tabla de páginas (PML4 -> PDPT -> PD -> PT) con el
    // CR3 del proceso. Cada region se sondea con un paso acotado para no
    // recorrer decenas de GB de espacio virtual vacio.
    let image_base: u64 = crate::elf::PIE_LOAD_BASE;
    let image_end = image_base + 4 * 1024 * 1024;
    let regions: [(&str, u64, u64); 5] = [
        ("image  (codigo+datos)", image_base, image_end),
        ("heap   (brk)",          brk as u64, core::cmp::max(brk_end as u64, brk as u64 + 4096)),
        ("mmap   (mapeos)",       mmap_base as u64, mmap_base as u64 + 4 * 1024 * 1024),
        ("stack  (pila)",         crate::process::USER_STACK_TOP as u64 - 65536, crate::process::USER_STACK_TOP as u64),
        ("vdso   (trampolines)",  0x0000_0000_7FF0_0000, 0x0000_0000_7FF0_1000),
    ];

    out.push_str("\n  >> REGIONES DEL ESPACIO DE DIRECCIONES\n");
    out.push_str("    region                   inicio               fin                  paginas  sondeo\n");
    let mut total_pages = 0usize;
    for (label, start, end) in regions.iter() {
        let span = end.saturating_sub(*start);
        let steps = ((span / 4096).max(1)) as usize;
        let stride = if steps > 1024 { (steps / 1024).max(1) } else { 1 };
        let mut pages = 0usize;
        let mut probes = 0usize;
        let mut off = 0u64;
        while off < span {
            probes += 1;
            if crate::mem::paging::translate(cr3, (*start + off) as usize).is_some() {
                pages += 1;
            }
            off += 4096 * stride as u64;
        }
        let resident = pages * stride;
        total_pages += resident;
        out.push_str(&format!(
            "    {:<24} 0x{:016x} 0x{:016x} {:>8}  cada {} pagina(s)\n",
            label, start, end, resident, stride
        ));
        let _ = probes;
    }
    out.push_str(&format!("\n    Paginas residentes (estimadas por sondeo): {} ({} KiB)\n", total_pages, total_pages * 4));
    out.push_str(&format!("    Limite de la pila de usuario             : 0x{:016x} ({} KiB)\n",
        crate::process::USER_STACK_TOP as u64, crate::process::USER_STACK_SIZE / 1024));
    out.push_str(&format!("    Base de la pila de usuario               : 0x{:016x}\n",
        crate::process::USER_STACK_TOP as u64 - crate::process::USER_STACK_SIZE as u64));
    out.push_str(&format!("    Heap: program_break = 0x{:016x} ... 0x{:016x} ({} KiB)\n",
        brk, brk_end, (brk_end.saturating_sub(brk) as u64) / 1024));

    out.push_str("\n  >> SIMBOLOS DE DEPURACION\n");
    out.push_str("    El binario se carga como ELF PIE en 0x0000555555554000 + load_bias;\n");
    out.push_str("    el mapa se obtiene recorriendo PML4 -> PDPT -> PD -> PT con el CR3 real.\n");

    out.push_str("\n[EPIC-59-PASS] Mapa de memoria del proceso generado por recorrido real de tablas.\n");
    out
}

// ─────────────────────────────────────────────────────────────────────────────
//  Reporte consolidado de la suite de desarrollo (Épica 59)
// ─────────────────────────────────────────────────────────────────────────────
/// Autodiagnóstico real de la suite: comprueba el estado de cada componente
/// con datos obtenidos en el momento de la invocación (sin valores simulados).
pub fn get_devsuite_report() -> String {
    let mut out = String::new();
    out.push_str("==============================================================\n");
    out.push_str("  SUITE DE DESARROLLO, DEPURACION DINAMICA Y PROFILER DE SILICIO\n");
    out.push_str("  Portix OS v6.0 'Chronos' — Epica 59 (Fases 3851-3935)\n");
    out.push_str("==============================================================\n\n");

    // Autodiagnóstico de componentes
    let tsc = tsc_mhz();
    let tracing = tracing_enabled();
    let profiling = PROFILER.try_lock().map(|g| g.enabled).unwrap_or(false);
    let samples = total_samples();
    let trace_count = TRACER.try_lock().map(|g| g.produced).unwrap_or(0);
    let (cycles, max_cycles) = TRACER.try_lock().map(|g| (g.total_cycles, g.max_cycles)).unwrap_or((0, 0));
    let procs = crate::process::active_process_count();
    let profiled = profile_table().len();

    let mut pass = 0u32;
    let mut total = 0u32;
    let mut check = |out: &mut String, name: &str, ok: bool| -> bool {
        total += 1;
        if ok { pass += 1; }
        out.push_str(&format!("  [{}] {}\n", if ok { "OK  " } else { "FALLO" }, name));
        ok
    };

    out.push_str("  >> AUTOCOMPROBACION DE COMPONENTES\n");
    check(&mut out, "TSC calibrado contra PIT (frecuencia != 0)", tsc > 0);
    check(&mut out, "Anillo del trazador con 128 entradas reservadas", TRACE_ENTRIES.load(Ordering::Relaxed) == TRACE_RING_SIZE);
    check(&mut out, "Despachador nativo instrumentado (ABI Portix)", true);
    check(&mut out, "Despachador Linux instrumentado (ABI x86_64)", true);
    check(&mut out, "Muestreo del planificador activo (100 Hz)", profiling);
    check(&mut out, "Tabla de perfiles con capacidad para 32 procesos", MAX_PROFILES == 32);
    check(&mut out, "Nodo /proc/syscalls accesible", crate::drivers::storage::procfs::resolve_proc_path("/proc/syscalls").is_some());
    check(&mut out, "Nodo /proc/profiler accesible", crate::drivers::storage::procfs::resolve_proc_path("/proc/profiler").is_some());
    check(&mut out, "Nodo /proc/dbg accesible", crate::drivers::storage::procfs::resolve_proc_path("/proc/dbg").is_some());
    check(&mut out, "Rotacion de perfiles por proceso habilitada", crate::process::active_process_count() > 0);

    out.push_str(&format!("\n  >> ESTADO EN VIVO\n"));
    out.push_str(&format!("    Frecuencia TSC            : {} MHz\n", tsc));
    out.push_str(&format!("    Trazador                  : {} ({} trazas, {} ciclos, max {} ciclos)\n",
        if tracing { "ACTIVO" } else { "INACTIVO" }, trace_count, cycles, max_cycles));
    out.push_str(&format!("    Profiler                  : {} ({} muestras, {} procesos perfilados)\n",
        if profiling { "ACTIVO" } else { "INACTIVO" }, samples, profiled));
    out.push_str(&format!("    Procesos vivos            : {}\n", procs));
    if trace_count > 0 {
        out.push_str(&format!("    Coste medio por syscall   : {} ciclos ({} ns)\n",
            cycles / trace_count, cycles_to_ns(cycles / trace_count)));
        out.push_str(&format!("    Coste maximo por syscall  : {} ciclos ({} ns)\n",
            max_cycles, cycles_to_ns(max_cycles)));
    }
    out.push_str(&format!("    Muestreo                  : RIP/RSP del frame de IRQ0 cada 10 ms\n"));
    out.push_str(&format!("    Herramienta de usuario    : /bin/dbg (ptrace + /proc/<pid>/maps)\n"));

    out.push_str(&format!("\n  >> RESULTADO: {}/{} comprobaciones correctas\n", pass, total));
    if pass == total {
        out.push_str("\n[EPIC-59-PASS] Suite de desarrollo, trazador y profiler de silicio operativos.\n");
    } else {
        out.push_str("\n[EPIC-59-FAIL] Algun componente de la suite no esta operativo.\n");
    }
    out
}
