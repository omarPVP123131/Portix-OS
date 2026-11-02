#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut file_path: Option<&str> = None;
    // Opciones POSIX: -l (líneas), -w (palabras), -c (bytes); sin opción, las tres.
    let (mut show_l, mut show_w, mut show_c) = (false, false, false);

    let mut i = 1;
    while i < argc && !argv.is_null() {
        let arg_ptr = unsafe { *argv.add(i) };
        i += 1;
        if arg_ptr.is_null() { continue; }
        let mut len = 0;
        while unsafe { *arg_ptr.add(len) } != 0 { len += 1; }
        if let Ok(arg_str) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }) {
            if arg_str.len() > 1 && arg_str.as_bytes()[0] == b'-' {
                for &c in &arg_str.as_bytes()[1..] {
                    match c {
                        b'l' => show_l = true,
                        b'w' => show_w = true,
                        b'c' | b'm' => show_c = true,
                        _ => { println!("wc: opcion invalida -- '{}'", c as char); return 1; }
                    }
                }
            } else if file_path.is_none() {
                file_path = Some(arg_str);
            }
        }
    }
    if !show_l && !show_w && !show_c { show_l = true; show_w = true; show_c = true; }

    let path = match file_path {
        Some(p) => p,
        None => {
            println!("Uso: wc <archivo>");
            return 1;
        }
    };

    let mut path_cstr = alloc::string::String::from(path);
    path_cstr.push('\0');

    let fd = portix_rt::syscall::sys_open(path_cstr.as_ptr(), 0);
    if fd < 0 {
        println!("wc: no se puede abrir '{}'", path);
        return 1;
    }

    let mut lines = 0usize;
    let mut words = 0usize;
    let mut bytes = 0usize;
    let mut in_word = false;

    let mut buf = [0u8; 1024];
    loop {
        let n = portix_rt::syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
        if n <= 0 {
            break;
        }
        let count = n as usize;
        bytes += count;

        for &b in &buf[..count] {
            if b == b'\n' {
                lines += 1;
            }
            let is_space = b == b' ' || b == b'\t' || b == b'\n' || b == b'\r';
            if is_space {
                in_word = false;
            } else if !in_word {
                in_word = true;
                words += 1;
            }
        }
    }

    let _ = portix_rt::syscall::sys_close(fd as i32);

    let mut out = alloc::string::String::new();
    if show_l { out.push_str(&alloc::format!(" {:4}", lines)); }
    if show_w { out.push_str(&alloc::format!(" {:4}", words)); }
    if show_c { out.push_str(&alloc::format!(" {:4}", bytes)); }
    println!("{} {}", out, path);
    0
}
