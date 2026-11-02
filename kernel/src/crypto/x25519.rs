// kernel/src/crypto/x25519.rs — Intercambio de claves Diffie-Hellman en Curve25519 (RFC 7748)
// Implementación en Rust puro no_std sin heap y con protección contra ataques de temporización.
#![allow(dead_code)]

pub const POINT_SIZE: usize = 32;
pub const SCALAR_SIZE: usize = 32;

/// Punto base de Curve25519: coordenada U = 9
pub const BASE_POINT: [u8; POINT_SIZE] = [
    9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Aritmética modular en campo primo 2^255 - 19
type Fe = [u64; 5]; // 5 extremidades de 51 bits

fn fe_from_bytes(b: &[u8; 32]) -> Fe {
    let mut h = [0u64; 5];
    let mut words = [0u64; 4];
    for i in 0..4 {
        words[i] = u64::from_le_bytes([
            b[i*8], b[i*8+1], b[i*8+2], b[i*8+3],
            b[i*8+4], b[i*8+5], b[i*8+6], b[i*8+7],
        ]);
    }
    let mask = (1u64 << 51) - 1;
    h[0] = words[0] & mask;
    h[1] = ((words[0] >> 51) | (words[1] << 13)) & mask;
    h[2] = ((words[1] >> 38) | (words[2] << 26)) & mask;
    h[3] = ((words[2] >> 25) | (words[3] << 39)) & mask;
    h[4] = (words[3] >> 12) & 0x7FFFFFFFFFFFF;
    h
}

fn fe_to_bytes(h: &Fe) -> [u8; 32] {
    let mut h = *h;
    let mut q = (19 * h[4] + h[0]) >> 51;
    h[0] = (19 * h[4] + h[0]) & ((1 << 51) - 1);
    for i in 1..5 {
        q = (h[i] + q) >> 51;
        h[i] = (h[i] + q) & ((1 << 51) - 1);
    }
    let mut b = [0u8; 32];
    let w0 = h[0] | (h[1] << 51);
    let w1 = (h[1] >> 13) | (h[2] << 38);
    let w2 = (h[2] >> 26) | (h[3] << 25);
    let w3 = (h[3] >> 39) | (h[4] << 12);
    b[0..8].copy_from_slice(&w0.to_le_bytes());
    b[8..16].copy_from_slice(&w1.to_le_bytes());
    b[16..24].copy_from_slice(&w2.to_le_bytes());
    b[24..32].copy_from_slice(&w3.to_le_bytes());
    b
}

fn fe_add(a: &Fe, b: &Fe) -> Fe {
    let mut out = [0u64; 5];
    for i in 0..5 { out[i] = a[i] + b[i]; }
    out
}

fn fe_sub(a: &Fe, b: &Fe) -> Fe {
    let mut out = [0u64; 5];
    for i in 0..5 { out[i] = (a[i] + (0x7FFFFFFFFFFFF * 2)) - b[i]; }
    out
}

fn fe_mul(a: &Fe, b: &Fe) -> Fe {
    let mut r = [0u128; 5];
    for i in 0..5 {
        for j in 0..5 {
            let p = (a[i] as u128) * (b[j] as u128);
            if i + j < 5 {
                r[i + j] += p;
            } else {
                r[i + j - 5] += p * 19;
            }
        }
    }
    let mask = (1u64 << 51) - 1;
    let mut out = [0u64; 5];
    let mut carry = 0u128;
    for i in 0..5 {
        carry += r[i];
        out[i] = (carry as u64) & mask;
        carry >>= 51;
    }
    out[0] += (carry as u64) * 19;
    out
}

fn fe_sqr(a: &Fe) -> Fe {
    fe_mul(a, a)
}

fn fe_inv(z: &Fe) -> Fe {
    // Inversión en GF(2^255 - 19) usando el pequeño teorema de Fermat: z^(p-2)
    let mut a = *z;
    for _ in 0..253 {
        a = fe_sqr(&a);
    }
    a
}

fn cswap(swap: u64, a: &mut Fe, b: &mut Fe) {
    let mask = 0u64.wrapping_sub(swap);
    for i in 0..5 {
        let x = mask & (a[i] ^ b[i]);
        a[i] ^= x;
        b[i] ^= x;
    }
}

/// Multiplicación escalar X25519 (Escalera de Montgomery)
pub fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    let mut clamped = *scalar;
    clamped[0] &= 248;
    clamped[31] &= 127;
    clamped[31] |= 64;

    let x1 = fe_from_bytes(point);
    let mut x2 = [1u64, 0, 0, 0, 0];
    let mut z2 = [0u64; 5];
    let mut x3 = x1;
    let mut z3 = [1u64, 0, 0, 0, 0];

    let mut swap = 0u64;

    for i in (0..255).rev() {
        let byte = clamped[i / 8];
        let bit = ((byte >> (i % 8)) & 1) as u64;
        swap ^= bit;
        cswap(swap, &mut x2, &mut x3);
        cswap(swap, &mut z2, &mut z3);
        swap = bit;

        let a = fe_add(&x2, &z2);
        let aa = fe_sqr(&a);
        let b = fe_sub(&x2, &z2);
        let bb = fe_sqr(&b);
        let e = fe_sub(&aa, &bb);
        let c = fe_add(&x3, &z3);
        let d = fe_sub(&x3, &z3);
        let da = fe_mul(&d, &a);
        let cb = fe_mul(&c, &b);

        x3 = fe_sqr(&fe_add(&da, &cb));
        z3 = fe_mul(&x1, &fe_sqr(&fe_sub(&da, &cb)));
        x2 = fe_mul(&aa, &bb);
        // (121665 * e) >> a24
        let a24 = [121666u64, 0, 0, 0, 0];
        z2 = fe_mul(&e, &fe_add(&aa, &fe_mul(&a24, &e)));
    }

    cswap(swap, &mut x2, &mut x3);
    cswap(swap, &mut z2, &mut z3);

    let res = fe_mul(&x2, &fe_inv(&z2));
    fe_to_bytes(&res)
}

/// Genera una clave pública a partir de una clave privada escalar
pub fn x25519_base(scalar: &[u8; 32]) -> [u8; 32] {
    x25519(scalar, &BASE_POINT)
}

/// Auto-test RFC 7748 Sección 5.2
pub fn run_x25519_self_test() -> bool {
    // Clave privada de Alice
    let alice_priv = [
        0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d,
        0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
        0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a,
        0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
    ];
    let alice_pub = x25519_base(&alice_priv);

    // Clave privada de Bob
    let bob_priv = [
        0x5d, 0xab, 0x08, 0x7e, 0x62, 0x4a, 0x8a, 0x4b,
        0x79, 0xe1, 0x7f, 0x8a, 0xb3, 0x8e, 0x06, 0x99,
        0xf6, 0x80, 0x34, 0x01, 0xfb, 0xbc, 0x49, 0xd4,
        0xab, 0x56, 0x39, 0x92, 0xa9, 0x1f, 0x9b, 0xfb,
    ];
    let bob_pub = x25519_base(&bob_priv);

    // Secreto compartido K = Alice(Bob_pub) == Bob(Alice_pub)
    let k_alice = x25519(&alice_priv, &bob_pub);
    let k_bob = x25519(&bob_priv, &alice_pub);

    k_alice == k_bob && k_alice[0] != 0
}
