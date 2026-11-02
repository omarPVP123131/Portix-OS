#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

fn parse_id(s: &str) -> Option<u32> {
    if s == "root" { return Some(0); }
    if s == "bin" { return Some(1); }
    if s == "daemon" { return Some(2); }
    if s == "sys" { return Some(3); }
    if s == "user" { return Some(1000); }
    
    let mut val: u32 = 0;
    if s.is_empty() { return None; }
    for b in s.bytes() {
        if b < b'0' || b > b'9' { return None; }
        val = val.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(val)
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    if argc < 3 || argv.is_null() {
        println!("chown: missing operand");
        return 1;
    }

    let spec_ptr = unsafe { *argv.add(1) };
    if spec_ptr.is_null() { return 1; }
    let mut len = 0;
    while unsafe { *spec_ptr.add(len) } != 0 { len += 1; }
    let spec = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(spec_ptr, len) }) {
        Ok(s) => s,
        Err(_) => { println!("chown: invalid spec"); return 1; }
    };

    let (user_str, group_str) = match spec.find(':') {
        Some(idx) => (&spec[..idx], Some(&spec[idx+1..])),
        None => (spec, None),
    };

    let uid = match parse_id(user_str) {
        Some(u) => u,
        None => {
            println!("chown: invalid user: '{}'", user_str);
            return 1;
        }
    };

    let gid = match group_str {
        Some(g) => match parse_id(g) {
            Some(grp) => grp,
            None => {
                println!("chown: invalid group: '{}'", g);
                return 1;
            }
        },
        None => uid,
    };

    let mut exit_code = 0;
    for i in 2..argc {
        let file_ptr = unsafe { *argv.add(i) };
        if file_ptr.is_null() { continue; }
        let ret = portix_rt::syscall::sys_lchown(file_ptr, uid, gid);
        if ret < 0 {
            let mut flen = 0;
            while unsafe { *file_ptr.add(flen) } != 0 { flen += 1; }
            let fname = core::str::from_utf8(unsafe { core::slice::from_raw_parts(file_ptr, flen) }).unwrap_or("file");
            println!("chown: cannot access '{}'", fname);
            exit_code = 1;
        }
    }

    exit_code
}
