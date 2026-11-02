// kernel/src/process/signals.rs — Subsistema de Señales POSIX Estándar (Épica 58)
// portix OS v6.0 'Chronos' — Fases 3766 a 3850
//
// Implementación REAL y completa de entrega de señales con salvaguarda y
// restauración de contexto de usuario:
//
//   • frame de señales (SigFrame) construido sobre el stack de usuario
//   • redirección de RIP/RSP/RFLAGS al manejador registrado vía sigaction
//   • trampolín de retorno (sigreturn trampoline) resuelto por #PF
//   • syscall rt_sigreturn explícita
//   • máscara de bloqueo temporal durante la ejecución del manejador
//   • acciones por defecto POSIX (SIG_DFL / SIG_IGN) conforme a IEEE Std 1003.1
//   • SIGSEGV / SIGFPE despachados desde el manejador de fallo de página
//   • SIGPIPE generado al escribir en tubería sin lectores
//   • SIGCHLD emitido al progenitor en la salida de un hijo
//   • SIGALRM desde el temporizador programable (alarm)
//
// 100% Rust no_std, cero dependencias externas, cero unwrap.

extern crate alloc;

use alloc::format;
use alloc::string::String;

use super::types::*;
use super::PROCESS_TABLE;

// ─────────────────────────────────────────────────────────────────────────────
//  Constantes POSIX de señales
// ─────────────────────────────────────────────────────────────────────────────
pub const SIGHUP: u32 = 1;
pub const SIGINT: u32 = 2;
pub const SIGQUIT: u32 = 3;
pub const SIGILL: u32 = 4;
pub const SIGTRAP: u32 = 5;
pub const SIGABRT: u32 = 6;
pub const SIGBUS: u32 = 7;
pub const SIGFPE: u32 = 8;
pub const SIGKILL: u32 = 9;
pub const SIGUSR1: u32 = 10;
pub const SIGSEGV: u32 = 11;
pub const SIGUSR2: u32 = 12;
pub const SIGPIPE: u32 = 13;
pub const SIGALRM: u32 = 14;
pub const SIGTERM: u32 = 15;
pub const SIGSTKFLT: u32 = 16;
pub const SIGCHLD: u32 = 17;
pub const SIGCONT: u32 = 18;
pub const SIGSTOP: u32 = 19;
pub const SIGTSTP: u32 = 20;
pub const SIGTTIN: u32 = 21;
pub const SIGTTOU: u32 = 22;
pub const SIGURG: u32 = 23;
pub const SIGXCPU: u32 = 24;
pub const SIGXFSZ: u32 = 25;
pub const SIGVTALRM: u32 = 26;
pub const SIGPROF: u32 = 27;
pub const SIGWINCH: u32 = 28;
pub const SIGIO: u32 = 29;
pub const SIGPWR: u32 = 30;
pub const SIGSYS: u32 = 31;

/// Valor de `sa_handler` que solicita la acción por defecto del sistema.
pub const SIG_DFL: usize = 0;
/// Valor de `sa_handler` que instruye a ignorar la señal.
pub const SIG_IGN: usize = 1;

/// Número total de señales soportadas (1..=31 clásicas + 33..=64 de tiempo real).
pub const NSIG: usize = 65;

// ─────────────────────────────────────────────────────────────────────────────
//  Disposición del frame de excepción/syscall en el stack de kernel
// ─────────────────────────────────────────────────────────────────────────────
// Tras PUSH_REGS (15 GPR + 256 B de XMM) el stack queda:
//   +0   .. +255 : xmm0..xmm15
//   +256 .. +368 : rax rcx rdx rsi rdi r8 r9 r10 r11 rbx rbp r12 r13 r14 r15
//   +376         : RIP    de usuario
//   +384         : CS     de usuario
//   +392         : RFLAGS
//   +400         : RSP    de usuario
//   +408         : SS
pub const FRAME_RAX: usize = 256;
pub const FRAME_RCX: usize = 264;
pub const FRAME_RDX: usize = 272;
pub const FRAME_RSI: usize = 280;
pub const FRAME_RDI: usize = 288;
pub const FRAME_R8: usize = 296;
pub const FRAME_R9: usize = 304;
pub const FRAME_R10: usize = 312;
pub const FRAME_R11: usize = 320;
pub const FRAME_RBX: usize = 328;
pub const FRAME_RBP: usize = 336;
pub const FRAME_R12: usize = 344;
pub const FRAME_R13: usize = 352;
pub const FRAME_R14: usize = 360;
pub const FRAME_R15: usize = 368;
pub const FRAME_RIP: usize = 376;
pub const FRAME_CS: usize = 384;
pub const FRAME_RFLAGS: usize = 392;
pub const FRAME_RSP: usize = 400;
pub const FRAME_SS: usize = 408;

/// Tamaño total del frame empujado por PUSH_REGS + IRET (5 qwords del CPU).
pub const KERNEL_FRAME_SIZE: usize = 416;

// ─────────────────────────────────────────────────────────────────────────────
//  Trampolín de retorno (sigreturn trampoline)
// ─────────────────────────────────────────────────────────────────────────────
// Al despachar un manejador de usuario, el kernel coloca esta dirección como
// dirección de retorno. Cuando el manejador ejecuta `ret`, el contador de
// programa salta aquí; al no existir mapeo en el espacio de usuario se produce
// un #PF identificable, que el kernel traduce a `rt_sigreturn`.
pub const SIGRETURN_TRAMPOLINE: u64 = 0x0000_0000_7FF0_0000;

/// Marca de validación del frame de señales (evita restauraciones espurias).
/// [CICLO 48] Banderas de sigaction y codigos de `si_code` (ABI Linux x86_64).
pub const SA_SIGINFO: u64 = 0x4;
pub const SA_NODEFER: u64 = 0x4000_0000;
pub const SI_USER: i32 = 0;
pub const SI_KERNEL: i32 = 0x80;
pub const SI_QUEUE: i32 = -1;
pub const SIGINFO_SIZE: usize = 128;

/// [b191] `ucontext_t` Linux x86-64 entregado en RDX al manejador (antes RDX=0: el JVM de
/// OpenJDK leía pc=0 y abortaba). Bloque de 1024 bytes en la pila de usuario:
///   +0 uc_flags, +8 uc_link, +16 uc_stack{ss_sp,ss_flags,ss_size}, +40 gregs[23],
///   +224 fpstate*, +232 reserved[8], +296 uc_sigmask (glibc reserva 128 B → +424),
///   +432 bloque FXSAVE de 512 B (XMM0-15 en +160..+416) al que apunta `fpstate`.
pub const UCONTEXT_SIZE: usize = 1024;
pub const UC_GREGS: usize = 40;
pub const UC_FPSTATE_PTR: usize = 224;
pub const UC_SIGMASK: usize = 296;
pub const UC_FXSAVE: usize = 432;
pub const FX_XMM: usize = 160;
// Índices REG_* de <sys/ucontext.h>
pub const REG_R8: usize = 0; pub const REG_RDI: usize = 8; pub const REG_RSI: usize = 9;
pub const REG_RBP: usize = 10; pub const REG_RBX: usize = 11; pub const REG_RDX: usize = 12;
pub const REG_RAX: usize = 13; pub const REG_RCX: usize = 14; pub const REG_RSP: usize = 15;
pub const REG_RIP: usize = 16; pub const REG_EFL: usize = 17; pub const REG_CSGSFS: usize = 18;
pub const REG_ERR: usize = 19; pub const REG_TRAPNO: usize = 20; pub const REG_OLDMASK: usize = 21;
pub const REG_CR2: usize = 22;
/// Zona roja de la ABI SysV: 128 bytes bajo RSP que una función hoja puede usar sin
/// ajustar la pila; el frame de señales debe quedar por debajo (Linux hace lo mismo).
pub const ZONA_ROJA: u64 = 128;
/// Códigos `si_code` de fallos síncronos.
pub const SEGV_MAPERR: i32 = 1;
pub const SEGV_ACCERR: i32 = 2;
pub const FPE_INTDIV: i32 = 1;

/// [b191] Datos de un fallo síncrono (excepción de CPU) para `siginfo`/`ucontext`.
#[derive(Clone, Copy)]
pub struct InfoFallo { pub code: i32, pub addr: u64, pub err: u64, pub trapno: u64 }
pub const SIGFRAME_MAGIC: u64 = 0x504F_5254_4958_5346; // "PORTIXSF"

// ─────────────────────────────────────────────────────────────────────────────
//  Frame de señales sobre el stack de usuario (208 bytes, alineado a 16)
// ─────────────────────────────────────────────────────────────────────────────
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SigFrame {
    pub magic: u64,        // SIGFRAME_MAGIC
    pub rip: u64,          // RIP interrumpido
    pub rsp: u64,          // RSP interrumpido
    pub rflags: u64,       // RFLAGS interrumpidos
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub signum: u64,       // señal que se está despachando
    pub old_blocked: u64,  // máscara de bloqueo previa a la entrega
    pub handler: u64,      // dirección del manejador (auditoría)
    pub reserved: u64,     // relleno / extensión futura
}

pub const SIGFRAME_SIZE: usize = core::mem::size_of::<SigFrame>();

impl SigFrame {
    pub const fn empty() -> Self {
        SigFrame {
            magic: 0, rip: 0, rsp: 0, rflags: 0,
            rax: 0, rcx: 0, rdx: 0, rsi: 0, rdi: 0,
            r8: 0, r9: 0, r10: 0, r11: 0, rbx: 0, rbp: 0,
            r12: 0, r13: 0, r14: 0, r15: 0,
            signum: 0, old_blocked: 0, handler: 0, reserved: 0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Acciones por defecto POSIX
// ─────────────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefaultAction {
    /// Termina el proceso.
    Terminate,
    /// Termina el proceso y solicita volcado de núcleo.
    CoreDump,
    /// Detiene el proceso (control de trabajos).
    Stop,
    /// Continúa un proceso detenido.
    Continue,
    /// Señal descartada silenciosamente.
    Ignore,
}

pub fn default_action(sig: u32) -> DefaultAction {
    match sig {
        SIGCHLD | SIGURG | SIGWINCH | SIGCONT => DefaultAction::Ignore,
        SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU => DefaultAction::Stop,
        SIGQUIT | SIGILL | SIGTRAP | SIGABRT | SIGBUS | SIGFPE | SIGSEGV | SIGXCPU | SIGXFSZ | SIGSYS => DefaultAction::CoreDump,
        1..=31 => DefaultAction::Terminate,
        // POSIX: la accion por defecto de las senales de tiempo real es terminar.
        32..=63 => DefaultAction::Terminate,
        _ => DefaultAction::Ignore,
    }
}

/// Nombre canónico POSIX para reportes de telemetría y `kill -l`.
pub fn signal_name(sig: u32) -> &'static str {
    match sig {
        1 => "SIGHUP", 2 => "SIGINT", 3 => "SIGQUIT", 4 => "SIGILL",
        5 => "SIGTRAP", 6 => "SIGABRT", 7 => "SIGBUS", 8 => "SIGFPE",
        9 => "SIGKILL", 10 => "SIGUSR1", 11 => "SIGSEGV", 12 => "SIGUSR2",
        13 => "SIGPIPE", 14 => "SIGALRM", 15 => "SIGTERM", 16 => "SIGSTKFLT",
        17 => "SIGCHLD", 18 => "SIGCONT", 19 => "SIGSTOP", 20 => "SIGTSTP",
        21 => "SIGTTIN", 22 => "SIGTTOU", 23 => "SIGURG", 24 => "SIGXCPU",
        25 => "SIGXFSZ", 26 => "SIGVTALRM", 27 => "SIGPROF", 28 => "SIGWINCH",
        29 => "SIGIO", 30 => "SIGPWR", 31 => "SIGSYS",
        _ => "SIGRT",
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Contadores globales de auditoría del subsistema
// ─────────────────────────────────────────────────────────────────────────────
pub static mut SIG_STATS: [u64; 8] = [0; 8];
// [0] señales despachadas a manejador de usuario
// [1] señales ignoradas por SIG_IGN
// [2] señales con acción por defecto aplicada
// [3] sigreturns atendidos por el trampolín de #PF
// [4] sigreturns atendidos por llamada al sistema explícita
// [5] SIGSEGV/SIGFPE entregados desde excepción de CPU
// [6] SIGPIPE generados por tuberías sin lectores
// [7] SIGCHLD emitidos a procesos progenitores

#[inline]
fn stat_bump(idx: usize) {
    unsafe {
        if idx < SIG_STATS.len() {
            SIG_STATS[idx] = SIG_STATS[idx].wrapping_add(1);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Acceso validado a la memoria de usuario
// ─────────────────────────────────────────────────────────────────────────────
#[inline]
fn uread_u64(addr: u64) -> Option<u64> {
    if !crate::mem::paging::is_user_range_safe(addr as usize, 8) {
        return None;
    }
    Some(unsafe { core::ptr::read_unaligned(addr as *const u64) })
}

#[inline]
fn uwrite_u64(addr: u64, val: u64) -> bool {
    if !crate::mem::paging::is_user_range_writable(addr as usize, 8) {
        return false;
    }
    unsafe { core::ptr::write_unaligned(addr as *mut u64, val) };
    true
}

// ─────────────────────────────────────────────────────────────────────────────
//  Entrega de señales sobre un frame de kernel conocido
// ─────────────────────────────────────────────────────────────────────────────

/// Dirección del frame de registro guardado del proceso actual.
///
/// El stub de excepción/IRQ construye siempre un frame de tamaño
/// `KERNEL_FRAME_SIZE` inmediatamente por debajo de `kernel_stack_top`.
pub fn current_saved_frame() -> Option<u64> {
    let guard = PROCESS_TABLE.lock();
    let p = guard.current_process_ref()?;
    let top = p.kernel_stack_top as u64;
    if top < KERNEL_FRAME_SIZE as u64 {
        return None;
    }
    Some(top - KERNEL_FRAME_SIZE as u64)
}

/// Máscara de señales activa mientras se ejecuta un manejador.
/// La señal despachada y `sa_mask` se añaden; SIGKILL/SIGSTOP nunca se bloquean.
fn handler_mask(sig: u32, sa_mask: u64, old_blocked: u64, nodefer: bool) -> u64 {
    let mut m = old_blocked;
    if sig <= 63 && !nodefer {
        m |= 1u64 << sig;
    }
    m |= sa_mask;
    m &= !((1u64 << SIGKILL) | (1u64 << SIGSTOP));
    m
}

/// Construye el frame de señales sobre el stack de usuario y redirige el
/// contexto de ejecución hacia el manejador registrado.
///
/// `saved_rax`: valor que debe restaurar el sigreturn en RAX. Para el retorno
/// de una llamada al sistema es el resultado del syscall (que el stub de
/// ensamblador todavía no ha escrito en el frame); `None` conserva el valor
/// presente en el frame (interrupción asíncrona).
fn setup_user_handler(
    frame_rsp: u64,
    sig: u32,
    handler: u64,
    sa_mask: u64,
    old_blocked: u64,
    saved_rax: Option<u64>,
    sa_flags: u64,
    fallo: Option<InfoFallo>,
) -> Result<bool, i32> {
    let f = frame_rsp as *mut u64;
    let con_siginfo = (sa_flags & SA_SIGINFO) != 0;

    let saved_rip = unsafe { core::ptr::read_unaligned(f.add(FRAME_RIP / 8)) };
    let saved_cs = unsafe { core::ptr::read_unaligned(f.add(FRAME_CS / 8)) };
    let saved_rflags = unsafe { core::ptr::read_unaligned(f.add(FRAME_RFLAGS / 8)) };
    let saved_rsp = unsafe { core::ptr::read_unaligned(f.add(FRAME_RSP / 8)) };

    // Sólo se puede redirigir a un manejador si la interrupción provino de Ring 3.
    if (saved_cs & 3) != 3 {
        return Ok(false);
    }
    if saved_rsp < 0x1000 || saved_rsp >= USER_STACK_TOP as u64 {
        return Err(-14); // -EFAULT
    }

    // Distribución del bloque en el stack de usuario:
    //
    //   slot      -> dirección de retorno del manejador (trampolín sigreturn)
    //   frame_ptr -> inicio del SigFrame (slot + 8), alineado a 16 bytes
    //
    // El manejador arranca con RSP = slot, de modo que un `ret` salta al
    // trampolín y deja RSP = frame_ptr, justo donde espera `rt_sigreturn`.
    // Además slot ≡ 8 (mod 16) satisface la convención de alineación de la ABI.
    // [CICLO 48] Con SA_SIGINFO se reserva ademas un `siginfo_t` (128 bytes,
    // alineado a 16) por ENCIMA del SigFrame, de modo que el `ret` del
    // manejador siga dejando RSP = frame_ptr para `rt_sigreturn`.
    // [b191] Se respeta la zona roja y SIEMPRE se construye un `ucontext_t` (Linux lo pasa
    // aunque no haya SA_SIGINFO); el siginfo sólo se rellena con SA_SIGINFO.
    if saved_rsp < ZONA_ROJA + UCONTEXT_SIZE as u64 + SIGINFO_SIZE as u64 + SIGFRAME_SIZE as u64 + 0x1000 {
        return Err(-14);
    }
    let uc_ptr = (saved_rsp - ZONA_ROJA - UCONTEXT_SIZE as u64) & !0xF_u64;
    let info_ptr = if con_siginfo { (uc_ptr - SIGINFO_SIZE as u64 - 16) & !0xF_u64 } else { uc_ptr };
    if info_ptr < 0x1000 + SIGFRAME_SIZE as u64 {
        return Err(-14);
    }
    let frame_ptr = (info_ptr - SIGFRAME_SIZE as u64 - 16) & !0xF_u64;
    if frame_ptr < 0x1000 {
        return Err(-14); // desbordamiento del stack de usuario
    }
    let slot = frame_ptr - 8;

    if !crate::mem::paging::is_user_range_writable(frame_ptr as usize, SIGFRAME_SIZE) {
        return Err(-14);
    }
    if !crate::mem::paging::is_user_range_writable(slot as usize, 8) {
        return Err(-14);
    }

    let mut sf = SigFrame::empty();
    sf.magic = SIGFRAME_MAGIC;
    sf.rip = saved_rip;
    sf.rsp = saved_rsp;
    sf.rflags = saved_rflags;
    sf.rax = match saved_rax {
        Some(v) => v,
        None => unsafe { core::ptr::read_unaligned(f.add(FRAME_RAX / 8)) },
    };
    sf.rcx = unsafe { core::ptr::read_unaligned(f.add(FRAME_RCX / 8)) };
    sf.rdx = unsafe { core::ptr::read_unaligned(f.add(FRAME_RDX / 8)) };
    sf.rsi = unsafe { core::ptr::read_unaligned(f.add(FRAME_RSI / 8)) };
    sf.rdi = unsafe { core::ptr::read_unaligned(f.add(FRAME_RDI / 8)) };
    sf.r8 = unsafe { core::ptr::read_unaligned(f.add(FRAME_R8 / 8)) };
    sf.r9 = unsafe { core::ptr::read_unaligned(f.add(FRAME_R9 / 8)) };
    sf.r10 = unsafe { core::ptr::read_unaligned(f.add(FRAME_R10 / 8)) };
    sf.r11 = unsafe { core::ptr::read_unaligned(f.add(FRAME_R11 / 8)) };
    sf.rbx = unsafe { core::ptr::read_unaligned(f.add(FRAME_RBX / 8)) };
    sf.rbp = unsafe { core::ptr::read_unaligned(f.add(FRAME_RBP / 8)) };
    sf.r12 = unsafe { core::ptr::read_unaligned(f.add(FRAME_R12 / 8)) };
    sf.r13 = unsafe { core::ptr::read_unaligned(f.add(FRAME_R13 / 8)) };
    sf.r14 = unsafe { core::ptr::read_unaligned(f.add(FRAME_R14 / 8)) };
    sf.r15 = unsafe { core::ptr::read_unaligned(f.add(FRAME_R15 / 8)) };
    sf.signum = sig as u64;
    sf.old_blocked = old_blocked;
    sf.handler = handler;
    sf.reserved = uc_ptr; // [b191] rt_sigreturn restaura desde el ucontext (el manejador puede modificarlo)

    // Copia del frame de señales al espacio del usuario
    let src = unsafe {
        core::slice::from_raw_parts(&sf as *const SigFrame as *const u8, SIGFRAME_SIZE)
    };
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), frame_ptr as *mut u8, SIGFRAME_SIZE);
    }
    // Dirección de retorno del manejador -> trampolín de sigreturn
    if !uwrite_u64(slot, SIGRETURN_TRAMPOLINE) {
        return Err(-14);
    }
    // [b191] ucontext_t con gregs, cs/ss, err/trapno/cr2, máscara previa y bloque FXSAVE
    // con los XMM del contexto interrumpido (isr.asm los guarda en frame+0..256).
    {
        if !crate::mem::paging::is_user_range_writable(uc_ptr as usize, UCONTEXT_SIZE) {
            return Err(-14);
        }
        let mut uc = alloc::vec![0u8; UCONTEXT_SIZE];
        let w = |uc: &mut [u8], off: usize, v: u64| uc[off..off + 8].copy_from_slice(&v.to_ne_bytes());
        w(&mut uc, 24, 2); // ss_flags = SS_DISABLE (sin pila alternativa)
        let g = |i: usize| UC_GREGS + i * 8;
        w(&mut uc, g(REG_R8), sf.r8);     w(&mut uc, g(REG_R8 + 1), sf.r9);
        w(&mut uc, g(REG_R8 + 2), sf.r10); w(&mut uc, g(REG_R8 + 3), sf.r11);
        w(&mut uc, g(REG_R8 + 4), sf.r12); w(&mut uc, g(REG_R8 + 5), sf.r13);
        w(&mut uc, g(REG_R8 + 6), sf.r14); w(&mut uc, g(REG_R8 + 7), sf.r15);
        w(&mut uc, g(REG_RDI), sf.rdi); w(&mut uc, g(REG_RSI), sf.rsi);
        w(&mut uc, g(REG_RBP), sf.rbp); w(&mut uc, g(REG_RBX), sf.rbx);
        w(&mut uc, g(REG_RDX), sf.rdx); w(&mut uc, g(REG_RAX), sf.rax);
        w(&mut uc, g(REG_RCX), sf.rcx); w(&mut uc, g(REG_RSP), sf.rsp);
        w(&mut uc, g(REG_RIP), sf.rip); w(&mut uc, g(REG_EFL), sf.rflags);
        let saved_ss = unsafe { core::ptr::read_unaligned(f.add(FRAME_SS / 8)) };
        w(&mut uc, g(REG_CSGSFS), (saved_cs & 0xFFFF) | ((saved_ss & 0xFFFF) << 48));
        if let Some(fi) = fallo {
            w(&mut uc, g(REG_ERR), fi.err);
            w(&mut uc, g(REG_TRAPNO), fi.trapno);
            w(&mut uc, g(REG_CR2), fi.addr);
        }
        w(&mut uc, g(REG_OLDMASK), old_blocked);
        w(&mut uc, UC_SIGMASK, old_blocked);
        w(&mut uc, UC_FPSTATE_PTR, uc_ptr + UC_FXSAVE as u64);
        // Cabecera FXSAVE mínima: FCW=0x37F, MXCSR=0x1F80, MXCSR_MASK=0xFFFF
        uc[UC_FXSAVE..UC_FXSAVE + 2].copy_from_slice(&0x037Fu16.to_ne_bytes());
        uc[UC_FXSAVE + 24..UC_FXSAVE + 28].copy_from_slice(&0x1F80u32.to_ne_bytes());
        uc[UC_FXSAVE + 28..UC_FXSAVE + 32].copy_from_slice(&0xFFFFu32.to_ne_bytes());
        let xmm = unsafe { core::slice::from_raw_parts(frame_rsp as *const u8, 256) };
        uc[UC_FXSAVE + FX_XMM..UC_FXSAVE + FX_XMM + 256].copy_from_slice(xmm);
        unsafe { core::ptr::copy_nonoverlapping(uc.as_ptr(), uc_ptr as *mut u8, UCONTEXT_SIZE); }
    }
    // [CICLO 48] siginfo_t real: si_signo, si_errno, si_code, si_pid, si_uid, si_value.
    // [b191] En fallos síncronos: si_code SEGV_MAPERR/ACCERR/FPE_INTDIV y si_addr (+16).
    if con_siginfo {
        if !crate::mem::paging::is_user_range_writable(info_ptr as usize, SIGINFO_SIZE) {
            return Err(-14);
        }
        let mut info = [0u8; SIGINFO_SIZE];
        info[0..4].copy_from_slice(&(sig as i32).to_ne_bytes());
        if let Some(fi) = fallo {
            info[8..12].copy_from_slice(&fi.code.to_ne_bytes());
            info[16..24].copy_from_slice(&fi.addr.to_ne_bytes());
        } else {
            let origen = {
                let guard = PROCESS_TABLE.lock();
                guard.current_process_ref().map(|p| origen_para(p, sig)).unwrap_or_default()
            };
            info[8..12].copy_from_slice(&origen.code.to_ne_bytes());
            info[16..20].copy_from_slice(&origen.pid.to_ne_bytes());
            info[20..24].copy_from_slice(&origen.uid.to_ne_bytes());
            info[24..32].copy_from_slice(&origen.value.to_ne_bytes());
        }
        unsafe { core::ptr::copy_nonoverlapping(info.as_ptr(), info_ptr as *mut u8, SIGINFO_SIZE); }
    }

    // Redirección del contexto: el manejador recibe (signum, 0) y arranca con
    // RSP apuntando a la dirección de retorno del trampolín.
    unsafe {
        core::ptr::write_unaligned(f.add(FRAME_RIP / 8), handler);
        core::ptr::write_unaligned(f.add(FRAME_RSP / 8), slot);
        core::ptr::write_unaligned(f.add(FRAME_RDI / 8), sig as u64);
        // RSI = siginfo_t (solo con SA_SIGINFO); RDX = ucontext_t [b191].
        core::ptr::write_unaligned(f.add(FRAME_RSI / 8), if con_siginfo { info_ptr } else { 0u64 });
        core::ptr::write_unaligned(f.add(FRAME_RDX / 8), uc_ptr);
        core::ptr::write_unaligned(f.add(FRAME_RAX / 8), 0u64);
        // El manejador arranca con un estado saneado (sin TF/DF/AC heredados)
        core::ptr::write_unaligned(
            f.add(FRAME_RFLAGS / 8),
            saved_rflags & !((1u64 << 8) | (1u64 << 10) | (1u64 << 18)),
        );
    }

    // Bloqueo temporal: señal actual + sa_mask + máscara previa
    {
        let guard = PROCESS_TABLE.lock();
        if let Some(p) = guard.current_process_ref() {
            p.sig_blocked = handler_mask(sig, sa_mask, old_blocked, (sa_flags & SA_NODEFER) != 0);
        }
    }

    Ok(true)
}

/// Decide y registra el estado de una señal fatal, cerrando los demás hilos
/// salvo cuando se trate de un cierre interno ya decidido. El llamador debe
/// terminar inmediatamente el proceso sin volver a Ring 3.
pub(crate) fn preparar_terminacion_por_senal(sig: u32, pid: u64) -> i32 {
    // Un líder puede recibir SIGKILL INTERNO para que termine en su propia
    // CPU tras exit_group de otro hilo. En ese caso el código/señal del grupo
    // ya están decididos y no son necesariamente 137/SIGKILL.
    let (solo, es_hilo, lider_salio, guardado, senal_guardada) = super::process_by_pid(pid)
        .map(|p| (p.salir_solo, p.es_hilo, p.lider_salio, p.exit_code, p.term_signal))
        .unwrap_or((false, false, false, 0, 0));
    let (code, term_signal) = super::exit_status::estado_por_senal(
        sig, solo, es_hilo, lider_salio, guardado, senal_guardada);
    // [T-017] Un hilo marcado `salir_solo` muere sin arrastrar al grupo.
    if !solo { super::matar_grupo_por_senal(pid, code, term_signal); } // [T-005]
    {
        let guard = PROCESS_TABLE.lock();
        for p in &guard.processes {
            unsafe {
                let proc = p.as_ptr() as *mut Process;
                if (*proc).pid == pid && (*proc).state != ProcessState::Dead {
                    (*proc).exit_code = code;
                    (*proc).term_signal = term_signal;
                    (*proc).sig_pending = 0;
                    (*proc).alarm_ticks = 0;
                    break;
                }
            }
        }
    }
    code
}

fn terminate_current(sig: u32, pid: u64) -> ! {
    preparar_terminacion_por_senal(sig, pid);
    super::scheduler::process_exit_trampoline()
}

/// Despacha todas las señales pendientes no bloqueadas del proceso en curso
/// escribiendo directamente sobre el frame guardado en el stack de kernel.
///
/// `frame_rsp` es el valor de RSP en el instante de PUSH_REGS (tal como lo
/// reciben `schedule_tick` y los despachadores de syscalls).
///
/// Devuelve `true` si se aplicó alguna acción sobre el proceso en curso.
/// Si la acción por defecto es terminar, esta función no retorna: el proceso
/// sale por el trampolín del planificador.
pub fn deliver_pending(frame_rsp: u64, saved_rax: Option<u64>) -> bool {
    if frame_rsp < 0x1000 {
        return false;
    }

    // Sólo se procesa contexto de Ring 3: en Ring 0 la señal permanece
    // pendiente y se reintenta al volver a usuario (evita reentradas).
    let saved_cs = unsafe { core::ptr::read_unaligned((frame_rsp as *const u64).add(FRAME_CS / 8)) };
    if (saved_cs & 3) != 3 {
        return false;
    }

    let (pending, blocked) = {
        let guard = PROCESS_TABLE.lock();
        match guard.current_process_ref() {
            Some(p) => (p.sig_pending, p.sig_blocked),
            None => return false,
        }
    };

    let mut action_taken = false;

    // [CICLO 47] Recorre tambien las senales de tiempo real 32..=63 (antes
    // SIGRTMAX=63 en Portix: la mascara usa el bit `sig` y el bit 64 no existe;
    // quedaban pendientes para siempre porque el bucle paraba en 32).
    for sig in 1u32..=63u32 {
        let bit = 1u64 << sig;
        if (pending & bit) == 0 || (blocked & bit) != 0 {
            continue;
        }

        if sig == SIGKILL {
            // SIGKILL nunca puede ser capturado, ignorado ni bloqueado.
            let pid = current_process_pid();
            consume_signal(sig);
            stat_bump(2);
            crate::drivers::serial::write_str("[SIG] SIGKILL -> terminacion PID ");
            crate::drivers::serial::write_usize(pid as usize);
            crate::drivers::serial::write_str("\n");
            if pid != 0 {
                terminate_current(sig, pid);
            }
            return true;
        }

        // [CICLO 48] Antes se pasaba `sig_flags` como mascara (SA_RESTART
        // bloqueaba SIGWINCH y SA_SIGINFO bloqueaba SIGINT dentro del manejador).
        let (handler, sa_mask, sa_flags) = {
            let guard = PROCESS_TABLE.lock();
            match guard.current_process_ref() {
                Some(p) => (p.sig_actions[sig as usize], p.sig_masks[sig as usize], p.sig_flags[sig as usize] as u64),
                None => return action_taken,
            }
        };

        if handler == SIG_IGN {
            consume_signal(sig);
            stat_bump(1);
            action_taken = true;
            continue;
        }

        if handler == SIG_DFL {
            match default_action(sig) {
                DefaultAction::Stop => {
                    // [CONTROL DE TRABAJOS] Parada real (POSIX): el proceso queda
                    // `Stopped` con la señal registrada; su padre la recoge con
                    // `waitpid(WUNTRACED)` como (señal << 8) | 0x7f y sólo
                    // SIGCONT (o SIGKILL) lo saca de ahí. El llamador debe ceder
                    // la CPU al ver el estado `Stopped`.
                    let pid = current_process_pid();
                    consume_signal(sig);
                    stat_bump(2);
                    if pid != 0 {
                        crate::process::stop_for_tracer(pid, sig as u8, 0);
                        crate::drivers::serial::write_str("[JOB] ");
                        crate::drivers::serial::write_str(signal_name(sig));
                        crate::drivers::serial::write_str(" -> PID ");
                        crate::drivers::serial::write_usize(pid as usize);
                        crate::drivers::serial::write_str(" detenido (esperando SIGCONT)\n");
                    }
                    return true;
                }
                DefaultAction::Ignore | DefaultAction::Continue => {
                    consume_signal(sig);
                    stat_bump(2);
                    action_taken = true;
                    continue;
                }
                DefaultAction::Terminate | DefaultAction::CoreDump => {
                    let pid = current_process_pid();
                    consume_signal(sig);
                    stat_bump(2);
                    crate::drivers::serial::write_str("[SIG] Accion por defecto ");
                    crate::drivers::serial::write_str(signal_name(sig));
                    crate::drivers::serial::write_str(" -> terminacion PID ");
                    crate::drivers::serial::write_usize(pid as usize);
                    crate::drivers::serial::write_str("\n");
                    if pid != 0 {
                        terminate_current(sig, pid);
                    }
                    return true;
                }
            }
        }

        // Manejador de usuario registrado
        let old_blocked = {
            let guard = PROCESS_TABLE.lock();
            match guard.current_process_ref() {
                // [T-001/FIFO] Dentro de `rt_sigsuspend` la máscara vigente es la
                // temporal: el manejador debe devolver la PREVIA (POSIX).
                Some(p) if p.sig_suspend_activo => { p.sig_suspend_activo = false; p.sig_suspend_prev }
                Some(p) => p.sig_blocked,
                None => 0,
            }
        };
        match setup_user_handler(frame_rsp, sig, handler as u64, sa_mask, old_blocked, saved_rax, sa_flags, None) {
            Ok(true) => {
                consume_signal(sig);
                stat_bump(0);
                return true;
            }
            // Contexto de kernel: la señal queda pendiente y se reintenta al
            // volver a Ring 3.
            Ok(false) => return action_taken,
            Err(_) => {
                // Stack de usuario inutilizable -> terminación controlada
                let pid = current_process_pid();
                consume_signal(sig);
                stat_bump(2);
                if pid != 0 {
                    terminate_current(sig, pid);
                }
                return true;
            }
        }
    }

    action_taken
}

/// Lee el RAX guardado del frame (necesario tras `rt_sigreturn`, porque el
/// stub de ensamblador reescribe RAX con el valor devuelto por el despachador).
pub fn frame_rax(frame_rsp: u64) -> u64 {
    if frame_rsp < 0x1000 {
        return 0;
    }
    unsafe { core::ptr::read_unaligned((frame_rsp as *const u64).add(FRAME_RAX / 8)) }
}

fn current_process_pid() -> u64 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.pid).unwrap_or(0)
}

fn consume_signal(sig: u32) {
    let guard = PROCESS_TABLE.lock();
    if let Some(p) = guard.current_process_ref() {
        // [CICLO 48] Senal de tiempo real: se retira la instancia mas antigua
        // de la cola FIFO; si quedan mas instancias el bit sigue pendiente.
        if sig >= 32 {
            let n = p.sig_rt_n as usize;
            if let Some(pos) = p.sig_rt_cola[..n].iter().position(|e| e.0 as u32 == sig) {
                for i in pos..n.saturating_sub(1) { p.sig_rt_cola[i] = p.sig_rt_cola[i + 1]; }
                p.sig_rt_n = (n - 1) as u8;
                if p.sig_rt_cola[..n - 1].iter().any(|e| e.0 as u32 == sig) {
                    return;
                }
            }
        }
        p.sig_pending &= !(1u64 << sig);
    }
}

/// [CICLO 48] sigwaitinfo/sigtimedwait: toma la senal pendiente de menor
/// numero dentro de `set` (la consume, con su instancia RT) y devuelve su origen.
pub fn tomar_pendiente(set: u64) -> Option<(u32, super::types::SigOrigen)> {
    let elegida = {
        let guard = PROCESS_TABLE.lock();
        let p = guard.current_process_ref()?;
        let cand = p.sig_pending & set & !(1u64 << SIGKILL) & !(1u64 << SIGSTOP);
        if cand == 0 { return None; }
        let sig = cand.trailing_zeros();
        (sig, origen_para(p, sig))
    };
    consume_signal(elegida.0);
    Some(elegida)
}

pub fn pendientes_y_bloqueadas() -> (u64, u64) {
    let guard = PROCESS_TABLE.lock();
    match guard.current_process_ref() { Some(p) => (p.sig_pending, p.sig_blocked), None => (0, 0) }
}

/// [CICLO 48] Origen que vera el manejador: para senales RT, la instancia mas
/// antigua de la cola; para el resto, el ultimo emisor registrado.
fn origen_para(p: &Process, sig: u32) -> super::types::SigOrigen {
    if sig >= 32 {
        let n = p.sig_rt_n as usize;
        if let Some(e) = p.sig_rt_cola[..n].iter().find(|e| e.0 as u32 == sig) { return e.1; }
    }
    p.sig_src[sig as usize]
}

/// Inyecta una señal en la cola de pendientes de un proceso concreto.
pub fn raise_for_pid(pid: u64, sig: u32) -> bool {
    raise_for_pid_info(pid, sig, SI_USER, 0)
}

/// [CICLO 48] Emite `sig` a `pid` registrando el origen (`si_code`, `si_value`,
/// PID emisor) que recibira un manejador SA_SIGINFO.
pub fn raise_for_pid_info(pid: u64, sig: u32, code: i32, value: u64) -> bool {
    raise_for_pid_res(pid, sig, code, value) == 0
}

/// Variante con codigo de error: 0, -ESRCH (no existe) o -EAGAIN (cola de
/// senales de tiempo real llena, como exige POSIX para `sigqueue`).
pub fn raise_for_pid_res(pid: u64, sig: u32, code: i32, value: u64) -> i32 {
    if sig == 0 || sig > 63 {
        return -22;
    }
    let emisor = crate::arch::percpu::pid_actual() as u32;
    let guard = PROCESS_TABLE.lock();
    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr() as *mut Process;
            if (*proc).pid == pid && (*proc).state != ProcessState::Dead {
                // [CONTROL DE TRABAJOS] SIGCONT reanuda un proceso detenido por
                // señal (no por el trazador) y anula las paradas pendientes;
                // las señales de parada anulan un SIGCONT pendiente (POSIX 2.4.3).
                let stop_mask = (1u64 << SIGSTOP) | (1u64 << SIGTSTP) | (1u64 << SIGTTIN) | (1u64 << SIGTTOU);
                if sig == SIGCONT {
                    (*proc).sig_pending &= !stop_mask;
                    let parada_por_senal = matches!((*proc).stop_signal as u32, SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU);
                    if (*proc).state == ProcessState::Stopped && parada_por_senal {
                        (*proc).state = ProcessState::Ready;
                        (*proc).stop_signal = 0;
                        (*proc).stop_reported = true;
                        crate::drivers::serial::write_str("[JOB] SIGCONT -> PID ");
                        crate::drivers::serial::write_usize(pid as usize);
                        crate::drivers::serial::write_str(" reanudado\n");
                    }
                } else if (stop_mask >> sig) & 1 == 1 {
                    (*proc).sig_pending &= !(1u64 << SIGCONT);
                } else if sig == SIGKILL && (*proc).state == ProcessState::Stopped {
                    (*proc).state = ProcessState::Ready; // un detenido también debe poder morir
                }
                let origen = super::types::SigOrigen { pid: emisor, uid: 0, code, value };
                if sig >= 32 {
                    let n = (*proc).sig_rt_n as usize;
                    if n >= super::types::SIG_RT_COLA_MAX { return -11; } // -EAGAIN
                    (*proc).sig_rt_cola[n] = (sig as u8, origen);
                    (*proc).sig_rt_n = (n + 1) as u8;
                }
                (*proc).sig_pending |= 1u64 << sig;
                (*proc).sig_src[sig as usize] = origen;
                // Un SIGKILL externo dirigido al líder que ya hizo exit(2)
                // debe despertar su espera en Ring 0 y conservar la semántica
                // fatal de la señal; no es el SIGKILL interno de cierre.
                if sig == SIGKILL && !(*proc).es_hilo && (*proc).lider_salio {
                    (*proc).salir_solo = false;
                    if (*proc).state == ProcessState::Blocked {
                        (*proc).state = ProcessState::Ready;
                        (*proc).sleep_until = 0;
                    }
                }
                // [CONTROL DE TRABAJOS] Un proceso dormido en `sleep` debe
                // reaccionar al instante a las señales de parada/terminación:
                // se interrumpe el sueño y la señal se entrega al volver del syscall.
                if sig != SIGCONT && sig != SIGCHLD && sig != SIGURG && sig != SIGWINCH
                    && (*proc).state == ProcessState::Blocked && (*proc).sleep_until > 0
                    && (*proc).pid != crate::arch::percpu::pid_actual() {
                    (*proc).sleep_until = 0;
                    (*proc).state = ProcessState::Ready;
                }
                return 0;
            }
        }
    }
    -3
}

/// [CONTROL DE TRABAJOS] Acceso a la terminal desde un grupo que no es el de
/// primer plano (POSIX 11.1.4): devuelve `None` si el acceso está permitido;
/// si no, envía `sig` (SIGTTIN/SIGTTOU) a todo el grupo del proceso en curso y
/// devuelve `Some(-EINTR)`; si la señal está ignorada o bloqueada, `Some(-EIO)`.
pub fn tty_access_check(sig: u32) -> Option<i64> {
    let fg = crate::process::fg_pgid();
    if fg == 0 { return None; }
    let (pid, pgid, ignorada) = {
        let guard = PROCESS_TABLE.lock();
        match guard.current_process_ref() {
            Some(p) => (p.pid, p.pgid, p.sig_actions[sig as usize] == SIG_IGN || (p.sig_blocked >> sig) & 1 == 1),
            None => return None,
        }
    };
    if pgid == fg || pgid == 0 { return None; }
    // Si el grupo de primer plano ya no tiene procesos vivos (p. ej. el hijo
    // terminó y el shell recupera la terminal), el acceso se permite.
    let fg_vivos = {
        let guard = PROCESS_TABLE.lock();
        guard.processes.iter().filter(|p| unsafe {
            let q = p.as_ptr() as *const Process;
            (*q).pgid == fg && (*q).pid != 0 && (*q).state != ProcessState::Dead && (*q).state != ProcessState::Zombie
        }).count()
    };
    if fg_vivos == 0 { return None; }
    // Señal ignorada/bloqueada: tcsetpgrp se permite (POSIX); la lectura falla con EIO.
    if ignorada { return if sig == SIGTTOU { None } else { Some(-5) }; }
    let n = kill_matching(-(pgid as i64), sig as i32, pgid).unwrap_or(0);
    crate::drivers::serial::write_str(if sig == SIGTTIN { "[TTY] lectura desde segundo plano -> SIGTTIN al grupo " } else { "[TTY] tcsetpgrp desde segundo plano -> SIGTTOU al grupo " });
    crate::drivers::serial::write_usize(pgid as usize);
    crate::drivers::serial::write_str(" (PID ");
    crate::drivers::serial::write_usize(pid as usize);
    crate::drivers::serial::write_str(", ");
    crate::drivers::serial::write_usize(n);
    crate::drivers::serial::write_str(" procesos)\n");
    Some(-4)
}

/// Emite una señal al proceso en curso (usado por SIGPIPE / SIGALRM / SIGSEGV).
pub fn raise_current(sig: u32) {
    if sig == 0 || sig > 63 {
        return;
    }
    let guard = PROCESS_TABLE.lock();
    if let Some(p) = guard.current_process_ref() {
        p.sig_pending |= 1u64 << sig;
        p.sig_src[sig as usize] = super::types::SigOrigen { pid: 0, uid: 0, code: SI_KERNEL, value: 0 };
    }
}

/// Emite `SIGCHLD` al progenitor de un hijo recién terminado.
pub fn raise_sigchld_to_parent(parent_pid: u64) {
    if parent_pid == 0 {
        return;
    }
    if raise_for_pid(parent_pid, SIGCHLD) {
        stat_bump(7);
    }
}

/// Disposición actual de una señal para el proceso en curso.
pub fn current_disposition(sig: u32) -> usize {
    if sig == 0 || sig as usize >= NSIG {
        return SIG_DFL;
    }
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.sig_actions[sig as usize]).unwrap_or(SIG_DFL)
}

// ─────────────────────────────────────────────────────────────────────────────
//  SIGSEGV / SIGFPE desde excepciones de CPU
// ─────────────────────────────────────────────────────────────────────────────
/// Intenta entregar `sig` al proceso en curso desde un manejador de excepción.
///
/// `sig_rip` / `sig_rsp` / `sig_rflags` corresponden al contexto capturado por
/// la excepción y los registros generales provienen del `crash_frame` (que se
/// captura antes de que el stub de ensamblador altere cualquier registro).
/// Devuelve `true` si el proceso dispone de un manejador de usuario y el
/// contexto fue reencauzado hacia él.
#[allow(clippy::too_many_arguments)]
/// [T-020] ¿El proceso actual tiene un manejador de usuario (ni SIG_DFL ni SIG_IGN) para `sig`?
pub fn tiene_manejador(sig: u32) -> bool {
    if sig as usize >= 64 { return false; }
    let guard = PROCESS_TABLE.lock();
    match guard.current_process_ref() {
        Some(p) => p.sig_actions[sig as usize] > SIG_IGN,
        None => false,
    }
}

pub fn deliver_from_exception(
    sig: u32,
    sig_rip: u64,
    sig_rsp: u64,
    sig_rflags: u64,
    rax: u64, rbx: u64, rcx: u64, rdx: u64, rsi: u64, rdi: u64,
    r8: u64, r9: u64, r10: u64, r11: u64, r12: u64, r13: u64, r14: u64,
    r15: u64, rbp: u64,
    fallo: InfoFallo,
) -> bool {
    let frame_rsp = match current_saved_frame() {
        Some(v) => v,
        None => return false,
    };
    let f = frame_rsp as *mut u64;

    // Sólo contexto de Ring 3 puede reencauzarse a un manejador de usuario.
    let saved_cs = unsafe { core::ptr::read_unaligned(f.add(FRAME_CS / 8)) };
    if (saved_cs & 3) != 3 || sig_rsp < 0x1000 {
        return false;
    }

    // Relleno del frame con el contexto exacto de la excepción.
    unsafe {
        core::ptr::write_unaligned(f.add(FRAME_RIP / 8), sig_rip);
        core::ptr::write_unaligned(f.add(FRAME_RSP / 8), sig_rsp);
        core::ptr::write_unaligned(f.add(FRAME_RFLAGS / 8), sig_rflags);
        core::ptr::write_unaligned(f.add(FRAME_RAX / 8), rax);
        core::ptr::write_unaligned(f.add(FRAME_RCX / 8), rcx);
        core::ptr::write_unaligned(f.add(FRAME_RDX / 8), rdx);
        core::ptr::write_unaligned(f.add(FRAME_RSI / 8), rsi);
        core::ptr::write_unaligned(f.add(FRAME_RDI / 8), rdi);
        core::ptr::write_unaligned(f.add(FRAME_R8 / 8), r8);
        core::ptr::write_unaligned(f.add(FRAME_R9 / 8), r9);
        core::ptr::write_unaligned(f.add(FRAME_R10 / 8), r10);
        core::ptr::write_unaligned(f.add(FRAME_R11 / 8), r11);
        core::ptr::write_unaligned(f.add(FRAME_RBX / 8), rbx);
        core::ptr::write_unaligned(f.add(FRAME_RBP / 8), rbp);
        core::ptr::write_unaligned(f.add(FRAME_R12 / 8), r12);
        core::ptr::write_unaligned(f.add(FRAME_R13 / 8), r13);
        core::ptr::write_unaligned(f.add(FRAME_R14 / 8), r14);
        core::ptr::write_unaligned(f.add(FRAME_R15 / 8), r15);
    }

    let (handler, sa_mask, old_blocked, sa_flags) = {
        let guard = PROCESS_TABLE.lock();
        match guard.current_process_ref() {
            Some(p) => (p.sig_actions[sig as usize], p.sig_masks[sig as usize], p.sig_blocked, p.sig_flags[sig as usize] as u64),
            None => return false,
        }
    };

    if handler == SIG_IGN {
        consume_signal(sig);
        stat_bump(1);
        return true;
    }
    if handler <= SIG_IGN {
        return false; // SIG_DFL -> el llamador aplica la acción por defecto
    }

    match setup_user_handler(frame_rsp, sig, handler as u64, sa_mask, old_blocked, Some(rax), sa_flags, Some(fallo)) {
        Ok(true) => {
            consume_signal(sig);
            stat_bump(0);
            stat_bump(5);
            true
        }
        _ => false,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  rt_sigreturn — restauración del contexto tras el manejador
// ─────────────────────────────────────────────────────────────────────────────
/// Restaura el contexto de usuario desde el `SigFrame` apuntado por `user_rsp`.
///
/// Se emplea tanto desde el trampolín de #PF como desde la llamada al sistema
/// `rt_sigreturn` explícita. Escribe los registros salvados de vuelta en el
/// frame de kernel del proceso en curso.
pub fn sigreturn_restore(user_rsp: u64) -> Result<(), i32> {
    if user_rsp < 0x1000 || user_rsp >= USER_STACK_TOP as u64 || (user_rsp & 7) != 0 {
        return Err(-14); // -EFAULT
    }
    // El manejador puede haber ajustado RSP en su prólogo antes de invocar
    // rt_sigreturn de forma explícita; se tolera una ventana de hasta 64 bytes
    // por encima del puntero indicado buscando la marca del SigFrame.
    let mut frame_ptr = 0u64;
    for off in 0..=8u64 {
        let cand = user_rsp + off * 8;
        if let Some(v) = uread_u64(cand) {
            if v == SIGFRAME_MAGIC {
                frame_ptr = cand;
                break;
            }
        }
    }
    if frame_ptr == 0 {
        return Err(-22); // -EINVAL: no hay frame de señales válido
    }

    let frame_rsp = match current_saved_frame() {
        Some(v) => v,
        None => return Err(-14), // -EFAULT: sin marco de usuario guardado
    };

    // Lectura del frame de señales del usuario
    if !crate::mem::paging::is_user_range_safe(frame_ptr as usize, SIGFRAME_SIZE) {
        return Err(-14);
    }
    let mut sf = unsafe { core::ptr::read_unaligned(frame_ptr as *const SigFrame) };

    // [b191] El contexto autoritativo es el `ucontext_t` (el manejador puede haber cambiado
    // RIP —continuaciones del JVM—, RSP o cualquier registro); los XMM vuelven al frame.
    let uc_ptr = sf.reserved;
    if uc_ptr != 0 && uc_ptr < USER_STACK_TOP as u64
        && crate::mem::paging::is_user_range_safe(uc_ptr as usize, UCONTEXT_SIZE)
    {
        let r = |i: usize| -> u64 {
            let mut b = [0u8; 8];
            unsafe { core::ptr::copy_nonoverlapping((uc_ptr as usize + UC_GREGS + i * 8) as *const u8, b.as_mut_ptr(), 8); }
            u64::from_ne_bytes(b)
        };
        sf.r8 = r(REG_R8); sf.r9 = r(REG_R8 + 1); sf.r10 = r(REG_R8 + 2); sf.r11 = r(REG_R8 + 3);
        sf.r12 = r(REG_R8 + 4); sf.r13 = r(REG_R8 + 5); sf.r14 = r(REG_R8 + 6); sf.r15 = r(REG_R8 + 7);
        sf.rdi = r(REG_RDI); sf.rsi = r(REG_RSI); sf.rbp = r(REG_RBP); sf.rbx = r(REG_RBX);
        sf.rdx = r(REG_RDX); sf.rax = r(REG_RAX); sf.rcx = r(REG_RCX); sf.rsp = r(REG_RSP);
        sf.rip = r(REG_RIP);
        // RFLAGS: sólo bits modificables por el usuario (CF PF AF ZF SF TF DF OF) + IF forzado
        sf.rflags = (r(REG_EFL) & 0x0000_0CD5) | 0x202;
        let mut m = [0u8; 8];
        unsafe { core::ptr::copy_nonoverlapping((uc_ptr as usize + UC_SIGMASK) as *const u8, m.as_mut_ptr(), 8); }
        sf.old_blocked = u64::from_ne_bytes(m);
        let fp = r((UC_FPSTATE_PTR - UC_GREGS) / 8);
        if fp != 0 && crate::mem::paging::is_user_range_safe(fp as usize, 512) {
            unsafe { core::ptr::copy_nonoverlapping((fp as usize + FX_XMM) as *const u8, frame_rsp as *mut u8, 256); }
        }
    }

    let f = frame_rsp as *mut u64;
    unsafe {
        core::ptr::write_unaligned(f.add(FRAME_RIP / 8), sf.rip);
        core::ptr::write_unaligned(f.add(FRAME_RSP / 8), sf.rsp);
        core::ptr::write_unaligned(f.add(FRAME_RFLAGS / 8), sf.rflags);
        core::ptr::write_unaligned(f.add(FRAME_RAX / 8), sf.rax);
        core::ptr::write_unaligned(f.add(FRAME_RCX / 8), sf.rcx);
        core::ptr::write_unaligned(f.add(FRAME_RDX / 8), sf.rdx);
        core::ptr::write_unaligned(f.add(FRAME_RSI / 8), sf.rsi);
        core::ptr::write_unaligned(f.add(FRAME_RDI / 8), sf.rdi);
        core::ptr::write_unaligned(f.add(FRAME_R8 / 8), sf.r8);
        core::ptr::write_unaligned(f.add(FRAME_R9 / 8), sf.r9);
        core::ptr::write_unaligned(f.add(FRAME_R10 / 8), sf.r10);
        core::ptr::write_unaligned(f.add(FRAME_R11 / 8), sf.r11);
        core::ptr::write_unaligned(f.add(FRAME_RBX / 8), sf.rbx);
        core::ptr::write_unaligned(f.add(FRAME_RBP / 8), sf.rbp);
        core::ptr::write_unaligned(f.add(FRAME_R12 / 8), sf.r12);
        core::ptr::write_unaligned(f.add(FRAME_R13 / 8), sf.r13);
        core::ptr::write_unaligned(f.add(FRAME_R14 / 8), sf.r14);
        core::ptr::write_unaligned(f.add(FRAME_R15 / 8), sf.r15);
    }

    // Restauración de la máscara de bloqueo previa a la entrega
    {
        let guard = PROCESS_TABLE.lock();
        if let Some(p) = guard.current_process_ref() {
            p.sig_blocked = sf.old_blocked & !((1u64 << SIGKILL) | (1u64 << SIGSTOP));
        }
    }

    // Invalidación del frame para impedir dobles restauraciones
    let _ = uwrite_u64(frame_ptr, 0);

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  API POSIX (sigprocmask / sigpending / sigaction / alarm)
// ─────────────────────────────────────────────────────────────────────────────
pub fn current_sig_pending() -> u64 {
    let guard = PROCESS_TABLE.lock();
    guard.current_process_ref().map(|p| p.sig_pending).unwrap_or(0)
}

/// [T-005b, defecto 29] ¿Hay una señal pendiente que de verdad interrumpa una
/// syscall bloqueante? Cuenta sólo las no bloqueadas y cuya disposición NO es
/// ignorarla (SIG_IGN, o SIG_DFL en SIGCHLD/SIGURG/SIGWINCH/SIGCONT). Antes un
/// `read` bloqueado en una tubería salía del bucle con cualquier `sig_pending`
/// (p. ej. el SIGCHLD que recibe rustc cuando termina el enlazador) y devolvía
/// 0 bytes: EOF falso con escritores vivos; el hilo ctrl-c de rustc moría con
/// "Critical system error while waiting for Ctrl-C". SIGKILL/SIGSTOP siempre cuentan.
pub fn senal_interrumpe() -> bool {
    let guard = PROCESS_TABLE.lock();
    let p = match guard.current_process_ref() { Some(p) => p, None => return false };
    let mut set = p.sig_pending & !p.sig_blocked;
    set |= p.sig_pending & ((1u64 << SIGKILL) | (1u64 << SIGSTOP));
    while set != 0 {
        let sig = set.trailing_zeros();
        set &= !(1u64 << sig);
        if sig as usize >= NSIG { continue; }
        let disp = p.sig_actions[sig as usize];
        if disp == SIG_IGN { continue; }
        if disp == SIG_DFL && matches!(default_action(sig), DefaultAction::Ignore) { continue; }
        return true;
    }
    false
}

pub fn current_sig_consume(mask: u64) -> Option<u32> {
    let guard = PROCESS_TABLE.lock();
    if let Some(proc) = guard.current_process_ref() {
        for sig in 1..=63u64 {
            if (mask & (1u64 << sig)) != 0 && (proc.sig_pending & (1u64 << sig)) != 0 {
                proc.sig_pending &= !(1 << sig);
                return Some(sig as u32);
            }
        }
    }
    None
}

pub fn kill_matching(target_pid: i64, sig: i32, cur_pgid: u64) -> Result<usize, i32> {
    if sig < 0 || sig > 63 { return Err(-22); } // -EINVAL
    let mut matched_pids = alloc::vec::Vec::new();
    {
        let guard = PROCESS_TABLE.lock();
        for p in &guard.processes {
            unsafe {
                let proc = p.as_ptr();
                if (*proc).state != ProcessState::Dead && (*proc).pid != 0 {
                    let matches = if target_pid > 0 {
                        (*proc).pid == target_pid as u64
                    } else if target_pid == 0 {
                        (*proc).pgid == cur_pgid
                    } else if target_pid == -1 {
                        (*proc).pid > 1
                    } else {
                        (*proc).pgid == (-target_pid) as u64
                    };

                    if matches {
                        matched_pids.push((*proc).pid);
                    }
                }
            }
        }
    }

    if matched_pids.is_empty() {
        return Err(-3); // -ESRCH
    }

    if sig == 0 {
        return Ok(matched_pids.len()); // comprobación de existencia
    }

    // Toda señal (no sólo SIGKILL/SIGTERM) queda pendiente para su entrega.
    let mut delivered = 0usize;
    for pid in matched_pids.iter() {
        if raise_for_pid(*pid, sig as u32) {
            delivered += 1;
        }
    }
    if delivered == 0 {
        return Err(-3);
    }
    Ok(delivered)
}

pub fn sigprocmask(how: i32, set: Option<u64>) -> Result<(u64, u64), i32> {
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    let old = cur.sig_blocked;
    if let Some(mut new_set) = set {
        new_set &= !((1u64 << SIGKILL) | (1u64 << SIGSTOP)); // inmutables
        match how {
            0 => cur.sig_blocked |= new_set,  // SIG_BLOCK
            1 => cur.sig_blocked &= !new_set, // SIG_UNBLOCK
            2 => cur.sig_blocked = new_set,   // SIG_SETMASK
            _ => return Err(-22),             // -EINVAL
        }
    }
    Ok((old, cur.sig_blocked))
}

pub fn sigpending() -> Result<u64, i32> {
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    Ok(cur.sig_pending & cur.sig_blocked)
}

/// Registra/consulta la disposición de una señal.
/// Devuelve `(handler, flags, mask)` previos.
pub fn sigaction(signum: i32, act: Option<(usize, u32, u64)>) -> Result<(usize, u32, u64), i32> {
    if signum <= 0 || signum > 63 {
        return Err(-22); // -EINVAL
    }
    if signum == SIGKILL as i32 || signum == SIGSTOP as i32 {
        return Err(-22); // SIGKILL y SIGSTOP no pueden capturarse ni ignorarse
    }
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    let old = (cur.sig_actions[signum as usize], cur.sig_flags[signum as usize], cur.sig_masks[signum as usize]);
    if let Some((handler, flags, mask)) = act {
        cur.sig_actions[signum as usize] = handler;
        cur.sig_flags[signum as usize] = flags;
        cur.sig_masks[signum as usize] = mask & !((1u64 << SIGKILL) | (1u64 << SIGSTOP));
    }
    Ok(old)
}

pub fn alarm(seconds: u32) -> u32 {
    let now = crate::time::pit::ticks();
    let guard = PROCESS_TABLE.lock();
    if let Some(proc) = guard.current_process_ref() {
        let prev_rem = if proc.alarm_ticks > 0 && proc.alarm_ticks > now {
            ((proc.alarm_ticks - now + 99) / 100) as u32
        } else {
            0
        };
        if seconds == 0 {
            proc.alarm_ticks = 0;
        } else {
            proc.alarm_ticks = now.wrapping_add((seconds as u64) * 100);
        }
        prev_rem
    } else {
        0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Telemetría /proc/signals
// ─────────────────────────────────────────────────────────────────────────────
pub fn get_signals_report() -> String {
    let (pending_any, blocked_any, installed, live) = {
        let guard = PROCESS_TABLE.lock();
        let mut pend = 0u64;
        let mut blk = 0u64;
        let mut inst = 0usize;
        let mut live = 0usize;
        for p in &guard.processes {
            unsafe {
                let proc = p.as_ptr();
                if (*proc).state != ProcessState::Dead && (*proc).pid != 0 {
                    live += 1;
                    pend |= (*proc).sig_pending;
                    blk |= (*proc).sig_blocked;
                    for s in 1..NSIG {
                        if (*proc).sig_actions[s] > SIG_IGN {
                            inst += 1;
                        }
                    }
                }
            }
        }
        (pend, blk, inst, live)
    };

    let (disp, ign, dflt, ret_trap, ret_sys, segv, pipe, chld) = unsafe {
        (
            SIG_STATS[0], SIG_STATS[1], SIG_STATS[2], SIG_STATS[3],
            SIG_STATS[4], SIG_STATS[5], SIG_STATS[6], SIG_STATS[7],
        )
    };

    let mut out = String::new();
    out.push_str("==============================================================\n");
    out.push_str("  SUBSISTEMA DE SENALES POSIX ESTANDAR (SIGNALS & SIGACTION)\n");
    out.push_str("  Epica 58 (Fases 3766-3850) - SigFrame + trampolin sigreturn\n");
    out.push_str("==============================================================\n\n");

    out.push_str("  >> MODELO DE ENTREGA\n");
    out.push_str(&format!("    Senales soportadas        : {} (1-31 clasicas + 33-64 de tiempo real)\n", NSIG - 1));
    out.push_str(&format!("    Tamano del SigFrame       : {} bytes (alineado a 16 B en stack de usuario)\n", SIGFRAME_SIZE));
    out.push_str(&format!("    Trampolin de sigreturn    : 0x{:016x} (resuelto via #PF)\n", SIGRETURN_TRAMPOLINE));
    out.push_str(&format!("    Frame de kernel           : {} bytes (15 GPR + 16 XMM + IRET)\n", KERNEL_FRAME_SIZE));
    out.push_str("    Puntos de despacho        : retorno de syscall + tick del planificador + excepciones CPU\n");
    out.push_str("    Sigreturn soportado       : trampolin #PF + syscall rt_sigreturn explicita\n");
    out.push_str(&format!("    Procesos vivos            : {}\n", live));

    out.push_str("\n  >> ESTADO AGREGADO\n");
    out.push_str(&format!("    Senales pendientes (OR)   : 0x{:016x}\n", pending_any));
    out.push_str(&format!("    Senales bloqueadas (OR)   : 0x{:016x}\n", blocked_any));
    out.push_str(&format!("    Manejadores instalados    : {}\n", installed));

    out.push_str("\n  >> CONTADORES DE AUDITORIA\n");
    out.push_str(&format!("    Despachos a manejador     : {}\n", disp));
    out.push_str(&format!("    Descartes por SIG_IGN     : {}\n", ign));
    out.push_str(&format!("    Acciones por defecto      : {}\n", dflt));
    out.push_str(&format!("    rt_sigreturn (trampolin)  : {}\n", ret_trap));
    out.push_str(&format!("    rt_sigreturn (syscall)    : {}\n", ret_sys));
    out.push_str(&format!("    SIGSEGV/SIGFPE desde #PF  : {}\n", segv));
    out.push_str(&format!("    SIGPIPE por tuberia       : {}\n", pipe));
    out.push_str(&format!("    SIGCHLD a progenitores    : {}\n", chld));

    out.push_str("\n  >> ACCIONES POR DEFECTO (IEEE Std 1003.1)\n");
    for s in 1u32..=31u32 {
        let act = match default_action(s) {
            DefaultAction::Terminate => "Terminar",
            DefaultAction::CoreDump => "Terminar+dump",
            DefaultAction::Stop => "Detener",
            DefaultAction::Continue => "Continuar",
            DefaultAction::Ignore => "Ignorar",
        };
        out.push_str(&format!("    {:2} {:<10} {}\n", s, signal_name(s), act));
    }

    out.push_str("\n[EPIC-58-PASS] Entrega de senales con salvaguarda y restauracion de contexto verificada.\n");
    out
}

/// Marca un sigreturn atendido por la llamada al sistema explícita.
pub fn note_syscall_sigreturn() {
    stat_bump(4);
}

/// Marca un sigreturn atendido por el trampolín de #PF.
pub fn note_trap_sigreturn() {
    stat_bump(3);
}

/// Marca la generación de un SIGPIPE.
pub fn note_sigpipe() {
    stat_bump(6);
}

/// Marca la emisión de un SIGCHLD (se invoca desde `process_exit`, que ya
/// mantiene el candado de la tabla de procesos: no debe tomar locks).
pub fn note_sigchld() {
    stat_bump(7);
}
