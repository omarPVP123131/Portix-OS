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

    /// Get mutable access to the inner data (requires &mut self - no lock needed)
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
