#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use portix_rt::{
    println, File, O_RDONLY, O_WRONLY, O_CREAT, O_TRUNC, fs::Stat
};

const AR_MAGIC: &[u8; 8] = b"!<arch>\n";
const AR_FMAG: &[u8; 2] = b"`\n";

struct ArMember {
    name: String,
    date: i64,
    uid: u32,
    gid: u32,
    mode: u32,
    data: Vec<u8>,
}

fn parse_ar(archive_data: &[u8]) -> Option<Vec<ArMember>> {
    if archive_data.len() < 8 || &archive_data[..8] != AR_MAGIC {
        return None;
    }
    let mut members = Vec::new();
    let mut offset = 8;
    while offset + 60 <= archive_data.len() {
        let hdr = &archive_data[offset..offset + 60];
        if &hdr[58..60] != AR_FMAG {
            break;
        }

        let name_raw = core::str::from_utf8(&hdr[0..16]).unwrap_or("");
        let name = name_raw.trim().trim_end_matches('/').to_string();

        let date_str = core::str::from_utf8(&hdr[16..28]).unwrap_or("0").trim();
        let date = date_str.parse::<i64>().unwrap_or(0);

        let uid_str = core::str::from_utf8(&hdr[28..34]).unwrap_or("0").trim();
        let uid = uid_str.parse::<u32>().unwrap_or(0);

        let gid_str = core::str::from_utf8(&hdr[34..40]).unwrap_or("0").trim();
        let gid = gid_str.parse::<u32>().unwrap_or(0);

        let mode_str = core::str::from_utf8(&hdr[40..48]).unwrap_or("100644").trim();
        let mode = u32::from_str_radix(mode_str, 8).unwrap_or(0o100644);

        let size_str = core::str::from_utf8(&hdr[48..58]).unwrap_or("0").trim();
        let size = size_str.parse::<usize>().unwrap_or(0);

        offset += 60;
        if offset + size > archive_data.len() {
            break;
        }

        let data = archive_data[offset..offset + size].to_vec();
        offset += size;
        if size % 2 != 0 && offset < archive_data.len() {
            offset += 1; // 2-byte alignment padding
        }

        // Skip symbol table or empty headers if name is empty or starts with '/'
        if !name.is_empty() && !name.starts_with('/') {
            members.push(ArMember {
                name,
                date,
                uid,
                gid,
                mode,
                data,
            });
        }
    }
    Some(members)
}

fn format_ar_field(val: &str, width: usize) -> [u8; 16] {
    let mut out = [b' '; 16];
    let b = val.as_bytes();
    let l = b.len().min(width);
    out[..l].copy_from_slice(&b[..l]);
    out
}

fn write_ar(path: &str, members: &[ArMember]) -> bool {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(AR_MAGIC);

    for m in members {
        let mut hdr = [b' '; 60];
        let mut name_with_slash = m.name.clone();
        name_with_slash.push('/');
        let nb = name_with_slash.as_bytes();
        let nl = nb.len().min(16);
        hdr[..nl].copy_from_slice(&nb[..nl]);

        let date_str = m.date.to_string();
        let db = date_str.as_bytes();
        let dl = db.len().min(12);
        hdr[16..16 + dl].copy_from_slice(&db[..dl]);

        let uid_str = m.uid.to_string();
        let ub = uid_str.as_bytes();
        let ul = ub.len().min(6);
        hdr[28..28 + ul].copy_from_slice(&ub[..ul]);

        let gid_str = m.gid.to_string();
        let gb = gid_str.as_bytes();
        let gl = gb.len().min(6);
        hdr[34..34 + gl].copy_from_slice(&gb[..gl]);

        // Mode formatted as octal
        let mut mode_str = String::new();
        let mut temp_mode = m.mode;
        if temp_mode == 0 {
            mode_str.push('0');
        } else {
            let mut digits = [0u8; 10];
            let mut d_idx = 0;
            while temp_mode > 0 {
                digits[d_idx] = (temp_mode % 8) as u8 + b'0';
                temp_mode /= 8;
                d_idx += 1;
            }
            while d_idx > 0 {
                d_idx -= 1;
                mode_str.push(digits[d_idx] as char);
            }
        }
        let mb = mode_str.as_bytes();
        let ml = mb.len().min(8);
        hdr[40..40 + ml].copy_from_slice(&mb[..ml]);

        let size_str = m.data.len().to_string();
        let sb = size_str.as_bytes();
        let sl = sb.len().min(10);
        hdr[48..48 + sl].copy_from_slice(&sb[..sl]);

        hdr[58..60].copy_from_slice(AR_FMAG);
        out.extend_from_slice(&hdr);
        out.extend_from_slice(&m.data);
        if m.data.len() % 2 != 0 {
            out.push(b'\n'); // 2-byte alignment padding
        }
    }

    match File::open(path, O_WRONLY | O_CREAT | O_TRUNC) {
        Ok(mut f) => f.write(&out).is_ok(),
        Err(_) => false,
    }
}

fn read_file_bytes(path: &str) -> Option<(Vec<u8>, Stat)> {
    let mut null_terminated = String::from(path);
    null_terminated.push('\0');
    let mut st = Stat::default();
    let ret = portix_rt::syscall::sys_fstatat(-100, null_terminated.as_ptr(), &mut st as *mut _, 0);
    if ret != 0 {
        return None;
    }

    let mut file = File::open(path, O_RDONLY).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => return None,
        }
    }
    Some((buf, st))
}

fn parse_args(argc: i32, argv: *const *const u8) -> Vec<String> {
    let mut args = Vec::new();
    if argc > 0 && !argv.is_null() {
        for idx in 0..argc as usize {
            unsafe {
                let ptr = *argv.add(idx);
                if !ptr.is_null() {
                    let mut len = 0;
                    while *ptr.add(len) != 0 { len += 1; }
                    if let Ok(s) = core::str::from_utf8(core::slice::from_raw_parts(ptr, len)) {
                        args.push(s.to_string());
                    }
                }
            }
        }
    }
    args
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = parse_args(argc, argv);
    if args.len() < 2 {
        println!("Uso: ar [opciones] <archivo.a> [miembros ...]");
        println!("Comandos principales:");
        println!("  r[cs]  insertar/reemplazar archivos con indice");
        println!("  t      mostrar tabla de contenidos");
        println!("  x      extraer miembros del archivo");
        println!("  d      eliminar miembros del archivo");
        println!("  --version");
        return 1;
    }

    let op = &args[1];
    if op == "--version" || op == "-v" {
        println!("GNU ar (Portix OS Native Binutils, 100% Rust) 2.42.0");
        println!("Copyright (C) 2026 Free Software Foundation / Portix Project");
        return 0;
    }

    if args.len() < 3 {
        println!("ar: falta el nombre del archivo de biblioteca");
        return 1;
    }

    let archive_path = &args[2];
    let member_names = &args[3..];

    let mut existing_members = Vec::new();
    if let Some((ar_bytes, _)) = read_file_bytes(archive_path) {
        if let Some(parsed) = parse_ar(&ar_bytes) {
            existing_members = parsed;
        }
    }

    if op.contains('t') {
        // Table of contents
        for m in &existing_members {
            println!("{}", m.name);
        }
        return 0;
    } else if op.contains('x') {
        // Extract
        for m in &existing_members {
            if member_names.is_empty() || member_names.iter().any(|n| n == &m.name) {
                if let Ok(mut f) = File::open(&m.name, O_WRONLY | O_CREAT | O_TRUNC) {
                    let _ = f.write(&m.data);
                }
            }
        }
        return 0;
    } else if op.contains('d') {
        // Delete
        existing_members.retain(|m| !member_names.iter().any(|n| n == &m.name));
        if write_ar(archive_path, &existing_members) {
            return 0;
        } else {
            println!("ar: error al escribir '{}'", archive_path);
            return 1;
        }
    } else if op.contains('r') || op.contains('q') {
        // Replace / Append
        for member_path in member_names {
            if let Some((data, st)) = read_file_bytes(member_path) {
                let filename = member_path.rsplit('/').next().unwrap_or(member_path).to_string();
                existing_members.retain(|m| m.name != filename);
                existing_members.push(ArMember {
                    name: filename,
                    date: st.st_mtime,
                    uid: st.st_uid,
                    gid: st.st_gid,
                    mode: st.st_mode,
                    data,
                });
            } else {
                println!("ar: {}: No such file", member_path);
                return 1;
            }
        }

        if write_ar(archive_path, &existing_members) {
            return 0;
        } else {
            println!("ar: error al escribir '{}'", archive_path);
            return 1;
        }
    } else {
        println!("ar: comando '{}' no reconocido", op);
        return 1;
    }
}
