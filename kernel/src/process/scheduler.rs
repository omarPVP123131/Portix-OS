// ── Scheduler and Context Switching Subsystem ─────────────────────────────

use crate::drivers::serial;
use crate::mem::paging;
use super::types::*;
use super::{PROCESS_TABLE, process_exit};

pub fn set_tss_rsp0(rsp0: u64) {
    unsafe {
        let tss = crate::arch::idt::get_tss_ptr();
        if !tss.is_null() {
            (*tss).rsp[0] = rsp0;
        }
        // [CICLO v6.x] La puerta `syscall` construye su frame en la pila de
        // kernel del proceso en ejecución; se publica en el mismo punto único
        // en el que se actualiza TSS.RSP0, de modo que ambas vistas de "la pila
        // del proceso actual" nunca se desincronizan.
        extern "C" {
            static mut current_kstack_top: u64;
        }
        current_kstack_top = rsp0;
        // [T-025 E3a] `syscall_entry` ya lee la pila por `gs:8` (por CPU).
        crate::arch::percpu::fijar_pila_nucleo(rsp0);
    }
}

pub fn get_affinity(pid: u64) -> Option<u64> {
    let guard = PROCESS_TABLE.lock();
    let target_pid = if pid == 0 {
        guard.current_process_ref().map(|p| p.pid)?
    } else {
        pid
    };
    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr() as *const Process;
            if (*proc).pid == target_pid && (*proc).state != ProcessState::Dead {
                return Some((*proc).affinity_mask);
            }
        }
    }
    None
}

pub fn set_affinity(pid: u64, mask: u64) -> Result<(), i32> {
    let guard = PROCESS_TABLE.lock();
    let target_pid = if pid == 0 {
        guard.current_process_ref().map(|p| p.pid).ok_or(-3)?
    } else {
        pid
    };
    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr() as *mut Process;
            if (*proc).pid == target_pid && (*proc).state != ProcessState::Dead {
                (*proc).affinity_mask = mask;
                return Ok(());
            }
        }
    }
    Err(-3) // -ESRCH
}

pub fn getpriority(which: i32, who: u64) -> Result<i32, i32> {
    if which < 0 || which > 2 { return Err(-22); }
    let guard = PROCESS_TABLE.lock();
    let cur = guard.current_process_ref().ok_or(-3)?;
    let target_pid = if who == 0 { cur.pid } else { who };
    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr() as *const Process;
            if (*proc).pid == target_pid && (*proc).state != ProcessState::Dead {
                return Ok((*proc).niceness);
            }
        }
    }
    Err(-3) // -ESRCH
}

pub fn setpriority(which: i32, who: u64, prio: i32) -> Result<(), i32> {
    if which < 0 || which > 2 { return Err(-22); }
    if prio < -20 || prio > 19 {
        return Err(-22); // -EINVAL
    }
    let guard = PROCESS_TABLE.lock();
    let cur_pid = guard.current_process_ref().map(|p| p.pid).ok_or(-3)?;
    let cur_euid = guard.current_process_ref().map(|p| p.euid).ok_or(-3)?;
    let target_pid = if who == 0 { cur_pid } else { who };

    for p in &guard.processes {
        unsafe {
            let proc = p.as_ptr() as *mut Process;
            if (*proc).pid == target_pid && (*proc).state != ProcessState::Dead {
                if cur_euid != 0 && prio < (*proc).niceness {
                    return Err(-1); // -EPERM
                }
                (*proc).niceness = prio;
                return Ok(());
            }
        }
    }
    Err(-3) // -ESRCH
}

/// Pre-arma el kernel stack con un frame PUSH_REGS + IRET sintético,
/// para que el scheduler pueda saltar directamente al proceso vía
/// POP_REGS + IRETQ.
pub fn setup_kernel_stack(proc: &mut Process) {
    unsafe {
        let top = proc.kernel_stack_top as *mut u64;
        let mut sp = top;

        // IRET frame (pushed first = highest addresses, CPU pops in reverse)
        sp = sp.sub(1); *sp = 0x1Bu64;              // SS = USER_DS | 3
        sp = sp.sub(1); *sp = proc.user_rsp;         // RSP (user)
        sp = sp.sub(1); *sp = 0x202u64;              // RFLAGS (IF=1)
        sp = sp.sub(1); *sp = 0x23u64;               // CS = USER_CS | 3
        sp = sp.sub(1); *sp = proc.user_rip;         // RIP

        // PUSH_REGS (15 registers, r15 first = highest, rax last = lowest)
        sp = sp.sub(1); *sp = 0; // r15
        sp = sp.sub(1); *sp = 0; // r14
        sp = sp.sub(1); *sp = 0; // r13
        sp = sp.sub(1); *sp = 0; // r12
        sp = sp.sub(1); *sp = 0; // rbp
        sp = sp.sub(1); *sp = 0; // rbx
        sp = sp.sub(1); *sp = 0; // r11
        sp = sp.sub(1); *sp = 0; // r10
        sp = sp.sub(1); *sp = 0; // r9
        sp = sp.sub(1); *sp = 0; // r8
        sp = sp.sub(1); *sp = 0; // rdi
        sp = sp.sub(1); *sp = 0; // rsi
        sp = sp.sub(1); *sp = 0; // rdx
        sp = sp.sub(1); *sp = 0; // rcx
        sp = sp.sub(1); *sp = 0; // rax

        // Space for 16 XMM registers (256 bytes = 32 u64s)
        sp = sp.sub(32);
        for i in 0..32 {
            *sp.add(i) = 0;
        }

        proc.kernel_rsp = sp as u64;

        serial::write_str("SCHED: setup_kernel_stack PID=");
        serial::write_usize(proc.pid as usize);
        serial::write_str(" rsp=");
        serial::write_hex(sp as usize);
        serial::write_str("\n");
    }
}

pub unsafe fn current_raw() -> *mut Process {
    let pid = crate::arch::percpu::pid_actual();
    // [T-025 E3c] El PID de ESTA CPU se busca SIN el cerrojo: las ranuras son
    // estáticas y el proceso actual de un núcleo no lo mueve nadie más. Con
    // `try_lock` un AP cuyo proceso salía mientras el BSP tenía la tabla
    // (padre reintentando `waitpid`) obtenía null, no marcaba Zombie al que
    // moría y `pick_next_ready` lo "reanudaba" con un frame ya consumido (#GP
    // en Ring 3 con RIP en el retorno de `exit`, hijo con código -1).
    if pid != 0 {
        let tabla = &*PROCESS_TABLE.as_mut_ptr();
        for p in &tabla.processes {
            let ptr = p.as_ptr() as *mut Process;
            if (*ptr).pid == pid && (*ptr).state != ProcessState::Dead {
                return ptr;
            }
        }
    }
    if let Some(guard) = PROCESS_TABLE.try_lock() {
        for p in &guard.processes {
            let ptr = p.as_ptr() as *mut Process;
            if (*ptr).state == ProcessState::Running {
                return ptr;
            }
        }
    }
    core::ptr::null_mut()
}

pub unsafe fn pick_next_ready() -> *mut Process {
    let guard = match PROCESS_TABLE.try_lock() {
        Some(g) => g,
        None => return core::ptr::null_mut(),
    };
    let current_pid = crate::arch::percpu::pid_actual();
    let cpu_id = crate::arch::smp::current_cpu_id();
    let cpu_mask = 1u64 << (cpu_id as u64);

    let mut start = 0;
    for (i, p) in guard.processes.iter().enumerate() {
        if (*p.as_ptr()).pid == current_pid {
            start = (i + 1) % MAX_PROCS;
            break;
        }
    }
    // [T-025 E3c] En un AP sólo se toman procesos sin continuación de consola
    // (`ring3_ret_addr == 0`: hijos de fork/clone, no el proceso de primer plano
    // que lanzó `run`, cuya vuelta a la consola vive en la pila del BSP; y
    // además con padre o hilo: un proceso recién creado por `run` está `Ready`
    // ANTES de que `enter_ring3` le fije la continuación, y su primer frame no
    // es un contexto reanudable: se vio un #GP al tomarlo un AP), y en
    // ninguna CPU se toma un proceso que otra CPU siga teniendo como actual
    // (está a medio bloquear/expropiar allí: sería doble ejecución).
    let soy_bsp = crate::arch::smp::is_bsp();
    for _ in 0..MAX_PROCS {
        let ptr = guard.processes[start].as_ptr() as *mut Process;
        let aff = (*ptr).affinity_mask;
        let is_eligible = aff == 0 || (aff & cpu_mask) != 0;
        let mi_idx = crate::arch::percpu::indice_actual() as u8;
        if (*ptr).state == ProcessState::Ready && (*ptr).pid != current_pid && is_eligible
            && (soy_bsp || ((*ptr).ring3_ret_addr == 0 && ((*ptr).ppid != 0 || (*ptr).es_hilo)))
            && !crate::arch::percpu::pid_en_otra_cpu((*ptr).pid)
            && ((*ptr).en_cpu == crate::process::types::SIN_CPU || (*ptr).en_cpu == mi_idx) {
            (*ptr).last_cpu = cpu_id;
            (*ptr).en_cpu = mi_idx; // [E3d] esta CPU se sube a su contexto
            // [E3d] Reclamado AQUÍ, bajo el cerrojo de la tabla: hasta ahora el
            // estado pasaba a Running en `preparar_cpu_para`, fuera del cerrojo,
            // y dos CPUs que elegían a la vez (salida de un hijo en el AP y bucle
            // de espera del BSP) tomaban el MISMO proceso y reanudaban dos veces
            // el mismo frame (#GP de núcleo al terminar gcc → as).
            (*ptr).state = ProcessState::Running;
            return ptr;
        }
        start = (start + 1) % MAX_PROCS;
    }
    // Fallback: return current process if still running
    for _ in 0..MAX_PROCS {
        let ptr = guard.processes[start].as_ptr() as *mut Process;
        let aff = (*ptr).affinity_mask;
        let is_eligible = aff == 0 || (aff & cpu_mask) != 0;
        if (*ptr).pid == current_pid && (*ptr).state == ProcessState::Running && is_eligible {
            (*ptr).last_cpu = cpu_id;
            return ptr;
        }
        start = (start + 1) % MAX_PROCS;
    }
    core::ptr::null_mut()
}

/// [T-025 E3c] Deja ESTA CPU lista para reanudar `next` (común a IRQ0 del BSP,
/// tick LAPIC de un AP, salida de proceso y muerte por excepción): estado,
/// TSS.RSP0, PID por CPU, registros de depuración, CR3 y FS_BASE del entrante.
/// [E3d] Detector de doble ejecución: `pid` va a reanudarse en esta CPU; si otra
/// CPU lo tiene como actual, algo eligió mal. Se deja constancia en el serie.
pub fn avisar_si_doble(pid: u64, donde: &str) {
    if let Some(c) = crate::process::cpu_que_ejecuta(pid) {
        serial::write_str("[DOBLE] cpu ");
        serial::write_usize(crate::arch::percpu::indice_actual());
        serial::write_str(" reanuda PID ");
        serial::write_usize(pid as usize);
        serial::write_str(" que ejecuta la cpu ");
        serial::write_usize(c);
        serial::write_str(" en ");
        serial::write_str(donde);
        serial::write_str("\n");
    }
}

pub unsafe fn preparar_cpu_para(next: *mut Process) {
    avisar_si_doble((*next).pid, "preparar_cpu_para");
    (*next).state = ProcessState::Running;
    (*next).en_cpu = crate::arch::percpu::indice_actual() as u8;
    (*next).ticks_used = 0;
    set_tss_rsp0((*next).kernel_stack_top as u64);
    crate::arch::percpu::fijar_pid_actual((*next).pid);
    crate::arch::debug_regs::apply_for(&*next);
    let current_cr3 = paging::read_cr3();
    if (*next).cr3 != current_cr3 {
        paging::write_cr3((*next).cr3);
    }
    let n_low = (*next).fs_base as u32;
    let n_high = ((*next).fs_base >> 32) as u32;
    core::arch::asm!("wrmsr", in("ecx") 0xC0000100u32, in("eax") n_low, in("edx") n_high, options(nostack, preserves_flags));
}

// [B / T-028] Contadores del despertador por tiempo. Si una espera con plazo
// no despierta, estos tres números dicen cuál de los modos de fallo fue: no se
// llamó (el tick vuelve antes de tiempo), el cerrojo estaba ocupado y se
// saltó el barrido, o el barrido corrió y despertó a alguien.
pub static WAKE_LLAMADAS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static WAKE_SIN_CERROJO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static WAKE_DESPERTADOS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Barridos en los que un plazo YA vencido seguía sin despertar (una vez por
/// episodio; mientras dure, cada barrido lo cuenta).
pub static ESPERA_ATASCADA: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Tick del último barrido COMPLETO del despertador (0 = ninguno todavía).
pub static WAKE_ULTIMO_OK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Procesos vistos Blocked con plazo en ese último barrido.
pub static BLOQUEADOS_CON_PLAZO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Barridos seguidos sin poder tomar el cerrojo (el despertador está ciego).
pub static WAKE_SEGUIDOS_SIN_CERROJO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static AVISO_ESPERA: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0); // bandera 0/1
/// Peor racha de barridos seguidos sin cerrojo y peor hueco (en ticks) entre
/// dos barridos completos. Es el número que decide si un plazo puede quedarse
/// colgado: un barrido que se salta de vez en cuando no hace daño; uno que se
/// salta durante 100 ticks sí.
pub static WAKE_PEOR_SEGUIDOS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static WAKE_PEOR_HUECO_TICKS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Contadores ADITIVOS de huecos largos entre barridos. El máximo acumulado no
/// sirve para juzgar una ventana concreta (arrastra lo peor de toda la sesión,
/// incluida la carga de la JVM); estos sí se pueden restar.
pub static WAKE_HUECOS_20: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static WAKE_HUECOS_100: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub unsafe fn wake_blocked() {
    use core::sync::atomic::Ordering as O;
    let now = crate::time::pit::ticks();
    WAKE_LLAMADAS.fetch_add(1, O::Relaxed);
    let Some(guard) = PROCESS_TABLE.try_lock() else {
        // Antes esto era un `if let` que no dejaba rastro: el barrido se
        // saltaba en silencio y nadie sabía que el despertador había quedado
        // ciego justo cuando el cerrojo estaba ocupado.
        let seguidos = WAKE_SEGUIDOS_SIN_CERROJO.fetch_add(1, O::Relaxed) + 1;
        WAKE_SIN_CERROJO.fetch_add(1, O::Relaxed);
        if seguidos > WAKE_PEOR_SEGUIDOS.load(O::Relaxed) { WAKE_PEOR_SEGUIDOS.store(seguidos, O::Relaxed); }
        // [T-028] Si el barrido lleva 50 ticks seguidos sin poder tomar el
        // cerrojo, alguien lo retiene mientras espera: las esperas con plazo
        // se quedan colgadas y hay que decirlo, no dejarlo en silencio.
        if seguidos == 50 {
            serial::write_str("[ESPERA-ATASCADA] 50 barridos sin cerrojo de procesos (el despertador por tiempo esta ciego)\n");
            ESPERA_ATASCADA.fetch_add(1, O::Relaxed);
        }
        return;
    };
    let mut con_plazo = 0u64;
    {
        for p in &guard.processes {
            let ptr = p.as_ptr() as *mut Process;
            if (*ptr).state == ProcessState::Blocked && (*ptr).sleep_until > 0 { con_plazo += 1; }
            if (*ptr).state != ProcessState::Dead {
                if (*ptr).state == ProcessState::Blocked && (*ptr).sleep_until > 0 {
                    // [E3d] Vencimiento con aritmética circular: antes "objetivo a más de
                    // 10 s" se trataba como desbordado y se despertaba al instante, así
                    // que `nanosleep(100 s)` volvía 0 enseguida (el `infinite_sleep` de la
                    // JVM tras un error fatal giraba a 29 MB de serie por minuto).
                    if (now.wrapping_sub((*ptr).sleep_until) as i64) >= 0 {
                        (*ptr).state = ProcessState::Ready;
                        (*ptr).sleep_until = 0;
                        WAKE_DESPERTADOS.fetch_add(1, O::Relaxed);
                        serial::write_str("SCHED: wake PID ");
                        serial::write_usize((*ptr).pid as usize);
                        serial::write_str("\n");
                    }
                }
                if (*ptr).alarm_ticks > 0 && now >= (*ptr).alarm_ticks {
                    (*ptr).alarm_ticks = 0;
                    (*ptr).sig_pending |= 1 << 14; // SIGALRM (14)
                    // [T-001/FIFO] una señal despierta a quien espera en pause/sigsuspend/wait
                    if (*ptr).state == ProcessState::Blocked && (*ptr).sig_blocked & (1 << 14) == 0 {
                        (*ptr).state = ProcessState::Ready;
                    }
                }
            }
        }
    }
    let previo = WAKE_ULTIMO_OK.load(O::Relaxed);
    if previo != 0 {
        let hueco = now.wrapping_sub(previo);
        if hueco > WAKE_PEOR_HUECO_TICKS.load(O::Relaxed) { WAKE_PEOR_HUECO_TICKS.store(hueco, O::Relaxed); }
        if hueco > 20 { WAKE_HUECOS_20.fetch_add(1, O::Relaxed); }
        if hueco > 100 { WAKE_HUECOS_100.fetch_add(1, O::Relaxed); }
    }
    WAKE_ULTIMO_OK.store(now, O::Relaxed);
    BLOQUEADOS_CON_PLAZO.store(con_plazo, O::Relaxed);
    WAKE_SEGUIDOS_SIN_CERROJO.store(0, O::Relaxed);
    AVISO_ESPERA.store(0, O::Relaxed);
}

/// [T-028] Vigilante del reloj de las esperas. En T-028 el `curl` quedó
/// `Blocked cpu 0 sleep_until 1799` más de 144 s y, a la vez, `tcp_wait_accept`
/// (tope de 1000 ticks) tampoco volvió: dos relojes distintos que no vencen es
/// la firma de que NO CORREN LOS TICKS o de que el barrido de vencimientos no
/// se ejecuta. Esto último puede pasar sin que nada avise: `schedule_tick`
/// vuelve antes de `wake_blocked` cuando la CPU no venía de modo usuario.
/// Se llama en cada tick ANTES de ese retorno, así que también vigila el ocio.
pub fn vigilar_reloj_de_esperas() {
    let now = crate::time::pit::ticks();
    if now < 300 { return; }
    let ultimo = WAKE_ULTIMO_OK.load(core::sync::atomic::Ordering::Relaxed);
    let rancio = if ultimo == 0 { now } else { now.wrapping_sub(ultimo) };
    if rancio < 300 { return; }
    if BLOQUEADOS_CON_PLAZO.load(core::sync::atomic::Ordering::Relaxed) == 0 { return; }
    // Un aviso por episodio: se vuelve a armar cuando el barrido vuelve a correr.
    if AVISO_ESPERA.swap(1, core::sync::atomic::Ordering::Relaxed) != 0 { return; }
    ESPERA_ATASCADA.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let (l, sc, d) = (WAKE_LLAMADAS.load(core::sync::atomic::Ordering::Relaxed),
                      WAKE_SIN_CERROJO.load(core::sync::atomic::Ordering::Relaxed),
                      WAKE_DESPERTADOS.load(core::sync::atomic::Ordering::Relaxed));
    serial::write_str("[ESPERA-ATASCADA] el barrido no corre: ticks=");
    serial::write_usize(now as usize);
    serial::write_str(" ultimo_barrido=");
    serial::write_usize(ultimo as usize);
    serial::write_str(" bloqueados_con_plazo=");
    serial::write_usize(con_plazo_ultimo() as usize);
    serial::write_str(" llamadas=");
    serial::write_usize(l as usize);
    serial::write_str(" sin_cerrojo=");
    serial::write_usize(sc as usize);
    serial::write_str(" despertados=");
    serial::write_usize(d as usize);
    serial::write_str("\n");
}

fn con_plazo_ultimo() -> u64 {
    BLOQUEADOS_CON_PLAZO.load(core::sync::atomic::Ordering::Relaxed)
}

/// [PREEMPCIÓN EN KERNEL] Indica que el proceso actual está parado en un
/// punto de espera del kernel sin cerrojos tomados (`wait_hlt_preemptible`),
/// de modo que IRQ0 puede cambiar de proceso aunque CS sea de Ring 0. Se
/// limpia en cada cambio de contexto para que nunca "herede" otro proceso.
// [T-025 E3a] por CPU: `percpu::actual().preempt_ok`.

/// Espera un tick cediendo la CPU a otros procesos listos. Es la alternativa
/// a `sti; hlt; cli` en los bucles de espera de red/poll: antes, un `accept()`
/// o `recv()` bloqueante monopolizaba la CPU (el planificador sólo
/// expropiaba frames de Ring 3) y dos procesos no podían hablar entre sí por
/// el bucle local. Debe llamarse SIN cerrojos tomados.
pub fn wait_hlt_preemptible() {
    unsafe {
        let cur = current_raw();
        if !cur.is_null() { (*cur).ticks_used = TIME_SLICE; }
        // [T-025 E3d] Punto de espera: se suelta el cerrojo grande para que
        // otra CPU pueda entrar al núcleo (el escritor de la tubería que este
        // lector espera, por ejemplo) y se retoma al despertar.
        let d_cgn = crate::arch::bkl::soltar_todo();
        crate::arch::percpu::actual().preempt_ok.store(true, core::sync::atomic::Ordering::SeqCst);
        core::arch::asm!("sti; hlt; cli", options(nomem, nostack));
        crate::arch::percpu::actual().preempt_ok.store(false, core::sync::atomic::Ordering::SeqCst);
        crate::arch::bkl::retomar(d_cgn);
        // [T-026/b203] Cierre diferido del líder de consola. Cuando un hilo del
        // grupo muere en un AP, `cerrar_lider_dormido` deja al líder con
        // `salir_solo` + SIGKILL interno y estado Ready para que lo termine el
        // BSP. Si el líder está durmiendo en un bucle de espera que NO mira
        // señales (futex, nanosleep, poll, tcp…), ese aviso se perdía: el líder
        // volvía a dormirse y su `run` de consola esperaba para siempre (la
        // consola quedó sorda tras la JVM en la suite de T-042). Éste es el
        // punto único por el que pasa toda espera del núcleo, así que aquí se
        // completa el cierre, venga del bucle que venga; el RIP dice cuál era.
        let cur = crate::process::current_raw();
        if !cur.is_null() && (*cur).lider_salio && (*cur).salir_solo
            && !(*cur).es_hilo && (*cur).ppid == 0
            && ((*cur).sig_pending & (1u64 << crate::process::signals::SIGKILL as u64)) != 0 {
            crate::drivers::serial::write_str("[T-005] cierre diferido del lider completado en su espera: pid=");
            crate::drivers::serial::write_usize((*cur).pid as usize);
            crate::drivers::serial::write_str(" rip=");
            crate::drivers::serial::write_hex(crate::time::pit::rip_actual() as usize);
            crate::drivers::serial::write_str("\n");
            let pid = (*cur).pid;
            crate::process::signals::preparar_terminacion_por_senal(crate::process::signals::SIGKILL, pid);
            process_exit_trampoline();
        }
    }
}

/// Called from IRQ0 handler assembly.
/// [E3d] Anota que esta CPU abandona el contexto de `cur`; `en_cpu` se limpia
/// en `liberar_contexto_saliente`, una vez sobre la pila del entrante.
pub fn diferir_liberacion(cur: *mut Process) {
    crate::arch::percpu::actual().saliente.store(cur as u64, core::sync::atomic::Ordering::SeqCst);
}

/// [E3d] Llamado por los stubs de `isr.asm` justo después de `mov rsp, <entrante>`
/// (y por el bucle ocioso del AP ya en su pila propia): el contexto saliente
/// deja de estar en uso por esta CPU y otra puede reclamarlo.
#[no_mangle]
pub extern "C" fn liberar_contexto_saliente() {
    let pc = crate::arch::percpu::actual();
    let p = pc.saliente.swap(0, core::sync::atomic::Ordering::SeqCst) as *mut Process;
    if p.is_null() { return; }
    unsafe {
        if (*p).en_cpu == crate::arch::percpu::indice_actual() as u8 && p != current_raw() {
            (*p).en_cpu = crate::process::types::SIN_CPU;
        }
    }
}

#[no_mangle]
pub extern "C" fn schedule_tick(current_rsp: u64, saved_cs: u64) -> u64 {
    let user_cs = (crate::arch::idt::USER_CS as u64) | 3;
    let in_user = saved_cs == user_cs;
    let kernel_preempt = !in_user && crate::arch::percpu::actual().preempt_ok.swap(false, core::sync::atomic::Ordering::SeqCst);

    // [ÉPICA 59] Muestreo del contador de programa para el profiler de silicio.
    // El frame capturado por isr_timer tiene el RIP en +376 y el RSP
    // interrumpido en +400, sea cual sea el anillo en el que se tomó la muestra.
    if current_rsp >= 0x1000 {
        let sample_rip = unsafe { core::ptr::read_volatile((current_rsp + 376) as *const u64) };
        let sample_rsp = unsafe { core::ptr::read_volatile((current_rsp + 400) as *const u64) };
        let pid = crate::arch::percpu::pid_actual();
        // [85ª, defecto 20] RIP interrumpido en el peor hueco del PIT (quién tenía IF=0).
        if crate::time::pit::IRQ0_HUECO_NUEVO.swap(0, core::sync::atomic::Ordering::Relaxed) != 0 {
            crate::time::pit::IRQ0_SITIO_RIP.store(sample_rip, core::sync::atomic::Ordering::Relaxed);
        }
        if pid != 0 && (in_user || sample_rip >= 0x1000) && crate::arch::smp::is_bsp() {
            crate::system::devsuite::sample(pid, sample_rip, sample_rsp, in_user);
        }
    }

    // [CONTROL DE TRABAJOS] ^C/^Z por la serie se atienden en cada tick,
    // lea o no stdin el proceso de primer plano. [E3c] Sólo el BSP (dueño de
    // la serie y de la consola); el tick de un AP no toca la TTY.
    if crate::arch::smp::is_bsp() { crate::process::tty_pump_from_irq(); }

    // [T-028] EL BARRIDO DE VENCIMIENTOS CORRE EN CADA TICK, venga la CPU de
    // modo usuario o esté ociosa en el núcleo. Antes vivía detrás del retorno
    // «si no vengo de modo usuario, no planifico», así que mientras todas las
    // CPUs estaban en el núcleo sin `preempt_ok` —justo el estado de un sistema
    // ocioso— NADIE despertaba a los que dormían con plazo. Medido con
    // `scripts/sonda_reloj_t028.py` sobre la imagen anterior: rachas de hasta
    // **150 ticks (1,5 s) sin un solo barrido**, con procesos `Blocked` con
    // plazo esperando. Es el mecanismo de T-028: dos plazos independientes que
    // no vencen a la vez. Despertar es barato (try_lock + recorrido) y no
    // cambia de contexto: sólo marca `Ready`.
    unsafe { wake_blocked(); }
    if !in_user && !kernel_preempt {
        vigilar_reloj_de_esperas();
        return 0; // Skip if not from user mode
    }
    unsafe {
        let cur = current_raw();
        if cur.is_null() {
            return 0;
        }
        (*cur).kernel_rsp = current_rsp;
        (*cur).ticks_used += 1;
        if saved_cs == user_cs {
            (*cur).utime += 1;
        } else {
            (*cur).stime += 1;
        }

        // (el barrido de vencimientos ya se hizo al entrar al tick)

        // [ÉPICA 58] Entrega de señales pendientes antes de decidir el cambio
        // de contexto: el frame de Ring 3 ya está en el stack del kernel.
        // [T-025 E3a] Sólo desde IRQ0: si `schedule_tick` lo llamó una syscall
        // (sleep, yield, futex, waitpid…) el frame es el de la syscall y su RAX
        // es el NÚMERO de syscall; entregar aquí guardaba ese número como RAX
        // del usuario y, tras `sigreturn`, glibc veía p. ej. 230
        // (clock_nanosleep) como resultado. Antes lo tapaba `current_process_ref`
        // (devolvía None con el proceso en `Blocked`); con el PID por CPU se
        // destapó: "[Errno -230]" en `time.sleep` interrumpido por SIGALRM.
        let desde_syscall = crate::arch::percpu::actual().en_syscall.load(core::sync::atomic::Ordering::SeqCst);
        if in_user && !desde_syscall { let _ = crate::process::signals::deliver_pending(current_rsp, None); }

        if (*cur).state == ProcessState::Running && (*cur).ticks_used < TIME_SLICE {
            return 0;
        }

        (*cur).ticks_used = 0;

        let next = pick_next_ready();
        if next.is_null() || next == cur {
            return 0;
        }

        // Check if current was blocked by SYS_SLEEP
        if (*cur).state == ProcessState::Running {
            (*cur).state = ProcessState::Ready;
        }
        // [E3d] Esta CPU abandona el contexto de `cur` (quedan sólo el epílogo
        // de esta función y el `mov rsp` del stub): otra CPU ya puede tomarlo.
        diferir_liberacion(cur);
        avisar_si_doble((*next).pid, "schedule_tick");

        (*next).state = ProcessState::Running;
        (*next).en_cpu = crate::arch::percpu::indice_actual() as u8;
        (*next).ticks_used = 0;
        set_tss_rsp0((*next).kernel_stack_top as u64);
        crate::arch::percpu::fijar_pid_actual((*next).pid);

        // [CICLO v6.x] Los registros de depuración son por CPU: se cargan los
        // del proceso entrante (o se desactivan si no tiene watchpoints).
        crate::arch::debug_regs::apply_for(&*next);


        let current_cr3 = paging::read_cr3();
        if (*next).cr3 != current_cr3 {
            paging::write_cr3((*next).cr3);
        }

        // Preserve and restore FS_BASE (MSR 0xC0000100) across processes for thread-local storage (TLS)
        let low: u32;
        let high: u32;
        core::arch::asm!("rdmsr", in("ecx") 0xC0000100u32, out("eax") low, out("edx") high, options(nostack, preserves_flags));
        (*cur).fs_base = ((high as u64) << 32) | (low as u64);

        if (*cur).fs_base != (*next).fs_base {
            let n_low = (*next).fs_base as u32;
            let n_high = ((*next).fs_base >> 32) as u32;
            core::arch::asm!("wrmsr", in("ecx") 0xC0000100u32, in("eax") n_low, in("edx") n_high, options(nostack, preserves_flags));
        }

        serial::write_str("SCHED[");
        serial::write_usize(crate::arch::percpu::indice_actual());
        serial::write_str("]: PID ");
        serial::write_usize((*cur).pid as usize);
        serial::write_str(" → ");
        serial::write_usize((*next).pid as usize);
        serial::write_str("\n");

        (*next).kernel_rsp
    }
}

/// Called from `enter_ring3_asm` to save the return address for ring-3 exit.
#[no_mangle]
pub extern "C" fn process_save_ring3_ret_addr(ret_addr: u64) {
    unsafe {
        let cur = current_raw();
        if !cur.is_null() {
            (*cur).ring3_ret_addr = ret_addr;
        }
    }
}

/// Called from `ring3_exit_trampoline`.
#[no_mangle]
pub extern "C" fn process_get_ring3_ret_addr() -> u64 {
    unsafe {
        let cur = current_raw();
        if cur.is_null() { 0 } else { (*cur).ring3_ret_addr }
    }
}

extern "C" {
    fn switch_to_process_asm(rsp: u64) -> !;
}

/// Trampoline for ring-3 process exit.
#[no_mangle]
pub extern "C" fn process_exit_trampoline() -> ! {
    unsafe {
        // [T-025 E3d] La limpieza (fds, tuberías, espacio de direcciones) toca
        // estado global: bajo el cerrojo grande. Se suelta del todo antes de
        // abandonar este contexto (salto a la consola, conmutación o bucle ocioso).
        crate::arch::bkl::tomar();
        let ret_addr = process_get_ring3_ret_addr();

        // Clean up current process or transition to Zombie
        let cur = current_raw();
        if !cur.is_null() {
            let pid = (*cur).pid;
            let ppid = (*cur).ppid;
            let code = (*cur).exit_code;
            // [T-025 E3d] Antes de que nadie (el padre, desde otra CPU, en cuanto
            // vea Zombie) pueda desmontar este proceso: (1) esta CPU deja sus
            // tablas (el padre las libera al recoger) y (2) se queda con la
            // propiedad de este kernel stack, sobre el que aún ejecuta.
            {
                let kcr3 = paging::cr3_consola();
                if kcr3 != 0 && paging::read_cr3() != kcr3 { paging::write_cr3(kcr3); }
            }
            crate::process::diferir_pila_propia(pid);
            (*cur).en_cpu = crate::process::types::SIN_CPU;
            if ppid != 0 {
                // If it has a parent, mark it as Zombie so parent can reap via waitpid()
                (*cur).state = ProcessState::Zombie;
                // Wake up parent if blocked
                if let Some(guard) = PROCESS_TABLE.try_lock() {
                    for p in &guard.processes {
                        let pp = p.as_ptr() as *mut Process;
                        if (*pp).pid == ppid {
                            // [T-001/FIFO] POSIX: SIGCHLD se genera al TERMINAR el hijo,
                            // no al recogerlo. `busybox sh` espera en `rt_sigsuspend`
                            // a que su manejador de SIGCHLD corra; con la señal sólo
                            // al reap (process_exit) el `wait` del shell no volvía.
                            (*pp).sig_pending |= 1u64 << 17;
                            crate::process::signals::note_sigchld();
                            if (*pp).state == ProcessState::Blocked { (*pp).state = ProcessState::Ready; }
                        }
                    }
                }
            } else {
                process_exit(pid, code);
            }
        }

        // [FRENTE C] El proceso que termina deja los registros DR0-DR3/DR7
        // cargados en el CPU: hay que apagarlos aquí mismo o la siguiente
        // conmutación por `ret_addr` (vuelta al llamador del kernel) o la de
        // abajo heredan ranuras fantasma de un proceso muerto. Se vio en
        // `dbg --watch self 32`: "sólo quedan 2 libres" tras un objetivo que
        // salió con dos ranuras armadas.
        crate::arch::debug_regs::disable_all();
        crate::arch::bkl::soltar_todo();
        // [T-025 E3d] Esta CPU deja de pertenecer al proceso que sale: vuelve al
        // CR3 del núcleo. Con una sola CPU daba igual seguir sobre las tablas del
        // muerto (nadie reutilizaba sus marcos hasta el siguiente `run`); con dos,
        // el BSP esperaba aquí sobre el CR3 de `as` mientras gcc, en el AP, lo
        // recogía (`waitpid` libera sus tablas) y hacía `fork` (reutiliza esos
        // marcos como tablas nuevas): el PML4 del BSP pasaba a ser basura y la
        // siguiente traducción era un triple fallo (reinicio tras "FORK … 151").
        // No se usa el maestro en sí (ver `paging::cr3_consola`).
        {
            let kcr3 = paging::cr3_consola();
            if kcr3 != 0 && paging::read_cr3() != kcr3 { paging::write_cr3(kcr3); }
        }
        if ret_addr != 0 {
            let r3 = crate::arch::percpu::ring3_ret_rsp_actual();
            let saved_rsp = if r3 != 0 { r3 } else { core::ptr::addr_of!(crate::__stack_top) as u64 };
            core::arch::asm!("
                mov rsp, {0}
                jmp {1}
            ", in(reg) saved_rsp, in(reg) ret_addr, options(noreturn));
        } else {
            // [CONTROL DE TRABAJOS] Si nadie está listo pero quedan procesos
            // bloqueados (durmiendo, esperando a un hijo) o detenidos, el sistema
            // NO ha terminado: se espera en `hlt` con interrupciones hasta que el
            // temporizador despierte a alguno. Antes se "apagaba" para siempre,
            // p. ej. al morir el último proceso activo mientras un `sleep` dormía
            // y su shell esperaba en `waitpid`.
            let mut next = pick_next_ready();
            if next.is_null() {
                let quedan = || {
                    let Some(guard) = PROCESS_TABLE.try_lock() else { return true };
                    guard.processes.iter().any(|p| {
                        let q = p.as_ptr() as *const Process;
                        (*q).pid != 0 && matches!((*q).state, ProcessState::Blocked | ProcessState::Stopped | ProcessState::Ready)
                    })
                };
                // [T-025 E3c] Un AP no espera aquí: devuelve el núcleo a su bucle
                // ocioso (el BSP es quien vela por los bloqueados y la red).
                if !crate::arch::smp::is_bsp() { crate::arch::smp::ap_ir_a_ocioso(); }
                if quedan() {
                    serial::write_str("SCHED: sin procesos listos; esperando a los bloqueados/detenidos\n");
                    while next.is_null() {
                        core::arch::asm!("sti; hlt; cli", options(nomem, nostack));
                        { let _g = crate::arch::bkl::Guard::tomar(); crate::net::poll(); }
                        wake_blocked();
                        next = pick_next_ready();
                        if next.is_null() && !quedan() { break; }
                    }
                }
            }
            if !next.is_null() {
                preparar_cpu_para(next);
                // [CICLO 50 / T1b] La conmutación por salida de proceso no
                // restauraba FS_BASE (TLS): el siguiente proceso heredaba el
                // puntero de hilo del que acababa de morir. Con dos binarios de
                // la misma imagen no se notaba (misma disposición virtual); con
                // `ld-musl` ejecutado como programa junto a `busybox-dyn`, el
                // hermano superviviente escribía por un puntero TLS ajeno (#PF
                // con CR2 basura). Aquí el proceso que sale ya no existe: sólo
                // se carga el FS_BASE del entrante.
                serial::write_str("SCHED[");
                serial::write_usize(crate::arch::percpu::indice_actual());
                serial::write_str("]: exit switch to PID ");
                serial::write_usize((*next).pid as usize);
                serial::write_str("\n");
                switch_to_process_asm((*next).kernel_rsp);
            }
            if !crate::arch::smp::is_bsp() { crate::arch::smp::ap_ir_a_ocioso(); }
            serial::write_str("SCHED: all processes exited, halting\n");
            loop { core::arch::asm!("hlt"); }
        }
    }
}

/// [T-025 E3a] `schedule_tick` invocado desde una syscall: mismo planificador,
/// pero sin entrega de señales sobre el frame de la syscall (ver comentario en
/// `schedule_tick`); las entrega el retorno de la syscall con `Some(resultado)`.
pub fn schedule_desde_syscall(current_rsp: u64, saved_cs: u64) -> u64 {
    let pc = crate::arch::percpu::actual();
    pc.en_syscall.store(true, core::sync::atomic::Ordering::SeqCst);
    // [T-025 E3d] Si se conmuta a otro proceso, este contexto de syscall se
    // abandona (el frame se reanuda por `iretq` directo a Ring 3): no puede
    // llevarse el cerrojo grande. Si no hay a quién conmutar, se retoma.
    let d_cgn = crate::arch::bkl::soltar_todo();
    let r = schedule_tick(current_rsp, saved_cs);
    if r == 0 { crate::arch::bkl::retomar(d_cgn); }
    // Si no hubo cambio de contexto seguimos en esta CPU y esta syscall; si lo
    // hubo, el frame entrante es otro: en ambos casos el indicador se apaga
    // (el planificador lo consulta sólo dentro de `schedule_tick`).
    crate::arch::percpu::actual().en_syscall.store(false, core::sync::atomic::Ordering::SeqCst);
    r
}

/// SYS_YIELD: give up the remainder of the current quantum.
pub fn yield_cpu() {
    unsafe {
        let cur = current_raw();
        if !cur.is_null() {
            (*cur).ticks_used = TIME_SLICE; // Force reschedule on next IRQ0
        }
    }
    unsafe { core::arch::asm!("sti"); }
}
