//! [T-004] Descompresor XZ / LZMA2 en Rust puro (`no_std` + `alloc`), sin
//! dependencias. Lo necesita `pkg` para los `.deb` de Debian (`data.tar.xz`,
//! `control.tar.xz`). Implementa el contenedor `.xz` (cabecera, bloques con
//! filtro LZMA2 único, comprobación CRC32/CRC64/SHA-256 o ninguna, índice y
//! pie) y el decodificador LZMA completo (codificador de rango, estados,
//! literales, coincidencias, repeticiones y ranuras de distancia).
//!
//! Diseñado para flujo: `Xz::new(entrada)` y `leer(&mut buf)` devuelve datos
//! descomprimidos según se van produciendo; la ventana del diccionario es el
//! único búfer grande (tamaño fijado por el propio flujo, ≤ 64 MiB aceptado).

use alloc::vec::Vec;

// ───────────────────────── CRC ─────────────────────────
fn crc32(datos: &[u8], ini: u32) -> u32 {
    let mut c = !ini;
    for &b in datos {
        c ^= b as u32;
        for _ in 0..8 { c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 }; }
    }
    !c
}
fn crc64(datos: &[u8], ini: u64) -> u64 {
    let mut c = !ini;
    for &b in datos {
        c ^= b as u64;
        for _ in 0..8 { c = if c & 1 != 0 { (c >> 1) ^ 0xC96C_5795_D787_0F42 } else { c >> 1 }; }
    }
    !c
}

// ───────────────────────── Codificador de rango ─────────────────────────
const TOP: u32 = 1 << 24;
const BITS_MODELO: u32 = 11;
const BIT_MODELO_TOTAL: u16 = 1 << BITS_MODELO;
const MOVE_BITS: u32 = 5;

struct Rango<'a> {
    datos: &'a [u8],
    pos: usize,
    rango: u32,
    codigo: u32,
}

impl<'a> Rango<'a> {
    fn nuevo(datos: &'a [u8]) -> Result<Self, &'static str> {
        if datos.len() < 5 || datos[0] != 0 { return Err("xz: cabecera del codificador de rango"); }
        let codigo = u32::from_be_bytes([datos[1], datos[2], datos[3], datos[4]]);
        Ok(Rango { datos, pos: 5, rango: 0xFFFF_FFFF, codigo })
    }
    #[inline] fn byte(&mut self) -> u8 {
        let b = if self.pos < self.datos.len() { self.datos[self.pos] } else { 0 };
        self.pos += 1; b
    }
    #[inline] fn normalizar(&mut self) {
        if self.rango < TOP { self.rango <<= 8; self.codigo = (self.codigo << 8) | self.byte() as u32; }
    }
    #[inline] fn bit(&mut self, p: &mut u16) -> u32 {
        self.normalizar();
        let cota = (self.rango >> BITS_MODELO) * (*p as u32);
        if self.codigo < cota {
            self.rango = cota;
            *p += (BIT_MODELO_TOTAL - *p) >> MOVE_BITS;
            0
        } else {
            self.rango -= cota; self.codigo -= cota;
            *p -= *p >> MOVE_BITS;
            1
        }
    }
    #[inline] fn directo(&mut self, n: u32) -> u32 {
        let mut r = 0u32;
        for _ in 0..n {
            self.normalizar();
            self.rango >>= 1;
            self.codigo = self.codigo.wrapping_sub(self.rango);
            let t = 0u32.wrapping_sub(self.codigo >> 31); // 0xFFFFFFFF si hubo acarreo
            self.codigo = self.codigo.wrapping_add(self.rango & t);
            r = (r << 1).wrapping_add(t.wrapping_add(1));
        }
        r
    }
    fn arbol(&mut self, probs: &mut [u16], bits: u32) -> u32 {
        let mut m = 1u32;
        for _ in 0..bits { m = (m << 1) | self.bit(&mut probs[m as usize]); }
        m - (1 << bits)
    }
    fn arbol_inverso(&mut self, probs: &mut [u16], bits: u32) -> u32 {
        let mut m = 1u32; let mut r = 0u32;
        // índice m-1: el nodo 1 del árbol ocupa la posición 0 del segmento
        for i in 0..bits { let b = self.bit(&mut probs[m as usize - 1]); m = (m << 1) | b; r |= b << i; }
        r
    }
    fn terminado(&self) -> bool { self.codigo == 0 }
}

// ───────────────────────── Decodificador LZMA ─────────────────────────
const ESTADOS: usize = 12;
const POS_BITS_MAX: u32 = 4;
const LEN_LOW_BITS: u32 = 3; const LEN_MID_BITS: u32 = 3; const LEN_HIGH_BITS: u32 = 8;
const LEN_LOW: usize = 1 << LEN_LOW_BITS; const LEN_MID: usize = 1 << LEN_MID_BITS; const LEN_HIGH: usize = 1 << LEN_HIGH_BITS;
const DIST_ESTADOS: usize = 4; const DIST_SLOT_BITS: u32 = 6; const DIST_SLOTS: usize = 1 << DIST_SLOT_BITS;
const DIST_MODEL_START: u32 = 4; const DIST_MODEL_END: u32 = 14; const FULL_DISTANCES: usize = 1 << (DIST_MODEL_END / 2);
const ALIGN_BITS: u32 = 4; const ALIGN_SIZE: usize = 1 << ALIGN_BITS;
const MATCH_LEN_MIN: u32 = 2;

struct Longitud { eleccion: u16, eleccion2: u16, low: [[u16; LEN_LOW]; 1 << POS_BITS_MAX], mid: [[u16; LEN_MID]; 1 << POS_BITS_MAX], high: [u16; LEN_HIGH] }
impl Longitud {
    fn nuevo() -> Self { Longitud { eleccion: 1024, eleccion2: 1024, low: [[1024; LEN_LOW]; 16], mid: [[1024; LEN_MID]; 16], high: [1024; LEN_HIGH] } }
    fn reiniciar(&mut self) { *self = Self::nuevo(); }
    fn decodificar(&mut self, rc: &mut Rango, pos_state: usize) -> u32 {
        if rc.bit(&mut self.eleccion) == 0 { return MATCH_LEN_MIN + rc.arbol(&mut self.low[pos_state], LEN_LOW_BITS); }
        if rc.bit(&mut self.eleccion2) == 0 { return MATCH_LEN_MIN + LEN_LOW as u32 + rc.arbol(&mut self.mid[pos_state], LEN_MID_BITS); }
        MATCH_LEN_MIN + (LEN_LOW + LEN_MID) as u32 + rc.arbol(&mut self.high, LEN_HIGH_BITS)
    }
}

pub struct Lzma {
    // propiedades
    lc: u32, lp: u32, pb: u32,
    // estado
    estado: usize, rep: [u32; 4],
    is_match: [[u16; 1 << POS_BITS_MAX]; ESTADOS], is_rep: [u16; ESTADOS], is_rep0: [u16; ESTADOS], is_rep1: [u16; ESTADOS], is_rep2: [u16; ESTADOS],
    is_rep0_long: [[u16; 1 << POS_BITS_MAX]; ESTADOS],
    dist_slot: [[u16; DIST_SLOTS]; DIST_ESTADOS], dist_special: [u16; FULL_DISTANCES - DIST_MODEL_END as usize], dist_align: [u16; ALIGN_SIZE],
    len_match: Longitud, len_rep: Longitud,
    literal: Vec<u16>, // 0x300 << (lc+lp)
    // diccionario circular
    dic: Vec<u8>, dic_pos: usize, dic_lleno: usize,
    /// bytes producidos en el trozo actual (se entregan al terminar el trozo)
    salida: Vec<u8>,
    total: u64,
}

impl Lzma {
    fn nuevo(dic_tam: usize) -> Self {
        let mut l = Lzma {
            lc: 0, lp: 0, pb: 0, estado: 0, rep: [0; 4],
            is_match: [[1024; 16]; ESTADOS], is_rep: [1024; ESTADOS], is_rep0: [1024; ESTADOS], is_rep1: [1024; ESTADOS], is_rep2: [1024; ESTADOS],
            is_rep0_long: [[1024; 16]; ESTADOS],
            dist_slot: [[1024; DIST_SLOTS]; DIST_ESTADOS], dist_special: [1024; FULL_DISTANCES - DIST_MODEL_END as usize], dist_align: [1024; ALIGN_SIZE],
            len_match: Longitud::nuevo(), len_rep: Longitud::nuevo(), literal: Vec::new(),
            dic: Vec::new(), dic_pos: 0, dic_lleno: 0, salida: Vec::new(), total: 0,
        };
        l.dic.resize(dic_tam.max(4096), 0);
        l
    }
    fn fijar_propiedades(&mut self, p: u8) -> Result<(), &'static str> {
        if p >= 9 * 5 * 5 { return Err("xz: propiedades LZMA"); }
        let mut p = p as u32;
        self.lc = p % 9; p /= 9; self.lp = p % 5; self.pb = p / 5;
        if self.lc + self.lp > 4 { return Err("xz: lc+lp > 4"); }
        self.literal.clear();
        self.literal.resize(0x300usize << (self.lc + self.lp), 1024);
        Ok(())
    }
    fn reiniciar_estado(&mut self) {
        self.estado = 0; self.rep = [0; 4];
        self.is_match = [[1024; 16]; ESTADOS]; self.is_rep = [1024; ESTADOS]; self.is_rep0 = [1024; ESTADOS]; self.is_rep1 = [1024; ESTADOS]; self.is_rep2 = [1024; ESTADOS];
        self.is_rep0_long = [[1024; 16]; ESTADOS];
        self.dist_slot = [[1024; DIST_SLOTS]; DIST_ESTADOS]; self.dist_special = [1024; FULL_DISTANCES - DIST_MODEL_END as usize]; self.dist_align = [1024; ALIGN_SIZE];
        self.len_match.reiniciar(); self.len_rep.reiniciar();
        for p in self.literal.iter_mut() { *p = 1024; }
    }
    #[inline] fn dic_byte(&self, dist: usize) -> u8 {
        let mut i = self.dic_pos as isize - dist as isize - 1;
        if i < 0 { i += self.dic.len() as isize; }
        self.dic[i as usize]
    }
    #[inline] fn dic_push(&mut self, b: u8) {
        self.dic[self.dic_pos] = b;
        self.dic_pos += 1;
        if self.dic_pos == self.dic.len() { self.dic_pos = 0; }
        if self.dic_lleno < self.dic.len() { self.dic_lleno += 1; }
        self.salida.push(b);
        self.total += 1;
    }

    /// Decodifica `sin_comprimir` bytes del trozo actual.
    fn decodificar(&mut self, rc: &mut Rango, sin_comprimir: usize) -> Result<(), &'static str> {
        let fin = self.total + sin_comprimir as u64;
        let pb_mask = (1u32 << self.pb) - 1;
        let lp_mask = (1u32 << self.lp) - 1;
        while self.total < fin {
            let pos_state = (self.total as u32 & pb_mask) as usize;
            if rc.bit(&mut self.is_match[self.estado][pos_state]) == 0 {
                // literal
                let prev = if self.total > 0 { self.dic_byte(0) } else { 0 };
                let lit_state = (((self.total as u32 & lp_mask) << self.lc) + ((prev as u32) >> (8 - self.lc))) as usize;
                let base = 0x300 * lit_state;
                let match_byte0 = if self.estado >= 7 { self.dic_byte(self.rep[0] as usize) as u32 } else { 0 };
                let probs = &mut self.literal[base..base + 0x300];
                let mut sym = 1u32;
                if self.estado < 7 {
                    while sym < 0x100 { sym = (sym << 1) | rc.bit(&mut probs[sym as usize]); }
                } else {
                    let mut match_byte = match_byte0;
                    let mut offs = 0x100u32;
                    while sym < 0x100 {
                        match_byte <<= 1;
                        let match_bit = match_byte & offs;
                        let bit = rc.bit(&mut probs[(offs + match_bit + sym) as usize]);
                        sym = (sym << 1) | bit;
                        if bit != 0 { offs &= match_bit; } else { offs &= !match_bit; }
                    }
                }
                self.dic_push(sym as u8);
                self.estado = if self.estado < 4 { 0 } else if self.estado < 10 { self.estado - 3 } else { self.estado - 6 };
                continue;
            }
            let len;
            if rc.bit(&mut self.is_rep[self.estado]) != 0 {
                if self.dic_lleno == 0 { return Err("xz: repeticion sin historial"); }
                if rc.bit(&mut self.is_rep0[self.estado]) == 0 {
                    if rc.bit(&mut self.is_rep0_long[self.estado][pos_state]) == 0 {
                        // short rep: un byte a distancia rep0
                        self.estado = if self.estado < 7 { 9 } else { 11 };
                        let b = self.dic_byte(self.rep[0] as usize);
                        self.dic_push(b);
                        continue;
                    }
                } else {
                    let d;
                    if rc.bit(&mut self.is_rep1[self.estado]) == 0 { d = self.rep[1]; }
                    else {
                        if rc.bit(&mut self.is_rep2[self.estado]) == 0 { d = self.rep[2]; }
                        else { d = self.rep[3]; self.rep[3] = self.rep[2]; }
                        self.rep[2] = self.rep[1];
                    }
                    self.rep[1] = self.rep[0];
                    self.rep[0] = d;
                }
                len = self.len_rep.decodificar(rc, pos_state);
                self.estado = if self.estado < 7 { 8 } else { 11 };
            } else {
                // match nuevo
                self.rep[3] = self.rep[2]; self.rep[2] = self.rep[1]; self.rep[1] = self.rep[0];
                len = self.len_match.decodificar(rc, pos_state);
                self.estado = if self.estado < 7 { 7 } else { 10 };
                let len_state = ((len - MATCH_LEN_MIN).min(DIST_ESTADOS as u32 - 1)) as usize;
                let slot = rc.arbol(&mut self.dist_slot[len_state], DIST_SLOT_BITS);
                let dist;
                if slot < DIST_MODEL_START { dist = slot; }
                else {
                    let limite = (slot >> 1) - 1;
                    let mut d = (2 | (slot & 1)) << limite;
                    if slot < DIST_MODEL_END {
                        let base = (d - slot) as usize; // nodo 1 del árbol en dist_special[d - slot - 1 + 1]
                        d += rc.arbol_inverso(&mut self.dist_special[base..], limite);
                    } else {
                        d += rc.directo(limite - ALIGN_BITS) << ALIGN_BITS;
                        d += rc.arbol_inverso(&mut self.dist_align, ALIGN_BITS);
                    }
                    dist = d;
                }
                if dist == 0xFFFF_FFFF { return Err("xz: marcador de fin inesperado"); }
                self.rep[0] = dist;
                if dist as usize >= self.dic_lleno { return Err("xz: distancia fuera del diccionario"); }
            }
            let dist = self.rep[0] as usize;
            if dist >= self.dic_lleno { return Err("xz: distancia fuera del diccionario"); }
            for _ in 0..len { let b = self.dic_byte(dist); self.dic_push(b); }
        }
        Ok(())
    }
}

// ───────────────────────── LZMA2 + contenedor XZ ─────────────────────────
pub struct Xz<'a> {
    datos: &'a [u8],
    pos: usize,
    lzma: Option<Lzma>,
    fin: bool,
    tipo_check: u8,
    /// bytes del bloque actual ya decodificados pero aún no entregados
    pendiente: Vec<u8>,
    pend_pos: usize,
    /// bloque actual: inicio de datos comprimidos para contabilizar el padding
    bloque_ini: usize,
    en_bloque: bool,
    bloque_sin_comprimir: u64,
    crc32_acum: u32, crc64_acum: u64,
    sha: Option<crate::sha256_estado::Sha256>,
}

fn tam_dic(byte: u8) -> Result<usize, &'static str> {
    if byte > 40 { return Err("xz: tamaño de diccionario"); }
    if byte == 40 { return Ok(0xFFFF_FFFF); }
    let d = (2 | (byte & 1) as usize) << (byte / 2 + 11);
    Ok(d)
}

fn vli(datos: &[u8], pos: &mut usize) -> Result<u64, &'static str> {
    let mut v = 0u64; let mut i = 0;
    loop {
        if *pos >= datos.len() || i > 9 { return Err("xz: entero de longitud variable"); }
        let b = datos[*pos]; *pos += 1;
        v |= ((b & 0x7F) as u64) << (7 * i);
        i += 1;
        if b & 0x80 == 0 { return Ok(v); }
    }
}

impl<'a> Xz<'a> {
    pub fn new(datos: &'a [u8]) -> Result<Self, &'static str> {
        if datos.len() < 12 + 12 || &datos[0..6] != b"\xFD7zXZ\x00" { return Err("xz: firma de cabecera"); }
        let flags = [datos[6], datos[7]];
        if flags[0] != 0 || flags[1] & 0xF0 != 0 { return Err("xz: banderas de flujo"); }
        let crc = u32::from_le_bytes([datos[8], datos[9], datos[10], datos[11]]);
        if crc32(&flags, 0) != crc { return Err("xz: CRC32 de la cabecera del flujo"); }
        Ok(Xz { datos, pos: 12, lzma: None, fin: false, tipo_check: flags[1] & 0x0F, pendiente: Vec::new(), pend_pos: 0,
                bloque_ini: 0, en_bloque: false, bloque_sin_comprimir: 0, crc32_acum: 0, crc64_acum: 0, sha: None })
    }

    fn abrir_bloque(&mut self) -> Result<bool, &'static str> {
        let d = self.datos;
        if self.pos >= d.len() { return Err("xz: fin prematuro"); }
        if d[self.pos] == 0 { // índice: fin de los bloques
            self.fin = true;
            return Ok(false);
        }
        let tam_cab = (d[self.pos] as usize + 1) * 4;
        if self.pos + tam_cab > d.len() { return Err("xz: cabecera de bloque truncada"); }
        let cab = &d[self.pos..self.pos + tam_cab];
        let crc = u32::from_le_bytes([cab[tam_cab - 4], cab[tam_cab - 3], cab[tam_cab - 2], cab[tam_cab - 1]]);
        if crc32(&cab[..tam_cab - 4], 0) != crc { return Err("xz: CRC32 de la cabecera de bloque"); }
        let bflags = cab[1];
        let nfiltros = (bflags & 3) as usize + 1;
        if bflags & 0x3C != 0 { return Err("xz: banderas de bloque reservadas"); }
        let mut p = 2usize;
        if bflags & 0x40 != 0 { let _ = vli(cab, &mut p)?; }
        if bflags & 0x80 != 0 { let _ = vli(cab, &mut p)?; }
        if nfiltros != 1 { return Err("xz: solo se admite el filtro LZMA2 unico"); }
        let id = vli(cab, &mut p)?;
        if id != 0x21 { return Err("xz: filtro distinto de LZMA2 (BCJ/delta no soportados)"); }
        let ptam = vli(cab, &mut p)?;
        if ptam != 1 || p >= cab.len() { return Err("xz: propiedades LZMA2"); }
        let dic = tam_dic(cab[p])?;
        if dic > 64 * 1024 * 1024 { return Err("xz: diccionario > 64 MiB"); }
        self.pos += tam_cab;
        self.bloque_ini = self.pos;
        self.lzma = Some(Lzma::nuevo(dic));
        self.en_bloque = true;
        self.bloque_sin_comprimir = 0;
        self.crc32_acum = 0; self.crc64_acum = 0;
        self.sha = if self.tipo_check == 0x0A { Some(crate::sha256_estado::Sha256::nuevo()) } else { None };
        Ok(true)
    }

    fn cerrar_bloque(&mut self) -> Result<(), &'static str> {
        // relleno del bloque a múltiplo de 4 + campo de comprobación
        let comprimido = self.pos - self.bloque_ini;
        let pad = (4 - (comprimido % 4)) % 4;
        for _ in 0..pad { if self.pos >= self.datos.len() || self.datos[self.pos] != 0 { return Err("xz: relleno de bloque"); } self.pos += 1; }
        let d = self.datos;
        match self.tipo_check {
            0 => {}
            1 => { if self.pos + 4 > d.len() { return Err("xz: CRC32 truncado"); }
                   let v = u32::from_le_bytes([d[self.pos], d[self.pos+1], d[self.pos+2], d[self.pos+3]]);
                   if v != self.crc32_acum { return Err("xz: CRC32 de los datos no coincide"); } self.pos += 4; }
            4 => { if self.pos + 8 > d.len() { return Err("xz: CRC64 truncado"); }
                   let mut b = [0u8; 8]; b.copy_from_slice(&d[self.pos..self.pos+8]);
                   if u64::from_le_bytes(b) != self.crc64_acum { return Err("xz: CRC64 de los datos no coincide"); } self.pos += 8; }
            0x0A => { if self.pos + 32 > d.len() { return Err("xz: SHA-256 truncado"); }
                   let h = self.sha.take().map(|s| s.terminar()).unwrap_or([0u8; 32]);
                   if h[..] != d[self.pos..self.pos+32] { return Err("xz: SHA-256 de los datos no coincide"); } self.pos += 32; }
            _ => return Err("xz: tipo de comprobacion no soportado"),
        }
        self.en_bloque = false;
        self.lzma = None;
        Ok(())
    }

    /// Decodifica el siguiente trozo LZMA2 del bloque actual a `pendiente`.
    /// Devuelve false si el bloque terminó (0x00).
    fn trozo(&mut self) -> Result<bool, &'static str> {
        let d = self.datos;
        if self.pos >= d.len() { return Err("xz: trozo LZMA2 truncado"); }
        let ctrl = d[self.pos]; self.pos += 1;
        if ctrl == 0 { return Ok(false); }
        let lz = self.lzma.as_mut().ok_or("xz: sin bloque")?;
        lz.salida.clear();
        if ctrl & 0x80 == 0 {
            // trozo sin comprimir: 1 = reinicio de diccionario, 2 = sin reinicio
            if ctrl > 2 { return Err("xz: control LZMA2"); }
            if self.pos + 2 > d.len() { return Err("xz: trozo truncado"); }
            let n = ((d[self.pos] as usize) << 8 | d[self.pos+1] as usize) + 1; self.pos += 2;
            if ctrl == 1 { lz.dic_pos = 0; lz.dic_lleno = 0; }
            if self.pos + n > d.len() { return Err("xz: datos sin comprimir truncados"); }
            for i in 0..n { lz.dic_push(d[self.pos + i]); }
            self.pos += n;
        } else {
            if self.pos + 4 > d.len() { return Err("xz: trozo truncado"); }
            let sin_comp = (((ctrl & 0x1F) as usize) << 16 | (d[self.pos] as usize) << 8 | d[self.pos+1] as usize) + 1;
            let comp = ((d[self.pos+2] as usize) << 8 | d[self.pos+3] as usize) + 1;
            self.pos += 4;
            let reinicio = (ctrl >> 5) & 3;
            if reinicio >= 2 {
                if self.pos >= d.len() { return Err("xz: propiedades truncadas"); }
                lz.fijar_propiedades(d[self.pos])?; self.pos += 1;
            }
            if reinicio == 3 { lz.dic_pos = 0; lz.dic_lleno = 0; }
            if reinicio >= 1 { lz.reiniciar_estado(); }
            if self.pos + comp > d.len() { return Err("xz: datos comprimidos truncados"); }
            let mut rc = Rango::nuevo(&d[self.pos..self.pos + comp])?;
            lz.decodificar(&mut rc, sin_comp)?;
            rc.normalizar(); // el codificador queda normalizado al cerrar el trozo (como xz-embedded)
            if !rc.terminado() || rc.pos != comp { return Err("xz: el codificador de rango no termino limpio"); }
            self.pos += comp;
        }
        // entregar lo producido en este trozo
        let producido = lz.salida.len();
        self.pendiente = core::mem::take(&mut lz.salida);
        self.pend_pos = 0;
        self.bloque_sin_comprimir += producido as u64;
        match self.tipo_check {
            1 => self.crc32_acum = crc32(&self.pendiente, self.crc32_acum),
            4 => self.crc64_acum = crc64(&self.pendiente, self.crc64_acum),
            0x0A => { if let Some(s) = self.sha.as_mut() { s.actualizar(&self.pendiente); } }
            _ => {}
        }
        Ok(true)
    }

    /// Lee hasta `buf.len()` bytes descomprimidos. 0 = fin del flujo.
    pub fn leer(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> {
        let mut n = 0usize;
        while n < buf.len() {
            if self.pend_pos < self.pendiente.len() {
                let k = (self.pendiente.len() - self.pend_pos).min(buf.len() - n);
                buf[n..n + k].copy_from_slice(&self.pendiente[self.pend_pos..self.pend_pos + k]);
                self.pend_pos += k; n += k;
                continue;
            }
            if self.fin { break; }
            if !self.en_bloque {
                if !self.abrir_bloque()? { break; }
            }
            if !self.trozo()? { self.cerrar_bloque()?; }
        }
        Ok(n)
    }

    /// Descomprime todo el flujo (para entradas pequeñas: control.tar.xz).
    pub fn todo(datos: &[u8]) -> Result<Vec<u8>, &'static str> {
        let mut x = Xz::new(datos)?;
        let mut salida = Vec::new();
        let mut tmp = alloc::vec![0u8; 65536]; // en el montículo: la pila inicial es de 64 KiB
        loop { let n = x.leer(&mut tmp)?; if n == 0 { break; } salida.extend_from_slice(&tmp[..n]); }
        Ok(salida)
    }
}
