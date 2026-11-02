// kernel/src/net/mod.rs — Lightweight Ethernet / ARP / IPv4 / ICMP / UDP / DHCP / DNS Stack
#![allow(dead_code)]

pub mod udp;
pub mod tcp;
pub mod socket;
pub mod dhcp;
pub mod dns;
pub mod ipv6;
pub mod firewall;
pub mod wireguard;
pub mod xdp;
pub mod wifi_80211;
pub mod smb_thunderbolt;
pub mod wifi_portal;

use crate::drivers::net::e1000;
use crate::drivers::serial;
use crate::arch::Spinlock;

#[derive(Clone, Copy)]
pub struct NetConfig {
    pub ip: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns: [u8; 4],
}

static NET_CONFIG: Spinlock<NetConfig> = Spinlock::new(NetConfig {
    ip: [10, 0, 2, 15],
    netmask: [255, 255, 255, 0],
    gateway: [10, 0, 2, 2],
    dns: [10, 0, 2, 3],
});

// ── [CIERRE RED] Caché ARP real con expiración ───────────────────────────
// Antes la MAC de la pasarela estaba cableada (52:54:00:12:34:02, la de
// slirp): fuera de QEMU ningún paquete habría salido. Ahora `send_ipv4`
// resuelve el siguiente salto (destino directo si está en la subred, la
// pasarela si no) con una petición ARP real y una caché con vencimiento.
pub const ARP_TTL_TICKS: u64 = 60 * 100; // 60 s a 100 Hz
pub const ARP_ENTRIES: usize = 16;

#[derive(Clone, Copy)]
pub struct ArpEntry { pub ip: [u8; 4], pub mac: [u8; 6], pub learned_ticks: u64, pub valid: bool, pub hits: u32 }

pub struct ArpStats { pub requests: u32, pub replies: u32, pub expired: u32, pub failures: u32 }

static ARP_CACHE: Spinlock<[ArpEntry; ARP_ENTRIES]> = Spinlock::new([ArpEntry { ip: [0; 4], mac: [0; 6], learned_ticks: 0, valid: false, hits: 0 }; ARP_ENTRIES]);
static ARP_STATS: Spinlock<ArpStats> = Spinlock::new(ArpStats { requests: 0, replies: 0, expired: 0, failures: 0 });

fn arp_lookup(ip: [u8; 4]) -> Option<[u8; 6]> {
    let now = crate::time::pit::ticks();
    let mut cache = ARP_CACHE.lock();
    for e in cache.iter_mut() {
        if e.valid && e.ip == ip {
            if now.wrapping_sub(e.learned_ticks) > ARP_TTL_TICKS {
                e.valid = false;
                ARP_STATS.lock().expired += 1;
                serial::write_str("[NET] ARP entrada expirada: "); print_ip(&ip); serial::write_str("\n");
                return None;
            }
            e.hits += 1;
            return Some(e.mac);
        }
    }
    None
}

fn arp_learn(ip: [u8; 4], mac: [u8; 6]) {
    if ip == [0, 0, 0, 0] { return; }
    let now = crate::time::pit::ticks();
    let mut cache = ARP_CACHE.lock();
    // actualizar si existe
    for e in cache.iter_mut() { if e.valid && e.ip == ip { e.mac = mac; e.learned_ticks = now; return; } }
    // hueco libre o la más antigua
    let mut idx = 0; let mut oldest = u64::MAX;
    for (i, e) in cache.iter().enumerate() {
        if !e.valid { idx = i; break; }
        if e.learned_ticks < oldest { oldest = e.learned_ticks; idx = i; }
    }
    cache[idx] = ArpEntry { ip, mac, learned_ticks: now, valid: true, hits: 0 };
}

fn arp_send_request(target_ip: [u8; 4]) -> Result<(), &'static str> {
    let my_mac = e1000::get_mac(); let my_ip = get_ip();
    let mut req = [0u8; 42];
    req[0..6].copy_from_slice(&[0xFF; 6]); req[6..12].copy_from_slice(&my_mac);
    req[12] = (ETHERTYPE_ARP >> 8) as u8; req[13] = (ETHERTYPE_ARP & 0xFF) as u8;
    req[14] = 0; req[15] = 1; req[16] = 0x08; req[17] = 0x00; req[18] = 6; req[19] = 4; req[20] = 0; req[21] = 1;
    req[22..28].copy_from_slice(&my_mac); req[28..32].copy_from_slice(&my_ip);
    req[38..42].copy_from_slice(&target_ip);
    ARP_STATS.lock().requests += 1;
    e1000::send_packet(&req)
}

/// Siguiente salto IP para `dst` (destino directo en la subred o pasarela).
pub fn next_hop(dst: [u8; 4]) -> [u8; 4] {
    let (ip, mask, gw) = { let c = NET_CONFIG.lock(); (c.ip, c.netmask, c.gateway) };
    let same = (0..4).all(|i| (ip[i] & mask[i]) == (dst[i] & mask[i]));
    if same { dst } else { gw }
}

/// Resuelve la MAC de `ip` (caché o petición ARP real, esperando la respuesta
/// hasta `timeout_ticks`). `None` si nadie contesta: el paquete NO se envía.
pub fn arp_resolve(ip: [u8; 4], timeout_ticks: u64) -> Option<[u8; 6]> {
    if let Some(m) = arp_lookup(ip) { return Some(m); }
    if arp_send_request(ip).is_err() { return None; }
    let inicio = crate::time::pit::ticks();
    loop {
        // Solo ARP: `arp_resolve` puede ejecutarse con TCP_TABLE tomado (envio
        // de segmentos), asi que las tramas no-ARP se difieren al proximo poll().
        poll_arp_only();
        if let Some(m) = arp_lookup(ip) {
            serial::write_str("[NET] ARP resuelto "); print_ip(&ip); serial::write_str(" -> "); print_mac(&m); serial::write_str("\n");
            return Some(m);
        }
        if crate::time::pit::ticks().wrapping_sub(inicio) > timeout_ticks { break; }
        unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
    }
    ARP_STATS.lock().failures += 1;
    serial::write_str("[NET] ARP sin respuesta para "); print_ip(&ip); serial::write_str("\n");
    None
}

/// Vacía la caché ARP (comando `arp flush`).
pub fn arp_flush() { for e in ARP_CACHE.lock().iter_mut() { e.valid = false; } }

/// Instantánea de la caché para la consola: (entradas, estadísticas, tick actual).
pub fn arp_snapshot() -> ([ArpEntry; ARP_ENTRIES], (u32, u32, u32, u32), u64) {
    let c = *ARP_CACHE.lock(); let s = ARP_STATS.lock();
    (c, (s.requests, s.replies, s.expired, s.failures), crate::time::pit::ticks())
}

pub fn print_mac(m: &[u8; 6]) {
    const H: &[u8; 16] = b"0123456789abcdef";
    for i in 0..6 {
        if i > 0 { serial::write_str(":"); }
        let b = [H[(m[i] >> 4) as usize], H[(m[i] & 15) as usize]];
        serial::write_str(core::str::from_utf8(&b).unwrap_or("??"));
    }
}

// ── [CIERRE RED] Respuestas ICMP echo reales ─────────────────────────────
/// Última respuesta echo recibida: (ip origen, seq, ticks de llegada, bytes ICMP).
static LAST_ECHO_REPLY: Spinlock<Option<([u8; 4], u16, u64, usize)>> = Spinlock::new(None);

pub fn take_echo_reply(seq: u16) -> Option<([u8; 4], u64, usize)> {
    let mut g = LAST_ECHO_REPLY.lock();
    if let Some((ip, s, t, n)) = *g { if s == seq { *g = None; return Some((ip, t, n)); } }
    None
}

pub fn get_ip() -> [u8; 4] { NET_CONFIG.lock().ip }
pub fn get_netmask() -> [u8; 4] { NET_CONFIG.lock().netmask }
pub fn get_gateway() -> [u8; 4] { NET_CONFIG.lock().gateway }
pub fn get_dns() -> [u8; 4] { NET_CONFIG.lock().dns }

pub fn set_ip(ip: [u8; 4]) { NET_CONFIG.lock().ip = ip; }
pub fn set_netmask(mask: [u8; 4]) { NET_CONFIG.lock().netmask = mask; }
pub fn set_gateway(gw: [u8; 4]) { NET_CONFIG.lock().gateway = gw; }
pub fn set_dns(dns: [u8; 4]) { NET_CONFIG.lock().dns = dns; }

/// [IPv6 REAL] Dirección IP de doble pila para la capa de transporte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpAddr { V4([u8; 4]), V6(ipv6::Ipv6Addr) }
impl IpAddr {
    pub const UNSPECIFIED4: IpAddr = IpAddr::V4([0, 0, 0, 0]);
    pub fn v4(&self) -> [u8; 4] { match self { IpAddr::V4(a) => *a, IpAddr::V6(_) => [0, 0, 0, 0] } }
    pub fn is_v6(&self) -> bool { matches!(self, IpAddr::V6(_)) }
    pub fn to_string(&self) -> alloc::string::String {
        match self { IpAddr::V4(a) => alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]), IpAddr::V6(a) => alloc::format!("[{}]", a.to_string()) }
    }
}
impl From<[u8; 4]> for IpAddr { fn from(a: [u8; 4]) -> Self { IpAddr::V4(a) } }

pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP:  u16 = 0x0806;

pub const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_TCP:  u8 = 6;
pub const IP_PROTO_UDP:  u8 = 17;

pub fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = ((data[i] as u32) << 8) | (data[i + 1] as u32);
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

pub fn init() {
    crate::drivers::net::init();
    ipv6::ipv6_init();
    wireguard::wireguard_init();
    xdp::xdp_init();
    if e1000::is_active() {
        serial::write_str("[NET] Stack online. Local IP: 10.0.2.15 Netmask: 255.255.255.0 Gateway: 10.0.2.2\n");
        serial::write_str("[NET] IPv6: direccion de enlace ");
        serial::write_str(&ipv6::IPV6_STATE.lock().link_local.to_string());
        serial::write_str(" (EUI-64 de la MAC real); global pendiente de Router Advertisement\n");
        serial::write_str("[NET] WireGuard wg0 online (port 51820) + XDP Hook L2 activo\n");
        // Iniciar negociación DHCP en background
        dhcp::start();
    } else {
        serial::write_str("[NET] No supported Ethernet adapter detected\n");
    }
}

/// Tramas recibidas durante `arp_resolve` que no eran ARP: se procesan en el
/// siguiente `poll()` para no reentrar en TCP/UDP con sus cerrojos tomados.
const DEFERRED_MAX: usize = 32;
struct DeferredFrames { n: usize, len: [usize; DEFERRED_MAX], buf: [[u8; 1518]; DEFERRED_MAX] }
static DEFERRED: Spinlock<DeferredFrames> = Spinlock::new(DeferredFrames { n: 0, len: [0; DEFERRED_MAX], buf: [[0; 1518]; DEFERRED_MAX] });

/// Procesa sólo tramas de resolución de enlace (ARP y NDP/ICMPv6) y difiere el
/// resto: se usa mientras el emisor tiene tomado el cerrojo de TCP/UDP.
pub(crate) fn poll_arp_only() {
    if !e1000::is_active() { return; }
    let mut buf = [0u8; 1518];
    while let Some(len) = e1000::poll_packet(&mut buf) {
        if len < 14 { continue; }
        let ethertype = ((buf[12] as u16) << 8) | (buf[13] as u16);
        if ethertype == 0x0806 { handle_ethernet_frame(&buf[..len]); continue; }
        // [IPv6 REAL] ICMPv6 (NS/NA/RA) no toca la tabla TCP: se atiende ya
        if ethertype == ipv6::ETHERTYPE_IPV6 && len >= 54 && buf[14 + 6] == ipv6::NEXT_HEADER_ICMPV6 { handle_ethernet_frame(&buf[..len]); continue; }
        let mut d = DEFERRED.lock();
        if d.n < DEFERRED_MAX { let i = d.n; d.len[i] = len; d.buf[i][..len].copy_from_slice(&buf[..len]); d.n += 1; }
    }
}

/// [LOOPBACK] Encola una trama Ethernet completa para que `poll()` la procese
/// como si hubiera llegado por el cable (127.0.0.0/8, ::1 y nuestras propias
/// direcciones). Se difiere en vez de entregarla en línea porque el emisor
/// puede tener tomado el cerrojo de TCP.
pub fn loopback_enqueue(frame: &[u8]) -> Result<(), &'static str> {
    if frame.len() > 1518 { return Err("trama de bucle local demasiado grande"); }
    let mut d = DEFERRED.lock();
    if d.n >= DEFERRED_MAX { return Err("cola de bucle local llena"); }
    let i = d.n; d.len[i] = frame.len(); d.buf[i][..frame.len()].copy_from_slice(frame); d.n += 1;
    Ok(())
}

pub fn is_loopback_v4(ip: [u8; 4]) -> bool { ip[0] == 127 || ip == get_ip() }

pub fn poll() {
    if !e1000::is_active() { return; }
    ipv6::dad_tick();

    // Primero las tramas diferidas por arp_resolve()
    loop {
        // FIFO: con LIFO los segmentos TCP del bucle local llegaban desordenados
        let frame = { let mut d = DEFERRED.lock(); if d.n == 0 { break; } let l = d.len[0]; let mut f = [0u8; 1518]; f[..l].copy_from_slice(&d.buf[0][..l]);
            let n = d.n; for i in 1..n { let li = d.len[i]; d.len[i-1] = li; let (a, b) = d.buf.split_at_mut(i); a[i-1][..li].copy_from_slice(&b[0][..li]); } d.n -= 1; (f, l) };
        let xdp_action = xdp::XDP_ENGINE.lock().process_packet(&frame.0[..frame.1]);
        if xdp_action != xdp::XDP_DROP { handle_ethernet_frame(&frame.0[..frame.1]); }
    }

    // [CIERRE TCP] temporizador de retransmisión de datos
    tcp::tcp_timer();

    let mut buf = [0u8; 1518];
    while let Some(len) = e1000::poll_packet(&mut buf) {
        if len < 14 { continue; }
        // Gancho XDP temprano en capa de enlace
        let xdp_action = xdp::XDP_ENGINE.lock().process_packet(&buf[..len]);
        if xdp_action == xdp::XDP_DROP {
            continue;
        }
        handle_ethernet_frame(&buf[..len]);
    }
}

fn handle_ethernet_frame(frame: &[u8]) {
    let my_mac = e1000::get_mac();
    let dst_mac = &frame[0..6];
    let is_broadcast = dst_mac == [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let is_for_me = dst_mac == my_mac;
    // [IPv6 REAL] multidifusión Ethernet de IPv6 (33:33:xx:xx:xx:xx, RFC 2464)
    let is_v6_multicast = dst_mac[0] == 0x33 && dst_mac[1] == 0x33;

    if !is_for_me && !is_broadcast && !is_v6_multicast {
        return;
    }

    let ethertype = ((frame[12] as u16) << 8) | (frame[13] as u16);
    match ethertype {
        ETHERTYPE_ARP => handle_arp(&frame[14..]),
        ETHERTYPE_IPV4 => handle_ipv4(&frame[14..], &frame[6..12]),
        ipv6::ETHERTYPE_IPV6 => ipv6::handle_ipv6(&frame[14..], &frame[6..12]),
        _ => {}
    }
}

fn handle_arp(arp: &[u8]) {
    if arp.len() < 28 { return; }
    let htype = ((arp[0] as u16) << 8) | (arp[1] as u16);
    let ptype = ((arp[2] as u16) << 8) | (arp[3] as u16);
    let hlen = arp[4];
    let plen = arp[5];
    let opcode = ((arp[6] as u16) << 8) | (arp[7] as u16);

    if htype != 1 || ptype != 0x0800 || hlen != 6 || plen != 4 {
        return;
    }

    let sender_mac = &arp[8..14];
    let sender_ip = &arp[14..18];
    let target_ip = [arp[24], arp[25], arp[26], arp[27]];

    let my_ip = get_ip();
    {
        let mut sm = [0u8; 6]; sm.copy_from_slice(sender_mac);
        let sip = [sender_ip[0], sender_ip[1], sender_ip[2], sender_ip[3]];
        if opcode == 2 { ARP_STATS.lock().replies += 1; }
        // Aprender del emisor (petición o respuesta) — RFC 826 §"Packet Reception".
        arp_learn(sip, sm);
    }
    if opcode == 1 && target_ip == my_ip {
        let my_mac = e1000::get_mac();
        let mut reply = [0u8; 42];

        reply[0..6].copy_from_slice(sender_mac);
        reply[6..12].copy_from_slice(&my_mac);
        reply[12] = (ETHERTYPE_ARP >> 8) as u8;
        reply[13] = (ETHERTYPE_ARP & 0xFF) as u8;

        reply[14] = 0; reply[15] = 1;
        reply[16] = 0x08; reply[17] = 0x00;
        reply[18] = 6;
        reply[19] = 4;
        reply[20] = 0; reply[21] = 2;
        reply[22..28].copy_from_slice(&my_mac);
        reply[28..32].copy_from_slice(&my_ip);
        reply[32..38].copy_from_slice(sender_mac);
        reply[38..42].copy_from_slice(sender_ip);

        let _ = e1000::send_packet(&reply);
        serial::write_str("[NET] ARP Reply sent to ");
        print_ip(sender_ip);
        serial::write_str("\n");
    }
}

fn handle_ipv4(ip: &[u8], src_mac: &[u8]) {
    if ip.len() < 20 { return; }
    let version_ihl = ip[0];
    let version = version_ihl >> 4;
    let ihl = (version_ihl & 0x0F) as usize * 4;
    if version != 4 || ihl < 20 || ip.len() < ihl { return; }

    let total_len = (((ip[2] as u16) << 8) | (ip[3] as u16)) as usize;
    if ip.len() < total_len { return; }

    let proto = ip[9];
    let src_ip = [ip[12], ip[13], ip[14], ip[15]];
    let dst_ip = [ip[16], ip[17], ip[18], ip[19]];

    let my_ip = get_ip();
    if dst_ip != my_ip && dst_ip != [255, 255, 255, 255] && dst_ip != [10, 0, 2, 15] && dst_ip[0] != 127 {
        return;
    }

    match proto {
        IP_PROTO_ICMP => handle_icmp(&ip[ihl..total_len], &src_ip, src_mac),
        IP_PROTO_TCP => {
            if total_len >= ihl + 20 {
                tcp::handle_tcp_packet(IpAddr::V4(src_ip), IpAddr::V4(my_ip), &ip[ihl..total_len]);
            }
        }
        IP_PROTO_UDP => {
            if total_len >= ihl + 8 {
                let udp = &ip[ihl..total_len];
                let src_port = ((udp[0] as u16) << 8) | (udp[1] as u16);
                let dst_port = ((udp[2] as u16) << 8) | (udp[3] as u16);
                let udp_len = (((udp[4] as u16) << 8) | (udp[5] as u16)) as usize;
                let payload_len = udp_len.saturating_sub(8).min(udp.len().saturating_sub(8));
                let payload = &udp[8..8 + payload_len];

                if dst_port == 68 {
                    // DHCP Client port
                    dhcp::handle_packet(payload);
                } else if dst_port == dns::CLIENT_DNS_PORT {
                    // DNS Client port
                    dns::handle_packet(payload);
                } else {
                    // Deliver to matching user socket
                    socket::deliver_udp_to_socket(IpAddr::V4(src_ip), src_port, dst_port, payload);
                }
            }
        }
        _ => {}
    }
}

fn handle_icmp(icmp: &[u8], sender_ip: &[u8], sender_mac: &[u8]) {
    if icmp.len() < 8 { return; }
    let icmp_type = icmp[0];
    let icmp_code = icmp[1];

    if icmp_type == 0 && icmp_code == 0 {
        // Echo reply real: lo recoge `send_ping` para medir el RTT.
        let seq = ((icmp[6] as u16) << 8) | icmp[7] as u16;
        let ip = [sender_ip[0], sender_ip[1], sender_ip[2], sender_ip[3]];
        *LAST_ECHO_REPLY.lock() = Some((ip, seq, crate::time::pit::ticks(), icmp.len()));
        return;
    }

    if icmp_type == 8 && icmp_code == 0 {
        let my_mac = e1000::get_mac();
        let my_ip = get_ip();
        let payload_len = icmp.len();
        let ip_total_len = 20 + payload_len;
        let frame_len = 14 + ip_total_len;

        let mut frame = [0u8; 1518];
        if frame_len > 1518 { return; }

        frame[0..6].copy_from_slice(sender_mac);
        frame[6..12].copy_from_slice(&my_mac);
        frame[12] = (ETHERTYPE_IPV4 >> 8) as u8;
        frame[13] = (ETHERTYPE_IPV4 & 0xFF) as u8;

        frame[14] = 0x45;
        frame[15] = 0;
        frame[16] = (ip_total_len >> 8) as u8;
        frame[17] = (ip_total_len & 0xFF) as u8;
        frame[18] = 0x12; frame[19] = 0x34;
        frame[20] = 0x40; frame[21] = 0x00;
        frame[22] = 64;
        frame[23] = IP_PROTO_ICMP;
        frame[24] = 0; frame[25] = 0;
        let sip = [sender_ip[0], sender_ip[1], sender_ip[2], sender_ip[3]];
        let lo = sip[0] == 127 || sip == my_ip;
        frame[26..30].copy_from_slice(if sip[0] == 127 { &sip } else { &my_ip });
        frame[30..34].copy_from_slice(sender_ip);

        let ip_csum = checksum(&frame[14..34]);
        frame[24] = (ip_csum >> 8) as u8;
        frame[25] = (ip_csum & 0xFF) as u8;

        let icmp_offset = 34;
        frame[icmp_offset..icmp_offset + payload_len].copy_from_slice(icmp);
        frame[icmp_offset] = 0;
        frame[icmp_offset + 1] = 0;
        frame[icmp_offset + 2] = 0;
        frame[icmp_offset + 3] = 0;

        let icmp_csum = checksum(&frame[icmp_offset..icmp_offset + payload_len]);
        frame[icmp_offset + 2] = (icmp_csum >> 8) as u8;
        frame[icmp_offset + 3] = (icmp_csum & 0xFF) as u8;

        if lo { let _ = loopback_enqueue(&frame[..frame_len]); } else {
            let _ = e1000::send_packet(&frame[..frame_len]); }

        let seq = ((icmp[6] as u16) << 8) | (icmp[7] as u16);
        serial::write_str("[NET] ICMP Echo Reply sent to ");
        print_ip(sender_ip);
        serial::write_str(" seq=");
        serial::write_usize(seq as usize);
        serial::write_str("\n");
    }
}

pub fn send_ipv4(dst_ip: [u8; 4], protocol: u8, payload: &[u8]) -> Result<(), &'static str> {
    if !e1000::is_active() {
        return Err("e1000 inactive");
    }

    let my_ip = get_ip();
    let my_mac = e1000::get_mac();
    let is_broadcast = dst_ip == [255, 255, 255, 255];
    let loopback = is_loopback_v4(dst_ip);
    let dest_mac = if is_broadcast || loopback {
        if loopback { my_mac } else { [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] }
    } else {
        // [CIERRE RED] Resolución ARP real del siguiente salto (antes MAC fija).
        match arp_resolve(next_hop(dst_ip), 100) {
            Some(m) => m,
            None => return Err("ARP sin resolver: siguiente salto no responde"),
        }
    };

    let ip_len = 20 + payload.len();
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
    frame[23] = protocol;
    frame[24] = 0; frame[25] = 0; // Checksum placeholder
    frame[26..30].copy_from_slice(if dst_ip[0] == 127 { &dst_ip } else { &my_ip });
    frame[30..34].copy_from_slice(&dst_ip);

    let ip_csum = checksum(&frame[14..34]);
    frame[24] = (ip_csum >> 8) as u8;
    frame[25] = (ip_csum & 0xFF) as u8;

    frame[34..frame_len].copy_from_slice(payload);

    if loopback { return loopback_enqueue(&frame); }
    e1000::send_packet(&frame)?;
    Ok(())
}

/// Envía un echo request y ESPERA la respuesta real (hasta 1 s).
/// Devuelve (bytes ICMP de la respuesta, RTT en microsegundos medido con el PIT/TSC).
pub fn ping_rtt(target_ip: [u8; 4], seq: u16, timeout_ticks: u64) -> Result<(usize, u64), &'static str> {
    let _ = take_echo_reply(seq);
    let t0 = crate::time::pit::ticks();
    let tsc0 = unsafe { core::arch::x86_64::_rdtsc() };
    send_ping(target_ip, seq)?;
    loop {
        poll();
        if let Some((_ip, _t, n)) = take_echo_reply(seq) {
            let tsc1 = unsafe { core::arch::x86_64::_rdtsc() };
            let hz = crate::time::pit::tsc_hz_estimate();
            let us = if hz > 0 { (tsc1.wrapping_sub(tsc0)) / (hz / 1_000_000).max(1) } else { crate::time::pit::ticks().wrapping_sub(t0) * 10_000 };
            return Ok((n, us));
        }
        if crate::time::pit::ticks().wrapping_sub(t0) > timeout_ticks { return Err("sin respuesta (tiempo agotado)"); }
        unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
    }
}

pub fn send_ping(target_ip: [u8; 4], seq: u16) -> Result<(), &'static str> {
    if !e1000::is_active() { return Err("e1000 inactive"); }

    let my_mac = e1000::get_mac();
    let my_ip = get_ip();
    let loopback = is_loopback_v4(target_ip);
    let gateway_mac = if loopback { my_mac } else { match arp_resolve(next_hop(target_ip), 100) { Some(m) => m, None => return Err("ARP sin resolver: siguiente salto no responde") } };
    let my_ip = if target_ip[0] == 127 { target_ip } else { my_ip };
    let payload_data = b"PORTIX_PING_ECHO_PACKET_2026";
    let icmp_len = 8 + payload_data.len();
    let ip_len = 20 + icmp_len;
    let frame_len = 14 + ip_len;

    let mut frame = [0u8; 256];

    frame[0..6].copy_from_slice(&gateway_mac);
    frame[6..12].copy_from_slice(&my_mac);
    frame[12] = (ETHERTYPE_IPV4 >> 8) as u8;
    frame[13] = (ETHERTYPE_IPV4 & 0xFF) as u8;

    frame[14] = 0x45;
    frame[15] = 0;
    frame[16] = (ip_len >> 8) as u8;
    frame[17] = (ip_len & 0xFF) as u8;
    frame[18] = 0x56; frame[19] = 0x78;
    frame[20] = 0x40; frame[21] = 0x00;
    frame[22] = 64;
    frame[23] = IP_PROTO_ICMP;
    frame[24] = 0; frame[25] = 0;
    frame[26..30].copy_from_slice(&my_ip);
    frame[30..34].copy_from_slice(&target_ip);

    let ip_csum = checksum(&frame[14..34]);
    frame[24] = (ip_csum >> 8) as u8;
    frame[25] = (ip_csum & 0xFF) as u8;

    let icmp_off = 34;
    frame[icmp_off] = 8;
    frame[icmp_off + 1] = 0;
    frame[icmp_off + 2] = 0; frame[icmp_off + 3] = 0;
    frame[icmp_off + 4] = 0x12; frame[icmp_off + 5] = 0x34;
    frame[icmp_off + 6] = (seq >> 8) as u8;
    frame[icmp_off + 7] = (seq & 0xFF) as u8;
    frame[icmp_off + 8..icmp_off + icmp_len].copy_from_slice(payload_data);

    let icmp_csum = checksum(&frame[icmp_off..icmp_off + icmp_len]);
    frame[icmp_off + 2] = (icmp_csum >> 8) as u8;
    frame[icmp_off + 3] = (icmp_csum & 0xFF) as u8;

    if loopback { return loopback_enqueue(&frame[..frame_len]); }
    e1000::send_packet(&frame[..frame_len])?;
    serial::write_str("[NET] PING sent to ");
    print_ip(&target_ip);
    serial::write_str(" seq=");
    serial::write_usize(seq as usize);
    serial::write_str("\n");
    Ok(())
}

pub fn print_ip(ip: &[u8]) {
    for i in 0..4 {
        serial::write_usize(ip[i] as usize);
        if i < 3 { serial::write_byte(b'.'); }
    }
}
