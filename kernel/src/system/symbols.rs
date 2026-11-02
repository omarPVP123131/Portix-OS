// kernel/src/system/symbols.rs — Tabla de símbolos por proceso (ciclo v6.x)
//
// Extrae la tabla de símbolos (`.symtab`) del ejecutable ELF que se carga y la
// conserva asociada a cada proceso, para dos fines concretos:
//
//   1. Perfilado simbólico: el profiler de silicio deja de agrupar muestras por
//      página de código y las atribuye a la función real del ELF.
//   2. Depuración real: `/proc/<pid>/symbols` publica las direcciones de las
//      funciones para que el depurador de Ring 3 (`/bin/dbg`) resuelva nombres,
//      ponga puntos de parada por símbolo y simbolice el desensamblado.
//
// Todo el análisis se hace con lecturas comprobadas sobre el binario (que es
// dato no confiable): ninguna ruta puede provocar pánico.

use alloc::string::String;

use crate::arch::spinlock::Spinlock;
use crate::drivers::serial;

/// Máximo de funciones conservadas por imagen.
/// [FRENTE C] 96 -> 256: con 96 el perfilador descartaba funciones pequeñas
/// (se conservan las de mayor tamaño) y un bucle caliente podía quedar sin
/// nombre. 256 cubre enteras las imágenes del sistema (test_posix tiene 201).
pub const MAX_SYMBOLS: usize = 256;
/// Longitud máxima de un nombre ya desenmascarado.
pub const SYM_NAME_MAX: usize = 40;
/// Imágenes distintas memorizadas (LRU implícita por reutilización).
pub const MAX_MAPS: usize = 8;
/// Vínculos pid → imagen.
pub const MAX_PID_LINKS: usize = 32;

const SHT_SYMTAB: u32 = 2;
const STT_FUNC: u8 = 2;

// ─────────────────────────────────────────────────────────────────────────────
//  Entidades
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct Symbol {
    pub addr: u64, // dirección en tiempo de ejecución (value + load_bias)
    pub size: u32,
    pub name_len: u8,
    pub name: [u8; SYM_NAME_MAX],
}

impl Symbol {
    pub const fn empty() -> Self {
        Symbol { addr: 0, size: 0, name_len: 0, name: [0u8; SYM_NAME_MAX] }
    }

    pub fn name_str(&self) -> &str {
        let n = self.name_len as usize;
        core::str::from_utf8(&self.name[..n.min(SYM_NAME_MAX)]).unwrap_or("?")
    }

    /// ¿`rip` cae dentro de esta función?
    pub fn contains(&self, rip: u64) -> bool {
        let end = self.addr.saturating_add(self.size.max(1) as u64);
        rip >= self.addr && rip < end
    }
}

#[derive(Clone, Copy)]
pub struct SymbolMap {
    pub in_use: bool,
    pub id: u32,        // generación (identifica de forma única cada imagen)
    pub base: u64,      // desplazamiento de carga (PIE) aplicado a los símbolos
    pub total: usize,   // símbolos presentes en el ELF
    pub kept: usize,    // símbolos conservados (ordenados por dirección)
    pub syms: [Symbol; MAX_SYMBOLS],
}

impl SymbolMap {
    pub const fn empty() -> Self {
        SymbolMap { in_use: false, id: 0, base: 0, total: 0, kept: 0, syms: [Symbol::empty(); MAX_SYMBOLS] }
    }

    /// Búsqueda binaria del símbolo que contiene `rip`.
    pub fn find(&self, rip: u64) -> Option<(usize, u64)> {
        let mut lo = 0usize;
        let mut hi = self.kept;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let s = &self.syms[mid];
            if rip < s.addr {
                hi = mid;
            } else if s.contains(rip) {
                return Some((mid, rip - s.addr));
            } else {
                lo = mid + 1;
            }
        }
        None
    }
}

pub struct SymbolRegistry {
    pub next_id: u32,
    pub maps: [SymbolMap; MAX_MAPS],
    /// (pid, map_id, generation) — el índice de la imagen se resuelve aquí.
    pub links: [(u64, u32); MAX_PID_LINKS],
}

impl SymbolRegistry {
    pub const fn new() -> Self {
        SymbolRegistry { next_id: 1, maps: [SymbolMap::empty(); MAX_MAPS], links: [(0, 0); MAX_PID_LINKS] }
    }
}

pub static SYMBOLS: Spinlock<SymbolRegistry> = Spinlock::new(SymbolRegistry::new());
static REGISTERED_TOTAL: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static RESOLVED_TOTAL: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static UNRESOLVED_TOTAL: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

// ─────────────────────────────────────────────────────────────────────────────
//  Lectura comprobada de estructuras ELF64
// ─────────────────────────────────────────────────────────────────────────────

fn rd_u16(d: &[u8], off: usize) -> Option<u16> {
    let b = d.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn rd_u32(d: &[u8], off: usize) -> Option<u32> {
    let b = d.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn rd_u64(d: &[u8], off: usize) -> Option<u64> {
    let b = d.get(off..off.checked_add(8)?)?;
    Some(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

fn cstr(bytes: &[u8], off: usize, max: usize) -> (String, usize) {
    let mut s = String::new();
    let mut i = off;
    let mut n = 0;
    while i < bytes.len() && n < max {
        let c = bytes[i];
        if c == 0 { break; }
        s.push(c as char);
        i += 1;
        n += 1;
    }
    (s, n)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Desenmascarado de nombres (Rust legacy `_ZN..` y Rust v0 `_R..`)
// ─────────────────────────────────────────────────────────────────────────────

/// Decodificador del esquema de nombres Rust v0 (`_R...`) y del esquema
/// heredado (`_ZN...E`).
///
/// Implementa las reglas del esquema publicado en la RFC 2603: longitud
/// decimal seguida de un separador `_` opcional antes de los caracteres,
/// disambiguadores `s<base62>_`, espacios de nombres `N<ns>`, argumentos
/// genéricos `I...E` y retroreferencias `B<base62>_`. El resultado es una
/// versión legible (no necesariamente idéntica a la de `rustc-demangle` en
/// los casos exoticos: impls de traits, clausuras y tipos compuestos), pero
/// siempre reconocible y sin caracteres de control.
pub fn demangle(raw: &str) -> String {
    let b = raw.as_bytes();
    if b.starts_with(b"_Z") && b.len() > 4 && b[b.len() - 1] == b'E' {
        return demangle_legacy(raw);
    }
    if !b.starts_with(b"_R") {
        return clip(raw);
    }
    let mut out = String::new();
    let mut p = 2usize;
    while p < b.len() && b[p].is_ascii_digit() { p += 1; } // versión de codificación
    let mut gen = 0usize;
    let mut guard = 0usize;
    while p < b.len() && guard < 96 && out.len() < 72 {
        guard += 1;
        let c = b[p];
        match c {
            b'C' => { p = skip_disa(b, p + 1).0; match ident(b, p, &mut out) { Some(np) => p = np, None => break } }
            b'N' => {
                p += 1;
                if p < b.len() {
                    let ns = b[p];
                    p += 1;
                    if ns.is_ascii_uppercase() {
                        let (np, val) = skip_disa(b, p);
                        p = np;
                        if out.len() + 16 < 72 {
                            let etiqueta = if ns == b'C' { "{closure}" } else if ns == b'S' { "{shim}" } else { "{fn}" };
                            out.push_str(etiqueta);
                            let _ = val;
                        }
                    }
                }
            }
            b'M' | b'X' => { p = skip_disa(b, p + 1).0; }
            b'Y' => { p += 1; }
            b'I' => { p += 1; gen += 1; out.push('<'); }
            b'E' => {
                p += 1;
                if gen > 0 { gen -= 1; out.push('>'); } else { break; }
            }
            b'B' => { p = skip_b62(b, p + 1).0; }
            b'0'..=b'9' | b'u' => match ident(b, p, &mut out) { Some(np) => p = np, None => break },
            b'S' | b'A' | b'T' | b'R' | b'Q' | b'P' | b'D' | b'L' | b'G' => { p += 1; }
            _ => break,
        }
    }
    while gen > 0 { out.push('>'); gen -= 1; }
    if out.is_empty() { clip(raw) } else { out }
}

/// Nombre heredado `_ZN<len><seg>...E` (se descartan los hashes `h<16>`).
fn demangle_legacy(raw: &str) -> String {
    let b = raw.as_bytes();
    let mut out = String::new();
    let mut p = 3usize;
    let end = b.len().saturating_sub(1);
    while p < end && out.len() < 72 {
        let mut len = 0usize;
        while p < end && b[p].is_ascii_digit() { len = len * 10 + (b[p] - b'0') as usize; p += 1; }
        if len == 0 || p + len > end { break; }
        let seg = match core::str::from_utf8(&b[p..p + len]) { Ok(s) => s, Err(_) => { p += len; continue } };
        p += len;
        if (seg.starts_with('h') && seg.len() == 17) || seg == "_" { continue; }
        if !out.is_empty() { out.push_str("::"); }
        out.push_str(seg);
    }
    if out.is_empty() { clip(raw) } else { out }
}

/// Identificador: `<len>[_]<caracteres>` con marca opcional de punycode.
fn ident(b: &[u8], pos: usize, out: &mut String) -> Option<usize> {
    let mut p = pos;
    if p < b.len() && b[p] == b'u' { p += 1; }
    let st = p;
    let mut len = 0usize;
    while p < b.len() && b[p].is_ascii_digit() {
        len = len.checked_mul(10)?.checked_add((b[p] - b'0') as usize)?;
        p += 1;
        if len > 4096 { return None; }
    }
    if p == st { return None; }
    if p < b.len() && b[p] == b'_' { p += 1; }
    if p.checked_add(len)? > b.len() { return None; }
    if len > 0 {
        let seg = core::str::from_utf8(&b[p..p + len]).ok()?;
        if seg.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
            if !out.is_empty() && !out.ends_with('<') { out.push_str("::"); }
            out.push_str(seg);
        }
    }
    Some(p + len)
}

/// Disambiguador `s<base62>_` → (nueva posición, valor).
fn skip_disa(b: &[u8], pos: usize) -> (usize, u64) {
    if pos < b.len() && b[pos] == b's' { skip_b62(b, pos + 1) } else { (pos, 0) }
}

/// Número base62 terminado en `_` (el primer `_` vale 0).
fn skip_b62(b: &[u8], pos: usize) -> (usize, u64) {
    let mut p = pos;
    if p < b.len() && b[p] == b'_' { return (p + 1, 0); }
    let mut x: u64 = 0;
    while p < b.len() && b[p] != b'_' {
        let c = b[p];
        let d = if c.is_ascii_digit() { (c - b'0') as u64 }
                else if c.is_ascii_lowercase() { 10 + (c - b'a') as u64 }
                else if c.is_ascii_uppercase() { 36 + (c - b'A') as u64 }
                else { break };
        x = x.saturating_mul(62).saturating_add(d);
        p += 1;
    }
    ((p + 1).min(b.len()), x.saturating_add(1))
}

/// Recorte de seguridad para nombres no desenmascarables.
fn clip(raw: &str) -> String {
    let mut s = String::new();
    for c in raw.chars().take(SYM_NAME_MAX) { s.push(c); }
    s
}

// ─────────────────────────────────────────────────────────────────────────────
//  Extracción de la tabla desde el ELF
// ─────────────────────────────────────────────────────────────────────────────

/// Extrae las funciones de `.symtab` y las inserta en `map`.
/// Devuelve el número de símbolos encontrados en el ELF.
pub fn extract_into(data: &[u8], base: u64, map: &mut SymbolMap) -> usize {
    // [FRENTE C-4] Reinicio IN SITU: `*map = SymbolMap::empty()` construía un
    // temporal de ~14 KiB en la pila del kernel (16 KiB) antes de copiarlo.
    map.in_use = false;
    map.id = 0;
    map.base = 0;
    map.total = 0;
    map.kept = 0;
    for s in map.syms.iter_mut() {
        s.addr = 0;
        s.size = 0;
        s.name_len = 0;
    }
    // Cabecera ELF64: e_shoff(0x28), e_shentsize(0x3A), e_shnum(0x3C), e_shstrndx(0x3E)
    if data.len() < 64 || &data[0..4] != b"\x7FELF" {
        return 0;
    }
    let (shoff, shentsize, shnum) = match (rd_u64(data, 0x28), rd_u16(data, 0x3A), rd_u16(data, 0x3C)) {
        (Some(a), Some(b), Some(c)) => (a as usize, b as usize, c as usize),
        _ => return 0,
    };
    if shentsize < 64 || shnum == 0 || shnum > 512 { return 0; }

    // Localizar SHT_SYMTAB (tipo 2) y su tabla de cadenas asociada (sh_link)
    let mut sym_off = 0usize;
    let mut sym_size = 0usize;
    let mut str_idx = usize::MAX;
    for i in 0..shnum {
        let sh = match shoff.checked_add(i.checked_mul(shentsize).unwrap_or(usize::MAX)) { Some(v) => v, None => break };
        let sh_type = match rd_u32(data, sh + 4) { Some(v) => v, None => break };
        if sh_type == SHT_SYMTAB {
            sym_off = rd_u64(data, sh + 24).unwrap_or(0) as usize;
            sym_size = rd_u64(data, sh + 32).unwrap_or(0) as usize;
            str_idx = rd_u32(data, sh + 40).unwrap_or(0) as usize;
            break;
        }
    }
    if sym_off == 0 || sym_size == 0 || str_idx == usize::MAX { return 0; }

    // Sección de cadenas
    let sh = match shoff.checked_add(str_idx.checked_mul(shentsize).unwrap_or(usize::MAX)) { Some(v) => v, None => return 0 };
    let str_off = rd_u64(data, sh + 24).unwrap_or(0) as usize;
    let str_size = rd_u64(data, sh + 32).unwrap_or(0) as usize;
    let strtab = match data.get(str_off..str_off.saturating_add(str_size)) {
        Some(s) => s,
        None => return 0,
    };

    map.base = base;
    let count = sym_size / 24;
    let mut total = 0usize;
    let mut worst_idx = 0usize;
    let mut worst_size = 0u32;
    for i in 0..count {
        let off = match sym_off.checked_add(i * 24) { Some(v) => v, None => break };
        let name_off = match rd_u32(data, off) { Some(v) => v as usize, None => break };
        let info = match data.get(off + 4) { Some(v) => *v, None => break };
        let shndx = rd_u16(data, off + 6).unwrap_or(0);
        let value = rd_u64(data, off + 8).unwrap_or(0);
        let size = rd_u64(data, off + 16).unwrap_or(0);
        if info & 0xF != STT_FUNC || shndx == 0 || value == 0 || size == 0 { continue; }
        total += 1;

        let (raw_name, raw_len) = cstr(strtab, name_off, 160);
        if raw_len == 0 { continue; }
        let pretty = demangle(&raw_name);
        let nb = pretty.as_bytes();
        let nlen = nb.len().min(SYM_NAME_MAX);
        // Rust local (no enmascarado) → se antepone marcador para distinguirlo
        if nb.is_empty() { continue; }

        let mut sym = Symbol::empty();
        sym.addr = value.wrapping_add(base);
        sym.size = if size > u32::MAX as u64 { u32::MAX } else { size as u32 };
        sym.name_len = nlen as u8;
        sym.name[..nlen].copy_from_slice(&nb[..nlen]);

        if map.kept < MAX_SYMBOLS {
            if map.kept == 0 || sym.size < worst_size || map.kept == MAX_SYMBOLS {
                worst_size = sym.size;
                worst_idx = map.kept;
            }
            map.syms[map.kept] = sym;
            map.kept += 1;
            // recalcular el peor (el mínimo) tras inserciones
            if sym.size < worst_size { worst_size = sym.size; worst_idx = map.kept - 1; }
        } else if sym.size > worst_size {
            map.syms[worst_idx] = sym;
            // recalcular mínimo
            let mut w = u32::MAX;
            let mut wi = 0usize;
            for (k, s) in map.syms.iter().enumerate() {
                if s.size < w { w = s.size; wi = k; }
            }
            worst_size = w;
            worst_idx = wi;
        }
    }

    // Orden por dirección (inserción, acotado a MAX_SYMBOLS)
    for i in 1..map.kept {
        let key = map.syms[i];
        let mut j = i;
        while j > 0 && map.syms[j - 1].addr > key.addr {
            map.syms[j] = map.syms[j - 1];
            j -= 1;
        }
        map.syms[j] = key;
    }

    map.total = total;
    map.in_use = map.kept > 0;
    map.id = 0;
    total
}

// ─────────────────────────────────────────────────────────────────────────────
//  Registro y consulta
// ─────────────────────────────────────────────────────────────────────────────

/// Registra la tabla de símbolos de un proceso recién cargado (o reemplazado
/// por `execve`). Devuelve el identificador de imagen asignado.
pub fn register_for_pid(pid: u64, data: &[u8], base: u64) -> u32 {
    // [FRENTE C-4] La tabla temporal vive en el montículo, NUNCA en la pila del
    // kernel: con MAX_SYMBOLS=256 ocupa ~14 KiB y la pila de kernel por proceso
    // es de 16 KiB. Construirla en la pila desbordaba hacia abajo y pisaba las
    // tablas de páginas del proceso vecino (el padre de un fork+exec perdía el
    // mapeo de su pila de usuario: #PF EC=6 al reanudar tras waitpid).
    // Se reserva memoria a cero directamente (un `SymbolMap` todo-ceros es
    // válido) para que ni siquiera exista un temporal en la pila al construirlo.
    let mut fresh: alloc::boxed::Box<SymbolMap> = unsafe {
        let layout = core::alloc::Layout::new::<SymbolMap>();
        let p = alloc::alloc::alloc_zeroed(layout) as *mut SymbolMap;
        if p.is_null() { return 0; }
        alloc::boxed::Box::from_raw(p)
    };
    let total = extract_into(data, base, &mut fresh);
    let mut guard = match SYMBOLS.try_lock() {
        Some(g) => g,
        None => return 0,
    };
    if total == 0 && fresh.kept == 0 {
        // Imagen sin símbolos (tiraje): se rompe el vínculo anterior.
        for l in guard.links.iter_mut() {
            if l.0 == pid { *l = (0, 0); }
        }
        return 0;
    }
    let id = guard.next_id;
    guard.next_id = guard.next_id.wrapping_add(1).max(1);
    fresh.id = id;
    fresh.in_use = true;

    // Reutilizar la entrada de esta misma imagen si ya existe (mismo base y
    // mismas direcciones): evita duplicar tablas idénticas.
    let mut slot = usize::MAX;
    for (i, m) in guard.maps.iter().enumerate() {
        if m.in_use && m.base == base && m.total == fresh.total && m.kept == fresh.kept
            && m.syms[0].addr == fresh.syms[0].addr && m.syms[m.kept.saturating_sub(1)].addr == fresh.syms[fresh.kept.saturating_sub(1)].addr {
            slot = i;
            break;
        }
    }
    let map_id;
    if slot != usize::MAX {
        map_id = guard.maps[slot].id;
    } else {
        let mut free = usize::MAX;
        let mut lru = 0usize;
        let mut lru_id = u32::MAX;
        for (i, m) in guard.maps.iter().enumerate() {
            if !m.in_use { free = i; break; }
            if m.id < lru_id { lru_id = m.id; lru = i; }
        }
        slot = if free != usize::MAX { free } else { lru };
        // Copia directa montículo -> tabla estática, sin temporal en la pila.
        unsafe { core::ptr::copy_nonoverlapping(&*fresh as *const SymbolMap, &mut guard.maps[slot] as *mut SymbolMap, 1); }
        map_id = id;
    }

    // Vínculo pid → imagen
    let mut placed = false;
    for l in guard.links.iter_mut() {
        if l.0 == pid { *l = (pid, map_id); placed = true; break; }
    }
    if !placed {
        for l in guard.links.iter_mut() {
            if l.0 == 0 { *l = (pid, map_id); placed = true; break; }
        }
    }
    if !placed {
        // Reemplazo del vínculo más antiguo (no hay orden temporal: el primero)
        guard.links[0] = (pid, map_id);
    }
    REGISTERED_TOTAL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    serial::write_str("[SYMS] PID=");
    serial::write_usize(pid as usize);
    serial::write_str(" imagen=");
    serial::write_usize(map_id as usize);
    serial::write_str(" simbolos=");
    serial::write_usize(guard.maps[slot].kept);
    serial::write_str("/");
    serial::write_usize(total);
    serial::write_str(" base=");
    serial::write_hex(base as usize);
    serial::write_str("\n");
    map_id
}

/// Copia el vínculo simbólico del padre al hijo (fork conserva la imagen).
pub fn inherit(parent_pid: u64, child_pid: u64) {
    if let Some(mut g) = SYMBOLS.try_lock() {
        let mut id = 0u32;
        for l in g.links.iter() {
            if l.0 == parent_pid { id = l.1; break; }
        }
        if id == 0 { return; }
        for l in g.links.iter_mut() {
            if l.0 == child_pid || l.0 == 0 { *l = (child_pid, id); return; }
        }
        g.links[0] = (child_pid, id);
    }
}

/// Rompe el vínculo de un proceso terminado (la imagen permanece en caché).
pub fn forget_pid(pid: u64) {
    if let Some(mut g) = SYMBOLS.try_lock() {
        for l in g.links.iter_mut() {
            if l.0 == pid { *l = (0, 0); }
        }
    }
}

pub fn map_id_of(pid: u64) -> Option<u32> {
    let g = SYMBOLS.try_lock()?;
    for l in g.links.iter() {
        if l.0 == pid && l.1 != 0 { return Some(l.1); }
    }
    None
}

/// Atribuye una dirección de instrucción a una función: (imagen, índice, offset).
pub fn attribute(pid: u64, rip: u64) -> Option<(u32, usize, u64)> {
    let g = SYMBOLS.try_lock()?;
    let mut id = 0u32;
    for l in g.links.iter() {
        if l.0 == pid { id = l.1; break; }
    }
    if id == 0 { return None; }
    for m in g.maps.iter() {
        if m.in_use && m.id == id {
            if let Some((idx, off)) = m.find(rip) {
                RESOLVED_TOTAL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                return Some((id, idx, off));
            }
            UNRESOLVED_TOTAL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            return None;
        }
    }
    None
}

/// Nombre y metadatos de un símbolo por (imagen, índice).
pub fn symbol_info(map_id: u32, idx: usize) -> Option<(String, u64, u32)> {
    let g = SYMBOLS.try_lock()?;
    for m in g.maps.iter() {
        if m.in_use && m.id == map_id && idx < m.kept {
            let s = &m.syms[idx];
            return Some((String::from(s.name_str()), s.addr, s.size));
        }
    }
    None
}

/// Nombre del símbolo de una imagen que contiene la dirección indicada.
pub fn name_at_address(map_id: u32, addr: u64) -> Option<String> {
    let g = SYMBOLS.try_lock()?;
    for m in g.maps.iter() {
        if m.in_use && m.id == map_id {
            if let Some((idx, _)) = m.find(addr) {
                return Some(String::from(m.syms[idx].name_str()));
            }
        }
    }
    None
}

/// Cuántos símbolos conserva la imagen de un proceso (0 si no hay tabla).
pub fn symbol_count(pid: u64) -> usize {
    let Some(g) = SYMBOLS.try_lock() else { return 0 };
    let mut id = 0u32;
    for l in g.links.iter() {
        if l.0 == pid { id = l.1; break; }
    }
    if id == 0 { return 0; }
    for m in g.maps.iter() {
        if m.in_use && m.id == id { return m.kept; }
    }
    0
}

/// Símbolo que contiene una dirección, como texto "nombre+0xN".
pub fn resolve_text(pid: u64, rip: u64) -> Option<String> {
    let (id, idx, off) = attribute(pid, rip)?;
    let (name, _, _) = symbol_info(id, idx)?;
    Some(alloc::format!("{}+{:#x}", name, off))
}

pub fn get_stats() -> (usize, usize, u64, u64, u64) {
    let maps_active = SYMBOLS.try_lock().map(|g| g.maps.iter().filter(|m| m.in_use).count()).unwrap_or(0);
    let links_active = SYMBOLS.try_lock().map(|g| g.links.iter().filter(|l| l.0 != 0).count()).unwrap_or(0);
    (
        maps_active,
        links_active,
        REGISTERED_TOTAL.load(core::sync::atomic::Ordering::Relaxed),
        RESOLVED_TOTAL.load(core::sync::atomic::Ordering::Relaxed),
        UNRESOLVED_TOTAL.load(core::sync::atomic::Ordering::Relaxed),
    )
}

/// Contenido de `/proc/<pid>/symbols`.
pub fn get_symbols_report(pid: u64) -> String {
    let mut out = String::new();
    let (_name, kept, total, base, present) = {
        let Some(g) = SYMBOLS.try_lock() else { return String::from("(registro de simbolos ocupado)\n") };
        let mut id = 0u32;
        for l in g.links.iter() {
            if l.0 == pid { id = l.1; break; }
        }
        let mut info = (String::new(), 0usize, 0usize, 0u64, false);
        for m in g.maps.iter() {
            if m.in_use && m.id == id {
                info = (String::new(), m.kept, m.total, m.base, true);
                break;
            }
        }
        info
    };
    out.push_str(&alloc::format!(
        "Tabla de simbolos de PID={} — {} funciones conservadas de {} del ELF\n", pid, kept, total));
    out.push_str(&alloc::format!("Desplazamiento de carga (load_bias): 0x{:x}\n", base));
    if !present {
        out.push_str("Sin tabla de simbolos asociada (imagen sin .symtab o proceso no vinculado)\n");
        return out;
    }
    out.push_str("DIRECCION            TAMANO  FUNCION\n");
    if let Some(g) = SYMBOLS.try_lock() {
        let mut id = 0u32;
        for l in g.links.iter() {
            if l.0 == pid { id = l.1; break; }
        }
        for m in g.maps.iter() {
            if m.in_use && m.id == id {
                for s in m.syms[..m.kept].iter() {
                    out.push_str(&alloc::format!("0x{:016x} {:6}  {}\n", s.addr, s.size, s.name_str()));
                }
                break;
            }
        }
    }
    out
}
