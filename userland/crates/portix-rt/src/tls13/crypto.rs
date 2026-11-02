//! Primitivas criptográficas para TLS 1.3 (100 % Rust `no_std`, sin crates).
//!
//! - SHA-384/512 (FIPS 180-4) — firmas `sha384WithRSAEncryption` de las CA.
//! - Poly1305 y AEAD ChaCha20-Poly1305 (RFC 8439).
//! - X25519 (RFC 7748) con aritmética de campo en 5 limbs de 51 bits.
//! - Enteros grandes y verificación RSA PKCS#1 v1.5 / RSASSA-PSS (RFC 8017).
//!
//! SHA-256, HMAC, HKDF y el flujo ChaCha20 viven en `tls.rs` y se reutilizan.

use alloc::vec::Vec;
use crate::tls::{sha256, hmac_sha256, chacha20_xor};

// ───────────────────────────── SHA-384 / SHA-512 ─────────────────────────────

const K512: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc, 0x3956c25bf348b538,
    0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118, 0xd807aa98a3030242, 0x12835b0145706fbe,
    0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2, 0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235,
    0xc19bf174cf692694, 0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5, 0x983e5152ee66dfab,
    0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4, 0xc6e00bf33da88fc2, 0xd5a79147930aa725,
    0x06ca6351e003826f, 0x142929670a0e6e70, 0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df, 0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30, 0xd192e819d6ef5218,
    0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8, 0x19a4c116b8d2d0c8, 0x1e376c085141ab53,
    0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8, 0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3, 0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b, 0xca273eceea26619c,
    0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178, 0x06f067aa72176fba, 0x0a637dc5a2c898a6,
    0x113f9804bef90dae, 0x1b710b35131c471b, 0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c, 0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

fn sha512_core(data: &[u8], h0: [u64; 8], out_len: usize) -> Vec<u8> {
    let mut h = h0;
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u128) * 8;
    msg.push(0x80);
    while msg.len() % 128 != 112 { msg.push(0); }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for block in msg.chunks(128) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            w[i] = u64::from_be_bytes([block[i*8], block[i*8+1], block[i*8+2], block[i*8+3], block[i*8+4], block[i*8+5], block[i*8+6], block[i*8+7]]);
        }
        for i in 16..80 {
            let s0 = w[i-15].rotate_right(1) ^ w[i-15].rotate_right(8) ^ (w[i-15] >> 7);
            let s1 = w[i-2].rotate_right(19) ^ w[i-2].rotate_right(61) ^ (w[i-2] >> 6);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K512[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(t1); d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f); h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }
    let mut out = Vec::with_capacity(64);
    for v in h.iter() { out.extend_from_slice(&v.to_be_bytes()); }
    out.truncate(out_len);
    out
}

pub fn sha384(data: &[u8]) -> [u8; 48] {
    let v = sha512_core(data, [
        0xcbbb9d5dc1059ed8, 0x629a292a367cd507, 0x9159015a3070dd17, 0x152fecd8f70e5939,
        0x67332667ffc00b31, 0x8eb44a8768581511, 0xdb0c2e0d64f98fa7, 0x47b5481dbefa4fa4], 48);
    let mut o = [0u8; 48]; o.copy_from_slice(&v); o
}

pub fn sha512(data: &[u8]) -> [u8; 64] {
    let v = sha512_core(data, [
        0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
        0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179], 64);
    let mut o = [0u8; 64]; o.copy_from_slice(&v); o
}

// ─────────────────────────────── Poly1305 ───────────────────────────────────

pub fn poly1305(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    // r con "clamping", 5 limbs de 26 bits (poly1305-donna-32).
    let t0 = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
    let t1 = u32::from_le_bytes([key[4], key[5], key[6], key[7]]);
    let t2 = u32::from_le_bytes([key[8], key[9], key[10], key[11]]);
    let t3 = u32::from_le_bytes([key[12], key[13], key[14], key[15]]);
    let r0 = t0 & 0x3ffffff;
    let r1 = ((t0 >> 26) | (t1 << 6)) & 0x3ffff03;
    let r2 = ((t1 >> 20) | (t2 << 12)) & 0x3ffc0ff;
    let r3 = ((t2 >> 14) | (t3 << 18)) & 0x3f03fff;
    let r4 = (t3 >> 8) & 0x00fffff;
    let (s1, s2, s3, s4) = (r1 * 5, r2 * 5, r3 * 5, r4 * 5);
    let (mut h0, mut h1, mut h2, mut h3, mut h4) = (0u32, 0u32, 0u32, 0u32, 0u32);

    let mut i = 0;
    while i < msg.len() {
        let mut block = [0u8; 17];
        let n = (msg.len() - i).min(16);
        block[..n].copy_from_slice(&msg[i..i + n]);
        block[n] = 1;
        let hibit = 0u32; // el bit 1 ya está en block[n]
        let b0 = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
        let b1 = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
        let b2 = u32::from_le_bytes([block[8], block[9], block[10], block[11]]);
        let b3 = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
        let b4 = block[16] as u32;
        h0 += b0 & 0x3ffffff;
        h1 += ((b0 >> 26) | (b1 << 6)) & 0x3ffffff;
        h2 += ((b1 >> 20) | (b2 << 12)) & 0x3ffffff;
        h3 += ((b2 >> 14) | (b3 << 18)) & 0x3ffffff;
        h4 += (b3 >> 8) | (b4 << 24) | hibit;

        let d0 = (h0 as u64) * (r0 as u64) + (h1 as u64) * (s4 as u64) + (h2 as u64) * (s3 as u64) + (h3 as u64) * (s2 as u64) + (h4 as u64) * (s1 as u64);
        let mut d1 = (h0 as u64) * (r1 as u64) + (h1 as u64) * (r0 as u64) + (h2 as u64) * (s4 as u64) + (h3 as u64) * (s3 as u64) + (h4 as u64) * (s2 as u64);
        let mut d2 = (h0 as u64) * (r2 as u64) + (h1 as u64) * (r1 as u64) + (h2 as u64) * (r0 as u64) + (h3 as u64) * (s4 as u64) + (h4 as u64) * (s3 as u64);
        let mut d3 = (h0 as u64) * (r3 as u64) + (h1 as u64) * (r2 as u64) + (h2 as u64) * (r1 as u64) + (h3 as u64) * (r0 as u64) + (h4 as u64) * (s4 as u64);
        let mut d4 = (h0 as u64) * (r4 as u64) + (h1 as u64) * (r3 as u64) + (h2 as u64) * (r2 as u64) + (h3 as u64) * (r1 as u64) + (h4 as u64) * (r0 as u64);

        let mut c = (d0 >> 26) as u32; h0 = (d0 as u32) & 0x3ffffff;
        d1 += c as u64; c = (d1 >> 26) as u32; h1 = (d1 as u32) & 0x3ffffff;
        d2 += c as u64; c = (d2 >> 26) as u32; h2 = (d2 as u32) & 0x3ffffff;
        d3 += c as u64; c = (d3 >> 26) as u32; h3 = (d3 as u32) & 0x3ffffff;
        d4 += c as u64; c = (d4 >> 26) as u32; h4 = (d4 as u32) & 0x3ffffff;
        h0 += c * 5; c = h0 >> 26; h0 &= 0x3ffffff;
        h1 += c;
        i += 16;
    }

    // Reducción final
    let mut c = h1 >> 26; h1 &= 0x3ffffff;
    h2 += c; c = h2 >> 26; h2 &= 0x3ffffff;
    h3 += c; c = h3 >> 26; h3 &= 0x3ffffff;
    h4 += c; c = h4 >> 26; h4 &= 0x3ffffff;
    h0 += c * 5; c = h0 >> 26; h0 &= 0x3ffffff;
    h1 += c;

    let mut g0 = h0.wrapping_add(5); c = g0 >> 26; g0 &= 0x3ffffff;
    let mut g1 = h1.wrapping_add(c); c = g1 >> 26; g1 &= 0x3ffffff;
    let mut g2 = h2.wrapping_add(c); c = g2 >> 26; g2 &= 0x3ffffff;
    let mut g3 = h3.wrapping_add(c); c = g3 >> 26; g3 &= 0x3ffffff;
    let g4 = h4.wrapping_add(c).wrapping_sub(1 << 26);

    let mask = (g4 >> 31).wrapping_sub(1); // todo 1 si g4 >= 0 (h >= p)
    g0 &= mask; g1 &= mask; g2 &= mask; g3 &= mask; let g4 = g4 & mask;
    let nmask = !mask;
    h0 = (h0 & nmask) | g0; h1 = (h1 & nmask) | g1; h2 = (h2 & nmask) | g2; h3 = (h3 & nmask) | g3; h4 = (h4 & nmask) | g4;

    let f0 = (h0 | (h1 << 26)) as u64 + u32::from_le_bytes([key[16], key[17], key[18], key[19]]) as u64;
    let f1 = ((h1 >> 6) | (h2 << 20)) as u64 + u32::from_le_bytes([key[20], key[21], key[22], key[23]]) as u64;
    let f2 = ((h2 >> 12) | (h3 << 14)) as u64 + u32::from_le_bytes([key[24], key[25], key[26], key[27]]) as u64;
    let f3 = ((h3 >> 18) | (h4 << 8)) as u64 + u32::from_le_bytes([key[28], key[29], key[30], key[31]]) as u64;
    let o0 = f0 as u32; let f1 = f1 + (f0 >> 32);
    let o1 = f1 as u32; let f2 = f2 + (f1 >> 32);
    let o2 = f2 as u32; let f3 = f3 + (f2 >> 32);
    let o3 = f3 as u32;
    let mut tag = [0u8; 16];
    tag[0..4].copy_from_slice(&o0.to_le_bytes()); tag[4..8].copy_from_slice(&o1.to_le_bytes());
    tag[8..12].copy_from_slice(&o2.to_le_bytes()); tag[12..16].copy_from_slice(&o3.to_le_bytes());
    tag
}

// ─────────────────────── AEAD ChaCha20-Poly1305 (RFC 8439 §2.8) ─────────────

fn poly_key(key: &[u8; 32], nonce: &[u8; 12]) -> [u8; 32] {
    let mut block = [0u8; 64];
    chacha20_xor(key, 0, nonce, &mut block);
    let mut k = [0u8; 32]; k.copy_from_slice(&block[..32]); k
}

fn aead_mac(pk: &[u8; 32], aad: &[u8], ct: &[u8]) -> [u8; 16] {
    let mut m = Vec::with_capacity(aad.len() + ct.len() + 32);
    m.extend_from_slice(aad); while m.len() % 16 != 0 { m.push(0); }
    m.extend_from_slice(ct); while m.len() % 16 != 0 { m.push(0); }
    m.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    m.extend_from_slice(&(ct.len() as u64).to_le_bytes());
    poly1305(pk, &m)
}

/// Cifra y autentica: devuelve `ct || tag`.
pub fn aead_seal(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let pk = poly_key(key, nonce);
    let mut ct = plaintext.to_vec();
    chacha20_xor(key, 1, nonce, &mut ct);
    let tag = aead_mac(&pk, aad, &ct);
    ct.extend_from_slice(&tag);
    ct
}

/// Verifica la etiqueta (tiempo constante) y descifra. `None` si la autenticación falla.
pub fn aead_open(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], ct_tag: &[u8]) -> Option<Vec<u8>> {
    if ct_tag.len() < 16 { return None; }
    let (ct, tag) = ct_tag.split_at(ct_tag.len() - 16);
    let pk = poly_key(key, nonce);
    let calc = aead_mac(&pk, aad, ct);
    if !ct_eq(&calc, tag) { return None; }
    let mut pt = ct.to_vec();
    chacha20_xor(key, 1, nonce, &mut pt);
    Some(pt)
}

pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut d = 0u8;
    for i in 0..a.len() { d |= a[i] ^ b[i]; }
    d == 0
}

// ──────────────────────────────── X25519 ────────────────────────────────────

pub(crate) type Fe = [u64; 5]; // 5 × 51 bits, módulo 2^255 - 19

pub(crate) fn fe_from_bytes(b: &[u8; 32]) -> Fe {
    let l = |i: usize| u64::from_le_bytes([b[i], b[i+1], b[i+2], b[i+3], b[i+4], b[i+5], b[i+6], b[i+7]]);
    let m = (1u64 << 51) - 1;
    [l(0) & m, (l(6) >> 3) & m, (l(12) >> 6) & m, (l(19) >> 1) & m, (l(24) >> 12) & m]
}

pub(crate) fn fe_to_bytes(f: &Fe) -> [u8; 32] {
    let mut t = *f;
    fe_carry(&mut t);
    // Reducción completa: t + 19, si cabe quitar p
    let m = (1u64 << 51) - 1;
    let mut q = (t[0] + 19) >> 51;
    q = (t[1] + q) >> 51; q = (t[2] + q) >> 51; q = (t[3] + q) >> 51; q = (t[4] + q) >> 51;
    t[0] += 19 * q;
    let mut c = t[0] >> 51; t[0] &= m; t[1] += c;
    c = t[1] >> 51; t[1] &= m; t[2] += c;
    c = t[2] >> 51; t[2] &= m; t[3] += c;
    c = t[3] >> 51; t[3] &= m; t[4] += c;
    t[4] &= m;
    let mut out = [0u8; 32];
    let v0 = t[0] | (t[1] << 51);
    let v1 = (t[1] >> 13) | (t[2] << 38);
    let v2 = (t[2] >> 26) | (t[3] << 25);
    let v3 = (t[3] >> 39) | (t[4] << 12);
    out[0..8].copy_from_slice(&v0.to_le_bytes()); out[8..16].copy_from_slice(&v1.to_le_bytes());
    out[16..24].copy_from_slice(&v2.to_le_bytes()); out[24..32].copy_from_slice(&v3.to_le_bytes());
    out
}

pub(crate) fn fe_carry(f: &mut Fe) {
    let m = (1u64 << 51) - 1;
    let mut c = f[0] >> 51; f[0] &= m; f[1] += c;
    c = f[1] >> 51; f[1] &= m; f[2] += c;
    c = f[2] >> 51; f[2] &= m; f[3] += c;
    c = f[3] >> 51; f[3] &= m; f[4] += c;
    c = f[4] >> 51; f[4] &= m; f[0] += c * 19;
    c = f[0] >> 51; f[0] &= m; f[1] += c;
}

pub(crate) fn fe_add(a: &Fe, b: &Fe) -> Fe { [a[0]+b[0], a[1]+b[1], a[2]+b[2], a[3]+b[3], a[4]+b[4]] }

pub(crate) fn fe_sub(a: &Fe, b: &Fe) -> Fe {
    // a - b + 2p (evita negativos)
    let mut r = [
        a[0] + 0xFFFFFFFFFFFDA - b[0], a[1] + 0xFFFFFFFFFFFFE - b[1], a[2] + 0xFFFFFFFFFFFFE - b[2],
        a[3] + 0xFFFFFFFFFFFFE - b[3], a[4] + 0xFFFFFFFFFFFFE - b[4]];
    fe_carry(&mut r); r
}

pub(crate) fn fe_mul(a: &Fe, b: &Fe) -> Fe {
    let (a0, a1, a2, a3, a4) = (a[0] as u128, a[1] as u128, a[2] as u128, a[3] as u128, a[4] as u128);
    let (b0, b1, b2, b3, b4) = (b[0] as u128, b[1] as u128, b[2] as u128, b[3] as u128, b[4] as u128);
    let (b1_19, b2_19, b3_19, b4_19) = (b1 * 19, b2 * 19, b3 * 19, b4 * 19);
    let mut t0 = a0 * b0 + a1 * b4_19 + a2 * b3_19 + a3 * b2_19 + a4 * b1_19;
    let mut t1 = a0 * b1 + a1 * b0 + a2 * b4_19 + a3 * b3_19 + a4 * b2_19;
    let mut t2 = a0 * b2 + a1 * b1 + a2 * b0 + a3 * b4_19 + a4 * b3_19;
    let mut t3 = a0 * b3 + a1 * b2 + a2 * b1 + a3 * b0 + a4 * b4_19;
    let mut t4 = a0 * b4 + a1 * b3 + a2 * b2 + a3 * b1 + a4 * b0;
    let m = (1u128 << 51) - 1;
    let mut c = t0 >> 51; t0 &= m; t1 += c;
    c = t1 >> 51; t1 &= m; t2 += c;
    c = t2 >> 51; t2 &= m; t3 += c;
    c = t3 >> 51; t3 &= m; t4 += c;
    c = t4 >> 51; t4 &= m; t0 += c * 19;
    c = t0 >> 51; t0 &= m; t1 += c;
    [t0 as u64, t1 as u64, t2 as u64, t3 as u64, t4 as u64]
}

pub(crate) fn fe_sq(a: &Fe) -> Fe { fe_mul(a, a) }

fn fe_mul121665(a: &Fe) -> Fe {
    // Producto en u128: un limb (< 2^52) por 121665 (~2^17) desborda u64.
    let m = (1u128 << 51) - 1;
    let mut t = [0u128; 5];
    for i in 0..5 { t[i] = (a[i] as u128) * 121665; }
    let mut c = t[0] >> 51; t[0] &= m; t[1] += c;
    c = t[1] >> 51; t[1] &= m; t[2] += c;
    c = t[2] >> 51; t[2] &= m; t[3] += c;
    c = t[3] >> 51; t[3] &= m; t[4] += c;
    c = t[4] >> 51; t[4] &= m; t[0] += c * 19;
    c = t[0] >> 51; t[0] &= m; t[1] += c;
    [t[0] as u64, t[1] as u64, t[2] as u64, t[3] as u64, t[4] as u64]
}

pub(crate) fn fe_invert(z: &Fe) -> Fe {
    // z^(p-2), p-2 = 2^255 - 21
    let mut r: Fe = [1, 0, 0, 0, 0];
    let mut base = *z;
    // exponente en bits (little-endian): 2^255-21
    let mut e = [0xFFu8; 32]; e[0] = 0xEB; e[31] = 0x7F;
    for i in 0..255 {
        if (e[i / 8] >> (i % 8)) & 1 == 1 { r = fe_mul(&r, &base); }
        base = fe_sq(&base);
    }
    r
}

fn fe_cswap(a: &mut Fe, b: &mut Fe, swap: u64) {
    let mask = 0u64.wrapping_sub(swap);
    for i in 0..5 { let t = mask & (a[i] ^ b[i]); a[i] ^= t; b[i] ^= t; }
}

/// Multiplicación escalar X25519 (RFC 7748 §5): escalera de Montgomery.
pub fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    let mut k = *scalar;
    k[0] &= 248; k[31] &= 127; k[31] |= 64;
    let mut u = *point; u[31] &= 127;
    let x1 = fe_from_bytes(&u);
    let mut x2: Fe = [1, 0, 0, 0, 0]; let mut z2: Fe = [0; 5];
    let mut x3 = x1; let mut z3: Fe = [1, 0, 0, 0, 0];
    let mut swap = 0u64;
    for t in (0..255).rev() {
        let kt = ((k[t / 8] >> (t % 8)) & 1) as u64;
        swap ^= kt;
        fe_cswap(&mut x2, &mut x3, swap); fe_cswap(&mut z2, &mut z3, swap);
        swap = kt;
        let a = fe_add(&x2, &z2); let aa = fe_sq(&a);
        let b = fe_sub(&x2, &z2); let bb = fe_sq(&b);
        let e = fe_sub(&aa, &bb);
        let c = fe_add(&x3, &z3); let d = fe_sub(&x3, &z3);
        let da = fe_mul(&d, &a); let cb = fe_mul(&c, &b);
        let mut s = fe_add(&da, &cb); fe_carry(&mut s);
        x3 = fe_sq(&s);
        let df = fe_sub(&da, &cb);
        z3 = fe_mul(&x1, &fe_sq(&df));
        x2 = fe_mul(&aa, &bb);
        let mut t2 = fe_add(&aa, &fe_mul121665(&e)); fe_carry(&mut t2);
        z2 = fe_mul(&e, &t2);
    }
    fe_cswap(&mut x2, &mut x3, swap); fe_cswap(&mut z2, &mut z3, swap);
    let r = fe_mul(&x2, &fe_invert(&z2));
    fe_to_bytes(&r)
}

pub fn x25519_base(scalar: &[u8; 32]) -> [u8; 32] {
    let mut g = [0u8; 32]; g[0] = 9;
    x25519(scalar, &g)
}

// ───────────────────────── Enteros grandes (RSA) ────────────────────────────

/// Entero sin signo de precisión arbitraria, limbs u32 little-endian.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigUint { pub limbs: Vec<u32> }

impl BigUint {
    pub fn from_be_bytes(b: &[u8]) -> Self {
        let mut limbs = Vec::with_capacity(b.len() / 4 + 1);
        let mut i = b.len();
        while i > 0 {
            let start = i.saturating_sub(4);
            let mut v = 0u32;
            for &x in &b[start..i] { v = (v << 8) | x as u32; }
            limbs.push(v);
            i = start;
        }
        let mut r = BigUint { limbs }; r.trim(); r
    }
    pub fn to_be_bytes(&self, len: usize) -> Vec<u8> {
        let mut out = alloc::vec![0u8; len];
        let mut pos = len;
        for &l in &self.limbs {
            for k in 0..4 {
                if pos == 0 { return out; }
                pos -= 1;
                out[pos] = (l >> (8 * k)) as u8;
            }
        }
        out
    }
    fn trim(&mut self) { while self.limbs.len() > 1 && *self.limbs.last().unwrap_or(&1) == 0 { self.limbs.pop(); } if self.limbs.is_empty() { self.limbs.push(0); } }
    pub fn is_zero(&self) -> bool { self.limbs.iter().all(|&l| l == 0) }
    pub fn bits(&self) -> usize {
        let n = self.limbs.len();
        if n == 0 { return 0; }
        let top = self.limbs[n - 1];
        if top == 0 { return 0; }
        (n - 1) * 32 + (32 - top.leading_zeros() as usize)
    }
    fn cmp(&self, o: &BigUint) -> core::cmp::Ordering {
        let (a, b) = (self.bits(), o.bits());
        if a != b { return a.cmp(&b); }
        let n = self.limbs.len().max(o.limbs.len());
        for i in (0..n).rev() {
            let x = *self.limbs.get(i).unwrap_or(&0); let y = *o.limbs.get(i).unwrap_or(&0);
            if x != y { return x.cmp(&y); }
        }
        core::cmp::Ordering::Equal
    }
    fn mul(&self, o: &BigUint) -> BigUint {
        let mut r = alloc::vec![0u32; self.limbs.len() + o.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in o.limbs.iter().enumerate() {
                let t = r[i + j] as u64 + (a as u64) * (b as u64) + carry;
                r[i + j] = t as u32; carry = t >> 32;
            }
            let mut k = i + o.limbs.len();
            while carry != 0 { let t = r[k] as u64 + carry; r[k] = t as u32; carry = t >> 32; k += 1; }
        }
        let mut b = BigUint { limbs: r }; b.trim(); b
    }
    /// Resto `self mod m` (Knuth, algoritmo D, base 2^32).
    fn rem(&self, m: &BigUint) -> BigUint {
        if self.cmp(m) == core::cmp::Ordering::Less { return self.clone(); }
        if m.limbs.len() == 1 {
            let d = m.limbs[0] as u64; let mut r = 0u64;
            for &l in self.limbs.iter().rev() { r = ((r << 32) | l as u64) % d; }
            return BigUint { limbs: alloc::vec![r as u32] };
        }
        let s = m.limbs.last().map(|v| v.leading_zeros()).unwrap_or(0);
        // [CIERRE] `shl_bits` deja un limbo alto a 0 cuando s>0 (p. ej. L de
        // Ed25519, 2^252+…): sin `trim` el divisor normalizado era 0.
        let mut v = m.shl_bits(s); v.trim(); let mut u = self.shl_bits(s);
        let n = v.limbs.len(); u.limbs.push(0);
        let mlen = u.limbs.len() - n - 1;
        let vn1 = v.limbs[n - 1] as u64; let vn2 = v.limbs[n - 2] as u64;
        for j in (0..=mlen).rev() {
            let num = ((u.limbs[j + n] as u64) << 32) | u.limbs[j + n - 1] as u64;
            let mut qhat = num / vn1; let mut rhat = num % vn1;
            while qhat >= (1 << 32) || qhat * vn2 > ((rhat << 32) | u.limbs[j + n - 2] as u64) {
                qhat -= 1; rhat += vn1; if rhat >= (1 << 32) { break; }
            }
            // u[j..j+n] -= qhat * v
            let mut borrow = 0i64; let mut carry = 0u64;
            for i in 0..n {
                let p = qhat * v.limbs[i] as u64 + carry; carry = p >> 32;
                let t = u.limbs[i + j] as i64 - (p & 0xFFFF_FFFF) as i64 + borrow;
                u.limbs[i + j] = t as u32; borrow = t >> 32;
            }
            let t = u.limbs[j + n] as i64 - carry as i64 + borrow;
            u.limbs[j + n] = t as u32;
            if t < 0 {
                // qhat era 1 de más: sumar v de vuelta
                let mut c = 0u64;
                for i in 0..n { let t = u.limbs[i + j] as u64 + v.limbs[i] as u64 + c; u.limbs[i + j] = t as u32; c = t >> 32; }
                u.limbs[j + n] = u.limbs[j + n].wrapping_add(c as u32);
            }
        }
        u.limbs.truncate(n);
        let mut r = u.shr_bits(s); r.trim(); r
    }
    fn shl_bits(&self, s: u32) -> BigUint {
        if s == 0 { return self.clone(); }
        let mut r = Vec::with_capacity(self.limbs.len() + 1); let mut carry = 0u32;
        for &l in &self.limbs { r.push((l << s) | carry); carry = l >> (32 - s); }
        r.push(carry);
        BigUint { limbs: r }
    }
    fn shr_bits(&self, s: u32) -> BigUint {
        if s == 0 { return self.clone(); }
        let mut r = alloc::vec![0u32; self.limbs.len()];
        for i in 0..self.limbs.len() {
            let hi = if i + 1 < self.limbs.len() { self.limbs[i + 1] << (32 - s) } else { 0 };
            r[i] = (self.limbs[i] >> s) | hi;
        }
        BigUint { limbs: r }
    }
    /// `self^e mod m` por cuadrados y multiplicaciones (e pequeño: 65537 típico).
    pub fn modpow(&self, e: &BigUint, m: &BigUint) -> BigUint {
        let mut result = BigUint { limbs: alloc::vec![1] };
        let mut base = self.rem(m);
        let nbits = e.bits();
        for i in 0..nbits {
            if (e.limbs[i / 32] >> (i % 32)) & 1 == 1 { result = result.mul(&base).rem(m); }
            if i + 1 < nbits { base = base.mul(&base).rem(m); }
        }
        result
    }
}

/// Clave pública RSA (n, e).
pub struct RsaPublicKey { pub n: BigUint, pub e: BigUint }

const DIGESTINFO_SHA256: [u8; 19] = [0x30,0x31,0x30,0x0d,0x06,0x09,0x60,0x86,0x48,0x01,0x65,0x03,0x04,0x02,0x01,0x05,0x00,0x04,0x20];
const DIGESTINFO_SHA384: [u8; 19] = [0x30,0x41,0x30,0x0d,0x06,0x09,0x60,0x86,0x48,0x01,0x65,0x03,0x04,0x02,0x02,0x05,0x00,0x04,0x30];
const DIGESTINFO_SHA512: [u8; 19] = [0x30,0x51,0x30,0x0d,0x06,0x09,0x60,0x86,0x48,0x01,0x65,0x03,0x04,0x02,0x03,0x05,0x00,0x04,0x40];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hash { Sha256, Sha384, Sha512 }

pub fn hash_msg(h: Hash, m: &[u8]) -> Vec<u8> {
    match h { Hash::Sha256 => sha256(m).to_vec(), Hash::Sha384 => sha384(m).to_vec(), Hash::Sha512 => sha512(m).to_vec() }
}

impl RsaPublicKey {
    fn em(&self, sig: &[u8]) -> Option<Vec<u8>> {
        let k = (self.n.bits() + 7) / 8;
        if sig.len() != k { return None; }
        let s = BigUint::from_be_bytes(sig);
        if s.cmp(&self.n) != core::cmp::Ordering::Less { return None; }
        Some(s.modpow(&self.e, &self.n).to_be_bytes(k))
    }
    /// RSASSA-PKCS1-v1_5 (RFC 8017 §8.2.2).
    pub fn verify_pkcs1(&self, h: Hash, msg: &[u8], sig: &[u8]) -> bool {
        let em = match self.em(sig) { Some(v) => v, None => return false };
        let digest = hash_msg(h, msg);
        let prefix: &[u8] = match h { Hash::Sha256 => &DIGESTINFO_SHA256, Hash::Sha384 => &DIGESTINFO_SHA384, Hash::Sha512 => &DIGESTINFO_SHA512 };
        let tlen = prefix.len() + digest.len();
        if em.len() < tlen + 11 { return false; }
        let mut expected = Vec::with_capacity(em.len());
        expected.push(0x00); expected.push(0x01);
        for _ in 0..(em.len() - tlen - 3) { expected.push(0xFF); }
        expected.push(0x00); expected.extend_from_slice(prefix); expected.extend_from_slice(&digest);
        ct_eq(&em, &expected)
    }
    /// [T-002] RSASSA-PKCS1-v1_5 sobre un resumen YA calculado con su
    /// `DigestInfo` DER (p. ej. SHA-1 de las firmas `.SIGN.RSA` de Alpine, cuyo
    /// hash no forma parte de TLS 1.3 y por eso no está en `Hash`).
    pub fn verify_pkcs1_digestinfo(&self, digestinfo: &[u8], digest: &[u8], sig: &[u8]) -> bool {
        let em = match self.em(sig) { Some(v) => v, None => return false };
        let tlen = digestinfo.len() + digest.len();
        if em.len() < tlen + 11 { return false; }
        let mut expected = Vec::with_capacity(em.len());
        expected.push(0x00); expected.push(0x01);
        for _ in 0..(em.len() - tlen - 3) { expected.push(0xFF); }
        expected.push(0x00); expected.extend_from_slice(digestinfo); expected.extend_from_slice(digest);
        ct_eq(&em, &expected)
    }
    /// RSASSA-PSS con MGF1 del mismo hash y sal de longitud = hash (RFC 8017 §9.1.2; TLS 1.3 §4.2.3).
    pub fn verify_pss(&self, h: Hash, msg: &[u8], sig: &[u8]) -> bool {
        let em = match self.em(sig) { Some(v) => v, None => return false };
        let mhash = hash_msg(h, msg);
        let hlen = mhash.len(); let slen = hlen;
        let embits = self.n.bits() - 1;
        let emlen = (embits + 7) / 8;
        let em = if em.len() > emlen { em[em.len() - emlen..].to_vec() } else { em };
        if emlen < hlen + slen + 2 { return false; }
        if *em.last().unwrap_or(&0) != 0xbc { return false; }
        let (masked_db, hh) = em.split_at(emlen - hlen - 1);
        let hh = &hh[..hlen];
        let top_bits = 8 * emlen - embits;
        if top_bits > 0 && (masked_db[0] >> (8 - top_bits)) != 0 { return false; }
        // MGF1
        let mut db = Vec::with_capacity(masked_db.len());
        let mut counter = 0u32;
        while db.len() < masked_db.len() {
            let mut seed = hh.to_vec(); seed.extend_from_slice(&counter.to_be_bytes());
            db.extend_from_slice(&hash_msg(h, &seed)); counter += 1;
        }
        db.truncate(masked_db.len());
        for i in 0..db.len() { db[i] ^= masked_db[i]; }
        if top_bits > 0 { db[0] &= 0xFF >> top_bits; }
        let ps_len = emlen - hlen - slen - 2;
        if db[..ps_len].iter().any(|&b| b != 0) || db[ps_len] != 0x01 { return false; }
        let salt = &db[ps_len + 1..];
        let mut mp = alloc::vec![0u8; 8]; mp.extend_from_slice(&mhash); mp.extend_from_slice(salt);
        ct_eq(&hash_msg(h, &mp), hh)
    }
}

impl BigUint {
    /// Suma.
    pub fn add(&self, o: &BigUint) -> BigUint {
        let n = self.limbs.len().max(o.limbs.len());
        let mut r = Vec::with_capacity(n + 1); let mut c = 0u64;
        for i in 0..n { let t = *self.limbs.get(i).unwrap_or(&0) as u64 + *o.limbs.get(i).unwrap_or(&0) as u64 + c; r.push(t as u32); c = t >> 32; }
        if c != 0 { r.push(c as u32); }
        let mut b = BigUint { limbs: r }; b.trim(); b
    }
    /// Resta (requiere self >= o; si no, devuelve 0).
    pub fn sub(&self, o: &BigUint) -> BigUint {
        if self.cmp(o) == core::cmp::Ordering::Less { return BigUint { limbs: alloc::vec![0] }; }
        let mut r = Vec::with_capacity(self.limbs.len()); let mut borrow = 0i64;
        for i in 0..self.limbs.len() { let t = self.limbs[i] as i64 - *o.limbs.get(i).unwrap_or(&0) as i64 + borrow; r.push(t as u32); borrow = t >> 32; }
        let mut b = BigUint { limbs: r }; b.trim(); b
    }
    pub fn mul_pub(&self, o: &BigUint) -> BigUint { self.mul(o) }
    pub fn rem_pub(&self, m: &BigUint) -> BigUint { self.rem(m) }
    pub fn lt(&self, o: &BigUint) -> bool { self.cmp(o) == core::cmp::Ordering::Less }
}

/// Clave privada RSA (RFC 8017 §3.2) con los parámetros CRT para firmar en ~1/4 del tiempo.
pub struct RsaPrivateKey { pub n: BigUint, pub e: BigUint, pub d: BigUint, pub p: BigUint, pub q: BigUint, pub dp: BigUint, pub dq: BigUint, pub qinv: BigUint }

impl RsaPrivateKey {
    pub fn bits(&self) -> usize { self.n.bits() }
    /// m^d mod n por el teorema chino del resto (§5.1.2, representación quíntuple).
    fn rsadp(&self, c: &BigUint) -> BigUint {
        let m1 = c.modpow(&self.dp, &self.p);
        let m2 = c.modpow(&self.dq, &self.q);
        let diff = if m1.lt(&m2) { m1.add(&self.p).sub(&m2) } else { m1.sub(&m2) };
        let h = self.qinv.mul(&diff).rem(&self.p);
        m2.add(&h.mul(&self.q))
    }
    /// RSASSA-PSS (RFC 8017 §8.1.1) con MGF1 del mismo hash y sal de longitud = hash (TLS 1.3 §4.2.3).
    pub fn sign_pss(&self, h: Hash, msg: &[u8]) -> Vec<u8> {
        let mhash = hash_msg(h, msg); let hlen = mhash.len(); let slen = hlen;
        let embits = self.n.bits() - 1; let emlen = (embits + 7) / 8;
        let mut salt = alloc::vec![0u8; slen]; random_bytes(&mut salt);
        let mut mp = alloc::vec![0u8; 8]; mp.extend_from_slice(&mhash); mp.extend_from_slice(&salt);
        let hh = hash_msg(h, &mp);
        let ps_len = emlen - slen - hlen - 2;
        let mut db = alloc::vec![0u8; ps_len]; db.push(0x01); db.extend_from_slice(&salt);
        let mut mask = Vec::with_capacity(db.len()); let mut counter = 0u32;
        while mask.len() < db.len() { let mut seed = hh.clone(); seed.extend_from_slice(&counter.to_be_bytes()); mask.extend_from_slice(&hash_msg(h, &seed)); counter += 1; }
        for i in 0..db.len() { db[i] ^= mask[i]; }
        let top_bits = 8 * emlen - embits; if top_bits > 0 { db[0] &= 0xFF >> top_bits; }
        let mut em = db; em.extend_from_slice(&hh); em.push(0xbc);
        let k = (self.n.bits() + 7) / 8;
        let s = self.rsadp(&BigUint::from_be_bytes(&em));
        s.to_be_bytes(k)
    }
    /// Autocomprobación: firma y verifica con la pública (detecta claves CRT corruptas).
    pub fn self_check(&self) -> bool {
        let pk = RsaPublicKey { n: self.n.clone(), e: self.e.clone() };
        let sig = self.sign_pss(Hash::Sha256, b"portix");
        pk.verify_pss(Hash::Sha256, b"portix", &sig)
    }
}

// ───────────────────────────── Aleatoriedad ─────────────────────────────────

/// Bytes aleatorios: `getrandom` del núcleo mezclado con TSC vía SHA-256.
pub fn random_bytes(out: &mut [u8]) {
    let mut pool = [0u8; 64];
    let _ = crate::syscall::sys_getrandom(pool.as_mut_ptr(), 32, 0);
    let mut ctr = 0u64;
    for chunk in out.chunks_mut(32) {
        let tsc: u64 = unsafe { core::arch::x86_64::_rdtsc() };
        pool[32..40].copy_from_slice(&tsc.to_le_bytes());
        pool[40..48].copy_from_slice(&ctr.to_le_bytes());
        let h = sha256(&pool);
        chunk.copy_from_slice(&h[..chunk.len()]);
        pool[48..64].copy_from_slice(&h[..16]);
        ctr += 1;
    }
}

/// HMAC-SHA256 re-exportado para `Finished`.
pub fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] { hmac_sha256(key, data) }
