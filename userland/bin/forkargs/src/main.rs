#![no_std]
#![no_main]
//! forkargs — prueba de regresión: argumento de `execve` situado en los
//! últimos bytes de la última página mapeada del heap (justo bajo el `brk`).
//! Antes del arreglo de `copy_user_str_array` el kernel copiaba 512 bytes en
//! bloque, la copia cruzaba a una página sin mapear y el argumento llegaba
//! vacío. El hijo (/bin/echo) debe imprimir la cadena completa.

use portix_rt::println;
use portix_rt::syscall::{sys_brk, sys_fork, sys_execve, sys_waitpid, sys_exit};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let cur = sys_brk(0);
    if cur <= 0 { println!("[FORKARGS-FAIL] brk"); return 1; }
    // Alinea el final del heap a página y sitúa la cadena en sus últimos 16 bytes.
    let fin = ((cur as usize) + 0x1000) & !0xFFF;
    if sys_brk(fin) < fin as i64 { println!("[FORKARGS-FAIL] brk extend"); return 1; }
    let msg = b"ARG-AL-BORDE-OK\0"; // 16 bytes exactos
    let dst = (fin - msg.len()) as *mut u8;
    unsafe { core::ptr::copy_nonoverlapping(msg.as_ptr(), dst, msg.len()); }
    println!("[FORKARGS] argumento en 0x{:x} (brk=0x{:x}, {} bytes antes del fin de pagina)", dst as usize, fin, msg.len());
    let ruta = b"/bin/echo\0";
    let a0 = b"echo\0";
    let argv: [*const u8; 3] = [a0.as_ptr(), dst as *const u8, core::ptr::null()];
    let pid = sys_fork();
    if pid == 0 {
        sys_execve(ruta.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
        sys_exit(127);
    }
    if pid < 0 { println!("[FORKARGS-FAIL] fork"); return 1; }
    let mut st: i32 = 0;
    sys_waitpid(pid as i32, &mut st as *mut i32, 0);
    println!("[FORKARGS-DONE] hijo rc={}", portix_rt::wexitstatus(st));
    // Regresión 2: `munmap` sobre el texto del programa debe rechazarse (EINVAL)
    // y el proceso debe seguir vivo y ejecutando código de esa página.
    let r = portix_rt::syscall::sys_munmap(0x2000_0000, 0x1000);
    if r == -22 { println!("[MUNMAP-TEXT-PROTEGIDO] munmap(0x20000000) -> EINVAL y el proceso sigue vivo"); }
    else { println!("[MUNMAP-TEXT-FAIL] munmap(0x20000000) -> {}", r); }
    // Regresión 3: estado POSIX de waitpid: exit(3) -> WIFEXITED, WEXITSTATUS=3.
    let pid2 = sys_fork();
    if pid2 == 0 { sys_exit(3); }
    let mut st2: i32 = 0;
    sys_waitpid(pid2 as i32, &mut st2 as *mut i32, 0);
    if portix_rt::wifexited(st2) && portix_rt::wexitstatus(st2) == 3 { println!("[WAIT-STATUS-POSIX-OK] status=0x{:x} WEXITSTATUS=3", st2); }
    else { println!("[WAIT-STATUS-POSIX-FAIL] status=0x{:x}", st2); }
    0
}
