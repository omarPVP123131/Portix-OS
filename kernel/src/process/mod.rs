use core::alloc::Layout;
use crate::arch::Spinlock;
use crate::drivers::serial;
use crate::mem::paging::{self, PAGE_SIZE};

pub mod types;
mod exit_status;
pub use types::*;

pub const MAX_PROCS: usize = 64; // [b191] 16 ranuras no bastaban: el JVM arranca 11 hilos y la sonda Java 4 más (pthread_create EAGAIN)
pub const KERNEL_STACK_SIZE: usize = 16384;
pub const USER_STACK_SIZE: usize = 65536;
pub const USER_STACK_TOP: usize = 0x7F00_0000_0000;
/// [T-004] Pila de usuario extensible hasta 8 MiB (RLIMIT_STACK de Linux): los
/// 64 KiB iniciales se asignan al crear el proceso y el resto crece por fallo
/// de página en `[USER_STACK_TOP - PILA_MAX, USER_STACK_TOP - USER_STACK_SIZE)`.
pub const PILA_MAX: usize = 8 * 1024 * 1024;
/// Rango de la pila extendida (lo, hi): se copia en `fork` y se libera en `exit`/`execve`.
pub fn pila_extendida() -> (usize, usize) { (USER_STACK_TOP - PILA_MAX, USER_STACK_TOP - USER_STACK_SIZE) }
pub const TIME_SLICE: u64 = 10; // 10 ticks = 100ms at 100Hz

// ── FD table ─────────────────────────────────────────────────────────────
pub const MAX_FDS: usize = 32;
pub const PROGRAM_BREAK_BASE: usize = 0x2000_0000_0000;
pub const MMAP_BASE: usize = 0x3000_0000_0000;
pub const PAGE_SIZE_USIZE: usize = 4096;

// [T-025 E3a] El PID en ejecución es POR CPU: `crate::arch::percpu::pid_actual()` / `fijar_pid_actual()`.

// ── Process Table protegido por Spinlock ──────────────────────────────────
pub struct ProcessTable {
    pub processes: [core::mem::MaybeUninit<Process>; MAX_PROCS],
    pub next_pid: u64,
}

impl ProcessTable {
    pub const fn new() -> Self {
        ProcessTable {
            processes: [core::mem::MaybeUninit::uninit(); MAX_PROCS],
            next_pid: 1,
        }
    }

    pub fn alloc_pid(&mut self) -> Option<u64> {
        let pid = self.next_pid;
        self.next_pid = self.next_pid.checked_add(1)?;
        Some(pid)
    }

    pub fn find_slot(&self) -> Option<usize> {
        for (i, p) in self.processes.iter().enumerate() {
            unsafe {
                if (*p.as_ptr()).state == ProcessState::Dead {
                    return Some(i);
                }
            }
        }
        None
    }

    pub fn current_process_ref(&self) -> Option<&'static mut Process> {
        // [T-025 E3a] Primero el PID que ESTA CPU tiene en ejecución; el
        // barrido por estado Running queda como respaldo del arranque.
        let pid = crate::arch::percpu::pid_actual();
        if pid != 0 {
            for p in &self.processes {
                unsafe {
                    let proc = p.as_ptr() as *mut Process;
                    if (*proc).pid == pid && (*proc).state != ProcessState::Dead {
                        return Some(&mut *proc);
                    }
                }
            }
        }
        for p in &self.processes {
            unsafe {
                let proc = p.as_ptr() as *mut Process;
                if (*proc).state == ProcessState::Running {
                    return Some(&mut *proc);
                }
            }
        }
        None
    }

    fn process_by_pid_ref(&self, pid: u64) -> Option<&'static mut Process> {
        for p in &self.processes {
            unsafe {
                let proc = p.as_ptr() as *mut Process;
                if (*proc).pid == pid {
                    return Some(&mut *proc);
                }
            }
        }
        None
    }
}

pub static COW_PAGES_SAVED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static COW_FAULTS_RESOLVED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn get_cow_metrics() -> (u64, u64) {
    (
        COW_PAGES_SAVED.load(core::sync::atomic::Ordering::Relaxed),
        COW_FAULTS_RESOLVED.load(core::sync::atomic::Ordering::Relaxed),
    )
}

pub fn active_process_count() -> u16 {
    let ptab = PROCESS_TABLE.lock();
    let mut count = 0u16;
    for p in ptab.processes.iter() {
        unsafe {
            if (*p.as_ptr()).state != ProcessState::Dead {
                count += 1;
            }
        }
    }
    count
}

pub static PROCESS_TABLE: Spinlock<ProcessTable> = Spinlock::new(ProcessTable::new());

pub const MAX_PIPES: usize = 64;
pub const PIPE_BUF_SIZE: usize = 4096;

#[derive(Copy, Clone)]
pub struct Pipe {
    pub buf: [u8; PIPE_BUF_SIZE],
    pub head: usize,
    pub tail: usize,
    pub count: usize,
    pub readers: usize,
    pub writers: usize,
    pub in_use: bool,
    /// [CICLO 48] Bytes acumulados escritos/leidos (anclan los SCM_RIGHTS a su mensaje).
    pub total_in: u64,
    pub total_out: u64,
}

impl Pipe {
    pub const fn new() -> Self {
        Pipe {
            buf: [0; PIPE_BUF_SIZE],
            head: 0,
            tail: 0,
            count: 0,
            readers: 0,
            writers: 0,
            in_use: false,
            total_in: 0,
            total_out: 0,
        }
    }
}

/// (bytes escritos en total, bytes leidos en total) de una tuberia.
pub fn pipe_totales(pipe_id: u32) -> (u64, u64) {
    let table = PIPE_TABLE.lock();
    table.pipes.get(pipe_id as usize).map(|p| (p.total_in, p.total_out)).unwrap_or((0, 0))
}

pub struct PipeTable {
    pub pipes: [Pipe; MAX_PIPES],
}

pub static PIPE_TABLE: Spinlock<PipeTable> = Spinlock::new(PipeTable {
    pipes: [Pipe::new(); MAX_PIPES],
});

pub fn pipe_alloc() -> Option<u32> {
    let mut table = PIPE_TABLE.lock();
    for (i, p) in table.pipes.iter_mut().enumerate() {
        if !p.in_use {
            p.in_use = true;
            p.head = 0;
            p.tail = 0;
            p.count = 0;
            p.readers = 1;
            p.writers = 1;
            p.total_in = 0;
            p.total_out = 0;
            return Some(i as u32);
        }
    }
    None
}

pub fn pipe_read_byte(pipe_id: u32) -> Option<u8> {
    let mut table = PIPE_TABLE.lock();
    let p = table.pipes.get_mut(pipe_id as usize)?;
    if !p.in_use || p.count == 0 {
        return None;
    }
    let b = p.buf[p.tail];
    p.tail = (p.tail + 1) % PIPE_BUF_SIZE;
    p.count -= 1;
    p.total_out += 1;
    Some(b)
}

pub fn pipe_peek_bytes(pipe_id: u32, out: &mut [u8]) -> usize {
    let table = PIPE_TABLE.lock();
    let p = match table.pipes.get(pipe_id as usize) {
        Some(p) if p.in_use => p,
        _ => return 0,
    };
    let to_read = out.len().min(p.count);
    let mut tail = p.tail;
    for i in 0..to_read {
        out[i] = p.buf[tail];
        tail = (tail + 1) % PIPE_BUF_SIZE;
    }
    to_read
}

pub fn pipe_write_byte(pipe_id: u32, b: u8) -> bool {
    let mut table = PIPE_TABLE.lock();
    let p = match table.pipes.get_mut(pipe_id as usize) {
        Some(p) => p,
        None => return false,
    };
    if !p.in_use || p.readers == 0 || p.count >= PIPE_BUF_SIZE {
        return false;
    }
    p.buf[p.head] = b;
    p.head = (p.head + 1) % PIPE_BUF_SIZE;
    p.count += 1;
    p.total_in += 1;
    true
}

pub fn find_zombie_child(parent_pid: u64, target_pid: i64, caller_pgid: u64) -> (bool, Option<(u64, i32)>) {
    let guard = PROCESS_TABLE.lock();
    let mut has_children = false;
    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr();
            if (*proc).ppid == parent_pid && (*proc).state != ProcessState::Dead {
                let matches = if target_pid == -1 {
                    true
                } else if target_pid == 0 {
                    (*proc).pgid == caller_pgid
                } else if target_pid < -1 {
                    (*proc).pgid == (-target_pid) as u64
                } else {
                    target_pid as u64 == (*proc).pid
                };

                if matches {
                    has_children = true;
                    if (*proc).state == ProcessState::Zombie {
                        return (true, Some(((*proc).pid, (*proc).wait_status())));
                    }
                }
            }
        }
    }
    (has_children, None)
}

pub fn pipe_close_reader(pipe_id: u32) {
    let mut muerta = false;
    {
        let mut table = PIPE_TABLE.lock();
        if let Some(p) = table.pipes.get_mut(pipe_id as usize) {
            if p.readers > 0 { p.readers -= 1; }
            if p.readers == 0 && p.writers == 0 {
                p.in_use = false;
                muerta = true;
            }
        }
    }
    if muerta { fifo_forget(pipe_id); }
}

pub fn pipe_close_writer(pipe_id: u32) {
    let mut muerta = false;
    {
        let mut table = PIPE_TABLE.lock();
        if let Some(p) = table.pipes.get_mut(pipe_id as usize) {
            if p.writers > 0 { p.writers -= 1; }
            if p.readers == 0 && p.writers == 0 {
                p.in_use = false;
                muerta = true;
            }
        }
    }
    if muerta { fifo_forget(pipe_id); }
}

fn pipe_retain(pipe_id: u32, lector: bool) {
    let mut table = PIPE_TABLE.lock();
    if let Some(p) = table.pipes.get_mut(pipe_id as usize) {
        if lector { p.readers += 1; } else { p.writers += 1; }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  [CICLO 48] FIFO con nombre (mkfifo / mknod S_IFIFO): el nodo vive en ramfs
//  con tipo S_IFIFO y, mientras hay algun extremo abierto, se respalda en una
//  tuberia del nucleo compartida por todos los que abren la misma ruta.
// ─────────────────────────────────────────────────────────────────────────────
struct FifoAbierta {
    ruta: alloc::string::String,
    tuberia: u32,
    /// La tuberia nace con readers=1/writers=1: el primer abridor de cada lado
    /// toma esa referencia base; los siguientes incrementan.
    base_lector: bool,
    base_escritor: bool,
}

static FIFOS: crate::arch::Spinlock<alloc::vec::Vec<FifoAbierta>> = crate::arch::Spinlock::new(alloc::vec::Vec::new());

fn fifo_forget(pipe_id: u32) {
    FIFOS.lock().retain(|f| f.tuberia != pipe_id);
}

/// Abre un FIFO con nombre. `acceso` = flags & 3 (0 lectura, 1 escritura,
/// 2 ambos). Semantica POSIX: la apertura bloquea hasta que exista el
/// extremo contrario (tope de 10 s); con O_NONBLOCK la lectura no bloquea y
/// la escritura sin lector devuelve ENXIO.
pub fn fifo_open(ruta: &str, acceso: u32, nonblock: bool) -> Result<FdType, i32> {
    let (tuberia, lector, escritor) = {
        let mut tabla = FIFOS.lock();
        let pos = match tabla.iter().position(|f| f.ruta == ruta) {
            Some(p) => p,
            None => {
                let id = pipe_alloc().ok_or(-24)?;
                tabla.push(FifoAbierta { ruta: alloc::string::String::from(ruta), tuberia: id, base_lector: true, base_escritor: true });
                tabla.len() - 1
            }
        };
        let f = &mut tabla[pos];
        if acceso == 1 && nonblock && f.base_lector {
            // Nadie ha abierto el lado de lectura todavia.
            if f.base_escritor {
                let id = f.tuberia;
                tabla.remove(pos);
                drop(tabla); // pipe_close_* puede llamar a fifo_forget (toma FIFOS)
                pipe_close_reader(id);
                pipe_close_writer(id);
            }
            return Err(-6); // -ENXIO
        }
        if acceso == 0 || acceso == 2 {
            if f.base_lector { f.base_lector = false; } else { pipe_retain(f.tuberia, true); }
        }
        if acceso == 1 || acceso == 2 {
            if f.base_escritor { f.base_escritor = false; } else { pipe_retain(f.tuberia, false); }
        }
        (f.tuberia, acceso == 0 || acceso == 2, acceso == 1 || acceso == 2)
    };
    // Espera al extremo contrario (apertura bloqueante POSIX).
    if !nonblock && acceso != 2 {
        let tope = crate::time::pit::ticks().wrapping_add(1000);
        loop {
            let listo = {
                let tabla = FIFOS.lock();
                match tabla.iter().find(|f| f.tuberia == tuberia) {
                    Some(f) => if lector { !f.base_escritor } else { !f.base_lector },
                    None => true,
                }
            };
            if listo || crate::time::pit::ticks() >= tope || signals::current_sig_pending() != 0 { break; }
            scheduler::wait_hlt_preemptible();
        }
    }
    Ok(match (lector, escritor) {
        (true, false) => FdType::PipeRead(tuberia),
        (false, true) => FdType::PipeWrite(tuberia),
        _ => FdType::SocketPairEnd { read_pipe: tuberia, write_pipe: tuberia },
    })
}

pub fn pipe_has_readers(pipe_id: u32) -> bool {
    let table = PIPE_TABLE.lock();
    table.pipes.get(pipe_id as usize).map(|p| p.in_use && p.readers > 0).unwrap_or(false)
}

/// [CICLO 47] ¿Hay bytes sin leer en la tuberia? (para el read() bloqueante)
/// [T-004] ¿Queda sitio en la tubería? (false también si no existe o no tiene lectores)
pub fn pipe_has_space(pipe_id: u32) -> bool {
    let table = PIPE_TABLE.lock();
    match table.pipes.get(pipe_id as usize) { Some(p) => p.in_use && p.readers > 0 && p.count < PIPE_BUF_SIZE, None => false }
}

pub fn pipe_has_data(pipe_id: u32) -> bool {
    let table = PIPE_TABLE.lock();
    table.pipes.get(pipe_id as usize).map(|p| p.in_use && p.count > 0).unwrap_or(false)
}

pub fn pipe_has_writers(pipe_id: u32) -> bool {
    let table = PIPE_TABLE.lock();
    table.pipes.get(pipe_id as usize).map(|p| p.in_use && p.writers > 0).unwrap_or(false)
}

/// Ejecuta `f` con una referencia de sólo lectura al proceso `pid`.
/// Devuelve `None` si el proceso no existe o ya está muerto.
pub fn with_process<R>(pid: u64, f: impl FnOnce(&Process) -> R) -> Option<R> {
    let guard = PROCESS_TABLE.lock();
    for p in &guard.processes {
        let proc = unsafe { &*(p.as_ptr() as *const Process) };
        if proc.pid == pid && proc.state != ProcessState::Dead {
            return Some(f(proc));
        }
    }
    None
}

pub fn get_pipes_report() -> alloc::string::String {
    use alloc::format;
    use alloc::string::String;

    let table = PIPE_TABLE.lock();
    let mut in_use_count = 0;
    let mut total_bytes = 0;
    for p in table.pipes.iter() {
        if p.in_use {
            in_use_count += 1;
            total_bytes += p.count;
        }
    }

    let mut out = String::from("╔══════════════════════════════════════════════════════════════════════╗\n");
    out.push_str("║       SUBSISTEMA DE TUBERÍAS POSIX (PIPES & DUP2/DUP3)               ║\n");
    out.push_str("║    Épica 54 (Fases 3426 a 3510) • Búfer Circular 4KB • Multi-Etapa   ║\n");
    out.push_str("╚══════════════════════════════════════════════════════════════════════╝\n\n");
    out.push_str(&format!("Capacidad de Tuberías : {} instancias simultáneas\n", MAX_PIPES));
    out.push_str(&format!("Tamaño de Búfer Pipe  : {} bytes por tubería (Circular Ring)\n", PIPE_BUF_SIZE));
    out.push_str(&format!("Tuberías Activas      : {}\n", in_use_count));
    out.push_str(&format!("Bytes en Tránsito     : {} bytes\n", total_bytes));
    out.push_str("Soporte de Llamadas   : sys_pipe (22), sys_pipe2 (293), sys_dup (32), sys_dup2 (33), sys_dup3 (292)\n");
    out.push_str("Encadenamiento Shell  : /bin/sh con pipes multi-etapa (cmd1 | cmd2 | cmd3) y redirección 2>&1 [OK]\n");
    out.push_str("\n[EPIC-54-PASS] Subsistema de tuberías anónimas y descriptores duplicados verificado.\n");
    out
}

pub const MAX_EVENTFDS: usize = 16;

#[derive(Copy, Clone)]
pub struct EventFdEntry {
    pub val: u64,
    pub flags: u32,
    pub in_use: bool,
    pub ref_count: usize,
}

pub struct EventFdTable {
    pub fds: [EventFdEntry; MAX_EVENTFDS],
}

pub static EVENTFD_TABLE: Spinlock<EventFdTable> = Spinlock::new(EventFdTable {
    fds: [EventFdEntry { val: 0, flags: 0, in_use: false, ref_count: 0 }; MAX_EVENTFDS],
});

pub fn eventfd_alloc(initval: u64, flags: u32) -> Option<u32> {
    let mut table = EVENTFD_TABLE.lock();
    for (i, ef) in table.fds.iter_mut().enumerate() {
        if !ef.in_use {
            ef.val = initval;
            ef.flags = flags;
            ef.in_use = true;
            ef.ref_count = 1;
            return Some(i as u32);
        }
    }
    None
}

pub fn eventfd_retain(id: u32) {
    let mut table = EVENTFD_TABLE.lock();
    if let Some(ef) = table.fds.get_mut(id as usize) {
        if ef.in_use {
            ef.ref_count += 1;
        }
    }
}

pub fn eventfd_close(id: u32) {
    let mut table = EVENTFD_TABLE.lock();
    if let Some(ef) = table.fds.get_mut(id as usize) {
        if ef.in_use {
            if ef.ref_count > 1 {
                ef.ref_count -= 1;
            } else {
                ef.in_use = false;
                ef.ref_count = 0;
                ef.val = 0;
            }
        }
    }
}

pub const MAX_EPOLLS: usize = 16;
pub const MAX_EPOLL_WATCHES: usize = 16;

#[repr(C)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
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

#[derive(Copy, Clone)]
pub struct EpollInstance {
    pub watches: [EpollWatch; MAX_EPOLL_WATCHES],
    pub flags: u32,
    pub in_use: bool,
    pub ref_count: usize,
}

pub struct EpollTable {
    pub instances: [EpollInstance; MAX_EPOLLS],
}

pub static EPOLL_TABLE: Spinlock<EpollTable> = Spinlock::new(EpollTable {
    instances: [EpollInstance {
        watches: [EpollWatch { fd: -1, event: EpollEvent { events: 0, data: 0 }, in_use: false }; MAX_EPOLL_WATCHES],
        flags: 0,
        in_use: false,
        ref_count: 0,
    }; MAX_EPOLLS],
});

pub fn epoll_alloc(flags: u32) -> Option<u32> {
    let mut table = EPOLL_TABLE.lock();
    for (i, inst) in table.instances.iter_mut().enumerate() {
        if !inst.in_use {
            inst.flags = flags;
            inst.in_use = true;
            inst.ref_count = 1;
            for w in inst.watches.iter_mut() {
                w.in_use = false;
                w.fd = -1;
                w.event = EpollEvent { events: 0, data: 0 };
            }
            return Some(i as u32);
        }
    }
    None
}

pub fn epoll_retain(id: u32) {
    let mut table = EPOLL_TABLE.lock();
    if let Some(inst) = table.instances.get_mut(id as usize) {
        if inst.in_use {
            inst.ref_count += 1;
        }
    }
}

pub fn epoll_close(id: u32) {
    let mut table = EPOLL_TABLE.lock();
    if let Some(inst) = table.instances.get_mut(id as usize) {
        if inst.in_use {
            if inst.ref_count > 1 {
                inst.ref_count -= 1;
            } else {
                inst.in_use = false;
                inst.ref_count = 0;
                for w in inst.watches.iter_mut() {
                    w.in_use = false;
                    w.fd = -1;
                }
            }
        }
    }
}

// ── TimerFd subsystem ──────────────────────────────────────────────────
pub const MAX_TIMERFDS: usize = 16;

#[derive(Copy, Clone)]
pub struct TimerFdEntry {
    pub clock_id: i32,
    pub flags: i32,
    pub interval_ticks: u64,
    pub expire_ticks: u64,
    pub expirations: u64,
    pub in_use: bool,
    pub ref_count: usize,
}

pub struct TimerFdTable {
    pub fds: [TimerFdEntry; MAX_TIMERFDS],
}

pub static TIMERFD_TABLE: Spinlock<TimerFdTable> = Spinlock::new(TimerFdTable {
    fds: [TimerFdEntry {
        clock_id: 0,
        flags: 0,
        interval_ticks: 0,
        expire_ticks: 0,
        expirations: 0,
        in_use: false,
        ref_count: 0,
    }; MAX_TIMERFDS],
});

pub fn timerfd_alloc(clock_id: i32, flags: i32) -> Option<u32> {
    let mut table = TIMERFD_TABLE.lock();
    for (i, tf) in table.fds.iter_mut().enumerate() {
        if !tf.in_use {
            tf.clock_id = clock_id;
            tf.flags = flags;
            tf.interval_ticks = 0;
            tf.expire_ticks = 0;
            tf.expirations = 0;
            tf.in_use = true;
            tf.ref_count = 1;
            return Some(i as u32);
        }
    }
    None
}

pub fn timerfd_retain(id: u32) {
    let mut table = TIMERFD_TABLE.lock();
    if let Some(tf) = table.fds.get_mut(id as usize) {
        if tf.in_use {
            tf.ref_count += 1;
        }
    }
}

pub fn timerfd_close(id: u32) {
    let mut table = TIMERFD_TABLE.lock();
    if let Some(tf) = table.fds.get_mut(id as usize) {
        if tf.in_use {
            if tf.ref_count > 1 {
                tf.ref_count -= 1;
            } else {
                tf.in_use = false;
                tf.ref_count = 0;
                tf.expire_ticks = 0;
                tf.expirations = 0;
            }
        }
    }
}

// ── SignalFd subsystem ─────────────────────────────────────────────────
pub const MAX_SIGNALFDS: usize = 16;

#[derive(Copy, Clone)]
pub struct SignalFdEntry {
    pub mask: u64,
    pub flags: i32,
    pub in_use: bool,
    pub ref_count: usize,
}

pub struct SignalFdTable {
    pub fds: [SignalFdEntry; MAX_SIGNALFDS],
}

pub static SIGNALFD_TABLE: Spinlock<SignalFdTable> = Spinlock::new(SignalFdTable {
    fds: [SignalFdEntry { mask: 0, flags: 0, in_use: false, ref_count: 0 }; MAX_SIGNALFDS],
});

pub fn signalfd_alloc(mask: u64, flags: i32) -> Option<u32> {
    let mut table = SIGNALFD_TABLE.lock();
    for (i, sf) in table.fds.iter_mut().enumerate() {
        if !sf.in_use {
            sf.mask = mask;
            sf.flags = flags;
            sf.in_use = true;
            sf.ref_count = 1;
            return Some(i as u32);
        }
    }
    None
}

pub fn signalfd_retain(id: u32) {
    let mut table = SIGNALFD_TABLE.lock();
    if let Some(sf) = table.fds.get_mut(id as usize) {
        if sf.in_use {
            sf.ref_count += 1;
        }
    }
}

pub fn signalfd_close(id: u32) {
    let mut table = SIGNALFD_TABLE.lock();
    if let Some(sf) = table.fds.get_mut(id as usize) {
        if sf.in_use {
            if sf.ref_count > 1 {
                sf.ref_count -= 1;
            } else {
                sf.in_use = false;
                sf.ref_count = 0;
                sf.mask = 0;
            }
        }
    }
}

// ── PTY (Pseudoterminal) subsystem ─────────────────────────────────────
pub const MAX_PTYS: usize = 8;

#[derive(Copy, Clone)]
pub struct PtyEntry {
    pub master_pipe: u32,
    pub slave_pipe: u32,
    pub in_use: bool,
    pub locked: bool,
    pub ref_count: usize,
}

pub struct PtyTable {
    pub ptys: [PtyEntry; MAX_PTYS],
}

pub static PTY_TABLE: Spinlock<PtyTable> = Spinlock::new(PtyTable {
    ptys: [PtyEntry {
        master_pipe: 0,
        slave_pipe: 0,
        in_use: false,
        locked: true,
        ref_count: 0,
    }; MAX_PTYS],
});

pub fn pty_alloc() -> Option<u32> {
    let p0 = pipe_alloc()?;
    let p1 = match pipe_alloc() {
        Some(p) => p,
        None => {
            pipe_close_reader(p0);
            pipe_close_writer(p0);
            return None;
        }
    };
    let mut table = PTY_TABLE.lock();
    for (i, pty) in table.ptys.iter_mut().enumerate() {
        if !pty.in_use {
            pty.master_pipe = p0;
            pty.slave_pipe = p1;
            pty.in_use = true;
            pty.locked = true;
            pty.ref_count = 1;
            return Some(i as u32);
        }
    }
    pipe_close_reader(p0);
    pipe_close_writer(p0);
    pipe_close_reader(p1);
    pipe_close_writer(p1);
    None
}

pub fn pty_retain(id: u32) {
    let mut table = PTY_TABLE.lock();
    if let Some(pty) = table.ptys.get_mut(id as usize) {
        if pty.in_use {
            pty.ref_count += 1;
        }
    }
}

pub fn pty_close(id: u32) {
    let mut table = PTY_TABLE.lock();
    if let Some(pty) = table.ptys.get_mut(id as usize) {
        if pty.in_use {
            if pty.ref_count > 1 {
                pty.ref_count -= 1;
            } else {
                pty.in_use = false;
                pty.ref_count = 0;
                pipe_close_reader(pty.master_pipe);
                pipe_close_writer(pty.master_pipe);
                pipe_close_reader(pty.slave_pipe);
                pipe_close_writer(pty.slave_pipe);
            }
        }
    }
}

// ── POSIX Message Queues ──────────────────────────────────────────────────
pub const MQ_MAX_QUEUES: usize = 16;
pub const MQ_MAX_MSG_COUNT: usize = 16;
pub const MQ_MAX_MSG_SIZE: usize = 256;

#[derive(Copy, Clone)]
pub struct MqMessage {
    pub len: usize,
    pub prio: u32,
    pub data: [u8; MQ_MAX_MSG_SIZE],
}

#[derive(Copy, Clone)]
pub struct PosixMqQueue {
    pub in_use: bool,
    pub ref_count: usize,
    pub name: [u8; 64],
    pub name_len: usize,
    pub flags: i64,
    pub maxmsg: i64,
    pub msgsize: i64,
    pub curmsgs: usize,
    pub messages: [MqMessage; MQ_MAX_MSG_COUNT],
}

impl PosixMqQueue {
    pub const fn empty() -> Self {
        Self {
            in_use: false,
            ref_count: 0,
            name: [0u8; 64],
            name_len: 0,
            flags: 0,
            maxmsg: 10,
            msgsize: 128,
            curmsgs: 0,
            messages: [MqMessage { len: 0, prio: 0, data: [0u8; MQ_MAX_MSG_SIZE] }; MQ_MAX_MSG_COUNT],
        }
    }
}

pub struct PosixMqTable {
    pub queues: [PosixMqQueue; MQ_MAX_QUEUES],
}

pub static POSIX_MQ_TABLE: Spinlock<PosixMqTable> = Spinlock::new(PosixMqTable {
    queues: [PosixMqQueue::empty(); MQ_MAX_QUEUES],
});

pub fn mq_retain(id: u32) {
    let mut table = POSIX_MQ_TABLE.lock();
    if let Some(q) = table.queues.get_mut(id as usize) {
        if q.in_use {
            q.ref_count += 1;
        }
    }
}

pub fn mq_close(id: u32) {
    let mut table = POSIX_MQ_TABLE.lock();
    if let Some(q) = table.queues.get_mut(id as usize) {
        if q.in_use {
            if q.ref_count > 1 {
                q.ref_count -= 1;
            } else {
                q.ref_count = 0;
            }
        }
    }
}

// ── Fanotify ──────────────────────────────────────────────────────────────
pub const MAX_FANOTIFY_INSTANCES: usize = 8;
pub const MAX_FANOTIFY_MARKS: usize = 16;

#[derive(Copy, Clone)]
pub struct FanotifyMark {
    pub in_use: bool,
    pub mask: u64,
    pub path: [u8; 64],
    pub path_len: usize,
}

#[derive(Copy, Clone)]
pub struct FanotifyInstance {
    pub in_use: bool,
    pub ref_count: usize,
    pub flags: u32,
    pub event_f_flags: u32,
    pub marks: [FanotifyMark; MAX_FANOTIFY_MARKS],
}

impl FanotifyInstance {
    pub const fn empty() -> Self {
        Self {
            in_use: false,
            ref_count: 0,
            flags: 0,
            event_f_flags: 0,
            marks: [FanotifyMark { in_use: false, mask: 0, path: [0u8; 64], path_len: 0 }; MAX_FANOTIFY_MARKS],
        }
    }
}

pub struct FanotifyTable {
    pub instances: [FanotifyInstance; MAX_FANOTIFY_INSTANCES],
}

pub static FANOTIFY_TABLE: Spinlock<FanotifyTable> = Spinlock::new(FanotifyTable {
    instances: [FanotifyInstance::empty(); MAX_FANOTIFY_INSTANCES],
});

pub fn fanotify_retain(id: u32) {
    let mut table = FANOTIFY_TABLE.lock();
    if let Some(inst) = table.instances.get_mut(id as usize) {
        if inst.in_use {
            inst.ref_count += 1;
        }
    }
}

pub fn fanotify_close(id: u32) {
    let mut table = FANOTIFY_TABLE.lock();
    if let Some(inst) = table.instances.get_mut(id as usize) {
        if inst.in_use {
            if inst.ref_count > 1 {
                inst.ref_count -= 1;
            } else {
                inst.in_use = false;
                inst.ref_count = 0;
            }
        }
    }
}

// ── Inotify Subsystem ──────────────────────────────────────────────────────

pub const MAX_INOTIFY_INSTANCES: usize = 16;
pub const MAX_INOTIFY_WATCHES: usize = 32;

#[derive(Copy, Clone)]
pub struct InotifyWatch {
    pub in_use: bool,
    pub wd: i32,
    pub mask: u32,
    pub path: [u8; 64],
    pub path_len: usize,
}

/// [CICLO 48] Evento inotify encolado (formato `struct inotify_event` al leer).
#[derive(Copy, Clone)]
pub struct InotifyEvento {
    pub wd: i32,
    pub mask: u32,
    pub cookie: u32,
    pub name: [u8; 64],
    pub name_len: u8,
}

pub const MAX_INOTIFY_EVENTOS: usize = 16;
pub const IN_MODIFY: u32 = 0x2;
pub const IN_CLOSE_WRITE: u32 = 0x8;
pub const IN_CREATE: u32 = 0x100;
pub const IN_DELETE: u32 = 0x200;
pub const IN_DELETE_SELF: u32 = 0x400;
pub const IN_Q_OVERFLOW: u32 = 0x4000;
pub const IN_IGNORED: u32 = 0x8000;
pub const IN_ISDIR: u32 = 0x4000_0000;

#[derive(Copy, Clone)]
pub struct InotifyInstance {
    pub in_use: bool,
    pub ref_count: usize,
    pub flags: i32,
    pub next_wd: i32,
    pub watches: [InotifyWatch; MAX_INOTIFY_WATCHES],
    pub eventos: [InotifyEvento; MAX_INOTIFY_EVENTOS],
    pub n_eventos: usize,
}

impl InotifyInstance {
    pub const fn empty() -> Self {
        Self {
            in_use: false,
            ref_count: 0,
            flags: 0,
            next_wd: 1,
            watches: [InotifyWatch { in_use: false, wd: 0, mask: 0, path: [0u8; 64], path_len: 0 }; MAX_INOTIFY_WATCHES],
            eventos: [InotifyEvento { wd: 0, mask: 0, cookie: 0, name: [0u8; 64], name_len: 0 }; MAX_INOTIFY_EVENTOS],
            n_eventos: 0,
        }
    }
}

/// Reserva una instancia inotify (inotify_init1). `flags` conserva IN_NONBLOCK.
pub fn inotify_alloc(flags: i32) -> Option<u32> {
    let mut table = INOTIFY_TABLE.lock();
    for (i, inst) in table.instances.iter_mut().enumerate() {
        if !inst.in_use {
            *inst = InotifyInstance::empty();
            inst.in_use = true;
            inst.ref_count = 1;
            inst.flags = flags;
            return Some(i as u32);
        }
    }
    None
}

/// Anade (o actualiza) una vigilancia sobre `path`. Devuelve el wd o -ENOSPC.
pub fn inotify_add_watch(id: u32, path: &str, mask: u32) -> i32 {
    let mut table = INOTIFY_TABLE.lock();
    let inst = match table.instances.get_mut(id as usize) { Some(i) if i.in_use => i, _ => return -9 };
    let limpio = path.trim_end_matches('/');
    let b = limpio.as_bytes();
    if b.len() > 64 { return -36; } // -ENAMETOOLONG
    if let Some(w) = inst.watches.iter_mut().find(|w| w.in_use && &w.path[..w.path_len] == b) {
        w.mask = if (mask & 0x2000_0000) != 0 { w.mask | mask } else { mask }; // IN_MASK_ADD
        return w.wd;
    }
    let wd = inst.next_wd;
    for w in inst.watches.iter_mut() {
        if !w.in_use {
            w.in_use = true; w.wd = wd; w.mask = mask;
            w.path = [0u8; 64]; w.path[..b.len()].copy_from_slice(b); w.path_len = b.len();
            inst.next_wd += 1;
            return wd;
        }
    }
    -28 // -ENOSPC
}

fn inotify_push(inst: &mut InotifyInstance, wd: i32, mask: u32, name: &[u8]) {
    if inst.n_eventos >= MAX_INOTIFY_EVENTOS {
        // Cola llena: el ultimo hueco pasa a IN_Q_OVERFLOW (como Linux).
        let ult = &mut inst.eventos[MAX_INOTIFY_EVENTOS - 1];
        ult.wd = -1; ult.mask = IN_Q_OVERFLOW; ult.name_len = 0;
        return;
    }
    let e = &mut inst.eventos[inst.n_eventos];
    e.wd = wd; e.mask = mask; e.cookie = 0;
    let n = name.len().min(63);
    e.name = [0u8; 64]; e.name[..n].copy_from_slice(&name[..n]); e.name_len = n as u8;
    inst.n_eventos += 1;
}

/// Retira una vigilancia; encola IN_IGNORED. -EINVAL si el wd no existe.
pub fn inotify_rm_watch(id: u32, wd: i32) -> i32 {
    let mut table = INOTIFY_TABLE.lock();
    let inst = match table.instances.get_mut(id as usize) { Some(i) if i.in_use => i, _ => return -9 };
    match inst.watches.iter().position(|w| w.in_use && w.wd == wd) {
        Some(pos) => { inst.watches[pos].in_use = false; inotify_push(inst, wd, IN_IGNORED, &[]); 0 }
        None => -22,
    }
}

/// Punto de enganche del VFS: `path` acaba de sufrir `mask` (IN_CREATE,
/// IN_MODIFY, IN_DELETE...). Se notifica a las vigilancias del propio nodo
/// (sin nombre) y a las de su directorio padre (con el nombre base).
pub fn inotify_notify(path: &str, mask: u32) {
    let limpio = path.trim_end_matches('/');
    let (padre, base) = match limpio.rfind('/') {
        Some(0) => ("/", &limpio[1..]),
        Some(i) => (&limpio[..i], &limpio[i + 1..]),
        None => return,
    };
    let mut table = INOTIFY_TABLE.lock();
    for inst in table.instances.iter_mut() {
        if !inst.in_use { continue; }
        let mut pendientes: [(i32, u32, bool); MAX_INOTIFY_WATCHES] = [(0, 0, false); MAX_INOTIFY_WATCHES];
        let mut n = 0;
        for w in inst.watches.iter() {
            if !w.in_use { continue; }
            let wp = &w.path[..w.path_len];
            if wp == limpio.as_bytes() {
                let m = if (mask & IN_DELETE) != 0 { IN_DELETE_SELF } else { mask };
                if (m & w.mask) != 0 { pendientes[n] = (w.wd, m, false); n += 1; }
            } else if wp == padre.as_bytes() && (mask & w.mask) != 0 {
                pendientes[n] = (w.wd, mask, true); n += 1;
            }
        }
        for i in 0..n {
            let (wd, m, con_nombre) = pendientes[i];
            inotify_push(inst, wd, m, if con_nombre { base.as_bytes() } else { &[] });
        }
    }
}

pub fn inotify_has_events(id: u32) -> bool {
    let table = INOTIFY_TABLE.lock();
    table.instances.get(id as usize).map(|i| i.in_use && i.n_eventos > 0).unwrap_or(false)
}

pub fn inotify_flags(id: u32) -> i32 {
    let table = INOTIFY_TABLE.lock();
    table.instances.get(id as usize).map(|i| i.flags).unwrap_or(0)
}

/// Serializa eventos en `out` con el formato de `struct inotify_event`
/// (wd, mask, cookie, len, name[len] con NUL y relleno a multiplo de 16).
/// Devuelve los bytes escritos; -EINVAL si no cabe ni el primer evento.
pub fn inotify_read(id: u32, out: &mut [u8]) -> i64 {
    let mut table = INOTIFY_TABLE.lock();
    let inst = match table.instances.get_mut(id as usize) { Some(i) if i.in_use => i, _ => return -9 };
    let mut off = 0usize;
    let mut consumidos = 0usize;
    while consumidos < inst.n_eventos {
        let e = inst.eventos[consumidos];
        let len = if e.name_len == 0 { 0 } else { ((e.name_len as usize + 1) + 15) & !15 };
        if off + 16 + len > out.len() {
            if consumidos == 0 { return -22; }
            break;
        }
        out[off..off + 4].copy_from_slice(&e.wd.to_ne_bytes());
        out[off + 4..off + 8].copy_from_slice(&e.mask.to_ne_bytes());
        out[off + 8..off + 12].copy_from_slice(&e.cookie.to_ne_bytes());
        out[off + 12..off + 16].copy_from_slice(&(len as u32).to_ne_bytes());
        for b in out[off + 16..off + 16 + len].iter_mut() { *b = 0; }
        out[off + 16..off + 16 + e.name_len as usize].copy_from_slice(&e.name[..e.name_len as usize]);
        off += 16 + len;
        consumidos += 1;
    }
    let restantes = inst.n_eventos - consumidos;
    for i in 0..restantes { inst.eventos[i] = inst.eventos[i + consumidos]; }
    inst.n_eventos = restantes;
    off as i64
}

pub struct InotifyTable {
    pub instances: [InotifyInstance; MAX_INOTIFY_INSTANCES],
}

pub static INOTIFY_TABLE: Spinlock<InotifyTable> = Spinlock::new(InotifyTable {
    instances: [InotifyInstance::empty(); MAX_INOTIFY_INSTANCES],
});

pub fn inotify_retain(id: u32) {
    let mut table = INOTIFY_TABLE.lock();
    if let Some(inst) = table.instances.get_mut(id as usize) {
        if inst.in_use {
            inst.ref_count += 1;
        }
    }
}

pub fn inotify_close(id: u32) {
    let mut table = INOTIFY_TABLE.lock();
    if let Some(inst) = table.instances.get_mut(id as usize) {
        if inst.in_use {
            if inst.ref_count > 1 {
                inst.ref_count -= 1;
            } else {
                inst.in_use = false;
                inst.ref_count = 0;
            }
        }
    }
}

// ── Extended Attributes (XATTR) Subsystem ──────────────────────────────────

pub const MAX_XATTR_ENTRIES: usize = 64;
pub const MAX_XATTR_NAME_LEN: usize = 64;
pub const MAX_XATTR_VAL_LEN: usize = 128;

#[derive(Copy, Clone)]
pub struct XattrEntry {
    pub in_use: bool,
    pub path_hash: u64,
    pub name: [u8; MAX_XATTR_NAME_LEN],
    pub name_len: usize,
    pub value: [u8; MAX_XATTR_VAL_LEN],
    pub value_len: usize,
}

impl XattrEntry {
    pub const fn empty() -> Self {
        Self {
            in_use: false,
            path_hash: 0,
            name: [0u8; MAX_XATTR_NAME_LEN],
            name_len: 0,
            value: [0u8; MAX_XATTR_VAL_LEN],
            value_len: 0,
        }
    }
}

pub struct XattrTable {
    pub entries: [XattrEntry; MAX_XATTR_ENTRIES],
}

pub static XATTR_TABLE: Spinlock<XattrTable> = Spinlock::new(XattrTable {
    entries: [XattrEntry::empty(); MAX_XATTR_ENTRIES],
});

fn alloc_slot_and_pid() -> Option<(usize, u64)> {
    let mut guard = PROCESS_TABLE.lock();
    let slot = guard.find_slot()?;
    let pid = guard.alloc_pid()?;
    // [T-017]/[T-020] las ranuras se reutilizan: estos contadores/banderas no
    // los pone nadie al crear el proceso.
    unsafe {
        let pp = guard.processes[slot].as_mut_ptr();
        (*pp).salir_solo = false;
        (*pp).senales_excepcion = 0;
    }
    Some((slot, pid))
}

pub fn current_process() -> Option<&'static mut Process> {
    PROCESS_TABLE.lock().current_process_ref()
}

pub fn current_pid() -> u64 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.pid).unwrap_or(0)
}

pub fn get_process_snapshot(pid: u64) -> Option<ProcessSnapshot> {
    let ptab = PROCESS_TABLE.lock();
    for p in &ptab.processes {
        unsafe {
            let proc = &*p.as_ptr();
            if proc.pid == pid && proc.state != ProcessState::Dead {
                let vmsize = (proc.program_break.saturating_sub(PROGRAM_BREAK_BASE) + 
                              proc.mmap_base.saturating_sub(MMAP_BASE) + 
                              USER_STACK_SIZE) / 1024;
                return Some(ProcessSnapshot {
                    pid: proc.pid,
                    ppid: proc.ppid,
                    state: proc.state,
                    name: proc.name,
                    name_len: proc.name_len,
                    uid: proc.uid,
                    gid: proc.gid,
                    ticks_used: proc.ticks_used,
                    exit_code: proc.exit_code,
                    vmsize_kb: (vmsize.max(64)) as u64,
                });
            }
        }
    }
    None
}

pub fn get_all_active_pids() -> alloc::vec::Vec<u64> {
    let mut pids = alloc::vec::Vec::new();
    let ptab = PROCESS_TABLE.lock();
    for p in &ptab.processes {
        unsafe {
            let proc = &*p.as_ptr();
            if proc.state != ProcessState::Dead {
                pids.push(proc.pid);
            }
        }
    }
    pids
}

/// [CIERRE B-3] Registra el fin de la imagen ELF del proceso `pid`.
/// [CICLO 49] Ventana virtual que ocupa la imagen ELF de un proceso a partir
/// de su `image_end`: programas nativos de Portix (0x20000000..), binarios
/// no-PIE de distribución (0x400000..) o PIE (`elf::PIE_LOAD_BASE`..).
pub fn ventana_imagen(image_end: usize) -> (usize, usize) {
    let pie = crate::elf::PIE_LOAD_BASE as usize;
    if image_end > 0 && image_end < 0x2000_0000 {
        (0x20_0000, image_end)
    } else if image_end >= pie {
        (pie, image_end)
    } else {
        (0x2000_0000, 0x2100_0000usize.max(image_end))
    }
}

/// [Fase B.4] Parte de la ventana baja (0x200000–0x20000000) que NO ocupa la
/// imagen: ahí viven los `MAP_FIXED` del proceso (p. ej. 0x2000000). La imagen
/// se excluye porque sus marcos los gobierna el conteo de referencias.
pub fn ventana_baja_libre(image_end: usize) -> (usize, usize) {
    let (ii, fi) = ventana_imagen(image_end);
    let lo = if ii == 0x20_0000 { (fi + PAGE_SIZE - 1) & !(PAGE_SIZE - 1) } else { 0x20_0000 };
    (lo, paging::VENTANA_BAJA_FIN)
}

pub fn set_process_image_end(pid: u64, end: usize) {
    let mut ptab = PROCESS_TABLE.lock();
    for p in &mut ptab.processes {
        unsafe {
            let proc = &mut *p.as_mut_ptr();
            if proc.pid == pid { proc.image_end = end; break; }
        }
    }
}

pub fn set_process_tls(pid: u64, tls: u64) {
    let mut ptab = PROCESS_TABLE.lock();
    for p in &mut ptab.processes {
        unsafe {
            let proc = &mut *p.as_mut_ptr();
            if proc.pid == pid {
                proc.fs_base = tls;
                break;
            }
        }
    }
}

pub fn get_process_regs(pid: u64) -> Option<UserRegs> {
    // [CICLO v6.x] Los registros que interesan a un depurador son los del
    // FRAME vivo (el que el `iretq` va a restaurar), no la copia estática del
    // PCB: un proceso detenido en un breakpoint tiene su RIP/RSP/EFLAGS reales
    // en el tope de su kernel stack. Se leen de ahí.
    let mut regs = None;
    let frame = {
        let ptab = PROCESS_TABLE.lock();
        let mut f = None;
        for p in &ptab.processes {
            unsafe {
                let proc = &*p.as_ptr();
                if proc.pid == pid && proc.state != ProcessState::Dead {
                    let mut r = proc.regs;
                    r.rip = proc.user_rip;
                    r.rsp = proc.user_rsp;
                    r.fs_base = proc.fs_base;
                    regs = Some(r);
                    let krsp = proc.kernel_rsp;
                    if krsp >= 0x1000 {
                        f = Some(krsp);
                    }
                    break;
                }
            }
        }
        f
    };
    if let (Some(r), Some(base)) = (regs.as_mut(), frame) {
        unsafe {
            use crate::process::signals::*;
            let rd = |off: usize| core::ptr::read_unaligned((base + off as u64) as *const u64);
            let rip = rd(FRAME_RIP);
            let rsp = rd(FRAME_RSP);
            let cs = rd(FRAME_CS);
            // Sólo se adopta el frame si es de Ring 3 (el proceso está detenido
            // o en el CPU ejecutando código de usuario).
            if (cs & 3) == 3 && rip >= 0x1000 {
                r.rip = rip;
                r.rsp = rsp;
                r.cs = cs;
                r.ss = rd(FRAME_SS);
                r.eflags = rd(FRAME_RFLAGS);
                r.rax = rd(FRAME_RAX);
                r.rcx = rd(FRAME_RCX);
                r.rdx = rd(FRAME_RDX);
                r.rsi = rd(FRAME_RSI);
                r.rdi = rd(FRAME_RDI);
                r.r8 = rd(FRAME_R8);
                r.r9 = rd(FRAME_R9);
                r.r10 = rd(FRAME_R10);
                r.r11 = rd(FRAME_R11);
                r.rbx = rd(FRAME_RBX);
                r.rbp = rd(FRAME_RBP);
                r.r12 = rd(FRAME_R12);
                r.r13 = rd(FRAME_R13);
                r.r14 = rd(FRAME_R14);
                r.r15 = rd(FRAME_R15);
            }
        }
    }
    regs
}

pub fn set_process_regs(pid: u64, regs: &UserRegs) -> bool {
    // [CICLO v6.x] Simétrico a `get_process_regs`: los registros de un proceso
    // DEPURADO viven en el frame IRET que el `iretq` restaurará. Escribirlos
    // sólo en la copia del PCB era inútil para un trazador, y es precisamente
    // lo que permite la maniobra clásica del depurador: al retirar el byte
    // 0xCC de un breakpoint hay que REBOBINAR el RIP a la dirección del
    // breakpoint (si no, el objetivo reanudaría en medio de la instrucción,
    // perdiendo su prefijo REX.W: `48 83 ec 18` se convertiría en `83 ec 18`
    // = `sub esp,0x18`, que anula la mitad alta de RSP y hace caer al proceso).
    // El marco VIVO sólo se escribe para OTRO proceso: si el llamador se
    // modifica a sí mismo, escribir su frame de syscall en curso haría que el
    // `iretq` lo devolviera a un punto anterior de su propio código (un
    // `GETREGS`+`SETREGS` sobre sí mismo se convertiría en un bucle infinito).
    // Es la misma restricción que aplica Linux: un proceso no se depura a sí
    // mismo; para eso existe `PTRACE_TRACEME` y un trazador distinto.
    let es_actual = crate::arch::percpu::pid_actual() == pid;
    let frame = {
        let mut ptab = PROCESS_TABLE.lock();
        let mut f = None;
        let mut encontrado = false;
        for p in ptab.processes.iter_mut() {
            unsafe {
                let proc = &mut *p.as_mut_ptr();
                if proc.pid == pid && proc.state != ProcessState::Dead {
                    proc.regs = *regs;
                    proc.user_rip = regs.rip;
                    proc.user_rsp = regs.rsp;
                    proc.fs_base = regs.fs_base;
                    let krsp = proc.kernel_rsp;
                    if !es_actual && krsp >= 0x1000 {
                        f = Some(krsp);
                    }
                    encontrado = true;
                    break;
                }
            }
        }
        if !encontrado { return false; }
        f
    };
    if let Some(base) = frame {
        unsafe {
            use crate::process::signals::*;
            let rd = |off: usize| core::ptr::read_unaligned((base + off as u64) as *const u64);
            let wr = |off: usize, v: u64| core::ptr::write_unaligned((base + off as u64) as *mut u64, v);
            let cs = rd(FRAME_CS);
            let rip = rd(FRAME_RIP);
            // Sólo se escribe si el frame es de Ring 3 (proceso detenido en una
            // excepción o en el CPU ejecutando código de usuario).
            if (cs & 3) == 3 && rip >= 0x1000 {
                wr(FRAME_RIP, regs.rip);
                wr(FRAME_RSP, regs.rsp);
                wr(FRAME_RFLAGS, regs.eflags);
                wr(FRAME_RAX, regs.rax);
                wr(FRAME_RCX, regs.rcx);
                wr(FRAME_RDX, regs.rdx);
                wr(FRAME_RSI, regs.rsi);
                wr(FRAME_RDI, regs.rdi);
                wr(FRAME_R8, regs.r8);
                wr(FRAME_R9, regs.r9);
                wr(FRAME_R10, regs.r10);
                wr(FRAME_R11, regs.r11);
                wr(FRAME_RBX, regs.rbx);
                wr(FRAME_RBP, regs.rbp);
                wr(FRAME_R12, regs.r12);
                wr(FRAME_R13, regs.r13);
                wr(FRAME_R14, regs.r14);
                wr(FRAME_R15, regs.r15);
            }
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
//  Puntos de vigilancia por hardware (DR0-DR3)
// ─────────────────────────────────────────────────────────────────────────────

/// Instala un punto de vigilancia en la primera ranura libre del proceso.
/// Devuelve el índice de ranura o -errno (-ENOSPC si las cuatro están ocupadas).
pub fn set_watchpoint(pid: u64, addr: u64, len_code: u8, rw: u8) -> i64 {
    if addr == 0 { return -22; }
    let mut guard = match PROCESS_TABLE.try_lock() { Some(g) => g, None => return -11 };
    for p in guard.processes.iter_mut() {
        let pp = p.as_mut_ptr();
        unsafe {
            if (*pp).pid != pid || (*pp).state == ProcessState::Dead { continue; }
            let n = ((*pp).watch_count as usize).min(4);
            for i in 0..n {
                if (*pp).watch_addr[i] == addr {
                    (*pp).watch_len[i] = len_code;
                    (*pp).watch_rw[i] = rw;
                    return i as i64;
                }
            }
            if n >= 4 { return -28; } // -ENOSPC
            (*pp).watch_addr[n] = addr;
            (*pp).watch_len[n] = len_code;
            (*pp).watch_rw[n] = rw;
            (*pp).watch_count = (n + 1) as u8;
            // Si el proceso es el que está en ejecución, se programa ya mismo.
            // Si el proceso ya está en ejecución, la vigilancia se programa en
            // el propio CPU al instante; si no, se cargará al conmutar contexto.
            if (*pp).pid == crate::arch::percpu::pid_actual() {
                crate::arch::debug_regs::apply_for(&*pp);
            }
            return n as i64;
        }
    }
    -3 // -ESRCH
}

pub fn clear_watchpoints(pid: u64) -> bool {
    let mut guard = match PROCESS_TABLE.try_lock() { Some(g) => g, None => return false };
    for p in guard.processes.iter_mut() {
        let pp = p.as_mut_ptr();
        unsafe {
            if (*pp).pid == pid {
                (*pp).watch_count = 0;
            (*pp).bp_cond_count = 0;
            (*pp).bp_cond_rearm = 0xFF;
            (*pp).bp_cond_ignore = [0u32; 4];
                (*pp).watch_addr = [0u64; 4];
                (*pp).watch_len = [0u8; 4];
                (*pp).watch_rw = [0u8; 4];
                if (*pp).pid == crate::arch::percpu::pid_actual() {
                    crate::arch::debug_regs::disable_all();
                }
                return true;
            }
        }
    }
    false
}

pub fn get_watchpoints(pid: u64) -> (usize, [u64; 4], [u8; 4], [u8; 4]) {
    let mut out = (0usize, [0u64; 4], [0u8; 4], [0u8; 4]);
    if let Some(guard) = PROCESS_TABLE.try_lock() {
        for p in guard.processes.iter() {
            let pp = p.as_ptr();
            unsafe {
                if (*pp).pid == pid {
                    out = ((*pp).watch_count as usize, (*pp).watch_addr, (*pp).watch_len, (*pp).watch_rw);
                    break;
                }
            }
        }
    }
    out
}

/// CR3 del proceso indicado (para el acceso a memoria del depurador).
pub fn get_process_cr3(pid: u64) -> Option<u64> {
    let guard = PROCESS_TABLE.try_lock()?;
    for p in guard.processes.iter() {
        let pp = p.as_ptr() as *const Process;
        unsafe {
            if (*pp).pid == pid && (*pp).state != ProcessState::Dead {
                return Some((*pp).cr3);
            }
        }
    }
    None
}

pub fn set_process_traced(pid: u64, tracer: u64) -> bool {
    let mut ptab = PROCESS_TABLE.lock();
    for p in &mut ptab.processes {
        unsafe {
            let proc = &mut *p.as_mut_ptr();
            if proc.pid == pid && proc.state != ProcessState::Dead {
                proc.ptrace_traced = true;
                proc.ptrace_tracer = tracer;
                return true;
            }
        }
    }
    false
}

pub fn set_process_singlestep(pid: u64, enable: bool) -> bool {
    let mut ptab = PROCESS_TABLE.lock();
    for p in &mut ptab.processes {
        unsafe {
            let proc = &mut *p.as_mut_ptr();
            if proc.pid == pid && proc.state != ProcessState::Dead {
                proc.ptrace_singlestep = enable;
                if enable {
                    proc.regs.eflags |= 0x100;
                } else {
                    proc.regs.eflags &= !0x100;
                }
                return true;
            }
        }
    }
    false
}

// ─────────────────────────────────────────────────────────────────────────────
//  [CICLO v6.x] Parada y reanudación de procesos trazados (depuración real)
//
//  Modelo POSIX: un proceso bajo `ptrace` que encuentra un punto de parada
//  (breakpoint, watchpoint, paso a paso o `execve`) queda en estado `Stopped`
//  conservando su contexto en el kernel stack. El trazador lo percibe con
//  `waitpid`, que devuelve `WIFSTOPPED` = (señal << 8) | 0x7f, y decide
//  después si reanuda (`PTRACE_CONT`), da un paso (`PTRACE_SINGLESTEP`) o se
//  desengancha (`PTRACE_DETACH`).
// ─────────────────────────────────────────────────────────────────────────────


// ─────────────────────────────────────────────────────────────────────────────
//  [CONTROL DE TRABAJOS] Terminal de control (consola): grupo de primer plano
//  y disciplina de línea mínima (^C → SIGINT, ^Z → SIGTSTP al grupo).
// ─────────────────────────────────────────────────────────────────────────────
static FG_PGID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub fn fg_pgid() -> u64 { FG_PGID.load(core::sync::atomic::Ordering::Relaxed) }
pub fn set_fg_pgid(pg: u64) { FG_PGID.store(pg, core::sync::atomic::Ordering::Relaxed); }

const TTY_RX_SIZE: usize = 512;
static mut TTY_RX: [u8; TTY_RX_SIZE] = [0; TTY_RX_SIZE];
static TTY_HEAD: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static TTY_TAIL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// Señal de terminal pendiente de entregar (si la tabla estaba ocupada al llegar la tecla).
static TTY_SIG_PENDING: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// Contadores: [^C entregados, ^Z entregados].
static TTY_SIG_STATS: [core::sync::atomic::AtomicU32; 2] = [core::sync::atomic::AtomicU32::new(0), core::sync::atomic::AtomicU32::new(0)];
pub fn tty_signal_stats() -> (u32, u32) { (TTY_SIG_STATS[0].load(core::sync::atomic::Ordering::Relaxed), TTY_SIG_STATS[1].load(core::sync::atomic::Ordering::Relaxed)) }

fn tty_try_signal(sig: u32) -> bool {
    use core::sync::atomic::Ordering;
    let pg = fg_pgid();
    if pg == 0 { return true; }
    let Some(guard) = PROCESS_TABLE.try_lock() else { return false };
    // ^Z sólo tiene sentido si hay un shell Ring 3 que pueda reanudar el trabajo:
    // un proceso lanzado directamente por la consola del núcleo (ppid 0) quedaría
    // detenido para siempre, así que la petición se ignora con aviso.
    if sig == signals::SIGTSTP {
        let sin_shell = guard.processes.iter().all(|p| unsafe {
            let q = p.as_ptr() as *const Process;
            (*q).pgid != pg || (*q).pid == 0 || (*q).state == ProcessState::Dead || (*q).state == ProcessState::Zombie || (*q).ppid == 0
        });
        if sin_shell {
            drop(guard);
            crate::drivers::serial::write_str("\n[TTY] ^Z ignorado: el proceso de primer plano no tiene shell de control de trabajos\n");
            return true;
        }
    }
    let mut n = 0usize;
    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr() as *mut Process;
            if (*proc).pgid == pg && (*proc).state != ProcessState::Dead && (*proc).state != ProcessState::Zombie && (*proc).pid != 0 {
                if sig == signals::SIGTSTP { (*proc).sig_pending &= !(1u64 << signals::SIGCONT); }
                (*proc).sig_pending |= 1u64 << sig;
                // Un `sleep` en curso se interrumpe para que la señal actúe de inmediato.
                // Si el dormido es el proceso que ocupa la CPU (espera en `hlt` dentro
                // del syscall) NO se toca su estado: su propio bucle ve `sig_pending`.
                if (*proc).state == ProcessState::Blocked && (*proc).sleep_until > 0 && (*proc).pid != crate::arch::percpu::pid_actual() {
                    (*proc).sleep_until = 0;
                    (*proc).state = ProcessState::Ready;
                }
                n += 1;
            }
        }
    }
    drop(guard);
    TTY_SIG_STATS[if sig == signals::SIGINT { 0 } else { 1 }].fetch_add(1, Ordering::Relaxed);
    crate::drivers::serial::write_str(if sig == signals::SIGINT { "\n[TTY] ^C -> SIGINT al grupo de primer plano " } else { "\n[TTY] ^Z -> SIGTSTP al grupo de primer plano " });
    crate::drivers::serial::write_usize(pg as usize);
    crate::drivers::serial::write_str(" ("); crate::drivers::serial::write_usize(n); crate::drivers::serial::write_str(" procesos)\n");
    true
}

fn tty_handle_byte(b: u8) -> bool {
    use core::sync::atomic::Ordering;
    if (b == 0x03 || b == 0x1A) && fg_pgid() != 0 {
        let sig = if b == 0x03 { signals::SIGINT } else { signals::SIGTSTP };
        if !tty_try_signal(sig) { TTY_SIG_PENDING.store(sig, Ordering::Relaxed); }
        return true;
    }
    false
}

/// La llama el temporizador: vacía la UART en el anillo de la terminal y
/// despacha ^C/^Z aunque ningún proceso esté leyendo stdin (p. ej. `sleep`).
pub fn tty_pump_from_irq() {
    use core::sync::atomic::Ordering;
    let pend = TTY_SIG_PENDING.swap(0, Ordering::Relaxed);
    if pend != 0 && !tty_try_signal(pend) { TTY_SIG_PENDING.store(pend, Ordering::Relaxed); }
    let mut nuevos = false;
    while crate::drivers::serial::rx_ready() {
        let Some(b) = crate::drivers::serial::read_byte() else { break };
        if b == 0x1D { volcado_smp(); continue; } // [E3d] ^] = volcado de diagnóstico SMP por serie
        if tty_handle_byte(b) { continue; }
        let h = TTY_HEAD.load(Ordering::Relaxed);
        let next = (h + 1) % TTY_RX_SIZE;
        if next == TTY_TAIL.load(Ordering::Relaxed) { break; }
        unsafe { TTY_RX[h] = b; }
        TTY_HEAD.store(next, Ordering::Release);
        nuevos = true;
    }
    // Hay bytes nuevos: despertar al proceso bloqueado en stdin (la UART no
    // tiene IRQ en esta consola; antes sólo IRQ1 del teclado despertaba y un
    // shell interactivo por la serie se quedaba dormido para siempre).
    if nuevos || TTY_TAIL.load(Ordering::Relaxed) != TTY_HEAD.load(Ordering::Relaxed) {
        let pid = crate::arch::isr_handlers::stdin_blocked_pid();
        if pid != 0 {
            if let Some(guard) = PROCESS_TABLE.try_lock() {
                for p in &guard.processes {
                    unsafe {
                        let proc = p.as_ptr() as *mut Process;
                        if (*proc).pid == pid && (*proc).state == ProcessState::Blocked {
                            (*proc).state = ProcessState::Ready; (*proc).sleep_until = 0;
                            crate::arch::isr_handlers::clear_stdin_blocked();
                        }
                    }
                }
            }
        }
    }
}

/// [T-025 E3d] Volcado de diagnóstico SMP por el puerto serie (lo dispara ^]
/// desde la IRQ0 del BSP; la suite lo envía cuando una prueba agota su plazo):
/// PID por CPU, última syscall por CPU, dueño del cerrojo grande y estado de
/// cada proceso vivo. Sólo lee; no toma cerrojos.
pub fn volcado_smp() {
    use crate::drivers::serial as ser;
    ser::write_str("[VOLCADO-SMP] cpus=");
    ser::write_usize(crate::arch::smp::nucleos_en_linea());
    ser::write_str(" cerrojo_grande dueno=");
    ser::write_usize(crate::arch::bkl::dueno() as usize);
    ser::write_str("\n");
    for i in 0..crate::arch::smp::nucleos_en_linea().min(crate::arch::percpu::MAX_CPUS) {
        let b = crate::arch::percpu::por_indice(i);
        ser::write_str("  cpu "); ser::write_usize(i);
        ser::write_str(" pid="); ser::write_usize(b.pid_actual.load(core::sync::atomic::Ordering::Relaxed) as usize);
        let (nr, fase) = crate::syscall::ultima_syscall(i);
        ser::write_str(" ultima_syscall="); ser::write_usize(nr as usize);
        ser::write_str(if fase == 1 { " (dentro)" } else { " (salio)" });
        ser::write_str("\n");
    }
    let tabla = unsafe { &*PROCESS_TABLE.as_mut_ptr() };
    for p in &tabla.processes {
        let q = unsafe { &*(p.as_ptr() as *const Process) };
        if q.pid == 0 || q.state == ProcessState::Dead { continue; }
        ser::write_str("  pid "); ser::write_usize(q.pid as usize);
        ser::write_str(" ppid "); ser::write_usize(q.ppid as usize);
        ser::write_str(" '"); ser::write_str(q.name_str()); ser::write_str("' ");
        ser::write_str(match q.state {
            ProcessState::Running => "Running", ProcessState::Ready => "Ready",
            ProcessState::Blocked => "Blocked", ProcessState::Zombie => "Zombie",
            ProcessState::Stopped => "Stopped", _ => "?" });
        ser::write_str(" cpu "); ser::write_usize(q.last_cpu as usize);
        ser::write_str(" sleep_until "); ser::write_usize(q.sleep_until as usize);
        ser::write_str("\n");
    }
}

/// Siguiente byte de la terminal serie (anillo del temporizador y, si está vacío, la UART).
pub fn tty_read_byte() -> Option<u8> {
    use core::sync::atomic::Ordering;
    let t = TTY_TAIL.load(Ordering::Relaxed);
    if t != TTY_HEAD.load(Ordering::Acquire) {
        let b = unsafe { TTY_RX[t] };
        TTY_TAIL.store((t + 1) % TTY_RX_SIZE, Ordering::Release);
        return Some(b);
    }
    // [CICLO 51] Antes se leía la UART directamente aquí; el temporizador
    // (`tty_pump_from_irq`) también la lee, y entre el `inb` y el consumo un
    // IRQ podía colar bytes posteriores en el anillo: las órdenes largas
    // llegaban REORDENADAS ("e1cho") o con huecos. Ahora la UART sólo se
    // vacía hacia el anillo, con las interrupciones inhibidas durante el
    // drenado, y se consume siempre del anillo en orden.
    let rflags: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}; cli", out(reg) rflags, options(nomem, nostack));
    }
    while crate::drivers::serial::rx_ready() {
        let Some(b) = crate::drivers::serial::read_byte() else { break };
        if b == 0x1D { volcado_smp(); continue; } // [E3d] ^] = volcado de diagnóstico SMP por serie
        if tty_handle_byte(b) { continue; }
        let h = TTY_HEAD.load(Ordering::Relaxed);
        let next = (h + 1) % TTY_RX_SIZE;
        if next == TTY_TAIL.load(Ordering::Relaxed) { break; }
        unsafe { TTY_RX[h] = b; }
        TTY_HEAD.store(next, Ordering::Release);
    }
    if rflags & 0x200 != 0 {
        unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
    }
    let t = TTY_TAIL.load(Ordering::Relaxed);
    if t != TTY_HEAD.load(Ordering::Acquire) {
        let b = unsafe { TTY_RX[t] };
        TTY_TAIL.store((t + 1) % TTY_RX_SIZE, Ordering::Release);
        return Some(b);
    }
    None
}

/// [85ª, defecto 22] El proceso en curso acaba de quedar `Stopped` (parada de
/// `execve`, breakpoint, SIGTSTP) y NO puede volver a Ring 3. Cede la CPU si
/// hay alguien listo (devuelve su `kernel_rsp`); si no, espera en `hlt` con el
/// cerrojo grande suelto hasta que el trazador/SIGCONT lo saque de `Stopped`
/// y devuelve 0 (continuar con el frame actual). Mientras espera conserva
/// `en_cpu`, así que ninguna otra CPU puede reanudarlo por su cuenta.
/// Antes, en un AP sin nadie listo, `switch_away()` devolvía 0 y el hijo
/// trazado seguía ejecutando como si nada (dbg: "no se detuvo en el breakpoint");
/// y en el #BP el AP se iba a `halt_loop` (cli;hlt) para siempre.
pub fn esperar_mientras_detenido() -> u64 {
    let d_cgn = crate::arch::bkl::soltar_todo();
    loop {
        let next = switch_away();
        if next != 0 { return next; }
        if !current_is_stopped() { break; }
        unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
        if !current_is_stopped() { break; }
    }
    crate::arch::bkl::retomar(d_cgn);
    unsafe { let cur = current_raw(); if !cur.is_null() && (*cur).state == ProcessState::Ready { (*cur).state = ProcessState::Running; } }
    0
}

/// ¿El proceso en curso quedó `Stopped` (señal de parada o trazador)?
pub fn current_is_stopped() -> bool {
    unsafe { let cur = current_raw(); !cur.is_null() && (*cur).state == ProcessState::Stopped }
}

/// Detiene un proceso para su trazador: estado `Stopped`, señal de la parada
/// registrada y despertar del trazador si estaba esperando en `waitpid`.
pub fn stop_for_tracer(pid: u64, signum: u8, addr: u64) -> bool {
    let mut ptab = PROCESS_TABLE.lock();
    let mut idx: Option<usize> = None;
    for (i, p) in ptab.processes.iter().enumerate() {
        unsafe {
            let proc = &*p.as_ptr();
            if proc.pid == pid && proc.state != ProcessState::Dead {
                idx = Some(i);
                break;
            }
        }
    }
    let Some(i) = idx else { return false };
    let es_actual = crate::arch::percpu::pid_actual() == pid;
    let (interesado, es_hijo) = unsafe {
        let proc = &mut *ptab.processes[i].as_mut_ptr();
        proc.state = ProcessState::Stopped;
        proc.stop_signal = signum;
        proc.stop_reported = false;
        proc.stop_addr = addr;
        proc.ptrace_singlestep = false;
        // [FIX] El evento de depuración llega mientras el proceso está EN EL
        // CPU: su frame vivo es el que yace en el tope de su kernel stack
        // (los stubs de ISR/syscall lo construyen siempre ahí). Hay que
        // registrarlo como `kernel_rsp` para que, al reanudar, el cambio de
        // contexto restaure ESTE frame y no uno antiguo (p. ej. el heredado
        // del fork). Sin esta línea el proceso reanudaba en el contexto
        // equivocado: el hijo trazado volvía al `waitpid` de su padre.
        if es_actual {
            let top = proc.kernel_stack_top as u64;
            if top >= crate::process::signals::KERNEL_FRAME_SIZE as u64 {
                proc.kernel_rsp = top - crate::process::signals::KERNEL_FRAME_SIZE as u64;
            }
        }
        // El trazador (o el padre, si no hay trazador designado) deja de estar
        // bloqueado: tiene una parada que recoger con `waitpid`.
        let interesado = if proc.ptrace_traced && proc.ptrace_tracer != 0 {
            proc.ptrace_tracer
        } else {
            proc.ppid
        };
        (interesado, proc.ppid)
    };
    let _ = es_hijo;
    for p in ptab.processes.iter_mut() {
        unsafe {
            let qq = p.as_mut_ptr();
            if (*qq).pid == interesado && (*qq).state == ProcessState::Blocked {
                (*qq).state = ProcessState::Ready;
            }
        }
    }
    true
}

/// Reanuda un proceso detenido. Con `singlestep` activa la bandera TF del
/// frame guardado, de modo que la ejecución se detenga tras UNA instrucción.
pub fn cont_process(pid: u64, singlestep: bool) -> bool {
    let mut ptab = PROCESS_TABLE.lock();
    let mut encontrado = false;
    for p in ptab.processes.iter_mut() {
        unsafe {
            let proc = &mut *p.as_mut_ptr();
            if proc.pid == pid && proc.state != ProcessState::Dead {
                if proc.state == ProcessState::Stopped {
                    proc.state = ProcessState::Ready;
                }
                proc.stop_signal = 0;
                proc.stop_reported = true;
                proc.stop_addr = 0;
                proc.ptrace_singlestep = singlestep;
                encontrado = true;
                break;
            }
        }
    }
    drop(ptab);
    if !encontrado {
        return false;
    }
    // TF se escribe en el frame que el `iretq` restaurará (bandera de paso a
    // paso del procesador), no en la copia de registros del kernel.
    if let Some(base) = saved_frame_of(pid) {
        unsafe {
            let slot = (base + crate::process::signals::FRAME_RFLAGS as u64) as *mut u64;
            let rf = core::ptr::read_unaligned(slot);
            core::ptr::write_unaligned(slot, if singlestep { rf | 0x100 } else { rf & !0x100u64 });
        }
    }
    true
}

/// Frame de excepción guardado de un proceso: los stubs de ISR construyen
/// siempre el frame inmediatamente por debajo de `kernel_stack_top`, esté el
/// proceso en el CPU o suspendido.
pub fn saved_frame_of(pid: u64) -> Option<u64> {
    let ptab = PROCESS_TABLE.lock();
    for p in &ptab.processes {
        unsafe {
            let proc = &*p.as_ptr();
            if proc.pid == pid && proc.state != ProcessState::Dead {
                let top = proc.kernel_stack_top as u64;
                if top < crate::process::signals::KERNEL_FRAME_SIZE as u64 {
                    return None;
                }
                return Some(top - crate::process::signals::KERNEL_FRAME_SIZE as u64);
            }
        }
    }
    None
}

/// Salta a otro proceso listo y devuelve su `kernel_rsp` para que el stub de
/// interrupción haga el cambio de contexto (mismo contrato que
/// `schedule_tick`: 0 = continuar el proceso actual).
pub fn switch_away() -> u64 {
    unsafe {
        let cur = current_raw();
        let next = pick_next_ready();
        if next.is_null() {
            return 0;
        }
        // [FIX] El cambio de contexto debe ser COMPLETO: no basta con devolver
        // el kernel_rsp del entrante. Si no se actualiza CURRENT_PID, el kernel
        // sigue creyendo que corre el proceso recién detenido y atribuye a ése
        // los syscalls del siguiente (se veía como "caller=3" ejecutando el
        // `waitpid` del padre). También hay que conmutar TSS.RSP0, CR3 y
        // FS_BASE (TLS), igual que el planificador.
        crate::process::scheduler::avisar_si_doble((*next).pid, "switch_away");
        serial::write_str("SCHED[");
        serial::write_usize(crate::arch::percpu::indice_actual());
        serial::write_str("]: switch_away -> PID ");
        serial::write_usize((*next).pid as usize);
        serial::write_str("\n");
        (*next).state = ProcessState::Running;
        (*next).ticks_used = 0;
        if !cur.is_null() { crate::process::scheduler::diferir_liberacion(cur); } // [E3d]
        (*next).en_cpu = crate::arch::percpu::indice_actual() as u8;
        set_tss_rsp0((*next).kernel_stack_top as u64);
        crate::arch::percpu::fijar_pid_actual((*next).pid);
        crate::arch::debug_regs::apply_for(&*next);

        let current_cr3 = crate::mem::paging::read_cr3();
        if (*next).cr3 != current_cr3 {
            crate::mem::paging::write_cr3((*next).cr3);
        }

        if !cur.is_null() {
            // Guardar TLS del saliente y restaurar el del entrante.
            let low: u32;
            let high: u32;
            core::arch::asm!("rdmsr", in("ecx") 0xC0000100u32, out("eax") low, out("edx") high,
                             options(nostack, preserves_flags));
            (*cur).fs_base = ((high as u64) << 32) | (low as u64);
            if (*cur).fs_base != (*next).fs_base {
                let n_low = (*next).fs_base as u32;
                let n_high = ((*next).fs_base >> 32) as u32;
                core::arch::asm!("wrmsr", in("ecx") 0xC0000100u32, in("eax") n_low, in("edx") n_high,
                                 options(nostack, preserves_flags));
            }
        }
        (*next).kernel_rsp
    }
}

/// Parada disponible para `waitpid`. Devuelve `(pid, status, es_parada)`:
///   * un hijo zombie (salida)  -> status = código de salida,  es_parada = false
///   * un proceso detenido      -> status = (señal << 8) | 0x7f, es_parada = true
///
/// Una parada se reporta si el llamador ES el trazador designado del proceso
/// (comportamiento de `ptrace`: el trazador siempre observa las paradas) o si
/// pidió explícitamente `WUNTRACED`.
pub fn find_waitable(
    parent_pid: u64, target_pid: i64, caller_pgid: u64, want_stops: bool,
) -> (bool, Option<(u64, i32, bool)>) {
    let mut ptab = PROCESS_TABLE.lock();
    let mut has_children = false;
    for p in ptab.processes.iter_mut() {
        unsafe {
            let proc = &mut *p.as_mut_ptr();
            if proc.state == ProcessState::Dead { continue; }
            let es_hijo = proc.ppid == parent_pid;
            let es_trazado_por_mi = proc.ptrace_traced && proc.ptrace_tracer == parent_pid;
            if !es_hijo && !es_trazado_por_mi { continue; }
            let matches = if target_pid == -1 {
                true
            } else if target_pid == 0 {
                proc.pgid == caller_pgid
            } else if target_pid < -1 {
                proc.pgid == (-target_pid) as u64
            } else {
                target_pid as u64 == proc.pid
            };
            if !matches { continue; }
            has_children = true;
            if proc.state == ProcessState::Zombie {
                return (true, Some((proc.pid, proc.wait_status(), false)));
            }
            if proc.state == ProcessState::Stopped && !proc.stop_reported
                && (want_stops || es_trazado_por_mi)
            {
                let sig = if proc.stop_signal == 0 { 5u8 } else { proc.stop_signal };
                proc.stop_reported = true;
                let status = ((sig as i32) << 8) | 0x7f;
                return (true, Some((proc.pid, status, true)));
            }
        }
    }
    (has_children, None)
}

/// Carácter de estado de un PID (diagnóstico).
pub fn get_state_char(pid: u64) -> char {
    let ptab = PROCESS_TABLE.lock();
    for p in &ptab.processes {
        unsafe {
            let proc = &*p.as_ptr();
            if proc.pid == pid {
                return match proc.state {
                    ProcessState::Dead => 'X',
                    ProcessState::Ready => 'R',
                    ProcessState::Running => 'A',
                    ProcessState::Blocked => 'B',
                    ProcessState::Zombie => 'Z',
                    ProcessState::Stopped => 'T',
                };
            }
        }
    }
    '?'
}

/// [CICLO v6.x] El trazador ha realizado al menos una operación real de
/// depuración sobre este proceso (no basta con `PTRACE_TRACEME`).
pub fn mark_tracer_engaged(pid: u64) {
    let ptab = PROCESS_TABLE.lock();
    for p in &ptab.processes {
        unsafe {
            let proc = &mut *(p.as_ptr() as *mut Process);
            if proc.pid == pid && proc.state != ProcessState::Dead {
                proc.ptrace_engaged = true;
                return;
            }
        }
    }
}

/// [CICLO v6.x] Hay una sesión de depuración ACTIVA: el proceso se declaró
/// trazable, su trazador es otro proceso vivo y éste ya ha operado sobre él.
/// Sólo en ese caso el trap detiene al objetivo y se notifica por `waitpid`;
/// si el proceso únicamente verificó el ABI de `TRACEME`, conserva el
/// comportamiento normal (SIGTRAP entregado a su manejador).
// ═══════════════════════════════════════════════════════════════════════════
//  Condiciones de puntos de parada (breakpoints condicionales)
//
//  A diferencia de un `if` en el trazador (que obligaría a despertarlo en cada
//  paso), la condición se evalúa DENTRO del manejo del trap, con los registros
//  reales del frame, y el kernel decide: si no se cumple, reanuda al objetivo
//  sin notificar a nadie. Es la semántica de `break ... if <cond>` de un
//  depurador de verdad, y la única forma de que el coste no dependa del
//  trazador.
// ═══════════════════════════════════════════════════════════════════════════

pub const COND_OP_EQ: u8 = 0;
pub const COND_OP_NE: u8 = 1;
pub const COND_OP_LT: u8 = 2;
pub const COND_OP_LE: u8 = 3;
pub const COND_OP_GT: u8 = 4;
pub const COND_OP_GE: u8 = 5;

/// Índices de registro usados por las condiciones (deben coincidir con
/// `portix-rt::syscall`, que es quien los publica al usuario).
pub const COND_REG_RAX: u8 = 0;
pub const COND_REG_RBX: u8 = 1;
pub const COND_REG_RCX: u8 = 2;
pub const COND_REG_RDX: u8 = 3;
pub const COND_REG_RSI: u8 = 4;
pub const COND_REG_RDI: u8 = 5;
pub const COND_REG_RBP: u8 = 6;
pub const COND_REG_RSP: u8 = 7;
pub const COND_REG_R8: u8 = 8;
pub const COND_REG_RIP: u8 = 16;
pub const COND_REG_RFLAGS: u8 = 17;
/// Pseudo-registro `addr`: en un accceso vigilado resuelve a la dirección que
/// disparó el trap. No es un registro del procesador: es la dirección del
/// evento, y permite expresar rangos sobre lo que se está vigilando.
pub const COND_REG_ADDR: u8 = 18;

fn reg_de_condicion(reg: u8, f: &crate::arch::isr_handlers::CrashFrame, addr: u64) -> u64 {
    match reg {
        COND_REG_RAX => f.rax,
        COND_REG_RBX => f.rbx,
        COND_REG_RCX => f.rcx,
        COND_REG_RDX => f.rdx,
        COND_REG_RSI => f.rsi,
        COND_REG_RDI => f.rdi,
        COND_REG_RBP => f.rbp,
        COND_REG_RSP => f.rsp,
        8 => f.r8,
        9 => f.r9,
        10 => f.r10,
        11 => f.r11,
        12 => f.r12,
        13 => f.r13,
        14 => f.r14,
        15 => f.r15,
        COND_REG_RIP => f.rip,
        COND_REG_RFLAGS => f.rflags,
        COND_REG_ADDR => addr,
        _ => 0,
    }
}

/// Nombre simbólico del registro, para el relato del kernel.
pub fn cond_reg_name(reg: u8) -> &'static str {
    match reg {
        COND_REG_RAX => "rax", COND_REG_RBX => "rbx", COND_REG_RCX => "rcx",
        COND_REG_RDX => "rdx", COND_REG_RSI => "rsi", COND_REG_RDI => "rdi",
        COND_REG_RBP => "rbp", COND_REG_RSP => "rsp",
        8 => "r8", 9 => "r9", 10 => "r10", 11 => "r11",
        12 => "r12", 13 => "r13", 14 => "r14", 15 => "r15",
        COND_REG_RIP => "rip", COND_REG_RFLAGS => "rflags", COND_REG_ADDR => "addr",
        _ => "?",
    }
}

pub fn cond_op_name(op: u8) -> &'static str {
    match op {
        COND_OP_EQ => "==", COND_OP_NE => "!=", COND_OP_LT => "<",
        COND_OP_LE => "<=", COND_OP_GT => ">", COND_OP_GE => ">=",
        _ => "?",
    }
}

fn evaluar_op(op: u8, actual: u64, valor: u64) -> bool {
    match op {
        COND_OP_NE => actual != valor,
        COND_OP_LT => (actual as i64) < (valor as i64),
        COND_OP_LE => (actual as i64) <= (valor as i64),
        COND_OP_GT => (actual as i64) > (valor as i64),
        COND_OP_GE => (actual as i64) >= (valor as i64),
        _ => actual == valor,
    }
}

/// Define (o reemplaza) la condición de un punto de parada en una dirección.
pub fn set_bp_cond(pid: u64, addr: u64, reg: u8, op: u8, value: u64) -> i64 {
    set_bp_condn(pid, addr, 1, 0, &[(reg, op, value), (0, 0, 0), (0, 0, 0), (0, 0, 0)])
}

/// Define una condición de DOS cláusulas unidas por Y lógico: se cumple si se
/// cumplen ambas. Es la forma de expresar un rango (`reg >= A` y `reg < B`).
pub fn set_bp_cond2(
    pid: u64, addr: u64,
    reg: u8, op: u8, value: u64,
    reg2: u8, op2: u8, value2: u64,
) -> i64 {
    set_bp_condn(pid, addr, 2, 0, &[(reg, op, value), (reg2, op2, value2), (0, 0, 0), (0, 0, 0)])
}

/// Forma general del lenguaje de condiciones: hasta CUATRO comparaciones
/// (`n` cláusulas) repartidas en DOS grupos; las cláusulas de un mismo grupo se
/// unen por Y lógico y los grupos por O lógico. `g2` es el índice de la primera
/// cláusula del segundo grupo (0 = no hay segundo grupo).
///
/// Con esto se expresa sin operadores nuevos:
///   * un rango ................ `A <= x < B`        -> n=2, g2=0
///   * una disyunción .......... `x == A || x == B`  -> n=2, g2=1
///   * un rango con guarda ..... `A <= x < B && y != 0` -> n=3, g2=0
///   * rango o valor exacto .... `(A <= x < B) || x == C` -> n=3, g2=2
pub fn set_bp_condn(pid: u64, addr: u64, n: u8, g2: u8, cl: &[(u8, u8, u64); 4]) -> i64 {
    if addr == 0 { return -22; }
    let n = n.clamp(1, 4);
    let g2 = if g2 >= n { 0 } else { g2 };
    let mut guard = match PROCESS_TABLE.try_lock() { Some(g) => g, None => return -11 };
    for p in guard.processes.iter_mut() {
        unsafe {
            let pp = p.as_mut_ptr();
            if (*pp).pid != pid || (*pp).state == ProcessState::Dead { continue; }
            let usadas = ((*pp).bp_cond_count as usize).min(4);
            let mut slot = usadas;
            for i in 0..usadas {
                if (*pp).bp_cond_addr[i] == addr { slot = i; break; }
            }
            if slot >= 4 { return -28; } // -ENOSPC
            (*pp).bp_cond_addr[slot] = addr;
            (*pp).bp_cond_clauses[slot] = n;
            (*pp).bp_cond_g2[slot] = g2;
            for c in 0..4usize {
                let (r, o, v) = cl[c];
                (*pp).bp_cond_regs[slot][c] = r;
                (*pp).bp_cond_ops[slot][c] = o;
                (*pp).bp_cond_vals[slot][c] = v;
            }
            (*pp).bp_cond_hits[slot] = 0;
            (*pp).bp_cond_skips[slot] = 0;
            (*pp).bp_cond_ignore[slot] = 0;
            if slot == usadas {
                (*pp).bp_cond_count = (usadas + 1) as u8;
            }
            return slot as i64;
        }
    }
    -3 // -ESRCH
}

pub fn clear_bp_cond(pid: u64, addr: u64) -> i64 {
    let mut guard = match PROCESS_TABLE.try_lock() { Some(g) => g, None => return -11 };
    for p in guard.processes.iter_mut() {
        unsafe {
            let pp = p.as_mut_ptr();
            if (*pp).pid != pid || (*pp).state == ProcessState::Dead { continue; }
            let n = ((*pp).bp_cond_count as usize).min(4);
            for i in 0..n {
                if (*pp).bp_cond_addr[i] == addr {
                    // Compactación de la tabla de condiciones del proceso.
                    for j in i..n.saturating_sub(1) {
                        (*pp).bp_cond_addr[j] = (*pp).bp_cond_addr[j + 1];
                        (*pp).bp_cond_clauses[j] = (*pp).bp_cond_clauses[j + 1];
                        (*pp).bp_cond_g2[j] = (*pp).bp_cond_g2[j + 1];
                        (*pp).bp_cond_regs[j] = (*pp).bp_cond_regs[j + 1];
                        (*pp).bp_cond_ops[j] = (*pp).bp_cond_ops[j + 1];
                        (*pp).bp_cond_vals[j] = (*pp).bp_cond_vals[j + 1];
                        (*pp).bp_cond_hits[j] = (*pp).bp_cond_hits[j + 1];
                        (*pp).bp_cond_skips[j] = (*pp).bp_cond_skips[j + 1];
                        (*pp).bp_cond_ignore[j] = (*pp).bp_cond_ignore[j + 1];
                    }
                    (*pp).bp_cond_count = n.saturating_sub(1) as u8;
                    return 0;
                }
            }
            return -2; // -ENOENT
        }
    }
    -3
}

/// Define una condición de DOS cláusulas unidas por Y lógico: se cumple si se
/// cumplen ambas. Es la forma de expresar un rango (`reg >= A` y `reg < B`).
pub fn set_bp_cond_ignore(pid: u64, addr: u64, n: u32) -> i64 {
    let mut guard = match PROCESS_TABLE.try_lock() { Some(g) => g, None => return -11 };
    for p in guard.processes.iter_mut() {
        unsafe {
            let pp = p.as_mut_ptr();
            if (*pp).pid != pid || (*pp).state == ProcessState::Dead { continue; }
            let cnt = ((*pp).bp_cond_count as usize).min(4);
            for i in 0..cnt {
                if (*pp).bp_cond_addr[i] == addr {
                    (*pp).bp_cond_ignore[i] = n;
                    return i as i64;
                }
            }
            return -2; // -ENOENT
        }
    }
    -3
}

/// Contadores de una condición: (aciertos, saltos).
pub fn bp_cond_stats(pid: u64, addr: u64) -> Option<(u32, u32, u32, u8, u8, [(u8, u8, u64); 4])> {
    let guard = PROCESS_TABLE.lock();
    for p in &guard.processes {
        unsafe {
            let pp = &*p.as_ptr();
            if pp.pid != pid || pp.state == ProcessState::Dead { continue; }
            let n = (pp.bp_cond_count as usize).min(4);
            for i in 0..n {
                if pp.bp_cond_addr[i] == addr {
                    let mut cl = [(0u8, 0u8, 0u64); 4];
                    for c in 0..4usize {
                        cl[c] = (pp.bp_cond_regs[i][c], pp.bp_cond_ops[i][c], pp.bp_cond_vals[i][c]);
                    }
                    return Some((pp.bp_cond_hits[i], pp.bp_cond_skips[i], pp.bp_cond_ignore[i], pp.bp_cond_clauses[i], pp.bp_cond_g2[i], cl));
                }
            }
            return None;
        }
    }
    None
}
pub enum CondEval {
    /// No hay condición para esta dirección: el trap se trata como siempre.
    SinCondicion,
    /// Hay condición y se cumple: el trazador debe ser notificado.
    Verdadera,
    /// Hay condición y no se cumple: el kernel reanuda sin despertar a nadie.
    Falsa,
}

/// Evalúa, si existe, la condición asociada a la dirección `addr` (la del punto
/// de parada) contra los registros del frame que provocó el trap.
pub fn eval_bp_cond(pid: u64, addr: u64, f: &crate::arch::isr_handlers::CrashFrame) -> CondEval {
    let mut guard = match PROCESS_TABLE.try_lock() { Some(g) => g, None => return CondEval::SinCondicion };
    for p in guard.processes.iter_mut() {
        unsafe {
            let pp = p.as_mut_ptr();
            if (*pp).pid != pid || (*pp).state == ProcessState::Dead { continue; }
            let total = ((*pp).bp_cond_count as usize).min(4);
            for i in 0..total {
                if (*pp).bp_cond_addr[i] != addr { continue; }
                let n = ((*pp).bp_cond_clauses[i] as usize).clamp(1, 4);
                let g2 = ((*pp).bp_cond_g2[i] as usize).min(4);
                // Y lógico dentro de cada grupo; el resultado es la O de los grupos.
                let mut grupo1 = true;
                let mut grupo2 = g2 > 0;
                crate::drivers::serial::write_str("[BPCOND] PID=");
                crate::drivers::serial::write_usize(pid as usize);
                crate::drivers::serial::write_str(" addr=");
                crate::drivers::serial::write_hex(addr as usize);
                for c in 0..n {
                    let reg = (*pp).bp_cond_regs[i][c];
                    let op = (*pp).bp_cond_ops[i][c];
                    let valor = (*pp).bp_cond_vals[i][c];
                    let actual = reg_de_condicion(reg, f, addr);
                    let cumple = evaluar_op(op, actual, valor);
                    let en_g2 = g2 > 0 && c >= g2;
                    if en_g2 { grupo2 = grupo2 && cumple; } else { grupo1 = grupo1 && cumple; }
                    crate::drivers::serial::write_str(if c == 0 { " " } else if en_g2 && c == g2 { " || " } else { " && " });
                    crate::drivers::serial::write_str(cond_reg_name(reg));
                    crate::drivers::serial::write_str(cond_op_name(op));
                    crate::drivers::serial::write_hex(valor as usize);
                    crate::drivers::serial::write_str(" actual=");
                    crate::drivers::serial::write_hex(actual as usize);
                }
                let cumple = grupo1 || grupo2;
                if cumple {
                    (*pp).bp_cond_hits[i] = (*pp).bp_cond_hits[i].wrapping_add(1);
                    // `ignore N`: las N primeras evaluaciones verdaderas se
                    // consumen sin despertar al trazador.
                    if (*pp).bp_cond_ignore[i] > 0 {
                        (*pp).bp_cond_ignore[i] -= 1;
                        (*pp).bp_cond_skips[i] = (*pp).bp_cond_skips[i].wrapping_add(1);
                        crate::drivers::serial::write_str(" -> CIERTA (grupo1=");
                        crate::drivers::serial::write_str(if grupo1 { "1" } else { "0" });
                        crate::drivers::serial::write_str(" grupo2=");
                        crate::drivers::serial::write_str(if grupo2 { "1" } else { "0" });
                        crate::drivers::serial::write_str(") pero IGNORADA (ignore restante=");
                        crate::drivers::serial::write_usize((*pp).bp_cond_ignore[i] as usize);
                        crate::drivers::serial::write_str("): reanudado por el kernel\n");
                        return CondEval::Falsa;
                    }
                    crate::drivers::serial::write_str(" -> CIERTA (aciertos=");
                    crate::drivers::serial::write_usize((*pp).bp_cond_hits[i] as usize);
                    crate::drivers::serial::write_str("): parada notificada al trazador\n");
                    return CondEval::Verdadera;
                }
                (*pp).bp_cond_skips[i] = (*pp).bp_cond_skips[i].wrapping_add(1);
                crate::drivers::serial::write_str(" -> FALSA (saltos=");
                crate::drivers::serial::write_usize((*pp).bp_cond_skips[i] as usize);
                crate::drivers::serial::write_str("): reanudado por el kernel sin notificar\n");
                return CondEval::Falsa;
            }
            return CondEval::SinCondicion;
        }
    }
    CondEval::SinCondicion
}

pub fn bp_trap_addr(pid: u64, rip: u64, es_int3: bool, dr6: u64) -> u64 {
    if es_int3 {
        return rip.wrapping_sub(1);
    }
    if crate::arch::debug_regs::dr6_is_data_breakpoint(dr6) {
        if let Some(slot) = crate::arch::debug_regs::fired_slot(dr6) {
            let guard = PROCESS_TABLE.lock();
            for p in &guard.processes {
                unsafe {
                    let pp = &*p.as_ptr();
                    if pp.pid == pid && pp.state != ProcessState::Dead {
                        if slot < (pp.watch_count as usize).min(4) {
                            return pp.watch_addr[slot];
                        }
                        break;
                    }
                }
            }
        }
    }
    rip
}

/// ¿La ranura que disparó el trap es un punto de parada de ejecución?
pub fn bp_trap_es_ejecucion(pid: u64, dr6: u64) -> bool {
    let Some(slot) = crate::arch::debug_regs::fired_slot(dr6) else { return false };
    let guard = PROCESS_TABLE.lock();
    for p in &guard.processes {
        unsafe {
            let pp = &*p.as_ptr();
            if pp.pid == pid && pp.state != ProcessState::Dead {
                return crate::arch::debug_regs::slot_is_exec(pp, slot);
            }
        }
    }
    false
}

/// Marca que la ranura `slot` debe volver a armarse en el próximo trap (tras
/// ejecutar UNA instrucción con el punto de parada de ejecución desactivado).
/// [FRENTE C] Guarda el DR6 de la parada en curso para que el trazador pueda
/// consultarlo (`PTRACE_GETTRAPINFO`).
pub fn anotar_dr6(pid: u64, dr6: u64) {
    let guard = PROCESS_TABLE.lock();
    for p in &guard.processes {
        unsafe {
            let pp = p.as_ptr() as *mut Process;
            if (*pp).pid == pid && (*pp).state != ProcessState::Dead {
                (*pp).stop_dr6 = dr6;
                return;
            }
        }
    }
}

/// [FRENTE C] Información de la última parada: (DR6, ranura que disparó o 0xFF,
/// dirección vigilada por esa ranura, código de longitud, RIP de la parada).
pub fn trap_info(pid: u64) -> Option<(u64, u8, u64, u8, u64)> {
    let guard = PROCESS_TABLE.lock();
    for p in &guard.processes {
        unsafe {
            let pp = &*p.as_ptr();
            if pp.pid == pid && pp.state != ProcessState::Dead {
                let dr6 = pp.stop_dr6;
                let n = (pp.watch_count as usize).min(4);
                let (slot, addr, len) = match crate::arch::debug_regs::fired_slot(dr6) {
                    Some(sl) if sl < n => (sl as u8, pp.watch_addr[sl], pp.watch_len[sl]),
                    _ => (0xFF, 0, 0),
                };
                return Some((dr6, slot, addr, len, pp.stop_addr));
            }
        }
    }
    None
}

pub fn marcar_rearmado(pid: u64, slot: usize) {
    let mut guard = match PROCESS_TABLE.try_lock() { Some(g) => g, None => return };
    for p in guard.processes.iter_mut() {
        unsafe {
            let pp = p.as_mut_ptr();
            if (*pp).pid == pid && (*pp).state != ProcessState::Dead {
                (*pp).bp_cond_rearm = slot as u8;
                return;
            }
        }
    }
}

/// Si hay un re-armado pendiente: devuelve la ranura, la re-arma y limpia el
/// estado. El trap que lo provoca (el paso de una instrucción) se consume.
pub fn tomar_rearmado(pid: u64) -> Option<usize> {
    let mut guard = match PROCESS_TABLE.try_lock() { Some(g) => g, None => return None };
    for p in guard.processes.iter_mut() {
        unsafe {
            let pp = p.as_mut_ptr();
            if (*pp).pid == pid && (*pp).state != ProcessState::Dead {
                let slot = (*pp).bp_cond_rearm;
                if slot == 0xFF {
                    return None;
                }
                (*pp).bp_cond_rearm = 0xFF;
                crate::arch::debug_regs::apply_for(&*pp);
                return Some(slot as usize);
            }
        }
    }
    None
}

pub fn is_process_actively_traced(pid: u64) -> bool {
    let ptab = PROCESS_TABLE.lock();
    for p in &ptab.processes {
        unsafe {
            let proc = &*p.as_ptr();
            if proc.pid == pid && proc.state != ProcessState::Dead {
                return proc.ptrace_traced
                    && proc.ptrace_tracer != 0
                    && proc.ptrace_tracer != pid
                    && proc.ptrace_engaged;
            }
        }
    }
    false
}

/// [CICLO v6.x] El proceso pidió el paso a paso PARA SÍ MISMO
/// (`ptrace(PTRACE_SINGLESTEP, propio_pid)`): es una auto-observación, así que
/// el trap resultante se consume en el kernel y la ejecución continúa, en
/// lugar de matar al proceso con un SIGTRAP que nadie pidió. Si además existe
/// un trazador externo activo, la parada se le entrega a él
/// (`is_process_actively_traced` tiene prioridad).
pub fn consume_self_singlestep(pid: u64) -> bool {
    let mut ptab = PROCESS_TABLE.lock();
    for p in ptab.processes.iter_mut() {
        unsafe {
            let proc = &mut *p.as_mut_ptr();
            if proc.pid == pid && proc.state != ProcessState::Dead && proc.ptrace_singlestep {
                proc.ptrace_singlestep = false;
                proc.stop_reported = true;
                return true;
            }
        }
    }
    false
}

pub fn is_process_traced(pid: u64) -> bool {
    let ptab = PROCESS_TABLE.lock();
    for p in &ptab.processes {
        unsafe {
            let proc = &*p.as_ptr();
            if proc.pid == pid && proc.state != ProcessState::Dead {
                return proc.ptrace_traced;
            }
        }
    }
    false
}

pub fn current_uid() -> u32 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.uid).unwrap_or(0)
}

pub fn current_gid() -> u32 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.gid).unwrap_or(0)
}

pub fn current_euid() -> u32 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.euid).unwrap_or(0)
}

pub fn current_egid() -> u32 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.egid).unwrap_or(0)
}

pub fn set_current_uid(uid: u32) -> bool {
    let guard = PROCESS_TABLE.lock();
    if let Some(p) = guard.current_process_ref() {
        if p.euid == 0 || p.uid == uid {
            p.uid = uid;
            p.euid = uid;
            return true;
        }
    }
    false
}

pub fn set_current_gid(gid: u32) -> bool {
    let guard = PROCESS_TABLE.lock();
    if let Some(p) = guard.current_process_ref() {
        if p.egid == 0 || p.gid == gid {
            p.gid = gid;
            p.egid = gid;
            return true;
        }
    }
    false
}

pub fn set_current_euid(euid: u32) -> bool {
    let guard = PROCESS_TABLE.lock();
    if let Some(p) = guard.current_process_ref() {
        if p.euid == 0 || p.uid == euid || p.euid == euid {
            p.euid = euid;
            return true;
        }
    }
    false
}

pub fn set_current_egid(egid: u32) -> bool {
    let guard = PROCESS_TABLE.lock();
    if let Some(p) = guard.current_process_ref() {
        if p.euid == 0 || p.gid == egid || p.egid == egid {
            p.egid = egid;
            return true;
        }
    }
    false
}

pub fn current_pgid() -> u64 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.pgid).unwrap_or(0)
}

pub fn current_sid() -> u64 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.sid).unwrap_or(0)
}

pub fn current_umask() -> u32 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.umask).unwrap_or(0o022)
}

pub fn set_current_umask(mask: u32) -> u32 {
    let guard = PROCESS_TABLE.lock();
    if let Some(p) = guard.current_process_ref() {
        let old = p.umask;
        p.umask = mask & 0o777;
        old
    } else {
        0o022
    }
}

pub fn get_pgid(pid: u64) -> Option<u64> {
    let guard = PROCESS_TABLE.lock();
    if pid == 0 {
        return guard.current_process_ref().map(|p| p.pgid);
    }
    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr() as *const Process;
            if (*proc).pid == pid && (*proc).state != ProcessState::Dead {
                return Some((*proc).pgid);
            }
        }
    }
    None
}

pub fn set_pgid(pid: u64, mut pgid: u64) -> Result<(), i32> {
    let guard = PROCESS_TABLE.lock();
    let cur_pid = guard.current_process_ref().map(|p| p.pid).ok_or(-3)?;
    let cur_sid = guard.current_process_ref().map(|p| p.sid).ok_or(-3)?;
    let target_pid = if pid == 0 { cur_pid } else { pid };

    let mut target_ptr: *mut Process = core::ptr::null_mut();
    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr() as *mut Process;
            if (*proc).pid == target_pid && (*proc).state != ProcessState::Dead {
                target_ptr = proc;
                break;
            }
        }
    }
    if target_ptr.is_null() {
        return Err(-3); // -ESRCH
    }

    unsafe {
        if (*target_ptr).pid != cur_pid && (*target_ptr).ppid != cur_pid {
            return Err(-3); // -ESRCH
        }
        if (*target_ptr).sid == (*target_ptr).pid && (*target_ptr).pid != cur_pid {
            return Err(-1); // -EPERM
        }
        if (*target_ptr).sid != cur_sid {
            return Err(-1); // -EPERM
        }
        if pgid == 0 {
            pgid = target_pid;
        }
        let mut group_found = false;
        for p in &guard.processes {
            let proc = p.as_ptr() as *const Process;
            if (*proc).pgid == pgid && (*proc).state != ProcessState::Dead {
                if (*proc).sid != cur_sid {
                    return Err(-1); // -EPERM
                }
                group_found = true;
                break;
            }
        }
        if pgid != target_pid && !group_found {
            return Err(-1); // -EPERM
        }
        (*target_ptr).pgid = pgid;
    }
    Ok(())
}

pub fn setsid() -> Result<u64, i32> {
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    if cur.pgid == cur.pid {
        return Err(-1); // -EPERM
    }
    cur.sid = cur.pid;
    cur.pgid = cur.pid;
    Ok(cur.sid)
}

pub fn getsid(pid: u64) -> Option<u64> {
    let guard = PROCESS_TABLE.lock();
    if pid == 0 {
        return guard.current_process_ref().map(|p| p.sid);
    }
    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr() as *const Process;
            if (*proc).pid == pid && (*proc).state != ProcessState::Dead {
                return Some((*proc).sid);
            }
        }
    }
    None
}

pub fn getrlimit(resource: i32) -> Result<(u64, u64), i32> {
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    match resource {
        7 => Ok((cur.rlimit_nofile, cur.rlimit_nofile_max)), // RLIMIT_NOFILE
        // [b191 / T-011] Valores VERDADEROS: la pila de usuario crece hasta 8 MiB (T-004);
        // el núcleo no impone límite de datos ni de espacio de direcciones (RLIM_INFINITY).
        // Antes RLIMIT_AS decía 64 MiB y el JVM recortaba su montículo a esa cifra.
        3 => Ok((8 * 1024 * 1024, 8 * 1024 * 1024)),         // RLIMIT_STACK
        2 | 9 => Ok((u64::MAX, u64::MAX)),                   // RLIMIT_DATA / RLIMIT_AS
        6 => Ok((MAX_PROCS as u64, MAX_PROCS as u64)),       // RLIMIT_NPROC: ranuras reales
        0 => Ok((u64::MAX, u64::MAX)),                       // RLIMIT_CPU
        1 => Ok((u64::MAX, u64::MAX)),                       // RLIMIT_FSIZE
        4 => Ok((0, u64::MAX)),                              // RLIMIT_CORE
        0..=15 => Ok((1024 * 1024, 1024 * 1024)),
        _ => Err(-22),                                       // -EINVAL
    }
}

pub fn setrlimit(resource: i32, soft: u64, hard: u64) -> Result<(), i32> {
    if soft > hard {
        return Err(-22); // -EINVAL
    }
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    match resource {
        7 => { // RLIMIT_NOFILE
            if hard > MAX_FDS as u64 {
                return Err(-22); // -EINVAL
            }
            if cur.euid != 0 && hard > cur.rlimit_nofile_max {
                return Err(-1); // -EPERM
            }
            cur.rlimit_nofile = soft;
            cur.rlimit_nofile_max = hard;
            Ok(())
        }
        0..=15 => Ok(()),
        _ => Err(-22),
    }
}

pub fn get_times(tms: &mut Tms) -> Result<u64, i32> {
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    tms.tms_utime = cur.utime as i64;
    tms.tms_stime = cur.stime as i64;
    tms.tms_cutime = cur.cutime as i64;
    tms.tms_cstime = cur.cstime as i64;
    Ok(crate::time::pit::ticks())
}

pub fn getgroups(size: usize, list: &mut [u32]) -> Result<usize, i32> {
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    if size == 0 {
        return Ok(cur.ngroups);
    }
    if size < cur.ngroups {
        return Err(-22); // -EINVAL
    }
    list[..cur.ngroups].copy_from_slice(&cur.groups[..cur.ngroups]);
    Ok(cur.ngroups)
}

pub fn setgroups(groups: &[u32]) -> Result<(), i32> {
    if groups.len() > 16 {
        return Err(-22); // -EINVAL
    }
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    if cur.euid != 0 {
        return Err(-1); // -EPERM
    }
    cur.groups[..groups.len()].copy_from_slice(groups);
    cur.ngroups = groups.len();
    Ok(())
}

pub fn getresuid() -> Option<(u32, u32, u32)> {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| (p.uid, p.euid, p.suid))
}

pub fn setresuid(ruid: u32, euid: u32, suid: u32) -> Result<(), i32> {
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    if cur.euid != 0 {
        if ruid != 0xFFFFFFFF && ruid != cur.uid && ruid != cur.euid && ruid != cur.suid {
            return Err(-1); // -EPERM
        }
        if euid != 0xFFFFFFFF && euid != cur.uid && euid != cur.euid && euid != cur.suid {
            return Err(-1); // -EPERM
        }
        if suid != 0xFFFFFFFF && suid != cur.uid && suid != cur.euid && suid != cur.suid {
            return Err(-1); // -EPERM
        }
    }
    if ruid != 0xFFFFFFFF { cur.uid = ruid; }
    if euid != 0xFFFFFFFF { cur.euid = euid; }
    if suid != 0xFFFFFFFF { cur.suid = suid; }
    Ok(())
}

pub fn getresgid() -> Option<(u32, u32, u32)> {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| (p.gid, p.egid, p.sgid))
}

pub fn setresgid(rgid: u32, egid: u32, sgid: u32) -> Result<(), i32> {
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    if cur.euid != 0 {
        if rgid != 0xFFFFFFFF && rgid != cur.gid && rgid != cur.egid && rgid != cur.sgid {
            return Err(-1); // -EPERM
        }
        if egid != 0xFFFFFFFF && egid != cur.gid && egid != cur.egid && egid != cur.sgid {
            return Err(-1); // -EPERM
        }
        if sgid != 0xFFFFFFFF && sgid != cur.gid && sgid != cur.egid && sgid != cur.sgid {
            return Err(-1); // -EPERM
        }
    }
    if rgid != 0xFFFFFFFF { cur.gid = rgid; }
    if egid != 0xFFFFFFFF { cur.egid = egid; }
    if sgid != 0xFFFFFFFF { cur.sgid = sgid; }
    Ok(())
}

pub fn process_by_pid(pid: u64) -> Option<&'static mut Process> {
    PROCESS_TABLE.lock().process_by_pid_ref(pid)
}

pub fn process_create(entry: u64, name: &str) -> Option<u64> {
    let cr3 = paging::new_address_space()?;
    process_create_into(cr3, entry, name)
}

pub fn process_create_into(cr3: u64, entry: u64, name: &str) -> Option<u64> {
    process_create_into_args(cr3, entry, name, name)
}

pub fn process_create_into_args(cr3: u64, entry: u64, name: &str, cmdline: &str) -> Option<u64> {
    process_create_into_args_auxv(cr3, entry, name, cmdline, None)
}

pub fn process_create_into_args_auxv(
    cr3: u64,
    entry: u64,
    name: &str,
    cmdline: &str,
    aux_info: Option<AuxvInfo>,
) -> Option<u64> {
    if entry as usize >= USER_STACK_TOP { return None; }
    let (slot, pid) = alloc_slot_and_pid()?;

    let ks_layout = Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE).ok()?;
    let ks_ptr = unsafe { alloc::alloc::alloc_zeroed(ks_layout) };
    if ks_ptr.is_null() { return None; }
    let ks_base = ks_ptr as usize;
    let ks_top = ks_base + KERNEL_STACK_SIZE;

    // [Fase B.4] pila de usuario: marcos físicos contiguos del asignador de marcos
    let us_base = paging::asignar_marcos_usuario_contiguos(USER_STACK_SIZE / PAGE_SIZE)?;

    let us_vaddr = USER_STACK_TOP - USER_STACK_SIZE;
    for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
        paging::map_page_user(cr3, us_vaddr + i * PAGE_SIZE, us_base + i * PAGE_SIZE).ok()?;
    }

    // Set up initial user stack with argc, argv, envp, and auxv conforming to AMD64 ABI
    let us_top_phys = us_base + USER_STACK_SIZE;
    let (user_rsp_va, proc) = unsafe {
        let mut sp = us_top_phys;

        let tokens: alloc::vec::Vec<&str> = cmdline.split_whitespace().collect();
        let final_tokens = if tokens.is_empty() {
            alloc::vec![name]
        } else {
            tokens
        };

        let mut arg_vaddrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
        for tok in &final_tokens {
            let bytes = tok.as_bytes();
            sp -= bytes.len() + 1;
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), sp as *mut u8, bytes.len());
            *((sp + bytes.len()) as *mut u8) = 0;
            let va = (us_vaddr + (sp - us_base)) as u64;
            arg_vaddrs.push(va);
        }

        // [CICLO 47-b] Entorno de arranque REAL para los procesos creados por el
        // nucleo (consola de rescate / init): equivale a lo que `login` exporta
        // en un Unix. Los hijos lo heredan y modifican via execve(envp).
        const ENTORNO_ARRANQUE: [&str; 6] = [
            "PATH=/bin:/usr/bin", "HOME=/root", "USER=root",
            "SHELL=/bin/sh", "TERM=xterm-256color", "OS=Portix",
        ];
        let mut env_vaddrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
        for kv in ENTORNO_ARRANQUE.iter() {
            let bytes = kv.as_bytes();
            sp -= bytes.len() + 1;
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), sp as *mut u8, bytes.len());
            *((sp + bytes.len()) as *mut u8) = 0;
            env_vaddrs.push((us_vaddr + (sp - us_base)) as u64);
        }

        // Push 16 pseudo-random bytes for AT_RANDOM
        sp -= 16;
        let random_va = (us_vaddr + (sp - us_base)) as u64;
        let rand_slice = core::slice::from_raw_parts_mut(sp as *mut u8, 16);
        for (i, b) in rand_slice.iter_mut().enumerate() {
            *b = (pid as u8).wrapping_mul(37).wrapping_add(i as u8).wrapping_mul(73) ^ 0xA5;
        }

        // [T-004] Base alineada a 16 (ABI AMD64: RSP % 16 == 0 en _start); la
        // paridad de palabras de abajo sólo lo garantiza partiendo de 16.
        sp &= !15;

        let ai = aux_info.unwrap_or(AuxvInfo {
            phdr: 0x0040_0040,
            phent: 56,
            phnum: 1,
            base: 0,
            entry,
            execfn: arg_vaddrs.first().copied().unwrap_or(0),
        });

        // Auxiliary Vector (type, value) pairs
        let auxv_pairs: [(u64, u64); 16] = [
            (AT_NULL, 0),
            (AT_SYSINFO_EHDR, 0),
            (AT_EXECFN, arg_vaddrs.first().copied().unwrap_or(ai.execfn)),
            (AT_RANDOM, random_va),
            (AT_SECURE, 0),
            (AT_CLKTCK, 100),
            (AT_EGID, 1000),
            (AT_GID, 1000),
            (AT_EUID, 1000),
            (AT_UID, 1000),
            (AT_ENTRY, ai.entry),
            (AT_FLAGS, 0),
            (AT_BASE, ai.base),
            (AT_PAGESZ, 4096),
            (AT_PHNUM, ai.phnum),
            (AT_PHENT, ai.phent),
        ];
        let auxv_hdr = (AT_PHDR, ai.phdr);

        // Ensure 16-byte alignment at _start
        // Total words pushed:
        // argc (1) + argv (final_tokens.len()) + argv_null (1) + envp_null (1) + auxv (34) = 37 + N
        let total_words = 1 + final_tokens.len() + 1 + env_vaddrs.len() + 1 + (auxv_pairs.len() + 1) * 2;
        if total_words % 2 != 0 {
            sp -= 8;
            *(sp as *mut u64) = 0;
        }

        // Push auxv in reverse
        for &(tag, val) in auxv_pairs.iter() {
            sp -= 8;
            *(sp as *mut u64) = val;
            sp -= 8;
            *(sp as *mut u64) = tag;
        }
        sp -= 8;
        *(sp as *mut u64) = auxv_hdr.1;
        sp -= 8;
        *(sp as *mut u64) = auxv_hdr.0;

        // Push envp (NULL + punteros en orden inverso)
        sp -= 8;
        *(sp as *mut u64) = 0;
        for va in env_vaddrs.iter().rev() {
            sp -= 8;
            *(sp as *mut u64) = *va;
        }

        // Push argv NULL terminator
        sp -= 8;
        *(sp as *mut u64) = 0;

        // Push argv pointers in reverse order
        for va in arg_vaddrs.iter().rev() {
            sp -= 8;
            *(sp as *mut u64) = *va;
        }

        // Push argc
        sp -= 8;
        *(sp as *mut u64) = final_tokens.len() as u64;

        let rsp = us_vaddr + (sp - us_base);
        let mut guard = PROCESS_TABLE.lock();
        let p = &mut *guard.processes[slot].as_mut_ptr();
        (rsp, &mut *(p as *mut Process))
    };
    let name_len = name.len().min(31);
    proc.name[..name_len].copy_from_slice(&name.as_bytes()[..name_len]);
    proc.name_len = name_len;
    proc.set_exe_path(name);
    proc.pid = pid;
    proc.state = ProcessState::Ready;
    proc.cr3 = cr3;
    proc.kernel_stack_base = ks_base;
    proc.kernel_stack_top = ks_top;
    proc.kernel_rsp = 0;
    proc.user_stack_phys = us_base;
    proc.user_rsp = user_rsp_va as u64;
    proc.user_rip = entry;
    proc.exit_code = 0;
    proc.term_signal = 0;
    proc.ticks_used = 0;
    proc.sleep_until = 0;
    proc.ring3_ret_rsp = 0;
    proc.ring3_ret_addr = 0;
    const NONE: Option<FdEntry> = None;
    proc.fds = [NONE; MAX_FDS];
    proc.fds[0] = Some(FdEntry::new(FdType::Stdin));
    proc.fds[1] = Some(FdEntry::new(FdType::Stdout));
    proc.fds[2] = Some(FdEntry::new(FdType::Stderr));
    proc.program_break = PROGRAM_BREAK_BASE;
    proc.program_break_end = PROGRAM_BREAK_BASE;
    proc.mmap_base = MMAP_BASE;
    proc.image_end = 0;
    crate::system::devsuite::reset_process(pid);
    proc.registered_ports = [0u16; 16];
    proc.registered_port_count = 0;
    proc.ppid = 0;
    proc.pgid = pid;
    proc.sid = pid;
    proc.umask = 0o022;
    proc.sig_actions = [0usize; 65];
    proc.sig_flags = [0u32; 65];
    proc.sig_masks = [0u64; 65];
    proc.sig_src = [types::SigOrigen::default(); 65];
    proc.sig_rt_cola = [(0u8, types::SigOrigen::default()); types::SIG_RT_COLA_MAX];
    proc.sig_rt_n = 0;
    proc.sig_blocked = 0;
    proc.sig_suspend_activo = false;
    proc.sig_suspend_prev = 0;
    proc.sig_pending = 0;
    proc.niceness = 0;
    proc.rlimit_nofile = MAX_FDS as u64;
    proc.rlimit_nofile_max = MAX_FDS as u64;
    proc.utime = 0;
    proc.stime = 0;
    proc.cutime = 0;
    proc.cstime = 0;
    proc.groups = [0u32; 16];
    proc.ngroups = 0;
    proc.suid = 0;
    proc.sgid = 0;
    proc.alarm_ticks = 0;
    proc.robust_list_head = 0;
    proc.robust_list_len = 0;
    proc.tgid = pid;            // [T-005]
    proc.es_hilo = false;
    proc.clear_child_tid = 0;
    proc.lider_salio = false;
    proc.en_cpu = crate::process::types::SIN_CPU;
    proc.affinity_mask = 0; // [T-025 E3c] 0 = cualquier CPU (antes 1: sólo el BSP, y ningún AP tomaba procesos)
    proc.last_cpu = 0;
    proc.cwd = [0u8; 64];
    proc.cwd[0] = b'/';
    proc.cwd_len = 1;
    proc.uid = 0;
    proc.gid = 0;
    proc.euid = 0;
    proc.egid = 0;
    proc.fs_base = 0;
    proc.gs_base = 0;
    proc.ptrace_traced = false;
    proc.ptrace_tracer = 0;
    proc.ptrace_singlestep = false;
    proc.stop_signal = 0;
    proc.stop_reported = false;
    proc.stop_addr = 0;
    proc.stop_dr6 = 0;
    proc.watch_count = 0;
    proc.bp_cond_count = 0;
    proc.bp_cond_rearm = 0xFF;
    proc.bp_cond_ignore = [0u32; 4];
    proc.watch_addr = [0u64; 4];
    proc.watch_len = [0u8; 4];
    proc.watch_rw = [0u8; 4];
    proc.regs = UserRegs::default();
    proc.regs.rip = entry as u64;
    proc.regs.cs = 0x23;
    proc.regs.ss = 0x1b;
    proc.regs.eflags = 0x202;
    proc.regs.rsp = USER_STACK_TOP as u64;

    setup_kernel_stack(proc);

    serial::write_str("PROC: create PID=");
    serial::write_usize(pid as usize);
    serial::write_str(" name='");
    serial::write_str(proc.name_str());
    serial::write_str("' entry=");
    serial::write_hex(entry as usize);
    serial::write_str(" cr3=");
    serial::write_hex(cr3 as usize);
    serial::write_str("\n");

    Some(pid)
}

pub fn process_fork(parent_rsp: u64) -> Option<u64> {
    process_fork_with_stack(parent_rsp, 0)
}

pub fn process_fork_with_stack(parent_rsp: u64, child_stack: usize) -> Option<u64> {
    let parent_ptr = unsafe { current_raw() };
    if parent_ptr.is_null() { return None; }
    let parent = unsafe { &*parent_ptr };
    // [T-005] Si quien bifurca es un hilo, el estado del espacio de direcciones
    // (brk, mmap, imagen, pila de usuario) vive en su líder.
    let mem: &Process = unsafe { &*lider_raw(parent_ptr) };

    let (slot, child_pid) = alloc_slot_and_pid()?;

    let ks_layout = Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE).ok()?;
    let ks_ptr = unsafe { alloc::alloc::alloc_zeroed(ks_layout) };
    if ks_ptr.is_null() { return None; }
    let ks_base = ks_ptr as usize;
    let ks_top = ks_base + KERNEL_STACK_SIZE;

    // [Fase B.4] pila de usuario: marcos físicos contiguos del asignador de marcos
    let us_base = paging::asignar_marcos_usuario_contiguos(USER_STACK_SIZE / PAGE_SIZE)?;

    unsafe {
        core::ptr::copy_nonoverlapping(
            mem.user_stack_phys as *const u8,
            us_base as *mut u8,
            USER_STACK_SIZE,
        );
    }

    let child_cr3 = paging::new_address_space()?;

    let us_vaddr = USER_STACK_TOP - USER_STACK_SIZE;
    for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
        if paging::map_page_user(child_cr3, us_vaddr + i * PAGE_SIZE, us_base + i * PAGE_SIZE).is_err() {
            return None;
        }
    }

    // Map user ELF text and data pages con Copy-On-Write real
    // [CICLO 49 / T1] La imagen no siempre vive en 0x20000000: un binario
    // estático de distro (no-PIE) se carga en 0x400000 y un PIE en
    // PIE_LOAD_BASE. Antes el hijo del fork salía SIN esas páginas y moría
    // con #PF al ejecutar su primera instrucción (CR2 == RIP).
    let (img_ini, img_fin) = ventana_imagen(mem.image_end);
    let mut v = img_ini;
    while v < img_fin {
        if let Some((paddr, flags)) = paging::translate(parent.cr3, v) {
            if (flags & paging::USER != 0) && (flags & paging::HUGE_PAGE == 0) {
                if flags & paging::WRITABLE != 0 {
                    let _ = paging::mark_page_cow(parent.cr3, v);
                    let cow_flags = (flags & !paging::WRITABLE) | paging::COW_FLAG;
                    let _ = paging::map_page(child_cr3, v, paddr, cow_flags);
                } else {
                    let _ = paging::map_page(child_cr3, v, paddr, flags);
                }
                // [FRENTE C] El hijo pasa a ser un dueño más del marco: su
                // cuenta de referencias sube y sólo se liberará cuando el
                // último de los dos lo suelte.
                let _ = crate::mem::frames::retain_if_tracked(paddr);
                COW_PAGES_SAVED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            }
        }
        v += PAGE_SIZE;
        if (v >> 12) & 0xFF == 0 { paging::respirar(); } // [T-026] cada 256 páginas
    }

    // [CICLO 50 / T1b] Intérprete dinámico (ld-musl / ld-linux) mapeado en
    // INTERP_LOAD_BASE: el hijo lo hereda con Copy-On-Write como la imagen.
    // Sin esto el hijo del fork moría con #PF (CR2 == RIP) dentro del cargador
    // al primer `execve` de un guion de ash dinámico.
    {
        let ini = crate::elf::INTERP_LOAD_BASE as usize;
        let mut v = ini;
        while v < ini + crate::elf::INTERP_WINDOW {
            if let Some((paddr, flags)) = paging::translate(parent.cr3, v) {
                if (flags & paging::USER != 0) && (flags & paging::HUGE_PAGE == 0) {
                    if flags & paging::WRITABLE != 0 {
                        let _ = paging::mark_page_cow(parent.cr3, v);
                        let cow_flags = (flags & !paging::WRITABLE) | paging::COW_FLAG;
                        let _ = paging::map_page(child_cr3, v, paddr, cow_flags);
                    } else {
                        let _ = paging::map_page(child_cr3, v, paddr, flags);
                    }
                    let _ = crate::mem::frames::retain_if_tracked(paddr);
                    COW_PAGES_SAVED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                }
            }
            v += PAGE_SIZE;
        }
    }

    // [Fase B.4] Mapeos de usuario fuera de las ventanas conocidas (MAP_FIXED en
    // la ventana baja, antiguo montículo incluido): copia profunda como brk/mmap.
    {
        let (lo, hi) = ventana_baja_libre(mem.image_end);
        let mut pendientes: alloc::vec::Vec<(usize, usize, u64)> = alloc::vec::Vec::new();
        paging::for_each_user_page(parent.cr3, lo, hi, &mut |v, p, f| pendientes.push((v, p, f)));
        for (k, (v, paddr, flags)) in pendientes.into_iter().enumerate() {
            if k & 63 == 63 { paging::respirar(); } // [T-026]
            if let Some(child_p) = paging::asignar_marco_usuario() {
                unsafe {
                    core::ptr::copy_nonoverlapping(paging::phys_to_virt(paddr) as *const u8, child_p as *mut u8, PAGE_SIZE);
                }
                let _ = paging::map_page(child_cr3, v, child_p, flags & paging::PTE_FLAGS_MASK & !paging::COW_FLAG);
            }
        }
    }

    // [T-004] Pila extendida (páginas crecidas por fallo): copia profunda.
    {
        let (lo, hi) = pila_extendida();
        let mut pendientes: alloc::vec::Vec<(usize, usize, u64)> = alloc::vec::Vec::new();
        paging::for_each_user_page(parent.cr3, lo, hi, &mut |v, p, f| pendientes.push((v, p, f)));
        for (k, (v, paddr, flags)) in pendientes.into_iter().enumerate() {
            if k & 63 == 63 { paging::respirar(); } // [T-026]
            if let Some(child_p) = paging::asignar_marco_usuario() {
                unsafe { core::ptr::copy_nonoverlapping(paging::phys_to_virt(paddr) as *const u8, child_p as *mut u8, PAGE_SIZE); }
                let _ = paging::map_page(child_cr3, v, child_p, flags & paging::PTE_FLAGS_MASK & !paging::COW_FLAG);
            }
        }
    }

    // Map heap (program break) pages - duplicate heap memory so parent and child don't corrupt each other
    let end_break = mem.program_break_end.max(mem.program_break);
    if end_break > PROGRAM_BREAK_BASE {
        let mut v = PROGRAM_BREAK_BASE;
        while v < end_break {
            if let Some((paddr, flags)) = paging::translate(parent.cr3, v) {
                if flags & paging::USER != 0 {
                    if let Ok(_child_page_layout) = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE) {
                        let child_p = paging::asignar_marco_usuario().unwrap_or(0) as *mut u8; // [Fase B.4]
                        if !child_p.is_null() {
                            unsafe {
                                core::ptr::copy_nonoverlapping(paging::phys_to_virt(paddr) as *const u8, child_p, PAGE_SIZE);
                            }
                            let _ = paging::map_page(child_cr3, v, child_p as usize, flags);
                        }
                    }
                }
            }
            v += PAGE_SIZE;
        }
    }

    // Map mmap pages - duplicate anonymous mmap memory
    if mem.mmap_base > MMAP_BASE {
        // [b191 / T-014] Las reservas perezosas (sin marco) se heredan como reservas.
        paging::copiar_perezosas(parent.cr3, child_cr3, MMAP_BASE, mem.mmap_base);
        crate::mem::vmas::duplicar(parent.cr3, child_cr3); // [T-014b] tramos de fichero perezosos
        let mut v = MMAP_BASE;
        while v < mem.mmap_base {
            if let Some((paddr, flags)) = paging::translate(parent.cr3, v) {
                if flags & paging::USER != 0 {
                    if let Ok(_child_page_layout) = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE) {
                        let child_p = paging::asignar_marco_usuario().unwrap_or(0) as *mut u8; // [Fase B.4]
                        if !child_p.is_null() {
                            unsafe {
                                core::ptr::copy_nonoverlapping(paging::phys_to_virt(paddr) as *const u8, child_p, PAGE_SIZE);
                            }
                            // [T1b.2] PTE_FLAGS_MASK conserva el bit 63 (NX); `& 0xFFF` lo perdia y el hijo
                            // heredaba paginas mmap ejecutables.
                            let _ = paging::map_page(child_cr3, v, child_p as usize, flags & paging::PTE_FLAGS_MASK);
                        }
                    }
                }
            }
            v += PAGE_SIZE;
        }
    }

    // [T-035] Páginas PROTEGIDAS con marco (p. ej. `mprotect(PROT_NONE)` sobre
    // datos ya escritos): el hijo hereda una COPIA profunda con la misma
    // protección. Sin este recorrido el hijo vería un hueco (perdería datos y
    // permisos) y al liberar el padre se devolvería el marco.
    {
        let mut copiadas = paging::copiar_protegidas(parent.cr3, child_cr3, img_ini, img_fin);
        let ini = crate::elf::INTERP_LOAD_BASE as usize;
        copiadas += paging::copiar_protegidas(parent.cr3, child_cr3, ini, ini + crate::elf::INTERP_WINDOW);
        let (lo, hi) = ventana_baja_libre(mem.image_end);
        copiadas += paging::copiar_protegidas(parent.cr3, child_cr3, lo, hi);
        let (plo, phi) = pila_extendida();
        copiadas += paging::copiar_protegidas(parent.cr3, child_cr3, plo, phi);
        copiadas += paging::copiar_protegidas(parent.cr3, child_cr3, PROGRAM_BREAK_BASE, mem.program_break_end.max(mem.program_break));
        copiadas += paging::copiar_protegidas(parent.cr3, child_cr3, MMAP_BASE, mem.mmap_base);
        if copiadas > 0 { paging::respirar(); }
    }

    let child_kernel_rsp = unsafe {
        let frame_size = if parent.kernel_stack_top > (parent_rsp as usize)
            && parent.kernel_stack_top - (parent_rsp as usize) <= KERNEL_STACK_SIZE
            && parent.kernel_stack_top - (parent_rsp as usize) >= 416
        {
            parent.kernel_stack_top - (parent_rsp as usize)
        } else {
            // Invocado via instrucción SYSCALL de AMD64 sobre el stack estático __stack_top
            // El frame sintético de IRETQ + PUSH_REGS ocupa exactamente 416 bytes
            416
        };
        let child_rsp_ptr = (ks_top - frame_size) as *mut u8;
        core::ptr::copy_nonoverlapping(parent_rsp as *const u8, child_rsp_ptr, frame_size);
        let rax_ptr = (child_rsp_ptr as usize + 256) as *mut u64;
        *rax_ptr = 0;
        if child_stack != 0 {
            let rsp_ptr = (child_rsp_ptr as usize + 400) as *mut u64;
            *rsp_ptr = child_stack as u64;
        }
        child_rsp_ptr as u64
    };

    let mut guard = PROCESS_TABLE.lock();
    let child = unsafe { &mut *guard.processes[slot].as_mut_ptr() };

    child.pid = child_pid;
    child.ppid = parent.pid;
    child.state = ProcessState::Ready;
    child.name = parent.name;
    child.name_len = parent.name_len;
    child.exe_path = parent.exe_path;
    child.exe_len = parent.exe_len;
    child.cr3 = child_cr3;
    child.kernel_stack_base = ks_base;
    child.kernel_stack_top = ks_top;
    child.kernel_rsp = child_kernel_rsp;
    child.user_stack_phys = us_base;
    child.user_rsp = if child_stack != 0 { child_stack as u64 } else { parent.user_rsp };
    child.user_rip = parent.user_rip;
    child.exit_code = 0;
    child.term_signal = 0;
    child.ticks_used = 0;
    child.sleep_until = 0;
    child.ring3_ret_rsp = 0;
    child.ring3_ret_addr = 0;
    child.fds = *tabla_fds(parent);                 // [T-005] la tabla real (la del líder si es hilo)
    child.program_break = mem.program_break;
    child.program_break_end = mem.program_break_end;
    child.mmap_base = mem.mmap_base;
    child.image_end = mem.image_end;
    crate::system::devsuite::reset_process(child.pid);
    child.cwd = parent.cwd;
    child.cwd_len = parent.cwd_len;
    child.uid = parent.uid;
    child.gid = parent.gid;
    child.euid = parent.euid;
    child.egid = parent.egid;
    child.pgid = parent.pgid;
    child.sid = parent.sid;
    child.umask = parent.umask;
    child.sig_actions = parent.sig_actions;
    child.sig_flags = parent.sig_flags;
    child.sig_masks = parent.sig_masks;
    child.sig_src = parent.sig_src;
    child.sig_rt_n = 0; // las senales pendientes no se heredan (POSIX fork)
    child.sig_blocked = parent.sig_blocked;
    child.sig_suspend_activo = false;
    child.sig_suspend_prev = 0;
    child.sig_pending = 0;
    child.niceness = parent.niceness;
    child.rlimit_nofile = parent.rlimit_nofile;
    child.rlimit_nofile_max = parent.rlimit_nofile_max;
    child.utime = 0;
    child.stime = 0;
    child.cutime = 0;
    child.cstime = 0;
    child.groups = parent.groups;
    child.ngroups = parent.ngroups;
    child.suid = parent.suid;
    child.sgid = parent.sgid;
    child.alarm_ticks = 0; // POSIX alarm timer is cleared across fork
    child.robust_list_head = 0;
    child.robust_list_len = 0;
    child.tgid = child_pid;     // [T-005] fork desde un hilo crea un proceso de un solo hilo
    child.es_hilo = false;
    child.clear_child_tid = 0;
    child.lider_salio = false;
    child.en_cpu = crate::process::types::SIN_CPU;
    child.affinity_mask = parent.affinity_mask;
    child.last_cpu = parent.last_cpu;
    child.fs_base = parent.fs_base;
    child.gs_base = parent.gs_base;
    child.ptrace_traced = false;
    child.ptrace_tracer = 0;
    child.ptrace_singlestep = false;
    child.ptrace_engaged = false;
    child.stop_signal = 0;
    child.stop_reported = false;
    child.stop_addr = 0;
    child.stop_dr6 = 0;
    child.watch_count = 0;
    child.bp_cond_count = 0;
    child.bp_cond_rearm = 0xFF;
    child.bp_cond_ignore = [0u32; 4];
    child.watch_addr = [0u64; 4];
    child.watch_len = [0u8; 4];
    child.watch_rw = [0u8; 4];
    child.regs = parent.regs;
    child.regs.rax = 0;

    for fd_opt in &child.fds {
        if let Some(fd) = fd_opt {
            match fd.fd_type {
                FdType::PipeRead(id) => {
                    let mut ptab = PIPE_TABLE.lock();
                    if let Some(p) = ptab.pipes.get_mut(id as usize) { p.readers += 1; }
                }
                FdType::PipeWrite(id) => {
                    let mut ptab = PIPE_TABLE.lock();
                    if let Some(p) = ptab.pipes.get_mut(id as usize) { p.writers += 1; }
                }
                FdType::Socket(sock_id) => {
                    crate::net::socket::socket_retain(sock_id);
                }
                FdType::SocketPairEnd { read_pipe, write_pipe } => {
                    let mut ptab = PIPE_TABLE.lock();
                    if let Some(p) = ptab.pipes.get_mut(read_pipe as usize) { p.readers += 1; }
                    if let Some(p) = ptab.pipes.get_mut(write_pipe as usize) { p.writers += 1; }
                }
                FdType::EventFd(id) => {
                    eventfd_retain(id);
                }
                FdType::Epoll(id) => {
                    epoll_retain(id);
                }
                FdType::TimerFd(id) => {
                    timerfd_retain(id);
                }
                FdType::SignalFd(id) => {
                    signalfd_retain(id);
                }
                FdType::PtyMaster(id) | FdType::PtySlave(id) => {
                    pty_retain(id);
                }
                FdType::Mq(id) => {
                    mq_retain(id);
                }
                FdType::Fanotify(id) => {
                    fanotify_retain(id);
                }
                FdType::Inotify(id) => {
                    inotify_retain(id);
                }
                _ => {}
            }
        }
    }

    serial::write_str("[FORK] parent PID=");
    serial::write_usize(parent.pid as usize);
    serial::write_str(" created child PID=");
    serial::write_usize(child_pid as usize);
    serial::write_str("\n");

    Some(child_pid)
}

pub fn set_current(pid: u64) -> Option<()> {
    let guard = PROCESS_TABLE.lock();
    let target = {
        let mut t: *mut Process = core::ptr::null_mut();
        for p in &guard.processes {
            unsafe {
                let pp = p.as_ptr() as *mut Process;
                if (*pp).pid == pid && (*pp).state != ProcessState::Dead {
                    t = pp;
                    break;
                }
            }
        }
        t
    };
    if target.is_null() {
        drop(guard);
        return None;
    }
    // [E3d] Sólo se degrada el proceso que ESTA CPU tenía como actual: antes se
    // ponían en Ready TODOS los Running, incluido el que otro núcleo ejecuta.
    let mio = crate::arch::percpu::pid_actual();
    for p in &guard.processes {
        unsafe {
            let pp = p.as_ptr() as *mut Process;
            if (*pp).state == ProcessState::Running && (*pp).pid == mio {
                (*pp).state = ProcessState::Ready;
                (*pp).en_cpu = crate::process::types::SIN_CPU;
            }
        }
    }
    unsafe {
        (*target).state = ProcessState::Running;
        (*target).en_cpu = crate::arch::percpu::indice_actual() as u8;
        set_tss_rsp0((*target).kernel_stack_top as u64);
    }
    crate::arch::percpu::fijar_pid_actual(pid);
    drop(guard);
    serial::write_str("PROC: set_current PID=");
    serial::write_usize(pid as usize);
    serial::write_str("\n");
    Some(())
}

/// [T-005 / T-025 E3d] Kernel stack del último contexto muerto EN ESTA CPU,
/// pendiente de liberar. Es por CPU: la CPU que sale de un proceso sigue
/// ejecutando sobre su kernel stack (trampolín de salida, bucle de espera,
/// `ap_ir_a_ocioso`) hasta conmutar; sólo ella sabe cuándo la ha abandonado
/// (la próxima vez que pase por aquí ya está sobre otra pila). Con una ranura
/// global, un hilo que moría en el AP veía su pila liberada por el siguiente
/// exit en el BSP mientras aún la usaba (montículo corrupto tras `run java`).
const PILA_HUERFANA_INIT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static PILA_HUERFANA: [core::sync::atomic::AtomicUsize; crate::arch::percpu::MAX_CPUS] =
    [PILA_HUERFANA_INIT; crate::arch::percpu::MAX_CPUS];

fn ranura_pila_huerfana() -> &'static core::sync::atomic::AtomicUsize {
    &PILA_HUERFANA[crate::arch::percpu::indice_actual().min(crate::arch::percpu::MAX_CPUS - 1)]
}

fn liberar_pila_huerfana_pendiente() {
    let ks = ranura_pila_huerfana().swap(0, core::sync::atomic::Ordering::SeqCst);
    if ks != 0 {
        // [T-018, defecto 25] NUNCA la pila sobre la que se está ejecutando: el
        // trampolín de salida la deja en la ranura (`diferir_pila_propia`) y
        // acto seguido `process_exit`, aún sobre ella, llamaba aquí y la
        // devolvía al montículo. Si otra CPU creaba un hilo en ese instante
        // (`alloc_zeroed` de 16 KiB = este mismo bloque) copiaba su frame
        // encima y la CPU saliente retornaba a basura (#UD RIP=3 en `exit`
        // de un hilo mientras main creaba el siguiente). Se queda en la
        // ranura y la libera la siguiente salida en esta CPU.
        let rsp: usize;
        unsafe { core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack)); }
        if rsp >= ks && rsp < ks + KERNEL_STACK_SIZE {
            ranura_pila_huerfana().store(ks, core::sync::atomic::Ordering::SeqCst);
            return;
        }
        if let Ok(layout) = core::alloc::Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE) {
            unsafe { alloc::alloc::dealloc(ks as *mut u8, layout); }
        }
    }
}

/// [T-018, defecto 25] Deja `ks` en la ranura diferida de esta CPU. Si la
/// ranura retiene la pila sobre la que se ejecuta ahora mismo (ver arriba),
/// `ks` no puede ser esa misma pila: se libera directamente.
fn diferir_pila(ks: usize) {
    if ks == 0 { return; }
    liberar_pila_huerfana_pendiente();
    if ranura_pila_huerfana().load(core::sync::atomic::Ordering::SeqCst) != 0 {
        let rsp: usize;
        unsafe { core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack)); }
        if !(rsp >= ks && rsp < ks + KERNEL_STACK_SIZE) {
            if let Ok(layout) = core::alloc::Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE) {
                unsafe { alloc::alloc::dealloc(ks as *mut u8, layout); }
            }
        }
        return;
    }
    ranura_pila_huerfana().store(ks, core::sync::atomic::Ordering::SeqCst);
}

/// [T-025 E3d] La CPU que va a abandonar el contexto del proceso `pid` (que
/// muere) se queda con la propiedad de su kernel stack: la retira del
/// `Process` (para que `process_exit`, lo ejecute quien lo ejecute —esta CPU o
/// el padre que recoge al zombi desde otra—, no la libere) y la deja en la
/// ranura diferida de esta CPU. Debe llamarse ANTES de marcar Zombie.
pub fn diferir_pila_propia(pid: u64) {
    let ks = {
        let guard = PROCESS_TABLE.lock();
        let mut ks = 0usize;
        for p in &guard.processes {
            let pp = p.as_ptr() as *mut Process;
            unsafe {
                if (*pp).pid == pid && (*pp).state != ProcessState::Dead {
                    ks = (*pp).kernel_stack_base;
                    (*pp).kernel_stack_base = 0;
                    break;
                }
            }
        }
        ks
    };
    diferir_pila(ks);
}

/// [T-025 E3d] Índice de OTRA CPU que esté ejecutando ahora mismo el proceso
/// `pid` (su `pid_actual`), o `None`.
pub fn cpu_que_ejecuta(pid: u64) -> Option<usize> {
    if pid == 0 { return None; }
    let yo = crate::arch::percpu::indice_actual();
    let n = crate::arch::smp::nucleos_en_linea().min(crate::arch::percpu::MAX_CPUS);
    for i in 0..n {
        if i == yo { continue; }
        if crate::arch::percpu::por_indice(i).pid_actual.load(core::sync::atomic::Ordering::SeqCst) == pid {
            return Some(i);
        }
    }
    None
}

/// [T-025 E3d] Termina el hilo `t` del grupo que muere. Si `t` está ejecutándose
/// en otra CPU no se puede desmontar desde aquí (su kernel stack y su contexto
/// están en uso allí): se le deja SIGKILL pendiente y se le avisa con una IPI;
/// su propia CPU lo matará en el siguiente tick/retorno a Ring 3 por el camino
/// normal de señales. Si no corre en ninguna CPU, su contexto está guardado y
/// se desmonta aquí mismo. Devuelve `true` si se desmontó ya.
pub fn terminar_hilo_del_grupo(t: u64, code: i32) -> bool {
    crate::syscall::futex::retirar(t);
    // Un hilo que ya salió por sí mismo (Zombie) sólo está recorriendo el
    // trampolín de salida en su CPU: su pila ya es de esa CPU (diferida) y no
    // posee tablas; se recoge aquí sin más.
    let zombi = with_process(t, |p| p.state == ProcessState::Zombie).unwrap_or(false);
    if zombi { process_exit(t, code); return true; }
    if let Some(cpu) = cpu_que_ejecuta(t) {
        {
            let guard = PROCESS_TABLE.lock();
            for p in &guard.processes {
                let pp = p.as_ptr() as *mut Process;
                unsafe {
                    if (*pp).pid == t && (*pp).state != ProcessState::Dead {
                        (*pp).sig_pending |= 1u64 << (crate::process::signals::SIGKILL as u64);
                        (*pp).exit_code = code;
                        // [T-005b, defecto 32] El grupo YA lo está cerrando quien llama
                        // (exit_group o señal fatal en otra CPU): este hilo muere SOLO.
                        // Sin esto, su SIGKILL entraba en `terminate_current` →
                        // `matar_grupo_por_senal` → segundo `cerrar_lider_dormido` del
                        // mismo líder en paralelo con el primero: las dos CPUs quedaban
                        // "dentro" de exit_group para siempre (rustc al terminar, tras
                        // enlazar con éxito: hilo principal en la CPU 0, hilo ctrl-c en la 1).
                        (*pp).salir_solo = true;
                        break;
                    }
                }
            }
        }
        let apic = crate::arch::smp::apic_id_de(cpu);
        if apic != u32::MAX { let _ = crate::arch::smp::send_ipi(apic as u8, crate::arch::smp::IPI_VECTOR_RESCHEDULE); }
        serial::write_str("[T-005] hilo tid=");
        serial::write_usize(t as usize);
        serial::write_str(" corre en la cpu ");
        serial::write_usize(cpu);
        serial::write_str(": SIGKILL pendiente, lo termina su CPU\n");
        false
    } else {
        process_exit(t, code);
        true
    }
}

pub fn process_exit(pid: u64, code: i32) {
    // [CIERRE B-3] El perfil del profiler no debe sobrevivir al proceso
    // (antes `devsuite::forget_process` nunca se invocaba y los PIDs
    // reciclados heredaban muestras ajenas).
    crate::system::devsuite::forget_process(pid);
    let guard = PROCESS_TABLE.lock();
    let mut huerfanos_parados = [0u64; 8];
    let (es_hilo, cr3, ks_base, us_phys, end_break, mmap_end, slot, image_end_fin) = {
        let mut ptr: *mut Process = core::ptr::null_mut();
        let mut sl = None;
        for (i, p) in guard.processes.iter().enumerate() {
            unsafe {
                let pp = p.as_ptr() as *mut Process;
                if (*pp).pid == pid && (*pp).state != ProcessState::Dead {
                    ptr = pp;
                    sl = Some(i);
                    break;
                }
            }
        }
        if ptr.is_null() {
            drop(guard);
            serial::write_str("PROC: exit unknown PID=");
            serial::write_usize(pid as usize);
            serial::write_str("\n");
            return;
        }
        unsafe {
            serial::write_str("PROC: exit PID=");
            serial::write_usize(pid as usize);
            serial::write_str(" name='");
            serial::write_str((*ptr).name_str());
            serial::write_str("' code=");
            serial::write_usize(code as usize);
            {   // [T-003] balance de marcos físicos al salir (detecta fugas)
                let (tot, libres, pico, respaldo) = crate::mem::marcos::estadisticas();
                serial::write_str(" marcos_libres="); serial::write_usize(libres);
                serial::write_str("/"); serial::write_usize(tot);
                serial::write_str(" pico="); serial::write_usize(pico);
                serial::write_str(" respaldo_heap="); serial::write_usize(respaldo as usize);
                // [T-041] Liberaciones rechazadas por no ser RAM del firmware.
                serial::write_str(" liberados_ignorados=");
                serial::write_usize(crate::mem::marcos::LIBERADOS_IGNORADOS.load(core::sync::atomic::Ordering::Relaxed) as usize);
            }
            if (*ptr).senales_excepcion > 0 { // [T-020]
                serial::write_str(" senales_por_excepcion="); serial::write_usize((*ptr).senales_excepcion as usize);
            }
            serial::write_str("\n");

            let cr3_val = (*ptr).cr3;
            let ks_val = (*ptr).kernel_stack_base;
            let us_val = (*ptr).user_stack_phys;
            let end_break = (*ptr).program_break_end.max((*ptr).program_break);
            let mmap_end = (*ptr).mmap_base;
            let image_end_val = (*ptr).image_end; // [Fase B.4]

            let ppid = (*ptr).ppid;
            let u_ticks = (*ptr).utime + (*ptr).cutime;
            let s_ticks = (*ptr).stime + (*ptr).cstime;
            if ppid != 0 {
                for p_parent in &guard.processes {
                    let parent_p = p_parent.as_ptr() as *mut Process;
                    if (*parent_p).pid == ppid && (*parent_p).state != ProcessState::Dead {
                        (*parent_p).cutime += u_ticks;
                        (*parent_p).cstime += s_ticks;
                        // Épica 58: notificación SIGCHLD al progenitor (POSIX)
                        (*parent_p).sig_pending |= 1u64 << 17;
                        signals::note_sigchld();
                        break;
                    }
                }
            }

            // [CONTROL DE TRABAJOS] Hijos DETENIDOS que quedan huérfanos (su grupo
            // pierde al único proceso que podía reanudarlos): POSIX manda SIGHUP y
            // SIGCONT a los grupos huérfanos con miembros parados para que no se
            // queden congelados para siempre.
            for p_h in &guard.processes {
                let h = p_h.as_ptr() as *mut Process;
                if (*h).ppid == pid && (*h).pid != 0 && (*h).state == ProcessState::Stopped {
                    for slot in huerfanos_parados.iter_mut() {
                        if *slot == 0 { *slot = (*h).pid; break; }
                    }
                }
            }

            (*ptr).state = ProcessState::Dead;
            (*ptr).pid = 0;
            (*ptr).pgid = 0;
            (*ptr).sid = 0;
            (*ptr).umask = 0o022;
            (*ptr).sig_actions = [0usize; 65];
            (*ptr).sig_flags = [0u32; 65];
            (*ptr).sig_masks = [0u64; 65];
            (*ptr).sig_src = [types::SigOrigen::default(); 65];
            (*ptr).sig_rt_n = 0;
            (*ptr).sig_blocked = 0;
            (*ptr).sig_suspend_activo = false;
            (*ptr).sig_pending = 0;
            (*ptr).niceness = 0;
            (*ptr).rlimit_nofile = MAX_FDS as u64;
            (*ptr).rlimit_nofile_max = MAX_FDS as u64;
            (*ptr).cr3 = 0;
            (*ptr).kernel_stack_base = 0;
            (*ptr).kernel_stack_top = 0;
            (*ptr).user_stack_phys = 0;
            (*ptr).es_hilo = false;
            (*ptr).tgid = 0;
            (*ptr).clear_child_tid = 0;
            (*ptr).lider_salio = false;

            ((*ptr).es_hilo, cr3_val, ks_val, us_val, end_break, mmap_end, sl, image_end_val)
        }
    };
    drop(guard);

    for &h in huerfanos_parados.iter() {
        if h != 0 {
            crate::drivers::serial::write_str("[JOB] proceso detenido huerfano PID ");
            crate::drivers::serial::write_usize(h as usize);
            crate::drivers::serial::write_str(" -> SIGHUP + SIGCONT\n");
            signals::raise_for_pid(h, signals::SIGHUP);
            signals::raise_for_pid(h, signals::SIGCONT);
        }
    }

    if let Some(s) = slot {
        crate::ipc::cleanup_process(s);
    }

    if es_hilo {
        // [T-005] Un hilo no posee el espacio de direcciones ni la pila de
        // usuario: sólo se libera su kernel stack, y se hace en DIFERIDO porque
        // `exit` de un hilo llega aquí ejecutando sobre esa misma pila (la
        // libera el siguiente hilo/proceso que muera).
        liberar_pila_huerfana_pendiente();
        diferir_pila(ks_base);
        return;
    }
    liberar_pila_huerfana_pendiente();

    if ks_base != 0 {
        let layout = match core::alloc::Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE) {
            Ok(l) => l,
            Err(_) => {
                serial::write_str("[PROCESS] ERROR: invalid kernel stack layout during cleanup\n");
                return;
            }
        };
        unsafe { alloc::alloc::dealloc(ks_base as *mut u8, layout); }
    }

    if us_phys != 0 {
        // [Fase B.4] la pila vuelve a su dueño real (asignador de marcos o montículo)
        paging::liberar_marcos_usuario_contiguos(us_phys, USER_STACK_SIZE / PAGE_SIZE);
    }

    // [FRENTE C] Si el espacio de direcciones que muere es el que está cargado
    // en CR3 (proceso sin padre que reaprovechar, p. ej. lanzado con `run`),
    // antes se saltaba la liberación entera: tablas, montículo e imagen
    // quedaban huérfanos para siempre. Ahora se conmuta al espacio maestro del
    // núcleo (que mapea todo lo que el kernel necesita, incluida esta pila) y
    // se libera igual que cuando lo reaprovecha el padre.
    let mut cr3_liberable = cr3 != 0;
    if cr3_liberable && cr3 == paging::read_cr3() {
        let maestro = paging::KERNEL_CR3.load(core::sync::atomic::Ordering::SeqCst);
        if maestro != 0 && maestro != cr3 {
            paging::write_cr3(maestro);
        } else {
            cr3_liberable = false;
        }
    }
    // [T-025 E3d] Nunca liberar unas tablas que otra CPU tiene cargadas: al
    // `exit_group` de la JVM, un hilo con SIGKILL pendiente seguía en el AP
    // sobre este CR3 (lo termina su propia CPU); el BSP liberaba las tablas,
    // se reutilizaban, y el AP fallaba en el núcleo al tocar la LAPIC
    // (#PF CR2=0xFEE000B0 en `ipi_handler_rust`, 17000 veces en cascada).
    // Se espera, con el cerrojo grande suelto e IF=1 (la otra CPU necesita
    // ambos para salir), hasta 2 s; si no se va, las tablas se dejan huérfanas
    // (fuga acotada y trazada) antes que corromper a la otra CPU.
    if cr3_liberable && crate::arch::percpu::cr3_en_otra_cpu(cr3) {
        let d_cgn = crate::arch::bkl::soltar_todo();
        let rf: u64; unsafe { core::arch::asm!("pushfq; pop {}", out(reg) rf, options(nomem)); }
        let t0 = crate::time::pit::ticks();
        unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
        while crate::arch::percpu::cr3_en_otra_cpu(cr3) && crate::time::pit::ticks().wrapping_sub(t0) < 200 {
            core::hint::spin_loop();
        }
        if rf & (1 << 9) == 0 { unsafe { core::arch::asm!("cli", options(nomem, nostack)); } }
        crate::arch::bkl::retomar(d_cgn);
        if crate::arch::percpu::cr3_en_otra_cpu(cr3) {
            serial::write_str("[E3d] exit PID=");
            serial::write_usize(pid as usize);
            serial::write_str(": otra CPU sigue sobre su CR3 tras 2 s; tablas huerfanas (no se liberan)\n");
            cr3_liberable = false;
        }
    }
    if cr3_liberable {
        if end_break > PROGRAM_BREAK_BASE {
            paging::free_user_heap_pages(cr3, PROGRAM_BREAK_BASE, end_break);
        }
        if mmap_end > MMAP_BASE {
            paging::free_user_heap_pages(cr3, MMAP_BASE, mmap_end);
        }
        { let (lo, hi) = ventana_baja_libre(image_end_fin); paging::free_user_pages_dispersos(cr3, lo, hi); } // [Fase B.4]
        { let (lo, hi) = pila_extendida(); paging::free_user_pages_dispersos(cr3, lo, hi); } // [T-004]
        paging::free_address_space(cr3);
    }
}

// ── FD helpers ───────────────────────────────────────────────────────────

pub fn fd_clone_entry(entry: &FdEntry) -> FdEntry {
    match entry.fd_type {
        FdType::PipeRead(id) => {
            let mut ptab = PIPE_TABLE.lock();
            if let Some(p) = ptab.pipes.get_mut(id as usize) {
                p.readers += 1;
            }
        }
        FdType::PipeWrite(id) => {
            let mut ptab = PIPE_TABLE.lock();
            if let Some(p) = ptab.pipes.get_mut(id as usize) {
                p.writers += 1;
            }
        }
        FdType::Socket(sock_id) => {
            crate::net::socket::socket_retain(sock_id);
        }
        FdType::SocketPairEnd { read_pipe, write_pipe } => {
            let mut ptab = PIPE_TABLE.lock();
            if let Some(p) = ptab.pipes.get_mut(read_pipe as usize) {
                p.readers += 1;
            }
            if let Some(p) = ptab.pipes.get_mut(write_pipe as usize) {
                p.writers += 1;
            }
        }
        FdType::EventFd(id) => {
            eventfd_retain(id);
        }
        FdType::Epoll(id) => {
            epoll_retain(id);
        }
        FdType::TimerFd(id) => {
            timerfd_retain(id);
        }
        FdType::SignalFd(id) => {
            signalfd_retain(id);
        }
        FdType::PtyMaster(id) | FdType::PtySlave(id) => {
            pty_retain(id);
        }
        FdType::Mq(id) => {
            mq_retain(id);
        }
        FdType::Fanotify(id) => {
            fanotify_retain(id);
        }
        FdType::Inotify(id) => {
            inotify_retain(id);
        }
        _ => {}
    }
    *entry
}

/// [T-005] Ranura del líder del grupo de hilos de `p` (el propio `p` si no
/// es un hilo o si el líder ya no existe).
pub fn lider_raw(p: *mut Process) -> *mut Process {
    unsafe {
        if p.is_null() || !(*p).es_hilo { return p; }
        let tgid = (*p).tgid;
        let Some(guard) = PROCESS_TABLE.try_lock() else {
            // Sin el cerrojo (reentrada desde una IRQ) se recorre igual: la
            // tabla es estática y sólo se lee `pid`/`state`.
            return buscar_lider_sin_cerrojo(p, tgid);
        };
        for q in &guard.processes {
            let qp = q.as_ptr() as *mut Process;
            if (*qp).pid == tgid && (*qp).state != ProcessState::Dead && !(*qp).es_hilo {
                return qp;
            }
        }
        p
    }
}

unsafe fn buscar_lider_sin_cerrojo(p: *mut Process, tgid: u64) -> *mut Process {
    let tabla = &*PROCESS_TABLE.as_mut_ptr();
    for q in &tabla.processes {
        let qp = q.as_ptr() as *mut Process;
        if (*qp).pid == tgid && (*qp).state != ProcessState::Dead && !(*qp).es_hilo {
            return qp;
        }
    }
    p
}

/// [T-005] Proceso que posee el espacio de direcciones del hilo en curso
/// (brk, mmap, imagen): el líder si el actual es un hilo.
pub fn proceso_memoria() -> Option<&'static mut Process> {
    let cur = unsafe { current_raw() };
    if cur.is_null() { return None; }
    Some(unsafe { &mut *lider_raw(cur) })
}

/// [T-005] Tabla de descriptores efectiva: los hilos comparten la del líder
/// (`CLONE_FILES`), así un `open` en un hilo es visible en todos y un `close`
/// cierra para todos, como en Linux.
pub fn tabla_fds(proc: &Process) -> &'static mut [Option<FdEntry>; MAX_FDS] {
    let p = proc as *const Process as *mut Process;
    unsafe { &mut (*lider_raw(p)).fds }
}

/// [T-005] `true` si `pid` es el líder de un grupo con otros hilos vivos.
pub fn tiene_hilos_vivos(tgid: u64) -> bool {
    hilos_del_grupo(tgid, 0) > 0
}

/// [T-005] Número de hilos secundarios vivos del grupo `tgid`, sin contar
/// `excepto`.
pub fn hilos_del_grupo(tgid: u64, excepto: u64) -> usize {
    // [E3d] cerrojo real: con `try_lock`, la contención de otra CPU devolvía "0 hilos"
    // y el líder cerraba el proceso con hilos vivos (T-005 pthread_exit en main).
    let guard = PROCESS_TABLE.lock();
    let mut n = 0;
    for q in &guard.processes {
        let qp = q.as_ptr() as *const Process;
        unsafe {
            if (*qp).es_hilo && (*qp).tgid == tgid && (*qp).pid != excepto
                && (*qp).pid != 0 && (*qp).state != ProcessState::Dead { n += 1; }
        }
    }
    n
}

/// [T-005] PIDs (tids) de los hilos secundarios vivos del grupo, hasta 64.
pub fn tids_del_grupo(tgid: u64, excepto: u64, salida: &mut [u64; 64]) -> usize {
    // [E3d] cerrojo real: con `try_lock`, la contención de otra CPU devolvía "0 hilos"
    // y el líder cerraba el proceso con hilos vivos (T-005 pthread_exit en main).
    let guard = PROCESS_TABLE.lock();
    let mut n = 0;
    for q in &guard.processes {
        let qp = q.as_ptr() as *const Process;
        unsafe {
            if (*qp).es_hilo && (*qp).tgid == tgid && (*qp).pid != excepto
                && (*qp).pid != 0 && (*qp).state != ProcessState::Dead {
                if n < 64 { salida[n] = (*qp).pid; n += 1; }
            }
        }
    }
    n
}

/// [T-005] Crea un hilo del proceso actual: misma CR3 (sin copiar nada),
/// misma tabla de descriptores, pila de usuario `child_stack` provista por
/// la libc, TLS propio (`FS_BASE = tls`), kernel stack propio. Devuelve el
/// tid. El hilo arranca en la instrucción siguiente al `syscall` con RAX=0.
pub fn process_clone_thread(parent_rsp: u64, child_stack: usize, tls: u64) -> Option<u64> {
    let parent_ptr = unsafe { current_raw() };
    if parent_ptr.is_null() || child_stack == 0 { return None; }
    let parent = unsafe { &*parent_ptr };
    let lider = unsafe { &*lider_raw(parent_ptr) };

    let (slot, tid) = alloc_slot_and_pid()?;

    let ks_layout = Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE).ok()?;
    let ks_ptr = unsafe { alloc::alloc::alloc_zeroed(ks_layout) };
    if ks_ptr.is_null() { return None; }
    let ks_base = ks_ptr as usize;
    let ks_top = ks_base + KERNEL_STACK_SIZE;

    let child_kernel_rsp = unsafe {
        let frame_size = if parent.kernel_stack_top > (parent_rsp as usize)
            && parent.kernel_stack_top - (parent_rsp as usize) <= KERNEL_STACK_SIZE
            && parent.kernel_stack_top - (parent_rsp as usize) >= 416
        {
            parent.kernel_stack_top - (parent_rsp as usize)
        } else {
            416
        };
        let child_rsp_ptr = (ks_top - frame_size) as *mut u8;
        core::ptr::copy_nonoverlapping(parent_rsp as *const u8, child_rsp_ptr, frame_size);
        *((child_rsp_ptr as usize + 256) as *mut u64) = 0;                 // RAX = 0 en el hijo
        *((child_rsp_ptr as usize + 400) as *mut u64) = child_stack as u64; // RSP = pila del hilo
        child_rsp_ptr as u64
    };

    let mut guard = PROCESS_TABLE.lock();
    let child = unsafe { &mut *guard.processes[slot].as_mut_ptr() };

    child.pid = tid;
    child.ppid = 0;                 // no es hijo de nadie: no se espera con waitpid ni manda SIGCHLD
    child.tgid = lider.pid;
    child.es_hilo = true;
    child.clear_child_tid = 0;
    child.lider_salio = false;
    child.en_cpu = crate::process::types::SIN_CPU;
    child.state = ProcessState::Ready;
    child.name = parent.name;
    child.name_len = parent.name_len;
    child.exe_path = parent.exe_path;
    child.exe_len = parent.exe_len;
    child.cr3 = parent.cr3;         // MISMO espacio de direcciones
    child.kernel_stack_base = ks_base;
    child.kernel_stack_top = ks_top;
    child.kernel_rsp = child_kernel_rsp;
    child.user_stack_phys = 0;      // la pila la puso la libc con mmap: no es del núcleo
    child.user_rsp = child_stack as u64;
    child.user_rip = parent.user_rip;
    child.exit_code = 0;
    child.term_signal = 0;
    child.ticks_used = 0;
    child.sleep_until = 0;
    child.ring3_ret_rsp = 0;
    child.ring3_ret_addr = 0;
    const NONE: Option<FdEntry> = None;
    child.fds = [NONE; MAX_FDS];    // nunca se usa: `tabla_fds` redirige al líder
    child.program_break = lider.program_break;
    child.program_break_end = lider.program_break_end;
    child.mmap_base = lider.mmap_base;
    child.image_end = lider.image_end;
    crate::system::devsuite::reset_process(child.pid);
    child.cwd = lider.cwd;
    child.cwd_len = lider.cwd_len;
    child.uid = parent.uid;
    child.gid = parent.gid;
    child.euid = parent.euid;
    child.egid = parent.egid;
    child.pgid = parent.pgid;
    child.sid = parent.sid;
    child.umask = parent.umask;
    child.sig_actions = parent.sig_actions;   // CLONE_SIGHAND: misma disposición
    child.sig_flags = parent.sig_flags;
    child.sig_masks = parent.sig_masks;
    child.sig_src = parent.sig_src;
    child.sig_rt_n = 0;
    child.sig_blocked = parent.sig_blocked;
    child.sig_suspend_activo = false;
    child.sig_suspend_prev = 0;
    child.sig_pending = 0;
    child.niceness = parent.niceness;
    child.rlimit_nofile = parent.rlimit_nofile;
    child.rlimit_nofile_max = parent.rlimit_nofile_max;
    child.utime = 0;
    child.stime = 0;
    child.cutime = 0;
    child.cstime = 0;
    child.groups = parent.groups;
    child.ngroups = parent.ngroups;
    child.suid = parent.suid;
    child.sgid = parent.sgid;
    child.alarm_ticks = 0;
    child.robust_list_head = 0;
    child.robust_list_len = 0;
    child.affinity_mask = parent.affinity_mask;
    child.last_cpu = parent.last_cpu;
    child.fs_base = tls;
    child.gs_base = parent.gs_base;
    child.ptrace_traced = false;
    child.ptrace_tracer = 0;
    child.ptrace_singlestep = false;
    child.ptrace_engaged = false;
    child.stop_signal = 0;
    child.stop_reported = false;
    child.stop_addr = 0;
    child.stop_dr6 = 0;
    child.watch_count = 0;
    child.bp_cond_count = 0;
    child.bp_cond_rearm = 0xFF;
    child.bp_cond_ignore = [0u32; 4];
    child.watch_addr = [0u64; 4];
    child.watch_len = [0u8; 4];
    child.watch_rw = [0u8; 4];
    child.regs = parent.regs;
    child.regs.rax = 0;
    drop(guard);

    serial::write_str("[T-005] hilo tid=");
    serial::write_usize(tid as usize);
    serial::write_str(" del proceso ");
    serial::write_usize(lider.pid as usize);
    serial::write_str(" pila=0x");
    serial::write_hex(child_stack);
    serial::write_str(" tls=0x");
    serial::write_hex(tls as usize);
    serial::write_str("\n");
    Some(tid)
}

/// [T-005] Cierra el proceso cuyo líder es `l` desde el contexto de otro hilo
/// (el último en morir, o el que recibió una señal fatal): cierra sus
/// descriptores, lo deja zombi y avisa al padre con SIGCHLD; si no tiene
/// padre lo libera ya (conmutando antes al espacio maestro del núcleo, porque
/// el hilo llamador aún corre sobre ese CR3).
pub fn cerrar_lider_dormido(l: &mut Process, code: i32, term_signal: u8) {
    if l.ppid == 0 && !crate::arch::smp::is_bsp() {
        // [T-025 E3d] Proceso lanzado por la consola (`run`) y el hilo que
        // cierra el grupo corre en un AP: la continuación de consola
        // (`ring3_ret_addr`, pila `__stack_top`) sólo existe en el BSP; heredarla
        // aquí hacía que el AP saltara al llamador del BSP con RSP sobre
        // `__stack_top` (#UD RIP=3 tras `exit_group` de la JVM). Se deja al líder
        // con el código, SIGKILL pendiente y `Ready`: sólo el BSP puede elegirlo
        // (tiene continuación de consola) y lo termina él, volviendo a la consola.
        crate::syscall::futex::retirar(l.pid);
        l.lider_salio = true;
        l.exit_code = code;
        l.term_signal = term_signal;
        // El SIGKILL sólo despierta al líder para completar un cierre ya
        // decidido; no debe iniciar otro cierre ni reemplazar el estado.
        l.salir_solo = true;
        l.sig_pending |= 1u64 << (signals::SIGKILL as u64);
        if l.state == ProcessState::Blocked { l.state = ProcessState::Ready; l.sleep_until = 0; }
        serial::write_str("[T-005] lider de consola cerrado desde un AP: lo termina el BSP (PID ");
        serial::write_usize(l.pid as usize);
        serial::write_str(")\n");
        return;
    }
    for i in 0..MAX_FDS { fd_close(l, i); }
    l.exit_code = code;
    l.term_signal = term_signal;
    l.lider_salio = false;
    if l.ppid != 0 {
        l.state = ProcessState::Zombie;
        if let Some(pp) = process_by_pid(l.ppid) {
            pp.sig_pending |= 1u64 << 17;
            signals::note_sigchld();
            if pp.state == ProcessState::Blocked { pp.state = ProcessState::Ready; }
        }
    } else {
        // Proceso lanzado desde la consola del núcleo (`run`): la vuelta al
        // llamador la guarda el líder; el hilo que cierra el grupo la hereda
        // para que `process_exit_trampoline` devuelva el control a la consola.
        unsafe {
            let cur = current_raw();
            if !cur.is_null() && (*cur).es_hilo && (*cur).ring3_ret_addr == 0 {
                (*cur).ring3_ret_addr = l.ring3_ret_addr;
                (*cur).ring3_ret_rsp = l.ring3_ret_rsp;
            }
        }
        let pid = l.pid;
        let maestro = paging::KERNEL_CR3.load(core::sync::atomic::Ordering::SeqCst);
        if maestro != 0 { paging::write_cr3(maestro); }
        process_exit(pid, code);
    }
}

/// [T-017] Espera, con el cerrojo grande suelto e IF=1, a que `cond` sea
/// cierta o pasen `max_ticks` del PIT. Devuelve `true` si se cumplió.
fn esperar_sin_cgn(max_ticks: u64, mut cond: impl FnMut() -> bool) -> bool {
    if cond() { return true; }
    let d_cgn = crate::arch::bkl::soltar_todo();
    let rf: u64; unsafe { core::arch::asm!("pushfq; pop {}", out(reg) rf, options(nomem)); }
    let t0 = crate::time::pit::ticks();
    unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
    let mut ok = false;
    while crate::time::pit::ticks().wrapping_sub(t0) < max_ticks {
        if cond() { ok = true; break; }
        core::hint::spin_loop();
    }
    if rf & (1 << 9) == 0 { unsafe { core::arch::asm!("cli", options(nomem, nostack)); } }
    crate::arch::bkl::retomar(d_cgn);
    ok || cond()
}

/// [T-017] `execve(2)` llamado desde un hilo secundario. Linux: el hilo que
/// llama pasa a ser el líder del grupo (conserva el PID del proceso) y todos
/// los demás hilos, el líder incluido, desaparecen. Aquí:
///  1. se terminan los demás hilos secundarios y se espera a que las CPUs que
///     los ejecutaban los suelten (tras esto nadie más usa el CR3 antiguo);
///  2. el llamador toma la identidad del líder: PID, parentesco, grupos,
///     credenciales, cwd, descriptores, pila de usuario, extensión del
///     montículo/mmap y la continuación de consola (`ring3_ret_addr`);
///  3. la ranura del líder antiguo pasa a ser un hilo con el tid viejo y se
///     termina como tal (sólo libera su pila de kernel).
/// Devuelve `Ok(true)` si el llamador era un hilo (ya convertido en líder),
/// `Ok(false)` si no lo era, y `Err(errno)` si no se pudo (el líder sigue
/// ejecutando en otra CPU tras 2 s: `EAGAIN`).
pub fn execve_desde_hilo() -> Result<bool, i64> {
    let cur = unsafe { current_raw() };
    if cur.is_null() || unsafe { !(*cur).es_hilo } { return Ok(false); }
    let (tid, tgid) = unsafe { ((*cur).pid, (*cur).tgid) };
    // El líder no puede estar en una CPU (en Ring 3 o en su bucle "espera al
    // último"): se le deja SIGKILL pendiente y se espera a que su CPU lo suelte.
    if crate::arch::percpu::pid_en_otra_cpu(tgid) {
        if let Some(l) = process_by_pid(tgid) { l.salir_solo = true; }
        terminar_hilo_del_grupo(tgid, 0);
        let fuera = esperar_sin_cgn(200, || !crate::arch::percpu::pid_en_otra_cpu(tgid));
        if !fuera {
            serial::write_str("[T-017] execve desde hilo: el lider sigue en otra CPU tras 2 s (EAGAIN)\n");
            return Err(-11);
        }
    }
    // 1. Los demás hilos secundarios.
    let mut tids = [0u64; 64];
    let n = tids_del_grupo(tgid, tid, &mut tids);
    for &t in tids.iter().take(n) {
        if let Some(h) = process_by_pid(t) { h.salir_solo = true; }
        terminar_hilo_del_grupo(t, 0);
    }
    let solos = esperar_sin_cgn(200, || hilos_del_grupo(tgid, tid) == 0);
    if !solos {
        serial::write_str("[T-017] execve desde hilo: quedan hilos vivos tras 2 s (EAGAIN)\n");
        return Err(-11);
    }
    // 2. Intercambio de identidad con el líder.
    crate::syscall::futex::retirar(tgid); // el líder dormía en un futex bajo su pid
    let l = lider_raw(cur);
    unsafe {
        if l != cur && !l.is_null() {
            let lp = &mut *l;
            let cp = &mut *cur;
            cp.ppid = lp.ppid; lp.ppid = 0;
            cp.pgid = lp.pgid; cp.sid = lp.sid;
            cp.uid = lp.uid; cp.gid = lp.gid; cp.euid = lp.euid; cp.egid = lp.egid;
            cp.suid = lp.suid; cp.sgid = lp.sgid; cp.umask = lp.umask;
            cp.groups = lp.groups; cp.ngroups = lp.ngroups;
            cp.cwd = lp.cwd; cp.cwd_len = lp.cwd_len;
            cp.rlimit_nofile = lp.rlimit_nofile; cp.rlimit_nofile_max = lp.rlimit_nofile_max;
            cp.niceness = lp.niceness;
            cp.fds = lp.fds;
            const NONE: Option<FdEntry> = None;
            lp.fds = [NONE; MAX_FDS];
            cp.user_stack_phys = lp.user_stack_phys; lp.user_stack_phys = 0;
            cp.program_break = lp.program_break; cp.program_break_end = lp.program_break_end;
            cp.mmap_base = lp.mmap_base; cp.image_end = lp.image_end;
            cp.ring3_ret_addr = lp.ring3_ret_addr; cp.ring3_ret_rsp = lp.ring3_ret_rsp;
            lp.ring3_ret_addr = 0; lp.ring3_ret_rsp = 0;
            cp.utime += lp.utime; cp.stime += lp.stime; cp.cutime = lp.cutime; cp.cstime = lp.cstime;
            cp.alarm_ticks = lp.alarm_ticks;
            cp.ptrace_traced = lp.ptrace_traced; cp.ptrace_tracer = lp.ptrace_tracer;
            cp.registered_ports = lp.registered_ports; cp.registered_port_count = lp.registered_port_count;
            // El líder antiguo pasa a ser un hilo cualquiera con el tid viejo.
            lp.pid = tid; lp.es_hilo = true; lp.tgid = tgid; lp.lider_salio = false;
            lp.clear_child_tid = 0; lp.sig_pending = 0;
            if lp.state == ProcessState::Running { lp.state = ProcessState::Ready; }
        }
        (*cur).pid = tgid;
        (*cur).tgid = tgid;
        (*cur).es_hilo = false;
        (*cur).lider_salio = false;
        (*cur).clear_child_tid = 0;
        crate::arch::percpu::fijar_pid_actual(tgid);
    }
    crate::syscall::futex::retirar(tid);
    // 3. La ranura del líder antiguo (ahora hilo `tid`) se termina.
    if l != cur && !l.is_null() {
        terminar_hilo_del_grupo(tid, 0);
        let _ = esperar_sin_cgn(200, || hilos_del_grupo(tgid, 0) == 0);
    }
    serial::write_str("[T-017] execve desde el hilo tid=");
    serial::write_usize(tid as usize);
    serial::write_str(": pasa a ser el lider pid=");
    serial::write_usize(tgid as usize);
    serial::write_str(" (");
    serial::write_usize(n);
    serial::write_str(" hilos terminados)\n");
    Ok(true)
}

/// [T-005] Una señal fatal (o una excepción) en cualquier hilo termina el
/// grupo entero, como en Linux. Mata a los demás hilos de `pid`; si `pid` es
/// un hilo secundario cierra además al líder con `code`/`term_signal`.
/// Devuelve cuántos hilos adicionales murieron.
pub fn matar_grupo_por_senal(pid: u64, code: i32, term_signal: u8) -> usize {
    let Some(p) = process_by_pid(pid) else { return 0 };
    let (es_hilo, tgid) = (p.es_hilo, p.tgid);
    let grupo = if es_hilo { tgid } else { pid };
    let mut tids = [0u64; 64];
    let n = tids_del_grupo(grupo, pid, &mut tids);
    for &t in tids.iter().take(n) {
        terminar_hilo_del_grupo(t, code); // [E3d] los que corren en otra CPU mueren allí
    }
    if es_hilo {
        if let Some(l) = process_by_pid(tgid) {
            cerrar_lider_dormido(l, code, term_signal);
        }
    }
    if n > 0 || es_hilo {
        serial::write_str("[T-005] senal fatal en el grupo ");
        serial::write_usize(grupo as usize);
        serial::write_str(": ");
        serial::write_usize(n);
        serial::write_str(" hilos mas terminados\n");
    }
    n
}

pub fn fd_alloc(proc: &mut Process, entry: FdEntry) -> Option<usize> {
    let limit = (proc.rlimit_nofile as usize).min(MAX_FDS);
    let fds = tabla_fds(proc);
    for i in 0..limit {
        if fds[i].is_none() {
            fds[i] = Some(entry);
            return Some(i);
        }
    }
    None
}

pub fn fd_alloc_at(proc: &mut Process, fd: usize, entry: FdEntry) -> Option<usize> {
    if fd >= MAX_FDS { return None; }
    tabla_fds(proc)[fd] = Some(entry);
    Some(fd)
}

pub fn fd_get(proc: &Process, fd: usize) -> Option<&FdEntry> {
    if fd >= MAX_FDS { return None; }
    tabla_fds(proc)[fd].as_ref()
}

pub fn clone_fd_from_pid(target_pid: u64, target_fd: usize) -> Option<FdEntry> {
    if target_fd >= MAX_FDS { return None; }
    let ptab = PROCESS_TABLE.lock();
    for p in &ptab.processes {
        unsafe {
            let proc = &*p.as_ptr();
            if proc.pid == target_pid && proc.state != ProcessState::Dead {
                let entry = proc.fds.get(target_fd)?.as_ref()?;
                return Some(fd_clone_entry(entry));
            }
        }
    }
    None
}

pub fn fd_get_mut(proc: &mut Process, fd: usize) -> Option<&mut FdEntry> {
    if fd >= MAX_FDS { return None; }
    tabla_fds(proc)[fd].as_mut()
}

pub fn fd_close(proc: &mut Process, fd: usize) {
    if fd < MAX_FDS {
        if let Some(entry) = tabla_fds(proc)[fd].take() {
            fd_entry_release(entry);
        }
    }
}

/// [CICLO 48] Libera las referencias de una entrada de descriptor que ya no
/// pertenece a ninguna tabla (p. ej. un fd recibido por SCM_RIGHTS que el
/// receptor no recoge).
pub fn fd_entry_release(entry: FdEntry) {
    match entry.fd_type {
        FdType::PipeRead(id) => pipe_close_reader(id),
        FdType::PipeWrite(id) => pipe_close_writer(id),
        FdType::Socket(sock_id) => crate::net::socket::socket_close(sock_id),
        FdType::SocketPairEnd { read_pipe, write_pipe } => {
            pipe_close_reader(read_pipe);
            pipe_close_writer(write_pipe);
        }
        FdType::EventFd(id) => {
            eventfd_close(id);
        }
        FdType::Epoll(id) => {
            epoll_close(id);
        }
        FdType::TimerFd(id) => {
            timerfd_close(id);
        }
        FdType::SignalFd(id) => {
            signalfd_close(id);
        }
        FdType::PtyMaster(id) | FdType::PtySlave(id) => {
            pty_close(id);
        }
        FdType::Mq(id) => {
            mq_close(id);
        }
        FdType::Fanotify(id) => {
            fanotify_close(id);
        }
        FdType::Inotify(id) => {
            inotify_close(id);
        }
        _ => {}
    }
}

pub fn pid_to_slot(pid: u64) -> Option<usize> {
    let guard = PROCESS_TABLE.lock();
    for (i, p) in guard.processes.iter().enumerate() {
        unsafe {
            let proc = p.as_ptr();
            if (*proc).pid == pid && (*proc).state != ProcessState::Dead {
                return Some(i);
            }
        }
    }
    None
}

pub fn init() {
    serial::write_str("PROC: init process table (max ");
    serial::write_usize(MAX_PROCS);
    serial::write_str(" slots)\n");
}

pub mod isolation;
pub use isolation::*;


pub mod signals;
pub use signals::*;

pub mod scheduler;
pub use scheduler::*;
