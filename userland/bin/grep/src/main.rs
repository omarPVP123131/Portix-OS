#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::{println, File, O_RDONLY};

fn match_line(pattern: &str, line: &str) -> bool {
    line.contains(pattern)
}

fn process_fd(pattern: &str, fd: i32, print_prefix: Option<&str>) -> (bool, bool) {
    let mut buf = [0u8; 1024];
    let mut line_buf = [0u8; 1024];
    let mut line_len = 0;
    let mut matched = false;

    loop {
        let ret = portix_rt::syscall::sys_read(fd, buf.as_mut_ptr(), buf.len());
        if ret <= 0 {
            break;
        }
        let n = ret as usize;
        for i in 0..n {
            let b = buf[i];
            if b == b'\n' {
                if let Ok(line) = core::str::from_utf8(&line_buf[..line_len]) {
                    if match_line(pattern, line) {
                        matched = true;
                        if let Some(prefix) = print_prefix {
                            println!("{}:{}", prefix, line);
                        } else {
                            println!("{}", line);
                        }
                    }
                }
                line_len = 0;
            } else if line_len < line_buf.len() {
                line_buf[line_len] = b;
                line_len += 1;
            }
        }
    }

    if line_len > 0 {
        if let Ok(line) = core::str::from_utf8(&line_buf[..line_len]) {
            if match_line(pattern, line) {
                matched = true;
                if let Some(prefix) = print_prefix {
                    println!("{}:{}", prefix, line);
                } else {
                    println!("{}", line);
                }
            }
        }
    }

    (matched, false)
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 2 || argv.is_null() {
        println!("Uso: grep <patron> [archivo...]");
        return 2;
    }

    let pat_ptr = unsafe { *argv.add(1) };
    if pat_ptr.is_null() {
        return 2;
    }
    let mut plen = 0;
    while unsafe { *pat_ptr.add(plen) } != 0 { plen += 1; }
    let pattern = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(pat_ptr, plen) }) {
        Ok(s) => s,
        Err(_) => return 2,
    };

    if argc == 2 {
        let (matched, _) = process_fd(pattern, 0, None);
        return if matched { 0 } else { 1 };
    }

    let multiple = argc > 3;
    let mut any_matched = false;
    let mut error_occurred = false;

    for i in 2..argc {
        let file_ptr = unsafe { *argv.add(i) };
        if file_ptr.is_null() { continue; }
        let mut flen = 0;
        while unsafe { *file_ptr.add(flen) } != 0 { flen += 1; }
        let fname = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(file_ptr, flen) }) {
            Ok(s) => s,
            Err(_) => {
                error_occurred = true;
                continue;
            }
        };

        if fname == "-" {
            let (m, _) = process_fd(pattern, 0, if multiple { Some("(standard input)") } else { None });
            if m { any_matched = true; }
        } else {
            match File::open(fname, O_RDONLY) {
                Ok(f) => {
                    let (m, _) = process_fd(pattern, f.fd, if multiple { Some(fname) } else { None });
                    if m { any_matched = true; }
                }
                Err(_) => {
                    println!("grep: {}: No such file or directory", fname);
                    error_occurred = true;
                }
            }
        }
    }

    if error_occurred && !any_matched {
        2
    } else if any_matched {
        0
    } else {
        1
    }
}
