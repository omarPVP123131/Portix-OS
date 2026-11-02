//! [T-025 E3d] Cerrojo grande del núcleo (CGN).
//!
//! El núcleo se escribió para una sola CPU: el VFS, ext2, el cargador ELF, la
//! red y la TTY usan estado global sin cerrojo. Con procesos corriendo en los
//! AP (E3c), dos `execve` simultáneos se pisaban los búferes del cargador
//! (`#PF` de instrucción en una página presente del hijo). Hasta que cada
//! subsistema tenga su cerrojo fino, TODA entrada al núcleo desde Ring 3
//! (syscall, fallo de página, salida de proceso) se ejecuta bajo este cerrojo,
//! que se SUELTA en los puntos de espera (`wait_hlt_preemptible`,
//! `schedule_desde_syscall`, bucles `hlt`) para que otra CPU pueda entrar.
//! El código de usuario (cómputo puro) sigue corriendo en paralelo: es el
//! modelo del Linux 2.0 (BKL), honesto y verificable; la suite mide la
//! aceleración y comprueba que nada se rompe.
//!
//! Reentrante por CPU: el dueño es un índice de CPU (+1) y cada CPU lleva su
//! profundidad, así una IRQ o una llamada anidada del mismo núcleo no se
//! bloquea a sí misma.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const LIBRE: u32 = 0;
static DUENO: AtomicU32 = AtomicU32::new(LIBRE);
const MAX: usize = crate::arch::percpu::MAX_CPUS;
#[allow(clippy::declare_interior_mutable_const)]
const CERO: AtomicU32 = AtomicU32::new(0);
static PROFUNDIDAD: [AtomicU32; MAX] = [CERO; MAX];
/// Diagnóstico (/proc/smp): veces que una CPU tuvo que esperar a otra.
pub static ESPERAS: AtomicU64 = AtomicU64::new(0);
pub static TOMAS: AtomicU64 = AtomicU64::new(0);

#[inline]
fn yo() -> u32 { (crate::arch::percpu::indice_actual().min(MAX - 1) as u32) + 1 }

/// Toma el cerrojo (reentrante en la misma CPU).
pub fn tomar() {
    let y = yo();
    if DUENO.load(Ordering::SeqCst) == y {
        PROFUNDIDAD[(y - 1) as usize].fetch_add(1, Ordering::SeqCst);
        return;
    }
    let mut espero = false;
    // Mientras se espera, las interrupciones deben estar ABIERTAS: la CPU
    // dueña puede estar aguardando el acuse de un TLB shootdown (IPI) de ESTA
    // CPU. Un #PF entra por puerta de interrupción (IF=0) y se bloqueaba
    // aquí con la dueña esperándolo: ambas quietas (gcc → as, b198 en pruebas).
    // Se restaura IF al salir, tal como estaba.
    let rflags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) rflags, options(nomem, preserves_flags)); }
    let if_cerrado = rflags & 0x200 == 0;
    while DUENO.compare_exchange_weak(LIBRE, y, Ordering::Acquire, Ordering::Relaxed).is_err() {
        if !espero {
            espero = true;
            if if_cerrado { unsafe { core::arch::asm!("sti", options(nomem, nostack)); } }
        }
        core::hint::spin_loop();
    }
    if espero {
        ESPERAS.fetch_add(1, Ordering::Relaxed);
        if if_cerrado { unsafe { core::arch::asm!("cli", options(nomem, nostack)); } }
    }
    TOMAS.fetch_add(1, Ordering::Relaxed);
    PROFUNDIDAD[(y - 1) as usize].store(1, Ordering::SeqCst);
}

/// Suelta un nivel. Si esta CPU no es la dueña, no hace nada (el nivel ya se
/// soltó en un punto de espera con `soltar_todo`).
pub fn soltar() {
    let y = yo();
    if DUENO.load(Ordering::SeqCst) != y { return; }
    let p = &PROFUNDIDAD[(y - 1) as usize];
    if p.load(Ordering::SeqCst) <= 1 {
        p.store(0, Ordering::SeqCst);
        DUENO.store(LIBRE, Ordering::Release);
    } else {
        p.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Suelta todos los niveles de esta CPU y devuelve cuántos había (0 si no era
/// dueña). Para los puntos de espera: `let d = soltar_todo(); … ; retomar(d);`.
pub fn soltar_todo() -> u32 {
    let y = yo();
    if DUENO.load(Ordering::SeqCst) != y { return 0; }
    let p = &PROFUNDIDAD[(y - 1) as usize];
    let d = p.swap(0, Ordering::SeqCst);
    DUENO.store(LIBRE, Ordering::Release);
    d
}

/// Vuelve a tomar el cerrojo con la profundidad que devolvió `soltar_todo`.
pub fn retomar(d: u32) {
    if d == 0 { return; }
    tomar();
    PROFUNDIDAD[(yo() - 1) as usize].store(d, Ordering::SeqCst);
}

/// Índice de CPU dueña + 1 (0 = libre). Diagnóstico.
pub fn dueno() -> u32 { DUENO.load(Ordering::Relaxed) }

/// ¿Esta CPU es la dueña ahora mismo?
pub fn soy_dueno() -> bool { DUENO.load(Ordering::SeqCst) == yo() }

/// Guard RAII: toma al crearse y suelta un nivel al caer.
pub struct Guard(());
impl Guard {
    pub fn tomar() -> Guard { tomar(); Guard(()) }
}
impl Drop for Guard {
    fn drop(&mut self) { soltar(); }
}
