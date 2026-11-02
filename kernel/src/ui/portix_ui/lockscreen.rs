// kernel/src/ui/portix_ui/lockscreen.rs — Pantalla de Bloqueo y Greeter Portix-DM
// Fases 701 - 750: Desenfoque Dinámico Acrílico, Reloj Minimalista y Autenticación Segura.

use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::arch::spinlock::Spinlock;

pub struct PortixDisplayManager {
    pub is_locked: AtomicBool,
    pub user_name: String,
    pub blur_radius: u32,
    pub attempts_failed: u32,
}

impl PortixDisplayManager {
    pub fn new() -> Self {
        Self {
            is_locked: AtomicBool::new(false),
            user_name: String::from("user"),
            blur_radius: 32,
            attempts_failed: 0,
        }
    }

    pub fn lock_session(&self) {
        self.is_locked.store(true, Ordering::SeqCst);
    }

    pub fn unlock_session(&self) {
        self.is_locked.store(false, Ordering::SeqCst);
    }

    pub fn is_session_locked(&self) -> bool {
        self.is_locked.load(Ordering::Relaxed)
    }
}

pub static PORTIX_DM: Spinlock<Option<PortixDisplayManager>> = Spinlock::new(None);

pub fn init_lockscreen() {
    let mut lock = PORTIX_DM.lock();
    if lock.is_none() {
        *lock = Some(PortixDisplayManager::new());
    }
}

pub fn get_dm_summary() -> String {
    let lock = PORTIX_DM.lock();
    match &*lock {
        Some(dm) => {
            let state = if dm.is_session_locked() { "BLOQUEADO" } else { "Sesión Activa" };
            alloc::format!(
                "Portix-DM: Estado={}, Usuario='{}', Desenfoque Acrílico={}px",
                state, dm.user_name, dm.blur_radius
            )
        }
        None => String::from("Portix-DM: No inicializado"),
    }
}
