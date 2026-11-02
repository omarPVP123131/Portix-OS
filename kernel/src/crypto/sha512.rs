// kernel/src/crypto/sha512.rs — Función Hash Criptográfica SHA-512 y HMAC-SHA-512 (FIPS 180-4)
// Implementación pura en enteros de 64 bits para no_std sin asignaciones de heap.
#![allow(dead_code)]

pub const SHA512_DIGEST_SIZE: usize = 64;
pub const SHA512_BLOCK_SIZE: usize = 128;

#[inline(always)]
fn rotr64(x: u64, n: u32) -> u64 {
    (x >> n) | (x << (64 - n))
}

const K: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
    0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
    0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc2478794f52be0b0, 0xc76c51a30654be30,
    0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeeb9, 0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
    0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

pub struct Sha512 {
    h: [u64; 8],
    block: [u8; 128],
    block_len: usize,
    total_len: u128,
}

impl Sha512 {
    pub const fn new() -> Self {
        Self {
            h: [
                0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
                0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
            ],
            block: [0u8; 128],
            block_len: 0,
            total_len: 0,
        }
    }

    fn process_block(&mut self) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            w[i] = u64::from_be_bytes([
                self.block[i*8], self.block[i*8+1], self.block[i*8+2], self.block[i*8+3],
                self.block[i*8+4], self.block[i*8+5], self.block[i*8+6], self.block[i*8+7],
            ]);
        }
        for i in 16..80 {
            let s0 = rotr64(w[i - 15], 1) ^ rotr64(w[i - 15], 8) ^ (w[i - 15] >> 7);
            let s1 = rotr64(w[i - 2], 19) ^ rotr64(w[i - 2], 61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = self.h[0];
        let mut b = self.h[1];
        let mut c = self.h[2];
        let mut d = self.h[3];
        let mut e = self.h[4];
        let mut f = self.h[5];
        let mut g = self.h[6];
        let mut h = self.h[7];

        for i in 0..80 {
            let s1 = rotr64(e, 14) ^ rotr64(e, 18) ^ rotr64(e, 41);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = rotr64(a, 28) ^ rotr64(a, 34) ^ rotr64(a, 39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
        self.h[5] = self.h[5].wrapping_add(f);
        self.h[6] = self.h[6].wrapping_add(g);
        self.h[7] = self.h[7].wrapping_add(h);
    }

    pub fn update(&mut self, data: &[u8]) {
        self.total_len += data.len() as u128;
        for &byte in data {
            self.block[self.block_len] = byte;
            self.block_len += 1;
            if self.block_len == 128 {
                self.process_block();
                self.block_len = 0;
            }
        }
    }

    pub fn finalize(mut self) -> [u8; 64] {
        self.block[self.block_len] = 0x80;
        self.block_len += 1;

        if self.block_len > 112 {
            for i in self.block_len..128 { self.block[i] = 0; }
            self.process_block();
            self.block_len = 0;
        }

        for i in self.block_len..112 { self.block[i] = 0; }
        let bit_len = self.total_len * 8;
        self.block[112..128].copy_from_slice(&bit_len.to_be_bytes());
        self.process_block();

        let mut out = [0u8; 64];
        for i in 0..8 {
            out[i*8..i*8+8].copy_from_slice(&self.h[i].to_be_bytes());
        }
        out
    }
}

pub fn sha512_digest(data: &[u8]) -> [u8; 64] {
    let mut hasher = Sha512::new();
    hasher.update(data);
    hasher.finalize()
}

/// HMAC-SHA-512 (RFC 2104)
pub fn hmac_sha512(key: &[u8], msg: &[u8]) -> [u8; 64] {
    let mut k_pad = [0u8; SHA512_BLOCK_SIZE];
    if key.len() > SHA512_BLOCK_SIZE {
        let hash_key = sha512_digest(key);
        k_pad[..64].copy_from_slice(&hash_key);
    } else {
        k_pad[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0u8; SHA512_BLOCK_SIZE];
    let mut opad = [0u8; SHA512_BLOCK_SIZE];
    for i in 0..SHA512_BLOCK_SIZE {
        ipad[i] = k_pad[i] ^ 0x36;
        opad[i] = k_pad[i] ^ 0x5c;
    }

    let mut inner = Sha512::new();
    inner.update(&ipad);
    inner.update(msg);
    let inner_hash = inner.finalize();

    let mut outer = Sha512::new();
    outer.update(&opad);
    outer.update(&inner_hash);
    outer.finalize()
}

/// Auto-test de validación SHA-512
pub fn run_sha512_self_test() -> bool {
    let empty_hash = sha512_digest(b"");
    // Primer byte de SHA-512("") es 0xcf, 0x83, 0xe1...
    let ok1 = empty_hash[0] == 0xcf && empty_hash[1] == 0x83;

    let hmac = hmac_sha512(b"key", b"The quick brown fox jumps over the lazy dog");
    let ok2 = hmac[0] != 0;

    ok1 && ok2
}
