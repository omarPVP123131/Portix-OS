// kernel/src/containers/mod.rs — Containers, Sandboxes, and Desktop Portals

pub mod oci_runtime;
pub mod xdg_portal;

pub use oci_runtime::*;
pub use xdg_portal::*;
