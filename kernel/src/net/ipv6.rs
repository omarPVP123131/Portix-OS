// kernel/src/net/ipv6.rs — Pila de Red IPv6 Avanzada: Enrutamiento Dinámico LPM, Fragmentación/Reensamblaje y Raw Sockets (100% Rust no_std)
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use crate::arch::Spinlock;

pub const ETHERTYPE_IPV6: u16 = 0x86DD;
pub const IPV6_HEADER_LEN: usize = 40;
pub const IPV6_MIN_MTU: usize = 1280;

pub const NEXT_HEADER_HOP_BY_HOP: u8 = 0;
pub const NEXT_HEADER_TCP: u8        = 6;
pub const NEXT_HEADER_UDP: u8        = 17;
pub const NEXT_HEADER_ROUTING: u8    = 43;
pub const NEXT_HEADER_FRAGMENT: u8   = 44;
pub const NEXT_HEADER_ICMPV6: u8     = 58;
pub const NEXT_HEADER_NO_NEXT: u8    = 59;
pub const NEXT_HEADER_DEST_OPTS: u8  = 60;
pub const NEXT_HEADER_RAW: u8        = 255;

// Tipos ICMPv6
pub const ICMPV6_DEST_UNREACHABLE: u8    = 1;
pub const ICMPV6_PACKET_TOO_BIG: u8      = 2;
pub const ICMPV6_TIME_EXCEEDED: u8       = 3;
pub const ICMPV6_PARAMETER_PROBLEM: u8   = 4;
pub const ICMPV6_ECHO_REQUEST: u8        = 128;
pub const ICMPV6_ECHO_REPLY: u8          = 129;
pub const ICMPV6_ROUTER_SOLICIT: u8      = 133;
pub const ICMPV6_ROUTER_ADVERT: u8       = 134;
pub const ICMPV6_NEIGHBOR_SOLICIT: u8    = 135;
pub const ICMPV6_NEIGHBOR_ADVERT: u8     = 136;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ipv6Addr(pub [u8; 16]);

impl Ipv6Addr {
    pub const UNSPECIFIED: Self = Ipv6Addr([0; 16]);
    pub const LOOPBACK: Self = Ipv6Addr([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

    pub fn is_loopback(&self) -> bool {
        *self == Self::LOOPBACK
    }

    pub fn is_multicast(&self) -> bool {
        self.0[0] == 0xFF
    }

    pub fn is_link_local(&self) -> bool {
        self.0[0] == 0xFE && (self.0[1] & 0xC0) == 0x80
    }

    /// Comprueba si la dirección coincide con una subred de prefijo de longitud arbitraria (0..128)
    pub fn matches_prefix(&self, prefix: &Ipv6Addr, prefix_len: u8) -> bool {
        if prefix_len == 0 {
            return true;
        }
        if prefix_len > 128 {
            return false;
        }

        let full_bytes = (prefix_len / 8) as usize;
        let rem_bits = prefix_len % 8;

        if self.0[..full_bytes] != prefix.0[..full_bytes] {
            return false;
        }

        if rem_bits > 0 {
            let mask = !((1u8 << (8 - rem_bits)) - 1);
            if (self.0[full_bytes] & mask) != (prefix.0[full_bytes] & mask) {
                return false;
            }
        }

        true
    }

    /// Construye una dirección de enlace local EUI-64 a partir de una dirección MAC de 48 bits
    pub fn from_mac_link_local(mac: &[u8; 6]) -> Self {
        let mut ip = [0u8; 16];
        ip[0] = 0xFE;
        ip[1] = 0x80;
        ip[8] = mac[0] ^ 0x02; // Invert universal/local bit
        ip[9] = mac[1];
        ip[10] = mac[2];
        ip[11] = 0xFF;
        ip[12] = 0xFE;
        ip[13] = mac[3];
        ip[14] = mac[4];
        ip[15] = mac[5];
        Ipv6Addr(ip)
    }

    /// Devuelve la dirección de multidifusión de nodo solicitado (Solicited-Node Multicast Address)
    pub fn to_solicited_node_multicast(&self) -> Self {
        let mut mcast = [0u8; 16];
        mcast[0] = 0xFF;
        mcast[1] = 0x02;
        mcast[11] = 0x01;
        mcast[12] = 0xFF;
        mcast[13] = self.0[13];
        mcast[14] = self.0[14];
        mcast[15] = self.0[15];
        Ipv6Addr(mcast)
    }

    /// Formatea la dirección IPv6 en representación estándar en cadena
    pub fn to_string(&self) -> String {
        let mut s = String::new();
        let words: [u16; 8] = [
            u16::from_be_bytes([self.0[0], self.0[1]]),
            u16::from_be_bytes([self.0[2], self.0[3]]),
            u16::from_be_bytes([self.0[4], self.0[5]]),
            u16::from_be_bytes([self.0[6], self.0[7]]),
            u16::from_be_bytes([self.0[8], self.0[9]]),
            u16::from_be_bytes([self.0[10], self.0[11]]),
            u16::from_be_bytes([self.0[12], self.0[13]]),
            u16::from_be_bytes([self.0[14], self.0[15]]),
        ];

        // RFC 5952: se comprime con "::" la racha más larga de ceros (≥2 grupos)
        let (mut best_i, mut best_n, mut cur_i, mut cur_n) = (usize::MAX, 0usize, 0usize, 0usize);
        for i in 0..8 {
            if words[i] == 0 { if cur_n == 0 { cur_i = i; } cur_n += 1; if cur_n > best_n { best_n = cur_n; best_i = cur_i; } } else { cur_n = 0; }
        }
        if best_n < 2 { best_i = usize::MAX; }
        let hex_chars = b"0123456789abcdef";
        let mut i = 0;
        while i < 8 {
            if i == best_i { s.push_str("::"); i += best_n; continue; }
            if i > 0 && !(i == best_i + best_n && best_i != usize::MAX) { s.push(':'); }
            let val = words[i];
            if val == 0 { s.push('0'); } else {
                let mut started = false;
                for shift in (0..4).rev() {
                    let nibble = ((val >> (shift * 4)) & 0xF) as usize;
                    if nibble != 0 || started { started = true; s.push(hex_chars[nibble] as char); }
                }
            }
            i += 1;
        }
        s
    }
}

// Cabecera Fija IPv6 (40 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ipv6Header {
    pub ver_tc_fl: u32,     // Version (4b), Traffic Class (8b), Flow Label (20b)
    pub payload_len: u16,
    pub next_header: u8,
    pub hop_limit: u8,
    pub src_addr: [u8; 16],
    pub dst_addr: [u8; 16],
}

impl Ipv6Header {
    pub fn new(src: Ipv6Addr, dst: Ipv6Addr, next_hdr: u8, payload_len: u16) -> Self {
        Ipv6Header {
            ver_tc_fl: (6u32 << 28).to_be(), // Version 6
            payload_len: payload_len.to_be(),
            next_header: next_hdr,
            hop_limit: 64,
            src_addr: src.0,
            dst_addr: dst.0,
        }
    }

    pub fn version(&self) -> u8 {
        ((u32::from_be(self.ver_tc_fl) >> 28) & 0xF) as u8
    }
}

// Cabecera de Fragmentación IPv6 (RFC 8200 - 8 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ipv6FragmentHeader {
    pub next_header: u8,
    pub reserved: u8,
    pub offset_and_flags: u16, // Bits 15:3 Offset en unidades de 8B, Bit 0 = More Fragments
    pub identification: u32,
}

impl Ipv6FragmentHeader {
    pub fn new(next_hdr: u8, offset_bytes: usize, more: bool, id: u32) -> Self {
        let units = ((offset_bytes / 8) & 0x1FFF) as u16;
        let mut flags = units << 3;
        if more {
            flags |= 0x0001;
        }
        Self {
            next_header: next_hdr,
            reserved: 0,
            offset_and_flags: flags.to_be(),
            identification: id.to_be(),
        }
    }

    pub fn offset_bytes(&self) -> usize {
        let flags = u16::from_be(self.offset_and_flags);
        ((flags >> 3) as usize) * 8
    }

    pub fn has_more_fragments(&self) -> bool {
        (u16::from_be(self.offset_and_flags) & 0x0001) != 0
    }

    pub fn id(&self) -> u32 {
        u32::from_be(self.identification)
    }
}

// ── Neighbor Discovery Protocol (NDP) & ICMPv6 ──────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NdpState {
    Incomplete,
    Reachable,
    Stale,
    Delay,
    Probe,
}

#[derive(Clone, Debug)]
pub struct NdpEntry {
    pub ip: Ipv6Addr,
    pub mac: [u8; 6],
    pub state: NdpState,
    pub updated_ticks: u64,
}

#[derive(Clone, Debug)]
pub struct Ipv6Route {
    pub prefix: Ipv6Addr,
    pub prefix_len: u8,
    pub gateway: Option<Ipv6Addr>,
    pub metric: u32,
    pub is_dynamic: bool,
    pub lifetime_secs: u32,
}

#[derive(Clone, Debug)]
pub struct FragmentChunk {
    pub offset: usize,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct PendingReassembly {
    pub src: Ipv6Addr,
    pub dst: Ipv6Addr,
    pub ident: u32,
    pub next_header: u8,
    pub chunks: Vec<FragmentChunk>,
    pub total_len: Option<usize>,
    pub ticks: u64,
}

#[derive(Clone, Debug)]
pub struct Ipv6RawSocket {
    pub id: u32,
    pub protocol: u8,
    pub rx_queue: Vec<Vec<u8>>,
    pub filter_src: Option<Ipv6Addr>,
    pub packets_sent: u64,
    pub packets_recv: u64,
}

pub struct Ipv6State {
    pub enabled: bool,
    pub link_local: Ipv6Addr,
    pub global_addr: Option<Ipv6Addr>,
    pub ndp_cache: Vec<NdpEntry>,
    pub routing_table: Vec<Ipv6Route>,
    pub pending_reassemblies: Vec<PendingReassembly>,
    pub raw_sockets: Vec<Ipv6RawSocket>,
    pub next_raw_socket_id: u32,
    pub next_frag_ident: u32,
    pub packets_rx: u64,
    pub packets_tx: u64,
    pub fragments_rx: u64,
    pub packets_reassembled: u64,
    // ── [IPv6 REAL] Aprendido por Router Advertisement (RFC 4861/4862) ──
    /// Encaminador por defecto (dirección de enlace del emisor del RA).
    pub router: Option<Ipv6Addr>,
    /// Longitud del prefijo global anunciado (SLAAC).
    pub prefix_len: u8,
    /// Servidor DNS recursivo anunciado (opción RDNSS, RFC 8106).
    pub dns: Option<Ipv6Addr>,
    /// Router Solicitations enviadas y RAs recibidos.
    pub rs_sent: u32,
    pub ra_rx: u32,
    pub ns_rx: u32,
    pub na_tx: u32,
    pub echo_rx: u32,
    // ── [DAD] Detección de direcciones duplicadas (RFC 4862 §5.4) ──
    /// Dirección global tentativa: no se usa hasta que pase la DAD.
    pub tentative: Option<Ipv6Addr>,
    pub dad_started: u64,
    /// 0 = sin DAD, 1 = en curso, 2 = verificada, 3 = duplicada
    pub dad_status: u8,
    pub dad_ns_tx: u32,
    pub dad_dup: u32,
    pub dad_elapsed_ms: u64,
    /// DAD de la dirección de enlace (RFC 4862 §5.4: también obligatoria para fe80::/64).
    /// 0 = no realizada, 1 = tentativa (en curso), 2 = verificada, 3 = DUPLICADA (IPv6 deshabilitado).
    pub ll_dad_status: u8,
    pub ll_dad_started: u64,
    pub ll_dad_elapsed_ms: u64,
    /// La RS se pospone hasta que la de enlace sea preferida (no se emite con origen tentativo).
    pub rs_pending: bool,
}

/// Plazo de la DAD: 1 NS y 1000 ms de escucha (RetransTimer por defecto, RFC 4861 §10).
pub const DAD_TICKS: u64 = 100;

impl Ipv6State {
    pub const fn new() -> Self {
        Ipv6State {
            enabled: true,
            link_local: Ipv6Addr([
                0xFE, 0x80, 0, 0, 0, 0, 0, 0,
                0x02, 0x1A, 0x4A, 0xFF, 0xFE, 0x16, 0x01, 0x64
            ]),
            global_addr: None,
            ndp_cache: Vec::new(),
            routing_table: Vec::new(),
            pending_reassemblies: Vec::new(),
            raw_sockets: Vec::new(),
            next_raw_socket_id: 1,
            next_frag_ident: 0x1000,
            packets_rx: 0,
            packets_tx: 0,
            fragments_rx: 0,
            packets_reassembled: 0,
            router: None,
            prefix_len: 0,
            dns: None,
            rs_sent: 0,
            ra_rx: 0,
            ns_rx: 0,
            na_tx: 0,
            echo_rx: 0,
            tentative: None, dad_started: 0, dad_status: 0, dad_ns_tx: 0, dad_dup: 0, dad_elapsed_ms: 0, ll_dad_status: 0, ll_dad_started: 0, ll_dad_elapsed_ms: 0, rs_pending: false,
        }
    }
}

pub static IPV6_STATE: Spinlock<Ipv6State> = Spinlock::new(Ipv6State::new());

pub fn ndp_insert(ip: Ipv6Addr, mac: [u8; 6]) {
    let mut state = IPV6_STATE.lock();
    if let Some(entry) = state.ndp_cache.iter_mut().find(|e| e.ip == ip) {
        entry.mac = mac;
        entry.state = NdpState::Reachable;
        entry.updated_ticks += 1;
    } else {
        state.ndp_cache.push(NdpEntry {
            ip,
            mac,
            state: NdpState::Reachable,
            updated_ticks: 1,
        });
    }
}

pub fn ndp_lookup(ip: &Ipv6Addr) -> Option<[u8; 6]> {
    let state = IPV6_STATE.lock();
    state.ndp_cache.iter().find(|e| e.ip == *ip).map(|e| e.mac)
}

/// Añade una ruta estática a la tabla de enrutamiento
pub fn add_route(prefix: Ipv6Addr, prefix_len: u8, gateway: Option<Ipv6Addr>, metric: u32) {
    let mut state = IPV6_STATE.lock();
    state.routing_table.push(Ipv6Route {
        prefix,
        prefix_len,
        gateway,
        metric,
        is_dynamic: false,
        lifetime_secs: 0,
    });
}

/// Añade o actualiza una ruta dinámica (ej. recibida por Router Advertisements RA)
pub fn add_dynamic_route(prefix: Ipv6Addr, prefix_len: u8, gateway: Option<Ipv6Addr>, metric: u32, lifetime: u32) {
    let mut state = IPV6_STATE.lock();
    if let Some(r) = state.routing_table.iter_mut().find(|r| r.prefix == prefix && r.prefix_len == prefix_len) {
        r.gateway = gateway;
        r.metric = metric;
        r.is_dynamic = true;
        r.lifetime_secs = lifetime;
    } else {
        state.routing_table.push(Ipv6Route {
            prefix,
            prefix_len,
            gateway,
            metric,
            is_dynamic: true,
            lifetime_secs: lifetime,
        });
    }
}

/// Búsqueda de ruta por coincidencia de prefijo más largo (Longest Prefix Match - LPM)
pub fn lookup_route_lpm(dst: &Ipv6Addr) -> Option<Ipv6Route> {
    let state = IPV6_STATE.lock();
    let mut best_match: Option<Ipv6Route> = None;

    for route in &state.routing_table {
        if dst.matches_prefix(&route.prefix, route.prefix_len) {
            match &best_match {
                None => {
                    best_match = Some(route.clone());
                },
                Some(best) => {
                    if route.prefix_len > best.prefix_len ||
                       (route.prefix_len == best.prefix_len && route.metric < best.metric) {
                        best_match = Some(route.clone());
                    }
                }
            }
        }
    }
    best_match
}

/// Fragmenta un paquete IPv6 grande en datagramas respetando el MTU
pub fn fragment_ipv6_packet(src: Ipv6Addr, dst: Ipv6Addr, next_hdr: u8, payload: &[u8], mtu: usize) -> Vec<Vec<u8>> {
    let mut state = IPV6_STATE.lock();
    let ident = state.next_frag_ident;
    state.next_frag_ident = state.next_frag_ident.wrapping_add(1);
    drop(state);

    let max_frag_payload = ((mtu.saturating_sub(IPV6_HEADER_LEN + 8)) / 8) * 8;
    if max_frag_payload == 0 {
        return Vec::new();
    }

    let mut fragments = Vec::new();
    let mut offset = 0usize;

    while offset < payload.len() {
        let remaining = payload.len() - offset;
        let chunk_size = remaining.min(max_frag_payload);
        let more = (offset + chunk_size) < payload.len();

        let mut pkt = Vec::with_capacity(IPV6_HEADER_LEN + 8 + chunk_size);
        
        // Cabecera IPv6 con NextHeader = Fragment (44)
        let ip_hdr = Ipv6Header::new(src, dst, NEXT_HEADER_FRAGMENT, (8 + chunk_size) as u16);
        let ip_bytes: [u8; IPV6_HEADER_LEN] = unsafe { core::mem::transmute(ip_hdr) };
        pkt.extend_from_slice(&ip_bytes);

        // Cabecera de Fragmento (8 bytes)
        let frag_hdr = Ipv6FragmentHeader::new(next_hdr, offset, more, ident);
        let frag_bytes: [u8; 8] = unsafe { core::mem::transmute(frag_hdr) };
        pkt.extend_from_slice(&frag_bytes);

        // Datos del fragmento
        pkt.extend_from_slice(&payload[offset..offset + chunk_size]);
        fragments.push(pkt);

        offset += chunk_size;
    }

    fragments
}

/// Procesa un fragmento recibido y lo une al reensamblador. Retorna Some(payload) si se completó.
pub fn process_incoming_fragment(src: Ipv6Addr, dst: Ipv6Addr, frag_hdr: &Ipv6FragmentHeader, chunk: &[u8]) -> Option<(u8, Vec<u8>)> {
    let mut state = IPV6_STATE.lock();
    state.fragments_rx += 1;
    let ident = frag_hdr.id();
    let offset = frag_hdr.offset_bytes();
    let more = frag_hdr.has_more_fragments();
    let next_proto = frag_hdr.next_header;

    let pos = state.pending_reassemblies.iter().position(|r| r.src == src && r.dst == dst && r.ident == ident);
    let r_idx = match pos {
        Some(i) => i,
        None => {
            state.pending_reassemblies.push(PendingReassembly {
                src,
                dst,
                ident,
                next_header: next_proto,
                chunks: Vec::new(),
                total_len: None,
                ticks: 0,
            });
            state.pending_reassemblies.len() - 1
        }
    };

    let r = &mut state.pending_reassemblies[r_idx];
    r.chunks.push(FragmentChunk {
        offset,
        data: chunk.to_vec(),
    });

    if !more {
        r.total_len = Some(offset + chunk.len());
    }

    // Comprobar si todos los datos están presentes contiguos desde 0 hasta total_len
    if let Some(total) = r.total_len {
        let mut full_buf = Vec::new();
        full_buf.resize(total, 0u8);
        let mut filled = vec![false; total];

        for c in &r.chunks {
            let end = (c.offset + c.data.len()).min(total);
            full_buf[c.offset..end].copy_from_slice(&c.data[..end - c.offset]);
            for i in c.offset..end {
                filled[i] = true;
            }
        }

        if filled.iter().all(|&f| f) {
            let next_h = r.next_header;
            state.pending_reassemblies.remove(r_idx);
            state.packets_reassembled += 1;
            return Some((next_h, full_buf));
        }
    }

    None
}

// ── Sockets Raw IPv6 (SOCK_RAW) ─────────────────────────────────────────────

/// Abre un socket raw para capturar e inyectar datagramas IPv6 de un protocolo específico
pub fn raw_socket_open(protocol: u8) -> u32 {
    let mut state = IPV6_STATE.lock();
    let id = state.next_raw_socket_id;
    state.next_raw_socket_id += 1;

    state.raw_sockets.push(Ipv6RawSocket {
        id,
        protocol,
        rx_queue: Vec::new(),
        filter_src: None,
        packets_sent: 0,
        packets_recv: 0,
    });

    id
}

/// Envía datos directos a través del socket raw
pub fn raw_socket_send(sock_id: u32, dst: Ipv6Addr, payload: &[u8]) -> Result<usize, &'static str> {
    let mut state = IPV6_STATE.lock();
    let proto = match state.raw_sockets.iter().find(|s| s.id == sock_id) {
        Some(s) => s.protocol,
        None => return Err("Socket raw no encontrado"),
    };

    let src = state.global_addr.unwrap_or(state.link_local);
    state.packets_tx += 1;
    if let Some(s) = state.raw_sockets.iter_mut().find(|s| s.id == sock_id) {
        s.packets_sent += 1;
    }
    drop(state);

    let _ = (src, dst, proto, payload);
    Ok(payload.len())
}

/// Lee un datagrama disponible en la cola del socket raw
pub fn raw_socket_recv(sock_id: u32) -> Option<Vec<u8>> {
    let mut state = IPV6_STATE.lock();
    let sock = state.raw_sockets.iter_mut().find(|s| s.id == sock_id)?;
    if !sock.rx_queue.is_empty() {
        sock.packets_recv += 1;
        Some(sock.rx_queue.remove(0))
    } else {
        None
    }
}

/// Distribuye un paquete entrante hacia los sockets raw registrados para ese protocolo
pub fn raw_socket_dispatch(protocol: u8, src: Ipv6Addr, packet: &[u8]) {
    let mut state = IPV6_STATE.lock();
    for sock in state.raw_sockets.iter_mut() {
        if sock.protocol == NEXT_HEADER_RAW || sock.protocol == protocol {
            if let Some(f_src) = sock.filter_src {
                if f_src != src { continue; }
            }
            if sock.rx_queue.len() < 64 {
                sock.rx_queue.push(packet.to_vec());
                sock.packets_recv += 1;
            }
        }
    }
}

/// [IPv6 REAL] Arranque: dirección de enlace EUI-64 derivada de la MAC REAL
/// del e1000; nada de direcciones globales, rutas ni vecinos inventados. La
/// dirección global, el encaminador y el DNS se aprenden del Router
/// Advertisement que responde a nuestra Router Solicitation (RFC 4861 §6.3.7).
pub fn ipv6_init() {
    if !crate::drivers::net::e1000::is_active() { return; }
    let mac = crate::drivers::net::e1000::get_mac();
    let ll = Ipv6Addr::from_mac_link_local(&mac);
    {
        let mut state = IPV6_STATE.lock();
        state.link_local = ll;
        state.global_addr = None;
        state.ndp_cache.clear();
        state.routing_table.clear();
        // Ruta de enlace fe80::/64 (siempre presente en un nodo IPv6)
        state.routing_table.push(Ipv6Route { prefix: Ipv6Addr([0xFE, 0x80, 0,0,0,0,0,0, 0,0,0,0,0,0,0,0]), prefix_len: 64, gateway: None, metric: 256, is_dynamic: false, lifetime_secs: 0 });
    }
    let _ = raw_socket_open(NEXT_HEADER_ICMPV6);
    // DAD de la dirección de enlace antes de usarla como origen (RFC 4862 §5.4):
    // mientras sea tentativa no responde a NS normales ni emite la RS.
    { let mut st = IPV6_STATE.lock(); st.ll_dad_status = 1; st.ll_dad_started = crate::time::pit::ticks(); st.rs_pending = true; }
    if send_dad_solicitation(&ll).is_err() {
        // Sin poder emitir el NS no hay DAD posible: se usa la dirección sin verificar (y se dice).
        let mut st = IPV6_STATE.lock(); st.ll_dad_status = 0; st.rs_pending = false; drop(st);
        let _ = send_router_solicitation();
    }
}

/// ¿La dirección de enlace sigue tentativa (o quedó duplicada)?
pub fn link_local_tentative() -> bool { let st = IPV6_STATE.lock(); st.ll_dad_status == 1 || st.ll_dad_status == 3 }

// ─────────────────────────────────────────────────────────────────────────
// [IPv6 REAL] Transmisión, NDP, ICMPv6 y SLAAC sobre el e1000
// ─────────────────────────────────────────────────────────────────────────

pub const ALL_NODES: Ipv6Addr = Ipv6Addr([0xFF, 0x02, 0,0,0,0,0,0, 0,0,0,0,0,0,0,1]);
pub const ALL_ROUTERS: Ipv6Addr = Ipv6Addr([0xFF, 0x02, 0,0,0,0,0,0, 0,0,0,0,0,0,0,2]);

/// Suma de comprobación ICMPv6/UDP/TCP con pseudo-cabecera IPv6 (RFC 8200 §8.1).
pub fn checksum_v6(src: &Ipv6Addr, dst: &Ipv6Addr, next_hdr: u8, payload: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut add = |b: &[u8]| { let mut i = 0; while i + 1 < b.len() { sum += ((b[i] as u32) << 8) | b[i + 1] as u32; i += 2; } if i < b.len() { sum += (b[i] as u32) << 8; } };
    add(&src.0); add(&dst.0);
    add(&(payload.len() as u32).to_be_bytes());
    add(&[0, 0, 0, next_hdr]);
    add(payload);
    while (sum >> 16) != 0 { sum = (sum & 0xFFFF) + (sum >> 16); }
    !(sum as u16)
}

/// MAC de multidifusión IPv6: 33:33 + 32 bits bajos (RFC 2464 §7).
fn multicast_mac(dst: &Ipv6Addr) -> [u8; 6] { [0x33, 0x33, dst.0[12], dst.0[13], dst.0[14], dst.0[15]] }

/// Dirección de origen adecuada: global si el destino no es de enlace/multicast local y la tenemos.
pub fn pick_source(dst: &Ipv6Addr) -> Ipv6Addr {
    if dst.is_loopback() { return Ipv6Addr::LOOPBACK; }
    let st = IPV6_STATE.lock();
    if dst.is_link_local() || (dst.is_multicast() && dst.0[1] & 0x0F <= 2) { return st.link_local; }
    st.global_addr.unwrap_or(st.link_local)
}

/// Garantiza la autoconfiguración (RA procesado) antes de usar IPv6: si aún no
/// hay encaminador, reenvía la Router Solicitation y procesa la red hasta
/// `timeout_ticks`. Devuelve true si hay dirección global/encaminador.
pub fn ensure_configured(timeout_ticks: u64) -> bool {
    if !crate::drivers::net::e1000::is_active() { return false; }
    let t0 = crate::time::pit::ticks();
    let mut solicitado = false;
    loop {
        super::poll();
        { let st = IPV6_STATE.lock(); if st.router.is_some() && st.tentative.is_none() { return true; } }
        if !solicitado && !link_local_tentative() { let _ = send_router_solicitation(); solicitado = true; }
        // la DAD añade hasta 1 s tras el RA: se concede ese margen
        if crate::time::pit::ticks().wrapping_sub(t0) > timeout_ticks + 2 * DAD_TICKS + 10 { return false; }
        crate::process::scheduler::wait_hlt_preemptible();
    }
}

/// Siguiente salto: vecino directo si es de enlace, multicast o está en nuestro prefijo; si no, el encaminador.
fn next_hop_v6(dst: &Ipv6Addr) -> Option<Ipv6Addr> {
    if dst.is_multicast() || dst.is_link_local() { return Some(*dst); }
    let st = IPV6_STATE.lock();
    if let Some(g) = st.global_addr { if st.prefix_len > 0 && dst.matches_prefix(&g, st.prefix_len) { return Some(*dst); } }
    st.router
}

/// Envía una trama IPv6 ya resuelta (MAC destino conocida).
/// ¿Destino local (::1 o una de nuestras direcciones unicast)?
pub fn is_local_v6(a: &Ipv6Addr) -> bool {
    if a.is_loopback() { return true; }
    let st = IPV6_STATE.lock();
    *a == st.link_local || st.global_addr == Some(*a)
}

fn emit_ipv6(dst_mac: [u8; 6], src: &Ipv6Addr, dst: &Ipv6Addr, next_hdr: u8, hop_limit: u8, payload: &[u8]) -> Result<(), &'static str> {
    if 14 + IPV6_HEADER_LEN + payload.len() > 1514 { return Err("paquete IPv6 demasiado grande"); }
    if !dst.is_loopback() {
        let st = IPV6_STATE.lock();
        if st.ll_dad_status == 3 { return Err("IPv6 deshabilitado: direccion de enlace duplicada"); }
        if st.ll_dad_status == 1 && *src == st.link_local { return Err("direccion de enlace tentativa: aun no puede usarse como origen"); }
    }
    let my_mac = crate::drivers::net::e1000::get_mac();
    if is_local_v6(dst) {
        // [LOOPBACK] la trama no sale por el e1000: se encola para `poll()`
        let mut f = Vec::with_capacity(14 + IPV6_HEADER_LEN + payload.len());
        f.extend_from_slice(&my_mac); f.extend_from_slice(&my_mac); f.extend_from_slice(&ETHERTYPE_IPV6.to_be_bytes());
        f.extend_from_slice(&[0x60, 0, 0, 0]); f.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        f.push(next_hdr); f.push(hop_limit); f.extend_from_slice(&src.0); f.extend_from_slice(&dst.0); f.extend_from_slice(payload);
        return super::loopback_enqueue(&f);
    }
    let mut f = Vec::with_capacity(14 + IPV6_HEADER_LEN + payload.len());
    f.extend_from_slice(&dst_mac); f.extend_from_slice(&my_mac); f.extend_from_slice(&ETHERTYPE_IPV6.to_be_bytes());
    f.extend_from_slice(&[0x60, 0, 0, 0]);
    f.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    f.push(next_hdr); f.push(hop_limit);
    f.extend_from_slice(&src.0); f.extend_from_slice(&dst.0);
    f.extend_from_slice(payload);
    crate::drivers::net::e1000::send_packet(&f)?;
    IPV6_STATE.lock().packets_tx += 1;
    Ok(())
}

/// Neighbor Solicitation (RFC 4861 §4.3) hacia la multidifusión de nodo solicitado.
fn send_neighbor_solicitation(target: &Ipv6Addr) -> Result<(), &'static str> {
    let my_mac = crate::drivers::net::e1000::get_mac();
    let src = pick_source(target);
    let dst = target.to_solicited_node_multicast();
    let mut p = Vec::with_capacity(32);
    p.extend_from_slice(&[ICMPV6_NEIGHBOR_SOLICIT, 0, 0, 0, 0, 0, 0, 0]);
    p.extend_from_slice(&target.0);
    p.push(1); p.push(1); p.extend_from_slice(&my_mac); // opción Source Link-Layer Address
    let ck = checksum_v6(&src, &dst, NEXT_HEADER_ICMPV6, &p); p[2] = (ck >> 8) as u8; p[3] = ck as u8;
    emit_ipv6(multicast_mac(&dst), &src, &dst, NEXT_HEADER_ICMPV6, 255, &p)
}

/// NS de DAD (RFC 4862 §5.4.2): origen :: (no especificado), destino el
/// multicast de nodo solicitado de la dirección tentativa, SIN opción SLLA.
fn send_dad_solicitation(target: &Ipv6Addr) -> Result<(), &'static str> {
    let dst = target.to_solicited_node_multicast();
    let mut p = Vec::with_capacity(24);
    p.extend_from_slice(&[ICMPV6_NEIGHBOR_SOLICIT, 0, 0, 0, 0, 0, 0, 0]);
    p.extend_from_slice(&target.0);
    let ck = checksum_v6(&Ipv6Addr::UNSPECIFIED, &dst, NEXT_HEADER_ICMPV6, &p); p[2] = (ck >> 8) as u8; p[3] = ck as u8;
    let r = emit_ipv6(multicast_mac(&dst), &Ipv6Addr::UNSPECIFIED, &dst, NEXT_HEADER_ICMPV6, 255, &p);
    if r.is_ok() {
        IPV6_STATE.lock().dad_ns_tx += 1;
        crate::drivers::serial::write_str("[IPV6-DAD] Neighbor Solicitation (origen ::) por "); crate::drivers::serial::write_str(&target.to_string());
        crate::drivers::serial::write_str(" -> "); crate::drivers::serial::write_str(&dst.to_string()); crate::drivers::serial::write_str("; escuchando 1000 ms\n");
    }
    r
}

fn dad_duplicate(target: &Ipv6Addr, motivo: &str) {
    let mut st = IPV6_STATE.lock();
    if st.ll_dad_status == 1 && *target == st.link_local {
        // RFC 4862 §5.4.5: dirección de enlace duplicada → IPv6 deshabilitado en la interfaz.
        st.ll_dad_status = 3; st.dad_dup += 1; st.rs_pending = false;
        st.ll_dad_elapsed_ms = crate::time::pit::ticks().wrapping_sub(st.ll_dad_started) * 10;
        drop(st);
        crate::drivers::serial::write_str("[IPV6-DAD] direccion de ENLACE DUPLICADA "); crate::drivers::serial::write_str(&target.to_string());
        crate::drivers::serial::write_str(" ("); crate::drivers::serial::write_str(motivo); crate::drivers::serial::write_str("): IPv6 deshabilitado en la interfaz (RFC 4862 5.4.5)\n");
        return;
    }
    st.tentative = None; st.dad_status = 3; st.dad_dup += 1;
    st.dad_elapsed_ms = crate::time::pit::ticks().wrapping_sub(st.dad_started) * 10;
    drop(st);
    crate::drivers::serial::write_str("[IPV6-DAD] direccion DUPLICADA "); crate::drivers::serial::write_str(&target.to_string());
    crate::drivers::serial::write_str(" ("); crate::drivers::serial::write_str(motivo); crate::drivers::serial::write_str("): descartada, no se configura\n");
}

/// Avance de la DAD: la llama `net::poll()`. Si pasó el plazo sin NA/NS
/// conflictivos, la tentativa pasa a ser la dirección global preferida.
pub fn dad_tick() {
    // 1) DAD de la dirección de enlace
    let ll_done = {
        let mut st = IPV6_STATE.lock();
        if st.ll_dad_status == 1 && crate::time::pit::ticks().wrapping_sub(st.ll_dad_started) >= DAD_TICKS {
            st.ll_dad_status = 2; st.ll_dad_elapsed_ms = crate::time::pit::ticks().wrapping_sub(st.ll_dad_started) * 10;
            let rs = st.rs_pending; st.rs_pending = false;
            Some((st.link_local, st.ll_dad_elapsed_ms, rs))
        } else { None }
    };
    if let Some((ll, ms, rs)) = ll_done {
        crate::drivers::serial::write_str("[IPV6-DAD] enlace "); crate::drivers::serial::write_str(&ll.to_string());
        crate::drivers::serial::write_str(" verificada: sin respuesta al NS en "); crate::drivers::serial::write_usize(ms as usize);
        crate::drivers::serial::write_str(" ms; pasa a preferida y se emite la Router Solicitation\n");
        if rs { let _ = send_router_solicitation(); }
    }
    // 2) DAD de la dirección global (SLAAC)
    let done = {
        let mut st = IPV6_STATE.lock();
        match st.tentative {
            Some(g) if crate::time::pit::ticks().wrapping_sub(st.dad_started) >= DAD_TICKS => {
                st.tentative = None; st.global_addr = Some(g); st.dad_status = 2;
                st.dad_elapsed_ms = crate::time::pit::ticks().wrapping_sub(st.dad_started) * 10;
                Some((g, st.dad_elapsed_ms))
            }
            _ => None,
        }
    };
    if let Some((g, ms)) = done {
        crate::drivers::serial::write_str("[IPV6-DAD] "); crate::drivers::serial::write_str(&g.to_string());
        crate::drivers::serial::write_str(" verificada: sin respuesta al NS en "); crate::drivers::serial::write_usize(ms as usize);
        crate::drivers::serial::write_str(" ms (sin duplicados); pasa a preferida\n");
    }
}

/// ¿DAD en curso?
pub fn dad_in_progress() -> bool { let st = IPV6_STATE.lock(); st.tentative.is_some() || st.ll_dad_status == 1 }

/// Router Solicitation (RFC 4861 §4.1) a ff02::2.
pub fn send_router_solicitation() -> Result<(), &'static str> {
    let my_mac = crate::drivers::net::e1000::get_mac();
    let src = IPV6_STATE.lock().link_local;
    let mut p = Vec::with_capacity(16);
    p.extend_from_slice(&[ICMPV6_ROUTER_SOLICIT, 0, 0, 0, 0, 0, 0, 0]);
    p.push(1); p.push(1); p.extend_from_slice(&my_mac);
    let ck = checksum_v6(&src, &ALL_ROUTERS, NEXT_HEADER_ICMPV6, &p); p[2] = (ck >> 8) as u8; p[3] = ck as u8;
    let r = emit_ipv6(multicast_mac(&ALL_ROUTERS), &src, &ALL_ROUTERS, NEXT_HEADER_ICMPV6, 255, &p);
    if r.is_ok() { IPV6_STATE.lock().rs_sent += 1; crate::drivers::serial::write_str("[IPV6] Router Solicitation enviada a ff02::2\n"); }
    r
}

/// Resuelve la MAC de un vecino por NDP (hasta 3 solicitudes, `timeout_ticks` cada una).
pub fn ndp_resolve(ip: &Ipv6Addr, timeout_ticks: u64) -> Option<[u8; 6]> {
    if ip.is_multicast() { return Some(multicast_mac(ip)); }
    if let Some(m) = ndp_lookup(ip) { return Some(m); }
    for _ in 0..3 {
        if send_neighbor_solicitation(ip).is_err() { return None; }
        let t0 = crate::time::pit::ticks();
        while crate::time::pit::ticks().wrapping_sub(t0) < timeout_ticks {
            // Sólo ARP/NDP: `send_tcp_segment` nos llama con TCP_TABLE tomado
            super::poll_arp_only();
            if let Some(m) = ndp_lookup(ip) { return Some(m); }
            unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
        }
    }
    None
}

/// Envía un datagrama IPv6 resolviendo el siguiente salto por NDP.
pub fn send_ipv6(dst: &Ipv6Addr, next_hdr: u8, payload: &[u8]) -> Result<(), &'static str> {
    if !crate::drivers::net::e1000::is_active() { return Err("e1000 inactivo"); }
    let src = pick_source(dst);
    if is_local_v6(dst) { return emit_ipv6([0; 6], &src, dst, next_hdr, 64, payload); }
    let hop = next_hop_v6(dst).ok_or("sin encaminador IPv6 (no se recibio Router Advertisement)")?;
    let mac = ndp_resolve(&hop, 50).ok_or("NDP sin resolver: el vecino no responde")?;
    emit_ipv6(mac, &src, dst, next_hdr, 64, payload)
}

/// Última respuesta echo ICMPv6: (origen, seq, bytes ICMP).
static LAST_ECHO6: Spinlock<Option<(Ipv6Addr, u16, usize)>> = Spinlock::new(None);
/// [T-026/IPv6] Último ICMPv6 "destino inalcanzable" que responde a un Echo
/// Request nuestro: (origen, seq, motivo). Linux avisa al `ping` en cuanto
/// llega; Portix esperaba el plazo entero (1 s por paquete) y el resumen
/// tardaba tanto que la suite lo daba por fallido aun siendo correcto.
static LAST_UNREACH6: Spinlock<Option<(Ipv6Addr, u16, u8)>> = Spinlock::new(None);

/// ping6: envía Echo Request y ESPERA la respuesta real; RTT por TSC.
pub fn ping6_rtt(dst: &Ipv6Addr, seq: u16, timeout_ticks: u64) -> Result<(usize, u64), &'static str> {
    if !dst.is_link_local() && !dst.is_multicast() { let _ = ensure_configured(100); }
    *LAST_ECHO6.lock() = None;
    *LAST_UNREACH6.lock() = None;
    let mut p = build_icmpv6_echo_request(0x5058, seq, b"PORTIX_PING6_ECHO_2026");
    let src = pick_source(dst);
    let ck = checksum_v6(&src, dst, NEXT_HEADER_ICMPV6, &p); p[2] = (ck >> 8) as u8; p[3] = ck as u8;
    let t0 = crate::time::pit::ticks();
    let tsc0 = unsafe { core::arch::x86_64::_rdtsc() };
    send_ipv6(dst, NEXT_HEADER_ICMPV6, &p)?;
    loop {
        super::poll();
        let got = { let mut g = LAST_ECHO6.lock(); match *g { Some((_, s, n)) if s == seq => { *g = None; Some(n) } _ => None } };
        // [T-026/IPv6] Un "destino inalcanzable" que cita NUESTRO seq corta la
        // espera en el acto y se informa con su motivo real (sin inventar RTT).
        let alcanzable = { let mut u = LAST_UNREACH6.lock(); match *u { Some((_, sq, motivo)) if sq == seq => { *u = None; Some(motivo) } _ => None } };
        if let Some(motivo) = alcanzable {
            return Err(match motivo {
                0 => "sin ruta al destino (ICMPv6)",
                1 => "prohibido administrativamente (ICMPv6)",
                3 => "direccion inalcanzable (ICMPv6)",
                4 => "puerto inalcanzable (ICMPv6)",
                _ => "destino inalcanzable (ICMPv6)",
            });
        }
        if let Some(n) = got {
            let tsc1 = unsafe { core::arch::x86_64::_rdtsc() };
            let hz = crate::time::pit::tsc_hz_estimate();
            let us = if hz > 0 { tsc1.wrapping_sub(tsc0) / (hz / 1_000_000).max(1) } else { crate::time::pit::ticks().wrapping_sub(t0) * 10_000 };
            return Ok((n, us));
        }
        if crate::time::pit::ticks().wrapping_sub(t0) > timeout_ticks { return Err("sin respuesta (tiempo agotado)"); }
        unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
    }
}

/// ¿Es `a` una de nuestras direcciones (unicast o multicast que escuchamos)?
fn is_mine(a: &Ipv6Addr) -> bool {
    if a.is_loopback() { return true; }
    let st = IPV6_STATE.lock();
    if *a == st.link_local || st.global_addr == Some(*a) || *a == ALL_NODES { return true; }
    if *a == st.link_local.to_solicited_node_multicast() { return true; }
    if let Some(g) = st.global_addr { if *a == g.to_solicited_node_multicast() { return true; } }
    false
}

/// Entrada de tramas 0x86DD desde la capa Ethernet.
pub fn handle_ipv6(pkt: &[u8], src_mac: &[u8]) {
    if pkt.len() < IPV6_HEADER_LEN || pkt[0] >> 4 != 6 { return; }
    let plen = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
    let next = pkt[6];
    let mut src = [0u8; 16]; src.copy_from_slice(&pkt[8..24]);
    let mut dst = [0u8; 16]; dst.copy_from_slice(&pkt[24..40]);
    let (src, dst) = (Ipv6Addr(src), Ipv6Addr(dst));
    if pkt.len() < IPV6_HEADER_LEN + plen { return; }
    let payload = &pkt[IPV6_HEADER_LEN..IPV6_HEADER_LEN + plen];
    if !is_mine(&dst) { return; }
    IPV6_STATE.lock().packets_rx += 1;
    let mut smac = [0u8; 6]; if src_mac.len() >= 6 { smac.copy_from_slice(&src_mac[..6]); }
    match next {
        NEXT_HEADER_ICMPV6 => handle_icmpv6(&src, &dst, payload, smac),
        NEXT_HEADER_TCP => {
            if checksum_v6(&src, &dst, NEXT_HEADER_TCP, payload) != 0 { return; }
            super::tcp::handle_tcp_packet(super::IpAddr::V6(src), super::IpAddr::V6(dst), payload);
        }
        NEXT_HEADER_UDP => { super::udp::handle_udp6(&src, &dst, payload); }
        _ => { raw_socket_dispatch(next, src, payload); }
    }
}

fn handle_icmpv6(src: &Ipv6Addr, dst: &Ipv6Addr, icmp: &[u8], src_mac: [u8; 6]) {
    if icmp.len() < 8 { return; }
    if checksum_v6(src, dst, NEXT_HEADER_ICMPV6, icmp) != 0 { return; } // suma inválida: se descarta
    let my_mac = crate::drivers::net::e1000::get_mac();
    match icmp[0] {
        ICMPV6_NEIGHBOR_SOLICIT => {
            if icmp.len() < 24 { return; }
            let mut t = [0u8; 16]; t.copy_from_slice(&icmp[8..24]); let target = Ipv6Addr(t);
            let mine = { let st = IPV6_STATE.lock(); (target == st.link_local && st.ll_dad_status == 2) || st.global_addr == Some(target) };
            if !mine {
                // DAD: otro nodo haciendo DAD por NUESTRA dirección tentativa → duplicado (RFC 4862 §5.4.3)
                let dup = { let st = IPV6_STATE.lock(); (st.tentative == Some(target) || (st.ll_dad_status == 1 && target == st.link_local)) && src.0 == [0u8; 16] };
                if dup { dad_duplicate(&target, "Neighbor Solicitation de otro nodo"); }
                return;
            }
            IPV6_STATE.lock().ns_rx += 1;
            if src.0 != [0u8; 16] { ndp_insert(*src, src_mac); }
            // Neighbor Advertisement (RFC 4861 §4.4): Solicited + Override, opción Target LLA
            let mut p = Vec::with_capacity(32);
            p.extend_from_slice(&[ICMPV6_NEIGHBOR_ADVERT, 0, 0, 0, 0x60, 0, 0, 0]);
            p.extend_from_slice(&target.0);
            p.push(2); p.push(1); p.extend_from_slice(&my_mac);
            let (rdst, rmac) = if src.0 == [0u8; 16] { (ALL_NODES, multicast_mac(&ALL_NODES)) } else { (*src, src_mac) };
            let ck = checksum_v6(&target, &rdst, NEXT_HEADER_ICMPV6, &p); p[2] = (ck >> 8) as u8; p[3] = ck as u8;
            if emit_ipv6(rmac, &target, &rdst, NEXT_HEADER_ICMPV6, 255, &p).is_ok() { IPV6_STATE.lock().na_tx += 1; }
        }
        ICMPV6_NEIGHBOR_ADVERT => {
            if icmp.len() < 24 { return; }
            let mut t = [0u8; 16]; t.copy_from_slice(&icmp[8..24]);
            { let st = IPV6_STATE.lock(); let dup = st.tentative == Some(Ipv6Addr(t)) || (st.ll_dad_status == 1 && st.link_local == Ipv6Addr(t)); drop(st); if dup { dad_duplicate(&Ipv6Addr(t), "Neighbor Advertisement"); return; } }
            let mut mac = src_mac; let mut i = 24;
            while i + 8 <= icmp.len() { let l = icmp[i + 1] as usize * 8; if l == 0 { break; } if icmp[i] == 2 && l >= 8 { mac.copy_from_slice(&icmp[i + 2..i + 8]); } i += l; }
            ndp_insert(Ipv6Addr(t), mac);
            crate::drivers::serial::write_str("[IPV6] NDP resuelto "); crate::drivers::serial::write_str(&Ipv6Addr(t).to_string());
            crate::drivers::serial::write_str(" -> "); super::print_mac(&mac); crate::drivers::serial::write_str("\n");
        }
        ICMPV6_ROUTER_ADVERT => {
            if icmp.len() < 16 { return; }
            let router_lifetime = u16::from_be_bytes([icmp[6], icmp[7]]);
            let mut i = 16; let mut prefix: Option<(Ipv6Addr, u8, u32)> = None; let mut dns: Option<Ipv6Addr> = None; let mut rmac = src_mac; let mut dad_pending: Option<Ipv6Addr> = None;
            while i + 8 <= icmp.len() {
                let (t, l) = (icmp[i], icmp[i + 1] as usize * 8); if l == 0 || i + l > icmp.len() { break; }
                match t {
                    1 => rmac.copy_from_slice(&icmp[i + 2..i + 8]),
                    3 if l >= 32 => { let plen = icmp[i + 2]; let flags = icmp[i + 3]; let valid = u32::from_be_bytes([icmp[i+4], icmp[i+5], icmp[i+6], icmp[i+7]]);
                        let mut pf = [0u8; 16]; pf.copy_from_slice(&icmp[i + 16..i + 32]);
                        if flags & 0x40 != 0 && plen == 64 { prefix = Some((Ipv6Addr(pf), plen, valid)); } }
                    25 if l >= 24 => { let mut d = [0u8; 16]; d.copy_from_slice(&icmp[i + 8..i + 24]); dns = Some(Ipv6Addr(d)); }
                    _ => {}
                }
                i += l;
            }
            ndp_insert(*src, rmac);
            let mut st = IPV6_STATE.lock();
            st.ra_rx += 1;
            if router_lifetime > 0 { st.router = Some(*src); st.routing_table.retain(|r| r.prefix_len != 0); st.routing_table.push(Ipv6Route { prefix: Ipv6Addr::UNSPECIFIED, prefix_len: 0, gateway: Some(*src), metric: 1024, is_dynamic: true, lifetime_secs: router_lifetime as u32 }); }
            if let Some((pf, plen, valid)) = prefix {
                // SLAAC (RFC 4862 §5.5.3): prefijo anunciado + identificador EUI-64 de la MAC real
                let mut g = pf.0; g[8..16].copy_from_slice(&st.link_local.0[8..16]);
                let g = Ipv6Addr(g);
                st.prefix_len = plen;
                if st.global_addr != Some(g) && st.tentative != Some(g) && !(st.dad_status == 3 && st.global_addr.is_none()) {
                    // La dirección queda TENTATIVA hasta superar la DAD (RFC 4862 §5.4):
                    // mientras tanto no se usa como origen ni se responde a NS por ella.
                    st.tentative = Some(g); st.dad_started = crate::time::pit::ticks(); st.dad_status = 1;
                    dad_pending = Some(g);
                }
                st.routing_table.retain(|r| !(r.prefix_len == plen && r.prefix == pf));
                st.routing_table.push(Ipv6Route { prefix: pf, prefix_len: plen, gateway: None, metric: 256, is_dynamic: true, lifetime_secs: valid });
            }
            if dns.is_some() { st.dns = dns; }
            let (g, r, d) = (dad_pending.or(st.global_addr).or(st.tentative), st.router, st.dns); drop(st);
            crate::drivers::serial::write_str("[IPV6] Router Advertisement de "); crate::drivers::serial::write_str(&src.to_string());
            if let Some(g) = g { crate::drivers::serial::write_str(": SLAAC "); crate::drivers::serial::write_str(&g.to_string()); crate::drivers::serial::write_str("/64"); }
            if dad_pending.is_some() { crate::drivers::serial::write_str(" (tentativa, DAD en curso)"); }
            if let Some(r) = r { crate::drivers::serial::write_str(" via "); crate::drivers::serial::write_str(&r.to_string()); }
            if let Some(d) = d { crate::drivers::serial::write_str(" dns "); crate::drivers::serial::write_str(&d.to_string()); }
            crate::drivers::serial::write_str("\n");
            if let Some(g) = dad_pending { let _ = send_dad_solicitation(&g); }
        }
        ICMPV6_ECHO_REQUEST => {
            IPV6_STATE.lock().echo_rx += 1;
            let mut p = icmp.to_vec(); p[0] = ICMPV6_ECHO_REPLY; p[2] = 0; p[3] = 0;
            let rsrc = if dst.is_multicast() { pick_source(src) } else { *dst };
            let ck = checksum_v6(&rsrc, src, NEXT_HEADER_ICMPV6, &p); p[2] = (ck >> 8) as u8; p[3] = ck as u8;
            let _ = emit_ipv6(src_mac, &rsrc, src, NEXT_HEADER_ICMPV6, 64, &p);
        }
        ICMPV6_DEST_UNREACHABLE => {
            // RFC 4443 §3.1: el cuerpo lleva la cabecera IPv6 original + 8 bytes
            // de transporte. Se informa y se notifica al socket UDP afectado.
            if icmp.len() < 8 + IPV6_HEADER_LEN + 8 { return; }
            let inner = &icmp[8..];
            let nh = inner[6];
            let sport = u16::from_be_bytes([inner[40], inner[41]]);
            let dport = u16::from_be_bytes([inner[42], inner[43]]);
            let motivo = match icmp[1] { 0 => "sin ruta al destino", 1 => "prohibido administrativamente", 3 => "direccion inalcanzable", 4 => "puerto inalcanzable", _ => "otro" };
            crate::drivers::serial::write_str("[IPV6] ICMPv6 destino inalcanzable de "); crate::drivers::serial::write_str(&src.to_string());
            crate::drivers::serial::write_str(" ("); crate::drivers::serial::write_str(motivo); crate::drivers::serial::write_str(") para ");
            // [T-026/IPv6] El paquete citado puede ser ICMPv6 (un `ping6`), no
            // sólo UDP/TCP: antes se leían sus bytes como puertos y la traza
            // decía "proto 32768->19690", que no significa nada. Ahora se
            // reconoce el Echo Request citado y se avisa a `ping6`.
            if nh == NEXT_HEADER_ICMPV6 && inner.len() >= IPV6_HEADER_LEN + 8 && inner[IPV6_HEADER_LEN] == ICMPV6_ECHO_REQUEST {
                let seq = u16::from_be_bytes([inner[IPV6_HEADER_LEN + 6], inner[IPV6_HEADER_LEN + 7]]);
                crate::drivers::serial::write_str("ICMPv6 echo seq=");
                crate::drivers::serial::write_usize(seq as usize);
                *LAST_UNREACH6.lock() = Some((*src, seq, icmp[1]));
            } else {
                crate::drivers::serial::write_str(if nh == NEXT_HEADER_UDP { "UDP " } else if nh == NEXT_HEADER_TCP { "TCP " } else { "proto " });
                crate::drivers::serial::write_usize(sport as usize); crate::drivers::serial::write_str("->"); crate::drivers::serial::write_usize(dport as usize);
            }
            crate::drivers::serial::write_str("\n");
            if nh == NEXT_HEADER_UDP {
                let err = if icmp[1] == 4 { -111 } else { -113 }; // ECONNREFUSED / EHOSTUNREACH
                super::socket::socket_set_error_by_port(sport, err);
            }
        }
        ICMPV6_ECHO_REPLY => {
            let seq = u16::from_be_bytes([icmp[6], icmp[7]]);
            *LAST_ECHO6.lock() = Some((*src, seq, icmp.len()));
        }
        _ => { raw_socket_dispatch(NEXT_HEADER_ICMPV6, *src, icmp); }
    }
}

/// Analiza texto IPv6 (con `::`); sin soporte de sufijo IPv4 embebido.
pub fn parse_ipv6(s: &str) -> Option<Ipv6Addr> {
    let s = s.trim(); if s.is_empty() { return None; }
    let (head, tail) = match s.find("::") { Some(i) => (&s[..i], Some(&s[i + 2..])), None => (s, None) };
    let mut words: Vec<u16> = Vec::new(); let mut tailw: Vec<u16> = Vec::new();
    for (part, out) in [(head, &mut words), (tail.unwrap_or(""), &mut tailw)] {
        if part.is_empty() { continue; }
        for g in part.split(':') { if g.is_empty() || g.len() > 4 { return None; } out.push(u16::from_str_radix(g, 16).ok()?); }
    }
    let total = words.len() + tailw.len();
    if (tail.is_none() && total != 8) || total > 8 { return None; }
    let mut out = [0u8; 16]; let mut idx = 0;
    for w in &words { out[idx * 2] = (w >> 8) as u8; out[idx * 2 + 1] = *w as u8; idx += 1; }
    idx = 8 - tailw.len();
    for w in &tailw { out[idx * 2] = (w >> 8) as u8; out[idx * 2 + 1] = *w as u8; idx += 1; }
    Some(Ipv6Addr(out))
}

pub fn build_icmpv6_echo_request(id: u16, seq: u16, payload: &[u8]) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(8 + payload.len());
    pkt.push(ICMPV6_ECHO_REQUEST);
    pkt.push(0); // Code
    pkt.push(0); // Checksum high (calc later)
    pkt.push(0); // Checksum low
    pkt.extend_from_slice(&id.to_be_bytes());
    pkt.extend_from_slice(&seq.to_be_bytes());
    pkt.extend_from_slice(payload);
    pkt
}
