//! [T-004] Paquetes `.deb` REALES de Debian (trixie, main, amd64) desde el
//! espejo oficial. Formato verificado en el anfitrión con `ar`/`dpkg-deb`:
//!
//!   .deb = archivo `ar` ("!<arch>\n") con tres miembros: `debian-binary`
//!          ("2.0\n"), `control.tar.xz` y `data.tar.xz` (también se acepta .gz).
//!   El índice `dists/<suite>/<comp>/binary-<arch>/Packages.gz` (13 MiB, 60 MiB
//!   inflados, ~70 000 paquetes) se recorre EN FLUJO línea a línea y sólo se
//!   retienen las estrofas de los paquetes pedidos: `Filename`, `Size`,
//!   `SHA256`, `Version`, `Depends`/`Pre-Depends`. Cada `.deb` descargado se
//!   comprueba contra el tamaño y el SHA-256 del índice antes de tocar nada.
//!
//! Integridad: el índice viaja por HTTPS (TLS 1.3 de portix-rt, certificado
//! verificado) y cada paquete se verifica con su SHA-256 del índice. La firma
//! OpenPGP de `InRelease` (RSA-4096/SHA-512) está PENDIENTE (docs/TICKETS.md):
//! hasta entonces la cadena de confianza del índice es la del transporte.
//!
//! Debian trixie es "merged /usr": los ficheros van bajo /usr/lib/... y
//! /usr/lib64/...; el intérprete de los ELF de glibc es /lib64/ld-linux-x86-64.so.2,
//! por lo que, si /lib64 no existe, se crea como enlace a usr/lib64.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::collections::{BTreeMap, BTreeSet};
use portix_rt::println;
use crate::{tar, xz};
use crate::inflate::gunzip_a;

const ESPEJO: &str = "https://deb.debian.org/debian";
const SUITE: &str = "trixie";
const COMPONENTE: &str = "main";
const ARCH: &str = "amd64";
const BASE: &str = "/var/pkg/deb";
/// Claves OpenPGP confiadas del archivo de Debian (ficheros `.asc` tal cual los
/// publica ftp-master.debian.org/keys). Sin ellas, `deb-update` FALLA.
const CLAVES: &[&str] = &["/etc/pkg/deb-keys/debian-archive-13.asc"];

#[derive(Clone, Default)]
pub struct Paquete { pub nombre: String, pub version: String, pub fichero: String, pub tamano: usize, pub sha256: String, pub deps: Vec<String>,
    /// Nombres virtuales que este paquete `Provides:` (p. ej. mawk → awk) y su `Priority:` (para elegir proveedor).
    pub provee: Vec<String>, pub prioridad: u8 }

/// Rango de `Priority:` (mayor = más preferido como proveedor de un paquete virtual).
fn rango_prioridad(p: &str) -> u8 { match p.trim() { "required" => 5, "important" => 4, "standard" => 3, "optional" => 2, _ => 1 } }

fn mkdir_p(ruta: &str) {
    let b = ruta.as_bytes();
    let mut i = 1;
    while i <= b.len() {
        if i == b.len() || b[i] == b'/' {
            let mut d: Vec<u8> = b[..i].to_vec(); d.push(0);
            let _ = portix_rt::syscall::sys_mkdir(d.as_ptr(), 0o755);
        }
        i += 1;
    }
}

fn indice_local() -> String { alloc::format!("{}/{}_{}_{}_Packages.gz", BASE, SUITE, COMPONENTE, ARCH) }

/// Descarga el índice `Packages.gz` del espejo (HTTPS) a /var/pkg/deb.
pub fn actualizar() -> Result<usize, String> {
    mkdir_p(BASE);
    // 1) InRelease (firmado en claro) y su verificación OpenPGP contra las claves confiadas
    let mut claves = Vec::new();
    for ruta in CLAVES {
        if let Ok(t) = portix_rt::fs::read(ruta) {
            if let Ok(t) = core::str::from_utf8(&t) { claves.extend(crate::pgp::claves_de_asc(t)); }
        }
    }
    if claves.is_empty() { return Err(alloc::format!("sin claves confiadas de Debian en {:?}: no se puede verificar InRelease", CLAVES)); }
    let url_rel = alloc::format!("{}/dists/{}/InRelease", ESPEJO, SUITE);
    let rel_local = alloc::format!("{}/{}_InRelease", BASE, SUITE);
    let rc = crate::descargar(&url_rel, &rel_local);
    if rc != 0 { return Err(alloc::format!("curl rc={} para {}", rc, url_rel)); }
    let rel_bytes = portix_rt::fs::read(&rel_local).map_err(|e| alloc::format!("no se pudo leer {} (errno {})", rel_local, e))?;
    let rel = core::str::from_utf8(&rel_bytes).map_err(|_| "InRelease no es UTF-8")?;
    let firma = crate::pgp::verificar_en_claro(rel, &claves).map_err(|e| alloc::format!("InRelease RECHAZADO: {}", e))?;
    let codename = rel.lines().find_map(|l| l.strip_prefix("Codename: ")).unwrap_or("").trim();
    if codename != SUITE { return Err(alloc::format!("InRelease firmado pero de otra distribución: Codename '{}' != '{}'", codename, SUITE)); }
    println!("[DEB] InRelease de {} verificado: firma OpenPGP RSA-{} {} de la clave {} (creada {})", codename, claves.iter().find(|c| c.huella == firma.huella).map(|c| c.rsa.n.bits()).unwrap_or(0), firma.hash, crate::pgp::hex20(&firma.huella), firma.creada);
    // 2) Packages.gz y su anclaje al InRelease (SHA-256 + tamaño)
    let ruta_rel = alloc::format!("{}/binary-{}/Packages.gz", COMPONENTE, ARCH);
    let (sha_esp, tam_esp) = crate::pgp::sha256_de_release(rel, &ruta_rel).ok_or(alloc::format!("InRelease no lista {}", ruta_rel))?;
    let url = alloc::format!("{}/dists/{}/{}", ESPEJO, SUITE, ruta_rel);
    let destino = indice_local();
    let rc = crate::descargar(&url, &destino);
    if rc != 0 { return Err(alloc::format!("curl rc={} para {}", rc, url)); }
    let bytes = portix_rt::fs::read(&destino).map_err(|e| alloc::format!("no se pudo leer {} (errno {})", destino, e))?;
    if bytes.len() < 18 || bytes[0] != 0x1f || bytes[1] != 0x8b { return Err("el indice descargado no es gzip".into()); }
    if bytes.len() != tam_esp { let _ = portix_rt::fs::unlink(&destino); return Err(alloc::format!("Packages.gz: tamaño {} != {} declarado en InRelease", bytes.len(), tam_esp)); }
    let real = { let mut h = crate::sha256_estado::Sha256::nuevo(); h.actualizar(&bytes); crate::hex32(&h.terminar()) };
    if real != sha_esp { let _ = portix_rt::fs::unlink(&destino); return Err(alloc::format!("Packages.gz: SHA256 {} != {} declarado en InRelease", real, sha_esp)); }
    println!("[DEB] indice {}/{}/{}: {} bytes comprimidos, SHA256 {} coincide con el InRelease firmado; guardado en {}", SUITE, COMPONENTE, ARCH, bytes.len(), &sha_esp[..16], destino);
    Ok(bytes.len())
}

/// `pkg deb-verify <ruta>`: verifica un InRelease local contra las claves confiadas
/// (sirve para demostrar que un fichero manipulado se RECHAZA).
pub fn verificar_fichero(ruta: &str) -> i32 {
    let mut claves = Vec::new();
    for r in CLAVES { if let Ok(t) = portix_rt::fs::read(r) { if let Ok(t) = core::str::from_utf8(&t) { claves.extend(crate::pgp::claves_de_asc(t)); } } }
    let Ok(b) = portix_rt::fs::read(ruta) else { println!("[DEB-FAIL] no se pudo leer {}", ruta); return 1; };
    let Ok(t) = core::str::from_utf8(&b) else { println!("[DEB-FAIL] {} no es UTF-8", ruta); return 1; };
    match crate::pgp::verificar_en_claro(t, &claves) {
        Ok(f) => { println!("[DEB-VERIFY-OK] {}: firma OpenPGP {} válida de la clave {} (creada {})", ruta, f.hash, crate::pgp::hex20(&f.huella), f.creada); 0 }
        Err(e) => { println!("[DEB-VERIFY-RECHAZADO] {}: {}", ruta, e); 1 }
    }
}

/// Nombre de paquete de una cláusula de dependencia: quita versión `(>= x)`,
/// alternativas `a | b` (se toma la primera) y calificadores `:any`.
fn nombre_dep(c: &str) -> String {
    let c = c.split('|').next().unwrap_or("").trim();
    let c = c.split('(').next().unwrap_or("").trim();
    let c = c.split(':').next().unwrap_or("").trim();
    c.to_string()
}

/// Recorre el índice EN FLUJO y devuelve las estrofas de `buscados`.
fn buscar(buscados: &BTreeSet<String>) -> Result<BTreeMap<String, Paquete>, String> {
    let ruta = indice_local();
    let bytes = portix_rt::fs::read(&ruta).map_err(|_| String::from("no hay indice: ejecuta `pkg deb-update`"))?;
    let mut encontrados: BTreeMap<String, Paquete> = BTreeMap::new();
    let mut linea: Vec<u8> = Vec::with_capacity(256);
    let mut actual: Option<Paquete> = None; // estrofa en curso si su Package: interesa
    let mut total = 0usize;
    {
        let mut procesar_linea = |l: &[u8], actual: &mut Option<Paquete>, encontrados: &mut BTreeMap<String, Paquete>, total: &mut usize| {
            if l.is_empty() {
                if let Some(p) = actual.take() {
                    if buscados.contains(&p.nombre) { encontrados.insert(p.nombre.clone(), p); }
                    else {
                        // Paquete virtual: se registra bajo el nombre buscado al mejor proveedor
                        // (Priority más alta; a igualdad, el primero por orden del índice).
                        for v in &p.provee {
                            if !buscados.contains(v) { continue; }
                            let mejor = match encontrados.get(v) { None => true, Some(e) => e.nombre != *v && p.prioridad > e.prioridad };
                            if mejor { encontrados.insert(v.clone(), p.clone()); }
                        }
                    }
                }
                return;
            }
            if let Some(v) = l.strip_prefix(b"Package: ") {
                *total += 1;
                let n = String::from_utf8_lossy(v).trim().to_string();
                // Toda estrofa se sigue (ligera): puede `Provides:` un nombre buscado (awk, libc-dev...).
                *actual = Some(Paquete { nombre: n, ..Default::default() });
                return;
            }
            let Some(p) = actual.as_mut() else { return };
            let texto = String::from_utf8_lossy(l);
            if let Some(v) = texto.strip_prefix("Provides: ") {
                for c in v.split(',') { let n = nombre_dep(c); if !n.is_empty() { p.provee.push(n); } }
            }
            else if let Some(v) = texto.strip_prefix("Priority: ") { p.prioridad = rango_prioridad(v); }
            else if let Some(v) = texto.strip_prefix("Version: ") { p.version = v.trim().to_string(); }
            else if let Some(v) = texto.strip_prefix("Filename: ") { p.fichero = v.trim().to_string(); }
            else if let Some(v) = texto.strip_prefix("Size: ") { p.tamano = v.trim().parse().unwrap_or(0); }
            else if let Some(v) = texto.strip_prefix("SHA256: ") { p.sha256 = v.trim().to_string(); }
            else if let Some(v) = texto.strip_prefix("Depends: ").or_else(|| texto.strip_prefix("Pre-Depends: ")) {
                for c in v.split(',') { let n = nombre_dep(c); if !n.is_empty() && !p.deps.contains(&n) { p.deps.push(n); } }
            }
        };
        let mut sum = |t: &[u8]| -> Result<(), &'static str> {
            for &b in t {
                if b == b'\n' { procesar_linea(&linea, &mut actual, &mut encontrados, &mut total); linea.clear(); }
                else if linea.len() < 4096 { linea.push(b); }
            }
            Ok(())
        };
        gunzip_a(&bytes, 0, &mut sum).map_err(|e| alloc::format!("indice: {}", e))?;
        if !linea.is_empty() { procesar_linea(&linea, &mut actual, &mut encontrados, &mut total); }
        procesar_linea(b"", &mut actual, &mut encontrados, &mut total);
    }
    println!("[DEB] indice recorrido: {} paquetes; {} de {} buscados", total, encontrados.len(), buscados.len());
    Ok(encontrados)
}

fn instalados() -> Vec<(String, String)> {
    let mut v = Vec::new();
    if let Ok(d) = portix_rt::fs::read(&alloc::format!("{}/installed", BASE)) {
        for l in String::from_utf8_lossy(&d).lines() { if let Some((a, b)) = l.split_once(' ') { v.push((a.to_string(), b.to_string())); } }
    }
    v
}
pub fn esta_instalado(n: &str) -> bool { instalados().iter().any(|(a, _)| a == n) }

/// Resuelve dependencias por pasadas sobre el índice (cada pasada es un
/// recorrido en flujo; normalmente 2-3). Devuelve el plan en orden topológico.
pub fn resolver(objetivos: &[String]) -> Result<Vec<Paquete>, String> {
    let mut conocidos: BTreeMap<String, Paquete> = BTreeMap::new();
    let mut virtuales: BTreeMap<String, String> = BTreeMap::new(); // nombre virtual -> paquete real
    let mut pendientes: BTreeSet<String> = objetivos.iter().cloned().collect();
    let mut pasadas = 0;
    while !pendientes.is_empty() {
        pasadas += 1;
        if pasadas > 8 { return Err("demasiadas pasadas resolviendo dependencias".into()); }
        let hallados = buscar(&pendientes)?;
        for n in pendientes.iter() {
            if !hallados.contains_key(n) { return Err(alloc::format!("'{}' no existe en {}/{} {}", n, SUITE, COMPONENTE, ARCH)); }
        }
        let mut nuevos = BTreeSet::new();
        for (n, p) in hallados {
            if n != p.nombre { println!("[DEB] '{}' es virtual: lo provee {} {}", n, p.nombre, p.version); virtuales.insert(n.clone(), p.nombre.clone()); }
            for d in &p.deps { if !conocidos.contains_key(d) && !virtuales.contains_key(d) && !pendientes.contains(d) && !esta_instalado(d) { nuevos.insert(d.clone()); } }
            conocidos.insert(p.nombre.clone(), p);
        }
        // un nombre virtual cuyo proveedor ya es conocido no vuelve a buscarse
        nuevos.retain(|d| !conocidos.contains_key(d) && !virtuales.contains_key(d));
        pendientes = nuevos;
    }
    // orden topológico (dependencias primero)
    let mut orden: Vec<Paquete> = Vec::new();
    let mut visitados: BTreeSet<String> = BTreeSet::new();
    fn visitar(n: &str, c: &BTreeMap<String, Paquete>, v: &BTreeMap<String, String>, vis: &mut BTreeSet<String>, orden: &mut Vec<Paquete>) {
        let n = v.get(n).map(|s| s.as_str()).unwrap_or(n);
        if vis.contains(n) { return; }
        vis.insert(n.to_string());
        if let Some(p) = c.get(n) { for d in &p.deps { visitar(d, c, v, vis, orden); } orden.push(p.clone()); }
    }
    for n in conocidos.keys() { visitar(n, &conocidos, &virtuales, &mut visitados, &mut orden); }
    orden.retain(|p| !esta_instalado(&p.nombre));
    Ok(orden)
}

/// Miembros del archivo `ar`: (nombre, inicio, fin).
fn miembros_ar(d: &[u8]) -> Result<Vec<(String, usize, usize)>, String> {
    if d.len() < 8 || &d[..8] != b"!<arch>\n" { return Err("no es un archivo ar (.deb)".into()); }
    let mut p = 8; let mut v = Vec::new();
    while p + 60 <= d.len() {
        let h = &d[p..p + 60];
        if &h[58..60] != b"`\n" { return Err("cabecera ar corrupta".into()); }
        let nombre = String::from_utf8_lossy(&h[..16]).trim().trim_end_matches('/').to_string();
        let tam: usize = String::from_utf8_lossy(&h[48..58]).trim().parse().map_err(|_| "tamano ar invalido".to_string())?;
        let ini = p + 60;
        if ini + tam > d.len() { return Err("miembro ar truncado".into()); }
        v.push((nombre, ini, ini + tam));
        p = ini + tam + (tam & 1);
    }
    Ok(v)
}

/// Descomprime un miembro `.tar.xz` / `.tar.gz` entero (control: pequeño).
fn tar_entero(nombre: &str, d: &[u8]) -> Result<Vec<u8>, String> {
    if nombre.ends_with(".xz") { xz::Xz::todo(d).map_err(|e| e.to_string()) }
    else if nombre.ends_with(".gz") { let mut out = Vec::new(); let mut s = |t: &[u8]| -> Result<(), &'static str> { out.extend_from_slice(t); Ok(()) }; gunzip_a(d, 0, &mut s).map_err(|e| e.to_string())?; Ok(out) }
    else { Err(alloc::format!("{}: compresion no soportada (solo xz y gz)", nombre)) }
}

fn campo<'a>(control: &'a str, clave: &str) -> Option<&'a str> {
    control.lines().find_map(|l| l.strip_prefix(clave).and_then(|r| r.strip_prefix(": ")).map(|v| v.trim()))
}

/// Descarga, verifica (tamaño + SHA-256 del índice) e instala un `.deb`.
/// [T-011/T-012] Reloj monótono en ms para medir cada fase del despliegue.
fn ahora_ms() -> u64 {
    let mut ts = portix_rt::syscall::Timespec { tv_sec: 0, tv_nsec: 0 };
    portix_rt::syscall::sys_clock_gettime(1, &mut ts as *mut portix_rt::syscall::Timespec);
    (ts.tv_sec as u64) * 1000 + (ts.tv_nsec as u64) / 1_000_000
}

pub fn instalar(p: &Paquete) -> Result<usize, String> {
    let t0 = ahora_ms();
    let fichero = p.fichero.rsplit('/').next().unwrap_or(&p.fichero).to_string();
    let url = alloc::format!("{}/{}", ESPEJO, p.fichero);
    let cache = alloc::format!("{}/cache", BASE); mkdir_p(&cache);
    let destino = alloc::format!("{}/{}", cache, fichero);
    let rc = crate::descargar_paquete(&url, &destino);
    if rc != 0 { return Err(alloc::format!("curl rc={} para {}", rc, url)); }
    let t1 = ahora_ms();
    let bytes = portix_rt::fs::read(&destino).map_err(|e| alloc::format!("no se pudo leer {} (errno {})", destino, e))?;
    let t2 = ahora_ms();
    if bytes.len() != p.tamano { return Err(alloc::format!("{}: tamano {} != Size: {} del indice", fichero, bytes.len(), p.tamano)); }
    let mut h = crate::sha256_estado::Sha256::nuevo();
    for t in bytes.chunks(1 << 16) { h.actualizar(t); }
    let real = crate::hex32(&h.terminar());
    if real != p.sha256 { return Err(alloc::format!("{}: SHA256 {} != {} del indice", fichero, real, p.sha256)); }
    let t3 = ahora_ms();
    let ms = miembros_ar(&bytes)?;
    let (_, b0, e0) = ms.iter().find(|m| m.0 == "debian-binary").ok_or("sin debian-binary")?;
    if &bytes[*b0..*e0] != b"2.0\n" { return Err("version de formato .deb distinta de 2.0".into()); }
    let ctl = ms.iter().find(|m| m.0.starts_with("control.tar")).ok_or("sin control.tar")?;
    let dat = ms.iter().find(|m| m.0.starts_with("data.tar")).ok_or("sin data.tar")?;
    // control: Package/Version y scripts de mantenedor (no se ejecutan)
    let control_tar = tar_entero(&ctl.0, &bytes[ctl.1..ctl.2])?;
    let entradas = tar::entradas(&control_tar).map_err(|e| e.to_string())?;
    let control = entradas.iter().find(|e| e.nombre.trim_start_matches("./") == "control").ok_or("sin fichero control")?;
    let control = String::from_utf8_lossy(control.datos).into_owned();
    if campo(&control, "Package") != Some(p.nombre.as_str()) { return Err(alloc::format!("{}: Package del control no coincide", fichero)); }
    if campo(&control, "Version") != Some(p.version.as_str()) { return Err(alloc::format!("{}: Version del control != indice", fichero)); }
    let scripts: Vec<&str> = entradas.iter().map(|e| e.nombre.trim_start_matches("./")).filter(|n| matches!(*n, "preinst" | "postinst" | "prerm" | "postrm" | "triggers")).collect();
    println!("[DEB] {}: Size OK, SHA256 {} OK, control: {} {}", fichero, &real[..16], p.nombre, p.version);
    if !scripts.is_empty() { println!("[DEB] {}: scripts de mantenedor NO ejecutados ({:?}); Portix no corre scripts de paquete", p.nombre, scripts); }
    // datos: xz -> tar -> ext2 EN FLUJO
    let mut desp = crate::apk::Despliegue::nuevo();
    // El error detallado (ruta + errno, p. ej. ENOSPC) vive en `desp.error`; el del
    // flujo tar sólo dice "escritura fallo". Se consulta primero el detallado.
    let resultado: Result<(), String> = (|| {
        let mut tarf = tar::TarFlujo::new(&mut desp);
        let d = &bytes[dat.1..dat.2];
        if dat.0.ends_with(".xz") {
            let mut x = xz::Xz::new(d).map_err(|e| alloc::format!("{}: {}", dat.0, e))?;
            let mut buf = alloc::vec![0u8; 1 << 16];
            loop {
                let n = x.leer(&mut buf).map_err(|e| alloc::format!("{}: {}", dat.0, e))?;
                if n == 0 { break; }
                tarf.alimentar(&buf[..n]).map_err(|e| e.to_string())?;
            }
        } else if dat.0.ends_with(".gz") {
            let mut sum = |t: &[u8]| -> Result<(), &'static str> { tarf.alimentar(t) };
            gunzip_a(d, 0, &mut sum).map_err(|e| alloc::format!("{}: {}", dat.0, e))?;
        } else { return Err(alloc::format!("{}: compresion no soportada", dat.0)); }
        tarf.terminar().map_err(|e| e.to_string())
    })();
    if let Some(e) = desp.error.take() { return Err(e); }
    resultado?;
    let t4 = ahora_ms();
    // Medida por fase (ms): lo que tarde cada tramo decide qué se optimiza (T-012).
    println!("[DEB-T] {}: {} KB descarga {} ms ({} KB/s) lectura {} ms sha256 {} ms extraccion {} ms ({} ficheros, {} KB/s de .deb)",
        p.nombre, bytes.len() / 1024, t1 - t0, (bytes.len() as u64) / (t1 - t0).max(1), t2 - t1, t3 - t2, t4 - t3, desp.n,
        (bytes.len() as u64) / (t4 - t3).max(1));
    let (lista, n) = (desp.lista, desp.n);
    // merged /usr: /lib64 -> usr/lib64 para el interprete /lib64/ld-linux-x86-64.so.2
    if lista.iter().any(|r| r.ends_with("ld-linux-x86-64.so.2")) {
        // Se comprueba el destino final (no `lstat("/lib64")`): un directorio /lib64
        // vacío o un enlace roto también dejarían a ld-linux inalcanzable.
        if portix_rt::fs::read("/lib64/ld-linux-x86-64.so.2").is_err() {
            let _ = portix_rt::fs::rmdir("/lib64"); // un /lib64 vacío heredado impide el enlace (EEXIST)
            match portix_rt::fs::symlink("usr/lib64", "/lib64") { Ok(_) => println!("[DEB] /lib64 -> usr/lib64 (merged /usr)"), Err(e) => println!("[DEB] aviso: no se pudo crear /lib64: errno {}", e) }
        } else { println!("[DEB] /lib64/ld-linux-x86-64.so.2 ya accesible"); }
        // [T-005b] merged /usr también para el multiarch: en Debian /lib es un enlace a
        // usr/lib, así que los guiones de enlazado de libc6-dev (`/usr/lib/x86_64-linux-gnu/
        // libm.so` = `GROUP ( /lib/x86_64-linux-gnu/libm.so.6 ... )`) nombran /lib/...
        // En Portix /lib es un directorio real (sistema base): se enlaza sólo el
        // subdirectorio multiarch, no /lib entero.
        let mut st = portix_rt::fs::Stat::default();
        if portix_rt::fs::lstat("/lib/x86_64-linux-gnu", &mut st).is_err() {
            match portix_rt::fs::symlink("/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu") {
                Ok(_) => println!("[DEB] /lib/x86_64-linux-gnu -> /usr/lib/x86_64-linux-gnu (merged /usr, multiarch)"),
                Err(e) => println!("[DEB] aviso: no se pudo crear /lib/x86_64-linux-gnu: errno {}", e),
            }
        }
    }
    mkdir_p(&alloc::format!("{}/files", BASE));
    let _ = portix_rt::fs::write(&alloc::format!("{}/files/{}", BASE, p.nombre), lista.join("\n").as_bytes());
    let mut inst = instalados(); inst.retain(|(nm, _)| nm != &p.nombre); inst.push((p.nombre.clone(), p.version.clone()));
    // nombres virtuales que este paquete provee (para que `awk` cuente como instalado si está mawk)
    for v in &p.provee { if !inst.iter().any(|(nm, _)| nm == v) { inst.push((v.clone(), alloc::format!("virtual:{}", p.nombre))); } }
    let txt: String = inst.iter().map(|(a, b)| alloc::format!("{} {}\n", a, b)).collect();
    portix_rt::fs::write(&alloc::format!("{}/installed", BASE), txt.as_bytes()).map_err(|e| alloc::format!("base de datos: errno {}", e))?;
    let _ = portix_rt::fs::unlink(&destino);
    Ok(n)
}

pub fn anadir(objetivos: &[String]) -> i32 {
    if portix_rt::fs::read(&indice_local()).is_err() {
        if let Err(e) = actualizar() { println!("[DEB-FAIL] {}", e); return 1; }
    }
    let plan = match resolver(objetivos) { Ok(p) => p, Err(e) => { println!("[DEB-FAIL] {}", e); return 1; } };
    if plan.is_empty() { println!("[DEB-OK] nada que instalar (ya instalados)"); return 0; }
    let nombres: Vec<&str> = plan.iter().map(|p| p.nombre.as_str()).collect();
    println!("[DEB] plan ({}): {}", plan.len(), nombres.join(" "));
    let mut hechos = Vec::new();
    for p in &plan {
        match instalar(p) {
            Ok(n) => { println!("[DEB] {} {} instalado ({} entradas)", p.nombre, p.version, n); hechos.push(alloc::format!("{}_{}", p.nombre, p.version)); }
            Err(e) => { println!("[DEB-FAIL] {}: {}", p.nombre, e); return 1; }
        }
    }
    println!("[DEB-OK] instalados: {}", hechos.join(" "));
    0
}

pub fn listar() {
    for (n, v) in instalados() { println!("{} {}", n, v); }
}
