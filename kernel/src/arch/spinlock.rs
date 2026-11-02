/// Spinlock - A simple spin-based mutual exclusion lock for kernel code
/// 
/// This is a basic spinlock suitable for protecting small critical sections.
/// NOT suitable for locks held for long periods (use semaphores instead).
///
/// # Safety
/// - Uses atomic operations for thread-safety
/// - Prevents multiple CPU cores from accessing data simultaneously
/// - Deadlock-free under normal conditions
///
/// # Example
/// ```rust
/// static MY_LOCK: Spinlock<u32> = Spinlock::new(0);
/// 
/// // Protect data access
/// let mut guard = MY_LOCK.lock();
/// *guard = 42;
/// drop(guard);  // Lock released automatically
/// ```

use core::sync::atomic::{AtomicU8, Ordering};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};

/// Spinlock state
const UNLOCKED: u8 = 0;
const LOCKED: u8 = 1;

/// A mutual exclusion lock based on spinning
/// [T-026] Secciones `lock_irqsave` abiertas en TODO el sistema (contador
/// global y no por CPU: con IF=0 una CPU no cambia de contexto, así que el
/// desequilibrio de otra CPU sólo hace a `respirar()` más conservador, nunca
/// menos seguro).
pub static IRQSAVE_PROF: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
pub fn en_seccion_irqsave() -> bool { IRQSAVE_PROF.load(Ordering::SeqCst) != 0 }

pub struct Spinlock<T> {
    lock: AtomicU8,
    data: UnsafeCell<T>,
}

/// Guard that releases lock on drop
pub struct SpinlockGuard<'a, T> {
    lock: &'a AtomicU8,
    data: &'a UnsafeCell<T>,
}

impl<T> Spinlock<T> {
    /// Create a new spinlock with the given data
    pub const fn new(data: T) -> Self {
        Spinlock {
            lock: AtomicU8::new(UNLOCKED),
            data: UnsafeCell::new(data),
        }
    }

    /// Try to acquire the lock (non-blocking)
    pub fn try_lock(&self) -> Option<SpinlockGuard<'_, T>> {
        match self.lock.compare_exchange_weak(
            UNLOCKED,
            LOCKED,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => Some(SpinlockGuard { lock: &self.lock, data: &self.data }),
            Err(_) => None,
        }
    }

    /// Acquire the lock (blocking with spinwait)
    /// 
    /// # Warning
    /// This will spin forever if the lock cannot be acquired.
    /// Do NOT hold locks for extended periods or during I/O waits.
    /// WARNING: Does NOT disable IRQs. If an ISR acquires the same lock
    /// while the interrupted code holds it, a deadlock will occur.
    /// Use `lock_irqsave()` pattern from BuddyAllocator for IRQ contexts.
    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        // Spin until lock is acquired
        loop {
            // Try to acquire with weak compare-exchange for performance
            match self.lock.compare_exchange_weak(
                UNLOCKED,
                LOCKED,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return SpinlockGuard { 
                        lock: &self.lock, 
                        data: &self.data,
                    };
                }
                Err(_) => {
                    // Lock is held, spin and check again
                    // Add pause hint to reduce CPU contention
                    #[cfg(target_arch = "x86_64")]
                    core::arch::x86_64::_mm_pause();
                }
            }
        }
    }

    /// [T-025 E2] Toma el cerrojo con las interrupciones deshabilitadas en ESTE
    /// núcleo y las restaura al soltar el guard. Es la variante segura cuando el
    /// mismo cerrojo puede tomarse desde una ISR (en SMP, deshabilitar IF no
    /// protege frente a otro núcleo: por eso además se gira sobre el atómico).
    pub fn lock_irqsave(&self) -> SpinlockGuardIrq<'_, T> {
        let rflags: u64;
        unsafe { core::arch::asm!("pushfq; pop {}; cli", out(reg) rflags, options(nomem)); }
        // [T-026] Profundidad de secciones irqsave de esta CPU: `paging::respirar()`
        // no abre interrupciones mientras sea > 0 (el IF=0 es de un cerrojo, no de FMASK).
        IRQSAVE_PROF.fetch_add(1, Ordering::SeqCst);
        let guard = core::mem::ManuallyDrop::new(self.lock());
        SpinlockGuardIrq { guard, rflags }
    }

    /// Get mutable access to the inner data (requires &mut self - no lock needed)
    /// [T-005] Puntero crudo al dato SIN tomar el cerrojo. Sólo para lecturas
    /// de campos atómicamente coherentes (pid/estado) desde contextos que ya
    /// podrían tener el cerrojo tomado (reentrada); nunca para escribir.
    pub unsafe fn as_mut_ptr(&self) -> *mut T {
        self.data.get()
    }

    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }

    /// Consume the spinlock and extract the inner data
    pub fn into_inner(self) -> T {
        // SAFETY: We own the spinlock, so no one else can access the data
        self.data.into_inner()
    }
}

impl<'a, T> Deref for SpinlockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: The lock is held, so only we can access the data
        unsafe { &*self.data.get() }
    }
}

impl<'a, T> DerefMut for SpinlockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: The lock is held, so only we can access the data
        unsafe { &mut *self.data.get() }
    }
}

impl<'a, T> Drop for SpinlockGuard<'a, T> {
    fn drop(&mut self) {
        // Release the lock
        self.lock.store(UNLOCKED, Ordering::Release);
    }
}

/// Guard de `lock_irqsave`: al caer suelta el cerrojo y restaura IF.
pub struct SpinlockGuardIrq<'a, T> {
    guard: core::mem::ManuallyDrop<SpinlockGuard<'a, T>>,
    rflags: u64,
}

impl<'a, T> Deref for SpinlockGuardIrq<'a, T> {
    type Target = T;
    fn deref(&self) -> &T { &self.guard }
}

impl<'a, T> DerefMut for SpinlockGuardIrq<'a, T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.guard }
}

impl<'a, T> Drop for SpinlockGuardIrq<'a, T> {
    fn drop(&mut self) {
        // Primero se suelta el cerrojo (drop del guard interno ocurre despues de
        // este cuerpo), asi que restauramos IF solo si estaba activo, tras liberar.
        self.guard.lock.store(UNLOCKED, Ordering::Release);
        IRQSAVE_PROF.fetch_sub(1, Ordering::SeqCst);
        if self.rflags & (1 << 9) != 0 {
            unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
        }
    }
}

// SAFETY: Spinlock<T> is Send if T is Send
unsafe impl<T: Send> Send for Spinlock<T> {}

// SAFETY: Spinlock<T> is Sync if T is Send (because it provides exclusive access)
unsafe impl<T: Send> Sync for Spinlock<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinlock_basic() {
        let lock = Spinlock::new(42);
        {
            let guard = lock.lock();
            assert_eq!(*guard, 42);
        }
        // Lock should be released after guard is dropped
    }

    #[test]
    fn test_spinlock_mutation() {
        let lock = Spinlock::new(vec![1, 2, 3]);
        {
            let mut guard = lock.lock();
            guard.push(4);
        }
        let guard = lock.lock();
        assert_eq!(guard.len(), 4);
    }

    #[test]
    fn test_spinlock_try_lock() {
        let lock = Spinlock::new(0);
        {
            let _guard1 = lock.lock();
            // Second try should fail since lock is held
            assert!(lock.try_lock().is_none());
        }
        // Now it should succeed
        assert!(lock.try_lock().is_some());
    }
}
