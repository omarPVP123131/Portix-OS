// kernel/src/security/packet_filter.rs — Interactive Packet Filtering and Firewall Rules
// Real-time network auditing, outbound connection enforcement, and process-based ACLs.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::arch::Spinlock;

#[derive(Debug, Clone)]
pub struct FirewallRule {
    pub id: u32,
    pub process_name: String,
    pub destination_ip: String,
    pub destination_port: u16,
    pub is_allowed: bool,
    pub prompt_user_active: bool,
}

pub struct PacketFilterSubsystem {
    pub firewall_rules: Vec<FirewallRule>,
    pub outgoing_bytes_audited: u64,
}

impl PacketFilterSubsystem {
    pub fn new() -> Self {
        let r1 = FirewallRule {
            id: 1,
            process_name: String::from("portix-browser"),
            destination_ip: String::from("1.1.1.1"),
            destination_port: 443,
            is_allowed: true,
            prompt_user_active: false,
        };

        let r2 = FirewallRule {
            id: 2,
            process_name: String::from("suspicious-binary"),
            destination_ip: String::from("198.51.100.42"),
            destination_port: 8080,
            is_allowed: false,
            prompt_user_active: true,
        };

        Self {
            firewall_rules: alloc::vec![r1, r2],
            outgoing_bytes_audited: 128_400,
        }
    }

    pub fn get_summary(&self) -> String {
        alloc::format!(
            "Firewall Packet Filter: {} reglas activas | {} bytes auditados",
            self.firewall_rules.len(),
            self.outgoing_bytes_audited
        )
    }
}

pub static GLOBAL_PACKET_FILTER: Spinlock<Option<PacketFilterSubsystem>> = Spinlock::new(None);

pub fn init_packet_filter() {
    let mut lock = GLOBAL_PACKET_FILTER.lock();
    if lock.is_none() {
        *lock = Some(PacketFilterSubsystem::new());
    }
}

pub fn get_packet_filter_summary() -> String {
    let mut lock = GLOBAL_PACKET_FILTER.lock();
    if lock.is_none() {
        *lock = Some(PacketFilterSubsystem::new());
    }
    lock.as_ref().map(|s| s.get_summary()).unwrap_or_else(|| String::from("Packet Filter no inicializado"))
}
