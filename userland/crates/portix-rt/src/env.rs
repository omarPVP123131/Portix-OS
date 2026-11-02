//! [CICLO 47-b] Entorno de proceso real (`envp` recibido en `_start`).
//!
//! `main` recibe `envp` como tercer argumento; los programas que quieran
//! consultarlo con `getenv` deben registrarlo con `set_environ(envp)` al
//! comenzar (lo hace `portix_rt::init_env`).
use core::sync::atomic::{AtomicPtr, Ordering};

static ENVIRON: AtomicPtr<*const u8> = AtomicPtr::new(core::ptr::null_mut());

/// Registra el puntero `envp` recibido por `main`.
pub fn set_environ(envp: *const *const u8) {
    ENVIRON.store(envp as *mut *const u8, Ordering::SeqCst);
}

fn cstr<'a>(p: *const u8) -> &'a str {
    if p.is_null() { return ""; }
    let mut n = 0usize;
    unsafe { while *p.add(n) != 0 && n < 4096 { n += 1; } }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, n) }).unwrap_or("")
}

/// Itera las entradas `CLAVE=VALOR` del entorno.
pub fn environ() -> impl Iterator<Item = &'static str> {
    let base = ENVIRON.load(Ordering::SeqCst) as *const *const u8;
    let mut i = 0usize;
    core::iter::from_fn(move || {
        if base.is_null() { return None; }
        let p = unsafe { *base.add(i) };
        if p.is_null() || i >= 512 { return None; }
        i += 1;
        Some(cstr(p))
    })
}

/// Devuelve el valor de la variable `name`, si existe.
pub fn getenv(name: &str) -> Option<&'static str> {
    for e in environ() {
        if let Some(v) = e.strip_prefix(name) {
            if let Some(v) = v.strip_prefix('=') { return Some(v); }
        }
    }
    None
}
