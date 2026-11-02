// kernel/src/system/mod.rs — Core System Management and Diagnostics

pub mod consagracion;
pub mod devsuite;
pub mod bench;
pub mod diagnostics;
pub mod migration;
pub mod sysinfo;
pub mod subsystem_suite;
pub mod symbols;
pub mod ab_update;
pub mod singularity;

pub use diagnostics::*;
pub use migration::*;
pub use sysinfo::*;
pub use subsystem_suite::*;
pub use ab_update::*;
pub use singularity::*;
