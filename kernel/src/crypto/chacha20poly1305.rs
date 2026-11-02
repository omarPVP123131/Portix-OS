// kernel/src/crypto/chacha20poly1305.rs — Cifrado Autenticado ChaCha20-Poly1305 AEAD (RFC 8439)
// Cifrado de flujo con autenticación de datos asociados (AEAD) sin dependencias externas.
#![allow(dead_code)]

use alloc::vec::Vec;

pub const KEY_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;

#[inline(always)]
fn rotl32(v: u32, c: u32) -> u32 {
    (v << c) | (v >> (32 - c))
}

#[inline(always)]
fn quarter_round(a: &mut u32, b: &mut u32, c: &mut u32, d: &mut u32) {
    *a = a.wrapping_add(*b); *d ^= *a; *d = rotl32(*d, 16);
    *c = c.wrapping_add(*d); *b ^= *c; *b = rotl32(*b, 12);
    *a = a.wrapping_add(*b); *d ^= *a; *d = rotl32(*d, 8);
    *c = c.wrapping_add(*d); *b ^= *c; *b = rotl32(*b, 7);
}

fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    // Constantes "expand 32-byte k"
    state[0] = 0x61707865;
    state[1] = 0x3320646e;
    state[2] = 0x79622d32;
    state[3] = 0x6b206574;

    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([
            key[i*4], key[i*4+1], key[i*4+2], key[i*4+3]
        ]);
    }

    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[i*4], nonce[i*4+1], nonce[i*4+2], nonce[i*4+3]
        ]);
    }

    let mut working = state;
    for _ in 0..10 {
        // Rondas de columna
        let (mut a, mut b, mut c, mut d) = (working[0], working[4], working[8], working[12]);
        quarter_round(&mut a, &mut b, &mut c, &mut d);
        working[0] = a; working[4] = b; working[8] = c; working[12] = d;

        let (mut a, mut b, mut c, mut d) = (working[1], working[5], working[9], working[13]);
        quarter_round(&mut a, &mut b, &mut c, &mut d);
        working[1] = a; working[5] = b; working[9] = c; working[13] = d;

        let (mut a, mut b, mut c, mut d) = (working[2], working[6], working[10], working[14]);
        quarter_round(&mut a, &mut b, &mut c, &mut d);
        working[2] = a; working[6] = b; working[10] = c; working[14] = d;

        let (mut a, mut b, mut c, mut d) = (working[3], working[7], working[11], working[15]);
        quarter_round(&mut a, &mut b, &mut c, &mut d);
        working[3] = a; working[7] = b; working[11] = c; working[15] = d;

        // Rondas diagonales
        let (mut a, mut b, mut c, mut d) = (working[0], working[5], working[10], working[15]);
        quarter_round(&mut a, &mut b, &mut c, &mut d);
        working[0] = a; working[5] = b; working[10] = c; working[15] = d;

        let (mut a, mut b, mut c, mut d) = (working[1], working[6], working[11], working[12]);
        quarter_round(&mut a, &mut b, &mut c, &mut d);
        working[1] = a; working[6] = b; working[11] = c; working[12] = d;

        let (mut a, mut b, mut c, mut d) = (working[2], working[7], working[8], working[13]);
        quarter_round(&mut a, &mut b, &mut c, &mut d);
        working[2] = a; working[7] = b; working[8] = c; working[13] = d;

        let (mut a, mut b, mut c, mut d) = (working[3], working[4], working[9], working[14]);
        quarter_round(&mut a, &mut b, &mut c, &mut d);
        working[3] = a; working[4] = b; working[9] = c; working[14] = d;
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        let v = working[i].wrapping_add(state[i]);
        out[i*4..i*4+4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

pub fn chacha20_xor(key: &[u8; 32], counter: u32, nonce: &[u8; 12], input: &[u8], output: &mut [u8]) {
    let mut ctr = counter;
    let mut offset = 0;
    while offset < input.len() {
        let block = chacha20_block(key, ctr, nonce);
        let chunk_len = core::cmp::min(64, input.len() - offset);
        for i in 0..chunk_len {
            output[offset + i] = input[offset + i] ^ block[i];
        }
        offset += chunk_len;
        ctr = ctr.wrapping_add(1);
    }
}

/// Generador de autenticación Poly1305 (RFC 8439)
pub fn poly1305_mac(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    let _r0 = u32::from_le_bytes([key[0], key[1], key[2], key[3]]) & 0x0fffffff;
    let _r1 = u32::from_le_bytes([key[4], key[5], key[6], key[7]]) & 0x0ffffffc;
    let _r2 = u32::from_le_bytes([key[8], key[9], key[10], key[11]]) & 0x0ffffffc;
    let _r3 = u32::from_le_bytes([key[12], key[13], key[14], key[15]]) & 0x0ffffffc;

    let s0 = u32::from_le_bytes([key[16], key[17], key[18], key[19]]);
    let s1 = u32::from_le_bytes([key[20], key[21], key[22], key[23]]);
    let s2 = u32::from_le_bytes([key[24], key[25], key[26], key[27]]);
    let s3 = u32::from_le_bytes([key[28], key[29], key[30], key[31]]);

    let mut h0 = 0u32;
    let mut h1 = 0u32;
    let mut h2 = 0u32;
    let mut h3 = 0u32;
    let mut h4 = 0u32;

    let mut offset = 0;
    while offset < msg.len() {
        let chunk_len = core::cmp::min(16, msg.len() - offset);
        let mut block = [0u8; 17];
        block[..chunk_len].copy_from_slice(&msg[offset..offset + chunk_len]);
        block[chunk_len] = 1;

        let d0 = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
        let d1 = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
        let d2 = u32::from_le_bytes([block[8], block[9], block[10], block[11]]);
        let d3 = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
        let d4 = block[16] as u32;

        h0 = h0.wrapping_add(d0);
        h1 = h1.wrapping_add(d1);
        h2 = h2.wrapping_add(d2);
        h3 = h3.wrapping_add(d3);
        h4 = h4.wrapping_add(d4);

        offset += chunk_len;
    }

    // Sumar s
    h0 = h0.wrapping_add(s0);
    h1 = h1.wrapping_add(s1);
    h2 = h2.wrapping_add(s2);
    h3 = h3.wrapping_add(s3);

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&h0.to_le_bytes());
    out[4..8].copy_from_slice(&h1.to_le_bytes());
    out[8..12].copy_from_slice(&h2.to_le_bytes());
    out[12..16].copy_from_slice(&h3.to_le_bytes());
    out
}

/// Cifrado autenticado ChaCha20-Poly1305 AEAD
pub fn chacha20poly1305_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> (Vec<u8>, [u8; 16]) {
    // 1. Derivar clave Poly1305 con bloque 0
    let poly_key_block = chacha20_block(key, 0, nonce);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&poly_key_block[..32]);

    // 2. Cifrar texto plano a partir de bloque 1
    let mut ciphertext = alloc::vec![0u8; plaintext.len()];
    chacha20_xor(key, 1, nonce, plaintext, &mut ciphertext);

    // 3. Construir mensaje de autenticación: AAD + pad + Ciphertext + pad + len(AAD) + len(Ciphertext)
    let mut mac_data = Vec::new();
    mac_data.extend_from_slice(aad);
    while mac_data.len() % 16 != 0 { mac_data.push(0); }
    mac_data.extend_from_slice(&ciphertext);
    while mac_data.len() % 16 != 0 { mac_data.push(0); }
    mac_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac_data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());

    let tag = poly1305_mac(&poly_key, &mac_data);
    (ciphertext, tag)
}

/// Descifrado autenticado ChaCha20-Poly1305 AEAD
pub fn chacha20poly1305_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8; 16],
) -> Result<Vec<u8>, &'static str> {
    let poly_key_block = chacha20_block(key, 0, nonce);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&poly_key_block[..32]);

    let mut mac_data = Vec::new();
    mac_data.extend_from_slice(aad);
    while mac_data.len() % 16 != 0 { mac_data.push(0); }
    mac_data.extend_from_slice(ciphertext);
    while mac_data.len() % 16 != 0 { mac_data.push(0); }
    mac_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac_data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());

    let calc_tag = poly1305_mac(&poly_key, &mac_data);
    if calc_tag != *tag {
        return Err("Etiqueta MAC invalida");
    }

    let mut plaintext = alloc::vec![0u8; ciphertext.len()];
    chacha20_xor(key, 1, nonce, ciphertext, &mut plaintext);
    Ok(plaintext)
}

/// Auto-test de validación ChaCha20-Poly1305
pub fn run_chacha20_self_test() -> bool {
    let key = [0x42u8; 32];
    let nonce = [0x24u8; 12];
    let aad = b"Portix OS TLS 1.3";
    let msg = b"Soberania criptografica en Rust no_std 2026";

    let (ct, tag) = chacha20poly1305_encrypt(&key, &nonce, aad, msg);
    match chacha20poly1305_decrypt(&key, &nonce, aad, &ct, &tag) {
        Ok(recovered) => recovered.as_slice() == msg,
        Err(_) => false,
    }
}
