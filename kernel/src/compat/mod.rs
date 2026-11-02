// kernel/src/compat/mod.rs — Binary Compatibility and Localization Subsystems

pub mod i18n;
pub mod linux;

pub use i18n::*;
pub use linux::*;
