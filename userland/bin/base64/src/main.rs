#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8], wrap: usize) {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        let idx0 = (b0 >> 2) & 0x3F;
        let idx1 = (((b0 & 0x03) << 4) | (b1 >> 4)) & 0x3F;
        let idx2 = (((b1 & 0x0F) << 2) | (b2 >> 6)) & 0x3F;
        let idx3 = b2 & 0x3F;

        out.push(B64_CHARS[idx0 as usize]);
        out.push(B64_CHARS[idx1 as usize]);
        if i + 1 < data.len() {
            out.push(B64_CHARS[idx2 as usize]);
        } else {
            out.push(b'=');
        }
        if i + 2 < data.len() {
            out.push(B64_CHARS[idx3 as usize]);
        } else {
            out.push(b'=');
        }

        i += 3;
    }

    if wrap == 0 {
        if let Ok(s) = core::str::from_utf8(&out) {
            println!("{}", s);
        }
    } else {
        let mut start = 0;
        while start < out.len() {
            let end = (start + wrap).min(out.len());
            if let Ok(s) = core::str::from_utf8(&out[start..end]) {
                println!("{}", s);
            }
            start = end;
        }
    }
}

fn b64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn b64_decode(data: &[u8]) {
    let mut clean = Vec::new();
    for &b in data {
        if !b.is_ascii_whitespace() {
            clean.push(b);
        }
    }

    let mut out = Vec::new();
    let mut i = 0;
    while i < clean.len() {
        if clean[i] == b'=' { break; }
        let c0 = clean[i];
        let c1 = if i + 1 < clean.len() { clean[i + 1] } else { b'A' };
        let c2 = if i + 2 < clean.len() { clean[i + 2] } else { b'=' };
        let c3 = if i + 3 < clean.len() { clean[i + 3] } else { b'=' };

        let v0 = b64_val(c0).unwrap_or(0);
        let v1 = b64_val(c1).unwrap_or(0);
        let v2 = b64_val(c2).unwrap_or(0);
        let v3 = b64_val(c3).unwrap_or(0);

        out.push((v0 << 2) | (v1 >> 4));
        if c2 != b'=' {
            out.push(((v1 & 0x0F) << 4) | (v2 >> 2));
        }
        if c3 != b'=' {
            out.push(((v2 & 0x03) << 6) | v3);
        }

        i += 4;
    }

    let _ = portix_rt::syscall::sys_write(1, out.as_ptr(), out.len());
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut decode = false;
    let mut filename: Option<String> = None;

    for i in 1..argc {
        let p = unsafe { *argv.add(i) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(arg) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                if arg == "-d" || arg == "--decode" {
                    decode = true;
                } else if !arg.starts_with('-') && filename.is_none() {
                    filename = Some(String::from(arg));
                }
            }
        }
    }

    let mut content = Vec::new();
    if let Some(ref fname) = filename {
        let mut cstr = fname.clone();
        cstr.push('\0');
        let fd = portix_rt::syscall::sys_open(cstr.as_ptr(), 0);
        if fd >= 0 {
            let mut buf = [0u8; 512];
            loop {
                let n = portix_rt::syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
                if n <= 0 { break; }
                content.extend_from_slice(&buf[..n as usize]);
            }
            let _ = portix_rt::syscall::sys_close(fd as i32);
        } else {
            println!("base64: no se puede abrir '{}'", fname);
            return 1;
        }
    } else {
        let mut buf = [0u8; 512];
        loop {
            let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
            if n <= 0 { break; }
            content.extend_from_slice(&buf[..n as usize]);
        }
    }

    if decode {
        b64_decode(&content);
    } else {
        b64_encode(&content, 76);
    }
    0
}
