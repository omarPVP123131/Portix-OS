// console/terminal/fmt.rs
// Helpers de bajo nivel para formatear y parsear datos en buffers fijos.
// Todas las funciones son `pub(crate)` — sin heap, sin std.
#![allow(dead_code)]

// ══ Escritura en buffer ═══════════════════════════════════════════════════════

pub(crate) fn append_str(buf: &mut [u8], pos: &mut usize, s: &[u8]) {
    let l = s.len().min(buf.len().saturating_sub(*pos));
    buf[*pos..*pos + l].copy_from_slice(&s[..l]);
    *pos += l;
}

pub(crate) fn append_u32(buf: &mut [u8], pos: &mut usize, mut n: u32) {
    if n == 0 { append_str(buf, pos, b"0"); return; }
    let mut tmp = [0u8; 10];
    let mut i = 0;
    while n > 0 { tmp[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    tmp[..i].reverse();
    append_str(buf, pos, &tmp[..i]);
}

pub(crate) fn append_hex8_byte(buf: &mut [u8], pos: &mut usize, v: u8) {
    const H: &[u8] = b"0123456789ABCDEF";
    append_str(buf, pos, &[H[(v >> 4) as usize], H[(v & 0xF) as usize]]);
}

pub(crate) fn append_hex16(buf: &mut [u8], pos: &mut usize, v: u16) {
    const H: &[u8] = b"0123456789ABCDEF";
    append_str(buf, pos, &[
        H[((v >> 12) & 0xF) as usize], H[((v >> 8) & 0xF) as usize],
        H[((v >> 4)  & 0xF) as usize], H[(v & 0xF) as usize],
    ]);
}

pub(crate) fn append_hex64_full(buf: &mut [u8], pos: &mut usize, mut v: u64) {
    const H: &[u8] = b"0123456789ABCDEF";
    let mut tmp = [0u8; 16];
    for i in (0..16).rev() { tmp[i] = H[(v & 0xF) as usize]; v >>= 4; }
    append_str(buf, pos, &tmp);
}

pub(crate) fn append_hex64_short(buf: &mut [u8], pos: &mut usize, mut v: u64) {
    const H: &[u8] = b"0123456789ABCDEF";
    let mut tmp = [0u8; 16];
    for i in (0..16).rev() { tmp[i] = H[(v & 0xF) as usize]; v >>= 4; }
    let start = tmp.iter().position(|&b| b != b'0').unwrap_or(7).min(7);
    append_str(buf, pos, &tmp[start..]);
}

pub(crate) fn append_mhz(buf: &mut [u8], pos: &mut usize, mhz: u32) {
    if mhz >= 1000 {
        let gi = mhz / 1000; let gf = (mhz % 1000) / 10;
        append_u32(buf, pos, gi); append_str(buf, pos, b".");
        if gf < 10 { append_str(buf, pos, b"0"); }
        append_u32(buf, pos, gf); append_str(buf, pos, b" GHz");
    } else {
        append_u32(buf, pos, mhz); append_str(buf, pos, b" MHz");
    }
}

pub(crate) fn append_mib(buf: &mut [u8], pos: &mut usize, mb: u64) {
    if mb == 0 { append_str(buf, pos, b"0 MB"); return; }
    if mb >= 1024 {
        append_u32(buf, pos, (mb / 1024) as u32); append_str(buf, pos, b".");
        append_u32(buf, pos, ((mb % 1024) * 10 / 1024) as u32); append_str(buf, pos, b" GB");
    } else {
        append_u32(buf, pos, mb as u32); append_str(buf, pos, b" MB");
    }
}

// ══ Parseo ════════════════════════════════════════════════════════════════════

pub(crate) fn parse_u64(s: &[u8]) -> Option<u64> {
    let s = trim(s); if s.is_empty() { return None; }
    let mut n = 0u64;
    for &b in s {
        if !b.is_ascii_digit() { return None; }
        n = n.wrapping_mul(10).wrapping_add((b - b'0') as u64);
    }
    Some(n)
}

pub(crate) fn parse_hex(s: &[u8]) -> Option<u64> {
    let s = trim(s);
    let s = if s.starts_with(b"0x") || s.starts_with(b"0X") { &s[2..] } else { s };
    parse_hex_raw(s)
}

pub(crate) fn parse_hex_raw(s: &[u8]) -> Option<u64> {
    if s.is_empty() { return None; }
    let mut n = 0u64;
    for &b in s {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        n = n.wrapping_shl(4).wrapping_add(d as u64);
    }
    Some(n)
}

pub(crate) fn trim(s: &[u8]) -> &[u8] {
    let s = match s.iter().position(|&b| b != b' ')  { Some(i) => &s[i..], None => &[] };
    match s.iter().rposition(|&b| b != b' ') { Some(i) => &s[..=i], None => s }
}

// ══ Evaluador aritmético simple (+, -, *, /) ══════════════════════════════════

pub(crate) fn simple_eval(expr: &[u8]) -> Option<i64> {
    let mut tokens = [(0i64, b'+'); 32]; let mut tcount = 0usize;
    let mut i = 0usize; let mut first = true;
    while i < expr.len() {
        while i < expr.len() && expr[i] == b' ' { i += 1; }
        if i >= expr.len() { break; }
        let neg = if expr[i] == b'-' && first { i += 1; true } else { false };
        let mut n: i64 = 0; let mut digits = 0;
        while i < expr.len() && expr[i].is_ascii_digit() {
            n = n * 10 + (expr[i] - b'0') as i64; i += 1; digits += 1;
        }
        if digits == 0 && !neg { return None; }
        if neg { n = -n; }
        while i < expr.len() && expr[i] == b' ' { i += 1; }
        let op = if i < expr.len() { let o = expr[i]; i += 1; o } else { b'+' };
        if tcount < 32 { tokens[tcount] = (n, op); tcount += 1; }
        first = false;
    }
    if tcount == 0 { return None; }
    let mut vals = [0i64; 32]; let mut ops = [b'+'; 32]; let mut vn = 0usize;
    let (mut acc, mut cur_op) = (tokens[0].0, tokens[0].1);
    for t in 1..tcount {
        let (num, next_op) = tokens[t];
        if cur_op == b'*'      { acc *= num; }
        else if cur_op == b'/' { if num == 0 { return None; } acc /= num; }
        else { vals[vn] = acc; ops[vn] = cur_op; vn += 1; acc = num; }
        cur_op = next_op;
    }
    vals[vn] = acc; vn += 1;
    let mut result = vals[0];
    for k in 1..vn {
        if ops[k - 1] == b'+' { result += vals[k]; }
        else if ops[k - 1] == b'-' { result -= vals[k]; }
    }
    Some(result)
}