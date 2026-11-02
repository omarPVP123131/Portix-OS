// kernel/src/system/diagnostics.rs — Ethical Local Telemetry and System Diagnostics
// 100% opt-in, zero forced telemetry, completely auditable locally.

#![allow(dead_code)]

use crate::arch::Spinlock;

pub struct EthicalTelemetry {
    pub opt_in_enabled: bool,
    pub anonymous_session_id: u64,
    pub kernel_uptime_seconds: u32,
    pub event_log_count: usize,
}

impl EthicalTelemetry {
    pub const fn new() -> Self {
        EthicalTelemetry {
            opt_in_enabled: false, // Privacidad estricta por defecto
            anonymous_session_id: 0x5052_5458_3130_3030, // "PRTX1000"
            kernel_uptime_seconds: 42,
            event_log_count: 0,
        }
    }

    pub fn set_opt_in(&mut self, enable: bool) {
        self.opt_in_enabled = enable;
    }

    pub fn record_health_metric(&mut self, metric_name: &str, value: u32) -> bool {
        if !self.opt_in_enabled {
            return false;
        }
        self.event_log_count += 1;
        crate::drivers::serial::write_str("[TELEMETRY-ETHICAL] SESS=0x");
        crate::drivers::serial::write_str(metric_name);
        crate::drivers::serial::write_str(" val=");
        crate::drivers::serial::write_usize(value as usize);
        crate::drivers::serial::write_str("\n");
        true
    }
}

pub static GLOBAL_TELEMETRY: Spinlock<EthicalTelemetry> = Spinlock::new(EthicalTelemetry::new());
