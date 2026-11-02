// kernel/src/net/dhcp.rs — DHCP Dynamic IP Configuration Client (RFC 2131)
#![allow(dead_code)]

use crate::drivers::net::e1000;
use crate::drivers::serial;
use super::udp::send_udp;
use super::{set_ip, set_netmask, set_gateway, set_dns};

const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER:    u8 = 2;
const DHCP_REQUEST:  u8 = 3;
const DHCP_ACK:      u8 = 5;

const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
const DHCP_XID: u32 = 0x3903F326;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DhcpState {
    Init,
    Discovering,
    Requesting,
    Bound,
}

static mut DHCP_STATE: DhcpState = DhcpState::Init;
static mut OFFERED_IP: [u8; 4] = [0, 0, 0, 0];
static mut SERVER_IP: [u8; 4] = [0, 0, 0, 0];

pub fn start() {
    unsafe {
        DHCP_STATE = DhcpState::Discovering;
    }
    serial::write_str("[DHCP] Broadcasting DHCP Discover...\n");
    let _ = send_dhcp_discover();
}

pub fn is_bound() -> bool {
    unsafe { DHCP_STATE == DhcpState::Bound }
}

fn send_dhcp_discover() -> Result<(), &'static str> {
    let mac = e1000::get_mac();
    let mut pkt = [0u8; 300];

    pkt[0] = 1; // Opcode: BootRequest
    pkt[1] = 1; // Hardware: Ethernet
    pkt[2] = 6; // HLEN: 6
    pkt[3] = 0; // Hops

    // Transaction ID
    pkt[4..8].copy_from_slice(&DHCP_XID.to_be_bytes());

    // Flags: Broadcast bit set (0x8000)
    pkt[10] = 0x80;
    pkt[11] = 0x00;

    // Client MAC address
    pkt[28..34].copy_from_slice(&mac);

    // Magic Cookie
    pkt[236..240].copy_from_slice(&MAGIC_COOKIE);

    // Option 53: DHCP Message Type = Discover (1)
    let mut off = 240;
    pkt[off] = 53; pkt[off + 1] = 1; pkt[off + 2] = DHCP_DISCOVER;
    off += 3;

    // Option 55: Parameter Request List (1: Subnet, 3: Router, 6: DNS)
    pkt[off] = 55; pkt[off + 1] = 3;
    pkt[off + 2] = 1; // Subnet Mask
    pkt[off + 3] = 3; // Router
    pkt[off + 4] = 6; // DNS
    off += 5;

    // Option 255: End
    pkt[off] = 255;
    off += 1;

    send_udp([0, 0, 0, 0], [255, 255, 255, 255], 68, 67, &pkt[..off])
}

fn send_dhcp_request(offered_ip: [u8; 4], server_ip: [u8; 4]) -> Result<(), &'static str> {
    let mac = e1000::get_mac();
    let mut pkt = [0u8; 300];

    pkt[0] = 1; // BootRequest
    pkt[1] = 1;
    pkt[2] = 6;
    pkt[3] = 0;

    pkt[4..8].copy_from_slice(&DHCP_XID.to_be_bytes());
    pkt[10] = 0x80; pkt[11] = 0x00;
    pkt[28..34].copy_from_slice(&mac);
    pkt[236..240].copy_from_slice(&MAGIC_COOKIE);

    let mut off = 240;
    // Option 53: DHCP Request (3)
    pkt[off] = 53; pkt[off + 1] = 1; pkt[off + 2] = DHCP_REQUEST;
    off += 3;

    // Option 50: Requested IP Address
    pkt[off] = 50; pkt[off + 1] = 4;
    pkt[off + 2..off + 6].copy_from_slice(&offered_ip);
    off += 6;

    // Option 54: Server Identifier
    pkt[off] = 54; pkt[off + 1] = 4;
    pkt[off + 2..off + 6].copy_from_slice(&server_ip);
    off += 6;

    // Option 55: Parameters
    pkt[off] = 55; pkt[off + 1] = 3;
    pkt[off + 2] = 1; pkt[off + 3] = 3; pkt[off + 4] = 6;
    off += 5;

    // End
    pkt[off] = 255;
    off += 1;

    send_udp([0, 0, 0, 0], [255, 255, 255, 255], 68, 67, &pkt[..off])
}

pub fn handle_packet(payload: &[u8]) {
    if payload.len() < 240 { return; }
    if &payload[236..240] != &MAGIC_COOKIE { return; }

    let xid = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
    if xid != DHCP_XID { return; }

    let yiaddr = [payload[16], payload[17], payload[18], payload[19]];

    // Parse options
    let mut off = 240;
    let mut msg_type = 0u8;
    let mut netmask = [255, 255, 255, 0];
    let mut router = [10, 0, 2, 2];
    let mut dns = [10, 0, 2, 3];
    let mut server_id = [10, 0, 2, 2];

    while off < payload.len() {
        let opt = payload[off];
        if opt == 255 { break; } // End
        if opt == 0 { off += 1; continue; } // Pad
        if off + 1 >= payload.len() { break; }
        let len = payload[off + 1] as usize;
        let data_start = off + 2;
        let data_end = data_start + len;
        if data_end > payload.len() { break; }

        match opt {
            53 => { if len >= 1 { msg_type = payload[data_start]; } }
            1  => { if len >= 4 { netmask.copy_from_slice(&payload[data_start..data_start + 4]); } }
            3  => { if len >= 4 { router.copy_from_slice(&payload[data_start..data_start + 4]); } }
            6  => { if len >= 4 { dns.copy_from_slice(&payload[data_start..data_start + 4]); } }
            54 => { if len >= 4 { server_id.copy_from_slice(&payload[data_start..data_start + 4]); } }
            _  => {}
        }
        off = data_end;
    }

    unsafe {
        match (DHCP_STATE, msg_type) {
            (DhcpState::Discovering, DHCP_OFFER) => {
                OFFERED_IP = yiaddr;
                SERVER_IP = server_id;
                DHCP_STATE = DhcpState::Requesting;
                serial::write_str("[DHCP] Received Offer: IP ");
                print_ip(&yiaddr);
                serial::write_str(" from server ");
                print_ip(&server_id);
                serial::write_str(". Sending Request...\n");
                let _ = send_dhcp_request(yiaddr, server_id);
            }
            (DhcpState::Requesting, DHCP_ACK) => {
                DHCP_STATE = DhcpState::Bound;
                set_ip(yiaddr);
                set_netmask(netmask);
                set_gateway(router);
                set_dns(dns);
                serial::write_str("[DHCP] Lease Bound! Assigned IP: ");
                print_ip(&yiaddr);
                serial::write_str(" Netmask: ");
                print_ip(&netmask);
                serial::write_str(" Gateway: ");
                print_ip(&router);
                serial::write_str(" DNS: ");
                print_ip(&dns);
                serial::write_str("\n");
            }
            _ => {}
        }
    }
}

fn print_ip(ip: &[u8]) {
    for i in 0..4 {
        serial::write_usize(ip[i] as usize);
        if i < 3 { serial::write_byte(b'.'); }
    }
}
