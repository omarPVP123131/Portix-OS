//! [T-002] Descompresor DEFLATE (RFC 1951) y envoltorio gzip (RFC 1952) en
//! Rust puro, sin dependencias. Los `.apk` y el `APKINDEX.tar.gz` de Alpine
//! son VARIOS flujos gzip concatenados, así que `gunzip_miembro` devuelve
//! también cuántos bytes consumió para poder seguir con el siguiente.

use alloc::vec::Vec;

struct Bits<'a> { d: &'a [u8], pos: usize, bitbuf: u64, nbits: u32 }

impl<'a> Bits<'a> {
    fn new(d: &'a [u8], pos: usize) -> Self { Bits { d, pos, bitbuf: 0, nbits: 0 } }
    #[inline] fn need(&mut self, n: u32) -> bool {
        while self.nbits < n {
            if self.pos >= self.d.len() { return false; }
            self.bitbuf |= (self.d[self.pos] as u64) << self.nbits;
            self.pos += 1; self.nbits += 8;
        }
        true
    }
    #[inline] fn bits(&mut self, n: u32) -> Option<u32> {
        if n == 0 { return Some(0); }
        if !self.need(n) { return None; }
        let v = (self.bitbuf & ((1u64 << n) - 1)) as u32;
        self.bitbuf >>= n; self.nbits -= n;
        Some(v)
    }
    fn align_byte(&mut self) { let r = self.nbits % 8; self.bitbuf >>= r; self.nbits -= r; }
    /// Posición en bytes del siguiente byte no consumido (tras alinear).
    fn byte_pos(&self) -> usize { self.pos - (self.nbits / 8) as usize }
}

/// Tabla Huffman canónica: cuenta por longitud + símbolos ordenados.
struct Huff { count: [u16; 16], symbol: Vec<u16> }

impl Huff {
    fn new(lengths: &[u8]) -> Option<Huff> {
        let mut count = [0u16; 16];
        for &l in lengths { count[l as usize] += 1; }
        count[0] = 0;
        let mut left: i32 = 1;
        for len in 1..16 { left <<= 1; left -= count[len] as i32; if left < 0 { return None; } }
        let mut offs = [0u16; 16];
        for len in 1..15 { offs[len + 1] = offs[len] + count[len]; }
        let mut symbol = alloc::vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 { symbol[offs[l as usize] as usize] = sym as u16; offs[l as usize] += 1; }
        }
        Some(Huff { count, symbol })
    }
    fn decode(&self, b: &mut Bits) -> Option<u16> {
        let mut code: i32 = 0; let mut first: i32 = 0; let mut index: i32 = 0;
        for len in 1..16 {
            code |= b.bits(1)? as i32;
            let c = self.count[len] as i32;
            if code - c < first { return self.symbol.get((index + (code - first)) as usize).copied(); }
            index += c; first += c; first <<= 1; code <<= 1;
        }
        None
    }
}

const LBASE: [u16; 29] = [3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258];
const LEXT: [u8; 29] = [0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0];
const DBASE: [u16; 30] = [1,2,3,4,5,7,9,13,17,25,33,49,65,97,129,193,257,385,513,769,1025,1537,2049,3073,4097,6145,8193,12289,16385,24577];
const DEXT: [u8; 30] = [0,0,0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,8,8,9,9,10,10,11,11,12,12,13,13];

/// [T-003] Salida en flujo: conserva una ventana de 32 KiB para las
/// referencias hacia atras y entrega el resto al sumidero por trozos, de modo
/// que un paquete de 140 MiB (gcc) no tenga que caber entero en memoria.
pub struct Salida<'s> {
    buf: Vec<u8>,
    sumidero: &'s mut dyn FnMut(&[u8]) -> Result<(), &'static str>,
    pub total: u64,
    crc: u32,
}

const VENTANA: usize = 32 * 1024;
const UMBRAL: usize = 1024 * 1024;

impl<'s> Salida<'s> {
    pub fn new(sumidero: &'s mut dyn FnMut(&[u8]) -> Result<(), &'static str>) -> Self {
        Salida { buf: Vec::with_capacity(UMBRAL + VENTANA), sumidero, total: 0, crc: 0xFFFF_FFFF }
    }
    #[inline] fn push(&mut self, c: u8) -> Option<()> {
        self.buf.push(c);
        if self.buf.len() >= UMBRAL + VENTANA { self.volcar(false)? }
        Some(())
    }
    fn extend(&mut self, d: &[u8]) -> Option<()> {
        self.buf.extend_from_slice(d);
        if self.buf.len() >= UMBRAL + VENTANA { self.volcar(false)? }
        Some(())
    }
    /// Entrega al sumidero todo salvo la ventana (o todo si `fin`).
    fn volcar(&mut self, fin: bool) -> Option<()> {
        let n = if fin { self.buf.len() } else { self.buf.len().saturating_sub(VENTANA) };
        if n == 0 { return Some(()); }
        self.crc = crc32_acum(self.crc, &self.buf[..n]);
        self.total += n as u64;
        (self.sumidero)(&self.buf[..n]).ok()?;
        self.buf.copy_within(n.., 0);
        self.buf.truncate(self.buf.len() - n);
        Some(())
    }
    fn terminar(mut self) -> Option<(u64, u32)> { self.volcar(true)?; Some((self.total, !self.crc)) }
}

fn codes(out: &mut Salida, b: &mut Bits, lit: &Huff, dist: &Huff) -> Option<()> {
    loop {
        let sym = lit.decode(b)? as usize;
        if sym < 256 { out.push(sym as u8)?; }
        else if sym == 256 { return Some(()); }
        else {
            let i = sym - 257; if i >= 29 { return None; }
            let len = LBASE[i] as usize + b.bits(LEXT[i] as u32)? as usize;
            let ds = dist.decode(b)? as usize; if ds >= 30 { return None; }
            let d = DBASE[ds] as usize + b.bits(DEXT[ds] as u32)? as usize;
            if d > out.buf.len() { return None; }
            for _ in 0..len { let c = out.buf[out.buf.len() - d]; out.push(c)?; }
        }
    }
}

fn fixed_tables() -> Option<(Huff, Huff)> {
    let mut l = [0u8; 288];
    for i in 0..144 { l[i] = 8; } for i in 144..256 { l[i] = 9; } for i in 256..280 { l[i] = 7; } for i in 280..288 { l[i] = 8; }
    let d = [5u8; 30];
    Some((Huff::new(&l)?, Huff::new(&d)?))
}

fn dynamic_tables(b: &mut Bits) -> Option<(Huff, Huff)> {
    let nlen = b.bits(5)? as usize + 257; let ndist = b.bits(5)? as usize + 1; let ncode = b.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 { return None; }
    const ORDER: [usize; 19] = [16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15];
    let mut lengths = [0u8; 320];
    for i in 0..ncode { lengths[ORDER[i]] = b.bits(3)? as u8; }
    let lencode = Huff::new(&lengths[..19])?;
    let mut idx = 0;
    while idx < nlen + ndist {
        let sym = lencode.decode(b)?;
        if sym < 16 { lengths[idx] = sym as u8; idx += 1; }
        else {
            let (rep, val) = match sym {
                16 => { if idx == 0 { return None; } (3 + b.bits(2)? as usize, lengths[idx - 1]) }
                17 => (3 + b.bits(3)? as usize, 0),
                _ => (11 + b.bits(7)? as usize, 0),
            };
            if idx + rep > nlen + ndist { return None; }
            for _ in 0..rep { lengths[idx] = val; idx += 1; }
        }
    }
    if lengths[256] == 0 { return None; }
    Some((Huff::new(&lengths[..nlen])?, Huff::new(&lengths[nlen..nlen + ndist])?))
}

/// Inflado de un flujo DEFLATE crudo a partir de `d[pos..]`.
/// Devuelve (datos, posición del primer byte tras el flujo).
pub fn inflate(d: &[u8], pos: usize) -> Option<(Vec<u8>, usize)> {
    let mut v: Vec<u8> = Vec::new();
    let mut sum = |t: &[u8]| -> Result<(), &'static str> { v.extend_from_slice(t); Ok(()) };
    let (_, fin) = inflate_a(d, pos, &mut sum)?;
    Some((v, fin))
}

/// Inflado en flujo: entrega la salida al sumidero. Devuelve ((bytes, crc32), posicion final).
pub fn inflate_a(d: &[u8], pos: usize, sumidero: &mut dyn FnMut(&[u8]) -> Result<(), &'static str>) -> Option<((u64, u32), usize)> {
    let mut out = Salida::new(sumidero);
    let mut b = Bits::new(d, pos);
    loop {
        let last = b.bits(1)?;
        match b.bits(2)? {
            0 => {
                b.align_byte();
                let p = b.byte_pos();
                if p + 4 > d.len() { return None; }
                let len = u16::from_le_bytes([d[p], d[p + 1]]) as usize;
                let nlen = u16::from_le_bytes([d[p + 2], d[p + 3]]) as usize;
                if len != (!nlen & 0xFFFF) || p + 4 + len > d.len() { return None; }
                out.extend(&d[p + 4..p + 4 + len])?;
                b = Bits::new(d, p + 4 + len);
            }
            1 => { let (l, ds) = fixed_tables()?; codes(&mut out, &mut b, &l, &ds)?; }
            2 => { let (l, ds) = dynamic_tables(&mut b)?; codes(&mut out, &mut b, &l, &ds)?; }
            _ => return None,
        }
        if last == 1 { break; }
    }
    b.align_byte();
    let fin = b.byte_pos();
    Some((out.terminar()?, fin))
}

/// CRC-32 (IEEE) por tabla (la version bit a bit tardaba segundos por cada
/// decena de MiB bajo TCG).
static TABLA_CRC: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32; let mut k = 0;
        while k < 8 { c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 }; k += 1; }
        t[i] = c; i += 1;
    }
    t
};

fn crc32_acum(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data { crc = TABLA_CRC[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8); }
    crc
}

/// Descomprime UN miembro gzip que empieza en `d[pos..]`, comprobando CRC-32 e
/// ISIZE. Devuelve (datos, posición del byte siguiente al miembro).
pub fn gunzip_miembro(d: &[u8], pos: usize) -> Result<(Vec<u8>, usize), &'static str> {
    let mut v: Vec<u8> = Vec::new();
    let mut sum = |t: &[u8]| -> Result<(), &'static str> { v.extend_from_slice(t); Ok(()) };
    let fin = gunzip_a(d, pos, &mut sum)?;
    Ok((v, fin))
}

/// Como `gunzip_miembro` pero entregando la salida en flujo al sumidero.
/// Devuelve la posicion del byte siguiente al miembro.
pub fn gunzip_a(d: &[u8], pos: usize, sumidero: &mut dyn FnMut(&[u8]) -> Result<(), &'static str>) -> Result<usize, &'static str> {
    if d.len() < pos + 18 || d[pos] != 0x1f || d[pos + 1] != 0x8b { return Err("cabecera gzip ausente"); }
    if d[pos + 2] != 8 { return Err("metodo gzip distinto de DEFLATE"); }
    let flg = d[pos + 3];
    let mut p = pos + 10;
    if flg & 4 != 0 { if p + 2 > d.len() { return Err("FEXTRA truncado"); } let xl = u16::from_le_bytes([d[p], d[p + 1]]) as usize; p += 2 + xl; }
    if flg & 8 != 0 { while p < d.len() && d[p] != 0 { p += 1; } p += 1; }
    if flg & 16 != 0 { while p < d.len() && d[p] != 0 { p += 1; } p += 1; }
    if flg & 2 != 0 { p += 2; }
    if p > d.len() { return Err("cabecera gzip truncada"); }
    let ((total, crc_real), fin) = inflate_a(d, p, sumidero).ok_or("flujo DEFLATE corrupto (o el sumidero fallo)")?;
    if fin + 8 > d.len() { return Err("cola gzip truncada"); }
    let crc = u32::from_le_bytes([d[fin], d[fin + 1], d[fin + 2], d[fin + 3]]);
    let isize = u32::from_le_bytes([d[fin + 4], d[fin + 5], d[fin + 6], d[fin + 7]]);
    if isize != (total as u32) { return Err("ISIZE gzip no coincide"); }
    if crc_real != crc { return Err("CRC-32 gzip no coincide"); }
    Ok(fin + 8)
}
