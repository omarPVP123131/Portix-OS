//! [T-002] Paquetes `.apk` REALES de Alpine Linux, descargados del espejo
//! oficial por HTTPS, con la cadena de verificación completa de `apk-tools`:
//!
//!   APKINDEX.tar.gz = gzip(firma) ‖ gzip(índice)
//!       firma `.SIGN.RSA.<clave>` = RSA PKCS#1 v1.5 / SHA-1 sobre los bytes
//!       COMPRIMIDOS del segundo miembro, con la clave de /etc/apk/keys/.
//!   paquete.apk     = gzip(firma) ‖ gzip(control) ‖ gzip(datos)
//!       firma sobre el segmento de control comprimido;
//!       identidad `C:Q1<base64(sha1(control))>` debe coincidir con el índice;
//!       `.PKGINFO` del control lleva `datahash = sha256(datos comprimidos)`.
//!
//! Nada se instala si falla cualquiera de los tres eslabones.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use portix_rt::println;

use crate::inflate::{gunzip_miembro, gunzip_a};
use crate::tar;
use crate::sha1::{sha1, base64, DIGESTINFO_SHA1};

const REPOS_CONF: &str = "/etc/apk/repositories";
const KEYS_DIR: &str = "/etc/apk/keys";
const BASE: &str = "/var/pkg/apk";
const ARCH: &str = "x86_64";

#[derive(Clone)]
pub struct Paquete {
    pub nombre: String,
    pub version: String,
    pub checksum: String, // "Q1...."
    pub tamano: usize,
    pub deps: Vec<String>,
    pub provides: Vec<String>,
    pub repo: String,
}

pub struct Indice { pub paquetes: BTreeMap<String, Paquete>, pub provee: BTreeMap<String, String> }

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

fn repos() -> Vec<String> {
    match portix_rt::fs::read(REPOS_CONF) {
        Ok(t) => String::from_utf8_lossy(&t).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect(),
        Err(_) => Vec::new(),
    }
}

fn nombre_repo(url: &str) -> String {
    url.trim_end_matches('/').rsplit('/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("-")
}

/// Separa los miembros gzip concatenados: devuelve los rangos comprimidos.
fn miembros(d: &[u8]) -> Result<Vec<(usize, usize, Vec<u8>)>, &'static str> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < d.len() {
        let (datos, fin) = gunzip_miembro(d, p)?;
        out.push((p, fin, datos));
        p = fin;
    }
    Ok(out)
}

/// Verifica la firma `.SIGN.RSA.<clave>` (tar del primer miembro) sobre `firmado`.
fn verificar_firma(tar_firma: &[u8], firmado: &[u8]) -> Result<String, String> {
    let ents = tar::entradas(tar_firma).map_err(|e| e.to_string())?;
    let e = ents.iter().find(|e| e.nombre.starts_with(".SIGN.RSA.")).ok_or("sin entrada .SIGN.RSA.* (paquete sin firmar)")?;
    let clave = &e.nombre[".SIGN.RSA.".len()..];
    if clave.contains('/') || clave.contains("..") { return Err("nombre de clave sospechoso".into()); }
    let pem = portix_rt::fs::read(&alloc::format!("{}/{}", KEYS_DIR, clave)).map_err(|_| alloc::format!("clave '{}' no esta en {} (no confiada)", clave, KEYS_DIR))?;
    let pem = core::str::from_utf8(&pem).map_err(|_| "clave PEM ilegible")?;
    let pk = portix_rt::tls13::x509::parse_rsa_public_key_pem(pem).ok_or("clave PEM no es RSA SPKI")?;
    if !pk.verify_pkcs1_digestinfo(&DIGESTINFO_SHA1, &sha1(firmado), e.datos) {
        return Err(alloc::format!("firma RSA de '{}' NO verifica", clave));
    }
    Ok(clave.to_string())
}

fn descargar(url: &str, destino: &str) -> Result<Vec<u8>, String> {
    let rc = crate::descargar(url, destino);
    if rc != 0 { return Err(alloc::format!("curl rc={} para {}", rc, url)); }
    portix_rt::fs::read(destino).map_err(|e| alloc::format!("no se pudo leer {} (errno {})", destino, e))
}

/// Descarga y verifica el APKINDEX de cada repositorio; guarda el texto
/// verificado en /var/pkg/apk/<repo>.APKINDEX.
pub fn actualizar() -> Result<usize, String> {
    let rs = repos();
    if rs.is_empty() { return Err(alloc::format!("{} vacio o ausente", REPOS_CONF)); }
    mkdir_p(BASE);
    let mut total = 0;
    for r in rs {
        let url = alloc::format!("{}/{}/APKINDEX.tar.gz", r, ARCH);
        let tgz = alloc::format!("{}/{}.tar.gz", BASE, nombre_repo(&r));
        println!("[APK] descargando {}", url);
        let bytes = descargar(&url, &tgz)?;
        let ms = miembros(&bytes).map_err(|e| alloc::format!("{}: {}", url, e))?;
        if ms.len() != 2 { return Err(alloc::format!("{}: se esperaban 2 miembros gzip, hay {}", url, ms.len())); }
        let clave = verificar_firma(&ms[0].2, &bytes[ms[1].0..ms[1].1])?;
        let ents = tar::entradas(&ms[1].2).map_err(|e| e.to_string())?;
        let idx = ents.iter().find(|e| e.nombre == "APKINDEX").ok_or("sin APKINDEX en el tar")?;
        let n = idx.datos.split(|&c| c == b'\n').filter(|l| l.starts_with(b"P:")).count();
        portix_rt::fs::write(&alloc::format!("{}/{}.APKINDEX", BASE, nombre_repo(&r)), idx.datos).map_err(|e| alloc::format!("escritura del indice: errno {}", e))?;
        let _ = portix_rt::fs::unlink(&tgz);
        println!("[APK] indice {}: firma RSA de '{}' verificada sobre {} bytes comprimidos; {} paquetes", nombre_repo(&r), clave, ms[1].1 - ms[1].0, n);
        total += n;
    }
    Ok(total)
}

pub fn cargar_indice() -> Result<Indice, String> {
    let mut ind = Indice { paquetes: BTreeMap::new(), provee: BTreeMap::new() };
    for r in repos() {
        let ruta = alloc::format!("{}/{}.APKINDEX", BASE, nombre_repo(&r));
        let texto = match portix_rt::fs::read(&ruta) { Ok(t) => t, Err(_) => continue };
        let mut cur: Option<Paquete> = None;
        for linea in texto.split(|&c| c == b'\n') {
            if linea.is_empty() {
                if let Some(p) = cur.take() { registrar(&mut ind, p); }
                continue;
            }
            if linea.len() < 2 || linea[1] != b':' { continue; }
            let val = String::from_utf8_lossy(&linea[2..]).into_owned();
            let p = cur.get_or_insert_with(|| Paquete { nombre: String::new(), version: String::new(), checksum: String::new(), tamano: 0, deps: Vec::new(), provides: Vec::new(), repo: r.clone() });
            match linea[0] {
                b'P' => p.nombre = val,
                b'V' => p.version = val,
                b'C' => p.checksum = val,
                b'S' => p.tamano = val.parse().unwrap_or(0),
                b'D' => p.deps = val.split_whitespace().map(|s| s.to_string()).collect(),
                b'p' => p.provides = val.split_whitespace().map(|s| s.to_string()).collect(),
                _ => {}
            }
        }
        if let Some(p) = cur.take() { registrar(&mut ind, p); }
    }
    if ind.paquetes.is_empty() { return Err("no hay indices verificados: ejecute 'pkg update'".into()); }
    Ok(ind)
}

fn registrar(ind: &mut Indice, p: Paquete) {
    if p.nombre.is_empty() { return; }
    for pr in &p.provides {
        let k = sin_version(pr);
        ind.provee.entry(k.to_string()).or_insert_with(|| p.nombre.clone());
    }
    ind.paquetes.entry(p.nombre.clone()).or_insert(p);
}

fn sin_version(dep: &str) -> &str {
    let fin = dep.find(|c| c == '=' || c == '<' || c == '>' || c == '~').unwrap_or(dep.len());
    &dep[..fin]
}

fn instalados() -> Vec<(String, String)> {
    match portix_rt::fs::read(&alloc::format!("{}/installed", BASE)) {
        Ok(t) => String::from_utf8_lossy(&t).lines().filter_map(|l| { let mut it = l.split_whitespace(); Some((it.next()?.to_string(), it.next()?.to_string())) }).collect(),
        Err(_) => Vec::new(),
    }
}

pub fn esta_instalado(nombre: &str) -> bool { instalados().iter().any(|(n, _)| n == nombre) }

/// Cierre transitivo de dependencias en orden de instalación (deps primero).
pub fn resolver(ind: &Indice, objetivos: &[String]) -> Result<Vec<String>, String> {
    let mut orden: Vec<String> = Vec::new();
    let mut visitando: Vec<String> = Vec::new();
    fn visitar(ind: &Indice, nombre: &str, orden: &mut Vec<String>, visitando: &mut Vec<String>) -> Result<(), String> {
        if orden.iter().any(|n| n == nombre) || visitando.iter().any(|n| n == nombre) { return Ok(()); }
        let p = ind.paquetes.get(nombre).ok_or_else(|| alloc::format!("'{}' no existe en los indices", nombre))?;
        visitando.push(nombre.to_string());
        for d in &p.deps {
            if d.starts_with('!') { continue; } // conflicto: no se instala nada por él
            let base = sin_version(d);
            let real = if ind.paquetes.contains_key(base) { base.to_string() } else if let Some(pr) = ind.provee.get(base) { pr.clone() } else {
                return Err(alloc::format!("dependencia '{}' de '{}' sin proveedor en los indices", d, nombre));
            };
            if esta_instalado(&real) { continue; }
            visitar(ind, &real, orden, visitando)?;
        }
        visitando.pop();
        orden.push(nombre.to_string());
        Ok(())
    }
    for o in objetivos {
        let real = if ind.paquetes.contains_key(o) { o.clone() } else if let Some(pr) = ind.provee.get(o.as_str()) { pr.clone() } else { return Err(alloc::format!("'{}' no existe en los indices", o)); };
        if esta_instalado(&real) { println!("[APK] {} ya instalado", real); continue; }
        visitar(ind, &real, &mut orden, &mut visitando)?;
    }
    Ok(orden)
}

fn campo_pkginfo<'a>(texto: &'a str, clave: &str) -> Option<&'a str> {
    texto.lines().find_map(|l| { let (k, v) = l.split_once('=')?; if k.trim() == clave { Some(v.trim()) } else { None } })
}

/// Descarga, verifica (firma + C:Q1 + datahash) e instala un paquete.
pub fn instalar(p: &Paquete) -> Result<usize, String> {
    let fichero = alloc::format!("{}-{}.apk", p.nombre, p.version);
    let url = alloc::format!("{}/{}/{}", p.repo, ARCH, fichero);
    let cache = alloc::format!("{}/cache", BASE);
    mkdir_p(&cache);
    let destino = alloc::format!("{}/{}", cache, fichero);
    let rc = crate::descargar_paquete(&url, &destino);
    if rc != 0 { return Err(alloc::format!("curl rc={} para {}", rc, url)); }
    let bytes = portix_rt::fs::read(&destino).map_err(|e| alloc::format!("no se pudo leer {} (errno {})", destino, e))?;
    if bytes.len() != p.tamano { return Err(alloc::format!("{}: tamano {} != S:{} del indice", fichero, bytes.len(), p.tamano)); }
    // Miembros 0 (firma) y 1 (control) son pequenos y se inflan en memoria; el
    // 2 (datos) ocupa el resto del fichero y se procesa en flujo mas abajo.
    let (firma_tar, fin0) = gunzip_miembro(&bytes, 0).map_err(|e| alloc::format!("{}: firma: {}", fichero, e))?;
    let (control_tar, fin1) = gunzip_miembro(&bytes, fin0).map_err(|e| alloc::format!("{}: control: {}", fichero, e))?;
    if fin1 >= bytes.len() { return Err(alloc::format!("{}: sin segmento de datos", fichero)); }
    let ms: [(usize, usize, Vec<u8>); 2] = [(0, fin0, firma_tar), (fin0, fin1, control_tar)];
    let control = &bytes[ms[1].0..ms[1].1];
    let datos = &bytes[fin1..];
    let ms2_inicio = fin1;
    // 1) firma RSA sobre el control comprimido
    let clave = verificar_firma(&ms[0].2, control)?;
    // 2) identidad del indice
    let ident = alloc::format!("Q1{}", base64(&sha1(control)));
    if ident != p.checksum { return Err(alloc::format!("{}: identidad C:{} != indice C:{}", fichero, ident, p.checksum)); }
    // 3) .PKGINFO y datahash
    let ctl = tar::entradas(&ms[1].2).map_err(|e| e.to_string())?;
    let info = ctl.iter().find(|e| e.nombre == ".PKGINFO").ok_or("sin .PKGINFO")?;
    let info = String::from_utf8_lossy(info.datos).into_owned();
    let datahash = campo_pkginfo(&info, "datahash").ok_or("sin datahash en .PKGINFO")?;
    let real = { let mut h = crate::sha256_estado::Sha256::nuevo(); h.actualizar(datos); crate::hex32(&h.terminar()) }; // sin copiar `datos` (tls::sha256 duplicaba el .apk entero)
    if real != datahash { return Err(alloc::format!("{}: datahash {} != {}", fichero, real, datahash)); }
    if campo_pkginfo(&info, "pkgname") != Some(p.nombre.as_str()) { return Err("pkgname de .PKGINFO no coincide".into()); }
    let scripts: Vec<&str> = ctl.iter().filter(|e| e.nombre.starts_with(".pre-") || e.nombre.starts_with(".post-") || e.nombre.starts_with(".trigger")).map(|e| e.nombre.as_str()).collect();
    println!("[APK] {}: firma RSA '{}' OK, C:{} OK, datahash OK ({} bytes de datos)", fichero, clave, ident, datos.len());
    if !scripts.is_empty() { println!("[APK] {}: scripts de instalacion NO ejecutados ({:?}); Portix no corre scripts de paquete", p.nombre, scripts); }
    // 4) desplegar el tar de datos sobre la raiz ext2 EN FLUJO (gzip -> tar ->
    //    fichero), sin inflar el paquete entero en memoria (gcc: 140 MiB).
    let mut desp = Despliegue { lista: Vec::new(), n: 0, fichero: None, error: None };
    {
        let mut tarf = tar::TarFlujo::new(&mut desp);
        let mut sum = |t: &[u8]| -> Result<(), &'static str> { tarf.alimentar(t) };
        gunzip_a(&bytes, ms2_inicio, &mut sum).map_err(|e| alloc::format!("{}: datos: {}", fichero, e))?;
        tarf.terminar().map_err(|e| e.to_string())?;
    }
    if let Some(e) = desp.error.take() { return Err(e); }
    let lista = desp.lista; let n = desp.n;
    mkdir_p(&alloc::format!("{}/files", BASE));
    let _ = portix_rt::fs::write(&alloc::format!("{}/files/{}", BASE, p.nombre), lista.join("\n").as_bytes());
    let mut inst = instalados(); inst.retain(|(nm, _)| nm != &p.nombre); inst.push((p.nombre.clone(), p.version.clone()));
    let txt: String = inst.iter().map(|(a, b)| alloc::format!("{} {}\n", a, b)).collect();
    portix_rt::fs::write(&alloc::format!("{}/installed", BASE), txt.as_bytes()).map_err(|e| alloc::format!("base de datos: errno {}", e))?;
    let _ = portix_rt::fs::unlink(&destino);
    Ok(n)
}

/// [T-003] Manejador del tar en flujo: crea directorios, escribe ficheros por
/// trozos, enlaces simbolicos y duros; guarda el error detallado.
pub struct Despliegue { pub lista: Vec<String>, pub n: usize, fichero: Option<(portix_rt::File, String, u32)>, pub error: Option<String> }

impl Despliegue { pub fn nuevo() -> Self { Despliegue { lista: Vec::new(), n: 0, fichero: None, error: None } } }

fn ruta_de(nombre: &str) -> Option<String> {
    let nombre = nombre.trim_start_matches("./").trim_end_matches('/');
    if nombre.is_empty() || nombre.starts_with('.') { return None; }
    Some(alloc::format!("/{}", nombre))
}

/// [b187] Identidad de Portix frente a los paquetes de otra distro: un fichero de
/// configuración que YA existe bajo /etc (os-release, passwd, profile, hostname,
/// issue...) se conserva, como hace dpkg con un conffile modificado localmente.
/// base-files de Debian traía `/etc/os-release -> ../usr/lib/os-release` y
/// convertía a Portix en "Debian GNU/Linux 13" ante `cat /etc/os-release`.
fn conservar_local(ruta: &str) -> bool {
    let protegido = ruta.starts_with("/etc/") || ruta == "/usr/lib/os-release";
    if !protegido { return false; }
    let mut st = portix_rt::fs::Stat::default();
    if portix_rt::fs::lstat(ruta, &mut st).is_err() { return false; }
    if (st.st_mode & 0o170000) == 0o040000 { return false; } // los directorios se reutilizan
    println!("[PKG] {} conservado: configuracion local de Portix (el paquete no la sustituye)", ruta);
    true
}

impl tar::Manejador for Despliegue {
    fn entrada(&mut self, c: &tar::Cabecera) -> Result<bool, &'static str> {
        let ruta = match ruta_de(&c.nombre) { Some(r) => r, None => return Ok(false) };
        if let Some(pos) = ruta.rfind('/') { if pos > 0 { mkdir_p(&ruta[..pos]); } }
        if matches!(c.tipo, b'0' | 0 | b'7' | b'2' | b'1') && conservar_local(&ruta) {
            return Ok(false); // `false`: el flujo tar descarta los datos de la entrada
        }
        match c.tipo {
            b'5' => { mkdir_p(&ruta); Ok(false) }
            b'0' | 0 | b'7' => {
                let _ = portix_rt::fs::unlink(&ruta);
                match portix_rt::File::open(&ruta, portix_rt::O_WRONLY | portix_rt::O_CREAT | portix_rt::O_TRUNC) {
                    Ok(f) => { self.fichero = Some((f, ruta, c.modo & 0o7777)); Ok(true) }
                    Err(e) => { self.error = Some(alloc::format!("creando {}: errno {}", ruta, e)); Err("no se pudo crear el fichero") }
                }
            }
            b'2' => {
                // [T-004] Como dpkg: un enlace NUNCA sustituye a un directorio existente
                // (base-files trae ./bin -> usr/bin, ./lib -> usr/lib, ./sbin, ./lib64:
                // en Portix /bin y /lib son directorios reales con el sistema base).
                let mut st = portix_rt::fs::Stat::default();
                if portix_rt::fs::lstat(&ruta, &mut st).is_ok() && (st.st_mode & 0o170000) == 0o040000 {
                    println!("[APK] {} -> {} omitido: {} es un directorio real (como hace dpkg)", ruta, c.enlace, ruta);
                    return Ok(false);
                }
                let _ = portix_rt::fs::unlink(&ruta);
                if let Err(e) = portix_rt::fs::symlink(&c.enlace, &ruta) { self.error = Some(alloc::format!("symlink {} -> {}: errno {}", ruta, c.enlace, e)); return Err("symlink fallo"); }
                self.lista.push(ruta); self.n += 1; Ok(false)
            }
            b'1' => {
                let origen = alloc::format!("/{}", c.enlace.trim_start_matches("./"));
                let _ = portix_rt::fs::unlink(&ruta);
                if let Err(e) = portix_rt::fs::link(&origen, &ruta) { self.error = Some(alloc::format!("link {} -> {}: errno {}", ruta, origen, e)); return Err("link fallo"); }
                self.lista.push(ruta); self.n += 1; Ok(false)
            }
            _ => { println!("[APK] {}: entrada de tipo '{}' ignorada", ruta, c.tipo as char); Ok(false) }
        }
    }
    fn datos(&mut self, mut d: &[u8]) -> Result<(), &'static str> {
        if let Some((f, ruta, _)) = &self.fichero {
            while !d.is_empty() {
                match f.write(d) {
                    Ok(0) => { self.error = Some(alloc::format!("escribiendo {}: escritura de 0 bytes", ruta)); return Err("escritura corta"); }
                    Ok(n) => d = &d[n..],
                    Err(e) => { self.error = Some(alloc::format!("escribiendo {}: errno {}", ruta, e)); return Err("escritura fallo"); }
                }
            }
        }
        Ok(())
    }
    fn fin_entrada(&mut self, _c: &tar::Cabecera) -> Result<(), &'static str> {
        if let Some((f, ruta, modo)) = self.fichero.take() {
            drop(f);
            let _ = portix_rt::fs::chmod(&ruta, modo);
            self.lista.push(ruta); self.n += 1;
        }
        Ok(())
    }
}

pub fn anadir(objetivos: &[String]) -> i32 {
    let ind = match cargar_indice() {
        Ok(i) => i,
        Err(_) => {
            if let Err(e) = actualizar() { println!("[APK-FAIL] {}", e); return 1; }
            match cargar_indice() { Ok(i) => i, Err(e) => { println!("[APK-FAIL] {}", e); return 1; } }
        }
    };
    let orden = match resolver(&ind, objetivos) { Ok(o) => o, Err(e) => { println!("[APK-FAIL] {}", e); return 1; } };
    if orden.is_empty() { println!("[APK-OK] nada que instalar"); return 0; }
    println!("[APK] plan ({}): {}", orden.len(), orden.join(" "));
    let mut resumen: Vec<String> = Vec::new();
    for nombre in &orden {
        let p = &ind.paquetes[nombre];
        match instalar(p) {
            Ok(n) => { println!("[APK] {}-{} instalado ({} entradas)", p.nombre, p.version, n); resumen.push(alloc::format!("{}-{}", p.nombre, p.version)); }
            Err(e) => { println!("[APK-FAIL] {}: {}", nombre, e); return 1; }
        }
    }
    println!("[APK-OK] instalados: {}", resumen.join(" "));
    0
}

pub fn eliminar(nombre: &str) -> i32 {
    let lista = match portix_rt::fs::read(&alloc::format!("{}/files/{}", BASE, nombre)) { Ok(t) => t, Err(_) => { println!("[APK-FAIL] '{}' no esta instalado", nombre); return 1; } };
    let mut n = 0;
    for l in String::from_utf8_lossy(&lista).lines().rev() { if portix_rt::fs::unlink(l) == 0 { n += 1; } }
    let _ = portix_rt::fs::unlink(&alloc::format!("{}/files/{}", BASE, nombre));
    let inst: Vec<(String, String)> = instalados().into_iter().filter(|(nm, _)| nm != nombre).collect();
    let txt: String = inst.iter().map(|(a, b)| alloc::format!("{} {}\n", a, b)).collect();
    let _ = portix_rt::fs::write(&alloc::format!("{}/installed", BASE), txt.as_bytes());
    println!("[APK-OK] {} eliminado ({} ficheros)", nombre, n);
    0
}

pub fn listar() {
    for (n, v) in instalados() { println!("{} {}", n, v); }
}
