//! [T-005] Futex real: colas de espera por (espacio de direcciones, dirección).
//!
//! Antes `FUTEX_WAIT` cedía el procesador una vez y devolvía 0 (un despertar
//! espurio permanente) y `FUTEX_WAKE` devolvía 0 sin despertar a nadie. Con
//! hilos de verdad eso es un bucle de espera activa en el mejor caso y un
//! interbloqueo en el peor (`pthread_join` espera a que el núcleo despierte el
//! futex de `tid` al morir el hilo).
//!
//! Modelo: tabla fija de esperadores `(pid, cr3, uaddr, bitset, despertado,
//! plazo)`. `WAIT` compara el valor, se inscribe y se bloquea con reinicio
//! transparente de la syscall; al volver a entrar, si su entrada está marcada
//! `despertado` devuelve 0; si no, decide entre `EINTR` (señal), `ETIMEDOUT`
//! (plazo) o volver a dormir. `WAKE` marca hasta `n` entradas de la misma
//! clave y las pone `Ready`. Clave = `(cr3, uaddr)`: los hilos de un proceso
//! comparten CR3, así que comparten futexes; procesos distintos con la misma
//! dirección virtual no se ven (futexes privados, que es lo que usan musl y
//! glibc para hilos).

use crate::arch::Spinlock;
use crate::process::{self, ProcessState};

pub const MAX_ESPERADORES: usize = 128;

#[derive(Clone, Copy)]
pub struct Esperador {
    pub pid: u64,
    pub cr3: u64,
    pub uaddr: usize,
    pub bitset: u32,
    pub despertado: bool,
    /// tick absoluto del PIT en que vence, 0 = sin plazo
    pub plazo: u64,
}

const VACIO: Esperador = Esperador { pid: 0, cr3: 0, uaddr: 0, bitset: 0, despertado: false, plazo: 0 };

static TABLA: Spinlock<[Esperador; MAX_ESPERADORES]> = Spinlock::new([VACIO; MAX_ESPERADORES]);

/// Estadísticas verificables desde fuera (`[FUTEX] ...` en la serie).
static DESPERTARES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static BLOQUEOS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

pub fn estadisticas() -> (usize, usize) {
    (BLOQUEOS.load(core::sync::atomic::Ordering::Relaxed), DESPERTARES.load(core::sync::atomic::Ordering::Relaxed))
}

fn buscar(tabla: &[Esperador; MAX_ESPERADORES], pid: u64) -> Option<usize> {
    tabla.iter().position(|e| e.pid == pid && pid != 0)
}

/// Resultado de un intento de espera.
pub enum Espera {
    /// Devolver este valor al usuario sin bloquear.
    Inmediato(i64),
    /// Bloquear al proceso actual (ya inscrito) hasta `plazo` (0 = sin plazo).
    Bloquear { plazo: u64 },
}

/// Primera fase de `FUTEX_WAIT`: inscribe (o resuelve) al proceso actual.
/// `valor_actual` es lo que hay en `*uaddr` ahora mismo (lo lee el llamador
/// con `copy_from_user`).
pub fn preparar_espera(pid: u64, cr3: u64, uaddr: usize, val: u32, valor_actual: u32, bitset: u32,
                       plazo: u64, hay_senal: bool, ahora: u64) -> Espera {
    let mut tabla = TABLA.lock();
    if let Some(i) = buscar(&tabla, pid) {
        // Reentrada tras un despertar: ¿fue un WAKE de verdad?
        let e = tabla[i];
        if e.despertado {
            tabla[i] = VACIO;
            return Espera::Inmediato(0);
        }
        if hay_senal {
            tabla[i] = VACIO;
            return Espera::Inmediato(-4); // -EINTR
        }
        if e.plazo != 0 && ahora >= e.plazo {
            tabla[i] = VACIO;
            return Espera::Inmediato(-110); // -ETIMEDOUT
        }
        // Despertar espurio (tick del temporizador): si el valor cambió ya no
        // tiene sentido seguir durmiendo (EAGAIN hace que la libc lo relea).
        if valor_actual != val {
            tabla[i] = VACIO;
            return Espera::Inmediato(-11); // -EAGAIN
        }
        return Espera::Bloquear { plazo: e.plazo };
    }
    if valor_actual != val {
        return Espera::Inmediato(-11); // -EAGAIN
    }
    if plazo != 0 && ahora >= plazo {
        return Espera::Inmediato(-110);
    }
    match tabla.iter().position(|e| e.pid == 0) {
        Some(i) => {
            tabla[i] = Esperador { pid, cr3, uaddr, bitset, despertado: false, plazo };
            BLOQUEOS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            Espera::Bloquear { plazo }
        }
        None => Espera::Inmediato(-11), // tabla llena: que la libc reintente
    }
}

/// `true` si la entrada del proceso ya fue despertada por un WAKE (se usa
/// en el bucle de espera en núcleo cuando no hay otro proceso listo).
pub fn fue_despertado(pid: u64) -> bool {
    let tabla = TABLA.lock();
    match buscar(&tabla, pid) {
        Some(i) => tabla[i].despertado,
        None => true,
    }
}

/// [T-018] ¿Tiene `pid` una inscripción viva (reentrada tras dormir)?
pub fn inscrito(pid: u64) -> bool {
    let tabla = TABLA.lock();
    buscar(&tabla, pid).is_some()
}

/// Retira la inscripción del proceso (salida, señal fatal) devolviendo si
/// había sido despertado.
pub fn retirar(pid: u64) -> bool {
    let mut tabla = TABLA.lock();
    if let Some(i) = buscar(&tabla, pid) {
        let d = tabla[i].despertado;
        tabla[i] = VACIO;
        return d;
    }
    false
}

/// `FUTEX_WAKE`/`FUTEX_WAKE_BITSET`: despierta hasta `n` esperadores de
/// `(cr3, uaddr)` cuyo bitset intersecte con `mascara`. Devuelve cuántos.
pub fn despertar(cr3: u64, uaddr: usize, n: u32, mascara: u32) -> i64 {
    if n == 0 { return 0; }
    let mut pids = [0u64; MAX_ESPERADORES];
    let mut k = 0usize;
    {
        let mut tabla = TABLA.lock();
        for e in tabla.iter_mut() {
            if k as u32 >= n { break; }
            if e.pid != 0 && e.cr3 == cr3 && e.uaddr == uaddr && !e.despertado && (e.bitset & mascara) != 0 {
                e.despertado = true;
                pids[k] = e.pid;
                k += 1;
            }
        }
    }
    for &pid in pids.iter().take(k) {
        poner_listo(pid);
    }
    DESPERTARES.fetch_add(k, core::sync::atomic::Ordering::Relaxed);
    k as i64
}

/// `FUTEX_REQUEUE`/`FUTEX_CMP_REQUEUE`: despierta hasta `n` de `uaddr` y
/// traslada hasta `n2` esperadores restantes a `uaddr2`. Devuelve los
/// despertados (semántica de `futex(2)` para REQUEUE sin flag de valor).
pub fn reencolar(cr3: u64, uaddr: usize, n: u32, uaddr2: usize, n2: u32) -> i64 {
    let despiertos = despertar(cr3, uaddr, n, !0);
    let mut movidos = 0u32;
    let mut tabla = TABLA.lock();
    for e in tabla.iter_mut() {
        if movidos >= n2 { break; }
        if e.pid != 0 && e.cr3 == cr3 && e.uaddr == uaddr && !e.despertado {
            e.uaddr = uaddr2;
            movidos += 1;
        }
    }
    despiertos + movidos as i64
}

/// [T-018] `FUTEX_UNLOCK_PI`: elige al primer esperador de `(cr3, uaddr)` y lo
/// marca despertado SIN ponerlo listo todavía (el que desbloquea debe escribir
/// antes su tid en la palabra del futex, o el elegido podría correr en otra
/// CPU, ver que no es dueño y fallar). Devuelve `(pid, quedan_mas_esperando)`.
pub fn elegir_pi(cr3: u64, uaddr: usize) -> Option<(u64, bool)> {
    let mut tabla = TABLA.lock();
    let mut elegido = 0u64;
    let mut quedan = false;
    for e in tabla.iter_mut() {
        if e.pid != 0 && e.cr3 == cr3 && e.uaddr == uaddr && !e.despertado {
            if elegido == 0 { e.despertado = true; elegido = e.pid; } else { quedan = true; break; }
        }
    }
    if elegido == 0 { None } else { Some((elegido, quedan)) }
}

/// [T-018] Segunda mitad de `elegir_pi`: el elegido pasa a `Ready`.
pub fn listo_pi(pid: u64) {
    poner_listo(pid);
    DESPERTARES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

fn poner_listo(pid: u64) {
    if let Some(guard) = process::PROCESS_TABLE.try_lock() {
        for p in &guard.processes {
            let ptr = p.as_ptr() as *mut process::Process;
            unsafe {
                if (*ptr).pid == pid && (*ptr).state == ProcessState::Blocked {
                    (*ptr).state = ProcessState::Ready;
                    (*ptr).sleep_until = 0;
                }
            }
        }
    } else {
        // Reentrada con el cerrojo tomado: se accede igual, sólo se toca `state`.
        unsafe {
            let tabla = &*process::PROCESS_TABLE.as_mut_ptr();
            for p in &tabla.processes {
                let ptr = p.as_ptr() as *mut process::Process;
                if (*ptr).pid == pid && (*ptr).state == ProcessState::Blocked {
                    (*ptr).state = ProcessState::Ready;
                    (*ptr).sleep_until = 0;
                }
            }
        }
    }
}
