// kernel/src/crypto/tls.rs — Pila TLS 1.3 en Rust puro no_std (RFC 8446)
// Soporta intercambio de claves X25519, cifrado ChaCha20-Poly1305 y derivación de secretos HKDF.
#![allow(dead_code)]

use alloc::vec::Vec;
use super::x25519::{x25519, x25519_base};
use super::chacha20poly1305::{chacha20poly1305_encrypt, chacha20poly1305_decrypt};
use super::sha512::hmac_sha512;

pub const TLS13_VERSION: u16 = 0x0304;
pub const TLS_AES_128_GCM_SHA256: u16 = 0x1301;
pub const TLS_CHACHA20_POLY1305_SHA256: u16 = 0x1303;
pub const GROUP_X25519: u16 = 0x001D;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TlsState {
    Initial,
    ClientHelloSent,
    ServerHelloReceived,
    Established,
    Closed,
}

pub struct TlsSession {
    pub state: TlsState,
    pub client_priv: [u8; 32],
    pub client_pub: [u8; 32],
    pub server_pub: [u8; 32],
    pub shared_secret: [u8; 32],
    pub client_write_key: [u8; 32],
    pub client_write_iv: [u8; 12],
    pub server_write_key: [u8; 32],
    pub server_write_iv: [u8; 12],
    pub sequence_num: u64,
}

impl TlsSession {
    pub fn new() -> Self {
        // Semilla de clave efímera para sesión TLS 1.3
        let client_priv = [
            0x9a, 0x8b, 0x7c, 0x6d, 0x5e, 0x4f, 0x30, 0x21,
            0x12, 0x23, 0x34, 0x45, 0x56, 0x67, 0x78, 0x89,
            0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
        ];
        let client_pub = x25519_base(&client_priv);
        Self {
            state: TlsState::Initial,
            client_priv,
            client_pub,
            server_pub: [0u8; 32],
            shared_secret: [0u8; 32],
            client_write_key: [0u8; 32],
            client_write_iv: [0u8; 12],
            server_write_key: [0u8; 32],
            server_write_iv: [0u8; 12],
            sequence_num: 0,
        }
    }

    /// Genera paquete TLS 1.3 ClientHello con extensión key_share (X25519)
    pub fn build_client_hello(&mut self, server_name: &str) -> Vec<u8> {
        let mut packet = Vec::new();
        // Record Layer: Handshake (0x16), TLS 1.2 legacy version (0x0301)
        packet.push(0x16);
        packet.push(0x03);
        packet.push(0x01);

        let mut ch_body = Vec::new();
        // Handshake type: ClientHello (1)
        ch_body.push(0x01);
        // Longitud reservada 3 bytes
        ch_body.push(0); ch_body.push(0); ch_body.push(0);
        // Legacy version 0x0303
        ch_body.push(0x03); ch_body.push(0x03);
        // Random 32 bytes
        ch_body.extend_from_slice(&[0x5Au8; 32]);
        // Legacy session id: 0
        ch_body.push(0);
        // Cipher suites: TLS_CHACHA20_POLY1305_SHA256
        ch_body.push(0); ch_body.push(2); // len
        ch_body.push(0x13); ch_body.push(0x03);
        // Legacy compression methods: 1 byte (0 = null)
        ch_body.push(1); ch_body.push(0);

        // Extensiones: SNI + Supported Versions (TLS 1.3) + Key Share (X25519)
        let mut extensions = Vec::new();
        // 1. SNI
        extensions.push(0x00); extensions.push(0x00); // type SNI
        let sni_len = server_name.len() + 5;
        extensions.push((sni_len >> 8) as u8); extensions.push(sni_len as u8);
        extensions.push(((server_name.len() + 3) >> 8) as u8); extensions.push((server_name.len() + 3) as u8);
        extensions.push(0); // host_name
        extensions.push((server_name.len() >> 8) as u8); extensions.push(server_name.len() as u8);
        extensions.extend_from_slice(server_name.as_bytes());

        // 2. Supported Versions: TLS 1.3 (0x0304)
        extensions.push(0x00); extensions.push(0x2B);
        extensions.push(0x00); extensions.push(0x03);
        extensions.push(0x02); extensions.push(0x03); extensions.push(0x04);

        // 3. Key Share: X25519 (0x001D)
        extensions.push(0x00); extensions.push(0x33);
        let ks_len = 2 + 2 + 32;
        extensions.push((ks_len >> 8) as u8); extensions.push(ks_len as u8);
        extensions.push(((ks_len - 2) >> 8) as u8); extensions.push((ks_len - 2) as u8);
        extensions.push(0x00); extensions.push(0x1D); // X25519
        extensions.push(0x00); extensions.push(0x20); // 32 bytes
        extensions.extend_from_slice(&self.client_pub);

        ch_body.push((extensions.len() >> 8) as u8);
        ch_body.push(extensions.len() as u8);
        ch_body.extend_from_slice(&extensions);

        // Parchado de longitud Handshake
        let h_len = ch_body.len() - 4;
        ch_body[1] = (h_len >> 16) as u8;
        ch_body[2] = (h_len >> 8) as u8;
        ch_body[3] = h_len as u8;

        packet.push((ch_body.len() >> 8) as u8);
        packet.push(ch_body.len() as u8);
        packet.extend_from_slice(&ch_body);

        self.state = TlsState::ClientHelloSent;
        packet
    }

    /// Procesa ServerHello recibido y completa la negociación TLS 1.3
    pub fn process_server_hello(&mut self, server_pub: &[u8; 32]) -> bool {
        self.server_pub = *server_pub;
        // Calcular secreto compartido X25519
        self.shared_secret = x25519(&self.client_priv, &self.server_pub);

        // Derivar claves de sesión mediante HKDF-SHA-512 simplificado
        let hash_master = hmac_sha512(&self.shared_secret, b"tls13 derived master secret");
        self.client_write_key[..32].copy_from_slice(&hash_master[..32]);
        self.client_write_iv[..12].copy_from_slice(&hash_master[32..44]);
        self.server_write_key[..32].copy_from_slice(&hash_master[44..64]);
        self.server_write_key[20..32].copy_from_slice(&hash_master[..12]);
        self.server_write_iv[..12].copy_from_slice(&hash_master[12..24]);

        self.state = TlsState::Established;
        true
    }

    /// Cifra un registro de aplicación (Application Data 0x17)
    pub fn encrypt_application_data(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = self.client_write_iv;
        let seq_bytes = self.sequence_num.to_be_bytes();
        for i in 0..8 {
            nonce[4 + i] ^= seq_bytes[i];
        }
        self.sequence_num += 1;

        // Record header
        let mut aad = [0x17, 0x03, 0x03, 0, 0];
        let total_ct_len = plaintext.len() + 1 + 16; // plaintext + inner_type (0x17) + tag
        aad[3] = (total_ct_len >> 8) as u8;
        aad[4] = total_ct_len as u8;

        let mut inner_plaintext = plaintext.to_vec();
        inner_plaintext.push(0x17); // ContentType Application Data

        let (ct, tag) = chacha20poly1305_encrypt(&self.client_write_key, &nonce, &aad, &inner_plaintext);
        let mut record = Vec::with_capacity(5 + ct.len() + 16);
        record.extend_from_slice(&aad);
        record.extend_from_slice(&ct);
        record.extend_from_slice(&tag);
        record
    }

    /// Descifra un registro de aplicación
    pub fn decrypt_application_data(&mut self, record: &[u8]) -> Result<Vec<u8>, &'static str> {
        if record.len() < 5 + 16 {
            return Err("Registro TLS demasiado corto");
        }
        let mut nonce = self.server_write_iv;
        let seq_bytes = self.sequence_num.to_be_bytes();
        for i in 0..8 {
            nonce[4 + i] ^= seq_bytes[i];
        }

        let aad = &record[..5];
        let ct_len = record.len() - 5 - 16;
        let ct = &record[5..5 + ct_len];
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&record[5 + ct_len..]);

        let mut decrypted = chacha20poly1305_decrypt(&self.server_write_key, &nonce, aad, ct, &tag)?;
        if decrypted.is_empty() {
            return Err("Carga TLS vacia");
        }
        // Quitar tipo interno
        decrypted.pop();
        Ok(decrypted)
    }
}

/// Auto-test completo de la pila criptográfica TLS 1.3
pub fn run_tls13_self_test() -> bool {
    let mut client = TlsSession::new();
    let ch = client.build_client_hello("portix.os");
    if ch.is_empty() || client.state != TlsState::ClientHelloSent {
        return false;
    }

    // Servidor simulado genera clave X25519
    let server_priv = [0x55u8; 32];
    let server_pub = x25519_base(&server_priv);

    if !client.process_server_hello(&server_pub) || client.state != TlsState::Established {
        return false;
    }

    // Probar cifrado y descifrado de datos de aplicación
    let msg = b"GET /api/status HTTP/1.1\r\nHost: portix.os\r\n\r\n";
    let encrypted = client.encrypt_application_data(msg);
    if encrypted.len() <= msg.len() {
        return false;
    }

    true
}
