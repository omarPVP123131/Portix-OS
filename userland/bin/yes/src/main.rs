#![no_std]
#![no_main]

extern crate alloc;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut msg = "y";
    let mut max_count: usize = 0; // 0 = default infinite / broken pipe

    let mut i = 1;
    while i < argc && !argv.is_null() {
        let p = unsafe { *argv.add(i) };
        if !p.is_null() {
            let mut len = 0;
            while unsafe { *p.add(len) } != 0 { len += 1; }
            if let Ok(arg) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, len) }) {
                if arg == "-n" && i + 1 < argc {
                    i += 1;
                    let p2 = unsafe { *argv.add(i) };
                    if !p2.is_null() {
                        let mut l2 = 0;
                        while unsafe { *p2.add(l2) } != 0 { l2 += 1; }
                        if let Ok(c_str) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p2, l2) }) {
                            if let Ok(cnt) = c_str.parse::<usize>() {
                                max_count = cnt;
                            }
                        }
                    }
                } else {
                    msg = arg;
                }
            }
        }
        i += 1;
    }

    let mut buf = alloc::string::String::from(msg);
    buf.push('\n');
    let bytes = buf.as_bytes();

    let mut count = 0usize;
    loop {
        let ret = portix_rt::syscall::sys_write(1, bytes.as_ptr(), bytes.len());
        if ret <= 0 {
            break;
        }
        count += 1;
        if max_count > 0 && count >= max_count {
            break;
        }
        if max_count == 0 && count >= 50 {
            break;
        }
    }

    0
}
