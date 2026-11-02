//! [T-002] SHA-1 (RFC 3174). No es criptográficamente fuerte, pero es lo que
//! Alpine usa como IDENTIDAD de paquete (`C:Q1<base64>` en APKINDEX) y como
//! hash de la firma `.SIGN.RSA` (PKCS#1 v1.5). La autenticidad la aporta la
//! firma RSA-4096; SHA-1 aquí sólo tiene que coincidir con lo que firmó Alpine.

pub fn sha1(msg: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut datos = alloc::vec::Vec::with_capacity(msg.len() + 72);
    datos.extend_from_slice(msg);
    datos.push(0x80);
    while datos.len() % 64 != 56 { datos.push(0); }
    datos.extend_from_slice(&((msg.len() as u64) * 8).to_be_bytes());
    for bloque in datos.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 { w[i] = u32::from_be_bytes([bloque[4 * i], bloque[4 * i + 1], bloque[4 * i + 2], bloque[4 * i + 3]]); }
        for i in 16..80 { w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1); }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d; d = c; c = b.rotate_left(30); b = a; a = t;
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d); h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for i in 0..5 { out[4 * i..4 * i + 4].copy_from_slice(&h[i].to_be_bytes()); }
    out
}

/// `DigestInfo` DER de SHA-1 (RFC 8017 §9.2, nota 1).
pub const DIGESTINFO_SHA1: [u8; 15] = [0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14];

pub fn base64(d: &[u8]) -> alloc::string::String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = alloc::string::String::new();
    for ch in d.chunks(3) {
        let n = ((ch[0] as u32) << 16) | ((*ch.get(1).unwrap_or(&0) as u32) << 8) | (*ch.get(2).unwrap_or(&0) as u32);
        s.push(T[(n >> 18) as usize & 63] as char); s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if ch.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if ch.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}
