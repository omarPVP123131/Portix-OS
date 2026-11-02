//! [T-002] Lector de archivos tar (POSIX ustar + extensiones PAX/GNU que usa
//! `abuild`): devuelve las entradas en orden con su contenido. Las cabeceras
//! PAX (`x`, `g`) sólo se leen para obtener `path`/`linkpath` largos; el resto
//! de sus claves (p. ej. `APK-TOOLS.checksum.SHA1`) se ignoran.

use alloc::string::String;
use alloc::vec::Vec;

pub struct Entrada<'a> {
    pub nombre: String,
    pub tipo: u8,      // b'0'/0 regular, b'5' dir, b'2' symlink, b'1' hardlink
    pub modo: u32,
    pub enlace: String,
    pub datos: &'a [u8],
}

fn octal(campo: &[u8]) -> u64 {
    let mut v = 0u64;
    for &c in campo { if c >= b'0' && c <= b'7' { v = v * 8 + (c - b'0') as u64; } else if c == 0 || c == b' ' { if v != 0 { break; } } else { break; } }
    v
}

fn texto(campo: &[u8]) -> String {
    let fin = campo.iter().position(|&c| c == 0).unwrap_or(campo.len());
    String::from_utf8_lossy(&campo[..fin]).into_owned()
}

/// Claves `path`/`linkpath` de un registro PAX.
fn pax(datos: &[u8]) -> (Option<String>, Option<String>) {
    let mut path = None; let mut link = None;
    let mut p = 0;
    while p < datos.len() {
        let mut q = p; let mut len = 0usize;
        while q < datos.len() && datos[q] != b' ' { if datos[q] < b'0' || datos[q] > b'9' { return (path, link); } len = len * 10 + (datos[q] - b'0') as usize; q += 1; }
        if len == 0 || p + len > datos.len() { break; }
        let reg = &datos[q + 1..p + len];
        let fin = if reg.last() == Some(&b'\n') { reg.len() - 1 } else { reg.len() };
        if let Some(eq) = reg[..fin].iter().position(|&c| c == b'=') {
            let (k, v) = (&reg[..eq], &reg[eq + 1..fin]);
            if k == b"path" { path = Some(String::from_utf8_lossy(v).into_owned()); }
            if k == b"linkpath" { link = Some(String::from_utf8_lossy(v).into_owned()); }
        }
        p += len;
    }
    (path, link)
}

pub fn entradas(d: &[u8]) -> Result<Vec<Entrada<'_>>, &'static str> {
    let mut out = Vec::new();
    let mut p = 0usize;
    let mut pax_path: Option<String> = None;
    let mut pax_link: Option<String> = None;
    let mut gnu_long: Option<String> = None;
    while p + 512 <= d.len() {
        let h = &d[p..p + 512];
        if h.iter().all(|&c| c == 0) { break; } // bloque de fin
        let suma_dicha = octal(&h[148..156]);
        let mut suma = 0u64;
        for (i, &c) in h.iter().enumerate() { suma += if (148..156).contains(&i) { 32 } else { c as u64 }; }
        if suma != suma_dicha { return Err("suma de control de cabecera tar invalida"); }
        let tam = octal(&h[124..136]) as usize;
        let tipo = h[156];
        let bloques = (tam + 511) / 512;
        if p + 512 + tam > d.len() { return Err("entrada tar truncada"); }
        let datos = &d[p + 512..p + 512 + tam];
        match tipo {
            b'x' => { let (a, b) = pax(datos); pax_path = a; pax_link = b; }
            b'g' => {}
            b'L' => { gnu_long = Some(texto(datos)); }
            b'K' => { pax_link = Some(texto(datos)); }
            _ => {
                let mut nombre = texto(&h[0..100]);
                if &h[257..262] == b"ustar" { let pre = texto(&h[345..500]); if !pre.is_empty() { nombre = alloc::format!("{}/{}", pre, nombre); } }
                if let Some(n) = pax_path.take() { nombre = n; }
                if let Some(n) = gnu_long.take() { nombre = n; }
                let mut enlace = texto(&h[157..257]);
                if let Some(l) = pax_link.take() { enlace = l; }
                out.push(Entrada { nombre, tipo, modo: octal(&h[100..108]) as u32, enlace, datos });
            }
        }
        p += 512 + bloques * 512;
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// [T-003] Lector tar EN FLUJO: se le entregan trozos arbitrarios (los que
// produce el inflador) y va llamando al manejador por entrada, sin tener el
// tar entero en memoria. Los registros PAX/GNU largos (pequeños) se retienen.
// ─────────────────────────────────────────────────────────────────────────────

pub struct Cabecera { pub nombre: String, pub tipo: u8, pub modo: u32, pub enlace: String, pub tam: u64 }

pub trait Manejador {
    /// Nueva entrada. Devuelve `true` si quiere recibir sus datos.
    fn entrada(&mut self, c: &Cabecera) -> Result<bool, &'static str>;
    fn datos(&mut self, d: &[u8]) -> Result<(), &'static str>;
    fn fin_entrada(&mut self, c: &Cabecera) -> Result<(), &'static str>;
}

pub struct TarFlujo<'m> {
    m: &'m mut dyn Manejador,
    cab: Vec<u8>,            // cabecera parcial (hasta 512)
    actual: Option<(Cabecera, bool)>,
    restante: u64,           // bytes de datos pendientes de la entrada actual
    relleno: usize,          // bytes de relleno a 512 pendientes
    meta: Vec<u8>,           // acumulador de PAX 'x' / GNU 'L' / 'K'
    meta_tipo: u8,
    pax_path: Option<String>, pax_link: Option<String>, gnu_long: Option<String>,
    ceros: u32,
    pub entradas: usize,
}

impl<'m> TarFlujo<'m> {
    pub fn new(m: &'m mut dyn Manejador) -> Self {
        TarFlujo { m, cab: Vec::with_capacity(512), actual: None, restante: 0, relleno: 0, meta: Vec::new(), meta_tipo: 0,
                   pax_path: None, pax_link: None, gnu_long: None, ceros: 0, entradas: 0 }
    }

    fn abrir(&mut self) -> Result<(), &'static str> {
        let h: Vec<u8> = core::mem::take(&mut self.cab);
        if h.iter().all(|&c| c == 0) { self.ceros += 1; return Ok(()); }
        let suma_dicha = octal(&h[148..156]);
        let mut suma = 0u64;
        for (i, &c) in h.iter().enumerate() { suma += if (148..156).contains(&i) { 32 } else { c as u64 }; }
        if suma != suma_dicha { return Err("suma de control de cabecera tar invalida"); }
        let tam = octal(&h[124..136]);
        let tipo = h[156];
        self.restante = tam;
        self.relleno = ((512 - (tam % 512)) % 512) as usize;
        match tipo {
            b'x' | b'g' | b'L' | b'K' => { self.meta.clear(); self.meta_tipo = tipo; self.actual = None; }
            _ => {
                let mut nombre = texto(&h[0..100]);
                if &h[257..262] == b"ustar" { let pre = texto(&h[345..500]); if !pre.is_empty() { nombre = alloc::format!("{}/{}", pre, nombre); } }
                if let Some(n) = self.pax_path.take() { nombre = n; }
                if let Some(n) = self.gnu_long.take() { nombre = n; }
                let mut enlace = texto(&h[157..257]);
                if let Some(l) = self.pax_link.take() { enlace = l; }
                let c = Cabecera { nombre, tipo, modo: octal(&h[100..108]) as u32, enlace, tam };
                let quiere = self.m.entrada(&c)?;
                self.entradas += 1;
                if tam == 0 { self.m.fin_entrada(&c)?; self.actual = None; } else { self.actual = Some((c, quiere)); }
            }
        }
        Ok(())
    }

    fn cerrar_meta(&mut self) {
        match self.meta_tipo {
            b'x' => { let (a, b) = pax(&self.meta); if a.is_some() { self.pax_path = a; } if b.is_some() { self.pax_link = b; } }
            b'L' => { self.gnu_long = Some(texto(&self.meta)); }
            b'K' => { self.pax_link = Some(texto(&self.meta)); }
            _ => {}
        }
        self.meta_tipo = 0; self.meta.clear();
    }

    pub fn alimentar(&mut self, mut d: &[u8]) -> Result<(), &'static str> {
        while !d.is_empty() {
            if self.restante > 0 {
                let n = (self.restante as usize).min(d.len());
                if self.meta_tipo != 0 { if self.meta.len() + n <= 1 << 20 { self.meta.extend_from_slice(&d[..n]); } }
                else if let Some((_, true)) = &self.actual { self.m.datos(&d[..n])?; }
                self.restante -= n as u64; d = &d[n..];
                if self.restante == 0 {
                    if self.meta_tipo != 0 { self.cerrar_meta(); }
                    else if let Some((c, _)) = self.actual.take() { self.m.fin_entrada(&c)?; }
                }
                continue;
            }
            if self.relleno > 0 { let n = self.relleno.min(d.len()); self.relleno -= n; d = &d[n..]; continue; }
            let falta = 512 - self.cab.len();
            let n = falta.min(d.len());
            self.cab.extend_from_slice(&d[..n]); d = &d[n..];
            if self.cab.len() == 512 { self.abrir()?; }
        }
        Ok(())
    }

    pub fn terminar(&mut self) -> Result<(), &'static str> {
        if self.restante > 0 || self.actual.is_some() { return Err("tar truncado a mitad de una entrada"); }
        if !self.cab.is_empty() && !self.cab.iter().all(|&c| c == 0) { return Err("tar truncado en una cabecera"); }
        Ok(())
    }
}
