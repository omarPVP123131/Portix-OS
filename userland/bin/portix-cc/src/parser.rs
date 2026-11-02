//! Analizador sintáctico (descenso recursivo) del subconjunto de C de PCC.
//!
//! Subconjunto soportado (documentado en `portix-cc --help`):
//!   tipos `int`/`long` (64 bits), `char` (8 bits), `void`, punteros `*`,
//!   arreglos `[N]`, `unsigned` se tolera como sinónimo;
//!   declaraciones globales y locales con inicializador, funciones con
//!   hasta 6 parámetros, `if/else`, `while`, `do/while`, `for`, `break`,
//!   `continue`, `return`, bloques; expresiones C completas de enteros y
//!   punteros (aritmética escalada, `&`, `*`, `[]`, `++/--`, `?:`,
//!   operadores compuestos, `&&`/`||` con cortocircuito, `sizeof`).
//!   `printf` con formato literal se expande en tiempo de compilación
//!   (`%d %s %c %x %%`).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::lexer::{Tok, Token};

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Int,
    Char,
    Void,
    Ptr(Box<Ty>),
    Arr(Box<Ty>, usize),
    /// (índice en `Program::structs`, tamaño en bytes)
    Struct(usize, usize),
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: String,
    pub members: Vec<(String, Ty, usize)>,
    pub size: usize,
}

impl Ty {
    pub fn size(&self) -> usize {
        match self {
            Ty::Int | Ty::Ptr(_) => 8,
            Ty::Char => 1,
            Ty::Void => 1,
            Ty::Struct(_, sz) => *sz,
            Ty::Arr(e, n) => e.size() * n,
        }
    }
    pub fn is_ptr_like(&self) -> bool { matches!(self, Ty::Ptr(_) | Ty::Arr(_, _)) }
    pub fn elem(&self) -> Ty {
        match self { Ty::Ptr(e) | Ty::Arr(e, _) => (**e).clone(), _ => Ty::Int }
    }
    pub fn decay(&self) -> Ty {
        match self { Ty::Arr(e, _) => Ty::Ptr(e.clone()), t => t.clone() }
    }
}

#[derive(Clone, Debug)]
pub enum Expr {
    Num(i64),
    Str(usize),
    Var(String),
    Unary(&'static str, Box<Expr>),         // "-", "!", "~"
    Deref(Box<Expr>),
    Addr(Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    /// `lhs op= rhs`
    OpAssign(&'static str, Box<Expr>, Box<Expr>),
    /// incremento/decremento: (op, es_postfijo, operando)
    IncDec(i64, bool, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Seq(Vec<Expr>),
    Sizeof(Ty),
    SizeofExpr(Box<Expr>),
    Cast(Ty, Box<Expr>),
    /// acceso a miembro de struct: `base.nombre` (`p->n` es `(*p).n`)
    Member(Box<Expr>, String),
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Expr(Expr),
    Decl(Ty, String, Option<Expr>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    Return(Option<Expr>),
    Block(Vec<Stmt>),
    Switch(Expr, Vec<Stmt>),
    Case(i64),
    Default,
    /// Varias sentencias sin abrir ámbito (declaraciones múltiples / inicializadores de arreglo)
    Multi(Vec<Stmt>),
    /// Marca de número de línea para los diagnósticos del generador
    Mark(usize),
    Break,
    Continue,
}

#[derive(Clone, Debug)]
pub struct Func {
    pub name: String,
    pub ret: Ty,
    pub params: Vec<(Ty, String)>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

#[derive(Default)]
pub struct Program {
    pub funcs: Vec<Func>,
    pub globals: Vec<(String, Ty)>,
    /// Asignaciones que inicializan globales; se ejecutan en `_start` antes de `main`.
    pub global_inits: Vec<Expr>,
    pub structs: Vec<StructDef>,
    pub strings: Vec<Vec<u8>>,
    pub protos: Vec<(String, Ty)>,
}

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    pub prog: Program,
    typedefs: Vec<(String, Ty)>,
    consts: Vec<(String, i64)>,
    /// [PCC 1.2] `static` visto en el último especificador de tipo.
    vio_static: bool,
    /// [PCC 1.2] Función en curso y alias nombre local -> global oculto de sus `static`.
    func_actual: String,
    static_alias: Vec<(String, String)>,
}

type PResult<T> = Result<T, String>;

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser { toks, pos: 0, prog: Program::default(), typedefs: Vec::new(), consts: Vec::new(), vio_static: false, func_actual: String::new(), static_alias: Vec::new() }
    }

    fn peek(&self) -> &Tok { &self.toks[self.pos].tok }
    fn line(&self) -> usize { self.toks[self.pos].line }
    fn next(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos + 1 < self.toks.len() { self.pos += 1; }
        t
    }
    fn is_punct(&self, p: &str) -> bool { matches!(self.peek(), Tok::Punct(q) if *q == p) }
    fn is_kw(&self, k: &str) -> bool { matches!(self.peek(), Tok::Ident(s) if s == k) }
    fn eat_punct(&mut self, p: &str) -> bool { if self.is_punct(p) { self.next(); true } else { false } }
    fn eat_kw(&mut self, k: &str) -> bool { if self.is_kw(k) { self.next(); true } else { false } }
    fn expect(&mut self, p: &str) -> PResult<()> {
        if self.eat_punct(p) { Ok(()) } else {
            Err(format!("linea {}: se esperaba '{}' y se encontro {:?}", self.line(), p, self.peek()))
        }
    }
    fn ident(&mut self) -> PResult<String> {
        match self.next() {
            Tok::Ident(s) => Ok(s),
            t => Err(format!("linea {}: se esperaba un identificador, se encontro {:?}", self.line(), t)),
        }
    }

    fn is_type_start(&self) -> bool {
        match self.peek() {
            Tok::Ident(s) => {
                matches!(s.as_str(), "int" | "char" | "void" | "long" | "unsigned" | "signed" | "short" | "const" | "static" | "size_t" | "bool" | "struct" | "union" | "enum")
                    || self.typedefs.iter().any(|(n, _)| n == s)
            }
            _ => false,
        }
    }

    /// Especificador de tipo base (sin declarador).
    fn base_type(&mut self) -> PResult<Ty> {
        self.vio_static = false;
        let mut ty: Option<Ty> = None;
        loop {
            if self.eat_kw("static") { self.vio_static = true; if ty.is_none() { ty = Some(Ty::Int); } continue; }
            if self.eat_kw("const") || self.eat_kw("unsigned") || self.eat_kw("signed") {
                if ty.is_none() { ty = Some(Ty::Int); }
                continue;
            }
            if self.eat_kw("int") || self.eat_kw("long") || self.eat_kw("short") || self.eat_kw("size_t") || self.eat_kw("bool") { ty = Some(Ty::Int); continue; }
            if self.eat_kw("char") { ty = Some(Ty::Char); continue; }
            if self.eat_kw("void") { ty = Some(Ty::Void); continue; }
            if self.eat_kw("struct") { ty = Some(self.struct_type(false)?); continue; }
            if self.eat_kw("union") { ty = Some(self.struct_type(true)?); continue; } // [PCC 1.2]
            if self.eat_kw("enum") { self.enum_type()?; ty = Some(Ty::Int); continue; }
            if let Tok::Ident(s) = self.peek() {
                if let Some((_, t)) = self.typedefs.iter().find(|(n, _)| n == s) {
                    let t = t.clone();
                    self.next();
                    ty = Some(t);
                    continue;
                }
            }
            break;
        }
        ty.ok_or_else(|| format!("linea {}: se esperaba un tipo", self.line()))
    }

    /// `struct Nombre { miembros }` | `struct Nombre` | `struct { miembros }`
    /// [PCC 1.2] Con `es_union` todos los miembros comparten el desplazamiento 0
    /// y el tamaño es el del miembro mayor.
    fn struct_type(&mut self, es_union: bool) -> PResult<Ty> {
        let line = self.line();
        let name = if let Tok::Ident(_) = self.peek() { self.ident()? } else { String::new() };
        if self.eat_punct("{") {
            let mut members: Vec<(String, Ty, usize)> = Vec::new();
            let mut off = 0usize;
            while !self.is_punct("}") {
                let base = self.base_type()?;
                loop {
                    let (mty, mname, _) = self.declarator(base.clone())?;
                    if mty == Ty::Void { return Err(format!("linea {}: miembro de tipo void", self.line())); }
                    if members.iter().any(|m| m.0 == mname) { return Err(format!("linea {}: miembro '{}' duplicado", self.line(), mname)); }
                    if es_union {
                        members.push((mname, mty.clone(), 0));
                        if mty.size() > off { off = mty.size(); }
                    } else {
                        let align = if mty.size() >= 8 || matches!(mty, Ty::Int | Ty::Ptr(_)) { 8 } else { 1 };
                        off = (off + align - 1) / align * align;
                        members.push((mname, mty.clone(), off));
                        off += mty.size();
                    }
                    if !self.eat_punct(",") { break; }
                }
                self.expect(";")?;
            }
            self.next();
            let size = (off + 7) / 8 * 8;
            let size = if size == 0 { 8 } else { size };
            let idx = self.prog.structs.len();
            if !name.is_empty() {
                if let Some(i) = self.prog.structs.iter().position(|d| d.name == name) {
                    if !self.prog.structs[i].members.is_empty() { return Err(format!("linea {}: struct '{}' redefinido", line, name)); }
                    self.prog.structs[i].members = members;
                    self.prog.structs[i].size = size;
                    return Ok(Ty::Struct(i, size));
                }
            }
            self.prog.structs.push(StructDef { name, members, size });
            return Ok(Ty::Struct(idx, size));
        }
        if name.is_empty() { return Err(format!("linea {}: 'struct' sin nombre ni cuerpo", line)); }
        match self.prog.structs.iter().position(|d| d.name == name) {
            Some(i) => Ok(Ty::Struct(i, self.prog.structs[i].size)),
            None => {
                // Declaración adelantada (sólo utilizable mediante punteros hasta definirse)
                self.prog.structs.push(StructDef { name, members: Vec::new(), size: 0 });
                Ok(Ty::Struct(self.prog.structs.len() - 1, 0))
            }
        }
    }

    /// `enum Nombre? { A, B = 5, C }` → constantes enteras; el tipo es `int`.
    fn enum_type(&mut self) -> PResult<()> {
        if let Tok::Ident(_) = self.peek() { self.ident()?; }
        if !self.eat_punct("{") { return Ok(()); }
        let mut v: i64 = 0;
        while !self.is_punct("}") {
            let name = self.ident()?;
            if self.eat_punct("=") {
                let e = self.ternary()?;
                v = self.const_eval(&e)?;
            }
            self.consts.retain(|c| c.0 != name);
            self.consts.push((name, v));
            v += 1;
            if !self.eat_punct(",") { break; }
        }
        self.expect("}")?;
        Ok(())
    }

    /// Evalúa una expresión constante entera (etiquetas `case`, enumeradores).
    fn const_eval(&self, e: &Expr) -> PResult<i64> {
        match e {
            Expr::Num(v) => Ok(*v),
            Expr::Unary("-", x) => Ok(self.const_eval(x)?.wrapping_neg()),
            Expr::Unary("!", x) => Ok((self.const_eval(x)? == 0) as i64),
            Expr::Unary("~", x) => Ok(!self.const_eval(x)?),
            Expr::Sizeof(t) => Ok(t.size() as i64),
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.const_eval(a)?, self.const_eval(b)?);
                Ok(match *op {
                    "+" => a.wrapping_add(b), "-" => a.wrapping_sub(b), "*" => a.wrapping_mul(b),
                    "/" => if b == 0 { return Err(format!("linea {}: division entre cero en constante", self.line())) } else { a / b },
                    "%" => if b == 0 { return Err(format!("linea {}: division entre cero en constante", self.line())) } else { a % b },
                    "<<" => a.wrapping_shl(b as u32), ">>" => a.wrapping_shr(b as u32),
                    "&" => a & b, "|" => a | b, "^" => a ^ b,
                    "==" => (a == b) as i64, "!=" => (a != b) as i64, "<" => (a < b) as i64,
                    "<=" => (a <= b) as i64, ">" => (a > b) as i64, ">=" => (a >= b) as i64,
                    "&&" => (a != 0 && b != 0) as i64, "||" => (a != 0 || b != 0) as i64,
                    _ => return Err(format!("linea {}: operador no constante", self.line())),
                })
            }
            _ => Err(format!("linea {}: se esperaba una expresion constante", self.line())),
        }
    }

    /// Declarador: `* * nombre [N][M]` → (tipo completo, nombre, arreglo sin tamaño `[]`)
    fn declarator(&mut self, base: Ty) -> PResult<(Ty, String, bool)> {
        let mut ty = base;
        while self.eat_punct("*") { self.eat_kw("const"); ty = Ty::Ptr(Box::new(ty)); }
        let name = self.ident()?;
        let mut dims = Vec::new();
        let mut sin_tam = false;
        while self.eat_punct("[") {
            if self.eat_punct("]") { dims.push(0); continue; }
            let e = self.ternary()?;
            let n = self.const_eval(&e)?;
            if n <= 0 { return Err(format!("linea {}: tamano de arreglo invalido", self.line())); }
            self.expect("]")?;
            dims.push(n as usize);
        }
        for (k, d) in dims.iter().enumerate().rev() {
            if *d == 0 {
                if k == 0 { sin_tam = true; } else { return Err(format!("linea {}: solo la primera dimension puede omitirse", self.line())); }
                continue;
            }
            ty = Ty::Arr(Box::new(ty), *d);
        }
        if sin_tam { ty = Ty::Ptr(Box::new(ty)); }
        Ok((ty, name, sin_tam))
    }

    /// Inicializador tras `nombre =`: devuelve el tipo definitivo y las
    /// asignaciones que lo materializan (`{…}` y cadenas en arreglos `char`).
    fn parse_init(&mut self, ty: Ty, name: &str, sin_tam: bool) -> PResult<(Ty, Vec<Expr>)> {
        let var = |n: &str| Expr::Var(String::from(n));
        let elem_at = |n: &str, i: usize| Expr::Deref(Box::new(Expr::Binary("+", Box::new(Expr::Var(String::from(n))), Box::new(Expr::Num(i as i64)))));
        if self.is_punct("{") {
            self.next();
            let mut elems = Vec::new();
            while !self.is_punct("}") {
                elems.push(self.assign()?);
                if !self.eat_punct(",") { break; }
            }
            self.expect("}")?;
            if let Ty::Struct(si, _) = &ty {
                // [PCC 1.2] `struct S v = { a, b, c };` -> asignacion miembro a miembro en orden.
                let miembros: Vec<String> = self.prog.structs[*si].members.iter().map(|m| m.0.clone()).collect();
                if elems.len() > miembros.len() { return Err(format!("linea {}: demasiados elementos en el inicializador de '{}'", self.line(), name)); }
                let inits = elems.into_iter().zip(miembros.into_iter()).map(|(e, m)| Expr::Assign(Box::new(Expr::Member(Box::new(var(name)), m)), Box::new(e))).collect();
                return Ok((ty, inits));
            }
            let elem_ty = match &ty { Ty::Arr(e, _) | Ty::Ptr(e) => (**e).clone(), _ => return Err(format!("linea {}: inicializador {{…}} para una variable que no es arreglo o struct", self.line())) };
            let n = match &ty { Ty::Arr(_, n) if !sin_tam => *n, _ => elems.len().max(1) };
            if elems.len() > n { return Err(format!("linea {}: demasiados elementos en el inicializador de '{}'", self.line(), name)); }
            let final_ty = Ty::Arr(Box::new(elem_ty), n);
            let inits = elems.into_iter().enumerate().map(|(i, e)| Expr::Assign(Box::new(elem_at(name, i)), Box::new(e))).collect();
            return Ok((final_ty, inits));
        }
        let e = self.assign()?;
        if let Expr::Str(idx) = &e {
            let is_char_arr = match &ty { Ty::Arr(e, _) => **e == Ty::Char, Ty::Ptr(e) if sin_tam => **e == Ty::Char, _ => false };
            if is_char_arr {
                let bytes = self.prog.strings[*idx].clone();
                let n = match &ty { Ty::Arr(_, n) if !sin_tam => *n, _ => bytes.len() + 1 };
                if bytes.len() + 1 > n { return Err(format!("linea {}: la cadena no cabe en '{}[{}]'", self.line(), name, n)); }
                let mut inits = Vec::new();
                for (i, b) in bytes.iter().enumerate() { inits.push(Expr::Assign(Box::new(elem_at(name, i)), Box::new(Expr::Num(*b as i64)))); }
                inits.push(Expr::Assign(Box::new(elem_at(name, bytes.len())), Box::new(Expr::Num(0))));
                return Ok((Ty::Arr(Box::new(Ty::Char), n), inits));
            }
        }
        Ok((ty, alloc::vec![Expr::Assign(Box::new(var(name)), Box::new(e))]))
    }

    /// Tipo abstracto para `sizeof(tipo)` y casts: `int *`, `char`.
    fn abstract_type(&mut self) -> PResult<Ty> {
        let mut ty = self.base_type()?;
        while self.eat_punct("*") { ty = Ty::Ptr(Box::new(ty)); }
        Ok(ty)
    }

    pub fn parse_program(&mut self) -> PResult<()> {
        while *self.peek() != Tok::Eof {
            if self.eat_kw("typedef") {
                let base = self.base_type()?;
                let (ty, name, _) = self.declarator(base)?;
                self.expect(";")?;
                self.typedefs.push((name, ty));
                continue;
            }
            if self.eat_kw("extern") { /* se tolera */ }
            let line = self.line();
            let base = self.base_type()?;
            if self.eat_punct(";") { continue; }
            let (ty, name, sin_tam) = self.declarator(base.clone())?;
            if self.is_punct("(") {
                self.next();
                let mut params = Vec::new();
                if !self.is_punct(")") {
                    loop {
                        if self.eat_punct("...") { break; }
                        let pb = self.base_type()?;
                        if pb == Ty::Void && self.is_punct(")") { break; }
                        // parámetro sin nombre (prototipo)
                        let mut pty = pb;
                        while self.eat_punct("*") { pty = Ty::Ptr(Box::new(pty)); }
                        let pname = if let Tok::Ident(_) = self.peek() { self.ident()? } else { String::new() };
                        if self.eat_punct("[") { self.expect("]")?; pty = Ty::Ptr(Box::new(pty)); }
                        params.push((pty.decay(), pname));
                        if !self.eat_punct(",") { break; }
                    }
                }
                self.expect(")")?;
                if self.eat_punct(";") {
                    self.prog.protos.push((name, ty));
                    continue;
                }
                if params.len() > 6 {
                    return Err(format!("linea {}: la funcion '{}' tiene mas de 6 parametros (limite de PCC)", line, name));
                }
                self.func_actual = name.clone();
                self.static_alias.clear();
                let body = self.block()?;
                self.func_actual.clear();
                self.static_alias.clear();
                self.prog.funcs.push(Func { name, ret: ty, params, body, line });
                continue;
            }
            // Variable(s) global(es)
            let mut cur = (ty, name, sin_tam);
            loop {
                if cur.0 == Ty::Void { return Err(format!("linea {}: variable de tipo void", self.line())); }
                if self.eat_punct("=") {
                    let (fty, inits) = self.parse_init(cur.0.clone(), &cur.1, cur.2)?;
                    self.prog.globals.push((cur.1.clone(), fty));
                    self.prog.global_inits.extend(inits);
                } else {
                    if cur.2 { return Err(format!("linea {}: arreglo '{}[]' sin inicializador", self.line(), cur.1)); }
                    self.prog.globals.push((cur.1.clone(), cur.0.clone()));
                }
                if self.eat_punct(",") { cur = self.declarator(base.clone())?; continue; }
                self.expect(";")?;
                break;
            }
        }
        Ok(())
    }

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect("{")?;
        let mut v = Vec::new();
        while !self.is_punct("}") {
            if *self.peek() == Tok::Eof { return Err(format!("linea {}: falta '}}'", self.line())); }
            v.push(self.stmt()?);
        }
        self.next();
        Ok(v)
    }

    fn decl_stmt(&mut self) -> PResult<Stmt> {
        let base = self.base_type()?;
        let mut out = Vec::new();
        let es_static = self.vio_static && !self.func_actual.is_empty();
        loop {
            let (ty, name, sin_tam) = self.declarator(base.clone())?;
            if es_static {
                // [PCC 1.2] `static` local: global oculto con inicializacion unica
                // (en `_start`) y alias dentro de la funcion.
                let oculto = format!("__st_{}_{}", self.func_actual, name);
                if self.eat_punct("=") {
                    let (fty, inits) = self.parse_init(ty, &oculto, sin_tam)?;
                    self.prog.globals.push((oculto.clone(), fty));
                    self.prog.global_inits.extend(inits);
                } else {
                    if sin_tam { return Err(format!("linea {}: arreglo '{}[]' sin inicializador", self.line(), name)); }
                    self.prog.globals.push((oculto.clone(), ty));
                }
                self.static_alias.push((name, oculto));
                if !self.eat_punct(",") { break; }
                continue;
            }
            if self.eat_punct("=") {
                let (fty, inits) = self.parse_init(ty, &name, sin_tam)?;
                out.push(Stmt::Decl(fty, name.clone(), None));
                for e in inits { out.push(Stmt::Expr(e)); }
            } else {
                if sin_tam { return Err(format!("linea {}: arreglo '{}[]' sin inicializador", self.line(), name)); }
                out.push(Stmt::Decl(ty, name, None));
            }
            if !self.eat_punct(",") { break; }
        }
        self.expect(";")?;
        if out.len() == 1 { Ok(out.pop().unwrap_or(Stmt::Multi(Vec::new()))) } else { Ok(Stmt::Multi(out)) }
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let line = self.line();
        let s = self.stmt_inner()?;
        Ok(Stmt::Multi(alloc::vec![Stmt::Mark(line), s]))
    }

    fn stmt_inner(&mut self) -> PResult<Stmt> {
        if self.is_punct("{") { return Ok(Stmt::Block(self.block()?)); }
        if self.eat_punct(";") { return Ok(Stmt::Block(Vec::new())); }
        if self.is_type_start() { return self.decl_stmt(); }
        if self.eat_kw("if") {
            self.expect("(")?;
            let c = self.expr()?;
            self.expect(")")?;
            let t = Box::new(self.stmt()?);
            let e = if self.eat_kw("else") { Some(Box::new(self.stmt()?)) } else { None };
            return Ok(Stmt::If(c, t, e));
        }
        if self.eat_kw("while") {
            self.expect("(")?;
            let c = self.expr()?;
            self.expect(")")?;
            return Ok(Stmt::While(c, Box::new(self.stmt()?)));
        }
        if self.eat_kw("do") {
            let body = Box::new(self.stmt()?);
            if !self.eat_kw("while") { return Err(format!("linea {}: se esperaba 'while' tras 'do'", self.line())); }
            self.expect("(")?;
            let c = self.expr()?;
            self.expect(")")?;
            self.expect(";")?;
            return Ok(Stmt::DoWhile(body, c));
        }
        if self.eat_kw("for") {
            self.expect("(")?;
            let init = if self.eat_punct(";") { None } else if self.is_type_start() {
                Some(Box::new(self.decl_stmt()?))
            } else { let e = self.expr()?; self.expect(";")?; Some(Box::new(Stmt::Expr(e))) };
            let cond = if self.is_punct(";") { None } else { Some(self.expr()?) };
            self.expect(";")?;
            let step = if self.is_punct(")") { None } else { Some(self.expr()?) };
            self.expect(")")?;
            let body = Box::new(self.stmt()?);
            return Ok(Stmt::For(init, cond, step, body));
        }
        if self.eat_kw("switch") {
            self.expect("(")?;
            let e = self.expr()?;
            self.expect(")")?;
            let body = self.block()?;
            return Ok(Stmt::Switch(e, body));
        }
        if self.eat_kw("case") {
            let e = self.ternary()?;
            let v = self.const_eval(&e)?;
            self.expect(":")?;
            return Ok(Stmt::Case(v));
        }
        if self.eat_kw("default") {
            self.expect(":")?;
            return Ok(Stmt::Default);
        }
        if self.eat_kw("return") {
            if self.eat_punct(";") { return Ok(Stmt::Return(None)); }
            let e = self.expr()?;
            self.expect(";")?;
            return Ok(Stmt::Return(Some(e)));
        }
        if self.eat_kw("break") { self.expect(";")?; return Ok(Stmt::Break); }
        if self.eat_kw("continue") { self.expect(";")?; return Ok(Stmt::Continue); }
        let e = self.expr()?;
        self.expect(";")?;
        Ok(Stmt::Expr(e))
    }

    // ── Expresiones ──────────────────────────────────────────────────────
    pub fn expr(&mut self) -> PResult<Expr> {
        let mut e = self.assign()?;
        while self.eat_punct(",") {
            let r = self.assign()?;
            e = Expr::Seq(alloc::vec![e, r]);
        }
        Ok(e)
    }

    fn assign(&mut self) -> PResult<Expr> {
        let lhs = self.ternary()?;
        if self.eat_punct("=") {
            let rhs = self.assign()?;
            return Ok(Expr::Assign(Box::new(lhs), Box::new(rhs)));
        }
        for (p, op) in [("+=", "+"), ("-=", "-"), ("*=", "*"), ("/=", "/"), ("%=", "%"), ("&=", "&"), ("|=", "|"), ("^=", "^"), ("<<=", "<<"), (">>=", ">>")] {
            if self.eat_punct(p) {
                let rhs = self.assign()?;
                return Ok(Expr::OpAssign(op, Box::new(lhs), Box::new(rhs)));
            }
        }
        Ok(lhs)
    }

    fn ternary(&mut self) -> PResult<Expr> {
        let c = self.binary(0)?;
        if self.eat_punct("?") {
            let a = self.expr()?;
            self.expect(":")?;
            let b = self.ternary()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    fn binop_at(&self, level: usize) -> Option<&'static str> {
        const LEVELS: &[&[&str]] = &[
            &["||"], &["&&"], &["|"], &["^"], &["&"], &["==", "!="],
            &["<", ">", "<=", ">="], &["<<", ">>"], &["+", "-"], &["*", "/", "%"],
        ];
        if level >= LEVELS.len() { return None; }
        if let Tok::Punct(p) = self.peek() {
            for op in LEVELS[level] { if op == p { return Some(op); } }
        }
        None
    }

    fn binary(&mut self, level: usize) -> PResult<Expr> {
        if level >= 10 { return self.unary(); }
        let mut lhs = self.binary(level + 1)?;
        while let Some(op) = self.binop_at(level) {
            self.next();
            let rhs = self.binary(level + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        if self.eat_punct("-") { return Ok(Expr::Unary("-", Box::new(self.unary()?))); }
        if self.eat_punct("+") { return self.unary(); }
        if self.eat_punct("!") { return Ok(Expr::Unary("!", Box::new(self.unary()?))); }
        if self.eat_punct("~") { return Ok(Expr::Unary("~", Box::new(self.unary()?))); }
        if self.eat_punct("*") { return Ok(Expr::Deref(Box::new(self.unary()?))); }
        if self.eat_punct("&") { return Ok(Expr::Addr(Box::new(self.unary()?))); }
        if self.eat_punct("++") { return Ok(Expr::IncDec(1, false, Box::new(self.unary()?))); }
        if self.eat_punct("--") { return Ok(Expr::IncDec(-1, false, Box::new(self.unary()?))); }
        if self.eat_kw("sizeof") {
            if self.is_punct("(") {
                // sizeof(tipo) o sizeof(expr)
                let save = self.pos;
                self.next();
                if self.is_type_start() {
                    let t = self.abstract_type()?;
                    self.expect(")")?;
                    return Ok(Expr::Sizeof(t));
                }
                self.pos = save;
            }
            return Ok(Expr::SizeofExpr(Box::new(self.unary()?)));
        }
        // cast: (tipo) expr
        if self.is_punct("(") {
            let save = self.pos;
            self.next();
            if self.is_type_start() {
                let t = self.abstract_type()?;
                self.expect(")")?;
                return Ok(Expr::Cast(t, Box::new(self.unary()?)));
            }
            self.pos = save;
        }
        self.postfix()
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.eat_punct("[") {
                let idx = self.expr()?;
                self.expect("]")?;
                e = Expr::Deref(Box::new(Expr::Binary("+", Box::new(e), Box::new(idx))));
                continue;
            }
            if self.eat_punct("++") { e = Expr::IncDec(1, true, Box::new(e)); continue; }
            if self.eat_punct("--") { e = Expr::IncDec(-1, true, Box::new(e)); continue; }
            if self.eat_punct(".") { let m = self.ident()?; e = Expr::Member(Box::new(e), m); continue; }
            if self.eat_punct("->") { let m = self.ident()?; e = Expr::Member(Box::new(Expr::Deref(Box::new(e))), m); continue; }
            break;
        }
        Ok(e)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let line = self.line();
        match self.next() {
            Tok::Num(v) => Ok(Expr::Num(v)),
            Tok::Str(s) => Ok(Expr::Str(self.add_string(s))),
            Tok::Punct("(") => {
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Ident(name) => {
                if self.eat_punct("(") {
                    let mut args = Vec::new();
                    if !self.is_punct(")") {
                        loop {
                            args.push(self.assign()?);
                            if !self.eat_punct(",") { break; }
                        }
                    }
                    self.expect(")")?;
                    if name == "printf" { return self.expand_printf(args, line); }
                    return Ok(Expr::Call(name, args));
                }
                if name == "NULL" { return Ok(Expr::Num(0)); }
                if let Some((_, v)) = self.consts.iter().find(|c| c.0 == name) { return Ok(Expr::Num(*v)); }
                if let Some((_, oculto)) = self.static_alias.iter().rev().find(|a| a.0 == name) { return Ok(Expr::Var(oculto.clone())); }
                Ok(Expr::Var(name))
            }
            t => Err(format!("linea {}: expresion invalida, se encontro {:?}", line, t)),
        }
    }

    fn add_string(&mut self, s: Vec<u8>) -> usize {
        self.prog.strings.push(s);
        self.prog.strings.len() - 1
    }

    /// `printf("x=%d %s\n", a, b)` → secuencia de llamadas al runtime
    /// resueltas en tiempo de compilación (no hay funciones variádicas).
    fn expand_printf(&mut self, mut args: Vec<Expr>, line: usize) -> PResult<Expr> {
        if args.is_empty() { return Err(format!("linea {}: printf sin formato", line)); }
        let fmt_idx = match args.remove(0) {
            Expr::Str(i) => i,
            _ => return Err(format!("linea {}: printf requiere un formato literal (limite de PCC)", line)),
        };
        let fmt = self.prog.strings[fmt_idx].clone();
        let mut seq = Vec::new();
        let mut lit: Vec<u8> = Vec::new();
        let mut args_iter = args.into_iter();
        let mut i = 0;
        let flush = |lit: &mut Vec<u8>, seq: &mut Vec<Expr>, p: &mut Parser| {
            if !lit.is_empty() {
                let n = lit.len() as i64;
                let idx = p.add_string(core::mem::take(lit));
                seq.push(Expr::Call(String::from("__out"), alloc::vec![Expr::Str(idx), Expr::Num(n)]));
            }
        };
        while i < fmt.len() {
            let c = fmt[i];
            if c == b'%' && i + 1 < fmt.len() {
                let mut j = i + 1;
                // ancho/flags se toleran e ignoran (%5d, %-3s, %ld, %lu, %02x)
                while j < fmt.len() && (fmt[j].is_ascii_digit() || fmt[j] == b'-' || fmt[j] == b'l' || fmt[j] == b'z' || fmt[j] == b'.') { j += 1; }
                if j >= fmt.len() { break; }
                let conv = fmt[j];
                if conv == b'%' { lit.push(b'%'); i = j + 1; continue; }
                flush(&mut lit, &mut seq, self);
                let arg = args_iter.next().ok_or_else(|| format!("linea {}: printf: faltan argumentos para el formato", line))?;
                let callee = match conv {
                    b'd' | b'i' | b'u' => "__printi",
                    b'x' | b'X' | b'p' => "__printx",
                    b's' => "__outs",
                    b'c' => "putchar",
                    other => return Err(format!("linea {}: printf: conversion '%{}' no soportada por PCC", line, other as char)),
                };
                seq.push(Expr::Call(String::from(callee), alloc::vec![arg]));
                i = j + 1;
                continue;
            }
            lit.push(c);
            i += 1;
        }
        flush(&mut lit, &mut seq, self);
        if args_iter.next().is_some() {
            return Err(format!("linea {}: printf: sobran argumentos para el formato", line));
        }
        seq.push(Expr::Num(0));
        Ok(Expr::Seq(seq))
    }
}
