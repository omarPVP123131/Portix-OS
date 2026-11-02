// kernel/src/net/udp.rs — UDP Protocol Implementation & Pseudo-Header Checksum
#![allow(dead_code)]

use crate::drivers::net::e1000;
use super::{checksum, ETHERTYPE_IPV4, IP_PROTO_UDP};

pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
}

pub fn udp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], udp_data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    // Source IP
    sum = sum.wrapping_add(((src_ip[0] as u32) << 8) | (src_ip[1] as u32));
    sum = sum.wrapping_add(((src_ip[2] as u32) << 8) | (src_ip[3] as u32));
    // Dest IP
    sum = sum.wrapping_add(((dst_ip[0] as u32) << 8) | (dst_ip[1] as u32));
    sum = sum.wrapping_add(((dst_ip[2] as u32) << 8) | (dst_ip[3] as u32));
    // Protocol (17)
    sum = sum.wrapping_add(17);
    // UDP Length
    sum = sum.wrapping_add(udp_data.len() as u32);

    let mut i = 0;
    while i + 1 < udp_data.len() {
        let w = ((udp_data[i] as u32) << 8) | (udp_data[i + 1] as u32);
        sum = sum.wrapping_add(w);
        i += 2;
    }
    if i < udp_data.len() {
        sum = sum.wrapping_add((udp_data[i] as u32) << 8);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let res = !(sum as u16);
    if res == 0 { 0xFFFF } else { res }
}

pub fn send_udp(
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> Result<(), &'static str> {
    if !e1000::is_active() {
        return Err("e1000 inactive");
    }

    let my_mac = e1000::get_mac();
    let is_broadcast = dst_ip == [255, 255, 255, 255];
    let loopback = super::is_loopback_v4(dst_ip);
    // [BUCLE LOCAL] destino 127/8: el origen tambien es 127.0.0.1 para que el receptor vea el mismo par
    let src_ip = if dst_ip[0] == 127 { dst_ip } else { src_ip };
    let dest_mac = if loopback {
        my_mac
    } else if is_broadcast {
        [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
    } else {
        // [IPv6/UDP] Resolución ARP REAL del siguiente salto (antes había una
        // MAC fija 52:54:00:12:34:02 que no corresponde a ningún equipo).
        match super::arp_resolve(super::next_hop(dst_ip), 100) {
            Some(m) => m,
            None => return Err("ARP sin resolver: siguiente salto no responde"),
        }
    };

    let udp_len = 8 + payload.len();
    let ip_len = 20 + udp_len;
    let frame_len = 14 + ip_len;

    if frame_len > 1518 {
        return Err("packet too large");
    }

    let mut frame = alloc::vec![0u8; frame_len];

    // Ethernet Header
    frame[0..6].copy_from_slice(&dest_mac);
    frame[6..12].copy_from_slice(&my_mac);
    frame[12] = (ETHERTYPE_IPV4 >> 8) as u8;
    frame[13] = (ETHERTYPE_IPV4 & 0xFF) as u8;

    // IPv4 Header
    frame[14] = 0x45;
    frame[15] = 0;
    frame[16] = (ip_len >> 8) as u8;
    frame[17] = (ip_len & 0xFF) as u8;
    frame[18] = 0xAB; frame[19] = 0xCD; // ID
    frame[20] = 0x40; frame[21] = 0x00; // DF
    frame[22] = 64;   // TTL
    frame[23] = IP_PROTO_UDP;
    frame[24] = 0; frame[25] = 0; // Checksum placeholder
    frame[26..30].copy_from_slice(&src_ip);
    frame[30..34].copy_from_slice(&dst_ip);

    let ip_csum = checksum(&frame[14..34]);
    frame[24] = (ip_csum >> 8) as u8;
    frame[25] = (ip_csum & 0xFF) as u8;

    // UDP Header
    let udp_off = 34;
    frame[udp_off]     = (src_port >> 8) as u8;
    frame[udp_off + 1] = (src_port & 0xFF) as u8;
    frame[udp_off + 2] = (dst_port >> 8) as u8;
    frame[udp_off + 3] = (dst_port & 0xFF) as u8;
    frame[udp_off + 4] = (udp_len >> 8) as u8;
    frame[udp_off + 5] = (udp_len & 0xFF) as u8;
    frame[udp_off + 6] = 0; frame[udp_off + 7] = 0; // Checksum placeholder
    frame[udp_off + 8..frame_len].copy_from_slice(payload);

    let udp_csum = udp_checksum(src_ip, dst_ip, &frame[udp_off..frame_len]);
    frame[udp_off + 6] = (udp_csum >> 8) as u8;
    frame[udp_off + 7] = (udp_csum & 0xFF) as u8;

    if loopback { return super::loopback_enqueue(&frame); }
    e1000::send_packet(&frame)?;
    Ok(())
}

/// [IPv6 REAL] UDP sobre IPv6: suma obligatoria con pseudo-cabecera (RFC 8200 §8.1).
pub fn send_udp6(src: &super::ipv6::Ipv6Addr, dst: &super::ipv6::Ipv6Addr, src_port: u16, dst_port: u16, payload: &[u8]) -> Result<(), &'static str> {
    let udp_len = 8 + payload.len();
    let mut seg = alloc::vec::Vec::with_capacity(udp_len);
    seg.extend_from_slice(&src_port.to_be_bytes());
    seg.extend_from_slice(&dst_port.to_be_bytes());
    seg.extend_from_slice(&(udp_len as u16).to_be_bytes());
    seg.extend_from_slice(&[0, 0]);
    seg.extend_from_slice(payload);
    let mut ck = super::ipv6::checksum_v6(src, dst, super::ipv6::NEXT_HEADER_UDP, &seg);
    if ck == 0 { ck = 0xFFFF; }
    seg[6] = (ck >> 8) as u8; seg[7] = ck as u8;
    super::ipv6::send_ipv6(dst, super::ipv6::NEXT_HEADER_UDP, &seg)
}

/// Entrada UDP/IPv6: valida la suma y entrega al socket por puerto destino.
pub fn handle_udp6(src: &super::ipv6::Ipv6Addr, dst: &super::ipv6::Ipv6Addr, seg: &[u8]) {
    if seg.len() < 8 { return; }
    if super::ipv6::checksum_v6(src, dst, super::ipv6::NEXT_HEADER_UDP, seg) != 0 {
        crate::drivers::serial::write_str("[UDP6] datagrama descartado: suma de comprobacion invalida\n");
        return;
    }
    let src_port = u16::from_be_bytes([seg[0], seg[1]]);
    let dst_port = u16::from_be_bytes([seg[2], seg[3]]);
    let len = (u16::from_be_bytes([seg[4], seg[5]]) as usize).min(seg.len());
    if len < 8 { return; }
    super::socket::deliver_udp_to_socket(super::IpAddr::V6(*src), src_port, dst_port, &seg[8..len]);
}
