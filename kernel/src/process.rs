use core::alloc::Layout;
use crate::drivers::serial;
use crate::mem::paging::{self, PAGE_SIZE};

pub const MAX_PROCS: usize = 64;
pub const KERNEL_STACK_SIZE: usize = 16384;
pub const USER_STACK_SIZE: usize = 65536;
pub const USER_STACK_TOP: usize = 0x7F00_0000_0000;

#[derive(Copy, Clone, PartialEq)]
#[repr(u8)]
pub enum ProcessState {
    Dead = 0,
    Ready = 1,
    Running = 2,
    Blocked = 3,
    Zombie = 4,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Process {
    pub pid: u64,
    pub state: ProcessState,
    pub name: [u8; 32],
    pub name_len: usize,
    pub cr3: u64,
    pub kernel_rsp: u64,
    pub user_rsp: u64,
    pub user_rip: u64,
    pub kernel_stack_base: usize,
    pub kernel_stack_top: usize,
    pub user_stack_phys: usize,
    pub exit_code: i32,
}

impl Process {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

static mut PROCESSES: [core::mem::MaybeUninit<Process>; MAX_PROCS] =
    [core::mem::MaybeUninit::uninit(); MAX_PROCS];

static mut NEXT_PID: u64 = 1;

fn alloc_pid() -> Option<u64> {
    unsafe {
        let pid = NEXT_PID;
        NEXT_PID += 1;
        Some(pid)
    }
}

fn find_slot() -> Option<usize> {
    unsafe {
        for (i, p) in PROCESSES.iter().enumerate() {
            if (*p.as_ptr()).state == ProcessState::Dead {
                return Some(i);
            }
        }
    }
    None
}

pub fn current_process() -> Option<&'static mut Process> {
    unsafe {
        for p in &mut PROCESSES {
            let proc = p.as_mut_ptr();
            if (*proc).state == ProcessState::Running {
                return Some(&mut *proc);
            }
        }
    }
    None
}

pub fn process_by_pid(pid: u64) -> Option<&'static mut Process> {
    unsafe {
        for p in &mut PROCESSES {
            let proc = p.as_mut_ptr();
            if (*proc).pid == pid {
                return Some(&mut *proc);
            }
        }
    }
    None
}

pub fn set_tss_rsp0(rsp0: u64) {
    unsafe {
        let tss = crate::arch::idt::get_tss_ptr();
        if !tss.is_null() {
            (*tss).rsp[0] = rsp0;
        }
    }
}

pub fn process_create(entry: u64, name: &str) -> Option<u64> {
    let cr3 = paging::new_address_space()?;
    process_create_into(cr3, entry, name)
}

pub fn process_create_into(cr3: u64, entry: u64, name: &str) -> Option<u64> {
    let slot = find_slot()?;
    let pid = alloc_pid()?;

    let ks_layout = Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE).ok()?;
    let ks_ptr = unsafe { alloc::alloc::alloc_zeroed(ks_layout) };
    if ks_ptr.is_null() { return None; }
    let ks_base = ks_ptr as usize;
    let ks_top = ks_base + KERNEL_STACK_SIZE;

    let us_layout = Layout::from_size_align(USER_STACK_SIZE, PAGE_SIZE).ok()?;
    let us_ptr = unsafe { alloc::alloc::alloc_zeroed(us_layout) };
    if us_ptr.is_null() { return None; }
    let us_base = us_ptr as usize;

    let us_vaddr = USER_STACK_TOP - USER_STACK_SIZE;
    for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
        paging::map_page_user(cr3, us_vaddr + i * PAGE_SIZE, us_base + i * PAGE_SIZE).ok()?;
    }

    // Set up initial user stack with argc=0, argv=NULL, envp=NULL
    // Layout (from top): [envp=NULL] [argv=NULL] [argc=0]
    let us_top_phys = us_base + USER_STACK_SIZE;
    let (user_rsp_va, proc) = unsafe {
        let mut sp = us_top_phys;
        sp -= 8; *(sp as *mut u64) = 0; // envp = NULL
        sp -= 8; *(sp as *mut u64) = 0; // argv = NULL
        sp -= 8; *(sp as *mut u64) = 0; // argc = 0
        let rsp = us_vaddr + (sp - us_base);
        let p = &mut *PROCESSES[slot].as_mut_ptr();
        (rsp, p)
    };
    let name_len = name.len().min(31);
    proc.name[..name_len].copy_from_slice(&name.as_bytes()[..name_len]);
    proc.name_len = name_len;
    proc.pid = pid;
    proc.state = ProcessState::Ready;
    proc.cr3 = cr3;
    proc.kernel_stack_base = ks_base;
    proc.kernel_stack_top = ks_top;
    proc.kernel_rsp = 0;
    proc.user_stack_phys = us_base;
    proc.user_rsp = user_rsp_va as u64;
    proc.user_rip = entry;
    proc.exit_code = 0;

    serial::write_str("PROC: create PID=");
    serial::write_usize(pid as usize);
    serial::write_str(" name='");
    serial::write_str(proc.name_str());
    serial::write_str("' entry=");
    serial::write_hex(entry as usize);
    serial::write_str(" cr3=");
    serial::write_hex(cr3 as usize);
    serial::write_str("\n");

    Some(pid)
}

pub fn set_current(pid: u64) -> Option<()> {
    let proc = process_by_pid(pid)?;
    unsafe {
        for p in &mut PROCESSES {
            let pp = p.as_mut_ptr();
            if (*pp).state == ProcessState::Running {
                (*pp).state = ProcessState::Ready;
            }
        }
    }
    proc.state = ProcessState::Running;
    set_tss_rsp0(proc.kernel_stack_top as u64);
    serial::write_str("PROC: set_current PID=");
    serial::write_usize(pid as usize);
    serial::write_str("\n");
    Some(())
}

pub fn process_exit(pid: u64, code: i32) {
    let proc = match process_by_pid(pid) {
        Some(p) => p,
        None => {
            serial::write_str("PROC: exit unknown PID=");
            serial::write_usize(pid as usize);
            serial::write_str("\n");
            return;
        }
    };

    serial::write_str("PROC: exit PID=");
    serial::write_usize(pid as usize);
    serial::write_str(" name='");
    serial::write_str(proc.name_str());
    serial::write_str("' code=");
    serial::write_usize(code as usize);
    serial::write_str("\n");

    let cr3 = proc.cr3;
    let ks_base = proc.kernel_stack_base;
    let us_phys = proc.user_stack_phys;

    if ks_base != 0 {
        let layout = Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE).unwrap();
        unsafe { alloc::alloc::dealloc(ks_base as *mut u8, layout); }
    }

    if us_phys != 0 {
        let layout = Layout::from_size_align(USER_STACK_SIZE, PAGE_SIZE).unwrap();
        unsafe { alloc::alloc::dealloc(us_phys as *mut u8, layout); }
    }

    if cr3 != 0 && cr3 != paging::read_cr3() {
        paging::free_address_space(cr3);
    }

    proc.state = ProcessState::Dead;
    proc.pid = 0;
    proc.cr3 = 0;
    proc.kernel_stack_base = 0;
    proc.kernel_stack_top = 0;
    proc.user_stack_phys = 0;
}

pub fn init() {
    serial::write_str("PROC: init process table (max ");
    serial::write_usize(MAX_PROCS);
    serial::write_str(" slots)\n");
}
