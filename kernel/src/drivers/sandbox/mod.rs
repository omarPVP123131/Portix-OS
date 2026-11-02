// kernel/src/drivers/sandbox/mod.rs — Subsistema de Aislamiento de Drivers Linux en Ring 3
pub mod ring3_driver_sandbox;

pub use ring3_driver_sandbox::{
    LinuxDriverSandboxManager,
    ring3_driver_sandbox_init,
    DRIVER_SANDBOX_MANAGER,
};
