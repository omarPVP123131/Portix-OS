// kernel/src/crypto/mod.rs — Pila Criptográfica Nativa de Portix OS
// Implementación en Rust puro no_std sin crates externas ni OpenSSL.
#![allow(dead_code)]

pub mod x25519;
pub mod chacha20poly1305;
pub mod sha512;
pub mod tls;

/// Ejecuta la suite de verificación criptográfica completa
pub fn run_crypto_self_test() -> bool {
    let ok_x25519 = x25519::run_x25519_self_test();
    let ok_chacha = chacha20poly1305::run_chacha20_self_test();
    let ok_sha = sha512::run_sha512_self_test();
    let ok_tls = tls::run_tls13_self_test();

    ok_x25519 && ok_chacha && ok_sha && ok_tls
}
