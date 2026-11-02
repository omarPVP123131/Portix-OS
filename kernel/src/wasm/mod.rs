// kernel/src/wasm/mod.rs — Motor Runtime WebAssembly (WASM) en 100% Rust Puro (no_std)
#![allow(dead_code)]

pub mod component_model;
pub use component_model::*;

use alloc::string::String;
use alloc::vec::Vec;

pub const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6D]; // "\0asm"
pub const WASM_VERSION_1: u32 = 1;

pub const SEC_TYPE: u8 = 1;
pub const SEC_IMPORT: u8 = 2;
pub const SEC_FUNCTION: u8 = 3;
pub const SEC_TABLE: u8 = 4;
pub const SEC_MEMORY: u8 = 5;
pub const SEC_GLOBAL: u8 = 6;
pub const SEC_EXPORT: u8 = 7;
pub const SEC_START: u8 = 8;
pub const SEC_ELEMENT: u8 = 9;
pub const SEC_CODE: u8 = 10;
pub const SEC_DATA: u8 = 11;

// Opcodes estándar WASM MVP
pub const OP_UNREACHABLE: u8 = 0x00;
pub const OP_NOP: u8         = 0x01;
pub const OP_BLOCK: u8       = 0x02;
pub const OP_LOOP: u8        = 0x03;
pub const OP_IF: u8          = 0x04;
pub const OP_ELSE: u8        = 0x05;
pub const OP_END: u8         = 0x0B;
pub const OP_BR: u8          = 0x0C;
pub const OP_BR_IF: u8       = 0x0D;
pub const OP_RETURN: u8      = 0x0F;
pub const OP_CALL: u8        = 0x10;
pub const OP_DROP: u8        = 0x1A;
pub const OP_SELECT: u8      = 0x1B;
pub const OP_LOCAL_GET: u8   = 0x20;
pub const OP_LOCAL_SET: u8   = 0x21;
pub const OP_LOCAL_TEE: u8   = 0x22;
pub const OP_GLOBAL_GET: u8  = 0x23;
pub const OP_GLOBAL_SET: u8  = 0x24;
pub const OP_I32_LOAD: u8    = 0x28;
pub const OP_I32_STORE: u8   = 0x36;
pub const OP_I32_CONST: u8   = 0x41;
pub const OP_I64_CONST: u8   = 0x42;
pub const OP_I32_EQZ: u8     = 0x45;
pub const OP_I32_EQ: u8      = 0x46;
pub const OP_I32_NE: u8      = 0x47;
pub const OP_I32_LT_S: u8    = 0x48;
pub const OP_I32_GT_S: u8    = 0x4A;
pub const OP_I32_LE_S: u8    = 0x4C;
pub const OP_I32_GE_S: u8    = 0x4E;
pub const OP_I32_ADD: u8     = 0x6A;
pub const OP_I32_SUB: u8     = 0x6B;
pub const OP_I32_MUL: u8     = 0x6C;
pub const OP_I32_DIV_S: u8   = 0x6D;
pub const OP_I32_REM_S: u8   = 0x6F;
pub const OP_I32_AND: u8     = 0x71;
pub const OP_I32_OR: u8      = 0x72;
pub const OP_I32_XOR: u8     = 0x73;
pub const OP_I32_SHL: u8     = 0x74;
pub const OP_I32_SHR_S: u8   = 0x75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
}

#[derive(Clone, Debug)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub returns: Vec<ValType>,
}

#[derive(Clone, Debug)]
pub struct WasmExport {
    pub name: String,
    pub kind: u8,
    pub index: u32,
}

#[derive(Clone, Debug)]
pub struct WasmFunction {
    pub type_index: u32,
    pub locals: Vec<ValType>,
    pub code: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct WasmModule {
    pub types: Vec<FuncType>,
    pub functions: Vec<WasmFunction>,
    pub exports: Vec<WasmExport>,
    pub memory_pages: Option<u32>,
}

// Lector de LEB128 (Little-Endian Base 128)
pub struct Leb128Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Leb128Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn set_pos(&mut self, p: usize) {
        self.pos = p;
    }

    pub fn has_more(&self) -> bool {
        self.pos < self.data.len()
    }

    pub fn read_u8(&mut self) -> Result<u8, &'static str> {
        if self.pos >= self.data.len() {
            return Err("EOF inesperado en byte WASM");
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn read_u32_leb128(&mut self) -> Result<u32, &'static str> {
        let mut result: u32 = 0;
        let mut shift = 0;
        loop {
            let byte = self.read_u8()?;
            result |= ((byte & 0x7F) as u32) << shift;
            if (byte & 0x80) == 0 {
                break;
            }
            shift += 7;
            if shift >= 35 {
                return Err("LEB128 u32 desbordado");
            }
        }
        Ok(result)
    }

    pub fn read_i32_leb128(&mut self) -> Result<i32, &'static str> {
        let mut result: i32 = 0;
        let mut shift = 0;
        let mut byte: u8;
        loop {
            byte = self.read_u8()?;
            result |= ((byte & 0x7F) as i32) << shift;
            shift += 7;
            if (byte & 0x80) == 0 {
                break;
            }
            if shift >= 35 {
                return Err("LEB128 i32 desbordado");
            }
        }
        if shift < 32 && (byte & 0x40) != 0 {
            result |= !0 << shift;
        }
        Ok(result)
    }

    pub fn read_bytes(&mut self, count: usize) -> Result<&'a [u8], &'static str> {
        if self.pos + count > self.data.len() {
            return Err("EOF leyendo buffer de bytes");
        }
        let slice = &self.data[self.pos..self.pos + count];
        self.pos += count;
        Ok(slice)
    }

    pub fn read_string(&mut self) -> Result<String, &'static str> {
        let len = self.read_u32_leb128()? as usize;
        let bytes = self.read_bytes(len)?;
        let s = core::str::from_utf8(bytes).map_err(|_| "Cadena UTF-8 invalida en WASM")?;
        Ok(String::from(s))
    }
}

impl WasmModule {
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 8 {
            return Err("Tamano de binario WASM demasiado pequeno");
        }
        if &bytes[0..4] != &WASM_MAGIC {
            return Err("Firma magica WASM invalida (se esperaba \\0asm)");
        }
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != WASM_VERSION_1 {
            return Err("Version WASM no soportada (solo v1)");
        }

        let mut reader = Leb128Reader::new(&bytes[8..]);
        let mut types = Vec::new();
        let mut func_type_indices = Vec::new();
        let mut exports = Vec::new();
        let mut codes = Vec::new();
        let mut memory_pages = None;

        while reader.has_more() {
            let sec_id = reader.read_u8()?;
            let sec_len = reader.read_u32_leb128()? as usize;
            let sec_end = reader.pos() + sec_len;

            match sec_id {
                SEC_TYPE => {
                    let count = reader.read_u32_leb128()?;
                    for _ in 0..count {
                        let form = reader.read_u8()?;
                        if form != 0x60 {
                            return Err("Tipo de funcion no soportado (esperado 0x60)");
                        }
                        let param_count = reader.read_u32_leb128()?;
                        let mut params = Vec::new();
                        for _ in 0..param_count {
                            let vt = match reader.read_u8()? {
                                0x7F => ValType::I32,
                                0x7E => ValType::I64,
                                0x7D => ValType::F32,
                                0x7C => ValType::F64,
                                _ => return Err("Tipo de parametro no soportado"),
                            };
                            params.push(vt);
                        }
                        let ret_count = reader.read_u32_leb128()?;
                        let mut returns = Vec::new();
                        for _ in 0..ret_count {
                            let vt = match reader.read_u8()? {
                                0x7F => ValType::I32,
                                0x7E => ValType::I64,
                                0x7D => ValType::F32,
                                0x7C => ValType::F64,
                                _ => return Err("Tipo de retorno no soportado"),
                            };
                            returns.push(vt);
                        }
                        types.push(FuncType { params, returns });
                    }
                }
                SEC_FUNCTION => {
                    let count = reader.read_u32_leb128()?;
                    for _ in 0..count {
                        let t_idx = reader.read_u32_leb128()?;
                        func_type_indices.push(t_idx);
                    }
                }
                SEC_MEMORY => {
                    let count = reader.read_u32_leb128()?;
                    if count > 0 {
                        let flags = reader.read_u8()?;
                        let initial = reader.read_u32_leb128()?;
                        if (flags & 1) != 0 {
                            let _max = reader.read_u32_leb128()?;
                        }
                        memory_pages = Some(initial);
                    }
                }
                SEC_EXPORT => {
                    let count = reader.read_u32_leb128()?;
                    for _ in 0..count {
                        let name = reader.read_string()?;
                        let kind = reader.read_u8()?;
                        let index = reader.read_u32_leb128()?;
                        exports.push(WasmExport { name, kind, index });
                    }
                }
                SEC_CODE => {
                    let count = reader.read_u32_leb128()?;
                    for _ in 0..count {
                        let body_size = reader.read_u32_leb128()? as usize;
                        let body_end = reader.pos() + body_size;
                        let local_decl_count = reader.read_u32_leb128()?;
                        let mut locals = Vec::new();
                        for _ in 0..local_decl_count {
                            let l_count = reader.read_u32_leb128()?;
                            let l_type = match reader.read_u8()? {
                                0x7F => ValType::I32,
                                0x7E => ValType::I64,
                                0x7D => ValType::F32,
                                0x7C => ValType::F64,
                                _ => return Err("Tipo local invalido"),
                            };
                            for _ in 0..l_count {
                                locals.push(l_type);
                            }
                        }
                        let code_len = body_end - reader.pos();
                        let code_bytes = reader.read_bytes(code_len)?.to_vec();
                        codes.push((locals, code_bytes));
                    }
                }
                _ => {
                    // Saltar sección no crítica
                    reader.set_pos(sec_end);
                }
            }
            if reader.pos() != sec_end {
                reader.set_pos(sec_end);
            }
        }

        if func_type_indices.len() != codes.len() {
            return Err("Discrepancia entre declaracion de funciones y cuerpos de codigo");
        }

        let mut functions = Vec::new();
        for (i, t_idx) in func_type_indices.iter().enumerate() {
            let (locals, code) = codes[i].clone();
            functions.push(WasmFunction {
                type_index: *t_idx,
                locals,
                code,
            });
        }

        Ok(Self {
            types,
            functions,
            exports,
            memory_pages,
        })
    }
}

// Instancia en ejecución
pub struct WasmInstance {
    pub module: WasmModule,
    pub memory: Vec<u8>,
    pub globals: Vec<i32>,
}

impl WasmInstance {
    pub fn new(module: WasmModule) -> Self {
        let pages = module.memory_pages.unwrap_or(1) as usize;
        let memory = alloc::vec![0u8; pages * 65536]; // Cada página WASM = 64 KB
        Self {
            module,
            memory,
            globals: alloc::vec![0; 16],
        }
    }

    pub fn invoke(&mut self, name: &str, args: &[i32]) -> Result<Option<i32>, &'static str> {
        let export = self.module.exports.iter().find(|e| e.name == name && e.kind == 0)
            .ok_or("Funcion exportada no encontrada en el modulo WASM")?;
        let func_idx = export.index as usize;
        self.execute_func(func_idx, args)
    }

    pub fn execute_func(&mut self, func_idx: usize, args: &[i32]) -> Result<Option<i32>, &'static str> {
        if func_idx >= self.module.functions.len() {
            return Err("Indice de funcion fuera de rango");
        }

        let (code, num_locals) = {
            let func = &self.module.functions[func_idx];
            (func.code.clone(), func.locals.len())
        };

        let mut locals = Vec::new();
        for &arg in args {
            locals.push(arg);
        }
        for _ in 0..num_locals {
            locals.push(0);
        }

        let mut stack: Vec<i32> = Vec::new();
        let mut pc = 0;

        while pc < code.len() {
            let op = code[pc];
            pc += 1;

            match op {
                OP_NOP => {}
                OP_BLOCK | OP_LOOP => {
                    // Leer tipo de bloque (0x40 para vacio)
                    if pc < code.len() && code[pc] == 0x40 {
                        pc += 1;
                    }
                }
                OP_END => {
                    // Fin de bloque o funcion
                }
                OP_RETURN => {
                    break;
                }
                OP_DROP => {
                    let _ = stack.pop();
                }
                OP_LOCAL_GET => {
                    let mut reader = Leb128Reader::new(&code[pc..]);
                    let idx = reader.read_u32_leb128()? as usize;
                    pc += reader.pos();
                    if idx >= locals.len() {
                        return Err("local.get indice fuera de rango");
                    }
                    stack.push(locals[idx]);
                }
                OP_LOCAL_SET => {
                    let mut reader = Leb128Reader::new(&code[pc..]);
                    let idx = reader.read_u32_leb128()? as usize;
                    pc += reader.pos();
                    let val = stack.pop().ok_or("Pila vacia para local.set")?;
                    if idx >= locals.len() {
                        return Err("local.set indice fuera de rango");
                    }
                    locals[idx] = val;
                }
                OP_LOCAL_TEE => {
                    let mut reader = Leb128Reader::new(&code[pc..]);
                    let idx = reader.read_u32_leb128()? as usize;
                    pc += reader.pos();
                    let val = *stack.last().ok_or("Pila vacia para local.tee")?;
                    if idx >= locals.len() {
                        return Err("local.tee indice fuera de rango");
                    }
                    locals[idx] = val;
                }
                OP_I32_CONST => {
                    let mut reader = Leb128Reader::new(&code[pc..]);
                    let val = reader.read_i32_leb128()?;
                    pc += reader.pos();
                    stack.push(val);
                }
                OP_I32_ADD => {
                    let b = stack.pop().ok_or("Pila vacia para i32.add (b)")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.add (a)")?;
                    stack.push(a.wrapping_add(b));
                }
                OP_I32_SUB => {
                    let b = stack.pop().ok_or("Pila vacia para i32.sub (b)")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.sub (a)")?;
                    stack.push(a.wrapping_sub(b));
                }
                OP_I32_MUL => {
                    let b = stack.pop().ok_or("Pila vacia para i32.mul (b)")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.mul (a)")?;
                    stack.push(a.wrapping_mul(b));
                }
                OP_I32_DIV_S => {
                    let b = stack.pop().ok_or("Pila vacia para i32.div_s (b)")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.div_s (a)")?;
                    if b == 0 {
                        return Err("Division por cero en WASM");
                    }
                    stack.push(a / b);
                }
                OP_I32_REM_S => {
                    let b = stack.pop().ok_or("Pila vacia para i32.rem_s (b)")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.rem_s (a)")?;
                    if b == 0 {
                        return Err("Modulo por cero en WASM");
                    }
                    stack.push(a % b);
                }
                OP_I32_AND => {
                    let b = stack.pop().ok_or("Pila vacia para i32.and")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.and")?;
                    stack.push(a & b);
                }
                OP_I32_OR => {
                    let b = stack.pop().ok_or("Pila vacia para i32.or")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.or")?;
                    stack.push(a | b);
                }
                OP_I32_XOR => {
                    let b = stack.pop().ok_or("Pila vacia para i32.xor")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.xor")?;
                    stack.push(a ^ b);
                }
                OP_I32_EQZ => {
                    let a = stack.pop().ok_or("Pila vacia para i32.eqz")?;
                    stack.push(if a == 0 { 1 } else { 0 });
                }
                OP_I32_EQ => {
                    let b = stack.pop().ok_or("Pila vacia para i32.eq")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.eq")?;
                    stack.push(if a == b { 1 } else { 0 });
                }
                OP_I32_NE => {
                    let b = stack.pop().ok_or("Pila vacia para i32.ne")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.ne")?;
                    stack.push(if a != b { 1 } else { 0 });
                }
                OP_I32_LT_S => {
                    let b = stack.pop().ok_or("Pila vacia para i32.lt_s")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.lt_s")?;
                    stack.push(if a < b { 1 } else { 0 });
                }
                OP_I32_GT_S => {
                    let b = stack.pop().ok_or("Pila vacia para i32.gt_s")?;
                    let a = stack.pop().ok_or("Pila vacia para i32.gt_s")?;
                    stack.push(if a > b { 1 } else { 0 });
                }
                OP_I32_LOAD => {
                    // alignment and offset
                    let mut reader = Leb128Reader::new(&code[pc..]);
                    let _align = reader.read_u32_leb128()?;
                    let offset = reader.read_u32_leb128()? as usize;
                    pc += reader.pos();
                    let addr = stack.pop().ok_or("Pila vacia para i32.load")? as usize + offset;
                    if addr + 4 > self.memory.len() {
                        return Err("Violacion de memoria en i32.load");
                    }
                    let val = i32::from_le_bytes([
                        self.memory[addr],
                        self.memory[addr + 1],
                        self.memory[addr + 2],
                        self.memory[addr + 3],
                    ]);
                    stack.push(val);
                }
                OP_I32_STORE => {
                    let mut reader = Leb128Reader::new(&code[pc..]);
                    let _align = reader.read_u32_leb128()?;
                    let offset = reader.read_u32_leb128()? as usize;
                    pc += reader.pos();
                    let val = stack.pop().ok_or("Pila vacia para i32.store (val)")?;
                    let addr = stack.pop().ok_or("Pila vacia para i32.store (addr)")? as usize + offset;
                    if addr + 4 > self.memory.len() {
                        return Err("Violacion de memoria en i32.store");
                    }
                    let bytes = val.to_le_bytes();
                    self.memory[addr..addr + 4].copy_from_slice(&bytes);
                }
                OP_CALL => {
                    let mut reader = Leb128Reader::new(&code[pc..]);
                    let target_idx = reader.read_u32_leb128()? as usize;
                    pc += reader.pos();
                    // Ejecucion anidada de funcion
                    let target_type = self.module.functions[target_idx].type_index as usize;
                    let num_params = self.module.types[target_type].params.len();
                    let mut sub_args = Vec::new();
                    for _ in 0..num_params {
                        sub_args.push(stack.pop().ok_or("Pila vacia para llamada de funcion")?);
                    }
                    sub_args.reverse();
                    let ret = self.execute_func(target_idx, &sub_args)?;
                    if let Some(r) = ret {
                        stack.push(r);
                    }
                }
                _ => {
                    return Err("Instruccion de bytecode WASM desconocida o no implementada");
                }
            }
        }

        Ok(stack.pop())
    }
}

/// Genera un módulo WASM binario canónico que exporta "add" (a + b) y "mul" (a * b)
pub fn generate_sample_wasm() -> Vec<u8> {
    let mut bin = Vec::new();
    // Magia y version
    bin.extend_from_slice(&WASM_MAGIC);
    bin.extend_from_slice(&WASM_VERSION_1.to_le_bytes());

    // Seccion 1: Tipos (1 tipo: (i32, i32) -> i32)
    bin.push(SEC_TYPE);
    bin.push(7); // tamano
    bin.push(1); // 1 tipo
    bin.push(0x60); // func
    bin.push(2); // 2 parametros
    bin.push(0x7F); // i32
    bin.push(0x7F); // i32
    bin.push(1); // 1 retorno
    bin.push(0x7F); // i32

    // Seccion 3: Funciones (2 funciones de tipo 0)
    bin.push(SEC_FUNCTION);
    bin.push(3); // tamano
    bin.push(2); // 2 funciones
    bin.push(0); // func 0 es tipo 0
    bin.push(0); // func 1 es tipo 0

    // Seccion 7: Exportaciones (2 exportaciones: "add" func 0, "mul" func 1)
    bin.push(SEC_EXPORT);
    bin.push(13); // tamano
    bin.push(2); // 2 exports
    // Export "add"
    bin.push(3); // len
    bin.extend_from_slice(b"add");
    bin.push(0); // func
    bin.push(0); // index 0
    // Export "mul"
    bin.push(3); // len
    bin.extend_from_slice(b"mul");
    bin.push(0); // func
    bin.push(1); // index 1

    // Seccion 10: Codigo
    bin.push(SEC_CODE);
    bin.push(17); // tamano seccion
    bin.push(2); // 2 cuerpos de funcion
    // Cuerpo func 0 (add): local.get 0, local.get 1, i32.add, end
    bin.push(7); // tamano cuerpo
    bin.push(0); // 0 locales
    bin.push(OP_LOCAL_GET);
    bin.push(0);
    bin.push(OP_LOCAL_GET);
    bin.push(1);
    bin.push(OP_I32_ADD);
    bin.push(OP_END);
    // Cuerpo func 1 (mul): local.get 0, local.get 1, i32.mul, end
    bin.push(7); // tamano cuerpo
    bin.push(0); // 0 locales
    bin.push(OP_LOCAL_GET);
    bin.push(0);
    bin.push(OP_LOCAL_GET);
    bin.push(1);
    bin.push(OP_I32_MUL);
    bin.push(OP_END);

    bin
}

/// Ejecuta la suite de verificación de diagnóstico del motor WASM
pub fn run_wasm_self_test() -> Result<bool, &'static str> {
    let wasm_bytes = generate_sample_wasm();
    let module = WasmModule::parse(&wasm_bytes)?;
    let mut instance = WasmInstance::new(module);

    let res_add = instance.invoke("add", &[19, 23])?;
    if res_add != Some(42) {
        return Err("Fallo en suma WASM: se esperaba 42");
    }

    let res_mul = instance.invoke("mul", &[6, 7])?;
    if res_mul != Some(42) {
        return Err("Fallo en multiplicacion WASM: se esperaba 42");
    }

    Ok(true)
}
