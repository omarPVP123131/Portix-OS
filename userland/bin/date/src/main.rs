#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

const DAYS_OF_WEEK: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAYS_PER_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn is_leap(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    // [CICLO 47-b] `date +FORMATO` (POSIX): %Y %m %d %H %M %S %j %a %b %e %s %%
    let mut formato: Option<&str> = None;
    if argc >= 2 && !argv.is_null() {
        let p = unsafe { *argv.add(1) };
        if !p.is_null() {
            let mut l = 0; while unsafe { *p.add(l) } != 0 { l += 1; }
            if let Ok(a) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, l) }) {
                if let Some(f) = a.strip_prefix('+') { formato = Some(f); }
            }
        }
    }
    let mut ts = portix_rt::Timespec { tv_sec: 0, tv_nsec: 0 };
    let sec = if portix_rt::syscall::sys_clock_gettime(0, &mut ts) == 0 && ts.tv_sec > 0 {
        ts.tv_sec as u64
    } else {
        portix_rt::syscall::sys_time(core::ptr::null_mut()) as u64
    };

    let total_days = sec / 86400;
    let day_seconds = sec % 86400;

    let hours = day_seconds / 3600;
    let minutes = (day_seconds % 3600) / 60;
    let seconds = day_seconds % 60;

    let dow = DAYS_OF_WEEK[(total_days % 7) as usize];

    let mut year = 1970u32;
    let mut rem_days = total_days as u32;

    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if rem_days >= days_in_year {
            rem_days -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }

    let mut month = 0usize;
    loop {
        let mut dim = DAYS_PER_MONTH[month];
        if month == 1 && is_leap(year) {
            dim = 29;
        }
        if rem_days >= dim {
            rem_days -= dim;
            month += 1;
        } else {
            break;
        }
    }

    let day = rem_days + 1;
    let month_str = MONTHS[month];

    if let Some(f) = formato {
        let mut out = alloc::string::String::new();
        let mut it = f.chars();
        while let Some(c) = it.next() {
            if c != '%' { out.push(c); continue; }
            match it.next() {
                Some('Y') => out.push_str(&alloc::format!("{}", year)),
                Some('m') => out.push_str(&alloc::format!("{:02}", month + 1)),
                Some('d') => out.push_str(&alloc::format!("{:02}", day)),
                Some('e') => out.push_str(&alloc::format!("{:2}", day)),
                Some('H') => out.push_str(&alloc::format!("{:02}", hours)),
                Some('M') => out.push_str(&alloc::format!("{:02}", minutes)),
                Some('S') => out.push_str(&alloc::format!("{:02}", seconds)),
                Some('j') => out.push_str(&alloc::format!("{:03}", rem_days + 1)),
                Some('s') => out.push_str(&alloc::format!("{}", sec)),
                Some('a') => out.push_str(dow),
                Some('b') => out.push_str(month_str),
                Some('%') => out.push('%'),
                Some(o) => { out.push('%'); out.push(o); }
                None => out.push('%'),
            }
        }
        println!("{}", out);
        return 0;
    }

    println!(
        "{} {} {:02} {:02}:{:02}:{:02} UTC {}",
        dow, month_str, day, hours, minutes, seconds, year
    );

    0
}
