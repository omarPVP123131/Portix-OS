// kernel/src/version.rs — Constantes unificadas de versión de Portix OS
#![allow(dead_code)]

// Numeración unificada del producto: Portix OS v1.0.0.
// El núcleo y el producto comparten versión.
pub const PORTIX_VERSION_MAJOR: u32 = 1;
pub const PORTIX_VERSION_MINOR: u32 = 0;
pub const PORTIX_VERSION_PATCH: u32 = 0;

pub const PORTIX_VERSION: &str = "1.0.0";
pub const PORTIX_CODENAME: &str = "";
pub const PORTIX_RELEASE: &str = "Portix OS 1.0.0 (x86_64 Rust Bare-Metal)";
pub const PORTIX_BANNER: &str = "PORTIX OS v1.0.0 (64-bit Freestanding Rust Kernel)";

/// Versión histórica de desarrollo del núcleo, previa a la consagración v6.0.
/// Se conserva para trazabilidad de la bitácora de fases.
pub const PORTIX_KERNEL_DEV_BASELINE: &str = "0.9.5 Genesis";
