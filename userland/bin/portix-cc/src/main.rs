//! PCC — Portix C Compiler. Compilador real de un subconjunto de C escrito
//! en Rust `no_std` que genera código máquina x86-64 y ejecutables ELF64
//! nativos de Portix (carga en 0x20000000, syscalls `int 0x80`).
//!
//! No depende de ningún ensamblador, enlazador ni libc: el runtime mínimo
//! (`putchar`, `puts`, `printf` expandido, `strlen`, `malloc`…) está escrito
//! en el mismo subconjunto de C y se compila junto al programa del usuario.

#![no_std]
#![no_main]

extern crate alloc;

mod lexer;
mod parser;
mod codegen;

use alloc::string::String;
use alloc::vec::Vec;
use portix_rt::{println, syscall};
use lexer::{Tok, Token};

const VERSION: &str = "1.2.0";

fn get_arg(idx: usize, argc: i32, argv: *const *const u8) -> Option<String> {
    if idx >= argc as usize || argv.is_null() { return None; }
    unsafe {
        let ptr = *argv.add(idx);
        if ptr.is_null() { return None; }
        let mut len = 0;
        while *ptr.add(len) != 0 { len += 1; }
        core::str::from_utf8(core::slice::from_raw_parts(ptr, len)).ok().map(String::from)
    }
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let mut p = Vec::with_capacity(path.len() + 1);
    p.extend_from_slice(path.as_bytes());
    p.push(0);
    let fd = syscall::sys_open(p.as_ptr(), 0);
    if fd < 0 { return None; }
    let mut content = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = syscall::sys_read(fd as i32, buf.as_mut_ptr(), buf.len());
        if n <= 0 { break; }
        content.extend_from_slice(&buf[..n as usize]);
    }
    syscall::sys_close(fd as i32);
    Some(content)
}

fn write_file(path: &str, data: &[u8]) -> bool {
    let mut p = Vec::with_capacity(path.len() + 1);
    p.extend_from_slice(path.as_bytes());
    p.push(0);
    // O_WRONLY | O_CREAT | O_TRUNC
    let fd = syscall::sys_open(p.as_ptr(), 0x241);
    if fd < 0 { return false; }
    let mut off = 0usize;
    while off < data.len() {
        let n = syscall::sys_write(fd as i32, data[off..].as_ptr(), data.len() - off);
        if n <= 0 { break; }
        off += n as usize;
    }
    syscall::sys_close(fd as i32);
    off == data.len()
}

/// Nombres de funciones definidas (con cuerpo) en los tokens del usuario.
fn funciones_definidas(toks: &[Token]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < toks.len() {
        if let (Tok::Ident(name), Tok::Punct("(")) = (&toks[i].tok, &toks[i + 1].tok) {
            let mut depth = 0;
            let mut j = i + 1;
            while j < toks.len() {
                match toks[j].tok { Tok::Punct("(") => depth += 1, Tok::Punct(")") => { depth -= 1; if depth == 0 { break; } }, _ => {} }
                j += 1;
            }
            if j + 1 < toks.len() && toks[j + 1].tok == Tok::Punct("{") { out.push(name.clone()); }
        }
        i += 1;
    }
    out
}

struct Salida { elf: Vec<u8>, code: usize, datos: usize, bss: usize, funcs: usize, strings: usize, globals: usize }

fn compilar(src: &[u8]) -> Result<Salida, String> {
    let mut toks = lexer::tokenize(src)?;
    let definidas = funciones_definidas(&toks);
    toks.pop(); // Eof del usuario
    for (name, code) in codegen::PRELUDIO {
        if definidas.iter().any(|d| d == name) { continue; }
        let mut t = lexer::tokenize(code.as_bytes())?;
        t.pop();
        for tk in t.iter_mut() { tk.line = 0; }
        toks.extend(t);
    }
    toks.push(Token { tok: Tok::Eof, line: 0 });

    let mut parser = parser::Parser::new(toks);
    parser.parse_program()?;
    let prog = parser.prog;
    if !prog.funcs.iter().any(|f| f.name == "main") {
        return Err(String::from("el programa no define 'main'"));
    }

    let mut gen = codegen::Gen::new(codegen::PUENTE_PORTIX, prog.structs.clone());
    gen.gen_program(&prog)?;

    // Disposición final: [cabecera ELF 120 B][código][cadenas][bss]
    let code_len = gen.code.len() as u64;
    let mut datos: Vec<u8> = Vec::new();
    let mut str_addrs = Vec::with_capacity(prog.strings.len());
    for s in &prog.strings {
        str_addrs.push(codegen::BASE + codegen::HDR + code_len + datos.len() as u64);
        datos.extend_from_slice(s);
        datos.push(0);
    }
    while datos.len() % 16 != 0 { datos.push(0); }
    let bss_base = codegen::BASE + codegen::HDR + code_len + datos.len() as u64;
    gen.resolve_abs(&str_addrs, bss_base);

    let filesz = codegen::HDR + code_len + datos.len() as u64;
    let memsz = filesz + ((gen.bss_size as u64 + 15) & !15) + 16;

    let mut elf = Vec::with_capacity(filesz as usize);
    elf.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0]);
    elf.extend_from_slice(&[0u8; 8]);
    elf.extend_from_slice(&2u16.to_le_bytes());     // ET_EXEC
    elf.extend_from_slice(&0x3Eu16.to_le_bytes());  // x86-64
    elf.extend_from_slice(&1u32.to_le_bytes());
    elf.extend_from_slice(&(codegen::BASE + codegen::HDR).to_le_bytes()); // entry = _start
    elf.extend_from_slice(&64u64.to_le_bytes());    // phoff
    elf.extend_from_slice(&0u64.to_le_bytes());     // shoff
    elf.extend_from_slice(&0u32.to_le_bytes());     // flags
    elf.extend_from_slice(&64u16.to_le_bytes());
    elf.extend_from_slice(&56u16.to_le_bytes());
    elf.extend_from_slice(&1u16.to_le_bytes());
    elf.extend_from_slice(&0u16.to_le_bytes());
    elf.extend_from_slice(&0u16.to_le_bytes());
    elf.extend_from_slice(&0u16.to_le_bytes());
    // PT_LOAD RWX
    elf.extend_from_slice(&1u32.to_le_bytes());
    elf.extend_from_slice(&7u32.to_le_bytes());
    elf.extend_from_slice(&0u64.to_le_bytes());
    elf.extend_from_slice(&codegen::BASE.to_le_bytes());
    elf.extend_from_slice(&codegen::BASE.to_le_bytes());
    elf.extend_from_slice(&filesz.to_le_bytes());
    elf.extend_from_slice(&memsz.to_le_bytes());
    elf.extend_from_slice(&0x1000u64.to_le_bytes());
    debug_assert!(elf.len() as u64 == codegen::HDR);
    elf.extend_from_slice(&gen.code);
    elf.extend_from_slice(&datos);
    anexar_simbolos(&mut elf, code_len, &gen.syms);

    Ok(Salida { elf, code: code_len as usize, datos: datos.len(), bss: gen.bss_size, funcs: gen.stats_funcs, strings: prog.strings.len(), globals: prog.globals.len() })
}

/// Añade `.symtab`/`.strtab`/`.shstrtab` y las cabeceras de sección al final
/// del archivo (fuera del segmento cargado) para que el núcleo registre los
/// símbolos de función (`[SYMS]`) y `/bin/dbg` pueda resolver nombres C.
fn anexar_simbolos(elf: &mut Vec<u8>, code_len: u64, syms: &[(String, usize, usize)]) {
    fn sh(out: &mut Vec<u8>, name: u32, ty: u32, flags: u64, addr: u64, off: u64, size: u64, link: u32, info: u32, align: u64, entsize: u64) {
        out.extend_from_slice(&name.to_le_bytes()); out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes()); out.extend_from_slice(&addr.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes()); out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&link.to_le_bytes()); out.extend_from_slice(&info.to_le_bytes());
        out.extend_from_slice(&align.to_le_bytes()); out.extend_from_slice(&entsize.to_le_bytes());
    }
    let mut strtab: Vec<u8> = alloc::vec![0];
    let mut symtab: Vec<u8> = alloc::vec![0; 24];
    for (name, ini, fin) in syms {
        let name_off = strtab.len() as u32;
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
        symtab.extend_from_slice(&name_off.to_le_bytes());
        symtab.push(0x12); // STB_GLOBAL | STT_FUNC
        symtab.push(0);
        symtab.extend_from_slice(&1u16.to_le_bytes()); // .text
        symtab.extend_from_slice(&(codegen::BASE + codegen::HDR + *ini as u64).to_le_bytes());
        symtab.extend_from_slice(&((*fin - *ini) as u64).to_le_bytes());
    }
    let shstrtab: &[u8] = b"\0.text\0.symtab\0.strtab\0.shstrtab\0";
    while elf.len() % 8 != 0 { elf.push(0); }
    let off_symtab = elf.len() as u64; elf.extend_from_slice(&symtab);
    let off_strtab = elf.len() as u64; elf.extend_from_slice(&strtab);
    let off_shstr = elf.len() as u64; elf.extend_from_slice(shstrtab);
    while elf.len() % 8 != 0 { elf.push(0); }
    let shoff = elf.len() as u64;
    sh(elf, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    sh(elf, 1, 1, 6, codegen::BASE + codegen::HDR, codegen::HDR, code_len, 0, 0, 16, 0);              // .text PROGBITS AX
    sh(elf, 7, 2, 0, 0, off_symtab, symtab.len() as u64, 3, 1, 8, 24);                                 // .symtab (link=.strtab)
    sh(elf, 15, 3, 0, 0, off_strtab, strtab.len() as u64, 0, 0, 1, 0);                                 // .strtab
    sh(elf, 23, 3, 0, 0, off_shstr, shstrtab.len() as u64, 0, 0, 1, 0);                                // .shstrtab
    elf[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
    elf[0x3A..0x3C].copy_from_slice(&64u16.to_le_bytes());
    elf[0x3C..0x3E].copy_from_slice(&5u16.to_le_bytes());
    elf[0x3E..0x40].copy_from_slice(&4u16.to_le_bytes());
}

fn ayuda() {
    println!("PCC - Portix C Compiler v{} (100% Rust, sin ensamblador ni libc externos)", VERSION);
    println!("Uso: portix-cc <fuente.c> [-o salida]      (por defecto /tmp/a.out)");
    println!("     portix-cc -v | --help");
    println!("Subconjunto de C soportado:");
    println!("  tipos int/long (64 bits), char, void, punteros, arreglos, struct (. y ->), typedef, enum");
    println!("  #define de constantes, switch/case/default, inicializadores {{...}} y char s[] = \"...\"");
    println!("  funciones (hasta 6 parametros), recursion, variables globales y locales");
    println!("  if/else, while, do/while, for, break, continue, return, bloques");
    println!("  expresiones completas: aritmetica, logica, comparaciones, ?:, ++/--, op=");
    println!("  printf con formato literal (%d %s %c %x %%), puts, putchar, strlen, strcmp,");
    println!("  strcpy, memset, atoi, abs, malloc/free (monticulo de 64 KiB), write, read,");
    println!("  getpid, exit. No soporta: float, union, macros con parametros, variadicas, struct por valor.");
    println!("Emite .symtab: el nucleo y /bin/dbg resuelven los nombres de las funciones C.");
    println!("Salida: ELF64 estatico nativo de Portix (int 0x80), ejecutable con `run /ruta`.");
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let a1 = get_arg(1, argc, argv);
    match a1.as_deref() {
        None | Some("--help") | Some("-h") => { ayuda(); return if a1.is_none() { 1 } else { 0 }; }
        Some("-v") | Some("--version") => {
            println!("Portix Native Compiler PCC v{} (100% Rust) - objetivo x86_64-portix-elf", VERSION);
            println!("Compilador real: lexico + sintactico + generador de codigo maquina x86-64 + escritor ELF64");
            return 0;
        }
        _ => {}
    }
    let mut src_path: Option<String> = None;
    let mut out_path = String::from("/tmp/a.out");
    let mut i = 1;
    while let Some(a) = get_arg(i, argc, argv) {
        if a == "-o" {
            match get_arg(i + 1, argc, argv) { Some(o) => { out_path = o; i += 2; continue; } None => { println!("[PCC] error: -o requiere un nombre de salida"); return 1; } }
        }
        if a.starts_with('-') { i += 1; continue; } // -O2, -Wall… se aceptan e ignoran
        src_path = Some(a);
        i += 1;
    }
    let src_path = match src_path { Some(p) => p, None => { ayuda(); return 1; } };
    let src = match read_file(&src_path) {
        Some(s) => s,
        None => { println!("[PCC] error: no se pudo abrir '{}'", src_path); return 1; }
    };
    let lineas = src.iter().filter(|&&b| b == b'\n').count();
    println!("[PCC] Compilando {} ({} lineas, {} bytes)", src_path, lineas, src.len());
    match compilar(&src) {
        Ok(s) => {
            println!("[PCC] funciones={} globales={} cadenas={} | codigo x86-64={} B datos={} B bss={} B", s.funcs, s.globals, s.strings, s.code, s.datos, s.bss);
            if write_file(&out_path, &s.elf) {
                println!("[PCC] Binario ELF64 generado: {} ({} bytes)", out_path, s.elf.len());
                0
            } else {
                println!("[PCC] error: no se pudo escribir '{}'", out_path);
                1
            }
        }
        Err(e) => {
            println!("[PCC] {}: error: {}", src_path, e);
            1
        }
    }
}
