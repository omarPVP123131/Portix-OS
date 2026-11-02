// userland/bin/dbg/src/disasm.rs — Desensamblador simbólico x86-64 (ciclo v6.x)
//
// Decodificador acotado de la codificación x86-64 que cubre el repertorio que
// emite un compilador para código de usuario: movimientos, aritmética, saltos,
// llamadas, `syscall`, `int3`, SSE básico y `nop` multibyte. Las instrucciones
// no reconocidas se muestran como `db 0xNN` (1 byte), de modo que el recorrido
// nunca se desincroniza silenciosamente.
//
// El objetivo no es ser un desensamblador completo, sino permitir que el
// depurador **nombre** lo que está mirando: cada destino de salto/llamada se
// resuelve contra la tabla de símbolos del proceso.

use alloc::format;
use alloc::string::String;

const REG64: [&str; 16] = ["rax","rcx","rdx","rbx","rsp","rbp","rsi","rdi",
                           "r8","r9","r10","r11","r12","r13","r14","r15"];
const REG32: [&str; 16] = ["eax","ecx","edx","ebx","esp","ebp","esi","edi",
                           "r8d","r9d","r10d","r11d","r12d","r13d","r14d","r15d"];
const REG16: [&str; 16] = ["ax","cx","dx","bx","sp","bp","si","di",
                           "r8w","r9w","r10w","r11w","r12w","r13w","r14w","r15w"];
const REG8:  [&str; 16] = ["al","cl","dl","bl","spl","bpl","sil","dil",
                           "r8b","r9b","r10b","r11b","r12b","r13b","r14b","r15b"];

#[derive(Clone, Copy, PartialEq)]
pub enum Tam { B8, B16, B32, B64 }

fn reg(i: u8, t: Tam) -> &'static str {
    let i = (i & 15) as usize;
    match t {
        Tam::B8 => REG8[i],
        Tam::B16 => REG16[i],
        Tam::B32 => REG32[i],
        Tam::B64 => REG64[i],
    }
}

/// Resultado de decodificar una instrucción.
pub struct Instruccion {
    pub len: usize,
    pub texto: String,
    /// Dirección destino de un salto o llamada (para simbolizar)
    pub destino: Option<u64>,
    /// Desplazamiento RIP-relativo de un operando de memoria (para simbolizar)
    pub rip_rel: Option<i64>,
}

fn rd_signed(b: &[u8], i: usize, n: usize) -> i64 {
    let mut v: i64 = 0;
    if n == 1 {
        v = b[i] as i8 as i64;
    } else if n == 4 {
        let mut x = [0u8; 4];
        x.copy_from_slice(&b[i..i + 4]);
        v = i32::from_le_bytes(x) as i64;
    }
    v
}

fn rd_unsigned(b: &[u8], i: usize, n: usize) -> u64 {
    let mut v: u64 = 0;
    for k in (0..n).rev() {
        v = (v << 8) | b[i + k] as u64;
    }
    v
}

struct ModRm {
    fin: usize,
    reg: u8,
    /// Texto del operando r/m
    rm: String,
    rip_rel: Option<i64>,
}

/// Decodifica ModRM + SIB + desplazamiento.
fn modrm(b: &[u8], ini: usize, rex: u8, t: Tam) -> Option<ModRm> {
    if ini >= b.len() { return None; }
    let byte = b[ini];
    let mut i = ini + 1;
    let modo = byte >> 6;
    let reg_n = ((byte >> 3) & 7) | ((rex & 4) << 1); // REX.R
    let rm_low = byte & 7;
    let ext_b = (rex & 1) != 0;                     // REX.B
    let ext_x = (rex & 2) != 0;                     // REX.X

    if modo == 3 {
        let r = rm_low | if ext_b { 8 } else { 0 };
        return Some(ModRm { fin: i, reg: reg_n, rm: String::from(reg(r, t)), rip_rel: None });
    }

    let mut texto = String::from("[");
    let mut rip_rel = None;
    let (base_reg, index_reg, escala, mut disp, mut hay_disp) = (None::<u8>, None::<u8>, 1u8, 0i64, false);

    let base = rm_low | if ext_b { 8 } else { 0 };
    if rm_low == 4 {
        // SIB
        if i >= b.len() { return None; }
        let sib = b[i];
        i += 1;
        let sc = sib >> 6;
        let idx = ((sib >> 3) & 7) | ((rex & 2) << 2);
        let bs = (sib & 7) | if ext_b { 8 } else { 0 };
        let escala_n = 1u8 << sc;
        let index_ok = (sib >> 3) & 7 != 4 || ext_x;
        texto.push_str("");
        let mut partes = String::new();
        if bs & 7 == 5 && modo == 0 {
            // disp32 puro sin base
            if i + 4 > b.len() { return None; }
            disp = rd_signed(b, i, 4);
            i += 4;
            hay_disp = true;
            partes.push_str(&format!("0x{:x}", disp as u64));
        } else {
            partes.push_str(reg(bs, Tam::B64));
        }
        if index_ok {
            partes.push_str(&format!(" + {}*{}", reg(idx, Tam::B64), escala_n));
        }
        texto.push_str(&partes);
        let _ = (base_reg, index_reg, escala);
    } else if rm_low == 5 && modo == 0 {
        // RIP-relativo
        if i + 4 > b.len() { return None; }
        let d = rd_signed(b, i, 4);
        i += 4;
        rip_rel = Some(d);
        texto.push_str(&format!("rip{}{:#x}", if d < 0 { "-" } else { "+" }, d.unsigned_abs()));
        return Some(ModRm { fin: i, reg: reg_n, rm: format!("{}{}", "[", String::from(texto.trim_start_matches('['))), rip_rel });
    } else {
        texto.push_str(reg(base, Tam::B64));
    }

    if modo == 1 {
        if i >= b.len() { return None; }
        disp = rd_signed(b, i, 1);
        i += 1;
        hay_disp = true;
    } else if modo == 2 {
        if i + 4 > b.len() { return None; }
        disp = rd_signed(b, i, 4);
        i += 4;
        hay_disp = true;
    }
    if hay_disp && disp != 0 {
        if texto.len() > 1 {
            texto.push_str(&format!("{}{:#x}", if disp < 0 { "-" } else { "+" }, disp.unsigned_abs()));
        } else {
            texto.push_str(&format!("0x{:x}", disp.unsigned_abs()));
        }
    }
    texto.push(']');
    Some(ModRm { fin: i, reg: reg_n, rm: texto, rip_rel })
}

/// Decodifica una instrucción completa a partir de `b` (se asume que empieza en
/// el límite de instrucción). `base` es la dirección virtual de `b[0]`.
pub fn decodificar(b: &[u8], base: u64) -> Instruccion {
    let mut i = 0usize;
    let mut rex = 0u8;
    let mut t = Tam::B32;
    // Prefijos
    loop {
        if i >= b.len() { return db(b); }
        match b[i] {
            0x66 => { t = Tam::B16; i += 1; }
            0x67 | 0x2E | 0x36 | 0x3E | 0x26 | 0x64 | 0x65 | 0xF0 | 0xF2 | 0xF3 => { i += 1; }
            0x40..=0x4F => { rex = b[i]; i += 1; if rex & 8 != 0 { t = Tam::B64; } }
            _ => break,
        }
    }
    if i >= b.len() { return db(b); }
    let op = b[i];

    macro_rules! inst {
        ($len:expr, $txt:expr) => {
            return Instruccion { len: $len, texto: String::from($txt), destino: None, rip_rel: None }
        };
    }

    // ── Aritmética base (ADD/OR/ADC/SBB/AND/SUB/XOR/CMP) ───────────────────
    if op < 0x40 && (op & 7) < 6 {
        let nombre = ["add","or","adc","sbb","and","sub","xor","cmp"][(op >> 3) as usize];
        let forma = op & 7;
        let mut j = i + 1;
        if forma == 0 || forma == 2 || forma == 3 {
            // op r/m8, r8 | op r8, r/m8
            let tt = if forma == 0 { Tam::B8 } else { t };
            let m = match modrm(b, j, rex, tt) { Some(m) => m, None => return db(b) };
            j = m.fin;
            let (a, c) = if forma == 0 || forma == 2 { (m.rm.clone(), String::from(reg(m.reg, tt))) } else { (String::from(reg(m.reg, tt)), m.rm.clone()) };
            return Instruccion { len: j, texto: format!("{} {}, {}", nombre, a, c), destino: None, rip_rel: m.rip_rel };
        } else if forma == 4 {
            if j >= b.len() { return db(b); }
            let v = b[j] as u8; j += 1;
            return Instruccion { len: j, texto: format!("{} al, {:#x}", nombre, v), destino: None, rip_rel: None };
        } else {
            let tt = if forma == 5 { if t == Tam::B64 { Tam::B64 } else { Tam::B32 } } else { t };
            let n = if tt == Tam::B64 { 4 } else { 2.min(4) };
            if j + n > b.len() { return db(b); }
            let v = rd_unsigned(b, j, n);
            j += n;
            let acc = if tt == Tam::B64 { reg(0, Tam::B64) } else { reg(0, t) };
            return Instruccion { len: j, texto: format!("{} {}, {:#x}", nombre, acc, v), destino: None, rip_rel: None };
        }
    }

    match op {
        0x50..=0x57 => inst!(i + 1, format!("push {}", reg((op - 0x50) | ((rex & 1) << 3), Tam::B64))),
        0x58..=0x5F => inst!(i + 1, format!("pop {}", reg((op - 0x58) | ((rex & 1) << 3), Tam::B64))),
        0x63 => { // movsxd
            let m = match modrm(b, i + 1, rex, Tam::B32) { Some(m) => m, None => return db(b) };
            inst!(m.fin, format!("movsxd {}, {}", reg(m.reg, Tam::B64), m.rm))
        }
        0x68 => { if i + 5 > b.len() { return db(b); } inst!(i + 5, format!("push {:#x}", rd_unsigned(b, i + 1, 4))) }
        0x69 => {
            let m = match modrm(b, i + 1, rex, t) { Some(m) => m, None => return db(b) };
            if m.fin + 4 > b.len() { return db(b); }
            inst!(m.fin + 4, format!("imul {}, {}, {:#x}", reg(m.reg, t), m.rm, rd_unsigned(b, m.fin, 4)))
        }
        0x6A => { if i + 2 > b.len() { return db(b); } inst!(i + 2, format!("push {:#x}", b[i + 1] as i8)) }
        0x6B => {
            let m = match modrm(b, i + 1, rex, t) { Some(m) => m, None => return db(b) };
            if m.fin + 1 > b.len() { return db(b); }
            inst!(m.fin + 1, format!("imul {}, {}, {:#x}", reg(m.reg, t), m.rm, b[m.fin] as i8))
        }
        0x70..=0x7F => {
            if i + 2 > b.len() { return db(b); }
            let rel = b[i + 1] as i8 as i64;
            let fin = i + 2;
            let destino = (base as i64 + fin as i64 + rel) as u64;
            return Instruccion { len: fin, texto: format!("{} {:#x}", jcc(op - 0x70), destino), destino: Some(destino), rip_rel: None };
        }
        0x80 | 0x81 | 0x83 => {
            let tt = if op == 0x80 { Tam::B8 } else { t };
            let m = match modrm(b, i + 1, rex, tt) { Some(m) => m, None => return db(b) };
            let (n, v) = if op == 0x83 { (1usize, b.get(m.fin).map(|x| *x as i8 as i64).unwrap_or(0)) } else if op == 0x80 { (1usize, m.fin.checked_add(0).and_then(|k| b.get(k)).map(|x| *x as i8 as i64).unwrap_or(0)) } else { (4usize, rd_signed(b, m.fin, 4)) };
            if m.fin + n > b.len() { return db(b); }
            let nombre = ["add","or","adc","sbb","and","sub","xor","cmp"][(m.reg & 7) as usize];
            inst!(m.fin + n, format!("{} {}, {}", nombre, m.rm, v))
        }
        0x84 | 0x85 => {
            let tt = if op == 0x84 { Tam::B8 } else { t };
            let m = match modrm(b, i + 1, rex, tt) { Some(m) => m, None => return db(b) };
            inst!(m.fin, format!("test {}, {}", m.rm, reg(m.reg, tt)))
        }
        0x88 | 0x89 | 0x8A | 0x8B => {
            let tt = if op == 0x88 || op == 0x8A { Tam::B8 } else { t };
            let m = match modrm(b, i + 1, rex, tt) { Some(m) => m, None => return db(b) };
            let txt = if op == 0x88 || op == 0x89 { format!("mov {}, {}", m.rm, reg(m.reg, tt)) } else { format!("mov {}, {}", reg(m.reg, tt), m.rm) };
            return Instruccion { len: m.fin, texto: txt, destino: None, rip_rel: m.rip_rel };
        }
        0x8D => {
            let m = match modrm(b, i + 1, rex, t) { Some(m) => m, None => return db(b) };
            return Instruccion { len: m.fin, texto: format!("lea {}, {}", reg(m.reg, t), m.rm), destino: None, rip_rel: m.rip_rel };
        }
        0x8F => {
            let m = match modrm(b, i + 1, rex, Tam::B64) { Some(m) => m, None => return db(b) };
            inst!(m.fin, format!("pop {}", m.rm))
        }
        0x90 => inst!(i + 1, String::from("nop")),
        0x98 => inst!(i + 1, String::from(if t == Tam::B64 { "cdqe" } else { "cwde" })),
        0x99 => inst!(i + 1, String::from(if t == Tam::B64 { "cqo" } else { "cdq" })),
        0x9C => inst!(i + 1, String::from("pushfq")),
        0x9D => inst!(i + 1, String::from("popfq")),
        0xA8 => { if i + 2 > b.len() { return db(b); } inst!(i + 2, format!("test al, {:#x}", b[i + 1])) }
        0xA9 => { if i + 5 > b.len() { return db(b); } inst!(i + 5, format!("test {}, {:#x}", reg(0, if t == Tam::B64 { Tam::B64 } else { Tam::B32 }), rd_unsigned(b, i + 1, 4))) }
        0xB0..=0xB7 => { if i + 2 > b.len() { return db(b); } inst!(i + 2, format!("mov {}, {:#x}", reg((op - 0xB0) | ((rex & 1) << 3), Tam::B8), b[i + 1])) }
        0xB8..=0xBF => {
            let r = (op - 0xB8) | ((rex & 1) << 3);
            let n = if t == Tam::B64 { 8 } else { 4 };
            if i + 1 + n > b.len() { return db(b); }
            inst!(i + 1 + n, format!("mov {}, {:#x}", reg(r, t), rd_unsigned(b, i + 1, n)))
        }
        0xC3 => inst!(i + 1, String::from("ret")),
        0xC6 => {
            let m = match modrm(b, i + 1, rex, Tam::B8) { Some(m) => m, None => return db(b) };
            if m.fin + 1 > b.len() { return db(b); }
            inst!(m.fin + 1, format!("mov {}, {:#x}", m.rm, b[m.fin]))
        }
        0xC7 => {
            let m = match modrm(b, i + 1, rex, t) { Some(m) => m, None => return db(b) };
            if m.fin + 4 > b.len() { return db(b); }
            inst!(m.fin + 4, format!("mov {}, {:#x}", m.rm, rd_unsigned(b, m.fin, 4)))
        }
        0xC9 => inst!(i + 1, String::from("leave")),
        0xCC => inst!(i + 1, String::from("int3")),
        0xCD => { if i + 2 > b.len() { return db(b); } inst!(i + 2, format!("int {:#x}", b[i + 1])) }
        0xD0..=0xD3 => {
            let m = match modrm(b, i + 1, rex, t) { Some(m) => m, None => return db(b) };
            let nombre = ["rol","ror","rcl","rcr","shl","shr","sal","sar"][(m.reg & 7) as usize];
            inst!(m.fin, format!("{} {}, {}", nombre, m.rm, if op == 0xD0 || op == 0xD2 { "1" } else { "cl" }))
        }
        0xE8 | 0xE9 => {
            if i + 5 > b.len() { return db(b); }
            let rel = rd_signed(b, i + 1, 4);
            let fin = i + 5;
            let destino = (base as i64 + fin as i64 + rel) as u64;
            let txt = if op == 0xE8 { format!("call {:#x}", destino) } else { format!("jmp {:#x}", destino) };
            return Instruccion { len: fin, texto: txt, destino: Some(destino), rip_rel: None };
        }
        0xEB => {
            if i + 2 > b.len() { return db(b); }
            let rel = b[i + 1] as i8 as i64;
            let fin = i + 2;
            let destino = (base as i64 + fin as i64 + rel) as u64;
            return Instruccion { len: fin, texto: format!("jmp {:#x}", destino), destino: Some(destino), rip_rel: None };
        }
        0xF4 => inst!(i + 1, String::from("hlt")),
        0xF6 | 0xF7 => {
            let tt = if op == 0xF6 { Tam::B8 } else { t };
            let m = match modrm(b, i + 1, rex, tt) { Some(m) => m, None => return db(b) };
            let nombre = ["test","test","not","neg","mul","imul","div","idiv"][(m.reg & 7) as usize];
            let mut fin = m.fin;
            let mut extra = String::new();
            if m.reg & 7 <= 1 {
                let n = if tt == Tam::B8 { 1 } else { 4 };
                if fin + n > b.len() { return db(b); }
                extra = format!(", {:#x}", rd_unsigned(b, fin, n));
                fin += n;
            }
            inst!(fin, format!("{} {}{}", nombre, m.rm, extra))
        }
        0xFE | 0xFF => {
            let tt = if op == 0xFE { Tam::B8 } else { t };
            let m = match modrm(b, i + 1, rex, tt) { Some(m) => m, None => return db(b) };
            let g = m.reg & 7;
            let nombre = if op == 0xFE {
                ["inc","dec","inc","dec","inc","dec","inc","dec"][g as usize]
            } else {
                ["inc","dec","call","callf","jmp","jmpf","push","?"][g as usize]
            };
            let txt = if op == 0xFF && (g == 2 || g == 4) { format!("{} {}", nombre, m.rm) }
                      else if op == 0xFF && g == 6 { format!("push {}", m.rm) }
                      else { format!("{} {}", nombre, m.rm) };
            return Instruccion { len: m.fin, texto: txt, destino: None, rip_rel: m.rip_rel };
        }
        0x0F => {
            if i + 1 >= b.len() { return db(b); }
            let op2 = b[i + 1];
            let j = i + 2;
            match op2 {
                0x05 => inst!(j, String::from("syscall")),
                0x0B => inst!(j, String::from("ud2")),
                0x1F => {
                    let m = match modrm(b, j, rex, t) { Some(m) => m, None => return db(b) };
                    inst!(m.fin, format!("nop {}", m.rm))
                }
                0x10 | 0x11 | 0x28 | 0x29 | 0x54 | 0x55 | 0x56 | 0x57 | 0x58 | 0x59 | 0x5A | 0x5B | 0x5C | 0x5D | 0x5E | 0x5F => {
                    let nombre = match op2 {
                        0x10 | 0x28 | 0x54 | 0x56 | 0x57 => "movups",
                        0x11 | 0x29 | 0x55 | 0x5B => "movups",
                        0x58 => "addps", 0x59 => "mulps", 0x5A => "cvtps2pd", 0x5C => "subps",
                        0x5D => "minps", 0x5E => "divps", 0x5F => "maxps",
                        _ => "sse",
                    };
                    let m = match modrm(b, j, rex, t) { Some(m) => m, None => return db(b) };
                    let xr = (m.reg & 7) as usize;
                    inst!(m.fin, format!("{} xmm{}, {}", nombre, xr, m.rm))
                }
                0x2E | 0x2F => {
                    let m = match modrm(b, j, rex, t) { Some(m) => m, None => return db(b) };
                    inst!(m.fin, format!("{} xmm{}, {}", if op2 == 0x2E { "ucomiss" } else { "comiss" }, m.reg & 7, m.rm))
                }
                0x31 => inst!(j, String::from("rdtsc")),
                0x40..=0x4F => {
                    let m = match modrm(b, j, rex, t) { Some(m) => m, None => return db(b) };
                    inst!(m.fin, format!("{} {}, {}", jcc(op2 - 0x40), reg(m.reg, t), m.rm))
                }
                0x80..=0x8F => {
                    if j + 4 > b.len() { return db(b); }
                    let rel = rd_signed(b, j, 4);
                    let fin = j + 4;
                    let destino = (base as i64 + fin as i64 + rel) as u64;
                    return Instruccion { len: fin, texto: format!("{} {:#x}", jcc(op2 - 0x80), destino), destino: Some(destino), rip_rel: None };
                }
                0x90..=0x9F => {
                    let m = match modrm(b, j, rex, Tam::B8) { Some(m) => m, None => return db(b) };
                    inst!(m.fin, format!("{} {}", ["seto","setno","setb","setae","sete","setne","setbe","seta","sets","setns","setp","setnp","setl","setge","setle","setg"][(op2 - 0x90) as usize], m.rm))
                }
                0xA2 => inst!(j, String::from("cpuid")),
                0xAF => {
                    let m = match modrm(b, j, rex, t) { Some(m) => m, None => return db(b) };
                    inst!(m.fin, format!("imul {}, {}", reg(m.reg, t), m.rm))
                }
                0xB6 | 0xB7 | 0xBE | 0xBF => {
                    let origen = if op2 == 0xB6 || op2 == 0xBE { Tam::B8 } else { Tam::B16 };
                    let m = match modrm(b, j, rex, origen) { Some(m) => m, None => return db(b) };
                    let nombre = if op2 == 0xB6 || op2 == 0xB7 { "movzx" } else { "movsx" };
                    inst!(m.fin, format!("{} {}, {}", nombre, reg(m.reg, t), m.rm))
                }
                0xEF => inst!(j, String::from("pxor")),
                _ => return db(b),
            }
        }
        _ => {}
    }
    db(b)
}

fn jcc(code: u8) -> &'static str {
    match code & 0xF {
        0x0 => "jo", 0x1 => "jno", 0x2 => "jb", 0x3 => "jae",
        0x4 => "je", 0x5 => "jne", 0x6 => "jbe", 0x7 => "ja",
        0x8 => "js", 0x9 => "jns", 0xA => "jp", 0xB => "jnp",
        0xC => "jl", 0xD => "jge", 0xE => "jle", _ => "jg",
    }
}

/// Fallback: byte de datos (nunca desincroniza el recorrido).
fn db(b: &[u8]) -> Instruccion {
    let mut texto = String::from("db ");
    if let Some(x) = b.first() { texto.push_str(&format!("{:#04x}", x)); }
    Instruccion { len: 1, texto, destino: None, rip_rel: None }
}

/// Tamaño mínimo de buffer de decodificación recomendado.
pub const VENTANA: usize = 15;
