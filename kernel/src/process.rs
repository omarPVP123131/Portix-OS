use core::alloc::Layout;
use crate::drivers::serial;
use crate::mem::paging::{self, PAGE_SIZE};

pub const MAX_PROCS: usize = 64;
pub const KERNEL_STACK_SIZE: usize = 16384;
pub const USER_STACK_SIZE: usize = 65536;
pub const USER_STACK_TOP: usize = 0x7F00_0000_0000;
pub const TIME_SLICE: u64 = 10; // 10 ticks = 100ms at 100Hz

// ── FD table ─────────────────────────────────────────────────────────────
pub const MAX_FDS: usize = 32;
pub const PROGRAM_BREAK_BASE: usize = 0x2000_0000_0000;
pub const PAGE_SIZE_USIZE: usize = 4096;

#[derive(Copy, Clone)]
pub struct OpenFileInfo {
    pub dir_cluster: u32,
    pub cluster: u32,
    pub size: u32,
    pub pos: u32,
    pub name: [u8; 256],
    pub name_len: usize,
}

#[derive(Copy, Clone)]
pub struct RamFileInfo {
    pub path: [u8; 256],
    pub path_len: usize,
    pub size: u32,
    pub pos: u32,
}

#[derive(Copy, Clone, PartialEq)]
pub enum DeviceType {
    Kbd,
    Fb,
    Sda,
    Null,
}

#[derive(Copy, Clone)]
pub struct DeviceInfo {
    pub dev_type: DeviceType,
    pub pos: u32,
}

#[derive(Copy, Clone)]
pub enum FdType {
    Stdin,
    Stdout,
    Stderr,
    File(OpenFileInfo),
    RamFile(RamFileInfo),
    Device(DeviceInfo),
}

#[derive(Copy, Clone)]
pub struct FdEntry {
    pub fd_type: FdType,
}

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
    pub ticks_used: u64,
    pub sleep_until: u64,
    pub ring3_ret_rsp: u64,
    pub ring3_ret_addr: u64,
    pub fds: [Option<FdEntry>; MAX_FDS],
    pub program_break: usize,
    pub program_break_end: usize,
    pub registered_ports: [u16; 16],
    pub registered_port_count: usize,
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
    proc.ticks_used = 0;
    proc.sleep_until = 0;
    proc.ring3_ret_rsp = 0;
    proc.ring3_ret_addr = 0;
    const NONE: Option<FdEntry> = None;
    proc.fds = [NONE; MAX_FDS];
    proc.fds[0] = Some(FdEntry { fd_type: FdType::Stdin });
    proc.fds[1] = Some(FdEntry { fd_type: FdType::Stdout });
    proc.fds[2] = Some(FdEntry { fd_type: FdType::Stderr });
    proc.program_break = PROGRAM_BREAK_BASE;
    proc.program_break_end = PROGRAM_BREAK_BASE;
    proc.registered_ports = [0u16; 16];
    proc.registered_port_count = 0;

    setup_kernel_stack(proc);

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
    let slot = pid_to_slot(pid);
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

    if let Some(s) = slot {
        crate::ipc::cleanup_process(s);
    }

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

// ── FD helpers ───────────────────────────────────────────────────────────

pub fn fd_alloc(proc: &mut Process, entry: FdEntry) -> Option<usize> {
    for i in 0..MAX_FDS {
        if proc.fds[i].is_none() {
            proc.fds[i] = Some(entry);
            return Some(i);
        }
    }
    None
}

pub fn fd_get(proc: &Process, fd: usize) -> Option<&FdEntry> {
    if fd >= MAX_FDS { return None; }
    proc.fds[fd].as_ref()
}

pub fn fd_get_mut(proc: &mut Process, fd: usize) -> Option<&mut FdEntry> {
    if fd >= MAX_FDS { return None; }
    proc.fds[fd].as_mut()
}

pub fn fd_close(proc: &mut Process, fd: usize) {
    if fd < MAX_FDS {
        proc.fds[fd] = None;
    }
}

pub fn pid_to_slot(pid: u64) -> Option<usize> {
    unsafe {
        for (i, p) in PROCESSES.iter().enumerate() {
            let proc = p.as_ptr();
            if (*proc).pid == pid && (*proc).state != ProcessState::Dead {
                return Some(i);
            }
        }
    }
    None
}

pub fn init() {
    serial::write_str("PROC: init process table (max ");
    serial::write_usize(MAX_PROCS);
    serial::write_str(" slots)\n");
}

// ── Scheduler ───────────────────────────────────────────────────────────

/// Pre-arma el kernel stack con un frame PUSH_REGS + IRET sintético,
/// para que el scheduler pueda saltar directamente al proceso vía
/// POP_REGS + IRETQ.
pub fn setup_kernel_stack(proc: &mut Process) {
    unsafe {
        let top = proc.kernel_stack_top as *mut u64;
        let mut sp = top;

        // IRET frame (pushed first = highest addresses, CPU pops in reverse)
        sp = sp.sub(1); *sp = 0x1Bu64;              // SS = USER_DS | 3
        sp = sp.sub(1); *sp = proc.user_rsp;         // RSP (user)
        sp = sp.sub(1); *sp = 0x202u64;              // RFLAGS (IF=1)
        sp = sp.sub(1); *sp = 0x23u64;               // CS = USER_CS | 3
        sp = sp.sub(1); *sp = proc.user_rip;         // RIP

        // PUSH_REGS (15 registers, r15 first = highest, rax last = lowest)
        sp = sp.sub(1); *sp = 0; // r15
        sp = sp.sub(1); *sp = 0; // r14
        sp = sp.sub(1); *sp = 0; // r13
        sp = sp.sub(1); *sp = 0; // r12
        sp = sp.sub(1); *sp = 0; // rbp
        sp = sp.sub(1); *sp = 0; // rbx
        sp = sp.sub(1); *sp = 0; // r11
        sp = sp.sub(1); *sp = 0; // r10
        sp = sp.sub(1); *sp = 0; // r9
        sp = sp.sub(1); *sp = 0; // r8
        sp = sp.sub(1); *sp = 0; // rdi
        sp = sp.sub(1); *sp = 0; // rsi
        sp = sp.sub(1); *sp = 0; // rdx
        sp = sp.sub(1); *sp = 0; // rcx
        sp = sp.sub(1); *sp = 0; // rax

        proc.kernel_rsp = sp as u64;

        serial::write_str("SCHED: setup_kernel_stack PID=");
        serial::write_usize(proc.pid as usize);
        serial::write_str(" rsp=");
        serial::write_hex(sp as usize);
        serial::write_str("\n");
    }
}

pub unsafe fn current_raw() -> *mut Process {
    for p in &mut PROCESSES {
        let ptr = p.as_mut_ptr();
        if (*ptr).state == ProcessState::Running {
            return ptr;
        }
    }
    core::ptr::null_mut()
}

pub unsafe fn pick_next_ready() -> *mut Process {
    let current_pid = {
        let cur = current_raw();
        if cur.is_null() { 0 } else { (*cur).pid }
    };
    let mut start = 0;
    for (i, p) in PROCESSES.iter().enumerate() {
        if (*p.as_ptr()).pid == current_pid {
            start = (i + 1) % MAX_PROCS;
            break;
        }
    }
    for _ in 0..MAX_PROCS {
        let ptr = PROCESSES[start].as_mut_ptr();
        if (*ptr).state == ProcessState::Ready && (*ptr).pid != current_pid {
            return ptr;
        }
        start = (start + 1) % MAX_PROCS;
    }
    core::ptr::null_mut()
}

unsafe fn wake_blocked() {
    let now = crate::time::pit::ticks();
    for p in &mut PROCESSES {
        let ptr = p.as_mut_ptr();
        if (*ptr).state == ProcessState::Blocked && (*ptr).sleep_until > 0 {
            if now >= (*ptr).sleep_until || (*ptr).sleep_until.wrapping_sub(now) > 1000 {
                // woke up: timeout expired or wraparound
                (*ptr).state = ProcessState::Ready;
                (*ptr).sleep_until = 0;
                serial::write_str("SCHED: wake PID ");
                serial::write_usize((*ptr).pid as usize);
                serial::write_str("\n");
            }
        }
    }
}

/// Called from IRQ0 handler assembly.
/// `current_rsp`: RSP after PUSH_REGS (points to saved r11).
/// `saved_cs`: CS value from IRET frame (for CPL detection).
/// Returns new RSP to load (0 = no switch).
#[no_mangle]
pub extern "C" fn schedule_tick(current_rsp: u64, saved_cs: u64) -> u64 {
    let user_cs = (crate::arch::idt::USER_CS as u64) | 3;
    if saved_cs != user_cs {
        return 0; // Skip if not from user mode
    }
    unsafe {
        let cur = current_raw();
        if cur.is_null() {
            return 0;
        }
        (*cur).kernel_rsp = current_rsp;
        (*cur).ticks_used += 1;

        wake_blocked();

        if (*cur).ticks_used < TIME_SLICE {
            return 0;
        }

        (*cur).ticks_used = 0;

        let next = pick_next_ready();
        if next.is_null() || next == cur {
            return 0;
        }

        // Check if current was blocked by SYS_SLEEP
        if (*cur).state == ProcessState::Running {
            (*cur).state = ProcessState::Ready;
        }

        (*next).state = ProcessState::Running;
        (*next).ticks_used = 0;
        set_tss_rsp0((*next).kernel_stack_top as u64);

        let current_cr3 = paging::read_cr3();
        if (*next).cr3 != current_cr3 {
            paging::write_cr3((*next).cr3);
        }

        serial::write_str("SCHED: PID ");
        serial::write_usize((*cur).pid as usize);
        serial::write_str(" → ");
        serial::write_usize((*next).pid as usize);
        serial::write_str("\n");

        (*next).kernel_rsp
    }
}

/// Called from `enter_ring3_asm` to save the return address for ring-3 exit.
#[no_mangle]
pub extern "C" fn process_save_ring3_ret_addr(ret_addr: u64) {
    unsafe {
        let cur = current_raw();
        if !cur.is_null() {
            (*cur).ring3_ret_addr = ret_addr;
        }
    }
}

/// Called from `ring3_exit_trampoline`.
/// Returns the saved ring3_ret_addr, or 0 if not set.
#[no_mangle]
pub extern "C" fn process_get_ring3_ret_addr() -> u64 {
    unsafe {
        let cur = current_raw();
        if cur.is_null() { 0 } else { (*cur).ring3_ret_addr }
    }
}

/// Trampoline for ring-3 process exit.
/// Cleans up the current process, then either:
///   - if ring3_ret_addr is set (enter_ring3_asm path): jumps to it on kernel's main stack
///   - if ring3_ret_addr is 0 (scheduler path): picks next process and switches to it
#[no_mangle]
pub extern "C" fn process_exit_trampoline() -> ! {
    unsafe {
        let ret_addr = process_get_ring3_ret_addr();

        // Clean up current process
        let cur = current_raw();
        if !cur.is_null() {
            let pid = (*cur).pid;
            process_exit(pid, 0);
        }

        let stack_top = core::ptr::addr_of!(crate::__stack_top) as u64;

        if ret_addr != 0 {
            // Process entered via enter_ring3_asm — return to kernel main loop
            core::arch::asm!("
                mov rsp, {0}
                xor rbp, rbp
                jmp {1}
            ", in(reg) stack_top, in(reg) ret_addr, options(noreturn));
        } else {
            // Scheduler-managed process — pick next and switch
            let next = pick_next_ready();
            if !next.is_null() {
                (*next).state = ProcessState::Running;
                set_tss_rsp0((*next).kernel_stack_top as u64);
                let current_cr3 = paging::read_cr3();
                if (*next).cr3 != current_cr3 {
                    paging::write_cr3((*next).cr3);
                }
                serial::write_str("SCHED: exit switch to PID ");
                serial::write_usize((*next).pid as usize);
                serial::write_str("\n");
                core::arch::asm!("
                    mov rsp, {0}
                    pop r11
                    pop r10
                    pop r9
                    pop r8
                    pop rdi
                    pop rsi
                    pop rdx
                    pop rcx
                    pop rax
                    iretq
                ", in(reg) (*next).kernel_rsp, options(noreturn));
            }
            serial::write_str("SCHED: all processes exited, halting\n");
            loop { core::arch::asm!("hlt"); }
        }
    }
}

/// SYS_YIELD: give up the remainder of the current quantum.
pub fn yield_cpu() {
    unsafe {
        let cur = current_raw();
        if !cur.is_null() {
            (*cur).ticks_used = TIME_SLICE; // Force reschedule on next IRQ0
        }
    }
    // Enable interrupts to allow IRQ0 to fire
    unsafe { core::arch::asm!("sti"); }
}
