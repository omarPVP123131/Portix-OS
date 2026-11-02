// kernel/src/net/tcp.rs — Implementación de la Pila y Máquina de Estados TCP en Rust Puro
#![allow(dead_code)]

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use super::IpAddr;
use crate::arch::Spinlock;
use super::{checksum, get_ip, IP_PROTO_TCP};

// Flags de cabecera TCP (RFC 793)
pub const TCP_FLAG_FIN: u8 = 0x01;
pub const TCP_FLAG_SYN: u8 = 0x02;
pub const TCP_FLAG_RST: u8 = 0x04;
pub const TCP_FLAG_PSH: u8 = 0x08;
pub const TCP_FLAG_ACK: u8 = 0x10;
pub const TCP_FLAG_URG: u8 = 0x20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

#[derive(Clone, Debug)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: u8, // en dwords (4 bytes)
    pub flags: u8,
    pub window_size: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

pub struct TcpConnection {
    pub local_ip: IpAddr,
    pub local_port: u16,
    pub remote_ip: IpAddr,
    pub remote_port: u16,
    pub state: TcpState,
    pub seq_num: u32,
    pub ack_num: u32,
    pub remote_window: u32,
    pub rx_buffer: VecDeque<u8>,
    pub in_use: bool,
    /// [CIERRE RED] Conexión aceptada desde un socket en escucha: índice del
    /// `Listen` padre; `accepted` indica que `accept()` ya la entregó.
    pub listener: Option<usize>,
    pub accepted: bool,
    /// Retransmisión del SYN (RFC 6298 con RTO inicial de 0,5 s y backoff ×2).
    pub syn_sent_ticks: u64,
    pub syn_retries: u8,
    // ── [CIERRE TCP] Fiabilidad y control de congestión (RFC 6298 / RFC 5681) ──
    /// Primer byte aún no confirmado por el par (SND.UNA).
    pub snd_una: u32,
    /// Segmentos enviados y pendientes de ACK (cola de retransmisión).
    pub retx: VecDeque<RetxSeg>,
    /// Ventana de congestión y umbral de arranque lento, en bytes.
    pub cwnd: u32,
    pub ssthresh: u32,
    /// RTO actual y estimadores SRTT/RTTVAR en ticks (10 ms); 0 = sin muestra.
    pub rto_ticks: u64,
    pub srtt: u64,
    pub rttvar: u64,
    pub dup_acks: u8,
    /// Estadísticas: retransmisiones por RTO, rápidas y muestras RTT.
    pub retx_rto: u32,
    pub retx_fast: u32,
    pub rtt_samples: u32,
    // ── [SACK] RFC 2018 ──
    /// El par aceptó SACK-Permitted en el apretón de manos.
    pub sack_ok: bool,
    /// Cola de recepción fuera de orden: (seq, datos), ordenada por seq y sin solapes.
    pub ooo: Vec<(u32, Vec<u8>)>,
    /// Secuencia hasta la que ya se hizo una retransmisión guiada por SACK (evita repetirla hasta el RTO).
    pub sack_recover: u32,
    /// Tick en que la conexión entró en un estado de cierre (para expirar TIME-WAIT y huérfanas).
    pub close_ticks: u64,
    // ── RFC 7323: escalado de ventana y marcas de tiempo ──
    /// Ambos extremos negociaron WS; `snd_wscale` es el desplazamiento del par.
    pub ws_ok: bool,
    pub snd_wscale: u8,
    /// Ambos extremos negociaron TS; `ts_recent` = último TSval válido del par (PAWS/RTTM).
    pub ts_ok: bool,
    pub ts_recent: u32,
    pub ts_recent_set: bool,
    /// Muestras RTT obtenidas de TSecr (una por ACK, sin la restricción de Karn).
    pub ts_rtt_samples: u32,
    pub ts_sampled_now: bool,
}

/// Desplazamiento de escala que anunciamos: búfer de 128 KiB → ventana de 16 bits ×2.
pub const RCV_WSCALE: u8 = 1;
/// Contadores RFC 7323: [conexiones con WS, conexiones con TS, muestras RTT por TS, descartes PAWS].
static RFC7323_STATS: crate::arch::Spinlock<[u32; 4]> = crate::arch::Spinlock::new([0; 4]);
fn ts_stat(i: usize) { let mut g = RFC7323_STATS.lock(); g[i] = g[i].saturating_add(1); }
pub fn tcp_rfc7323_stats() -> [u32; 4] { *RFC7323_STATS.lock() }
/// Marca de tiempo local (ticks de 10 ms; RFC 7323 §5.4 admite de 1 ms a 1 s).
fn ts_now() -> u32 { crate::time::pit::ticks() as u32 }

/// TIME-WAIT: 2·MSL. Con RTT de milisegundos en QEMU/bucle local, 2 s bastan y
/// no bloquean las 16 ranuras; las huérfanas en FIN-WAIT-2/LAST-ACK/CLOSING
/// caducan a los 60 s (como `tcp_fin_timeout` de Linux).
const TIME_WAIT_TICKS: u64 = 200;
const ORPHAN_TICKS: u64 = 6000;

/// Segmento de datos pendiente de confirmación. `sacked`: el receptor lo
/// confirmó selectivamente (bloque SACK) y no debe retransmitirse.
pub struct RetxSeg { pub seq: u32, pub data: Vec<u8>, pub sent_ticks: u64, pub retries: u8, pub sacked: bool }

/// Límites de la cola fuera de orden: segmentos y bytes.
const OOO_MAX_SEGS: usize = 32;
/// Contadores globales SACK: (fuera de orden guardados, fusionados, ACK con SACK enviados, ACK con SACK recibidos, retransmisiones guiadas por SACK, segmentos marcados SACKed)
static SACK_STATS: crate::arch::Spinlock<[u32; 6]> = crate::arch::Spinlock::new([0; 6]);
pub fn tcp_sack_stats() -> [u32; 6] { *SACK_STATS.lock() }
fn sack_stat(i: usize) { let mut st = SACK_STATS.lock(); st[i] = st[i].saturating_add(1); }

/// [PRUEBA DE FALLOS] Descarte deliberado de un segmento de datos para
/// ejercitar la recuperación real (SACK, cola fuera de orden, retransmisión
/// selectiva). `perder-rx N` descarta el N-ésimo segmento de datos entrante en
/// orden; `perder-tx N` omite el envío del N-ésimo segmento de datos saliente
/// (queda en la cola de retransmisión). Son contadores de un solo uso.
pub static DROP_RX: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
pub static DROP_TX: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// `tcp ts-vieja N`: antes del N-ésimo segmento de datos saliente se emite un
/// duplicado con TSval antiguo (como un segmento rezagado de otra encarnación)
/// para ejercitar el descarte PAWS real del receptor (RFC 7323 §5.3).
pub static STALE_TS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
fn drop_hit(c: &core::sync::atomic::AtomicU32) -> bool {
    use core::sync::atomic::Ordering;
    let v = c.load(Ordering::Relaxed);
    if v == 0 { return false; }
    c.store(v - 1, Ordering::Relaxed);
    v == 1
}

/// Opciones TCP analizadas de un segmento entrante.
struct TcpOpts { sack_perm: bool, blocks: Vec<(u32, u32)>, wscale: Option<u8>, ts: Option<(u32, u32)> }

/// Analiza las opciones TCP: SACK-Permitted (4), SACK (5), escala de ventana (3) y marcas de tiempo (8).
fn parse_options(opts: &[u8]) -> TcpOpts {
    let mut perm = false; let mut blocks = Vec::new(); let mut i = 0;
    let mut wscale = None; let mut ts = None;
    while i < opts.len() {
        match opts[i] {
            0 => break,
            1 => { i += 1; }
            kind => {
                if i + 1 >= opts.len() { break; }
                let len = opts[i + 1] as usize;
                if len < 2 || i + len > opts.len() { break; }
                if kind == 4 && len == 2 { perm = true; }
                if kind == 3 && len == 3 { wscale = Some(opts[i + 2].min(14)); }
                if kind == 8 && len == 10 {
                    let v = u32::from_be_bytes([opts[i+2], opts[i+3], opts[i+4], opts[i+5]]);
                    let e = u32::from_be_bytes([opts[i+6], opts[i+7], opts[i+8], opts[i+9]]);
                    ts = Some((v, e));
                }
                if kind == 5 {
                    let mut j = i + 2;
                    while j + 8 <= i + len && blocks.len() < 4 {
                        let l = u32::from_be_bytes([opts[j], opts[j+1], opts[j+2], opts[j+3]]);
                        let r = u32::from_be_bytes([opts[j+4], opts[j+5], opts[j+6], opts[j+7]]);
                        blocks.push((l, r)); j += 8;
                    }
                }
                i += len;
            }
        }
    }
    TcpOpts { sack_perm: perm, blocks, wscale, ts }
}

/// Opción SACK-Permitted alineada a 4 bytes (NOP NOP 4 2).
const OPT_SACK_PERMITTED: [u8; 4] = [1, 1, 4, 2];

/// Opciones del SYN / SYN-ACK: SACK-Permitted, escala de ventana y marcas de tiempo
/// (cada una sólo si se ofrece / el par la ofreció). Siempre múltiplo de 4 bytes.
fn syn_options(sack: bool, ws: bool, ts: Option<u32>) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    if sack { v.extend_from_slice(&[4, 2]); }
    if ws { v.extend_from_slice(&[3, 3, RCV_WSCALE]); }
    if let Some(ecr) = ts {
        v.extend_from_slice(&[8, 10]);
        v.extend_from_slice(&ts_now().to_be_bytes());
        v.extend_from_slice(&ecr.to_be_bytes());
    }
    while v.len() % 4 != 0 { v.push(1); }
    v
}

pub const TCP_MSS: u32 = 1460;
const RTO_MIN: u64 = 20;    // 200 ms
const RTO_INIT: u64 = 100;  // 1 s (RFC 6298 §2.1)
const RTO_MAX: u64 = 6000;  // 60 s
const MAX_DATA_RETRIES: u8 = 8;

impl TcpConnection {
    pub const fn new() -> Self {
        Self {
            local_ip: IpAddr::UNSPECIFIED4,
            local_port: 0,
            remote_ip: IpAddr::UNSPECIFIED4,
            remote_port: 0,
            state: TcpState::Closed,
            seq_num: 1000,
            ack_num: 0,
            remote_window: 65535,
            rx_buffer: VecDeque::new(),
            in_use: false,
            listener: None,
            accepted: false,
            syn_sent_ticks: 0,
            syn_retries: 0,
            snd_una: 1000,
            retx: VecDeque::new(),
            cwnd: 10 * TCP_MSS,   // IW10 (RFC 6928)
            ssthresh: u32::MAX,
            rto_ticks: RTO_INIT,
            srtt: 0,
            rttvar: 0,
            dup_acks: 0,
            retx_rto: 0,
            retx_fast: 0,
            rtt_samples: 0,
            sack_ok: false,
            ooo: Vec::new(),
            sack_recover: 0,
            close_ticks: 0,
            ws_ok: false, snd_wscale: 0, ts_ok: false, ts_recent: 0, ts_recent_set: false, ts_rtt_samples: 0, ts_sampled_now: false,
        }
    }

    /// Opciones de un segmento ya establecido: marca de tiempo (si se negoció) + `extra`.
    fn seg_options(&self, extra: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(12 + extra.len());
        if self.ts_ok {
            v.extend_from_slice(&[1, 1, 8, 10]);
            v.extend_from_slice(&ts_now().to_be_bytes());
            v.extend_from_slice(&self.ts_recent.to_be_bytes());
        }
        v.extend_from_slice(extra);
        v
    }

    /// [PRUEBA DE FALLOS] Duplicado del segmento con una marca de tiempo antigua.
    fn xmit_stale_ts(&self, seq: u32, payload: &[u8]) {
        if !self.ts_ok { return; }
        let stale = ts_now().wrapping_sub(120_000);
        let mut v = Vec::with_capacity(12);
        v.extend_from_slice(&[1, 1, 8, 10]);
        v.extend_from_slice(&stale.to_be_bytes());
        v.extend_from_slice(&self.ts_recent.to_be_bytes());
        let _ = send_tcp_segment_full(self.local_ip, self.remote_ip, self.local_port, self.remote_port, seq, self.ack_num, TCP_FLAG_ACK | TCP_FLAG_PSH, payload, self.adv_window(), &v);
        crate::drivers::serial::write_str("[TCP-PRUEBA] duplicado con marca de tiempo antigua emitido adrede (seq=");
        crate::drivers::serial::write_usize(seq as usize);
        crate::drivers::serial::write_str(", TSval=");
        crate::drivers::serial::write_usize(stale as usize);
        crate::drivers::serial::write_str("): el receptor debe descartarlo por PAWS\n");
    }

    /// Emite un segmento de esta conexión con la ventana real anunciada y las opciones negociadas.
    fn xmit(&self, seq: u32, ack: u32, flags: u8, payload: &[u8], window: Option<u16>, extra: &[u8]) {
        let opts = self.seg_options(extra);
        let w = window.unwrap_or_else(|| self.adv_window());
        let _ = send_tcp_segment_full(self.local_ip, self.remote_ip, self.local_port, self.remote_port, seq, ack, flags, payload, w, &opts);
    }

    /// Opción SACK (RFC 2018 §3) con los bloques contiguos de la cola fuera de orden
    /// (hasta 4; el primero, el más reciente, como recomienda la RFC).
    fn sack_option(&self, recent: Option<u32>) -> Vec<u8> {
        if !self.sack_ok || self.ooo.is_empty() { return Vec::new(); }
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for (seq, d) in self.ooo.iter() {
            let end = seq.wrapping_add(d.len() as u32);
            match runs.last_mut() { Some(r) if r.1 == *seq => { r.1 = end; } _ => runs.push((*seq, end)) }
        }
        if let Some(rs) = recent { if let Some(pos) = runs.iter().position(|r| (rs.wrapping_sub(r.0) as i32) >= 0 && (r.1.wrapping_sub(rs) as i32) > 0) { let r = runs.remove(pos); runs.insert(0, r); } }
        runs.truncate(4);
        let mut o = Vec::with_capacity(4 + 8 * runs.len());
        o.push(1); o.push(1); o.push(5); o.push((2 + 8 * runs.len()) as u8);
        for (l, r) in runs { o.extend_from_slice(&l.to_be_bytes()); o.extend_from_slice(&r.to_be_bytes()); }
        o
    }

    /// Inserta un segmento futuro en la cola fuera de orden (sin solapes, ordenada).
    fn ooo_insert(&mut self, seq: u32, data: &[u8]) -> bool {
        if data.is_empty() { return false; }
        if self.ooo.len() >= OOO_MAX_SEGS { return false; }
        let total: usize = self.ooo.iter().map(|(_, d)| d.len()).sum();
        if total + data.len() > RX_BUF_LIMIT.saturating_sub(self.rx_buffer.len()) { return false; }
        // recorte contra segmentos ya guardados (solapes parciales)
        let mut seq = seq; let mut data = data.to_vec();
        for (s, d) in self.ooo.iter() {
            let s_end = s.wrapping_add(d.len() as u32);
            let end = seq.wrapping_add(data.len() as u32);
            if (seq.wrapping_sub(*s) as i32) >= 0 && (s_end.wrapping_sub(end) as i32) >= 0 { return false; } // ya contenido
            if (seq.wrapping_sub(*s) as i32) >= 0 && (s_end.wrapping_sub(seq) as i32) > 0 { let k = s_end.wrapping_sub(seq) as usize; data.drain(..k.min(data.len())); seq = s_end; }
            else if (s.wrapping_sub(seq) as i32) > 0 && (end.wrapping_sub(*s) as i32) > 0 { let k = s.wrapping_sub(seq) as usize; data.truncate(k); }
        }
        if data.is_empty() { return false; }
        let pos = self.ooo.iter().position(|(s, _)| (s.wrapping_sub(seq) as i32) > 0).unwrap_or(self.ooo.len());
        self.ooo.insert(pos, (seq, data));
        true
    }

    /// Pasa a `rx_buffer` todo lo que la cola fuera de orden tenga contiguo a RCV.NXT.
    fn ooo_merge(&mut self) -> usize {
        let mut merged = 0usize;
        while let Some((s, _)) = self.ooo.first() {
            let s = *s;
            if (s.wrapping_sub(self.ack_num) as i32) > 0 { break; }
            let (_, d) = self.ooo.remove(0);
            let skip = (self.ack_num.wrapping_sub(s) as usize).min(d.len());
            for &b in &d[skip..] { self.rx_buffer.push_back(b); }
            self.ack_num = self.ack_num.wrapping_add((d.len() - skip) as u32);
            merged += d.len() - skip;
        }
        merged
    }

    /// Marca como confirmados selectivamente los segmentos cubiertos por bloques SACK.
    fn apply_sack(&mut self, blocks: &[(u32, u32)]) -> u32 {
        let mut n = 0;
        for seg in self.retx.iter_mut() {
            if seg.sacked { continue; }
            let end = seg.seq.wrapping_add(seg.data.len() as u32);
            if blocks.iter().any(|(l, r)| (seg.seq.wrapping_sub(*l) as i32) >= 0 && (r.wrapping_sub(end) as i32) >= 0) { seg.sacked = true; n += 1; }
        }
        n
    }

    /// MSS efectivo según la familia IP del par (RFC 8200: 1500 - 40 - 20).
    pub fn mss(&self) -> u32 {
        let base = match self.remote_ip { IpAddr::V6(_) => 1440, _ => TCP_MSS };
        // Las marcas de tiempo (RFC 7323 §3.1) ocupan 12 bytes de opciones en cada segmento.
        if self.ts_ok { base - 12 } else { base }
    }

    /// Bytes enviados y aún no confirmados.
    pub fn in_flight(&self) -> u32 { self.seq_num.wrapping_sub(self.snd_una) }
    /// Ventana efectiva de envío: min(cwnd, ventana anunciada por el par).
    pub fn send_window(&self) -> u32 { self.cwnd.min(self.remote_window) }

    /// Procesa el campo ACK de un segmento entrante (RFC 5681 §3.1/§3.2 y RFC 6298 §5).
    fn process_ack(&mut self, ack: u32, seg_has_data: bool, now: u64) {
        let advanced = ack.wrapping_sub(self.snd_una) as i32;
        if advanced > 0 && (self.seq_num.wrapping_sub(ack) as i32) >= 0 {
            let acked = advanced as u32;
            // Muestra RTT (Karn: solo segmentos nunca retransmitidos)
            while let Some(f) = self.retx.front() {
                if (ack.wrapping_sub(f.seq.wrapping_add(f.data.len() as u32)) as i32) >= 0 {
                    if f.retries == 0 && !self.ts_sampled_now { let r = now.wrapping_sub(f.sent_ticks); self.update_rtt(r); }
                    self.retx.pop_front();
                } else { break; }
            }
            self.snd_una = ack;
            self.dup_acks = 0;
            if (ack.wrapping_sub(self.sack_recover) as i32) >= 0 { self.sack_recover = ack; }
            if self.cwnd < self.ssthresh { self.cwnd = self.cwnd.saturating_add(acked.min(TCP_MSS)); }      // arranque lento
            else { self.cwnd = self.cwnd.saturating_add(((TCP_MSS as u64 * TCP_MSS as u64) / self.cwnd.max(1) as u64) as u32).max(self.cwnd + 1); } // evitación
            if self.retx.is_empty() { self.rto_ticks = self.rto_from_estimators(); }
        } else if advanced == 0 && !seg_has_data && !self.retx.is_empty() {
            self.dup_acks = self.dup_acks.saturating_add(1);
            if self.dup_acks == 3 {
                // Retransmisión rápida (RFC 5681 §3.2)
                let flight = self.in_flight();
                self.ssthresh = (flight / 2).max(2 * TCP_MSS);
                self.cwnd = self.ssthresh + 3 * TCP_MSS;
                let hueco_ya_reenviado = self.retx.iter().find(|s| !s.sacked).map(|s| (self.sack_recover.wrapping_sub(s.seq) as i32) > 0).unwrap_or(false);
                if hueco_ya_reenviado {
                    crate::drivers::serial::write_str("[TCP] 3 ACK duplicados: el hueco ya fue retransmitido por SACK; no se reenvia\n");
                } else if let Some(f) = self.retx.iter_mut().find(|s| !s.sacked) {
                    f.retries = f.retries.saturating_add(1); f.sent_ticks = now;
                    let (lip, rip, lp, rp, seq, ack_n, d) = (self.local_ip, self.remote_ip, self.local_port, self.remote_port, f.seq, self.ack_num, f.data.clone());
                    self.sack_recover = seq.wrapping_add(d.len() as u32);
                    self.retx_fast += 1;
                    let _ = (lip, rip, lp, rp, ack_n); self.xmit(seq, self.ack_num, TCP_FLAG_ACK | TCP_FLAG_PSH, &d, None, &[]);
                    crate::drivers::serial::write_str("[TCP] retransmision rapida tras 3 ACK duplicados (seq=");
                    crate::drivers::serial::write_usize(seq as usize);
                    crate::drivers::serial::write_str(", cwnd=");
                    crate::drivers::serial::write_usize(self.cwnd as usize);
                    crate::drivers::serial::write_str(")\n");
                }
            }
        }
    }

    /// Recuperación guiada por SACK (RFC 6675 simplificada): si hay ≥ 3
    /// segmentos confirmados selectivamente por delante del primer hueco, el
    /// hueco se da por perdido y se retransmite SOLO ese segmento (una vez
    /// hasta que avance SND.UNA o venza el RTO).
    fn sack_retransmit(&mut self, now: u64) {
        let hole = match self.retx.iter().position(|s| !s.sacked) { Some(i) => i, None => return };
        let beyond = self.retx.iter().skip(hole + 1).filter(|s| s.sacked).count();
        if beyond < 3 { return; }
        let seq = self.retx[hole].seq;
        if (self.sack_recover.wrapping_sub(seq) as i32) > 0 { return; } // ya retransmitido en esta recuperación
        let f = &mut self.retx[hole];
        f.retries = f.retries.saturating_add(1); f.sent_ticks = now;
        let (lip, rip, lp, rp, ack_n, d) = (self.local_ip, self.remote_ip, self.local_port, self.remote_port, self.ack_num, f.data.clone());
        self.sack_recover = seq.wrapping_add(d.len() as u32);
        if self.cwnd > 2 * TCP_MSS { self.ssthresh = (self.in_flight() / 2).max(2 * TCP_MSS); self.cwnd = self.ssthresh; }
        sack_stat(4);
        let _ = (lip, rip, lp, rp, ack_n); self.xmit(seq, self.ack_num, TCP_FLAG_ACK | TCP_FLAG_PSH, &d, None, &[]);
        crate::drivers::serial::write_str("[TCP-SACK] retransmision selectiva del hueco (seq=");
        crate::drivers::serial::write_usize(seq as usize);
        crate::drivers::serial::write_str(", ");
        crate::drivers::serial::write_usize(d.len());
        crate::drivers::serial::write_str(" bytes) con ");
        crate::drivers::serial::write_usize(beyond);
        crate::drivers::serial::write_str(" segmentos posteriores ya confirmados por SACK; no se reenvian\n");
    }

    fn update_rtt(&mut self, r: u64) {
        if self.srtt == 0 { self.srtt = r.max(1); self.rttvar = (r / 2).max(1); }
        else {
            let diff = if self.srtt > r { self.srtt - r } else { r - self.srtt };
            self.rttvar = (3 * self.rttvar + diff) / 4;
            self.srtt = (7 * self.srtt + r) / 8;
        }
        self.rtt_samples += 1;
        self.rto_ticks = self.rto_from_estimators();
    }
    fn rto_from_estimators(&self) -> u64 {
        if self.srtt == 0 { return RTO_INIT; }
        (self.srtt + (4 * self.rttvar).max(1)).clamp(RTO_MIN, RTO_MAX)
    }
}

pub const MAX_TCP_CONNS: usize = 16;
pub struct TcpTable {
    pub conns: [TcpConnection; MAX_TCP_CONNS],
}

impl TcpTable {
    pub const fn new() -> Self {
        const INIT: TcpConnection = TcpConnection::new();
        Self { conns: [INIT; MAX_TCP_CONNS] }
    }
}

pub static TCP_TABLE: Spinlock<TcpTable> = Spinlock::new(TcpTable::new());

/// Calcula el checksum TCP con pseudo-cabecera IPv4
pub fn tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], tcp_segment: &[u8]) -> u16 {
    let mut pseudo = Vec::with_capacity(12 + tcp_segment.len());
    pseudo.extend_from_slice(&src_ip);
    pseudo.extend_from_slice(&dst_ip);
    pseudo.push(0);
    pseudo.push(IP_PROTO_TCP);
    let len = tcp_segment.len() as u16;
    pseudo.push((len >> 8) as u8);
    pseudo.push((len & 0xFF) as u8);
    pseudo.extend_from_slice(tcp_segment);
    if pseudo.len() % 2 != 0 {
        pseudo.push(0);
    }
    checksum(&pseudo)
}

/// Envía un segmento TCP básico
pub fn send_tcp_segment(src_ip: IpAddr, dst_ip: IpAddr, src_port: u16, dst_port: u16, seq: u32, ack: u32, flags: u8, payload: &[u8]) -> Result<(), &'static str> {
    send_tcp_segment_win(src_ip, dst_ip, src_port, dst_port, seq, ack, flags, payload, RX_WINDOW_MAX)
}

/// Límite del búfer de recepción por conexión. La ventana anunciada es el
/// espacio libre real (acotado a 16 bits sin escalado): si la aplicación no
/// lee, la ventana se cierra y el emisor se detiene en vez de que el núcleo
/// tire bytes a escondidas (antes se descartaban y aun así se acusaban).
pub const RX_BUF_LIMIT: usize = 128 * 1024;
pub const RX_WINDOW_MAX: u16 = 65535;
/// Contadores: (ventanas cero anunciadas, actualizaciones de ventana enviadas al leer, bytes recortados por falta de espacio)
static RX_WND_STATS: crate::arch::Spinlock<(u32, u32, u64)> = crate::arch::Spinlock::new((0, 0, 0));
pub fn tcp_window_stats() -> (u32, u32, u64) { let s = RX_WND_STATS.lock(); (s.0, s.1, s.2) }

impl TcpConnection {
    /// Ventana de recepción anunciada: espacio libre real del búfer.
    /// Ventana anunciada tal como viaja en la cabecera: bytes libres >> RCV_WSCALE si se negoció WS.
    pub fn adv_window(&self) -> u16 {
        let libre = RX_BUF_LIMIT.saturating_sub(self.rx_buffer.len());
        let v = if self.ws_ok { libre >> RCV_WSCALE } else { libre };
        v.min(RX_WINDOW_MAX as usize) as u16
    }
    /// Lo mismo, en bytes (para estadísticas y umbrales).
    pub fn adv_bytes(&self) -> usize { let libre = RX_BUF_LIMIT.saturating_sub(self.rx_buffer.len()); if self.ws_ok { (libre >> RCV_WSCALE << RCV_WSCALE).min((RX_WINDOW_MAX as usize) << RCV_WSCALE) } else { libre.min(RX_WINDOW_MAX as usize) } }
}

pub fn send_tcp_segment_win(src_ip: IpAddr, dst_ip: IpAddr, src_port: u16, dst_port: u16, seq: u32, ack: u32, flags: u8, payload: &[u8], window: u16) -> Result<(), &'static str> {
    send_tcp_segment_full(src_ip, dst_ip, src_port, dst_port, seq, ack, flags, payload, window, &[])
}

/// Segmento con opciones TCP (`options` ya alineadas a 4 bytes, ≤ 40).
pub fn send_tcp_segment_full(
    src_ip: IpAddr,
    dst_ip: IpAddr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
    window: u16,
    options: &[u8],
) -> Result<(), &'static str> {
    let options = if options.len() % 4 != 0 || options.len() > 40 { &[][..] } else { options };
    let mut seg = Vec::with_capacity(20 + options.len() + payload.len());
    seg.push((src_port >> 8) as u8);
    seg.push((src_port & 0xFF) as u8);
    seg.push((dst_port >> 8) as u8);
    seg.push((dst_port & 0xFF) as u8);

    seg.extend_from_slice(&seq.to_be_bytes());
    seg.extend_from_slice(&ack.to_be_bytes());

    let data_offset_val = ((20 + options.len()) / 4) as u8; // dwords
    seg.push(data_offset_val << 4);
    seg.push(flags);

    seg.push((window >> 8) as u8);
    seg.push((window & 0xFF) as u8);

    seg.push(0); seg.push(0); // Checksum provisional 0
    seg.push(0); seg.push(0); // Urgent pointer

    seg.extend_from_slice(options);
    seg.extend_from_slice(payload);

    // [IPv6 REAL] doble pila: suma con la pseudo-cabecera que corresponda
    match (src_ip, dst_ip) {
        (IpAddr::V4(s4), IpAddr::V4(d4)) => {
            let csum = tcp_checksum(s4, d4, &seg);
            seg[16] = (csum >> 8) as u8; seg[17] = (csum & 0xFF) as u8;
            crate::net::send_ipv4(d4, IP_PROTO_TCP, &seg)
        }
        (IpAddr::V6(s6), IpAddr::V6(d6)) => {
            let csum = crate::net::ipv6::checksum_v6(&s6, &d6, crate::net::ipv6::NEXT_HEADER_TCP, &seg);
            seg[16] = (csum >> 8) as u8; seg[17] = (csum & 0xFF) as u8;
            crate::net::ipv6::send_ipv6(&d6, crate::net::ipv6::NEXT_HEADER_TCP, &seg)
        }
        _ => Err("familias IP mezcladas en el segmento TCP"),
    }
}

/// Manejador de paquetes TCP entrantes
pub fn handle_tcp_packet(src_ip: IpAddr, dst_ip: IpAddr, payload: &[u8]) {
    if payload.len() < 20 {
        return;
    }

    let src_port = ((payload[0] as u16) << 8) | (payload[1] as u16);
    let dst_port = ((payload[2] as u16) << 8) | (payload[3] as u16);
    let seq_num = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let seg_ack = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
    let data_offset = ((payload[12] >> 4) as usize) * 4;
    let flags = payload[13];
    let seg_window = ((payload[14] as u16) << 8) | (payload[15] as u16);

    if data_offset > payload.len() || data_offset < 20 {
        return;
    }
    let data = &payload[data_offset..];
    let opts = parse_options(&payload[20..data_offset]);
    let (sack_perm, sack_blocks) = (opts.sack_perm, opts.blocks);

    let mut table = TCP_TABLE.lock();
    for i in 0..MAX_TCP_CONNS {
        let conn = &mut table.conns[i];
        if !conn.in_use {
            continue;
        }

        // Caso 1: Servidor escuchando (Listen) — se atiende tras el bucle
        // para poder crear una conexión HIJA sin dejar de escuchar.
        if conn.state == TcpState::Listen { continue; }

        // Caso 2: Conexión saliente activa o establecida
        if conn.local_port == dst_port && conn.remote_port == src_port && conn.remote_ip == src_ip {
            // [CIERRE TCP] ACK acumulativo, ventana anunciada y congestión
            // para cualquier estado con datos en vuelo.
            if (flags & TCP_FLAG_ACK) != 0 && matches!(conn.state, TcpState::Established | TcpState::FinWait1 | TcpState::FinWait2 | TcpState::CloseWait | TcpState::Closing | TcpState::LastAck) {
                conn.remote_window = (seg_window as u32) << conn.snd_wscale;
                let now = crate::time::pit::ticks();
                // RTTM (RFC 7323 §4): una muestra por ACK a partir de TSecr.
                conn.ts_sampled_now = false;
                if conn.ts_ok {
                    if let Some((_, ecr)) = opts.ts {
                        if ecr != 0 && (seg_ack.wrapping_sub(conn.snd_una) as i32) > 0 {
                            let r = ts_now().wrapping_sub(ecr) as u64;
                            if r < 60_000 { conn.update_rtt(r); conn.ts_rtt_samples += 1; conn.ts_sampled_now = true; ts_stat(2); }
                        }
                    }
                }
                if !sack_blocks.is_empty() && conn.sack_ok {
                    sack_stat(3);
                    let n = conn.apply_sack(&sack_blocks);
                    for _ in 0..n { sack_stat(5); }
                }
                // RFC 6675: con SACK, la deteccion de perdida guiada por los bloques
                // tiene prioridad sobre el conteo de ACK duplicados (RFC 5681), que
                // queda como respaldo para pares sin SACK.
                if !sack_blocks.is_empty() && conn.sack_ok { conn.sack_retransmit(now); }
                conn.process_ack(seg_ack, !data.is_empty(), now);
            }
            match conn.state {
                TcpState::SynSent => {
                    if (flags & TCP_FLAG_RST) != 0 {
                        conn.state = TcpState::Closed;
                        crate::drivers::serial::write_str("[TCP] RST en SYN-SENT: conexion rechazada\n");
                        return;
                    }
                    if (flags & (TCP_FLAG_SYN | TCP_FLAG_ACK)) == (TCP_FLAG_SYN | TCP_FLAG_ACK) {
                        conn.ack_num = seq_num.wrapping_add(1);
                        conn.state = TcpState::Established;
                        conn.sack_ok = sack_perm;
                        if sack_perm { crate::drivers::serial::write_str("[TCP-SACK] el par acepta SACK (SACK-Permitted en el SYN-ACK)\n"); }
                        conn.snd_una = conn.seq_num;
                        conn.remote_window = seg_window as u32; // el SYN-ACK nunca va escalado
                        if let Some(w) = opts.wscale { conn.ws_ok = true; conn.snd_wscale = w; ts_stat(0); }
                        if let Some((v, _)) = opts.ts { conn.ts_ok = true; conn.ts_recent = v; conn.ts_recent_set = true; ts_stat(1); }
                        if conn.ws_ok || conn.ts_ok {
                            crate::drivers::serial::write_str("[TCP-7323] el par acepta");
                            if conn.ws_ok { crate::drivers::serial::write_str(" escalado de ventana (factor del par 2^"); crate::drivers::serial::write_usize(conn.snd_wscale as usize); crate::drivers::serial::write_str(", local 2^1)"); }
                            if conn.ts_ok { crate::drivers::serial::write_str(" marcas de tiempo (RTTM+PAWS)"); }
                            crate::drivers::serial::write_str("\n");
                        }
                        if conn.syn_retries == 0 { let r = crate::time::pit::ticks().wrapping_sub(conn.syn_sent_ticks); conn.update_rtt(r); }

                        // Responder con ACK final del 3-way handshake
                        conn.xmit(conn.seq_num, conn.ack_num, TCP_FLAG_ACK, &[], None, &[]);
                    }
                }
                TcpState::SynReceived => {
                    if (flags & TCP_FLAG_ACK) != 0 {
                        conn.state = TcpState::Established;
                        conn.snd_una = conn.seq_num;
                        conn.remote_window = (seg_window as u32) << conn.snd_wscale;
                        if let Some((v, _)) = opts.ts { if conn.ts_ok { conn.ts_recent = v; conn.ts_recent_set = true; } }
                        if conn.listener.is_some() { crate::drivers::serial::write_str("[TCP] conexion hija ESTABLISHED (3-way handshake completo)\n"); }
                        if !data.is_empty() && seq_num == conn.ack_num {
                            for &b in data { conn.rx_buffer.push_back(b); }
                            conn.ack_num = conn.ack_num.wrapping_add(data.len() as u32);
                            conn.xmit(conn.seq_num, conn.ack_num, TCP_FLAG_ACK, &[], None, &[]);
                        }
                    } else if (flags & TCP_FLAG_SYN) != 0 {
                        // SYN duplicado: repetir SYN-ACK
                        let opt = syn_options(conn.sack_ok, conn.ws_ok, if conn.ts_ok { Some(conn.ts_recent) } else { None });
                        let _ = send_tcp_segment_full(dst_ip, src_ip, conn.local_port, src_port, conn.seq_num.wrapping_sub(1), conn.ack_num, TCP_FLAG_SYN | TCP_FLAG_ACK, &[], RX_WINDOW_MAX, &opt);
                    }
                }
                TcpState::Established => {
                    // [FRENTE B] Control de secuencia RFC 793 §3.9: solo se
                    // aceptan los bytes a partir de RCV.NXT (conn.ack_num).
                    // Las retransmisiones duplicadas se recortan y los
                    // segmentos futuros (fuera de orden) se descartan
                    // re-enviando el ACK acumulativo; antes se concatenaba
                    // todo y las retransmisiones corrompian el flujo.
                    let mut data = data;
                    let mut seg_seq = seq_num;
                    // PAWS (RFC 7323 §5.3): un TSval anterior al último visto delata un
                    // segmento viejo de otra encarnación; se descarta y se repite el ACK.
                    if conn.ts_ok && conn.ts_recent_set && (flags & TCP_FLAG_RST) == 0 {
                        if let Some((v, _)) = opts.ts {
                            if (v.wrapping_sub(conn.ts_recent) as i32) < 0 {
                                ts_stat(3);
                                crate::drivers::serial::write_str("[TCP-7323] PAWS: segmento con marca de tiempo antigua descartado (seq=");
                                crate::drivers::serial::write_usize(seg_seq as usize);
                                crate::drivers::serial::write_str(")\n");
                                conn.xmit(conn.seq_num, conn.ack_num, TCP_FLAG_ACK, &[], None, &[]);
                                return;
                            }
                            // Actualizar TS.Recent si el segmento no es futuro (§4.3 simplificado)
                            if (conn.ack_num.wrapping_sub(seg_seq) as i32) >= 0 { conn.ts_recent = v; }
                        }
                    }
                    if !data.is_empty() {
                        let delta = conn.ack_num.wrapping_sub(seg_seq) as i32;
                        if delta > 0 {
                            // Solape con bytes ya recibidos: recortar.
                            let skip = (delta as usize).min(data.len());
                            data = &data[skip..];
                            seg_seq = seg_seq.wrapping_add(skip as u32);
                        } else if delta < 0 {
                            // Hueco: segmento futuro. Con SACK se guarda en la cola
                            // fuera de orden y se anuncia en el ACK; sin SACK se
                            // descarta y se repite el ACK acumulativo.
                            if conn.ooo_insert(seg_seq, data) {
                                sack_stat(0);
                                crate::drivers::serial::write_str(if conn.sack_ok { "[TCP-SACK] hueco de " } else { "[TCP-OOO] hueco de " });
                                crate::drivers::serial::write_usize((-delta) as usize);
                                crate::drivers::serial::write_str(" bytes: segmento fuera de orden guardado (seq=");
                                crate::drivers::serial::write_usize(seg_seq as usize);
                                crate::drivers::serial::write_str(", ");
                                crate::drivers::serial::write_usize(data.len());
                                crate::drivers::serial::write_str(if conn.sack_ok { " bytes); se acusa con bloque SACK\n" } else { " bytes); el par no acepta SACK: se repite el ACK acumulativo\n" });
                            } else {
                                crate::drivers::serial::write_str("[TCP] segmento fuera de orden descartado; se repite ACK\n");
                            }
                            data = &[];
                        } else if drop_hit(&DROP_RX) {
                            crate::drivers::serial::write_str("[TCP-PRUEBA] segmento de datos entrante descartado adrede (seq=");
                            crate::drivers::serial::write_usize(seg_seq as usize);
                            crate::drivers::serial::write_str(", ");
                            crate::drivers::serial::write_usize(data.len());
                            crate::drivers::serial::write_str(" bytes): se simula una perdida en la red\n");
                            return;
                        }
                    }
                    let mut sack_recent: Option<u32> = if !data.is_empty() { None } else { Some(seg_seq) };
                    if !data.is_empty() {
                        let libre = RX_BUF_LIMIT.saturating_sub(conn.rx_buffer.len());
                        let aceptados = data.len().min(libre);
                        if aceptados < data.len() { let mut st = RX_WND_STATS.lock(); st.2 += (data.len() - aceptados) as u64; }
                        for &b in &data[..aceptados] { conn.rx_buffer.push_back(b); }
                        conn.ack_num = conn.ack_num.wrapping_add(aceptados as u32);
                        if !conn.ooo.is_empty() {
                            let m = conn.ooo_merge();
                            if m > 0 {
                                sack_stat(1);
                                crate::drivers::serial::write_str("[TCP-SACK] hueco cerrado: ");
                                crate::drivers::serial::write_usize(m);
                                crate::drivers::serial::write_str(" bytes fusionados desde la cola fuera de orden (RCV.NXT=");
                                crate::drivers::serial::write_usize(conn.ack_num as usize);
                                crate::drivers::serial::write_str(")\n");
                            }
                        }
                        sack_recent = None;
                    }
                    if !data.is_empty() || (payload.len() > data_offset && (flags & TCP_FLAG_FIN) == 0) {
                        // Enviar ACK (acumulativo) por los datos recibidos o
                        // repetirlo ante duplicados/fuera de orden, anunciando
                        // la ventana de recepción real (espacio libre del búfer).
                        let w = conn.adv_window();
                        if w == 0 { let mut st = RX_WND_STATS.lock(); st.0 += 1; }
                        let opt = conn.sack_option(sack_recent);
                        if !opt.is_empty() { sack_stat(2); }
                        conn.xmit(conn.seq_num, conn.ack_num, TCP_FLAG_ACK, &[], Some(w), &opt);
                    }

                    let fin_en_orden = seg_seq.wrapping_add(data.len() as u32) == conn.ack_num;
                    if (flags & TCP_FLAG_FIN) != 0 && fin_en_orden {
                        conn.ack_num = conn.ack_num.wrapping_add(1);
                        conn.state = TcpState::CloseWait;
                        conn.xmit(conn.seq_num, conn.ack_num, TCP_FLAG_ACK, &[], None, &[]);
                    }
                }
                TcpState::FinWait1 => {
                    if (flags & TCP_FLAG_ACK) != 0 {
                        conn.state = TcpState::FinWait2;
                    }
                    if (flags & TCP_FLAG_FIN) != 0 {
                        conn.ack_num = conn.ack_num.wrapping_add(1);
                        conn.state = TcpState::TimeWait; conn.close_ticks = crate::time::pit::ticks();
                        conn.xmit(conn.seq_num, conn.ack_num, TCP_FLAG_ACK, &[], None, &[]);
                    }
                }
                TcpState::FinWait2 => {
                    if (flags & TCP_FLAG_FIN) != 0 {
                        conn.ack_num = conn.ack_num.wrapping_add(1);
                        conn.state = TcpState::TimeWait; conn.close_ticks = crate::time::pit::ticks();
                        conn.xmit(conn.seq_num, conn.ack_num, TCP_FLAG_ACK, &[], None, &[]);
                    }
                }
                TcpState::LastAck => {
                    if (flags & TCP_FLAG_ACK) != 0 {
                        conn.state = TcpState::Closed;
                        conn.in_use = false;
                    }
                }
                _ => {}
            }
            return;
        }
    }

    // [CIERRE RED] SYN hacia un puerto en escucha: nueva conexión hija en
    // SYN-RECEIVED (el `Listen` permanece), SYN-ACK real y, al llegar el ACK,
    // queda ESTABLISHED a la espera de `accept()`.
    if (flags & TCP_FLAG_SYN) != 0 && (flags & TCP_FLAG_ACK) == 0 {
        let mut listener = None;
        for i in 0..MAX_TCP_CONNS {
            if table.conns[i].in_use && table.conns[i].state == TcpState::Listen && table.conns[i].local_port == dst_port { listener = Some(i); break; }
        }
        if let Some(l) = listener {
            let pendientes = table.conns.iter().filter(|c| c.in_use && c.listener == Some(l) && !c.accepted).count();
            if pendientes >= 8 { return; } // backlog lleno: se descarta (el cliente retransmitirá)
            for i in 0..MAX_TCP_CONNS {
                if !table.conns[i].in_use {
                    let isn = 1000 + (i as u32 * 500) + (crate::time::pit::ticks() as u32).wrapping_mul(64);
                    let conn = &mut table.conns[i];
                    *conn = TcpConnection::new();
                    conn.in_use = true;
                    conn.local_ip = dst_ip; conn.local_port = dst_port;
                    conn.remote_ip = src_ip; conn.remote_port = src_port;
                    conn.listener = Some(l);
                    conn.seq_num = isn;
                    conn.ack_num = seq_num.wrapping_add(1);
                    conn.state = TcpState::SynReceived;
                    conn.sack_ok = sack_perm;
                    if let Some(w) = opts.wscale { conn.ws_ok = true; conn.snd_wscale = w; ts_stat(0); }
                    if let Some((v, _)) = opts.ts { conn.ts_ok = true; conn.ts_recent = v; conn.ts_recent_set = true; ts_stat(1); }
                    let opt = syn_options(sack_perm, conn.ws_ok, if conn.ts_ok { Some(conn.ts_recent) } else { None });
                    let _ = send_tcp_segment_full(dst_ip, src_ip, dst_port, src_port, conn.seq_num, conn.ack_num, TCP_FLAG_SYN | TCP_FLAG_ACK, &[], RX_WINDOW_MAX, &opt);
                    conn.seq_num = conn.seq_num.wrapping_add(1);
                    crate::drivers::serial::write_str("[TCP] SYN en puerto en escucha: SYN-ACK enviado (conexion hija)\n");
                    return;
                }
            }
        }
    }
}

/// [CIERRE RED] Pone un puerto en escucha (socket pasivo real).
pub fn tcp_listen(local_port: u16) -> Result<usize, i32> {
    let mut table = TCP_TABLE.lock();
    for i in 0..MAX_TCP_CONNS {
        if table.conns[i].in_use && table.conns[i].state == TcpState::Listen && table.conns[i].local_port == local_port { return Err(-98); } // EADDRINUSE
    }
    for i in 0..MAX_TCP_CONNS {
        if !table.conns[i].in_use {
            let conn = &mut table.conns[i];
            *conn = TcpConnection::new();
            conn.in_use = true; conn.local_ip = IpAddr::V4(get_ip()); conn.local_port = local_port; conn.state = TcpState::Listen;
            return Ok(i);
        }
    }
    Err(-24)
}

/// [CIERRE RED] Entrega una conexión hija ya establecida del `Listen` dado.
pub fn tcp_accept(listener: usize) -> Option<(usize, IpAddr, u16)> {
    let mut table = TCP_TABLE.lock();
    for i in 0..MAX_TCP_CONNS {
        let c = &mut table.conns[i];
        if c.in_use && c.listener == Some(listener) && !c.accepted && c.state == TcpState::Established {
            c.accepted = true;
            return Some((i, c.remote_ip, c.remote_port));
        }
    }
    None
}

/// [CIERRE RED] Espera (procesando la red) hasta que haya una conexión que
/// aceptar o venza el plazo (0 = sin plazo).
pub fn tcp_wait_accept(listener: usize, timeout_ticks: u64) -> Option<(usize, IpAddr, u16)> {
    let inicio = crate::time::pit::ticks();
    loop {
        super::poll();
        if let Some(r) = tcp_accept(listener) { return Some(r); }
        if timeout_ticks != 0 && crate::time::pit::ticks().wrapping_sub(inicio) > timeout_ticks { return None; }
        crate::process::scheduler::wait_hlt_preemptible();
    }
}

/// Abre una conexión TCP activa
pub fn tcp_connect(remote_ip: IpAddr, remote_port: u16, local_port: u16) -> Result<usize, i32> {
    // [IPv6 REAL] antes de elegir la dirección de origen hay que tener el RA
    // procesado (dirección global SLAAC + encaminador); sin él, -ENETUNREACH.
    if let IpAddr::V6(d) = remote_ip {
        if !d.is_link_local() && !crate::net::ipv6::is_local_v6(&d) && !crate::net::ipv6::ensure_configured(100) {
            crate::drivers::serial::write_str("[TCP] IPv6 sin autoconfigurar (sin Router Advertisement): red inalcanzable\n");
            return Err(-101);
        }
    }
    let mut table = TCP_TABLE.lock();
    for i in 0..MAX_TCP_CONNS {
        if !table.conns[i].in_use {
            let conn = &mut table.conns[i];
            // [CIERRE TCP] Estado limpio: una ranura reutilizada arrastraba la
            // cola de retransmision, cwnd y RTO de la conexion anterior y el
            // temporizador la abortaba ("tras 8 retransmisiones") al conectar.
            *conn = TcpConnection::new();
            conn.in_use = true;
            conn.local_ip = match remote_ip { IpAddr::V4(d) => IpAddr::V4(if d[0] == 127 { d } else { get_ip() }), IpAddr::V6(d) => IpAddr::V6(crate::net::ipv6::pick_source(&d)) };
            conn.local_port = local_port;
            conn.remote_ip = remote_ip;
            conn.remote_port = remote_port;
            conn.state = TcpState::SynSent;
            conn.seq_num = 1000 + (i as u32 * 500);
            conn.snd_una = conn.seq_num;
            conn.ack_num = 0;

            // Enviar SYN inicial (ofrece SACK-Permitted, RFC 2018 §2)
            let _ = send_tcp_segment_full(conn.local_ip, remote_ip, local_port, remote_port, conn.seq_num, 0, TCP_FLAG_SYN, &[], RX_WINDOW_MAX, &syn_options(true, true, Some(0)));
            conn.seq_num = conn.seq_num.wrapping_add(1);
            conn.syn_sent_ticks = crate::time::pit::ticks();
            conn.syn_retries = 0;
            // [FRENTE B] Se queda en SYN-SENT: `tcp_wait_established` (llamado
            // por sys_connect) procesa la red hasta recibir el SYN-ACK real.
            // Antes se marcaba Established al instante y los datos salian con
            // ACK=0: el servidor los descartaba y "curl" fingia la descarga
            // leyendo /var/pkg/repo.
            return Ok(i);
        }
    }
    Err(-24) // -EMFILE
}

/// [FRENTE B] Estado actual de una conexión (para esperas en el despachador).
pub fn tcp_state(conn_id: usize) -> Option<TcpState> {
    let table = TCP_TABLE.lock();
    if conn_id >= MAX_TCP_CONNS || !table.conns[conn_id].in_use { return None; }
    Some(table.conns[conn_id].state)
}

/// [FRENTE B] Procesa la red hasta que la conexión queda establecida o vence
/// el plazo (en ticks del PIT). Devuelve Ok(()) o Err(-110 ETIMEDOUT).
pub fn tcp_wait_established(conn_id: usize, timeout_ticks: u64) -> Result<(), i32> {
    let inicio = crate::time::pit::ticks();
    loop {
        super::poll();
        match tcp_state(conn_id) {
            Some(TcpState::Established) => return Ok(()),
            Some(TcpState::SynSent) => { tcp_retransmit_syn_if_due(conn_id); }
            Some(_) | None => return Err(-111), // ECONNREFUSED / cerrada
        }
        if crate::time::pit::ticks().wrapping_sub(inicio) > timeout_ticks {
            tcp_abort(conn_id);
            return Err(-110);
        }
        crate::process::scheduler::wait_hlt_preemptible();
    }
}

/// [CIERRE RED] Retransmite el SYN si venció el RTO (0,5 s, 1 s, 2 s, …; RFC 6298
/// con backoff exponencial). Devuelve true si se retransmitió.
pub fn tcp_retransmit_syn_if_due(conn_id: usize) -> bool {
    let mut table = TCP_TABLE.lock();
    if conn_id >= MAX_TCP_CONNS || !table.conns[conn_id].in_use { return false; }
    let now = crate::time::pit::ticks();
    let conn = &mut table.conns[conn_id];
    if conn.state != TcpState::SynSent { return false; }
    let rto = 50u64 << conn.syn_retries.min(6); // ticks de 10 ms
    if now.wrapping_sub(conn.syn_sent_ticks) < rto { return false; }
    conn.syn_retries += 1;
    conn.syn_sent_ticks = now;
    let (lip, rip, lp, rp, seq, retries) = (conn.local_ip, conn.remote_ip, conn.local_port, conn.remote_port, conn.seq_num.wrapping_sub(1), conn.syn_retries);
    drop(table); // el envio puede necesitar ARP (poll -> handle_tcp_packet -> TCP_TABLE)
    let _ = send_tcp_segment_full(lip, rip, lp, rp, seq, 0, TCP_FLAG_SYN, &[], RX_WINDOW_MAX, &syn_options(true, true, Some(0)));
    crate::drivers::serial::write_str("[TCP] SYN retransmitido (intento ");
    crate::drivers::serial::write_usize(retries as usize);
    crate::drivers::serial::write_str(", RTO ");
    crate::drivers::serial::write_usize((rto * 10) as usize);
    crate::drivers::serial::write_str(" ms)\n");
    true
}

/// [CIERRE RED] Instantánea de las conexiones para la consola.
pub struct TcpInfo { pub id: usize, pub state: TcpState, pub local_ip: IpAddr, pub local_port: u16, pub remote_ip: IpAddr, pub remote_port: u16,
    pub rx: usize, pub syn_retries: u8, pub cwnd: u32, pub ssthresh: u32, pub rto_ms: u64, pub srtt_ms: u64, pub in_flight: u32, pub retx_rto: u32, pub retx_fast: u32, pub rtt_samples: u32, pub rwnd: u32, pub adv_wnd: u32, pub ws_ok: bool, pub ts_ok: bool }

pub fn tcp_snapshot() -> alloc::vec::Vec<TcpInfo> {
    let table = TCP_TABLE.lock();
    let mut v = alloc::vec::Vec::new();
    for (i, c) in table.conns.iter().enumerate() {
        if c.in_use { v.push(TcpInfo { id: i, state: c.state, local_ip: c.local_ip, local_port: c.local_port, remote_ip: c.remote_ip, remote_port: c.remote_port,
            rx: c.rx_buffer.len(), syn_retries: c.syn_retries, cwnd: c.cwnd, ssthresh: c.ssthresh, rto_ms: c.rto_ticks * 10, srtt_ms: c.srtt * 10, in_flight: c.in_flight(),
            retx_rto: c.retx_rto, retx_fast: c.retx_fast, rtt_samples: c.rtt_samples, rwnd: c.remote_window, adv_wnd: c.adv_bytes() as u32, ws_ok: c.ws_ok, ts_ok: c.ts_ok }); }
    }
    v
}

/// Estadísticas globales acumuladas (incluye conexiones ya cerradas).
pub fn tcp_global_stats() -> (u32, u32) { let s = TCP_STATS.lock(); (s.0, s.1) }
static TCP_STATS: crate::arch::Spinlock<(u32, u32)> = crate::arch::Spinlock::new((0, 0));

/// [FRENTE B] Libera una conexión sin ceremonia (plazo vencido).
pub fn tcp_abort(conn_id: usize) {
    let mut table = TCP_TABLE.lock();
    if conn_id < MAX_TCP_CONNS {
        let c = &mut table.conns[conn_id];
        { let mut g = TCP_STATS.lock(); g.0 += c.retx_rto; g.1 += c.retx_fast; }
        c.state = TcpState::Closed;
        c.in_use = false;
        c.rx_buffer.clear(); c.ooo.clear();
        c.retx.clear();
    }
}

/// Envía datos por una conexión TCP establecida.
///
/// [CIERRE TCP] Segmenta a MSS, respeta min(cwnd, ventana del par), guarda
/// cada segmento en la cola de retransmisión y, si la ventana está llena,
/// procesa la red (ACKs, RTO) hasta que haya hueco (plazo 15 s).
pub fn tcp_send(conn_id: usize, data: &[u8]) -> Result<usize, i32> {
    let mut sent = 0usize;
    let inicio = crate::time::pit::ticks();
    while sent < data.len() {
        let mut table = TCP_TABLE.lock();
        if conn_id >= MAX_TCP_CONNS || !table.conns[conn_id].in_use { return Err(-9); }
        let conn = &mut table.conns[conn_id];
        // MSS efectivo: la cabecera IPv6 (40 B) deja 1440 B de carga en una trama de
        // 1500 B. Antes se troceaba siempre a 1460 y los segmentos llenos sobre IPv6
        // (1534 B de trama) se descartaban al emitir: toda transferencia grande por
        // IPv6 de salida se atascaba en el RTO.
        let chunk_len = (data.len() - sent).min(conn.mss() as usize);
        if conn.state != TcpState::Established && conn.state != TcpState::CloseWait {
            if sent > 0 { return Ok(sent); }
            return Err(if conn.state == TcpState::Closed { -32 } else { -107 }); // EPIPE / ENOTCONN
        }
        let window = conn.send_window();
        if conn.in_flight().saturating_add(chunk_len as u32) <= window.max(TCP_MSS) || conn.retx.is_empty() {
            let chunk = &data[sent..sent + chunk_len];
            let seq = conn.seq_num;
            let now = crate::time::pit::ticks();
            conn.retx.push_back(RetxSeg { seq, data: chunk.to_vec(), sent_ticks: now, retries: 0, sacked: false });
            conn.seq_num = conn.seq_num.wrapping_add(chunk_len as u32);
            let (lip, rip, lp, rp, ack) = (conn.local_ip, conn.remote_ip, conn.local_port, conn.remote_port, conn.ack_num);
            if drop_hit(&DROP_TX) {
                crate::drivers::serial::write_str("[TCP-PRUEBA] segmento de datos saliente NO enviado adrede (seq=");
                crate::drivers::serial::write_usize(seq as usize);
                crate::drivers::serial::write_str(", ");
                crate::drivers::serial::write_usize(chunk_len);
                crate::drivers::serial::write_str(" bytes): queda en la cola de retransmision\n");
            } else {
                if drop_hit(&STALE_TS) { conn.xmit_stale_ts(seq, chunk); }
                let _ = (lip, rip, lp, rp, ack); conn.xmit(seq, conn.ack_num, TCP_FLAG_ACK | TCP_FLAG_PSH, chunk, None, &[]);
            }
            sent += chunk_len;
            continue;
        }
        drop(table);
        // Ventana llena: esperar ACKs (la red se procesa aquí, incluido el RTO)
        if crate::time::pit::ticks().wrapping_sub(inicio) > 1500 { return if sent > 0 { Ok(sent) } else { Err(-11) }; }
        super::poll();
        crate::process::scheduler::wait_hlt_preemptible();
    }
    Ok(sent)
}

/// [CIERRE TCP] Temporizador de retransmisión (RFC 6298 §5): se invoca desde
/// `net::poll()`. Al vencer el RTO se reenvía el segmento más antiguo, se
/// duplica el RTO (backoff) y se reinicia el arranque lento (RFC 5681 §3.1).
pub fn tcp_timer() {
    let now = crate::time::pit::ticks();
    let mut pending: Vec<(IpAddr, IpAddr, u16, u16, u32, u32, Vec<u8>, u16, Vec<u8>)> = Vec::new();
    {
        let mut table = TCP_TABLE.lock();
        for i in 0..MAX_TCP_CONNS {
            let conn = &mut table.conns[i];
            if !conn.in_use { continue; }
            // Expiración de TIME-WAIT (2·MSL) y de conexiones huérfanas en cierre.
            let age = now.wrapping_sub(conn.close_ticks);
            let expira = match conn.state {
                TcpState::TimeWait => age >= TIME_WAIT_TICKS,
                TcpState::FinWait1 | TcpState::FinWait2 | TcpState::Closing | TcpState::LastAck => age >= ORPHAN_TICKS,
                _ => false,
            };
            if expira {
                conn.in_use = false; conn.state = TcpState::Closed; conn.retx.clear(); conn.rx_buffer.clear(); conn.ooo.clear();
                continue;
            }
            if conn.retx.is_empty() { continue; }
            let rto = conn.rto_ticks;
            let due = match conn.retx.front() { Some(f) => now.wrapping_sub(f.sent_ticks) >= rto, None => false };
            if !due { continue; }
            let flight = conn.in_flight();
            let retries = conn.retx.front().map(|f| f.retries).unwrap_or(0);
            if retries >= MAX_DATA_RETRIES {
                crate::drivers::serial::write_str("[TCP] conexion abortada: datos sin confirmar tras 8 retransmisiones\n");
                conn.state = TcpState::Closed; conn.retx.clear();
                continue;
            }
            conn.ssthresh = (flight / 2).max(2 * TCP_MSS);
            conn.cwnd = TCP_MSS;
            conn.rto_ticks = (rto * 2).min(RTO_MAX);
            conn.retx_rto += 1;
            conn.dup_acks = 0;
            let (lip, rip, lp, rp, ack_n) = (conn.local_ip, conn.remote_ip, conn.local_port, conn.remote_port, conn.ack_num);
            if let Some(f) = conn.retx.front_mut() {
                f.retries += 1; f.sent_ticks = now;
                crate::drivers::serial::write_str("[TCP] datos retransmitidos por RTO (seq=");
                crate::drivers::serial::write_usize(f.seq as usize);
                crate::drivers::serial::write_str(", ");
                crate::drivers::serial::write_usize(f.data.len());
                crate::drivers::serial::write_str(" bytes, intento ");
                crate::drivers::serial::write_usize(f.retries as usize);
                crate::drivers::serial::write_str(", RTO ");
                crate::drivers::serial::write_usize((rto * 10) as usize);
                crate::drivers::serial::write_str(" ms, cwnd=1 MSS, ssthresh=");
                crate::drivers::serial::write_usize(conn.ssthresh as usize);
                crate::drivers::serial::write_str(")\n");
                let (fs, fd) = (f.seq, f.data.clone());
                pending.push((lip, rip, lp, rp, fs, ack_n, fd, conn.adv_window(), conn.seg_options(&[])));
            }
        }
    }
    for (lip, rip, lp, rp, seq, ack, d, w, o) in pending { let _ = send_tcp_segment_full(lip, rip, lp, rp, seq, ack, TCP_FLAG_ACK | TCP_FLAG_PSH, &d, w, &o); }
}

/// Lee datos del buffer de recepción TCP
pub fn tcp_recv(conn_id: usize, buf: &mut [u8]) -> Result<usize, i32> {
    let mut table = TCP_TABLE.lock();
    if conn_id >= MAX_TCP_CONNS || !table.conns[conn_id].in_use {
        return Err(-9); // -EBADF
    }
    let conn = &mut table.conns[conn_id];
    if conn.rx_buffer.is_empty() {
        if conn.state == TcpState::CloseWait || conn.state == TcpState::Closed {
            return Ok(0); // EOF
        }
        return Err(-11); // -EAGAIN
    }

    let antes = conn.adv_bytes();
    let count = buf.len().min(conn.rx_buffer.len());
    for i in 0..count {
        if let Some(b) = conn.rx_buffer.pop_front() {
            buf[i] = b;
        }
    }
    // Actualización de ventana (RFC 9293 §3.8.6.2): si la lectura reabre una
    // ventana que estaba cerrada o casi cerrada, se avisa al emisor para que
    // no dependa de sus sondas de persistencia.
    let despues = conn.adv_bytes();
    if conn.state == TcpState::Established && antes < TCP_MSS as usize && despues >= TCP_MSS as usize {
        { let mut st = RX_WND_STATS.lock(); st.1 += 1; }
        conn.xmit(conn.seq_num, conn.ack_num, TCP_FLAG_ACK, &[], None, &[]);
    }
    Ok(count)
}

/// Cierra una conexión TCP
pub fn tcp_close(conn_id: usize) {
    let mut table = TCP_TABLE.lock();
    if conn_id < MAX_TCP_CONNS && table.conns[conn_id].in_use {
        let conn = &mut table.conns[conn_id];
        { let mut g = TCP_STATS.lock(); g.0 += conn.retx_rto; g.1 += conn.retx_fast; conn.retx_rto = 0; conn.retx_fast = 0; }
        if conn.ts_ok && conn.ws_ok && conn.ts_rtt_samples > 0 {
            crate::drivers::serial::write_str("[TCP-7323] cierre con marcas de tiempo verificadas: ");
            crate::drivers::serial::write_usize(conn.ts_rtt_samples as usize);
            crate::drivers::serial::write_str(" muestras RTT por TSecr, ventana del par escalada (2^");
            crate::drivers::serial::write_usize(conn.snd_wscale as usize);
            crate::drivers::serial::write_str(") = ");
            crate::drivers::serial::write_usize(conn.remote_window as usize);
            crate::drivers::serial::write_str(" bytes\n");
        }
        if conn.state == TcpState::Established || conn.state == TcpState::CloseWait {
            // Cierre activo (FIN-WAIT-1) o pasivo tras el FIN del par (LAST-ACK).
            // Antes, en CLOSE-WAIT se liberaba la ranura SIN enviar FIN: el par se
            // quedaba en FIN-WAIT-2 para siempre (visible en el bucle local, donde
            // el par es esta misma pila, y en el anfitrión como huérfanas).
            conn.state = if conn.state == TcpState::Established { TcpState::FinWait1 } else { TcpState::LastAck };
            conn.close_ticks = crate::time::pit::ticks();
            conn.xmit(conn.seq_num, conn.ack_num, TCP_FLAG_FIN | TCP_FLAG_ACK, &[], None, &[]);
            conn.seq_num = conn.seq_num.wrapping_add(1);
        } else {
            conn.state = TcpState::Closed;
            conn.in_use = false;
            conn.retx.clear();
        }
    }
}
