// kernel/src/lkm/mod.rs — Loadable Kernel Module (LKM) Subsystem (Epoch II/VI)
#![allow(dead_code)]

pub mod ksyms;

use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::Layout;
use crate::arch::Spinlock;
use crate::drivers::serial;

pub const ET_REL: u16 = 1;
pub const EM_X86_64: u16 = 0x3E;

pub const SHT_NULL: u32 = 0;
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_REL: u32 = 9;

pub const SHF_WRITE: u64 = 0x1;
pub const SHF_ALLOC: u64 = 0x2;
pub const SHF_EXECINSTR: u64 = 0x4;

pub const R_X86_64_NONE: u32 = 0;
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_PC32: u32 = 2;
pub const R_X86_64_PLT32: u32 = 4;
pub const R_X86_64_32: u32 = 10;
pub const R_X86_64_32S: u32 = 11;

pub const SHN_UNDEF: u16 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Ehdr {
    pub ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Shdr {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u64,
    pub sh_entsize: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Sym {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}

#[derive(Clone)]
pub struct KernelModule {
    pub name: String,
    pub base_addr: usize,
    pub size: usize,
    pub init_fn: Option<extern "C" fn() -> i32>,
    pub exit_fn: Option<extern "C" fn()>,
    pub ref_count: usize,
}

static MODULES: Spinlock<Vec<KernelModule>> = Spinlock::new(Vec::new());

pub fn init() {
    serial::write_str("[LKM] Loadable Kernel Module subsystem ready\n");
}

fn align_up(addr: usize, align: usize) -> usize {
    if align <= 1 { addr } else { (addr + align - 1) & !(align - 1) }
}

fn get_str_from_table<'a>(table: &'a [u8], offset: usize) -> &'a str {
    if offset >= table.len() { return ""; }
    let end = table[offset..].iter().position(|&b| b == 0).unwrap_or(table.len() - offset);
    core::str::from_utf8(&table[offset..offset + end]).unwrap_or("")
}

pub fn load_module(data: &[u8], name_opt: Option<&str>) -> Result<usize, &'static str> {
    if data.len() < core::mem::size_of::<Elf64Ehdr>() {
        return Err("ELF image too small");
    }

    let ehdr: &Elf64Ehdr = unsafe { &*(data.as_ptr() as *const Elf64Ehdr) };
    if ehdr.ident[..4] != [0x7F, b'E', b'L', b'F'] {
        return Err("Invalid ELF magic");
    }
    if ehdr.ident[4] != 2 || ehdr.ident[5] != 1 {
        return Err("ELF must be 64-bit little endian");
    }
    if ehdr.e_type != ET_REL {
        return Err("ELF must be relocatable (ET_REL)");
    }
    if ehdr.e_machine != EM_X86_64 {
        return Err("ELF architecture must be x86_64");
    }

    let shnum = ehdr.e_shnum as usize;
    let shentsize = ehdr.e_shentsize as usize;
    if shentsize < core::mem::size_of::<Elf64Shdr>() {
        return Err("Invalid section header size");
    }
    let sh_table_end = (ehdr.e_shoff as usize)
        .checked_add(shnum * shentsize)
        .ok_or("Section header table overflow")?;
    if sh_table_end > data.len() {
        return Err("Section header table extends beyond image");
    }

    let shdrs: &[Elf64Shdr] = unsafe {
        core::slice::from_raw_parts(data.as_ptr().add(ehdr.e_shoff as usize) as *const Elf64Shdr, shnum)
    };

    // Section header string table
    let shstrtab_sh = &shdrs[ehdr.e_shstrndx as usize];
    let _shstrtab = &data[shstrtab_sh.sh_offset as usize..(shstrtab_sh.sh_offset + shstrtab_sh.sh_size) as usize];

    // Compute memory layout for SHF_ALLOC sections
    let mut sec_offsets = alloc::vec![0usize; shnum];
    let mut current_offset = 0usize;

    for i in 0..shnum {
        let sh = &shdrs[i];
        if (sh.sh_flags & SHF_ALLOC) != 0 && sh.sh_size > 0 {
            let align = sh.sh_addralign.max(1) as usize;
            current_offset = align_up(current_offset, align);
            sec_offsets[i] = current_offset;
            current_offset += sh.sh_size as usize;
        }
    }

    if current_offset == 0 {
        return Err("No allocatable sections in module");
    }

    let total_alloc_size = align_up(current_offset, 4096);
    let layout = Layout::from_size_align(total_alloc_size, 4096).map_err(|_| "Invalid layout")?;
    let mem_ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if mem_ptr.is_null() {
        return Err("Out of memory allocating module space");
    }
    // [CICLO 49 / mitad superior] El monticulo es identidad (fisico bajo), pero el
    // nucleo vive en 0xFFFFFFFF80000000+. Un modulo enlazado con PC32/PLT32 o
    // 32S no puede alcanzar simbolos del nucleo desde una direccion baja (el
    // desplazamiento no cabe en 32 bits con signo: la suite lo destapo como
    // #PF en una direccion truncada 0x802E9D90). Se ejecuta el modulo por el
    // alias alto del mismo marco (HH_PD cubre el primer GiB fisico), de modo
    // que modulo y nucleo comparten los 2 GiB superiores.
    // [Fase B.2] El monticulo YA es alias alto: `phys_to_virt` deja intacta una
    // direccion alta y sigue elevando una fisica, asi que vale para ambos casos
    // (sumar el desplazamiento dos veces producia 0xFFFFFFFF04EEB000 y #PF).
    let base_vaddr = crate::mem::paging::phys_to_virt(mem_ptr as usize);

    serial::write_str("[LKM] Allocated ");
    serial::write_usize(total_alloc_size);
    serial::write_str(" bytes @ ");
    serial::write_hex(base_vaddr);
    serial::write_str("\n");

    // Copy PROGBITS sections
    for i in 0..shnum {
        let sh = &shdrs[i];
        if (sh.sh_flags & SHF_ALLOC) != 0 && sh.sh_size > 0 {
            let dest = base_vaddr + sec_offsets[i];
            if sh.sh_type == SHT_PROGBITS {
                let src_off = sh.sh_offset as usize;
                let n = sh.sh_size as usize;
                if src_off + n > data.len() {
                    unsafe { alloc::alloc::dealloc(mem_ptr, layout); }
                    return Err("Section data exceeds file");
                }
                unsafe {
                    core::ptr::copy_nonoverlapping(data.as_ptr().add(src_off), dest as *mut u8, n);
                }
            }
        }
    }

    // Locate Symbol Table and String Table
    let mut symtab_idx = None;
    for i in 0..shnum {
        if shdrs[i].sh_type == SHT_SYMTAB {
            symtab_idx = Some(i);
            break;
        }
    }

    let Some(sym_idx) = symtab_idx else {
        unsafe { alloc::alloc::dealloc(mem_ptr, layout); }
        return Err("No symbol table found in module");
    };

    let sym_sh = &shdrs[sym_idx];
    let str_sh = &shdrs[sym_sh.sh_link as usize];
    let strtab = &data[str_sh.sh_offset as usize..(str_sh.sh_offset + str_sh.sh_size) as usize];

    let sym_count = sym_sh.sh_size as usize / core::mem::size_of::<Elf64Sym>();
    let syms: &[Elf64Sym] = unsafe {
        core::slice::from_raw_parts(data.as_ptr().add(sym_sh.sh_offset as usize) as *const Elf64Sym, sym_count)
    };

    // Helper to resolve symbol address
    let resolve_symbol_addr = |sym: &Elf64Sym| -> Result<usize, &'static str> {
        let name = get_str_from_table(strtab, sym.st_name as usize);
        if sym.st_shndx == SHN_UNDEF {
            // External symbol — look up in kernel symbols
            if let Some(addr) = ksyms::lookup_symbol(name) {
                Ok(addr)
            } else {
                serial::write_str("[LKM] Unresolved kernel symbol: '");
                serial::write_str(name);
                serial::write_str("'\n");
                Err("Unresolved external symbol")
            }
        } else if (sym.st_shndx as usize) < shnum {
            let sec_idx = sym.st_shndx as usize;
            if (shdrs[sec_idx].sh_flags & SHF_ALLOC) != 0 {
                Ok(base_vaddr + sec_offsets[sec_idx] + sym.st_value as usize)
            } else {
                Ok(sym.st_value as usize)
            }
        } else {
            Ok(sym.st_value as usize)
        }
    };

    // Apply relocations
    for i in 0..shnum {
        let sh = &shdrs[i];
        if sh.sh_type != SHT_RELA { continue; }

        let target_sec_idx = sh.sh_info as usize;
        if target_sec_idx >= shnum { continue; }
        let target_sh = &shdrs[target_sec_idx];
        if (target_sh.sh_flags & SHF_ALLOC) == 0 { continue; }

        let target_sec_base = base_vaddr + sec_offsets[target_sec_idx];
        let rela_count = sh.sh_size as usize / core::mem::size_of::<Elf64Rela>();
        let relas: &[Elf64Rela] = unsafe {
            core::slice::from_raw_parts(data.as_ptr().add(sh.sh_offset as usize) as *const Elf64Rela, rela_count)
        };

        for rela in relas {
            let r_sym = (rela.r_info >> 32) as usize;
            let r_type = (rela.r_info & 0xFFFFFFFF) as u32;

            if r_sym >= sym_count {
                unsafe { alloc::alloc::dealloc(mem_ptr, layout); }
                return Err("Relocation symbol index out of range");
            }

            let sym = &syms[r_sym];
            let s_addr = match resolve_symbol_addr(sym) {
                Ok(a) => a,
                Err(e) => {
                    unsafe { alloc::alloc::dealloc(mem_ptr, layout); }
                    return Err(e);
                }
            };

            let p_addr = target_sec_base + rela.r_offset as usize;
            let addend = rela.r_addend;

            match r_type {
                R_X86_64_NONE => {}
                R_X86_64_64 => {
                    let val = (s_addr as i64 + addend) as u64;
                    unsafe { *(p_addr as *mut u64) = val; }
                }
                R_X86_64_32 => {
                    let full = s_addr as i64 + addend;
                    if full < 0 || full > u32::MAX as i64 {
                        serial::write_str("[LKM] R_X86_64_32 fuera de rango (simbolo en la mitad superior)\n");
                        unsafe { alloc::alloc::dealloc(mem_ptr, layout); }
                        return Err("Relocation out of range");
                    }
                    unsafe { *(p_addr as *mut u32) = full as u32; }
                }
                R_X86_64_32S => {
                    let full = s_addr as i64 + addend;
                    if full < i32::MIN as i64 || full > i32::MAX as i64 {
                        serial::write_str("[LKM] R_X86_64_32S fuera de rango\n");
                        unsafe { alloc::alloc::dealloc(mem_ptr, layout); }
                        return Err("Relocation out of range");
                    }
                    unsafe { *(p_addr as *mut u32) = full as i32 as u32; }
                }
                R_X86_64_PC32 | R_X86_64_PLT32 => {
                    let disp = (s_addr as i64 + addend) - p_addr as i64;
                    if disp < i32::MIN as i64 || disp > i32::MAX as i64 {
                        serial::write_str("[LKM] desplazamiento PC32 fuera de rango\n");
                        unsafe { alloc::alloc::dealloc(mem_ptr, layout); }
                        return Err("Relocation out of range");
                    }
                    unsafe { *(p_addr as *mut u32) = disp as i32 as u32; }
                }
                _ => {
                    serial::write_str("[LKM] Unsupported relocation type: ");
                    serial::write_usize(r_type as usize);
                    serial::write_str("\n");
                    unsafe { alloc::alloc::dealloc(mem_ptr, layout); }
                    return Err("Unsupported relocation type");
                }
            }
        }
    }

    // Discover entry points: init_module / cleanup_module
    let mut init_fn: Option<extern "C" fn() -> i32> = None;
    let mut exit_fn: Option<extern "C" fn()> = None;

    for sym in syms {
        let name = get_str_from_table(strtab, sym.st_name as usize);
        if name == "init_module" || name == "module_init" {
            if let Ok(addr) = resolve_symbol_addr(sym) {
                init_fn = Some(unsafe { core::mem::transmute(addr) });
            }
        } else if name == "cleanup_module" || name == "module_exit" {
            if let Ok(addr) = resolve_symbol_addr(sym) {
                exit_fn = Some(unsafe { core::mem::transmute(addr) });
            }
        }
    }

    let mod_name = name_opt.unwrap_or("unnamed_module");

    // Execute init_module()
    if let Some(init) = init_fn {
        serial::write_str("[LKM] Calling init_module() for '");
        serial::write_str(mod_name);
        serial::write_str("' ...\n");
        let ret = init();
        if ret != 0 {
            serial::write_str("[LKM] init_module() returned error code: ");
            serial::write_usize(ret as usize);
            serial::write_str("\n");
            unsafe { alloc::alloc::dealloc(mem_ptr, layout); }
            return Err("init_module failed");
        }
    }

    serial::write_str("[LKM] Module '");
    serial::write_str(mod_name);
    serial::write_str("' loaded successfully!\n");

    let module = KernelModule {
        name: String::from(mod_name),
        base_addr: base_vaddr,
        size: total_alloc_size,
        init_fn,
        exit_fn,
        ref_count: 1,
    };

    let mut guard = MODULES.lock();
    guard.push(module);

    Ok(base_vaddr)
}

pub fn unload_module(name: &str) -> Result<(), &'static str> {
    let clean_name = if let Some(stripped) = name.strip_suffix(".ko") { stripped } else { name };
    let mut guard = MODULES.lock();
    let pos = guard.iter().position(|m| m.name == name || m.name == clean_name);

    let Some(idx) = pos else {
        return Err("Module not found");
    };

    let m = guard.remove(idx);
    drop(guard);

    if let Some(exit) = m.exit_fn {
        serial::write_str("[LKM] Calling cleanup_module() for '");
        serial::write_str(&m.name);
        serial::write_str("' ...\n");
        exit();
    }

    let layout = Layout::from_size_align(m.size, 4096).map_err(|_| "Invalid layout")?;
    unsafe {
        // base_addr es el alias alto; el monticulo entrega punteros bajos.
        alloc::alloc::dealloc((m.base_addr - crate::mem::paging::KERNEL_VMA_OFFSET) as *mut u8, layout);
    }

    serial::write_str("[LKM] Module '");
    serial::write_str(&m.name);
    serial::write_str("' unloaded successfully.\n");

    Ok(())
}

pub fn list_modules_text() -> String {
    let guard = MODULES.lock();
    if guard.is_empty() {
        return String::from("No hay modulos del kernel cargados.\n");
    }

    let mut out = String::from("Nombre               Tamano     Direccion Base       Estado\n");
    out.push_str("-------------------- ---------- -------------------- ------\n");

    for m in guard.iter() {
        let line = alloc::format!(
            "{:<20} {:>7} B   0x{:016X}   Activo\n",
            m.name,
            m.size,
            m.base_addr
        );
        out.push_str(&line);
    }
    out
}

pub fn module_count() -> usize {
    MODULES.lock().len()
}
