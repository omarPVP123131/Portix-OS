//! Módulo Criptográfico TLS 1.3 y HTTPS Puro para Portix OS (100% Rust no_std)
//! Implementación ligera de RFC 8446 (TLS 1.3), RFC 8439 (ChaCha20-Poly1305) y SHA-256

use alloc::vec::Vec;

/// Función de Hash Criptográfico SHA-256 estándar (FIPS 180-4)
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 64);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, part) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(part.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = h[0]; let mut b = h[1]; let mut c = h[2]; let mut d = h[3];
        let mut e = h[4]; let mut f = h[5]; let mut g = h[6]; let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g; g = f; f = e; e = d.wrapping_add(temp1);
            d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
    }
    out
}

/// HMAC-SHA256 estándar (RFC 2104)
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        let hash = sha256(key);
        k[..32].copy_from_slice(&hash);
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    let mut inner_data = Vec::with_capacity(64 + data.len());
    inner_data.extend_from_slice(&ipad);
    inner_data.extend_from_slice(data);
    let inner_hash = sha256(&inner_data);

    let mut outer_data = Vec::with_capacity(64 + 32);
    outer_data.extend_from_slice(&opad);
    outer_data.extend_from_slice(&inner_hash);
    sha256(&outer_data)
}

/// Derivación de claves HKDF-Extract (RFC 5869 / TLS 1.3)
pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    let s = if salt.is_empty() { &[0u8; 32] } else { salt };
    hmac_sha256(s, ikm)
}

/// Derivación de claves HKDF-Expand-Label (RFC 8446 Sec. 7.1)
pub fn hkdf_expand_label(secret: &[u8; 32], label: &str, context: &[u8], length: u16) -> Vec<u8> {
    let prefix = b"tls13 ";
    let mut hkdf_label = Vec::new();
    hkdf_label.extend_from_slice(&length.to_be_bytes());
    let full_label_len = (prefix.len() + label.len()) as u8;
    hkdf_label.push(full_label_len);
    hkdf_label.extend_from_slice(prefix);
    hkdf_label.extend_from_slice(label.as_bytes());
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);

    let mut t = Vec::new();
    let mut prev = Vec::new();
    let mut round = 1u8;
    while t.len() < length as usize {
        let mut block_data = Vec::new();
        block_data.extend_from_slice(&prev);
        block_data.extend_from_slice(&hkdf_label);
        block_data.push(round);
        let block_hash = hmac_sha256(secret, &block_data);
        t.extend_from_slice(&block_hash);
        prev = block_hash.to_vec();
        round += 1;
    }
    t.truncate(length as usize);
    t
}

/// Cifrador de Flujo ChaCha20 (RFC 8439)
pub fn chacha20_xor(key: &[u8; 32], counter: u32, nonce: &[u8; 12], buf: &mut [u8]) {
    let mut state = [0u32; 16];
    // Constantes "expand 32-byte k"
    state[0] = 0x61707865; state[1] = 0x3320646e; state[2] = 0x79622d32; state[3] = 0x6b206574;
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes(key[i * 4..(i + 1) * 4].try_into().unwrap());
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes(nonce[i * 4..(i + 1) * 4].try_into().unwrap());
    }

    let mut cur_counter = counter;
    for chunk in buf.chunks_mut(64) {
        state[12] = cur_counter;
        let mut x = state;
        for _ in 0..10 {
            // Rondas de columnas y diagonales
            macro_rules! qr {
                ($a:expr, $b:expr, $c:expr, $d:expr) => {
                    x[$a] = x[$a].wrapping_add(x[$b]); x[$d] = (x[$d] ^ x[$a]).rotate_left(16);
                    x[$c] = x[$c].wrapping_add(x[$d]); x[$b] = (x[$b] ^ x[$c]).rotate_left(12);
                    x[$a] = x[$a].wrapping_add(x[$b]); x[$d] = (x[$d] ^ x[$a]).rotate_left(8);
                    x[$c] = x[$c].wrapping_add(x[$d]); x[$b] = (x[$b] ^ x[$c]).rotate_left(7);
                };
            }
            qr!(0, 4, 8, 12); qr!(1, 5, 9, 13); qr!(2, 6, 10, 14); qr!(3, 7, 11, 15);
            qr!(0, 5, 10, 15); qr!(1, 6, 11, 12); qr!(2, 7, 8, 13); qr!(3, 4, 9, 14);
        }
        for i in 0..16 {
            x[i] = x[i].wrapping_add(state[i]);
        }
        let mut key_stream = [0u8; 64];
        for i in 0..16 {
            key_stream[i * 4..(i + 1) * 4].copy_from_slice(&x[i].to_le_bytes());
        }
        let clen = chunk.len();
        for (b, &k) in chunk.iter_mut().zip(&key_stream[..clen]) {
            *b ^= k;
        }
        cur_counter = cur_counter.wrapping_add(1);
    }
}

// [CIERRE B-3] El antiguo `Tls13Server` (ServerHello con valores fijos y un
// "vuelo cifrado" que no cifraba nada) se ha ELIMINADO: era simulación y sólo
// lo consumían `httpd --test` y los TEST 2713/2714, que ahora ejercitan
// ECDSA P-256 y la cadena X.509 reales de `crate::tls13`. El TLS 1.3 de lado
// servidor (firma RSA-PSS/ECDSA con clave privada PKCS#8) queda PENDIENTE y
// así se declara en `httpd`.

