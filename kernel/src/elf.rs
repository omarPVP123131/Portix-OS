use crate::drivers::storage::fat32::Fat32Volume;
use crate::drivers::storage::registry;
use crate::drivers::serial;
use crate::mem::paging::{self, PAGE_SIZE};
use crate::process;

const EI_MAG: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

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
}

fn validate_elf(data: &[u8]) -> Result<(), &'static str> {
    if data.len() < 64 { return Err("ELF too small"); }
    let hdr: &Elf64Header = unsafe { &*(data.as_ptr() as *const Elf64Header) };
    if hdr.ident[..4] != EI_MAG { return Err("bad magic"); }
    if hdr.ident[4] != 2 { return Err("not 64-bit"); }
    if hdr.ident[5] != 1 { return Err("not little-endian"); }
    if hdr.e_type != 2 { return Err("not ET_EXEC"); }
    if hdr.e_machine != 0x3E { return Err("not x86-64"); }
    if hdr.e_phentsize as usize != core::mem::size_of::<Elf64Phdr>() { return Err("bad phentsize"); }
    Ok(())
}

const KERNEL_BASE: usize = 0xFFFF_8000_0000_0000;

pub(crate) fn load_segments_into_cr3(cr3: u64, data: &[u8], info: &ElfLoader) -> Result<(), &'static str> {
    let hdr: &Elf64Header = unsafe { &*(data.as_ptr() as *const Elf64Header) };
    let phdr_slice = unsafe {
        let ptr = data.as_ptr().add(hdr.e_phoff as usize);
        core::slice::from_raw_parts(ptr as *const Elf64Phdr, hdr.e_phnum as usize)
    };

    for phdr in phdr_slice {
        if phdr.p_type != PT_LOAD { continue; }

        let vaddr = phdr.p_vaddr as usize;
        let memsz = phdr.p_memsz as usize;
        let filesz = phdr.p_filesz as usize;
        let offset = phdr.p_offset as usize;

        if vaddr + memsz > KERNEL_BASE {
            return Err("segment extends into kernel space");
        }

        if vaddr + memsz < vaddr {
            return Err("segment size overflow");
        }

        let vaddr_page = paging::page_align_down(vaddr);
        let end_page = paging::page_align_up(vaddr + memsz);
        let pages = (end_page - vaddr_page) / PAGE_SIZE;

        let mut flags = paging::PRESENT | paging::USER | paging::ACCESSED;
        if phdr.p_flags & PF_W != 0 { flags |= paging::WRITABLE; }

        for i in 0..pages {
            let page_va = vaddr_page + i * PAGE_SIZE;
            let heap_va = alloc_kernel_page().ok_or("OOM")?;

            paging::map_page(cr3, page_va, heap_va, flags)?;

            let page_seg_start = page_va.max(vaddr);
            let page_seg_end = (page_va + PAGE_SIZE).min(vaddr + filesz);
            if page_seg_start < page_seg_end {
                let dst_off = page_seg_start - page_va;
                let src_off = offset + (page_seg_start - vaddr);
                let n = page_seg_end - page_seg_start;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr().add(src_off),
                        (heap_va + dst_off) as *mut u8,
                        n,
                    );
                }
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

        if (vaddr + memsz) as u64 > info.total_size {
            // not reached since we compute total_size upfront
        }
    }

    Ok(())
}

pub(crate) fn alloc_kernel_page() -> Option<usize> {
    let layout = core::alloc::Layout::from_size_align(PAGE_SIZE, PAGE_SIZE).ok()?;
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() { return None; }
    Some(ptr as usize)
}

fn parse_elf(data: &[u8]) -> Result<ElfLoader, &'static str> {
    let hdr: &Elf64Header = unsafe { &*(data.as_ptr() as *const Elf64Header) };
    let mut load_end: u64 = 0;
    let mut seg_count = 0usize;

    let phdr_slice = unsafe {
        let ptr = data.as_ptr().add(hdr.e_phoff as usize);
        core::slice::from_raw_parts(ptr as *const Elf64Phdr, hdr.e_phnum as usize)
    };

    for phdr in phdr_slice {
        if phdr.p_type != PT_LOAD { continue; }
        seg_count += 1;
        let end = phdr.p_vaddr + phdr.p_memsz;
        if end > load_end { load_end = end; }
    }

    Ok(ElfLoader {
        entry: hdr.e_entry,
        total_size: load_end,
        segment_count: seg_count,
        stack_size: ELF_DEFAULT_STACK_SIZE,
    })
}
pub fn elf_load_raw(data: &[u8]) -> Result<ElfLoader, &'static str> {
    validate_elf(data)?;
    parse_elf(data)
}

pub fn elf_load(path: &str) -> Result<ElfLoader, &'static str> {
    let drive = registry::get_device(0).ok_or("no device 0")?;
    let mut vol = Fat32Volume::mount(drive).map_err(|_| "mount failed")?;
    let root = vol.root_cluster();

    let (dir_cluster, filename) = resolve_path(&mut vol, root, path)?;
    let entry = vol.find_entry(dir_cluster, filename).map_err(|_| "file not found")?;

    if entry.is_dir {
        return Err("is a directory");
    }

    let file_size = entry.size as usize;
    let mut file_data = alloc::vec![0u8; file_size];
    let read = vol.read_file(&entry, &mut file_data).map_err(|_| "read failed")?;
    if read != file_size {
        return Err("short read");
    }

    elf_load_raw(&file_data)
}

pub(crate) fn resolve_path<'p>(
    vol: &mut Fat32Volume,
    root_cluster: u32,
    path: &'p str,
) -> Result<(u32, &'p str), &'static str> {
    let path = path.trim_start_matches('/');
    if !path.contains('/') {
        return Ok((root_cluster, path));
    }

    let mut cluster = root_cluster;
    let mut remaining = path;

    loop {
        let slash = match remaining.find('/') {
            Some(i) => i,
            None => return Ok((cluster, remaining)),
        };

        let component = &remaining[..slash];
        if component.is_empty() || component == "." {
            remaining = &remaining[slash + 1..];
            continue;
        }

        let entry = vol.find_entry(cluster, component).map_err(|_| "path component not found")?;
        if !entry.is_dir {
            return Err("not a directory");
        }
        cluster = entry.cluster;
        remaining = &remaining[slash + 1..];
    }
}

pub fn elf_map_into_cr3(cr3: u64, data: &[u8]) -> Result<(), &'static str> {
    let info = elf_load_raw(data)?;
    load_segments_into_cr3(cr3, data, &info)
}

pub fn elf_load_from_raw(data: &[u8]) -> Result<ElfLoader, &'static str> {
    elf_load_raw(data)
}

pub fn elf_load_and_create_process(data: &[u8], name: &str) -> Option<u64> {
    let info = elf_load_raw(data).ok()?;
    let cr3 = paging::new_address_space()?;

    serial::write_str("[ELF] loading ");
    serial::write_str(name);
    serial::write_str(": entry=");
    serial::write_hex(info.entry as usize);
    serial::write_str(" segments=");
    serial::write_usize(info.segment_count);
    serial::write_str(" stack=");
    serial::write_usize(info.stack_size / 1024);
    serial::write_str("K\n");

    load_segments_into_cr3(cr3, data, &info).ok()?;

    let entry_page = paging::page_align_down(info.entry as usize);
    if paging::translate(cr3, entry_page).is_none() {
        serial::write_str("[ELF] CRITICAL - entry page NOT mapped!\n");
        return None;
    }

    let pid = process::process_create_into(cr3, info.entry, name)?;
    serial::write_str("[ELF] loaded ");
    serial::write_str(name);
    serial::write_str(" PID=");
    serial::write_usize(pid as usize);
    serial::write_str("\n");

    Some(pid)
}

pub fn init() {
    serial::write_str("[ELF] loader ready\n");
}
