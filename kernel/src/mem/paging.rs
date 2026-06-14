use core::alloc::Layout;
use core::ptr;
use crate::drivers::serial;

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_ENTRIES: usize = 512;

pub const PRESENT: u64       = 1 << 0;
pub const WRITABLE: u64      = 1 << 1;
pub const USER: u64          = 1 << 2;
pub const WRITE_THROUGH: u64 = 1 << 3;
pub const CACHE_DISABLE: u64 = 1 << 4;
pub const ACCESSED: u64      = 1 << 5;
pub const DIRTY: u64         = 1 << 6;
pub const HUGE_PAGE: u64     = 1 << 7;
pub const GLOBAL: u64        = 1 << 8;
pub const NO_EXECUTE: u64    = 1 << 63;

#[inline]
pub fn pml4_index(vaddr: usize) -> usize {
    (vaddr >> 39) & 0x1FF
}
#[inline]
pub fn pdpt_index(vaddr: usize) -> usize {
    (vaddr >> 30) & 0x1FF
}
#[inline]
pub fn pd_index(vaddr: usize) -> usize {
    (vaddr >> 21) & 0x1FF
}
#[inline]
pub fn pt_index(vaddr: usize) -> usize {
    (vaddr >> 12) & 0x1FF
}

#[inline]
pub fn page_align_down(x: usize) -> usize {
    x & !(PAGE_SIZE - 1)
}
#[inline]
pub fn page_align_up(x: usize) -> usize {
    (x + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

pub fn read_cr3() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) v, options(nostack, nomem)) }
    v
}

pub fn write_cr3(val: u64) {
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) val, options(nostack, nomem)) }
}

pub fn flush_tlb() {
    let cr3 = read_cr3();
    write_cr3(cr3);
}

pub fn flush_tlb_page(vaddr: usize) {
    unsafe { core::arch::asm!("invlpg [{}]", in(reg) vaddr, options(nostack, nomem)) }
}

unsafe fn read_entry(table_phys: usize, index: usize) -> u64 {
    ptr::read_volatile((table_phys as *const u64).add(index))
}
unsafe fn write_entry(table_phys: usize, index: usize, val: u64) {
    ptr::write_volatile((table_phys as *mut u64).add(index), val);
}

pub fn entry_paddr(entry: u64) -> usize {
    (entry & 0x000F_FFFF_FFFF_F000) as usize
}

pub fn translate(cr3: u64, vaddr: usize) -> Option<(usize, u64)> {
    let cr3_p = cr3 as usize;
    unsafe {
        let pml4e = read_entry(cr3_p, pml4_index(vaddr));
        if pml4e & PRESENT == 0 { return None; }
        let pdpt_p = entry_paddr(pml4e);
        let pdpte = read_entry(pdpt_p, pdpt_index(vaddr));
        if pdpte & PRESENT == 0 { return None; }
        if pdpte & HUGE_PAGE != 0 {
            let offset = vaddr & 0x3FFF_FFFF;
            let paddr = entry_paddr(pdpte) + offset;
            return Some((paddr, pdpte));
        }
        let pd_p = entry_paddr(pdpte);
        let pde = read_entry(pd_p, pd_index(vaddr));
        if pde & PRESENT == 0 {
            return None;
        }
        if pde & HUGE_PAGE != 0 {
            let offset = vaddr & 0x1F_FFFF;
            let paddr = entry_paddr(pde) + offset;
            return Some((paddr, pde));
        }
        let pt_p = entry_paddr(pde);
        let pte = read_entry(pt_p, pt_index(vaddr));
        if pte & PRESENT == 0 {
            return None;
        }
        let paddr = entry_paddr(pte) + (vaddr & 0xFFF);
        Some((paddr, pte))
    }
}

fn alloc_page_table() -> Option<usize> {
    let layout = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE).ok()?;
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() { return None; }
    Some(ptr as usize)
}

fn free_page(paddr: usize) {
    unsafe {
        let layout = match Layout::from_size_align(PAGE_SIZE, PAGE_SIZE) {
            Ok(l) => l,
            Err(_) => {
                serial::log("PAGING", "CRITICAL: invalid layout in free_page\n");
                return;
            }
        };
        alloc::alloc::dealloc(paddr as *mut u8, layout);
    }
}

fn ensure_or_create(phys: usize, index: usize, user: bool) -> Option<usize> {
    unsafe {
        let entry = read_entry(phys, index);
        if entry & PRESENT != 0 {
            if user && entry & USER == 0 && entry & HUGE_PAGE == 0 {
                write_entry(phys, index, entry | USER);
                if phys == crate::mem::paging::read_cr3() as usize {
                    flush_tlb();
                }
            }
            return Some(entry_paddr(entry));
        }
    }
    let new_tbl = alloc_page_table()?;
    let mut flags: u64 = PRESENT | WRITABLE;
    if user { flags |= USER; }
    unsafe {
        write_entry(phys, index, new_tbl as u64 | flags);
    }
    Some(new_tbl)
}

fn split_huge_pde(pd_p: usize, pde_index: usize) -> Option<()> {
    unsafe {
        let pde = read_entry(pd_p, pde_index);
        if pde & PRESENT == 0 || pde & HUGE_PAGE == 0 {
            return None;
        }
        let huge_paddr = entry_paddr(pde);
        let huge_flags = pde & !(0x000F_FFFF_FFFF_F000);
        let pt_p = alloc_page_table()?;
        for i in 0..PAGE_ENTRIES {
            let pte = (huge_paddr + i * PAGE_SIZE) as u64 | huge_flags;
            write_entry(pt_p, i, pte);
        }
        let new_pde_flags = PRESENT | WRITABLE | ACCESSED | (pde & (USER | NO_EXECUTE));
        write_entry(pd_p, pde_index, pt_p as u64 | new_pde_flags);
        flush_tlb();
        Some(())
    }
}

fn split_huge_pdpt(pdpt_p: usize, pdpt_index: usize) -> Option<()> {
    unsafe {
        let pdpte = read_entry(pdpt_p, pdpt_index);
        if pdpte & PRESENT == 0 || pdpte & HUGE_PAGE == 0 {
            return None;
        }
        let huge_paddr = entry_paddr(pdpte);
        let huge_flags = pdpte & !(0x000F_FFFF_FFFF_F000);
        let pd_p = alloc_page_table()?;
        for i in 0..PAGE_ENTRIES {
            let pde = (huge_paddr + i * 0x200000) as u64 | huge_flags | HUGE_PAGE;
            write_entry(pd_p, i, pde);
        }
        let new_pdpte_flags = PRESENT | WRITABLE | ACCESSED | (pdpte & (USER | NO_EXECUTE));
        write_entry(pdpt_p, pdpt_index, pd_p as u64 | new_pdpte_flags);
        flush_tlb();
        Some(())
    }
}

pub fn map_page(cr3: u64, vaddr: usize, paddr: usize, flags: u64) -> Result<(), &'static str> {
    let cr3_p = cr3 as usize;
    let user = flags & USER != 0;

    let pdpt_p = ensure_or_create(cr3_p, pml4_index(vaddr), user).ok_or("no PDPT")?;

    let pdpt_idx = pdpt_index(vaddr);
    unsafe {
        let pdpte = read_entry(pdpt_p, pdpt_idx);
        if pdpte & PRESENT != 0 && pdpte & HUGE_PAGE != 0 {
            split_huge_pdpt(pdpt_p, pdpt_idx).ok_or("split 1G failed")?;
        }
    }

    let pd_p = ensure_or_create(pdpt_p, pdpt_idx, user).ok_or("no PD")?;

    let pde_index = pd_index(vaddr);
    unsafe {
        let pde = read_entry(pd_p, pde_index);
        if pde & PRESENT != 0 && pde & HUGE_PAGE != 0 {
            split_huge_pde(pd_p, pde_index).ok_or("split 2M failed")?;
        }
    }

    let pt_p = ensure_or_create(pd_p, pde_index, user).ok_or("no PT")?;

    let pte_index = pt_index(vaddr);
    unsafe {
        let pte_val = (paddr as u64 & 0x000F_FFFF_FFFF_F000) | flags;
        write_entry(pt_p, pte_index, pte_val);
    }

    if cr3 == read_cr3() {
        flush_tlb_page(vaddr);
    }

    crate::drivers::serial::write_str("[PAGING] map_page vaddr=");
    crate::drivers::serial::write_hex(vaddr);
    crate::drivers::serial::write_str(" paddr=");
    crate::drivers::serial::write_hex(paddr);
    crate::drivers::serial::write_str(" flags=");
    crate::drivers::serial::write_hex(flags as usize);
    crate::drivers::serial::write_str("\n");

    Ok(())
}

pub fn unmap_page(cr3: u64, vaddr: usize) -> Result<(), &'static str> {
    let cr3_p = cr3 as usize;
    let pml4e = unsafe { read_entry(cr3_p, pml4_index(vaddr)) };
    if pml4e & PRESENT == 0 { return Err("no PML4E"); }
    let pdpt_p = entry_paddr(pml4e);
    let pdpte = unsafe { read_entry(pdpt_p, pdpt_index(vaddr)) };
    if pdpte & PRESENT == 0 { return Err("no PDPTE"); }
    if pdpte & HUGE_PAGE != 0 { return Err("1G page"); }
    let pd_p = entry_paddr(pdpte);
    let pde = unsafe { read_entry(pd_p, pd_index(vaddr)) };
    if pde & PRESENT == 0 { return Err("no PDE"); }
    if pde & HUGE_PAGE != 0 {
        split_huge_pde(pd_p, pd_index(vaddr)).ok_or("split failed")?;
    }
    let pt_p = entry_paddr(pde);
    let pte_index = pt_index(vaddr);
    unsafe {
        if read_entry(pt_p, pte_index) & PRESENT == 0 { return Err("no PTE"); }
        write_entry(pt_p, pte_index, 0);
    }
    if cr3 == read_cr3() {
        flush_tlb_page(vaddr);
    }

    crate::drivers::serial::write_str("[PAGING] unmap_page vaddr=");
    crate::drivers::serial::write_hex(vaddr);
    crate::drivers::serial::write_str(" cr3=");
    crate::drivers::serial::write_hex(cr3 as usize);
    crate::drivers::serial::write_str("\n");

    Ok(())
}

pub fn map_range(cr3: u64, vaddr_start: usize, paddr_start: usize, pages: usize, flags: u64) -> Result<(), &'static str> {
    for i in 0..pages {
        map_page(cr3, vaddr_start + i * PAGE_SIZE, paddr_start + i * PAGE_SIZE, flags)?;
    }
    Ok(())
}

pub fn map_page_user(cr3: u64, vaddr: usize, paddr: usize) -> Result<(), &'static str> {
    map_page(cr3, vaddr, paddr, PRESENT | WRITABLE | USER | ACCESSED | DIRTY)
}

pub fn new_address_space() -> Option<u64> {
    let current_cr3 = read_cr3();
    let current_pml4_p = current_cr3 as usize;
    let new_pml4 = alloc_page_table()?;
    unsafe {
        for i in 0..PAGE_ENTRIES {
            let entry = read_entry(current_pml4_p, i);
            if entry & PRESENT != 0 {
                write_entry(new_pml4, i, entry);
            }
        }
    }
    Some(new_pml4 as u64)
}

fn is_dynamic_addr(paddr: usize) -> bool {
    paddr >= crate::mem::HEAP_START && paddr < crate::mem::HEAP_START + crate::mem::HEAP_SIZE
}

pub fn free_address_space(cr3: u64) {
    let cr3_p = cr3 as usize;
    if !is_dynamic_addr(cr3_p) { return; }

    for i in 0..PAGE_ENTRIES {
        let entry = unsafe { read_entry(cr3_p, i) };
        if entry & PRESENT == 0 { continue; }
        let pdpt_p = entry_paddr(entry);
        if !is_dynamic_addr(pdpt_p) { continue; }
        for j in 0..PAGE_ENTRIES {
            let pdpte = unsafe { read_entry(pdpt_p, j) };
            if pdpte & PRESENT == 0 { continue; }
            if pdpte & HUGE_PAGE != 0 { continue; }
            let pd_p = entry_paddr(pdpte);
            if !is_dynamic_addr(pd_p) { continue; }
            for k in 0..PAGE_ENTRIES {
                let pde = unsafe { read_entry(pd_p, k) };
                if pde & PRESENT == 0 { continue; }
                if pde & HUGE_PAGE != 0 { continue; }
                let pt_p = entry_paddr(pde);
                if is_dynamic_addr(pt_p) { 
                    // Free data pages mapped by this PT
                    for l in 0..PAGE_ENTRIES {
                        let pte = unsafe { read_entry(pt_p, l) };
                        if pte & PRESENT == 0 { continue; }
                        let data_paddr = entry_paddr(pte);
                        if is_dynamic_addr(data_paddr) {
                            free_page(data_paddr);
                        }
                    }
                    free_page(pt_p); 
                }
            }
            free_page(pd_p);
        }
        free_page(pdpt_p);
    }
    free_page(cr3_p);
    flush_tlb();
}

pub fn dump_page_tables(cr3: u64) {
    let cr3_p = cr3 as usize;
    serial::write_str("=== Page Table Dump ===\n");
    serial::write_str("CR3=");
    serial::write_hex(cr3_p);
    serial::write_str("\n");

    for i in 0..PAGE_ENTRIES {
        let pml4e = unsafe { read_entry(cr3_p, i) };
        if pml4e & PRESENT == 0 { continue; }
        let pdpt_p = entry_paddr(pml4e);
        serial::write_str("  PML4[");
        serial::write_usize(i);
        serial::write_str("] -> ");
        serial::write_hex(pdpt_p);
        serial::write_str("\n");

        for j in 0..PAGE_ENTRIES {
            let pdpte = unsafe { read_entry(pdpt_p, j) };
            if pdpte & PRESENT == 0 { continue; }
            if pdpte & HUGE_PAGE != 0 {
                serial::write_str("    PDPT[");
                serial::write_usize(j);
                serial::write_str("] 1G page paddr=");
                serial::write_hex(entry_paddr(pdpte));
                serial::write_str("\n");
                continue;
            }
            let pd_p = entry_paddr(pdpte);
            serial::write_str("    PDPT[");
            serial::write_usize(j);
            serial::write_str("] -> ");
            serial::write_hex(pd_p);
            serial::write_str("\n");

            for k in 0..PAGE_ENTRIES {
                let pde = unsafe { read_entry(pd_p, k) };
                if pde & PRESENT == 0 { continue; }
                if pde & HUGE_PAGE != 0 {
                    serial::write_str("      PD[");
                    serial::write_usize(k);
                    serial::write_str("] 2M page paddr=");
                    serial::write_hex(entry_paddr(pde));
                    serial::write_str(" flags=");
                    serial::write_hex(pde as usize);
                    serial::write_str("\n");
                    continue;
                }
                serial::write_str("      PD[");
                serial::write_usize(k);
                serial::write_str("] -> PT\n");
            }
        }
    }
    serial::write_str("=== End Dump ===\n");
}

pub fn probe_readable(vaddr: usize, count: usize) -> bool {
    is_user_range_safe(vaddr, count)
}

pub fn is_user_range_safe(vaddr: usize, count: usize) -> bool {
    let cr3 = read_cr3();
    let mut offset = 0usize;
    while offset < count {
        let addr = vaddr + offset;
        match translate(cr3, addr) {
            Some((_, pte)) => {
                if pte & PRESENT == 0 || pte & USER == 0 {
                    return false;
                }
            }
            None => { return false; }
        }
        offset += PAGE_SIZE;
    }
    true
}

pub fn copy_from_user(dst: &mut [u8], src: usize, count: usize) -> Result<usize, ()> {
    if count == 0 { return Ok(0); }
    if dst.len() < count { return Err(()); }
    let saved_flags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) saved_flags, options(nostack)); }
    unsafe { core::arch::asm!("cli", options(nostack)); }
    let ok = is_user_range_safe(src, count);
    if ok {
        unsafe {
            core::ptr::copy_nonoverlapping(src as *const u8, dst.as_mut_ptr(), count);
        }
    }
    if saved_flags & 0x200 != 0 {
        unsafe { core::arch::asm!("sti", options(nostack)); }
    }
    crate::drivers::serial::write_str("[PAGING] copy_from_user src=");
    crate::drivers::serial::write_hex(src);
    crate::drivers::serial::write_str(" count=");
    crate::drivers::serial::write_usize(count);
    if ok {
        crate::drivers::serial::write_str(" OK\n");
        Ok(count)
    } else {
        crate::drivers::serial::write_str(" FAIL\n");
        Err(())
    }
}

pub fn copy_to_user(dst: usize, src: &[u8]) -> Result<usize, ()> {
    let count = src.len();
    if count == 0 { return Ok(0); }
    let saved_flags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) saved_flags, options(nostack)); }
    unsafe { core::arch::asm!("cli", options(nostack)); }
    let ok = is_user_range_safe(dst, count);
    if ok {
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, count);
        }
    }
    if saved_flags & 0x200 != 0 {
        unsafe { core::arch::asm!("sti", options(nostack)); }
    }
    crate::drivers::serial::write_str("[PAGING] copy_to_user dst=");
    crate::drivers::serial::write_hex(dst);
    crate::drivers::serial::write_str(" count=");
    crate::drivers::serial::write_usize(count);
    if ok {
        crate::drivers::serial::write_str(" OK\n");
        Ok(count)
    } else {
        crate::drivers::serial::write_str(" FAIL\n");
        Err(())
    }
}

static mut EXPECT_USER_FAULT: bool = false;

pub fn set_expect_user_fault() {
    unsafe { EXPECT_USER_FAULT = true; }
}

pub fn clear_expect_user_fault() {
    unsafe { EXPECT_USER_FAULT = false; }
}

pub fn is_expecting_user_fault() -> bool {
    unsafe { EXPECT_USER_FAULT }
}

pub fn init() {
    let cr3 = read_cr3();
    serial::write_str("PAGING: init CR3=");
    serial::write_hex(cr3 as usize);
    serial::write_str("\n");
    serial::write_str("PAGING: ready\n");
}
