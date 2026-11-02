//! Curvas primas de Weierstrass genéricas (a = −3) con aritmética de
//! Montgomery sobre `N` palabras de 32 bits — usada para **P-384**
//! (`ecdsa_secp384r1_sha384`). P-256 conserva su reducción NIST dedicada en
//! `p256.rs`; esta implementación es la vía general para cualquier módulo.
//!
//! Sin crates: multiplicación CIOS, inversión por Fermat, puntos jacobianos.

use alloc::vec::Vec;
use super::crypto::BigUint;

/// Parámetros de una curva y² = x³ − 3x + b sobre F_p (big-endian hex → palabras LE).
pub struct CurveParams<const N: usize> {
    pub p: [u32; N],
    pub b: [u32; N],
    pub gx: [u32; N],
    pub gy: [u32; N],
    pub n_be: &'static [u8],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Fp<const N: usize>([u32; N]);

pub struct Field<const N: usize> {
    p: [u32; N],
    r2: [u32; N],   // R² mod p (R = 2^(32N))
    n0: u32,        // −p⁻¹ mod 2^32
    one: Fp<N>,     // R mod p (1 en Montgomery)
}

impl<const N: usize> Field<N> {
    pub fn new(p: [u32; N]) -> Self {
        // n0 = −p⁻¹ mod 2^32 (Newton)
        let mut inv: u32 = 1;
        for _ in 0..5 { inv = inv.wrapping_mul(2u32.wrapping_sub(p[0].wrapping_mul(inv))); }
        let n0 = inv.wrapping_neg();
        // R mod p: partir de 1 y duplicar 32N veces módulo p
        let mut r = [0u32; N]; r[0] = 1;
        for _ in 0..(32 * N) { r = Self::dbl_mod(&r, &p); }
        let one = Fp(r);
        // R² mod p: duplicar otras 32N veces
        let mut r2 = r;
        for _ in 0..(32 * N) { r2 = Self::dbl_mod(&r2, &p); }
        Field { p, r2, n0, one }
    }
    fn geq(a: &[u32; N], b: &[u32; N]) -> bool {
        for i in (0..N).rev() { if a[i] > b[i] { return true; } if a[i] < b[i] { return false; } }
        true
    }
    fn sub_raw(a: &[u32; N], b: &[u32; N]) -> ([u32; N], u32) {
        let mut r = [0u32; N]; let mut br = 0u64;
        for i in 0..N { let d = (a[i] as u64).wrapping_sub(b[i] as u64).wrapping_sub(br); r[i] = d as u32; br = (d >> 63) & 1; }
        (r, br as u32)
    }
    fn add_raw(a: &[u32; N], b: &[u32; N]) -> ([u32; N], u32) {
        let mut r = [0u32; N]; let mut c = 0u64;
        for i in 0..N { let s = a[i] as u64 + b[i] as u64 + c; r[i] = s as u32; c = s >> 32; }
        (r, c as u32)
    }
    fn dbl_mod(a: &[u32; N], p: &[u32; N]) -> [u32; N] {
        let (s, c) = Self::add_raw(a, a);
        if c == 1 || Self::geq(&s, p) { Self::sub_raw(&s, p).0 } else { s }
    }
    pub fn add(&self, a: &Fp<N>, b: &Fp<N>) -> Fp<N> {
        let (s, c) = Self::add_raw(&a.0, &b.0);
        if c == 1 || Self::geq(&s, &self.p) { Fp(Self::sub_raw(&s, &self.p).0) } else { Fp(s) }
    }
    pub fn sub(&self, a: &Fp<N>, b: &Fp<N>) -> Fp<N> {
        let (d, br) = Self::sub_raw(&a.0, &b.0);
        if br == 1 { Fp(Self::add_raw(&d, &self.p).0) } else { Fp(d) }
    }
    /// Multiplicación de Montgomery (CIOS): a·b·R⁻¹ mod p.
    pub fn mul(&self, a: &Fp<N>, b: &Fp<N>) -> Fp<N> {
        let mut t = [0u64; 32]; // N ≤ 30
        for i in 0..N {
            let mut c = 0u64;
            for j in 0..N { let s = t[j] + (a.0[i] as u64) * (b.0[j] as u64) + c; t[j] = s & 0xFFFF_FFFF; c = s >> 32; }
            let s = t[N] + c; t[N] = s & 0xFFFF_FFFF; t[N + 1] = s >> 32;
            let m = ((t[0] as u32).wrapping_mul(self.n0)) as u64;
            let s = t[0] + m * (self.p[0] as u64); let mut c = s >> 32;
            for j in 1..N { let s = t[j] + m * (self.p[j] as u64) + c; t[j - 1] = s & 0xFFFF_FFFF; c = s >> 32; }
            let s = t[N] + c; t[N - 1] = s & 0xFFFF_FFFF; t[N] = t[N + 1] + (s >> 32); t[N + 1] = 0;
        }
        let mut r = [0u32; N]; for i in 0..N { r[i] = t[i] as u32; }
        if t[N] != 0 || Self::geq(&r, &self.p) { r = Self::sub_raw(&r, &self.p).0; }
        Fp(r)
    }
    pub fn sq(&self, a: &Fp<N>) -> Fp<N> { self.mul(a, a) }
    pub fn to_mont(&self, a: &[u32; N]) -> Fp<N> { self.mul(&Fp(*a), &Fp(self.r2)) }
    pub fn from_mont(&self, a: &Fp<N>) -> [u32; N] { let mut one = [0u32; N]; one[0] = 1; self.mul(a, &Fp(one)).0 }
    pub fn one(&self) -> Fp<N> { self.one }
    pub fn zero(&self) -> Fp<N> { Fp([0u32; N]) }
    pub fn is_zero(&self, a: &Fp<N>) -> bool { a.0.iter().all(|&w| w == 0) }
    /// a^(p−2) (Fermat).
    pub fn inv(&self, a: &Fp<N>) -> Fp<N> {
        let mut two = [0u32; N]; two[0] = 2;
        let e = Self::sub_raw(&self.p, &two).0;
        let mut r = self.one; let mut base = *a;
        for i in 0..(32 * N) {
            if (e[i / 32] >> (i % 32)) & 1 == 1 { r = self.mul(&r, &base); }
            base = self.sq(&base);
        }
        r
    }
    pub fn in_range(&self, a: &[u32; N]) -> bool { !Self::geq(a, &self.p) }
}

pub fn words_from_be<const N: usize>(b: &[u8]) -> Option<[u32; N]> {
    if b.len() != 4 * N { return None; }
    let mut w = [0u32; N];
    for i in 0..N { let j = b.len() - 4 - 4 * i; w[i] = u32::from_be_bytes([b[j], b[j + 1], b[j + 2], b[j + 3]]); }
    Some(w)
}
pub fn words_to_be<const N: usize>(w: &[u32; N]) -> Vec<u8> {
    let mut out = alloc::vec![0u8; 4 * N];
    for i in 0..N { let j = 4 * N - 4 - 4 * i; out[j..j + 4].copy_from_slice(&w[i].to_be_bytes()); }
    out
}

#[derive(Clone, Copy)]
struct Jac<const N: usize> { x: Fp<N>, y: Fp<N>, z: Fp<N> }

pub struct Curve<const N: usize> { f: Field<N>, b: Fp<N>, gx: Fp<N>, gy: Fp<N>, n_be: &'static [u8] }

impl<const N: usize> Curve<N> {
    pub fn new(p: &CurveParams<N>) -> Self {
        let f = Field::new(p.p);
        let b = f.to_mont(&p.b); let gx = f.to_mont(&p.gx); let gy = f.to_mont(&p.gy);
        Curve { f, b, gx, gy, n_be: p.n_be }
    }
    fn inf(&self) -> Jac<N> { Jac { x: self.f.one(), y: self.f.one(), z: self.f.zero() } }
    fn double(&self, q: &Jac<N>) -> Jac<N> {
        let f = &self.f;
        if f.is_zero(&q.z) { return *q; }
        let zz = f.sq(&q.z);
        let m = f.mul(&f.sub(&q.x, &zz), &f.add(&q.x, &zz)); let m = f.add(&m, &f.add(&m, &m));
        let yy = f.sq(&q.y);
        let s = f.mul(&q.x, &yy); let s = f.add(&s, &s); let s = f.add(&s, &s);
        let x3 = f.sub(&f.sq(&m), &f.add(&s, &s));
        let y4 = f.sq(&yy); let y4 = f.add(&y4, &y4); let y4 = f.add(&y4, &y4); let y4 = f.add(&y4, &y4);
        let y3 = f.sub(&f.mul(&m, &f.sub(&s, &x3)), &y4);
        let z3 = f.mul(&q.y, &q.z); let z3 = f.add(&z3, &z3);
        Jac { x: x3, y: y3, z: z3 }
    }
    fn add(&self, a: &Jac<N>, b: &Jac<N>) -> Jac<N> {
        let f = &self.f;
        if f.is_zero(&a.z) { return *b; }
        if f.is_zero(&b.z) { return *a; }
        let z1z1 = f.sq(&a.z); let z2z2 = f.sq(&b.z);
        let u1 = f.mul(&a.x, &z2z2); let u2 = f.mul(&b.x, &z1z1);
        let s1 = f.mul(&f.mul(&a.y, &b.z), &z2z2); let s2 = f.mul(&f.mul(&b.y, &a.z), &z1z1);
        let h = f.sub(&u2, &u1); let r = f.sub(&s2, &s1);
        if f.is_zero(&h) { return if f.is_zero(&r) { self.double(a) } else { self.inf() }; }
        let hh = f.sq(&h); let hhh = f.mul(&hh, &h); let v = f.mul(&u1, &hh);
        let x3 = f.sub(&f.sub(&f.sq(&r), &hhh), &f.add(&v, &v));
        let y3 = f.sub(&f.mul(&r, &f.sub(&v, &x3)), &f.mul(&s1, &hhh));
        let z3 = f.mul(&f.mul(&a.z, &b.z), &h);
        Jac { x: x3, y: y3, z: z3 }
    }
    fn mul_scalar(&self, p: &Jac<N>, k: &[u8]) -> Jac<N> {
        let mut r = self.inf();
        for byte in k { for bit in (0..8).rev() { r = self.double(&r); if (byte >> bit) & 1 == 1 { r = self.add(&r, p); } } }
        r
    }
    fn affine_x(&self, p: &Jac<N>) -> Option<[u32; N]> {
        if self.f.is_zero(&p.z) { return None; }
        let zi = self.f.inv(&p.z); let zi2 = self.f.sq(&zi);
        Some(self.f.from_mont(&self.f.mul(&p.x, &zi2)))
    }
    /// Punto SEC1 sin comprimir → (x, y) en Montgomery, comprobando la curva.
    pub fn decode_point(&self, b: &[u8]) -> Option<(Fp<N>, Fp<N>)> {
        if b.len() != 1 + 8 * N || b[0] != 0x04 { return None; }
        let xw = words_from_be::<N>(&b[1..1 + 4 * N])?; let yw = words_from_be::<N>(&b[1 + 4 * N..])?;
        if !self.f.in_range(&xw) || !self.f.in_range(&yw) { return None; }
        let f = &self.f;
        let x = f.to_mont(&xw); let y = f.to_mont(&yw);
        let lhs = f.sq(&y);
        let x3 = f.mul(&f.sq(&x), &x);
        let three_x = f.add(&f.add(&x, &x), &x);
        let rhs = f.add(&f.sub(&x3, &three_x), &self.b);
        if lhs != rhs { return None; }
        Some((x, y))
    }
    /// Verificación ECDSA (SEC 1 §4.1.4) con firma DER y digest truncado a 4N bytes.
    pub fn verify(&self, qx: &Fp<N>, qy: &Fp<N>, digest: &[u8], sig_der: &[u8]) -> bool {
        let (r, s) = match super::p256::parse_der_sig_n(sig_der, 4 * N) { Some(v) => v, None => return false };
        let n = BigUint::from_be_bytes(self.n_be);
        let one = BigUint::from_be_bytes(&[1]);
        let rb = BigUint::from_be_bytes(&r); let sb = BigUint::from_be_bytes(&s);
        if rb.is_zero() || sb.is_zero() || !lt(&rb, &n) || !lt(&sb, &n) { return false; }
        let e = BigUint::from_be_bytes(&digest[..digest.len().min(4 * N)]);
        let n2 = { let mut v = self.n_be.to_vec(); let mut br = 2i32; for i in (0..v.len()).rev() { let d = v[i] as i32 - br; if d < 0 { v[i] = (d + 256) as u8; br = 1; } else { v[i] = d as u8; break; } } v };
        let w = sb.modpow(&BigUint::from_be_bytes(&n2), &n);
        let u1 = mulmod(&e, &w, &n, 4 * N); let u2 = mulmod(&rb, &w, &n, 4 * N);
        let g = Jac { x: self.gx, y: self.gy, z: self.f.one() };
        let q = Jac { x: *qx, y: *qy, z: self.f.one() };
        let rp = self.add(&self.mul_scalar(&g, &u1.to_be_bytes(4 * N)), &self.mul_scalar(&q, &u2.to_be_bytes(4 * N)));
        let xr = match self.affine_x(&rp) { Some(x) => x, None => return false };
        let v = BigUint::from_be_bytes(&words_to_be(&xr)).modpow(&one, &n);
        v == rb
    }
}

fn lt(a: &BigUint, b: &BigUint) -> bool { a.bits() < b.bits() || (a.bits() == b.bits() && a.to_be_bytes(64) < b.to_be_bytes(64)) }
fn mulmod(a: &BigUint, b: &BigUint, m: &BigUint, len: usize) -> BigUint {
    let prod = super::p256::mul_bytes_pub(&a.to_be_bytes(len), &b.to_be_bytes(len));
    BigUint::from_be_bytes(&prod).modpow(&BigUint::from_be_bytes(&[1]), m)
}

// ── P-384 (secp384r1) ─────────────────────────────────────────────────────
pub const P384_N_BE: [u8; 48] = [
    0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,
    0xc7,0x63,0x4d,0x81,0xf4,0x37,0x2d,0xdf,0x58,0x1a,0x0d,0xb2,0x48,0xb0,0xa7,0x7a,0xec,0xec,0x19,0x6a,0xcc,0xc5,0x29,0x73];

fn w12(hex: &str) -> [u32; 12] { let b = hexb(hex); words_from_be::<12>(&b).unwrap_or([0; 12]) }
fn hexb(s: &str) -> Vec<u8> {
    let b = s.as_bytes(); let mut v = Vec::with_capacity(b.len() / 2);
    let h = |c: u8| if c >= b'a' { c - b'a' + 10 } else if c >= b'A' { c - b'A' + 10 } else { c - b'0' };
    for i in (0..b.len()).step_by(2) { v.push(h(b[i]) * 16 + h(b[i + 1])); }
    v
}

pub fn p384() -> Curve<12> {
    let params = CurveParams::<12> {
        p:  w12("fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffeffffffff0000000000000000ffffffff"),
        b:  w12("b3312fa7e23ee7e4988e056be3f82d19181d9c6efe8141120314088f5013875ac656398d8a2ed19d2a85c8edd3ec2aef"),
        gx: w12("aa87ca22be8b05378eb1c71ef320ad746e1d3b628ba79b9859f741e082542a385502f25dbf55296c3a545e3872760ab7"),
        gy: w12("3617de4a96262c6f5d9e98bf9292dc29f8f41dbd289a147ce9da3113b5f0b8c00a60b1ce1d7e819d7a431d7c90ea0e5f"),
        n_be: &P384_N_BE,
    };
    Curve::new(&params)
}

/// Clave pública P-384 (coordenadas en Montgomery).
#[derive(Clone)]
pub struct P384PublicKey { x: Fp<12>, y: Fp<12> }

impl P384PublicKey {
    pub fn from_sec1(b: &[u8]) -> Option<Self> {
        let c = p384(); let (x, y) = c.decode_point(b)?; Some(P384PublicKey { x, y })
    }
    pub fn verify_der(&self, digest: &[u8], sig_der: &[u8]) -> bool {
        p384().verify(&self.x, &self.y, digest, sig_der)
    }
}

/// Autoprueba: RFC 6979 A.2.6 (P-384, SHA-384, mensaje "sample").
pub fn self_test_p384() -> bool {
    let ux = hexb("EC3A4E415B4E19A4568618029F427FA5DA9A8BC4AE92E02E06AAE5286B300C64DEF8F0EA9055866064A254515480BC13");
    let uy = hexb("8015D9B72D7D57244EA8EF9AC0C621896708A59367F9DFB9F54CA84B3F1C9DB1288B231C3AE0D4FE7344FD2533264720");
    let r = hexb("94EDBB92A5ECB8AAD4736E56C691916B3F88140666CE9FA73D64C4EA95AD133C81A648152E44ACF96E36DD1E80FABE46");
    let s = hexb("99EF4AEB15F178CEA1FE40DB2603138F130E740A19624526203B6351D0A3A94FA329C145786E679E7B82C71A38628AC8");
    let mut pk = alloc::vec![0x04u8]; pk.extend_from_slice(&ux); pk.extend_from_slice(&uy);
    let key = match P384PublicKey::from_sec1(&pk) { Some(k) => k, None => return false };
    let digest = super::crypto::sha384(b"sample");
    let der = super::p256::der_sig_from_rs(&r, &s);
    let ok = key.verify_der(&digest, &der);
    let mut bad = der.clone(); let l = bad.len(); bad[l - 1] ^= 1;
    ok && !key.verify_der(&digest, &bad)
}
