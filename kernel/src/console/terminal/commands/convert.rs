// console/terminal/commands/convert.rs
// Comandos: calc, hex, dec, bin, rgb

use crate::console::terminal::{Terminal, LineColor};
use crate::console::terminal::fmt::*;

pub fn cmd_calc(t: &mut Terminal, args: &[u8]) {
    if args.is_empty() {
        t.write_line("  Uso: calc 2+3*4   o   = 100/7", LineColor::Warning); return;
    }
    match simple_eval(args) {
        Some(r) => {
            let mut buf = [0u8; 80]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"  = ");
            if r < 0 { buf[pos] = b'-'; pos += 1; append_u32(&mut buf, &mut pos, (-r) as u32); }
            else { append_u32(&mut buf, &mut pos, r as u32); }
            append_str(&mut buf, &mut pos, b"  (0x"); append_hex64_short(&mut buf, &mut pos, r as u64);
            append_str(&mut buf, &mut pos, b")");
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
        None => t.write_line("  Error: expresion invalida", LineColor::Error),
    }
}

pub fn cmd_hex(t: &mut Terminal, args: &[u8]) {
    if args.is_empty() { t.write_line("  Uso: hex <decimal>", LineColor::Warning); return; }
    match parse_u64(trim(args)) {
        Some(n) => {
            let mut buf = [0u8; 80]; let mut pos = 0;
            append_u32(&mut buf, &mut pos, (n & 0xFFFF_FFFF) as u32);
            append_str(&mut buf, &mut pos, b" = 0x"); append_hex64_short(&mut buf, &mut pos, n);
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
        None => t.write_line("  Error: numero decimal invalido", LineColor::Error),
    }
}

pub fn cmd_dec(t: &mut Terminal, args: &[u8]) {
    if args.is_empty() { t.write_line("  Uso: dec <0xHEX>", LineColor::Warning); return; }
    match parse_hex(trim(args)) {
        Some(n) => {
            let mut buf = [0u8; 80]; let mut pos = 0;
            append_str(&mut buf, &mut pos, b"0x"); append_hex64_short(&mut buf, &mut pos, n);
            append_str(&mut buf, &mut pos, b" = "); append_u32(&mut buf, &mut pos, (n & 0xFFFF_FFFF) as u32);
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
        None => t.write_line("  Error: hexadecimal invalido", LineColor::Error),
    }
}

pub fn cmd_bin(t: &mut Terminal, args: &[u8]) {
    if args.is_empty() { t.write_line("  Uso: bin <decimal>", LineColor::Warning); return; }
    match parse_u64(trim(args)) {
        Some(n) => {
            let v = n & 0xFFFF_FFFF;
            let mut buf = [0u8; 80]; let mut pos = 0;
            append_u32(&mut buf, &mut pos, v as u32); append_str(&mut buf, &mut pos, b" = 0b");
            let bits = if v == 0 { 1 } else { (64 - v.leading_zeros() as usize + 3) / 4 * 4 };
            for i in (0..bits).rev() {
                buf[pos] = if (v >> i) & 1 != 0 { b'1' } else { b'0' }; pos += 1;
                if i > 0 && i % 4 == 0 { buf[pos] = b'_'; pos += 1; }
            }
            t.write_bytes(&buf[..pos], LineColor::Success);
        }
        None => t.write_line("  Error: decimal invalido", LineColor::Error),
    }
}

pub fn cmd_rgb(t: &mut Terminal, args: &[u8]) {
    let args = trim(args);
    if args.starts_with(b"#") || args.starts_with(b"0x") {
        let hex = if args.starts_with(b"#") { &args[1..] } else { &args[2..] };
        let val = match parse_hex_raw(hex) {
            Some(v) => v,
            None => { t.write_line("  Error: color hex invalido", LineColor::Error); return; }
        };
        let r = ((val >> 16) & 0xFF) as u8;
        let g = ((val >>  8) & 0xFF) as u8;
        let b = (val & 0xFF) as u8;
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  RGB("); append_u32(&mut buf, &mut pos, r as u32);
        append_str(&mut buf, &mut pos, b", "); append_u32(&mut buf, &mut pos, g as u32);
        append_str(&mut buf, &mut pos, b", "); append_u32(&mut buf, &mut pos, b as u32);
        append_str(&mut buf, &mut pos, b")  =  0x"); append_hex64_short(&mut buf, &mut pos, val);
        t.write_bytes(&buf[..pos], LineColor::Success);
    } else {
        // Parsear "r g b" separado por espacios
        let mut nums = [0u32; 3]; let mut ni = 0; let mut start = 0;
        for i in 0..=args.len() {
            let at_space = i == args.len() || args[i] == b' ';
            if at_space && i > start && ni < 3 {
                if let Some(n) = parse_u64(&args[start..i]) { nums[ni] = n as u32 & 0xFF; ni += 1; }
                start = i + 1;
            }
        }
        if ni < 3 {
            t.write_line("  Uso: rgb <r> <g> <b>  o  rgb #RRGGBB", LineColor::Warning); return;
        }
        let val = ((nums[0] as u64) << 16) | ((nums[1] as u64) << 8) | nums[2] as u64;
        let mut buf = [0u8; 80]; let mut pos = 0;
        append_str(&mut buf, &mut pos, b"  RGB("); append_u32(&mut buf, &mut pos, nums[0]);
        append_str(&mut buf, &mut pos, b", "); append_u32(&mut buf, &mut pos, nums[1]);
        append_str(&mut buf, &mut pos, b", "); append_u32(&mut buf, &mut pos, nums[2]);
        append_str(&mut buf, &mut pos, b")  =  0x"); append_hex64_short(&mut buf, &mut pos, val);
        t.write_bytes(&buf[..pos], LineColor::Success);
    }
}
