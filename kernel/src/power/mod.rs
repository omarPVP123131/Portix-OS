// kernel/src/power/mod.rs — Advanced Power Management and Energy Governors

pub mod governor;
pub mod s0ix;

pub use governor::*;
pub use s0ix::*;
