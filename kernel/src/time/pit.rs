// kernel/src/pit.rs — PORTIX PIT Timer (IRQ0, 100 Hz)
#![allow(dead_code)]

const PIT_CHANNEL0: u16 = 0x40;
const PIT_CMD:      u16 = 0x43;

// 1_193_182 Hz / 100 = 11931 → ~100 Hz
pub const PIT_HZ: u32 = 100;
const PIT_DIVISOR: u16 = 11932;

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nostack, nomem));
}

use core::sync::atomic::{AtomicU64, Ordering};

/// Global tick counter, incremented each IRQ0.  100 ticks = 1 second.
pub static TICKS: AtomicU64 = AtomicU64::new(0);

// ── [85ª, defecto 20] Ticks del PIT perdidos por el BSP ─────────────────────
// Con la planificación SMP encendida el BSP pasa ratos con IF=0 (esperas a un
// AP, cerrojos) y las interrupciones del PIT que caen en ese hueco se pierden
// (el PIC sólo retiene una). Como CLOCK_MONOTONIC/REALTIME salen de `TICKS`,
// el reloj del sistema se atrasaba (medido: 7-8 % con -smp 2 bajo TCG).
// Aquí se mide cada hueco con el TSC y, si el TSC ya está calibrado, se añaden
// los ticks que faltan para que el reloj no mienta; la pérdida se publica
// (`/proc/smp`: `irq0_perdidos`, `irq0_peor_hueco_us`, `irq0_sitio`).
pub static TSC_ULTIMO_TICK: AtomicU64 = AtomicU64::new(0);
pub static IRQ0_PERDIDOS: AtomicU64 = AtomicU64::new(0);
pub static IRQ0_PEOR_HUECO_US: AtomicU64 = AtomicU64::new(0);
/// pid<<32 | syscall del BSP en el peor hueco (quién tenía IF=0).
pub static IRQ0_SITIO: AtomicU64 = AtomicU64::new(0);
/// Puesto a 1 por `pit_tick` al detectar el peor hueco; `schedule_tick` (misma
/// IRQ, con el frame a mano) guarda entonces el RIP interrumpido en `IRQ0_SITIO_RIP`.
pub static IRQ0_HUECO_NUEVO: AtomicU64 = AtomicU64::new(0);
pub static IRQ0_SITIO_RIP: AtomicU64 = AtomicU64::new(0);

// ── [T-026] Ventanas con IF=0: ¿el hueco lo causó el núcleo o el anfitrión? ──
// Cada syscall corre con IF=0 (la puerta limpia IF y no se vuelve a poner hasta
// regresar a Ring 3; los puntos de respiro `sti;hlt;cli` son la excepción). Por
// eso un hueco del PIT SÓLO es culpa del núcleo si cayó entero dentro de una de
// estas ventanas. Si la ventana estaba cerrada, el invitado no tenía IF=0: el
// vCPU simplemente no recibió CPU del anfitrión (bajo TCG, con el host cargado,
// esto es medible y no se arregla dentro del kernel). Antes esto no se podía
// distinguir: `irq0_sitio` mezclaba el pid actual con la ÚLTIMA syscall de la
// CPU, que podía ser de otro proceso y de otro momento.
const MAX_CPU_VENTANA: usize = crate::arch::percpu::MAX_CPUS;
static V_ENTRADA_TSC: [AtomicU64; MAX_CPU_VENTANA] = [const { AtomicU64::new(0) }; MAX_CPU_VENTANA];
static V_ENTRADA_RIP: [AtomicU64; MAX_CPU_VENTANA] = [const { AtomicU64::new(0) }; MAX_CPU_VENTANA];
static V_ENTRADA_PID: [AtomicU64; MAX_CPU_VENTANA] = [const { AtomicU64::new(0) }; MAX_CPU_VENTANA];
/// nr | (abi << 32): abi 0 = nativa (`int 0x80`), 1 = Linux (`syscall`).
static V_ENTRADA_NR: [AtomicU64; MAX_CPU_VENTANA] = [const { AtomicU64::new(0) }; MAX_CPU_VENTANA];
static V_ENTRADA_A1: [AtomicU64; MAX_CPU_VENTANA] = [const { AtomicU64::new(0) }; MAX_CPU_VENTANA];
static V_ENTRADA_A2: [AtomicU64; MAX_CPU_VENTANA] = [const { AtomicU64::new(0) }; MAX_CPU_VENTANA];
static V_ENTRADA_A3: [AtomicU64; MAX_CPU_VENTANA] = [const { AtomicU64::new(0) }; MAX_CPU_VENTANA];

pub static VENTANAS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static VENTANA_PEOR_US: AtomicU64 = AtomicU64::new(0);
pub static VENTANA_PEOR_PID: AtomicU64 = AtomicU64::new(0);
pub static VENTANA_PEOR_NR: AtomicU64 = AtomicU64::new(0);
pub static VENTANA_PEOR_RIP_IN: AtomicU64 = AtomicU64::new(0);
pub static VENTANA_PEOR_RIP_OUT: AtomicU64 = AtomicU64::new(0);
pub static VENTANA_PEOR_CPU: AtomicU64 = AtomicU64::new(0);
/// Argumentos (a1, a2, a3) de la peor ventana: sin ellos se sabe QUÉ syscall
/// fue pero no sobre qué (un `mmap` de 2 MB no es lo mismo que uno de 128 MB).
pub static VENTANA_PEOR_A1: AtomicU64 = AtomicU64::new(0);
pub static VENTANA_PEOR_A2: AtomicU64 = AtomicU64::new(0);
pub static VENTANA_PEOR_A3: AtomicU64 = AtomicU64::new(0);
/// 1 = el peor hueco del PIT cayó DENTRO de un syscall con IF=0 (culpa del
/// núcleo); 0 = fuera (modo usuario o `hlt`: el anfitrión no dio CPU).
pub static IRQ0_HUECO_DENTRO: AtomicU64 = AtomicU64::new(u64::MAX);
/// Cuánto llevaba abierta la ventana cuando llegó el tick (µs).
pub static IRQ0_HUECO_VENTANA_US: AtomicU64 = AtomicU64::new(0);
/// Tick (uptime × 100 Hz) en el que ocurrió el peor hueco: sirve para situarlo
/// en el log de la consola.
pub static IRQ0_HUECO_TICK: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
fn cpu_ventana() -> usize {
    crate::arch::percpu::indice_actual().min(MAX_CPU_VENTANA - 1)
}

/// Dirección de la instrucción siguiente (para saber DÓNDE se abrió/cerró).
#[inline(always)]
pub fn rip_actual() -> u64 {
    let rip: u64;
    unsafe { core::arch::asm!("lea {}, [rip + 0]", out(reg) rip, options(nomem, nostack)) };
    rip
}

/// Ventana con IF=0 abierta por una syscall. Al soltarse (Drop) se cierra y,
/// si ha sido la más larga, se publica en `/proc/smp`.
pub struct VentanaIf0 {
    cpu: usize,
    activa: bool,
}

/// ¿Están las interrupciones abiertas AHORA en esta CPU?
#[inline(always)]
pub fn if_abierto() -> bool {
    let rf: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) rf, options(nomem, preserves_flags)); }
    rf & (1 << 9) != 0
}

impl VentanaIf0 {
    /// Sólo abre ventana si esta CPU entra con IF=0 (la ABI Linux entra así por
    /// FMASK; la nativa hace `sti` en la puerta y corre con IF=1). Medir lo que
    /// no es IF=0 fue justo el error que hacía inútil `irq0_sitio`.
    #[inline(always)]
    pub fn abrir_si_if0(nr: u64, abi: u64, a1: u64, a2: u64, a3: u64) -> Self {
        if if_abierto() {
            return VentanaIf0 { cpu: MAX_CPU_VENTANA, activa: false };
        }
        Self::abrir_args(nr, abi, a1, a2, a3)
    }

    #[inline(always)]
    pub fn abrir(nr: u64, abi: u64) -> Self {
        Self::abrir_args(nr, abi, 0, 0, 0)
    }

    #[inline(always)]
    pub fn abrir_args(nr: u64, abi: u64, a1: u64, a2: u64, a3: u64) -> Self {
        let cpu = cpu_ventana();
        V_ENTRADA_A1[cpu].store(a1, Ordering::Relaxed);
        V_ENTRADA_A2[cpu].store(a2, Ordering::Relaxed);
        V_ENTRADA_A3[cpu].store(a3, Ordering::Relaxed);
        V_ENTRADA_NR[cpu].store((nr & 0xFFFF_FFFF) | ((abi & 0xF) << 32), Ordering::Relaxed);
        V_ENTRADA_PID[cpu].store(crate::arch::percpu::pid_actual(), Ordering::Relaxed);
        V_ENTRADA_RIP[cpu].store(rip_actual(), Ordering::Relaxed);
        // El TSC se escribe AL FINAL: mientras valga 0 la ventana no cuenta, así
        // un tick que llegue a mitad de los `store` no lee un registro a medias.
        V_ENTRADA_TSC[cpu].store(unsafe { core::arch::x86_64::_rdtsc() }, Ordering::Relaxed);
        VENTANAS_TOTAL.fetch_add(1, Ordering::Relaxed);
        VentanaIf0 { cpu, activa: true }
    }
}

impl Drop for VentanaIf0 {
    fn drop(&mut self) {
        if !self.activa {
            return;
        }
        let t = V_ENTRADA_TSC[self.cpu].swap(0, Ordering::Relaxed);
        if t == 0 {
            return;
        }
        let hz = TSC_HZ.load(Ordering::Relaxed);
        if hz == 0 {
            return;
        }
        let us = unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(t) / (hz / 1_000_000).max(1);
        if us > VENTANA_PEOR_US.load(Ordering::Relaxed) {
            let rip_out = rip_actual();
            VENTANA_PEOR_RIP_OUT.store(rip_out, Ordering::Relaxed);
            VENTANA_PEOR_RIP_IN.store(V_ENTRADA_RIP[self.cpu].load(Ordering::Relaxed), Ordering::Relaxed);
            VENTANA_PEOR_PID.store(V_ENTRADA_PID[self.cpu].load(Ordering::Relaxed), Ordering::Relaxed);
            VENTANA_PEOR_NR.store(V_ENTRADA_NR[self.cpu].load(Ordering::Relaxed), Ordering::Relaxed);
            VENTANA_PEOR_CPU.store(self.cpu as u64, Ordering::Relaxed);
            VENTANA_PEOR_A1.store(V_ENTRADA_A1[self.cpu].load(Ordering::Relaxed), Ordering::Relaxed);
            VENTANA_PEOR_A2.store(V_ENTRADA_A2[self.cpu].load(Ordering::Relaxed), Ordering::Relaxed);
            VENTANA_PEOR_A3.store(V_ENTRADA_A3[self.cpu].load(Ordering::Relaxed), Ordering::Relaxed);
            VENTANA_PEOR_US.store(us, Ordering::Relaxed); // último: da validez al resto
        }
    }
}

/// ¿La ventana de esta CPU estaba abierta antes del tick anterior?
/// Devuelve (dentro, µs_abierta, nr|abi<<32, pid).
fn ventana_cubre(cpu: usize, tsc_tick_anterior: u64, ahora: u64) -> (bool, u64, u64, u64) {
    let entrada = V_ENTRADA_TSC[cpu].load(Ordering::Relaxed);
    if entrada == 0 {
        return (false, 0, 0, 0);
    }
    // La ventana tiene que haberse abierto ANTES del tick anterior para que el
    // hueco entero haya ocurrido con IF=0.
    let dentro = (tsc_tick_anterior.wrapping_sub(entrada) as i64) >= 0;
    let hz = TSC_HZ.load(Ordering::Relaxed);
    let us = if hz == 0 { 0 } else { ahora.wrapping_sub(entrada) / (hz / 1_000_000).max(1) };
    (dentro, us, V_ENTRADA_NR[cpu].load(Ordering::Relaxed), V_ENTRADA_PID[cpu].load(Ordering::Relaxed))
}

/// Called from the IRQ0 stub in isr.asm.
#[no_mangle]
pub extern "C" fn pit_tick() {
    let ahora = unsafe { core::arch::x86_64::_rdtsc() };
    let antes = TSC_ULTIMO_TICK.swap(ahora, Ordering::Relaxed);
    let hz = TSC_HZ.load(Ordering::Relaxed);
    let mut sumar = 1u64;
    if antes != 0 && hz > 0 {
        let por_tick = (hz / PIT_HZ as u64).max(1);
        let hueco = ahora.wrapping_sub(antes);
        // ≥ 1,5 ticks desde el anterior: se perdieron interrupciones.
        if hueco * 2 >= por_tick * 3 {
            let perdidos = (hueco / por_tick).saturating_sub(1).min(1_000_000);
            if perdidos > 0 {
                IRQ0_PERDIDOS.fetch_add(perdidos, Ordering::Relaxed);
                sumar += perdidos;
                let us = hueco / (hz / 1_000_000).max(1);
                if us > IRQ0_PEOR_HUECO_US.load(Ordering::Relaxed) {
                    IRQ0_PEOR_HUECO_US.store(us, Ordering::Relaxed);
                    // [T-026] ¿Estaba el BSP dentro de un syscall (IF=0) o fuera?
                    let cpu = cpu_ventana();
                    let (dentro, abierta_us, nr_abi, pid_ventana) = ventana_cubre(cpu, antes, ahora);
                    IRQ0_HUECO_DENTRO.store(if dentro { 1 } else { 0 }, Ordering::Relaxed);
                    IRQ0_HUECO_VENTANA_US.store(abierta_us, Ordering::Relaxed);
                    if dentro {
                        // El sitio ES la ventana: mismo pid y misma syscall.
                        IRQ0_SITIO.store((pid_ventana << 32) | (nr_abi & 0xFFFF_FFFF), Ordering::Relaxed);
                    } else {
                        let (nr, _) = crate::syscall::ultima_syscall(crate::arch::percpu::indice_actual());
                        IRQ0_SITIO.store((crate::arch::percpu::pid_actual() << 32) | nr as u64, Ordering::Relaxed);
                    }
                    IRQ0_HUECO_NUEVO.store(1, Ordering::Relaxed);
                    IRQ0_HUECO_TICK.store(TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
                    // [T-026] Traza INMEDIATA cuando el hueco es grande: la
                    // posición de esta línea en el log de la consola dice qué
                    // estaba haciendo el sistema en ese preciso instante.
                    if us >= 150_000 {
                        let s = crate::drivers::serial::write_str;
                        let n = crate::drivers::serial::write_usize;
                        s("[IRQ0-HUECO] us="); n(us as usize);
                        s(" perdidos="); n(perdidos as usize);
                        s(" dentro_if0="); s(if dentro { "1" } else { "0" });
                        s(" pid="); n(pid_ventana as usize);
                        s(" nr="); n((nr_abi & 0xFFFF_FFFF) as usize);
                        s(" abi="); s(if (nr_abi >> 32) & 0xF == 1 { "linux" } else { "nativa" });
                        s(" ventana_us="); n(abierta_us as usize);
                        s(" tsc="); n(ahora as usize);
                        s("\n");
                    }
                }
            }
        }
    }
    TICKS.fetch_add(sumar, Ordering::Relaxed);
}

/// Atomically read current tick count.
#[inline(always)]
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Uptime in full seconds.
#[inline(always)]
pub fn uptime_secs() -> u64 { ticks() / PIT_HZ as u64 }

/// Uptime decomposed for display.
pub fn uptime_hms() -> (u32, u32, u32) {
    let s = uptime_secs() as u32;
    (s / 3600, (s % 3600) / 60, s % 60)
}

/// Program channel 0, mode 3 (square wave), 100 Hz.
pub fn init() {
    unsafe {
        // Channel 0 | access lo/hi | mode 3 | binary
        outb(PIT_CMD, 0x36);
        outb(PIT_CHANNEL0, (PIT_DIVISOR & 0xFF) as u8);
        outb(PIT_CHANNEL0, (PIT_DIVISOR >> 8) as u8);
    }
}
/// [CIERRE RED] Estimación de la frecuencia del TSC calibrada contra el PIT:
/// se mide una sola vez (TSC transcurrido durante 10 ticks = 100 ms) y se
/// cachea. 0 si aún no se pudo calibrar (interrupciones deshabilitadas).
pub static TSC_HZ: AtomicU64 = AtomicU64::new(0);
/// Frecuencia del TSC ya calibrada (0 si aún no), sin bloquear.
pub fn tsc_hz_cacheado() -> u64 { TSC_HZ.load(Ordering::Relaxed) }
pub fn tsc_hz_estimate() -> u64 {
    let v = TSC_HZ.load(Ordering::Relaxed);
    if v != 0 { return v; }
    // [85ª, defecto 20] Empezar a contar en un FLANCO del PIT: antes `t0` caía en
    // un punto cualquiera del periodo y los "10 ticks" duraban entre 90 y 100 ms
    // (hasta un 10 % de error sistemático que heredaban `esperar_us`, el
    // temporizador LAPIC de los AP y la medida `reloj` de /proc/smp). Ahora se
    // espera al primer flanco y se miden 20 ticks (200 ms) desde él.
    let mut guard = 0u64;
    let t_ini = ticks();
    while ticks() == t_ini {
        unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
        guard += 1; if guard > 1_000_000 { return 0; }
    }
    let t0 = ticks();
    let c0 = unsafe { core::arch::x86_64::_rdtsc() };
    while ticks().wrapping_sub(t0) < 20 {
        unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
        guard += 1; if guard > 1_000_000 { return 0; }
    }
    let c1 = unsafe { core::arch::x86_64::_rdtsc() };
    let hz = c1.wrapping_sub(c0) * (PIT_HZ as u64 / 20);
    TSC_HZ.store(hz, Ordering::Relaxed);
    hz
}
