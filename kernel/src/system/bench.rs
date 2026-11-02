// kernel/src/system/bench.rs — [T-011] Mediciones REALES del propio sistema.
//
// Sustituye a las cifras afirmadas ("48 ns vs Linux", "170x menos RAM que
// Windows") por medidas tomadas en esta máquina, en este arranque:
//   - arranque hasta escritorio: TSC leído cuando el compositor presenta el
//     escritorio (`present done`), convertido con la frecuencia calibrada
//     contra el PIT; incluye firmware y cargador porque el TSC cuenta desde
//     el reset.
//   - memoria física en uso: del asignador de marcos (`mem::marcos`).
//   - lectura ext2: MB/s leyendo entero un fichero grande de la raíz por el
//     contrato `SistemaDeFicheros` (VFS -> ext2 -> ATA), medido con el TSC.
// La latencia de syscall se mide desde Ring 3 por `/bin/portix-bench`
// (getpid x N por TSC): el núcleo no puede medirla honestamente desde Ring 0.
//
// No hay comparativas con otros sistemas: no se pueden medir aquí en
// igualdad de condiciones, así que no se afirman.

use core::sync::atomic::{AtomicU64, Ordering};
use alloc::string::String;
use alloc::format;
use crate::arch::Spinlock;

static TSC_ESCRITORIO: AtomicU64 = AtomicU64::new(0);

/// Llamar una sola vez cuando el escritorio queda presentado.
pub fn marcar_escritorio_listo() {
    if TSC_ESCRITORIO.load(Ordering::Relaxed) == 0 {
        TSC_ESCRITORIO.store(crate::system::devsuite::read_tsc(), Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Default)]
pub struct Medidas {
    pub arranque_ms: u64,      // 0 = no disponible (TSC sin calibrar)
    pub ram_usada_kib: u64,    // fijo del nucleo + monticulo usado + marcos de usuario
    pub ram_total_kib: u64,    // fijo del nucleo + marcos administrables
    pub ram_pico_kib: u64,     // pico de marcos de usuario
    pub ram_fija_kib: u64,     // nucleo + monticulo (64 MiB) + back buffer: reservado siempre
    pub heap_vivas: u64,       // asignaciones vivas del monticulo (allocs - frees)
    pub heap_usado_kib: u64,   // bytes entregados por el buddy (contador exacto)
    pub heap_libre_kib: u64,   // suma de bloques libres por orden (comprobacion cruzada)
    pub marcos_usados_kib: u64,
    pub ext2_bytes: u64,
    pub ext2_us: u64,
    pub ext2_kbps: u64,        // KiB/s (0 = no se pudo leer)
    pub ext2_us2: u64,         // segunda pasada (caliente)
    pub ext2_kbps2: u64,
    pub cache_aciertos: u64,
    pub cache_fallos: u64,
    pub cache_bytes: u64,
    pub tsc_hz: u64,
}

static ULTIMAS: Spinlock<Option<Medidas>> = Spinlock::new(None);

/// Última medición realizada en este arranque (para el panel gráfico).
pub fn ultimas() -> Option<Medidas> { *ULTIMAS.lock() }

/// Fichero grande de la raíz ext2 usado para la medida de lectura.
pub const FICHERO_LECTURA: &str = "/bin/test_posix";

/// Realiza las mediciones ahora y las deja en `ultimas()`.
pub fn medir() -> Medidas {
    let mut m = Medidas::default();
    m.tsc_hz = crate::time::pit::tsc_hz_estimate();
    let t_esc = TSC_ESCRITORIO.load(Ordering::Relaxed);
    if m.tsc_hz > 0 && t_esc > 0 {
        m.arranque_ms = t_esc / (m.tsc_hz / 1000).max(1);
    }
    // Memoria fisica: (a) zona fija reservada por el nucleo (codigo, monticulo
    // de 64 MiB, back buffer) = todo lo que hay por debajo de MARCOS_BASE;
    // (b) del monticulo, lo realmente ocupado (contando los bloques libres por
    // orden del buddy, sin tocar allocator.rs); (c) marcos de usuario.
    let (total, libres, pico, _) = crate::mem::marcos::estadisticas();
    m.ram_fija_kib = (crate::mem::marcos::MARCOS_BASE as u64) / 1024;
    // Monticulo: bytes entregados (contador exacto) y, como comprobacion
    // cruzada, HEAP_SIZE - suma de bloques libres por orden; si no coinciden
    // los contadores del buddy estan rotos y la prueba de la suite lo detecta.
    let st = &crate::mem::allocator::ALLOC_STATS;
    let a = st.total_allocs.load(Ordering::Relaxed) as u64;
    let f = st.total_frees.load(Ordering::Relaxed) as u64;
    m.heap_vivas = a.saturating_sub(f);
    m.heap_usado_kib = (st.bytes_en_uso.load(Ordering::Relaxed) as u64) / 1024;
    let mut libre: u64 = 0;
    for (i, fb) in st.free_blocks.iter().enumerate() {
        libre = libre.saturating_add((fb.load(Ordering::Relaxed) as u64) << (crate::mem::MIN_ORDER + i));
    }
    m.heap_libre_kib = libre / 1024;
    m.marcos_usados_kib = (total.saturating_sub(libres) as u64) * 4;
    m.ram_total_kib = m.ram_fija_kib + (total as u64) * 4;
    m.ram_usada_kib = m.ram_fija_kib + m.marcos_usados_kib;
    m.ram_pico_kib = (pico as u64) * 4;

    // Lectura secuencial completa por el contrato, en trozos de 64 KiB, DOS
    // veces: la primera suele venir del disco, la segunda de la caché de
    // bloques (T-001). Se publican ambas y los aciertos/fallos de la caché.
    if let Some(tam) = crate::drivers::storage::vfs::tam_fs(FICHERO_LECTURA) {
        let mut buf = alloc::vec![0u8; 65536];
        let mut pasada = |m: &mut Medidas, segunda: bool| {
            let mut leido = 0u64;
            let t0 = crate::system::devsuite::read_tsc();
            while (leido as usize) < tam {
                match crate::drivers::storage::vfs::leer_fs(FICHERO_LECTURA, leido, &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => leido += n as u64,
                }
            }
            let t1 = crate::system::devsuite::read_tsc();
            if m.tsc_hz == 0 { return; }
            let us = t1.wrapping_sub(t0) / (m.tsc_hz / 1_000_000).max(1);
            let kbps = if us > 0 { (leido * 1_000_000 / 1024) / us } else { 0 };
            if segunda { m.ext2_us2 = us; m.ext2_kbps2 = kbps; }
            else { m.ext2_bytes = leido; m.ext2_us = us; m.ext2_kbps = kbps; }
        };
        // Fria de verdad: se descarta la cache antes (es write-through, no se pierde nada).
        let _ = crate::drivers::storage::vfs::con_fs(FICHERO_LECTURA, |fs| fs.descartar_cache_bloques());
        pasada(&mut m, false);
        pasada(&mut m, true);
    }
    if let Some(Some((a, f, bytes))) = crate::drivers::storage::vfs::con_fs(FICHERO_LECTURA, |fs| fs.cache_bloques()) {
        m.cache_aciertos = a; m.cache_fallos = f; m.cache_bytes = bytes as u64;
    }
    *ULTIMAS.lock() = Some(m);
    m
}

/// Líneas de texto (sin color) con las medidas, para terminal y panel.
pub fn lineas(m: &Medidas) -> [String; 4] {
    let arranque = if m.arranque_ms > 0 {
        format!("[BENCH-REAL] arranque hasta escritorio: {} ms (TSC desde reset, incluye firmware; TSC {} MHz)", m.arranque_ms, m.tsc_hz / 1_000_000)
    } else {
        String::from("[BENCH-REAL] arranque hasta escritorio: no disponible (TSC sin calibrar)")
    };
    let ram = format!("[BENCH-REAL] memoria fisica en uso: {} KiB de {} KiB (reserva fija del nucleo {} KiB, que incluye un monticulo de {} KiB con {} KiB ocupados en {} asignaciones vivas y {} KiB libres; marcos de usuario {} KiB, pico {} KiB)",
        m.ram_usada_kib, m.ram_total_kib, m.ram_fija_kib, (crate::mem::HEAP_SIZE as u64) / 1024, m.heap_usado_kib, m.heap_vivas, m.heap_libre_kib, m.marcos_usados_kib, m.ram_pico_kib);
    let ext2 = if m.ext2_kbps > 0 {
        format!("[BENCH-REAL] lectura ext2 ({}): {} bytes en {} us = {} KiB/s por VFS->ext2->ATA; segunda pasada {} us = {} KiB/s; cache de bloques {} KiB: {} aciertos, {} fallos",
            FICHERO_LECTURA, m.ext2_bytes, m.ext2_us, m.ext2_kbps, m.ext2_us2, m.ext2_kbps2, m.cache_bytes / 1024, m.cache_aciertos, m.cache_fallos)
    } else {
        format!("[BENCH-REAL] lectura ext2 ({}): no medida (fichero no legible)", FICHERO_LECTURA)
    };
    let sys = String::from("[BENCH-REAL] latencia de syscall: medir desde Ring 3 con `run portix-bench` (getpid x100000 por TSC)");
    [arranque, ram, ext2, sys]
}
