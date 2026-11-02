//! Ed25519 (RFC 8032) — verificación y FIRMA (clave de cliente mTLS), sin crates. Reutiliza el cuerpo
//! F_(2^255−19) de `crypto.rs` (X25519). Coordenadas extendidas (X:Y:Z:T),
//! curva torcida de Edwards −x² + y² = 1 + d·x²·y².
//!
//! Comprobación: [S]B = R + [k]A con k = SHA-512(R ‖ A ‖ M) mod L.

use alloc::vec::Vec;
use super::crypto::{Fe, fe_from_bytes, fe_to_bytes, fe_add, fe_sub, fe_mul, fe_sq, fe_invert, fe_carry, sha512, BigUint};

const D_BYTES: [u8; 32] = [0xa3,0x78,0x59,0x13,0xca,0x4d,0xeb,0x75,0xab,0xd8,0x41,0x41,0x4d,0x0a,0x70,0x00,0x98,0xe8,0x79,0x77,0x79,0x40,0xc7,0x8c,0x73,0xfe,0x6f,0x2b,0xee,0x6c,0x03,0x52];
const SQRT_M1_BYTES: [u8; 32] = [0xb0,0xa0,0x0e,0x4a,0x27,0x1b,0xee,0xc4,0x78,0xe4,0x2f,0xad,0x06,0x18,0x43,0x2f,0xa7,0xd7,0xfb,0x3d,0x99,0x00,0x4d,0x2b,0x0b,0xdf,0xc1,0x4f,0x80,0x24,0x83,0x2b];
/// Orden L del subgrupo (big-endian).
const L_BE: [u8; 32] = [0x10,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0x14,0xde,0xf9,0xde,0xa2,0xf7,0x9c,0xd6,0x58,0x12,0x63,0x1a,0x5c,0xf5,0xd3,0xed];
const BX_BYTES: [u8; 32] = [0x1a,0xd5,0x25,0x8f,0x60,0x2d,0x56,0xc9,0xb2,0xa7,0x25,0x95,0x60,0xc7,0x2c,0x69,0x5c,0xdc,0xd6,0xfd,0x31,0xe2,0xa4,0xc0,0xfe,0x53,0x6e,0xcd,0xd3,0x36,0x69,0x21];
const BY_BYTES: [u8; 32] = [0x58,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66,0x66];

fn fe_one() -> Fe { [1, 0, 0, 0, 0] }
fn fe_zero() -> Fe { [0; 5] }
fn fe_eq(a: &Fe, b: &Fe) -> bool { fe_to_bytes(a) == fe_to_bytes(b) }
fn fe_neg(a: &Fe) -> Fe { fe_sub(&fe_zero(), &fe_mul(a, &fe_one())) }
fn fe_is_neg(a: &Fe) -> bool { fe_to_bytes(a)[0] & 1 == 1 }
/// a^((p−5)/8) para la raíz cuadrada (RFC 8032 §5.1.3).
fn fe_pow_p58(a: &Fe) -> Fe {
    // (p-5)/8 = 2^252 - 3 : bits LE: todos 1 salvo bit0=1,bit1=0 -> 0b...11111101
    let mut e = [0xFFu8; 32]; e[0] = 0xFD; e[31] = 0x0F;
    let mut r = fe_one(); let mut base = *a;
    for i in 0..252 { if (e[i / 8] >> (i % 8)) & 1 == 1 { r = fe_mul(&r, &base); } base = fe_sq(&base); }
    r
}

#[derive(Clone, Copy)]
struct Pt { x: Fe, y: Fe, z: Fe, t: Fe }

fn pt_identity() -> Pt { Pt { x: fe_zero(), y: fe_one(), z: fe_one(), t: fe_zero() } }

fn pt_add(p: &Pt, q: &Pt, d2: &Fe) -> Pt {
    // add-2008-hwcd-3 (a = −1)
    let a = fe_mul(&fe_sub(&p.y, &p.x), &fe_sub(&q.y, &q.x));
    let b = fe_mul(&fe_add(&p.y, &p.x), &fe_add(&q.y, &q.x));
    let c = fe_mul(&fe_mul(&p.t, d2), &q.t);
    let dd = fe_mul(&fe_add(&p.z, &p.z), &q.z);
    let e = fe_sub(&b, &a); let f = fe_sub(&dd, &c); let g = fe_add(&dd, &c); let h = fe_add(&b, &a);
    let mut r = Pt { x: fe_mul(&e, &f), y: fe_mul(&g, &h), z: fe_mul(&f, &g), t: fe_mul(&e, &h) };
    fe_carry(&mut r.x); fe_carry(&mut r.y); fe_carry(&mut r.z); fe_carry(&mut r.t); r
}
fn pt_double(p: &Pt) -> Pt {
    // dbl-2008-hwcd
    let a = fe_sq(&p.x); let b = fe_sq(&p.y); let c = { let z2 = fe_sq(&p.z); fe_add(&z2, &z2) };
    let h = fe_add(&a, &b);
    let e = fe_sub(&h, &fe_sq(&fe_add(&p.x, &p.y)));
    let g = fe_sub(&a, &b); let f = fe_add(&c, &g);
    let mut r = Pt { x: fe_mul(&e, &f), y: fe_mul(&g, &h), z: fe_mul(&f, &g), t: fe_mul(&e, &h) };
    fe_carry(&mut r.x); fe_carry(&mut r.y); fe_carry(&mut r.z); fe_carry(&mut r.t); r
}
fn pt_mul(p: &Pt, k: &[u8; 32], d2: &Fe) -> Pt {
    let mut r = pt_identity();
    for i in (0..256).rev() { r = pt_double(&r); if (k[i / 8] >> (i % 8)) & 1 == 1 { r = pt_add(&r, p, d2); } }
    r
}
fn pt_encode(p: &Pt) -> [u8; 32] {
    let zi = fe_invert(&p.z); let x = fe_mul(&p.x, &zi); let y = fe_mul(&p.y, &zi);
    let mut out = fe_to_bytes(&y); out[31] |= (fe_to_bytes(&x)[0] & 1) << 7; out
}
/// Descompresión (RFC 8032 §5.1.3).
fn pt_decode(b: &[u8; 32], d: &Fe) -> Option<Pt> {
    let mut yb = *b; let sign = yb[31] >> 7; yb[31] &= 0x7f;
    // y < p
    if yb[31] == 0x7f && yb[..31].iter().all(|&v| v == 0xff) && yb[0] >= 0xed { return None; }
    let y = fe_from_bytes(&yb);
    let y2 = fe_sq(&y);
    let u = fe_sub(&y2, &fe_one()); let v = fe_add(&fe_mul(d, &y2), &fe_one());
    let v3 = fe_mul(&fe_sq(&v), &v); let v7 = fe_mul(&fe_sq(&v3), &v);
    let mut x = fe_mul(&fe_mul(&u, &v3), &fe_pow_p58(&fe_mul(&u, &v7)));
    let vx2 = fe_mul(&v, &fe_sq(&x));
    if fe_eq(&vx2, &u) {
    } else if fe_eq(&vx2, &fe_neg(&u)) {
        x = fe_mul(&x, &fe_from_bytes(&SQRT_M1_BYTES));
    } else { return None; }
    if fe_to_bytes(&x) == [0u8; 32] && sign == 1 { return None; }
    if (fe_is_neg(&x) as u8) != sign { x = fe_neg(&x); }
    let t = fe_mul(&x, &y);
    Some(Pt { x, y, z: fe_one(), t })
}

/// Reduce 64 bytes little-endian módulo L → 32 bytes little-endian.
fn reduce_mod_l(h: &[u8; 64]) -> [u8; 32] {
    let mut be = [0u8; 64]; for i in 0..64 { be[i] = h[63 - i]; }
    let l = BigUint::from_be_bytes(&L_BE);
    let r = BigUint::from_be_bytes(&be).modpow(&BigUint::from_be_bytes(&[1]), &l).to_be_bytes(32);
    let mut le = [0u8; 32]; for i in 0..32 { le[i] = r[31 - i]; } le
}

/// Clave pública Ed25519 (32 bytes comprimidos).
#[derive(Clone)]
pub struct Ed25519PublicKey { a_bytes: [u8; 32] }

impl Ed25519PublicKey {
    pub fn bytes(&self) -> [u8; 32] { self.a_bytes }
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() != 32 { return None; }
        let mut a = [0u8; 32]; a.copy_from_slice(b);
        let d = fe_from_bytes(&D_BYTES);
        pt_decode(&a, &d)?;
        Some(Ed25519PublicKey { a_bytes: a })
    }
    /// Verifica `sig` (64 bytes R‖S) sobre `msg` (PureEdDSA, sin prehash).
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        if sig.len() != 64 { return false; }
        let d = fe_from_bytes(&D_BYTES); let d2 = fe_add(&d, &d);
        let mut rb = [0u8; 32]; rb.copy_from_slice(&sig[..32]);
        let mut sb = [0u8; 32]; sb.copy_from_slice(&sig[32..]);
        // S < L (big-endian de S little-endian)
        let mut s_be = [0u8; 32]; for i in 0..32 { s_be[i] = sb[31 - i]; }
        if s_be >= L_BE { return false; }
        let a = match pt_decode(&self.a_bytes, &d) { Some(p) => p, None => return false };
        let r = match pt_decode(&rb, &d) { Some(p) => p, None => return false };
        let mut m = Vec::with_capacity(64 + msg.len());
        m.extend_from_slice(&rb); m.extend_from_slice(&self.a_bytes); m.extend_from_slice(msg);
        let k = reduce_mod_l(&sha512(&m));
        let bx = fe_from_bytes(&BX_BYTES); let by = fe_from_bytes(&BY_BYTES);
        let b = Pt { x: bx, y: by, z: fe_one(), t: fe_mul(&bx, &by) };
        let lhs = pt_mul(&b, &sb, &d2);
        let rhs = pt_add(&r, &pt_mul(&a, &k, &d2), &d2);
        pt_encode(&lhs) == pt_encode(&rhs)
    }
}

/// Autoprueba: RFC 8032 §7.1 TEST 2 (mensaje de 1 byte 0x72).
pub fn self_test() -> bool {
    let pk = [0x3d,0x40,0x17,0xc3,0xe8,0x43,0x89,0x5a,0x92,0xb7,0x0a,0xa7,0x4d,0x1b,0x7e,0xbc,0x9c,0x98,0x2c,0xcf,0x2e,0xc4,0x96,0x8c,0xc0,0xcd,0x55,0xf1,0x2a,0xf4,0x66,0x0c];
    let sig: [u8; 64] = [0x92,0xa0,0x09,0xa9,0xf0,0xd4,0xca,0xb8,0x72,0x0e,0x82,0x0b,0x5f,0x64,0x25,0x40,0xa2,0xb2,0x7b,0x54,0x16,0x50,0x3f,0x8f,0xb3,0x76,0x22,0x23,0xeb,0xdb,0x69,0xda,0x08,0x5a,0xc1,0xe4,0x3e,0x15,0x99,0x6e,0x45,0x8f,0x36,0x13,0xd0,0xf1,0x1d,0x8c,0x38,0x7b,0x2e,0xae,0xb4,0x30,0x2a,0xee,0xb0,0x0d,0x29,0x16,0x12,0xbb,0x0c,0x00];
    let key = match Ed25519PublicKey::from_bytes(&pk) { Some(k) => k, None => return false };
    let ok = key.verify(&[0x72], &sig);
    let mut bad = sig; bad[0] ^= 1;
    let mut bad_msg_ok = key.verify(&[0x73], &sig);
    bad_msg_ok |= key.verify(&[0x72], &bad);
    ok && !bad_msg_ok
}

/// Clave privada Ed25519 (RFC 8032 §5.1.5): semilla de 32 bytes → (s, prefijo, A).
pub struct Ed25519PrivateKey { s: [u8; 32], prefix: [u8; 32], a_bytes: [u8; 32] }

fn le_to_big(le: &[u8]) -> BigUint { let mut be = alloc::vec![0u8; le.len()]; for i in 0..le.len() { be[i] = le[le.len() - 1 - i]; } BigUint::from_be_bytes(&be) }
fn big_to_le32(b: &BigUint) -> [u8; 32] { let v = b.to_be_bytes(32); let mut le = [0u8; 32]; for i in 0..32 { le[i] = v[31 - i]; } le }

impl Ed25519PrivateKey {
    pub fn from_seed(seed: &[u8]) -> Option<Self> {
        if seed.len() != 32 { return None; }
        let h = sha512(seed);
        let mut s = [0u8; 32]; s.copy_from_slice(&h[..32]);
        s[0] &= 248; s[31] &= 127; s[31] |= 64;
        let mut prefix = [0u8; 32]; prefix.copy_from_slice(&h[32..]);
        let d2 = { let d = fe_from_bytes(&D_BYTES); fe_add(&d, &d) };
        let b = Pt { x: fe_from_bytes(&BX_BYTES), y: fe_from_bytes(&BY_BYTES), z: fe_one(), t: fe_mul(&fe_from_bytes(&BX_BYTES), &fe_from_bytes(&BY_BYTES)) };
        let a_bytes = pt_encode(&pt_mul(&b, &s, &d2));
        Some(Ed25519PrivateKey { s, prefix, a_bytes })
    }
    pub fn public_bytes(&self) -> [u8; 32] { self.a_bytes }
    /// Firma pura (§5.1.6): R = [r]B, S = (r + k·s) mod L, r = SHA-512(prefijo ‖ M), k = SHA-512(R ‖ A ‖ M).
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let d2 = { let d = fe_from_bytes(&D_BYTES); fe_add(&d, &d) };
        let b = Pt { x: fe_from_bytes(&BX_BYTES), y: fe_from_bytes(&BY_BYTES), z: fe_one(), t: fe_mul(&fe_from_bytes(&BX_BYTES), &fe_from_bytes(&BY_BYTES)) };
        let mut pm = Vec::with_capacity(32 + msg.len()); pm.extend_from_slice(&self.prefix); pm.extend_from_slice(msg);
        let r = reduce_mod_l(&sha512(&pm));
        let rb = pt_encode(&pt_mul(&b, &r, &d2));
        let mut km = Vec::with_capacity(64 + msg.len()); km.extend_from_slice(&rb); km.extend_from_slice(&self.a_bytes); km.extend_from_slice(msg);
        let k = reduce_mod_l(&sha512(&km));
        let l = BigUint::from_be_bytes(&L_BE);
        let sbig = le_to_big(&k).mul_pub(&le_to_big(&self.s)).add(&le_to_big(&r)).rem_pub(&l);
        let mut sig = Vec::with_capacity(64); sig.extend_from_slice(&rb); sig.extend_from_slice(&big_to_le32(&sbig)); sig
    }
    /// Autocomprobación con el verificador existente.
    pub fn self_check(&self) -> bool {
        match Ed25519PublicKey::from_bytes(&self.a_bytes) { Some(pk) => pk.verify(b"portix", &self.sign(b"portix")), None => false }
    }
}
