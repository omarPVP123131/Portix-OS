#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::println;
use portix_rt::syscall::{sys_open, sys_read, sys_close, sys_getdents};

struct ProcessEntry {
    pid: u64,
    name: String,
    state: char,
    vmsize_kb: u64,
    rss_kb: u64,
}

fn read_file_string(path: &[u8]) -> Option<String> {
    let fd = sys_open(path.as_ptr(), 0);
    if fd < 0 { return None; }
    let mut buf = [0u8; 512];
    let n = sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
    sys_close(fd as i32);
    if n <= 0 { return None; }
    core::str::from_utf8(&buf[..n as usize]).ok().map(String::from)
}

fn parse_process_status(pid_str: &str) -> Option<ProcessEntry> {
    let mut path = [0u8; 64];
    let prefix = b"/proc/";
    let suffix = b"/status";
    let mut p = 0;
    for &b in prefix { path[p] = b; p += 1; }
    for &b in pid_str.as_bytes() { path[p] = b; p += 1; }
    for &b in suffix { path[p] = b; p += 1; }
    path[p] = 0;

    let content = read_file_string(&path[..p + 1])?;
    let mut name = String::new();
    let mut state = 'S';
    let mut pid = 0u64;
    let mut vmsize_kb = 0u64;
    let mut rss_kb = 0u64;

    for line in content.lines() {
        if line.starts_with("Name:\t") {
            name = String::from(&line[6..]);
        } else if line.starts_with("State:\t") {
            if let Some(ch) = line[7..].chars().next() {
                state = ch;
            }
        } else if line.starts_with("Pid:\t") {
            pid = line[5..].trim().parse::<u64>().unwrap_or(0);
        } else if line.starts_with("VmSize:\t") {
            let part = line[8..].trim();
            let num_str = part.split_whitespace().next().unwrap_or("0");
            vmsize_kb = num_str.parse::<u64>().unwrap_or(0);
        } else if line.starts_with("VmRSS:\t") {
            let part = line[7..].trim();
            let num_str = part.split_whitespace().next().unwrap_or("0");
            rss_kb = num_str.parse::<u64>().unwrap_or(0);
        }
    }

    if pid > 0 {
        Some(ProcessEntry { pid, name, state, vmsize_kb, rss_kb })
    } else {
        None
    }
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let uptime_str = read_file_string(b"/proc/uptime\0").unwrap_or_else(|| String::from("0.0 0.0"));
    let uptime_secs: u64 = uptime_str.split_whitespace().next().and_then(|s| s.split('.').next()).and_then(|s| s.parse().ok()).unwrap_or(0);

    let loadavg_str = read_file_string(b"/proc/loadavg\0").unwrap_or_else(|| String::from("0.00 0.00 0.00 1/1 1"));
    let mut load_parts = loadavg_str.split_whitespace();
    let l1 = load_parts.next().unwrap_or("0.00");
    let l5 = load_parts.next().unwrap_or("0.00");
    let l15 = load_parts.next().unwrap_or("0.00");

    let meminfo_str = read_file_string(b"/proc/meminfo\0").unwrap_or_else(|| String::from(""));
    let mut mem_total_kb = 65536u64;
    let mut mem_free_kb = 48128u64;
    for line in meminfo_str.lines() {
        if line.starts_with("MemTotal:") {
            mem_total_kb = line[9..].trim().split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(65536);
        } else if line.starts_with("MemFree:") {
            mem_free_kb = line[8..].trim().split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(48128);
        }
    }
    let mem_used_kb = mem_total_kb.saturating_sub(mem_free_kb);

    let mut dirent_buf = [0u8; 1024];
    let n = sys_getdents(b"/proc\0".as_ptr(), dirent_buf.as_mut_ptr(), dirent_buf.len());
    let mut procs: Vec<ProcessEntry> = Vec::new();

    if n > 0 {
        let total = n as usize;
        let mut offset = 0usize;
        while offset + 19 <= total {
            let reclen = u16::from_le_bytes([dirent_buf[offset + 16], dirent_buf[offset + 17]]) as usize;
            let dtype = dirent_buf[offset + 18];
            if reclen == 0 || offset + reclen > total { break; }

            let name_start = offset + 19;
            let mut name_end = name_start;
            while name_end < offset + reclen && dirent_buf[name_end] != 0 {
                name_end += 1;
            }

            if dtype == 2 {
                if let Ok(name_str) = core::str::from_utf8(&dirent_buf[name_start..name_end]) {
                    if name_str != "." && name_str != ".." && name_str != "self" && name_str.chars().all(|c| c.is_ascii_digit()) {
                        if let Some(entry) = parse_process_status(name_str) {
                            procs.push(entry);
                        }
                    }
                }
            }
            offset += reclen;
        }
    }

    let running_count = procs.iter().filter(|p| p.state == 'R').count();
    let sleeping_count = procs.iter().filter(|p| p.state == 'S').count();

    println!("top - {:02}:{:02}:{:02} up {} min,  1 user,  load average: {}, {}, {}",
             (uptime_secs / 3600) % 24, (uptime_secs / 60) % 60, uptime_secs % 60,
             uptime_secs / 60, l1, l5, l15);
    println!("Tasks: {} total, {} running, {} sleeping, 0 stopped, 0 zombie",
             procs.len(), running_count, sleeping_count);
    println!("%Cpu(s):  0.1 us,  0.1 sy,  0.0 ni, 99.8 id,  0.0 wa,  0.0 hi,  0.0 si");
    println!("MiB Mem :  {:5.1} total,  {:5.1} free,  {:5.1} used,    4.0 buff/cache",
             mem_total_kb as f32 / 1024.0, mem_free_kb as f32 / 1024.0, mem_used_kb as f32 / 1024.0);
    println!("");
    println!("  PID USER      PR  NI    VIRT    RES  S  %CPU  %MEM     TIME+ COMMAND");
    for p in &procs {
        println!("{:5} root      20   0   {:5}   {:5}  {}   0.0   0.4   0:00.01 {}",
                 p.pid, p.vmsize_kb, p.rss_kb, p.state, p.name);
    }

    0
}
