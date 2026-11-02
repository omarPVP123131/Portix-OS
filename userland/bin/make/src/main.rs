#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use portix_rt::{
    println, exit, chdir, fork, waitpid,
    File, O_RDONLY, fs::Stat
};

struct Rule {
    targets: Vec<String>,
    prerequisites: Vec<String>,
    recipes: Vec<String>,
}

fn file_mtime(path: &str) -> Option<i64> {
    let mut null_terminated = String::from(path);
    null_terminated.push('\0');
    let mut st = Stat::default();
    let ret = portix_rt::syscall::sys_fstatat(-100, null_terminated.as_ptr(), &mut st as *mut _, 0);
    if ret == 0 {
        Some(st.st_mtime)
    } else {
        None
    }
}

fn file_exists(path: &str) -> bool {
    file_mtime(path).is_some()
}

fn expand_variables(text: &str, vars: &BTreeMap<String, String>, target: &str, first_dep: &str, all_deps: &str) -> String {
    let mut out = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'@' {
                out.push_str(target);
                i += 2;
                continue;
            } else if next == b'<' {
                out.push_str(first_dep);
                i += 2;
                continue;
            } else if next == b'^' {
                out.push_str(all_deps);
                i += 2;
                continue;
            } else if next == b'(' || next == b'{' {
                let close = if next == b'(' { b')' } else { b'}' };
                let mut j = i + 2;
                while j < bytes.len() && bytes[j] != close {
                    j += 1;
                }
                if j < bytes.len() {
                    let var_name = &text[i + 2..j];
                    if let Some(val) = vars.get(var_name) {
                        out.push_str(val);
                    }
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn execute_command(cmd: &str) -> i32 {
    let pid = fork();
    if pid < 0 {
        println!("make: fork failed");
        return 1;
    }
    if pid == 0 {
        let sh_path = b"/bin/sh\0";
        let c_flag = b"-c\0";
        let mut cmd_bytes = String::from(cmd);
        cmd_bytes.push('\0');

        let argv: [*const u8; 4] = [
            sh_path.as_ptr(),
            c_flag.as_ptr(),
            cmd_bytes.as_ptr(),
            core::ptr::null(),
        ];
        portix_rt::syscall::sys_execve(sh_path.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
        exit(127);
    }
    let mut status = 0;
    waitpid(pid as i32, &mut status, 0);
    codigo_de_estado(status)
}

/// Código de salida convencional a partir del estado POSIX de `waitpid`.
fn codigo_de_estado(st: i32) -> i32 {
    if portix_rt::wifexited(st) { portix_rt::wexitstatus(st) } else { 128 + portix_rt::wtermsig(st) }
}

struct Contexto<'a> {
    rules: &'a [Rule],
    variables: &'a BTreeMap<String, String>,
    dry_run: bool,
    always_make: bool,
}

/// Construye `objetivo` recursivamente (prerequisitos primero), como GNU make:
/// se reconstruye si no existe, si algun prerequisito es mas nuevo (mtime) o si
/// algun prerequisito se reconstruyo en esta pasada. Devuelve Ok(true) si se
/// ejecutaron recetas, Ok(false) si estaba al dia, Err(codigo) si una receta
/// fallo (y no iba precedida de `-`).
fn construir(ctx: &Contexto, objetivo: &str, profundidad: usize) -> Result<bool, i32> {
    if profundidad > 32 {
        println!("make: *** dependencia circular o demasiado profunda en '{}'.  Stop.", objetivo);
        return Err(2);
    }
    let Some(rule) = ctx.rules.iter().find(|r| r.targets.iter().any(|t| t == objetivo)) else {
        if file_exists(objetivo) { return Ok(false); }
        println!("make: *** No rule to make target '{}'.  Stop.", objetivo);
        return Err(2);
    };
    let target_mtime = if ctx.always_make { None } else { file_mtime(objetivo) };
    let mut needs_rebuild = target_mtime.is_none();
    let mut dep_reconstruido = false;
    let mut first_dep = String::new();
    let mut all_deps = String::new();
    for (idx, dep) in rule.prerequisites.iter().enumerate() {
        if idx == 0 { first_dep = dep.clone(); }
        if idx > 0 { all_deps.push(' '); }
        all_deps.push_str(dep);
        let tiene_regla = ctx.rules.iter().any(|r| r.targets.iter().any(|t| t == dep));
        if tiene_regla {
            if construir(ctx, dep, profundidad + 1)? { needs_rebuild = true; dep_reconstruido = true; }
        } else if !file_exists(dep) {
            println!("make: *** No rule to make target '{}', needed by '{}'.  Stop.", dep, objetivo);
            return Err(2);
        }
        if let (Some(dm), Some(tm)) = (file_mtime(dep), target_mtime) {
            if dm > tm { needs_rebuild = true; }
        }
    }
    // Objetivos sin prerequisitos y sin fichero (all, clean, run…) siempre se ejecutan.
    if !needs_rebuild && !(rule.prerequisites.is_empty() && target_mtime.is_none()) {
        return Ok(false);
    }
    if rule.recipes.is_empty() { return Ok(dep_reconstruido); }
    for recipe in &rule.recipes {
        let exp = expand_variables(recipe, ctx.variables, objetivo, &first_dep, &all_deps);
        let mut cmd = exp.as_str();
        let mut silent = false;
        let mut ignore_err = false;
        loop {
            if let Some(r) = cmd.strip_prefix('@') { silent = true; cmd = r; }
            else if let Some(r) = cmd.strip_prefix('-') { ignore_err = true; cmd = r; }
            else { break; }
        }
        if !silent { println!("{}", cmd); }
        if !ctx.dry_run {
            let code = execute_command(cmd);
            if code != 0 && !ignore_err {
                println!("make: *** [{}] Error {}", objetivo, code);
                return Err(code);
            }
        }
    }
    Ok(true)
}

fn parse_args(argc: i32, argv: *const *const u8) -> Vec<String> {
    let mut args = Vec::new();
    if argc > 0 && !argv.is_null() {
        for idx in 0..argc as usize {
            unsafe {
                let ptr = *argv.add(idx);
                if !ptr.is_null() {
                    let mut len = 0;
                    while *ptr.add(len) != 0 { len += 1; }
                    if let Ok(s) = core::str::from_utf8(core::slice::from_raw_parts(ptr, len)) {
                        args.push(s.to_string());
                    }
                }
            }
        }
    }
    args
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = parse_args(argc, argv);
    let mut makefile_path = String::from("Makefile");
    let mut custom_dir: Option<String> = None;
    let mut dry_run = false;
    let mut always_make = false;
    let mut requested_targets: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-f" && i + 1 < args.len() {
            makefile_path = args[i + 1].clone();
            i += 1;
        } else if arg == "-C" && i + 1 < args.len() {
            custom_dir = Some(args[i + 1].clone());
            i += 1;
        } else if arg == "-n" || arg == "--dry-run" {
            dry_run = true;
        } else if arg == "-B" || arg == "--always-make" {
            always_make = true;
        } else if arg == "-h" || arg == "--help" {
            println!("Uso: make [opciones] [objetivo] ...");
            println!("Opciones:");
            println!("  -f <archivo>    Leer <archivo> como makefile.");
            println!("  -C <directorio> Cambiar a <directorio> antes de leer el makefile.");
            println!("  -n, --dry-run   Solo imprimir los comandos sin ejecutarlos.");
            println!("  -B, --always-make Reconstruir incondicionalmente todos los objetivos.");
            println!("  -h, --help      Mostrar este mensaje de ayuda.");
            println!("  -v, --version   Mostrar la version del programa.");
            return 0;
        } else if arg == "-v" || arg == "--version" {
            println!("Portix make 1.0.0 (100% Rust) - compatible con el subconjunto basico de la sintaxis de GNU Make");
            println!("reglas, prerequisitos por mtime, variables $(X) $@ $< $^, -f -C -n -B; recetas via /bin/sh -c");
            println!("Copyright (C) 2026 Free Software Foundation / Portix Project");
            println!("Este es software libre; consulte el codigo fuente para condiciones de copia.");
            return 0;
        } else if !arg.starts_with('-') {
            requested_targets.push(arg.clone());
        }
        i += 1;
    }

    if let Some(dir) = custom_dir {
        if chdir(&dir) != 0 {
            println!("make: *** No such directory: '{}'.  Stop.", dir);
            return 2;
        }
    }

    if !file_exists(&makefile_path) {
        if file_exists("makefile") {
            makefile_path = String::from("makefile");
        } else if file_exists("GNUMakefile") {
            makefile_path = String::from("GNUMakefile");
        } else {
            println!("make: *** No targets specified and no makefile found.  Stop.");
            return 2;
        }
    }

    let mut file = match File::open(&makefile_path, O_RDONLY) {
        Ok(f) => f,
        Err(_) => {
            println!("make: *** Error opening '{}'.  Stop.", makefile_path);
            return 2;
        }
    };

    let mut content_buf = [0u8; 8192];
    let n = match file.read(&mut content_buf) {
        Ok(sz) => sz,
        Err(_) => 0,
    };
    let content = core::str::from_utf8(&content_buf[..n]).unwrap_or("");

    let mut variables: BTreeMap<String, String> = BTreeMap::new();
    variables.insert(String::from("CC"), String::from("gcc"));
    variables.insert(String::from("CXX"), String::from("g++"));
    variables.insert(String::from("AR"), String::from("ar"));
    variables.insert(String::from("RM"), String::from("rm -f"));
    variables.insert(String::from("CFLAGS"), String::from("-O2"));

    let mut rules: Vec<Rule> = Vec::new();
    let mut current_rule: Option<Rule> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        // Recipe line (starts with Tab or 2+ spaces after a rule header)
        if line.starts_with('\t') || (current_rule.is_some() && line.starts_with("  ")) {
            if let Some(ref mut rule) = current_rule {
                rule.recipes.push(trimmed.to_string());
                continue;
            }
        }

        // Variable assignment: NAME = VALUE or NAME := VALUE
        if let Some(eq_pos) = trimmed.find('=') {
            let colon_eq = eq_pos > 0 && trimmed.as_bytes()[eq_pos - 1] == b':';
            let name_end = if colon_eq { eq_pos - 1 } else { eq_pos };
            let var_name = trimmed[..name_end].trim().to_string();
            let var_val = trimmed[eq_pos + 1..].trim().to_string();

            // Only variable if not a rule line (no ':' before '=')
            let is_rule = trimmed[..eq_pos].contains(':') && !colon_eq;
            if !is_rule {
                if let Some(r) = current_rule.take() {
                    rules.push(r);
                }
                variables.insert(var_name, var_val);
                continue;
            }
        }

        // Target rule line: target [target2...]: [dep1 dep2...]
        if let Some(colon_pos) = trimmed.find(':') {
            if let Some(r) = current_rule.take() {
                rules.push(r);
            }
            // GNU make expande las variables de la linea de regla de inmediato
            // (`$(OUT): $(SRC)`), asi que aqui tambien.
            let targets_exp = expand_variables(trimmed[..colon_pos].trim(), &variables, "", "", "");
            let deps_exp = expand_variables(trimmed[colon_pos + 1..].trim(), &variables, "", "", "");
            let targets_part = targets_exp.as_str();
            let deps_part = deps_exp.as_str();

            let targets: Vec<String> = targets_part
                .split_ascii_whitespace()
                .map(|s| s.to_string())
                .collect();
            let prerequisites: Vec<String> = deps_part
                .split_ascii_whitespace()
                .map(|s| s.to_string())
                .collect();

            if !targets.is_empty() {
                current_rule = Some(Rule {
                    targets,
                    prerequisites,
                    recipes: Vec::new(),
                });
            }
        }
    }
    if let Some(r) = current_rule.take() {
        rules.push(r);
    }

    if rules.is_empty() {
        println!("make: *** No targets.  Stop.");
        return 2;
    }

    let default_target = rules[0].targets[0].clone();
    let targets_to_build = if requested_targets.is_empty() {
        alloc::vec![default_target]
    } else {
        requested_targets
    };

    let ctx = Contexto { rules: &rules, variables: &variables, dry_run, always_make };
    let mut any_built = false;
    for target_name in &targets_to_build {
        match construir(&ctx, target_name, 0) {
            Ok(true) => any_built = true,
            Ok(false) => println!("make: '{}' is up to date.", target_name),
            Err(code) => return code,
        }
    }
    if !any_built && !targets_to_build.is_empty() {
        println!("make: Nothing to be done for '{}'.", targets_to_build[0]);
    }

    0
}
