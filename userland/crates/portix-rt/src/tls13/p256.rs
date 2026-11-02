//! ECDSA sobre secp256r1 (NIST P-256) — sólo VERIFICACIÓN, sin crates.
//!
//! [CIERRE B-3] Cierra el pendiente "ECDSA P-256" del Frente B-2: muchas hojas
//! públicas (y CAs intermedias) firman con `ecdsa_secp256r1_sha256`.
//!
//! * Aritmética del cuerpo F_p con 8 palabras de 32 bits y la reducción rápida
//!   de NIST (FIPS 186-4, D.2.3) tras el producto escolar de 512 bits.
//! * Puntos en coordenadas jacobianas (a = −3); una sola inversión (Fermat) al
//!   volver a afines.
//! * Las operaciones módulo el orden `n` (pocas) usan el `BigUint` genérico de
//!   `crypto.rs`.
//!
//! Verificación (SEC 1 §4.1.4): w = s⁻¹ mod n; u1 = e·w; u2 = r·w;
//! R = u1·G + u2·Q; válida si R.x ≡ r (mod n).

use alloc::vec::Vec;
use super::crypto::BigUint;

/// Elemento de F_p, 8 palabras little-endian.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fe(pub [u32; 8]);

const P: Fe = Fe([0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0x00000000, 0x00000000, 0x00000000, 0x00000001, 0xFFFFFFFF]);
const N_BYTES: [u8; 32] = [0xFF,0xFF,0xFF,0xFF,0x00,0x00,0x00,0x00,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xBC,0xE6,0xFA,0xAD,0xA7,0x17,0x9E,0x84,0xF3,0xB9,0xCA,0xC2,0xFC,0x63,0x25,0x51];
const B: Fe = Fe([0x27D2604B, 0x3BCE3C3E, 0xCC53B0F6, 0x651D06B0, 0x769886BC, 0xB3EBBD55, 0xAA3A93E7, 0x5AC635D8]);
const GX: Fe = Fe([0xD898C296, 0xF4A13945, 0x2DEB33A0, 0x77037D81, 0x63A440F2, 0xF8BCE6E5, 0xE12C4247, 0x6B17D1F2]);
const GY: Fe = Fe([0x37BF51F5, 0xCBB64068, 0x6B315ECE, 0x2BCE3357, 0x7C0F9E16, 0x8EE7EB4A, 0xFE1A7F9B, 0x4FE342E2]);

impl Fe {
    pub const ZERO: Fe = Fe([0; 8]);
    pub const ONE: Fe = Fe([1, 0, 0, 0, 0, 0, 0, 0]);

    pub fn from_be_bytes(b: &[u8; 32]) -> Fe {
        let mut w = [0u32; 8];
        for i in 0..8 {
            let j = 28 - i * 4;
            w[i] = u32::from_be_bytes([b[j], b[j + 1], b[j + 2], b[j + 3]]);
        }
        Fe(w)
    }
    pub fn to_be_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..8 { out[28 - i * 4..32 - i * 4].copy_from_slice(&self.0[i].to_be_bytes()); }
        out
    }
    fn is_zero(&self) -> bool { self.0.iter().all(|&w| w == 0) }
    fn geq_p(&self) -> bool {
        for i in (0..8).rev() {
            if self.0[i] > P.0[i] { return true; }
            if self.0[i] < P.0[i] { return false; }
        }
        true
    }
    /// Suma con acarreo; devuelve (resultado sin reducir, acarreo).
    fn add_raw(a: &Fe, b: &Fe) -> (Fe, u32) {
        let mut r = [0u32; 8]; let mut c = 0u64;
        for i in 0..8 { let s = a.0[i] as u64 + b.0[i] as u64 + c; r[i] = s as u32; c = s >> 32; }
        (Fe(r), c as u32)
    }
    fn sub_raw(a: &Fe, b: &Fe) -> (Fe, u32) {
        let mut r = [0u32; 8]; let mut br = 0i64;
        for i in 0..8 { let d = a.0[i] as i64 - b.0[i] as i64 - br; if d < 0 { r[i] = (d + (1i64 << 32)) as u32; br = 1; } else { r[i] = d as u32; br = 0; } }
        (Fe(r), br as u32)
    }
    pub fn add(&self, o: &Fe) -> Fe {
        let (r, c) = Fe::add_raw(self, o);
        if c == 1 || r.geq_p() { Fe::sub_raw(&r, &P).0 } else { r }
    }
    pub fn sub(&self, o: &Fe) -> Fe {
        let (r, br) = Fe::sub_raw(self, o);
        if br == 1 { Fe::add_raw(&r, &P).0 } else { r }
    }
    pub fn neg(&self) -> Fe { if self.is_zero() { *self } else { Fe::sub_raw(&P, self).0 } }
    pub fn dbl(&self) -> Fe { self.add(self) }

    /// Producto y reducción rápida NIST P-256.
    pub fn mul(&self, o: &Fe) -> Fe {
        let mut c = [0u64; 16];
        for i in 0..8 {
            let mut carry = 0u64;
            for j in 0..8 {
                let t = c[i + j] + (self.0[i] as u64) * (o.0[j] as u64) + carry;
                c[i + j] = t & 0xFFFF_FFFF; carry = t >> 32;
            }
            c[i + 8] += carry;
        }
        let w = |k: usize| c[k] as i64;
        // Términos s1..s9 (FIPS 186-4 D.2.3), palabras de menor a mayor peso.
        let mut acc = [0i64; 8];
        let terms: [([usize; 8], i64); 9] = [
            ([0, 1, 2, 3, 4, 5, 6, 7], 1),                               // s1
            ([16, 16, 16, 11, 12, 13, 14, 15], 2),                       // s2 (16 = cero)
            ([16, 16, 16, 12, 13, 14, 15, 16], 2),                       // s3
            ([8, 9, 10, 16, 16, 16, 14, 15], 1),                         // s4
            ([9, 10, 11, 13, 14, 15, 13, 8], 1),                         // s5
            ([11, 12, 13, 16, 16, 16, 8, 10], -1),                       // s6
            ([12, 13, 14, 15, 16, 16, 9, 11], -1),                       // s7
            ([13, 14, 15, 8, 9, 10, 16, 12], -1),                        // s8
            ([14, 15, 16, 9, 10, 11, 16, 13], -1),                       // s9
        ];
        for (idx, sign) in terms.iter() {
            for k in 0..8 { if idx[k] < 16 { acc[k] += sign * w(idx[k]); } }
        }
        // Propagar acarreos (con signo) a un entero de 9 palabras.
        let mut carry: i64 = 0; let mut r = [0u32; 8];
        for k in 0..8 { let t = acc[k] + carry; r[k] = (t & 0xFFFF_FFFF) as u32; carry = t >> 32; }
        let mut out = Fe(r);
        // carry ∈ [-4, 5]: corregir sumando/restando p.
        while carry < 0 { let (s, c) = Fe::add_raw(&out, &P); out = s; carry += c as i64; }
        while carry > 0 { let (s, b) = Fe::sub_raw(&out, &P); out = s; carry -= b as i64; }
        while out.geq_p() { out = Fe::sub_raw(&out, &P).0; }
        out
    }
    pub fn sq(&self) -> Fe { self.mul(self) }
    /// Inverso por Fermat: a^(p-2).
    pub fn inv(&self) -> Fe {
        let e = Fe::sub_raw(&P, &Fe([2, 0, 0, 0, 0, 0, 0, 0])).0;
        let mut r = Fe::ONE; let mut base = *self;
        for i in 0..256 {
            if (e.0[i / 32] >> (i % 32)) & 1 == 1 { r = r.mul(&base); }
            base = base.sq();
        }
        r
    }
}

/// Punto jacobiano (X, Y, Z); Z = 0 ⇒ infinito.
#[derive(Clone, Copy)]
struct Jac { x: Fe, y: Fe, z: Fe }

impl Jac {
    const INF: Jac = Jac { x: Fe::ONE, y: Fe::ONE, z: Fe::ZERO };
    fn from_affine(x: Fe, y: Fe) -> Jac { Jac { x, y, z: Fe::ONE } }
    fn is_inf(&self) -> bool { self.z.is_zero() }
    /// Duplicación (a = −3), fórmula dbl-2001-b.
    fn double(&self) -> Jac {
        if self.is_inf() { return *self; }
        let zz = self.z.sq();
        let m = self.x.sub(&zz).mul(&self.x.add(&zz)); let m = m.add(&m.dbl()); // 3(x−z²)(x+z²)
        let yy = self.y.sq();
        let s = self.x.mul(&yy).dbl().dbl();                                   // 4·x·y²
        let x3 = m.sq().sub(&s.dbl());
        let yyyy8 = yy.sq().dbl().dbl().dbl();                                 // 8·y⁴
        let y3 = m.mul(&s.sub(&x3)).sub(&yyyy8);
        let z3 = self.y.mul(&self.z).dbl();
        Jac { x: x3, y: y3, z: z3 }
    }
    /// Suma general (add-1998-cmo-2).
    fn add(&self, o: &Jac) -> Jac {
        if self.is_inf() { return *o; }
        if o.is_inf() { return *self; }
        let z1z1 = self.z.sq(); let z2z2 = o.z.sq();
        let u1 = self.x.mul(&z2z2); let u2 = o.x.mul(&z1z1);
        let s1 = self.y.mul(&o.z).mul(&z2z2); let s2 = o.y.mul(&self.z).mul(&z1z1);
        let h = u2.sub(&u1); let r = s2.sub(&s1);
        if h.is_zero() { return if r.is_zero() { self.double() } else { Jac::INF }; }
        let hh = h.sq(); let hhh = hh.mul(&h); let v = u1.mul(&hh);
        let x3 = r.sq().sub(&hhh).sub(&v.dbl());
        let y3 = r.mul(&v.sub(&x3)).sub(&s1.mul(&hhh));
        let z3 = self.z.mul(&o.z).mul(&h);
        Jac { x: x3, y: y3, z: z3 }
    }
    fn mul_scalar(&self, k: &[u8; 32]) -> Jac {
        let mut r = Jac::INF;
        for byte in k.iter() {
            for bit in (0..8).rev() {
                r = r.double();
                if (byte >> bit) & 1 == 1 { r = r.add(self); }
            }
        }
        r
    }
    fn to_affine(&self) -> Option<(Fe, Fe)> {
        if self.is_inf() { return None; }
        let zi = self.z.inv(); let zi2 = zi.sq(); let zi3 = zi2.mul(&zi);
        Some((self.x.mul(&zi2), self.y.mul(&zi3)))
    }
}

/// Clave pública P-256 (punto afín).
#[derive(Clone)]
pub struct P256PublicKey { pub x: Fe, pub y: Fe }
impl P256PublicKey {
    /// Punto sin comprimir SEC1 (0x04 ‖ X ‖ Y).
    pub fn sec1_bytes(&self) -> alloc::vec::Vec<u8> { let mut v = alloc::vec![0x04u8]; v.extend_from_slice(&self.x.to_be_bytes()); v.extend_from_slice(&self.y.to_be_bytes()); v }
}

impl P256PublicKey {
    /// Punto SEC1 sin comprimir `04 || X || Y`; comprueba que esté en la curva.
    pub fn from_sec1(b: &[u8]) -> Option<Self> {
        if b.len() != 65 || b[0] != 0x04 { return None; }
        let mut xb = [0u8; 32]; xb.copy_from_slice(&b[1..33]);
        let mut yb = [0u8; 32]; yb.copy_from_slice(&b[33..65]);
        let x = Fe::from_be_bytes(&xb); let y = Fe::from_be_bytes(&yb);
        if x.geq_p() || y.geq_p() { return None; }
        // y² = x³ − 3x + b
        let lhs = y.sq();
        let rhs = x.sq().mul(&x).sub(&x.dbl().add(&x)).add(&B);
        if lhs != rhs { return None; }
        Some(P256PublicKey { x, y })
    }

    /// Verifica una firma ECDSA codificada en DER (`SEQUENCE { INTEGER r, INTEGER s }`)
    /// sobre el `digest` ya calculado (SHA-256 o SHA-384: se toman los 32 bytes
    /// más significativos, SEC 1 §4.1.4 paso 2).
    pub fn verify_der(&self, digest: &[u8], sig_der: &[u8]) -> bool {
        let (r, s) = match parse_der_sig(sig_der) { Some(v) => v, None => return false };
        let n = BigUint::from_be_bytes(&N_BYTES);
        let one = BigUint::from_be_bytes(&[1]);
        let rb = BigUint::from_be_bytes(&r); let sb = BigUint::from_be_bytes(&s);
        if rb.is_zero() || sb.is_zero() || !lt(&rb, &n) || !lt(&sb, &n) { return false; }
        let e = BigUint::from_be_bytes(&digest[..digest.len().min(32)]);
        // w = s^(n-2) mod n (Fermat; n primo)
        let n_minus_2 = BigUint::from_be_bytes(&sub_small(&N_BYTES, 2));
        let w = sb.modpow(&n_minus_2, &n);
        let u1 = mulmod(&e, &w, &n); let u2 = mulmod(&rb, &w, &n);
        let _ = one;
        let u1b = to32(&u1); let u2b = to32(&u2);
        let g = Jac::from_affine(GX, GY);
        let q = Jac::from_affine(self.x, self.y);
        let rp = g.mul_scalar(&u1b).add(&q.mul_scalar(&u2b));
        let (xr, _) = match rp.to_affine() { Some(p) => p, None => return false };
        let v = BigUint::from_be_bytes(&xr.to_be_bytes()).modpow(&one_big(), &n); // v = x mod n
        v == rb
    }
}

/// Clave privada P-256 (escalar `d`) para firmar `CertificateVerify` en el servidor.
#[derive(Clone)]
pub struct P256PrivateKey { d: [u8; 32], pub public: P256PublicKey }

impl P256PrivateKey {
    /// `d` big-endian de 32 bytes; la clave pública se deriva como d·G.
    pub fn from_scalar(d: &[u8; 32]) -> Option<Self> {
        let n = BigUint::from_be_bytes(&N_BYTES); let db = BigUint::from_be_bytes(d);
        if db.is_zero() || !lt(&db, &n) { return None; }
        let (x, y) = Jac::from_affine(GX, GY).mul_scalar(d).to_affine()?;
        Some(P256PrivateKey { d: *d, public: P256PublicKey { x, y } })
    }
    /// Clave efímera aleatoria (ECDHE).
    pub fn generate() -> Option<Self> {
        for _ in 0..16 {
            let mut d = [0u8; 32]; super::crypto::random_bytes(&mut d);
            if let Some(k) = Self::from_scalar(&d) { return Some(k); }
        }
        None
    }
    /// ECDH: coordenada X de d·Q (RFC 8446 §7.4.2), `None` si Q no está en la curva.
    pub fn ecdh(&self, peer_sec1: &[u8]) -> Option<[u8; 32]> {
        let q = P256PublicKey::from_sec1(peer_sec1)?;
        let (x, _) = Jac::from_affine(q.x, q.y).mul_scalar(&self.d).to_affine()?;
        Some(x.to_be_bytes())
    }
    /// Punto público SEC1 sin comprimir.
    pub fn public_sec1(&self) -> Vec<u8> {
        let mut v = alloc::vec![0x04u8]; v.extend_from_slice(&self.public.x.to_be_bytes()); v.extend_from_slice(&self.public.y.to_be_bytes()); v
    }
    /// Firma ECDSA determinista (RFC 6979, HMAC-SHA256) del `digest` SHA-256; salida DER.
    pub fn sign_der(&self, digest: &[u8; 32]) -> Vec<u8> {
        let n = BigUint::from_be_bytes(&N_BYTES);
        let e = BigUint::from_be_bytes(digest).modpow(&one_big(), &n);
        let h1 = to32(&e);
        // RFC 6979 §3.2
        let hmac = |k: &[u8], parts: &[&[u8]]| -> [u8; 32] { let mut m = Vec::new(); for p in parts { m.extend_from_slice(p); } super::crypto::hmac(k, &m) };
        let mut v = [1u8; 32]; let mut k = [0u8; 32];
        k = hmac(&k, &[&v, &[0u8], &self.d, &h1]); v = hmac(&k, &[&v]);
        k = hmac(&k, &[&v, &[1u8], &self.d, &h1]); v = hmac(&k, &[&v]);
        loop {
            v = hmac(&k, &[&v]);
            let kb = BigUint::from_be_bytes(&v);
            if !kb.is_zero() && lt(&kb, &n) {
                let kbytes = to32(&kb);
                if let Some((xr, _)) = Jac::from_affine(GX, GY).mul_scalar(&kbytes).to_affine() {
                    let r = BigUint::from_be_bytes(&xr.to_be_bytes()).modpow(&one_big(), &n);
                    if !r.is_zero() {
                        let n_minus_2 = BigUint::from_be_bytes(&sub_small(&N_BYTES, 2));
                        let kinv = kb.modpow(&n_minus_2, &n);
                        let rd = mulmod(&r, &BigUint::from_be_bytes(&self.d), &n);
                        let sum = BigUint::from_be_bytes(&add_bytes(&e.to_be_bytes(32), &rd.to_be_bytes(32))).modpow(&one_big(), &n);
                        let s = mulmod(&kinv, &sum, &n);
                        if !s.is_zero() { return der_sig_from_rs(&r.to_be_bytes(32), &s.to_be_bytes(32)); }
                    }
                }
            }
            k = hmac(&k, &[&v, &[0u8]]); v = hmac(&k, &[&v]);
        }
    }
}
fn add_bytes(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = alloc::vec![0u8; a.len().max(b.len()) + 1]; let mut c = 0u32;
    for i in 0..out.len() - 1 {
        let x = if i < a.len() { a[a.len() - 1 - i] as u32 } else { 0 }; let y = if i < b.len() { b[b.len() - 1 - i] as u32 } else { 0 };
        let t = x + y + c; let l = out.len(); out[l - 1 - i] = t as u8; c = t >> 8;
    }
    out[0] = c as u8; out
}

fn one_big() -> BigUint { BigUint::from_be_bytes(&[1]) }
fn lt(a: &BigUint, b: &BigUint) -> bool { a.bits() < b.bits() || (a.bits() == b.bits() && a.to_be_bytes(32) < b.to_be_bytes(32)) }
fn mulmod(a: &BigUint, b: &BigUint, m: &BigUint) -> BigUint {
    // (a·b) mod m usando modpow(…,1) para no exponer `mul`/`rem` privados.
    let prod = BigUint::from_be_bytes(&mul_bytes(&a.to_be_bytes(32), &b.to_be_bytes(32)));
    prod.modpow(&one_big(), m)
}
/// Producto escolar de dos enteros big-endian de 32 bytes → 64 bytes.
fn mul_bytes(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut acc = alloc::vec![0u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate().rev() {
        let mut carry = 0u32;
        for (j, &y) in b.iter().enumerate().rev() {
            let t = acc[i + j + 1] + (x as u32) * (y as u32) + carry;
            acc[i + j + 1] = t & 0xFF; carry = t >> 8;
        }
        acc[i] += carry;
    }
    // normalizar acarreos restantes
    let mut out = alloc::vec![0u8; acc.len()];
    let mut carry = 0u32;
    for k in (0..acc.len()).rev() { let t = acc[k] + carry; out[k] = (t & 0xFF) as u8; carry = t >> 8; }
    out
}
fn sub_small(a: &[u8; 32], v: u8) -> [u8; 32] {
    let mut r = *a; let mut borrow = v as i32;
    for i in (0..32).rev() { let d = r[i] as i32 - borrow; if d < 0 { r[i] = (d + 256) as u8; borrow = 1; } else { r[i] = d as u8; borrow = 0; if borrow == 0 { break; } } }
    r
}
fn to32(b: &BigUint) -> [u8; 32] { let v = b.to_be_bytes(32); let mut o = [0u8; 32]; o.copy_from_slice(&v[v.len() - 32..]); o }

/// `SEQUENCE { INTEGER r, INTEGER s }` → (r, s) de 32 bytes (sin ceros de signo).
fn parse_der_sig(d: &[u8]) -> Option<([u8; 32], [u8; 32])> {
    let (r, s) = parse_der_sig_n(d, 32)?;
    let mut ro = [0u8; 32]; ro.copy_from_slice(&r); let mut so = [0u8; 32]; so.copy_from_slice(&s);
    Some((ro, so))
}
/// Versión genérica: enteros de `len` bytes (32 para P-256, 48 para P-384).
pub fn parse_der_sig_n(d: &[u8], len: usize) -> Option<(Vec<u8>, Vec<u8>)> {
    if d.len() < 8 || d[0] != 0x30 { return None; }
    let (body, _) = der_len(&d[1..])?;
    let (r, rest) = der_int_n(body, len)?;
    let (s, _) = der_int_n(rest, len)?;
    Some((r, s))
}
fn der_int_n(d: &[u8], len: usize) -> Option<(Vec<u8>, &[u8])> {
    if d.is_empty() || d[0] != 0x02 { return None; }
    let (body, rest) = der_len(&d[1..])?;
    let mut b = body; while b.len() > 1 && b[0] == 0 { b = &b[1..]; }
    if b.len() > len { return None; }
    let mut out = alloc::vec![0u8; len]; out[len - b.len()..].copy_from_slice(b);
    Some((out, rest))
}
/// Codifica (r, s) como `SEQUENCE { INTEGER r, INTEGER s }` DER (con ceros de signo).
pub fn der_sig_from_rs(r: &[u8], s: &[u8]) -> Vec<u8> {
    fn int(v: &[u8]) -> Vec<u8> {
        let mut b = v; while b.len() > 1 && b[0] == 0 { b = &b[1..]; }
        let mut o = alloc::vec![0x02u8];
        let pad = (b[0] & 0x80) != 0;
        o.push((b.len() + pad as usize) as u8); if pad { o.push(0); } o.extend_from_slice(b); o
    }
    let mut body = int(r); body.extend_from_slice(&int(s));
    let mut out = alloc::vec![0x30u8];
    if body.len() < 128 { out.push(body.len() as u8); } else { out.push(0x81); out.push(body.len() as u8); }
    out.extend_from_slice(&body); out
}
pub fn mul_bytes_pub(a: &[u8], b: &[u8]) -> Vec<u8> { mul_bytes(a, b) }
fn der_len(d: &[u8]) -> Option<(&[u8], &[u8])> {
    let (len, off) = if d[0] < 0x80 { (d[0] as usize, 1) } else if d[0] == 0x81 { (*d.get(1)? as usize, 2) } else if d[0] == 0x82 { (((*d.get(1)? as usize) << 8) | *d.get(2)? as usize, 3) } else { return None; };
    if d.len() < off + len { return None; }
    Some((&d[off..off + len], &d[off + len..]))
}
fn der_int(d: &[u8]) -> Option<([u8; 32], &[u8])> {
    if d.is_empty() || d[0] != 0x02 { return None; }
    let (body, rest) = der_len(&d[1..])?;
    let mut b = body; while b.len() > 1 && b[0] == 0 { b = &b[1..]; }
    if b.len() > 32 { return None; }
    let mut out = [0u8; 32]; out[32 - b.len()..].copy_from_slice(b);
    Some((out, rest))
}

/// Autoprueba con el vector de ejemplo de NIST (P-256, SHA-256, FIPS 186-4 /
/// RFC 6979 A.2.5: clave de prueba, mensaje "sample").
pub fn self_test() -> bool {
    // RFC 6979 A.2.5: Ux, Uy; firma de "sample" con SHA-256.
    let ux = hex("60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6");
    let uy = hex("7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299");
    let r  = hex("EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716");
    let s  = hex("F7CB1C942D657C41D436C7A1B6E29F65F3E900DBB9AFF4064DC4AB2F843ACDA8");
    let mut pk = alloc::vec![0x04u8]; pk.extend_from_slice(&ux); pk.extend_from_slice(&uy);
    let key = match P256PublicKey::from_sec1(&pk) { Some(k) => k, None => return false };
    let digest = crate::tls::sha256(b"sample");
    let mut der = alloc::vec![0x30u8, 0x46, 0x02, 0x21, 0x00]; der.extend_from_slice(&r); der.extend_from_slice(&[0x02, 0x21, 0x00]); der.extend_from_slice(&s);
    let ok = key.verify_der(&digest, &der);
    let mut bad = der.clone(); let l = bad.len(); bad[l - 1] ^= 1;
    // Firma determinista RFC 6979 A.2.5 con la clave privada x: debe reproducir (r, s) EXACTOS.
    let x = hex("C9AFA9D845BA75166B5C215767B1D6934E50C3DB36E89B127B8A622B120F6721");
    let mut xd = [0u8; 32]; xd.copy_from_slice(&x);
    let sk = match P256PrivateKey::from_scalar(&xd) { Some(k) => k, None => return false };
    let firmado = sk.sign_der(&digest);
    let (fr, fs) = match parse_der_sig(&firmado) { Some(v) => v, None => return false };
    let det_ok = fr[..] == r[..] && fs[..] == s[..] && sk.public_sec1() == pk;
    ok && !key.verify_der(&digest, &bad) && det_ok
}
fn hex(s: &str) -> Vec<u8> {
    let b = s.as_bytes(); let mut v = Vec::with_capacity(b.len() / 2);
    for i in (0..b.len()).step_by(2) { let h = |c: u8| if c >= b'A' { c - b'A' + 10 } else if c >= b'a' { c - b'a' + 10 } else { c - b'0' }; v.push(h(b[i]) * 16 + h(b[i + 1])); }
    v
}
