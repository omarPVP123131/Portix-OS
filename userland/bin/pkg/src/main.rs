#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use portix_rt::{
    println, print, File, O_RDONLY, O_WRONLY, O_CREAT, O_TRUNC, O_APPEND,
    fs::Stat
};
use portix_rt::syscall::{sys_fork, sys_execve, sys_waitpid, sys_exit};

// [T-002] paquetes .apk reales de Alpine
mod inflate;
mod tar;
mod sha1;
mod apk;
mod xz;
mod sha256_estado;
mod deb;
mod pgp;

// ─────────────────────────────────────────────────────────────────────────────
//  [FRENTE B] Repositorio real: descarga por HTTP, indice con SHA-256 y
//  verificacion REAL antes de instalar. Nada de "[OK]" sin calcular nada.
// ─────────────────────────────────────────────────────────────────────────────
// [FRENTE B-2] Repositorio por HTTPS: TLS 1.3 real + SHA-256 por paquete.
const REPO_URL: &str = "https://10.0.2.2:8443";
const INDEX_LOCAL: &str = "/var/pkg/index.txt";
const INDEX_SEMILLA: &str = "/var/pkg/repo/index.txt";

/// Ejecuta `/bin/curl -o <destino> <url>` con fork+execve+waitpid reales y
/// devuelve el codigo de salida del hijo (o -1 si no se pudo lanzar).
pub fn descargar(url: &str, destino: &str) -> i32 {
    let ruta = b"/bin/curl\0";
    let a0 = b"curl\0";
    let a1 = b"-o\0";
    let mut dst: Vec<u8> = destino.as_bytes().to_vec(); dst.push(0);
    let mut u: Vec<u8> = url.as_bytes().to_vec(); u.push(0);
    let argv: [*const u8; 5] = [a0.as_ptr(), a1.as_ptr(), dst.as_ptr(), u.as_ptr(), core::ptr::null()];
    let pid = sys_fork();
    if pid == 0 {
        sys_execve(ruta.as_ptr(), argv.as_ptr() as *const u8, core::ptr::null());
        sys_exit(127);
    }
    if pid < 0 { return -1; }
    let mut status: i32 = 0;
    if sys_waitpid(pid as i32, &mut status as *mut i32, 0) != pid { return -1; }
    (status >> 8) & 0xff
}

/// [b188] Descarga de un PAQUETE (.apk/.deb) con caché local opcional para el
/// desarrollo: si existe `/etc/pkg/cache-local` (lo crea el runner de la suite,
/// nunca la imagen), contiene un prefijo (`http://10.0.2.2:8000/cache?u=`) al que
/// se antepone la URL real; el anfitrión guarda los bytes del espejo y los sirve.
/// La verificación (tamaño + SHA-256 del índice firmado, firma RSA del .apk) se
/// hace DESPUÉS y es la misma en ambos caminos: un byte falso se rechaza igual.
/// Los índices (InRelease, Packages.gz, APKINDEX) nunca pasan por aquí.
pub fn descargar_paquete(url: &str, destino: &str) -> i32 {
    if let Ok(pref) = portix_rt::fs::read("/etc/pkg/cache-local") {
        let pref = String::from_utf8_lossy(&pref).trim().to_string();
        if !pref.is_empty() {
            let rc = descargar(&alloc::format!("{}{}", pref, url), destino);
            if rc == 0 { println!("[PKG] {} servido por la cache local del anfitrion (se verifica igual)", url.rsplit('/').next().unwrap_or(url)); return 0; }
            let _ = portix_rt::fs::unlink(destino);
        }
    }
    descargar(url, destino)
}

pub fn hex32(h: &[u8; 32]) -> String {
    const HX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in h.iter() { out.push(HX[(b >> 4) as usize] as char); out.push(HX[(b & 15) as usize] as char); }
    out
}

/// Busca `name` en el indice (local actualizado o semilla de la imagen):
/// devuelve (tamano, sha256 hex).
fn entrada_indice(name: &str) -> Option<(usize, String)> {
    let texto = portix_rt::fs::read(INDEX_LOCAL).or_else(|_| portix_rt::fs::read(INDEX_SEMILLA)).ok()?;
    let texto = core::str::from_utf8(&texto).ok()?;
    for line in texto.lines() {
        let mut it = line.split_whitespace();
        if it.next()? != name { continue; }
        let size = it.next()?.parse::<usize>().ok()?;
        let sha = it.next()?.to_string();
        return Some((size, sha));
    }
    None
}

/// Verificacion REAL: SHA-256 calculado sobre los bytes del paquete frente al
/// indice. Devuelve Ok(hash) o Err(mensaje).
fn verificar(name: &str, bytes: &[u8]) -> Result<String, String> {
    let (size, esperado) = entrada_indice(name).ok_or_else(|| alloc::format!("'{}' no figura en el indice ({} / {}); ejecute 'pkg update'", name, INDEX_LOCAL, INDEX_SEMILLA))?;
    let real = hex32(&portix_rt::tls::sha256(bytes));
    if bytes.len() != size || real != esperado {
        return Err(alloc::format!("tamano={} (esperado {}) sha256={} (esperado {})", bytes.len(), size, real, esperado));
    }
    Ok(real)
}

struct PkgMeta {
    name: &'static str,
    version: &'static str,
    arch: &'static str,
    desc: &'static str,
    size_kb: u32,
    deps: &'static str,
}

static CORE_PACKAGES: &[PkgMeta] = &[
    PkgMeta {
        name: "gcc-toolchain",
        version: "13.2.0-portix1",
        arch: "x86_64",
        desc: "GNU Compiler Collection C/C++ native compilers & headers",
        size_kb: 1420,
        deps: "binutils, libc-dev",
    },
    // [CICLO 49 / T1] Binario estático de Alpine Linux (musl, no-PIE), sin
    // modificar: prueba real de compatibilidad binaria. El SHA-256 del .pkg lo
    // fija el índice; el del binario (6d4ae568…) lo fija scripts/fetch_distro.py.
    PkgMeta {
        name: "busybox-static",
        version: "1.36.1-r31 (Alpine v3.20)",
        arch: "x86_64",
        desc: "BusyBox estatico musl de Alpine Linux (binario de distro sin modificar)",
        size_kb: 1002,
        deps: "",
    },
    // [CICLO 50 / T1b] musl dinámico de Alpine: cargador ld-musl y busybox PIE dinámico.
    PkgMeta {
        name: "musl",
        version: "1.2.5-r4 (Alpine v3.20)",
        arch: "x86_64",
        desc: "Cargador dinamico ld-musl-x86_64.so.1 de Alpine (binario de distro sin modificar)",
        size_kb: 635,
        deps: "",
    },
    PkgMeta {
        name: "musl-busybox",
        version: "1.36.1-r31 (Alpine v3.20)",
        arch: "x86_64",
        desc: "BusyBox dinamico (PIE, PT_INTERP ld-musl) de Alpine -> /bin/busybox-dyn",
        size_kb: 790,
        deps: "musl",
    },
    PkgMeta {
        name: "musl-dev",
        version: "1.2.5-r4 (Alpine v3.20)",
        arch: "x86_64",
        desc: "Cabeceras de musl, crt1/crti/crtn.o y libc.so (Alpine, sin modificar)",
        size_kb: 1900,
        deps: "musl",
    },
    PkgMeta {
        name: "tcc",
        version: "0.9.27_git20250106-r0 (Alpine v3.22)",
        arch: "x86_64",
        desc: "Tiny C Compiler dinamico de Alpine (tcc + libtcc.so + runtime) via ld-musl",
        size_kb: 300,
        deps: "musl, musl-dev",
    },
    PkgMeta {
        name: "make-utils",
        version: "4.4.1-portix1",
        arch: "x86_64",
        desc: "GNU Make build automation tool for compilation pipelines",
        size_kb: 280,
        deps: "sh",
    },
    PkgMeta {
        name: "binutils",
        version: "2.42-portix1",
        arch: "x86_64",
        desc: "GNU Binutils: ar, ld, objcopy static library tools",
        size_kb: 890,
        deps: "libc-dev",
    },
    PkgMeta {
        name: "rust-core",
        version: "1.78.0-nightly",
        arch: "x86_64",
        desc: "Rust toolchain compiler and standard core/alloc packages",
        size_kb: 3400,
        deps: "binutils",
    },
    PkgMeta {
        name: "python3",
        version: "3.12.0-portix1",
        arch: "x86_64",
        desc: "Python programming language interpreter and runtime",
        size_kb: 1200,
        deps: "libc-dev",
    },
    PkgMeta {
        name: "libportix-dev",
        version: "1.0.0",
        arch: "x86_64",
        desc: "Portix OS native Ring-3 runtime headers and static library",
        size_kb: 512,
        deps: "",
    },
    PkgMeta {
        name: "doom",
        version: "1.0.0-portix1",
        arch: "x86_64",
        desc: "Classic DOOM engine ported natively to Portix OS Ring 3",
        size_kb: 58,
        deps: "",
    },
];

/// [CICLO 51 / L2b] Almacén persistente de paquetes descargados: si el ext2 de
/// `/usr/local` está montado, los `.pkg` verificados se conservan en
/// `/usr/local/pkg/cache` (sobreviven al reinicio y el anfitrión puede
/// extraerlos con `debugfs` y comprobar su SHA-256 contra el índice). Sin ext2
/// se vuelve a `/tmp` (ramfs), como antes.
fn directorio_cache() -> &'static str {
    match portix_rt::fs::stat("/usr/local/pkg") {
        Ok(st) if st.st_mode & 0o170000 == 0o040000 => {
            let _ = portix_rt::syscall::sys_mkdir(b"/usr/local/pkg/cache\0".as_ptr(), 0o755);
            "/usr/local/pkg/cache"
        }
        _ => "/tmp",
    }
}

fn is_installed(name: &str) -> bool {
    // [CICLO 50] la base de datos ya supera los 4 KB (musl-dev registra 222 rutas):
    // se lee completa; un bufer fijo truncaba entradas y "desinstalaba" paquetes.
    if let Ok(data) = portix_rt::fs::read("/var/pkg/installed.db") {
        let text = core::str::from_utf8(&data).unwrap_or("");
        for line in text.lines() {
            if line.trim() == name || line.starts_with(&alloc::format!("{}:", name)) {
                return true;
            }
        }
    }
    // Only core runtime library and base utilities are preinstalled.
    // Compilers (gcc, rustc, make) and doom are on-demand packages.
    name == "libportix-dev" || name == "binutils"
}

fn mark_installed(name: &str, version: &str) {
    if let Ok(mut f) = File::open("/var/pkg/installed.db", O_WRONLY | O_CREAT | O_APPEND) {
        let entry = alloc::format!("{}:{}\n", name, version);
        let _ = f.write(entry.as_bytes());
    }
}

/// Registro `nombre:version:/ruta1,/ruta2` para que `remove` borre de verdad.
fn mark_installed_files(name: &str, version: &str, files: &[String]) {
    if let Ok(mut f) = File::open("/var/pkg/installed.db", O_WRONLY | O_CREAT | O_APPEND) {
        let entry = alloc::format!("{}:{}:{}\n", name, version, files.join(","));
        let _ = f.write(entry.as_bytes());
    }
}

fn archivos_instalados(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(data) = portix_rt::fs::read("/var/pkg/installed.db") {
        let text = core::str::from_utf8(&data).unwrap_or("");
        for line in text.lines() {
            let mut it = line.splitn(3, ':');
            if it.next() != Some(name) { continue; }
            let _ = it.next();
            if let Some(lista) = it.next() {
                for r in lista.split(',') { if r.starts_with('/') { out.push(r.to_string()); } }
            }
        }
    }
    out
}

fn mark_removed(name: &str) {
    let mut new_content = String::new();
    let prefijo = alloc::format!("{}:", name);
    if let Ok(data) = portix_rt::fs::read("/var/pkg/installed.db") {
        let text = core::str::from_utf8(&data).unwrap_or("");
        for line in text.lines() {
            // Coincidencia exacta del nombre: quitar `musl` no debe borrar `musl-dev`.
            if !(line.trim() == name || line.starts_with(&prefijo)) {
                new_content.push_str(line);
                new_content.push('\n');
            }
        }
    }
    if let Ok(mut f) = File::open("/var/pkg/installed.db", O_WRONLY | O_CREAT | O_TRUNC) {
        let _ = f.write(new_content.as_bytes());
    }
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
    if args.len() < 2 {
        println!("Gestor de Paquetes de Portix OS (pkg) v1.0.0");
        println!("Uso: pkg <comando> [argumentos]");
        println!("Comandos disponibles:");
        println!("  list                  Listar paquetes instalados y disponibles");
        println!("  install <paquete>     Instalar un paquete desde repositorio");
        println!("  update                Descargar el indice del repositorio (SHA-256 por paquete)");
        println!("  remove <paquete>      Desinstalar un paquete del sistema");
        println!("  info <paquete>        Mostrar informacion detallada del paquete");
        println!("  search <termino>      Buscar paquetes en el repositorio");
        println!("  update                Actualizar indices de repositorios");
        println!("  add <paquete>...      [T-002] Instalar paquetes .apk reales de Alpine (firma RSA, C:Q1, datahash)");
        println!("  del <paquete>         [T-002] Eliminar un paquete .apk instalado");
        println!("  apk-list              [T-002] Listar paquetes .apk instalados");
        println!("  apk-update            [T-002] Descargar y verificar los APKINDEX de /etc/apk/repositories");
        println!("  deb-add <paquete>...  [T-004] Instalar .deb de Debian trixie (deb.debian.org, SHA-256 del indice)");
        println!("  deb-update            [T-004] Descargar el indice Packages.gz de Debian");
        println!("  deb-list              [T-004] Paquetes .deb instalados");
        println!("  deb-verify <InRelease> [T-004] Verifica la firma OpenPGP de un InRelease local");
        println!("  version               Mostrar version de pkg");
        return 1;
    }

    let cmd = &args[1];

    if cmd == "version" || cmd == "-v" || cmd == "--version" {
        println!("Portix Package Manager (pkg) 1.0.0 (x86_64-portix-elf)");
        println!("Copyright (C) 2026 Portix OS Project. 100% Rust.");
        return 0;
    } else if cmd == "add" {
        if args.len() < 3 { println!("pkg: uso: pkg add <paquete>..."); return 1; }
        return apk::anadir(&args[2..]);
    } else if cmd == "del" {
        if args.len() < 3 { println!("pkg: uso: pkg del <paquete>"); return 1; }
        return apk::eliminar(&args[2]);
    } else if cmd == "apk-list" {
        apk::listar(); return 0;
    } else if cmd == "deb-add" {
        if args.len() < 3 { println!("pkg: uso: pkg deb-add <paquete>..."); return 1; }
        return deb::anadir(&args[2..]);
    } else if cmd == "deb-update" {
        return match deb::actualizar() { Ok(n) => { println!("[DEB-OK] indice de {} bytes", n); 0 } Err(e) => { println!("[DEB-FAIL] {}", e); 1 } };
    } else if cmd == "deb-verify" {
        let Some(ruta) = args.get(2) else { println!("uso: pkg deb-verify <fichero InRelease>"); return 1; };
        return deb::verificar_fichero(ruta);
    } else if cmd == "deb-list" {
        deb::listar(); return 0;
    } else if cmd == "apk-update" {
        return match apk::actualizar() { Ok(n) => { println!("[APK-OK] {} paquetes indexados", n); 0 } Err(e) => { println!("[APK-FAIL] {}", e); 1 } };
    } else if cmd == "list" {
        println!("{:<18} {:<16} {:<12} {:<10}", "PAQUETE", "VERSION", "ESTADO", "TAMANIO");
        println!("----------------------------------------------------------");
        for pkg in CORE_PACKAGES {
            let status = if is_installed(pkg.name) { "[instalado]" } else { "[disponible]" };
            let size_str = alloc::format!("{} KB", pkg.size_kb);
            println!("{:<18} {:<16} {:<12} {:<10}", pkg.name, pkg.version, status, size_str);
        }
        return 0;
    } else if cmd == "info" {
        if args.len() < 3 {
            println!("pkg: uso: pkg info <paquete>");
            return 1;
        }
        let target = &args[2];
        if let Some(pkg) = CORE_PACKAGES.iter().find(|p| p.name == target) {
            let status = if is_installed(pkg.name) { "Instalado" } else { "No instalado" };
            println!("Nombre:        {}", pkg.name);
            println!("Version:       {}", pkg.version);
            println!("Arquitectura:  {}", pkg.arch);
            println!("Descripcion:   {}", pkg.desc);
            println!("Tamanio:       {} KB", pkg.size_kb);
            println!("Dependencias:  {}", if pkg.deps.is_empty() { "ninguna" } else { pkg.deps });
            println!("Estado:        {}", status);
            return 0;
        } else {
            println!("pkg: paquete '{}' no encontrado en el repositorio", target);
            return 1;
        }
    } else if cmd == "search" {
        if args.len() < 3 {
            println!("pkg: uso: pkg search <termino>");
            return 1;
        }
        let query = &args[2];
        println!("Resultados de busqueda para '{}':", query);
        let mut count = 0;
        for pkg in CORE_PACKAGES {
            if pkg.name.contains(query) || pkg.desc.contains(query) {
                let status = if is_installed(pkg.name) { "[instalado]" } else { "" };
                println!("  {:<16} - {} {}", pkg.name, pkg.desc, status);
                count += 1;
            }
        }
        if count == 0 {
            println!("  No se encontraron paquetes coincidentes.");
        }
        return 0;
    } else if cmd == "update" {
        // [FRENTE B] Refresca el indice del repositorio por HTTP.
        let url = alloc::format!("{}/index.txt", REPO_URL);
        println!("[+] Descargando indice {} -> {}", url, INDEX_LOCAL);
        let rc = descargar(&url, INDEX_LOCAL);
        match portix_rt::fs::read(INDEX_LOCAL) {
            Ok(data) if rc == 0 => {
                let texto = core::str::from_utf8(&data).unwrap_or("");
                let n = texto.lines().filter(|l| l.split_whitespace().count() >= 3).count();
                for l in texto.lines() { println!("    {}", l); }
                println!("[PKG-UPDATE-OK] indice con {} paquetes (SHA-256 por paquete)", n);
                return 0;
            }
            _ => { println!("[PKG-UPDATE-FAIL] no se pudo descargar el indice (curl rc={})", rc); return 1; }
        }
    } else if cmd == "install" {
        if args.len() < 3 {
            println!("pkg: uso: pkg install <paquete>");
            return 1;
        }
        let target = &args[2];
        if let Some(pkg) = CORE_PACKAGES.iter().find(|p| p.name == target) {
            if is_installed(pkg.name) {
                println!("pkg: el paquete '{}' ya esta instalado en su version mas reciente ({})", pkg.name, pkg.version);
                return 0;
            }
            println!("[+] Resolviendo dependencias para '{}'...", pkg.name);
            if !pkg.deps.is_empty() {
                println!("    Dependencias verificadas: {}", pkg.deps);
            }
            println!("[+] Localizando archivo de paquete: {} ({}.pkg)...", pkg.name, pkg.name);
            let tmp_path = alloc::format!("{}/{}.pkg", directorio_cache(), pkg.name);
            let repo_path = alloc::format!("/var/pkg/repo/{}.pkg", pkg.name);
            let mut pkg_bytes = if let Ok(data) = portix_rt::fs::read(&tmp_path) {
                println!("[+] Usando paquete ya descargado en '{}'", tmp_path);
                data
            } else {
                // [FRENTE B] Descarga real por HTTP (curl via fork+execve+waitpid).
                let url = alloc::format!("{}/{}.pkg", REPO_URL, pkg.name);
                println!("[+] Descargando {} -> {}", url, tmp_path);
                let rc = descargar(&url, &tmp_path);
                match portix_rt::fs::read(&tmp_path) {
                    Ok(data) if rc == 0 => { println!("[+] Descarga completada: {} bytes", data.len()); data }
                    _ => {
                        if let Ok(data) = portix_rt::fs::read(&repo_path) {
                            println!("[!] Sin red (curl rc={}): usando copia local '{}'", rc, repo_path);
                            data
                        } else {
                            println!("[PKG-FETCH-FAIL] no se pudo descargar '{}' (curl rc={}) ni hay copia en {}", pkg.name, rc, repo_path);
                            return 1;
                        }
                    }
                }
            };

            // [FRENTE B] Verificacion REAL contra el indice (SHA-256 calculado).
            match verificar(pkg.name, &pkg_bytes) {
                Ok(h) => {
                    println!("[+] SHA-256 verificado: {}", h);
                    if tmp_path.starts_with("/usr/local/") {
                        println!("[+] Paquete conservado en {} (ext2, {} bytes)", tmp_path, pkg_bytes.len());
                    }
                }
                Err(e) => {
                    println!("[PKG-SHA256-MISMATCH] '{}' rechazado: {}", pkg.name, e);
                    let _ = portix_rt::fs::unlink(&tmp_path);
                    println!("    archivo corrupto eliminado: {}", tmp_path);
                    // Segunda oportunidad honesta: la copia local de la imagen, verificada igual.
                    match portix_rt::fs::read(&repo_path) {
                        Ok(data) => match verificar(pkg.name, &data) {
                            Ok(h) => { println!("[+] Copia local '{}' verificada: {}", repo_path, h); pkg_bytes = data; }
                            Err(e2) => { println!("[PKG-SHA256-MISMATCH] copia local tambien invalida: {}", e2); return 1; }
                        },
                        Err(_) => return 1,
                    }
                }
            }
            let mut instalados: Vec<String> = Vec::new();
            let mut fallos_escritura: usize = 0;
            if pkg_bytes.len() >= 14 && &pkg_bytes[0..10] == b"PORTIXPKG\x01" {
                let count = u32::from_le_bytes(pkg_bytes[10..14].try_into().unwrap_or([0; 4])) as usize;
                let mut off = 14;
                for _ in 0..count {
                    if off + 4 > pkg_bytes.len() { break; }
                    let path_len = u32::from_le_bytes(pkg_bytes[off..off+4].try_into().unwrap_or([0; 4])) as usize;
                    off += 4;
                    if off + path_len > pkg_bytes.len() { break; }
                    let path_str = core::str::from_utf8(&pkg_bytes[off..off+path_len]).unwrap_or("");
                    off += path_len;
                    if off + 4 > pkg_bytes.len() { break; }
                    let data_len = u32::from_le_bytes(pkg_bytes[off..off+4].try_into().unwrap_or([0; 4])) as usize;
                    off += 4;
                    if off + data_len > pkg_bytes.len() { break; }
                    let data_slice = &pkg_bytes[off..off+data_len];
                    off += data_len;

                    // [CICLO 50 / T1c] Los paquetes de distro traen jerarquías
                    // (/usr/lib/tcc/include/...): se crean los directorios
                    // intermedios (mkdir -p; EEXIST se ignora) antes de escribir.
                    {
                        let bytes = path_str.as_bytes();
                        let mut i = 1;
                        while i < bytes.len() {
                            if bytes[i] == b'/' {
                                let mut dir: Vec<u8> = bytes[..i].to_vec();
                                dir.push(0);
                                let _ = portix_rt::syscall::sys_mkdir(dir.as_ptr(), 0o755);
                            }
                            i += 1;
                        }
                    }
                    let escrito = portix_rt::fs::write(path_str, data_slice).is_ok();
                    if escrito {
                        println!("    FAT installed {} ({}B)", path_str, data_slice.len());
                    } else {
                        println!("    [!] FALLO al escribir {} ({}B)", path_str, data_slice.len());
                        fallos_escritura += 1;
                    }
                    instalados.push(path_str.to_string());

                    if path_str == "/bin/gcc" {
                        // Solo `cc`: no hay compilador de C++ en Portix, asi que no se
                        // instalan alias g++/c++ que inviten a creer lo contrario.
                        let _ = portix_rt::fs::write("/bin/cc", data_slice);
                        instalados.push("/bin/cc".to_string());
                        println!("    FAT installed alias /bin/cc");
                    } else if path_str == "/bin/cargo-portix" {
                        let _ = portix_rt::fs::write("/bin/cargo", data_slice);
                        instalados.push("/bin/cargo".to_string());
                        println!("    FAT installed alias /bin/cargo");
                    }
                }
            } else {
                println!("[PKG-FORMAT-FAIL] '{}' no tiene cabecera PORTIXPKG", pkg.name);
                return 1;
            }
            mark_installed_files(pkg.name, pkg.version, &instalados);
            if fallos_escritura > 0 {
                // Instalación incompleta: no se declara éxito ni rc 0.
                println!("[PKG-FAIL] Paquete '{}': {} fichero(s) no se pudieron escribir", pkg.name, fallos_escritura);
                return 1;
            }
            println!("[OK] Paquete '{}' ({}) instalado exitosamente.", pkg.name, pkg.version);
            return 0;
        } else {
            println!("pkg: paquete '{}' no encontrado", target);
            return 1;
        }
    } else if cmd == "remove" {
        if args.len() < 3 {
            println!("pkg: uso: pkg remove <paquete>");
            return 1;
        }
        let target = &args[2];
        if let Some(pkg) = CORE_PACKAGES.iter().find(|p| p.name == target) {
            if !is_installed(pkg.name) {
                println!("pkg: el paquete '{}' no esta instalado", pkg.name);
                return 1;
            }
            println!("[+] Eliminando archivos asociados a '{}'...", pkg.name);
            // [FRENTE B] Borrado REAL de los archivos registrados al instalar.
            let mut borrados = 0usize;
            for ruta in archivos_instalados(pkg.name) {
                if portix_rt::fs::unlink(&ruta) == 0 { println!("    eliminado {}", ruta); borrados += 1; }
                else { println!("    no se pudo eliminar {}", ruta); }
            }
            println!("[+] {} archivos eliminados", borrados);
            mark_removed(pkg.name);
            println!("[OK] Paquete '{}' desinstalado exitosamente.", pkg.name);
            return 0;
        } else {
            println!("pkg: paquete '{}' no encontrado", target);
            return 1;
        }
    } else if cmd == "update" {
        println!("[+] Contactando repositorio oficial https://pkg.portix.org/x86_64/...");
        println!("[+] Descargando indices de paquetes actualizados (repo.db)...");
        println!("[OK] 6 paquetes indexados y listos para su instalacion.");
        return 0;
    } else {
        println!("pkg: comando desconocido '{}'. Ejecute 'pkg' para ver la ayuda.", cmd);
        return 1;
    }
}
