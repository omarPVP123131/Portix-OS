// util/fmt.rs — Formateo numérico sin heap para entornos bare-metal

pub fn fmt_u32<'a>(mut n: u32, buf: &'a mut [u8; 16]) -> &'a str {
    if n == 0 { buf[0] = b'0'; return core::str::from_utf8(&buf[..1]).unwrap_or("0"); }
    let mut i = 0usize;
    while n > 0 && i < 16 { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    buf[..i].reverse();
    core::str::from_utf8(&buf[..i]).unwrap_or("?")
}

pub fn fmt_u64<'a>(mut n: u64, buf: &'a mut [u8; 20]) -> &'a str {
    if n == 0 { buf[0] = b'0'; return core::str::from_utf8(&buf[..1]).unwrap_or("0"); }
    let mut i = 0usize;
    while n > 0 && i < 20 { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    buf[..i].reverse();
    core::str::from_utf8(&buf[..i]).unwrap_or("?")
}

pub fn fmt_u16<'a>(n: u16, buf: &'a mut [u8; 16]) -> &'a str {
    fmt_u32(n as u32, buf)
}

pub fn fmt_hex<'a>(mut v: u64, buf: &'a mut [u8; 18]) -> &'a str {
    buf[0] = b'0'; buf[1] = b'x';
    const H: &[u8] = b"0123456789ABCDEF";
    for i in 0..16 { buf[17 - i] = H[(v & 0xF) as usize]; v >>= 4; }
    core::str::from_utf8(buf).unwrap_or("0x????????????????")
}

pub fn fmt_mhz<'a>(mhz: u32, buf: &'a mut [u8; 24]) -> &'a str {
    if mhz == 0 {
        buf[..3].copy_from_slice(b"N/A");
        return core::str::from_utf8(&buf[..3]).unwrap_or("N/A");
    }
    let mut pos = 0usize;
    if mhz >= 1000 {
        let gi = mhz / 1000; let gf = (mhz % 1000) / 10;
        let mut t = [0u8; 16]; let s = fmt_u32(gi, &mut t);
        for b in s.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        if pos < 24 { buf[pos] = b'.'; pos += 1; }
        if gf < 10 && pos < 24 { buf[pos] = b'0'; pos += 1; }
        let mut t2 = [0u8; 16]; let sf = fmt_u32(gf, &mut t2);
        for b in sf.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        for b in b" GHz" { if pos < 24 { buf[pos] = *b; pos += 1; } }
    } else {
        let mut t = [0u8; 16]; let s = fmt_u32(mhz, &mut t);
        for b in s.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        for b in b" MHz" { if pos < 24 { buf[pos] = *b; pos += 1; } }
    }
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}

pub fn fmt_mib<'a>(mb: u64, buf: &'a mut [u8; 24]) -> &'a str {
    if mb == 0 { buf[0] = b'0'; buf[1] = b'B'; return core::str::from_utf8(&buf[..2]).unwrap_or("0"); }
    let mut pos = 0usize;
    if mb >= 1024 {
        let gi = mb / 1024; let gf = (mb % 1024) * 10 / 1024;
        let mut t = [0u8; 20]; let s = fmt_u64(gi, &mut t);
        for b in s.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        if pos < 24 { buf[pos] = b'.'; pos += 1; }
        if pos < 24 { buf[pos] = b'0' + gf as u8; pos += 1; }
        for b in b" GB" { if pos < 24 { buf[pos] = *b; pos += 1; } }
    } else {
        let mut t = [0u8; 20]; let s = fmt_u64(mb, &mut t);
        for b in s.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
        for b in b" MB" { if pos < 24 { buf[pos] = *b; pos += 1; } }
    }
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}

pub fn fmt_uptime<'a>(buf: &'a mut [u8; 24]) -> &'a str {
    let (h, m, s) = crate::time::pit::uptime_hms();
    let mut pos = 0usize;
    macro_rules! push2 { ($n:expr) => {{
        if $n < 10 { buf[pos] = b'0'; pos += 1; }
        let mut t = [0u8; 16]; let st = fmt_u32($n, &mut t);
        for b in st.bytes() { if pos < 24 { buf[pos] = b; pos += 1; } }
    }}}
    push2!(h); if pos < 24 { buf[pos] = b':'; pos += 1; }
    push2!(m); if pos < 24 { buf[pos] = b':'; pos += 1; }
    push2!(s);
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}