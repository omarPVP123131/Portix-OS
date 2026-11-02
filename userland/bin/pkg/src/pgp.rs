//! [T-004] Verificación de firmas OpenPGP (RFC 4880) para el `InRelease` de
//! Debian: mensaje con firma en claro (cleartext), firmas v4 RSA PKCS#1 v1.5
//! con SHA-256/SHA-512, claves públicas v4 RSA (primaria y subclaves) de los
//! ficheros `.asc` de `/etc/pkg/deb-keys`. Con la firma verificada, el SHA-256
//! de `Packages.gz` que figura en `InRelease` ancla todo el índice (y de ahí
//! cada `.deb`) a la clave del archivo de Debian, no sólo al canal HTTPS.
//!
//! Lo que NO se comprueba (anotado en TICKETS): caducidad y revocación de la
//! clave (van en auto-firmas que aquí no se validan), firmas Ed25519 (la clave
//! "Stable Release Key" es Ed25519; basta con una firma RSA válida de la clave
//! del archivo), firmas v3/v5/v6.

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::tls13::crypto::{BigUint, RsaPublicKey, Hash};

/// Clave pública RSA con su huella v4 (SHA-1 de 0x99 || len || cuerpo).
pub struct Clave { pub huella: [u8; 20], pub rsa: RsaPublicKey }

/// Resultado de una verificación correcta.
pub struct Firma { pub huella: [u8; 20], pub creada: u32, pub hash: &'static str }

/// Quita la armadura ASCII del bloque que empieza por `inicio` y devuelve los bytes.
fn desarmar(texto: &str, inicio: &str) -> Option<Vec<u8>> {
    let p = texto.find(inicio)?;
    let resto = &texto[p + inicio.len()..];
    // cabeceras de armadura hasta la línea vacía
    let mut lineas = resto.lines();
    for l in lineas.by_ref() { if l.trim().is_empty() { break; } }
    let mut b64 = String::new();
    for l in lineas {
        let l = l.trim();
        if l.starts_with("-----") { break; }
        if l.starts_with('=') { continue; } // CRC24 (no se comprueba: la firma manda)
        b64.push_str(l);
    }
    Some(portix_rt::tls13::x509::base64_decode(&b64))
}

/// Paquetes OpenPGP (etiqueta, cuerpo) con cabeceras de formato antiguo y nuevo.
fn paquetes(d: &[u8]) -> Vec<(u8, &[u8])> {
    let mut v = Vec::new();
    let mut p = 0usize;
    while p < d.len() {
        let tag = d[p];
        if tag & 0x80 == 0 { break; }
        let (etiqueta, len, cab) = if tag & 0x40 != 0 {
            if p + 1 >= d.len() { break; }
            let l = d[p + 1] as usize;
            if l < 192 { (tag & 0x3f, l, 2) }
            else if l < 224 { if p + 2 >= d.len() { break; } (tag & 0x3f, ((l - 192) << 8) + d[p + 2] as usize + 192, 3) }
            else if l == 255 { if p + 5 >= d.len() { break; } (tag & 0x3f, u32::from_be_bytes([d[p+2], d[p+3], d[p+4], d[p+5]]) as usize, 6) }
            else { break; } // longitud parcial: no aparece en claves ni firmas
        } else {
            let t = (tag >> 2) & 0x0f;
            match tag & 3 {
                0 => { if p + 1 >= d.len() { break; } (t, d[p + 1] as usize, 2) }
                1 => { if p + 2 >= d.len() { break; } (t, u16::from_be_bytes([d[p+1], d[p+2]]) as usize, 3) }
                2 => { if p + 4 >= d.len() { break; } (t, u32::from_be_bytes([d[p+1], d[p+2], d[p+3], d[p+4]]) as usize, 5) }
                _ => break,
            }
        };
        let ini = p + cab;
        if ini + len > d.len() { break; }
        v.push((etiqueta, &d[ini..ini + len]));
        p = ini + len;
    }
    v
}

/// MPI: 2 bytes de longitud en bits + magnitud big-endian.
fn mpi(b: &[u8], o: usize) -> Option<(&[u8], usize)> {
    if o + 2 > b.len() { return None; }
    let bits = u16::from_be_bytes([b[o], b[o + 1]]) as usize;
    let n = (bits + 7) / 8;
    if o + 2 + n > b.len() { return None; }
    Some((&b[o + 2..o + 2 + n], o + 2 + n))
}

/// Claves RSA v4 (primaria, etiqueta 6, y subclaves, etiqueta 14) de un `.asc`.
pub fn claves_de_asc(texto: &str) -> Vec<Clave> {
    let mut out = Vec::new();
    let Some(bin) = desarmar(texto, "-----BEGIN PGP PUBLIC KEY BLOCK-----") else { return out };
    for (t, b) in paquetes(&bin) {
        if (t != 6 && t != 14) || b.len() < 8 || b[0] != 4 || b[5] != 1 { continue; } // v4, RSA
        let Some((n, o)) = mpi(b, 6) else { continue };
        let Some((e, _)) = mpi(b, o) else { continue };
        let mut m = Vec::with_capacity(b.len() + 3);
        m.push(0x99); m.extend_from_slice(&(b.len() as u16).to_be_bytes()); m.extend_from_slice(b);
        out.push(Clave { huella: crate::sha1::sha1(&m), rsa: RsaPublicKey { n: BigUint::from_be_bytes(n), e: BigUint::from_be_bytes(e) } });
    }
    out
}

/// Texto firmado de un mensaje en claro, canonizado según RFC 4880 §7.1:
/// sin el escape de guion (`- `), sin blancos finales de línea y con CRLF
/// entre líneas (la última línea no lleva salto). Devuelve (texto canónico, texto original).
fn texto_en_claro(msg: &str) -> Option<(Vec<u8>, &str)> {
    let ini = msg.find("-----BEGIN PGP SIGNED MESSAGE-----")?;
    let tras_cab = msg[ini..].find("\n\n")? + ini + 2; // tras la línea vacía de las cabeceras (Hash: ...)
    let fin = msg[tras_cab..].find("-----BEGIN PGP SIGNATURE-----")? + tras_cab;
    let cuerpo = &msg[tras_cab..fin];
    // El salto de línea inmediatamente anterior a la armadura de la firma no forma parte del texto.
    let cuerpo_sin_ultimo = cuerpo.strip_suffix('\n').unwrap_or(cuerpo);
    let mut canon = Vec::with_capacity(cuerpo.len());
    for (i, l) in cuerpo_sin_ultimo.split('\n').enumerate() {
        if i > 0 { canon.extend_from_slice(b"\r\n"); }
        let l = l.strip_suffix('\r').unwrap_or(l);
        let l = l.strip_prefix("- ").unwrap_or(l);
        canon.extend_from_slice(l.trim_end_matches(|c| c == ' ' || c == '\t').as_bytes());
    }
    Some((canon, cuerpo))
}

/// Verifica el mensaje en claro `msg` contra `claves`. Devuelve la primera
/// firma RSA v4 válida de una clave confiada, o el motivo del rechazo.
pub fn verificar_en_claro(msg: &str, claves: &[Clave]) -> Result<Firma, String> {
    let (canon, _) = texto_en_claro(msg).ok_or("no es un mensaje OpenPGP en claro (falta BEGIN PGP SIGNED MESSAGE / SIGNATURE)")?;
    let bin = desarmar(msg, "-----BEGIN PGP SIGNATURE-----").ok_or("armadura de la firma ilegible")?;
    let mut vistas = 0usize;
    let mut motivo = String::from("ninguna firma RSA v4 de una clave confiada");
    let mut fallo_confiada: Option<String> = None; // una clave confiada firmó, pero la firma no cuadra: manda sobre lo demás
    for (t, b) in paquetes(&bin) {
        if t != 2 || b.len() < 10 || b[0] != 4 { continue; }
        vistas += 1;
        let (tipo, pk_algo, hash_algo) = (b[1], b[2], b[3]);
        if tipo != 0x01 { continue; }        // firma de texto canónico
        if pk_algo != 1 && pk_algo != 3 { if fallo_confiada.is_none() { motivo = alloc::format!("firma con algoritmo de clave {} (sólo RSA)", pk_algo); } continue; }
        let (hash, nombre) = match hash_algo { 8 => (Hash::Sha256, "SHA-256"), 10 => (Hash::Sha512, "SHA-512"), 9 => (Hash::Sha384, "SHA-384"), h => { motivo = alloc::format!("firma con hash {} no soportado", h); continue; } };
        let hl = u16::from_be_bytes([b[4], b[5]]) as usize;
        if 6 + hl + 2 > b.len() { continue; }
        let hashed = &b[..6 + hl];
        let subs = &b[6..6 + hl];
        let ul = u16::from_be_bytes([b[6 + hl], b[7 + hl]]) as usize;
        let o = 8 + hl + ul;
        if o + 2 > b.len() { continue; }
        let Some((sig, _)) = mpi(b, o + 2) else { continue };
        // subpaquetes con hash: 2 = fecha de creación, 33 = huella del emisor
        let mut creada = 0u32; let mut huella_emisor: Option<[u8; 20]> = None;
        let mut q = 0usize;
        while q < subs.len() {
            let mut l = subs[q] as usize; q += 1;
            if l >= 192 && l < 255 { if q >= subs.len() { break; } l = ((l - 192) << 8) + subs[q] as usize + 192; q += 1; }
            else if l == 255 { if q + 4 > subs.len() { break; } l = u32::from_be_bytes([subs[q], subs[q+1], subs[q+2], subs[q+3]]) as usize; q += 4; }
            if l == 0 || q + l > subs.len() { break; }
            let st = subs[q] & 0x7f; let cuerpo = &subs[q + 1..q + l];
            if st == 2 && cuerpo.len() == 4 { creada = u32::from_be_bytes([cuerpo[0], cuerpo[1], cuerpo[2], cuerpo[3]]); }
            if st == 33 && cuerpo.len() == 21 && cuerpo[0] == 4 { let mut h = [0u8; 20]; h.copy_from_slice(&cuerpo[1..]); huella_emisor = Some(h); }
            q += l;
        }
        // mensaje = texto canónico || parte con hash de la firma || 0x04 0xFF || len BE32
        let mut m = Vec::with_capacity(canon.len() + hashed.len() + 6);
        m.extend_from_slice(&canon); m.extend_from_slice(hashed);
        m.push(0x04); m.push(0xFF); m.extend_from_slice(&(hashed.len() as u32).to_be_bytes());
        // firma RSA con relleno a la longitud del módulo (el MPI puede venir más corto)
        for c in claves {
            if let Some(h) = huella_emisor { if h != c.huella { continue; } }
            let k = (c.rsa.n.bits() + 7) / 8;
            if sig.len() > k { continue; }
            let mut s = alloc::vec![0u8; k - sig.len()]; s.extend_from_slice(sig);
            if c.rsa.verify_pkcs1(hash, &m, &s) {
                return Ok(Firma { huella: c.huella, creada, hash: nombre });
            }
            fallo_confiada = Some(alloc::format!("la firma RSA del emisor {} NO verifica: contenido alterado o firma falsa", hex20(&c.huella)));
        }
        if huella_emisor.map(|h| !claves.iter().any(|c| c.huella == h)).unwrap_or(false) && fallo_confiada.is_none() {
            motivo = alloc::format!("firma de una clave NO confiada ({})", hex20(&huella_emisor.unwrap_or([0; 20])));
        }
    }
    if vistas == 0 { return Err("el bloque de firma no contiene paquetes de firma v4".into()); }
    Err(fallo_confiada.unwrap_or(motivo))
}

pub fn hex20(h: &[u8; 20]) -> String {
    let mut s = String::with_capacity(40);
    for b in h { s.push_str(&alloc::format!("{:02X}", b)); }
    s
}

/// Entrada `SHA256:` de `InRelease`/`Release` para `ruta` (p. ej. `main/binary-amd64/Packages.gz`):
/// devuelve (sha256 en hex, tamaño).
pub fn sha256_de_release(texto: &str, ruta: &str) -> Option<(String, usize)> {
    let mut en_sha256 = false;
    for l in texto.lines() {
        if !l.starts_with(' ') {
            en_sha256 = l.starts_with("SHA256:");
            continue;
        }
        if !en_sha256 { continue; }
        let mut it = l.split_whitespace();
        let (Some(h), Some(t), Some(r)) = (it.next(), it.next(), it.next()) else { continue };
        if r == ruta && h.len() == 64 { return Some((String::from(h), t.parse().ok()?)); }
    }
    None
}
