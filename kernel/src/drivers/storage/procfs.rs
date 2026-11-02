// drivers/storage/procfs.rs — PORTIX Virtual Process Filesystem (/proc)
//
// Provee la infraestructura de pseudo-sistema de archivos /proc para inspección
// de métricas globales del sistema (versión, tiempo activo, memoria, cpu, montajes,
// módulos) y estado detallado de procesos (/proc/<pid>/stat, status, cmdline)
// y alias contextual (/proc/self).

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::process::{self, ProcFileType, ProcessState};
use crate::syscall::Stat;

/// Comprueba si la ruta pertenece a la jerarquía de procfs.
pub fn is_procfs_path(path: &str) -> bool {
    path == "/proc" || path == "/proc/" || path.starts_with("/proc/")
}

/// Resuelve una ruta /proc a su tipo correspondiente y si es un directorio.
pub fn resolve_proc_path(path: &str) -> Option<(ProcFileType, bool)> {
    if !is_procfs_path(path) {
        return None;
    }
    let p = path.trim_end_matches('/');
    if p == "/proc" || p.is_empty() {
        return Some((ProcFileType::Directory(0), true));
    }
    let rel = &p[6..]; // Salta "/proc/"
    if rel == "version" {
        return Some((ProcFileType::Version, false));
    }
    if rel == "uptime" {
        return Some((ProcFileType::Uptime, false));
    }
    if rel == "meminfo" {
        return Some((ProcFileType::Meminfo, false));
    }
    if rel == "cpuinfo" {
        return Some((ProcFileType::Cpuinfo, false));
    }
    if rel == "smp" {
        return Some((ProcFileType::Smp, false));
    }
    if rel == "smp_estres" {
        return Some((ProcFileType::SmpEstres, false));
    }
    if rel == "loadavg" {
        return Some((ProcFileType::Loadavg, false));
    }
    if rel == "mounts" {
        return Some((ProcFileType::Mounts, false));
    }
    if rel == "modules" {
        return Some((ProcFileType::Modules, false));
    }
    if rel == "stat" {
        return Some((ProcFileType::Stat, false));
    }
    if rel == "nvme" || rel == "nvme_status" {
        return Some((ProcFileType::NvmeStatus, false));
    }
    if rel == "pipes" || rel == "pipe" {
        return Some((ProcFileType::PipesStatus, false));
    }
    if rel == "signals" || rel == "sigaction" {
        return Some((ProcFileType::SignalsStatus, false));
    }
    if rel == "syscalls" || rel == "trace" || rel == "syscall_trace" {
        return Some((ProcFileType::SyscallTrace, false));
    }
    if rel == "profiler" || rel == "profile" {
        return Some((ProcFileType::CpuProfiler, false));
    }
    if rel == "frames" || rel == "cow" {
        return Some((ProcFileType::Frames, false));
    }
    if rel == "dbg" || rel == "debug" {
        return Some((ProcFileType::DebugChannel, false));
    }
    if rel == "abi" || rel == "abi_freeze" {
        return Some((ProcFileType::AbiFreeze, false));
    }
    if rel == "certification" || rel == "certify" {
        return Some((ProcFileType::Certification, false));
    }
    if rel == "release" || rel == "chronos" || rel == "version_full" {
        return Some((ProcFileType::ReleaseInfo, false));
    }
    if rel == "raytracing" || rel == "vulkan_rt" {
        return Some((ProcFileType::RayTracing, false));
    }
    if rel == "driver_sandbox" || rel == "ring3_drivers" {
        return Some((ProcFileType::DriverSandbox, false));
    }
    if rel == "riscv64_hal" || rel == "riscv64" || rel == "rv64" {
        return Some((ProcFileType::Riscv64Hal, false));
    }
    if rel == "pqc_status" || rel == "pqc" || rel == "quantum" {
        return Some((ProcFileType::PqcStatus, false));
    }
    if rel == "ebpf" || rel == "probe" || rel == "portix_probe" {
        return Some((ProcFileType::EbpfProbe, false));
    }
    if rel == "rt_audio" || rel == "rtaudio" || rel == "audio_rt" {
        return Some((ProcFileType::RtAudio, false));
    }
    if rel == "wasi_components" || rel == "wasi" || rel == "wasm_comp" {
        return Some((ProcFileType::WasiComponents, false));
    }
    if rel == "portix_dfs" || rel == "dfs" || rel == "crdt" {
        return Some((ProcFileType::PortixDfs, false));
    }
    if rel == "self" {
        let cur = process::current_pid();
        return Some((ProcFileType::Directory(cur), true));
    }
    if rel == "self/stat" {
        let cur = process::current_pid();
        return Some((ProcFileType::ProcessStat(cur), false));
    }
    if rel == "self/status" {
        let cur = process::current_pid();
        return Some((ProcFileType::ProcessStatus(cur), false));
    }
    if rel == "self/cmdline" {
        let cur = process::current_pid();
        return Some((ProcFileType::ProcessCmdline(cur), false));
    }

    // Comprobar <pid> o <pid>/<archivo>
    if let Some(slash_pos) = rel.find('/') {
        let pid_str = &rel[..slash_pos];
        let sub = &rel[slash_pos + 1..];
        let pid = pid_str.parse::<u64>().ok()?;
        if process::get_process_snapshot(pid).is_none() {
            return None;
        }
        match sub {
            "stat" => Some((ProcFileType::ProcessStat(pid), false)),
            "status" => Some((ProcFileType::ProcessStatus(pid), false)),
            "cmdline" => Some((ProcFileType::ProcessCmdline(pid), false)),
            "maps" => Some((ProcFileType::ProcessMaps(pid), false)),
            "symbols" | "symtab" => Some((ProcFileType::ProcessSymbols(pid), false)),
            _ => None,
        }
    } else {
        let pid = rel.parse::<u64>().ok()?;
        if process::get_process_snapshot(pid).is_some() {
            Some((ProcFileType::Directory(pid), true))
        } else {
            None
        }
    }
}

/// Nombre canónico de un nodo de la suite de desarrollo (para escritura).
pub fn node_name(ft: ProcFileType) -> &'static str {
    match ft {
        ProcFileType::SyscallTrace => "syscalls",
        ProcFileType::CpuProfiler => "profiler",
        ProcFileType::Frames => "frames",
        ProcFileType::DebugChannel => "dbg",
        _ => "",
    }
}

/// Genera el contenido dinámico del archivo pseudo-proc.
pub fn generate_content(ft: ProcFileType) -> Vec<u8> {
    match ft {
        ProcFileType::Version => {
            String::from("Linux version 6.6.0-portix (portix-dev@buildhost) (rustc 1.80.0-nightly) #1 SMP Sun Sep 20 2026\n").into_bytes()
        }
        ProcFileType::Uptime => {
            let ticks = crate::time::pit::ticks();
            let sec = ticks / 100;
            let csec = ticks % 100;
            format!("{}.{:02} {}.{:02}\n", sec, csec, sec, csec).into_bytes()
        }
        ProcFileType::Frames => {
            let (cow_f, cow_c, dem_f, _, _, _) = crate::mem::paging::get_vm_advanced_stats();
            let mut s = crate::mem::frames::informe();
            s.push_str(&format!("[COW] fallos resueltos={} paginas clonadas={} demanda={}\n", cow_f, cow_c, dem_f));
            s.into_bytes()
        }
        ProcFileType::Meminfo => {
            // [b191 / T-011] Memoria física real para procesos (marcos de usuario), no el
            // montículo del núcleo: el JVM dimensiona su montículo con MemTotal.
            let (total_m, libres_m, _, _) = crate::mem::marcos::estadisticas();
            let total_kb = total_m * 4;
            let free_kb = libres_m * 4;
            format!(
                "MemTotal:       {:8} kB\nMemFree:        {:8} kB\nMemAvailable:   {:8} kB\nBuffers:               0 kB\nCached:             4096 kB\nSwapTotal:             0 kB\nSwapFree:              0 kB\n",
                total_kb, free_kb, free_kb
            ).into_bytes()
        }
        ProcFileType::Cpuinfo => {
            // T-025: sólo los núcleos planificables (hoy 1), con el APIC ID
            // real leído de la LAPIC y la frecuencia medida del TSC.
            let count = crate::arch::smp::cpu_count();
            let mhz = crate::time::pit::tsc_hz_cacheado() / 1_000_000;
            let mut info = String::new();
            for i in 0..count {
                use core::fmt::Write;
                let apic = crate::arch::smp::get_core(i).map(|c| c.apic_id as u32).unwrap_or(0);
                let _ = write!(
                    info,
                    "processor\t: {}\nvendor_id\t: GenuinePortix\ncpu family\t: 6\nmodel\t\t: 1\nmodel name\t: Portix x86_64 (APIC ID {})\nstepping\t: 1\ncpu MHz\t\t: {}.000\napicid\t\t: {}\nflags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 ss syscall nx lm\n\n",
                    i, apic, mhz, apic
                );
            }
            info.into_bytes()
        }
        ProcFileType::Smp => {
            // T-025 E1: estado real del SMP. LEERLO dispara una IPI 0xF0 de
            // ida y vuelta a cada AP en línea: si el AP no la atiende, la
            // línea dice FALLO (prueba que puede fallar).
            use core::fmt::Write;
            use core::sync::atomic::Ordering;
            use crate::arch::smp;
            let mut s = String::new();
            let _ = write!(s, "nucleos_madt: {}\nnucleos_en_linea: {}\nnucleos_planificables: {}\nmadt: 0x{:x}\nlapic_base: 0x{:x}\napic_id_bsp: {}\n",
                smp::nucleos_madt(), smp::nucleos_en_linea(), smp::cpu_count(),
                smp::madt_phys(), smp::lapic_base_fisica(), smp::apic_id_bsp());
            let motivo = smp::motivo_un_nucleo();
            if !motivo.is_empty() { let _ = write!(s, "un_nucleo: {}\n", motivo); }
            let (tlb_env, tlb_ack, tlb_sin) = smp::tlb_estadisticas();
            {
                use core::sync::atomic::Ordering as O;
                let sitio = crate::time::pit::IRQ0_SITIO.load(O::Relaxed);
                let _ = write!(s, "irq0_perdidos: {} irq0_peor_hueco_us: {} irq0_sitio: pid {} syscall {} rip {:#x}\n",
                    crate::time::pit::IRQ0_PERDIDOS.load(O::Relaxed), crate::time::pit::IRQ0_PEOR_HUECO_US.load(O::Relaxed),
                    sitio >> 32, sitio & 0xFFFF_FFFF, crate::time::pit::IRQ0_SITIO_RIP.load(O::Relaxed));
                // [T-026] ¿El peor hueco cayó dentro de un syscall con IF=0
                // (culpa del núcleo) o fuera (el anfitrión no dio CPU al vCPU)?
                let dentro = crate::time::pit::IRQ0_HUECO_DENTRO.load(O::Relaxed);
                let _ = write!(s, "irq0_hueco_tick: {}\n", crate::time::pit::IRQ0_HUECO_TICK.load(O::Relaxed));
                let _ = write!(s, "irq0_hueco_dentro: {} irq0_hueco_ventana_us: {}\n",
                    if dentro == u64::MAX { "sin_medir" } else if dentro == 1 { "1" } else { "0" },
                    crate::time::pit::IRQ0_HUECO_VENTANA_US.load(O::Relaxed));
                let nr_abi = crate::time::pit::VENTANA_PEOR_NR.load(O::Relaxed);
                let _ = write!(s, "ventanas_if0: abiertas {} peor_us {} pid {} abi {} nr {} a1 {:#x} a2 {:#x} a3 {:#x} cpu {} rip_in {:#x} rip_out {:#x}\n",
                    crate::time::pit::VENTANAS_TOTAL.load(O::Relaxed),
                    crate::time::pit::VENTANA_PEOR_US.load(O::Relaxed),
                    crate::time::pit::VENTANA_PEOR_PID.load(O::Relaxed),
                    if (nr_abi >> 32) & 0xF == 1 { "linux" } else { "nativa" },
                    nr_abi & 0xFFFF_FFFF,
                    crate::time::pit::VENTANA_PEOR_A1.load(O::Relaxed),
                    crate::time::pit::VENTANA_PEOR_A2.load(O::Relaxed),
                    crate::time::pit::VENTANA_PEOR_A3.load(O::Relaxed),
                    crate::time::pit::VENTANA_PEOR_CPU.load(O::Relaxed),
                    crate::time::pit::VENTANA_PEOR_RIP_IN.load(O::Relaxed),
                    crate::time::pit::VENTANA_PEOR_RIP_OUT.load(O::Relaxed));
            }
            {
                use core::sync::atomic::Ordering as O;
                let _ = write!(s, "respiros: abiertos {} saltados_irqsave {} peor_tramo_us {} en {}:{} peor_intervalo_us {} shootdowns_ahorrados_en_lote {}\n",
                    crate::mem::paging::RESPIROS_ABIERTOS.load(O::Relaxed), crate::mem::paging::RESPIROS_SALTADOS.load(O::Relaxed),
                    crate::mem::paging::RESPIRO_PEOR_US.load(O::Relaxed), crate::mem::paging::respiro_peor_fichero(),
                    crate::mem::paging::RESPIRO_PEOR_LINEA.load(O::Relaxed), crate::mem::paging::RESPIRO_PEOR_INTERVALO_US.load(O::Relaxed), crate::mem::paging::LOTES_TLB_AHORRADOS.load(O::Relaxed));
            }
            {
                // [T-027] Carreras de PTE detectadas y resueltas: una actualización
                // encontró la entrada cambiada por otra CPU y rehízo la decisión.
                let _ = write!(s, "pte: carreras {}\n",
                    crate::mem::paging::PTE_CARRERAS.load(core::sync::atomic::Ordering::Relaxed));
            }
            {
                // [T-015] Sueños: cuántos, cuántos volvieron antes de tiempo y por
                // cuánto. Un retorno corto sin señal no tiene excusa POSIX.
                use core::sync::atomic::Ordering as O;
                let _ = write!(s, "suenos: totales {} cortos {} con_senal {} con_switch {} peor_deficit_ticks {}\n",
                    crate::syscall::SUENOS.load(O::Relaxed),
                    crate::syscall::SUENOS_CORTOS.load(O::Relaxed),
                    crate::syscall::SUENOS_CON_SENAL.load(O::Relaxed),
                    crate::syscall::SUENOS_CON_CAMBIO.load(O::Relaxed),
                    crate::syscall::SUENO_PEOR_DEFICIT.load(O::Relaxed));
            }
            {
                // [T-028] Reloj y despertador por tiempo, visibles desde fuera:
                // una prueba puede leer `ticks` dos veces con una espera en medio
                // y comprobar que el reloj avanza lo que debe. Si no avanza, la
                // espera colgada no es de la syscall, es del reloj.
                use core::sync::atomic::Ordering as O;
                let _ = write!(s, "reloj: ticks {} ultimo_barrido {} llamadas {} sin_cerrojo {} seguidos_sin_cerrojo {} despertados {} atascada {} peor_seguidos {} peor_hueco {} huecos_20 {} huecos_100 {}\n",
                    crate::time::pit::ticks(),
                    crate::process::scheduler::WAKE_ULTIMO_OK.load(O::Relaxed),
                    crate::process::scheduler::WAKE_LLAMADAS.load(O::Relaxed),
                    crate::process::scheduler::WAKE_SIN_CERROJO.load(O::Relaxed),
                    crate::process::scheduler::WAKE_SEGUIDOS_SIN_CERROJO.load(O::Relaxed),
                    crate::process::scheduler::WAKE_DESPERTADOS.load(O::Relaxed),
                    crate::process::scheduler::ESPERA_ATASCADA.load(O::Relaxed),
                    crate::process::scheduler::WAKE_PEOR_SEGUIDOS.load(O::Relaxed),
                    crate::process::scheduler::WAKE_PEOR_HUECO_TICKS.load(O::Relaxed),
                    crate::process::scheduler::WAKE_HUECOS_20.load(O::Relaxed),
                    crate::process::scheduler::WAKE_HUECOS_100.load(O::Relaxed));
            }
            {
                use core::sync::atomic::Ordering as O;
                let p = &crate::syscall::MMAP_PERFIL;
                let _ = write!(s, "mmap_perfil_mciclos: pte_cruda {} mprotect {} cero {} alloc {} map {} sustituidas {} nuevas {}\n",
                    p[0].load(O::Relaxed) / 1_000_000, p[1].load(O::Relaxed) / 1_000_000, p[2].load(O::Relaxed) / 1_000_000,
                    p[3].load(O::Relaxed) / 1_000_000, p[4].load(O::Relaxed) / 1_000_000, p[5].load(O::Relaxed), p[6].load(O::Relaxed));
            }
            {
                use core::sync::atomic::Ordering as O;
                let p = &crate::mem::marcos::PERFIL;
                let _ = write!(s, "marcos_perfil_mciclos: cerrojo {} recorrido {} limpiar {} palabras {}\n",
                    p[0].load(O::Relaxed) / 1_000_000, p[1].load(O::Relaxed) / 1_000_000, p[2].load(O::Relaxed) / 1_000_000, p[3].load(O::Relaxed));
            }
            {
                use core::sync::atomic::Ordering as O;
                let _ = write!(s, "mmap_fichero_perezoso: tramos_vivos {} registrados {} paginas_desde_fichero {} bytes {}\n",
                    crate::mem::vmas::vivas(), crate::mem::vmas::VMAS_REGISTRADAS.load(O::Relaxed),
                    crate::mem::vmas::PAGINAS_DESDE_FICHERO.load(O::Relaxed), crate::mem::vmas::BYTES_DESDE_FICHERO.load(O::Relaxed));
            }
            let (tlb_tarde, tlb_peor) = smp::tlb_tardios();
            let _ = write!(s, "tlb_shootdowns: {} acuses: {} sin_acuse: {} fallos_pf_espurios: {} acuses_tardios: {} peor_acuse_us: {}\n", tlb_env, tlb_ack, tlb_sin,
                crate::mem::paging::FALLOS_ESPURIOS.load(core::sync::atomic::Ordering::Relaxed), tlb_tarde, tlb_peor);
            // [T-025 E3d] Cerrojo grande del núcleo: tomas y veces que una CPU esperó a otra.
            let _ = write!(s, "cerrojo_grande: tomas {} esperas {}\n",
                crate::arch::bkl::TOMAS.load(core::sync::atomic::Ordering::Relaxed),
                crate::arch::bkl::ESPERAS.load(core::sync::atomic::Ordering::Relaxed));
            // [T-025 E3a] GS por CPU: lecturas de `percpu::actual()` sin GS válido
            // (debe ser 0 tras el arranque; las del arranque temprano se descuentan en la sonda).
            let _ = write!(s, "gs_invalido: {} cpu_actual: {} ultimo_gs: {:#x} p: {:#x} apic: {} rsp: {:#x}\n",
                crate::arch::percpu::GS_INVALIDO.load(core::sync::atomic::Ordering::Relaxed),
                crate::arch::percpu::indice_actual(),
                crate::arch::percpu::GS_INVALIDO_ULTIMO.load(core::sync::atomic::Ordering::Relaxed),
                crate::arch::percpu::GS_INVALIDO_P.load(core::sync::atomic::Ordering::Relaxed),
                crate::arch::percpu::GS_INVALIDO_APIC.load(core::sync::atomic::Ordering::Relaxed),
                crate::arch::percpu::GS_INVALIDO_RSP.load(core::sync::atomic::Ordering::Relaxed));
            for i in 0..smp::MAX_CORES {
                let n = &smp::NUCLEOS[i];
                if !n.presente_madt.load(Ordering::SeqCst) && !n.es_bsp.load(Ordering::SeqCst) { continue; }
                let _ = write!(s, "nucleo {} apic {} visto {} {} ipi_recibidas {}",
                    i, n.apic_id.load(Ordering::SeqCst),
                    n.apic_id_visto.load(Ordering::SeqCst) as i32,
                    if n.es_bsp.load(Ordering::SeqCst) { "bsp" } else if n.en_linea.load(Ordering::SeqCst) { "en_linea" } else { "apagado" },
                    n.ipi_recibidas.load(Ordering::SeqCst));
                // [E3b] TSS propia por CPU: dirección distinta por núcleo y `tr` = 0x28 cargado.
                let _ = write!(s, " tss {:#x}", crate::arch::idt::tss_direccion(i));
                if n.es_bsp.load(Ordering::SeqCst) {
                    let _ = write!(s, " tr {:#x}", crate::arch::idt::tr_actual());
                }
                if n.en_linea.load(Ordering::SeqCst) && !n.es_bsp.load(Ordering::SeqCst) {
                    let (a, d, ok) = smp::probar_ipi(i);
                    let _ = write!(s, " ipi_prueba {} -> {} {}", a, d, if ok { "OK" } else { "FALLO" });
                    let tr = smp::tr_en_ap(i);
                    let _ = write!(s, " tr {:#x} {}", tr, if tr == crate::arch::idt::TSS_SEL { "OK" } else { "FALLO" });
                    // [E3b] Temporizador LAPIC del AP a 100 Hz frente al PIT del BSP
                    // desde que arrancó: la relación debe quedar en [0,85, 1,15].
                    let lapic = crate::arch::percpu::por_indice(i).ticks_lapic.load(Ordering::Relaxed);
                    let pit = crate::time::pit::ticks().saturating_sub(n.pit_al_arrancar.load(Ordering::SeqCst));
                    // Tolerancia ±15 %: QEMU TCG desplanifica hilos de vCPU; en silicio
                    // real ambos relojes son cristales y la desviación es < 1 %.
                    // [85ª] Tercera referencia: el TSC. `tsc` = ticks de 10 ms que han pasado de
                    // verdad desde que arrancó el temporizador. Si `pit` < `tsc` el BSP perdió
                    // interrupciones del PIT (IF=0 demasiado tiempo), no es culpa del LAPIC: el
                    // veredicto `reloj` compara el LAPIC con el TSC; la pérdida del PIT se
                    // publica aparte (`pit_perdidos`) para cazarla, no para esconderla.
                    let hz = crate::time::pit::tsc_hz_estimate();
                    let tsc_d = unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(n.tsc_al_arrancar.load(Ordering::SeqCst));
                    let tsc = if hz > 0 { tsc_d / (hz / 100).max(1) } else { 0 };
                    let referencia = if tsc > 0 { tsc } else { pit };
                    let ok_reloj = referencia >= 50 && lapic * 100 >= referencia * 85 && lapic * 100 <= referencia * 115;
                    let _ = write!(s, " ticks_lapic {} pit {} tsc {} pit_perdidos {} periodo {} reloj {}",
                        lapic, pit, tsc, tsc.saturating_sub(pit), n.lapic_periodo.load(Ordering::SeqCst), if ok_reloj { "OK" } else { "FALLO" });
                }
                s.push('\n');
            }
            s.into_bytes()
        }
        ProcFileType::SmpEstres => {
            // T-025 E2: estrés concurrente REAL de los cerrojos del núcleo: todos
            // los núcleos en línea (AP y BSP a la vez) asignan/firman/liberan
            // marcos físicos y bloques del montículo y pelean por un contador
            // bajo Spinlock. Luego un TLB shootdown con acuse. Cualquier
            // invariante rota -> FALLO.
            use core::fmt::Write;
            use crate::arch::smp;
            let r = smp::estres_concurrente(4000);
            let mut s = String::new();
            let _ = write!(s, "nucleos: {} rondas: {} us: {}\n", r.nucleos, r.rondas, r.us);
            let mut errores = 0u64;
            for i in 0..smp::MAX_CORES {
                if r.ok_por_nucleo[i] == 0 && r.err_por_nucleo[i] == 0 { continue; }
                let _ = write!(s, "nucleo {} ok {} errores {}\n", i, r.ok_por_nucleo[i], r.err_por_nucleo[i]);
                errores += r.err_por_nucleo[i];
            }
            let _ = write!(s, "contador: {} esperado: {}\nmarcos_libres: {} -> {}\nsin_respuesta: {}\ntlb_shootdown: {}\n",
                r.contador, r.contador_esperado, r.libres_antes, r.libres_despues, r.sin_respuesta,
                if r.tlb_ok { "acusado" } else { "SIN ACUSE" });
            let ok = errores == 0 && r.contador == r.contador_esperado && r.libres_antes == r.libres_despues
                && r.sin_respuesta == 0 && r.tlb_ok && r.nucleos == smp::nucleos_en_linea();
            let _ = write!(s, "resultado: {}\n", if ok { "OK" } else { "FALLO" });
            s.into_bytes()
        }
        ProcFileType::Loadavg => {
            let active = process::active_process_count();
            format!("0.02 0.01 0.00 1/{} 10\n", active).into_bytes()
        }
        ProcFileType::Mounts => {
            String::from(
                "/dev/sda0 / fat32 rw,relatime 0 0\nproc /proc procfs rw,nosuid,nodev,noexec,relatime 0 0\ndevfs /dev devfs rw,nosuid,noexec,relatime 0 0\nramfs /tmp ramfs rw,nosuid,nodev,relatime 0 0\n"
            ).into_bytes()
        }
        ProcFileType::Modules => {
            let mut out = String::new();
            let text = crate::lkm::list_modules_text();
            if !text.starts_with("No hay") {
                out.push_str("demo_mod 2888 1 - Live 0x0\n");
            }
            out.into_bytes()
        }
        ProcFileType::Stat => {
            let ticks = crate::time::pit::ticks();
            format!(
                "cpu  {} 0 50 {} 0 0 0 0 0 0\nintr {}\nctxt {}\nbtime 1700000000\nprocesses {}\nprocs_running 1\nprocs_blocked 0\n",
                ticks / 10, ticks, ticks, ticks * 2, process::active_process_count()
            ).into_bytes()
        }
        ProcFileType::NvmeStatus => {
            crate::drivers::storage::nvme::nvme_init();
            crate::drivers::storage::nvme::GLOBAL_NVME.lock().generate_telemetry().into_bytes()
        }
        ProcFileType::PipesStatus => {
            crate::process::get_pipes_report().into_bytes()
        }
        ProcFileType::SignalsStatus => {
            crate::process::signals::get_signals_report().into_bytes()
        }
        ProcFileType::AbiFreeze => {
            crate::system::consagracion::get_abi_report().into_bytes()
        }
        ProcFileType::Certification => {
            crate::system::consagracion::get_certification_report().into_bytes()
        }
        ProcFileType::ReleaseInfo => {
            crate::system::consagracion::get_release_report().into_bytes()
        }
        ProcFileType::SyscallTrace => {
            crate::system::devsuite::get_tracer_report().into_bytes()
        }
        ProcFileType::CpuProfiler => {
            crate::system::devsuite::get_profiler_report().into_bytes()
        }
        ProcFileType::DebugChannel => {
            crate::system::devsuite::get_dbg_report().into_bytes()
        }
        ProcFileType::RayTracing => {
            crate::graphics::raytracing::RAYTRACING_PIPELINE.lock().get_status_report().into_bytes()
        }
        ProcFileType::DriverSandbox => {
            crate::drivers::sandbox::DRIVER_SANDBOX_MANAGER.lock().get_status_report().into_bytes()
        }
        ProcFileType::Riscv64Hal => {
            let mut guard = crate::arch::riscv64::GLOBAL_RISCV64.lock();
            if guard.is_none() {
                let mut h = crate::arch::riscv64::Riscv64Hal::new();
                h.init();
                *guard = Some(h);
            }
            match guard.as_ref() { Some(h) => h.generate_telemetry().into_bytes(), None => alloc::vec::Vec::new() }
        }
        ProcFileType::PqcStatus => {
            crate::arch::post_quantum::POST_QUANTUM.lock().generate_telemetry().into_bytes()
        }
        ProcFileType::EbpfProbe => {
            crate::microkernel::ebpf::ebpf_init();
            crate::microkernel::ebpf::GLOBAL_EBPF.lock().generate_telemetry().into_bytes()
        }
        ProcFileType::RtAudio => {
            crate::drivers::audio::rt_audio::rt_audio_init();
            crate::drivers::audio::rt_audio::GLOBAL_RT_AUDIO.lock().generate_telemetry().into_bytes()
        }
        ProcFileType::WasiComponents => {
            crate::wasm::component_model::wasi_components_init();
            crate::wasm::component_model::GLOBAL_WASI_COMPONENTS.lock().generate_telemetry().into_bytes()
        }
        ProcFileType::PortixDfs => {
            crate::drivers::storage::portix_dfs::portix_dfs_init();
            crate::drivers::storage::portix_dfs::GLOBAL_PORTIX_DFS.lock().generate_telemetry().into_bytes()
        }
        ProcFileType::ProcessStat(pid) => {
            if let Some(snap) = process::get_process_snapshot(pid) {
                let name = core::str::from_utf8(&snap.name[..snap.name_len]).unwrap_or("unknown");
                let state_ch = match snap.state {
                    ProcessState::Running => 'R',
                    // 'T' = detenido por su trazador (job control / ptrace), como en Linux.
                    ProcessState::Stopped => 'T',
                    ProcessState::Ready => 'R',
                    ProcessState::Blocked => 'S',
                    ProcessState::Zombie => 'Z',
                    ProcessState::Dead => 'X',
                };
                let vsize = snap.vmsize_kb * 1024;
                let rss = snap.vmsize_kb / 4;
                format!(
                    "{} ({}) {} {} 0 0 0 0 0 0 0 0 {} 0 0 20 0 1 0 0 {} {} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
                    snap.pid, name, state_ch, snap.ppid, snap.ticks_used, vsize, rss
                ).into_bytes()
            } else {
                Vec::new()
            }
        }
        ProcFileType::ProcessStatus(pid) => {
            if let Some(snap) = process::get_process_snapshot(pid) {
                let name = core::str::from_utf8(&snap.name[..snap.name_len]).unwrap_or("unknown");
                let (state_ch, state_str) = match snap.state {
                    ProcessState::Running => ('R', "running"),
                    ProcessState::Stopped => ('T', "stopped (tracer)"),
                    ProcessState::Ready => ('R', "running"),
                    ProcessState::Blocked => ('S', "sleeping"),
                    ProcessState::Zombie => ('Z', "zombie"),
                    ProcessState::Dead => ('X', "dead"),
                };
                // [T-017] Threads: hilos vivos del grupo de verdad (antes siempre 1).
                let tgid = process::with_process(pid, |p| if p.es_hilo { p.tgid } else { p.pid }).unwrap_or(pid);
                let hilos = 1 + process::hilos_del_grupo(tgid, 0);
                format!(
                    "Name:\t{}\nState:\t{} ({})\nTgid:\t{}\nPid:\t{}\nPPid:\t{}\nUid:\t{} {} {} {}\nGid:\t{} {} {} {}\nVmSize:\t{:8} kB\nVmRSS:\t{:8} kB\nThreads:\t{}\n",
                    name, state_ch, state_str, tgid, snap.pid, snap.ppid,
                    snap.uid, snap.uid, snap.uid, snap.uid,
                    snap.gid, snap.gid, snap.gid, snap.gid,
                    snap.vmsize_kb, snap.vmsize_kb / 4, hilos
                ).into_bytes()
            } else {
                Vec::new()
            }
        }
        ProcFileType::ProcessMaps(pid) => {
            crate::system::devsuite::get_maps_report(pid).into_bytes()
        }
        ProcFileType::ProcessSymbols(pid) => {
            crate::system::symbols::get_symbols_report(pid).into_bytes()
        }
        ProcFileType::ProcessCmdline(pid) => {
            if let Some(snap) = process::get_process_snapshot(pid) {
                let name = core::str::from_utf8(&snap.name[..snap.name_len]).unwrap_or("unknown");
                let mut v = Vec::from(name.as_bytes());
                v.push(0);
                v
            } else {
                Vec::new()
            }
        }
        ProcFileType::Directory(_) => Vec::new(),
    }
}

/// Lista el contenido de un directorio en /proc para getdents.
pub fn list_proc_dir(path: &str) -> Vec<(String, bool)> {
    let mut entries = Vec::new();
    let p = path.trim_end_matches('/');
    if p == "/proc" || p.is_empty() {
        entries.push((String::from("."), true));
        entries.push((String::from(".."), true));
        entries.push((String::from("version"), false));
        entries.push((String::from("uptime"), false));
        entries.push((String::from("meminfo"), false));
        entries.push((String::from("cpuinfo"), false));
        entries.push((String::from("smp"), false));
        entries.push((String::from("smp_estres"), false));
        entries.push((String::from("loadavg"), false));
        entries.push((String::from("mounts"), false));
        entries.push((String::from("modules"), false));
        entries.push((String::from("stat"), false));
        entries.push((String::from("nvme"), false));
        entries.push((String::from("pipes"), false));
        entries.push((String::from("signals"), false));
        entries.push((String::from("syscalls"), false));
        entries.push((String::from("profiler"), false));
        entries.push((String::from("frames"), false));
        entries.push((String::from("dbg"), false));
        entries.push((String::from("abi"), false));
        entries.push((String::from("certification"), false));
        entries.push((String::from("release"), false));
        entries.push((String::from("raytracing"), false));
        entries.push((String::from("driver_sandbox"), false));
        entries.push((String::from("riscv64_hal"), false));
        entries.push((String::from("pqc_status"), false));
        entries.push((String::from("ebpf"), false));
        entries.push((String::from("rt_audio"), false));
        entries.push((String::from("wasi_components"), false));
        entries.push((String::from("portix_dfs"), false));
        entries.push((String::from("self"), true));
        for pid in process::get_all_active_pids() {
            entries.push((format!("{}", pid), true));
        }
    } else if p == "/proc/self" || p.starts_with("/proc/") {
        let rel = &p[6..];
        let valid = if rel == "self" {
            true
        } else if let Ok(pid) = rel.parse::<u64>() {
            process::get_process_snapshot(pid).is_some()
        } else {
            false
        };
        if valid {
            entries.push((String::from("."), true));
            entries.push((String::from(".."), true));
            entries.push((String::from("stat"), false));
            entries.push((String::from("status"), false));
            entries.push((String::from("cmdline"), false));
            entries.push((String::from("maps"), false));
            entries.push((String::from("symbols"), false));
        }
    }
    entries
}

/// Obtiene los metadatos Stat de una entrada en /proc.
pub fn stat_proc(path: &str) -> Result<Stat, i32> {
    let mut st = Stat::default();
    let (ft, is_dir) = match resolve_proc_path(path) {
        Some(x) => x,
        None => return Err(-2), // -ENOENT
    };
    if is_dir {
        st.st_mode = 0o040555; // S_IFDIR | 0555
        st.st_nlink = 2;
        st.st_size = 0;
    } else {
        let content = generate_content(ft);
        st.st_mode = 0o100444; // S_IFREG | 0444
        st.st_nlink = 1;
        st.st_size = content.len() as i64;
    }
    st.st_blksize = 512;
    st.st_blocks = (st.st_size + 511) / 512;
    Ok(st)
}
