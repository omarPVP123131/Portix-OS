// userland/bin/wp-demo/src/main.rs — Autoprueba del subsistema de depuración
// (ciclo v6.x): puntos de vigilancia por HARDWARE (DR0-DR3 + #DB), puntos de
// parada INT3 (#BP) con ajuste de RIP y parcheo verificado de código mediante
// PTRACE_POKEDATA sobre el espacio de direcciones del propio proceso.
//
// El binario emite un veredicto explícito para que la suite de certificación
// pueda comprobarlo sin ambigüedad:
//
//     [WATCHPOINT-PASS] / [WATCHPOINT-FAIL]
//     [BREAKPOINT-PASS] / [BREAKPOINT-FAIL]
//     [CODESPATCH-PASS] / [CODESPATCH-FAIL]
//     [DBGSUITE-PASS]   (sólo si las tres anteriores pasan)
#![no_std]
#![no_main]

extern crate alloc;

use core::sync::atomic::{AtomicU64, Ordering};
use portix_rt::syscall::{
    sys_ptrace, sys_getpid,
    PTRACE_SETWATCHPOINT, PTRACE_CLEARWATCHPOINTS, PTRACE_GETWATCHPOINTS,
    PTRACE_PEEKTEXT, PTRACE_POKEDATA,
    DR7_RW_WRITE,
};
use portix_rt::{install_handler, println};

/// Variable vigilada: cualquier escritura dispara la excepción #DB.
static mut VIGILADA: u64 = 0x1111_1111;
/// Contadores que rellenan los manejadores de señal (Ring 3, sin syscalls).
static WP_HITS: AtomicU64 = AtomicU64::new(0);
static BP_HITS: AtomicU64 = AtomicU64::new(0);

/// Función que NO se ejecuta: sólo se usa como zona de código para verificar
/// el parcheo (leer byte original → escribir 0xCC → leer → restaurar).
#[no_mangle]
pub extern "C" fn zona_de_parcheo(x: u64) -> u64 {
    x.wrapping_mul(3).wrapping_add(7)
}

/// Manejador de SIGTRAP: contabiliza el origen de la trampa. Si la trampa fue
/// del vigilante de datos, desarma la vigilancia para que el bucle termine.
extern "C" fn trap_handler(sig: u64) {
    if sig != 5 { return; }
    let prev = WP_HITS.load(Ordering::SeqCst);
    if prev == 0 {
        // Primer #DB: provenía del vigilante de datos (el punto de parada INT3
        // se prueba después). Se desarma para no repetir la trampa.
        if BP_HITS.load(Ordering::SeqCst) == 0 {
            let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, 0, 0, 0);
        }
        WP_HITS.store(1, Ordering::SeqCst);
    } else if BP_HITS.load(Ordering::SeqCst) == 0 {
        BP_HITS.store(1, Ordering::SeqCst);
    }
}

fn dir_vigilada() -> u64 {
    core::ptr::addr_of!(VIGILADA) as u64
}

fn main_impl() -> i32 {
    let pid = sys_getpid();
    println!("======================================================================");
    println!("  PORTIX OS — AUTOPRUEBA DEL SUBSISTEMA DE DEPURACION (v6.0 'Chronos')");
    println!("======================================================================");
    println!("  PID={}  direccion de la variable vigilada: 0x{:016x}", pid, dir_vigilada());

    if install_handler(5 /* SIGTRAP */, trap_handler).is_err() {
        println!("  [WP-DEMO] no se pudo instalar el manejador de SIGTRAP");
        return 1;
    }

    // ── 1. Punto de vigilancia POR HARDWARE sobre una variable de datos ────
    let wp = dir_vigilada();
    let cfg = portix_rt::syscall::watch_cfg(8, DR7_RW_WRITE);
    let slot = sys_ptrace(PTRACE_SETWATCHPOINT, 0, wp as usize, cfg as usize);
    println!("  [WP-DEMO] PTRACE_SETWATCHPOINT -> ranura {}", slot);
    if slot < 0 {
        println!("  [WATCHPOINT-FAIL] el kernel rechazo la vigilancia (error {})", slot);
        return 1;
    }
    // Confirmación de que el kernel guardó la vigilancia del proceso
    let mut buf = [0u8; 41];
    let n = sys_ptrace(PTRACE_GETWATCHPOINTS, 0, 0, buf.as_mut_ptr() as usize);
    let mut dir_leida = 0u64;
    if n >= 1 && buf[0] >= 1 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[1..9]);
        dir_leida = u64::from_ne_bytes(b);
    }
    println!("  [WP-DEMO] vigilancia registrada por el kernel: {} ranura(s), DR0=0x{:016x}", n, dir_leida);
    if n < 1 || dir_leida != wp {
        println!("  [WATCHPOINT-FAIL] el kernel no registro la direccion vigilada");
        return 1;
    }

    // Escritura real: la CPU debe elevar #DB y el kernel entregar SIGTRAP.
    let t0 = portix_rt::syscall::sys_uptime();
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(VIGILADA), 0xDEAD_BEEF); }
    let t1 = portix_rt::syscall::sys_uptime();
    let hits = WP_HITS.load(Ordering::SeqCst);
    println!("  [WP-DEMO] escritura en 0x{:016x} completada (tick {} -> {}), #DB contabilizados={}",
             wp, t0, t1, hits);
    if hits == 1 {
        println!("  [WATCHPOINT-PASS] escritura detectada por hardware en 0x{:016x} y entregada como SIGTRAP", wp);
    } else {
        println!("  [WATCHPOINT-FAIL] el vigilante de hardware no disparo (hits={})", hits);
        return 1;
    }

    // ── 2. Punto de parada INT3 (#BP) con ajuste de RIP ────────────────────
    let antes = BP_HITS.load(Ordering::SeqCst);
    unsafe { core::arch::asm!("int3", options(nostack)); }
    let despues = BP_HITS.load(Ordering::SeqCst);
    if despues > antes {
        println!("  [BREAKPOINT-PASS] INT3 detuvo el proceso y el kernel reanudo tras el byte 0xCC");
    } else {
        println!("  [BREAKPOINT-FAIL] el punto de parada INT3 no fue entregado (hits={})", despues);
        return 1;
    }

    // ── 3. Parcheo de código verificado (PTRACE_PEEKTEXT/POKEDATA) ─────────
    let objetivo = zona_de_parcheo as *const () as usize;
    let mut orig: u64 = 0;
    let leer_ok = sys_ptrace(PTRACE_PEEKTEXT, 0, objetivo, &mut orig as *mut u64 as usize);
    if leer_ok != 0 {
        println!("  [CODESPATCH-FAIL] PEEKTEXT sobre 0x{:x} fallo ({})", objetivo, leer_ok);
        return 1;
    }
    let bajo = sys_ptrace(PTRACE_POKEDATA, 0, objetivo, 0xCC);
    let mut leido: u64 = 0;
    let _ = sys_ptrace(PTRACE_PEEKTEXT, 0, objetivo, &mut leido as *mut u64 as usize);
    let restaurado = sys_ptrace(PTRACE_POKEDATA, 0, objetivo, ((orig as u64) & 0xFF) as usize);
    let mut verificado: u64 = 0;
    let _ = sys_ptrace(PTRACE_PEEKTEXT, 0, objetivo, &mut verificado as *mut u64 as usize);
    println!("  [WP-DEMO] parcheo de 0x{:x}: original=0x{:02x} escrito=0x{:02x} lectura=0x{:02x} restaurado=0x{:02x}",
             objetivo, orig & 0xFF, 0xCC, leido & 0xFF, verificado & 0xFF);
    if bajo == 0 && (leido & 0xFF) == 0xCC && restaurado == 0 && (verificado & 0xFF) == (orig & 0xFF) {
        println!("  [CODESPATCH-PASS] byte 0xCC instalado, verificado y restaurado en la pagina de codigo");
    } else {
        println!("  [CODESPATCH-FAIL] el parcheo del codigo no se pudo verificar");
        return 1;
    }

    println!("  [DBGSUITE-PASS] vigilancia por hardware, puntos de parada INT3 y parcheo de codigo operativos");
    let _ = PTRACE_GETWATCHPOINTS;
    0
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    main_impl()
}
