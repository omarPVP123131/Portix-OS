//! Generador de código máquina x86-64 de PCC.
//!
//! Emite bytes de instrucción directamente (no hay ensamblador externo):
//! máquina de pila sobre `rax`/`rdi`, marco de pila clásico `rbp`, llamadas
//! con la convención System V (rdi, rsi, rdx, rcx, r8, r9) y llamadas al
//! sistema nativas de Portix (`int 0x80`: rax=numero, rdi, rsi, rdx, r10).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::parser::{Expr, Func, Program, Stmt, StructDef, Ty};

pub const BASE: u64 = 0x2000_0000;

/// Puente `__syscall` nativo de Portix: rax=n, rdi, rsi, rdx, r10 → `int 0x80`.
pub const PUENTE_PORTIX: &[u8] = &[0x48, 0x89, 0xF8, 0x48, 0x89, 0xF7, 0x48, 0x89, 0xD6, 0x48, 0x89, 0xCA, 0x4D, 0x89, 0xC2, 0xCD, 0x80, 0xC3];
pub const HDR: u64 = 120;

type GResult<T> = Result<T, String>;

#[derive(Clone, Copy)]
enum Abs { Str(usize), Global(usize) }

struct Local { name: String, ty: Ty, off: i32 }

pub struct Gen {
    pub code: Vec<u8>,
    bridge: &'static [u8],
    structs: Vec<StructDef>,
    /// Símbolos de función para `.symtab`: (nombre, inicio, fin) relativos al código.
    pub syms: Vec<(String, usize, usize)>,
    /// Pila de `switch` activos: etiquetas de `case` en orden de aparición y cursor.
    switches: Vec<(Vec<usize>, usize)>,
    funcs: Vec<(String, usize)>,
    func_types: Vec<(String, Ty)>,
    call_fixups: Vec<(usize, String, usize)>,
    abs_fixups: Vec<(usize, Abs)>,
    globals: Vec<(String, Ty, usize)>,
    pub bss_size: usize,
    labels: Vec<Option<usize>>,
    label_fixups: Vec<(usize, usize)>,
    locals: Vec<Local>,
    scopes: Vec<usize>,
    stack_size: i32,
    breaks: Vec<usize>,
    continues: Vec<usize>,
    ret_label: usize,
    cur_line: usize,
    pub stats_funcs: usize,
}

const ARG_REGS_STORE: [&[u8]; 6] = [
    &[0x48, 0x89, 0xBD], // mov [rbp+d], rdi
    &[0x48, 0x89, 0xB5], // rsi
    &[0x48, 0x89, 0x95], // rdx
    &[0x48, 0x89, 0x8D], // rcx
    &[0x4C, 0x89, 0x85], // r8
    &[0x4C, 0x89, 0x8D], // r9
];
const ARG_REGS_POP: [&[u8]; 6] = [
    &[0x5F], &[0x5E], &[0x5A], &[0x59], &[0x41, 0x58], &[0x41, 0x59],
];

impl Gen {
    pub fn new(bridge: &'static [u8], structs: Vec<StructDef>) -> Self {
        Gen {
            code: Vec::with_capacity(4096), bridge, structs, syms: Vec::new(), switches: Vec::new(), funcs: Vec::new(), func_types: Vec::new(),
            call_fixups: Vec::new(), abs_fixups: Vec::new(), globals: Vec::new(), bss_size: 0,
            labels: Vec::new(), label_fixups: Vec::new(), locals: Vec::new(), scopes: Vec::new(),
            stack_size: 0, breaks: Vec::new(), continues: Vec::new(), ret_label: 0, cur_line: 0,
            stats_funcs: 0,
        }
    }

    // ── Emisión básica ──────────────────────────────────────────────────
    fn emit(&mut self, b: &[u8]) { self.code.extend_from_slice(b); }
    fn emit32(&mut self, v: i32) { self.code.extend_from_slice(&v.to_le_bytes()); }
    fn emit64(&mut self, v: u64) { self.code.extend_from_slice(&v.to_le_bytes()); }
    fn pos(&self) -> usize { self.code.len() }

    fn new_label(&mut self) -> usize { self.labels.push(None); self.labels.len() - 1 }
    fn place(&mut self, l: usize) { self.labels[l] = Some(self.pos()); }
    fn jmp(&mut self, l: usize) { self.emit(&[0xE9]); self.label_fixups.push((self.pos(), l)); self.emit32(0); }
    fn je(&mut self, l: usize) { self.emit(&[0x0F, 0x84]); self.label_fixups.push((self.pos(), l)); self.emit32(0); }
    fn jne(&mut self, l: usize) { self.emit(&[0x0F, 0x85]); self.label_fixups.push((self.pos(), l)); self.emit32(0); }
    fn test_rax(&mut self) { self.emit(&[0x48, 0x85, 0xC0]); }
    fn push_rax(&mut self) { self.emit(&[0x50]); }
    fn pop_rdi(&mut self) { self.emit(&[0x5F]); }
    fn mov_rax_imm(&mut self, v: i64) {
        if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
            self.emit(&[0x48, 0xC7, 0xC0]); self.emit32(v as i32);
        } else {
            self.emit(&[0x48, 0xB8]); self.emit64(v as u64);
        }
    }
    fn mov_rax_abs(&mut self, a: Abs) { self.emit(&[0x48, 0xB8]); self.abs_fixups.push((self.pos(), a)); self.emit64(0); }
    fn lea_rax_rbp(&mut self, off: i32) { self.emit(&[0x48, 0x8D, 0x85]); self.emit32(off); }
    fn call(&mut self, name: &str) { self.emit(&[0xE8]); self.call_fixups.push((self.pos(), String::from(name), self.cur_line)); self.emit32(0); }
    fn bool_from_flags(&mut self, setcc: u8) { self.emit(&[0x0F, setcc, 0xC0, 0x48, 0x0F, 0xB6, 0xC0]); }

    /// Carga `rax = *(ty*)rax` (los arreglos no se cargan: decaen a puntero).
    fn load(&mut self, ty: &Ty) {
        match ty {
            Ty::Char => self.emit(&[0x48, 0x0F, 0xBE, 0x00]),
            Ty::Arr(_, _) | Ty::Struct(_, _) => {}
            _ => self.emit(&[0x48, 0x8B, 0x00]),
        }
    }
    /// Guarda `*(ty*)rdi = rax`.
    fn store_rdi(&mut self, ty: &Ty) {
        match ty { Ty::Char => self.emit(&[0x88, 0x07]), _ => self.emit(&[0x48, 0x89, 0x07]) }
    }
    /// Guarda `*(ty*)r8 = rax`.
    fn store_r8(&mut self, ty: &Ty) {
        match ty { Ty::Char => self.emit(&[0x41, 0x88, 0x00]), _ => self.emit(&[0x49, 0x89, 0x00]) }
    }

    fn err<T>(&self, m: &str) -> GResult<T> { Err(format!("linea {}: {}", self.cur_line, m)) }

    // ── Símbolos ────────────────────────────────────────────────────────
    fn find_local(&self, name: &str) -> Option<(Ty, i32)> {
        self.locals.iter().rev().find(|l| l.name == name).map(|l| (l.ty.clone(), l.off))
    }
    fn find_global(&self, name: &str) -> Option<(Ty, usize)> {
        self.globals.iter().find(|g| g.0 == name).map(|g| (g.1.clone(), g.2))
    }
    fn add_local(&mut self, name: &str, ty: Ty) -> i32 {
        let sz = ((ty.size() + 7) / 8 * 8) as i32;
        self.stack_size += sz;
        let off = -self.stack_size;
        self.locals.push(Local { name: String::from(name), ty, off });
        off
    }
    fn func_ret(&self, name: &str) -> Ty {
        self.func_types.iter().find(|f| f.0 == name).map(|f| f.1.clone()).unwrap_or(Ty::Int)
    }

    // ── Programa ────────────────────────────────────────────────────────
    pub fn gen_program(&mut self, prog: &Program) -> GResult<()> {
        for (name, ty) in &prog.globals {
            if self.find_global(name).is_some() { return Err(format!("variable global '{}' duplicada", name)); }
            if ty.size() == 0 { return Err(format!("variable global '{}' de struct incompleto", name)); }
            let off = (self.bss_size + 7) / 8 * 8;
            self.globals.push((name.clone(), ty.clone(), off));
            self.bss_size = off + ty.size();
        }
        for f in &prog.funcs { self.func_types.push((f.name.clone(), f.ret.clone())); }
        for (n, t) in &prog.protos { self.func_types.push((n.clone(), t.clone())); }
        self.func_types.push((String::from("__syscall"), Ty::Int));

        // _start: argc/argv → main → exit
        let start_ini = self.pos();
        self.emit(&[0x48, 0x8B, 0x3C, 0x24]);       // mov rdi, [rsp]
        self.emit(&[0x48, 0x8D, 0x74, 0x24, 0x08]); // lea rsi, [rsp+8]
        self.emit(&[0x48, 0x83, 0xE4, 0xF0]);       // and rsp, -16
        self.emit(&[0x55, 0x48, 0x89, 0xE5]);       // push rbp; mov rbp, rsp (marco para inicializadores)
        self.emit(&[0x57, 0x56]);                   // push rdi; push rsi
        // Inicializadores de variables globales (se ejecutan antes de main)
        for e in &prog.global_inits { self.gen_expr(e)?; }
        self.emit(&[0x5E, 0x5F]);                   // pop rsi; pop rdi
        self.call("main");
        self.emit(&[0x48, 0x89, 0xC6]);             // mov rsi, rax (codigo de salida)
        self.emit(&[0x48, 0x31, 0xFF]);             // xor rdi, rdi (SYS_EXIT = 0)
        self.call("__syscall");
        self.emit(&[0xEB, 0xFE]);                   // jmp $
        self.syms.push((String::from("_start"), start_ini, self.pos()));

        // __syscall(n, a, b, c, d): puente directo a `int 0x80`
        let br_ini = self.pos();
        self.funcs.push((String::from("__syscall"), self.pos()));
        let bridge = self.bridge;
        self.emit(bridge);
        self.syms.push((String::from("__syscall"), br_ini, self.pos()));

        for f in &prog.funcs { self.gen_func(f)?; }

        // Resolución de etiquetas y llamadas
        let fixups = core::mem::take(&mut self.label_fixups);
        for (p, l) in fixups {
            let target = self.labels[l].ok_or_else(|| String::from("etiqueta interna sin colocar"))?;
            let rel = (target as i64 - (p as i64 + 4)) as i32;
            self.code[p..p + 4].copy_from_slice(&rel.to_le_bytes());
        }
        let calls = core::mem::take(&mut self.call_fixups);
        for (p, name, line) in calls {
            let target = self.funcs.iter().find(|f| f.0 == name).map(|f| f.1)
                .ok_or_else(|| format!("linea {}: funcion '{}' no definida", line, name))?;
            let rel = (target as i64 - (p as i64 + 4)) as i32;
            self.code[p..p + 4].copy_from_slice(&rel.to_le_bytes());
        }
        Ok(())
    }

    /// Resuelve direcciones absolutas una vez conocida la disposición final.
    pub fn resolve_abs(&mut self, str_offsets: &[u64], bss_base: u64) {
        let fix = core::mem::take(&mut self.abs_fixups);
        for (p, a) in fix {
            let v = match a { Abs::Str(i) => str_offsets[i], Abs::Global(off) => bss_base + off as u64 };
            self.code[p..p + 8].copy_from_slice(&v.to_le_bytes());
        }
    }

    fn gen_func(&mut self, f: &Func) -> GResult<()> {
        if self.funcs.iter().any(|g| g.0 == f.name) { return Err(format!("linea {}: funcion '{}' duplicada", f.line, f.name)); }
        self.cur_line = f.line;
        self.funcs.push((f.name.clone(), self.pos()));
        self.stats_funcs += 1;
        self.locals.clear();
        self.scopes.clear();
        self.stack_size = 0;
        self.ret_label = self.new_label();
        self.emit(&[0x55, 0x48, 0x89, 0xE5]); // push rbp; mov rbp, rsp
        self.emit(&[0x48, 0x81, 0xEC]);       // sub rsp, imm32 (se parchea)
        let sub_pos = self.pos();
        self.emit32(0);
        for (i, (ty, name)) in f.params.iter().enumerate() {
            let off = self.add_local(name, ty.clone());
            self.emit(ARG_REGS_STORE[i]);
            self.emit32(off);
        }
        for s in &f.body { self.gen_stmt(s)?; }
        self.mov_rax_imm(0);
        self.place(self.ret_label);
        self.emit(&[0x48, 0x89, 0xEC, 0x5D, 0xC3]); // mov rsp, rbp; pop rbp; ret
        let frame = ((self.stack_size + 15) / 16 * 16) as i32;
        self.code[sub_pos..sub_pos + 4].copy_from_slice(&frame.to_le_bytes());
        let ini = self.funcs.last().map(|f| f.1).unwrap_or(0);
        self.syms.push((f.name.clone(), ini, self.pos()));
        Ok(())
    }

    // ── Sentencias ──────────────────────────────────────────────────────
    fn gen_stmt(&mut self, s: &Stmt) -> GResult<()> {
        match s {
            Stmt::Expr(e) => { self.gen_expr(e)?; }
            Stmt::Decl(ty, name, init) => {
                if *ty == Ty::Void { return self.err("variable de tipo void"); }
                if ty.size() == 0 { return self.err(&format!("variable '{}' de struct incompleto", name)); }
                let scope_start = *self.scopes.last().unwrap_or(&0);
                if self.locals[scope_start..].iter().any(|l| &l.name == name) {
                    return self.err(&format!("variable '{}' redeclarada en el mismo ambito", name));
                }
                self.add_local(name, ty.clone());
                if let Some(e) = init {
                    self.gen_expr(&Expr::Assign(Box::new(Expr::Var(name.clone())), Box::new(e.clone())))?;
                }
            }
            Stmt::Multi(v) => { for s in v { self.gen_stmt(s)?; } }
            Stmt::Switch(e, body) => {
                // Pre-exploración de `case`/`default` en orden de aparición.
                let mut cases: Vec<(Option<i64>, usize)> = Vec::new();
                fn scan(g: &mut Gen, v: &[Stmt], out: &mut Vec<(Option<i64>, usize)>) {
                    for s in v {
                        match s {
                            Stmt::Case(k) => { let l = g.new_label(); out.push((Some(*k), l)); }
                            Stmt::Default => { let l = g.new_label(); out.push((None, l)); }
                            Stmt::Multi(w) | Stmt::Block(w) => scan(g, w, out),
                            Stmt::If(_, a, b) => { scan(g, core::slice::from_ref(&**a), out); if let Some(b) = b { scan(g, core::slice::from_ref(&**b), out); } }
                            Stmt::While(_, a) | Stmt::DoWhile(a, _) | Stmt::For(_, _, _, a) => scan(g, core::slice::from_ref(&**a), out),
                            _ => {}
                        }
                    }
                }
                scan(self, body, &mut cases);
                let l_end = self.new_label();
                self.gen_expr(e)?;
                let tmp = self.add_local("", Ty::Int);
                self.emit(&[0x48, 0x89, 0x85]); self.emit32(tmp); // mov [rbp+tmp], rax
                let mut default_label = l_end;
                let mut seen: Vec<i64> = Vec::new();
                for (k, l) in &cases {
                    match k {
                        Some(k) => {
                            if seen.contains(k) { return self.err(&format!("etiqueta 'case {}' duplicada", k)); }
                            seen.push(*k);
                            self.mov_rax_imm(*k);
                            self.emit(&[0x48, 0x89, 0xC7]);                  // mov rdi, rax
                            self.emit(&[0x48, 0x8B, 0x85]); self.emit32(tmp); // mov rax, [rbp+tmp]
                            self.emit(&[0x48, 0x39, 0xF8]);                  // cmp rax, rdi
                            self.je(*l);
                        }
                        None => { default_label = *l; }
                    }
                }
                self.jmp(default_label);
                let labels: Vec<usize> = cases.iter().map(|c| c.1).collect();
                self.switches.push((labels, 0));
                self.breaks.push(l_end);
                self.scopes.push(self.locals.len());
                for s in body { self.gen_stmt(s)?; }
                let n = self.scopes.pop().unwrap_or(0);
                self.locals.truncate(n);
                self.breaks.pop();
                self.switches.pop();
                self.place(l_end);
            }
            Stmt::Case(_) | Stmt::Default => {
                let (labels, cur) = match self.switches.last_mut() {
                    Some(sw) => (sw.0.clone(), { let c = sw.1; sw.1 += 1; c }),
                    None => return self.err("'case'/'default' fuera de un switch"),
                };
                match labels.get(cur) { Some(l) => self.place(*l), None => return self.err("case fuera de orden (error interno)") }
            }
            Stmt::Mark(l) => { if *l > 0 { self.cur_line = *l; } }
            Stmt::Block(v) => {
                self.scopes.push(self.locals.len());
                for s in v { self.gen_stmt(s)?; }
                let n = self.scopes.pop().unwrap_or(0);
                self.locals.truncate(n);
            }
            Stmt::If(c, t, e) => {
                let l_else = self.new_label();
                let l_end = self.new_label();
                self.gen_expr(c)?;
                self.test_rax();
                self.je(l_else);
                self.gen_stmt(t)?;
                self.jmp(l_end);
                self.place(l_else);
                if let Some(e) = e { self.gen_stmt(e)?; }
                self.place(l_end);
            }
            Stmt::While(c, body) => {
                let l_top = self.new_label();
                let l_end = self.new_label();
                self.place(l_top);
                self.gen_expr(c)?;
                self.test_rax();
                self.je(l_end);
                self.breaks.push(l_end);
                self.continues.push(l_top);
                self.gen_stmt(body)?;
                self.breaks.pop();
                self.continues.pop();
                self.jmp(l_top);
                self.place(l_end);
            }
            Stmt::DoWhile(body, c) => {
                let l_top = self.new_label();
                let l_cond = self.new_label();
                let l_end = self.new_label();
                self.place(l_top);
                self.breaks.push(l_end);
                self.continues.push(l_cond);
                self.gen_stmt(body)?;
                self.breaks.pop();
                self.continues.pop();
                self.place(l_cond);
                self.gen_expr(c)?;
                self.test_rax();
                self.jne(l_top);
                self.place(l_end);
            }
            Stmt::For(init, cond, step, body) => {
                self.scopes.push(self.locals.len());
                if let Some(i) = init { self.gen_stmt(i)?; }
                let l_top = self.new_label();
                let l_step = self.new_label();
                let l_end = self.new_label();
                self.place(l_top);
                if let Some(c) = cond { self.gen_expr(c)?; self.test_rax(); self.je(l_end); }
                self.breaks.push(l_end);
                self.continues.push(l_step);
                self.gen_stmt(body)?;
                self.breaks.pop();
                self.continues.pop();
                self.place(l_step);
                if let Some(s) = step { self.gen_expr(s)?; }
                self.jmp(l_top);
                self.place(l_end);
                let n = self.scopes.pop().unwrap_or(0);
                self.locals.truncate(n);
            }
            Stmt::Return(e) => {
                match e { Some(e) => { self.gen_expr(e)?; } None => self.mov_rax_imm(0) }
                self.jmp(self.ret_label);
            }
            Stmt::Break => {
                let l = *self.breaks.last().ok_or_else(|| format!("linea {}: 'break' fuera de un bucle", self.cur_line))?;
                self.jmp(l);
            }
            Stmt::Continue => {
                let l = *self.continues.last().ok_or_else(|| format!("linea {}: 'continue' fuera de un bucle", self.cur_line))?;
                self.jmp(l);
            }
        }
        Ok(())
    }

    // ── Expresiones ─────────────────────────────────────────────────────
    /// Deja en `rax` la dirección del lvalue y devuelve el tipo del objeto.
    fn gen_addr(&mut self, e: &Expr) -> GResult<Ty> {
        match e {
            Expr::Var(name) => {
                if let Some((ty, off)) = self.find_local(name) { self.lea_rax_rbp(off); return Ok(ty); }
                if let Some((ty, off)) = self.find_global(name) { self.mov_rax_abs(Abs::Global(off)); return Ok(ty); }
                self.err(&format!("variable '{}' no declarada", name))
            }
            Expr::Deref(inner) => {
                let t = self.gen_expr(inner)?;
                if !t.is_ptr_like() { return self.err("se desreferencia un valor que no es puntero"); }
                Ok(t.elem())
            }
            Expr::Str(i) => { self.mov_rax_abs(Abs::Str(*i)); Ok(Ty::Ptr(Box::new(Ty::Char))) }
            Expr::Member(base, m) => {
                let t = self.gen_addr(base)?;
                let idx = match t { Ty::Struct(i, _) => i, _ => return self.err(&format!("acceso a miembro '{}' sobre algo que no es struct", m)) };
                let (mty, off) = match self.structs[idx].members.iter().find(|x| &x.0 == m) {
                    Some(x) => (x.1.clone(), x.2),
                    None => return self.err(&format!("struct '{}' no tiene el miembro '{}'", self.structs[idx].name, m)),
                };
                if off != 0 { self.emit(&[0x48, 0x05]); self.emit32(off as i32); } // add rax, off
                Ok(mty)
            }
            _ => self.err("la expresion no es un lvalue asignable"),
        }
    }

    fn scale_rdi(&mut self, n: usize) {
        if n != 1 { self.emit(&[0x48, 0x69, 0xFF]); self.emit32(n as i32); } // imul rdi, rdi, imm32
    }
    fn scale_rax(&mut self, n: usize) {
        if n != 1 { self.emit(&[0x48, 0x69, 0xC0]); self.emit32(n as i32); } // imul rax, rax, imm32
    }

    /// Operación binaria con `rax = lhs`, `rdi = rhs`. Devuelve el tipo resultante.
    fn binop(&mut self, op: &str, tl: &Ty, tr: &Ty) -> GResult<Ty> {
        let (pl, pr) = (tl.is_ptr_like(), tr.is_ptr_like());
        match op {
            "+" => {
                if pl && pr { return self.err("suma de dos punteros"); }
                if pl { self.scale_rdi(tl.elem().size()); }
                if pr { self.scale_rax(tr.elem().size()); }
                self.emit(&[0x48, 0x01, 0xF8]);
                Ok(if pl { tl.decay() } else if pr { tr.decay() } else { Ty::Int })
            }
            "-" => {
                if pl && pr {
                    self.emit(&[0x48, 0x29, 0xF8]);
                    let sz = tl.elem().size();
                    if sz != 1 {
                        self.emit(&[0x48, 0xC7, 0xC7]); self.emit32(sz as i32); // mov rdi, sz
                        self.emit(&[0x48, 0x99, 0x48, 0xF7, 0xFF]);            // cqo; idiv rdi
                    }
                    return Ok(Ty::Int);
                }
                if pl { self.scale_rdi(tl.elem().size()); }
                self.emit(&[0x48, 0x29, 0xF8]);
                Ok(if pl { tl.decay() } else { Ty::Int })
            }
            "*" => { self.emit(&[0x48, 0x0F, 0xAF, 0xC7]); Ok(Ty::Int) }
            "/" => { self.emit(&[0x48, 0x99, 0x48, 0xF7, 0xFF]); Ok(Ty::Int) }
            "%" => { self.emit(&[0x48, 0x99, 0x48, 0xF7, 0xFF, 0x48, 0x89, 0xD0]); Ok(Ty::Int) }
            "&" => { self.emit(&[0x48, 0x21, 0xF8]); Ok(Ty::Int) }
            "|" => { self.emit(&[0x48, 0x09, 0xF8]); Ok(Ty::Int) }
            "^" => { self.emit(&[0x48, 0x31, 0xF8]); Ok(Ty::Int) }
            "<<" => { self.emit(&[0x48, 0x89, 0xF9, 0x48, 0xD3, 0xE0]); Ok(Ty::Int) }
            ">>" => { self.emit(&[0x48, 0x89, 0xF9, 0x48, 0xD3, 0xF8]); Ok(Ty::Int) }
            "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                self.emit(&[0x48, 0x39, 0xF8]); // cmp rax, rdi
                let cc = match op { "==" => 0x94, "!=" => 0x95, "<" => 0x9C, "<=" => 0x9E, ">" => 0x9F, _ => 0x9D };
                self.bool_from_flags(cc);
                Ok(Ty::Int)
            }
            _ => self.err("operador binario desconocido"),
        }
    }

    /// Genera `e`, deja el valor en `rax` y devuelve su tipo.
    pub fn gen_expr(&mut self, e: &Expr) -> GResult<Ty> {
        match e {
            Expr::Num(v) => { self.mov_rax_imm(*v); Ok(Ty::Int) }
            Expr::Str(i) => { self.mov_rax_abs(Abs::Str(*i)); Ok(Ty::Ptr(Box::new(Ty::Char))) }
            Expr::Var(_) => {
                let t = self.gen_addr(e)?;
                self.load(&t);
                Ok(t)
            }
            Expr::Deref(_) | Expr::Member(_, _) => {
                let t = self.gen_addr(e)?;
                self.load(&t);
                Ok(t)
            }
            Expr::Addr(inner) => {
                let t = self.gen_addr(inner)?;
                Ok(Ty::Ptr(Box::new(t)))
            }
            Expr::Unary(op, inner) => {
                self.gen_expr(inner)?;
                match *op {
                    "-" => self.emit(&[0x48, 0xF7, 0xD8]),
                    "~" => self.emit(&[0x48, 0xF7, 0xD0]),
                    _ => { self.emit(&[0x48, 0x83, 0xF8, 0x00]); self.bool_from_flags(0x94); }
                }
                Ok(Ty::Int)
            }
            Expr::Binary(op, l, r) => {
                if *op == "&&" || *op == "||" {
                    let l_short = self.new_label();
                    let l_end = self.new_label();
                    self.gen_expr(l)?;
                    self.test_rax();
                    if *op == "&&" { self.je(l_short); } else { self.jne(l_short); }
                    self.gen_expr(r)?;
                    self.test_rax();
                    self.bool_from_flags(0x95);
                    self.jmp(l_end);
                    self.place(l_short);
                    self.mov_rax_imm(if *op == "&&" { 0 } else { 1 });
                    self.place(l_end);
                    return Ok(Ty::Int);
                }
                let tr = self.gen_expr(r)?;
                self.push_rax();
                let tl = self.gen_expr(l)?;
                self.pop_rdi();
                self.binop(op, &tl, &tr)
            }
            Expr::Assign(l, r) => {
                let tr = self.gen_expr(r)?;
                self.push_rax();
                let tl = self.gen_addr(l)?;
                if matches!(tl, Ty::Arr(_, _)) { return self.err("no se puede asignar a un arreglo completo"); }
                if let Ty::Struct(si, sz) = tl.clone() {
                    // [PCC 1.2] Asignacion de struct/union por valor: copia de `sz`
                    // bytes (rax = destino, pila = origen) con `rep movsb`.
                    match tr { Ty::Struct(sj, _) if sj == si => {}, _ => return self.err("asignacion de struct con un valor de otro tipo") }
                    self.emit(&[0x48, 0x89, 0xC7]);               // mov rdi, rax
                    self.emit(&[0x5E]);                           // pop rsi
                    self.emit(&[0x48, 0xC7, 0xC1]); self.emit32(sz as i32); // mov rcx, sz
                    self.emit(&[0x57, 0xF3, 0xA4, 0x58]);         // push rdi; rep movsb; pop rax
                    return Ok(tl);
                }
                self.emit(&[0x48, 0x89, 0xC7]); // mov rdi, rax
                self.emit(&[0x58]);             // pop rax
                self.store_rdi(&tl);
                Ok(tl)
            }
            Expr::OpAssign(op, l, r) => {
                let tr = self.gen_expr(r)?;
                self.push_rax();
                let tl = self.gen_addr(l)?;
                self.emit(&[0x49, 0x89, 0xC0]); // mov r8, rax
                self.load(&tl);
                self.pop_rdi();
                let t = self.binop(op, &tl, &tr)?;
                self.store_r8(&tl);
                Ok(t)
            }
            Expr::IncDec(d, post, inner) => {
                let tl = self.gen_addr(inner)?;
                self.emit(&[0x49, 0x89, 0xC0]); // mov r8, rax
                self.load(&tl);
                self.push_rax();
                let step = if tl.is_ptr_like() { tl.elem().size() as i64 } else { 1 } * d;
                self.emit(&[0x48, 0x05]); self.emit32(step as i32); // add rax, imm32
                self.store_r8(&tl);
                if *post { self.emit(&[0x58]); } else { self.emit(&[0x48, 0x83, 0xC4, 0x08]); }
                Ok(tl)
            }
            Expr::Cond(c, a, b) => {
                let l_else = self.new_label();
                let l_end = self.new_label();
                self.gen_expr(c)?;
                self.test_rax();
                self.je(l_else);
                let ta = self.gen_expr(a)?;
                self.jmp(l_end);
                self.place(l_else);
                self.gen_expr(b)?;
                self.place(l_end);
                Ok(ta)
            }
            Expr::Call(name, args) => {
                if args.len() > 6 { return self.err("mas de 6 argumentos en una llamada (limite de PCC)"); }
                for a in args { self.gen_expr(a)?; self.push_rax(); }
                for i in (0..args.len()).rev() { self.emit(ARG_REGS_POP[i]); }
                self.call(name);
                Ok(self.func_ret(name))
            }
            Expr::Seq(v) => {
                let mut t = Ty::Int;
                for e in v { t = self.gen_expr(e)?; }
                Ok(t)
            }
            Expr::Sizeof(t) => { self.mov_rax_imm(t.size() as i64); Ok(Ty::Int) }
            Expr::SizeofExpr(inner) => {
                // Se calcula el tipo generando el código a un búfer descartable.
                let saved = self.code.len();
                let saved_fix = (self.label_fixups.len(), self.call_fixups.len(), self.abs_fixups.len(), self.labels.len());
                let t = self.gen_expr(inner)?;
                self.code.truncate(saved);
                self.label_fixups.truncate(saved_fix.0);
                self.call_fixups.truncate(saved_fix.1);
                self.abs_fixups.truncate(saved_fix.2);
                self.labels.truncate(saved_fix.3);
                self.mov_rax_imm(t.size() as i64);
                Ok(Ty::Int)
            }
            Expr::Cast(t, inner) => {
                self.gen_expr(inner)?;
                if *t == Ty::Char { self.emit(&[0x48, 0x0F, 0xBE, 0xC0]); } // movsx rax, al
                Ok(t.clone())
            }
        }
    }
}

/// Runtime mínimo escrito en el propio subconjunto de C y compilado junto
/// al programa del usuario (sólo las funciones que el usuario no redefine).
pub const PRELUDIO: &[(&str, &str)] = &[
    ("__out", "int __out(char *s, int n) { return __syscall(1, 1, s, n, 0); }"),
    ("strlen", "int strlen(char *s) { int n = 0; while (s[n]) n++; return n; }"),
    ("__outs", "int __outs(char *s) { if (!s) s = \"(null)\"; return __out(s, strlen(s)); }"),
    ("putchar", "int putchar(int c) { char b[1]; b[0] = c; __out(b, 1); return c; }"),
    ("puts", "int puts(char *s) { __outs(s); putchar(10); return 0; }"),
    ("__printi", "int __printi(int v) { char b[24]; int i = 23; int neg = 0; if (v < 0) { neg = 1; v = 0 - v; } if (v == 0) { b[i] = 48; i--; } while (v > 0) { b[i] = 48 + v % 10; v = v / 10; i--; } if (neg) { b[i] = 45; i--; } return __out(b + i + 1, 23 - i); }"),
    ("__printx", "int __printx(int v) { char b[16]; int i = 15; int k = 0; if (v == 0) { b[i] = 48; i--; } while (k < 16 && v != 0) { int d = v & 15; b[i] = d < 10 ? 48 + d : 87 + d; v = v >> 4; if (k == 15) v = 0; i--; k++; } return __out(b + i + 1, 15 - i); }"),
    ("exit", "void exit(int c) { __syscall(0, c, 0, 0, 0); }"),
    ("getpid", "int getpid() { return __syscall(2, 0, 0, 0, 0); }"),
    ("write", "int write(int fd, char *b, int n) { return __syscall(1, fd, b, n, 0); }"),
    ("read", "int read(int fd, char *b, int n) { return __syscall(5, fd, b, n, 0); }"),
    ("strcmp", "int strcmp(char *a, char *b) { int i = 0; while (a[i] && a[i] == b[i]) i++; return a[i] - b[i]; }"),
    ("strcpy", "char *strcpy(char *d, char *s) { int i = 0; while (s[i]) { d[i] = s[i]; i++; } d[i] = 0; return d; }"),
    ("memset", "char *memset(char *d, int c, int n) { int i = 0; while (i < n) { d[i] = c; i++; } return d; }"),
    ("atoi", "int atoi(char *s) { int v = 0; int neg = 0; while (*s == 32) s++; if (*s == 45) { neg = 1; s++; } while (*s >= 48 && *s <= 57) { v = v * 10 + (*s - 48); s++; } return neg ? 0 - v : v; }"),
    ("abs", "int abs(int v) { return v < 0 ? 0 - v : v; }"),
    ("malloc", "char __pcc_heap[65536]; int __pcc_heap_pos; char *malloc(int n) { n = (n + 15) & ~15; if (__pcc_heap_pos + n > 65536) return 0; char *p = __pcc_heap + __pcc_heap_pos; __pcc_heap_pos += n; return p; }"),
    ("free", "void free(char *p) { }"),
];
