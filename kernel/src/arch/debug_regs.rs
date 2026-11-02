// kernel/src/arch/debug_regs.rs — Registros de depuración x86-64 (ciclo v6.x)
//
// Los registros DR0–DR3 permiten vigilancia **por hardware** de hasta cuatro
// direcciones de datos: la CPU compara cada acceso a memoria del programa con
// los valores programados y, si coinciden, dispara la excepción #DB (vector 1)
// *sin* modificar el programa vigilado y con el RIP exactamente después de la
// instrucción que realizó el acceso.
//
// El kernel programa los registros en cada cambio de contexto (son por CPU, no
// por proceso), igual que hace un sistema operativo de producción.

use core::arch::asm;

pub const MAX_WATCH: usize = 4;

/// Punto de vigilancia de datos.
#[derive(Clone, Copy)]
pub struct WatchPoint {
    pub addr: u64,
    /// Código de longitud del registro DR7: 0=1 B, 1=2 B, 3=4 B, 2=8 B
    pub len_code: u8,
    /// Código de acceso: 1 = escritura, 2 = E/S, 3 = lectura/escritura
    pub rw: u8,
}

impl WatchPoint {
    pub const fn empty() -> Self {
        WatchPoint { addr: 0, len_code: 0, rw: 0 }
    }
}

/// Código de acceso para un punto de parada de EJECUCIÓN (R/W = 00). Es el
/// único caso en que el #DB se produce ANTES de ejecutar la instrucción, lo que
/// permite reanudar sin parchear código; exige LEN = 00 (1 byte).
pub const RW_EXEC: u8 = 0;

/// Longitud en bytes → código DR7 (0,1,2,3) si es representable por hardware.
pub fn len_to_code(len: u64) -> Option<u8> {
    match len {
        1 => Some(0),
        2 => Some(1),
        4 => Some(3),
        8 => Some(2),
        _ => None,
    }
}

pub fn code_to_len(code: u8) -> u64 {
    match code {
        0 => 1,
        1 => 2,
        3 => 4,
        2 => 8,
        _ => 1,
    }
}

#[inline]
fn write_dr(n: usize, val: u64) {
    unsafe {
        match n {
            0 => asm!("mov dr0, {}", in(reg) val, options(nostack, preserves_flags)),
            1 => asm!("mov dr1, {}", in(reg) val, options(nostack, preserves_flags)),
            2 => asm!("mov dr2, {}", in(reg) val, options(nostack, preserves_flags)),
            _ => asm!("mov dr3, {}", in(reg) val, options(nostack, preserves_flags)),
        }
    }
}

#[inline]
fn write_dr7(val: u64) {
    unsafe { asm!("mov dr7, {}", in(reg) val, options(nostack, preserves_flags)); }
}

#[inline]
fn read_dr6() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, dr6", out(reg) v, options(nostack, preserves_flags)); }
    v
}

/// Limpia el estado del #DB (bits B0-B3 y BS). El CPU no lo hace solo.
pub fn clear_dr6() -> u64 {
    let prev = read_dr6();
    unsafe { asm!("mov dr6, {}", in(reg) 0u64, options(nostack, preserves_flags)); }
    prev
}

/// Desactiva toda la vigilancia (contexto saliente sin watchpoints).
pub fn disable_all() {
    write_dr7(1u64 << 10); // bit reservado 10 = 1, todos los enables a 0
    clear_dr6();
}

/// Programa DR0–DR3 y DR7 para el conjunto indicado.
/// `slots[i] == None` desactiva esa ranura.
pub fn apply(slots: &[Option<WatchPoint>; MAX_WATCH]) -> usize {
    let mut dr7 = 1u64 << 10; // reserved (must be 1), LE/GE a 0: exactitud local
    let mut activos = 0usize;
    for i in 0..MAX_WATCH {
        match slots[i] {
            Some(w) => {
                write_dr(i, w.addr);
                dr7 |= 1u64 << (i * 2);                        // Ln: habilitar localmente
                dr7 |= ((w.rw & 3) as u64) << (16 + i * 4);    // R/Wn
                dr7 |= ((w.len_code & 3) as u64) << (18 + i * 4); // LENn
                activos += 1;
            }
            None => write_dr(i, 0),
        }
    }
    write_dr7(dr7);
    clear_dr6();
    activos
}

/// Carga el estado de vigilancia del proceso que va a ejecutarse.
pub fn apply_for(proc: &crate::process::Process) {
    if proc.watch_count == 0 {
        disable_all();
        return;
    }
    let mut slots: [Option<WatchPoint>; MAX_WATCH] = [None; MAX_WATCH];
    let n = (proc.watch_count as usize).min(MAX_WATCH);
    for i in 0..n {
        slots[i] = Some(WatchPoint {
            addr: proc.watch_addr[i],
            len_code: proc.watch_len[i],
            rw: proc.watch_rw[i],
        });
    }
    apply(&slots);
}

/// Instantánea de los registros de depuración: (DR0, DR1, DR2, DR3, DR6, DR7).
/// Se emplea para verificar que la programación del hardware es real.
pub fn snapshot() -> (u64, u64, u64, u64, u64, u64) {
    let (d0, d1, d2, d3, d6, d7): (u64, u64, u64, u64, u64, u64);
    unsafe {
        asm!("mov {}, dr0", out(reg) d0, options(nostack, preserves_flags));
        asm!("mov {}, dr1", out(reg) d1, options(nostack, preserves_flags));
        asm!("mov {}, dr2", out(reg) d2, options(nostack, preserves_flags));
        asm!("mov {}, dr3", out(reg) d3, options(nostack, preserves_flags));
        asm!("mov {}, dr6", out(reg) d6, options(nostack, preserves_flags));
        asm!("mov {}, dr7", out(reg) d7, options(nostack, preserves_flags));
    }
    (d0, d1, d2, d3, d6, d7)
}

/// Ranura del hardware que disparó el #DB (bits B0-B3 de DR6).
pub fn fired_slot(dr6: u64) -> Option<usize> {
    for i in 0..MAX_WATCH {
        if dr6 & (1u64 << i) != 0 {
            return Some(i);
        }
    }
    None
}

/// ¿La ranura `i` es un punto de parada de ejecución (se dispara ANTES de
/// ejecutar la instrucción)?
pub fn slot_is_exec(proc: &crate::process::Process, i: usize) -> bool {
    i < (proc.watch_count as usize).min(MAX_WATCH) && (proc.watch_rw[i] & 3) == RW_EXEC
}

#[inline]
fn read_dr7() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, dr7", out(reg) v, options(nostack, preserves_flags)); }
    v
}

/// Deshabilita temporalmente la ranura `i` (bit Ln = 0) conservando su
/// dirección, su longitud y su código de acceso. Es lo que permite "pisar" un
/// punto de parada de ejecución para ejecutar UNA instrucción (auto-continúo) y
/// volver a armarlo después.
pub fn disable_slot(i: usize) {
    if i >= MAX_WATCH { return; }
    let dr7 = read_dr7() & !(1u64 << (i * 2));
    write_dr7(dr7);
}

/// ¿El #DB provino de un punto de vigilancia de datos (y no de un trap de TF)?
pub fn dr6_is_data_breakpoint(dr6: u64) -> bool {
    (dr6 & 0xF) != 0
}
