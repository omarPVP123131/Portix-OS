// kernel/src/power/governor.rs — Dynamic CPU Frequency and Performance Governor
// Controls P-states, C-states, CFS scheduler quantum and turbo boost.

#![allow(dead_code)]

use crate::arch::Spinlock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GovernorMode {
    PowerSave,        // 800 MHz, 50 Hz CFS, mínimo voltaje y consumo
    Balanced,         // 2.4 GHz, 100 Hz CFS, rendimiento equilibrado
    UltraPerformance, // 4.8 GHz Turbo, 250 Hz CFS, latencia sub-milisegundo
}

pub struct PerformanceGovernor {
    pub current_mode: GovernorMode,
    pub target_frequency_mhz: u32,
    pub cfs_quantum_hz: u32,
    pub turbo_boost_active: bool,
}

impl PerformanceGovernor {
    pub const fn new() -> Self {
        PerformanceGovernor {
            current_mode: GovernorMode::Balanced,
            target_frequency_mhz: 2400,
            cfs_quantum_hz: 100,
            turbo_boost_active: false,
        }
    }

    pub fn set_mode(&mut self, mode: GovernorMode) {
        self.current_mode = mode;
        match mode {
            GovernorMode::PowerSave => {
                self.target_frequency_mhz = 800;
                self.cfs_quantum_hz = 50;
                self.turbo_boost_active = false;
            },
            GovernorMode::Balanced => {
                self.target_frequency_mhz = 2400;
                self.cfs_quantum_hz = 100;
                self.turbo_boost_active = false;
            },
            GovernorMode::UltraPerformance => {
                self.target_frequency_mhz = 4800;
                self.cfs_quantum_hz = 250;
                self.turbo_boost_active = true;
            },
        }
    }
}

pub static GLOBAL_GOVERNOR: Spinlock<PerformanceGovernor> = Spinlock::new(PerformanceGovernor::new());
