// kernel/src/net/firewall.rs — Motor de Filtrado de Paquetes y Cortafuegos en 100% Rust no_std
#![allow(dead_code)]

use alloc::vec::Vec;
use crate::arch::Spinlock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FwAction {
    Accept,
    Drop,
    Reject,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FwChain {
    Input,
    Output,
    Forward,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FwProtocol {
    Any,
    Icmp,
    Tcp,
    Udp,
    Raw,
}

#[derive(Clone, Debug)]
pub struct FwRule {
    pub chain: FwChain,
    pub action: FwAction,
    pub protocol: FwProtocol,
    pub src_ip: [u8; 4],
    pub src_mask: [u8; 4],
    pub dst_port: u16,      // 0 = any
    pub packets_matched: u64,
    pub bytes_matched: u64,
}

pub struct FirewallState {
    pub enabled: bool,
    pub default_input_policy: FwAction,
    pub default_output_policy: FwAction,
    pub default_forward_policy: FwAction,
    pub rules: Vec<FwRule>,
    pub syn_rate_limit_pps: u32,
    pub last_syn_timestamp: u64,
    pub syn_counter_current_sec: u32,
}

impl FirewallState {
    pub const fn new() -> Self {
        FirewallState {
            enabled: true,
            default_input_policy: FwAction::Accept,
            default_output_policy: FwAction::Accept,
            default_forward_policy: FwAction::Drop,
            rules: Vec::new(),
            syn_rate_limit_pps: 1000,
            last_syn_timestamp: 0,
            syn_counter_current_sec: 0,
        }
    }
}

pub static FIREWALL: Spinlock<FirewallState> = Spinlock::new(FirewallState::new());

pub fn add_rule(chain: FwChain, action: FwAction, protocol: FwProtocol, src_ip: [u8; 4], src_mask: [u8; 4], dst_port: u16) {
    let mut fw = FIREWALL.lock();
    fw.rules.push(FwRule {
        chain, action, protocol,
        src_ip, src_mask, dst_port,
        packets_matched: 0,
        bytes_matched: 0,
    });
}

pub fn filter_packet(chain: FwChain, protocol: FwProtocol, src_ip: [u8; 4], dst_port: u16, len: usize) -> FwAction {
    let mut fw = FIREWALL.lock();
    if !fw.enabled { return FwAction::Accept; }

    for rule in fw.rules.iter_mut() {
        if rule.chain != chain { continue; }
        if rule.protocol != FwProtocol::Any && rule.protocol != protocol { continue; }
        if rule.dst_port != 0 && rule.dst_port != dst_port { continue; }

        let mut ip_matches = true;
        for i in 0..4 {
            if (src_ip[i] & rule.src_mask[i]) != (rule.src_ip[i] & rule.src_mask[i]) {
                ip_matches = false;
                break;
            }
        }
        if !ip_matches { continue; }

        rule.packets_matched += 1;
        rule.bytes_matched += len as u64;
        return rule.action;
    }

    match chain {
        FwChain::Input => fw.default_input_policy,
        FwChain::Output => fw.default_output_policy,
        FwChain::Forward => fw.default_forward_policy,
    }
}
