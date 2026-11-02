#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December"
];

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(month: u32, year: u32) -> u32 {
    match month {
        1 => 31,
        2 => if is_leap_year(year) { 29 } else { 28 },
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 30,
    }
}

fn day_of_week(day: u32, month: u32, year: u32) -> u32 {
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = year;
    if month < 3 {
        y -= 1;
    }
    let m_idx = (month - 1) as usize;
    (y + y / 4 - y / 100 + y / 400 + t[m_idx] + day) % 7
}

fn parse_u32(s: &str) -> Option<u32> {
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
    let mut month: u32 = 9;
    let mut year: u32 = 2026;

    if argc >= 3 && !argv.is_null() {
        let m_ptr = unsafe { *argv.add(1) };
        let y_ptr = unsafe { *argv.add(2) };
        if !m_ptr.is_null() && !y_ptr.is_null() {
            let mut mlen = 0; while unsafe { *m_ptr.add(mlen) } != 0 { mlen += 1; }
            let mut ylen = 0; while unsafe { *y_ptr.add(ylen) } != 0 { ylen += 1; }
            let m_str = core::str::from_utf8(unsafe { core::slice::from_raw_parts(m_ptr, mlen) }).unwrap_or("");
            let y_str = core::str::from_utf8(unsafe { core::slice::from_raw_parts(y_ptr, ylen) }).unwrap_or("");
            if let (Some(m), Some(y)) = (parse_u32(m_str), parse_u32(y_str)) {
                if m >= 1 && m <= 12 && y >= 1 {
                    month = m;
                    year = y;
                }
            }
        }
    } else if argc == 2 && !argv.is_null() {
        let arg_ptr = unsafe { *argv.add(1) };
        if !arg_ptr.is_null() {
            let mut len = 0; while unsafe { *arg_ptr.add(len) } != 0 { len += 1; }
            let arg_str = core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }).unwrap_or("");
            if let Some(val) = parse_u32(arg_str) {
                if val >= 1 && val <= 12 {
                    month = val;
                } else if val > 12 {
                    year = val;
                }
            }
        }
    }

    let m_name = MONTH_NAMES[(month - 1) as usize];
    println!("    {} {}", m_name, year);
    println!("Su Mo Tu We Th Fr Sa");

    let first_day = day_of_week(1, month, year);
    let total_days = days_in_month(month, year);

    for _ in 0..first_day {
        portix_rt::print!("   ");
    }

    let mut col = first_day;
    for day in 1..=total_days {
        if day < 10 {
            portix_rt::print!(" {} ", day);
        } else {
            portix_rt::print!("{} ", day);
        }
        col += 1;
        if col == 7 {
            println!("");
            col = 0;
        }
    }
    if col != 0 {
        println!("");
    }

    0
}
