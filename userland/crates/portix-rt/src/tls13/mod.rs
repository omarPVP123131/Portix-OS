//! TLS 1.3 real (RFC 8446) para Portix OS — Rust `no_std`, sin crates.
//!
//! * **Cliente** (`TlsClient`): X25519 + TLS_CHACHA20_POLY1305_SHA256; firmas
//!   del servidor RSA-PSS/PKCS#1, ECDSA P-256, ECDSA P-384 y Ed25519; cadena
//!   X.509 contra el almacén de raíces, nombre de host y vigencia (RTC real);
//!   reanudación por ticket PSK (`psk_dhe_ke`) y `KeyUpdate` en ambos sentidos.
//! * **Servidor** (`TlsServer`): mismo conjunto de primitivas, autenticación
//!   ECDSA P-256 con clave privada real, emisión de tickets *stateless*
//!   cifrados (ChaCha20-Poly1305 con clave del proceso) y aceptación de PSK.
//!
//! Todo lo no soportado se rechaza con un motivo explícito; nunca se "da por
//! bueno" un paso que no se verificó.

pub mod crypto;
pub mod p256;
pub mod ecgen;
pub mod ed25519;
pub mod x509;

use alloc::string::String;
use alloc::vec::Vec;
use crate::syscall;
use crate::tls::{sha256, hkdf_extract, hkdf_expand_label};
use crypto::{aead_seal, aead_open, x25519, x25519_base, random_bytes, hmac, ct_eq, Hash};
use x509::{Certificate, PublicKey};
use p256::P256PrivateKey;

const REC_CCS: u8 = 20; const REC_ALERT: u8 = 21; const REC_HANDSHAKE: u8 = 22; const REC_APPDATA: u8 = 23;
const HS_CLIENT_HELLO: u8 = 1; const HS_SERVER_HELLO: u8 = 2; const HS_NEW_SESSION_TICKET: u8 = 4;
const HS_ENCRYPTED_EXT: u8 = 8; const HS_CERTIFICATE: u8 = 11; const HS_CERT_VERIFY: u8 = 15; const HS_FINISHED: u8 = 20;
const HS_KEY_UPDATE: u8 = 24; const HS_CERT_REQUEST: u8 = 13; const HS_MESSAGE_HASH: u8 = 254;
const GROUP_X25519: u16 = 0x001d; const GROUP_SECP256R1: u16 = 0x0017;
const ALERT_HANDSHAKE_FAILURE: u8 = 40; const ALERT_ILLEGAL_PARAMETER: u8 = 47; const ALERT_CERT_REQUIRED: u8 = 116; const ALERT_BAD_CERT: u8 = 42;
const SUITE_CHACHA20_POLY1305_SHA256: u16 = 0x1303;
const SIG_RSA_PKCS1_SHA256: u16 = 0x0401; const SIG_RSA_PSS_RSAE_SHA256: u16 = 0x0804;
const SIG_ECDSA_SECP256R1_SHA256: u16 = 0x0403; const SIG_RSA_PSS_RSAE_SHA384: u16 = 0x0805; const SIG_RSA_PKCS1_SHA384: u16 = 0x0501;
const SIG_ECDSA_SECP384R1_SHA384: u16 = 0x0503; const SIG_ED25519: u16 = 0x0807;
const EXT_SNI: u16 = 0; const EXT_GROUPS: u16 = 10; const EXT_SIGALGS: u16 = 13; const EXT_PSK: u16 = 41;
const EXT_VERSIONS: u16 = 43; const EXT_PSK_MODES: u16 = 45; const EXT_KEY_SHARE: u16 = 51;
const HRR_RANDOM: [u8; 32] = [0xCF,0x21,0xAD,0x74,0xE5,0x9A,0x61,0x11,0xBE,0x1D,0x8C,0x02,0x1E,0x65,0xB8,0x91,0xC2,0xA2,0x11,0x16,0x7A,0xBB,0x8C,0x5E,0x07,0x9E,0x09,0xE2,0xC8,0xA8,0x33,0x9C];
const TICKET_AAD: &[u8] = b"portix-tls13-ticket-v1";
const TICKET_LIFETIME: u32 = 7200;

/// Claves de tráfico de una dirección.
struct Keys { key: [u8; 32], iv: [u8; 12], seq: u64 }

impl Keys {
    fn from_secret(secret: &[u8; 32]) -> Keys {
        let k = hkdf_expand_label(secret, "key", &[], 32);
        let iv = hkdf_expand_label(secret, "iv", &[], 12);
        let mut key = [0u8; 32]; key.copy_from_slice(&k);
        let mut ivb = [0u8; 12]; ivb.copy_from_slice(&iv);
        Keys { key, iv: ivb, seq: 0 }
    }
    fn nonce(&mut self) -> [u8; 12] {
        let mut n = self.iv;
        let s = self.seq.to_be_bytes();
        for i in 0..8 { n[4 + i] ^= s[i]; }
        self.seq += 1;
        n
    }
}

fn derive_secret(secret: &[u8; 32], label: &str, transcript_hash: &[u8]) -> [u8; 32] {
    let v = hkdf_expand_label(secret, label, transcript_hash, 32);
    let mut o = [0u8; 32]; o.copy_from_slice(&v); o
}

fn put_u16(v: &mut Vec<u8>, x: u16) { v.extend_from_slice(&x.to_be_bytes()); }
fn put_u24(v: &mut Vec<u8>, x: usize) { v.push((x >> 16) as u8); v.push((x >> 8) as u8); v.push(x as u8); }
fn put_u32(v: &mut Vec<u8>, x: u32) { v.extend_from_slice(&x.to_be_bytes()); }
fn rd_u16(b: &[u8]) -> u16 { u16::from_be_bytes([b[0], b[1]]) }
fn rd_u24(b: &[u8]) -> usize { ((b[0] as usize) << 16) | ((b[1] as usize) << 8) | b[2] as usize }
fn rd_u32(b: &[u8]) -> u32 { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) }
fn hs_msg(t: u8, body: &[u8]) -> Vec<u8> { let mut m = Vec::with_capacity(body.len() + 4); m.push(t); put_u24(&mut m, body.len()); m.extend_from_slice(body); m }

/// Lee exactamente `n` bytes del socket (bloqueante, con reintentos ante EAGAIN).
fn read_exact(fd: i32, n: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(n);
    let mut buf = [0u8; 4096];
    let mut eagain = 0;
    while out.len() < n {
        let want = (n - out.len()).min(buf.len());
        let r = syscall::sys_read(fd, buf.as_mut_ptr(), want);
        if r > 0 { out.extend_from_slice(&buf[..r as usize]); eagain = 0; continue; }
        if r == 0 { return Err(String::from("conexion cerrada por el servidor (FIN)")); }
        if r == -11 { eagain += 1; if eagain >= 6 { return Err(String::from("tiempo de espera agotado leyendo del socket")); } continue; }
        return Err(alloc::format!("read() fallo (err={})", r));
    }
    Ok(out)
}

fn write_all(fd: i32, data: &[u8]) -> Result<(), String> {
    let mut off = 0;
    while off < data.len() {
        let r = syscall::sys_write(fd, data[off..].as_ptr(), data.len() - off);
        if r <= 0 { return Err(alloc::format!("write() fallo (err={})", r)); }
        off += r as usize;
    }
    Ok(())
}

/// Lee un registro TLS en claro: (tipo, contenido).
fn read_record(fd: i32) -> Result<(u8, Vec<u8>), String> {
    let hdr = read_exact(fd, 5)?;
    let len = rd_u16(&hdr[3..5]) as usize;
    if len > 16384 + 256 { return Err(alloc::format!("registro demasiado grande ({} bytes)", len)); }
    let body = read_exact(fd, len)?;
    Ok((hdr[0], body))
}

fn write_plain_record(fd: i32, ctype: u8, data: &[u8]) -> Result<(), String> {
    let mut rec = Vec::with_capacity(data.len() + 5);
    rec.push(ctype); put_u16(&mut rec, 0x0303); put_u16(&mut rec, data.len() as u16); rec.extend_from_slice(data);
    write_all(fd, &rec)
}

// ───────────────────────── Capa de registro común ─────────────────────────

/// Estado de registro compartido por cliente y servidor: claves de tráfico
/// por dirección, los secretos de los que derivan (para `KeyUpdate`), datos
/// pendientes y buffer de mensajes de handshake post-apretón.
struct Record {
    fd: i32,
    write_keys: Keys,
    read_keys: Keys,
    write_secret: [u8; 32],
    read_secret: [u8; 32],
    pending: Vec<u8>,
    hs_buf: Vec<u8>,
    closed: bool,
    /// KeyUpdate recibidos del par (y aplicados a la clave de lectura).
    pub key_updates_rx: u32,
    /// KeyUpdate enviados (y aplicados a la clave de escritura).
    pub key_updates_tx: u32,
}

/// Evento devuelto por la capa de registro al leer.
enum RecordEvent { Data(Vec<u8>), Handshake(Vec<u8>), Closed }

impl Record {
    fn new(fd: i32, write_secret: &[u8; 32], read_secret: &[u8; 32]) -> Record {
        Record { fd, write_keys: Keys::from_secret(write_secret), read_keys: Keys::from_secret(read_secret),
                 write_secret: *write_secret, read_secret: *read_secret, pending: Vec::new(), hs_buf: Vec::new(), closed: false,
                 key_updates_rx: 0, key_updates_tx: 0 }
    }
    fn set_write(&mut self, s: &[u8; 32]) { self.write_secret = *s; self.write_keys = Keys::from_secret(s); }
    fn set_read(&mut self, s: &[u8; 32]) { self.read_secret = *s; self.read_keys = Keys::from_secret(s); }

    fn send(&mut self, ctype: u8, data: &[u8]) -> Result<(), String> {
        let mut pt = data.to_vec(); pt.push(ctype);
        let total = pt.len() + 16;
        let mut aad = Vec::with_capacity(5); aad.push(REC_APPDATA); put_u16(&mut aad, 0x0303); put_u16(&mut aad, total as u16);
        let nonce = self.write_keys.nonce();
        let ct = aead_seal(&self.write_keys.key, &nonce, &aad, &pt);
        let mut rec = aad; rec.extend_from_slice(&ct);
        write_all(self.fd, &rec)
    }

    fn send_hs(&mut self, msg: &[u8]) -> Result<(), String> { self.send(REC_HANDSHAKE, msg) }

    /// Descifra un registro de datos de aplicación: (tipo interno, texto claro).
    fn open(&mut self, body: &[u8]) -> Result<(u8, Vec<u8>), String> {
        let mut aad = Vec::with_capacity(5); aad.push(REC_APPDATA); put_u16(&mut aad, 0x0303); put_u16(&mut aad, body.len() as u16);
        let nonce = self.read_keys.nonce();
        let mut pt = aead_open(&self.read_keys.key, &nonce, &aad, body).ok_or_else(|| String::from("[TLS-AEAD-FAIL] etiqueta Poly1305 invalida"))?;
        while let Some(&0) = pt.last() { pt.pop(); }
        let ctype = pt.pop().unwrap_or(0);
        Ok((ctype, pt))
    }

    /// Lee el siguiente registro cifrado y lo clasifica. Los `KeyUpdate` se
    /// procesan aquí mismo (clave de lectura rotada y, si el par lo pide,
    /// respuesta con nuestro propio KeyUpdate).
    fn next(&mut self) -> Result<RecordEvent, String> {
        loop {
            if self.closed { return Ok(RecordEvent::Closed); }
            // Mensajes de handshake ya completos en el buffer
            if self.hs_buf.len() >= 4 {
                let mlen = rd_u24(&self.hs_buf[1..4]);
                if self.hs_buf.len() >= 4 + mlen {
                    let msg: Vec<u8> = self.hs_buf.drain(..4 + mlen).collect();
                    if msg[0] == HS_KEY_UPDATE {
                        let requested = msg.get(4).copied().unwrap_or(0) == 1;
                        self.apply_peer_key_update(requested)?;
                        continue;
                    }
                    return Ok(RecordEvent::Handshake(msg));
                }
            }
            let (t, body) = match read_record(self.fd) {
                Ok(v) => v,
                Err(e) => { if e.contains("FIN") { self.closed = true; return Ok(RecordEvent::Closed); } return Err(e); }
            };
            if t == REC_CCS { continue; }
            if t == REC_ALERT { self.closed = true; return alert_event(&body); }
            if t != REC_APPDATA { return Err(alloc::format!("registro inesperado tipo {}", t)); }
            let (ctype, pt) = self.open(&body)?;
            match ctype {
                REC_APPDATA => { if !pt.is_empty() { return Ok(RecordEvent::Data(pt)); } }
                REC_HANDSHAKE => { self.hs_buf.extend_from_slice(&pt); }
                REC_ALERT => { self.closed = true; return alert_event(&pt); }
                _ => {}
            }
        }
    }

    fn apply_peer_key_update(&mut self, requested: bool) -> Result<(), String> {
        let ns = derive_secret(&self.read_secret, "traffic upd", &[]);
        self.set_read(&ns);
        self.key_updates_rx += 1;
        if requested { self.send_key_update(false)?; }
        Ok(())
    }

    /// Envía KeyUpdate y rota la clave de escritura (RFC 8446 §4.6.3).
    fn send_key_update(&mut self, request_peer: bool) -> Result<(), String> {
        let m = hs_msg(HS_KEY_UPDATE, &[if request_peer { 1 } else { 0 }]);
        self.send_hs(&m)?;
        let ns = derive_secret(&self.write_secret, "traffic upd", &[]);
        self.set_write(&ns);
        self.key_updates_tx += 1;
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        for chunk in data.chunks(16000) { self.send(REC_APPDATA, chunk)?; }
        Ok(())
    }

    fn close(&mut self) { let _ = self.send(REC_ALERT, &[1, 0]); self.closed = true; }
}

/// Nombre RFC 8446 §6 de una alerta.
fn alert_name(d: u8) -> &'static str {
    match d { 0 => "close_notify", 10 => "unexpected_message", 20 => "bad_record_mac", 40 => "handshake_failure", 42 => "bad_certificate",
        43 => "unsupported_certificate", 44 => "certificate_revoked", 45 => "certificate_expired", 46 => "certificate_unknown", 47 => "illegal_parameter",
        48 => "unknown_ca", 49 => "access_denied", 50 => "decode_error", 51 => "decrypt_error", 70 => "protocol_version", 80 => "internal_error",
        109 => "missing_extension", 110 => "unsupported_extension", 112 => "unrecognized_name", 116 => "certificate_required", _ => "?" }
}

/// close_notify → cierre limpio; cualquier alerta fatal → error con su nombre.
fn alert_event(body: &[u8]) -> Result<RecordEvent, String> {
    let level = body.first().copied().unwrap_or(0); let desc = body.get(1).copied().unwrap_or(0);
    if desc == 0 { return Ok(RecordEvent::Closed); }
    if level == 2 { return Err(alloc::format!("[TLS-ALERT] alerta fatal del par: {} ({})", alert_name(desc), desc)); }
    Ok(RecordEvent::Closed)
}

/// Lector de mensajes de handshake durante el apretón (registros cifrados con
/// claves de handshake o en claro antes de ellas).
struct HsReader { buf: Vec<u8> }
impl HsReader {
    fn new() -> HsReader { HsReader { buf: Vec::new() } }
    fn pop(&mut self) -> Option<Vec<u8>> {
        if self.buf.len() < 4 { return None; }
        let mlen = rd_u24(&self.buf[1..4]);
        if self.buf.len() < 4 + mlen { return None; }
        Some(self.buf.drain(..4 + mlen).collect())
    }
    /// Siguiente mensaje de handshake cifrado con `rec.read_keys`.
    fn next_encrypted(&mut self, rec: &mut Record) -> Result<Vec<u8>, String> {
        loop {
            if let Some(m) = self.pop() { return Ok(m); }
            let (t, body) = read_record(rec.fd)?;
            if t == REC_CCS { continue; }
            if t == REC_ALERT { return Err(alloc::format!("alerta del par durante el handshake: desc={}", body.get(1).copied().unwrap_or(0))); }
            if t != REC_APPDATA { return Err(alloc::format!("registro inesperado tipo {} durante el handshake", t)); }
            let (ctype, pt) = rec.open(&body).map_err(|_| String::from("[TLS-AEAD-FAIL] etiqueta Poly1305 invalida en registro de handshake"))?;
            if ctype == REC_ALERT { return Err(alloc::format!("alerta cifrada del par: desc={}", pt.get(1).copied().unwrap_or(0))); }
            if ctype != REC_HANDSHAKE { return Err(alloc::format!("contenido inesperado {} durante el handshake", ctype)); }
            self.buf.extend_from_slice(&pt);
        }
    }
    /// Siguiente mensaje de handshake en claro.
    fn next_plain(&mut self, fd: i32) -> Result<Vec<u8>, String> {
        loop {
            if let Some(m) = self.pop() { return Ok(m); }
            let (t, body) = read_record(fd)?;
            if t == REC_CCS { continue; }
            if t == REC_ALERT { return Err(alloc::format!("alerta del par: nivel={} desc={}", body.first().copied().unwrap_or(0), body.get(1).copied().unwrap_or(0))); }
            if t != REC_HANDSHAKE { return Err(alloc::format!("se esperaba handshake en claro, llego tipo {}", t)); }
            self.buf.extend_from_slice(&body);
        }
    }
}

// ───────────────────────── Tickets / PSK ─────────────────────────

/// Ticket de sesión recibido de un servidor, con la PSK derivada del
/// `resumption_master_secret` (RFC 8446 §4.6.1). Vive en memoria del proceso.
#[derive(Clone)]
pub struct SessionTicket {
    pub hostname: String,
    pub ticket: Vec<u8>,
    psk: [u8; 32],
    age_add: u32,
    pub lifetime: u32,
    issued_epoch: i64,
}

impl SessionTicket {
    pub fn ticket_len(&self) -> usize { self.ticket.len() }
}

fn binder_key_from_psk(psk: &[u8; 32], external: bool) -> ([u8; 32], [u8; 32]) {
    let early = hkdf_extract(&[0u8; 32], psk);
    let bk = derive_secret(&early, if external { "ext binder" } else { "res binder" }, &sha256(&[]));
    (early, bk)
}

fn finished_mac(secret: &[u8; 32], th: &[u8; 32]) -> [u8; 32] {
    let fk = hkdf_expand_label(secret, "finished", &[], 32);
    hmac(&fk, th)
}

// ───────────────────────── Cliente ─────────────────────────

/// Identidad X.509 (cadena DER + clave P-256) para autenticarse ante el par
/// (certificado de cliente en mTLS, o certificado de servidor).
pub struct Identity { pub chain_der: Vec<Vec<u8>>, pub key: SigningKey }

/// Clave privada de la identidad TLS: cada variante produce su propio SignatureScheme.
pub enum SigningKey { P256(P256PrivateKey), Rsa(crypto::RsaPrivateKey), Ed25519(ed25519::Ed25519PrivateKey) }

impl SigningKey {
    /// Firma el contenido de CertificateVerify (RFC 8446 §4.4.3) y devuelve (esquema, firma).
    pub fn sign_cert_verify(&self, payload: &[u8]) -> (u16, Vec<u8>) {
        match self {
            SigningKey::P256(k) => (SIG_ECDSA_SECP256R1_SHA256, k.sign_der(&sha256(payload))),
            SigningKey::Rsa(k) => (SIG_RSA_PSS_RSAE_SHA256, k.sign_pss(Hash::Sha256, payload)),
            SigningKey::Ed25519(k) => (SIG_ED25519, k.sign(payload)),
        }
    }
    pub fn scheme_name(&self) -> &'static str {
        match self { SigningKey::P256(_) => "ecdsa_secp256r1_sha256", SigningKey::Rsa(_) => "rsa_pss_rsae_sha256", SigningKey::Ed25519(_) => "ed25519" }
    }
    pub fn describe(&self) -> String {
        match self { SigningKey::P256(_) => String::from("clave P-256"), SigningKey::Rsa(k) => alloc::format!("clave RSA {} bits (CRT)", k.bits()), SigningKey::Ed25519(_) => String::from("clave Ed25519") }
    }
    pub fn scheme(&self) -> u16 { self.sign_cert_verify(b"").0 }
}

impl Identity {
    /// Carga PEM de certificado(s) y clave (EC P-256, RSA o Ed25519) desde texto.
    pub fn from_pem(cert_pem: &str, key_pem: &str) -> Result<Identity, String> {
        let chain = x509::pem_blocks(cert_pem, "CERTIFICATE");
        if chain.is_empty() { return Err(String::from("el PEM no contiene certificados")); }
        let key = if let Some(k) = x509::parse_ec_private_key_pem(key_pem) { SigningKey::P256(k) }
            else if let Some(k) = x509::parse_rsa_private_key_pem(key_pem) {
                if !k.self_check() { return Err(String::from("la clave RSA no supera la autocomprobacion firma/verificacion")); }
                SigningKey::Rsa(k)
            }
            else if let Some(k) = x509::parse_ed25519_private_key_pem(key_pem) {
                if !k.self_check() { return Err(String::from("la clave Ed25519 no supera la autocomprobacion firma/verificacion")); }
                SigningKey::Ed25519(k)
            }
            else { return Err(String::from("la clave no es EC P-256, RSA ni Ed25519 valida (SEC1/PKCS#1/PKCS#8)")); };
        // La clave debe corresponder a la hoja: se comprueba contra la publica del certificado.
        if let Some(c) = x509::parse_certificate(&chain[0]) {
            let ok = match (&key, &c.public_key) {
                (SigningKey::P256(k), PublicKey::EcP256(pk)) => k.public_sec1() == pk.sec1_bytes(),
                (SigningKey::Rsa(k), PublicKey::Rsa(pk)) => k.n.to_be_bytes((k.n.bits() + 7) / 8) == pk.n.to_be_bytes((pk.n.bits() + 7) / 8),
                (SigningKey::Ed25519(k), PublicKey::Ed25519(pk)) => k.public_bytes() == pk.bytes(),
                _ => false,
            };
            if !ok { return Err(String::from("la clave privada no corresponde a la clave publica del certificado de cliente")); }
        }
        Ok(Identity { chain_der: chain, key })
    }
    pub fn leaf_cn(&self) -> String { x509::parse_certificate(&self.chain_der[0]).map(|c| c.subject_cn).unwrap_or_default() }
}

/// Mensaje Certificate (contexto + lista de DER con extensiones vacías).
fn build_certificate_msg(ctx: &[u8], chain_der: &[Vec<u8>]) -> Vec<u8> {
    let mut cb = Vec::new(); cb.push(ctx.len() as u8); cb.extend_from_slice(ctx);
    let mut list = Vec::new();
    for der in chain_der.iter() { put_u24(&mut list, der.len()); list.extend_from_slice(der); put_u16(&mut list, 0); }
    put_u24(&mut cb, list.len()); cb.extend_from_slice(&list);
    hs_msg(HS_CERTIFICATE, &cb)
}

/// Analiza un mensaje Certificate: (contexto, cadena).
fn parse_certificate_msg(mb: &[u8]) -> Result<(Vec<u8>, Vec<Certificate>), String> {
    if mb.is_empty() { return Err(String::from("Certificate vacio")); }
    let ctx_len = mb[0] as usize; let mut p = 1 + ctx_len;
    if mb.len() < p { return Err(String::from("Certificate truncado")); }
    let ctx = mb[1..p].to_vec();
    if mb.len() < p + 3 { return Err(String::from("Certificate truncado")); }
    let list_len = rd_u24(&mb[p..]); p += 3;
    let end = (p + list_len).min(mb.len());
    let mut chain = Vec::new();
    while p + 3 <= end {
        let clen = rd_u24(&mb[p..]); p += 3;
        if p + clen > end { return Err(String::from("entrada de certificado truncada")); }
        let der = &mb[p..p + clen]; p += clen;
        if p + 2 > end { return Err(String::from("extensiones de certificado truncadas")); }
        let elen = rd_u16(&mb[p..]) as usize; p += 2 + elen;
        let c = x509::parse_certificate(der).ok_or_else(|| String::from("[TLS-X509-FAIL] certificado DER no analizable"))?;
        chain.push(c);
    }
    Ok((ctx, chain))
}

/// Verifica un CertificateVerify (`mb` = cuerpo) con la clave de la hoja.
/// Devuelve el nombre del algoritmo.
fn verify_cert_verify(leaf: &Certificate, mb: &[u8], th: &[u8; 32], server: bool) -> Result<&'static str, String> {
    if mb.len() < 4 { return Err(String::from("CertificateVerify truncado")); }
    let alg = rd_u16(mb); let slen = rd_u16(&mb[2..]) as usize;
    if mb.len() < 4 + slen { return Err(String::from("firma truncada")); }
    let sig = &mb[4..4 + slen];
    let signed = cert_verify_payload(th, server);
    let (name, ok) = match (&leaf.public_key, alg) {
        (PublicKey::Rsa(k), SIG_RSA_PSS_RSAE_SHA256) => ("rsa_pss_rsae_sha256", k.verify_pss(Hash::Sha256, &signed, sig)),
        (PublicKey::Rsa(k), SIG_RSA_PSS_RSAE_SHA384) => ("rsa_pss_rsae_sha384", k.verify_pss(Hash::Sha384, &signed, sig)),
        (PublicKey::Rsa(k), SIG_RSA_PKCS1_SHA256) => ("rsa_pkcs1_sha256", k.verify_pkcs1(Hash::Sha256, &signed, sig)),
        (PublicKey::EcP256(k), SIG_ECDSA_SECP256R1_SHA256) => ("ecdsa_secp256r1_sha256", k.verify_der(&sha256(&signed), sig)),
        (PublicKey::EcP384(k), SIG_ECDSA_SECP384R1_SHA384) => ("ecdsa_secp384r1_sha384", k.verify_der(&crypto::sha384(&signed), sig)),
        (PublicKey::Ed25519(k), SIG_ED25519) => ("ed25519", k.verify(&signed, sig)),
        (PublicKey::EcP256(_), _) => return Err(alloc::format!("[TLS-SIG-ALG-NO-SOPORTADO] clave P-256 con algoritmo 0x{:04x}", alg)),
        (PublicKey::EcP384(_), _) => return Err(alloc::format!("[TLS-SIG-ALG-NO-SOPORTADO] clave P-384 con algoritmo 0x{:04x}", alg)),
        (PublicKey::Ed25519(_), _) => return Err(alloc::format!("[TLS-SIG-ALG-NO-SOPORTADO] clave Ed25519 con algoritmo 0x{:04x}", alg)),
        (PublicKey::EcOtra, _) => return Err(String::from("[TLS-SIG-ALG-NO-SOPORTADO] curva EC distinta de P-256/P-384")),
        _ => return Err(alloc::format!("[TLS-SIG-ALG-NO-SOPORTADO] algoritmo de firma 0x{:04x}", alg)),
    };
    if !ok { return Err(String::from("[TLS-CERT-VERIFY-FAIL] la firma sobre el transcript NO verifica")); }
    Ok(name)
}

pub struct TlsClient {
    rec: Record,
    pub hostname: String,
    pub cipher: &'static str,
    pub server_sig: &'static str,
    pub trust_anchor: String,
    pub leaf_cn: String,
    /// `true` si el servidor aceptó la PSK ofrecida (no hubo Certificate).
    pub resumed: bool,
    /// Último ticket recibido vía NewSessionTicket (si lo hubo).
    pub ticket: Option<SessionTicket>,
    /// `true` si el servidor pidió certificado y enviamos el nuestro (mTLS).
    pub client_authenticated: bool,
    /// Esquema con el que firmamos nuestro CertificateVerify (mTLS), o "".
    pub client_sig: &'static str,
    res_master: [u8; 32],
    transcript_hash_final: [u8; 32],
}

/// ClientHello. Si `psk` está presente, se añade `psk_key_exchange_modes` y
/// `pre_shared_key` (última extensión) con un binder calculado sobre el
/// transcript truncado.
fn build_client_hello(hostname: &str, pubkey: &[u8; 32], random: &[u8; 32], session_id: &[u8; 32], psk: Option<&SessionTicket>, now: i64) -> Vec<u8> {
    let mut body = Vec::new();
    put_u16(&mut body, 0x0303);
    body.extend_from_slice(random);
    body.push(32); body.extend_from_slice(session_id);
    put_u16(&mut body, 2); put_u16(&mut body, SUITE_CHACHA20_POLY1305_SHA256);
    body.push(1); body.push(0);
    let mut ext = Vec::new();
    // server_name
    { let mut e = Vec::new(); let h = hostname.as_bytes();
      put_u16(&mut e, (h.len() + 3) as u16); e.push(0); put_u16(&mut e, h.len() as u16); e.extend_from_slice(h);
      put_u16(&mut ext, EXT_SNI); put_u16(&mut ext, e.len() as u16); ext.extend_from_slice(&e); }
    // supported_versions
    { put_u16(&mut ext, EXT_VERSIONS); put_u16(&mut ext, 3); ext.push(2); put_u16(&mut ext, 0x0304); }
    // supported_groups: x25519
    { put_u16(&mut ext, EXT_GROUPS); put_u16(&mut ext, 4); put_u16(&mut ext, 2); put_u16(&mut ext, 0x001d); }
    // signature_algorithms
    { let algs = [SIG_RSA_PSS_RSAE_SHA256, SIG_RSA_PKCS1_SHA256, SIG_RSA_PSS_RSAE_SHA384, SIG_RSA_PKCS1_SHA384,
                  SIG_ECDSA_SECP256R1_SHA256, SIG_ECDSA_SECP384R1_SHA384, SIG_ED25519];
      put_u16(&mut ext, EXT_SIGALGS); put_u16(&mut ext, (algs.len() * 2 + 2) as u16); put_u16(&mut ext, (algs.len() * 2) as u16);
      for a in algs { put_u16(&mut ext, a); } }
    // key_share
    { put_u16(&mut ext, EXT_KEY_SHARE); put_u16(&mut ext, 2 + 4 + 32); put_u16(&mut ext, 4 + 32); put_u16(&mut ext, 0x001d); put_u16(&mut ext, 32); ext.extend_from_slice(pubkey); }
    if let Some(t) = psk {
        // psk_key_exchange_modes: psk_dhe_ke
        { put_u16(&mut ext, EXT_PSK_MODES); put_u16(&mut ext, 2); ext.push(1); ext.push(1); }
        // pre_shared_key (debe ser la última)
        let age_ms = ((now - t.issued_epoch).max(0) as u64 * 1000) as u32;
        let obf = age_ms.wrapping_add(t.age_add);
        let mut e = Vec::new();
        put_u16(&mut e, (2 + t.ticket.len() + 4) as u16); put_u16(&mut e, t.ticket.len() as u16); e.extend_from_slice(&t.ticket); put_u32(&mut e, obf);
        put_u16(&mut e, 33); e.push(32); e.extend_from_slice(&[0u8; 32]); // binder provisional
        put_u16(&mut ext, EXT_PSK); put_u16(&mut ext, e.len() as u16); ext.extend_from_slice(&e);
    }
    put_u16(&mut body, ext.len() as u16); body.extend_from_slice(&ext);
    let mut hs = hs_msg(HS_CLIENT_HELLO, &body);
    if let Some(t) = psk {
        // Binder = HMAC(finished_key(binder_key), H(ClientHello sin binders))
        let (_, bk) = binder_key_from_psk(&t.psk, false);
        let cut = hs.len() - 35; // 2 (len binders) + 1 (len binder) + 32
        let th = sha256(&hs[..cut]);
        let mac = finished_mac(&bk, &th);
        let n = hs.len();
        hs[n - 32..].copy_from_slice(&mac);
    }
    hs
}

struct ServerHello { server_pub: [u8; 32], psk_selected: bool }

fn parse_server_hello(msg: &[u8]) -> Result<ServerHello, String> {
    if msg.len() < 4 || msg[0] != HS_SERVER_HELLO { return Err(String::from("se esperaba ServerHello")); }
    let b = &msg[4..];
    if b.len() < 38 { return Err(String::from("ServerHello truncado")); }
    if b[2..34] == HRR_RANDOM { return Err(String::from("el servidor pidio HelloRetryRequest (grupo no aceptado)")); }
    let sid_len = b[34] as usize;
    let mut p = 35 + sid_len;
    if b.len() < p + 3 { return Err(String::from("ServerHello truncado")); }
    let suite = rd_u16(&b[p..]); p += 2;
    if suite != SUITE_CHACHA20_POLY1305_SHA256 { return Err(alloc::format!("suite no soportada 0x{:04x}", suite)); }
    p += 1; // compression
    if b.len() < p + 2 { return Err(String::from("ServerHello sin extensiones")); }
    let ext_len = rd_u16(&b[p..]) as usize; p += 2;
    let exts = &b[p..(p + ext_len).min(b.len())];
    let mut q = 0; let mut server_pub = None; let mut v13 = false; let mut psk_selected = false;
    while q + 4 <= exts.len() {
        let t = rd_u16(&exts[q..]); let l = rd_u16(&exts[q + 2..]) as usize; q += 4;
        if q + l > exts.len() { break; }
        let d = &exts[q..q + l]; q += l;
        match t {
            EXT_VERSIONS => { if l == 2 && rd_u16(d) == 0x0304 { v13 = true; } }
            EXT_KEY_SHARE => { if l >= 4 && rd_u16(d) == 0x001d && rd_u16(&d[2..]) == 32 && l >= 36 { let mut k = [0u8; 32]; k.copy_from_slice(&d[4..36]); server_pub = Some(k); } }
            EXT_PSK => { if l == 2 && rd_u16(d) == 0 { psk_selected = true; } }
            _ => {}
        }
    }
    if !v13 { return Err(String::from("el servidor no negocio TLS 1.3")); }
    let server_pub = server_pub.ok_or_else(|| String::from("ServerHello sin key_share X25519"))?;
    Ok(ServerHello { server_pub, psk_selected })
}

/// Analiza el cuerpo de NewSessionTicket y deriva la PSK de reanudación.
fn parse_new_session_ticket(mb: &[u8], res_master: &[u8; 32], hostname: &str, now: i64) -> Option<SessionTicket> {
    if mb.len() < 9 { return None; }
    let lifetime = rd_u32(mb); let age_add = rd_u32(&mb[4..]);
    let nlen = mb[8] as usize; let mut p = 9;
    if mb.len() < p + nlen + 2 { return None; }
    let nonce = &mb[p..p + nlen]; p += nlen;
    let tlen = rd_u16(&mb[p..]) as usize; p += 2;
    if mb.len() < p + tlen { return None; }
    let ticket = mb[p..p + tlen].to_vec();
    let psk_v = hkdf_expand_label(res_master, "resumption", nonce, 32);
    let mut psk = [0u8; 32]; psk.copy_from_slice(&psk_v);
    Some(SessionTicket { hostname: String::from(hostname), ticket, psk, age_add, lifetime, issued_epoch: now })
}

impl TlsClient {
    /// Handshake TLS 1.3 completo sobre un socket TCP ya conectado.
    pub fn handshake(fd: i32, hostname: &str, roots: &[Certificate], now: i64, log: &mut dyn FnMut(&str)) -> Result<TlsClient, String> {
        Self::handshake_with(fd, hostname, roots, now, None, log)
    }

    /// Como `handshake`, ofreciendo además la PSK de `ticket` para reanudar.
    pub fn handshake_with(fd: i32, hostname: &str, roots: &[Certificate], now: i64, ticket: Option<&SessionTicket>, log: &mut dyn FnMut(&str)) -> Result<TlsClient, String> {
        Self::handshake_full(fd, hostname, roots, now, ticket, None, log)
    }

    /// Handshake completo con PSK opcional e identidad de cliente opcional (mTLS):
    /// si el servidor envía CertificateRequest se responde con Certificate +
    /// CertificateVerify (ECDSA P-256 real); sin identidad se envía Certificate vacío.
    pub fn handshake_full(fd: i32, hostname: &str, roots: &[Certificate], now: i64, ticket: Option<&SessionTicket>, identity: Option<&Identity>, log: &mut dyn FnMut(&str)) -> Result<TlsClient, String> {
        let ticket = ticket.filter(|t| t.hostname == hostname && (now - t.issued_epoch) >= 0 && ((now - t.issued_epoch) as u64) < t.lifetime as u64);
        // 1. ClientHello
        let mut priv_key = [0u8; 32]; random_bytes(&mut priv_key);
        let pubkey = x25519_base(&priv_key);
        let mut random = [0u8; 32]; random_bytes(&mut random);
        let mut sid = [0u8; 32]; random_bytes(&mut sid);
        let ch = build_client_hello(hostname, &pubkey, &random, &sid, ticket, now);
        write_plain_record(fd, REC_HANDSHAKE, &ch)?;
        let mut transcript: Vec<u8> = ch.clone();
        log(&alloc::format!("[TLS] ClientHello enviado ({} bytes): X25519 + TLS_CHACHA20_POLY1305_SHA256{}", ch.len(),
            if let Some(t) = ticket { alloc::format!(" + pre_shared_key (ticket de {} bytes, psk_dhe_ke)", t.ticket.len()) } else { String::new() }));

        // 2. ServerHello
        let mut hr = HsReader::new();
        let sh = hr.next_plain(fd).map_err(|e| if e.contains("alerta") { alloc::format!("{} tras ClientHello", e) } else { e })?;
        let shello = parse_server_hello(&sh)?;
        transcript.extend_from_slice(&sh);
        let resumed = shello.psk_selected && ticket.is_some();
        if shello.psk_selected && ticket.is_none() { return Err(String::from("el servidor selecciono una PSK que no ofrecimos")); }
        log(if resumed { "[TLS] ServerHello: TLS 1.3 negociado, PSK aceptada (reanudacion) + key_share X25519 del servidor" }
            else { "[TLS] ServerHello: TLS 1.3 negociado, key_share X25519 del servidor recibido" });

        // 3. Secretos de handshake
        let shared = x25519(&priv_key, &shello.server_pub);
        if shared.iter().all(|&b| b == 0) { return Err(String::from("secreto X25519 nulo (punto de orden baja)")); }
        let early = match (resumed, ticket) { (true, Some(t)) => hkdf_extract(&[0u8; 32], &t.psk), _ => hkdf_extract(&[0u8; 32], &[0u8; 32]) };
        let derived = derive_secret(&early, "derived", &sha256(&[]));
        let hs_secret = hkdf_extract(&derived, &shared);
        let th_sh = sha256(&transcript);
        let c_hs = derive_secret(&hs_secret, "c hs traffic", &th_sh);
        let s_hs = derive_secret(&hs_secret, "s hs traffic", &th_sh);
        let mut client = TlsClient {
            rec: Record::new(fd, &c_hs, &s_hs), hostname: String::from(hostname), cipher: "TLS_CHACHA20_POLY1305_SHA256",
            server_sig: if resumed { "psk (reanudacion)" } else { "?" }, trust_anchor: if resumed { String::from("reanudacion PSK") } else { String::new() },
            leaf_cn: String::new(), resumed, ticket: None, client_authenticated: false, client_sig: "", res_master: [0u8; 32], transcript_hash_final: [0u8; 32],
        };
        let mut cert_request_ctx: Option<Vec<u8>> = None;
        let mut cert_request_sigalgs: Option<Vec<u16>> = None;
        log("[TLS] Secreto compartido X25519 calculado; claves de handshake derivadas (HKDF-SHA256)");

        // 4. Mensajes cifrados del servidor hasta Finished
        let mut chain: Vec<Certificate> = Vec::new();
        let mut th_before_cv = [0u8; 32];
        let mut th_before_fin = sha256(&transcript);
        let mut server_finished_ok = false;
        let mut cert_verified = resumed;
        while !server_finished_ok {
            let msg = hr.next_encrypted(&mut client.rec)?;
            let mtype = msg[0]; let mb = &msg[4..];
            match mtype {
                HS_ENCRYPTED_EXT => { transcript.extend_from_slice(&msg); th_before_fin = sha256(&transcript); log("[TLS] EncryptedExtensions recibidas"); }
                HS_CERT_REQUEST => {
                    if resumed { return Err(String::from("CertificateRequest inesperado en una reanudacion PSK")); }
                    transcript.extend_from_slice(&msg);
                    let ctx_len = mb.first().copied().unwrap_or(0) as usize;
                    cert_request_ctx = Some(mb.get(1..1 + ctx_len).unwrap_or(&[]).to_vec());
                    // Extensiones: signature_algorithms (13) del servidor para elegir/validar nuestro esquema
                    {
                        let mut q = 1 + ctx_len;
                        if mb.len() >= q + 2 { let elen = rd_u16(&mb[q..]) as usize; q += 2; let end = (q + elen).min(mb.len());
                            while q + 4 <= end { let et = rd_u16(&mb[q..]); let el = rd_u16(&mb[q + 2..]) as usize; q += 4;
                                if et == 13 && q + 2 <= end { let ll = rd_u16(&mb[q..]) as usize; let mut algs = Vec::new(); let mut r = q + 2; while r + 2 <= (q + 2 + ll).min(end) { algs.push(rd_u16(&mb[r..])); r += 2; } cert_request_sigalgs = Some(algs); }
                                q += el; } }
                    }
                    log(&alloc::format!("[TLS] CertificateRequest recibido (mTLS): el servidor exige certificado de cliente{}", if identity.is_some() { "; se enviara el nuestro" } else { "; NO tenemos identidad" }));
                }
                HS_CERTIFICATE => {
                    if resumed { return Err(String::from("Certificate inesperado en una reanudacion PSK")); }
                    transcript.extend_from_slice(&msg);
                    let (_ctx, ch_) = parse_certificate_msg(mb)?;
                    chain = ch_;
                    if chain.is_empty() { return Err(String::from("el servidor no envio certificados")); }
                    log(&alloc::format!("[TLS] Certificate: {} certificado(s); hoja CN='{}' SAN={:?}", chain.len(), chain[0].subject_cn, chain[0].dns_names));
                    th_before_cv = sha256(&transcript);
                }
                HS_CERT_VERIFY => {
                    if chain.is_empty() { return Err(String::from("CertificateVerify antes de Certificate")); }
                    if mb.len() < 4 { return Err(String::from("CertificateVerify truncado")); }
                    let alg = rd_u16(mb); let slen = rd_u16(&mb[2..]) as usize;
                    if mb.len() < 4 + slen { return Err(String::from("firma truncada")); }
                    let sig = &mb[4..4 + slen];
                    let signed = cert_verify_payload(&th_before_cv, true);
                    let ok = match (&chain[0].public_key, alg) {
                        (PublicKey::Rsa(k), SIG_RSA_PSS_RSAE_SHA256) => { client.server_sig = "rsa_pss_rsae_sha256"; k.verify_pss(Hash::Sha256, &signed, sig) }
                        (PublicKey::Rsa(k), SIG_RSA_PSS_RSAE_SHA384) => { client.server_sig = "rsa_pss_rsae_sha384"; k.verify_pss(Hash::Sha384, &signed, sig) }
                        (PublicKey::Rsa(k), SIG_RSA_PKCS1_SHA256) => { client.server_sig = "rsa_pkcs1_sha256"; k.verify_pkcs1(Hash::Sha256, &signed, sig) }
                        (PublicKey::EcP256(k), SIG_ECDSA_SECP256R1_SHA256) => { client.server_sig = "ecdsa_secp256r1_sha256"; k.verify_der(&sha256(&signed), sig) }
                        (PublicKey::EcP384(k), SIG_ECDSA_SECP384R1_SHA384) => { client.server_sig = "ecdsa_secp384r1_sha384"; k.verify_der(&crypto::sha384(&signed), sig) }
                        (PublicKey::Ed25519(k), SIG_ED25519) => { client.server_sig = "ed25519"; k.verify(&signed, sig) }
                        (PublicKey::EcP256(_), _) => return Err(alloc::format!("[TLS-SIG-ALG-NO-SOPORTADO] clave P-256 con algoritmo 0x{:04x}", alg)),
                        (PublicKey::EcP384(_), _) => return Err(alloc::format!("[TLS-SIG-ALG-NO-SOPORTADO] clave P-384 con algoritmo 0x{:04x}", alg)),
                        (PublicKey::Ed25519(_), _) => return Err(alloc::format!("[TLS-SIG-ALG-NO-SOPORTADO] clave Ed25519 con algoritmo 0x{:04x}", alg)),
                        (PublicKey::EcOtra, _) => return Err(String::from("[TLS-SIG-ALG-NO-SOPORTADO] curva EC distinta de P-256/P-384")),
                        _ => return Err(alloc::format!("[TLS-SIG-ALG-NO-SOPORTADO] algoritmo de firma 0x{:04x}", alg)),
                    };
                    if !ok { return Err(String::from("[TLS-CERT-VERIFY-FAIL] la firma del servidor sobre el transcript NO verifica")); }
                    log(&alloc::format!("[TLS] CertificateVerify: firma {} del servidor verificada sobre el transcript", client.server_sig));
                    let anchor = x509::validate_chain(&chain, roots, hostname, now).map_err(|e| alloc::format!("[TLS-X509-FAIL] {}", e))?;
                    client.trust_anchor = anchor.clone();
                    client.leaf_cn = chain[0].subject_cn.clone();
                    cert_verified = true;
                    log(&alloc::format!("[TLS] Cadena X.509 valida: '{}' -> raiz de confianza '{}' (host y vigencia comprobados)", chain[0].subject_cn, anchor));
                    transcript.extend_from_slice(&msg);
                    th_before_fin = sha256(&transcript);
                }
                HS_FINISHED => {
                    if !cert_verified { return Err(String::from("Finished sin certificado verificado")); }
                    let expected = finished_mac(&s_hs, &th_before_fin);
                    if !ct_eq(&expected, mb) { return Err(String::from("[TLS-FINISHED-FAIL] el Finished del servidor no coincide")); }
                    transcript.extend_from_slice(&msg);
                    server_finished_ok = true;
                    log("[TLS] Finished del servidor verificado (HMAC-SHA256 del transcript)");
                }
                _ => return Err(alloc::format!("mensaje de handshake inesperado tipo {}", mtype)),
            }
        }

        // 5. Secretos de aplicación
        let th_fin = sha256(&transcript);
        let derived2 = derive_secret(&hs_secret, "derived", &sha256(&[]));
        let master = hkdf_extract(&derived2, &[0u8; 32]);
        let c_ap = derive_secret(&master, "c ap traffic", &th_fin);
        let s_ap = derive_secret(&master, "s ap traffic", &th_fin);

        // 6. (mTLS) Certificate + CertificateVerify del cliente, aún con claves de handshake
        write_all(fd, &[REC_CCS, 3, 3, 0, 1, 1])?;
        if let Some(ctx) = cert_request_ctx.as_ref() {
            match identity {
                Some(id) => {
                    let cm = build_certificate_msg(ctx, &id.chain_der);
                    client.rec.send_hs(&cm)?; transcript.extend_from_slice(&cm);
                    let th = sha256(&transcript);
                    let (scheme, sig) = id.key.sign_cert_verify(&cert_verify_payload(&th, false));
                    if let Some(algs) = cert_request_sigalgs.as_ref() { if !algs.contains(&scheme) { return Err(alloc::format!("el servidor no admite nuestro esquema de firma de cliente {} en su CertificateRequest", id.key.scheme_name())); } }
                    let mut cvb = Vec::new(); put_u16(&mut cvb, scheme); put_u16(&mut cvb, sig.len() as u16); cvb.extend_from_slice(&sig);
                    let cv = hs_msg(HS_CERT_VERIFY, &cvb);
                    client.rec.send_hs(&cv)?; transcript.extend_from_slice(&cv);
                    client.client_authenticated = true;
                    client.client_sig = id.key.scheme_name();
                    log(&alloc::format!("[TLS] Certificate de cliente ('{}', {} cert) + CertificateVerify {} ({} bytes de firma) enviados", id.leaf_cn(), id.chain_der.len(), id.key.scheme_name(), sig.len()));
                }
                None => {
                    let cm = build_certificate_msg(ctx, &[]);
                    client.rec.send_hs(&cm)?; transcript.extend_from_slice(&cm);
                    log("[TLS] Certificate de cliente VACIO enviado (sin identidad): el servidor decidira");
                }
            }
        }
        // 7. Finished del cliente
        let th_client_fin = sha256(&transcript);
        let verify = finished_mac(&c_hs, &th_client_fin);
        let fin = hs_msg(HS_FINISHED, &verify);
        client.rec.send_hs(&fin)?;
        transcript.extend_from_slice(&fin);
        client.res_master = derive_secret(&master, "res master", &sha256(&transcript));
        client.transcript_hash_final = sha256(&transcript);
        client.rec.set_write(&c_ap);
        client.rec.set_read(&s_ap);
        log(if resumed { "[TLS] Finished del cliente enviado; sesion REANUDADA con PSK, canal de aplicacion activo" }
            else { "[TLS] Finished del cliente enviado; canal de aplicacion activo (claves c/s ap traffic)" });
        Ok(client)
    }

    /// Envía datos de aplicación cifrados.
    pub fn write(&mut self, data: &[u8]) -> Result<(), String> { self.rec.write(data) }

    /// Lee datos de aplicación descifrados; `Ok(vacío)` al cerrar el servidor.
    /// Procesa NewSessionTicket (guarda el ticket) y KeyUpdate por el camino.
    pub fn read(&mut self) -> Result<Vec<u8>, String> {
        loop {
            if !self.rec.pending.is_empty() { return Ok(core::mem::take(&mut self.rec.pending)); }
            match self.rec.next()? {
                RecordEvent::Data(d) => return Ok(d),
                RecordEvent::Closed => return Ok(Vec::new()),
                RecordEvent::Handshake(m) => {
                    if m[0] == HS_NEW_SESSION_TICKET {
                        if let Some(t) = parse_new_session_ticket(&m[4..], &self.res_master, &self.hostname, now_epoch()) { self.ticket = Some(t); }
                    }
                }
            }
        }
    }

    /// Solicita rotación de claves: envía KeyUpdate(update_requested) y rota
    /// la clave de escritura. El par debe responder con su propio KeyUpdate.
    pub fn key_update(&mut self, request_peer: bool) -> Result<(), String> { self.rec.send_key_update(request_peer) }
    pub fn key_updates(&self) -> (u32, u32) { (self.rec.key_updates_tx, self.rec.key_updates_rx) }
    pub fn is_closed(&self) -> bool { self.rec.closed }

    /// Envía close_notify.
    pub fn close(&mut self) { self.rec.close(); }
}

fn cert_verify_payload(th: &[u8; 32], server: bool) -> Vec<u8> {
    let mut signed = alloc::vec![0x20u8; 64];
    signed.extend_from_slice(if server { b"TLS 1.3, server CertificateVerify" } else { b"TLS 1.3, client CertificateVerify" }); signed.push(0);
    signed.extend_from_slice(th);
    signed
}

/// Hora actual (epoch) según el núcleo (RTC real).
pub fn now_epoch() -> i64 {
    let mut t: i64 = 0;
    let r = syscall::sys_time(&mut t as *mut i64);
    if t != 0 { t } else { r }
}

// ───────────────────────── Servidor ─────────────────────────

/// Configuración del servidor: cadena DER (hoja primero), clave P-256 y
/// clave de cifrado de tickets (aleatoria por proceso).
pub struct TlsServerConfig {
    pub chain_der: Vec<Vec<u8>>,
    pub key: SigningKey,
    /// Identidades adicionales (RSA, Ed25519…): se elige por `signature_algorithms` del cliente.
    pub extra: Vec<Identity>,
    ticket_key: [u8; 32],
    /// mTLS: raíces que deben firmar el certificado del cliente (vacío = no se exige).
    pub client_ca: Vec<Certificate>,
}

impl TlsServerConfig {
    pub fn new(chain_der: Vec<Vec<u8>>, key: SigningKey) -> TlsServerConfig {
        let mut tk = [0u8; 32]; random_bytes(&mut tk);
        TlsServerConfig { chain_der, key, extra: Vec::new(), ticket_key: tk, client_ca: Vec::new() }
    }
    /// Añade otra identidad (certificado + clave) seleccionable por el esquema que anuncie el cliente.
    pub fn add_identity(&mut self, id: Identity) { self.extra.push(id); }
    /// Elige la identidad cuyo esquema de firma aparezca en los `signature_algorithms` del cliente
    /// (RFC 8446 §4.4.2.2): la principal tiene prioridad; si el cliente no anuncia ninguno de los
    /// nuestros se usa la principal (y el cliente decidirá con una alerta).
    pub fn select_identity(&self, sigalgs: &[u16]) -> (&Vec<Vec<u8>>, &SigningKey, usize) {
        if sigalgs.contains(&self.key.scheme()) || sigalgs.is_empty() { return (&self.chain_der, &self.key, 0); }
        for (i, id) in self.extra.iter().enumerate() { if sigalgs.contains(&id.key.scheme()) { return (&id.chain_der, &id.key, i + 1); } }
        (&self.chain_der, &self.key, 0)
    }
    /// Exige certificado de cliente firmado por alguna de `roots`.
    pub fn require_client_cert(&mut self, roots: Vec<Certificate>) { self.client_ca = roots; }
    /// Ticket stateless: nonce(12) || AEAD(ticket_key, nonce, aad, PSK(32) || emitido(8)).
    fn seal_ticket(&self, psk: &[u8; 32], issued: i64) -> Vec<u8> {
        let mut nonce = [0u8; 12]; random_bytes(&mut nonce);
        let mut pt = Vec::with_capacity(40); pt.extend_from_slice(psk); pt.extend_from_slice(&issued.to_be_bytes());
        let mut out = nonce.to_vec(); out.extend_from_slice(&aead_seal(&self.ticket_key, &nonce, TICKET_AAD, &pt));
        out
    }
    fn open_ticket(&self, ticket: &[u8], now: i64) -> Option<[u8; 32]> {
        if ticket.len() != 12 + 40 + 16 { return None; }
        let mut nonce = [0u8; 12]; nonce.copy_from_slice(&ticket[..12]);
        let pt = aead_open(&self.ticket_key, &nonce, TICKET_AAD, &ticket[12..])?;
        if pt.len() != 40 { return None; }
        let mut issued = [0u8; 8]; issued.copy_from_slice(&pt[32..40]);
        let issued = i64::from_be_bytes(issued);
        if now < issued || now - issued > TICKET_LIFETIME as i64 { return None; }
        let mut psk = [0u8; 32]; psk.copy_from_slice(&pt[..32]);
        Some(psk)
    }
}

struct ClientHelloInfo {
    session_id: Vec<u8>,
    /// key_share X25519 (32 bytes) y/o secp256r1 (65 bytes SEC1) ofrecidos.
    share_x25519: Option<[u8; 32]>,
    share_p256: Option<Vec<u8>>,
    /// Grupos que el cliente soporta (para HelloRetryRequest).
    groups: Vec<u16>,
    sni: String,
    /// (identidad del ticket, binder, desplazamiento donde empiezan los binders en el mensaje)
    psk: Option<(Vec<u8>, Vec<u8>, usize)>,
    psk_dhe: bool,
    sig_ecdsa_p256: bool,
    /// signature_algorithms completos del cliente (para elegir identidad de servidor).
    sigalgs: Vec<u16>,
}

fn parse_client_hello(msg: &[u8]) -> Result<ClientHelloInfo, String> {
    if msg.len() < 4 || msg[0] != HS_CLIENT_HELLO { return Err(String::from("se esperaba ClientHello")); }
    let b = &msg[4..];
    if b.len() < 35 { return Err(String::from("ClientHello truncado")); }
    let sid_len = b[34] as usize; let mut p = 35;
    if b.len() < p + sid_len + 2 { return Err(String::from("ClientHello truncado (session_id)")); }
    let session_id = b[p..p + sid_len].to_vec(); p += sid_len;
    let cs_len = rd_u16(&b[p..]) as usize; p += 2;
    if b.len() < p + cs_len { return Err(String::from("ClientHello truncado (suites)")); }
    let mut has_suite = false;
    let mut i = p; while i + 2 <= p + cs_len { if rd_u16(&b[i..]) == SUITE_CHACHA20_POLY1305_SHA256 { has_suite = true; } i += 2; }
    p += cs_len;
    if !has_suite { return Err(String::from("[TLS-SRV] el cliente no ofrece TLS_CHACHA20_POLY1305_SHA256")); }
    if b.len() < p + 1 { return Err(String::from("ClientHello truncado (compresion)")); }
    let comp_len = b[p] as usize; p += 1 + comp_len;
    if b.len() < p + 2 { return Err(String::from("ClientHello sin extensiones")); }
    let ext_len = rd_u16(&b[p..]) as usize; p += 2;
    let exts_start = p;
    let exts = &b[p..(p + ext_len).min(b.len())];
    let mut q = 0; let mut v13 = false; let mut share_x25519 = None; let mut share_p256 = None; let mut groups = Vec::new(); let mut sni = String::new(); let mut psk = None; let mut psk_dhe = false; let mut sig_p256 = false; let mut sigalgs: Vec<u16> = Vec::new();
    while q + 4 <= exts.len() {
        let t = rd_u16(&exts[q..]); let l = rd_u16(&exts[q + 2..]) as usize; let ext_off = q; q += 4;
        if q + l > exts.len() { break; }
        let d = &exts[q..q + l]; q += l;
        match t {
            EXT_VERSIONS => { if l >= 1 { let n = d[0] as usize; let mut j = 1; while j + 2 <= 1 + n && j + 2 <= l { if rd_u16(&d[j..]) == 0x0304 { v13 = true; } j += 2; } } }
            EXT_GROUPS => { if l >= 2 { let n = rd_u16(d) as usize; let mut j = 2; while j + 2 <= 2 + n && j + 2 <= l { groups.push(rd_u16(&d[j..])); j += 2; } } }
            EXT_SIGALGS => { if l >= 2 { let n = rd_u16(d) as usize; let mut j = 2; while j + 2 <= 2 + n && j + 2 <= l { let a = rd_u16(&d[j..]); if a == SIG_ECDSA_SECP256R1_SHA256 { sig_p256 = true; } sigalgs.push(a); j += 2; } } }
            EXT_SNI => { if l >= 5 { let nl = rd_u16(&d[3..]) as usize; if 5 + nl <= l { sni = String::from_utf8_lossy(&d[5..5 + nl]).into_owned(); } } }
            EXT_KEY_SHARE => {
                if l >= 2 { let n = rd_u16(d) as usize; let mut j = 2;
                    while j + 4 <= 2 + n && j + 4 <= l { let g = rd_u16(&d[j..]); let kl = rd_u16(&d[j + 2..]) as usize; j += 4;
                        if g == GROUP_X25519 && kl == 32 && j + 32 <= l { let mut k = [0u8; 32]; k.copy_from_slice(&d[j..j + 32]); share_x25519 = Some(k); }
                        if g == GROUP_SECP256R1 && kl == 65 && j + 65 <= l { share_p256 = Some(d[j..j + 65].to_vec()); }
                        j += kl; } }
            }
            EXT_PSK_MODES => { if l >= 1 { let n = d[0] as usize; for j in 1..(1 + n).min(l) { if d[j] == 1 { psk_dhe = true; } } } }
            EXT_PSK => {
                if l >= 2 { let il = rd_u16(d) as usize;
                    if 2 + il + 2 <= l && il >= 6 {
                        let tl = rd_u16(&d[2..]) as usize;
                        if 4 + tl + 4 <= 2 + il {
                            let ident = d[4..4 + tl].to_vec();
                            let bl_off = 2 + il; let bl = rd_u16(&d[bl_off..]) as usize;
                            if bl_off + 2 + bl <= l && bl >= 1 {
                                let b0 = d[bl_off + 2] as usize;
                                if bl_off + 3 + b0 <= l {
                                    let binder = d[bl_off + 3..bl_off + 3 + b0].to_vec();
                                    // desplazamiento absoluto en `msg` del campo binders (u16 len)
                                    let abs = 4 + exts_start + ext_off + 4 + bl_off;
                                    psk = Some((ident, binder, abs));
                                } } } } }
            }
            _ => {}
        }
    }
    if !v13 { return Err(String::from("[TLS-SRV] el cliente no ofrece TLS 1.3 (supported_versions)")); }
    if !groups.iter().any(|&g| g == GROUP_X25519 || g == GROUP_SECP256R1) { return Err(String::from("[TLS-SRV] el cliente no ofrece ni X25519 ni secp256r1")); }
    Ok(ClientHelloInfo { session_id, share_x25519, share_p256, groups, sni, psk, psk_dhe, sig_ecdsa_p256: sig_p256, sigalgs })
}

pub struct TlsServer {
    rec: Record,
    pub cipher: &'static str,
    pub sni: String,
    pub resumed: bool,
    pub ticket_issued: bool,
    /// Grupo (EC)DHE negociado: "x25519" o "secp256r1".
    pub group: &'static str,
    /// `true` si hubo HelloRetryRequest.
    pub hrr: bool,
    /// Esquema con el que firmamos nuestro CertificateVerify ("" si reanudación PSK).
    pub server_sig: &'static str,
    /// CN del certificado de cliente verificado (mTLS).
    pub client_cn: Option<String>,
}

/// Clave efímera del servidor para el grupo elegido.
enum ServerShare { X25519([u8; 32]), P256(P256PrivateKey) }
impl ServerShare {
    fn public(&self) -> Vec<u8> { match self { ServerShare::X25519(k) => x25519_base(k).to_vec(), ServerShare::P256(k) => k.public_sec1() } }
    fn group(&self) -> u16 { match self { ServerShare::X25519(_) => GROUP_X25519, ServerShare::P256(_) => GROUP_SECP256R1 } }
}

fn build_server_hello(random: &[u8; 32], session_id: &[u8], group: u16, share: Option<&[u8]>, psk_selected: bool) -> Vec<u8> {
    let mut body = Vec::new();
    put_u16(&mut body, 0x0303); body.extend_from_slice(random);
    body.push(session_id.len() as u8); body.extend_from_slice(session_id);
    put_u16(&mut body, SUITE_CHACHA20_POLY1305_SHA256); body.push(0);
    let mut ext = Vec::new();
    { put_u16(&mut ext, EXT_VERSIONS); put_u16(&mut ext, 2); put_u16(&mut ext, 0x0304); }
    match share {
        Some(pk) => { put_u16(&mut ext, EXT_KEY_SHARE); put_u16(&mut ext, (4 + pk.len()) as u16); put_u16(&mut ext, group); put_u16(&mut ext, pk.len() as u16); ext.extend_from_slice(pk); }
        None => { put_u16(&mut ext, EXT_KEY_SHARE); put_u16(&mut ext, 2); put_u16(&mut ext, group); } // HelloRetryRequest
    }
    if psk_selected { put_u16(&mut ext, EXT_PSK); put_u16(&mut ext, 2); put_u16(&mut ext, 0); }
    put_u16(&mut body, ext.len() as u16); body.extend_from_slice(&ext);
    hs_msg(HS_SERVER_HELLO, &body)
}

impl TlsServer {
    /// Acepta una conexión TLS 1.3 sobre `fd` (socket ya aceptado por TCP).
    pub fn accept(fd: i32, cfg: &TlsServerConfig, now: i64, log: &mut dyn FnMut(&str)) -> Result<TlsServer, String> {
        let mut hr = HsReader::new();
        let mut ch = hr.next_plain(fd)?;
        let mut info = parse_client_hello(&ch)?;
        let mut transcript: Vec<u8> = Vec::new();
        let mut hrr = false;

        // ── Selección de grupo; HelloRetryRequest si no hay key_share utilizable ──
        if info.share_x25519.is_none() && info.share_p256.is_none() {
            let want = if info.groups.contains(&GROUP_X25519) { GROUP_X25519 } else { GROUP_SECP256R1 };
            let mut random = [0u8; 32]; random_bytes(&mut random);
            let hrr_msg = build_server_hello(&HRR_RANDOM, &info.session_id, want, None, false);
            write_plain_record(fd, REC_HANDSHAKE, &hrr_msg)?;
            write_all(fd, &[REC_CCS, 3, 3, 0, 1, 1])?;
            // Transcript: message_hash(CH1) || HRR || CH2 (RFC 8446 §4.4.1)
            let mh = hs_msg(HS_MESSAGE_HASH, &sha256(&ch));
            transcript.extend_from_slice(&mh); transcript.extend_from_slice(&hrr_msg);
            log(&alloc::format!("[TLS-SRV] HelloRetryRequest enviado: el cliente ofrecio key_share de otros grupos; se pide {}", if want == GROUP_X25519 { "x25519" } else { "secp256r1" }));
            ch = hr.next_plain(fd)?;
            info = parse_client_hello(&ch)?;
            let ok = (want == GROUP_X25519 && info.share_x25519.is_some()) || (want == GROUP_SECP256R1 && info.share_p256.is_some());
            if !ok { let _ = write_plain_record(fd, REC_ALERT, &[2, ALERT_ILLEGAL_PARAMETER]); return Err(String::from("[TLS-SRV] el segundo ClientHello no trae el key_share pedido")); }
            hrr = true;
        }
        let share = if info.share_x25519.is_some() { let mut k = [0u8; 32]; random_bytes(&mut k); ServerShare::X25519(k) }
                    else { ServerShare::P256(P256PrivateKey::generate().ok_or_else(|| String::from("[TLS-SRV] no se pudo generar clave efimera P-256"))?) };
        let group_name = if share.group() == GROUP_X25519 { "x25519" } else { "secp256r1" };

        // ¿Reanudación? Validar ticket y binder.
        let mut psk: Option<[u8; 32]> = None;
        if let (Some((ident, binder, boff)), true) = (&info.psk, info.psk_dhe) {
            match cfg.open_ticket(ident, now) {
                Some(k) => {
                    let (_, bk) = binder_key_from_psk(&k, false);
                    let mut pre = transcript.clone(); pre.extend_from_slice(&ch[..*boff]);
                    let th = sha256(&pre);
                    let expected = finished_mac(&bk, &th);
                    if ct_eq(&expected, binder) { psk = Some(k); log("[TLS-SRV] reanudacion PSK aceptada: ticket descifrado y binder verificado"); }
                    else { let _ = write_plain_record(fd, REC_ALERT, &[2, ALERT_ILLEGAL_PARAMETER]); return Err(String::from("[TLS-SRV] binder PSK invalido (illegal_parameter)")); }
                }
                None => log("[TLS-SRV] ticket PSK no valido o caducado: se hace handshake completo"),
            }
        }
        let resumed = psk.is_some();
        if !resumed {
            // RFC 8446 §4.4.2.2: hace falta alguna identidad cuyo esquema anuncie el cliente.
            let alguna = info.sigalgs.contains(&cfg.key.scheme()) || cfg.extra.iter().any(|id| info.sigalgs.contains(&id.key.scheme()));
            if !alguna {
                let _ = write_plain_record(fd, REC_ALERT, &[2, ALERT_HANDSHAKE_FAILURE]);
                return Err(alloc::format!("[TLS-SRV] el cliente no acepta ninguno de nuestros esquemas de firma ({} identidad(es): {}{})", cfg.extra.len() + 1, cfg.key.scheme_name(), if cfg.extra.is_empty() { String::new() } else { alloc::format!(" + {} mas", cfg.extra.len()) }));
            }
        }
        transcript.extend_from_slice(&ch);
        log(&alloc::format!("[TLS-SRV] ClientHello ({} bytes) SNI='{}' grupo {}{}{}", ch.len(), info.sni, group_name, if hrr { " (tras HRR)" } else { "" }, if info.psk.is_some() { " + pre_shared_key" } else { "" }));

        // ServerHello
        let mut random = [0u8; 32]; random_bytes(&mut random);
        let pubkey = share.public();
        let sh = build_server_hello(&random, &info.session_id, share.group(), Some(&pubkey), resumed);
        write_plain_record(fd, REC_HANDSHAKE, &sh)?;
        transcript.extend_from_slice(&sh);

        // Secretos de handshake ((EC)DHE real)
        let shared: [u8; 32] = match &share {
            ServerShare::X25519(k) => { let p = info.share_x25519.ok_or_else(|| String::from("sin share"))?; x25519(k, &p) }
            ServerShare::P256(k) => { let p = info.share_p256.as_ref().ok_or_else(|| String::from("sin share"))?; k.ecdh(p).ok_or_else(|| { let _ = write_plain_record(fd, REC_ALERT, &[2, ALERT_ILLEGAL_PARAMETER]); String::from("[TLS-SRV] key_share secp256r1 del cliente no esta en la curva") })? }
        };
        if shared.iter().all(|&b| b == 0) { return Err(String::from("[TLS-SRV] secreto X25519 nulo")); }
        let early = match psk { Some(k) => hkdf_extract(&[0u8; 32], &k), None => hkdf_extract(&[0u8; 32], &[0u8; 32]) };
        let derived = derive_secret(&early, "derived", &sha256(&[]));
        let hs_secret = hkdf_extract(&derived, &shared);
        let th_sh = sha256(&transcript);
        let c_hs = derive_secret(&hs_secret, "c hs traffic", &th_sh);
        let s_hs = derive_secret(&hs_secret, "s hs traffic", &th_sh);
        let mut rec = Record::new(fd, &s_hs, &c_hs);
        write_all(fd, &[REC_CCS, 3, 3, 0, 1, 1])?;

        // EncryptedExtensions
        let ee = hs_msg(HS_ENCRYPTED_EXT, &[0, 0]);
        rec.send_hs(&ee)?; transcript.extend_from_slice(&ee);

        let mtls = !resumed && !cfg.client_ca.is_empty();
        if mtls {
            // CertificateRequest (RFC 8446 §4.3.2): contexto vacío + signature_algorithms
            let algs = [SIG_ECDSA_SECP256R1_SHA256, SIG_ECDSA_SECP384R1_SHA384, SIG_ED25519, SIG_RSA_PSS_RSAE_SHA256, SIG_RSA_PKCS1_SHA256];
            let mut crb = Vec::new(); crb.push(0);
            let mut ext = Vec::new();
            put_u16(&mut ext, EXT_SIGALGS); put_u16(&mut ext, (algs.len() * 2 + 2) as u16); put_u16(&mut ext, (algs.len() * 2) as u16);
            for a in algs { put_u16(&mut ext, a); }
            put_u16(&mut crb, ext.len() as u16); crb.extend_from_slice(&ext);
            let cr = hs_msg(HS_CERT_REQUEST, &crb);
            rec.send_hs(&cr)?; transcript.extend_from_slice(&cr);
            log("[TLS-SRV] CertificateRequest enviado (mTLS)");
        }
        let mut server_sig: &'static str = "";
        if !resumed {
            // Certificate
            let (chain, key, idx) = cfg.select_identity(&info.sigalgs);
            if !cfg.extra.is_empty() {
                let cn = x509::parse_certificate(&chain[0]).map(|c| c.subject_cn).unwrap_or_default();
                log(&alloc::format!("[TLS-SRV] identidad elegida por signature_algorithms del cliente: '{}' ({}, identidad #{} de {})", cn, key.scheme_name(), idx + 1, cfg.extra.len() + 1));
            }
            let cert = build_certificate_msg(&[], chain);
            rec.send_hs(&cert)?; transcript.extend_from_slice(&cert);
            // CertificateVerify con la clave de la identidad (P-256, RSA-PSS o Ed25519)
            let th = sha256(&transcript);
            let payload = cert_verify_payload(&th, true);
            let (scheme, sig) = key.sign_cert_verify(&payload);
            let mut cvb = Vec::new(); put_u16(&mut cvb, scheme); put_u16(&mut cvb, sig.len() as u16); cvb.extend_from_slice(&sig);
            let cv = hs_msg(HS_CERT_VERIFY, &cvb);
            rec.send_hs(&cv)?; transcript.extend_from_slice(&cv);
            server_sig = key.scheme_name();
            log(&alloc::format!("[TLS-SRV] Certificate ({} cert) + CertificateVerify {} ({} bytes de firma) enviados", chain.len(), key.scheme_name(), sig.len()));
        }
        // Finished del servidor
        let th = sha256(&transcript);
        let fin = hs_msg(HS_FINISHED, &finished_mac(&s_hs, &th));
        rec.send_hs(&fin)?; transcript.extend_from_slice(&fin);

        // Secretos de aplicación
        let th_fin = sha256(&transcript);
        let derived2 = derive_secret(&hs_secret, "derived", &sha256(&[]));
        let master = hkdf_extract(&derived2, &[0u8; 32]);
        let c_ap = derive_secret(&master, "c ap traffic", &th_fin);
        let s_ap = derive_secret(&master, "s ap traffic", &th_fin);
        rec.set_write(&s_ap);

        // (mTLS) Certificate + CertificateVerify del cliente
        let mut client_cn: Option<String> = None;
        if mtls {
            let cm = hr.next_encrypted(&mut rec)?;
            if cm[0] != HS_CERTIFICATE { let _ = rec.send(REC_ALERT, &[2, ALERT_CERT_REQUIRED]); return Err(alloc::format!("[TLS-SRV] se esperaba Certificate del cliente, llego tipo {}", cm[0])); }
            transcript.extend_from_slice(&cm);
            let (_ctx, chain) = parse_certificate_msg(&cm[4..])?;
            if chain.is_empty() {
                let _ = rec.send(REC_ALERT, &[2, ALERT_CERT_REQUIRED]);
                return Err(String::from("[TLS-SRV-MTLS-RECHAZADO] el cliente no presento certificado (alerta certificate_required)"));
            }
            let th_cert = sha256(&transcript);
            let cv = hr.next_encrypted(&mut rec)?;
            if cv[0] != HS_CERT_VERIFY { let _ = rec.send(REC_ALERT, &[2, ALERT_BAD_CERT]); return Err(String::from("[TLS-SRV] falta CertificateVerify del cliente")); }
            let alg = match verify_cert_verify(&chain[0], &cv[4..], &th_cert, false) { Ok(a) => a, Err(e) => { let _ = rec.send(REC_ALERT, &[2, ALERT_BAD_CERT]); return Err(alloc::format!("[TLS-SRV-MTLS-RECHAZADO] {}", e)); } };
            transcript.extend_from_slice(&cv);
            let anchor = match x509::validate_chain_opt(&chain, &cfg.client_ca, None, now) { Ok(a) => a, Err(e) => { let _ = rec.send(REC_ALERT, &[2, ALERT_BAD_CERT]); return Err(alloc::format!("[TLS-SRV-MTLS-RECHAZADO] cadena del cliente: {}", e)); } };
            log(&alloc::format!("[TLS-SRV] cliente autenticado (mTLS): CN='{}' firma {} verificada, cadena -> raiz '{}'", chain[0].subject_cn, alg, anchor));
            client_cn = Some(chain[0].subject_cn.clone());
        }
        // Finished del cliente (con claves de handshake del cliente)
        let th_client_fin = sha256(&transcript);
        let cfin = hr.next_encrypted(&mut rec)?;
        if cfin[0] != HS_FINISHED { return Err(alloc::format!("[TLS-SRV] se esperaba Finished del cliente, llego tipo {}", cfin[0])); }
        let expected = finished_mac(&c_hs, &th_client_fin);
        if !ct_eq(&expected, &cfin[4..]) { let _ = rec.send(REC_ALERT, &[2, 51]); return Err(String::from("[TLS-SRV] Finished del cliente invalido (decrypt_error)")); }
        transcript.extend_from_slice(&cfin);
        rec.set_read(&c_ap);
        log(if resumed { "[TLS-SRV] Finished del cliente verificado; sesion reanudada activa" } else { "[TLS-SRV] Finished del cliente verificado; canal de aplicacion activo" });

        // NewSessionTicket (stateless)
        let res_master = derive_secret(&master, "res master", &sha256(&transcript));
        let nonce = [0u8];
        let psk_v = hkdf_expand_label(&res_master, "resumption", &nonce, 32);
        let mut new_psk = [0u8; 32]; new_psk.copy_from_slice(&psk_v);
        let ticket = cfg.seal_ticket(&new_psk, now);
        let mut age_add = [0u8; 4]; random_bytes(&mut age_add);
        let mut nb = Vec::new(); put_u32(&mut nb, TICKET_LIFETIME); nb.extend_from_slice(&age_add); nb.push(1); nb.push(0);
        put_u16(&mut nb, ticket.len() as u16); nb.extend_from_slice(&ticket); put_u16(&mut nb, 0);
        let nst = hs_msg(HS_NEW_SESSION_TICKET, &nb);
        let ticket_issued = rec.send_hs(&nst).is_ok();
        if ticket_issued { log(&alloc::format!("[TLS-SRV] NewSessionTicket emitido ({} bytes, {} s)", ticket.len(), TICKET_LIFETIME)); }

        Ok(TlsServer { rec, cipher: "TLS_CHACHA20_POLY1305_SHA256", sni: info.sni, resumed, ticket_issued, group: group_name, hrr, client_cn, server_sig })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), String> { self.rec.write(data) }
    /// Lee datos de aplicación; `Ok(vacío)` al cierre. KeyUpdate se procesa internamente.
    pub fn read(&mut self) -> Result<Vec<u8>, String> {
        loop {
            match self.rec.next()? {
                RecordEvent::Data(d) => return Ok(d),
                RecordEvent::Closed => return Ok(Vec::new()),
                RecordEvent::Handshake(_) => {}
            }
        }
    }
    pub fn key_update(&mut self, request_peer: bool) -> Result<(), String> { self.rec.send_key_update(request_peer) }
    pub fn key_updates(&self) -> (u32, u32) { (self.rec.key_updates_tx, self.rec.key_updates_rx) }
    pub fn is_closed(&self) -> bool { self.rec.closed }
    pub fn close(&mut self) { self.rec.close(); }
}
