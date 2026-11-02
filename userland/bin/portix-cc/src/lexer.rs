//! Analizador léxico del subconjunto de C de PCC (Portix C Compiler).
//! 100 % Rust `no_std`. Ignora directivas `#...`, comentarios `//` y `/* */`.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Num(i64),
    Str(Vec<u8>),
    Ident(String),
    /// Signos de puntuación y operadores (`+`, `==`, `<<=`…).
    Punct(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
}

const PUNCTS: &[&str] = &[
    "<<=", ">>=", "...", "==", "!=", "<=", ">=", "&&", "||", "++", "--", "+=", "-=", "*=", "/=",
    "%=", "&=", "|=", "^=", "<<", ">>", "->", "+", "-", "*", "/", "%", "=", "<", ">", "!", "~",
    "&", "|", "^", "(", ")", "{", "}", "[", "]", ";", ",", "?", ":", ".",
];

fn escape(c: u8) -> u8 {
    match c {
        b'n' => b'\n',
        b't' => b'\t',
        b'r' => b'\r',
        b'0' => 0,
        b'a' => 7,
        b'b' => 8,
        b'e' => 27,
        other => other,
    }
}

pub fn tokenize(src: &[u8]) -> Result<Vec<Token>, String> {
    let mut macros: Vec<(String, Vec<Tok>)> = Vec::new();
    tokenize_with(src, &mut macros, 1)
}

/// Tokeniza expandiendo macros objeto (`#define NOMBRE valor`). Las demás
/// directivas (`#include`, `#ifdef`…) se ignoran línea completa.
fn tokenize_with(src: &[u8], macros: &mut Vec<(String, Vec<Tok>)>, first_line: usize) -> Result<Vec<Token>, String> {
    let mut toks = Vec::new();
    let mut i = 0usize;
    let mut line = first_line;
    let n = src.len();
    while i < n {
        let c = src[i];
        if c == b'\n' { line += 1; i += 1; continue; }
        if c == b' ' || c == b'\t' || c == b'\r' { i += 1; continue; }
        if c == b'#' {
            let s = i;
            while i < n && src[i] != b'\n' { i += 1; }
            let dir = &src[s + 1..i];
            let dir_txt = core::str::from_utf8(dir).unwrap_or("").trim_start();
            if let Some(rest) = dir_txt.strip_prefix("define") {
                let rest = rest.trim();
                let name_end = rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')).unwrap_or(rest.len());
                let name = &rest[..name_end];
                if name.is_empty() { return Err(format!("linea {}: #define sin nombre", line)); }
                if rest[name_end..].starts_with('(') {
                    return Err(format!("linea {}: #define con parametros no soportado por PCC", line));
                }
                let body = tokenize_with(rest[name_end..].trim().as_bytes(), macros, line)?;
                let body: Vec<Tok> = body.into_iter().map(|t| t.tok).filter(|t| *t != Tok::Eof).collect();
                macros.retain(|m| m.0 != name);
                macros.push((String::from(name), body));
            }
            continue;
        }
        if c == b'/' && i + 1 < n && src[i + 1] == b'/' {
            while i < n && src[i] != b'\n' { i += 1; }
            continue;
        }
        if c == b'/' && i + 1 < n && src[i + 1] == b'*' {
            i += 2;
            loop {
                if i + 1 >= n { return Err(format!("linea {}: comentario sin cerrar", line)); }
                if src[i] == b'\n' { line += 1; }
                if src[i] == b'*' && src[i + 1] == b'/' { i += 2; break; }
                i += 1;
            }
            continue;
        }
        if c.is_ascii_digit() {
            let mut v: i64 = 0;
            if c == b'0' && i + 1 < n && (src[i + 1] == b'x' || src[i + 1] == b'X') {
                i += 2;
                while i < n && src[i].is_ascii_hexdigit() {
                    let d = (src[i] as char).to_digit(16).unwrap_or(0) as i64;
                    v = v.wrapping_mul(16).wrapping_add(d);
                    i += 1;
                }
            } else {
                while i < n && src[i].is_ascii_digit() {
                    v = v.wrapping_mul(10).wrapping_add((src[i] - b'0') as i64);
                    i += 1;
                }
            }
            // Sufijos L/U se toleran.
            while i < n && (src[i] == b'L' || src[i] == b'U' || src[i] == b'l' || src[i] == b'u') { i += 1; }
            toks.push(Token { tok: Tok::Num(v), line });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let s = i;
            while i < n && (src[i].is_ascii_alphanumeric() || src[i] == b'_') { i += 1; }
            let id = core::str::from_utf8(&src[s..i]).map_err(|_| format!("linea {}: identificador invalido", line))?;
            if let Some((_, body)) = macros.iter().find(|m| m.0 == id) {
                for t in body { toks.push(Token { tok: t.clone(), line }); }
                continue;
            }
            toks.push(Token { tok: Tok::Ident(String::from(id)), line });
            continue;
        }
        if c == b'"' {
            i += 1;
            let mut bytes = Vec::new();
            loop {
                if i >= n { return Err(format!("linea {}: cadena sin cerrar", line)); }
                let b = src[i];
                if b == b'"' { i += 1; break; }
                if b == b'\\' && i + 1 < n { bytes.push(escape(src[i + 1])); i += 2; continue; }
                if b == b'\n' { return Err(format!("linea {}: salto de linea dentro de cadena", line)); }
                bytes.push(b);
                i += 1;
            }
            // Concatenación de literales adyacentes: "a" "b"
            if let Some(Token { tok: Tok::Str(prev), .. }) = toks.last_mut() {
                prev.extend_from_slice(&bytes);
            } else {
                toks.push(Token { tok: Tok::Str(bytes), line });
            }
            continue;
        }
        if c == b'\'' {
            i += 1;
            if i >= n { return Err(format!("linea {}: caracter sin cerrar", line)); }
            let v = if src[i] == b'\\' && i + 1 < n { let e = escape(src[i + 1]); i += 2; e } else { let e = src[i]; i += 1; e };
            if i >= n || src[i] != b'\'' { return Err(format!("linea {}: literal de caracter invalido", line)); }
            i += 1;
            toks.push(Token { tok: Tok::Num(v as i64), line });
            continue;
        }
        let mut matched = false;
        for p in PUNCTS {
            let pb = p.as_bytes();
            if i + pb.len() <= n && &src[i..i + pb.len()] == pb {
                toks.push(Token { tok: Tok::Punct(p), line });
                i += pb.len();
                matched = true;
                break;
            }
        }
        if !matched {
            return Err(format!("linea {}: caracter inesperado '{}'", line, c as char));
        }
    }
    toks.push(Token { tok: Tok::Eof, line });
    Ok(toks)
}
