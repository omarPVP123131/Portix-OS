// kernel/src/net/dns.rs — DNS Resolver Client (RFC 1035)
#![allow(dead_code)]

use crate::drivers::serial;
use super::udp::send_udp;
use super::{get_ip, get_dns};

pub const DNS_PORT: u16 = 53;
pub const CLIENT_DNS_PORT: u16 = 53535;

static mut RESOLVED_IP: Option<[u8; 4]> = None;
static mut QUERY_ID: u16 = 0x1234;

pub fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut cur = 0u32;
    let mut oct_idx = 0;
    let mut has_digits = false;

    for b in s.bytes() {
        if b >= b'0' && b <= b'9' {
            cur = cur * 10 + (b - b'0') as u32;
            if cur > 255 { return None; }
            has_digits = true;
        } else if b == b'.' {
            if !has_digits || oct_idx >= 3 { return None; }
            octets[oct_idx] = cur as u8;
            oct_idx += 1;
            cur = 0;
            has_digits = false;
        } else {
            return None;
        }
    }
    if !has_digits || oct_idx != 3 { return None; }
    octets[3] = cur as u8;
    Some(octets)
}

pub fn resolve(domain: &str) -> Option<[u8; 4]> {
    if let Some(ip) = parse_ipv4(domain) {
        return Some(ip);
    }

    unsafe {
        RESOLVED_IP = None;
        QUERY_ID = QUERY_ID.wrapping_add(1);
        if QUERY_ID == 0 { QUERY_ID = 1; }
    }

    let mut query = [0u8; 256];
    let id = unsafe { QUERY_ID };
    query[0] = (id >> 8) as u8;
    query[1] = (id & 0xFF) as u8;
    query[2] = 0x01; // Flags: RD (Recursion Desired)
    query[3] = 0x00;
    query[4] = 0x00; query[5] = 0x01; // Questions: 1
    query[6] = 0x00; query[7] = 0x00; // Answers: 0
    query[8] = 0x00; query[9] = 0x00; // Authority: 0
    query[10] = 0x00; query[11] = 0x00; // Additional: 0

    let mut off = 12;
    // Format domain labels (e.g. google.com -> \x06google\x03com\x00)
    for part in domain.split('.') {
        let len = part.len();
        if len == 0 || len > 63 || off + len + 1 >= query.len() {
            return None;
        }
        query[off] = len as u8;
        off += 1;
        query[off..off + len].copy_from_slice(part.as_bytes());
        off += len;
    }
    query[off] = 0; // Terminating zero length
    off += 1;

    // QTYPE = 1 (A record)
    query[off] = 0x00; query[off + 1] = 0x01;
    // QCLASS = 1 (IN)
    query[off + 2] = 0x00; query[off + 3] = 0x01;
    off += 4;

    let src_ip = get_ip();
    let dns_server = get_dns();

    serial::write_str("[DNS] Resolving domain '");
    serial::write_str(domain);
    serial::write_str("' via server ");
    print_ip(&dns_server);
    serial::write_str("...\n");

    let _ = send_udp(src_ip, dns_server, CLIENT_DNS_PORT, DNS_PORT, &query[..off]);

    // Poll for response up to ~100 iterations (1 second)
    for _ in 0..100 {
        crate::net::poll();
        if let Some(ip) = unsafe { RESOLVED_IP } {
            serial::write_str("[DNS] '");
            serial::write_str(domain);
            serial::write_str("' resolved to ");
            print_ip(&ip);
            serial::write_str("\n");
            return Some(ip);
        }
        let start = crate::time::pit::ticks();
        while crate::time::pit::ticks() == start {}
    }

    serial::write_str("[DNS] Resolution timed out for '");
    serial::write_str(domain);
    serial::write_str("'\n");
    None
}

pub fn handle_packet(payload: &[u8]) {
    if payload.len() < 12 { return; }
    let id = ((payload[0] as u16) << 8) | (payload[1] as u16);
    if id != unsafe { QUERY_ID } { return; }

    let ancount = ((payload[6] as u16) << 8) | (payload[7] as u16);
    if ancount == 0 { return; }

    // Skip Header (12 bytes) + Question section
    let mut off = 12;
    // Skip QNAME
    while off < payload.len() {
        let len = payload[off] as usize;
        if len == 0 { off += 1; break; }
        if (len & 0xC0) == 0xC0 { off += 2; break; } // Pointer
        off += 1 + len;
    }
    // Skip QTYPE and QCLASS (4 bytes)
    off += 4;

    // Parse Answers
    for _ in 0..ancount {
        if off >= payload.len() { break; }
        // Skip Name
        if (payload[off] & 0xC0) == 0xC0 {
            off += 2;
        } else {
            while off < payload.len() {
                let len = payload[off] as usize;
                if len == 0 { off += 1; break; }
                off += 1 + len;
            }
        }
        if off + 10 > payload.len() { break; }
        let atype = ((payload[off] as u16) << 8) | (payload[off + 1] as u16);
        let rdlen = ((payload[off + 8] as usize) << 8) | (payload[off + 9] as usize);
        off += 10;
        if atype == 1 && rdlen == 4 && off + 4 <= payload.len() {
            let ip = [payload[off], payload[off + 1], payload[off + 2], payload[off + 3]];
            unsafe {
                RESOLVED_IP = Some(ip);
            }
            return;
        }
        off += rdlen;
    }
}

fn print_ip(ip: &[u8]) {
    for i in 0..4 {
        serial::write_usize(ip[i] as usize);
        if i < 3 { serial::write_byte(b'.'); }
    }
}
