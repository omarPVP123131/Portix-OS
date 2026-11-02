#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;

fn process_line(line: &str, delim: char, field: usize) {
    let parts: Vec<&str> = line.split(delim).collect();
    if field >= 1 && field <= parts.len() {
        println!("{}", parts[field - 1]);
    } else {
        println!();
    }
}

fn process_file(filename: &str, delim: char, field: usize) {
    let mut cstr = String::from(filename);
    cstr.push('\0');
    let fd = portix_rt::syscall::sys_open(cstr.as_ptr(), 0);
    if fd < 0 {
        println!("cut: no se puede abrir '{}'", filename);
        return;
    }

    let mut content = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let n = portix_rt::syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
        if n <= 0 {
            break;
        }
        content.extend_from_slice(&buf[..n as usize]);
    }
    let _ = portix_rt::syscall::sys_close(fd as i32);

    if let Ok(text) = core::str::from_utf8(&content) {
        for line in text.split('\n') {
            if !line.is_empty() {
                process_line(line, delim, field);
            }
        }
    }
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut delim = '\t';
    let mut field = 1usize;
    let mut file_idx = 1;

    let mut i = 1;
    while i < argc && !argv.is_null() {
        let p = unsafe { *argv.add(i) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(arg) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                if arg.len() > 2 && arg.starts_with("-d") {
                    // [CICLO 47-b] forma pegada POSIX: -d:
                    if let Some(c) = arg[2..].chars().next() { delim = c; }
                    file_idx = i + 1;
                } else if arg.len() > 2 && arg.starts_with("-f") {
                    // [CICLO 47-b] forma pegada POSIX: -f2
                    if let Ok(f_num) = arg[2..].parse::<usize>() { field = f_num; }
                    file_idx = i + 1;
                } else if arg == "-d" && i + 1 < argc {
                    i += 1;
                    let pd = unsafe { *argv.add(i) };
                    if !pd.is_null() {
                        let mut ld = 0; while unsafe { *pd.add(ld) } != 0 { ld += 1; }
                        if let Ok(ds) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(pd, ld) }) {
                            if let Some(c) = ds.chars().next() {
                                delim = c;
                            }
                        }
                    }
                    file_idx = i + 1;
                } else if arg == "-f" && i + 1 < argc {
                    i += 1;
                    let pf = unsafe { *argv.add(i) };
                    if !pf.is_null() {
                        let mut lf = 0; while unsafe { *pf.add(lf) } != 0 { lf += 1; }
                        if let Ok(fs) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(pf, lf) }) {
                            if let Ok(f_num) = fs.parse::<usize>() {
                                field = f_num;
                            }
                        }
                    }
                    file_idx = i + 1;
                } else {
                    break;
                }
            }
        }
        i += 1;
    }

    if file_idx >= argc {
        // Read from stdin
        let mut content = Vec::new();
        let mut buf = [0u8; 512];
        loop {
            let n = portix_rt::syscall::sys_read(0, buf.as_mut_ptr(), buf.len());
            if n <= 0 { break; }
            content.extend_from_slice(&buf[..n as usize]);
        }
        if let Ok(text) = core::str::from_utf8(&content) {
            for line in text.split('\n') {
                if !line.is_empty() {
                    process_line(line, delim, field);
                }
            }
        }
        return 0;
    }

    for idx in file_idx..argc {
        let p = unsafe { *argv.add(idx) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(fname) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                process_file(fname, delim, field);
            }
        }
    }

    0
}
