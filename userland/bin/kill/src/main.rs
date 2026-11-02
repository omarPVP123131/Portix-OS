#![no_std]
#![no_main]

use portix_rt::println;
use portix_rt::syscall::sys_kill;

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    if argc <= 1 {
        println!("Uso: kill [-sig] <pid>");
        return 1;
    }

    let mut sig = 15i32; // SIGTERM by default
    let mut pid_idx = 1;

    unsafe {
        let arg1_ptr = *argv.offset(1);
        if !arg1_ptr.is_null() && *arg1_ptr == b'-' {
            let mut val: i32 = 0;
            let mut offset = 1;
            while *arg1_ptr.add(offset) >= b'0' && *arg1_ptr.add(offset) <= b'9' {
                val = val * 10 + (*arg1_ptr.add(offset) - b'0') as i32;
                offset += 1;
            }
            if val > 0 {
                sig = val;
                pid_idx = 2;
            }
        }
    }

    if pid_idx >= argc {
        println!("kill: falta especificar el PID");
        return 1;
    }

    let mut target_pid: u64 = 0;
    unsafe {
        let p_ptr = *argv.offset(pid_idx as isize);
        if !p_ptr.is_null() {
            let mut offset = 0;
            while *p_ptr.add(offset) >= b'0' && *p_ptr.add(offset) <= b'9' {
                target_pid = target_pid * 10 + (*p_ptr.add(offset) - b'0') as u64;
                offset += 1;
            }
        }
    }

    if target_pid == 0 {
        println!("kill: PID invalido");
        return 1;
    }

    let res = sys_kill(target_pid as i32, sig);
    if res != 0 {
        println!("kill: error al enviar senal {} al proceso {}", sig, target_pid);
        return 1;
    }

    0
}
