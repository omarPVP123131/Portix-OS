// userland/bin/dbg/src/main.rs — Depurador de Ring 3 de Portix OS (v3.0.0)
//
// Ciclo de mantenimiento v6.x: el depurador deja de ser una demostración y
// pasa a operar sobre información real del sistema:
//
//   * PTRACE_PEEKTEXT/POKEDATA operan sobre el espacio de direcciones del
//     PROCESO OBJETIVO (el kernel los resuelve con el CR3 del objetivo y
//     atraviesa el bit de sólo lectura de la página, como un depurador real).
//   * `/proc/<pid>/symbols` publica la tabla `.symtab` del ejecutable, de modo
//     que el depurador resuelve nombres, direcciones y desensambla con
//     simbolización de destinos.
//   * Puntos de parada persistentes con semántica de depurador real: el byte
//     0xCC pertenece al PROGRAMA, así que el registro (dirección + byte
//     original) vive en `/tmp/dbg-<imagen>.bps` y se rearma automáticamente
//     sobre el objetivo vivo, verificando cada paso con PTRACE_PEEKTEXT.
//   * Puntos de vigilancia por HARDWARE (DR0-DR3 del CPU) vía las extensiones
//     PTRACE_SETWATCHPOINT/CLEARWATCHPOINTS/GETWATCHPOINTS.
#![no_std]
#![no_main]

extern crate alloc;

mod disasm;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use disasm::{decodificar, Instruccion, VENTANA};
use portix_rt::{
    println, print, fork, waitpid, fs,
    process::{WUNTRACED, wifexited, wexitstatus, wifstopped, wstopsig, wtermsig},
    syscall::{
        sys_ptrace, UserRegs,
        PTRACE_TRACEME, PTRACE_PEEKDATA, PTRACE_POKEDATA, PTRACE_PEEKTEXT,
        PTRACE_CONT, PTRACE_SINGLESTEP, PTRACE_SETREGS,
        PTRACE_GETREGS, PTRACE_ATTACH, PTRACE_DETACH,
        PTRACE_SETWATCHPOINT, PTRACE_CLEARWATCHPOINTS, PTRACE_GETWATCHPOINTS,
        watch_cfg, dr7_len_code, DR7_RW_WRITE, DR7_RW_RW, DR7_RW_EXEC,
        PTRACE_SETBPCOND, PTRACE_GETBPCOND, PTRACE_CLEARBPCOND, PTRACE_SETBPCIGNORE,
        PTRACE_SETBPCOND2, BpCond2, PTRACE_SETBPCONDN, BpCondN, CondClause,
        PTRACE_GETTRAPINFO, TrapInfo, repartir_vigilancia,
        BpCond, BpCondStat,
        CREG_RAX, CREG_RBX, CREG_RCX, CREG_RDX, CREG_RSI, CREG_RDI, CREG_RBP,
        CREG_RSP, CREG_R8, CREG_R9, CREG_R10, CREG_R11, CREG_R12, CREG_R13,
        CREG_R14, CREG_R15, CREG_RIP, CREG_RFLAGS, CREG_ADDR,
        COP_EQ, COP_NE, COP_LT, COP_LE, COP_GT, COP_GE,
    }
};

// ═══════════════════════════════════════════════════════════════════════════
//  Utilidades de línea de comandos y de texto
// ═══════════════════════════════════════════════════════════════════════════

fn parse_args(argc: i32, argv: *const *const u8) -> Vec<String> {
    let mut args = Vec::new();
    for i in 0..argc {
        unsafe {
            let ptr = *argv.add(i as usize);
            if ptr.is_null() { break; }
            let mut len = 0;
            while *ptr.add(len) != 0 { len += 1; }
            let bytes = core::slice::from_raw_parts(ptr, len);
            if let Ok(s) = core::str::from_utf8(bytes) {
                args.push(s.to_string());
            }
        }
    }
    args
}

/// Nombre publicado por `/proc/<pid>/status` (campo `Name:`).
fn nombre_de_proceso(pid: u64) -> Option<String> {
    let txt = leer_proc(pid, "status")?;
    for linea in txt.lines() {
        let t = linea.trim_start();
        if let Some(resto) = t.strip_prefix("Name:") {
            return Some(resto.trim().to_string());
        }
    }
    None
}

/// Procesos vivos (PID, nombre) enumerados a través del directorio `/proc`.
fn listar_procesos() -> Vec<(u64, String)> {
    let mut out: Vec<(u64, String)> = Vec::new();
    let mut buf = [0u8; 2048];
    let n = portix_rt::syscall::sys_getdents(b"/proc\0".as_ptr(), buf.as_mut_ptr(), buf.len());
    if n <= 0 { return out; }
    let total = n as usize;
    let mut off = 0usize;
    while off + 19 <= total {
        let reclen = u16::from_le_bytes([buf[off + 16], buf[off + 17]]) as usize;
        if reclen == 0 || off + reclen > total { break; }
        let ns = off + 19;
        let mut ne = ns;
        while ne < off + reclen && buf[ne] != 0 { ne += 1; }
        if let Ok(nombre) = core::str::from_utf8(&buf[ns..ne]) {
            if let Ok(pid) = nombre.parse::<u64>() {
                if let Some(n) = nombre_de_proceso(pid) {
                    out.push((pid, n));
                }
            }
        }
        off += reclen;
    }
    out
}

/// Resuelve el objetivo de depuración. Admite tres formas, igual que las
/// herramientas reales del ecosistema Unix:
///   * `self`      → el propio depurador (equivalente a `gdb -p $$`);
///   * un número   → ese PID literal;
///   * un NOMBRE   → primer proceso vivo con ese nombre (equivalente a
///                   `gdb -p $(pgrep -o nombre)`), eligiendo el PID más bajo
///                   —el proceso más antiguo y por tanto el más estable— y
///                   descartando al propio depurador.
fn parse_pid(s: &str) -> Option<u64> {
    if s == "self" || s == "propio" { return Some(portix_rt::syscall::sys_getpid()); }
    if let Some(p) = parse_u64(s) { return Some(p); }
    let propio = portix_rt::syscall::sys_getpid();
    let mut mejor: Option<u64> = None;
    for (pid, nombre) in listar_procesos() {
        if pid == propio || nombre != s { continue; }
        mejor = match mejor { Some(m) if m <= pid => Some(m), _ => Some(pid) };
    }
    mejor
}

fn parse_u64(s: &str) -> Option<u64> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<u64>().ok()
    }
}

fn print_registers(regs: &UserRegs) {
    println!("Registers (x86_64):");
    println!("  RAX: {:#018x}  RBX: {:#018x}", regs.rax, regs.rbx);
    println!("  RCX: {:#018x}  RDX: {:#018x}", regs.rcx, regs.rdx);
    println!("  RSI: {:#018x}  RDI: {:#018x}", regs.rsi, regs.rdi);
    println!("  RBP: {:#018x}  RSP: {:#018x}", regs.rbp, regs.rsp);
    println!("  R8:  {:#018x}  R9:  {:#018x}", regs.r8, regs.r9);
    println!("  R10: {:#018x}  R11: {:#018x}", regs.r10, regs.r11);
    println!("  R12: {:#018x}  R13: {:#018x}", regs.r12, regs.r13);
    println!("  R14: {:#018x}  R15: {:#018x}", regs.r14, regs.r15);
    println!("  RIP: {:#018x}  EFLAGS: {:#010x}", regs.rip, regs.eflags);
    println!("  FS:  {:#018x}  GS:     {:#018x}", regs.fs_base, regs.gs_base);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Tabla de símbolos del proceso (/proc/<pid>/symbols)
// ═══════════════════════════════════════════════════════════════════════════

struct Simbolo {
    addr: u64,
    size: u64,
    nombre: String,
}

fn leer_proc(pid: u64, archivo: &str) -> Option<String> {
    let ruta = format!("/proc/{}/{}", pid, archivo);
    let f = fs::File::open(&ruta, 0).ok()?;
    let mut out = String::new();
    let mut buf = [0u8; 1024];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                match core::str::from_utf8(&buf[..n]) {
                    Ok(s) => out.push_str(s),
                    Err(_) => break,
                }
                if out.len() > 262_144 { break; }
            }
            Err(_) => break,
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn leer_simbolos(pid: u64) -> Vec<Simbolo> {
    let mut out = Vec::new();
    let texto = match leer_proc(pid, "symbols") {
        Some(t) => t,
        None => return out,
    };
    for linea in texto.lines() {
        if !linea.starts_with("0x") { continue; }
        let mut it = linea.split_whitespace();
        let a = it.next().and_then(parse_u64);
        let s = it.next().and_then(|x| x.parse::<u64>().ok());
        let n = it.next();
        if let (Some(addr), Some(size), Some(nombre)) = (a, s, n) {
            out.push(Simbolo { addr, size, nombre: nombre.to_string() });
        }
    }
    out
}

/// Resuelve una dirección a "simbolo+0xNN".
fn simbolizar(syms: &[Simbolo], addr: u64) -> Option<String> {
    for s in syms {
        let fin = s.addr.saturating_add(s.size.max(1));
        if addr >= s.addr && addr < fin {
            let off = addr - s.addr;
            return Some(if off == 0 { s.nombre.clone() } else { format!("{}+{:#x}", s.nombre, off) });
        }
    }
    None
}

fn buscar_simbolo<'a>(syms: &'a [Simbolo], patron: &str) -> Option<&'a Simbolo> {
    // Coincidencia exacta primero; si no, el primer símbolo cuyo nombre acabe
    // en `::<patrón>` o sea igual al último segmento.
    for s in syms {
        if s.nombre == patron { return Some(s); }
    }
    for s in syms {
        if s.nombre.ends_with(&format!("::{}", patron)) { return Some(s); }
    }
    for s in syms {
        if s.nombre.contains(patron) { return Some(s); }
    }
    None
}

/// Traduce un token de la línea de órdenes (dirección o símbolo) a dirección.
fn resolver(pid: u64, token: &str, syms: &mut Vec<Simbolo>) -> Option<u64> {
    if let Some(a) = parse_u64(token) {
        if a >= 0x1000 { return Some(a); }
    }
    if syms.is_empty() { *syms = leer_simbolos(pid); }
    buscar_simbolo(syms, token).map(|s| s.addr)
}

// ═══════════════════════════════════════════════════════════════════════════
//  Acceso a la memoria del proceso objetivo
// ═══════════════════════════════════════════════════════════════════════════

/// Lee una palabra de 64 bits del objetivo. La salida se entrega por puntero
/// (`data`) porque el valor leído puede ser negativo como i64 y ese valor no
/// debe confundirse con un código de error del kernel.
fn peek_u64(pid: u64, addr: u64) -> Option<u64> {
    let mut out: u64 = 0;
    let r = sys_ptrace(PTRACE_PEEKTEXT, pid as i64, addr as usize, &mut out as *mut u64 as usize);
    if r == 0 { Some(out) } else { None }
}

fn poke_u64(pid: u64, addr: u64, val: u64) -> bool {
    sys_ptrace(PTRACE_POKEDATA, pid as i64, addr as usize, val as usize) == 0
}

/// Lee `n` bytes del objetivo (palabra a palabra, como hace `ptrace`).
fn leer_bytes(pid: u64, addr: u64, n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        let a = addr + i as u64;
        match peek_u64(pid, a) {
            Some(v) => {
                let b = v.to_ne_bytes();
                for k in 0..8 { if i + k < n { out.push(b[k]); } }
            }
            None => break,
        }
        i += 8;
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
//  Puntos de parada persistentes
//
//  Semántica de depurador real: un punto de parada pertenece al PROGRAMA, no
//  al PID efímero que lo ejecuta. El registro vive en `/tmp/dbg-<imagen>.bps`
//  (imagen = nombre del proceso, tal como lo publica /proc/<pid>/status), de
//  modo que al reiniciarse el objetivo el punto de parada sigue existiendo y
//  se rearma automáticamente en el nuevo espacio de direcciones.
// ═══════════════════════════════════════════════════════════════════════════

/// Clave de persistencia: la imagen del objetivo (o `pidN` si no se puede leer).
fn clave_imagen(pid: u64) -> String {
    match nombre_de_proceso(pid) {
        Some(n) if !n.is_empty() => n,
        _ => format!("pid{}", pid),
    }
}

fn ruta_estado(pid: u64) -> String {
    format!("/tmp/dbg-{}.bps", clave_imagen(pid))
}

/// Lectura cruda del almacén: cada línea declara su clase de registro.
///   `B <direccion> <byte_original> <nombre>`   punto de parada INT3
///   `W <direccion> <longitud> <rw> <nombre>`   punto de vigilancia por hardware
fn cargar_lineas(pid: u64) -> Vec<(char, u64, u64, u64, String)> {
    let mut v = Vec::new();
    if let Ok(f) = fs::File::open(&ruta_estado(pid), 0) {
        let mut buf = [0u8; 1024];
        if let Ok(n) = f.read(&mut buf) {
            if let Ok(txt) = core::str::from_utf8(&buf[..n]) {
                for l in txt.lines() {
                    let mut it = l.split_whitespace();
                    let clase = it.next().unwrap_or("B").chars().next().unwrap_or('B');
                    let a = it.next().and_then(parse_u64);
                    let b = it.next().and_then(parse_u64);
                    match (clase, a, b) {
                        ('B', Some(addr), Some(byte)) => {
                            let n = it.next().unwrap_or("-").to_string();
                            v.push(('B', addr, byte, 0, n));
                        }
                        ('W', Some(addr), Some(len)) => {
                            let rw = it.next().and_then(parse_u64).unwrap_or(1);
                            let n = it.next().unwrap_or("-").to_string();
                            v.push(('W', addr, len, rw, n));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    v
}

fn cargar_bps(pid: u64) -> Vec<(u64, u8, String)> {
    cargar_lineas(pid).into_iter()
        .filter(|e| e.0 == 'B')
        .map(|e| (e.1, e.2 as u8, e.4))
        .collect()
}

fn cargar_wps(pid: u64) -> Vec<(u64, u64, u64, String)> {
    cargar_lineas(pid).into_iter()
        .filter(|e| e.0 == 'W')
        .map(|e| (e.1, e.2, e.3, e.4))
        .collect()
}

fn guardar_estado(pid: u64, bps: &[(u64, u8, String)], wps: &[(u64, u64, u64, String)]) -> bool {
    let ruta = ruta_estado(pid);
    // O_WRONLY | O_CREAT | O_TRUNC
    let f = match fs::File::open(&ruta, 0x1 | 0x40 | 0x200) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut txt = String::new();
    for (a, b, n) in bps {
        txt.push_str(&format!("B 0x{:x} 0x{:02x} {}\n", a, b, n));
    }
    for (a, l, rw, n) in wps {
        txt.push_str(&format!("W 0x{:x} {} {} {}\n", a, l, rw, n));
    }
    f.write(txt.as_bytes()).map(|_| true).unwrap_or(false)
}

fn cmd_break(pid: u64, token: &str) -> i32 {
    let mut syms = Vec::new();
    let addr = match resolver(pid, token, &mut syms) {
        Some(a) => a,
        None => {
            println!("[dbg] no se pudo resolver '{}' (¿símbolo inexistente?)", token);
            return 1;
        }
    };
    let actual = match peek_u64(pid, addr) {
        Some(v) => v,
        None => {
            println!("[dbg] PEEKTEXT 0x{:x} falló: el proceso no tiene esa página mapeada", addr);
            return 1;
        }
    };
    let orig = (actual & 0xFF) as u8;
    let nombre = simbolizar(&syms, addr).unwrap_or_else(|| String::from(token));
    if orig == 0xCC {
        println!("[BP-SET] 0x{:016x} <{}> ya tenía un INT3 instalado (idempotente)", addr, nombre);
    } else {
        // Sustituye el byte más bajo por 0xCC conservando el resto de la palabra
        if !poke_u64(pid, addr, (actual & !0xFF) | 0xCC) {
            println!("[BP-SET-FAIL] POKEDATA rechazado en 0x{:x}", addr);
            return 1;
        }
    }
    let verif = peek_u64(pid, addr).map(|v| (v & 0xFF) as u8).unwrap_or(0);
    println!("[BP-SET] pid={} addr=0x{:016x} <{}> byte_original={:#04x} byte_actual={:#04x}",
             pid, addr, nombre, orig, verif);
    if verif != 0xCC {
        println!("[BP-SET-FAIL] la verificación de lectura no devolvió 0xCC");
        return 1;
    }
    let mut lista = cargar_bps(pid);
    let wps = cargar_wps(pid);
    let mut reemplazado = false;
    for e in lista.iter_mut() {
        if e.0 == addr { e.1 = orig; e.2 = nombre.clone(); reemplazado = true; }
    }
    if !reemplazado { lista.push((addr, orig, nombre.clone())); }
    if !guardar_estado(pid, &lista, &wps) {
        println!("[dbg] aviso: no se pudo persistir el estado en {}", ruta_estado(pid));
    }
    println!("[BP-SET-PASS] punto de parada persistente instalado en 0x{:016x}", addr);
    0
}

fn cmd_bp_list(pid: u64) -> i32 {
    let lista = cargar_bps(pid);
    if lista.is_empty() {
        println!("[dbg] no hay puntos de parada registrados para pid={} (archivo {})", pid, ruta_estado(pid));
        return 0;
    }
    println!("PUNTOS DE PARADA DE {} (imagen '{}', estado persistente + verificación en memoria)",
             pid, clave_imagen(pid));
    let mut ok = 0;
    for (addr, orig, nombre) in lista.iter() {
        let mut vivo = peek_u64(pid, *addr).map(|v| (v & 0xFF) as u8).unwrap_or(0xFF);
        let mut rearmado = false;
        // Rearme: el 0xCC pertenece al programa, así que se reinstala en el
        // espacio de direcciones del objetivo actual si aún no está puesto.
        if vivo != 0xCC {
            if let Some(v) = peek_u64(pid, *addr) {
                if poke_u64(pid, *addr, (v & !0xFF) | 0xCC) {
                    vivo = peek_u64(pid, *addr).map(|x| (x & 0xFF) as u8).unwrap_or(0);
                    rearmado = vivo == 0xCC;
                }
            }
        }
        let estado = if vivo == 0xCC { "INT3 ACTIVO" } else { "ausente" };
        println!("  0x{:016x}  <{}>  original={:#04x}  memoria={:#04x}  {}{}",
                 addr, nombre, orig, vivo, estado, if rearmado { " (rearmado)" } else { "" });
        if vivo == 0xCC { ok += 1; }
    }
    println!("[BP-VERIFIED] {}/{} puntos de parada presentes en la imagen del proceso", ok, lista.len());
    if ok == lista.len() { 0 } else { 1 }
}

fn cmd_bp_clear(pid: u64) -> i32 {
    let lista = cargar_bps(pid);
    if lista.is_empty() {
        println!("[dbg] nada que restaurar para pid={}", pid);
        return 0;
    }
    let mut restaurados = 0;
    for (addr, orig, nombre) in lista.iter() {
        if let Some(v) = peek_u64(pid, *addr) {
            if (v & 0xFF) as u8 == 0xCC {
                if poke_u64(pid, *addr, (v & !0xFF) | (*orig as u64)) {
                    let fin = peek_u64(pid, *addr).map(|x| (x & 0xFF) as u8).unwrap_or(0xFF);
                    if fin == *orig { restaurados += 1; }
                    println!("  0x{:016x} <{}> 0xCC -> {:#04x} (verificado por PEEK)", addr, nombre, fin);
                }
            } else {
                restaurados += 1;
            }
        }
    }
    let wps = cargar_wps(pid);
    let _ = guardar_estado(pid, &[], &wps);
    println!("[BP-RESTORED] {}/{} byte(s) originales devueltos a la imagen '{}' (pid={})",
             restaurados, lista.len(), clave_imagen(pid), pid);
    0
}

// ═══════════════════════════════════════════════════════════════════════════
//  Vigilancia por hardware (DR0-DR3)
// ═══════════════════════════════════════════════════════════════════════════

fn cmd_watch(pid: u64, token: &str, len: u64, rw: u64) -> i32 {
    let mut syms = Vec::new();
    let addr = match resolver(pid, token, &mut syms) {
        Some(a) => a,
        None => {
            println!("[dbg] no se pudo resolver '{}'", token);
            return 1;
        }
    };
    let nombre = simbolizar(&syms, addr).unwrap_or_else(|| String::from(token));
    // [FRENTE C] Un búfer mayor de 8 bytes (o no alineado) no cabe en una
    // ranura: se reparte en trozos alineados de 8/4/2/1 bytes entre DR0-DR3.
    let trozos = match repartir_vigilancia(addr, len.max(1), 4) {
        Some(t) => t,
        None => {
            println!("[WATCH-FAIL] el búfer de {} B en 0x{:016x} necesita más de 4 ranuras de hardware (máximo 32 B alineados)", len, addr);
            return 1;
        }
    };
    if trozos.len() > 1 {
        return cmd_watch_bufer(pid, addr, len, rw, &nombre, &trozos);
    }
    let cfg = watch_cfg(len, rw);
    let slot = sys_ptrace(PTRACE_SETWATCHPOINT, pid as i64, addr as usize, cfg);
    if slot < 0 {
        println!("[WATCH-FAIL] el kernel rechazó la vigilancia (error {})", slot);
        return 1;
    }
    // La vigilancia pertenece al PROGRAMA vigilado: se registra por imagen para
    // que el depurador pueda rearmarla si el objetivo se reinicia.
    let mut wps = cargar_wps(pid);
    let mut reemplazado = false;
    for e in wps.iter_mut() {
        if e.0 == addr { e.1 = len; e.2 = rw; e.3 = nombre.clone(); reemplazado = true; }
    }
    if !reemplazado { wps.push((addr, len, rw, nombre.clone())); }
    let bps = cargar_bps(pid);
    if !guardar_estado(pid, &bps, &wps) {
        println!("[dbg] aviso: no se pudo persistir la vigilancia en {}", ruta_estado(pid));
    }
    let activos = leer_watch(pid);
    println!("[WATCH-ARMED] pid={} imagen='{}' ranura=DR{} addr=0x{:016x} <{}> longitud={}B acceso={} (ranuras activas en el CPU={})",
             pid, clave_imagen(pid), slot, addr, nombre, len,
             if rw == DR7_RW_WRITE { "escritura" } else { "lectura/escritura" }, activos.len());
    0
}

/// [FRENTE C] Arma un búfer completo repartido entre varias ranuras DR0-DR3.
fn cmd_watch_bufer(pid: u64, addr: u64, len: u64, rw: u64, nombre: &str, trozos: &[(u64, u64)]) -> i32 {
    let previos = leer_watch(pid).len();
    if previos + trozos.len() > 4 {
        println!("[WATCH-FAIL] el búfer necesita {} ranuras y sólo quedan {} libres (DR0-DR3)", trozos.len(), 4 - previos);
        return 1;
    }
    let mut ranuras: Vec<(i64, u64, u64)> = Vec::new();
    for (a, l) in trozos.iter() {
        let slot = sys_ptrace(PTRACE_SETWATCHPOINT, pid as i64, *a as usize, watch_cfg(*l, rw));
        if slot < 0 {
            println!("[WATCH-FAIL] el kernel rechazó el trozo 0x{:016x}+{} (error {})", a, l, slot);
            let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, pid as i64, 0, 0);
            return 1;
        }
        ranuras.push((slot, *a, *l));
    }
    let mut wps = cargar_wps(pid);
    for (_, a, l) in ranuras.iter() {
        if !wps.iter().any(|e| e.0 == *a) { wps.push((*a, *l, rw, String::from(nombre))); }
    }
    let bps = cargar_bps(pid);
    let _ = guardar_estado(pid, &bps, &wps);
    let activos = leer_watch(pid);
    println!("[WATCH-ARMED] pid={} imagen='{}' búfer de {} B en 0x{:016x} <{}> acceso={} repartido en {} ranuras (activas en el CPU={})",
             pid, clave_imagen(pid), len, addr, nombre,
             if rw == DR7_RW_WRITE { "escritura" } else { "lectura/escritura" }, ranuras.len(), activos.len());
    for (slot, a, l) in ranuras.iter() {
        println!("  DR{} -> [0x{:016x}, 0x{:016x})  desplazamiento=+{}  longitud={}B", slot, a, a + l, a - addr, l);
    }
    0
}

/// [FRENTE C] Lee del kernel qué ranura disparó la última parada por #DB.
fn leer_trapinfo(pid: u64) -> Option<TrapInfo> {
    let mut ti = TrapInfo::default();
    let r = sys_ptrace(PTRACE_GETTRAPINFO, pid as i64, 0, &mut ti as *mut TrapInfo as usize);
    if r < 0 { None } else { Some(ti) }
}

/// Ensayo del búfer repartido (`dbg --buftest [programa]`): un búfer de 32 B
/// en el marco de <main> se vigila con las CUATRO ranuras del procesador; cada
/// parada se atribuye, con el DR6 real, al trozo exacto del búfer que el
/// programa tocó. Después, el mismo búfer vigilado sólo en su mitad alta debe
/// dejar pasar en silencio las escrituras en la mitad baja.
fn cmd_buftest(programa: &str) -> i32 {
    println!("ENSAYO DE BÚFER REPARTIDO ENTRE DR0-DR3 — objetivo: {}", programa);
    let mut st = 0i32;

    // ── Fase 0: el marco de <main>, derivado de su prólogo ──────────────────
    let hijo0 = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let mut syms = leer_simbolos(hijo0);
    let main = match resolver(hijo0, "main", &mut syms) {
        Some(a) => a,
        None => { println!("[SESSION-FAIL] no se pudo resolver 'main'"); return 1; }
    };
    let mut r0 = UserRegs::default();
    if sys_ptrace(PTRACE_GETREGS, hijo0 as i64, 0, &mut r0 as *mut UserRegs as usize) < 0 { return 1; }
    let ins_main = instruccion_en(hijo0, main);
    let mut reserva = 0x18u64;
    if let Some(resto) = ins_main.texto.strip_prefix("sub rsp, ") {
        if let Some(v) = parse_u64(resto.trim()) { reserva = v; }
    }
    let marco = (r0.rsp - 8 - reserva) & !7;
    let _ = sys_ptrace(PTRACE_DETACH, hijo0 as i64, 0, 0);
    let _ = waitpid(hijo0 as i32, &mut st, 0);
    const TAM: u64 = 32;
    if !es_pila(marco) || !es_pila(marco + TAM - 1) {
        println!("[SESSION-FAIL] el búfer 0x{:016x}+{} no está en la pila del objetivo", marco, TAM);
        return 1;
    }
    let trozos = match repartir_vigilancia(marco, TAM, 4) {
        Some(t) => t,
        None => { println!("[SESSION-FAIL] el búfer no se puede repartir en 4 ranuras"); return 1; }
    };
    println!("[SESSION-BUF-SCAN] marco de <main> derivado del prólogo: 0x{:016x} ({} B reservados); búfer vigilado = [0x{:016x}, 0x{:016x})",
             marco, reserva, marco, marco + TAM);

    // ── Fase A: las cuatro ranuras cubren el búfer completo ─────────────────
    let hijo_a = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let mut ranuras: Vec<(i64, u64, u64)> = Vec::new();
    for (a, l) in trozos.iter() {
        let slot = sys_ptrace(PTRACE_SETWATCHPOINT, hijo_a as i64, *a as usize, watch_cfg(*l, DR7_RW_WRITE));
        if slot < 0 { println!("[SESSION-FAIL] el kernel rechazó el trozo 0x{:016x}", a); return 1; }
        ranuras.push((slot, *a, *l));
    }
    let activas = leer_watch(hijo_a);
    println!("[SESSION-BUF-ARMED] {} ranuras armadas sobre el búfer ({} confirmadas por el kernel):", ranuras.len(), activas.len());
    for (slot, a, l) in ranuras.iter() {
        println!("  DR{} -> [0x{:016x}, 0x{:016x})  desplazamiento=+{}", slot, a, a + l, a - marco);
    }
    let mut golpes: Vec<(u64, u64, u64, u64)> = Vec::new(); // (ranura, desplazamiento, valor, rip)
    let mut mascara = 0u8;
    let mut atribuidos_bien = true;
    let mut salida_a: Option<i32> = None;
    for _ in 0..64 {
        if sys_ptrace(PTRACE_CONT, hijo_a as i64, 0, 0) < 0 { break; }
        let r = waitpid(hijo_a as i32, &mut st, WUNTRACED);
        if r != hijo_a as i64 { break; }
        if wifexited(st) { salida_a = Some(wexitstatus(st)); break; }
        if !wifstopped(st) { break; }
        let mut regs = UserRegs::default();
        let _ = sys_ptrace(PTRACE_GETREGS, hijo_a as i64, 0, &mut regs as *mut UserRegs as usize);
        match leer_trapinfo(hijo_a) {
            Some(ti) if ti.slot < 4 => {
                let dentro = ti.watch_addr >= marco && ti.watch_addr < marco + TAM;
                if !dentro { atribuidos_bien = false; }
                let desp = ti.watch_addr.wrapping_sub(marco);
                let valor = peek_u64(hijo_a, ti.watch_addr).unwrap_or(0);
                mascara |= 1 << ti.slot;
                println!("[SESSION-BUF-HIT] #{} ranura=DR{} (DR6={:#x}) trozo=[+{}, +{}) valor={:#x} RIP=0x{:016x} <{}>",
                         golpes.len() + 1, ti.slot, ti.dr6, desp, desp + 8, valor, regs.rip,
                         simbolizar(&syms, regs.rip).unwrap_or_else(|| String::from("?")));
                golpes.push((ti.slot, desp, valor, regs.rip));
            }
            _ => {
                println!("[SESSION-BUF-STOP] parada sin ranura atribuible (RIP=0x{:016x}); se reanuda", regs.rip);
            }
        }
    }
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_a as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_a as i64, 0, 0);
    if salida_a.is_none() { let _ = waitpid(hijo_a as i32, &mut st, 0); }
    let ranuras_distintas = mascara.count_ones();
    println!("[SESSION-BUF-STATS] paradas={} ranuras distintas que dispararon={} (máscara DR={:#06b}) todas dentro del búfer={} salida del objetivo={:?}",
             golpes.len(), ranuras_distintas, mascara, atribuidos_bien, salida_a);
    let ok_a = golpes.len() >= 2 && ranuras_distintas >= 2 && atribuidos_bien && activas.len() == trozos.len();

    // ── Fase B: sólo la mitad alta del búfer: la mitad baja debe callar ─────
    let hijo_b = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let bajo_tocado: Vec<u64> = golpes.iter().filter(|g| g.1 < TAM / 2).map(|g| g.1).collect();
    for (a, l) in trozos.iter().filter(|(a, _)| *a >= marco + TAM / 2) {
        if sys_ptrace(PTRACE_SETWATCHPOINT, hijo_b as i64, *a as usize, watch_cfg(*l, DR7_RW_WRITE)) < 0 { return 1; }
    }
    println!("[SESSION-BUF-HALF] vigilada sólo la mitad alta [0x{:016x}, 0x{:016x}); la fase A tocó desplazamientos bajos {:?}",
             marco + TAM / 2, marco + TAM, bajo_tocado);
    let mut paradas_b = 0usize;
    let mut paradas_b_bajas = 0usize;
    let mut salida_b: Option<i32> = None;
    for _ in 0..64 {
        if sys_ptrace(PTRACE_CONT, hijo_b as i64, 0, 0) < 0 { break; }
        let r = waitpid(hijo_b as i32, &mut st, WUNTRACED);
        if r != hijo_b as i64 { break; }
        if wifexited(st) { salida_b = Some(wexitstatus(st)); break; }
        if !wifstopped(st) { break; }
        paradas_b += 1;
        if let Some(ti) = leer_trapinfo(hijo_b) {
            if ti.slot < 4 && ti.watch_addr < marco + TAM / 2 { paradas_b_bajas += 1; }
        }
    }
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_b as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_b as i64, 0, 0);
    if salida_b.is_none() { let _ = waitpid(hijo_b as i32, &mut st, 0); }
    println!("[SESSION-BUF-SILENT] con la mitad baja sin vigilar: paradas={} (atribuidas a la mitad baja={}) salida del objetivo={:?}",
             paradas_b, paradas_b_bajas, salida_b);
    let ok_b = paradas_b_bajas == 0 && !bajo_tocado.is_empty() && paradas_b < golpes.len();

    if ok_a && ok_b {
        println!("[SESSION-BUF-PASS] búfer de {} B repartido entre {} ranuras DR0-DR3: cada parada atribuida por el DR6 real", TAM, trozos.len());
        println!("                   al trozo tocado, y los trozos sin ranura no detienen al objetivo");
        0
    } else {
        println!("[SESSION-FAIL] el ensayo del búfer repartido no se completó (completo={} mitad={})", ok_a, ok_b);
        1
    }
}

/// Vigilancias que el kernel confirma para el objetivo: si es el proceso que
/// está en el CPU, corresponden a los registros DR0-DR3 leídos del procesador.
fn leer_watch(pid: u64) -> Vec<(u64, u64, u64)> {
    let mut out = Vec::new();
    let mut buf = [0u8; 41];
    let n = sys_ptrace(PTRACE_GETWATCHPOINTS, pid as i64, 0, buf.as_mut_ptr() as usize);
    if n <= 0 { return out; }
    for i in 0..(n as usize).min(4) {
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[1 + i * 8..9 + i * 8]);
        out.push((u64::from_ne_bytes(b), buf[33 + i] as u64, buf[37 + i] as u64));
    }
    out
}

fn cmd_watch_list(pid: u64) -> i32 {
    let wps = cargar_wps(pid);
    if wps.is_empty() {
        println!("[dbg] la imagen '{}' no tiene vigilancias registradas (archivo {})", clave_imagen(pid), ruta_estado(pid));
        return 0;
    }
    let previos = leer_watch(pid);
    let mut rearmados = 0usize;
    for (addr, len, rw, _n) in wps.iter() {
        if !previos.iter().any(|(a, _, _)| a == addr) {
            if sys_ptrace(PTRACE_SETWATCHPOINT, pid as i64, *addr as usize, watch_cfg(*len, *rw)) >= 0 {
                rearmados += 1;
            }
        }
    }
    let activos = leer_watch(pid);
    println!("PUNTOS DE VIGILANCIA POR HARDWARE DE {} (imagen '{}', registros DR0-DR3)", pid, clave_imagen(pid));
    for (addr, len_code, rw) in activos.iter() {
        println!("  0x{:016x}  longitud={}B  acceso={}  (leído del CPU)", addr,
                 match *len_code { 0 => 1, 1 => 2, 3 => 4, _ => 8 },
                 if *rw == DR7_RW_WRITE { "escritura" } else { "lectura/escritura" });
    }
    println!("[WATCH-VERIFIED] {}/{} vigilancias activas en el objetivo{}",
             activos.len(), wps.len(), if rearmados > 0 { " (rearmadas sobre el programa)" } else { "" });
    if activos.len() >= wps.len() { 0 } else { 1 }
}

fn cmd_watch_clear(pid: u64) -> i32 {
    let r = sys_ptrace(PTRACE_CLEARWATCHPOINTS, pid as i64, 0, 0);
    if r < 0 {
        println!("[WATCH-FAIL] CLEARWATCHPOINTS falló (error {})", r);
        return 1;
    }
    let bps = cargar_bps(pid);
    let _ = guardar_estado(pid, &bps, &[]);
    let restantes = leer_watch(pid);
    println!("[WATCH-CLEARED] vigilancias desarmadas en pid={} (quedan {} activas)", pid, restantes.len());
    0
}

// ═══════════════════════════════════════════════════════════════════════════
//  Inspección: símbolos, volcado hexadecimal y desensamblado simbólico
// ═══════════════════════════════════════════════════════════════════════════

fn cmd_symbols(pid: u64, filtro: Option<&str>) -> i32 {
    let texto = match leer_proc(pid, "symbols") {
        Some(t) => t,
        None => {
            println!("[dbg] /proc/{}/symbols no disponible", pid);
            return 1;
        }
    };
    let mut mostrados = 0;
    for l in texto.lines() {
        if let Some(f) = filtro {
            if !l.contains(f) && !l.starts_with("Tabla") && !l.starts_with("Desplazamiento") && !l.starts_with("DIRECCION") { continue; }
        }
        println!("  {}", l);
        mostrados += 1;
    }
    println!("[SYMBOLS-OK] {} línea(s) publicadas por el kernel para PID {}", mostrados, pid);
    0
}

fn cmd_peek(pid: u64, token: &str, n: usize) -> i32 {
    let mut syms = Vec::new();
    let addr = match resolver(pid, token, &mut syms) {
        Some(a) => a,
        None => {
            println!("[dbg] no se pudo resolver '{}'", token);
            return 1;
        }
    };
    let datos = leer_bytes(pid, addr, n);
    if datos.is_empty() {
        println!("[dbg] PEEKTEXT no devolvió datos en 0x{:x}", addr);
        return 1;
    }
    println!("VOLCADO DE MEMORIA DEL OBJETIVO PID {} EN 0x{:016x} ({} bytes)", pid, addr, datos.len());
    for (i, trozo) in datos.chunks(16).enumerate() {
        let dir = addr + (i * 16) as u64;
        let mut hex = String::new();
        let mut asc = String::new();
        for b in trozo {
            hex.push_str(&format!("{:02x} ", b));
            asc.push(if *b >= 0x20 && *b < 0x7F { *b as char } else { '.' });
        }
        println!("  0x{:016x}  {:<48} |{}|", dir, hex, asc);
    }
    println!("[PEEK-OK] {} bytes leídos del espacio de direcciones del proceso objetivo", datos.len());
    0
}

fn cmd_disasm(pid: u64, token: &str, n: usize) -> i32 {
    let mut syms = Vec::new();
    let mut addr = match resolver(pid, token, &mut syms) {
        Some(a) => a,
        None => {
            println!("[dbg] no se pudo resolver '{}'", token);
            return 1;
        }
    };
    println!("DESENSAMBLADO SIMBOLICO DE PID {} DESDE 0x{:016x} ({} instrucciones)", pid, addr, n);
    if let Some(s) = simbolizar(&syms, addr) {
        println!("  destino: <{}>", s);
    }
    let mut decodificadas = 0;
    for _ in 0..n {
        let bytes = leer_bytes(pid, addr, VENTANA);
        if bytes.is_empty() {
            println!("  0x{:016x}  <memoria no mapeada>", addr);
            break;
        }
        let ins = decodificar(&bytes, addr);
        let mut hex = String::new();
        for b in bytes.iter().take(ins.len.min(8)) {
            hex.push_str(&format!("{:02x} ", b));
        }
        let mut anotacion = String::new();
        if let Some(destino) = ins.destino {
            if let Some(s) = simbolizar(&syms, destino) {
                anotacion = format!("   <{}>", s);
            }
        } else if let Some(rel) = ins.rip_rel {
            let objetivo = (addr as i64 + ins.len as i64 + rel) as u64;
            if let Some(s) = simbolizar(&syms, objetivo) {
                anotacion = format!("   ; [rip] -> 0x{:x} <{}>", objetivo, s);
            }
        }
        println!("  0x{:016x}  {:<26} {}{}", addr, hex, ins.texto, anotacion);
        addr += ins.len.max(1) as u64;
        decodificadas += 1;
    }
    println!("[DISASM-OK] {} instrucciones decodificadas del binario real del proceso", decodificadas);
    0
}

// ═══════════════════════════════════════════════════════════════════════════
//  Inspección de procesos mediante el sistema de archivos /proc
// ═══════════════════════════════════════════════════════════════════════════

fn dump_proc_file(path: &str) -> usize {
    let file = match fs::File::open(path, 0) {
        Ok(f) => f,
        Err(e) => {
            println!("  [dbg] {}: no disponible (error {})", path, e);
            return 0;
        }
    };
    let mut buf = [0u8; 4096];
    let mut lines = 0usize;
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
                for line in text.split('\n') {
                    let l = line.trim_end_matches('\r');
                    if !l.is_empty() { println!("  {}", l); lines += 1; }
                }
            }
            Err(_) => break,
        }
    }
    lines
}

fn dump_profiler_for(pid: u64) -> bool {
    let file = match fs::File::open("/proc/profiler", 0) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 4096];
    let needle = format!("    {} ", pid);
    let mut found = false;
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
                for line in text.split('\n') {
                    let l = line.trim_end_matches('\r');
                    if l.starts_with(&needle) || l.contains("Muestras totales")
                        || l.contains("Granularidad") || l.contains("Estado ")
                    {
                        println!("  {}", l);
                        if l.starts_with(&needle) { found = true; }
                    }
                }
            }
            Err(_) => break,
        }
    }
    found
}

fn inspect_process(pid: u64, what: &str, extra: &[String]) -> i32 {
    let mut code = 0;
    match what {
        "all" | "" => {
            println!("[dbg] Inspeccion de PID={} (estado, cmdline, mapa de memoria)", pid);
            if dump_proc_file(&format!("/proc/{}/status", pid)) == 0 { code = 1; }
            let _ = dump_proc_file(&format!("/proc/{}/cmdline", pid));
            if dump_proc_file(&format!("/proc/{}/maps", pid)) == 0 { code = 1; }
            let syms = leer_simbolos(pid);
            println!("[dbg] Simbolos publicados por el kernel: {}", syms.len());
            println!("[dbg] Perfil de CPU del profiler:");
            if !dump_profiler_for(pid) { println!("  (sin muestras acumuladas para este PID)"); }
        }
        "status" => { if dump_proc_file(&format!("/proc/{}/status", pid)) == 0 { code = 1; } }
        "cmdline" => { if dump_proc_file(&format!("/proc/{}/cmdline", pid)) == 0 { code = 1; } }
        "maps" => { if dump_proc_file(&format!("/proc/{}/maps", pid)) == 0 { code = 1; } }
        "symbols" | "syms" | "sym" => { code = cmd_symbols(pid, extra.first().map(|s| s.as_str())); }
        "peek" => {
            let tok = extra.first().map(|s| s.as_str()).unwrap_or("0x0");
            let n = extra.get(1).and_then(|s| parse_u64(s)).unwrap_or(64) as usize;
            code = cmd_peek(pid, tok, n.clamp(8, 512));
        }
        "disasm" | "dis" => {
            let tok = extra.first().map(|s| s.as_str()).unwrap_or("main");
            let n = extra.get(1).and_then(|s| parse_u64(s)).unwrap_or(12) as usize;
            code = cmd_disasm(pid, tok, n.clamp(1, 200));
        }
        "profiler" | "profile" => {
            println!("[dbg] Perfil de CPU de PID={} (/proc/profiler)", pid);
            if !dump_profiler_for(pid) { println!("  (sin muestras para este PID)"); }
        }
        "syscalls" | "trace" => {
            println!("[dbg] Trazas de llamadas al sistema (/proc/syscalls)");
            let _ = dump_proc_file("/proc/syscalls");
        }
        other => {
            println!("[dbg] Accion desconocida: '{}'", other);
            println!("[dbg] Acciones: status | cmdline | maps | symbols [filtro] | peek <dir> [n]");
            println!("[dbg]           disasm <simbolo|dir> [n] | profiler | syscalls");
            code = 1;
        }
    }
    code
}

// ═══════════════════════════════════════════════════════════════════════════
//  Sesión de depuración interactiva (modelo `gdb`): bifurcar, trazar y parar
//
//  Secuencia verificable end-to-end:
//    1. `fork` + `PTRACE_TRACEME` en el hijo + `execve` del programa objetivo;
//    2. PARADA DE EXECVE notificada al trazador (waitpid -> WIFSTOPPED);
//    3. el trazador lee los registros del objetivo detenido y resuelve `main`
//       con la tabla de símbolos que el kernel publica en /proc/<pid>/symbols;
//    4. instala un INT3 (0xCC) en la imagen y CONTINÚA;
//    5. el objetivo se detiene al alcanzarlo: parada notificada con SIGTRAP,
//       RIP corregido al byte siguiente y simbolizado;
//    6. PASO A PASO: una instrucción y otra parada; el avance del RIP debe
//       coincidir con la longitud de la instrucción decodificada;
//    7. restauración del byte original, desenganche y salida real del hijo.
// ═══════════════════════════════════════════════════════════════════════════

/// Ejecuta el hijo trazado y devuelve su código de salida, o -1 si el exec falló.
fn hijo_trazado(programa: &str) -> i32 {
    // El hijo pide ser trazado ANTES de reemplazar su imagen: así el `execve`
    // provoca la parada inicial que el trazador observa.
    let _ = sys_ptrace(PTRACE_TRACEME, 0, 0, 0);
    let mut path = String::from(programa);
    if !path.starts_with('/') { path = format!("/bin/{}", programa); }
    path.push('\0');
    let argv: [*const u8; 2] = [path.as_ptr(), core::ptr::null()];
    let r = portix_rt::syscall::sys_execve(path.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
    println!("[dbg] el hijo no pudo ejecutar '{}' (error {})", programa, r);
    127
}

fn esperar_parada(pid: u64, status: &mut i32, etiqueta: &str) -> bool {
    let r = waitpid(pid as i32, status, WUNTRACED);
    let st = *status;
    let ok = r == pid as i64 && wifstopped(st);
    println!("[dbg] {}: waitpid -> pid={} status=0x{:04x} ({}: señal={})", etiqueta, r, st,
             if wifstopped(st) { "DETENIDO" } else if wifexited(st) { "TERMINADO" } else { "otro" },
             if wifstopped(st) { wstopsig(st) } else { 0 });
    ok
}

// ═══════════════════════════════════════════════════════════════════════════
//  Control de ejecución del objetivo: paso a paso, paso sobre llamada y pila
// ═══════════════════════════════════════════════════════════════════════════

/// Rango del segmento de código de las imágenes de usuario y de su pila (ver
/// `elf.rs`: el texto se carga en 0x2000_0000 y la pila crece bajo STACK_TOP).
/// Se usan para validar los candidatos de retorno en el backtrace.
const TEXTO_BASE: u64 = 0x2000_0000;
const TEXTO_FIN: u64 = 0x2100_0000;
const PILA_BASE: u64 = 0x7EFF_FFF0_0000;
const PILA_FIN: u64 = 0x7F00_0000_0000;

fn es_codigo(a: u64) -> bool { a >= TEXTO_BASE && a < TEXTO_FIN }
fn es_pila(a: u64) -> bool { a >= PILA_BASE && a < PILA_FIN }

fn instruccion_en(pid: u64, addr: u64) -> Instruccion {
    let bytes = leer_bytes(pid, addr, VENTANA);
    decodificar(&bytes, addr)
}

/// Ejecuta EXACTAMENTE una instrucción del objetivo usando el Trap Flag real
/// del procesador (`PTRACE_SINGLESTEP` escribe TF en el frame IRET) y relata la
/// instrucción ejecutada, con su símbolo y su longitud.
fn paso_instruccion(hijo: u64, st: &mut i32, syms: &[Simbolo]) -> bool {
    let mut r0 = UserRegs::default();
    if sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut r0 as *mut UserRegs as usize) < 0 {
        return false;
    }
    let ins = instruccion_en(hijo, r0.rip);
    if sys_ptrace(PTRACE_SINGLESTEP, hijo as i64, 0, 0) < 0 {
        println!("[SESSION-FAIL] PTRACE_SINGLESTEP rechazado");
        return false;
    }
    if !esperar_parada(hijo, st, "paso") {
        println!("[SESSION-FAIL] el objetivo no se detuvo tras el paso");
        return false;
    }
    let mut r1 = UserRegs::default();
    let _ = sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut r1 as *mut UserRegs as usize);
    let nombre = simbolizar(syms, r0.rip).unwrap_or_else(|| String::from("?"));
    println!("[SESSION-STEP] 0x{:016x} <{}>: {} ({} B) -> RIP=0x{:016x}",
             r0.rip, nombre, ins.texto, ins.len, r1.rip);
    true
}

/// Paso a NIVEL de llamada (`next`): si la instrucción actual es un `call`, se
/// planta un breakpoint temporal en la dirección de RETORNO y se reanuda, de
/// modo que la llamada se ejecuta completa y el objetivo se detiene al volver.
/// Si no es una llamada, se comporta como un paso normal.
fn paso_sobre_llamada(hijo: u64, st: &mut i32, syms: &[Simbolo]) -> bool {
    let mut r0 = UserRegs::default();
    if sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut r0 as *mut UserRegs as usize) < 0 {
        return false;
    }
    let ins = instruccion_en(hijo, r0.rip);
    if !ins.texto.starts_with("call") {
        println!("[SESSION-NEXT] 0x{:016x}: {} no es una llamada -> paso simple", r0.rip, ins.texto);
        return paso_instruccion(hijo, st, syms);
    }
    let retorno = r0.rip + ins.len as u64;
    let orig = peek_u64(hijo, retorno).map(|v| (v & 0xFF) as u8).unwrap_or(0);
    if !poke_u64(hijo, retorno, (peek_u64(hijo, retorno).unwrap_or(0) & !0xFF) | 0xCC) {
        println!("[SESSION-FAIL] no se pudo plantar el breakpoint temporal en 0x{:016x}", retorno);
        return false;
    }
    println!("[SESSION-NEXT] 0x{:016x}: {} ({} B) -> breakpoint temporal en el retorno 0x{:016x}",
             r0.rip, ins.texto, ins.len, retorno);
    if sys_ptrace(PTRACE_CONT, hijo as i64, 0, 0) < 0 {
        return false;
    }
    if !esperar_parada(hijo, st, "retorno de llamada") {
        println!("[SESSION-FAIL] el objetivo no se detuvo al retornar de la llamada");
        return false;
    }
    // El breakpoint temporal se retira SIEMPRE: no dejar basura en el código del
    // objetivo es la regla de oro de un depurador.
    let mut r1 = UserRegs::default();
    let _ = sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut r1 as *mut UserRegs as usize);
    if let Some(v) = peek_u64(hijo, retorno) {
        let _ = poke_u64(hijo, retorno, (v & !0xFF) | orig as u64);
    }
    if r1.rip == retorno + 1 {
        r1.rip = retorno;
        let _ = sys_ptrace(PTRACE_SETREGS, hijo as i64, 0, &r1 as *const UserRegs as usize);
        let nombre = simbolizar(syms, r1.rip).unwrap_or_else(|| String::from("?"));
        println!("[SESSION-NEXT] llamada completada: parada en el retorno 0x{:016x} <{}> (byte temporal restaurado, RIP rebobinado)",
                 retorno, nombre);
    } else {
        println!("[SESSION-NEXT] el objetivo se detuvo en 0x{:016x} y no en el retorno 0x{:016x} (byte temporal restaurado)",
                 r1.rip, retorno);
    }
    true
}

/// Pila de llamadas del objetivo. Primero intenta la cadena clásica de frame
/// pointers (`RBP` -> [RBP] = RBP anterior, [RBP+8] = dirección de retorno); si
/// el código no conserva marco (habitual en binarios optimizados), barre el
/// marco de pila buscando valores que caigan en el segmento de código. Se
/// declara siempre qué estrategia se usó: un backtrace heurístico no debe
/// disfrazarse de exacto.
fn backtrace(hijo: u64, syms: &[Simbolo], max: usize) -> usize {
    let mut r = UserRegs::default();
    if sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut r as *mut UserRegs as usize) < 0 {
        return 0;
    }
    println!("[SESSION-BT] pila de llamadas del objetivo (RIP=0x{:016x} RSP=0x{:016x} RBP=0x{:016x})",
             r.rip, r.rsp, r.rbp);
    let nombre_de = |a: u64| simbolizar(syms, a).unwrap_or_else(|| String::from("?"));
    let mut marcos = 1usize;

    let cadena_valida = es_pila(r.rbp)
        && peek_u64(hijo, r.rbp + 8).map(es_codigo).unwrap_or(false)
        && r.rbp >= r.rsp;
    if !cadena_valida {
        println!("[SESSION-BT] estrategia: barrido del marco de pila (el objetivo no conserva frame pointer)");
        println!("[SESSION-BT]   #0  0x{:016x} <{}>  (punto de parada)", r.rip, nombre_de(r.rip));
        let mut off = 0u64;
        while off < 512 && marcos < max {
            if let Some(v) = peek_u64(hijo, r.rsp + off) {
                if es_codigo(v) && v != r.rip {
                    println!("[SESSION-BT]   #{}  0x{:016x} <{}>  (pila+0x{:x})", marcos, v, nombre_de(v), off);
                    marcos += 1;
                }
            }
            off += 8;
        }
        if marcos == 1 {
            println!("[SESSION-BT]   (sin direcciones de retorno reconocibles en el marco actual)");
        }
        return marcos;
    }

    println!("[SESSION-BT] estrategia: cadena de frame pointers (RBP)");
    println!("[SESSION-BT]   #0  0x{:016x} <{}>  (punto de parada)", r.rip, nombre_de(r.rip));
    let mut rbp = r.rbp;
    while marcos < max && es_pila(rbp) {
        let ret = match peek_u64(hijo, rbp + 8) { Some(v) => v, None => break };
        if !es_codigo(ret) { break; }
        println!("[SESSION-BT]   #{}  0x{:016x} <{}>  (marco RBP=0x{:016x})", marcos, ret, nombre_de(ret), rbp);
        marcos += 1;
        match peek_u64(hijo, rbp) {
            Some(sig) if sig > rbp && es_pila(sig) => rbp = sig,
            _ => break,
        }
    }
    marcos
}

/// Intérprete de comandos del depurador: la misma superficie que un depurador
/// de consola — paso a paso, paso a nivel de llamada, pila de llamadas,
/// registros, memoria, continuar y salir.
fn ejecutar_comandos(
    hijo: u64, st: &mut i32, syms: &[Simbolo], regs_bp: &UserRegs, lista: &str,
) -> usize {
    let mut ejecutados = 0usize;
    for bruto in lista.split(',') {
        let cmd = bruto.trim();
        if cmd.is_empty() { continue; }
        let partes: Vec<&str> = cmd.split_whitespace().collect();
        println!("[SESSION-CMD] >> {}", cmd);
        match partes[0] {
            "step" | "s" => { if paso_instruccion(hijo, st, syms) { ejecutados += 1; } }
            "next" | "n" => { if paso_sobre_llamada(hijo, st, syms) { ejecutados += 1; } }
            "bt" => { if backtrace(hijo, syms, 8) > 0 { ejecutados += 1; } }
            "cond" => {
                // cond <registro> <op> <valor>: la condición se aplica al punto
                // de parada de la sesión y la evalúa el kernel en cada disparo.
                if partes.len() >= 4 {
                    let r = reg_de_nombre(partes[1]);
                    let o = op_de_nombre(partes[2]);
                    let v = parse_u64(partes[3]);
                    match (r, o, v) {
                        (Some(r), Some(o), Some(v)) => {
                            if poner_condicion(hijo, regs_bp.rip, r, o, v) {
                                println!("[SESSION-COND] condición activa en 0x{:016x}: {} {} {:#x}",
                                         regs_bp.rip, nombre_de_reg(r), nombre_de_op(o), v);
                                ejecutados += 1;
                            } else {
                                println!("[SESSION-FAIL] el kernel rechazó la condición");
                            }
                        }
                        _ => println!("[SESSION-CMD] uso: cond <registro> <op> <valor>"),
                    }
                } else {
                    println!("[SESSION-CMD] uso: cond <registro> <op> <valor>");
                }
            }
            "wcond" => {
                // wcond <dirección> <registro> <op> <valor> [ignore N]
                if partes.len() >= 5 {
                    let (a, r, o, v) = (parse_u64(partes[1]), reg_de_nombre(partes[2]),
                                        op_de_nombre(partes[3]), parse_u64(partes[4]));
                    match (a, r, o, v) {
                        (Some(a), Some(r), Some(o), Some(v)) => {
                            if armar_watch_cond(hijo, a, r, o, v) {
                                if partes.len() >= 7 && partes[5] == "ignore" {
                                    if let Some(n) = parse_u64(partes[6]) {
                                        let _ = poner_ignore(hijo, a, n as u32);
                                        println!("[SESSION-WCOND] ignore {} aplicado a 0x{:016x}", n, a);
                                    }
                                }
                                println!("[SESSION-WCOND] vigilancia de escritura en 0x{:016x} si {} {} {:#x}", a, nombre_de_reg(r), nombre_de_op(o), v);
                                ejecutados += 1;
                            } else {
                                println!("[SESSION-FAIL] el kernel rechazó el watchpoint condicional");
                            }
                        }
                        _ => println!("[SESSION-CMD] uso: wcond <dirección> <registro> <op> <valor> [ignore N]"),
                    }
                } else {
                    println!("[SESSION-CMD] uso: wcond <dirección> <registro> <op> <valor> [ignore N]");
                }
            }
            "ignore" => {
                if partes.len() >= 3 {
                    if let (Some(a), Some(n)) = (parse_u64(partes[1]), parse_u64(partes[2])) {
                        if poner_ignore(hijo, a, n as u32) {
                            println!("[SESSION-IGNORE] 0x{:016x}: los primeros {} aciertos no despertarán al trazador", a, n);
                            ejecutados += 1;
                        } else {
                            println!("[SESSION-FAIL] esa dirección no tiene condición registrada");
                        }
                    }
                } else {
                    println!("[SESSION-CMD] uso: ignore <dirección> <n>");
                }
            }
            "range" => {
                // range <registro|addr> <min> <max>
                if partes.len() >= 4 {
                    let (r, lo, hi) = (reg_de_nombre(partes[1]), parse_u64(partes[2]), parse_u64(partes[3]));
                    match (r, lo, hi) {
                        (Some(r), Some(lo), Some(hi)) => {
                            if poner_rango(hijo, regs_bp.rip, r, lo, hi) {
                                println!("[SESSION-RANGE] 0x{:016x}: {} <= {} < {:#x}", regs_bp.rip, nombre_de_reg(r), "valor", hi);
                                ejecutados += 1;
                            } else {
                                println!("[SESSION-FAIL] el kernel rechazó el rango");
                            }
                        }
                        _ => println!("[SESSION-CMD] uso: range <registro|addr> <min> <max>"),
                    }
                } else {
                    println!("[SESSION-CMD] uso: range <registro|addr> <min> <max>");
                }
            }
            "cond2" => {
                // cond2 <reg> <op> <val> <reg> <op> <val>
                if partes.len() >= 7 {
                    let (a, b, c) = (reg_de_nombre(partes[1]), op_de_nombre(partes[2]), parse_u64(partes[3]));
                    let (d, e, f) = (reg_de_nombre(partes[4]), op_de_nombre(partes[5]), parse_u64(partes[6]));
                    match (a, b, c, d, e, f) {
                        (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) => {
                            if poner_condicion2(hijo, regs_bp.rip, a, b, c, d, e, f) {
                                println!("[SESSION-COND2] 0x{:016x}: {} {} {:#x} && {} {} {:#x}",
                                         regs_bp.rip, nombre_de_reg(a), nombre_de_op(b), c,
                                         nombre_de_reg(d), nombre_de_op(e), f);
                                ejecutados += 1;
                            } else {
                                println!("[SESSION-FAIL] el kernel rechazó la conjunción");
                            }
                        }
                        _ => println!("[SESSION-CMD] uso: cond2 <reg> <op> <val> <reg> <op> <val>"),
                    }
                } else {
                    println!("[SESSION-CMD] uso: cond2 <reg> <op> <val> <reg> <op> <val>");
                }
            }
            "condn" => {
                // condn <n> <g2> <reg> <op> <valor> [<reg> <op> <valor> ...]
                if partes.len() >= 6 {
                    let n = parse_u64(partes[1]).unwrap_or(1) as u32;
                    let g2 = parse_u64(partes[2]).unwrap_or(0) as u32;
                    let mut cl = [CondClause::default(); 4];
                    let mut ok = true;
                    for c in 0..(n.min(4) as usize) {
                        let base = 3 + c * 3;
                        if base + 2 >= partes.len() { ok = false; break; }
                        match (reg_de_nombre(partes[base]), op_de_nombre(partes[base + 1]), parse_u64(partes[base + 2])) {
                            (Some(r), Some(o), Some(v)) => cl[c] = CondClause { reg: r, op: o, value: v },
                            _ => { ok = false; break; }
                        }
                    }
                    if ok && poner_condicion_n(hijo, regs_bp.rip, n, g2, &cl) {
                        if let Some(x) = leer_condicion(hijo, regs_bp.rip) {
                            println!("[SESSION-CONDN] 0x{:016x}: {}", regs_bp.rip, describir_condicion(&x));
                        }
                        ejecutados += 1;
                    } else {
                        println!("[SESSION-FAIL] el kernel rechazó la condición general");
                    }
                } else {
                    println!("[SESSION-CMD] uso: condn <n> <g2> <reg> <op> <valor> [<reg> <op> <valor> ...]");
                }
            }
            "conds" => {
                let mut n = 0;
                let bps = cargar_bps(hijo);
                for (addr, _, sim) in bps.iter() {
                    if let Some(st) = leer_condicion(hijo, *addr) {
                        println!("[SESSION-CONDS] 0x{:016x} <{}> if {} {} {:#x}  aciertos={} saltos={}",
                                 addr, sim, nombre_de_reg(st.clauses[0].reg), nombre_de_op(st.clauses[0].op), st.clauses[0].value, st.hits, st.skips);
                        n += 1;
                    }
                }
                if n == 0 { println!("[SESSION-CONDS] (sin condiciones activas)"); }
                ejecutados += 1;
            }
            "regs" | "r" => {
                let mut r = *regs_bp;
                if sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut r as *mut UserRegs as usize) == 0 {
                    print_registers(&r);
                    ejecutados += 1;
                }
            }
            "mem" | "m" => {
                if partes.len() >= 2 {
                    if let Some(a) = parse_u64(partes[1]) {
                        let n = if partes.len() >= 3 { parse_u64(partes[2]).unwrap_or(32) as usize } else { 32 };
                        let bytes = leer_bytes(hijo, a, n);
                        println!("[SESSION-MEM] 0x{:016x} ({} B)", a, bytes.len());
                        for (i, b) in bytes.iter().enumerate() {
                            if i % 16 == 0 { print!("  0x{:016x}: ", a + i as u64); }
                            print!("{:02x} ", b);
                            if i % 16 == 15 { println!(""); }
                        }
                        if bytes.len() % 16 != 0 { println!(""); }
                        ejecutados += 1;
                    }
                } else {
                    println!("[SESSION-CMD] uso: mem <dirección> [bytes]");
                }
            }
            "cont" | "c" => {
                if sys_ptrace(PTRACE_CONT, hijo as i64, 0, 0) == 0 && esperar_parada(hijo, st, "continuar") {
                    ejecutados += 1;
                }
            }
            "quit" | "q" => {
                println!("[SESSION-CMD] salida solicitada");
                break;
            }
            otro => println!("[SESSION-CMD] comando desconocido: {} (step|next|bt|regs|mem|cont|quit)", otro),
        }
    }
    ejecutados
}

// ═══════════════════════════════════════════════════════════════════════════
//  Breakpoints condicionales (`break ... if`) evaluados por el KERNEL
// ═══════════════════════════════════════════════════════════════════════════

/// Nombres de registro que acepta la línea de comandos y la sesión.
fn reg_de_nombre(n: &str) -> Option<u32> {
    Some(match n {
        "rax" => CREG_RAX, "rbx" => CREG_RBX, "rcx" => CREG_RCX, "rdx" => CREG_RDX,
        "rsi" => CREG_RSI, "rdi" => CREG_RDI, "rbp" => CREG_RBP, "rsp" => CREG_RSP,
        "r8" => CREG_R8, "r9" => CREG_R9, "r10" => CREG_R10, "r11" => CREG_R11,
        "r12" => CREG_R12, "r13" => CREG_R13, "r14" => CREG_R14, "r15" => CREG_R15,
        "rip" => CREG_RIP, "rflags" => CREG_RFLAGS, "addr" => CREG_ADDR,
        _ => return None,
    })
}

fn nombre_de_reg(r: u32) -> &'static str {
    match r {
        CREG_RAX => "rax", CREG_RBX => "rbx", CREG_RCX => "rcx", CREG_RDX => "rdx",
        CREG_RSI => "rsi", CREG_RDI => "rdi", CREG_RBP => "rbp", CREG_RSP => "rsp",
        CREG_R8 => "r8", CREG_R9 => "r9", CREG_R10 => "r10", CREG_R11 => "r11",
        CREG_R12 => "r12", CREG_R13 => "r13", CREG_R14 => "r14", CREG_R15 => "r15",
        CREG_RIP => "rip", CREG_RFLAGS => "rflags", CREG_ADDR => "addr",
        _ => "?",
    }
}

fn op_de_nombre(n: &str) -> Option<u32> {
    Some(match n {
        "==" | "eq" => COP_EQ, "!=" | "ne" => COP_NE,
        "<" | "lt" => COP_LT, "<=" | "le" => COP_LE,
        ">" | "gt" => COP_GT, ">=" | "ge" => COP_GE,
        _ => return None,
    })
}

fn nombre_de_op(o: u32) -> &'static str {
    match o {
        COP_EQ => "==", COP_NE => "!=", COP_LT => "<",
        COP_LE => "<=", COP_GT => ">", COP_GE => ">=",
        _ => "?",
    }
}

/// Instala una condición sobre un punto de parada. Devuelve el índice si el
/// kernel la aceptó.
fn poner_condicion(hijo: u64, addr: u64, reg: u32, op: u32, valor: u64) -> bool {
    let spec = BpCond { reg, op, value: valor };
    sys_ptrace(PTRACE_SETBPCOND, hijo as i64, addr as usize,
               &spec as *const BpCond as usize) >= 0
}

fn quitar_condicion(hijo: u64, addr: u64) -> bool {
    sys_ptrace(PTRACE_CLEARBPCOND, hijo as i64, addr as usize, 0) == 0
}

fn leer_condicion(hijo: u64, addr: u64) -> Option<BpCondStat> {
    let mut st = BpCondStat::default();
    if sys_ptrace(PTRACE_GETBPCOND, hijo as i64, addr as usize,
                  &mut st as *mut BpCondStat as usize) == 0 { Some(st) } else { None }
}

/// `--bcond <pid> <dirección|símbolo> <registro> <operador> <valor>`
fn cmd_bcond(pid: u64, token: &str, reg: &str, op: &str, valor: &str) -> i32 {
    let mut syms = leer_simbolos(pid);
    let addr = match resolver(pid, token, &mut syms) {
        Some(a) => a,
        None => { println!("[dbg] no se pudo resolver '{}'", token); return 1; }
    };
    let r = match reg_de_nombre(reg) { Some(v) => v, None => { println!("[dbg] registro inválido: {}", reg); return 1; } };
    let o = match op_de_nombre(op) { Some(v) => v, None => { println!("[dbg] operador inválido: {}", op); return 1; } };
    let v = match parse_u64(valor) { Some(v) => v, None => { println!("[dbg] valor inválido: {}", valor); return 1; } };
    if !poner_condicion(pid, addr, r, o, v) {
        println!("[dbg] el kernel rechazó la condición en 0x{:016x}", addr);
        return 1;
    }
    println!("[BPCOND-SET] 0x{:016x} condicionada a {} {} {:#x} (evaluada por el kernel)",
             addr, nombre_de_reg(r), nombre_de_op(o), v);
    match leer_condicion(pid, addr) {
        Some(st) => {
            println!("[BPCOND-VERIFIED] 0x{:016x} {} {} {:#x} hits={} skips={}",
                     addr, nombre_de_reg(st.clauses[0].reg), nombre_de_op(st.clauses[0].op), st.clauses[0].value, st.hits, st.skips);
            0
        }
        None => { println!("[dbg] la condición no se pudo releer"); 1 }
    }
}

fn cmd_bcond_list(pid: u64) -> i32 {
    let mut n = 0;
    for p in listar_procesos() {
        if p.0 == pid {
            println!("CONDICIONES DE PARADA DEL PROCESO {} ({})", pid, p.1);
            n = 1;
        }
    }
    if n == 0 { println!("[dbg] proceso {} no encontrado", pid); return 1; }
    // Las condiciones viven en el kernel: se enumeran consultando los puntos de
    // parada registrados en la imagen y preguntando por su estado.
    let bps = cargar_bps(pid);
    let mut activas = 0;
    for (addr, byte, sim) in bps.iter() {
        if let Some(st) = leer_condicion(pid, *addr) {
            println!("  0x{:016x} <{}>  if {} {} {:#x}   aciertos={} saltos={}  (byte {:#04x})",
                     addr, sim, nombre_de_reg(st.clauses[0].reg), nombre_de_op(st.clauses[0].op), st.clauses[0].value, st.hits, st.skips, byte);
            activas += 1;
        }
    }
    if activas == 0 {
        println!("  (ninguna: `dbg --bcond <pid> <dirección> <registro> <op> <valor>`)");
    }
    println!("[BPCOND-LIST] condiciones activas={}", activas);
    if activas > 0 { 0 } else { 1 }
}

fn cmd_bcond_clear(pid: u64, token: &str) -> i32 {
    let mut syms = leer_simbolos(pid);
    let addr = match resolver(pid, token, &mut syms) {
        Some(a) => a,
        None => { println!("[dbg] no se pudo resolver '{}'", token); return 1; }
    };
    if quitar_condicion(pid, addr) {
        println!("[BPCOND-CLEARED] 0x{:016x} vuelve a detener al objetivo en cada disparo", addr);
        0
    } else {
        println!("[dbg] no había condición en 0x{:016x}", addr);
        1
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Watchpoints con expresión: la condición se aplica a los ACCESOS DE DATOS
//  vigilados por el hardware (DR0-DR3), no al código. El kernel evalúa la misma
//  condición con los registros del frame y decide si despierta al trazador.
// ═══════════════════════════════════════════════════════════════════════════

/// `ignore N` sobre una condición ya definida.
fn poner_ignore(hijo: u64, addr: u64, n: u32) -> bool {
    sys_ptrace(PTRACE_SETBPCIGNORE, hijo as i64, addr as usize, n as usize) >= 0
}

/// Arma una vigilancia de escritura (DR, 8 bytes) con una condición.
fn armar_watch_cond(hijo: u64, addr: u64, reg: u32, op: u32, valor: u64) -> bool {
    sys_ptrace(PTRACE_SETWATCHPOINT, hijo as i64, addr as usize,
               watch_cfg(8, DR7_RW_WRITE)) >= 0
        && poner_condicion(hijo, addr, reg, op, valor)
}

/// Deriva del CÓDIGO REAL del objetivo la dirección de datos que su función
/// principal va a escribir: lee la primera instrucción de `<main>` (que reserva
/// el marco) y calcula `RSP de entrada a main - reserva`. Nada se supone: la
/// dirección vigilada sale del propio programa.
fn derivar_dato_escrito(hijo: u64, rsp_entrada: u64, main_addr: u64, syms: &[Simbolo]) -> Option<u64> {
    let ins = instruccion_en(hijo, main_addr);
    println!("[SESSION-WCOND-SCAN] primera instrucción de <main>: {} ({} B)", ins.texto, ins.len);
    let mut reserva = 0x18u64; // valor observado en los binarios del SDK
    if let Some(resto) = ins.texto.strip_prefix("sub rsp, ") {
        if let Some(v) = parse_u64(resto.trim()) {
            reserva = v;
        }
    }
    // En el punto de entrada el objetivo está en `_start`: el `call main` ya
    // empujó su dirección de retorno, así que el RSP que verá `main` es el de
    // aquí menos 8, y la reserva del marco viene de la propia instrucción.
    let rsp_main = rsp_entrada - 8;
    let dato = rsp_main - reserva;
    if !es_pila(dato) {
        println!("[SESSION-FAIL] la dirección derivada 0x{:016x} no está en la pila del objetivo", dato);
        return None;
    }
    let dueño = simbolizar(syms, main_addr).unwrap_or_else(|| String::from("main"));
    println!("[SESSION-WCOND-SCAN] RSP al entrar en <{}>: 0x{:016x}; la reserva deja el marco en 0x{:016x}",
             dueño, rsp_main, dato);
    Some(dato)
}

/// Ensayo de watchpoints con expresión (`dbg --wcondtest`). Tres fases sobre
/// objetivos recién ejecutados, todas verificadas por los contadores del kernel:
///   A. condición FALSA  -> el kernel calla y el objetivo termina solo;
///   B. condición VERDADERA + condición encadenada falsa -> el trazador despierta
///      sólo por el watchpoint, y el valor observado en memoria es el esperado;
///   C. `ignore 1` con condición verdadera -> el acierto se consume sin notificar.
fn cmd_wcondtest(programa: &str) -> i32 {
    println!("ENSAYO DE WATCHPOINTS CON EXPRESIÓN — objetivo: {}", programa);
    let mut st = 0i32;

    // ── Fase 0: descubrir QUÉ vigilar a partir del código del objetivo ──────
    let hijo0 = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let mut syms = leer_simbolos(hijo0);
    let main = match resolver(hijo0, "main", &mut syms) {
        Some(a) => a,
        None => { println!("[SESSION-FAIL] no se pudo resolver 'main'"); return 1; }
    };
    let mut r0 = UserRegs::default();
    if sys_ptrace(PTRACE_GETREGS, hijo0 as i64, 0, &mut r0 as *mut UserRegs as usize) < 0 { return 1; }
    let dato = match derivar_dato_escrito(hijo0, r0.rsp, main, &syms) { Some(d) => d, None => return 1 };
    let _ = sys_ptrace(PTRACE_DETACH, hijo0 as i64, 0, 0);
    let _ = waitpid(hijo0 as i32, &mut st, 0);

    // ── Fase A: condición falsa ──────────────────────────────────────────────
    let hijo_a = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let pid_a = hijo_a;
    if !armar_watch_cond(hijo_a, dato, CREG_RAX, COP_EQ, pid_a + 0x1000) {
        println!("[SESSION-FAIL] no se pudo armar el watchpoint condicional");
        return 1;
    }
    println!("[SESSION-WCOND-ARMED] vigilancia de ESCRITURA en 0x{:016x} con condición FALSA (rax == {:#x})", dato, pid_a + 0x1000);
    if sys_ptrace(PTRACE_CONT, hijo_a as i64, 0, 0) < 0 { return 1; }
    let _ = waitpid(hijo_a as i32, &mut st, WUNTRACED);
    let st_a = st;
    let ok_falsa = wifexited(st_a);
    println!("[SESSION-WCOND-SKIP] condición falsa: el objetivo terminó solo (código {}) sin despertar al trazador",
             if ok_falsa { wexitstatus(st_a) } else { -1 });
    if !ok_falsa {
        println!("[SESSION-FAIL] el objetivo no debía detenerse con la condición falsa (status=0x{:04x})", st_a);
        return 1;
    }

    // ── Fase B: condición verdadera + condición encadenada ──────────────────
    let hijo_b = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let pid_b = hijo_b;
    // (1) el watchpoint con la condición verdadera: rax == PID del objetivo
    if !armar_watch_cond(hijo_b, dato, CREG_RAX, COP_EQ, pid_b) {
        println!("[SESSION-FAIL] no se pudo armar el watchpoint condicional");
        return 1;
    }
    // (2) condición ENCADENADA: punto de parada de ejecución en <main> con una
    // condición imposible. Debe ser evaluada y descartada por el kernel mientras
    // el watchpoint es el que despierta al trazador.
    if sys_ptrace(PTRACE_SETWATCHPOINT, hijo_b as i64, main as usize, watch_cfg(1, DR7_RW_EXEC)) < 0
        || !poner_condicion(hijo_b, main, CREG_RSP, COP_EQ, 0) {
        println!("[SESSION-FAIL] no se pudo encadenar la segunda condición");
        return 1;
    }
    println!("[SESSION-WCOND-CHAIN] dos condiciones activas a la vez: watchpoint 0x{:016x} (rax == {:#x}, verdadera)", dato, pid_b);
    println!("                        y punto de parada de ejecución 0x{:016x} (rsp == 0, falsa)", main);
    if sys_ptrace(PTRACE_CONT, hijo_b as i64, 0, 0) < 0 { return 1; }
    let r_b = waitpid(hijo_b as i32, &mut st, WUNTRACED);
    let st_b = st;
    if !(r_b == hijo_b as i64 && wifstopped(st_b)) {
        println!("[SESSION-FAIL] con la condición verdadera el objetivo debía detenerse (status=0x{:04x})", st_b);
        return 1;
    }
    let mut regs_b = UserRegs::default();
    let _ = sys_ptrace(PTRACE_GETREGS, hijo_b as i64, 0, &mut regs_b as *mut UserRegs as usize);
    let observado = peek_u64(hijo_b, dato).unwrap_or(0);
    println!("[SESSION-WCOND-HIT] el trazador despertó por el WATCHPOINT: RIP=0x{:016x} <{}> rax={:#x}",
             regs_b.rip, simbolizar(&syms, regs_b.rip.wrapping_sub(1)).unwrap_or_else(|| String::from("?")), regs_b.rax);
    println!("[SESSION-WCOND-VALUE] 0x{:016x} contiene {:#x} (PID del objetivo = {}) -> escritura observada",
             dato, observado, pid_b);
    let st_wp = leer_condicion(hijo_b, dato);
    let st_code = leer_condicion(hijo_b, main);
    if let Some(x) = st_wp {
        println!("[SESSION-WCOND-STATS] watchpoint: aciertos={} saltos={} (contadores del KERNEL)", x.hits, x.skips);
    }
    if let Some(x) = st_code {
        println!("[SESSION-WCOND-STATS] condición encadenada (código): aciertos={} saltos={} (descartada por el kernel)", x.hits, x.skips);
    }
    let ok_hit = st_wp.map(|x| x.hits >= 1).unwrap_or(false) && observado == pid_b;
    // Cierre de la fase B
    let _ = quitar_condicion(hijo_b, dato);
    let _ = quitar_condicion(hijo_b, main);
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_b as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_b as i64, 0, 0);
    let _ = waitpid(hijo_b as i32, &mut st, 0);

    // ── Fase C: `ignore 1` sobre la condición verdadera ─────────────────────
    let hijo_c = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let pid_c = hijo_c;
    if !armar_watch_cond(hijo_c, dato, CREG_RAX, COP_EQ, pid_c) {
        println!("[SESSION-FAIL] no se pudo armar el watchpoint condicional");
        return 1;
    }
    if !poner_ignore(hijo_c, dato, 1) {
        println!("[SESSION-FAIL] el kernel rechazó el contador de omisión");
        return 1;
    }
    let st_ignore = leer_condicion(hijo_c, dato);
    println!("[SESSION-WCOND-IGNORE] `ignore 1`: el primer acierto se consume sin despertar al trazador{}",
             st_ignore.map(|x| format!(" (omitidos={})", x.ignore)).unwrap_or_default());
    if sys_ptrace(PTRACE_CONT, hijo_c as i64, 0, 0) < 0 { return 1; }
    let _ = waitpid(hijo_c as i32, &mut st, WUNTRACED);
    let st_c = st;
    let ok_ignore = wifexited(st_c);
    println!("[SESSION-WCOND-IGNORE-SKIP] el objetivo terminó solo (código {}) tras consumirse el acierto ignorado",
             if ok_ignore { wexitstatus(st_c) } else { -1 });
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_c as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_c as i64, 0, 0);
    let _ = waitpid(hijo_c as i32, &mut st, 0);

    if ok_falsa && ok_hit && ok_ignore {
        println!("[SESSION-WCOND-PASS] watchpoints con expresión verificados: el KERNEL evalúa la condición sobre el");
        println!("                      acceso de datos, respeta condiciones encadenadas y consume los aciertos ignorados");
        0
    } else {
        println!("[SESSION-FAIL] el ensayo de watchpoints no se completó (falsa={} hit={} ignore={})",
                 ok_falsa, ok_hit, ok_ignore);
        1
    }
}

/// `--wcond <pid> <dirección> <registro> <op> <valor>`: watchpoint con expresión.
fn cmd_wcond(pid: u64, token: &str, reg: &str, op: &str, valor: &str, ignorar: u32) -> i32 {
    let addr = match parse_u64(token) { Some(a) => a, None => { println!("[dbg] dirección inválida: {}", token); return 1; } };
    let r = match reg_de_nombre(reg) { Some(v) => v, None => { println!("[dbg] registro inválido: {}", reg); return 1; } };
    let o = match op_de_nombre(op) { Some(v) => v, None => { println!("[dbg] operador inválido: {}", op); return 1; } };
    let v = match parse_u64(valor) { Some(v) => v, None => { println!("[dbg] valor inválido: {}", valor); return 1; } };
    if !armar_watch_cond(pid, addr, r, o, v) {
        println!("[dbg] el kernel rechazó el watchpoint condicional en 0x{:016x}", addr);
        return 1;
    }
    println!("[WCOND-SET] vigilancia de escritura en 0x{:016x} con condición {} {} {:#x}", addr, nombre_de_reg(r), nombre_de_op(o), v);
    if ignorar > 0 {
        let _ = poner_ignore(pid, addr, ignorar);
        println!("[WCOND-IGNORE] los primeros {} aciertos no despertarán al trazador", ignorar);
    }
    match leer_condicion(pid, addr) {
        Some(x) => {
            println!("[WCOND-VERIFIED] 0x{:016x} {} {} {:#x} ignore={} hits={} skips={}",
                     addr, nombre_de_reg(x.clauses[0].reg), nombre_de_op(x.clauses[0].op), x.clauses[0].value, x.ignore, x.hits, x.skips);
            0
        }
        None => 1,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Condiciones de dos cláusulas (conjunción) y disparo por RANGO
//
//  Un rango `A <= x < B` no necesita operador propio: son dos cláusulas sobre el
//  mismo registro unidas por Y lógico. Sobre el pseudo-registro `addr` (la
//  dirección que disparó el trap) un rango filtra CUÁL de los accesos vigilados
//  despierta al trazador, que es la forma de vigilar un búfer sin que cada
//  escritura ajena interrumpa.
// ═══════════════════════════════════════════════════════════════════════════

/// Instala una condición de dos cláusulas: `c1 && c2`.
fn poner_condicion2(
    hijo: u64, addr: u64,
    r1: u32, o1: u32, v1: u64,
    r2: u32, o2: u32, v2: u64,
) -> bool {
    let spec = BpCond2 { n: 2, _pad: 0, reg: r1, op: o1, value: v1, reg2: r2, op2: o2, value2: v2 };
    sys_ptrace(PTRACE_SETBPCOND2, hijo as i64, addr as usize,
               &spec as *const BpCond2 as usize) >= 0
}

/// `rango`: condición `min <= reg < max` (dos cláusulas, comparaciones con signo).
fn poner_rango(hijo: u64, addr: u64, reg: u32, min: u64, max: u64) -> bool {
    poner_condicion2(hijo, addr, reg, COP_GE, min, reg, COP_LT, max)
}

/// Arma una vigilancia de escritura de 8 bytes con una condición de rango sobre
/// la dirección vigilada: sólo los accesos que caigan dentro del rango despertarán
/// al trazador. Devuelve 0 si el kernel aceptó la vigilancia.
fn armar_watch_rango(hijo: u64, slot_addr: u64, min: u64, max: u64) -> bool {
    sys_ptrace(PTRACE_SETWATCHPOINT, hijo as i64, slot_addr as usize,
               watch_cfg(8, DR7_RW_WRITE)) >= 0
        && poner_rango(hijo, slot_addr, CREG_ADDR, min, max)
}

/// Ensayo de condiciones de dos cláusulas y rango (`dbg --rangetest`):
///   A. conjunción VERDADERA sobre dos registros distintos (punto de parada de
///      ejecución en el prólogo de `<main>`: `rdi == A && rsi == B`), derivando
///      ambas constantes del propio código del objetivo;
///   B. conjunción FALSA (segunda cláusula imposible) -> el kernel calla y el
///      objetivo termina solo;
///   C. RANGO sobre la dirección vigilada: dos vigilancia de escritura contiguas
///      (marco y marco+8) con `marco+8 <= addr < marco+16`; la primera escritura
///      del objetivo cae fuera del rango y debe filtrarse en silencio, la segunda
///      cae dentro y debe despertar al trazador.
fn cmd_rangetest(programa: &str) -> i32 {
    println!("ENSAYO DE CONDICIONES CONJUNTAS Y RANGO — objetivo: {}", programa);
    let mut st = 0i32;

    // ── Fase 0: derivar del código qué comparar y qué vigilar ───────────────
    let hijo0 = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let mut syms = leer_simbolos(hijo0);
    let main = match resolver(hijo0, "main", &mut syms) {
        Some(a) => a,
        None => { println!("[SESSION-FAIL] no se pudo resolver 'main'"); return 1; }
    };
    let mut r0 = UserRegs::default();
    if sys_ptrace(PTRACE_GETREGS, hijo0 as i64, 0, &mut r0 as *mut UserRegs as usize) < 0 { return 1; }
    // Dos cargas inmediatas consecutivas del prólogo (p. ej. `mov edi, imm32` y
    // `mov esi, imm32`): son las dos cláusulas del ensayo, tomadas del programa.
    let mut addr = main;
    let mut cargas: Vec<(u64, u64, u32)> = Vec::new();
    for _ in 0..8 {
        if cargas.len() >= 2 { break; }
        let bytes = leer_bytes(hijo0, addr, VENTANA);
        if bytes.len() < 6 { break; }
        let ins = decodificar(&bytes, addr);
        if ins.len == 0 { break; }
        if ins.len == 5 && (0xB8..=0xBF).contains(&bytes[0]) {
            let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
            let reg = match bytes[0] {
                0xB8 => CREG_RAX, 0xB9 => CREG_RCX, 0xBA => CREG_RDX, 0xBB => CREG_RBX,
                0xBC => CREG_RSP, 0xBD => CREG_RBP, 0xBE => CREG_RSI, _ => CREG_RDI,
            };
            cargas.push((addr, imm, reg));
        }
        addr += ins.len as u64;
    }
    if cargas.len() < 2 {
        println!("[SESSION-FAIL] el prólogo de <main> no tiene dos cargas inmediatas seguidas");
        return 1;
    }
    let (dir_carga2, v1, reg1) = cargas[1];
    let (_, v0, reg0) = cargas[0];
    // La conjunción se evalúa en la instrucción SIGUIENTE a la segunda carga: en
    // la dirección de la carga misma, su registro todavía no tiene el valor nuevo
    // (el punto de parada de ejecución se dispara ANTES de ejecutarla), así que la
    // cláusula 2 resultaría falsa por definición.
    let ins_carga2 = instruccion_en(hijo0, dir_carga2);
    let dir_bp = dir_carga2 + ins_carga2.len as u64;
    println!("[SESSION-RANGE-SCAN] dos cargas en el prólogo: {} = {:#x} y {} = {:#x}",
             nombre_de_reg(reg0), v0, nombre_de_reg(reg1), v1);
    println!("[SESSION-RANGE-SCAN] el punto de parada se instala en 0x{:016x} ({}), ya con ambos registros cargados",
             dir_bp, instruccion_en(hijo0, dir_bp).texto);

    // Dirección de datos: el marco de <main>, derivado de su prólogo.
    let ins_main = instruccion_en(hijo0, main);
    let mut reserva = 0x18u64;
    if let Some(resto) = ins_main.texto.strip_prefix("sub rsp, ") {
        if let Some(v) = parse_u64(resto.trim()) { reserva = v; }
    }
    let marco = r0.rsp - 8 - reserva;
    let _ = sys_ptrace(PTRACE_DETACH, hijo0 as i64, 0, 0);
    let _ = waitpid(hijo0 as i32, &mut st, 0);
    if !es_pila(marco) {
        println!("[SESSION-FAIL] el marco derivado 0x{:016x} no está en la pila del objetivo", marco);
        return 1;
    }
    println!("[SESSION-RANGE-SCAN] marco de <main> derivado del prólogo: 0x{:016x} ({} B reservados)", marco, reserva);

    // ── Fase A: conjunción VERDADERA sobre dos registros ────────────────────
    let hijo_a = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    if sys_ptrace(PTRACE_SETWATCHPOINT, hijo_a as i64, dir_bp as usize, watch_cfg(1, DR7_RW_EXEC)) < 0 {
        println!("[SESSION-FAIL] el kernel no aceptó el punto de parada de ejecución");
        return 1;
    }
    if !poner_condicion2(hijo_a, dir_bp, reg0, COP_EQ, v0, reg1, COP_EQ, v1) {
        println!("[SESSION-FAIL] el kernel rechazó la conjunción");
        return 1;
    }
    println!("[SESSION-RANGE-ARMED] conjunción en 0x{:016x}: {} == {:#x} && {} == {:#x} (ambas verdaderas)",
             dir_bp, nombre_de_reg(reg0), v0, nombre_de_reg(reg1), v1);
    if sys_ptrace(PTRACE_CONT, hijo_a as i64, 0, 0) < 0 { return 1; }
    let r_a = waitpid(hijo_a as i32, &mut st, WUNTRACED);
    let st_a = st;
    let ok_conj = r_a == hijo_a as i64 && wifstopped(st_a);
    if let Some(x) = leer_condicion(hijo_a, dir_bp) {
        println!("[SESSION-RANGE-STATS] conjunción verdadera: aciertos={} saltos={} (contadores del KERNEL)", x.hits, x.skips);
        if x.n >= 2 {
            println!("[SESSION-RANGE-CLAUSES] cláusulas registradas: {} {} {:#x} && {} {} {:#x}",
                     nombre_de_reg(x.clauses[0].reg), nombre_de_op(x.clauses[0].op), x.clauses[0].value,
                     nombre_de_reg(x.clauses[1].reg), nombre_de_op(x.clauses[1].op), x.clauses[1].value);
        }
    }
    println!("[SESSION-RANGE-HIT] el trazador despertó por la conjunción en 0x{:016x}", dir_bp);
    let _ = quitar_condicion(hijo_a, dir_bp);
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_a as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_a as i64, 0, 0);
    let _ = waitpid(hijo_a as i32, &mut st, 0);

    // ── Fase B: conjunción FALSA (segunda cláusula imposible) ───────────────
    let hijo_b = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    if sys_ptrace(PTRACE_SETWATCHPOINT, hijo_b as i64, dir_bp as usize, watch_cfg(1, DR7_RW_EXEC)) < 0 {
        return 1;
    }
    if !poner_condicion2(hijo_b, dir_bp, reg0, COP_EQ, v0, reg1, COP_EQ, v1.wrapping_add(0x1000)) {
        println!("[SESSION-FAIL] el kernel rechazó la conjunción falsa");
        return 1;
    }
    println!("[SESSION-RANGE-ARMED] conjunción en 0x{:016x}: {} == {:#x} && {} == {:#x} (la 2ª imposible)",
             dir_bp, nombre_de_reg(reg0), v0, nombre_de_reg(reg1), v1.wrapping_add(0x1000));
    if sys_ptrace(PTRACE_CONT, hijo_b as i64, 0, 0) < 0 { return 1; }
    let _ = waitpid(hijo_b as i32, &mut st, WUNTRACED);
    let st_b = st;
    let ok_falsa = wifexited(st_b);
    println!("[SESSION-RANGE-SKIP] conjunción falsa: el kernel calló y el objetivo terminó solo (código {})",
             if ok_falsa { wexitstatus(st_b) } else { -1 });
    let _ = sys_ptrace(PTRACE_DETACH, hijo_b as i64, 0, 0);
    let _ = waitpid(hijo_b as i32, &mut st, 0);
    if !ok_falsa {
        println!("[SESSION-FAIL] el objetivo no debía detenerse con la conjunción falsa");
        return 1;
    }

    // ── Fase C: RANGO sobre la dirección vigilada ───────────────────────────
    // Dos vigilancia contiguas de 8 bytes: el marco (fuera del rango) y marco+8
    // (dentro). El objetivo escribe primero en el marco y después en el marco+8:
    // la primera debe filtrarse, la segunda debe despertar al trazador.
    let hijo_c = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let min = marco + 8;
    let max = marco + 16;
    if !armar_watch_rango(hijo_c, marco, min, max) {
        println!("[SESSION-FAIL] no se pudo armar la vigilancia del marco");
        return 1;
    }
    if !armar_watch_rango(hijo_c, marco + 8, min, max) {
        println!("[SESSION-FAIL] no se pudo armar la vigilancia contigua");
        return 1;
    }
    println!("[SESSION-RANGE-WATCH] vigilancia de escritura en 0x{:016x} y 0x{:016x} con rango {} {:016x} <= addr < {:016x}",
             marco, marco + 8, "addr", min, max);
    if sys_ptrace(PTRACE_CONT, hijo_c as i64, 0, 0) < 0 { return 1; }
    let r_c = waitpid(hijo_c as i32, &mut st, WUNTRACED);
    let st_c = st;
    let detenido = r_c == hijo_c as i64 && wifstopped(st_c);
    let mut regs_c = UserRegs::default();
    let _ = sys_ptrace(PTRACE_GETREGS, hijo_c as i64, 0, &mut regs_c as *mut UserRegs as usize);
    let en_rango = leer_condicion(hijo_c, marco + 8);
    let fuera = leer_condicion(hijo_c, marco);
    if let Some(x) = fuera {
        println!("[SESSION-RANGE-STATS] vigilancia fuera del rango: aciertos={} saltos={} (filtrada por el kernel)", x.hits, x.skips);
    }
    if let Some(x) = en_rango {
        println!("[SESSION-RANGE-STATS] vigilancia dentro del rango: aciertos={} saltos={}", x.hits, x.skips);
    }
    println!("[SESSION-RANGE-HIT-WATCH] el trazador despertó por el acceso DENTRO del rango: RIP=0x{:016x} <{}>",
             regs_c.rip, simbolizar(&syms, regs_c.rip.wrapping_sub(1)).unwrap_or_else(|| String::from("?")));
    // El valor que el objetivo escribió en el marco se lee de su memoria: el
    // ensayo no certifica «algo pasó», certifica qué dato pasó.
    let valor = peek_u64(hijo_c, marco + 8).unwrap_or(0);
    let rsp_objetivo = regs_c.rsp;
    println!("[SESSION-RANGE-VALUE] 0x{:016x} contiene {:#x} (RSP del objetivo = {:#x}) -> escritura observada",
             marco + 8, valor, rsp_objetivo);
    let ok_rango = detenido
        && valor != 0
        && fuera.map(|x| x.skips >= 1 && x.hits == 0).unwrap_or(false)
        && en_rango.map(|x| x.hits >= 1).unwrap_or(false);
    let _ = quitar_condicion(hijo_c, marco);
    let _ = quitar_condicion(hijo_c, marco + 8);
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_c as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_c as i64, 0, 0);
    let _ = waitpid(hijo_c as i32, &mut st, 0);

    if ok_conj && ok_falsa && ok_rango {
        println!("[SESSION-RANGE-PASS] condiciones conjuntas y rango verificados: el KERNEL evalúa dos cláusulas,");
        println!("                       filtra los accesos fuera del rango y despierta al trazador sólo dentro de él");
        0
    } else {
        println!("[SESSION-FAIL] el ensayo de conjunción/rango no se completó (conj={} falsa={} rango={})",
                 ok_conj, ok_falsa, ok_rango);
        1
    }
}

/// `--range <pid> <dirección> <registro|addr> <min> <max>`.
fn cmd_range(pid: u64, token: &str, reg: &str, min: &str, max: &str) -> i32 {
    let addr = match parse_u64(token) { Some(a) => a, None => { println!("[dbg] dirección inválida: {}", token); return 1; } };
    let r = match reg_de_nombre(reg) { Some(v) => v, None => { println!("[dbg] registro inválido: {}", reg); return 1; } };
    let lo = match parse_u64(min) { Some(v) => v, None => { println!("[dbg] mínimo inválido"); return 1; } };
    let hi = match parse_u64(max) { Some(v) => v, None => { println!("[dbg] máximo inválido"); return 1; } };
    if !poner_rango(pid, addr, r, lo, hi) {
        println!("[dbg] el kernel rechazó el rango");
        return 1;
    }
    println!("[RANGE-SET] 0x{:016x}: {} <= {} < {:#x}", addr, nombre_de_reg(r), "valor", hi);
    match leer_condicion(pid, addr) {
        Some(x) => {
            println!("[RANGE-VERIFIED] cláusulas={} {} >= {:#x} && {} < {:#x}",
                     x.n, nombre_de_reg(x.clauses[0].reg), x.clauses[0].value,
                     nombre_de_reg(x.clauses[1].reg), x.clauses[1].value);
            0
        }
        None => 1,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Lenguaje completo de condiciones: hasta CUATRO comparaciones en dos grupos
//  (Y dentro de cada grupo, O entre los grupos). Sin operadores nuevos:
//    rango ................ A <= x < B            -> n=2, g2=0
//    disyunción ........... x == A || x == B      -> n=2, g2=1
//    rango con guarda ..... A <= x < B && y != 0  -> n=3, g2=0
//    rango o valor exacto . (A <= x < B) || x == C -> n=3, g2=2
// ═══════════════════════════════════════════════════════════════════════════

/// Instala una condición general (hasta 4 cláusulas y sus grupos).
fn poner_condicion_n(hijo: u64, addr: u64, n: u32, g2: u32, cl: &[CondClause; 4]) -> bool {
    let spec = BpCondN { n, g2, clauses: *cl };
    sys_ptrace(PTRACE_SETBPCONDN, hijo as i64, addr as usize,
               &spec as *const BpCondN as usize) >= 0
}

/// Describe una condición tal como la reporta el kernel.
fn describir_condicion(x: &BpCondStat) -> String {
    let mut out = String::new();
    let n = (x.n as usize).min(4);
    let g2 = (x.g2 as usize).min(4);
    for c in 0..n {
        if c > 0 {
            out.push_str(if g2 > 0 && c == g2 { " || " } else { " && " });
        }
        out.push_str(nombre_de_reg(x.clauses[c].reg));
        out.push_str(nombre_de_op(x.clauses[c].op));
        out.push_str(&format!("{:#x}", x.clauses[c].value));
    }
    out
}

/// Arma una vigilancia de escritura con una condición general.
/// [T-044] `waitpid` con plazo, midiendo de verdad el tiempo del invitado.
///
/// Sin esto, si una parada de depuración no llega nunca (o el objetivo muere
/// antes de producirla) el ensayo se queda bloqueado y lo único que queda en el
/// registro es «sin evidencia»: no se sabe si el kernel rechazó la vigilancia, si
/// el hijo murió o si la parada se perdió. En la primera recertificación de b203
/// eso dejó cinco comprobaciones sin diagnóstico posible (132 s de espera).
/// Devuelve 0 si venció el plazo; si no, lo mismo que `waitpid`.
fn waitpid_plazo(pid: i32, st: &mut i32, options: i32, ms: u64) -> i64 {
    let mut visto = 0u64;
    while visto <= ms {
        let r = waitpid(pid, st, options | 1 /* WNOHANG */);
        if r != 0 { return r; }
        portix_rt::process::sleep(1); // tick de 10 ms en Portix
        visto += 10;
    }
    0
}

/// [T-044] Deja en el registro el resultado CRUDO de la espera de una fase:
/// código devuelto, si el hijo quedó detenido y el estado completo. Es lo que
/// distingue «no llegó la parada» de «el objetivo terminó antes».
fn traza_espera(fase: &str, hijo: u64, r: i64, st: i32, vencido: bool) {
    println!("[SESSION-OR-WAIT] fase={} hijo={} r={} stopped={} vencido={} status=0x{:04x}",
             fase, hijo, r, wifstopped(st), vencido, st);
}

fn esperar_parada_or(fase: &str, hijo: u64, st: &mut i32, ms: u64) -> (bool, bool) {
    let r = waitpid_plazo(hijo as i32, st, WUNTRACED, ms);
    let vencido = r == 0;
    traza_espera(fase, hijo, r, *st, vencido);
    if vencido {
        println!("[SESSION-OR-TIMEOUT] fase={} sin parada del hijo {} tras {} ms (parada perdida o objetivo terminado)",
                 fase, hijo, ms);
    }
    (r == hijo as i64 && wifstopped(*st), vencido)
}

fn armar_watch_general(hijo: u64, addr: u64, n: u32, g2: u32, cl: &[CondClause; 4]) -> bool {
    sys_ptrace(PTRACE_SETWATCHPOINT, hijo as i64, addr as usize, watch_cfg(8, DR7_RW_WRITE)) >= 0
        && poner_condicion_n(hijo, addr, n, g2, cl)
}

/// Ensayo del lenguaje completo de condiciones (`dbg --ortest`). Cuatro fases,
/// cada una verificada con los contadores del kernel:
///   A. disyunción `x == A || x == B` (el segundo grupo hace verdadera la
///      condición aunque el primero sea falso);
///   B. conjunción de TRES cláusulas en un solo grupo;
///   C. rango con guarda de cuatro cláusulas (`A <= addr < B && registro != 0`)
///      sobre datos vigilados;
///   D. `(A <= addr < B) || addr == C`: la escritura exactamente en `C` —que el
///      rango puro filtraba en silencio— ahora despierta al trazador por la
///      segunda alternativa. Mismo hardware, misma escritura, otra condición.
fn cmd_ortest(programa: &str) -> i32 {
    println!("ENSAYO DEL LENGUAJE DE CONDICIONES (Y / O / RANGO) — objetivo: {}", programa);
    let mut st = 0i32;

    // ── Fase 0: derivar del código qué comparar y qué vigilar ───────────────
    let hijo0 = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let mut syms = leer_simbolos(hijo0);
    let main = match resolver(hijo0, "main", &mut syms) {
        Some(a) => a,
        None => { println!("[SESSION-FAIL] no se pudo resolver 'main'"); return 1; }
    };
    let mut r0 = UserRegs::default();
    if sys_ptrace(PTRACE_GETREGS, hijo0 as i64, 0, &mut r0 as *mut UserRegs as usize) < 0 { return 1; }
    let mut addr = main;
    let mut cargas: Vec<(u64, u64, u32)> = Vec::new();
    for _ in 0..8 {
        if cargas.len() >= 2 { break; }
        let bytes = leer_bytes(hijo0, addr, VENTANA);
        if bytes.len() < 6 { break; }
        let ins = decodificar(&bytes, addr);
        if ins.len == 0 { break; }
        if ins.len == 5 && (0xB8..=0xBF).contains(&bytes[0]) {
            let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
            let reg = match bytes[0] {
                0xB8 => CREG_RAX, 0xB9 => CREG_RCX, 0xBA => CREG_RDX, 0xBB => CREG_RBX,
                0xBC => CREG_RSP, 0xBD => CREG_RBP, 0xBE => CREG_RSI, _ => CREG_RDI,
            };
            cargas.push((addr, imm, reg));
        }
        addr += ins.len as u64;
    }
    if cargas.len() < 2 {
        println!("[SESSION-FAIL] el prólogo de <main> no tiene dos cargas inmediatas seguidas");
        return 1;
    }
    let (carga2, v1, reg1) = cargas[1];
    let (_, v0, reg0) = cargas[0];
    let dir_bp = carga2 + instruccion_en(hijo0, carga2).len as u64;
    let ins_main = instruccion_en(hijo0, main);
    let mut reserva = 0x18u64;
    if let Some(resto) = ins_main.texto.strip_prefix("sub rsp, ") {
        if let Some(v) = parse_u64(resto.trim()) { reserva = v; }
    }
    let marco = r0.rsp - 8 - reserva;
    let _ = sys_ptrace(PTRACE_DETACH, hijo0 as i64, 0, 0);
    let _ = waitpid(hijo0 as i32, &mut st, 0);
    println!("[SESSION-OR-SCAN] prólogo: {} = {:#x} y {} = {:#x}; parada en 0x{:016x}; marco en 0x{:016x}",
             nombre_de_reg(reg0), v0, nombre_de_reg(reg1), v1, dir_bp, marco);

    // ── Fase A: disyunción (dos grupos de una cláusula) ─────────────────────
    let hijo_a = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    if sys_ptrace(PTRACE_SETWATCHPOINT, hijo_a as i64, dir_bp as usize, watch_cfg(1, DR7_RW_EXEC)) < 0 { return 1; }
    let cl_disy = [
        CondClause { reg: reg1, op: COP_EQ, value: v1.wrapping_add(0x1000) }, // falsa
        CondClause { reg: reg1, op: COP_EQ, value: v1 },                      // verdadera
        CondClause::default(), CondClause::default(),
    ];
    if !poner_condicion_n(hijo_a, dir_bp, 2, 1, &cl_disy) {
        println!("[SESSION-FAIL] el kernel rechazó la disyunción");
        return 1;
    }
    println!("[SESSION-OR-DISJUNCTION] 0x{:016x}: {} == {:#x} || {} == {:#x} (la 1ª falsa, la 2ª verdadera)",
             dir_bp, nombre_de_reg(reg1), v1.wrapping_add(0x1000), nombre_de_reg(reg1), v1);
    if sys_ptrace(PTRACE_CONT, hijo_a as i64, 0, 0) < 0 { return 1; }
    let (ok_disj, _venc_a) = esperar_parada_or("A-disyuncion", hijo_a, &mut st, 15000);
    if let Some(x) = leer_condicion(hijo_a, dir_bp) {
        println!("[SESSION-OR-STATS] disyunción: aciertos={} saltos={} condición={}", x.hits, x.skips, describir_condicion(&x));
    }
    let _ = quitar_condicion(hijo_a, dir_bp);
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_a as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_a as i64, 0, 0);
    let _ = waitpid(hijo_a as i32, &mut st, 0);

    // ── Fase B: conjunción de tres cláusulas en un grupo ───────────────────
    let hijo_b = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    if sys_ptrace(PTRACE_SETWATCHPOINT, hijo_b as i64, dir_bp as usize, watch_cfg(1, DR7_RW_EXEC)) < 0 { return 1; }
    let cl_tres = [
        CondClause { reg: reg0, op: COP_EQ, value: v0 },
        CondClause { reg: reg1, op: COP_EQ, value: v1 },
        CondClause { reg: CREG_RSP, op: COP_GE, value: PILA_BASE },
        CondClause::default(),
    ];
    if !poner_condicion_n(hijo_b, dir_bp, 3, 0, &cl_tres) {
        println!("[SESSION-FAIL] el kernel rechazó la conjunción de tres cláusulas");
        return 1;
    }
    println!("[SESSION-OR-TRIPLE] 0x{:016x}: {} == {:#x} && {} == {:#x} && rsp >= {:#x}",
             dir_bp, nombre_de_reg(reg0), v0, nombre_de_reg(reg1), v1, PILA_BASE);
    if sys_ptrace(PTRACE_CONT, hijo_b as i64, 0, 0) < 0 { return 1; }
    let (ok_tres, _venc_b) = esperar_parada_or("B-tres", hijo_b, &mut st, 15000);
    if let Some(x) = leer_condicion(hijo_b, dir_bp) {
        println!("[SESSION-OR-STATS] tres cláusulas: aciertos={} saltos={} condición={}", x.hits, x.skips, describir_condicion(&x));
    }
    let _ = quitar_condicion(hijo_b, dir_bp);
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_b as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_b as i64, 0, 0);
    let _ = waitpid(hijo_b as i32, &mut st, 0);

    // ── Fase C: rango con guarda de cuatro cláusulas, y Fase D: rango O exacto ──
    let min = marco + 8;
    let max = marco + 16;
    // Fase C sobre el tramo vigilado, con la guarda `rsp != 0`.
    let hijo_c = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let cl_guarda = [
        CondClause { reg: CREG_ADDR, op: COP_GE, value: min },
        CondClause { reg: CREG_ADDR, op: COP_LT, value: max },
        CondClause { reg: CREG_RSP, op: COP_NE, value: 0 },
        CondClause::default(),
    ];
    if !armar_watch_general(hijo_c, marco + 8, 3, 0, &cl_guarda) {
        println!("[SESSION-FAIL] no se pudo armar el rango con guarda");
        return 1;
    }
    println!("[SESSION-OR-GUARDED] 0x{:016x}: addr >= {:#x} && addr < {:#x} && rsp != 0x0", marco + 8, min, max);
    if sys_ptrace(PTRACE_CONT, hijo_c as i64, 0, 0) < 0 { return 1; }
    let (ok_guarda, _venc_c) = esperar_parada_or("C-guarda", hijo_c, &mut st, 15000);
    if let Some(x) = leer_condicion(hijo_c, marco + 8) {
        println!("[SESSION-OR-STATS] rango con guarda: aciertos={} saltos={} condición={}", x.hits, x.skips, describir_condicion(&x));
    }
    let _ = quitar_condicion(hijo_c, marco + 8);
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_c as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_c as i64, 0, 0);
    let _ = waitpid(hijo_c as i32, &mut st, 0);

    // Fase D: la MISMA vigilancia que en `--rangetest` filtraba la escritura en
    // `marco`; con `|| addr == marco` esa misma escritura debe despertar al
    // trazador. La demostración se hace en dos pasos sobre el mismo objetivo y
    // la misma condición de tres cláusulas: primero el acceso exacto `marco`
    // (el que el rango puro filtraba en silencio) y después el acceso contiguo
    // `marco + 8`, que cae dentro del tramo del primer grupo.
    let hijo_d = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let cl_or = [
        CondClause { reg: CREG_ADDR, op: COP_GE, value: min },
        CondClause { reg: CREG_ADDR, op: COP_LT, value: max },
        CondClause { reg: CREG_ADDR, op: COP_EQ, value: marco },
        CondClause::default(),
    ];
    if !armar_watch_general(hijo_d, marco, 3, 2, &cl_or) {
        println!("[SESSION-FAIL] no se pudo armar el rango con alternativa exacta");
        return 1;
    }
    println!("[SESSION-OR-RANGE-OR] 0x{:016x}: (addr >= {:#x} && addr < {:#x}) || addr == {:#x}",
             marco, min, max, marco);
    if sys_ptrace(PTRACE_CONT, hijo_d as i64, 0, 0) < 0 { return 1; }
    let (primera, _venc_d) = esperar_parada_or("D-exacto", hijo_d, &mut st, 15000);
    let st_d = st;
    let mut regs_d = UserRegs::default();
    let _ = sys_ptrace(PTRACE_GETREGS, hijo_d as i64, 0, &mut regs_d as *mut UserRegs as usize);
    println!("[SESSION-OR-HIT-EXACT] el trazador despertó por la escritura en 0x{:016x} (antes filtrada): RIP=0x{:016x} <{}>",
             marco, regs_d.rip, simbolizar(&syms, regs_d.rip.wrapping_sub(1)).unwrap_or_else(|| String::from("?")));
    // Segunda parada: el acceso contiguo, dentro del tramo del primer grupo. La
    // vigilancia se arma con la MISMA condición general y el MISMO hardware.
    let mut segunda = false;
    let mut regs2 = UserRegs::default();
    if primera && armar_watch_general(hijo_d, marco + 8, 3, 2, &cl_or) {
        if sys_ptrace(PTRACE_CONT, hijo_d as i64, 0, 0) == 0 {
            let (ok2, _venc_d2) = esperar_parada_or("D-tramo", hijo_d, &mut st, 15000);
            segunda = ok2;
            let st_d2 = st;
            let _ = sys_ptrace(PTRACE_GETREGS, hijo_d as i64, 0, &mut regs2 as *mut UserRegs as usize);
            println!("[SESSION-OR-HIT-RANGE] segunda parada: escritura DENTRO del tramo en 0x{:016x}: RIP=0x{:016x} <{}> (notificada={})",
                     marco + 8, regs2.rip, simbolizar(&syms, regs2.rip.wrapping_sub(1)).unwrap_or_else(|| String::from("?")), segunda);
        }
    }
    let valor = peek_u64(hijo_d, marco + 8).unwrap_or(0);
    println!("[SESSION-OR-VALUE] 0x{:016x} contiene {:#x} (RSP del objetivo = {:#x})", marco + 8, valor, regs2.rsp);
    let h_exacto = leer_condicion(hijo_d, marco).map(|x| x.hits).unwrap_or(0);
    let s_exacto = leer_condicion(hijo_d, marco).map(|x| x.skips).unwrap_or(0);
    let h_tramo = leer_condicion(hijo_d, marco + 8).map(|x| x.hits).unwrap_or(0);
    let s_tramo = leer_condicion(hijo_d, marco + 8).map(|x| x.skips).unwrap_or(0);
    if let Some(x) = leer_condicion(hijo_d, marco) {
        println!("[SESSION-OR-STATS] rango o exacto: aciertos={} saltos={} condición={}",
                 h_exacto + h_tramo, s_exacto + s_tramo, describir_condicion(&x));
    }
    let ok_or = primera && segunda && valor != 0 && valor == regs2.rsp
        && (h_exacto + h_tramo) >= 2 && (s_exacto + s_tramo) == 0;
    let _ = quitar_condicion(hijo_d, marco);
    let _ = quitar_condicion(hijo_d, marco + 8);
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_d as i64, 0, 0);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_d as i64, 0, 0);
    let _ = waitpid(hijo_d as i32, &mut st, 0);

    if ok_disj && ok_tres && ok_guarda && ok_or {
        println!("[SESSION-OR-PASS] lenguaje de condiciones verificado: disyunción de dos grupos, conjunción de");
        println!("                    tres cláusulas, rango con guarda y rango con alternativa exacta, todo");
        println!("                    evaluado por el KERNEL sobre los registros y sobre la dirección vigilada");
        0
    } else {
        println!("[SESSION-FAIL] el ensayo del lenguaje de condiciones no se completó (disj={} tres={} guarda={} or={})",
                 ok_disj, ok_tres, ok_guarda, ok_or);
        1
    }
}

/// Lanza un objetivo trazado y espera su parada de `execve`.
fn lanzar_trazado(programa: &str, st: &mut i32) -> Option<u64> {
    let pid = fork();
    if pid < 0 {
        println!("[dbg] fork falló ({})", pid);
        return None;
    }
    if pid == 0 {
        let code = hijo_trazado(programa);
        portix_rt::syscall::sys_exit(code);
    }
    let hijo = pid as u64;
    if !esperar_parada(hijo, st, "execve") {
        println!("[SESSION-FAIL] el objetivo no se detuvo tras execve (status=0x{:04x})", st);
        return None;
    }
    Some(hijo)
}

/// Busca en el prólogo del símbolo indicado una carga inmediata a un registro
/// (`mov r32, imm32`) y devuelve (dirección de la instrucción siguiente, valor).
/// Es lo que permite que el ensayo de condiciones se derive del CÓDIGO REAL del
/// objetivo en lugar de suponer un valor: el programa dice qué comparar.
fn buscar_carga_inmediata(hijo: u64, desde: u64) -> Option<(u64, u64, &'static str)> {
    let mut addr = desde;
    for _ in 0..12 {
        let bytes = leer_bytes(hijo, addr, VENTANA);
        if bytes.len() < 6 { return None; }
        let ins = decodificar(&bytes, addr);
        if ins.len == 0 { return None; }
        // mov esi, imm32 (BE) / mov edi, imm32 (BF) / mov eax, imm32 (B8) ...
        if ins.len == 5 && (bytes[0] == 0xBE || bytes[0] == 0xBF || bytes[0] == 0xB8
            || bytes[0] == 0xB9 || bytes[0] == 0xBA || bytes[0] == 0xBB) {
            let imm = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u64;
            let reg = match bytes[0] {
                0xB8 => "rax", 0xB9 => "rcx", 0xBA => "rdx", 0xBB => "rbx",
                0xBE => "rsi", 0xBF => "rdi", _ => "rax",
            };
            let nombre: &'static str = match reg {
                "rax" => "rax", "rcx" => "rcx", "rdx" => "rdx",
                "rbx" => "rbx", "rsi" => "rsi", _ => "rdi",
            };
            return Some((addr + ins.len as u64, imm, nombre));
        }
        addr += ins.len as u64;
    }
    None
}

/// Ensayo de breakpoints condicionales (`dbg --condtest`): el mismo punto de
/// parada se prueba dos veces sobre objetivos recién ejecutados — una con una
/// condición FALSA (el kernel debe reanudar sin despertar al trazador y el
/// objetivo debe llegar a terminar solo) y otra con una VERDADERA (el kernel
/// debe detenerlo y notificar). Los contadores del kernel cierran la prueba.
fn cmd_condtest(programa: &str) -> i32 {
    println!("ENSAYO DE BREAKPOINTS CONDICIONALES — objetivo: {}", programa);
    let mut st = 0i32;

    // ── Fase previa: descubrir qué comparar, leyendo el código real ──────────
    let hijo = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    let mut syms = leer_simbolos(hijo);
    let main = match resolver(hijo, "main", &mut syms) {
        Some(a) => a,
        None => { println!("[SESSION-FAIL] no se pudo resolver 'main'"); return 1; }
    };
    let (bp_addr, valor, reg) = match buscar_carga_inmediata(hijo, main) {
        Some(v) => v,
        None => { println!("[SESSION-FAIL] no se encontró una carga inmediata en el prólogo de <main>"); return 1; }
    };
    println!("[SESSION-COND-SCAN] el objetivo carga {} = {:#x}; el punto de parada se instala en 0x{:016x}",
             reg, valor, bp_addr);
    let _ = sys_ptrace(PTRACE_DETACH, hijo as i64, 0, 0);
    let _ = waitpid(hijo as i32, &mut st, 0);

    // ── Fase A: condición FALSA -> el kernel reanuda, el objetivo termina solo ──
    let hijo_a = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    // Punto de parada de EJECUCIÓN por hardware (DR, R/W = 00): se dispara ANTES
    // de ejecutar la instrucción y no modifica ni un byte del código del
    // objetivo, que es lo que permite reanudar de forma exacta.
    let reg_id = reg_de_nombre(reg).unwrap_or(CREG_RSI);
    if sys_ptrace(PTRACE_SETWATCHPOINT, hijo_a as i64, bp_addr as usize,
                  watch_cfg(1, DR7_RW_EXEC)) < 0 {
        println!("[SESSION-FAIL] el kernel no aceptó el punto de parada de ejecución en 0x{:016x}", bp_addr);
        return 1;
    }
    let falsa = valor.wrapping_add(0x1000);
    if !poner_condicion(hijo_a, bp_addr, reg_id, COP_EQ, falsa) {
        println!("[SESSION-FAIL] el kernel rechazó la condición falsa");
        return 1;
    }
    println!("[SESSION-COND-ARMED] DR de ejecución en 0x{:016x} con condición FALSA ({} == {:#x}, el objetivo lleva {:#x})",
             bp_addr, reg, falsa, valor);
    if sys_ptrace(PTRACE_CONT, hijo_a as i64, 0, 0) < 0 {
        println!("[SESSION-FAIL] PTRACE_CONT rechazado");
        return 1;
    }
    let r_a = waitpid(hijo_a as i32, &mut st, WUNTRACED);
    let st_a = st;
    let terminó = r_a == hijo_a as i64 && wifexited(st_a);
    let stats_a = leer_condicion(hijo_a, bp_addr);
    if let Some(s) = stats_a {
        println!("[SESSION-COND-STATS] condición falsa: aciertos={} saltos={} (contadores del KERNEL)",
                 s.hits, s.skips);
    }
    if terminó {
        println!("[SESSION-COND-SKIP] con la condición falsa el kernel reanudó sin notificar y el objetivo");
        println!("                    terminó por sí mismo (código {}): el trazador nunca fue despertado por el breakpoint",
                 wexitstatus(st_a));
    } else {
        println!("[SESSION-FAIL] con la condición falsa el objetivo no debía detenerse (status=0x{:04x})", st_a);
        return 1;
    }
    // Evidencia de la fase falsa: el objetivo llegó a terminar por sí mismo SIN
    // que el trazador recibiera parada alguna (si la condición se hubiera
    // cumplido, `waitpid` habría devuelto WIFSTOPPED). Los contadores del kernel
    // quedan publicados en su traza `[BPCOND]` incluso después de recolectar al
    // hijo, que es donde se comprueban.
    let ok_skip = terminó;

    // ── Fase B: condición VERDADERA -> parada y notificación ────────────────
    let hijo_b = match lanzar_trazado(programa, &mut st) { Some(h) => h, None => return 1 };
    if sys_ptrace(PTRACE_SETWATCHPOINT, hijo_b as i64, bp_addr as usize,
                  watch_cfg(1, DR7_RW_EXEC)) < 0 {
        println!("[SESSION-FAIL] el kernel no aceptó el punto de parada de ejecución en 0x{:016x}", bp_addr);
        return 1;
    }
    if !poner_condicion(hijo_b, bp_addr, reg_id, COP_EQ, valor) {
        println!("[SESSION-FAIL] el kernel rechazó la condición verdadera");
        return 1;
    }
    println!("[SESSION-COND-ARMED] DR de ejecución en 0x{:016x} con condición VERDADERA ({} == {:#x})", bp_addr, reg, valor);
    if sys_ptrace(PTRACE_CONT, hijo_b as i64, 0, 0) < 0 { return 1; }
    let r_b = waitpid(hijo_b as i32, &mut st, WUNTRACED);
    let st_b = st;
    let detenido = r_b == hijo_b as i64 && wifstopped(st_b);
    let stats_b = leer_condicion(hijo_b, bp_addr);
    if let Some(s) = stats_b {
        println!("[SESSION-COND-STATS] condición verdadera: aciertos={} saltos={} (contadores del KERNEL)",
                 s.hits, s.skips);
    }
    if !detenido {
        println!("[SESSION-FAIL] con la condición verdadera el objetivo debía detenerse (status=0x{:04x})", st_b);
        return 1;
    }
    let mut regs_b = UserRegs::default();
    let _ = sys_ptrace(PTRACE_GETREGS, hijo_b as i64, 0, &mut regs_b as *mut UserRegs as usize);
    println!("[SESSION-COND-HIT] parada notificada al trazador en RIP=0x{:016x} <{}> (señal={})",
             regs_b.rip, simbolizar(&syms, regs_b.rip.wrapping_sub(1)).unwrap_or_else(|| String::from("?")),
             wstopsig(st_b));
    let ok_hit = stats_b.map(|s| s.hits >= 1 && s.skips == 0).unwrap_or(false);

    // ── Cierre ordenado: se retiran condición y breakpoint y se suelta el objetivo ──
    let _ = quitar_condicion(hijo_b, bp_addr);
    let _ = sys_ptrace(PTRACE_CLEARWATCHPOINTS, hijo_b as i64, 0, 0);
    let mut leido = UserRegs::default();
    let _ = sys_ptrace(PTRACE_GETREGS, hijo_b as i64, 0, &mut leido as *mut UserRegs as usize);
    let _ = sys_ptrace(PTRACE_DETACH, hijo_b as i64, 0, 0);
    let fin_b = waitpid(hijo_b as i32, &mut st, 0);
    println!("[SESSION-COND-CLEANUP] punto de parada de hardware y condición retirados; objetivo finalizado: pid={}", fin_b);

    if ok_skip && ok_hit {
        println!("[SESSION-COND-PASS] breakpoints condicionales verificados: el KERNEL evalúa la condición,");
        println!("                     reanuda sin notificar cuando es falsa y detiene al objetivo cuando es verdadera");
        0
    } else {
        println!("[SESSION-FAIL] el ensayo de condiciones no se completó (skip={} hit={})", ok_skip, ok_hit);
        1
    }
}

fn cmd_session(programa: &str, simbolo: &str, comandos: &str) -> i32 {
    println!("SESIÓN DE DEPURACIÓN INTERACTIVA — objetivo: {} (símbolo inicial: {})", programa, simbolo);
    let pid = fork();
    if pid < 0 {
        println!("[dbg] fork falló ({})", pid);
        return 1;
    }
    if pid == 0 {
        let code = hijo_trazado(programa);
        portix_rt::syscall::sys_exit(code);
    }
    let hijo = pid as u64;

    // ── 1. Parada de execve ───────────────────────────────────────────────
    let mut st = 0i32;
    if !esperar_parada(hijo, &mut st, "execve") {
        println!("[SESSION-FAIL] el objetivo no se detuvo tras execve (status=0x{:04x})", st);
        return 1;
    }
    println!("[SESSION-EXEC-STOP] PID={} detenido en el punto de entrada tras execve (WIFSTOPPED señal={})",
             hijo, wstopsig(st));
    let mut regs = UserRegs::default();
    if sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut regs as *mut UserRegs as usize) < 0 {
        println!("[SESSION-FAIL] GETREGS rechazado por el kernel");
        return 1;
    }
    let mut syms = leer_simbolos(hijo);
    println!("[SESSION-ENTRY] RIP=0x{:016x} RSP=0x{:016x} símbolos del ELF={}", regs.rip, regs.rsp, syms.len());
    println!("{}", simbolizar(&syms, regs.rip).map(|n| format!("   punto de entrada: <{}>", n))
        .unwrap_or_else(|| String::from("   punto de entrada: (sin símbolo)")));

    // ── 2. Punto de parada simbólico ──────────────────────────────────────
    let main_addr = match resolver(hijo, simbolo, &mut syms) {
        Some(a) => a,
        None => {
            println!("[SESSION-FAIL] no se pudo resolver '{}' en el ELF del objetivo", simbolo);
            let _ = sys_ptrace(PTRACE_DETACH, hijo as i64, 0, 0);
            return 1;
        }
    };
    let orig = match peek_u64(hijo, main_addr) { Some(v) => (v & 0xFF) as u8, None => 0 };
    if !poke_u64(hijo, main_addr, (peek_u64(hijo, main_addr).unwrap_or(0) & !0xFF) | 0xCC) {
        println!("[SESSION-FAIL] POKETEXT rechazado en 0x{:x}", main_addr);
        return 1;
    }
    let verif = peek_u64(hijo, main_addr).map(|v| (v & 0xFF) as u8).unwrap_or(0);
    println!("[SESSION-BP-INSTALLED] INT3 en 0x{:016x} <{}> byte_original={:#04x} memoria={:#04x}",
             main_addr, simbolo, orig, verif);

    // ── 3. Continuar: el objetivo corre hasta el breakpoint ───────────────
    if sys_ptrace(PTRACE_CONT, hijo as i64, 0, 0) < 0 {
        println!("[SESSION-FAIL] PTRACE_CONT rechazado");
        return 1;
    }
    if !esperar_parada(hijo, &mut st, "breakpoint") {
        println!("[SESSION-FAIL] el objetivo no se detuvo en el breakpoint");
        return 1;
    }
    let _ = sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut regs as *mut UserRegs as usize);
    let rip_bp = regs.rip;
    let nombre_bp = simbolizar(&syms, rip_bp.wrapping_sub(1)).unwrap_or_else(|| String::from(simbolo));
    println!("[SESSION-BP-HIT] PID={} detenido por SIGTRAP en RIP=0x{:016x} <{}>, byte 0xCC consumido",
             hijo, rip_bp, nombre_bp);

    // ── 4. Restauración, rebobinado y desenganche ────────────────────────────
    // Maniobra clásica del depurador: retirar el byte 0xCC del breakpoint y
    // devolver el RIP a la dirección original para que el objetivo EJECUTE la
    // instrucción completa (el #BP deja en el frame la dirección siguiente).
    if let Some(v) = peek_u64(hijo, main_addr) {
        let _ = poke_u64(hijo, main_addr, (v & !0xFF) | (orig as u64));
    }
    let limpio = peek_u64(hijo, main_addr).map(|v| (v & 0xFF) as u8).unwrap_or(0xFF);
    println!("[SESSION-BP-RESTORED] 0x{:016x} restaurado a {:#04x} (verificado por PEEKTEXT)", main_addr, limpio);

    // La instrucción original debe RE-EJECUTARSE: al retirar el 0xCC hay que
    // devolver el RIP a la dirección del breakpoint (el CPU dejó RIP+1 en el
    // frame). Sin este rebobinado, el objetivo reanudaría en medio de la
    // instrucción: `48 83 ec 18` se ejecutaría como `83 ec 18` (`sub esp,…`),
    // anulando la mitad alta de RSP.
    let mut rb = UserRegs::default();
    if sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut rb as *mut UserRegs as usize) == 0 {
        let rip_antes = rb.rip;
        rb.rip = main_addr;
        let st_regs = sys_ptrace(PTRACE_SETREGS, hijo as i64, 0, &rb as *const UserRegs as usize);
        let mut rb2 = UserRegs::default();
        let _ = sys_ptrace(PTRACE_GETREGS, hijo as i64, 0, &mut rb2 as *mut UserRegs as usize);
        println!("[SESSION-RIP-REWIND] RIP 0x{:016x} -> 0x{:016x} (SETREGS={}, verificado=0x{:016x})",
                 rip_antes, rb.rip, st_regs, rb2.rip);
    }
    // ── 5. Comandos de sesión (step / next / bt / regs / mem / cont / quit) ──
    let mut ejecutados = 0usize;
    let pedidos = comandos.split(',').filter(|c| !c.trim().is_empty()).count();
    if pedidos > 0 {
        ejecutados = ejecutar_comandos(hijo, &mut st, &syms, &regs, comandos);
        println!("[SESSION-CMD-RESUMEN] comandos ejecutados={} de {}", ejecutados, pedidos);
    }

    let det = sys_ptrace(PTRACE_DETACH, hijo as i64, 0, 0);
    let fin = waitpid(hijo as i32, &mut st, 0);
    println!("[SESSION-EXIT] detach={} salida del objetivo: pid={} status=0x{:04x} ({}={})",
             det, fin, st,
             if wifexited(st) { "código" } else { "señal" },
             if wifexited(st) { wexitstatus(st) } else { wtermsig(st) });

    let ejecutado = fin == hijo as i64 && wifexited(st);
    let ok = limpio == orig && ejecutado && ejecutados == pedidos;
    if ok {
        println!("[SESSION-PASS] sesión de depuración verificada: parada de execve notificada al trazador,");
        println!("               breakpoint simbólico alcanzado, byte restaurado y objetivo finalizado (código {})",
                 if wifexited(st) { wexitstatus(st) } else { wtermsig(st) });
    } else {
        println!("[SESSION-FAIL] algún paso de la sesión no se cumplió");
    }
    if ok { 0 } else { 1 }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Trazador del kernel
// ═══════════════════════════════════════════════════════════════════════════

fn control_tracer(arg: &str) -> i32 {
    let file = match fs::File::open("/proc/syscalls", 1) {
        Ok(f) => f,
        Err(e) => {
            println!("[dbg] No se pudo abrir /proc/syscalls para escritura (error {})", e);
            return 1;
        }
    };
    match file.write(arg.as_bytes()) {
        Ok(n) => {
            println!("[dbg] Trazador: comando '{}' aplicado ({} bytes escritos en /proc/syscalls)", arg, n);
            0
        }
        Err(e) => {
            println!("[dbg] Trazador: comando rechazado por el kernel (error {})", e);
            1
        }
    }
}

fn ayuda() -> i32 {
    println!("Portix Debugger (dbg) v3.6.0 — depurador de Ring 3 simbolizado");
    println!("usage: dbg <executable> [args]        lanza un binario bajo el depurador (ptrace)");
    println!("       dbg -p <pid>                   se engancha a un proceso vivo (PTRACE_ATTACH)");
    println!("       dbg <pid|self|nombre> [accion]    inspecciona un proceso vivo via /proc");
    println!("           status|cmdline|maps|profiler|syscalls");
    println!("           symbols [filtro]           tabla .symtab publicada por el kernel");
    println!("           peek <dir> [n]             volcado hexadecimal de la memoria del objetivo");
    println!("           disasm <simbolo|dir> [n]   desensamblado con simbolizacion de destinos");
    println!("       dbg --break <pid|self|nombre> <sim|dir>   instala un INT3 persistente en el objetivo");
    println!("       dbg --bp-list <pid>            verifica los puntos de parada instalados");
    println!("       dbg --bp-clear <pid>           restaura los bytes originales");
    println!("       dbg --watch <pid|self|nombre> <sim|dir> <len> [w|rw]   vigilancia por hardware (DR0-DR3)");
    println!("       dbg --watch-list <pid> | --watch-clear <pid>");
    println!("       dbg --trace [on|off|clear]     controla el trazador de syscalls del kernel");
    println!("       dbg --version                  version y capacidades");
    0
}

// ═══════════════════════════════════════════════════════════════════════════
//  Entrada
// ═══════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = parse_args(argc, argv);
    if args.len() < 2 {
        return ayuda();
    }

    if args[1] == "--version" || args[1] == "version" {
        println!("portix-dbg v3.6.0 (depurador simbolizado de Ring 3 + telemetria del kernel)");
        println!("  simbolos            : /proc/<pid>/symbols (tabla .symtab del ELF en runtime)");
        println!("  memoria del objetivo: PTRACE_PEEKTEXT/POKEDATA sobre el CR3 del proceso");
        println!("  puntos de parada    : INT3 persistente en la imagen (/tmp/dbg-<pid>.bps)");
        println!("  objetivo            : <pid> | self | <nombre de proceso vivo> (via /proc)");
        println!("  vigilancia hardware : DR0-DR3 via PTRACE_SETWATCHPOINT (extension de Portix)");
        println!("  desensamblado       : decodificador x86-64 con simbolizacion de destinos");
        println!("  sesion interactiva  : --session <programa> [símbolo] [comandos] (fork+TRACEME+exec, paradas por waitpid)");
        println!("  comandos de sesion  : step|next|bt|regs|mem <dir> [n]|cond <reg> <op> <valor>|conds|cont|quit");
        println!("  condiciones (kernel): --condtest [programa] | --bcond <pid> <dir> <reg> <op> <valor> | --bcond-list <pid>");
        println!("  datos con expresión  : --wcondtest [programa] | --wcond <pid> <dir> <reg> <op> <valor> [ignore N]");
        println!("  conjunción y rango   : --rangetest [programa] | --range <pid> <dir> <reg|addr> <min> <max>");
        println!("  lenguaje completo    : --ortest [programa] (Y, O, rango, rango con guarda)");
        println!("  búfer repartido      : --buftest [programa] | --watch <pid> <dir> <hasta 32> (trozos alineados en DR0-DR3)");
        println!("  perfiles de CPU     : /proc/profiler (muestreo a 100 Hz por funcion del ELF)");
        println!("  trazas de syscalls  : /proc/syscalls (trazador TSC)");
        return 0;
    }

    if args[1] == "--session" || args[1] == "--run" {
        if args.len() < 3 { println!("usage: dbg --session <programa> [simbolo_inicial]"); return 1; }
        let objetivo = args[2].clone();
        let simbolo = if args.len() >= 4 { args[3].clone() } else { String::from("main") };
        // El shell entrega los argumentos ya separados: se recompone la lista de
        // comandos con todo lo que venga a partir del cuarto argumento, de modo
        // que `mem 0x20000000 32` llegue íntegro al intérprete.
        let comandos = if args.len() >= 5 { args[4..].join(" ") } else { String::new() };
        return cmd_session(&objetivo, &simbolo, &comandos);
    }

    if args[1] == "--trace" {
        let arg = if args.len() >= 3 { args[2].as_str() } else { "" };
        if arg.is_empty() {
            let _ = dump_proc_file("/proc/syscalls");
            return 0;
        }
        return control_tracer(arg);
    }

    if args[1] == "--ortest" {
        let objetivo = if args.len() >= 3 { args[2].clone() } else { String::from("hello") };
        return cmd_ortest(&objetivo);
    }
    if args[1] == "--buftest" {
        let objetivo = if args.len() >= 3 { args[2].clone() } else { String::from("hello") };
        return cmd_buftest(&objetivo);
    }
    if args[1] == "--rangetest" {
        let objetivo = if args.len() >= 3 { args[2].clone() } else { String::from("hello") };
        return cmd_rangetest(&objetivo);
    }
    if args[1] == "--range" {
        if args.len() < 7 {
            println!("usage: dbg --range <pid> <dirección> <registro|addr> <min> <max>");
            return 1;
        }
        let pid = match parse_pid(&args[2]) { Some(p) if p > 0 => p, _ => { println!("[dbg] pid invalido"); return 1; } };
        return cmd_range(pid, &args[3], &args[4], &args[5], &args[6]);
    }
    if args[1] == "--wcondtest" {
        let objetivo = if args.len() >= 3 { args[2].clone() } else { String::from("hello") };
        return cmd_wcondtest(&objetivo);
    }
    if args[1] == "--wcond" {
        if args.len() < 7 {
            println!("usage: dbg --wcond <pid> <dirección> <registro> <op> <valor> [ignore N]");
            return 1;
        }
        let pid = match parse_pid(&args[2]) { Some(p) if p > 0 => p, _ => { println!("[dbg] pid invalido"); return 1; } };
        let ign = if args.len() >= 8 { parse_u64(&args[7]).unwrap_or(0) as u32 } else { 0 };
        return cmd_wcond(pid, &args[3], &args[4], &args[5], &args[6], ign);
    }
    if args[1] == "--condtest" {
        let objetivo = if args.len() >= 3 { args[2].clone() } else { String::from("hello") };
        return cmd_condtest(&objetivo);
    }
    if args[1] == "--bcond" {
        if args.len() < 7 {
            println!("usage: dbg --bcond <pid> <dirección|símbolo> <registro> <op> <valor>");
            return 1;
        }
        let pid = match parse_pid(&args[2]) { Some(p) if p > 0 => p, _ => { println!("[dbg] pid invalido"); return 1; } };
        return cmd_bcond(pid, &args[3], &args[4], &args[5], &args[6]);
    }
    if args[1] == "--bcond-list" {
        if args.len() < 3 { println!("usage: dbg --bcond-list <pid>"); return 1; }
        let pid = match parse_pid(&args[2]) { Some(p) => p, _ => { println!("[dbg] pid invalido"); return 1; } };
        return cmd_bcond_list(pid);
    }
    if args[1] == "--bcond-clear" {
        if args.len() < 4 { println!("usage: dbg --bcond-clear <pid> <dirección|símbolo>"); return 1; }
        let pid = match parse_pid(&args[2]) { Some(p) => p, _ => { println!("[dbg] pid invalido"); return 1; } };
        return cmd_bcond_clear(pid, &args[3]);
    }
    if args[1] == "--break" || args[1] == "--bp" {
        if args.len() < 4 { println!("usage: dbg --break <pid> <simbolo|direccion>"); return 1; }
        let pid = match parse_pid(&args[2]) { Some(p) if p > 0 => p, _ => { println!("[dbg] pid invalido"); return 1; } };
        return cmd_break(pid, &args[3]);
    }
    if args[1] == "--bp-list" {
        if args.len() < 3 { println!("usage: dbg --bp-list <pid>"); return 1; }
        let pid = parse_pid(&args[2]).unwrap_or(0);
        return cmd_bp_list(pid);
    }
    if args[1] == "--bp-clear" {
        if args.len() < 3 { println!("usage: dbg --bp-clear <pid>"); return 1; }
        let pid = parse_pid(&args[2]).unwrap_or(0);
        return cmd_bp_clear(pid);
    }
    if args[1] == "--watch" {
        if args.len() < 5 { println!("usage: dbg --watch <pid> <simbolo|direccion> <longitud> [w|rw]"); return 1; }
        let pid = parse_pid(&args[2]).unwrap_or(0);
        let len = parse_u64(&args[4]).unwrap_or(8);
        let rw = if args.len() >= 6 && args[5] == "rw" { DR7_RW_RW } else { DR7_RW_WRITE };
        return cmd_watch(pid, &args[3], len, rw);
    }
    if args[1] == "--watch-list" {
        if args.len() < 3 { println!("usage: dbg --watch-list <pid>"); return 1; }
        return cmd_watch_list(parse_pid(&args[2]).unwrap_or(0));
    }
    if args[1] == "--watch-clear" {
        if args.len() < 3 { println!("usage: dbg --watch-clear <pid>"); return 1; }
        return cmd_watch_clear(parse_pid(&args[2]).unwrap_or(0));
    }

    // Inspección de un proceso por PID, `self` o NOMBRE
    if args[1] == "self" || args[1] == "propio" || args[1].parse::<u64>().is_err() {
        if let Some(pid) = parse_pid(&args[1]) {
            let what = if args.len() >= 3 { args[2].as_str() } else { "all" };
            let extra: Vec<String> = args.iter().skip(3).cloned().collect();
            return inspect_process(pid, what, &extra);
        }
    }
    if let Ok(pid) = args[1].parse::<u64>() {
        let what = if args.len() >= 3 { args[2].as_str() } else { "all" };
        let extra: Vec<String> = args.iter().skip(3).cloned().collect();
        return inspect_process(pid, what, &extra);
    }

    if args[1] == "-p" {
        if args.len() < 3 { println!("fatal: missing pid argument"); return 1; }
        let pid: i64 = args[2].parse().unwrap_or(0);
        if pid <= 0 { println!("fatal: invalid pid {}", args[2]); return 1; }
        println!("[dbg] Attaching to process PID={}...", pid);
        let ret = sys_ptrace(PTRACE_ATTACH, pid, 0, 0);
        if ret < 0 { println!("[dbg] Failed to attach: error {}", ret); return 1; }
        println!("[dbg] Attached to PID={}. Reading registers...", pid);
        let mut regs = UserRegs::default();
        if sys_ptrace(PTRACE_GETREGS, pid, 0, &mut regs as *mut _ as usize) == 0 {
            print_registers(&regs);
            let syms = leer_simbolos(pid as u64);
            if let Some(s) = simbolizar(&syms, regs.rip) {
                println!("[dbg] RIP resuelto: <{}>", s);
            }
        }
        let _ = sys_ptrace(PTRACE_DETACH, pid, 0, 0);
        println!("[dbg] Detached from PID={}", pid);
        return 0;
    }

    // Sesión completa bajo el depurador
    let target_bin = &args[1];
    println!("[dbg] Loading target executable '{}'...", target_bin);
    let child = fork();
    if child == 0 {
        let _ = sys_ptrace(PTRACE_TRACEME, 0, 0, 0);
        let mut path_c = Vec::from(target_bin.as_bytes());
        path_c.push(0);
        let argv: [*const u8; 2] = [path_c.as_ptr(), core::ptr::null()];
        portix_rt::syscall::sys_execve(path_c.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
        portix_rt::exit(127);
    } else if child > 0 {
        let mut status: i32 = 0;
        let _ = waitpid(child as i32, &mut status, 0);
        println!("[dbg] Process PID={} started under debugger.", child);
        let mut regs = UserRegs::default();
        let pid = child as u64;
        if sys_ptrace(PTRACE_GETREGS, child as i64, 0, &mut regs as *mut _ as usize) == 0 {
            print_registers(&regs);
            let syms = leer_simbolos(pid);
            println!("[dbg] Imagen del hijo: {} simbolos publicados por el kernel", syms.len());
            if let Some(s) = simbolizar(&syms, regs.rip) {
                println!("[dbg] Punto de entrada resuelto a <{}>", s);
            }
        }
        let bp_addr = regs.rip as usize;
        println!("[dbg] Setting software breakpoint (INT3) at RIP={:#x}...", bp_addr);
        let old_word = sys_ptrace(PTRACE_PEEKDATA, child as i64, bp_addr, 0);
        let _ = sys_ptrace(PTRACE_POKEDATA, child as i64, bp_addr, 0xCC);
        println!("[dbg] Single stepping process PID={}...", child);
        let _ = sys_ptrace(PTRACE_SINGLESTEP, child as i64, 0, 0);
        let _ = sys_ptrace(PTRACE_CONT, child as i64, 0, 0);
        let _ = old_word;
        println!("[dbg] Debug session completed successfully for PID={}.", child);
        0
    } else {
        println!("[dbg] fork() failed");
        1
    }
}
