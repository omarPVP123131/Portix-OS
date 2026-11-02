// userland/bin/abi-linux — Verificador de la compatibilidad binaria Linux
// Portix OS v6.0 'Chronos' (Bloque VII / ciclo de mantenimiento v6.x)
//
// Ejercita la puerta SYSCALL (MSR IA32_LSTAR) del kernel con el ABI Linux
// x86_64 y comprueba que el traductor del microkernel las atiende: es la
// demostración, desde Ring 3, de la compatibilidad binaria declarada en la
// congelación de la ABI (/proc/abi).
//
// 100% Rust no_std, sin libc, sin dependencias externas.

#![no_std]
#![no_main]

use portix_rt::println;

/// Invoca una llamada al sistema por la puerta `syscall` (ABI Linux x86_64).
///
/// Convención del ABI Linux: número en RAX, argumentos en RDI, RSI, RDX,
/// R10, R8 y R9; el kernel devuelve el resultado en RAX. `syscall` destruye
/// RCX (RIP de retorno) y R11 (RFLAGS), que el kernel reconstruye.
#[inline]
unsafe fn linux_syscall6(n: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") n as i64 => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        in("r9") a6,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline]
unsafe fn linux_syscall(n: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    linux_syscall6(n, a1, a2, a3, 0, 0, 0)
}

const MSG: &[u8] = b"[ABI-LINUX] write(1) entregado por la puerta SYSCALL del kernel Portix\n";

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    println!("=== Verificador de ABI Linux (puerta SYSCALL / MSR LSTAR) ===");

    let mut ok = 0u32;
    let mut total = 0u32;

    // 1) getpid (39): traducción directa a sys_getpid del ABI nativo
    let pid = unsafe { linux_syscall(39, 0, 0, 0) };
    total += 1;
    if pid > 0 {
        ok += 1;
        println!("  [OK] getpid()    [39]  -> {}", pid);
    } else {
        println!("  [FALLO] getpid() [39]  -> {}", pid);
    }

    // 2) getppid (110)
    let ppid = unsafe { linux_syscall(110, 0, 0, 0) };
    total += 1;
    if ppid >= 0 {
        ok += 1;
        println!("  [OK] getppid()   [110] -> {}", ppid);
    } else {
        println!("  [FALLO] getppid() [110] -> {}", ppid);
    }

    // 3) getuid (102) / geteuid (107)
    let uid = unsafe { linux_syscall(102, 0, 0, 0) };
    let euid = unsafe { linux_syscall(107, 0, 0, 0) };
    total += 2;
    if uid >= 0 { ok += 1; println!("  [OK] getuid()    [102] -> {}", uid); }
    else { println!("  [FALLO] getuid() [102] -> {}", uid); }
    if euid >= 0 { ok += 1; println!("  [OK] geteuid()   [107] -> {}", euid); }
    else { println!("  [FALLO] geteuid() [107] -> {}", euid); }

    // 4) write (1) con el mensaje real, por la misma puerta
    let w = unsafe { linux_syscall(1, 1, MSG.as_ptr() as u64, MSG.len() as u64) };
    total += 1;
    if w == MSG.len() as i64 {
        ok += 1;
        println!("  [OK] write(1)    [1]   -> {} bytes entregados", w);
    } else {
        println!("  [FALLO] write(1) [1]   -> {}", w);
    }

    // 5) uname (63) con estructura de 390 bytes del ABI Linux
    let mut buf = [0u8; 512];
    let u = unsafe { linux_syscall(63, buf.as_mut_ptr() as u64, 0, 0) };
    total += 1;
    if u == 0 {
        ok += 1;
        let sysname = core::str::from_utf8(&buf[0..6]).unwrap_or("?");
        println!("  [OK] uname()     [63]  -> sysname='{}' (struct con relleno del ABI)", sysname.trim_end_matches('\0'));
    } else {
        println!("  [FALLO] uname()  [63]  -> {}", u);
    }

    // 7) [FRENTE C-5] Numero Linux sin traduccion (999 no existe en ningun
    //    kernel Linux x86_64): el traductor debe devolver -ENOSYS (38) y
    //    contabilizarlo, NUNCA ejecutar un syscall nativo de Portix con ese
    //    numero. El resultado queda medido en /proc/abi.
    let e = unsafe { linux_syscall(999, 0, 0, 0) };
    total += 1;
    if e == -38 {
        ok += 1;
        println!("  [OK] syscall 999 (inexistente) -> ENOSYS(-38): fallback medido, sin ejecutar nada nativo");
    } else {
        println!("  [FALLO] syscall 999 (inexistente) -> {} (se esperaba -38 ENOSYS)", e);
    }

    println!("  Resultado: {}/{} llamadas del ABI Linux atendidas por el kernel", ok, total);
    if ok == total {
        println!("[LINUX-ABI-PASS] Compatibilidad binaria Linux ejercida y verificada desde Ring 3");
        0
    } else {
        println!("[LINUX-ABI-FAIL] El traductor no atendio todas las llamadas");
        1
    }
}
