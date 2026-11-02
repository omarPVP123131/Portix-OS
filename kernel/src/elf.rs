use crate::drivers::serial;
use crate::mem::paging::{self, PAGE_SIZE};
use crate::process;

const EI_MAG: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const PT_NULL: u32 = 0;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_SHLIB: u32 = 5;
pub const PT_PHDR: u32 = 6;
pub const PT_TLS: u32 = 7;
pub const PT_GNU_EH_FRAME: u32 = 0x6474e550;
pub const PT_GNU_STACK: u32 = 0x6474e551;
pub const PT_GNU_RELRO: u32 = 0x6474e552;

const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

pub const PIE_LOAD_BASE: u64 = 0x0000_5555_5555_4000;
pub const INTERP_LOAD_BASE: u64 = 0x0000_7000_0000_0000;
/// Tamaño máximo de la ventana del intérprete dinámico (ld-musl ≈ 650 KiB,
/// ld-linux ≈ 250 KiB); fork la recorre página a página para heredarla.
pub const INTERP_WINDOW: usize = 4 * 1024 * 1024;

#[repr(C)]
struct Elf64Header {
    ident:     [u8; 16],
    e_type:    u16,
    e_machine: u16,
    e_version: u32,
    e_entry:   u64,
    e_phoff:   u64,
    e_shoff:   u64,
    e_flags:   u32,
    e_ehsize:  u16,
    e_phentsize: u16,
    e_phnum:   u16,
    e_shentsize: u16,
    e_shnum:   u16,
    e_shstrndx: u16,
}

#[repr(C)]
struct Elf64Phdr {
    p_type:   u32,
    p_flags:  u32,
    p_offset: u64,
    p_vaddr:  u64,
    p_paddr:  u64,
    p_filesz: u64,
    p_memsz:  u64,
    p_align:  u64,
}

pub const ELF_DEFAULT_STACK_SIZE: usize = 65536;

pub struct ElfLoader {
    pub entry: u64,
    pub total_size: u64,
    pub segment_count: usize,
    pub stack_size: usize,
    pub is_pie: bool,
    pub load_bias: u64,
    pub interp_path: Option<alloc::string::String>,
    pub auxv: process::AuxvInfo,
}

fn validate_elf(data: &[u8]) -> Result<(), &'static str> {
    if data.len() < 64 { return Err("ELF too small"); }
    let hdr: &Elf64Header = unsafe { &*(data.as_ptr() as *const Elf64Header) };
    if hdr.ident[..4] != EI_MAG { return Err("bad magic"); }
    if hdr.ident[4] != 2 { return Err("not 64-bit"); }
    if hdr.ident[5] != 1 { return Err("not little-endian"); }
    if hdr.e_type != 2 && hdr.e_type != 3 { return Err("not ET_EXEC or ET_DYN"); }
    if hdr.e_machine != 0x3E { return Err("not x86-64"); }
    if hdr.e_phentsize as usize != core::mem::size_of::<Elf64Phdr>() { return Err("bad phentsize"); }
    Ok(())
}

const KERNEL_BASE: usize = 0xFFFF_8000_0000_0000;

/// [T-003] Los ejecutables mayores que esto NO se leen enteros al montículo
/// del núcleo (cc1 de gcc son 37 MiB y el montículo tiene 64 MiB: el intento
/// tiraba el núcleo con "memory allocation of 38981384 bytes failed"). Se
/// leen por trozos directamente a los marcos del proceso.
pub const ELF_EN_MEMORIA_MAX: usize = 4 * 1024 * 1024;
const CABECERA_ELF: usize = 64 * 1024;
const TROZO_ELF: usize = 256 * 1024;

/// Origen de los bytes de un ELF: en memoria (pequeño o embebido) o un
/// fichero del que se leen los segmentos por trozos.
pub enum FuenteElf<'a> {
    Memoria(&'a [u8]),
    Propia(alloc::vec::Vec<u8>),
    Fichero { ruta: alloc::string::String, tam: usize, cab: alloc::vec::Vec<u8>, cache: alloc::vec::Vec<u8>, base: usize, cache_len: usize },
}

impl<'a> FuenteElf<'a> {
    /// Abre `path`: entero en memoria si es pequeño, por trozos si es grande.
    pub fn abrir(path: &str) -> Option<FuenteElf<'static>> {
        if !crate::drivers::storage::vfs::es_ruta_fs(path) { crate::syscall::sin_raiz(path); return None; }
        let tam = crate::drivers::storage::vfs::tam_fs(path).unwrap_or(0);
        if tam == 0 { return None; }
        if tam <= ELF_EN_MEMORIA_MAX {
            let mut v = alloc::vec![0u8; tam];
            if crate::drivers::storage::vfs::leer_fs(path, 0, &mut v).is_err() { return None; }
            return Some(FuenteElf::Propia(v));
        }
        let n = tam.min(CABECERA_ELF);
        let mut cab = alloc::vec![0u8; n];
        if !leer_exacto(path, 0, &mut cab) { return None; }
        Some(FuenteElf::Fichero { ruta: alloc::string::String::from(path), tam, cab, cache: alloc::vec::Vec::new(), base: usize::MAX, cache_len: 0 })
    }
    pub fn len(&self) -> usize {
        match self { FuenteElf::Memoria(d) => d.len(), FuenteElf::Propia(v) => v.len(), FuenteElf::Fichero { tam, .. } => *tam }
    }
    /// Bytes con cabecera ELF y tabla de programa (todo el fichero si está en memoria).
    pub fn cabecera(&self) -> &[u8] {
        match self { FuenteElf::Memoria(d) => d, FuenteElf::Propia(v) => v, FuenteElf::Fichero { cab, .. } => cab }
    }
    /// Imagen completa si está en memoria (para la tabla de símbolos); `None` si se lee por trozos.
    pub fn imagen(&self) -> Option<&[u8]> {
        match self { FuenteElf::Memoria(d) => Some(d), FuenteElf::Propia(v) => Some(v), FuenteElf::Fichero { .. } => None }
    }
    /// Copia `n` bytes desde el desplazamiento `off` del ELF a `dst`.
    /// El rango nunca cruza una frontera de página del fichero (p_offset ≡ p_vaddr mod 4096).
    fn copiar(&mut self, off: usize, dst: *mut u8, n: usize) -> Result<(), &'static str> {
        match self {
            FuenteElf::Memoria(d) => { unsafe { core::ptr::copy_nonoverlapping(d.as_ptr().add(off), dst, n); } Ok(()) }
            FuenteElf::Propia(v) => { unsafe { core::ptr::copy_nonoverlapping(v.as_ptr().add(off), dst, n); } Ok(()) }
            FuenteElf::Fichero { ruta, tam, cache, base, cache_len, .. } => {
                let b = off & !(TROZO_ELF - 1);
                if *base != b || off + n > b + *cache_len {
                    if cache.len() != TROZO_ELF { *cache = alloc::vec![0u8; TROZO_ELF]; }
                    let l = TROZO_ELF.min(*tam - b);
                    if !leer_exacto(ruta, b, &mut cache[..l]) { return Err("lectura del ELF por trozos fallo"); }
                    *base = b; *cache_len = l;
                }
                if off + n > b + *cache_len { return Err("segment data beyond ELF"); }
                unsafe { core::ptr::copy_nonoverlapping(cache.as_ptr().add(off - b), dst, n); }
                Ok(())
            }
        }
    }
}

/// Lee exactamente `buf.len()` bytes de `path` desde `off` (reintenta lecturas cortas).
fn leer_exacto(path: &str, off: usize, buf: &mut [u8]) -> bool {
    let mut hecho = 0;
    while hecho < buf.len() {
        match crate::drivers::storage::vfs::leer_fs(path, (off + hecho) as u64, &mut buf[hecho..]) {
            Ok(0) | Err(()) => return false,
            Ok(n) => hecho += n,
        }
    }
    true
}

pub(crate) fn load_segments_into_cr3(cr3: u64, data: &[u8], _info: &ElfLoader, load_bias: u64) -> Result<(), &'static str> {
    load_segments_fuente(cr3, &mut FuenteElf::Memoria(data), load_bias)
}

/// [T-003] Carga los PT_LOAD de `fuente` en `cr3` leyendo los datos por trozos.
pub(crate) fn load_segments_fuente(cr3: u64, fuente: &mut FuenteElf, load_bias: u64) -> Result<(), &'static str> {
    let total = fuente.len();
    let data = fuente.cabecera();
    if data.len() < 64 { return Err("ELF too small"); }
    let hdr: &Elf64Header = unsafe { &*(data.as_ptr() as *const Elf64Header) };
    let phdr_end = (hdr.e_phoff as usize)
        .checked_add(hdr.e_phnum as usize * core::mem::size_of::<Elf64Phdr>())
        .ok_or("phdr overflow")?;
    if phdr_end > data.len() { return Err("phdr beyond EOF"); }
    // Copia de las cabeceras de programa: `fuente` se presta mutable al copiar.
    let phdrs: alloc::vec::Vec<Elf64Phdr> = (0..hdr.e_phnum as usize).map(|i| unsafe {
        core::ptr::read_unaligned(data.as_ptr().add(hdr.e_phoff as usize + i * core::mem::size_of::<Elf64Phdr>()) as *const Elf64Phdr)
    }).collect();

    let por_trozos = matches!(fuente, FuenteElf::Fichero { .. });
    for phdr in phdrs.iter() {
        if phdr.p_type != PT_LOAD { continue; }
        let vaddr = (phdr.p_vaddr + load_bias) as usize;
        let memsz = phdr.p_memsz as usize;
        let filesz = phdr.p_filesz as usize;
        let offset = phdr.p_offset as usize;

        let seg_end = vaddr.checked_add(memsz).ok_or("segment size overflow")?;
        if seg_end >= KERNEL_BASE {
            return Err("segment extends into kernel space");
        }
        // [CICLO 49 / Fase A] Nada de usuario sobre las zonas que el núcleo
        // todavía toca por identidad (tablas de arranque, bootinfo, montículo, MMIO).
        if !paging::rango_usuario_permitido(vaddr, seg_end.max(vaddr + 1)) {
            return Err("segmento solapa memoria de identidad reservada del nucleo (Fase A)");
        }

        let vaddr_page = paging::page_align_down(vaddr);
        let end_page = paging::page_align_up(vaddr + memsz);
        let pages = (end_page - vaddr_page) / PAGE_SIZE;

        let mut flags = paging::PRESENT | paging::USER | paging::ACCESSED;
        if phdr.p_flags & PF_W != 0 { flags |= paging::WRITABLE; }
        // [T1b.2] Sólo los segmentos PF_X son ejecutables; datos/bss/rodata llevan NX.
        if phdr.p_flags & PF_X == 0 { flags |= paging::nx_bit(); }

        for i in 0..pages {
            let page_va = vaddr_page + i * PAGE_SIZE;
            // [T-026] Cada 64 páginas (256 KiB) del segmento pasan las IRQ
            // pendientes: la carga de un ELF grande llegaba a 900 ms con IF=0.
            if i & 63 == 63 { paging::respirar(); }
            // [CICLO 49 / T1] Un binario estático de distribución (musl, gcc -static)
            // se enlaza en 0x400000, dentro de la identidad física que el núcleo
            // hereda en PML4[0]. `translate` devolvía ese marco *físico del núcleo*
            // y el cargador escribía la imagen encima del propio kernel (triple
            // fallo). Sólo se reutiliza un marco si es dinámico (de usuario);
            // cualquier traducción heredada de la identidad se ignora y se mapea
            // un marco nuevo (map_page divide la página gigante en privado).
            // [T-003] Las traducciones heredadas de la identidad son páginas
            // gigantes de 2 MiB (HUGE_PAGE); un marco de usuario privado es
            // siempre de 4 KiB. Sin esta comprobación, un ejecutable grande
            // (cc1: 0x400000–0x2B06000) caía sobre el montículo del núcleo
            // (físico 0x1000000–0x5000000), `is_dynamic_addr` lo daba por
            // marco de usuario y la imagen se escribía ENCIMA del montículo
            // (#PF en BuddyAllocator::dealloc al primer `free`).
            // [T-041] Sólo se reutiliza una traducción que YA pertenece al
            // espacio de usuario: PTE con USER y físico en la ventana dinámica.
            // Antes bastaba con que no fuera gigante; una página de identidad de
            // 4 KiB (creada al partir una gigante del firmware) pasaba el filtro,
            // el cargador escribía sobre ella y el proceso acababa ejecutando
            // memoria ajena —o inexistente— con la imagen a medias.
            let heredado = match paging::translate(cr3, page_va) {
                Some((paddr, fl)) => {
                    (fl & paging::USER == 0)
                        || (fl & paging::HUGE_PAGE != 0)
                        || !paging::is_dynamic_addr(paddr as usize)
                }
                None => true,
            };
            let (heap_va, _already_mapped) = if let (false, Some((paddr, old_flags))) = (heredado, paging::translate(cr3, page_va)) {
                // Dos segmentos pueden compartir página (fin de .text / inicio de .data):
                // la página es ejecutable si CUALQUIERA de los dos lo es.
                let mut combined_flags = old_flags | flags;
                if (old_flags & paging::NO_EXECUTE == 0) || (flags & paging::NO_EXECUTE == 0) {
                    combined_flags &= !paging::NO_EXECUTE;
                }
                let _ = paging::map_page(cr3, page_va, paddr as usize, combined_flags);
                (paging::phys_to_virt(paddr as usize), true) // [Fase B.2] se escribe por el alias alto
            } else {
                let hva = alloc_kernel_page().ok_or("OOM")?;
                paging::map_page(cr3, page_va, hva, flags)?;
                // [FRENTE C] El marco de imagen nace con una referencia: la de
                // este espacio de direcciones. Se liberará al llegar a 0.
                crate::mem::frames::retain(hva);
                (hva, false)
            };

            let page_seg_start = page_va.max(vaddr);
            let page_seg_end = (page_va + PAGE_SIZE).min(vaddr + filesz);
            if page_seg_start < page_seg_end {
                let dst_off = page_seg_start - page_va;
                let src_off = offset + (page_seg_start - vaddr);
                let n = page_seg_end - page_seg_start;
                if src_off.checked_add(n).ok_or("seg copy overflow")? > total {
                    return Err("segment data beyond ELF");
                }
                fuente.copiar(src_off, (heap_va + dst_off) as *mut u8, n)?;
            }

            let zero_start = (vaddr + filesz).max(page_va);
            let zero_end = (vaddr + memsz).min(page_va + PAGE_SIZE);
            if zero_start < zero_end {
                let dst_off = zero_start - page_va;
                let n = zero_end - zero_start;
                unsafe {
                    core::ptr::write_bytes((heap_va + dst_off) as *mut u8, 0, n);
                }
            }
        }
    }

    if por_trozos {
        serial::write_str("[ELF] imagen cargada por trozos desde disco: ");
        serial::write_usize(fuente.len()); serial::write_str(" bytes");
        let (tot, libres, pico, respaldo) = crate::mem::marcos::estadisticas();
        serial::write_str(" marcos="); serial::write_usize(libres); serial::write_str("/"); serial::write_usize(tot);
        serial::write_str(" pico="); serial::write_usize(pico);
        serial::write_str(" respaldo_heap="); serial::write_usize(respaldo as usize);
        serial::write_str(" refs_pico="); serial::write_usize(crate::mem::frames::PICO_RASTREADOS.load(core::sync::atomic::Ordering::Relaxed) as usize);
        serial::write_str(" tabla_llena="); serial::write_usize(crate::mem::frames::TABLA_LLENA.load(core::sync::atomic::Ordering::Relaxed) as usize);
        serial::write_str("\n");
    }
    Ok(())
}

pub(crate) fn alloc_kernel_page() -> Option<usize> {
    // [Fase B.4] Marco de imagen de usuario: asignador de marcos, no montículo.
    paging::asignar_marco_usuario()
}

fn parse_elf(data: &[u8]) -> Result<ElfLoader, &'static str> {
    let hdr: &Elf64Header = unsafe { &*(data.as_ptr() as *const Elf64Header) };
    let mut load_end: u64 = 0;
    let mut seg_count = 0usize;

    let phdr_end_off = (hdr.e_phoff as usize)
        .checked_add(hdr.e_phnum as usize * core::mem::size_of::<Elf64Phdr>())
        .ok_or("phdr overflow")?;
    if phdr_end_off > data.len() { return Err("phdr beyond EOF"); }
    let phdr_slice = unsafe {
        let ptr = data.as_ptr().add(hdr.e_phoff as usize);
        core::slice::from_raw_parts(ptr as *const Elf64Phdr, hdr.e_phnum as usize)
    };

    let is_pie = hdr.e_type == 3;
    let load_bias = if is_pie { PIE_LOAD_BASE } else { 0u64 };

    let mut interp_path: Option<alloc::string::String> = None;
    let mut phdr_va: u64 = 0;

    for phdr in phdr_slice {
        if phdr.p_type == PT_PHDR {
            phdr_va = phdr.p_vaddr + load_bias;
        }
        if phdr.p_type == PT_INTERP {
            let offset = phdr.p_offset as usize;
            let filesz = phdr.p_filesz as usize;
            if offset + filesz <= data.len() {
                let s_bytes = &data[offset..offset + filesz];
                let len = s_bytes.iter().position(|&b| b == 0).unwrap_or(filesz);
                if let Ok(s) = core::str::from_utf8(&s_bytes[..len]) {
                    interp_path = Some(alloc::string::String::from(s));
                }
            }
        }
        if phdr.p_type == PT_LOAD {
            seg_count += 1;
            let end = phdr.p_vaddr + load_bias + phdr.p_memsz;
            if end > load_end { load_end = end; }
        }
    }

    if phdr_va == 0 && hdr.e_phoff != 0 {
        // [CICLO 49 / T1] Sin PT_PHDR (binarios estáticos de distro) la tabla
        // de cabeceras vive DENTRO del segmento PT_LOAD que contiene e_phoff:
        // AT_PHDR = p_vaddr - p_offset + e_phoff. Antes se pasaba e_phoff a
        // secas (0x40) y musl leía la dirección 0x40 -> #PF en __init_tls.
        for ph in phdr_slice {
            if ph.p_type == PT_LOAD && ph.p_offset <= hdr.e_phoff && hdr.e_phoff < ph.p_offset + ph.p_filesz {
                phdr_va = ph.p_vaddr - ph.p_offset + hdr.e_phoff + load_bias;
                break;
            }
        }
        if phdr_va == 0 { phdr_va = hdr.e_phoff + load_bias; }
    }

    let auxv = process::AuxvInfo {
        phdr: phdr_va,
        phent: hdr.e_phentsize as u64,
        phnum: hdr.e_phnum as u64,
        base: 0,
        entry: hdr.e_entry + load_bias,
        execfn: 0,
    };

    Ok(ElfLoader {
        entry: hdr.e_entry + load_bias,
        total_size: load_end,
        segment_count: seg_count,
        stack_size: ELF_DEFAULT_STACK_SIZE,
        is_pie,
        load_bias,
        interp_path,
        auxv,
    })
}

pub fn elf_load_raw(data: &[u8]) -> Result<ElfLoader, &'static str> {
    validate_elf(data)?;
    parse_elf(data)
}

pub fn elf_load(path: &str) -> Result<ElfLoader, &'static str> {
    if let Some(file_data) = read_binary_file(path) {
        elf_load_raw(&file_data)
    } else {
        Err("file not found")
    }
}

pub fn elf_map_into_cr3(cr3: u64, data: &[u8]) -> Result<(), &'static str> {
    let info = elf_load_raw(data)?;
    load_segments_into_cr3(cr3, data, &info, info.load_bias)
}

pub fn elf_load_from_raw(data: &[u8]) -> Result<ElfLoader, &'static str> {
    elf_load_raw(data)
}

pub fn elf_load_and_create_process(data: &[u8], name: &str) -> Option<u64> {
    elf_load_and_create_process_args(data, name, name)
}

pub fn elf_load_and_create_process_args(data: &[u8], name: &str, cmdline: &str) -> Option<u64> {
    crear_proceso_desde_fuente(&mut FuenteElf::Memoria(data), name, cmdline)
}

fn crear_proceso_desde_fuente(fuente: &mut FuenteElf, name: &str, cmdline: &str) -> Option<u64> {
    let mut info = elf_load_raw(fuente.cabecera()).ok()?;
    let cr3 = paging::new_address_space()?;

    serial::write_str("[ELF] loading ");
    serial::write_str(name);
    serial::write_str(if info.is_pie { " (PIE): entry=" } else { ": entry=" });
    serial::write_hex(info.entry as usize);
    serial::write_str(" segments=");
    serial::write_usize(info.segment_count);
    serial::write_str(" stack=");
    serial::write_usize(info.stack_size / 1024);
    serial::write_str("K\n");

    if let Err(e) = load_segments_fuente(cr3, fuente, info.load_bias) {
        serial::write_str("[ELF] segment load failed: "); serial::write_str(e); serial::write_str("\n");
        return None;
    }

    let entry_page = paging::page_align_down(info.entry as usize);
    if paging::translate(cr3, entry_page).is_none() {
        serial::write_str("[ELF] CRITICAL - entry page NOT mapped!\n");
        return None;
    }

    let initial_rip = cargar_interprete(cr3, &mut info)?;

    let pid = process::process_create_into_args_auxv(cr3, initial_rip, name, cmdline, Some(info.auxv))?;
    // [CICLO v6.x] Tabla de símbolos de la imagen recién cargada: habilita el
    // perfilado simbólico y la simbolización del depurador de Ring 3.
    if let Some(data) = fuente.imagen() { crate::system::symbols::register_for_pid(pid, data, info.load_bias); }
    process::set_process_image_end(pid, info.total_size as usize);
    serial::write_str("[ELF] loaded ");
    serial::write_str(name);
    serial::write_str(" PID=");
    serial::write_usize(pid as usize);
    serial::write_str("\n");

    Some(pid)
}

/// [CICLO 50 / T1b] Si el ELF declara PT_INTERP, carga el intérprete dinámico
/// real (ld-musl / ld-linux) en `INTERP_LOAD_BASE` dentro de `cr3`, fija
/// `auxv.base` y devuelve el RIP inicial (la entrada del intérprete). Sin
/// PT_INTERP devuelve la entrada del programa. Lo usan tanto el arranque
/// desde la consola como `execve`, para que un guion de ash dinámico pueda
/// volver a ejecutar binarios dinámicos.
/// Devuelve el RIP inicial; `None` si el ELF exige un PT_INTERP que no existe o
/// no cabe (Linux: `execve` falla con ENOENT; arrancar sin intérprete acabaría
/// saltando a 0 porque la PLT/GOT nunca se resuelve).
pub fn cargar_interprete(cr3: u64, info: &mut ElfLoader) -> Option<u64> {
    let mut initial_rip = info.entry;
    let mut cargado = info.interp_path.is_none();
    // Comprobar si requiere intérprete dinámico (PT_INTERP: ld-linux-x86-64.so.2 o ld-portix.so)
    if let Some(ref interp) = info.interp_path {
        serial::write_str("[ELF] PT_INTERP requested: ");
        serial::write_str(interp);
        serial::write_str("\n");

        // Intentar leer el intérprete dinámico
        // [T-004] Sólo la ruta pedida (más /usr para raíces merged-/usr sin el
        // enlace /lib64); antes se caía a /bin/ldd, un binario cualquiera.
        let interp_bytes = read_binary_file(interp)
            .or_else(|| { let mut r = alloc::string::String::from("/usr"); r.push_str(interp); read_binary_file(&r) });

        if let Some(idata) = interp_bytes {
            if let Ok(interp_info) = elf_load_raw(&idata) {
                let interp_bias = INTERP_LOAD_BASE;
                let cabe = (interp_info.total_size - interp_info.load_bias) as usize <= INTERP_WINDOW;
                if !cabe {
                    serial::write_str("[ELF] interprete mayor que INTERP_WINDOW: no se carga\n");
                }
                if cabe && load_segments_into_cr3(cr3, &idata, &interp_info, interp_bias).is_ok() {
                    // [CICLO 50 / T1b] `elf_load_raw` ya sumó PIE_LOAD_BASE a la
                    // entrada (el intérprete es ET_DYN); aquí se recoloca en
                    // INTERP_LOAD_BASE: entrada = base + (e_entry sin sesgo).
                    initial_rip = interp_bias + (interp_info.entry - interp_info.load_bias);
                    info.auxv.base = interp_bias;
                    cargado = true;
                    serial::write_str("[ELF] Dynamic linker mapped @ ");
                    serial::write_hex(interp_bias as usize);
                    serial::write_str(" entry=");
                    serial::write_hex(initial_rip as usize);
                    serial::write_str("\n");
                }
            }
        }
        if !cargado { serial::write_str("[ELF] PT_INTERP no disponible: execve -> ENOENT\n"); }
    }

    if cargado { Some(initial_rip) } else { None }
}

pub fn read_binary_file(path: &str) -> Option<alloc::vec::Vec<u8>> {
    let mut file_data = alloc::vec::Vec::new();
    if crate::drivers::storage::vfs::es_ruta_fs(path) {
        // [CICLO 51] contrato `SistemaDeFicheros` (ramfs hoy; ext2 en el lote 2)
        let sz = crate::drivers::storage::vfs::tam_fs(path).unwrap_or(0);
        if sz == 0 { return None; }
        file_data.resize(sz, 0);
        let ok = crate::drivers::storage::vfs::leer_fs(path, 0, &mut file_data).is_ok();
        if ok && !file_data.is_empty() { return Some(file_data); }
        return None;
    }

    // [T-001] Sin raiz montada no hay de donde leer: la FAT de arranque ya no
    // actua de rescate. `None` explicito (quien llama decide el errno/mensaje).
    crate::syscall::sin_raiz(path);
    None
}

pub fn elf_load_from_path_and_create(path: &str, name: &str) -> Option<u64> {
    elf_load_from_path_and_create_args(path, name, name)
}

pub fn elf_load_from_path_and_create_args(path: &str, name: &str, cmdline: &str) -> Option<u64> {
    let mut fuente = FuenteElf::abrir(path)?;
    crear_proceso_desde_fuente(&mut fuente, name, cmdline)
}

pub fn init() {
    serial::write_str("[ELF] loader ready (PIE & Auxiliary Vector standard Linux support)\n");
}
