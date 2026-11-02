//! [T-025 E3a] Estado por CPU accesible por `gs:` (ver `isr.asm`, macros
//! `SWAPGS_*`). Los desplazamientos de los tres primeros campos son ABI con el
//! ensamblador: NO reordenar.
//!
//! - En Ring 0, `GS_BASE` apunta al `PerCpu` de la CPU actual y
//!   `KERNEL_GS_BASE` guarda el GS del usuario (0).
//! - En Ring 3 es al revés; `swapgs` los intercambia en cada entrada/salida.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const MSR_GS_BASE: u32 = 0xC000_0101;
const MSR_KERNEL_GS_BASE: u32 = 0xC000_0102;
pub const MAX_CPUS: usize = crate::arch::smp::MAX_CORES;

#[repr(C, align(64))]
pub struct PerCpu {
    /// +0  dirección de este mismo bloque (lo lee `actual()` con `mov gs:[0]`).
    pub propio: u64,
    /// +8  cima de la pila de núcleo del proceso en ejecución en esta CPU
    ///     (la lee `syscall_entry`; igual que TSS.RSP0 de esta CPU).
    pub pila_nucleo: AtomicU64,
    /// +16 RSP de usuario guardado por `syscall_entry` (antes era global).
    pub rsp_usuario: u64,
    /// +24 índice lógico de la CPU (0 = BSP).
    pub indice: u64,
    /// +32 APIC ID.
    pub apic_id: u64,
    /// +40 PID del proceso en ejecución en esta CPU (0 = ninguno).
    pub pid_actual: AtomicU64,
    /// +48 [E3b] CS del frame de la última excepción en esta CPU (lo escriben
    ///     las macros ISR_* de `isr.asm`; antes `static mut exception_cs`).
    pub exception_cs: u64,
    /// +56 [E3b] pila del invocador de `enter_ring3_asm` en esta CPU (antes
    ///     `ring3_ret_rsp` global en `isr.asm`).
    pub ring3_ret_rsp: u64,
    /// +64 [E3b] ticks del temporizador LAPIC de esta CPU (sólo AP; el BSP
    ///     sigue con el PIT como reloj de planificación).
    pub ticks_lapic: AtomicU64,
    /// +72 [E3c] RDI original al entrar en una excepción con código de error
    ///     (ISR_ERR lo guarda antes del `pop rdi`; antes `crash_frame+72`, global
    ///     y por tanto una carrera entre dos CPUs con #PF a la vez).
    pub rdi_excepcion: u64,
    /// +80 [E3c] CR3 cargado en esta CPU (lo mantiene `paging::write_cr3`): sirve
    ///     para difundir el TLB shootdown sólo a quien comparte el espacio.
    pub cr3_actual: AtomicU64,
    /// +88 `wait_hlt_preemptible` en curso en esta CPU (IRQ0 puede expropiar en Ring 0).
    pub preempt_ok: AtomicBool,
    pub instalado: AtomicBool,
    /// `schedule_tick` invocado desde una syscall (no desde IRQ0): el frame es el
    /// de la syscall (RAX = número, no el RAX del usuario) y las señales las
    /// entrega el propio retorno de la syscall con el resultado correcto.
    pub en_syscall: AtomicBool,
    /// [E3d] Proceso cuyo contexto esta CPU acaba de abandonar y que aún no
    /// puede reclamar otra CPU: su `en_cpu` se limpia en
    /// `liberar_contexto_saliente`, YA SOBRE LA PILA DEL ENTRANTE. Antes se
    /// limpiaba dentro de `schedule_tick`, con el epílogo de la función y el
    /// `mov rsp` del stub todavía pendientes sobre la pila del saliente: otra
    /// CPU lo reanudaba (si dormía en el núcleo, p. ej. `wait_hlt_preemptible`)
    /// y las dos CPU escribían la misma pila (#UD con RIP dentro de la pila).
    pub saliente: AtomicU64,
    /// +104 [E3d] `CrashFrame` de la última excepción EN ESTA CPU (160 B; lo
    ///     escribe `CAPTURE_FRAME` de `isr.asm` por `gs:`). Antes era el
    ///     `static crash_frame` global: dos CPU con #PF a la vez se pisaban y la
    ///     señal de una se entregaba con el RIP/RSP de la otra (la JVM en el
    ///     bucle de NPE implícitas del AP y C1 en el BSP: RIP=0x3000000010).
    pub crash_frame: [u64; 20],
}
pub const OFF_CRASH_FRAME: usize = 104;
const _: () = assert!(core::mem::offset_of!(PerCpu, crash_frame) == OFF_CRASH_FRAME);
const _: () = assert!(core::mem::offset_of!(PerCpu, rdi_excepcion) == 72);

impl PerCpu {
    const fn nuevo() -> Self {
        Self {
            propio: 0, pila_nucleo: AtomicU64::new(0), rsp_usuario: 0, indice: 0, apic_id: 0,
            pid_actual: AtomicU64::new(0), exception_cs: 0, ring3_ret_rsp: 0,
            ticks_lapic: AtomicU64::new(0), rdi_excepcion: 0, cr3_actual: AtomicU64::new(0),
            preempt_ok: AtomicBool::new(false),
            instalado: AtomicBool::new(false), en_syscall: AtomicBool::new(false),
            saliente: AtomicU64::new(0),
            crash_frame: [0; 20],
        }
    }
}

macro_rules! bloques { () => { [
    PerCpu::nuevo(), PerCpu::nuevo(), PerCpu::nuevo(), PerCpu::nuevo(),
    PerCpu::nuevo(), PerCpu::nuevo(), PerCpu::nuevo(), PerCpu::nuevo(),
    PerCpu::nuevo(), PerCpu::nuevo(), PerCpu::nuevo(), PerCpu::nuevo(),
    PerCpu::nuevo(), PerCpu::nuevo(), PerCpu::nuevo(), PerCpu::nuevo(),
] } }

/// Veces que `actual()` encontró un GS que no apunta a ningún bloque.
pub static GS_INVALIDO: AtomicU64 = AtomicU64::new(0);
pub static GS_INVALIDO_ULTIMO: AtomicU64 = AtomicU64::new(0);
pub static GS_INVALIDO_P: AtomicU64 = AtomicU64::new(0);
pub static GS_INVALIDO_APIC: AtomicU64 = AtomicU64::new(0);
pub static GS_INVALIDO_RSP: AtomicU64 = AtomicU64::new(0);

static mut BLOQUES: [PerCpu; MAX_CPUS] = bloques!();

#[inline(always)]
unsafe fn wrmsr(msr: u32, v: u64) {
    core::arch::asm!("wrmsr", in("ecx") msr, in("eax") v as u32, in("edx") (v >> 32) as u32,
                     options(nostack, nomem, preserves_flags));
}

/// Instala el bloque de la CPU `indice` en GS_BASE (núcleo) y pone
/// KERNEL_GS_BASE = 0 (GS del usuario). Debe llamarse en Ring 0, en cada CPU,
/// antes de cualquier uso de `actual()` y antes del primer `iretq` a Ring 3.
pub fn instalar(indice: usize, apic_id: u32) {
    let i = indice.min(MAX_CPUS - 1);
    unsafe {
        let b = core::ptr::addr_of_mut!(BLOQUES[i]);
        (*b).propio = b as u64;
        (*b).indice = i as u64;
        (*b).apic_id = apic_id as u64;
        (*b).instalado.store(true, Ordering::SeqCst);
        wrmsr(MSR_GS_BASE, b as u64);
        wrmsr(MSR_KERNEL_GS_BASE, 0);
    }
}

/// Bloque de la CPU que ejecuta esta función. Sólo válido en Ring 0 con GS ya
/// instalado; si `gs:[0]` no coincide con ningún bloque (arranque muy
/// temprano) devuelve el del BSP.
#[inline]
pub fn actual() -> &'static PerCpu {
    let p: u64;
    unsafe { core::arch::asm!("mov {}, gs:[0]", out(reg) p, options(nostack, readonly, preserves_flags)); }
    let base = core::ptr::addr_of!(BLOQUES) as u64;
    let fin = base + (core::mem::size_of::<PerCpu>() * MAX_CPUS) as u64;
    if p >= base && p < fin && (p - base) % core::mem::size_of::<PerCpu>() as u64 == 0 {
        unsafe { &*(p as *const PerCpu) }
    } else {
        // GS no instalado (arranque temprano: no cuenta) o invariante swapgs
        // roto tras `instalar()`: se contabiliza para que /proc/smp lo delate
        // (la suite exige `gs_invalido: 0`).
        if !unsafe { (*core::ptr::addr_of!(BLOQUES[0])).instalado.load(Ordering::Relaxed) } {
            return unsafe { &*core::ptr::addr_of!(BLOQUES[0]) };
        }
        GS_INVALIDO.fetch_add(1, Ordering::Relaxed);
        unsafe {
            let lo: u32; let hi: u32;
            core::arch::asm!("rdmsr", in("ecx") MSR_GS_BASE, out("eax") lo, out("edx") hi, options(nostack, nomem, preserves_flags));
            GS_INVALIDO_ULTIMO.store(((hi as u64) << 32) | lo as u64, Ordering::Relaxed);
            GS_INVALIDO_P.store(p, Ordering::Relaxed);
            let apic = (core::arch::x86_64::__cpuid(1).ebx >> 24) & 0xFF;
            GS_INVALIDO_APIC.store(apic as u64, Ordering::Relaxed);
            let rsp: u64; core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nostack, nomem, preserves_flags));
            GS_INVALIDO_RSP.store(rsp, Ordering::Relaxed);
            // Diagnóstico (primeras 8 veces): contexto crudo por la serie, sin cerrojo.
            if GS_INVALIDO.load(Ordering::Relaxed) <= 8 {
                let cr3: u64; core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nostack, nomem, preserves_flags));
                let ret: u64; core::arch::asm!("mov {}, [rsp+8]", out(reg) ret, options(nostack, readonly, preserves_flags));
                let ret2: u64; core::arch::asm!("mov {}, [rsp+16]", out(reg) ret2, options(nostack, readonly, preserves_flags));
                let ret3: u64; core::arch::asm!("mov {}, [rsp+24]", out(reg) ret3, options(nostack, readonly, preserves_flags));
                let hex = b"0123456789abcdef";
                let emitir = |tag: &[u8], v: u64| {
                    for &c in tag { crate::drivers::serial::write_byte(c); }
                    for sh in (0..16).rev() { crate::drivers::serial::write_byte(hex[((v >> (sh * 4)) & 0xF) as usize]); }
                };
                emitir(b"[GS!] rsp=", rsp); emitir(b" cr3=", cr3); emitir(b" s8=", ret); emitir(b" s16=", ret2); emitir(b" s24=", ret3);
                crate::drivers::serial::write_byte(b'\n');
            }
        }
        unsafe { &*core::ptr::addr_of!(BLOQUES[0]) }
    }
}

/// `actual()` sin tocar GS: busca por APIC ID (para contextos donde GS pueda
/// no estar instalado todavía, p. ej. el bucle ocioso de un AP recién llegado).
pub fn por_indice(i: usize) -> &'static PerCpu {
    unsafe { &*core::ptr::addr_of!(BLOQUES[i.min(MAX_CPUS - 1)]) }
}

/// [E3b] Comprobación en tiempo de compilación de los desplazamientos que usa `isr.asm`.
const _: () = {
    assert!(core::mem::offset_of!(PerCpu, pila_nucleo) == 8);
    assert!(core::mem::offset_of!(PerCpu, rsp_usuario) == 16);
    assert!(core::mem::offset_of!(PerCpu, exception_cs) == 48);
    assert!(core::mem::offset_of!(PerCpu, ring3_ret_rsp) == 56);
    assert!(core::mem::offset_of!(PerCpu, ticks_lapic) == 64);
    assert!(core::mem::offset_of!(PerCpu, rdi_excepcion) == 72);
    assert!(core::mem::offset_of!(PerCpu, cr3_actual) == 80);
};

/// [E3c] ¿Hay otra CPU (distinta de la actual) ejecutando el PID `pid`?
/// Un proceso `Ready` que todavía figura como actual en otra CPU está a medio
/// bloquear/expropiar en ella y NO puede tomarse (doble ejecución).
pub fn pid_en_otra_cpu(pid: u64) -> bool {
    let yo = indice_actual();
    for i in 0..MAX_CPUS {
        if i == yo { continue; }
        let b = por_indice(i);
        if b.instalado.load(Ordering::Relaxed) && b.pid_actual.load(Ordering::SeqCst) == pid { return true; }
    }
    false
}

/// [E3c] ¿Alguna otra CPU tiene cargado el CR3 `cr3`? (hilos del mismo proceso
/// corriendo en paralelo: un `munmap`/`mprotect` debe invalidarles el TLB).
pub fn cr3_en_otra_cpu(cr3: u64) -> bool {
    let yo = indice_actual();
    for i in 0..MAX_CPUS {
        if i == yo { continue; }
        let b = por_indice(i);
        if b.instalado.load(Ordering::Relaxed) && b.cr3_actual.load(Ordering::Relaxed) == cr3 { return true; }
    }
    false
}

/// [E3b] `exception_cs` / `ring3_ret_rsp` de la CPU actual (lectura).
pub fn exception_cs_actual() -> u64 { unsafe { core::ptr::read_volatile(core::ptr::addr_of!(actual().exception_cs)) } }
pub fn ring3_ret_rsp_actual() -> u64 { unsafe { core::ptr::read_volatile(core::ptr::addr_of!(actual().ring3_ret_rsp)) } }

#[inline] pub fn indice_actual() -> usize { actual().indice as usize }
#[inline] pub fn pid_actual() -> u64 { actual().pid_actual.load(Ordering::SeqCst) }
#[inline] pub fn fijar_pid_actual(pid: u64) { actual().pid_actual.store(pid, Ordering::SeqCst) }
#[inline] pub fn fijar_pila_nucleo(cima: u64) { actual().pila_nucleo.store(cima, Ordering::SeqCst) }
