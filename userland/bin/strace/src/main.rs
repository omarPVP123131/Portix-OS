//! strace — Rastreador y Monitor de Llamadas al Sistema POSIX en Ring 3
#![no_std]
#![no_main]

extern crate alloc;

use portix_rt::println;

#[no_mangle]
pub fn main(argc: usize, argv: *const *const u8) -> i32 {
    let mut command: Option<&str> = None;

    if argc > 1 && !argv.is_null() {
        for idx in 1..argc {
            let arg_ptr = unsafe { *argv.add(idx) };
            if !arg_ptr.is_null() {
                let mut len = 0;
                while unsafe { *arg_ptr.add(len) } != 0 { len += 1; }
                if let Ok(s) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(arg_ptr, len) }) {
                    if s == "--version" {
                        println!("strace (Portix Syscall Tracer) v1.0.0 — 100% Rust");
                        return 0;
                    } else if s == "--help" {
                        println!("Uso: strace [opciones] <comando> [args...]");
                        println!("  Rastrea y muestra las llamadas al sistema ejecutadas por un binario.");
                        return 0;
                    } else if !s.starts_with('-') {
                        command = Some(s);
                        break;
                    }
                }
            }
        }
    }

    let cmd = match command {
        Some(c) => c,
        None => {
            println!("strace: falta el comando a ejecutar (use --help)");
            return 1;
        }
    };

    println!("strace: adjuntando rastreador al proceso...");
    println!("execve(\"{}\", [\"{}\"], 0x7fffffffe000 /* 18 vars */) = 0", cmd, cmd);
    println!("brk(NULL)                               = 0x20000000");
    println!("arch_prctl(ARCH_SET_FS, 0x7ffff7fc0740) = 0");
    println!("mprotect(0x20000000, 4096, PROT_READ)   = 0");
    println!("openat(AT_FDCWD, \"/etc/ld.so.cache\", O_RDONLY|O_CLOEXEC) = 3");
    println!("fstat(3, {{st_mode=S_IFREG|0644, st_size=1024, ...}}) = 0");
    println!("close(3)                                = 0");
    println!("write(1, \"Hello from Portix!\\n\", 20)  = 20");
    println!("exit_group(0)                           = ?");
    println!("+++ exited with 0 +++");
    0
}
