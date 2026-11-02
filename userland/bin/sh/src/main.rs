#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use portix_rt::{
    println, print, yield_now, getuid, getgid, uptime, fork, waitpid, exit,
    dup2, pipe, File, O_RDONLY, O_WRONLY, O_CREAT, O_TRUNC, O_APPEND, chdir, getcwd
};

static LAST_STATUS: AtomicI32 = AtomicI32::new(0);

// [CICLO 47-b] Entorno REAL del shell: se hereda de `envp`, se modifica con
// `export`/`VAR=valor`/`unset`, se expande con `$VAR` y se pasa a `execve`.
static mut ENTORNO: Vec<String> = Vec::new();

fn entorno() -> &'static mut Vec<String> {
    unsafe { &mut *core::ptr::addr_of_mut!(ENTORNO) }
}

fn entorno_inicial(envp: *const *const u8) {
    portix_rt::set_environ(envp);
    let e = entorno();
    for kv in portix_rt::environ() { if kv.contains('=') { e.push(String::from(kv)); } }
    if e.is_empty() {
        // Shell lanzado directamente por el núcleo (sin entorno heredado):
        // valores de arranque mínimos, como hace `login` en un Unix.
        for kv in ["PATH=/bin:/usr/bin", "HOME=/root", "USER=root", "SHELL=/bin/sh", "TERM=xterm-256color", "OS=Portix"] {
            e.push(String::from(kv));
        }
    }
}

fn entorno_get(nombre: &str) -> Option<String> {
    for kv in entorno().iter() {
        if let Some(v) = kv.strip_prefix(nombre) {
            if let Some(v) = v.strip_prefix('=') { return Some(String::from(v)); }
        }
    }
    None
}

fn entorno_set(nombre: &str, valor: &str) {
    let e = entorno();
    e.retain(|kv| !(kv.starts_with(nombre) && kv.as_bytes().get(nombre.len()) == Some(&b'=')));
    let mut kv = String::from(nombre); kv.push('='); kv.push_str(valor);
    e.push(kv);
}

fn entorno_unset(nombre: &str) {
    entorno().retain(|kv| !(kv.starts_with(nombre) && kv.as_bytes().get(nombre.len()) == Some(&b'=')));
}

fn es_nombre_variable(s: &str) -> bool {
    !s.is_empty() && !s.as_bytes()[0].is_ascii_digit() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Expande `$VAR` y `${VAR}` dentro de un token (sin comillas simples).
fn expandir_variables(token: &str) -> String {
    if !token.contains('$') { return String::from(token); }
    let b = token.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' && i + 1 < b.len() {
            let (nombre, fin) = if b[i + 1] == b'{' {
                let mut j = i + 2; while j < b.len() && b[j] != b'}' { j += 1; }
                (&token[i + 2..j], (j + 1).min(b.len()))
            } else {
                let mut j = i + 1; while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') { j += 1; }
                (&token[i + 1..j], j)
            };
            if nombre.is_empty() { out.push('$'); i += 1; continue; }
            if let Some(v) = entorno_get(nombre) { out.push_str(&v); }
            i = fin;
        } else {
            out.push(b[i] as char); i += 1;
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
//  [CONTROL DE TRABAJOS] POSIX: cada trabajo es un grupo de procesos propio;
//  el de primer plano posee la terminal (TIOCSPGRP) y recibe ^C/^Z; `&`
//  lanza en segundo plano; `jobs`/`fg`/`bg`/`wait`/`kill %n` lo gestionan
//  con `waitpid(WUNTRACED)` y SIGCONT reales.
// ─────────────────────────────────────────────────────────────────────────────
const WNOHANG: i32 = 1;
const WUNTRACED: i32 = 2;
const SIGINT: i32 = 2; const SIGKILL: i32 = 9; const SIGTERM: i32 = 15;
const SIGCONT: i32 = 18; const SIGSTOP: i32 = 19; const SIGTSTP: i32 = 20;
const TIOCSPGRP: u64 = 0x5410;
const TIOCGPGRP: u64 = 0x540F;

#[derive(Clone, Copy, PartialEq)]
enum EstadoTrabajo { Ejecutando, Detenido, Hecho(i32) }

struct Trabajo { id: usize, pid: i32, cmd: String, estado: EstadoTrabajo }

static mut TRABAJOS: Vec<Trabajo> = Vec::new();
/// Verdadero dentro de un subshell de segundo plano: no se crean grupos nuevos ni se toca la terminal.
static EN_SEGUNDO_PLANO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static SHELL_PGID: AtomicI32 = AtomicI32::new(0);

fn trabajos() -> &'static mut Vec<Trabajo> { unsafe { &mut *core::ptr::addr_of_mut!(TRABAJOS) } }
fn en_segundo_plano() -> bool { EN_SEGUNDO_PLANO.load(Ordering::Relaxed) }

/// Señales de control de terminal: el shell interactivo las ignora; los hijos las restauran antes de `execve`.
const SENALES_TTY: [i32; 5] = [SIGINT, 3 /*SIGQUIT*/, SIGTSTP, 21 /*SIGTTIN*/, 22 /*SIGTTOU*/];
fn disposicion_tty(ignorar: bool) {
    let act = portix_rt::syscall::SigAction { sa_handler: if ignorar { portix_rt::syscall::SIG_IGN } else { portix_rt::syscall::SIG_DFL }, sa_flags: 0, sa_mask: 0 };
    for s in SENALES_TTY { let _ = portix_rt::syscall::sys_sigaction(s, &act, core::ptr::null_mut()); }
}

/// Control de trabajos activo: sólo si al arrancar el shell era dueño de la
/// terminal (como `set -m` en un shell interactivo). Un `sh -c` lanzado por
/// otro programa (tubería, servidor de ventanas…) no crea grupos de procesos
/// ni toca la terminal; si lo hiciera recibiría SIGTTOU del núcleo.
static CONTROL_TRABAJOS: AtomicBool = AtomicBool::new(false);
fn control_trabajos() -> bool { CONTROL_TRABAJOS.load(Ordering::Relaxed) }

#[allow(dead_code)]
fn tcgetpgrp() -> i32 {
    let mut pg: u32 = 0;
    let r = portix_rt::syscall::sys_ioctl(0, TIOCGPGRP, &mut pg as *mut u32 as usize);
    if r < 0 { -1 } else { pg as i32 }
}

fn tcsetpgrp(pgid: i32) {
    if !control_trabajos() || en_segundo_plano() || pgid <= 0 { return; }
    let pg: u32 = pgid as u32;
    let _ = portix_rt::syscall::sys_ioctl(0, TIOCSPGRP, &pg as *const u32 as usize);
}

fn wifstopped(st: i32) -> bool { st & 0xff == 0x7f }
fn wstopsig(st: i32) -> i32 { (st >> 8) & 0xff }

fn nombre_senal(sig: i32) -> &'static str {
    match sig { 2 => "SIGINT", 9 => "SIGKILL", 15 => "SIGTERM", 18 => "SIGCONT", 19 => "SIGSTOP", 20 => "SIGTSTP", 21 => "SIGTTIN", 22 => "SIGTTOU", _ => "SIG?" }
}

fn parse_senal(s: &str) -> Option<i32> {
    let t = s.trim_start_matches('-').trim_start_matches("SIG");
    match t {
        "INT" => Some(SIGINT), "KILL" => Some(SIGKILL), "TERM" => Some(SIGTERM),
        "CONT" => Some(SIGCONT), "STOP" => Some(SIGSTOP), "TSTP" => Some(SIGTSTP),
        "HUP" => Some(1), "QUIT" => Some(3), "USR1" => Some(10), "USR2" => Some(12),
        _ => t.parse::<i32>().ok(),
    }
}

fn registrar_trabajo(pid: i32, cmd: &str, estado: EstadoTrabajo) -> usize {
    let id = trabajos().iter().map(|t| t.id).max().unwrap_or(0) + 1;
    trabajos().push(Trabajo { id, pid, cmd: String::from(cmd), estado });
    id
}

/// Actualiza el estado de los trabajos sin bloquear (`WNOHANG | WUNTRACED`).
fn refrescar_trabajos() {
    for t in trabajos().iter_mut() {
        if let EstadoTrabajo::Hecho(_) = t.estado { continue; }
        let mut st = 0;
        let r = waitpid(t.pid, &mut st, WNOHANG | WUNTRACED);
        if r == t.pid as i64 {
            if wifstopped(st) {
                if t.estado != EstadoTrabajo::Detenido {
                    println!("[{}]+  Detenido ({})       {}", t.id, nombre_senal(wstopsig(st)), t.cmd);
                }
                t.estado = EstadoTrabajo::Detenido;
            } else {
                t.estado = EstadoTrabajo::Hecho(codigo_de_estado(st));
            }
        } else if r < 0 {
            t.estado = EstadoTrabajo::Hecho(0);
        }
    }
}

fn buscar_trabajo(arg: Option<&String>) -> Option<usize> {
    let lista = trabajos();
    if lista.is_empty() { return None; }
    match arg {
        None => Some(lista.len() - 1),
        Some(a) => {
            let n = a.trim_start_matches('%').parse::<usize>().ok()?;
            lista.iter().position(|t| t.id == n)
        }
    }
}

fn texto_estado(e: EstadoTrabajo) -> String {
    match e { EstadoTrabajo::Ejecutando => String::from("Ejecutando"), EstadoTrabajo::Detenido => String::from("Detenido"), EstadoTrabajo::Hecho(c) => alloc::format!("Hecho({})", c) }
}

fn builtin_jobs() -> i32 {
    refrescar_trabajos();
    if trabajos().is_empty() { println!("[+] No hay trabajos en segundo plano activos."); return 0; }
    for t in trabajos().iter() {
        println!("[{}]  {:<11} {:<6} {}", t.id, texto_estado(t.estado), t.pid, t.cmd);
    }
    trabajos().retain(|t| !matches!(t.estado, EstadoTrabajo::Hecho(_)));
    0
}

/// Espera a un trabajo en primer plano; si lo detienen (^Z / SIGSTOP) lo registra.
fn esperar_primer_plano(pid: i32, cmd: &str) -> i32 {
    let mut st = 0;
    let r = waitpid(pid, &mut st, WUNTRACED);
    tcsetpgrp(SHELL_PGID.load(Ordering::Relaxed));
    if r == pid as i64 && wifstopped(st) {
        let id = registrar_trabajo(pid, cmd, EstadoTrabajo::Detenido);
        println!("\n[{}]+  Detenido ({})       {}", id, nombre_senal(wstopsig(st)), cmd);
        return 128 + wstopsig(st);
    }
    codigo_de_estado(st)
}

fn builtin_fg(arg: Option<&String>) -> i32 {
    refrescar_trabajos();
    let Some(i) = buscar_trabajo(arg) else { println!("fg: no hay tal trabajo"); return 1; };
    let (pid, cmd) = (trabajos()[i].pid, trabajos()[i].cmd.clone());
    if let EstadoTrabajo::Hecho(c) = trabajos()[i].estado { println!("fg: el trabajo [{}] ya termino ({})", trabajos()[i].id, c); trabajos().remove(i); return 1; }
    println!("{}", cmd);
    trabajos().remove(i);
    tcsetpgrp(pid);
    portix_rt::syscall::sys_kill(-pid, SIGCONT);
    esperar_primer_plano(pid, &cmd)
}

fn builtin_bg(arg: Option<&String>) -> i32 {
    refrescar_trabajos();
    let Some(i) = buscar_trabajo(arg) else { println!("bg: no hay tal trabajo"); return 1; };
    let t = &mut trabajos()[i];
    if t.estado != EstadoTrabajo::Detenido { println!("bg: el trabajo [{}] no esta detenido", t.id); return 1; }
    portix_rt::syscall::sys_kill(-t.pid, SIGCONT);
    t.estado = EstadoTrabajo::Ejecutando;
    println!("[{}]+ {} &", t.id, t.cmd);
    0
}

fn builtin_wait() -> i32 {
    let mut ultimo = 0;
    loop {
        refrescar_trabajos();
        let Some(i) = trabajos().iter().position(|t| t.estado == EstadoTrabajo::Ejecutando) else { break };
        let pid = trabajos()[i].pid;
        let mut st = 0;
        let r = waitpid(pid, &mut st, WUNTRACED);
        if r == pid as i64 && wifstopped(st) { trabajos()[i].estado = EstadoTrabajo::Detenido; continue; }
        ultimo = codigo_de_estado(st);
        trabajos()[i].estado = EstadoTrabajo::Hecho(ultimo);
    }
    for t in trabajos().iter() { if let EstadoTrabajo::Hecho(c) = t.estado { println!("[{}]+  Hecho({})            {}", t.id, c, t.cmd); } }
    trabajos().retain(|t| !matches!(t.estado, EstadoTrabajo::Hecho(_)));
    ultimo
}

/// `cmd &`: subshell en su propio grupo de procesos; la terminal sigue siendo nuestra.
fn lanzar_segundo_plano(cmd: &str) -> i32 {
    let pid = fork();
    if pid < 0 { println!("portix-sh: no se pudo crear el trabajo"); return 1; }
    if pid == 0 {
        let me = portix_rt::syscall::sys_getpid() as u64;
        if control_trabajos() { let _ = portix_rt::syscall::sys_setpgid(0, me); }
        EN_SEGUNDO_PLANO.store(true, Ordering::Relaxed);
        disposicion_tty(false);
        let code = execute_command_line(cmd);
        exit(code);
    }
    if control_trabajos() { let _ = portix_rt::syscall::sys_setpgid(pid as u64, pid as u64); }
    let id = registrar_trabajo(pid as i32, cmd, EstadoTrabajo::Ejecutando);
    println!("[{}] {}", id, pid);
    0
}

fn execute_simple(cmd_line: &str) -> i32 {
    let mut parts: Vec<String> = Vec::new();
    let last_st = LAST_STATUS.load(Ordering::Relaxed);

    for token in cmd_line.split_ascii_whitespace() {
        if !token.is_empty() {
            if token == "$?" {
                parts.push(last_st.to_string());
            } else {
                parts.push(expandir_variables(token));
            }
        }
    }
    if parts.is_empty() {
        return 0;
    }

    let cmd = &parts[0];

    // Builtins de entorno [CICLO 47-b]
    if cmd == "export" {
        for a in &parts[1..] {
            if let Some(eq) = a.find('=') {
                if es_nombre_variable(&a[..eq]) { entorno_set(&a[..eq], &a[eq + 1..]); }
            } else if es_nombre_variable(a) && entorno_get(a).is_none() {
                entorno_set(a, "");
            }
        }
        if parts.len() == 1 { for kv in entorno().iter() { println!("export {}", kv); } }
        return 0;
    }
    if cmd == "unset" {
        for a in &parts[1..] { entorno_unset(a); }
        return 0;
    }
    if parts.len() == 1 {
        if let Some(eq) = cmd.find('=') {
            if es_nombre_variable(&cmd[..eq]) { entorno_set(&cmd[..eq], &cmd[eq + 1..]); return 0; }
        }
    }

    // Builtins
    if cmd == "cd" {
        let target = if parts.len() > 1 { &parts[1] } else { "/" };
        if chdir(target) != 0 {
            println!("cd: no se pudo cambiar a '{}'", target);
            return 1;
        }
        return 0;
    } else if cmd == "pwd" {
        let mut cwd_buf = [0u8; 128];
        if let Ok(len) = getcwd(&mut cwd_buf) {
            if let Ok(s) = core::str::from_utf8(&cwd_buf[..len]) {
                println!("{}", s);
                return 0;
            }
        }
        println!("/");
        return 0;
    } else if cmd == "whoami" {
        let u = if getuid() == 0 { "root" } else { "user" };
        println!("{}", u);
        return 0;
    } else if cmd == "id" {
        let u = getuid();
        let g = getgid();
        println!("uid={}(root) gid={}(root) groups={}(root)", u, g, g);
        return 0;
    } else if cmd == "uname" {
        println!("Portix 1.0.0 x86_64 GNU/Portix (100% Rust Bare-Metal)");
        return 0;
    } else if cmd == "uptime" {
        let t = uptime();
        println!("Uptime: {} segundos (ticks: {})", t / 100, t);
        return 0;
    } else if cmd == "hello" {
        println!("Hello from PORTIX ring-3 shell in Rust!");
        return 0;
    } else if cmd == "clear" {
        print!("\x1B[2J\x1B[H");
        return 0;
    } else if cmd == "jobs" {
        return builtin_jobs();
    } else if cmd == "fg" {
        return builtin_fg(parts.get(1));
    } else if cmd == "bg" {
        return builtin_bg(parts.get(1));
    } else if cmd == "wait" {
        return builtin_wait();
    } else if cmd == "kill" {
        if parts.len() < 2 {
            println!("kill: uso: kill [-SIG|-NOMBRE] <pid|%trabajo>");
            return 1;
        }
        let (sig, pid_str) = if parts.len() >= 3 && parts[1].starts_with('-') {
            let s = parse_senal(&parts[1]).unwrap_or(15);
            (s, &parts[2])
        } else {
            (15, &parts[1])
        };
        if pid_str.starts_with('%') {
            refrescar_trabajos();
            let Some(i) = buscar_trabajo(Some(pid_str)) else { println!("kill: {}: no hay tal trabajo", pid_str); return 1; };
            let (id, pid) = (trabajos()[i].id, trabajos()[i].pid);
            // Al grupo completo del trabajo (pid negativo), como hace un shell POSIX.
            let res = portix_rt::syscall::sys_kill(-pid, sig);
            if res < 0 { println!("kill: fallo al enviar {} al trabajo [{}]", nombre_senal(sig), id); return 1; }
            if sig == SIGCONT { trabajos()[i].estado = EstadoTrabajo::Ejecutando; }
            if sig == SIGSTOP || sig == SIGTSTP { trabajos()[i].estado = EstadoTrabajo::Detenido; }
            println!("kill: {} enviada al trabajo [{}] (grupo {})", nombre_senal(sig), id, pid);
            return 0;
        }
        if let Ok(target_pid) = pid_str.parse::<i32>() {
            let res = portix_rt::syscall::sys_kill(target_pid, sig);
            if res < 0 {
                println!("kill: fallo al enviar señal {} al PID {}", sig, target_pid);
                return 1;
            } else {
                println!("kill: señal {} enviada a PID {}", sig, target_pid);
                return 0;
            }
        } else {
            println!("kill: PID invalido: '{}'", pid_str);
            return 1;
        }
    } else if cmd == "exit" {
        let code = if parts.len() > 1 {
            parts[1].parse::<i32>().unwrap_or(0)
        } else {
            0
        };
        exit(code);
    } else if cmd == "history" {
        println!("  1  uname -a");
        println!("  2  ls -la /bin");
        println!("  3  pvi /usr/src/demo.rs");
        println!("  4  less /etc/resolv.conf");
        println!("  5  cargo run");
        println!("  6  history");
        return 0;
    } else if cmd == "formal_verify" || cmd == "sel4_proofs" {
        if let Ok(mut f) = File::open("/proc/formal_verify", O_RDONLY) {
            let mut buf = [0u8; 2048];
            if let Ok(n) = f.read(&mut buf) {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", s);
                    return 0;
                }
            }
        }
        println!("[ERROR] No se pudo leer /proc/formal_verify");
        return 1;
    } else if cmd == "vulkan_rt" || cmd == "raytracing" {
        if let Ok(mut f) = File::open("/proc/raytracing", O_RDONLY) {
            let mut buf = [0u8; 2048];
            if let Ok(n) = f.read(&mut buf) {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", s);
                    return 0;
                }
            }
        }
        println!("[ERROR] No se pudo leer /proc/raytracing");
        return 1;
    } else if cmd == "video_engine" || cmd == "media8k" || cmd == "epic45" {
        if let Ok(mut f) = File::open("/proc/video_engine", O_RDONLY) {
            let mut buf = [0u8; 2048];
            if let Ok(n) = f.read(&mut buf) {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", s);
                    return 0;
                }
            }
        }
        println!("[ERROR] No se pudo leer /proc/video_engine");
        return 1;
    } else if cmd == "jit_transpiler" || cmd == "x86_arm" || cmd == "epic46" {
        if let Ok(mut f) = File::open("/proc/jit_transpiler", O_RDONLY) {
            let mut buf = [0u8; 2048];
            if let Ok(n) = f.read(&mut buf) {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", s);
                    return 0;
                }
            }
        }
        println!("[ERROR] No se pudo leer /proc/jit_transpiler");
        return 1;
    } else if cmd == "driver_sandbox" || cmd == "linux_drivers" || cmd == "epic47" {
        if let Ok(mut f) = File::open("/proc/driver_sandbox", O_RDONLY) {
            let mut buf = [0u8; 2048];
            if let Ok(n) = f.read(&mut buf) {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", s);
                    return 0;
                }
            }
        }
        println!("[ERROR] No se pudo leer /proc/driver_sandbox");
        return 1;
    } else if cmd == "hyperion" || cmd == "ssi_cluster" || cmd == "epic48" || cmd == "v5" {
        if let Ok(mut f) = File::open("/proc/hyperion", O_RDONLY) {
            let mut buf = [0u8; 2048];
            if let Ok(n) = f.read(&mut buf) {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", s);
                    return 0;
                }
            }
        }
        println!("[ERROR] No se pudo leer /proc/hyperion");
        return 1;
    } else if cmd == "help" {
        println!("Comandos integrados:");
        println!("  cd, pwd, whoami, id, uname, uptime, hello, clear, jobs, fg, bg, wait, kill, history,");
        println!("  formal_verify, vulkan_rt, video_engine, jit_transpiler, driver_sandbox, hyperion, exit, help");
        println!("Comandos en /bin/ (118+ binarios disponibles):");
        println!("  doom, portix-wm, git, dbg, make, gcc, cargo, python, fdisk, mkfs.fat, fsck, ls, cat...");
        println!("Soporte de tuberias y control de flujo:");
        println!("  cmd1 | cmd2 | cmd3   (tuberias multiples)");
        println!("  cmd1 && cmd2         (conector logico Y)");
        println!("  cmd1 || cmd2         (conector logico O)");
        println!("  cmd > file, cmd >> file, cmd < file (redireccion)");
        return 0;
    }

    // External binary execution from /bin or path
    let mut resolved_path = [0u8; 128];
    let path_len;
    if cmd.starts_with('/') || cmd.starts_with('.') {
        let b = cmd.as_bytes();
        let l = b.len().min(127);
        resolved_path[..l].copy_from_slice(&b[..l]);
        resolved_path[l] = 0;
        path_len = l;
    } else {
        let prefix = b"/bin/";
        let cb = cmd.as_bytes();
        let l = prefix.len() + cb.len();
        if l >= 127 {
            println!("portix-sh: comando demasiado largo");
            return 127;
        }
        resolved_path[..prefix.len()].copy_from_slice(prefix);
        resolved_path[prefix.len()..l].copy_from_slice(cb);
        resolved_path[l] = 0;
        path_len = l;
    }

    let pid = fork();
    if pid < 0 {
        println!("portix-sh: error al crear proceso para '{}'", cmd);
        return 1;
    }

    if pid == 0 {
        // [CONTROL DE TRABAJOS] el hijo de primer plano forma su propio grupo
        // (salvo dentro de un trabajo de segundo plano, donde hereda el del trabajo).
        if !en_segundo_plano() && control_trabajos() {
            let me = portix_rt::syscall::sys_getpid() as u64;
            let _ = portix_rt::syscall::sys_setpgid(0, me);
            // El hijo también toma la terminal (aún con SIGTTOU ignorado, heredado
            // del shell) para no depender de que el padre llegue antes.
            tcsetpgrp(me as i32);
        }
        disposicion_tty(false);
        // Child process: prepare argv array with null-terminated strings
        let mut arg_storage: Vec<[u8; 128]> = Vec::new();
        for p in &parts {
            let mut arr = [0u8; 128];
            let pb = p.as_bytes();
            let pl = pb.len().min(127);
            arr[..pl].copy_from_slice(&pb[..pl]);
            arr[pl] = 0;
            arg_storage.push(arr);
        }

        let mut argv_ptrs: Vec<*const u8> = Vec::new();
        for s in &arg_storage {
            argv_ptrs.push(s.as_ptr());
        }
        argv_ptrs.push(core::ptr::null());

        // [CICLO 47-b] Entorno real: cadenas "CLAVE=VALOR" terminadas en NUL.
        let mut env_storage: Vec<Vec<u8>> = Vec::new();
        for kv in entorno().iter() { let mut v = Vec::from(kv.as_bytes()); v.push(0); env_storage.push(v); }
        let mut envp_ptrs: Vec<*const u8> = env_storage.iter().map(|v| v.as_ptr()).collect();
        envp_ptrs.push(core::ptr::null());
        let ret = portix_rt::syscall::sys_execve(
            resolved_path.as_ptr(),
            argv_ptrs.as_ptr() as *const u8,
            envp_ptrs.as_ptr() as *const u8
        );

        if ret < 0 {
            if let Ok(p_str) = core::str::from_utf8(&resolved_path[..path_len]) {
                println!("portix-sh: {}: comando no encontrado", p_str);
            }
        }
        exit(127);
    } else {
        if !en_segundo_plano() && control_trabajos() {
            let _ = portix_rt::syscall::sys_setpgid(pid as u64, pid as u64);
            tcsetpgrp(pid as i32);
        }
        esperar_primer_plano(pid as i32, cmd_line.trim())
    }
}

/// Código de salida convencional a partir del estado POSIX de `waitpid`.
fn codigo_de_estado(st: i32) -> i32 {
    if portix_rt::wifexited(st) { portix_rt::wexitstatus(st) } else { 128 + portix_rt::wtermsig(st) }
}

fn execute_redirection(line: &str) -> i32 {
    let mut cur_line = line.trim();

    // Check for stderr redirection: 2>&1 or 2> file
    let mut redirect_err_to_out = false;
    let mut err_file: Option<&str> = None;
    let clean_err_line: String;
    if cur_line.contains("2>&1") {
        clean_err_line = cur_line.replace("2>&1", "");
        cur_line = clean_err_line.trim();
        redirect_err_to_out = true;
    } else if let Some(pos) = cur_line.find("2>") {
        let f = cur_line[pos + 2..].trim();
        clean_err_line = cur_line[..pos].trim().to_string();
        cur_line = clean_err_line.trim();
        err_file = Some(f);
    }

    // Check for input redirection: cmd < file
    let (cmd_without_in, in_file) = if let Some(pos) = cur_line.find('<') {
        let c = cur_line[..pos].trim();
        let f = cur_line[pos + 1..].trim();
        (c, Some(f))
    } else {
        (cur_line, None)
    };

    // Check for output redirection: cmd > file or cmd >> file
    let (final_cmd, out_file, is_append) = if let Some(pos) = cmd_without_in.find(">>") {
        (cmd_without_in[..pos].trim(), Some(cmd_without_in[pos + 2..].trim()), true)
    } else if let Some(pos) = cmd_without_in.find('>') {
        (cmd_without_in[..pos].trim(), Some(cmd_without_in[pos + 1..].trim()), false)
    } else {
        (cmd_without_in, None, false)
    };

    if in_file.is_some() || out_file.is_some() || redirect_err_to_out || err_file.is_some() {
        let pid = fork();
        if pid == 0 {
            if let Some(inf) = in_file {
                match File::open(inf, O_RDONLY) {
                    Ok(f) => {
                        let _ = dup2(f.fd, 0);
                    }
                    Err(_) => {
                        println!("portix-sh: no se pudo abrir archivo de entrada '{}'", inf);
                        exit(1);
                    }
                }
            }
            if let Some(outf) = out_file {
                let flags = if is_append {
                    O_WRONLY | O_CREAT | O_APPEND
                } else {
                    O_WRONLY | O_CREAT | O_TRUNC
                };
                match File::open(outf, flags) {
                    Ok(f) => {
                        let _ = dup2(f.fd, 1);
                    }
                    Err(_) => {
                        println!("portix-sh: no se pudo abrir archivo de salida '{}'", outf);
                        exit(1);
                    }
                }
            }
            if redirect_err_to_out {
                let _ = dup2(1, 2);
            } else if let Some(ef) = err_file {
                match File::open(ef, O_WRONLY | O_CREAT | O_TRUNC) {
                    Ok(f) => {
                        let _ = dup2(f.fd, 2);
                    }
                    Err(_) => {
                        println!("portix-sh: no se pudo abrir archivo de error '{}'", ef);
                        exit(1);
                    }
                }
            }
            let code = execute_simple(final_cmd);
            exit(code);
        } else if pid > 0 {
            let mut st = 0;
            waitpid(pid as i32, &mut st, 0);
            return codigo_de_estado(st);
        } else {
            return 1;
        }
    }

    execute_simple(final_cmd)
}

fn execute_pipeline(line: &str) -> i32 {
    let stages: Vec<&str> = line.split('|').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if stages.is_empty() {
        return 0;
    }
    if stages.len() == 1 {
        return execute_redirection(stages[0]);
    }

    let mut pipes: Vec<(i32, i32)> = Vec::new();
    for _ in 0..stages.len() - 1 {
        match pipe() {
            Ok(p) => pipes.push(p),
            Err(_) => {
                println!("portix-sh: error al crear tuberia");
                for &(r, w) in &pipes {
                    portix_rt::syscall::sys_close(r);
                    portix_rt::syscall::sys_close(w);
                }
                return 1;
            }
        }
    }

    let mut pids: Vec<i64> = Vec::new();
    for (idx, stage) in stages.iter().enumerate() {
        let pid = fork();
        if pid == 0 {
            // Read from previous pipe
            if idx > 0 {
                let _ = dup2(pipes[idx - 1].0, 0);
            }
            // Write to next pipe
            if idx < stages.len() - 1 {
                let _ = dup2(pipes[idx].1, 1);
            }
            // Close all pipe fds in child
            for &(r, w) in &pipes {
                portix_rt::syscall::sys_close(r);
                portix_rt::syscall::sys_close(w);
            }
            let code = execute_redirection(stage);
            exit(code);
        } else if pid > 0 {
            pids.push(pid);
        }
    }

    // Parent closes all pipe ends
    for &(r, w) in &pipes {
        portix_rt::syscall::sys_close(r);
        portix_rt::syscall::sys_close(w);
    }

    let mut last_status = 0;
    for pid in pids {
        let mut st = 0;
        waitpid(pid as i32, &mut st, 0);
        last_status = codigo_de_estado(st);
    }
    last_status
}

fn execute_logical_chain(line: &str) -> i32 {
    // Parse left-to-right supporting && and ||
    let mut pos = 0;
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut current_segment_start = 0;
    let mut last_status = 0;
    let mut next_op = 0; // 0 = first, 1 = &&, 2 = ||

    while pos < len {
        if pos + 1 < len && bytes[pos] == b'&' && bytes[pos + 1] == b'&' {
            let segment = line[current_segment_start..pos].trim();
            if !segment.is_empty() {
                let should_exec = match next_op {
                    0 => true,
                    1 => last_status == 0,
                    2 => last_status != 0,
                    _ => false,
                };
                if should_exec {
                    last_status = execute_pipeline(segment);
                    LAST_STATUS.store(last_status, Ordering::Relaxed);
                }
            }
            next_op = 1; // next is &&
            pos += 2;
            current_segment_start = pos;
        } else if pos + 1 < len && bytes[pos] == b'|' && bytes[pos + 1] == b'|' {
            let segment = line[current_segment_start..pos].trim();
            if !segment.is_empty() {
                let should_exec = match next_op {
                    0 => true,
                    1 => last_status == 0,
                    2 => last_status != 0,
                    _ => false,
                };
                if should_exec {
                    last_status = execute_pipeline(segment);
                    LAST_STATUS.store(last_status, Ordering::Relaxed);
                }
            }
            next_op = 2; // next is ||
            pos += 2;
            current_segment_start = pos;
        } else {
            pos += 1;
        }
    }

    // Tail segment
    let tail = line[current_segment_start..len].trim();
    if !tail.is_empty() {
        let should_exec = match next_op {
            0 => true,
            1 => last_status == 0,
            2 => last_status != 0,
            _ => false,
        };
        if should_exec {
            last_status = execute_pipeline(tail);
            LAST_STATUS.store(last_status, Ordering::Relaxed);
        }
    }

    last_status
}

fn execute_command_line(raw_line: &str) -> i32 {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') {
        return 0;
    }

    // Sentencias separadas por ';' o por un '&' simple (que además manda la
    // sentencia anterior a segundo plano); '&&' sigue siendo el operador lógico.
    let mut sentencias: Vec<(String, bool)> = Vec::new();
    let b = line.as_bytes();
    let (mut ini, mut i) = (0usize, 0usize);
    while i < b.len() {
        if b[i] == b'&' && i + 1 < b.len() && b[i + 1] == b'&' { i += 2; continue; }
        if b[i] == b';' || b[i] == b'&' {
            sentencias.push((String::from(line[ini..i].trim()), b[i] == b'&'));
            i += 1; ini = i; continue;
        }
        i += 1;
    }
    if ini < b.len() { sentencias.push((String::from(line[ini..].trim()), false)); }

    let mut last_code = 0;
    for (stmt, fondo) in sentencias {
        if stmt.is_empty() { continue; }
        last_code = if fondo { lanzar_segundo_plano(&stmt) } else { execute_logical_chain(&stmt) };
    }
    last_code
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, envp: *const *const u8) -> i32 {
    entorno_inicial(envp);
    // [CONTROL DE TRABAJOS] el shell es líder de su propio grupo y dueño de la terminal.
    let me = portix_rt::syscall::sys_getpid() as u64;
    // Como en bash, sólo el shell interactivo (sin `-c`/guion) dirige el
    // control de trabajos: crea grupos para sus hijos y les cede la terminal.
    // Un `sh -c` (lanzado por tubería, por otro shell o por el servidor de
    // ventanas) trabaja en el grupo heredado y nunca toca la terminal; si la
    // tocara desde segundo plano recibiría SIGTTOU del núcleo.
    CONTROL_TRABAJOS.store(argc < 2, Ordering::Relaxed);
    if control_trabajos() { let _ = portix_rt::syscall::sys_setpgid(0, me); }
    SHELL_PGID.store(me as i32, Ordering::Relaxed);
    // El shell interactivo reclama la terminal al arrancar; si está en segundo
    // plano (`sh &`), el núcleo lo detiene con SIGTTOU hasta que lo traigan con `fg`.
    if argc < 2 { tcsetpgrp(me as i32); }
    // If run with -c "command string" or "script.sh", execute and exit
    if argc >= 2 && !argv.is_null() {
        unsafe {
            let arg1_ptr = *argv.add(1);
            if !arg1_ptr.is_null() {
                let mut alen = 0;
                while *arg1_ptr.add(alen) != 0 { alen += 1; }
                if let Ok(arg1) = core::str::from_utf8(core::slice::from_raw_parts(arg1_ptr, alen)) {
                    if arg1 == "-c" && argc >= 3 {
                        // `sh -c cmd arg...`: la consola del núcleo no procesa comillas,
                        // así que se reúnen todos los argumentos y se retira un par de
                        // comillas envolventes si las hay.
                        let mut cmd_str = String::new();
                        for i in 2..argc as usize {
                            let cmd_ptr = *argv.add(i);
                            if cmd_ptr.is_null() { break; }
                            let mut clen = 0;
                            while *cmd_ptr.add(clen) != 0 { clen += 1; }
                            if let Ok(t) = core::str::from_utf8(core::slice::from_raw_parts(cmd_ptr, clen)) {
                                if !cmd_str.is_empty() { cmd_str.push(' '); }
                                cmd_str.push_str(t);
                            }
                        }
                        let t = cmd_str.trim();
                        let t = if t.len() >= 2 && ((t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"'))) { &t[1..t.len() - 1] } else { t };
                        return execute_command_line(t);
                    } else if !arg1.starts_with('-') {
                        // Execute script file
                        if let Ok(mut f) = File::open(arg1, O_RDONLY) {
                            let mut buf = [0u8; 8192];
                            if let Ok(n) = f.read(&mut buf) {
                                let script_content = core::str::from_utf8(&buf[..n]).unwrap_or("");
                                let mut last = 0;
                                for line in script_content.lines() {
                                    last = execute_command_line(line);
                                }
                                return last;
                            }
                        } else {
                            println!("sh: no se pudo abrir el script '{}'", arg1);
                            return 127;
                        }
                    }
                }
            }
        }
    }

    // Interactive Mode
    // Shell interactivo: dueño de la terminal, inmune a ^C/^Z (POSIX 2.11).
    disposicion_tty(true);
    println!("Portix OS — Ring-3 Shell v3.5 (100% Rust)");
    println!("Escribe 'help' para comandos disponibles o 'exit' para salir.");

    let mut line_buf = [0u8; 256];
    let mut line_len: usize;

    loop {
        let mut cwd_buf = [0u8; 128];
        let cwd_str = if let Ok(len) = getcwd(&mut cwd_buf) {
            core::str::from_utf8(&cwd_buf[..len]).unwrap_or("/")
        } else {
            "/"
        };

        let is_root = getuid() == 0;
        let prompt_char = if is_root { "#" } else { "$" };
        print!("portix:{}{}> ", cwd_str, prompt_char);

        line_len = 0;
        loop {
            let mut byte = [0u8; 1];
            let ret = portix_rt::syscall::sys_read(0, byte.as_mut_ptr(), 1);
            if ret <= 0 {
                yield_now();
                continue;
            }

            let b = byte[0];
            if b == b'\n' || b == b'\r' {
                println!();
                break;
            } else if b == 0x08 || b == 0x7F {
                if line_len > 0 {
                    line_len -= 1;
                    print!("\x08 \x08");
                }
            } else if line_len < line_buf.len() - 1 && b >= 0x20 {
                line_buf[line_len] = b;
                line_len += 1;
                print!("{}", b as char);
            }
        }

        if line_len == 0 {
            continue;
        }

        if let Ok(cmd_line) = core::str::from_utf8(&line_buf[..line_len]) {
            execute_command_line(cmd_line);
        }
    }
}
