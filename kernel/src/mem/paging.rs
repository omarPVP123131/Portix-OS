use core::alloc::Layout;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::drivers::serial;

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_ENTRIES: usize = 512;

pub const PRESENT: u64       = 1 << 0;
pub const WRITABLE: u64      = 1 << 1;
pub const USER: u64          = 1 << 2;
pub const WRITE_THROUGH: u64 = 1 << 3;
pub const CACHE_DISABLE: u64 = 1 << 4;
pub const ACCESSED: u64      = 1 << 5;
pub const DIRTY: u64         = 1 << 6;
pub const HUGE_PAGE: u64     = 1 << 7;
pub const GLOBAL: u64        = 1 << 8;
pub const NO_EXECUTE: u64    = 1 << 63;
pub const NX: u64            = NO_EXECUTE;
/// [T1b.2] `true` cuando EFER.NXE está activo: sólo entonces se escribe el bit 63.
static NX_ACTIVO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
pub fn activar_nx() { NX_ACTIVO.store(true, Ordering::SeqCst); serial::write_str("[NX] EFER.NXE=1: paginas de usuario no ejecutables salvo PROT_EXEC/PF_X\n"); }
pub fn nx_activo() -> bool { NX_ACTIVO.load(Ordering::Relaxed) }
/// Bit NX a aplicar a una página que NO debe ejecutarse (0 si la CPU no lo soporta).
#[inline] pub fn nx_bit() -> u64 { if nx_activo() { NO_EXECUTE } else { 0 } }
pub const COW_FLAG: u64      = 1 << 9; // Bit 9 disponible para software (Copy-On-Write)
pub const DEMAND_PAGING_FLAG: u64 = 1 << 10; // Bit 10 disponible para software (Demand Paging)
pub const SWAP_FLAG: u64     = 1 << 11; // Bit 11 disponible para software (Swap Space)

pub static COW_FAULTS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static COW_PAGES_CLONED: AtomicU64 = AtomicU64::new(0);
pub static DEMAND_FAULTS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static SWAP_IN_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static SWAP_OUT_TOTAL: AtomicU64 = AtomicU64::new(0);

pub const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
pub const PTE_FLAGS_MASK: u64 = !PTE_ADDR_MASK;

#[inline]
pub fn pml4_index(vaddr: usize) -> usize {
    (vaddr >> 39) & 0x1FF
}
#[inline]
pub fn pdpt_index(vaddr: usize) -> usize {
    (vaddr >> 30) & 0x1FF
}
#[inline]
pub fn pd_index(vaddr: usize) -> usize {
    (vaddr >> 21) & 0x1FF
}
#[inline]
pub fn pt_index(vaddr: usize) -> usize {
    (vaddr >> 12) & 0x1FF
}

#[inline]
pub fn page_align_down(x: usize) -> usize {
    x & !(PAGE_SIZE - 1)
}
#[inline]
pub fn page_align_up(x: usize) -> usize {
    (x + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

pub const KERNEL_BASE: usize = 0xFFFF_8000_0000_0000;

/// [CICLO 49] Desplazamiento virtual de la IMAGEN del núcleo (mitad superior,
/// 2 GiB altos, modelo de código `kernel`). Debe coincidir con
/// `KERNEL_VMA_OFFSET` de `kernel/linker.ld`. `_start` mapea
/// `KERNEL_VMA_OFFSET + p -> p` para p en 0..1 GiB (PML4[511], PDPT[510]).
pub const KERNEL_VMA_OFFSET: usize = 0xFFFF_FFFF_8000_0000;
/// Dirección física donde el cargador deposita la imagen (sin cambios).
pub const KERNEL_PHYS_BASE: usize = 0x20_0000;

/// [CICLO 50 / Fase B.1] Tamaño del mapa físico directo que `_start` construye
/// en la mitad superior (`HH_PD`: 512 páginas de 2 MiB = 1 GiB a partir de 0).
pub const MAPA_FISICO_DIRECTO_LEN: usize = 0x4000_0000;

/// [CICLO 50 / Fase B.1] Alias ALTO de una dirección física: el núcleo accede
/// a tablas de páginas, `PortixBootInfo` y memoria baja por aquí, nunca por la
/// identidad plana de PML4[0]. Así un proceso puede recibir mapeos de usuario
/// en esas direcciones bajas sin ocultárselas al núcleo. Para físicos por
/// encima de 1 GiB (MMIO/LFB) se conserva la identidad (pendiente de B.3).
#[inline(always)]
pub fn phys_to_virt(paddr: usize) -> usize {
    if paddr < MAPA_FISICO_DIRECTO_LEN { KERNEL_VMA_OFFSET + paddr } else { paddr }
}

pub fn virt_to_phys(vaddr: usize) -> usize {
    if vaddr >= KERNEL_VMA_OFFSET {
        vaddr - KERNEL_VMA_OFFSET
    } else if vaddr >= KERNEL_BASE {
        vaddr - KERNEL_BASE
    } else {
        vaddr
    }
}

/// [CICLO 49 / Fase A] Zonas FÍSICAS que el núcleo sigue tocando por la
/// identidad plana de PML4[0] (compartida con todo espacio de usuario):
/// tablas de arranque y memoria baja, pila de arranque + PortixBootInfo +
/// datos del cargador, montículo del núcleo y ventana MMIO/LFB bajo 4 GiB.
/// Un mapeo de usuario en estas direcciones virtuales OCULTARÍA esa memoria
/// al núcleo mientras corre sobre el CR3 del proceso, así que se prohíben.
/// [Fase B.1, ciclo 50] 0x500000–0x800000 (PortixBootInfo y tablas del cargador
/// UEFI; desde T-025 E3b viven en 0xD00000–0xF10000) YA NO está reservada: el núcleo
/// las lee por `phys_to_virt` (alias alto) y no por la identidad. Con ello los
/// ejecutables de distro enlazados por defecto (tcc: texto 0x400000, datos
/// 0x600000) cargan sin `-Ttext`. Quedan para B.2/B.3: montículo y MMIO.
/// Back buffer del compositor (físico 80 MiB, 3 MiB): el núcleo lo escribe por
/// su alias alto; se reserva porque es memoria del núcleo, no del proceso.
pub const BACKBUF_PHYS: usize = 0x0500_0000;
pub const BACKBUF_LEN: usize = 0x0030_0000;
/// Nota B.2/B.4: el núcleo YA NO toca el montículo por identidad (todo `alloc`
/// devuelve alias alto y los marcos se leen con `phys_to_virt`) y desde B.4 los
/// marcos de usuario salen de `mem::marcos` (RAM del firmware fuera del
/// montículo). El rango virtual 0x1000000–0x5000000 queda por tanto disponible
/// para los procesos (prueba 4910); el back buffer sigue reservado porque es
/// memoria del núcleo escrita por alias alto.
pub const ZONAS_IDENTIDAD_RESERVADAS: [(usize, usize); 3] = [
    (0x0, 0x20_0000),                      // IVT/BDA/EBDA, stage2, tablas de arranque BIOS (0x1000), trampolín SMP
    (BACKBUF_PHYS, BACKBUF_PHYS + BACKBUF_LEN), // back buffer de vídeo (físico, tras el montículo)
    (0xC000_0000, 0x1_0000_0000),          // BARs PCI, LFB, APIC/IOAPIC, ACPI alto — B.3
];

/// `true` si `[ini, fin)` puede concederse a un proceso de usuario sin
/// solapar la mitad del núcleo ni las zonas de identidad reservadas.
pub fn rango_usuario_permitido(ini: usize, fin: usize) -> bool {
    if fin <= ini || fin > 0x0000_8000_0000_0000 { return false; }
    for (zi, zf) in ZONAS_IDENTIDAD_RESERVADAS.iter() {
        if ini < *zf && fin > *zi { return false; }
    }
    true
}

pub fn read_cr3() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) v, options(nostack, nomem)) }
    v
}

pub fn write_cr3(val: u64) {
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) val, options(nostack, nomem)) }
    // [T-025 E3c] registro por CPU del espacio cargado (para el shootdown de usuario).
    crate::arch::percpu::actual().cr3_actual.store(val, Ordering::Relaxed);
}

pub fn flush_tlb() {
    let cr3 = read_cr3();
    write_cr3(cr3);
    // [T-025 E2] Si lo que cambió es el espacio del NÚCLEO (compartido por todos
    // los núcleos), los demás deben invalidar también: IPI 0xF1 con acuse.
    // Los cambios en un espacio de usuario sólo afectan a los núcleos que lo
    // tengan cargado; hasta E3 ningún AP ejecuta procesos, así que no se
    // difunden (E3 añadirá la máscara de CPUs por `mm`).
    if cr3 == KERNEL_CR3.load(Ordering::Relaxed) {
        let _ = crate::arch::smp::tlb_shootdown();
    } else if crate::arch::smp::planificacion_smp_activa() && crate::arch::percpu::cr3_en_otra_cpu(cr3) {
        // [T-025 E3c] Otro núcleo ejecuta un hilo de este mismo espacio: también
        // debe invalidar (los AP responden con un recargado de CR3 completo).
        let _ = crate::arch::smp::tlb_shootdown();
    }
}

/// [T-026] Lote de invalidaciones: mientras una CPU está "en lote", `flush_tlb_page`
/// sólo hace el `invlpg` local y apunta que hay un shootdown pendiente; `fin_lote_tlb`
/// lo difunde UNA vez. Motivo medido: el `mmap` MAP_FIXED de `modules` por la JVM
/// (34 000 páginas sobre una reserva previa, hilos en el otro núcleo) enviaba una
/// IPI con acuse por página, ~1,8 ms cada una en QEMU TCG: 1,5 s con IF=0.
/// Es correcto porque ningún marco se libera dentro del lote (sólo cambian
/// permisos o se superponen marcos nuevos): la otra CPU puede usar una
/// traducción vieja durante el lote, nunca una traducción a memoria ajena.
#[allow(clippy::declare_interior_mutable_const)]
const CERO_U32: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static LOTE_TLB: [core::sync::atomic::AtomicU32; crate::arch::percpu::MAX_CPUS] = [CERO_U32; crate::arch::percpu::MAX_CPUS];
static LOTE_TLB_PENDIENTE: [core::sync::atomic::AtomicU32; crate::arch::percpu::MAX_CPUS] = [CERO_U32; crate::arch::percpu::MAX_CPUS];
pub static LOTES_TLB_AHORRADOS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn inicio_lote_tlb() {
    let i = crate::arch::percpu::indice_actual().min(crate::arch::percpu::MAX_CPUS - 1);
    LOTE_TLB[i].fetch_add(1, core::sync::atomic::Ordering::SeqCst);
}

pub fn fin_lote_tlb() {
    use core::sync::atomic::Ordering as O;
    let i = crate::arch::percpu::indice_actual().min(crate::arch::percpu::MAX_CPUS - 1);
    if LOTE_TLB[i].load(O::SeqCst) == 0 { return; }
    if LOTE_TLB[i].fetch_sub(1, O::SeqCst) == 1 && LOTE_TLB_PENDIENTE[i].swap(0, O::SeqCst) != 0 {
        let _ = crate::arch::smp::tlb_shootdown();
    }
}

pub fn flush_tlb_page(vaddr: usize) {
    unsafe { core::arch::asm!("invlpg [{}]", in(reg) vaddr, options(nostack, nomem)) }
    // [T-025 E2] Página de la mitad superior (núcleo): invalidar en todos los núcleos.
    let hace_falta = if vaddr >= KERNEL_VMA_OFFSET.min(KERNEL_BASE) {
        true
    } else {
        // [T-025 E3c] página de usuario de un espacio compartido con otro núcleo (hilos).
        crate::arch::smp::planificacion_smp_activa() && crate::arch::percpu::cr3_en_otra_cpu(read_cr3())
    };
    if !hace_falta { return; }
    let i = crate::arch::percpu::indice_actual().min(crate::arch::percpu::MAX_CPUS - 1);
    if LOTE_TLB[i].load(core::sync::atomic::Ordering::SeqCst) != 0 {
        LOTE_TLB_PENDIENTE[i].store(1, core::sync::atomic::Ordering::SeqCst);
        LOTES_TLB_AHORRADOS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        return;
    }
    let _ = crate::arch::smp::tlb_shootdown();
}

// [Fase B.1] Las tablas se tocan SIEMPRE por su alias alto: funciona igual
// sobre el CR3 del núcleo que sobre el de un proceso que haya mapeado esa
// dirección baja para sí (p. ej. las tablas UEFI en 0x700000).
pub(crate) unsafe fn read_entry(table_phys: usize, index: usize) -> u64 {
    ptr::read_volatile((phys_to_virt(table_phys) as *const u64).add(index))
}

pub(crate) unsafe fn write_entry(table_phys: usize, index: usize, val: u64) {
    ptr::write_volatile((phys_to_virt(table_phys) as *mut u64).add(index), val);
}

pub fn entry_paddr(entry: u64) -> usize {
    (entry & PTE_ADDR_MASK) as usize
}

pub fn translate(cr3: u64, vaddr: usize) -> Option<(usize, u64)> {
    let cr3_p = cr3 as usize;
    unsafe {
        let pml4e = read_entry(cr3_p, pml4_index(vaddr));
        if pml4e & PRESENT == 0 { return None; }
        let pdpt_p = entry_paddr(pml4e);
        let pdpte = read_entry(pdpt_p, pdpt_index(vaddr));
        if pdpte & PRESENT == 0 { return None; }
        if pdpte & HUGE_PAGE != 0 {
            let offset = vaddr & 0x3FFF_FFFF;
            let paddr = entry_paddr(pdpte) + offset;
            let flags = pdpte & PTE_FLAGS_MASK;
            return Some((paddr, flags));
        }
        let pd_p = entry_paddr(pdpte);
        let pde = read_entry(pd_p, pd_index(vaddr));
        if pde & PRESENT == 0 {
            return None;
        }
        if pde & HUGE_PAGE != 0 {
            let offset = vaddr & 0x1F_FFFF;
            let paddr = entry_paddr(pde) + offset;
            let flags = pde & PTE_FLAGS_MASK;
            return Some((paddr, flags));
        }
        let pt_p = entry_paddr(pde);
        let pte = read_entry(pt_p, pt_index(vaddr));
        if pte & PRESENT == 0 {
            return None;
        }
        let paddr = entry_paddr(pte) + (vaddr & 0xFFF);
        let flags = pte & PTE_FLAGS_MASK;
        Some((paddr, flags))
    }
}

fn alloc_page_table() -> Option<usize> {
    let layout = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE).ok()?;
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() { return None; }
    Some(ptr as usize)
}

/// [Fase B.4] Marco de 4 KiB para el espacio de USUARIO (imagen, brk, mmap,
/// copias COW). Sale del asignador de marcos; si éste no tiene mapa o se agotó,
/// se recurre al montículo y se contabiliza (`marcos::RESPALDO_HEAP`).
/// Devuelve el ALIAS ALTO (escribible directamente; `map_page` lo normaliza).
/// [T-003] Reserva del montículo que el respaldo de marcos NUNCA invade: si
/// los procesos agotan la RAM física (pkg + curl con 240 MiB de brk), el
/// respaldo llegó a comerse 42 de los 64 MiB del montículo y el núcleo
/// acabó sin memoria para tablas de páginas (PTE con bits reservados, triple
/// fallo en `execve`). Mejor devolver ENOMEM al proceso que corromper el núcleo.
pub const RESERVA_MONTICULO: usize = 24 * 1024 * 1024;

#[inline]
fn monticulo_con_margen() -> bool {
    let en_uso = crate::mem::allocator::ALLOC_STATS.bytes_en_uso.load(Ordering::Relaxed);
    crate::mem::HEAP_SIZE.saturating_sub(en_uso) > RESERVA_MONTICULO
}

pub fn asignar_marco_usuario() -> Option<usize> {
    if let Some(p) = crate::mem::marcos::asignar() {
        return Some(phys_to_virt(p));
    }
    if !monticulo_con_margen() {
        crate::mem::marcos::RESPALDO_DENEGADO.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    crate::mem::marcos::RESPALDO_HEAP.fetch_add(1, Ordering::Relaxed);
    alloc_page_table()
}

/// [Fase B.4] `n` marcos contiguos para la pila de usuario (alias alto).
pub fn asignar_marcos_usuario_contiguos(n: usize) -> Option<usize> {
    if let Some(p) = crate::mem::marcos::asignar_contiguos(n) {
        return Some(phys_to_virt(p));
    }
    if !monticulo_con_margen() {
        crate::mem::marcos::RESPALDO_DENEGADO.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    crate::mem::marcos::RESPALDO_HEAP.fetch_add(1, Ordering::Relaxed);
    let layout = Layout::from_size_align(n * PAGE_SIZE, PAGE_SIZE).ok()?;
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() { None } else { Some(ptr as usize) }
}

/// [Fase B.4] Libera lo que entregó `asignar_marcos_usuario_contiguos`
/// (física o alias alto), devolviéndolo a su dueño real.
pub fn liberar_marcos_usuario_contiguos(base: usize, n: usize) {
    if base == 0 || n == 0 { return; }
    if crate::mem::marcos::es_marco(base) {
        crate::mem::marcos::liberar_contiguos(virt_to_phys(base), n);
        return;
    }
    if let Ok(layout) = Layout::from_size_align(n * PAGE_SIZE, PAGE_SIZE) {
        unsafe { alloc::alloc::dealloc(phys_to_virt(virt_to_phys(base)) as *mut u8, layout); }
    } else {
        serial::log("PAGING", "CRITICAL: layout invalido al liberar marcos contiguos\n");
    }
}

pub(crate) fn free_page(paddr: usize) {
    // [Fase B.4] Los marcos del asignador físico vuelven a su mapa de bits; el
    // resto (tablas de páginas, respaldo) al montículo.
    if crate::mem::marcos::es_marco(paddr) {
        crate::mem::marcos::liberar(paddr);
        return;
    }
    unsafe {
        let layout = match Layout::from_size_align(PAGE_SIZE, PAGE_SIZE) {
            Ok(l) => l,
            Err(_) => {
                serial::log("PAGING", "CRITICAL: invalid layout in free_page\n");
                return;
            }
        };
        // [Fase B.2] Puede llegar la física (desde una PTE) o el alias alto.
        alloc::alloc::dealloc(phys_to_virt(paddr) as *mut u8, layout);
    }
}

/// [FIX-ALIAS] Tabla del MAESTRO correspondiente a una entrada de un espacio de
/// direcciones de usuario. `new_address_space()` copia por referencia las
/// entradas del PML4 del núcleo (la identidad de la memoria física en la mitad
/// baja), así que un proceso comparte esas TABLAS con el maestro: tocarlas
/// (crear entradas, marcar USER/WRITABLE, dividir páginas gigantes) alteraría a
/// TODOS los espacios de direcciones a la vez, y liberarlas al salir destruiría
/// el mapeo del propio núcleo. Con esta ruta se sabe, nivel a nivel, qué tabla
/// es compartida y cuándo hay que clonarla.
fn master_subtable(master: Option<usize>, index: usize) -> Option<usize> {
    let m = master?;
    unsafe {
        let e = read_entry(m, index);
        if e & PRESENT == 0 || e & HUGE_PAGE != 0 {
            return None;
        }
        Some(entry_paddr(e))
    }
}

/// [FIX-ALIAS] Copia el contenido de una tabla para que el proceso tenga una
/// versión privada antes de modificarla.
fn clone_table(src: usize) -> Option<usize> {
    let nueva = alloc_page_table()?;
    unsafe {
        core::ptr::copy_nonoverlapping(phys_to_virt(src) as *const u8, nueva as *mut u8, 4096);
    }
    Some(nueva)
}

fn ensure_or_create(
    phys: usize,
    index: usize,
    user: bool,
    master: Option<usize>,
) -> Option<usize> {
    unsafe {
        let entry = read_entry(phys, index);
        if entry & PRESENT != 0 {
            let child_tbl = entry_paddr(entry);
            // Si la tabla es la MISMA que usa el maestro (heredada de
            // `new_address_space`), primero se clona: a partir de aquí el
            // proceso trabaja sobre su propia copia y el núcleo queda intacto.
            if master_subtable(master, index) == Some(child_tbl) {
                let privada = clone_table(child_tbl)?;
                let mut flags = entry & 0xFFF;
                flags |= PRESENT;
                write_entry(phys, index, virt_to_phys(privada) as u64 | flags);
                flush_tlb();
                let mut updated = read_entry(phys, index);
                if user && updated & USER == 0 {
                    updated |= USER;
                    write_entry(phys, index, updated);
                }
                return Some(privada);
            }
            let mut updated = entry;
            if entry & WRITABLE == 0 {
                updated |= WRITABLE;
            }
            if user && entry & USER == 0 && entry & HUGE_PAGE == 0 {
                updated |= USER;
            }
            if updated != entry {
                write_entry(phys, index, updated);
                flush_tlb();
            }
            return Some(child_tbl);
        }
    }
    let new_tbl = alloc_page_table()?;
    let mut flags: u64 = PRESENT | WRITABLE;
    if user { flags |= USER; }
    unsafe {
        write_entry(phys, index, virt_to_phys(new_tbl) as u64 | flags);
    }
    flush_tlb();
    Some(new_tbl)
}

/// [FIX-ALIAS] Ruta privada hasta la tabla de último nivel de una dirección.
/// Clona, si hace falta, las tablas heredadas del núcleo y devuelve el puntero
/// de la tabla de páginas que el proceso puede modificar sin efectos colaterales.
fn private_leaf_table(cr3: u64, vaddr: usize, user: bool) -> Option<usize> {
    let master_root = master_pml4();
    let m1 = master_subtable(Some(master_root), pml4_index(vaddr));
    let l2 = ensure_or_create(cr3 as usize, pml4_index(vaddr), user, Some(master_root))?;
    // [b191] Igual que `map_page`: si el rango está cubierto por una página
    // grande (identidad de 1 GiB / 2 MiB de la memoria baja), se parte ANTES de
    // descender. Sin esto, `ensure_or_create` tomaba la dirección física de la
    // página grande como si fuera una tabla y `mapear_perezosa` escribía la
    // PTE dentro de la propia memoria del rango (TEST 4910: MAP_FIXED 0x2000000
    // -> #PF EC=7 y corrupción silenciosa).
    unsafe {
        let pdpte = read_entry(l2, pdpt_index(vaddr));
        if pdpte & PRESENT != 0 && pdpte & HUGE_PAGE != 0 { split_huge_pdpt(l2, pdpt_index(vaddr))?; }
    }
    let m2 = master_subtable(m1, pdpt_index(vaddr));
    let l3 = ensure_or_create(l2, pdpt_index(vaddr), user, m1)?;
    unsafe {
        let pde = read_entry(l3, pd_index(vaddr));
        if pde & PRESENT != 0 && pde & HUGE_PAGE != 0 { split_huge_pde(l3, pd_index(vaddr))?; }
    }
    let m3 = master_subtable(m2, pd_index(vaddr));
    ensure_or_create(l3, pd_index(vaddr), user, m2).map(|pt| { let _ = m3; pt })
}

/// PML4 del maestro (el espacio de direcciones del núcleo).
pub fn master_pml4() -> usize {
    let master_cr3 = KERNEL_CR3.load(Ordering::SeqCst);
    if master_cr3 != 0 { master_cr3 as usize } else { read_cr3() as usize }
}

fn split_huge_pde(pd_p: usize, pde_index: usize) -> Option<()> {
    unsafe {
        let pde = read_entry(pd_p, pde_index);
        if pde & PRESENT == 0 || pde & HUGE_PAGE == 0 {
            return None;
        }
        let huge_paddr = entry_paddr(pde);
        let huge_flags = ((pde & PTE_FLAGS_MASK) & !HUGE_PAGE) | WRITABLE;
        let pt_p = alloc_page_table()?;
        for i in 0..PAGE_ENTRIES {
            let pte = (huge_paddr + i * PAGE_SIZE) as u64 | huge_flags;
            write_entry(pt_p, i, pte);
        }
        let new_pde_flags = PRESENT | WRITABLE | ACCESSED | (pde & (USER | NO_EXECUTE));
        write_entry(pd_p, pde_index, virt_to_phys(pt_p) as u64 | new_pde_flags);
        flush_tlb();
        Some(())
    }
}

fn split_huge_pdpt(pdpt_p: usize, pdpt_index: usize) -> Option<()> {
    unsafe {
        let pdpte = read_entry(pdpt_p, pdpt_index);
        if pdpte & PRESENT == 0 || pdpte & HUGE_PAGE == 0 {
            return None;
        }
        let huge_paddr = entry_paddr(pdpte);
        let huge_flags = (pdpte & PTE_FLAGS_MASK) | WRITABLE;
        let pd_p = alloc_page_table()?;
        for i in 0..PAGE_ENTRIES {
            let pde = (huge_paddr + i * 0x200000) as u64 | huge_flags | HUGE_PAGE;
            write_entry(pd_p, i, pde);
        }
        let new_pdpte_flags = PRESENT | WRITABLE | ACCESSED | (pdpte & (USER | NO_EXECUTE));
        write_entry(pdpt_p, pdpt_index, virt_to_phys(pd_p) as u64 | new_pdpte_flags);
        flush_tlb();
        Some(())
    }
}

pub fn map_page(cr3: u64, vaddr: usize, paddr: usize, flags: u64) -> Result<(), &'static str> {
    // [Fase B.2] Los marcos de usuario salen del montículo (alias alto): a la
    // tabla de páginas siempre va la dirección FÍSICA.
    let paddr = virt_to_phys(paddr);
    let cr3_p = cr3 as usize;
    let user = flags & USER != 0;
    let master_root = master_pml4();

    let pdpt_p = ensure_or_create(cr3_p, pml4_index(vaddr), user, Some(master_root))
        .ok_or("no PDPT")?;

    let pdpt_idx = pdpt_index(vaddr);
    unsafe {
        let pdpte = read_entry(pdpt_p, pdpt_idx);
        if pdpte & PRESENT != 0 && pdpte & HUGE_PAGE != 0 {
            split_huge_pdpt(pdpt_p, pdpt_idx).ok_or("split 1G failed")?;
        }
    }

    let pd_p = ensure_or_create(
        pdpt_p,
        pdpt_idx,
        user,
        master_subtable(Some(master_root), pml4_index(vaddr)),
    )
    .ok_or("no PD")?;

    let pde_index = pd_index(vaddr);
    unsafe {
        let pde = read_entry(pd_p, pde_index);
        if pde & PRESENT != 0 && pde & HUGE_PAGE != 0 {
            split_huge_pde(pd_p, pde_index).ok_or("split 2M failed")?;
        }
    }

    let pt_p = ensure_or_create(
        pd_p,
        pde_index,
        user,
        master_subtable(
            master_subtable(Some(master_root), pml4_index(vaddr)),
            pdpt_idx,
        ),
    )
    .ok_or("no PT")?;

    let pte_index = pt_index(vaddr);
    unsafe {
        let clean_flags = flags & PTE_FLAGS_MASK;
        let pte_val = (paddr as u64 & PTE_ADDR_MASK) | clean_flags;
        write_entry(pt_p, pte_index, pte_val);
    }

    if cr3 == read_cr3() {
        flush_tlb_page(vaddr);
    }

    Ok(())
}

pub fn unmap_page(cr3: u64, vaddr: usize) -> Result<(), &'static str> {
    // [FIX-ALIAS] Se recorre la ruta privada: si la tabla venía heredada del
    // núcleo se clona antes de tocar nada.
    let pt_p = private_leaf_table(cr3, vaddr, false).ok_or("sin tabla")?;
    let pte_index = pt_index(vaddr);
    unsafe {
        let pte = read_entry(pt_p, pte_index);
        if pte & PRESENT == 0 && !es_perezosa(pte) && !es_protegida(pte) { return Err("no PTE"); }
        write_entry(pt_p, pte_index, 0);
    }
    if cr3 == read_cr3() {
        flush_tlb_page(vaddr);
    }

    Ok(())
}

// ── [T-027] Cerrojo de actualización de PTE ────────────────────────────────
// Leer una entrada, decidir y escribirla NO era atómico entre CPUs. Dos casos
// reales se pisaban:
//   * `mprotect` en una CPU (p. ej. HotSpot haciendo escribible la pila de un
//     hilo nuevo) y un fallo de página en otra, materializando la misma página
//     perezosa. El que escribía segundo partía de su lectura vieja y dejaba la
//     página con la protección ANTERIOR; si esa anterior era PROT_NONE, el
//     primer `push rbp` del hilo daba SIGSEGV con la pila «no escribible».
//   * `munmap`/`map_page` sobre la misma tabla.
// La sección crítica es corta (leer, decidir, escribir): la asignación de marco
// y el relleno desde fichero quedan FUERA del cerrojo.
static CERROJO_PTE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Veces que una actualización encontró la entrada cambiada por otra CPU y tuvo
/// que rehacer la decisión con la protección vigente (la carrera, ya resuelta).
pub static PTE_CARRERAS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

struct GuardPte;
fn cerrojo_pte() -> GuardPte {
    while CERROJO_PTE.compare_exchange_weak(false, true,
            core::sync::atomic::Ordering::Acquire, core::sync::atomic::Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
    GuardPte
}
impl Drop for GuardPte {
    fn drop(&mut self) { CERROJO_PTE.store(false, core::sync::atomic::Ordering::Release); }
}

pub fn mprotect_page(cr3: u64, vaddr: usize, prot: i32) -> Result<(), &'static str> {
    // [FIX-ALIAS] Ruta privada antes de modificar los permisos.
    let pt_p = private_leaf_table(cr3, vaddr, false).ok_or("sin tabla")?;
    let pte_index = pt_index(vaddr);
    // [T-027] Toda la decisión y la escritura de la entrada, bajo el cerrojo.
    let _g = cerrojo_pte();
    unsafe {
        let pte = read_entry(pt_p, pte_index);
        if es_perezosa(pte) { reproteger_perezosa(pt_p, pte_index, prot); if cr3 == read_cr3() { flush_tlb_page(vaddr); } return Ok(()); }
        // [T-035] Página protegida con marco: se actualiza su protección sin
        // perder el marco. Si la nueva protección permite algún acceso, vuelve a
        // ser presente con el MISMO marco (el contenido se conserva).
        if es_protegida(pte) {
            let paddr = pte & PTE_ADDR_MASK;
            let cow = pte & COW_FLAG != 0;
            let nueva = if prot & 7 == 0 { pte_protegida(paddr, 0, cow) }
                        else { paddr | flags_desde_prot(prot as u32 & 7, cow) };
            write_entry(pt_p, pte_index, nueva);
            if cr3 == read_cr3() { flush_tlb_page(vaddr); }
            return Ok(());
        }
        if pte & PRESENT == 0 { return Err("no PTE"); }
        let paddr = pte & PTE_ADDR_MASK;
        // [T-035] `PROT_NONE` sobre una página materializada: se oculta (sin
        // PRESENT) conservando marco, contenido y marca COW. Dejarla presente
        // con sólo quitar WRITABLE no protege nada en x86-64.
        if prot & 7 == 0 {
            let cow = pte & COW_FLAG != 0;
            write_entry(pt_p, pte_index, pte_protegida(paddr, 0, cow));
            if cr3 == read_cr3() { flush_tlb_page(vaddr); }
            return Ok(());
        }
        let mut new_flags = pte & PTE_FLAGS_MASK;
        // [T1b.2] PROT_EXEC (4) quita NX; sin él, la página deja de ser ejecutable.
        if prot & 4 != 0 { new_flags &= !NO_EXECUTE; } else { new_flags |= nx_bit(); }
        if prot & 2 != 0 {
            // [CICLO 50 / T1c] Una página COW compartida tras fork NO se vuelve
            // escribible aquí: el permiso de escritura lo concede el manejador
            // de COW al copiar el marco. Si no, padre e hijo escribirían el
            // mismo marco físico.
            if pte & COW_FLAG == 0 { new_flags |= WRITABLE; }
        } else {
            new_flags &= !WRITABLE;
        }
        write_entry(pt_p, pte_index, paddr | new_flags);
    }
    if cr3 == read_cr3() {
        flush_tlb_page(vaddr);
    }
    Ok(())
}

pub fn mprotect_range(cr3: u64, vaddr_start: usize, pages: usize, prot: i32) -> Result<(), &'static str> {
    inicio_lote_tlb();
    for i in 0..pages {
        let _ = mprotect_page(cr3, vaddr_start + i * PAGE_SIZE, prot);
        // [T-026] `modules` de la JVM: 34 000 páginas con un shootdown por página
        // (hilos en el otro núcleo) sumaban 1,5 s con IF=0 al final del `mmap`.
        if i & 0xFF == 0xFF { respirar(); }
    }
    fin_lote_tlb();
    Ok(())
}

pub fn mark_page_cow(cr3: u64, vaddr: usize) -> Result<(), &'static str> {
    // [FIX-ALIAS] La marca COW se escribe en tablas privadas del proceso.
    let pt_p = private_leaf_table(cr3, vaddr, false).ok_or("sin tabla")?;
    let pte_index = pt_index(vaddr);
    unsafe {
        let pte = read_entry(pt_p, pte_index);
        if pte & PRESENT == 0 { return Err("no PTE"); }
        let paddr = pte & PTE_ADDR_MASK;
        let new_flags = (pte & PTE_FLAGS_MASK & !WRITABLE) | COW_FLAG;
        write_entry(pt_p, pte_index, paddr | new_flags);
    }
    if cr3 == read_cr3() {
        flush_tlb_page(vaddr);
    }
    Ok(())
}

pub fn mark_range_cow(cr3: u64, vaddr_start: usize, pages: usize) -> Result<(), &'static str> {
    inicio_lote_tlb();
    for i in 0..pages {
        if let Err(e) = mark_page_cow(cr3, vaddr_start + i * PAGE_SIZE) { fin_lote_tlb(); return Err(e); }
        if i & 0xFF == 0xFF { respirar(); } // [T-026]
    }
    fin_lote_tlb();
    Ok(())
}

pub fn handle_cow_fault(cr3: u64, vaddr: usize) -> bool {
    let aligned = page_align_down(vaddr);
    let (old_paddr, flags) = match translate(cr3, aligned) {
        Some(t) => t,
        None => return false,
    };
    if flags & COW_FLAG == 0 {
        return false;
    }
    let new_flags = (flags & !COW_FLAG) | WRITABLE;
    // [FRENTE C] Si este espacio de direcciones es el ÚNICO dueño del marco
    // (los demás ya lo soltaron o copiaron), no hay nada que proteger: la
    // página se vuelve escribible en el sitio y se evita la copia.
    if crate::mem::frames::count(old_paddr) == 1 {
        let _ = map_page(cr3, aligned, old_paddr, new_flags);
        COW_FAULTS_TOTAL.fetch_add(1, Ordering::Relaxed);
        crate::mem::frames::COPIAS_EVITADAS.fetch_add(1, Ordering::Relaxed);
        if cr3 == read_cr3() {
            flush_tlb_page(aligned);
        }
        return true;
    }
    let new_page = match asignar_marco_usuario() { // [Fase B.4]
        Some(p) => p,
        None => return false,
    };
    unsafe {
        core::ptr::copy_nonoverlapping(phys_to_virt(old_paddr) as *const u8, new_page as *mut u8, PAGE_SIZE);
    }
    let _ = map_page(cr3, aligned, new_page, new_flags);
    // La copia privada nace con una referencia y el original pierde la nuestra.
    crate::mem::frames::retain(new_page);
    if let Some(0) = crate::mem::frames::release(old_paddr) {
        free_page(old_paddr);
    }
    crate::mem::frames::COPIAS_REALIZADAS.fetch_add(1, Ordering::Relaxed);
    COW_FAULTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    COW_PAGES_CLONED.fetch_add(1, Ordering::Relaxed);
    if cr3 == read_cr3() {
        flush_tlb_page(aligned);
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
//  [b191 / T-014] Páginas PEREZOSAS: reserva de espacio de direcciones sin marco.
//
//  `mmap` anónimo ya no asigna un marco por página al instante: escribe una PTE
//  NO presente con `DEMAND_PAGING_FLAG` y la protección pedida en los bits
//  52..54 (R=1 W=2 X=4). El primer acceso provoca #PF y `materializar_perezosa`
//  asigna un marco a cero con esa protección; un acceso que la protección no
//  permite (PROT_NONE, escritura en sólo lectura) NO materializa y acaba en
//  SIGSEGV, exactamente como en Linux. Así el JVM de OpenJDK puede reservar
//  cientos de MB (montículo, caché de código, espacio de clases) sobre 428 MB
//  de RAM, y un `calloc` grande deja de costar RAM hasta que se usa.
// ─────────────────────────────────────────────────────────────────────────────
pub const PEREZOSA_PROT_SHIFT: u64 = 52;
/// [T-035] Restauraciones de una página protegida con marco al permitir el acceso.
pub static PROTEGIDAS_RESTAURADAS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub const PEREZOSA_PROT_MASK: u64 = 0x7 << PEREZOSA_PROT_SHIFT;

#[inline] pub fn es_perezosa(pte: u64) -> bool { pte & PRESENT == 0 && pte & DEMAND_PAGING_FLAG != 0 }
#[inline] pub fn prot_perezosa(pte: u64) -> u32 { ((pte & PEREZOSA_PROT_MASK) >> PEREZOSA_PROT_SHIFT) as u32 }

// ─────────────────────────────────────────────────────────────────────────────
//  [T-035] Páginas PROTEGIDAS con marco.
//
//  x86-64 no tiene páginas PRESENTES ilegibles para Ring 3: el bit R/W sólo
//  limita la ESCRITURA, así que una página "presente de sólo lectura" siempre
//  se puede leer. `mprotect(PROT_NONE)` sobre una página ya materializada
//  tiene que QUITAR `PRESENT`, como hace Linux, y guardar en la entrada la
//  dirección física y la protección pedida. El bit 55 distingue esta página
//  (existe, tiene marco, está prohibida) de una reserva perezosa (bit 10, sin
//  marco) y de un hueco (PTE cero). La protección se codifica igual que en las
//  perezosas (bits 52..54) para reutilizar lectores.
//
//  Al no estar presente, `translate` no la devuelve (el núcleo no puede
//  tocarla por la dirección de usuario: las copias darán error), pero los
//  recorridos de propiedad, `fork`, `munmap` y liberación DEBEN verla o se
//  perdería el marco.
// ─────────────────────────────────────────────────────────────────────────────
pub const PROTEGIDA_FLAG: u64 = 1 << 55;

#[inline] pub fn es_protegida(pte: u64) -> bool { pte & PRESENT == 0 && pte & PROTEGIDA_FLAG != 0 }
#[inline] pub fn prot_guardada(pte: u64) -> u32 { ((pte & PEREZOSA_PROT_MASK) >> PEREZOSA_PROT_SHIFT) as u32 }

/// Entrada de una página protegida con marco: sin `PRESENT`, con su dirección
/// física, su protección y, si venía de un `mprotect` sobre una página COW, la
/// marca COW (que impide conceder escritura sin copiar el marco).
fn pte_protegida(paddr: u64, prot: u32, cow: bool) -> u64 {
    let mut v = (paddr & PTE_ADDR_MASK) | PROTEGIDA_FLAG | nx_bit() | ((prot as u64 & 7) << PEREZOSA_PROT_SHIFT);
    if cow { v |= COW_FLAG; }
    v
}

/// Flags de una PTE presente a partir de la protección pedida. No incluye la
/// dirección física. Igual que antes: `PROT_EXEC` quita NX y una página COW no
/// nace escribible aunque se pida `PROT_WRITE` (lo concede el manejador de COW).
fn flags_desde_prot(prot: u32, cow: bool) -> u64 {
    let mut f = PRESENT | USER;
    if prot & 2 != 0 && !cow { f |= WRITABLE; }
    if prot & 4 == 0 { f |= nx_bit(); }
    if cow { f |= COW_FLAG; }
    f
}

/// Valor de PTE perezosa para `prot`. USER sólo si es accesible (prot != 0): así
/// `for_each_user_page` y las comprobaciones de ocupación la ven como del proceso.
fn pte_perezosa(prot: u32) -> u64 {
    let mut v = DEMAND_PAGING_FLAG | ((prot as u64 & 7) << PEREZOSA_PROT_SHIFT);
    if prot != 0 { v |= USER; }
    if prot & 2 != 0 { v |= WRITABLE; }
    if prot & 4 == 0 { v |= nx_bit(); }
    v
}

/// PTE cruda (presente o no) de `vaddr`; None si no hay tabla hoja o es página grande.
pub fn pte_cruda(cr3: u64, vaddr: usize) -> Option<u64> {
    let cr3_p = cr3 as usize;
    unsafe {
        let pml4e = read_entry(cr3_p, pml4_index(vaddr));
        if pml4e & PRESENT == 0 { return None; }
        let pdpte = read_entry(entry_paddr(pml4e), pdpt_index(vaddr));
        if pdpte & PRESENT == 0 || pdpte & HUGE_PAGE != 0 { return None; }
        let pde = read_entry(entry_paddr(pdpte), pd_index(vaddr));
        if pde & PRESENT == 0 || pde & HUGE_PAGE != 0 { return None; }
        Some(read_entry(entry_paddr(pde), pt_index(vaddr)))
    }
}

/// ¿`vaddr` pertenece al proceso (página de usuario presente o reserva perezosa)?
pub fn pagina_del_proceso(cr3: u64, vaddr: usize) -> bool {
    match pte_cruda(cr3, vaddr) {
        Some(pte) => (pte & PRESENT != 0 && pte & USER != 0) || es_perezosa(pte) || es_protegida(pte),
        None => translate(cr3, vaddr).map_or(false, |(_, f)| f & USER != 0),
    }
}

/// Reserva perezosa de una página con `prot` (sin marco). Sustituye cualquier
/// reserva perezosa previa; si había una página presente, el llamador debe
/// liberarla antes.
pub fn mapear_perezosa(cr3: u64, vaddr: usize, prot: u32) -> Result<(), &'static str> {
    let aligned = page_align_down(vaddr);
    let pt_p = private_leaf_table(cr3, aligned, true).ok_or("sin tabla")?;
    unsafe { write_entry(pt_p, pt_index(aligned), pte_perezosa(prot)); }
    if cr3 == read_cr3() { flush_tlb_page(aligned); }
    Ok(())
}

/// Cambia la protección de una reserva perezosa sin materializarla.
fn reproteger_perezosa(pt_p: usize, idx: usize, prot: i32) {
    unsafe { write_entry(pt_p, idx, pte_perezosa(prot as u32 & 7)); }
}

/// Materializa la página perezosa que contiene `vaddr` si su protección permite
/// el acceso (`escritura` exige PROT_WRITE; lectura/ejecución exigen prot != 0).
/// Devuelve false si no es perezosa, si el acceso no está permitido o sin memoria.
pub fn materializar_perezosa(cr3: u64, vaddr: usize, escritura: bool) -> bool {
    let aligned = page_align_down(vaddr);
    let pte = match pte_cruda(cr3, aligned) { Some(p) => p, None => return false };
    // [T-035] Página protegida CON marco: el fallo es legítimo si la protección
    // permite el acceso; basta devolver la entrada a presente con su mismo
    // marco (no se asigna memoria ni se pierde el contenido). Si la protección
    // no lo permite, se devuelve false y el fallo acaba en SIGSEGV/ACCERR.
    if es_protegida(pte) {
        let prot = prot_guardada(pte);
        if prot == 0 || (escritura && prot & 2 == 0) { return false; }
        let cow = pte & COW_FLAG != 0;
        let nueva = (pte & PTE_ADDR_MASK) | flags_desde_prot(prot, cow);
        let pt_p = match private_leaf_table(cr3, aligned, true) { Some(t) => t, None => return false };
        unsafe { write_entry(pt_p, pt_index(aligned), nueva); }
        if cr3 == read_cr3() { flush_tlb_page(aligned); }
        PROTEGIDAS_RESTAURADAS.fetch_add(1, Ordering::Relaxed);
        return true;
    }
    if !es_perezosa(pte) { return false; }
    let prot = prot_perezosa(pte);
    if prot == 0 || (escritura && prot & 2 == 0) { return false; }
    let marco = match asignar_marco_usuario() { Some(p) => p, None => return false };
    unsafe { core::ptr::write_bytes(marco as *mut u8, 0, PAGE_SIZE); }
    // [T-014b] Reserva respaldada por fichero (mmap privado perezoso): el marco
    // se rellena con su trozo de fichero antes de aparecer en el espacio.
    {
        let buf = unsafe { core::slice::from_raw_parts_mut(marco as *mut u8, PAGE_SIZE) };
        let _ = crate::mem::vmas::rellenar_desde_fichero(cr3, aligned, buf);
    }
    // [T-027] La decisión se REHACE con la entrada vigente: entre la lectura de
    // arriba (donde se vio perezosa) y este instante, otra CPU pudo aplicar un
    // `mprotect` (escribible la pila de un hilo nuevo, por ejemplo). Escribir
    // con la protección vieja dejaba la página materializada con los permisos
    // ANTERIORES —PROT_NONE incluido— y el primer acceso del dueño acababa en
    // SIGSEGV. La instrucción de página manda sobre la reserva.
    let _g = cerrojo_pte();
    let mut prot = prot;
    if let Some(actual) = pte_cruda(cr3, aligned) {
        if es_perezosa(actual) {
            let p2 = prot_perezosa(actual);
            if p2 != prot { PTE_CARRERAS.fetch_add(1, Ordering::Relaxed); prot = p2; }
        } else if actual & PRESENT != 0 {
            // Otra CPU ya la materializó: no se duplica el marco.
            liberar_marco_usuario(marco);
            if cr3 == read_cr3() { flush_tlb_page(aligned); }
            return true;
        }
    }
    if prot == 0 || (escritura && prot & 2 == 0) {
        // La reserva cambió a PROT_NONE mientras materializábamos: el acceso ya
        // no está permitido y debe acabar en señal, como en Linux.
        liberar_marco_usuario(marco);
        return false;
    }
    let mut flags = PRESENT | USER;
    if prot & 2 != 0 { flags |= WRITABLE; }
    if prot & 4 == 0 { flags |= nx_bit(); }
    if map_page(cr3, aligned, marco, flags).is_err() { liberar_marco_usuario(marco); return false; }
    drop(_g);
    DEMAND_FAULTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    if cr3 == read_cr3() { flush_tlb_page(aligned); }
    true
}

/// [T-025 E3d] Fallo de página ESPURIO: la PTE ya está presente, de usuario y
/// con el permiso que se pedía. Pasa con dos CPU sobre el mismo espacio: dos
/// hilos tocan la misma página perezosa a la vez; el segundo espera el cerrojo
/// grande y, cuando entra, el primero ya la materializó (o la TLB de esta CPU
/// guarda una traducción vieja que otra CPU cambió). Antes se entregaba un
/// SIGSEGV a la JVM en `memset`. Se purga la TLB local y se reintenta.
pub fn fallo_espurio(cr3: u64, vaddr: usize, escritura: bool, ejecucion: bool) -> bool {
    let aligned = page_align_down(vaddr);
    let pte = match pte_cruda(cr3, aligned) { Some(p) => p, None => return false };
    if pte & PRESENT == 0 || pte & USER == 0 { return false; }
    if escritura && pte & WRITABLE == 0 { return false; }
    // [88ª, defecto 26] Fallo de CAPTACIÓN de instrucción (EC bit 4) sobre una
    // página ya presente y ejecutable: dos hilos de la JVM entraban a la vez en
    // la misma página de texto de libjvm.so (mmap de fichero perezoso, T-014b)
    // desde dos CPUs; la segunda llegaba con la PTE ya puesta por la primera y
    // se le entregaba SIGSEGV con pc == CR2 (abort de HotSpot en `java -version`,
    // suite 95b). Antes el reintento espurio excluía toda captación.
    if ejecucion && pte & nx_bit() != 0 { return false; }
    FALLOS_ESPURIOS.fetch_add(1, Ordering::Relaxed);
    if cr3 == read_cr3() { flush_tlb_page(aligned); }
    true
}
pub static FALLOS_ESPURIOS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Copia las reservas perezosas de `[lo, hi)` de un espacio a otro (fork).
pub fn copiar_perezosas(cr3_origen: u64, cr3_destino: u64, lo: usize, hi: usize) {
    if hi <= lo { return; }
    let src = cr3_origen as usize;
    let mut v = lo & !(PAGE_SIZE - 1);
    while v < hi {
        let pml4e = unsafe { read_entry(src, pml4_index(v)) };
        if pml4e & PRESENT == 0 { v = ((v >> 39) + 1) << 39; continue; }
        let pdpte = unsafe { read_entry(entry_paddr(pml4e), pdpt_index(v)) };
        if pdpte & PRESENT == 0 || pdpte & HUGE_PAGE != 0 { v = ((v >> 30) + 1) << 30; continue; }
        let pde = unsafe { read_entry(entry_paddr(pdpte), pd_index(v)) };
        if pde & PRESENT == 0 || pde & HUGE_PAGE != 0 { v = ((v >> 21) + 1) << 21; continue; }
        let pte = unsafe { read_entry(entry_paddr(pde), pt_index(v)) };
        if es_perezosa(pte) {
            let _ = mapear_perezosa(cr3_destino, v, prot_perezosa(pte));
        }
        v += PAGE_SIZE;
    }
}

/// [T-035] `fork`: copia profunda de las páginas protegidas con marco de
/// `[lo, hi)`. Se copia el contenido en un marco nuevo en vez de compartirlo
/// con COW porque la página está prohibida: compartir el marco obligaría a
/// tratarla como COW en cuanto se restaure, y ese camino no está probado. El
/// padre conserva su marco y su protección; el hijo recibe una copia con la
/// misma protección. Devuelve cuántas copió.
pub fn copiar_protegidas(cr3_origen: u64, cr3_destino: u64, lo: usize, hi: usize) -> usize {
    if hi <= lo { return 0; }
    let mut pendientes: alloc::vec::Vec<(usize, usize, u64)> = alloc::vec::Vec::new();
    for_each_user_page(cr3_origen, lo, hi, &mut |v, p, f| {
        if es_protegida(f) { pendientes.push((v, p, f)); }
    });
    let mut copiadas = 0usize;
    for (i, (v, paddr, flags)) in pendientes.into_iter().enumerate() {
        if i & 63 == 63 { respirar(); }
        let marco = match asignar_marco_usuario() { Some(m) => m, None => break };
        unsafe {
            core::ptr::copy_nonoverlapping(phys_to_virt(paddr) as *const u8, marco as *mut u8, PAGE_SIZE);
        }
        let pte_nueva = (virt_to_phys(marco) as u64 & PTE_ADDR_MASK) | (flags & PTE_FLAGS_MASK);
        match private_leaf_table(cr3_destino, v, true) {
            Some(pt_p) => {
                unsafe { write_entry(pt_p, pt_index(v), pte_nueva); }
                copiadas += 1;
            }
            None => liberar_marco_usuario(marco),
        }
    }
    if copiadas > 0 && cr3_destino == read_cr3() { flush_tlb(); }
    copiadas
}

/// Compatibilidad: el #PF llama aquí con el bit de escritura del código de error.
pub fn handle_demand_fault(cr3: u64, vaddr: usize, escritura: bool) -> bool {
    materializar_perezosa(cr3, vaddr, escritura)
}

/// Resuelve un fallo de página por asignación diferida (Demand Paging)
/// [T-004] Crecimiento de la pila de usuario por fallo de página: si `vaddr`
/// cae en la pila extendida y no está mapeada, se asigna un marco a cero,
/// escribible, de usuario y NX. Devuelve false fuera del rango o sin memoria.
pub fn crecer_pila(cr3: u64, vaddr: usize) -> bool {
    let (lo, hi) = crate::process::pila_extendida();
    if vaddr < lo || vaddr >= hi { return false; }
    let aligned = page_align_down(vaddr);
    if let Some((_, f)) = translate(cr3, aligned) { if f & PRESENT != 0 { return false; } }
    let marco = match asignar_marco_usuario() { Some(p) => p, None => return false };
    unsafe { core::ptr::write_bytes(marco as *mut u8, 0, PAGE_SIZE); }
    if map_page(cr3, aligned, marco, PRESENT | WRITABLE | USER | NO_EXECUTE).is_err() { liberar_marco_usuario(marco); return false; }
    if cr3 == read_cr3() { flush_tlb_page(aligned); }
    true
}

// ── Gestor de Espacio de Intercambio (Swap Space) ───────────────────────────
pub const SWAP_SLOTS_MAX: usize = 1024; // 1024 páginas = 4 MB de swap

pub struct SwapManager {
    pub slots: [bool; SWAP_SLOTS_MAX],
    pub pages: alloc::collections::BTreeMap<u32, alloc::vec::Vec<u8>>,
}

impl SwapManager {
    pub const fn new() -> Self {
        Self {
            slots: [false; SWAP_SLOTS_MAX],
            pages: alloc::collections::BTreeMap::new(),
        }
    }

    pub fn alloc_slot(&mut self) -> Option<u32> {
        for (i, used) in self.slots.iter_mut().enumerate() {
            if !*used {
                *used = true;
                return Some(i as u32);
            }
        }
        None
    }

    pub fn free_slot(&mut self, slot: u32) {
        if (slot as usize) < SWAP_SLOTS_MAX {
            self.slots[slot as usize] = false;
            self.pages.remove(&slot);
        }
    }

    pub fn swap_out(&mut self, slot: u32, paddr: usize) -> bool {
        if (slot as usize) >= SWAP_SLOTS_MAX {
            return false;
        }
        let mut buf = alloc::vec::Vec::with_capacity(PAGE_SIZE);
        buf.resize(PAGE_SIZE, 0);
        unsafe {
            core::ptr::copy_nonoverlapping(phys_to_virt(paddr) as *const u8, buf.as_mut_ptr(), PAGE_SIZE);
        }
        self.pages.insert(slot, buf);
        SWAP_OUT_TOTAL.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub fn swap_in(&mut self, slot: u32, paddr: usize) -> bool {
        if let Some(buf) = self.pages.get(&slot) {
            unsafe {
                core::ptr::copy_nonoverlapping(buf.as_ptr(), phys_to_virt(paddr) as *mut u8, PAGE_SIZE);
            }
            SWAP_IN_TOTAL.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn used_count(&self) -> usize {
        self.slots.iter().filter(|&&u| u).count()
    }
}

pub static SWAP_MANAGER: crate::arch::spinlock::Spinlock<SwapManager> =
    crate::arch::spinlock::Spinlock::new(SwapManager::new());

/// Desaloja una página de memoria física y la transfiere al espacio de intercambio (Swap Out)
pub fn swap_page_out(cr3: u64, vaddr: usize) -> Result<u32, &'static str> {
    let aligned = page_align_down(vaddr);
    let (paddr, _) = translate(cr3, aligned).ok_or("pagina no traducible")?;
    let mut sm = SWAP_MANAGER.lock();
    let slot = sm.alloc_slot().ok_or("swap lleno")?;
    if !sm.swap_out(slot, paddr) {
        sm.free_slot(slot);
        return Err("fallo al volcar en swap");
    }
    let swap_pte = ((slot as u64) << 12) | SWAP_FLAG | USER;
    let _ = map_page(cr3, aligned, 0, swap_pte);
    if cr3 == read_cr3() {
        flush_tlb_page(aligned);
    }
    Ok(slot)
}

/// Recupera una página desde el espacio de intercambio a la memoria física (Swap In)
pub fn swap_page_in(cr3: u64, vaddr: usize) -> Result<(), &'static str> {
    let aligned = page_align_down(vaddr);
    let (pte_val, flags) = translate(cr3, aligned).ok_or("pagina no traducible")?;
    if flags & SWAP_FLAG == 0 {
        return Err("pagina no en swap");
    }
    let slot = ((pte_val >> 12) & 0xFFFFF) as u32;
    let new_page = alloc_page_table().ok_or("sin memoria para swap-in")?;
    let mut sm = SWAP_MANAGER.lock();
    if !sm.swap_in(slot, new_page) {
        return Err("error leyendo de swap");
    }
    sm.free_slot(slot);
    let new_flags = PRESENT | WRITABLE | USER | ACCESSED | DIRTY;
    let _ = map_page(cr3, aligned, new_page, new_flags);
    if cr3 == read_cr3() {
        flush_tlb_page(aligned);
    }
    Ok(())
}

/// Devuelve métricas de memoria virtual avanzada: (cow_faults, cow_cloned, demand_faults, swap_in, swap_out, swap_used)
pub fn get_vm_advanced_stats() -> (u64, u64, u64, u64, u64, usize) {
    let cow_f = COW_FAULTS_TOTAL.load(Ordering::Relaxed);
    let cow_c = COW_PAGES_CLONED.load(Ordering::Relaxed);
    let dem_f = DEMAND_FAULTS_TOTAL.load(Ordering::Relaxed);
    let sw_in = SWAP_IN_TOTAL.load(Ordering::Relaxed);
    let sw_out = SWAP_OUT_TOTAL.load(Ordering::Relaxed);
    let sw_used = SWAP_MANAGER.lock().used_count();
    (cow_f, cow_c, dem_f, sw_in, sw_out, sw_used)
}

pub fn map_range(cr3: u64, vaddr_start: usize, paddr_start: usize, pages: usize, flags: u64) -> Result<(), &'static str> {
    for i in 0..pages {
        map_page(cr3, vaddr_start + i * PAGE_SIZE, paddr_start + i * PAGE_SIZE, flags)?;
    }
    Ok(())
}

pub fn map_page_user(cr3: u64, vaddr: usize, paddr: usize) -> Result<(), &'static str> {
    // [T1b.2] Pila, brk y mmap anónimo nacen NO ejecutables (W^X); PROT_EXEC se
    // concede después con `mprotect_range` (sys_mmap aplica `prot` real).
    map_page(cr3, vaddr, paddr, PRESENT | WRITABLE | USER | ACCESSED | DIRTY | nx_bit())
}

pub static KERNEL_CR3: AtomicU64 = AtomicU64::new(0);
static EXPECT_USER_FAULT: AtomicBool = AtomicBool::new(false);

/// [T-025 E3d] Espacio de direcciones de la CONSOLA del núcleo: una copia del
/// maestro creada una sola vez y nunca liberada. Cuando una CPU abandona el
/// contexto de un proceso que muere (vuelta al intérprete, bucle de espera de
/// salida, `kill_current_process`) no puede quedarse sobre las tablas del
/// muerto (otra CPU las libera y reutiliza → PML4 basura → triple fallo) ni
/// tampoco sobre el PML4 maestro directamente: `map_page`/`ensure_or_create`
/// clonan "la tabla heredada del maestro" al escribir y, si el CR3 activo ES
/// el maestro, lo mutan a sí mismo (el framebuffer dejó de estar mapeado →
/// #PF en `Framebuffer::present` tras `run java`).
pub static CR3_CONSOLA: AtomicU64 = AtomicU64::new(0);

pub fn cr3_consola() -> u64 {
    let v = CR3_CONSOLA.load(Ordering::SeqCst);
    if v != 0 { return v; }
    match new_address_space() {
        Some(nuevo) => {
            // Si otra CPU lo creó antes, se queda con el suyo (el nuestro se pierde:
            // una página, una sola vez; no merece un cerrojo).
            match CR3_CONSOLA.compare_exchange(0, nuevo, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => nuevo,
                Err(existente) => existente,
            }
        }
        None => KERNEL_CR3.load(Ordering::SeqCst),
    }
}

pub fn set_kernel_cr3(cr3: u64) {
    KERNEL_CR3.store(cr3, Ordering::SeqCst);
}

pub fn is_expecting_user_fault() -> bool {
    EXPECT_USER_FAULT.load(Ordering::SeqCst)
}

pub fn clear_expect_user_fault() {
    EXPECT_USER_FAULT.store(false, Ordering::SeqCst);
}

pub fn set_expect_user_fault(val: bool) {
    EXPECT_USER_FAULT.store(val, Ordering::SeqCst);
}

pub fn init() {
    let cr3 = read_cr3();
    set_kernel_cr3(cr3);
    serial::write_str("PAGING: ready\n");
}

pub fn new_address_space() -> Option<u64> {
    let master_cr3 = KERNEL_CR3.load(Ordering::SeqCst);
    let master_pml4_p = if master_cr3 != 0 { master_cr3 as usize } else { read_cr3() as usize };
    let new_pml4 = alloc_page_table()?;
    unsafe {
        for i in 0..PAGE_ENTRIES {
            let entry = read_entry(master_pml4_p, i);
            if entry & PRESENT != 0 {
                write_entry(new_pml4, i, entry);
            }
        }
    }
    Some(virt_to_phys(new_pml4) as u64)
}

pub(crate) fn is_dynamic_addr(paddr: usize) -> bool {
    // [Fase B.2] Acepta física o alias alto; compara siempre en física.
    let p = virt_to_phys(paddr);
    (p >= crate::mem::HEAP_PHYS_START && p < crate::mem::HEAP_PHYS_START + crate::mem::HEAP_SIZE)
        || crate::mem::marcos::es_marco(p) // [Fase B.4] marcos de usuario fuera del montículo
}

/// [FRENTE C] Devuelve un marco de usuario al asignador respetando el conteo
/// de referencias: si está rastreado (imagen ELF compartible por COW) sólo se
/// libera cuando era la última referencia; si no, se libera directamente.
pub fn liberar_marco_usuario(paddr: usize) {
    match crate::mem::frames::release(paddr) {
        Some(0) => free_page(paddr),
        Some(_) => {}
        None => free_page(paddr),
    }
}

/// [Fase B.4] Fin de la "ventana baja" de usuario: 0x200000–0x20000000, donde
/// un proceso puede pedir `MAP_FIXED` (antiguo montículo incluido) y donde
/// cargan los binarios no-PIE de distro. `fork`/`exit`/`execve` la recorren de
/// forma DISPERSA (por tablas, no página a página) con `for_each_user_page`.
pub const VENTANA_BAJA_FIN: usize = 0x2000_0000;

/// Recorre las páginas de USUARIO presentes en `[lo, hi)` saltando tablas
/// enteras no presentes o del núcleo (identidad plana: PDE gigante sin USER).
/// `f(vaddr, paddr, flags)`. No modifica nada.
pub fn for_each_user_page(cr3: u64, lo: usize, hi: usize, f: &mut dyn FnMut(usize, usize, u64)) {
    if cr3 == 0 || hi <= lo { return; }
    let cr3_p = cr3 as usize;
    let mut v = lo & !(PAGE_SIZE - 1);
    while v < hi {
        let pml4e = unsafe { read_entry(cr3_p, pml4_index(v)) };
        if pml4e & PRESENT == 0 || pml4e & USER == 0 {
            v = ((v >> 39) + 1) << 39; continue;
        }
        let pdpt_p = entry_paddr(pml4e);
        let pdpte = unsafe { read_entry(pdpt_p, pdpt_index(v)) };
        if pdpte & PRESENT == 0 || pdpte & HUGE_PAGE != 0 || pdpte & USER == 0 {
            v = ((v >> 30) + 1) << 30; continue;
        }
        let pd_p = entry_paddr(pdpte);
        let pde = unsafe { read_entry(pd_p, pd_index(v)) };
        if pde & PRESENT == 0 || pde & HUGE_PAGE != 0 || pde & USER == 0 {
            v = ((v >> 21) + 1) << 21; continue;
        }
        let pt_p = entry_paddr(pde);
        let pte = unsafe { read_entry(pt_p, pt_index(v)) };
        // [T-035] Una página protegida con marco no está presente pero SÍ es del
        // proceso: si este recorrido la ignorara, su marco no se liberaría
        // (fuga) y `fork` no la heredaría.
        if (pte & PRESENT != 0 && pte & USER != 0) || es_protegida(pte) {
            f(v, entry_paddr(pte), pte & PTE_FLAGS_MASK);
        }
        v += PAGE_SIZE;
    }
}

/// [Fase B.4] Libera los marcos de usuario de `[lo, hi)` por recorrido disperso
/// (mapeos `MAP_FIXED` fuera de brk/mmap). Mismo criterio que
/// `free_user_heap_pages`: sólo marcos dinámicos, respetando el conteo COW.
pub fn free_user_pages_dispersos(cr3: u64, lo: usize, hi: usize) {
    let mut lista: alloc::vec::Vec<(usize, usize)> = alloc::vec::Vec::new();
    for_each_user_page(cr3, lo, hi, &mut |v, p, _| {
        if is_dynamic_addr(p) { lista.push((v, p)); }
    });
    for (i, (v, p)) in lista.into_iter().enumerate() {
        liberar_marco_usuario(p);
        let _ = unmap_page(cr3, v);
        if i % 256 == 255 { respirar(); }
    }
}

/// [85ª, defecto 20] Punto de respiro: si esta CPU tiene IF=0 (toda syscall
/// entra así por FMASK), deja pasar las interrupciones pendientes un instante
/// y vuelve a cerrarlas. En un bucle largo del núcleo (liberar cientos de
/// miles de páginas al morir la JVM) el BSP pasaba hasta 884 ms sin atender
/// el PIT: `irq0_perdidos` en `/proc/smp` lo delataba. Con IF=1 no hace nada.
/// [T-026] Diagnóstico (/proc/smp): respiros que abrieron IF, los saltados por
/// una sección irqsave, y el peor tramo con IF=0 que ALCANZÓ un respiro
/// (microsegundos desde el último tick del PIT) con el fichero:línea del sitio.
pub static RESPIROS_ABIERTOS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static RESPIROS_SALTADOS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static RESPIRO_ULTIMO_TSC: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static RESPIRO_PEOR_INTERVALO_US: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static RESPIRO_PEOR_US: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static RESPIRO_PEOR_LINEA: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static RESPIRO_PEOR_FICHERO: core::sync::atomic::AtomicPtr<u8> = core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
pub static RESPIRO_PEOR_FICHERO_LEN: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[inline]
#[track_caller]
pub fn respirar() {
    use core::sync::atomic::Ordering as O;
    let rf: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) rf, options(nomem)); }
    if rf & (1 << 9) != 0 { return; }
    if crate::arch::spinlock::en_seccion_irqsave() { RESPIROS_SALTADOS.fetch_add(1, O::Relaxed); return; }
    // Sólo el BSP recibe el PIT: el tramo se mide frente a su último tick.
    if crate::arch::percpu::indice_actual() == 0 {
        let hz = crate::time::pit::TSC_HZ.load(O::Relaxed);
        let antes = crate::time::pit::TSC_ULTIMO_TICK.load(O::Relaxed);
        let ahora = unsafe { core::arch::x86_64::_rdtsc() };
        // Intervalo entre dos respiros consecutivos del BSP (¿es lento el trabajo
        // entre respiros, o el respiro no deja entrar la IRQ?).
        let previo = RESPIRO_ULTIMO_TSC.swap(ahora, O::Relaxed);
        if hz > 0 && antes != 0 {
            let us = ahora.wrapping_sub(antes) / (hz / 1_000_000).max(1);
            if us > RESPIRO_PEOR_US.load(O::Relaxed) {
                RESPIRO_PEOR_US.store(us, O::Relaxed);
                // Intervalo desde el respiro anterior EN ese peor tramo.
                let iu = if previo != 0 { ahora.wrapping_sub(previo) / (hz / 1_000_000).max(1) } else { 0 };
                RESPIRO_PEOR_INTERVALO_US.store(iu, O::Relaxed);
                let l = core::panic::Location::caller();
                RESPIRO_PEOR_LINEA.store(l.line() as u64, O::Relaxed);
                RESPIRO_PEOR_FICHERO.store(l.file().as_ptr() as *mut u8, O::Relaxed);
                RESPIRO_PEOR_FICHERO_LEN.store(l.file().len(), O::Relaxed);
            }
        }
    }
    RESPIROS_ABIERTOS.fetch_add(1, O::Relaxed);
    unsafe { core::arch::asm!("sti; nop; nop; nop; cli", options(nomem, nostack)); }
}

/// Nombre corto (sin ruta) del fichero del peor respiro, para /proc/smp.
pub fn respiro_peor_fichero() -> &'static str {
    use core::sync::atomic::Ordering as O;
    let p = RESPIRO_PEOR_FICHERO.load(O::Relaxed);
    let n = RESPIRO_PEOR_FICHERO_LEN.load(O::Relaxed);
    if p.is_null() || n == 0 { return "-"; }
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(p as *const u8, n)) };
    s.rsplit('/').next().unwrap_or(s)
}

pub fn free_user_heap_pages(cr3: u64, start: usize, end: usize) {
    if cr3 == 0 || start >= end { return; }
    let v0 = start & !(PAGE_SIZE - 1);
    let end_aligned = match end.checked_add(PAGE_SIZE - 1) {
        Some(e) => e & !(PAGE_SIZE - 1),
        None => return,
    };
    // [85ª, defecto 20] Recorrido DISPERSO (salta tablas no presentes) en vez de
    // `translate` página a página: el rango mmap de la JVM (cientos de MB, en
    // su mayoría huecos) costaba 884 ms con IF=0. `unmap_page` sólo toca la PTE
    // ya visitada, así que es seguro hacerlo dentro del recorrido. Cada 256
    // páginas liberadas se deja respirar a las interrupciones.
    let mut liberadas: usize = 0;
    for_each_user_page(cr3, v0, end_aligned, &mut |v, paddr, _flags| {
        if is_dynamic_addr(paddr) {
            liberar_marco_usuario(paddr);
            let _ = unmap_page(cr3, v);
            liberadas += 1;
            if liberadas % 256 == 0 { respirar(); }
        }
    });
}

pub fn free_address_space(cr3: u64) {
    let cr3_p = cr3 as usize;
    if !is_dynamic_addr(cr3_p) { return; }
    crate::mem::vmas::olvidar(cr3); // [T-014b]
    // [FIX-ALIAS] Las tablas heredadas del maestro NO se liberan: siguen siendo
    // las del núcleo (y las comparten todos los procesos). Sólo se devuelven al
    // asignador las tablas privadas de este espacio de direcciones.
    let master_root = master_pml4();

    for i in 0..PAGE_ENTRIES {
        let entry = unsafe { read_entry(cr3_p, i) };
        if entry & PRESENT == 0 { continue; }
        let pdpt_p = entry_paddr(entry);
        if !is_dynamic_addr(pdpt_p) { continue; }
        let m1 = master_subtable(Some(master_root), i);
        for j in 0..PAGE_ENTRIES {
            let pdpte = unsafe { read_entry(pdpt_p, j) };
            if pdpte & PRESENT == 0 { continue; }
            if pdpte & HUGE_PAGE != 0 { continue; }
            let pd_p = entry_paddr(pdpte);
            if !is_dynamic_addr(pd_p) { continue; }
            let m2 = master_subtable(m1, j);
            for k in 0..PAGE_ENTRIES {
                let pde = unsafe { read_entry(pd_p, k) };
                if pde & PRESENT == 0 { continue; }
                if pde & HUGE_PAGE != 0 { continue; }
                let pt_p = entry_paddr(pde);
                if is_dynamic_addr(pt_p) && master_subtable(m2, k) != Some(pt_p) {
                    // [FRENTE C] Antes de soltar la tabla, este espacio de
                    // direcciones devuelve su referencia a cada marco de imagen
                    // que mapeaba; el marco se libera sólo si era el último
                    // dueño. Los marcos no rastreados (pila de usuario, montículo
                    // ya liberado por `free_user_heap_pages`) no se tocan.
                    for e in 0..PAGE_ENTRIES {
                        let pte = unsafe { read_entry(pt_p, e) };
                        // [T-035] También las protegidas con marco devuelven su
                        // referencia al morir el espacio de direcciones.
                        if (pte & PRESENT == 0 || pte & USER == 0) && !es_protegida(pte) { continue; }
                        let frame = entry_paddr(pte);
                        if let Some(0) = crate::mem::frames::release(frame) {
                            free_page(frame);
                        }
                    }
                    free_page(pt_p);
                }
            }
            if m2 != Some(pd_p) {
                free_page(pd_p);
            }
        }
        if master_subtable(Some(master_root), i) != Some(pdpt_p) {
            free_page(pdpt_p);
        }
    }
    free_page(cr3_p);
    flush_tlb();
}

pub fn dump_page_tables(cr3: u64) {
    let cr3_p = cr3 as usize;
    serial::write_str("=== Page Table Dump ===\n");
    serial::write_str("CR3=");
    serial::write_hex(cr3_p);
    serial::write_str("\n");

    for i in 0..PAGE_ENTRIES {
        let pml4e = unsafe { read_entry(cr3_p, i) };
        if pml4e & PRESENT == 0 { continue; }
        let pdpt_p = entry_paddr(pml4e);
        serial::write_str("  PML4[");
        serial::write_usize(i);
        serial::write_str("] -> ");
        serial::write_hex(pdpt_p);
        serial::write_str("\n");

        for j in 0..PAGE_ENTRIES {
            let pdpte = unsafe { read_entry(pdpt_p, j) };
            if pdpte & PRESENT == 0 { continue; }
            if pdpte & HUGE_PAGE != 0 {
                serial::write_str("    PDPT[");
                serial::write_usize(j);
                serial::write_str("] 1G page paddr=");
                serial::write_hex(entry_paddr(pdpte));
                serial::write_str("\n");
                continue;
            }
            let pd_p = entry_paddr(pdpte);
            serial::write_str("    PDPT[");
            serial::write_usize(j);
            serial::write_str("] -> ");
            serial::write_hex(pd_p);
            serial::write_str("\n");

            for k in 0..PAGE_ENTRIES {
                let pde = unsafe { read_entry(pd_p, k) };
                if pde & PRESENT == 0 { continue; }
                if pde & HUGE_PAGE != 0 {
                    serial::write_str("      PD[");
                    serial::write_usize(k);
                    serial::write_str("] 2M page paddr=");
                    serial::write_hex(entry_paddr(pde));
                    serial::write_str(" flags=");
                    serial::write_hex(pde as usize);
                    serial::write_str("\n");
                    continue;
                }
                serial::write_str("      PD[");
                serial::write_usize(k);
                serial::write_str("] -> PT\n");
            }
        }
    }
    serial::write_str("=== End Dump ===\n");
}

pub fn probe_readable(vaddr: usize, count: usize) -> bool {
    is_user_range_safe(vaddr, count)
}

pub fn is_user_range_safe(vaddr: usize, count: usize) -> bool {
    if count == 0 { return true; }
    let end = match vaddr.checked_add(count) {
        Some(e) => e,
        None => return false,
    };
    if vaddr < 0x1000 || end > crate::process::USER_STACK_TOP {
        return false;
    }

    let cr3 = read_cr3();
    let start_page = vaddr & !(PAGE_SIZE - 1);
    let end_page = (end - 1) & !(PAGE_SIZE - 1);

    let mut page = start_page;
    loop {
        match translate(cr3, page) {
            Some((_, flags)) => {
                if flags & PRESENT == 0 || flags & USER == 0 {
                    return false;
                }
            }
            // [b191] Reserva perezosa: el núcleo la materializa en nombre del usuario
            None => if !materializar_perezosa(cr3, page, false) { return false; },
        }
        if page == end_page {
            break;
        }
        page += PAGE_SIZE;
    }
    true
}

pub fn is_user_range_writable(vaddr: usize, count: usize) -> bool {
    if count == 0 { return true; }
    let end = match vaddr.checked_add(count) {
        Some(e) => e,
        None => return false,
    };
    if vaddr < 0x1000 || end > crate::process::USER_STACK_TOP {
        return false;
    }

    let cr3 = read_cr3();
    let start_page = vaddr & !(PAGE_SIZE - 1);
    let end_page = (end - 1) & !(PAGE_SIZE - 1);

    let mut page = start_page;
    loop {
        match translate(cr3, page) {
            Some((_, flags)) => {
                if flags & PRESENT == 0 || flags & USER == 0 || flags & WRITABLE == 0 {
                    return false;
                }
            }
            None => if !materializar_perezosa(cr3, page, true) { return false; },
        }
        if page == end_page {
            break;
        }
        page += PAGE_SIZE;
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
//  Acceso a memoria de OTRO espacio de direcciones (depurador de Ring 3)
// ─────────────────────────────────────────────────────────────────────────────
//
// El kernel accede al marco físico a través de su alias identidad, de modo que
// puede leer y escribir la memoria de un proceso ajeno sin cambiar CR3 y sin
// respetar el bit de sólo lectura del PTE de usuario. Es exactamente la
// semántica que necesita un depurador (equivalente a `FOLL_FORCE` en Linux) y
// permite parchear la página de código de un proceso ajeno para instalar un
// punto de parada `INT3`.

fn remote_range_ok(vaddr: usize, count: usize) -> bool {
    match vaddr.checked_add(count) {
        Some(end) => vaddr >= 0x1000 && end <= crate::process::USER_STACK_TOP,
        None => false,
    }
}

/// Lee `out.len()` bytes del espacio de direcciones `cr3` a partir de `vaddr`.
pub fn read_remote(cr3: u64, vaddr: usize, out: &mut [u8]) -> bool {
    let n = out.len();
    if !remote_range_ok(vaddr, n) { return false; }
    let mut done = 0usize;
    while done < n {
        let va = vaddr + done;
        let (paddr, flags) = match translate(cr3, va) {
            Some(v) => v,
            None => return false,
        };
        if flags & PRESENT == 0 || flags & USER == 0 { return false; }
        let chunk = (PAGE_SIZE - (va & (PAGE_SIZE - 1))).min(n - done);
        unsafe {
            core::ptr::copy_nonoverlapping(phys_to_virt(paddr) as *const u8, out.as_mut_ptr().add(done), chunk);
        }
        done += chunk;
    }
    true
}

/// Escribe `data` en el espacio de direcciones `cr3` a partir de `vaddr`.
pub fn write_remote(cr3: u64, vaddr: usize, data: &[u8]) -> bool {
    let n = data.len();
    if !remote_range_ok(vaddr, n) { return false; }
    let mut done = 0usize;
    while done < n {
        let va = vaddr + done;
        let (paddr, flags) = match translate(cr3, va) {
            Some(v) => v,
            None => return false,
        };
        if flags & PRESENT == 0 || flags & USER == 0 { return false; }
        let chunk = (PAGE_SIZE - (va & (PAGE_SIZE - 1))).min(n - done);
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr().add(done), phys_to_virt(paddr) as *mut u8, chunk);
        }
        done += chunk;
    }
    true
}

pub fn copy_from_user(dst: &mut [u8], src: usize, count: usize) -> Result<usize, ()> {
    if count == 0 { return Ok(0); }
    if dst.len() < count { return Err(()); }
    if !is_user_range_safe(src, count) {
        return Err(());
    }
    unsafe {
        core::ptr::copy_nonoverlapping(src as *const u8, dst.as_mut_ptr(), count);
    }
    Ok(count)
}

/// [CICLO 49 / T1] Copia una cadena C de usuario hasta el NUL o hasta llenar
/// `dst`, validando PÁGINA A PÁGINA. Un binario Linux pasa rutas que viven
/// justo bajo el tope de la pila o al final de su .rodata: exigir que los 256
/// bytes siguientes estén mapeados (como hacía `copy_from_user(.., 256)`)
/// rechazaba cadenas perfectamente válidas (`open("/etc/os-release")` -> EPERM).
/// Devuelve el índice del NUL (o los bytes copiados si no apareció).
pub fn copy_cstr_from_user(dst: &mut [u8], src: usize) -> Result<usize, ()> {
    let mut i = 0usize;
    while i < dst.len() {
        let va = match src.checked_add(i) { Some(v) => v, None => return Err(()) };
        let fin_pagina = (va & !(PAGE_SIZE - 1)) + PAGE_SIZE;
        let n = (fin_pagina - va).min(dst.len() - i);
        if !is_user_range_safe(va, n) {
            return if i == 0 { Err(()) } else { Ok(i) };
        }
        for k in 0..n {
            let b = unsafe { core::ptr::read_volatile((va + k) as *const u8) };
            dst[i + k] = b;
            if b == 0 { return Ok(i + k); }
        }
        i += n;
    }
    Ok(i)
}

pub fn copy_to_user(dst: usize, src: &[u8]) -> Result<usize, ()> {
    let count = src.len();
    if count == 0 { return Ok(0); }
    if !is_user_range_writable(dst, count) {
        return Err(());
    }
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, count);
    }
    Ok(count)
}
