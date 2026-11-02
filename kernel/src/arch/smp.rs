//! Multiprocesamiento simétrico (SMP) REAL — T-025 (ROADMAP 5b), entrega 1.
//!
//! Qué hace de verdad este módulo (y qué se puede comprobar desde fuera):
//!
//! 1. Localiza la RSDP (tabla de firmware entregada por el cargador UEFI o
//!    búsqueda en EBDA / 0xE0000-0xFFFFF en BIOS), valida sumas de control,
//!    recorre la XSDT/RSDT y lee la **MADT**. Sólo cuenta los procesadores
//!    cuyas entradas LAPIC están habilitadas u "online capable". Si no hay
//!    MADT, **no se inventa nada**: se sigue con un núcleo y se declara el
//!    motivo por serie (`[SMP] 1 nucleo: <motivo>`).
//! 2. Programa la LAPIC del BSP (base leída de la MADT), copia el trampolín
//!    de `smp_trampolin.asm` a 0x8000 y arranca cada AP con INIT-SIPI-SIPI
//!    escribiendo el ICR real. El AP recorre modo real → protegido → largo,
//!    carga la GDT/IDT del núcleo, habilita su LAPIC y queda en `hlt` con
//!    interrupciones activas.
//! 3. Cada AP en línea se verifica con una IPI (vector 0xF0) cuyo manejador
//!    corre **en el AP** e incrementa un contador atómico que el BSP lee.
//!    `/proc/smp` repite esa prueba a demanda y puede fallar.
//!
//! Lo que todavía NO hace (entregas 2 y 3 de T-025): los AP no ejecutan
//! procesos; `cpu_count()` sigue devolviendo los núcleos *planificables* (1)
//! para no mentir a `nproc`/`sysconf`. `nucleos_en_linea()` da los núcleos
//! realmente arrancados.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use crate::arch::spinlock::Spinlock;
use crate::drivers::serial;
use crate::mem::paging;

// ── Registros de la LAPIC (offsets sobre la base MMIO) ──────────────────────
pub const LAPIC_DEFAULT_BASE: u64 = 0xFEE0_0000;
pub const LAPIC_ID: u32 = 0x0020;
pub const LAPIC_VERSION: u32 = 0x0030;
pub const LAPIC_TPR: u32 = 0x0080;
pub const LAPIC_EOI: u32 = 0x00B0;
pub const LAPIC_SVR: u32 = 0x00F0;
pub const LAPIC_ESR: u32 = 0x0280;
pub const LAPIC_ICR_LOW: u32 = 0x0300;
pub const LAPIC_ICR_HIGH: u32 = 0x0310;
pub const LAPIC_LVT_TIMER: u32 = 0x0320;
pub const LAPIC_LVT_LINT0: u32 = 0x0350;
pub const LAPIC_LVT_LINT1: u32 = 0x0360;
pub const LAPIC_LVT_ERROR: u32 = 0x0370;
pub const LAPIC_TIMER_INIT: u32 = 0x0380;
pub const LAPIC_TIMER_CURR: u32 = 0x0390;
pub const LAPIC_TIMER_DIV: u32 = 0x03E0;

// ── Campos del ICR ──────────────────────────────────────────────────────────
pub const ICR_FIXED: u32 = 0x0000_0000;
pub const ICR_INIT: u32 = 0x0000_0500;
pub const ICR_STARTUP: u32 = 0x0000_0600;
pub const ICR_LEVEL_ASSERT: u32 = 0x0000_4000;
pub const ICR_TRIGGER_LEVEL: u32 = 0x0000_8000;
pub const ICR_DELIVERY_PENDING: u32 = 0x0000_1000;
pub const ICR_DEST_ALL_EXCLUDING_SELF: u32 = 0x000C_0000;

/// Vectores de IPI (definidos junto a la IDT; aquí sólo alias).
pub const IPI_VECTOR_PRUEBA: u8 = crate::arch::idt::VECTOR_IPI_PRUEBA;
pub const IPI_VECTOR_TLB_FLUSH: u8 = crate::arch::idt::VECTOR_IPI_TLB;
pub const IPI_VECTOR_RESCHEDULE: u8 = crate::arch::idt::VECTOR_IPI_REPLANIFICAR;

/// Dirección física del trampolín (página libre < 1 MiB una vez arrancado el
/// núcleo: stage2 vivía ahí y ya terminó; reservada en `ZONAS_IDENTIDAD_RESERVADAS`).
pub const TRAMPOLIN_BASE: usize = 0x8000;
const TRAMPOLIN_BIN: &[u8] = include_bytes!("../../../build/smp_trampolin.bin");
const D_CR3: usize = 0xF00;
const D_PILA: usize = 0xF08;
const D_ENTRADA: usize = 0xF10;
const D_INDICE: usize = 0xF18;
const D_CR4: usize = 0xF20;
const D_EFER: usize = 0xF28;
const D_SELLO: usize = 0xF30;
const PILA_AP_BYTES: usize = 64 * 1024;

pub const MAX_CORES: usize = 16;
pub const MAX_CPUS: usize = MAX_CORES;

// ── Estado por núcleo (todo atómico: lo escriben BSP y AP a la vez) ─────────
pub struct NucleoEstado {
    /// APIC ID leído de la MADT (y confirmado por el propio AP al arrancar).
    pub apic_id: AtomicU32,
    /// APIC ID que el AP leyó de SU LAPIC al entrar en Rust (prueba de que es él).
    pub apic_id_visto: AtomicU32,
    pub presente_madt: AtomicBool,
    pub en_linea: AtomicBool,
    pub es_bsp: AtomicBool,
    /// IPI atendidas por este núcleo (incrementa el manejador que corre en él).
    pub ipi_recibidas: AtomicU64,
    pub ipi_prueba: AtomicU64,
    pub ipi_tlb: AtomicU64,
    pub ipi_replanificar: AtomicU64,
    /// [E3b] cuenta inicial del temporizador LAPIC (divisor 16) para 10 ms; 0 = no calibrado.
    pub lapic_periodo: AtomicU64,
    /// [E3b] ticks del PIT (BSP) cuando arrancó el temporizador LAPIC de este AP.
    pub pit_al_arrancar: AtomicU64,
    /// [85ª] TSC al arrancar el temporizador: tercera referencia para saber si
    /// el que se desvía es el LAPIC del AP o el PIT del BSP (ticks perdidos con IF=0).
    pub tsc_al_arrancar: AtomicU64,
    /// [E3c] cima de la pila propia del AP (para volver al bucle ocioso).
    pub pila_top: AtomicU64,
    /// [E3c] procesos arrancados en este núcleo desde su bucle ocioso.
    pub procesos_arrancados: AtomicU64,
    /// [E3c] veces que un proceso salió/bloqueó aquí y el núcleo volvió a ocioso.
    pub vueltas_ocioso: AtomicU64,
    /// Contador de tareas asignadas (compatibilidad con la telemetría de la UI).
    pub tareas: AtomicUsize,
    /// [E2] Tarea de núcleo encargada por el BSP: fn(arg) -> u64. Estado:
    /// 0 libre, 1 pendiente, 2 ejecutando, 3 terminada (resultado válido).
    pub tarea_fn: AtomicUsize,
    pub tarea_arg: AtomicUsize,
    pub tarea_estado: AtomicU32,
    pub tarea_resultado: AtomicU64,
    pub tareas_ejecutadas: AtomicU64,
}

impl NucleoEstado {
    const fn nuevo() -> Self {
        Self {
            apic_id: AtomicU32::new(u32::MAX),
            apic_id_visto: AtomicU32::new(u32::MAX),
            presente_madt: AtomicBool::new(false),
            en_linea: AtomicBool::new(false),
            es_bsp: AtomicBool::new(false),
            ipi_recibidas: AtomicU64::new(0),
            ipi_prueba: AtomicU64::new(0),
            ipi_tlb: AtomicU64::new(0),
            ipi_replanificar: AtomicU64::new(0),
            lapic_periodo: AtomicU64::new(0),
            pit_al_arrancar: AtomicU64::new(0),
            tsc_al_arrancar: AtomicU64::new(0),
            pila_top: AtomicU64::new(0),
            procesos_arrancados: AtomicU64::new(0),
            vueltas_ocioso: AtomicU64::new(0),
            tareas: AtomicUsize::new(0),
            tarea_fn: AtomicUsize::new(0),
            tarea_arg: AtomicUsize::new(0),
            tarea_estado: AtomicU32::new(0),
            tarea_resultado: AtomicU64::new(0),
            tareas_ejecutadas: AtomicU64::new(0),
        }
    }
}

macro_rules! nucleos_vacios {
    () => { [
        NucleoEstado::nuevo(), NucleoEstado::nuevo(), NucleoEstado::nuevo(), NucleoEstado::nuevo(),
        NucleoEstado::nuevo(), NucleoEstado::nuevo(), NucleoEstado::nuevo(), NucleoEstado::nuevo(),
        NucleoEstado::nuevo(), NucleoEstado::nuevo(), NucleoEstado::nuevo(), NucleoEstado::nuevo(),
        NucleoEstado::nuevo(), NucleoEstado::nuevo(), NucleoEstado::nuevo(), NucleoEstado::nuevo(),
    ] };
}

pub static NUCLEOS: [NucleoEstado; MAX_CORES] = nucleos_vacios!();
/// Núcleos listados en la MADT (habilitados u online-capable). 0 = sin MADT.
static N_MADT: AtomicUsize = AtomicUsize::new(0);
/// Núcleos realmente en línea (BSP incluido).
static N_EN_LINEA: AtomicUsize = AtomicUsize::new(1);
static LAPIC_BASE: AtomicU64 = AtomicU64::new(0);
static MADT_PHYS: AtomicUsize = AtomicUsize::new(0);
static APIC_ID_BSP: AtomicU32 = AtomicU32::new(0);
static INICIALIZADO: AtomicBool = AtomicBool::new(false);
/// Motivo (texto fijo) por el que se sigue con un solo núcleo; vacío si hay AP.
static MOTIVO_UN_NUCLEO: Spinlock<&'static str> = Spinlock::new("");
/// Serializa el uso del ICR (y del bloque de datos del trampolín).
static ICR_LOCK: Spinlock<()> = Spinlock::new(());

// ── Acceso MMIO a la LAPIC ──────────────────────────────────────────────────
#[inline]
fn lapic_base() -> usize {
    let b = LAPIC_BASE.load(Ordering::Relaxed);
    (if b == 0 { LAPIC_DEFAULT_BASE } else { b }) as usize
}

#[inline]
pub fn lapic_leer(reg: u32) -> u32 {
    unsafe { core::ptr::read_volatile((lapic_base() + reg as usize) as *const u32) }
}

#[inline]
pub fn lapic_escribir(reg: u32, val: u32) {
    unsafe { core::ptr::write_volatile((lapic_base() + reg as usize) as *mut u32, val) }
}

/// APIC ID del núcleo que ejecuta esta función (registro de SU LAPIC).
#[inline]
pub fn apic_id_actual() -> u32 {
    lapic_leer(LAPIC_ID) >> 24
}

/// Asegura que la página MMIO de la LAPIC esté mapeada (identidad, sin caché)
/// en el PML4 maestro. Devuelve `false` si no se pudo.
fn asegurar_mapeo_lapic(base: u64) -> bool {
    let cr3 = paging::KERNEL_CR3.load(Ordering::SeqCst);
    if cr3 == 0 { return false; }
    if paging::translate(cr3, base as usize).is_some() { return true; }
    let flags = paging::PRESENT | paging::WRITABLE | paging::CACHE_DISABLE
        | paging::WRITE_THROUGH | paging::nx_bit();
    match paging::map_page(cr3, base as usize, base as usize, flags) {
        Ok(()) => {
            unsafe { core::arch::asm!("invlpg [{}]", in(reg) base as usize, options(nostack)); }
            true
        }
        Err(_) => false,
    }
}

// ── Esperas cortas (TSC calibrado contra el PIT) ────────────────────────────
fn tsc_hz() -> u64 {
    let hz = crate::time::pit::tsc_hz_cacheado();
    if hz != 0 { return hz; }
    // tsc_hz_estimate hace sti/hlt/cli: conservar el estado de IF del llamante.
    let rflags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) rflags, options(nomem)); }
    let hz = crate::time::pit::tsc_hz_estimate();
    if rflags & (1 << 9) != 0 { unsafe { core::arch::asm!("sti", options(nomem, nostack)); } }
    if hz == 0 { 2_000_000_000 } else { hz }
}

fn esperar_us(us: u64) {
    let hz = tsc_hz();
    let ciclos = hz / 1_000_000 * us;
    let t0 = unsafe { core::arch::x86_64::_rdtsc() };
    while unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(t0) < ciclos {
        core::hint::spin_loop();
    }
}

// ── ACPI: RSDP → XSDT/RSDT → MADT ───────────────────────────────────────────
#[inline]
fn leer_u8(p: usize) -> u8 { unsafe { core::ptr::read_volatile(paging::phys_to_virt(p) as *const u8) } }
#[inline]
fn leer_u16(p: usize) -> u16 { unsafe { core::ptr::read_unaligned(paging::phys_to_virt(p) as *const u16) } }
#[inline]
fn leer_u32(p: usize) -> u32 { unsafe { core::ptr::read_unaligned(paging::phys_to_virt(p) as *const u32) } }
#[inline]
fn leer_u64(p: usize) -> u64 { unsafe { core::ptr::read_unaligned(paging::phys_to_virt(p) as *const u64) } }

fn suma_cero(p: usize, len: usize) -> bool {
    if len == 0 || len > 1 << 20 { return false; }
    let mut s: u8 = 0;
    for i in 0..len { s = s.wrapping_add(leer_u8(p + i)); }
    s == 0
}

fn rsdp_valida(p: usize) -> bool {
    if p == 0 || p > 0xFFFF_F000 { return false; }
    for (i, b) in b"RSD PTR ".iter().enumerate() {
        if leer_u8(p + i) != *b { return false; }
    }
    if !suma_cero(p, 20) { return false; }
    let rev = leer_u8(p + 15);
    if rev >= 2 {
        let len = leer_u32(p + 20) as usize;
        if len < 36 || len > 4096 || !suma_cero(p, len) { return false; }
    }
    true
}

/// Devuelve (dirección física de la RSDP, origen).
fn buscar_rsdp() -> Option<(usize, &'static str)> {
    // 1. Tabla entregada por el cargador (UEFI: ConfigurationTable).
    if let Some(info) = crate::bootinfo::get() {
        let tablas = crate::bootinfo::firmware_tables(info);
        serial::write_str("[SMP] tablas de firmware del cargador: ");
        serial::write_usize(tablas.len());
        for e in tablas {
            serial::write_str(" (tipo ");
            serial::write_usize(e.kind as usize);
            serial::write_str(" en ");
            serial::write_hex(e.address as usize);
            serial::write_str(")");
        }
        serial::write_str("\n");
        for e in tablas {
            if e.kind == crate::bootinfo::FW_TABLE_ACPI_RSDP && rsdp_valida(e.address as usize) {
                return Some((e.address as usize, "cargador"));
            }
        }
    } else {
        serial::write_str("[SMP] sin bootinfo valido del cargador\n");
    }
    // 2. BIOS: EBDA (primer KiB) y 0xE0000-0xFFFFF, en límites de 16 bytes.
    let ebda = (leer_u16(0x40E) as usize) << 4;
    if ebda >= 0x8_0000 && ebda < 0xA_0000 {
        let mut p = ebda;
        while p < ebda + 1024 { if rsdp_valida(p) { return Some((p, "EBDA")); } p += 16; }
    }
    let mut p = 0xE_0000usize;
    while p < 0x10_0000 { if rsdp_valida(p) { return Some((p, "BIOS")); } p += 16; }
    None
}

fn firma_en(p: usize, firma: &[u8; 4]) -> bool {
    firma.iter().enumerate().all(|(i, b)| leer_u8(p + i) == *b)
}

/// Busca la MADT a partir de la RSDP. Devuelve su dirección física.
fn buscar_madt(rsdp: usize) -> Option<usize> {
    let rev = leer_u8(rsdp + 15);
    let xsdt = if rev >= 2 { leer_u64(rsdp + 24) as usize } else { 0 };
    let rsdt = leer_u32(rsdp + 16) as usize;
    let (tabla, ancho, firma): (usize, usize, &[u8; 4]) =
        if xsdt != 0 && xsdt < 0x1_0000_0000 && firma_en(xsdt, b"XSDT") { (xsdt, 8, b"XSDT") }
        else if rsdt != 0 && firma_en(rsdt, b"RSDT") { (rsdt, 4, b"RSDT") }
        else { return None; };
    let len = leer_u32(tabla + 4) as usize;
    if len < 36 || len > 64 * 1024 || !suma_cero(tabla, len) { return None; }
    let _ = firma;
    let n = (len - 36) / ancho;
    for i in 0..n {
        let e = tabla + 36 + i * ancho;
        let dir = if ancho == 8 { leer_u64(e) as usize } else { leer_u32(e) as usize };
        if dir == 0 || dir >= 0x1_0000_0000 { continue; }
        if firma_en(dir, b"APIC") {
            let l = leer_u32(dir + 4) as usize;
            if l >= 44 && l <= 64 * 1024 && suma_cero(dir, l) { return Some(dir); }
        }
    }
    None
}

/// Recorre los registros de la MADT: rellena `NUCLEOS` y la base LAPIC.
/// Devuelve el número de procesadores habilitados u online-capable.
fn parsear_madt(madt: usize) -> usize {
    let len = leer_u32(madt + 4) as usize;
    let mut lapic = leer_u32(madt + 36) as u64;
    let mut n = 0usize;
    let mut p = madt + 44;
    let fin = madt + len;
    while p + 2 <= fin {
        let tipo = leer_u8(p);
        let l = leer_u8(p + 1) as usize;
        if l < 2 || p + l > fin { break; }
        match tipo {
            0 if l >= 8 => {
                let apic = leer_u8(p + 3) as u32;
                let flags = leer_u32(p + 4);
                if flags & 0x3 != 0 && n < MAX_CORES {
                    NUCLEOS[n].apic_id.store(apic, Ordering::SeqCst);
                    NUCLEOS[n].presente_madt.store(true, Ordering::SeqCst);
                    n += 1;
                }
            }
            5 if l >= 12 => { lapic = leer_u64(p + 4); }
            9 if l >= 16 => {
                let apic = leer_u32(p + 4);
                let flags = leer_u32(p + 8);
                // x2APIC: sólo se acepta si cabe en 8 bits (modo xAPIC del ICR).
                if flags & 0x3 != 0 && apic < 255 && n < MAX_CORES
                    && !(0..n).any(|i| NUCLEOS[i].apic_id.load(Ordering::SeqCst) == apic) {
                    NUCLEOS[n].apic_id.store(apic, Ordering::SeqCst);
                    NUCLEOS[n].presente_madt.store(true, Ordering::SeqCst);
                    n += 1;
                }
            }
            _ => {}
        }
        p += l;
    }
    LAPIC_BASE.store(lapic & !0xFFF, Ordering::SeqCst);
    n
}

// ── Entrada de los AP (llamada desde el trampolín en modo largo) ────────────
#[no_mangle]
pub extern "C" fn ap_entrada_rust(indice: u64) -> ! {
    let idx = (indice as usize).min(MAX_CORES - 1);
    // [E3b] Cima de la pila de este AP (la dejó el BSP en el trampolín y aún no
    // la ha pisado el siguiente arranque: el BSP espera a `en_linea`).
    let pila_top = leer_tramp(D_PILA);
    NUCLEOS[idx].pila_top.store(pila_top, Ordering::SeqCst);
    unsafe { crate::arch::idt::cargar_tablas_ap(idx, pila_top); }
    // [T-025 E3a] GS por CPU del AP antes de cualquier escritura por la serie
    // (cerrojo por índice de CPU) o ISR/IPI que lo consulte.
    crate::arch::percpu::instalar(idx, apic_id_actual());
    // LAPIC del AP: habilitar (SVR.bit8) con vector espurio 0xFF, TPR = 0,
    // LINT0/LINT1 enmascarados (el PIC sólo llega al BSP), temporizador parado.
    lapic_escribir(LAPIC_SVR, 0x100 | crate::arch::idt::VECTOR_IPI_ESPURIO as u32);
    lapic_escribir(LAPIC_TPR, 0);
    lapic_escribir(LAPIC_LVT_LINT0, 1 << 16);
    lapic_escribir(LAPIC_LVT_LINT1, 1 << 16);
    lapic_escribir(LAPIC_LVT_TIMER, 1 << 16);
    lapic_escribir(LAPIC_ESR, 0);
    // [E3b] Temporizador LAPIC propio a 100 Hz (como el PIT del BSP), calibrado
    // contra el TSC: divisor 16, cuenta regresiva durante 50 ms (÷5 → 10 ms) y se programa
    // ese valor en modo periódico. Si la calibración da 0 el temporizador se
    // queda parado y `/proc/smp` lo delata (`ticks_lapic` no avanza).
    lapic_escribir(LAPIC_TIMER_DIV, 0x3);
    lapic_escribir(LAPIC_TIMER_INIT, 0xFFFF_FFFF);
    // 50 ms de calibración (no 10): bajo TCG el hilo de la vCPU puede quedar
    // desplanificado por el anfitrión y una muestra corta se desvía un 15 %.
    esperar_us(50_000);
    let transcurrido = 0xFFFF_FFFFu32.wrapping_sub(lapic_leer(LAPIC_TIMER_CURR)) / 5;
    lapic_escribir(LAPIC_TIMER_INIT, 0);
    NUCLEOS[idx].lapic_periodo.store(transcurrido as u64, Ordering::SeqCst);
    if transcurrido > 0 {
        NUCLEOS[idx].pit_al_arrancar.store(crate::time::pit::ticks(), Ordering::SeqCst);
        NUCLEOS[idx].tsc_al_arrancar.store(unsafe { core::arch::x86_64::_rdtsc() }, Ordering::SeqCst);
        lapic_escribir(LAPIC_LVT_TIMER, (1 << 17) | crate::arch::idt::VECTOR_TEMPORIZADOR_LAPIC as u32);
        lapic_escribir(LAPIC_TIMER_INIT, transcurrido);
    }
    let visto = apic_id_actual();
    NUCLEOS[idx].apic_id_visto.store(visto, Ordering::SeqCst);
    NUCLEOS[idx].en_linea.store(true, Ordering::SeqCst);
    N_EN_LINEA.fetch_add(1, Ordering::SeqCst);
    ap_bucle_ocioso(idx as u64)
}

/// [E3c] Bucle ocioso de un AP: tareas de núcleo encargadas por el BSP (E2) y
/// `hlt`. Con la planificación SMP activa, el tick del LAPIC (`ap_tick_rust`)
/// arranca aquí procesos `Ready`; cuando el proceso sale o no queda nada que
/// ejecutar, `ap_ir_a_ocioso` vuelve a esta función con la pila original.
#[no_mangle]
pub extern "C" fn ap_bucle_ocioso(indice: u64) -> ! {
    let idx = (indice as usize).min(MAX_CORES - 1);
    let n = &NUCLEOS[idx];
    // [E3d] Ya sobre la pila propia del AP: el proceso que se abandonó en
    // `ap_ir_a_ocioso` puede reclamarlo otra CPU.
    crate::process::scheduler::liberar_contexto_saliente();
    loop {
        // [E2] Tarea encargada por el BSP (la IPI 0xF2 solo nos despierta del hlt).
        if n.tarea_estado.compare_exchange(1, 2, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            let f = n.tarea_fn.load(Ordering::SeqCst);
            let arg = n.tarea_arg.load(Ordering::SeqCst);
            let r = if f != 0 {
                let f: fn(usize) -> u64 = unsafe { core::mem::transmute(f) };
                f(arg)
            } else { 0 };
            n.tarea_resultado.store(r, Ordering::SeqCst);
            n.tareas_ejecutadas.fetch_add(1, Ordering::SeqCst);
            n.tarea_estado.store(3, Ordering::SeqCst);
            continue;
        }
        unsafe { core::arch::asm!("sti; hlt", options(nomem, nostack)); }
    }
}

/// [E3b] `tr` cargado en el AP `idx` (0 si no contesta): se ejecuta EN el AP.
pub fn tr_en_ap(idx: usize) -> u16 {
    fn leer(_a: usize) -> u64 { crate::arch::idt::tr_actual() as u64 }
    if !ejecutar_en_ap(idx, leer, 0) { return 0; }
    esperar_tarea(idx, 200_000).unwrap_or(0) as u16
}

/// [E2] Encarga `f(arg)` al AP `idx` (se ejecuta en su bucle ocioso, con
/// interrupciones activas, no dentro de la ISR). `false` si el AP no está en
/// línea o aún tiene una tarea sin recoger.
pub fn ejecutar_en_ap(idx: usize, f: fn(usize) -> u64, arg: usize) -> bool {
    if idx == 0 || idx >= MAX_CORES || !NUCLEOS[idx].en_linea.load(Ordering::SeqCst) { return false; }
    let n = &NUCLEOS[idx];
    if n.tarea_estado.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst).is_err() { return false; }
    n.tarea_fn.store(f as usize, Ordering::SeqCst);
    n.tarea_arg.store(arg, Ordering::SeqCst);
    // El estado ya es 1; la IPI despierta al AP si está en hlt.
    send_ipi(n.apic_id.load(Ordering::SeqCst) as u8, IPI_VECTOR_RESCHEDULE)
}

/// [E2] Espera (hasta `timeout_us`) el resultado de la tarea del AP `idx` y
/// libera la ranura. `None` si no terminó a tiempo (la ranura queda ocupada).
pub fn esperar_tarea(idx: usize, timeout_us: u64) -> Option<u64> {
    if idx >= MAX_CORES { return None; }
    let n = &NUCLEOS[idx];
    let pasos = timeout_us / 50 + 1;
    for _ in 0..pasos {
        if n.tarea_estado.load(Ordering::SeqCst) == 3 {
            let r = n.tarea_resultado.load(Ordering::SeqCst);
            n.tarea_estado.store(0, Ordering::SeqCst);
            return Some(r);
        }
        esperar_us(50);
    }
    None
}

// ── [E2] TLB shootdown con acuse ───────────────────────────────────────────
static TLB_ENVIADOS: AtomicU64 = AtomicU64::new(0);
static TLB_SIN_ACUSE: AtomicU64 = AtomicU64::new(0);
static TLB_ACUSES: AtomicU64 = AtomicU64::new(0);
/// [85ª] Acuses que llegaron después de los 10 ms pero antes de 500 ms: el IPI
/// no se perdió, el AP tardó (el anfitrión desalojó su vCPU). Se cuentan aparte
/// para distinguir "tarde" de "nunca" (T-011: medir, no suponer).
static TLB_TARDIOS: AtomicU64 = AtomicU64::new(0);
static TLB_PEOR_US: AtomicU64 = AtomicU64::new(0);
pub fn tlb_estadisticas() -> (u64, u64, u64) {
    (TLB_ENVIADOS.load(Ordering::SeqCst), TLB_ACUSES.load(Ordering::SeqCst), TLB_SIN_ACUSE.load(Ordering::SeqCst))
}
/// (acuses tardíos, peor espera en µs de un acuse que sí llegó).
pub fn tlb_tardios() -> (u64, u64) { (TLB_TARDIOS.load(Ordering::SeqCst), TLB_PEOR_US.load(Ordering::SeqCst)) }

/// Invalida el TLB de todos los AP en línea (IPI 0xF1) y espera su acuse (cada
/// AP incrementa `TLB_ACUSES` tras recargar CR3). Devuelve `true` si todos
/// acusaron en 10 ms. Con un solo núcleo no hace nada y devuelve `true`.
pub fn tlb_shootdown() -> bool {
    let aps = nucleos_en_linea().saturating_sub(1) as u64;
    if aps == 0 || !INICIALIZADO.load(Ordering::SeqCst) { return true; }
    let _g = TLB_LOCK.lock();
    let antes = TLB_ACUSES.load(Ordering::SeqCst);
    if broadcast_ipi(IPI_VECTOR_TLB_FLUSH) == 0 { return false; }
    TLB_ENVIADOS.fetch_add(1, Ordering::SeqCst);
    for i in 0..2000u64 {
        if TLB_ACUSES.load(Ordering::SeqCst).wrapping_sub(antes) >= aps {
            TLB_PEOR_US.fetch_max(i * 5, Ordering::SeqCst);
            return true;
        }
        esperar_us(5);
    }
    // Pasados 10 ms: ¿tarde o nunca? Se espera hasta 500 ms más antes de darlo
    // por perdido (seguir sin el acuse dejaría un TLB rancio en el AP).
    for i in 0..4900u64 {
        if TLB_ACUSES.load(Ordering::SeqCst).wrapping_sub(antes) >= aps {
            TLB_TARDIOS.fetch_add(1, Ordering::SeqCst);
            TLB_PEOR_US.fetch_max(10_000 + i * 100, Ordering::SeqCst);
            return true;
        }
        esperar_us(100);
    }
    TLB_SIN_ACUSE.fetch_add(1, Ordering::SeqCst);
    false
}
static TLB_LOCK: Spinlock<()> = Spinlock::new(());

// ── [E2] Estrés concurrente de los cerrojos del núcleo ─────────────────────
/// Contador protegido por cerrojo que martillean todos los núcleos a la vez.
static CONTADOR_ESTRES: Spinlock<u64> = Spinlock::new(0);

/// Trabajador de estrés (corre en cada AP y en el BSP). Por ronda: un marco
/// físico del asignador global, firmado y verificado tras competir con los
/// demás núcleos; un bloque del montículo del núcleo firmado y verificado; un
/// incremento bajo `CONTADOR_ESTRES`. Devuelve `errores << 32 | rondas_ok`.
pub fn trabajador_estres(rondas: usize) -> u64 {
    let yo = apic_id_actual() as u64;
    let mut ok = 0u64; let mut err = 0u64;
    for r in 0..rondas as u64 {
        let firma = (yo << 48) ^ (r << 8) ^ 0xA5A5;
        let marco = crate::mem::marcos::asignar();
        let bloque = alloc::boxed::Box::new([0u64; 32]);
        let mut bloque = bloque;
        bloque[0] = firma; bloque[31] = !firma;
        { let mut c = CONTADOR_ESTRES.lock(); *c += 1; }
        if let Some(p) = marco {
            let v = paging::phys_to_virt(p) as *mut u64;
            unsafe {
                core::ptr::write_volatile(v, firma);
                core::ptr::write_volatile(v.add(511), !firma);
            }
            core::hint::spin_loop();
            let a = unsafe { core::ptr::read_volatile(v) };
            let b = unsafe { core::ptr::read_volatile(v.add(511)) };
            if a != firma || b != !firma { err += 1; } else { ok += 1; }
            crate::mem::marcos::liberar(p);
        } else {
            err += 1;
        }
        if bloque[0] != firma || bloque[31] != !firma { err += 1; }
        drop(bloque);
    }
    (err << 32) | ok
}

pub struct ResultadoEstres {
    pub nucleos: usize,
    pub rondas: usize,
    pub ok_por_nucleo: [u64; MAX_CORES],
    pub err_por_nucleo: [u64; MAX_CORES],
    pub sin_respuesta: usize,
    pub contador: u64,
    pub contador_esperado: u64,
    pub libres_antes: usize,
    pub libres_despues: usize,
    pub tlb_ok: bool,
    pub us: u64,
}

/// Lanza `trabajador_estres(rondas)` en todos los AP en línea y en el BSP a la
/// vez, y comprueba las invariantes globales (contador exacto, marcos libres
/// restaurados, ninguna firma pisada, TLB shootdown acusado).
pub fn estres_concurrente(rondas: usize) -> ResultadoEstres {
    let mut r = ResultadoEstres {
        nucleos: 0, rondas, ok_por_nucleo: [0; MAX_CORES], err_por_nucleo: [0; MAX_CORES],
        sin_respuesta: 0, contador: 0, contador_esperado: 0,
        libres_antes: 0, libres_despues: 0, tlb_ok: false, us: 0,
    };
    let _serie = ESTRES_LOCK.lock();
    r.libres_antes = crate::mem::marcos::estadisticas().1;
    *CONTADOR_ESTRES.lock() = 0;
    let hz = tsc_hz();
    let t0 = unsafe { core::arch::x86_64::_rdtsc() };
    let mut lanzados = [false; MAX_CORES];
    for idx in 1..MAX_CORES {
        if NUCLEOS[idx].en_linea.load(Ordering::SeqCst) {
            lanzados[idx] = ejecutar_en_ap(idx, trabajador_estres, rondas);
            if lanzados[idx] { r.nucleos += 1; }
        }
    }
    // El BSP trabaja mientras los AP trabajan (de verdad en paralelo).
    let mio = trabajador_estres(rondas);
    r.ok_por_nucleo[0] = mio & 0xFFFF_FFFF; r.err_por_nucleo[0] = mio >> 32; r.nucleos += 1;
    for idx in 1..MAX_CORES {
        if !lanzados[idx] { continue; }
        match esperar_tarea(idx, 5_000_000) {
            Some(v) => { r.ok_por_nucleo[idx] = v & 0xFFFF_FFFF; r.err_por_nucleo[idx] = v >> 32; }
            None => { r.sin_respuesta += 1; }
        }
    }
    r.tlb_ok = tlb_shootdown();
    let t1 = unsafe { core::arch::x86_64::_rdtsc() };
    r.us = t1.wrapping_sub(t0) / (hz / 1_000_000).max(1);
    r.contador = *CONTADOR_ESTRES.lock();
    r.contador_esperado = (r.nucleos * rondas) as u64;
    r.libres_despues = crate::mem::marcos::estadisticas().1;
    r
}
static ESTRES_LOCK: Spinlock<()> = Spinlock::new(());

/// Manejador común de IPI (lo invocan los stubs `ipi_stub_f0/f1/f2` de isr.asm
/// en el núcleo que RECIBE la interrupción).
#[no_mangle]
pub extern "C" fn ipi_handler_rust(vector: u32) {
    let yo = apic_id_actual();
    for n in NUCLEOS.iter() {
        if n.apic_id.load(Ordering::Relaxed) == yo {
            n.ipi_recibidas.fetch_add(1, Ordering::SeqCst);
            match vector as u8 {
                IPI_VECTOR_PRUEBA => { n.ipi_prueba.fetch_add(1, Ordering::SeqCst); }
                IPI_VECTOR_TLB_FLUSH => {
                    n.ipi_tlb.fetch_add(1, Ordering::SeqCst);
                    unsafe { core::arch::asm!("mov {0}, cr3; mov cr3, {0}", out(reg) _, options(nostack)); }
                    TLB_ACUSES.fetch_add(1, Ordering::SeqCst);
                }
                IPI_VECTOR_RESCHEDULE => {
                    n.ipi_replanificar.fetch_add(1, Ordering::SeqCst);
                    // [E3d] Si este AP está ejecutando un proceso, su bucle ocioso no
                    // corre y la tarea del BSP (`tr_en_ap`, `/proc/smp`, estrés) no
                    // se atendería nunca: se ejecuta aquí, dentro de la IPI (las
                    // tareas son cortas y seguras en IRQ: leen registros/contadores
                    // o toman cerrojos irqsave).
                    if crate::arch::percpu::pid_actual() != 0
                        && n.tarea_estado.compare_exchange(1, 2, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                        let f = n.tarea_fn.load(Ordering::SeqCst);
                        let arg = n.tarea_arg.load(Ordering::SeqCst);
                        let r = if f != 0 {
                            let f: fn(usize) -> u64 = unsafe { core::mem::transmute(f) };
                            f(arg)
                        } else { 0 };
                        n.tarea_resultado.store(r, Ordering::SeqCst);
                        n.tareas_ejecutadas.fetch_add(1, Ordering::SeqCst);
                        n.tarea_estado.store(3, Ordering::SeqCst);
                    }
                }
                crate::arch::idt::VECTOR_TEMPORIZADOR_LAPIC => {
                    // [E3b] Tick del temporizador LAPIC de este AP (no cuenta como IPI).
                    n.ipi_recibidas.fetch_sub(1, Ordering::SeqCst);
                    crate::arch::percpu::actual().ticks_lapic.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }
            break;
        }
    }
    lapic_escribir(LAPIC_EOI, 0);
}

// ── Envío de IPI por el ICR ─────────────────────────────────────────────────
fn icr_esperar_libre() -> bool {
    for _ in 0..200_000 {
        if lapic_leer(LAPIC_ICR_LOW) & ICR_DELIVERY_PENDING == 0 { return true; }
        core::hint::spin_loop();
    }
    false
}

fn icr_enviar(apic_dest: u32, low: u32) -> bool {
    if !icr_esperar_libre() { return false; }
    lapic_escribir(LAPIC_ICR_HIGH, apic_dest << 24);
    lapic_escribir(LAPIC_ICR_LOW, low);
    icr_esperar_libre()
}

/// Envía una IPI de vector fijo al APIC ID destino. Devuelve `false` si la
/// LAPIC no aceptó la orden (no hay LAPIC programada o el ICR sigue ocupado).
pub fn send_ipi(dest_apic_id: u8, vector: u8) -> bool {
    if !INICIALIZADO.load(Ordering::SeqCst) { return false; }
    let _g = ICR_LOCK.lock();
    icr_enviar(dest_apic_id as u32, ICR_FIXED | vector as u32)
}

/// Envía una IPI a todos los núcleos salvo el actual. Devuelve cuántos AP en
/// línea había como destinatarios (0 si sólo hay un núcleo).
pub fn broadcast_ipi(vector: u8) -> usize {
    let aps = nucleos_en_linea().saturating_sub(1);
    if aps == 0 || !INICIALIZADO.load(Ordering::SeqCst) { return 0; }
    let _g = ICR_LOCK.lock();
    if !icr_esperar_libre() { return 0; }
    lapic_escribir(LAPIC_ICR_HIGH, 0);
    lapic_escribir(LAPIC_ICR_LOW, ICR_DEST_ALL_EXCLUDING_SELF | ICR_FIXED | vector as u32);
    let _ = icr_esperar_libre();
    aps
}

/// Prueba de ida y vuelta: envía 0xF0 al AP `idx`, espera a que SU contador
/// avance. Devuelve `(contador_antes, contador_despues, ok)`.
pub fn probar_ipi(idx: usize) -> (u64, u64, bool) {
    if idx >= MAX_CORES || !NUCLEOS[idx].en_linea.load(Ordering::SeqCst)
        || NUCLEOS[idx].es_bsp.load(Ordering::SeqCst) {
        return (0, 0, false);
    }
    let apic = NUCLEOS[idx].apic_id.load(Ordering::SeqCst);
    let antes = NUCLEOS[idx].ipi_prueba.load(Ordering::SeqCst);
    if !send_ipi(apic as u8, IPI_VECTOR_PRUEBA) { return (antes, antes, false); }
    for _ in 0..1000 {               // hasta 10 ms
        if NUCLEOS[idx].ipi_prueba.load(Ordering::SeqCst) > antes {
            return (antes, NUCLEOS[idx].ipi_prueba.load(Ordering::SeqCst), true);
        }
        esperar_us(10);
    }
    (antes, NUCLEOS[idx].ipi_prueba.load(Ordering::SeqCst), false)
}

// ── Arranque de los AP ──────────────────────────────────────────────────────
fn escribir_tramp(off: usize, v: u64) {
    unsafe { core::ptr::write_volatile(paging::phys_to_virt(TRAMPOLIN_BASE + off) as *mut u64, v); }
}
fn leer_tramp(off: usize) -> u64 {
    unsafe { core::ptr::read_volatile(paging::phys_to_virt(TRAMPOLIN_BASE + off) as *const u64) }
}

fn preparar_trampolin() -> bool {
    if TRAMPOLIN_BIN.len() != 4096 { return false; }
    let cr3 = paging::KERNEL_CR3.load(Ordering::SeqCst);
    if cr3 == 0 || cr3 >= 0x1_0000_0000 { return false; }
    // La página debe estar mapeada y ser ejecutable para el AP (tras activar
    // paginación sigue ejecutando en 0x8000 unas instrucciones).
    match paging::translate(cr3, TRAMPOLIN_BASE) {
        Some((_, flags)) if flags & paging::PRESENT != 0 && flags & paging::NO_EXECUTE == 0 => {}
        _ => return false,
    }
    let dst = paging::phys_to_virt(TRAMPOLIN_BASE) as *mut u8;
    unsafe { core::ptr::copy_nonoverlapping(TRAMPOLIN_BIN.as_ptr(), dst, 4096); }
    let cr4: u64; let efer_lo: u32; let efer_hi: u32;
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        core::arch::asm!("rdmsr", in("ecx") 0xC000_0080u32, out("eax") efer_lo, out("edx") efer_hi, options(nomem, nostack));
    }
    escribir_tramp(D_CR3, cr3);
    escribir_tramp(D_CR4, cr4 & !(1u64 << 17));          // sin PCIDE en el AP
    escribir_tramp(D_EFER, ((efer_hi as u64) << 32 | efer_lo as u64) & 0x0000_0D01); // SCE|LME|NXE
    escribir_tramp(D_ENTRADA, ap_entrada_rust as *const () as usize as u64);
    true
}

fn pila_para_ap() -> Option<u64> {
    let layout = match core::alloc::Layout::from_size_align(PILA_AP_BYTES, 4096) {
        Ok(l) => l, Err(_) => return None,
    };
    let p = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if p.is_null() { return None; }
    Some(p as u64 + PILA_AP_BYTES as u64)
}

/// INIT-SIPI-SIPI al AP `idx`. Devuelve `true` si entró en Rust y se declaró en línea.
fn arrancar_ap(idx: usize) -> bool {
    let apic = NUCLEOS[idx].apic_id.load(Ordering::SeqCst);
    let pila = match pila_para_ap() { Some(p) => p, None => return false };
    let _g = ICR_LOCK.lock();
    escribir_tramp(D_PILA, pila);
    escribir_tramp(D_INDICE, idx as u64);
    escribir_tramp(D_SELLO, 0);
    lapic_escribir(LAPIC_ESR, 0);
    // INIT (nivel, afirmado) → 10 ms → INIT de-assert.
    if !icr_enviar(apic, ICR_INIT | ICR_LEVEL_ASSERT | ICR_TRIGGER_LEVEL) { return false; }
    esperar_us(10_000);
    let _ = icr_enviar(apic, ICR_INIT | ICR_TRIGGER_LEVEL);
    esperar_us(200);
    // Dos SIPI con el vector de la página del trampolín (0x8000 >> 12 = 0x08).
    for _ in 0..2 {
        if !icr_enviar(apic, ICR_STARTUP | (TRAMPOLIN_BASE as u32 >> 12)) { return false; }
        esperar_us(200);
        for _ in 0..1000 {       // hasta 100 ms por SIPI
            if NUCLEOS[idx].en_linea.load(Ordering::SeqCst) { return true; }
            esperar_us(100);
        }
    }
    NUCLEOS[idx].en_linea.load(Ordering::SeqCst)
}

fn declarar_un_nucleo(motivo: &'static str) {
    *MOTIVO_UN_NUCLEO.lock() = motivo;
    serial::write_str("[SMP] 1 nucleo: ");
    serial::write_str(motivo);
    serial::write_str("\n");
}

/// Inicializa el SMP real. Se llama en el BSP con la IDT y el montículo listos.
pub fn init() {
    // El BSP ocupa siempre la ranura 0 (APIC ID real leído más abajo).
    NUCLEOS[0].es_bsp.store(true, Ordering::SeqCst);
    NUCLEOS[0].en_linea.store(true, Ordering::SeqCst);
    N_EN_LINEA.store(1, Ordering::SeqCst);

    // ¿Hay LAPIC? (CPUID.1:EDX bit 9)
    let cpuid1 = core::arch::x86_64::__cpuid(1);
    if cpuid1.edx & (1 << 9) == 0 {
        NUCLEOS[0].apic_id.store((cpuid1.ebx >> 24) & 0xFF, Ordering::SeqCst);
        declarar_un_nucleo("la CPU no declara LAPIC (CPUID.1:EDX.9=0)");
        return;
    }

    // RSDP → MADT.
    let (rsdp, origen) = match buscar_rsdp() {
        Some(r) => r,
        None => {
            NUCLEOS[0].apic_id.store((cpuid1.ebx >> 24) & 0xFF, Ordering::SeqCst);
            declarar_un_nucleo("sin RSDP (ni del cargador, ni en EBDA/0xE0000)");
            return;
        }
    };
    let madt = match buscar_madt(rsdp) {
        Some(m) => m,
        None => {
            NUCLEOS[0].apic_id.store((cpuid1.ebx >> 24) & 0xFF, Ordering::SeqCst);
            declarar_un_nucleo("RSDP valida pero sin MADT en XSDT/RSDT");
            return;
        }
    };
    MADT_PHYS.store(madt, Ordering::SeqCst);

    // Los registros de la MADT pueden listar el BSP en cualquier posición: se
    // parsea a una lista temporal y luego se coloca el BSP en la ranura 0.
    let n = parsear_madt(madt);
    N_MADT.store(n, Ordering::SeqCst);
    let base = LAPIC_BASE.load(Ordering::SeqCst);
    serial::write_str("[SMP] RSDP (");
    serial::write_str(origen);
    serial::write_str(") MADT en ");
    serial::write_hex(madt);
    serial::write_str(": ");
    serial::write_usize(n);
    serial::write_str(" LAPIC habilitadas, base LAPIC ");
    serial::write_hex(base as usize);
    serial::write_str(", ids");
    for i in 0..n {
        serial::write_str(" ");
        serial::write_usize(NUCLEOS[i].apic_id.load(Ordering::SeqCst) as usize);
    }
    serial::write_str("\n");

    if !asegurar_mapeo_lapic(base) {
        NUCLEOS[0].apic_id.store((cpuid1.ebx >> 24) & 0xFF, Ordering::SeqCst);
        declarar_un_nucleo("no se pudo mapear la LAPIC en el PML4 maestro");
        return;
    }

    // LAPIC del BSP: APIC ID real, habilitar si el firmware la dejó apagada.
    let bsp_apic = apic_id_actual();
    APIC_ID_BSP.store(bsp_apic, Ordering::SeqCst);
    let svr = lapic_leer(LAPIC_SVR);
    if svr & 0x100 == 0 {
        // Al habilitarla, LINT0 debe quedar como ExtINT para que el PIC (PIT,
        // teclado, ATA…) siga llegando al BSP.
        lapic_escribir(LAPIC_SVR, 0x100 | crate::arch::idt::VECTOR_IPI_ESPURIO as u32);
        lapic_escribir(LAPIC_LVT_LINT0, 0x700);
        lapic_escribir(LAPIC_LVT_LINT1, 0x400);
    } else {
        lapic_escribir(LAPIC_SVR, (svr & !0xFF) | crate::arch::idt::VECTOR_IPI_ESPURIO as u32);
    }
    lapic_escribir(LAPIC_TPR, 0);
    serial::write_str("[SMP] BSP APIC ID ");
    serial::write_usize(bsp_apic as usize);
    serial::write_str(" version ");
    serial::write_hex(lapic_leer(LAPIC_VERSION) as usize);
    serial::write_str(" SVR ");
    serial::write_hex(lapic_leer(LAPIC_SVR) as usize);
    serial::write_str(" LINT0 ");
    serial::write_hex(lapic_leer(LAPIC_LVT_LINT0) as usize);
    serial::write_str("\n");

    // Colocar el BSP en la ranura 0 intercambiando con quien tenga su APIC ID.
    let pos_bsp = (0..n).find(|&i| NUCLEOS[i].apic_id.load(Ordering::SeqCst) == bsp_apic);
    match pos_bsp {
        Some(0) => {}
        Some(k) => {
            let a0 = NUCLEOS[0].apic_id.load(Ordering::SeqCst);
            NUCLEOS[0].apic_id.store(bsp_apic, Ordering::SeqCst);
            NUCLEOS[k].apic_id.store(a0, Ordering::SeqCst);
        }
        None => {
            // La MADT no lista al BSP: anómalo; se añade si cabe.
            let a0 = NUCLEOS[0].apic_id.load(Ordering::SeqCst);
            NUCLEOS[0].apic_id.store(bsp_apic, Ordering::SeqCst);
            if n < MAX_CORES && NUCLEOS[0].presente_madt.load(Ordering::SeqCst) {
                NUCLEOS[n].apic_id.store(a0, Ordering::SeqCst);
                NUCLEOS[n].presente_madt.store(true, Ordering::SeqCst);
                N_MADT.store(n + 1, Ordering::SeqCst);
            }
            NUCLEOS[0].presente_madt.store(false, Ordering::SeqCst);
        }
    }
    NUCLEOS[0].apic_id_visto.store(bsp_apic, Ordering::SeqCst);

    unsafe { crate::arch::idt::instalar_vectores_ipi(); }
    INICIALIZADO.store(true, Ordering::SeqCst);

    let total = N_MADT.load(Ordering::SeqCst);
    if total <= 1 {
        declarar_un_nucleo("la MADT solo lista un procesador");
        return;
    }
    if !preparar_trampolin() {
        declarar_un_nucleo("trampolin 0x8000 no preparable (CR3 >4GiB, pagina NX o sin mapear)");
        return;
    }

    for idx in 1..total {
        let apic = NUCLEOS[idx].apic_id.load(Ordering::SeqCst);
        let ok = arrancar_ap(idx);
        serial::write_str("[SMP] AP ");
        serial::write_usize(idx);
        serial::write_str(" (APIC ID ");
        serial::write_usize(apic as usize);
        serial::write_str("): ");
        if ok {
            serial::write_str("en linea, ve su APIC ID ");
            serial::write_usize(NUCLEOS[idx].apic_id_visto.load(Ordering::SeqCst) as usize);
            let (a, d, ipi_ok) = probar_ipi(idx);
            serial::write_str(", IPI 0xF0 contador ");
            serial::write_u64(a);
            serial::write_str(" -> ");
            serial::write_u64(d);
            serial::write_str(if ipi_ok { " OK\n" } else { " SIN RESPUESTA\n" });
        } else {
            serial::write_str("NO RESPONDIO (sello trampolin ");
            serial::write_hex(leer_tramp(D_SELLO) as usize);
            serial::write_str(")\n");
        }
    }
    let en_linea = N_EN_LINEA.load(Ordering::SeqCst);
    serial::write_str("[SMP] ");
    serial::write_usize(en_linea);
    serial::write_str(" de ");
    serial::write_usize(total);
    serial::write_str(" nucleos en linea (planificacion SMP encendida por defecto; `smp off` la apaga)\n");
    if en_linea <= 1 {
        declarar_un_nucleo("ningun AP de la MADT respondio al INIT-SIPI-SIPI");
    }
}

// ── Consultas ───────────────────────────────────────────────────────────────
// ── [T-025 E3c] Planificación de procesos en los AP ───────────────────────────
/// Interruptor global (`smp on|off` en la consola). **Encendido por defecto
/// desde la 85ª parte (b199)**: la suite completa pasó dos veces seguidas con
/// `smp on` (87a/87b, b198), así que el segundo núcleo planifica desde el
/// arranque sin que nadie lo pida. `smp off` sigue siendo el control negativo.
static SMP_PLANIFICAR: AtomicBool = AtomicBool::new(true);
pub fn planificacion_smp_activa() -> bool { SMP_PLANIFICAR.load(Ordering::SeqCst) }
pub fn fijar_planificacion_smp(on: bool) { SMP_PLANIFICAR.store(on, Ordering::SeqCst); }

/// Tick del temporizador LAPIC de un AP (vector 0xF3, desde `ap_timer_handler`).
/// Devuelve 0 (seguir donde estaba) o el RSP de núcleo de otro proceso.
#[no_mangle]
pub extern "C" fn ap_tick_rust(frame_rsp: u64, saved_cs: u64) -> u64 {
    lapic_escribir(LAPIC_EOI, 0);
    crate::arch::percpu::actual().ticks_lapic.fetch_add(1, Ordering::Relaxed);
    if !SMP_PLANIFICAR.load(Ordering::SeqCst) { return 0; }
    let pid = crate::arch::percpu::pid_actual();
    if pid != 0 {
        // Un proceso corre en este AP: mismo planificador que IRQ0 en el BSP.
        return crate::process::schedule_tick(frame_rsp, saved_cs);
    }
    // Bucle ocioso: ¿hay algo listo para este núcleo? (Nunca mientras ejecuta
    // una tarea de núcleo encargada por el BSP: se abandonaría a medias.)
    let idx = crate::arch::percpu::indice_actual();
    if NUCLEOS[idx.min(MAX_CORES - 1)].tarea_estado.load(Ordering::SeqCst) != 0
        && NUCLEOS[idx.min(MAX_CORES - 1)].tarea_estado.load(Ordering::SeqCst) != 3 { return 0; }
    unsafe {
        let next = crate::process::pick_next_ready();
        if next.is_null() { return 0; }
        NUCLEOS[idx.min(MAX_CORES - 1)].procesos_arrancados.fetch_add(1, Ordering::SeqCst);
        crate::process::preparar_cpu_para(next);
        crate::drivers::serial::write_str("SCHED[");
        crate::drivers::serial::write_usize(idx);
        crate::drivers::serial::write_str("]: ocioso -> PID ");
        crate::drivers::serial::write_usize((*next).pid as usize);
        crate::drivers::serial::write_str("\n");
        (*next).kernel_rsp
    }
}

/// [E3c] Abandona el contexto actual de este AP (proceso que salió o nada que
/// ejecutar) y vuelve al bucle ocioso con la pila original del AP.
pub fn ap_ir_a_ocioso() -> ! {
    let idx = crate::arch::percpu::indice_actual().min(MAX_CORES - 1);
    let n = &NUCLEOS[idx];
    n.vueltas_ocioso.fetch_add(1, Ordering::SeqCst);
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }
    crate::arch::bkl::soltar_todo(); // [E3d] nunca irse al bucle ocioso con el cerrojo grande
    unsafe { let c = crate::process::current_raw(); if !c.is_null() { crate::process::scheduler::diferir_liberacion(c); } }
    crate::arch::debug_regs::disable_all();
    // Primero fuera de las tablas del proceso, después "ya no lo ejecuto".
    let kcr3 = paging::KERNEL_CR3.load(Ordering::Relaxed);
    if kcr3 != 0 && paging::read_cr3() != kcr3 { paging::write_cr3(kcr3); }
    crate::arch::percpu::fijar_pid_actual(0);
    let top = n.pila_top.load(Ordering::SeqCst);
    unsafe {
        core::arch::asm!(
            "mov rsp, {top}",
            "mov rdi, {idx}",
            "jmp {f}",
            top = in(reg) top, idx = in(reg) idx as u64, f = sym ap_bucle_ocioso,
            options(noreturn)
        );
    }
}

/// Núcleos en los que se PLANIFICAN procesos: 1 mientras la planificación SMP
/// esté apagada (los AP están en línea pero ociosos y mentir aquí haría que
/// `nproc`/`sysconf` prometieran un paralelismo que no existe: T-011), y los
/// núcleos en línea cuando está encendida (E3c).
pub fn cpu_count() -> usize {
    if SMP_PLANIFICAR.load(Ordering::SeqCst) { nucleos_en_linea().max(1) } else { 1 }
}

/// Núcleos realmente arrancados (BSP + AP que llegaron a Rust).
pub fn nucleos_en_linea() -> usize { N_EN_LINEA.load(Ordering::SeqCst) }
/// [E3d] APIC ID del núcleo `idx` (u32::MAX si no existe).
pub fn apic_id_de(idx: usize) -> u32 { NUCLEOS[idx.min(MAX_CORES - 1)].apic_id.load(Ordering::SeqCst) }
/// Procesadores que lista la MADT (0 si no hay MADT).
pub fn nucleos_madt() -> usize { N_MADT.load(Ordering::SeqCst) }
pub fn madt_phys() -> usize { MADT_PHYS.load(Ordering::SeqCst) }
pub fn lapic_base_fisica() -> u64 { LAPIC_BASE.load(Ordering::SeqCst) }
pub fn motivo_un_nucleo() -> &'static str { *MOTIVO_UN_NUCLEO.lock() }
pub fn apic_id_bsp() -> u32 { APIC_ID_BSP.load(Ordering::SeqCst) }

/// Consulta si el núcleo actual es el procesador de arranque (BSP).
pub fn is_bsp() -> bool { current_cpu_id() as u32 == APIC_ID_BSP.load(Ordering::Relaxed) }

/// APIC ID inicial del procesador actual (CPUID.1:EBX[31:24]).
pub fn current_cpu_id() -> u8 {
    let cpuid = core::arch::x86_64::__cpuid(1);
    (cpuid.ebx >> 24) as u8
}

#[derive(Clone, Copy, Debug)]
pub struct CpuCoreInfo {
    pub processor_id: u8,
    pub apic_id: u8,
    pub is_bsp: bool,
    pub is_enabled: bool,
}

pub fn get_core(idx: usize) -> Option<CpuCoreInfo> {
    if idx >= MAX_CORES || !NUCLEOS[idx].presente_madt.load(Ordering::SeqCst)
        && !NUCLEOS[idx].es_bsp.load(Ordering::SeqCst) { return None; }
    Some(CpuCoreInfo {
        processor_id: idx as u8,
        apic_id: NUCLEOS[idx].apic_id.load(Ordering::SeqCst) as u8,
        is_bsp: NUCLEOS[idx].es_bsp.load(Ordering::SeqCst),
        is_enabled: NUCLEOS[idx].en_linea.load(Ordering::SeqCst),
    })
}

/// Máscara de afinidad de CPU (usada por sched_setaffinity en E3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuAffinityMask(pub u64);

impl CpuAffinityMask {
    pub const fn all() -> Self { Self(u64::MAX) }
    pub const fn single(cpu_id: u8) -> Self { Self(1u64 << (cpu_id as u64 & 63)) }
    pub fn is_allowed(&self, cpu_id: u8) -> bool { cpu_id < 64 && (self.0 >> cpu_id) & 1 == 1 }
    pub fn set_allowed(&mut self, cpu_id: u8, allowed: bool) {
        if cpu_id < 64 {
            if allowed { self.0 |= 1u64 << cpu_id; } else { self.0 &= !(1u64 << cpu_id); }
        }
    }
}

/// TLB shootdown real: IPI 0xF1 a todos los AP en línea (cada uno recarga CR3).
pub fn trigger_tlb_shootdown() -> usize { broadcast_ipi(IPI_VECTOR_TLB_FLUSH) }

/// Pide replanificar al núcleo lógico `cpu_id` (índice en `NUCLEOS`).
pub fn notify_reschedule(cpu_id: u8) -> bool {
    let idx = cpu_id as usize;
    if idx >= MAX_CORES || !NUCLEOS[idx].en_linea.load(Ordering::SeqCst) { return false; }
    send_ipi(NUCLEOS[idx].apic_id.load(Ordering::SeqCst) as u8, IPI_VECTOR_RESCHEDULE)
}

// ── Telemetría por núcleo (datos reales; compatibilidad con la UI) ──────────
#[derive(Clone, Copy, Debug)]
pub struct PerCpuData {
    pub core_id: u8,
    pub apic_id: u8,
    pub is_online: bool,
    pub is_bsp: bool,
    pub current_pid: u32,
    pub task_count: usize,
    pub ticks: u64,
    pub ipi_count: u64,
    pub load_percent: u8,
}

pub fn smp_online_count() -> usize { nucleos_en_linea() }

/// Núcleo en línea con menos tareas asignadas (BSP si sólo hay uno).
pub fn smp_least_loaded_core() -> u8 {
    let mut mejor = 0u8;
    let mut min = usize::MAX;
    for (i, n) in NUCLEOS.iter().enumerate() {
        if n.en_linea.load(Ordering::SeqCst) {
            let t = n.tareas.load(Ordering::SeqCst);
            if t < min { min = t; mejor = i as u8; }
        }
    }
    mejor
}

pub fn smp_assign_task(cpu_id: u8, _pid: u32) {
    let idx = cpu_id as usize;
    if idx < MAX_CORES && NUCLEOS[idx].en_linea.load(Ordering::SeqCst) {
        NUCLEOS[idx].tareas.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn smp_get_core_telemetry(cpu_id: u8) -> Option<PerCpuData> {
    let idx = cpu_id as usize;
    if idx >= MAX_CORES || !NUCLEOS[idx].en_linea.load(Ordering::SeqCst) { return None; }
    let n = &NUCLEOS[idx];
    let es_bsp = n.es_bsp.load(Ordering::SeqCst);
    Some(PerCpuData {
        core_id: cpu_id,
        apic_id: n.apic_id.load(Ordering::SeqCst) as u8,
        is_online: true,
        is_bsp: es_bsp,
        current_pid: if es_bsp { crate::process::current_pid() as u32 } else { 0 },
        task_count: n.tareas.load(Ordering::SeqCst),
        ticks: if es_bsp { crate::time::pit::ticks() } else { 0 },
        ipi_count: n.ipi_recibidas.load(Ordering::SeqCst),
        load_percent: 0,
    })
}
