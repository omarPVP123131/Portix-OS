//! Analizador DER/X.509 y validación de cadenas (RFC 5280) — Rust `no_std`.
//!
//! Soporta claves RSA con firmas `sha256/384/512WithRSAEncryption`. Las
//! firmas ECDSA se reconocen y se rechazan explícitamente (pendiente), nunca
//! se dan por buenas.

use alloc::string::String;
use alloc::vec::Vec;
use super::crypto::{BigUint, RsaPublicKey, Hash};

// ───────────────────────────────── DER ──────────────────────────────────────

#[derive(Clone, Copy)]
pub struct Der<'a> { pub tag: u8, pub body: &'a [u8], pub raw: &'a [u8] }

/// Lee un TLV al inicio de `d`; devuelve (elemento, resto).
pub fn der_read(d: &[u8]) -> Option<(Der<'_>, &[u8])> {
    if d.len() < 2 { return None; }
    let tag = d[0];
    let mut i = 1;
    let first = d[i] as usize; i += 1;
    let len = if first < 0x80 { first } else {
        let n = first & 0x7F;
        if n == 0 || n > 4 || d.len() < i + n { return None; }
        let mut l = 0usize;
        for _ in 0..n { l = (l << 8) | d[i] as usize; i += 1; }
        l
    };
    if d.len() < i + len { return None; }
    Some((Der { tag, body: &d[i..i + len], raw: &d[..i + len] }, &d[i + len..]))
}

fn expect<'a>(d: &'a [u8], tag: u8) -> Option<(Der<'a>, &'a [u8])> {
    let (e, rest) = der_read(d)?;
    if e.tag != tag { return None; }
    Some((e, rest))
}

const TAG_SEQ: u8 = 0x30; const TAG_SET: u8 = 0x31; const TAG_INT: u8 = 0x02; const TAG_OID: u8 = 0x06;
const TAG_BITSTR: u8 = 0x03; const TAG_OCTSTR: u8 = 0x04; const TAG_UTCTIME: u8 = 0x17; const TAG_GENTIME: u8 = 0x18;
const TAG_BOOL: u8 = 0x01;

// OIDs (cuerpo DER)
const OID_RSA_ENC: &[u8] = &[0x2a,0x86,0x48,0x86,0xf7,0x0d,0x01,0x01,0x01];
const OID_SHA256_RSA: &[u8] = &[0x2a,0x86,0x48,0x86,0xf7,0x0d,0x01,0x01,0x0b];
const OID_SHA384_RSA: &[u8] = &[0x2a,0x86,0x48,0x86,0xf7,0x0d,0x01,0x01,0x0c];
const OID_SHA512_RSA: &[u8] = &[0x2a,0x86,0x48,0x86,0xf7,0x0d,0x01,0x01,0x0d];
const OID_ECDSA_SHA256: &[u8] = &[0x2a,0x86,0x48,0xce,0x3d,0x04,0x03,0x02];
const OID_ECDSA_SHA384: &[u8] = &[0x2a,0x86,0x48,0xce,0x3d,0x04,0x03,0x03];
const OID_EC_PUBKEY: &[u8] = &[0x2a,0x86,0x48,0xce,0x3d,0x02,0x01];
const OID_SECP384R1: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x22];
const OID_ED25519: &[u8] = &[0x2b, 0x65, 0x70];
const OID_PRIME256V1: &[u8] = &[0x2a,0x86,0x48,0xce,0x3d,0x03,0x01,0x07];
const OID_SAN: &[u8] = &[0x55,0x1d,0x11];
const OID_BASIC_CONSTRAINTS: &[u8] = &[0x55,0x1d,0x13];
const OID_CN: &[u8] = &[0x55,0x04,0x03];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SigAlg { RsaPkcs1(Hash), EcdsaSha256, EcdsaSha384, Desconocido, Ed25519 }

pub enum PublicKey { Rsa(RsaPublicKey), EcP256(super::p256::P256PublicKey), EcP384(super::ecgen::P384PublicKey), Ed25519(super::ed25519::Ed25519PublicKey), EcOtra, Desconocida }

pub struct Certificate {
    pub raw: Vec<u8>,
    pub tbs: Vec<u8>,
    pub issuer: Vec<u8>,
    pub subject: Vec<u8>,
    pub subject_cn: String,
    pub not_before: i64,
    pub not_after: i64,
    pub public_key: PublicKey,
    pub sig_alg: SigAlg,
    pub signature: Vec<u8>,
    pub dns_names: Vec<String>,
    pub is_ca: bool,
}

fn parse_time(e: &Der) -> Option<i64> {
    let s = e.body;
    let (year, rest) = match e.tag {
        TAG_UTCTIME => { if s.len() < 12 { return None; } let yy = num(&s[0..2])?; (if yy >= 50 { 1900 + yy } else { 2000 + yy }, &s[2..]) }
        TAG_GENTIME => { if s.len() < 14 { return None; } (num(&s[0..4])?, &s[4..]) }
        _ => return None,
    };
    let mo = num(&rest[0..2])?; let d = num(&rest[2..4])?; let h = num(&rest[4..6])?; let mi = num(&rest[6..8])?;
    let sec = if rest.len() >= 10 && rest[8].is_ascii_digit() { num(&rest[8..10])? } else { 0 };
    Some(days_from_civil(year, mo, d) * 86400 + h * 3600 + mi * 60 + sec)
}

fn num(b: &[u8]) -> Option<i64> { let mut v = 0i64; for &c in b { if !c.is_ascii_digit() { return None; } v = v * 10 + (c - b'0') as i64; } Some(v) }

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn sig_alg_from(seq: &Der) -> SigAlg {
    let oid = match expect(seq.body, TAG_OID) { Some((o, _)) => o.body, None => return SigAlg::Desconocido };
    if oid == OID_SHA256_RSA { SigAlg::RsaPkcs1(Hash::Sha256) }
    else if oid == OID_SHA384_RSA { SigAlg::RsaPkcs1(Hash::Sha384) }
    else if oid == OID_SHA512_RSA { SigAlg::RsaPkcs1(Hash::Sha512) }
    else if oid == OID_ECDSA_SHA256 { SigAlg::EcdsaSha256 }
    else if oid == OID_ECDSA_SHA384 { SigAlg::EcdsaSha384 }
    else if oid == OID_ED25519 { SigAlg::Ed25519 }
    else { SigAlg::Desconocido }
}

fn parse_spki(spki: &Der) -> PublicKey {
    let (alg, rest) = match expect(spki.body, TAG_SEQ) { Some(v) => v, None => return PublicKey::Desconocida };
    let oid = match expect(alg.body, TAG_OID) { Some((o, _)) => o.body, None => return PublicKey::Desconocida };
    let (bits, _) = match expect(rest, TAG_BITSTR) { Some(v) => v, None => return PublicKey::Desconocida };
    if bits.body.is_empty() { return PublicKey::Desconocida; }
    let key = &bits.body[1..];
    if oid == OID_RSA_ENC {
        let (seq, _) = match expect(key, TAG_SEQ) { Some(v) => v, None => return PublicKey::Desconocida };
        let (n, rest) = match expect(seq.body, TAG_INT) { Some(v) => v, None => return PublicKey::Desconocida };
        let (e, _) = match expect(rest, TAG_INT) { Some(v) => v, None => return PublicKey::Desconocida };
        PublicKey::Rsa(RsaPublicKey { n: BigUint::from_be_bytes(n.body), e: BigUint::from_be_bytes(e.body) })
    } else if oid == OID_EC_PUBKEY {
        // Parámetros: namedCurve OID; sólo prime256v1 (1.2.840.10045.3.1.7).
        let curve = alg.body.get(2 + oid.len()..).and_then(|r| expect(r, TAG_OID)).map(|(c, _)| c.body);
        if curve == Some(OID_PRIME256V1) {
            return match super::p256::P256PublicKey::from_sec1(key) { Some(k) => PublicKey::EcP256(k), None => PublicKey::EcOtra };
        }
        if curve == Some(OID_SECP384R1) {
            return match super::ecgen::P384PublicKey::from_sec1(key) { Some(k) => PublicKey::EcP384(k), None => PublicKey::EcOtra };
        }
        PublicKey::EcOtra
    } else if oid == OID_ED25519 {
        match super::ed25519::Ed25519PublicKey::from_bytes(key) { Some(k) => PublicKey::Ed25519(k), None => PublicKey::Desconocida }
    } else { PublicKey::Desconocida }
}

fn find_cn(name: &[u8]) -> String {
    let mut rest = name;
    while let Some((set, r)) = der_read(rest) {
        rest = r;
        if set.tag != TAG_SET { continue; }
        if let Some((seq, _)) = expect(set.body, TAG_SEQ) {
            if let Some((oid, v)) = expect(seq.body, TAG_OID) {
                if oid.body == OID_CN {
                    if let Some((s, _)) = der_read(v) { return String::from_utf8_lossy(s.body).into_owned(); }
                }
            }
        }
    }
    String::new()
}

fn parse_extensions(exts: &[u8], cert: &mut Certificate) {
    let mut rest = exts;
    while let Some((ext, r)) = der_read(rest) {
        rest = r;
        if ext.tag != TAG_SEQ { continue; }
        let (oid, mut body) = match expect(ext.body, TAG_OID) { Some(v) => v, None => continue };
        if let Some((b, r2)) = der_read(body) { if b.tag == TAG_BOOL { body = r2; } }
        let (val, _) = match expect(body, TAG_OCTSTR) { Some(v) => v, None => continue };
        if oid.body == OID_SAN {
            if let Some((names, _)) = expect(val.body, TAG_SEQ) {
                let mut nr = names.body;
                while let Some((gn, r3)) = der_read(nr) {
                    nr = r3;
                    if gn.tag == 0x82 { cert.dns_names.push(String::from_utf8_lossy(gn.body).to_ascii_lowercase()); }
                    // iPAddress (IPv4) se guarda en notacion decimal para comparar con el host literal.
                    if gn.tag == 0x87 && gn.body.len() == 4 { cert.dns_names.push(alloc::format!("{}.{}.{}.{}", gn.body[0], gn.body[1], gn.body[2], gn.body[3])); }
                    // [IPv6] SAN iPAddress de 16 bytes, en forma canónica RFC 5952 para compararlo con el host de la URL
                    if gn.tag == 0x87 && gn.body.len() == 16 { let mut a = [0u8; 16]; a.copy_from_slice(gn.body); cert.dns_names.push(crate::net::fmt_ipv6(&a)); }
                }
            }
        } else if oid.body == OID_BASIC_CONSTRAINTS {
            if let Some((seq, _)) = expect(val.body, TAG_SEQ) {
                if let Some((b, _)) = der_read(seq.body) { if b.tag == TAG_BOOL && !b.body.is_empty() && b.body[0] != 0 { cert.is_ca = true; } }
            }
        }
    }
}

/// Analiza un certificado X.509 v3 en DER.
pub fn parse_certificate(der: &[u8]) -> Option<Certificate> {
    let (outer, _) = expect(der, TAG_SEQ)?;
    let (tbs, rest) = expect(outer.body, TAG_SEQ)?;
    let (sig_alg_seq, rest) = expect(rest, TAG_SEQ)?;
    let (sig_bits, _) = expect(rest, TAG_BITSTR)?;
    if sig_bits.body.is_empty() { return None; }

    let mut t = tbs.body;
    if let Some((v, r)) = der_read(t) { if v.tag == 0xA0 { t = r; } }
    let (_serial, r) = expect(t, TAG_INT)?;
    let (_tbs_alg, r) = expect(r, TAG_SEQ)?;
    let (issuer, r) = expect(r, TAG_SEQ)?;
    let (validity, r) = expect(r, TAG_SEQ)?;
    let (nb, vr) = der_read(validity.body)?; let (na, _) = der_read(vr)?;
    let (subject, r) = expect(r, TAG_SEQ)?;
    let (spki, mut r) = expect(r, TAG_SEQ)?;

    let mut cert = Certificate {
        raw: outer.raw.to_vec(), tbs: tbs.raw.to_vec(),
        issuer: issuer.raw.to_vec(), subject: subject.raw.to_vec(),
        subject_cn: find_cn(subject.body),
        not_before: parse_time(&nb)?, not_after: parse_time(&na)?,
        public_key: parse_spki(&spki), sig_alg: sig_alg_from(&sig_alg_seq),
        signature: sig_bits.body[1..].to_vec(), dns_names: Vec::new(), is_ca: false,
    };
    while let Some((e, r2)) = der_read(r) {
        r = r2;
        if e.tag == 0xA3 { if let Some((exts, _)) = expect(e.body, TAG_SEQ) { parse_extensions(exts.body, &mut cert); } }
    }
    Some(cert)
}

/// Comprueba la firma de `child` con la clave pública de `issuer`.
pub fn verify_signature(child: &Certificate, issuer: &Certificate) -> Result<(), &'static str> {
    match (&issuer.public_key, child.sig_alg) {
        (PublicKey::Rsa(k), SigAlg::RsaPkcs1(h)) => if k.verify_pkcs1(h, &child.tbs, &child.signature) { Ok(()) } else { Err("firma RSA invalida") },
        (PublicKey::EcP256(k), SigAlg::EcdsaSha256) => if k.verify_der(&crate::tls::sha256(&child.tbs), &child.signature) { Ok(()) } else { Err("firma ECDSA P-256 invalida") },
        (PublicKey::EcP256(k), SigAlg::EcdsaSha384) => if k.verify_der(&crate::tls13::crypto::sha384(&child.tbs), &child.signature) { Ok(()) } else { Err("firma ECDSA P-256/SHA-384 invalida") },
        (PublicKey::EcP384(k), SigAlg::EcdsaSha384) => if k.verify_der(&crate::tls13::crypto::sha384(&child.tbs), &child.signature) { Ok(()) } else { Err("firma ECDSA P-384 invalida") },
        (PublicKey::EcP384(k), SigAlg::EcdsaSha256) => if k.verify_der(&crate::tls::sha256(&child.tbs), &child.signature) { Ok(()) } else { Err("firma ECDSA P-384/SHA-256 invalida") },
        (PublicKey::Ed25519(k), SigAlg::Ed25519) => if k.verify(&child.tbs, &child.signature) { Ok(()) } else { Err("firma Ed25519 invalida") },
        (PublicKey::EcOtra, _) => Err("[TLS-SIG-ALG-NO-SOPORTADO] curva EC del emisor distinta de P-256/P-384"),
        (PublicKey::Rsa(_), SigAlg::EcdsaSha256) | (PublicKey::Rsa(_), SigAlg::EcdsaSha384) => Err("firma ECDSA con clave RSA: incoherente"),
        _ => Err("algoritmo de firma desconocido"),
    }
}

/// ¿`host` coincide con el certificado (SAN dNSName, o CN si no hay SAN)?
pub fn hostname_matches(cert: &Certificate, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let names: Vec<String> = if cert.dns_names.is_empty() { alloc::vec![cert.subject_cn.to_ascii_lowercase()] } else { cert.dns_names.clone() };
    for n in names {
        if n == host { return true; }
        if let Some(suffix) = n.strip_prefix("*.") {
            if let Some(hs) = host.split_once('.') { if hs.1 == suffix && !hs.0.is_empty() { return true; } }
        }
    }
    false
}

/// Valida la cadena `chain` (hoja primero) contra las raíces `roots` en el instante `now`.
/// Devuelve el nombre (CN) de la raíz que ancla la confianza.
pub fn validate_chain(chain: &[Certificate], roots: &[Certificate], host: &str, now: i64) -> Result<String, String> {
    validate_chain_opt(chain, roots, Some(host), now)
}

/// Igual que `validate_chain`, con nombre de host opcional (certificados de cliente en mTLS).
pub fn validate_chain_opt(chain: &[Certificate], roots: &[Certificate], host: Option<&str>, now: i64) -> Result<String, String> {
    let leaf = chain.first().ok_or_else(|| String::from("cadena vacia"))?;
    if let Some(host) = host { if !hostname_matches(leaf, host) {
        return Err(alloc::format!("el certificado no es para '{}' (SAN: {:?}, CN: '{}')", host, leaf.dns_names, leaf.subject_cn));
    } }
    let mut current = leaf;
    for depth in 0..8 {
        if now < current.not_before || now > current.not_after {
            return Err(alloc::format!("certificado '{}' fuera de vigencia (ahora={} validez={}..{})", current.subject_cn, now, current.not_before, current.not_after));
        }
        // ¿Lo firma una raíz de confianza?
        for root in roots {
            if root.subject == current.issuer {
                match verify_signature(current, root) {
                    Ok(()) => return Ok(root.subject_cn.clone()),
                    Err(e) => return Err(alloc::format!("firma de '{}' por la raiz '{}' no valida: {}", current.subject_cn, root.subject_cn, e)),
                }
            }
        }
        // ¿Lo firma un intermedio de la cadena?
        let mut next: Option<&Certificate> = None;
        for c in chain.iter().skip(1) {
            if c.subject == current.issuer && !core::ptr::eq(c, current) { next = Some(c); break; }
        }
        match next {
            Some(iss) => {
                if !iss.is_ca { return Err(alloc::format!("'{}' firma sin ser CA (basicConstraints)", iss.subject_cn)); }
                verify_signature(current, iss).map_err(|e| alloc::format!("firma de '{}' por '{}' no valida: {}", current.subject_cn, iss.subject_cn, e))?;
                current = iss;
            }
            None => return Err(alloc::format!("emisor de '{}' no esta en la cadena ni en el almacen de raices (profundidad {})", current.subject_cn, depth)),
        }
    }
    Err(String::from("cadena demasiado larga"))
}

// ───────────────────────────── PEM / almacén ────────────────────────────────

fn b64_val(c: u8) -> Option<u32> {
    match c { b'A'..=b'Z' => Some((c - b'A') as u32), b'a'..=b'z' => Some((c - b'a' + 26) as u32), b'0'..=b'9' => Some((c - b'0' + 52) as u32), b'+' => Some(62), b'/' => Some(63), _ => None }
}

pub fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32; let mut nbits = 0;
    for &c in s.as_bytes() {
        if let Some(v) = b64_val(c) { acc = (acc << 6) | v; nbits += 6; if nbits >= 8 { nbits -= 8; out.push((acc >> nbits) as u8); acc &= (1 << nbits) - 1; } }
    }
    out
}

/// Extrae todos los bloques `-----BEGIN CERTIFICATE-----` de un PEM.
pub fn parse_pem_certificates(pem: &str) -> Vec<Certificate> {
    let mut out = Vec::new();
    let mut rest = pem;
    while let Some(start) = rest.find("-----BEGIN CERTIFICATE-----") {
        let after = &rest[start + 27..];
        let end = match after.find("-----END CERTIFICATE-----") { Some(e) => e, None => break };
        let der = base64_decode(&after[..end]);
        if let Some(c) = parse_certificate(&der) { out.push(c); }
        rest = &after[end + 25..];
    }
    out
}

/// Carga las raíces de `/etc/ssl/certs/ca-certificates.crt`.
pub fn load_system_roots() -> Vec<Certificate> {
    match crate::fs::read("/etc/ssl/certs/ca-certificates.crt") {
        Ok(data) => parse_pem_certificates(&String::from_utf8_lossy(&data)),
        Err(_) => Vec::new(),
    }
}


/// Bloques PEM `-----BEGIN <tag>-----` decodificados a DER.
pub fn pem_blocks(pem: &str, tag: &str) -> Vec<Vec<u8>> {
    let begin = alloc::format!("-----BEGIN {}-----", tag);
    let end = alloc::format!("-----END {}-----", tag);
    let mut out = Vec::new();
    let mut rest = pem;
    while let Some(st) = rest.find(&begin) {
        let after = &rest[st + begin.len()..];
        let Some(en) = after.find(&end) else { break };
        let b64: String = after[..en].chars().filter(|c| !c.is_whitespace()).collect();
        out.push(base64_decode(&b64));
        rest = &after[en + end.len()..];
    }
    out
}

/// [T-002] Clave pública RSA en PEM SPKI (`PUBLIC KEY`), p. ej. las claves de
/// firma de los repositorios de Alpine (`/etc/apk/keys/*.rsa.pub`).
pub fn parse_rsa_public_key_pem(pem: &str) -> Option<RsaPublicKey> {
    let der = pem_blocks(pem, "PUBLIC KEY").into_iter().next()?;
    let (spki, _) = expect(&der, TAG_SEQ)?;
    match parse_spki(&spki) { PublicKey::Rsa(k) => Some(k), _ => None }
}

/// Clave EC P-256 privada en PEM: SEC1 (`EC PRIVATE KEY`) o PKCS#8 (`PRIVATE KEY`).
pub fn parse_ec_private_key_pem(pem: &str) -> Option<super::p256::P256PrivateKey> {
    fn tlv(d: &[u8]) -> Option<(u8, &[u8], &[u8])> {
        if d.len() < 2 { return None; }
        let tag = d[0]; let mut l = d[1] as usize; let mut p = 2;
        if l & 0x80 != 0 { let n = l & 0x7f; if n == 0 || n > 2 || d.len() < 2 + n { return None; } l = 0; for i in 0..n { l = (l << 8) | d[2 + i] as usize; } p += n; }
        if d.len() < p + l { return None; }
        Some((tag, &d[p..p + l], &d[p + l..]))
    }
    fn sec1(der: &[u8]) -> Option<[u8; 32]> {
        let (t, body, _) = tlv(der)?; if t != 0x30 { return None; }
        let (t, v, rest) = tlv(body)?; if t != 0x02 || v != [1] { return None; }
        let (t, d, _) = tlv(rest)?; if t != 0x04 || d.len() != 32 { return None; }
        let mut k = [0u8; 32]; k.copy_from_slice(d); Some(k)
    }
    let d = if let Some(b) = pem_blocks(pem, "EC PRIVATE KEY").into_iter().next() { sec1(&b)? }
        else if let Some(b) = pem_blocks(pem, "PRIVATE KEY").into_iter().next() {
            let (t, body, _) = tlv(&b)?; if t != 0x30 { return None; }
            let (_, _, rest) = tlv(body)?;
            let (_, _, rest) = tlv(rest)?;
            let (t, inner, _) = tlv(rest)?; if t != 0x04 { return None; }
            sec1(inner)?
        } else { return None; };
    super::p256::P256PrivateKey::from_scalar(&d)
}

/// Clave privada RSA en PEM: `RSA PRIVATE KEY` (PKCS#1) o `PRIVATE KEY` (PKCS#8 con OID rsaEncryption).
pub fn parse_rsa_private_key_pem(pem: &str) -> Option<super::crypto::RsaPrivateKey> {
    fn tlv(d: &[u8]) -> Option<(u8, &[u8], &[u8])> {
        if d.len() < 2 { return None; }
        let tag = d[0]; let mut l = d[1] as usize; let mut p = 2;
        if l & 0x80 != 0 { let n = l & 0x7f; if n == 0 || n > 2 || d.len() < 2 + n { return None; } l = 0; for i in 0..n { l = (l << 8) | d[2 + i] as usize; } p += n; }
        if d.len() < p + l { return None; }
        Some((tag, &d[p..p + l], &d[p + l..]))
    }
    fn pkcs1(der: &[u8]) -> Option<super::crypto::RsaPrivateKey> {
        use super::crypto::BigUint;
        let (t, body, _) = tlv(der)?; if t != 0x30 { return None; }
        let mut rest = body; let mut ints: alloc::vec::Vec<BigUint> = alloc::vec::Vec::new();
        while ints.len() < 9 { let (t, v, r) = tlv(rest)?; if t != 0x02 { return None; } ints.push(BigUint::from_be_bytes(v)); rest = r; }
        if !ints[0].is_zero() { return None; } // version 0 (sin multi-primo)
        let mut it = ints.into_iter().skip(1);
        let n = it.next()?; let e = it.next()?; let d = it.next()?; let p = it.next()?; let q = it.next()?; let dp = it.next()?; let dq = it.next()?; let qinv = it.next()?;
        Some(super::crypto::RsaPrivateKey { n, e, d, p, q, dp, dq, qinv })
    }
    if let Some(b) = pem_blocks(pem, "RSA PRIVATE KEY").into_iter().next() { return pkcs1(&b); }
    let b = pem_blocks(pem, "PRIVATE KEY").into_iter().next()?;
    let (t, body, _) = tlv(&b)?; if t != 0x30 { return None; }
    let (_, _, rest) = tlv(body)?;                 // version
    let (t, alg, rest) = tlv(rest)?; if t != 0x30 { return None; }
    let (t, oid, _) = tlv(alg)?; if t != 0x06 || oid != [0x2a,0x86,0x48,0x86,0xf7,0x0d,0x01,0x01,0x01] { return None; } // rsaEncryption
    let (t, inner, _) = tlv(rest)?; if t != 0x04 { return None; }
    pkcs1(inner)
}

/// Clave privada Ed25519 en PEM PKCS#8 (RFC 8410): OID 1.3.101.112, semilla en OCTET STRING anidado.
pub fn parse_ed25519_private_key_pem(pem: &str) -> Option<super::ed25519::Ed25519PrivateKey> {
    fn tlv(d: &[u8]) -> Option<(u8, &[u8], &[u8])> {
        if d.len() < 2 { return None; }
        let tag = d[0]; let mut l = d[1] as usize; let mut p = 2;
        if l & 0x80 != 0 { let n = l & 0x7f; if n == 0 || n > 2 || d.len() < 2 + n { return None; } l = 0; for i in 0..n { l = (l << 8) | d[2 + i] as usize; } p += n; }
        if d.len() < p + l { return None; }
        Some((tag, &d[p..p + l], &d[p + l..]))
    }
    let b = pem_blocks(pem, "PRIVATE KEY").into_iter().next()?;
    let (t, body, _) = tlv(&b)?; if t != 0x30 { return None; }
    let (_, _, rest) = tlv(body)?;
    let (t, alg, rest) = tlv(rest)?; if t != 0x30 { return None; }
    let (t, oid, _) = tlv(alg)?; if t != 0x06 || oid != [0x2b, 0x65, 0x70] { return None; }
    let (t, inner, _) = tlv(rest)?; if t != 0x04 { return None; }
    let (t, seed, _) = tlv(inner)?; if t != 0x04 { return None; }
    super::ed25519::Ed25519PrivateKey::from_seed(seed)
}
