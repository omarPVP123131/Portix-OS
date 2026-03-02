// kernel/src/mem/allocator.rs
//
// Buddy System Allocator — O(log N) alloc/free, sin fragmentación externa.
//
// Cambios respecto a v1:
//   • AllocStats: contadores atómicos visibles desde la UI (sin lock).
//   • Serial debug en init() — imprime mapa de bloques iniciales.
//   • Trazas alloc/free bajo cfg(debug_assertions).

use super::{HEAP_SIZE, HEAP_START, MAX_ORDER, MIN_ORDER, ORDER_COUNT};
use crate::drivers::serial::{self, Level};
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// ── Estadísticas públicas (accesibles desde la UI sin bloqueo) ────────────────

pub struct AllocStats {
    pub total_allocs:  AtomicUsize,
    pub total_frees:   AtomicUsize,
    pub failed_allocs: AtomicUsize,
    pub free_blocks:   [AtomicUsize; ORDER_COUNT],
}

// Macro auxiliar para inicializar el array en const context
macro_rules! atomic_array {
    ($n:expr) => {{
        // Rust permite esto en nightly con feature(inline_const) pero en stable
        // la única forma es listar literalmente. Como ORDER_COUNT = 19, lo listamos.
        [
            AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
            AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
            AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
            AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
            AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
            AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
            AtomicUsize::new(0), // 19 elementos = ORDER_COUNT
        ]
    }};
}

impl AllocStats {
    const fn new() -> Self {
        Self {
            total_allocs:  AtomicUsize::new(0),
            total_frees:   AtomicUsize::new(0),
            failed_allocs: AtomicUsize::new(0),
            free_blocks:   atomic_array!(ORDER_COUNT),
        }
    }
}

unsafe impl Sync for AllocStats {}

pub static ALLOC_STATS: AllocStats = AllocStats::new();

// ── Tipos internos ────────────────────────────────────────────────────────────

#[repr(C)]
struct FreeNode {
    next: *mut FreeNode,
    prev: *mut FreeNode,
}

impl FreeNode {
    #[inline]
    unsafe fn init(ptr: *mut u8) -> *mut FreeNode {
        let node = ptr as *mut FreeNode;
        (*node).next = ptr::null_mut();
        (*node).prev = ptr::null_mut();
        node
    }
}

// ── Estado del allocator ──────────────────────────────────────────────────────

struct BuddyInner {
    free_lists: [*mut FreeNode; ORDER_COUNT],
}

unsafe impl Send for BuddyInner {}
unsafe impl Sync for BuddyInner {}

impl BuddyInner {
    const fn new() -> Self {
        Self {
            free_lists: [ptr::null_mut(); ORDER_COUNT],
        }
    }
}

pub struct BuddyAllocator {
    inner:   core::cell::UnsafeCell<BuddyInner>,
    inited:  AtomicBool,
    locked:  AtomicBool,
}

unsafe impl Send for BuddyAllocator {}
unsafe impl Sync for BuddyAllocator {}

impl BuddyAllocator {
    pub const fn new() -> Self {
        Self {
            inner:  core::cell::UnsafeCell::new(BuddyInner::new()),
            inited: AtomicBool::new(false),
            locked: AtomicBool::new(false),
        }
    }

    /// Inicializa el heap y emite el mapa de bloques por serial.
    ///
    /// # Safety
    /// Llamar exactamente una vez, antes de cualquier alloc/free.
    pub unsafe fn init(&self) {
        if self.inited.swap(true, Ordering::AcqRel) {
            return;
        }

        serial::log_level(Level::Info, "HEAP", "Inicializando buddy allocator...");
        serial::write_str("[  INF ] HEAP  rango: ");
        serial::write_hex(HEAP_START);
        serial::write_str(" - ");
        serial::write_hex(HEAP_START + HEAP_SIZE);
        serial::write_byte(b'\n');

        let inner = &mut *self.inner.get();
        let mut addr  = HEAP_START;
        let end       = HEAP_START + HEAP_SIZE;
        let mut count = 0usize;

        while addr < end {
            let remaining = end - addr;
            let mut ord = MAX_ORDER;
            loop {
                let sz = 1usize << ord;
                if sz <= remaining && (addr & (sz - 1)) == 0 {
                    break;
                }
                if ord == MIN_ORDER { break; }
                ord -= 1;
            }
            let sz = 1usize << ord;

            // Log de cada bloque inicial
            serial::write_str("[  INF ] HEAP  bloque ord=");
            serial::write_u32(ord as u32);
            serial::write_str(" sz=");
            serial::write_usize(sz >> 10); // KiB
            serial::write_str("K @ ");
            serial::write_hex(addr);
            serial::write_byte(b'\n');

            inner_push(inner, ord, addr as *mut u8);
            ALLOC_STATS.free_blocks[ord_idx(ord)].fetch_add(1, Ordering::Relaxed);
            addr  += sz;
            count += 1;
        }

        serial::write_str("[   OK ] HEAP  listo — ");
        serial::write_usize(count);
        serial::write_str(" bloques libres, ");
        serial::write_usize(HEAP_SIZE >> 10);
        serial::write_str(" KiB totales\n");
    }

    #[inline(always)]
    fn lock(&self) {
        while self.locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

// ── Operaciones sobre listas libres ──────────────────────────────────────────

#[inline(always)]
fn ord_idx(order: usize) -> usize {
    order - MIN_ORDER
}

#[inline]
unsafe fn inner_push(inner: &mut BuddyInner, order: usize, ptr: *mut u8) {
    let idx  = ord_idx(order);
    let node = FreeNode::init(ptr);
    let head = inner.free_lists[idx];
    (*node).next = head;
    (*node).prev = ptr::null_mut();
    if !head.is_null() {
        (*head).prev = node;
    }
    inner.free_lists[idx] = node;
}

#[inline]
unsafe fn inner_pop(inner: &mut BuddyInner, order: usize) -> Option<*mut u8> {
    let idx  = ord_idx(order);
    let node = inner.free_lists[idx];
    if node.is_null() {
        return None;
    }
    let next = (*node).next;
    inner.free_lists[idx] = next;
    if !next.is_null() {
        (*next).prev = ptr::null_mut();
    }
    Some(node as *mut u8)
}

#[inline]
unsafe fn inner_remove(inner: &mut BuddyInner, order: usize, ptr: *mut u8) {
    let idx  = ord_idx(order);
    let node = ptr as *mut FreeNode;
    let prev = (*node).prev;
    let next = (*node).next;
    if prev.is_null() {
        inner.free_lists[idx] = next;
    } else {
        (*prev).next = next;
    }
    if !next.is_null() {
        (*next).prev = prev;
    }
}

#[inline]
unsafe fn find_buddy(inner: &mut BuddyInner, order: usize, buddy_addr: usize) -> bool {
    let buddy = buddy_addr as *mut FreeNode;
    let idx   = ord_idx(order);
    let mut cur = inner.free_lists[idx];
    while !cur.is_null() {
        if cur == buddy { return true; }
        cur = (*cur).next;
    }
    false
}

#[inline(always)]
fn buddy_of(addr: usize, order: usize) -> usize {
    let size   = 1usize << order;
    let offset = addr - HEAP_START;
    HEAP_START + (offset ^ size)
}

// ── GlobalAlloc ───────────────────────────────────────────────────────────────

unsafe impl GlobalAlloc for BuddyAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !self.inited.load(Ordering::Acquire) {
            return ptr::null_mut();
        }
        let need  = layout.size().max(layout.align()).max(1 << MIN_ORDER);
        let order = order_for(need);
        if order > MAX_ORDER {
            ALLOC_STATS.failed_allocs.fetch_add(1, Ordering::Relaxed);
            return ptr::null_mut();
        }

        self.lock();
        let result = buddy_alloc(&mut *self.inner.get(), order);
        self.unlock();

        if result.is_null() {
            ALLOC_STATS.failed_allocs.fetch_add(1, Ordering::Relaxed);
            #[cfg(debug_assertions)]
            serial::log_level(Level::Warn, "HEAP", "OOM — alloc fallida");
        } else {
            ALLOC_STATS.total_allocs.fetch_add(1, Ordering::Relaxed);
            ALLOC_STATS.free_blocks[ord_idx(order)].fetch_sub(1, Ordering::Relaxed);
            #[cfg(debug_assertions)]
            {
                serial::write_str("[ DBG ] HEAP  alloc ord=");
                serial::write_u32(order as u32);
                serial::write_str(" @ ");
                serial::write_hex(result as usize);
                serial::write_byte(b'\n');
            }
        }
        result
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let need  = layout.size().max(layout.align()).max(1 << MIN_ORDER);
        let order = order_for(need);

        self.lock();
        buddy_free(&mut *self.inner.get(), ptr, order);
        self.unlock();

        ALLOC_STATS.total_frees.fetch_add(1, Ordering::Relaxed);
        ALLOC_STATS.free_blocks[ord_idx(order)].fetch_add(1, Ordering::Relaxed);

        #[cfg(debug_assertions)]
        {
            serial::write_str("[ DBG ] HEAP  free  ord=");
            serial::write_u32(order as u32);
            serial::write_str(" @ ");
            serial::write_hex(ptr as usize);
            serial::write_byte(b'\n');
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = self.alloc(layout);
        if !ptr.is_null() {
            ptr::write_bytes(ptr, 0, layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, old_ptr: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = match Layout::from_size_align(new_size, old_layout.align()) {
            Ok(l)  => l,
            Err(_) => return ptr::null_mut(),
        };
        let new_ptr = self.alloc(new_layout);
        if !new_ptr.is_null() {
            let copy_size = old_layout.size().min(new_size);
            ptr::copy_nonoverlapping(old_ptr, new_ptr, copy_size);
            self.dealloc(old_ptr, old_layout);
        }
        new_ptr
    }
}

// ── Lógica core alloc/free ────────────────────────────────────────────────────

#[inline]
fn order_for(size: usize) -> usize {
    let mut ord = MIN_ORDER;
    let mut blk = 1usize << MIN_ORDER;
    while blk < size && ord < MAX_ORDER {
        ord += 1;
        blk <<= 1;
    }
    ord
}

unsafe fn buddy_alloc(inner: &mut BuddyInner, order: usize) -> *mut u8 {
    let mut found_ord = MAX_ORDER + 1;
    for o in order..=MAX_ORDER {
        if !inner.free_lists[ord_idx(o)].is_null() {
            found_ord = o;
            break;
        }
    }
    if found_ord > MAX_ORDER {
        return ptr::null_mut();
    }

    let ptr = inner_pop(inner, found_ord).unwrap();
    let mut cur_ord = found_ord;

    while cur_ord > order {
        cur_ord -= 1;
        let buddy_ptr = (ptr as usize + (1 << cur_ord)) as *mut u8;
        inner_push(inner, cur_ord, buddy_ptr);
        // Actualizar contador de bloques libres por split
        ALLOC_STATS.free_blocks[ord_idx(cur_ord)].fetch_add(1, Ordering::Relaxed);

        #[cfg(target_arch = "x86_64")]
        core::arch::x86_64::_mm_prefetch(
            buddy_ptr as *const i8,
            core::arch::x86_64::_MM_HINT_T0,
        );
    }
    ptr
}

unsafe fn buddy_free(inner: &mut BuddyInner, ptr: *mut u8, order: usize) {
    let mut addr = ptr as usize;
    let mut ord  = order;
    let mut merges = 0u32;

    loop {
        if ord >= MAX_ORDER { break; }
        let buddy_addr = buddy_of(addr, ord);
        if buddy_addr < HEAP_START || buddy_addr >= HEAP_START + HEAP_SIZE { break; }
        if !find_buddy(inner, ord, buddy_addr) { break; }

        inner_remove(inner, ord, buddy_addr as *mut u8);
        // El bloque del buddy desaparece de su orden
        ALLOC_STATS.free_blocks[ord_idx(ord)].fetch_sub(1, Ordering::Relaxed);

        addr  = addr.min(buddy_addr);
        ord  += 1;
        merges += 1;
    }

    inner_push(inner, ord, addr as *mut u8);

    #[cfg(debug_assertions)]
    if merges > 0 {
        serial::write_str("[ DBG ] HEAP  merge x");
        serial::write_u32(merges);
        serial::write_str(" → ord=");
        serial::write_u32(ord as u32);
        serial::write_byte(b'\n');
    }
}