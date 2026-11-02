#![no_std]
#![no_main]

use portix_rt::{println, print};

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut path_buf = [0u8; 128];
    let path = if argc > 1 {
        unsafe {
            let ptr = *argv.offset(1);
            let mut l = 0;
            while *ptr.add(l) != 0 && l < 127 {
                path_buf[l] = *ptr.add(l);
                l += 1;
            }
            path_buf[l] = 0;
            &path_buf[..l + 1]
        }
    } else {
        b"/\0"
    };

    let mut buf = [0u8; 2048];
    let ret = portix_rt::syscall::sys_getdents(path.as_ptr(), buf.as_mut_ptr(), buf.len());
    if ret < 0 {
        println!("ls: fallo al listar directorio");
        return 1;
    }
    let n = ret as usize;
    if n == 0 {
        println!("(directorio vacio)");
        return 0;
    }

    let mut pos = 0usize;
    while pos + 19 < n {
        let d_reclen = u16::from_le_bytes([buf[pos + 16], buf[pos + 17]]) as usize;
        if d_reclen == 0 || pos + d_reclen > n {
            break;
        }
        let d_type = buf[pos + 18];
        let name_bytes = &buf[pos + 19..pos + d_reclen];
        let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
        if let Ok(name) = core::str::from_utf8(&name_bytes[..name_len]) {
            if !name.is_empty() {
                if d_type == 2 {
                    print!("{}/  ", name);
                } else {
                    print!("{}  ", name);
                }
            }
        }
        pos += d_reclen;
    }
    println!();
    0
}
