extern crate alloc;
use crate::drivers::serial;
use crate::drivers::storage::fat32::{Fat32Volume, DirEntryInfo};
use crate::drivers::storage::registry;
use crate::drivers::storage::vfs;
use crate::mem::paging::{self, PAGE_SIZE};
use crate::process::{self, FdEntry, FdType, OpenFileInfo};
use crate::drivers::input::keyboard::{KeyboardState, Key};
use crate::arch::isr_handlers::{pop_ring3_scancode, set_stdin_blocked, clear_stdin_blocked};

fn alloc_page() -> Option<usize> {
    let layout = core::alloc::Layout::from_size_align(PAGE_SIZE, PAGE_SIZE).ok()?;
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() { None } else { Some(ptr as usize) }
}

pub const SYS_EXIT:   u64 = 0;
pub const SYS_WRITE:  u64 = 1;
pub const SYS_GETPID: u64 = 2;
pub const SYS_YIELD:  u64 = 3;
pub const SYS_SLEEP:  u64 = 4;
pub const SYS_READ:   u64 = 5;
pub const SYS_OPEN:   u64 = 6;
pub const SYS_CLOSE:  u64 = 7;
pub const SYS_BRK:    u64 = 8;
pub const SYS_MMAP:   u64 = 9;
pub const SYS_GETDIRENTS: u64 = 10;
pub const SYS_EXECVE: u64 = 11;
pub const SYS_DUP2:   u64 = 12;
pub const SYS_UPTIME: u64 = 13;
pub const SYS_SEND:   u64 = 14;
pub const SYS_RECV:   u64 = 15;
pub const SYS_REG_IRQ:  u64 = 16;
pub const SYS_BLOCK_READ: u64 = 17;
pub const SYS_IOPORT: u64 = 18;
pub const SYS_IOREAD: u64 = 19;
pub const SYS_IOWRITE: u64 = 20;
pub const SYS_MMAP_DEVICE: u64 = 21;

extern "C" {
    fn ring3_exit_trampoline();
}

#[repr(C)]
pub struct SyscallResult(pub u64, pub u64);

#[no_mangle]
extern "C" fn syscall_dispatch(
    num: u64, a1: u64, a2: u64, a3: u64, a4: u64, _a5: u64,
    current_rsp: u64,
) -> SyscallResult {
    match num {
        SYS_EXIT   => sys_exit(a1 as usize),
        SYS_WRITE  => SyscallResult(sys_write(a1 as i32, a2 as usize, a3 as usize) as u64, 0),
        SYS_GETPID => SyscallResult(sys_getpid(), 0),
        SYS_YIELD  => SyscallResult(0, sys_yield_switch(current_rsp)),
        SYS_SLEEP  => SyscallResult(0, sys_sleep_switch(a1, current_rsp)),
        SYS_READ   => {
            let (res, sw) = sys_read(a1 as i32, a2 as usize, a3 as usize, current_rsp);
            SyscallResult(res as u64, sw)
        }
        SYS_OPEN   => SyscallResult(sys_open(a1 as usize, a2 as u32) as u64, 0),
        SYS_CLOSE  => SyscallResult(sys_close(a1 as i32) as u64, 0),
        SYS_BRK    => SyscallResult(sys_brk(a1 as usize) as u64, 0),
        SYS_MMAP   => SyscallResult(sys_mmap(a1 as usize, a2 as usize, a3 as u32, a4 as u32) as u64, 0),
        SYS_GETDIRENTS => SyscallResult(sys_getdents(a1 as usize, a2 as usize, a3 as usize) as u64, 0),
        SYS_EXECVE => sys_execve(a1 as usize, a2 as usize, a3 as usize, current_rsp),
        SYS_DUP2   => SyscallResult(sys_dup2(a1 as i32, a2 as i32) as u64, 0),
        SYS_UPTIME => SyscallResult(sys_uptime(), 0),
        SYS_SEND   => SyscallResult(sys_send(a1, a2, a3, a4) as u64, 0),
        SYS_RECV   => {
            let (res, sw) = sys_recv(a1 as usize, a2 as usize, current_rsp);
            SyscallResult(res as u64, sw)
        }
        SYS_REG_IRQ => SyscallResult(sys_reg_irq(a1, a2) as u64, 0),
        SYS_BLOCK_READ => SyscallResult(sys_block_read(a1 as i32, a2, a3, a4) as u64, 0),
        SYS_IOPORT     => SyscallResult(sys_ioport(a1 as u16, a2 as u8) as u64, 0),
        SYS_IOREAD     => SyscallResult(sys_ioread(a1 as u16) as u64, 0),
        SYS_IOWRITE    => SyscallResult(sys_iowrite(a1 as u16, a2 as u8) as u64, 0),
        SYS_MMAP_DEVICE => SyscallResult(sys_mmap_device(a1 as usize, a2 as usize) as u64, 0),
        _          => SyscallResult(u64::MAX, 0),
    }
}

// ── SYS_EXIT ─────────────────────────────────────────────────────────────

fn sys_exit(_status: usize) -> ! {
    serial::write_str("[R3] SYS_EXIT called\n");
    unsafe { ring3_exit_trampoline(); }
    unreachable!()
}

// ── SYS_WRITE ────────────────────────────────────────────────────────────

fn sys_write(fd: i32, buf: usize, count: usize) -> i64 {
    // Check if this is a device FD first
    if let Some(proc) = process::current_process() {
        if let Some(entry) = process::fd_get(proc, fd as usize) {
            if let process::FdType::Device(ref info) = entry.fd_type {
                return sys_write_device(fd, buf, count, info);
            }
        }
    }
    if fd != 1 && fd != 2 {
        return -1;
    }
    let mut kbuf = [0u8; 256];
    let to_copy = count.min(256);
    match paging::copy_from_user(&mut kbuf[..to_copy], buf, to_copy) {
        Ok(copied) => {
            for &b in &kbuf[..copied] { serial::write_byte(b); }
            copied as i64
        }
        Err(()) => -1,
    }
}

// Simple ring buffer for /dev/kbd — keyboard driver writes, shell reads
const KBD_BUF_SIZE: usize = 256;
static mut KBD_BUF: [u8; KBD_BUF_SIZE] = [0u8; KBD_BUF_SIZE];
static mut KBD_BUF_HEAD: usize = 0;
static mut KBD_BUF_TAIL: usize = 0;

fn kbd_buf_write(data: &[u8]) {
    unsafe {
        for &b in data {
            let next = (KBD_BUF_HEAD + 1) % KBD_BUF_SIZE;
            if next != KBD_BUF_TAIL {
                KBD_BUF[KBD_BUF_HEAD] = b;
                KBD_BUF_HEAD = next;
            }
        }
    }
}

fn kbd_buf_read() -> Option<u8> {
    unsafe {
        if KBD_BUF_TAIL != KBD_BUF_HEAD {
            let b = KBD_BUF[KBD_BUF_TAIL];
            KBD_BUF_TAIL = (KBD_BUF_TAIL + 1) % KBD_BUF_SIZE;
            Some(b)
        } else {
            None
        }
    }
}

fn sys_write_device(_fd: i32, buf: usize, count: usize, info: &process::DeviceInfo) -> i64 {
    match info.dev_type {
        process::DeviceType::Null => count as i64,
        process::DeviceType::Kbd => {
            // Keyboard driver writing scancodes
            let mut kbuf = [0u8; 64];
            let to_copy = count.min(64);
            if paging::copy_from_user(&mut kbuf[..to_copy], buf, to_copy).is_err() {
                return -1;
            }
            kbd_buf_write(&kbuf[..to_copy]);
            serial::write_str("[DEV] /dev/kbd write ");
            serial::write_usize(to_copy);
            serial::write_str(" bytes\n");
            to_copy as i64
        }
        process::DeviceType::Fb => {
            // Write to framebuffer via VGA text mode (0xB8000) or direct mapping
            let mut kbuf = alloc::vec![0u8; count.min(4096)];
            let to_copy = count.min(4096);
            if paging::copy_from_user(&mut kbuf[..to_copy], buf, to_copy).is_err() {
                return -1;
            }
            // For now, just log it
            serial::write_str("[DEV] /dev/fb0 write ");
            serial::write_usize(to_copy);
            serial::write_str(" bytes\n");
            to_copy as i64
        }
        process::DeviceType::Sda => {
            serial::write_str("[DEV] write not supported on /dev/sda0\n");
            -1
        }
    }
}

// ── SYS_GETPID ───────────────────────────────────────────────────────────

fn sys_getpid() -> u64 {
    process::current_process().map(|p| p.pid).unwrap_or(0)
}

// ── Scheduler helpers ────────────────────────────────────────────────────

fn saved_cs_from_rsp(current_rsp: u64) -> u64 {
    // After PUSH_REGS (15 regs × 8 = 120 bytes), the IRET frame sits above.
    // Layout: [rax][rcx][rdx][rsi][rdi][r8][r9][r10][r11][rbx][rbp][r12][r13][r14][r15]
    //         ^ RSP                                                                     ^
    // CS is at offset +128 from RSP (15×8=120 for regs, then RIP=+120, CS=+128)
    unsafe { (current_rsp as *const u64).add(16).read() }
}

fn sys_yield_switch(current_rsp: u64) -> u64 {
    unsafe {
        let cur = process::current_raw();
        if !cur.is_null() { (*cur).ticks_used = process::TIME_SLICE; }
    }
    let cs = saved_cs_from_rsp(current_rsp);
    process::schedule_tick(current_rsp, cs)
}

fn sys_sleep_switch(ticks: u64, current_rsp: u64) -> u64 {
    serial::write_str("[R3] SYS_SLEEP ");
    serial::write_usize(ticks as usize);
    serial::write_str(" ticks\n");
    if let Some(proc) = process::current_process() {
        let now = crate::time::pit::ticks();
        proc.sleep_until = now.wrapping_add(ticks);
        proc.state = process::ProcessState::Blocked;
    }
    unsafe {
        let cur = process::current_raw();
        if !cur.is_null() { (*cur).ticks_used = process::TIME_SLICE; }
    }
    let cs = saved_cs_from_rsp(current_rsp);
    process::schedule_tick(current_rsp, cs)
}

// ── SYS_READ ─────────────────────────────────────────────────────────────

fn sys_read(fd: i32, buf: usize, count: usize, current_rsp: u64) -> (i64, u64) {
    if fd < 0 || fd as usize >= process::MAX_FDS { return (-1, 0); }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return (-1, 0),
    };
    let entry = match process::fd_get(&proc, fd as usize) {
        Some(e) => e.clone(),
        None => return (-1, 0),
    };
    match &entry.fd_type {
        FdType::Stdin              => sys_read_stdin(buf, count, current_rsp),
        FdType::Stdout | FdType::Stderr => {
            serial::write_str("[SYS] READ: fd not readable\n");
            (-1, 0)
        }
        FdType::File(info) => {
            let info_clone = info.clone();
            match sys_read_file(fd, buf, count, info_clone) {
                Ok(n)  => (n, 0),
                Err(_) => (-1, 0),
            }
        }
        FdType::RamFile(ref info) => {
            sys_read_ramfile(fd, buf, count, info)
        }
        FdType::Device(ref info) => {
            sys_read_device(fd, buf, count, info)
        }
    }
}

fn sys_read_file(fd: i32, buf: usize, count: usize, info: OpenFileInfo) -> Result<i64, ()> {
    if info.pos >= info.size { return Ok(0); }
    let to_read = (count as u32).min(info.size - info.pos) as usize;
    if to_read == 0 { return Ok(0); }

    let mut kbuf = alloc::vec![0u8; to_read];
    let drive = registry::get_device(0).ok_or(())?;
    let mut vol = Fat32Volume::mount(drive).map_err(|_| ())?;

    let entry = DirEntryInfo {
        name:       info.name,
        name_len:   info.name_len,
        is_dir:     false,
        size:       info.size,
        cluster:    info.cluster,
        dir_sector: 0,
        dir_offset: 0,
    };

    let full_data = {
        let mut tmp = alloc::vec![0u8; info.size as usize];
        vol.read_file(&entry, &mut tmp).map_err(|_| ())?;
        tmp
    };
    drop(vol);

    let start = info.pos as usize;
    let end   = (start + to_read).min(full_data.len());
    let n     = end - start;
    kbuf[..n].copy_from_slice(&full_data[start..end]);

    paging::copy_to_user(buf, &kbuf[..n]).map_err(|_| ())?;

    if let Some(proc) = process::current_process() {
        if let Some(fd_entry) = process::fd_get_mut(proc, fd as usize) {
            if let FdType::File(ref mut fi) = fd_entry.fd_type {
                fi.pos = info.pos + n as u32;
            }
        }
    }

    serial::write_str("[SYS] READ file fd=");
    serial::write_usize(fd as usize);
    serial::write_str(" bytes=");
    serial::write_usize(n);
    serial::write_str("\n");

    Ok(n as i64)
}

// ── SYS_READ stdin ───────────────────────────────────────────────────────
//
// REGLA CRÍTICA: NUNCA leer del puerto PS/2 (0x60/0x64) directamente aquí.
// IRQ1 es el único propietario de 0x60. Si sys_read_stdin roba bytes del
// hardware compite con irq1_handler, desincroniza el estado del KBD del
// kernel y genera break-codes huérfanos → teclas fantasma → #UD → #DF.
//
// El único canal legítimo es el ring buffer SCANCODE_BUF llenado por IRQ1.
// Para esperar input sin quemar CPU usamos el mecanismo de blocking:
//   1. Marcar el proceso como Blocked (set_stdin_blocked).
//   2. Forzar reschedule vía schedule_tick → el scheduler saltará a otro
//      proceso; IRQ1 llamará wake_stdin_blocked cuando llegue un scancode.
//   3. Cuando este proceso retome, el loop reintenta pop_scancode.
//
// El KBD estático local es necesario porque syscall.rs no tiene acceso al
// KeyboardState del loop principal. SOLO procesa scancodes del ring buffer
// de IRQ1 — nunca del hardware directamente.

fn sys_read_stdin(buf: usize, count: usize, current_rsp: u64) -> (i64, u64) {
    if count == 0 { return (0, 0); }

    // KBD local para decodificar scancodes del ring buffer de IRQ1.
    // No comparte estado con el KBD del kernel — correcto, son contextos
    // distintos (ring-3 stdin vs UI del kernel).
    static mut KBD: KeyboardState = KeyboardState::new();

    // Buffer circular de caracteres ya decodificados, pendientes de entregar.
    static mut CHAR_BUF:  [u8; 256] = [0u8; 256];
    static mut CHAR_HEAD: usize = 0;  // próxima posición de escritura
    static mut CHAR_TAIL: usize = 0;  // próxima posición de lectura

    unsafe {
        // ── Fase 1: bombear scancodes → chars hasta tener al menos uno ──
        loop {
            // Drena todos los scancodes disponibles en el ring buffer de IRQ1
            loop {
                match pop_ring3_scancode() {
                    Some(sc) => {
                        if let Some(key) = KBD.feed_byte(sc) {
                            let ch: u8 = match key {
                                Key::Char(c)  => c,
                                Key::Enter    => b'\n',
                                Key::Backspace => 0x08,
                                Key::Tab      => b'\t',
                                _             => 0,
                            };
                            if ch != 0 {
                                let next = (CHAR_HEAD + 1) % CHAR_BUF.len();
                                if next != CHAR_TAIL {
                                    CHAR_BUF[CHAR_HEAD] = ch;
                                    CHAR_HEAD = next;
                                }
                            }
                        }
                    }
                    None => break,
                }
            }

            // Si hay caracteres disponibles, salir del loop de espera
            if CHAR_HEAD != CHAR_TAIL { break; }

            // Sin caracteres: bloquear el proceso y forzar reschedule.
            // IRQ1 → irq1_handler_rust → wake_stdin_blocked nos despertará
            // cuando llegue el próximo scancode.
            if let Some(cur) = process::current_process() {
                set_stdin_blocked(cur.pid);
                cur.state = process::ProcessState::Blocked;
            }

            // Guardar RSP actual y hacer reschedule cooperativo.
            // schedule_tick devuelve el RSP del siguiente proceso (o 0 si
            // no hay nadie más). En ambos casos, cuando este proceso retome
            // continuará en el siguiente tick desde aquí.
            let cs = saved_cs_from_rsp(current_rsp);
            let new_rsp = process::schedule_tick(current_rsp, cs);
            if new_rsp != 0 {
                // Hay otro proceso listo: devolver (0, new_rsp) para que
                // int80_handler/syscall_entry haga el context switch.
                // El proceso actual retomará en el próximo schedule_tick
                // que lo elija, con su RSP guardado.
                clear_stdin_blocked();
                return (0, new_rsp);
            }

            // No hay otros procesos: esperar con HLT (bajo consumo).
            // STI primero para que IRQ1 pueda despertar al proceso.
            // CLI inmediatamente después del HLT para volver a deshabilitar.
            core::arch::asm!("sti", options(nostack, nomem));
            core::arch::asm!("hlt", options(nostack, nomem));
            core::arch::asm!("cli", options(nostack, nomem));
            clear_stdin_blocked();
        }

        // ── Fase 2: copiar caracteres al buffer de usuario ───────────────
        let mut copied = 0usize;
        while CHAR_TAIL != CHAR_HEAD && copied < count {
            let ch = CHAR_BUF[CHAR_TAIL];
            CHAR_TAIL = (CHAR_TAIL + 1) % CHAR_BUF.len();
            if paging::copy_to_user(buf + copied, &[ch]).is_err() { break; }
            copied += 1;
        }

        serial::write_str("[SYS] READ stdin chars=");
        serial::write_usize(copied);
        serial::write_str("\n");

        (copied as i64, 0)
    }
}

// ── SYS_OPEN ─────────────────────────────────────────────────────────────

fn sys_open(path_ptr: usize, _flags: u32) -> i64 {
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_from_user(&mut path_buf, path_ptr, 256) {
        Ok(n)   => n,
        Err(()) => return -1,
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    // VFS routing: check if path belongs to devfs (/dev)
    if crate::drivers::storage::vfs::is_devfs_path(path) {
        return sys_open_devfs(path, &path_buf, actual_len);
    }

    // VFS routing: check if path belongs to a mounted ramfs (/tmp)
    if crate::drivers::storage::vfs::is_ramfs_path(path) {
        return sys_open_ramfs(path, &path_buf, actual_len);
    }

    serial::write_str("[SYS] OPEN path='");
    serial::write_str(path);
    serial::write_str("'\n");

    let drive = match registry::get_device(0) {
        Some(d) => d,
        None    => { serial::write_str("[SYS] OPEN: no device\n"); return -1; }
    };
    let mut vol = match Fat32Volume::mount(drive) {
        Ok(v)   => v,
        Err(_)  => { serial::write_str("[SYS] OPEN: mount failed\n"); return -1; }
    };

    let root = vol.root_cluster();
    let mut bufs = [[0u8; 64]; 16];
    let mut lens = [0usize; 16];
    let n = vfs::path_split(path, &mut bufs, &mut lens);
    if n == 0 {
        serial::write_str("[SYS] OPEN: empty path\n");
        return -1;
    }

    let mut cur = root;
    for i in 0..n {
        let comp = vfs::component_str(&bufs, &lens, i);
        if i == n - 1 {
            let entry = match vol.find_entry(cur, comp) {
                Ok(e)  => e,
                Err(_) => { serial::write_str("[SYS] OPEN: not found\n"); return -1; }
            };
            if entry.is_dir {
                serial::write_str("[SYS] OPEN: is a directory\n");
                return -1;
            }
            let info = OpenFileInfo {
                dir_cluster: cur,
                cluster:     entry.cluster,
                size:        entry.size,
                pos:         0,
                name:        entry.name,
                name_len:    entry.name_len,
            };
            drop(vol);
            let fd_entry = FdEntry { fd_type: FdType::File(info) };
            if let Some(proc) = process::current_process() {
                match process::fd_alloc(proc, fd_entry) {
                    Some(fd) => {
                        serial::write_str("[SYS] OPEN → fd=");
                        serial::write_usize(fd);
                        serial::write_str("\n");
                        return fd as i64;
                    }
                    None => { serial::write_str("[SYS] OPEN: no free fd\n"); return -1; }
                }
            }
            return -1;
        } else {
            match vol.find_entry(cur, comp) {
                Ok(e) if e.is_dir => cur = e.cluster,
                _ => { serial::write_str("[SYS] OPEN: path component not found\n"); return -1; }
            }
        }
    }
    -1
}

// ── RamFS helpers (Phase 10) ──────────────────────────────────────────

fn sys_open_ramfs(path: &str, path_buf: &[u8; 256], actual_len: usize) -> i64 {
    use crate::drivers::storage::vfs;
    let exists = vfs::with_ramfs(|ram| {
        if ram.is_dir(path) { return false; }
        ram.file_size(path).is_some()
    });
    if !exists {
        vfs::with_ramfs(|ram| ram.create(path));
    }
    let size = vfs::with_ramfs(|ram| ram.file_size(path).unwrap_or(0)) as u32;
    let mut rpath = [0u8; 256];
    rpath[..actual_len].copy_from_slice(&path_buf[..actual_len]);
    let info = process::RamFileInfo {
        path: rpath,
        path_len: actual_len,
        size,
        pos: 0,
    };
    let fd_entry = process::FdEntry { fd_type: process::FdType::RamFile(info) };
    if let Some(proc) = process::current_process() {
        match process::fd_alloc(proc, fd_entry) {
            Some(fd) => {
                serial::write_str("[RAMFS] OPEN -> fd=");
                serial::write_usize(fd);
                serial::write_str(" path='");
                serial::write_str(path);
                serial::write_str("'\n");
                return fd as i64;
            }
            None => { serial::write_str("[RAMFS] OPEN: no free fd\n"); return -1; }
        }
    }
    -1
}

fn sys_read_ramfile(fd: i32, buf: usize, count: usize, info: &process::RamFileInfo) -> (i64, u64) {
    if info.pos >= info.size { return (0, 0); }
    let to_read = (count as u32).min(info.size - info.pos) as usize;
    if to_read == 0 { return (0, 0); }

    let path_str = core::str::from_utf8(&info.path[..info.path_len]).unwrap_or("/tmp/f");
    let mut kbuf = alloc::vec![0u8; to_read];

    let n = crate::drivers::storage::vfs::with_ramfs(|ram| {
        match ram.read(path_str, &mut kbuf, info.pos as u64) {
            Ok(n) => n,
            Err(_) => 0,
        }
    });

    if n == 0 { return (0, 0); }
    if paging::copy_to_user(buf, &kbuf[..n]).is_err() { return (-1, 0); }

    if let Some(proc) = process::current_process() {
        if let Some(fd_entry) = process::fd_get_mut(proc, fd as usize) {
            if let process::FdType::RamFile(ref mut fi) = fd_entry.fd_type {
                fi.pos += n as u32;
            }
        }
    }

    serial::write_str("[RAMFS] READ fd=");
    serial::write_usize(fd as usize);
    serial::write_str(" bytes=");
    serial::write_usize(n);
    serial::write_str("\n");

    (n as i64, 0)
}

fn sys_getdents_ramfs(path: &str, buf: usize, count: usize) -> i64 {
    let mut entries: alloc::vec::Vec<([u8; 64], usize, bool)> = alloc::vec::Vec::new();
    crate::drivers::storage::vfs::with_ramfs(|ram| {
        let _ = ram.list_dir(path, &mut |name, is_dir| {
            let mut nb = [0u8; 64];
            let nl = name.len().min(63);
            nb[..nl].copy_from_slice(&name.as_bytes()[..nl]);
            entries.push((nb, nl, is_dir));
        });
    });

    let mut total = 0usize;
    for (name_bytes, name_len, is_dir) in &entries {
        let d_reclen = 19 + name_len + 1;
        if total + d_reclen > count { break; }
        let mut hdr = [0u8; 512];
        let d_type: u8 = if *is_dir { 2u8 } else { 1u8 };
        hdr[0..8].copy_from_slice(&0u64.to_le_bytes());
        hdr[8..16].copy_from_slice(&(total as u64).to_le_bytes());
        hdr[16..18].copy_from_slice(&(d_reclen as u16).to_le_bytes());
        hdr[18] = d_type;
        hdr[19..19 + name_len].copy_from_slice(&name_bytes[..*name_len]);
        if paging::copy_to_user(buf + total, &hdr[..d_reclen.min(512)]).is_err() { break; }
        total += d_reclen;
    }

    serial::write_str("[RAMFS] GETDIRENTS path='");
    serial::write_str(path);
    serial::write_str("' entries=");
    serial::write_usize(entries.len());
    serial::write_str(" bytes=");
    serial::write_usize(total);
    serial::write_str("\n");

    total as i64
}

fn sys_getdents_devfs(path: &str, buf: usize, count: usize) -> i64 {
    let _ = path;
    let devs = crate::drivers::storage::vfs::DEVFS_ENTRIES;
    let mut total = 0usize;
    for name in devs {
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len();
        let d_reclen = 19 + name_len + 1;
        if total + d_reclen > count { break; }
        let mut hdr = [0u8; 512];
        let d_type: u8 = 1u8; // DT_FILE
        hdr[0..8].copy_from_slice(&0u64.to_le_bytes());
        hdr[8..16].copy_from_slice(&(total as u64).to_le_bytes());
        hdr[16..18].copy_from_slice(&(d_reclen as u16).to_le_bytes());
        hdr[18] = d_type;
        hdr[19..19 + name_len].copy_from_slice(name_bytes);
        if paging::copy_to_user(buf + total, &hdr[..d_reclen.min(512)]).is_err() { break; }
        total += d_reclen;
    }

    serial::write_str("[DEVFS] GETDIRENTS entries=");
    serial::write_usize(devs.len());
    serial::write_str(" bytes=");
    serial::write_usize(total);
    serial::write_str("\n");

    total as i64
}

// ── DevFS helpers (Phase 12) ──────────────────────────────────────────

fn sys_open_devfs(path: &str, _path_buf: &[u8; 256], _actual_len: usize) -> i64 {
    let name = match crate::drivers::storage::vfs::resolve_devfs(path) {
        Some(n) => n,
        None => {
            serial::write_str("[DEVFS] no such device: '");
            serial::write_str(path);
            serial::write_str("'\n");
            return -1;
        }
    };

    let dev_type = match name {
        "kbd" => process::DeviceType::Kbd,
        "fb0" => process::DeviceType::Fb,
        "sda0" => process::DeviceType::Sda,
        "null" => process::DeviceType::Null,
        _ => {
            serial::write_str("[DEVFS] unknown device: ");
            serial::write_str(name);
            serial::write_str("\n");
            return -1;
        }
    };

    let info = process::DeviceInfo { dev_type, pos: 0 };
    let fd_entry = process::FdEntry { fd_type: process::FdType::Device(info) };
    if let Some(proc) = process::current_process() {
        match process::fd_alloc(proc, fd_entry) {
            Some(fd) => {
                serial::write_str("[DEVFS] OPEN '");
                serial::write_str(path);
                serial::write_str("' -> fd=");
                serial::write_usize(fd);
                serial::write_str("\n");
                return fd as i64;
            }
            None => {
                serial::write_str("[DEVFS] OPEN: no free fd\n");
                return -1;
            }
        }
    }
    -1
}

// ── SYS_CLOSE ────────────────────────────────────────────────────────────

fn sys_close(fd: i32) -> i64 {
    if fd < 0 || fd as usize >= process::MAX_FDS { return -1; }
    if let Some(mut proc) = process::current_process() {
        process::fd_close(&mut proc, fd as usize);
        serial::write_str("[SYS] CLOSE fd=");
        serial::write_usize(fd as usize);
        serial::write_str("\n");
        0
    } else {
        -1
    }
}

// ── SYS_DUP2 ─────────────────────────────────────────────────────────────

fn sys_dup2(oldfd: i32, newfd: i32) -> i64 {
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -1,
    };
    if oldfd < 0 || oldfd as usize >= process::MAX_FDS { return -1; }
    if newfd < 0 || newfd as usize >= process::MAX_FDS { return -1; }
    if oldfd == newfd { return newfd as i64; }

    let src = match process::fd_get(proc, oldfd as usize) {
        Some(e) => e.clone(),
        None => return -1,
    };

    process::fd_close(proc, newfd as usize);
    process::fd_alloc(proc, src);

    serial::write_str("[SYS] DUP2 oldfd=");
    serial::write_usize(oldfd as usize);
    serial::write_str(" newfd=");
    serial::write_usize(newfd as usize);
    serial::write_str("\n");

    newfd as i64
}

// ── SYS_UPTIME ───────────────────────────────────────────────────────────

fn sys_uptime() -> u64 {
    let ticks = crate::time::pit::ticks();
    serial::write_str("[SYS] UPTIME ticks=");
    serial::write_usize(ticks as usize);
    serial::write_str("\n");
    ticks
}

// ── SYS_BRK ──────────────────────────────────────────────────────────────

fn sys_brk(addr: usize) -> i64 {
    let proc = match process::current_process() {
        Some(p) => p,
        None    => return -1,
    };
    if addr == 0 { return proc.program_break as i64; }

    let cr3         = proc.cr3;
    let current_end = proc.program_break_end;
    let new_brk     = addr.max(process::PROGRAM_BREAK_BASE);

    if new_brk > current_end {
        let start_page = (current_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let end_page   = (new_brk   + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let pages      = (end_page - start_page) / PAGE_SIZE;
        for i in 0..pages {
            let vaddr = start_page + i * PAGE_SIZE;
            let paddr = match alloc_page() {
                Some(p) => p,
                None    => { serial::write_str("[SYS] BRK: OOM\n"); return proc.program_break as i64; }
            };
            if paging::map_page_user(cr3, vaddr, paddr).is_err() {
                return proc.program_break as i64;
            }
        }
    } else if new_brk < process::PROGRAM_BREAK_BASE {
        return -1;
    }

    proc.program_break = new_brk;
    if new_brk > proc.program_break_end { proc.program_break_end = new_brk; }

    serial::write_str("[SYS] BRK → ");
    serial::write_hex(new_brk);
    serial::write_str("\n");

    new_brk as i64
}

// ── SYS_MMAP ─────────────────────────────────────────────────────────────

fn sys_mmap(addr: usize, len: usize, _prot: u32, _flags: u32) -> i64 {
    if len == 0 { return -1; }

    let aligned_addr = if addr == 0 {
        process::current_process()
            .map(|p| (p.program_break_end + 0x10_0000 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
            .unwrap_or(0x3000_0000_0000)
    } else {
        addr & !(PAGE_SIZE - 1)
    };

    let pages = (len + PAGE_SIZE - 1) / PAGE_SIZE;
    let cr3 = match process::current_process() {
        Some(p) => p.cr3,
        None    => return -1,
    };

    for i in 0..pages {
        let vaddr = aligned_addr + i * PAGE_SIZE;
        let paddr = match alloc_page() {
            Some(p) => p,
            None    => return -1,
        };
        if paging::map_page_user(cr3, vaddr, paddr).is_err() { return -1; }
    }

    if let Some(proc) = process::current_process() {
        let end = aligned_addr + pages * PAGE_SIZE;
        if end > proc.program_break_end { proc.program_break_end = end; }
    }

    serial::write_str("[SYS] MMAP addr=");
    serial::write_hex(aligned_addr);
    serial::write_str(" pages=");
    serial::write_usize(pages);
    serial::write_str("\n");

    aligned_addr as i64
}

// ── SYS_GETDIRENTS ─────────────────────────────────────────────────────────

const DIRENT_HEADER_SIZE: usize = 19; // matches portix.h: d_ino(8)+d_off(8)+d_reclen(2)+d_type(1)
const DT_FILE: u8 = 1;
const DT_DIR: u8 = 2;

fn resolve_dir_cluster(path: &str, vol: &mut Fat32Volume, root: u32) -> Result<u32, ()> {
    if path.is_empty() || path == "/" { return Ok(root); }
    let mut bufs = [[0u8; 64]; 16];
    let mut lens = [0usize; 16];
    let n = crate::drivers::storage::vfs::path_split(path, &mut bufs, &mut lens);
    let mut cur = root;
    for i in 0..n {
        let comp = crate::drivers::storage::vfs::component_str(&bufs, &lens, i);
        let entry = vol.find_entry(cur, comp).map_err(|_| ())?;
        if !entry.is_dir { return Err(()); }
        cur = entry.cluster;
    }
    Ok(cur)
}

fn sys_getdents(path_ptr: usize, buf: usize, count: usize) -> i64 {
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_from_user(&mut path_buf, path_ptr, 256) {
        Ok(n) => n,
        Err(_) => return -1,
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    if crate::drivers::storage::vfs::is_ramfs_path(path) {
        return sys_getdents_ramfs(path, buf, count);
    }

    if crate::drivers::storage::vfs::is_devfs_path(path) {
        return sys_getdents_devfs(path, buf, count);
    }

    let drive = match registry::get_device(0) {
        Some(d) => d,
        None => return -1,
    };
    let mut vol = match Fat32Volume::mount(drive) {
        Ok(v) => v,
        Err(_) => return -1,
    };
    let root = vol.root_cluster();
    let dir_clus = match resolve_dir_cluster(path, &mut vol, root) {
        Ok(c) => c,
        Err(_) => return -1,
    };

    #[derive(Clone)]
    struct DirEntry {
        name: [u8; 256],
        name_len: usize,
        is_dir: bool,
    }
    let mut entries: alloc::vec::Vec<DirEntry> = alloc::vec::Vec::new();
    let _ = vol.list_dir(dir_clus, |e| {
        entries.push(DirEntry {
            name: e.name,
            name_len: e.name_len,
            is_dir: e.is_dir,
        });
    });

    let mut total = 0usize;
    for entry in &entries {
        let name_bytes = &entry.name[..entry.name_len];
        let reclen = DIRENT_HEADER_SIZE + name_bytes.len() + 1;
        if total + reclen > count { break; }

        let mut hdr = [0u8; 512];
        let hdr_len = reclen.min(512);
        // d_ino = 0
        // d_off = current offset
        let d_off = total as u64;
        let d_type: u8 = if entry.is_dir { DT_DIR } else { DT_FILE };
        let reclen16 = reclen as u16;

        hdr[0..8].copy_from_slice(&0u64.to_le_bytes());          // d_ino
        hdr[8..16].copy_from_slice(&d_off.to_le_bytes());         // d_off
        hdr[16..18].copy_from_slice(&reclen16.to_le_bytes());     // d_reclen
        hdr[18] = d_type;                                         // d_type
        hdr[19..19 + name_bytes.len()].copy_from_slice(name_bytes); // d_name

        if paging::copy_to_user(buf + total, &hdr[..hdr_len]).is_err() {
            break;
        }
        total += reclen;
    }

    serial::write_str("[SYS] GETDIRENTS path='");
    serial::write_str(path);
    serial::write_str("' entries=");
    serial::write_usize(entries.len());
    serial::write_str(" bytes=");
    serial::write_usize(total);
    serial::write_str("\n");

    total as i64
}

// ── SYS_EXECVE ─────────────────────────────────────────────────────────────

unsafe fn copy_user_str_array(arr_ptr: usize) -> alloc::vec::Vec<alloc::vec::Vec<u8>> {
    let mut result: alloc::vec::Vec<alloc::vec::Vec<u8>> = alloc::vec::Vec::new();
    if arr_ptr == 0 { return result; }
    let mut ptr = arr_ptr;
    loop {
        let mut ptr_bytes = [0u8; 8];
        if paging::copy_from_user(&mut ptr_bytes, ptr, 8).is_err() { break; }
        let s_ptr = u64::from_le_bytes(ptr_bytes) as usize;
        if s_ptr == 0 { break; }
        let mut buf = [0u8; 512];
        let copied = paging::copy_from_user(&mut buf, s_ptr, 512).unwrap_or(0);
        let len = buf.iter().position(|&b| b == 0).unwrap_or(copied);
        result.push(buf[..len].to_vec());
        ptr += 8;
    }
    result
}

fn sys_execve(path_ptr: usize, argv: usize, envp: usize, current_rsp: u64) -> SyscallResult {
    // 1. Copy path from user
    let mut path_buf = [0u8; 256];
    let path_len = match paging::copy_from_user(&mut path_buf, path_ptr, 256) {
        Ok(n) => n,
        Err(_) => return SyscallResult(-1i64 as u64, 0),
    };
    let actual_len = path_buf.iter().position(|&b| b == 0).unwrap_or(path_len);
    let path = core::str::from_utf8(&path_buf[..actual_len]).unwrap_or("");

    serial::write_str("[SYS] EXECVE path='");
    serial::write_str(path);
    serial::write_str("'\n");

    // 2. Read ELF file from FAT32
    let file_data = {
        let drive = match registry::get_device(0) {
            Some(d) => d,
            None => { serial::write_str("[EXEC] no device\n"); return SyscallResult(-1i64 as u64, 0); }
        };
        let mut vol = match Fat32Volume::mount(drive) {
            Ok(v) => v,
            Err(_) => { serial::write_str("[EXEC] mount failed\n"); return SyscallResult(-1i64 as u64, 0); }
        };
        let root = vol.root_cluster();
        let (dir_cluster, filename) = match crate::elf::resolve_path(&mut vol, root, path) {
            Ok(r) => r,
            Err(_) => { serial::write_str("[EXEC] path not found\n"); return SyscallResult(-1i64 as u64, 0); }
        };
        let entry = match vol.find_entry(dir_cluster, filename) {
            Ok(e) => e,
            Err(_) => { serial::write_str("[EXEC] file not found\n"); return SyscallResult(-1i64 as u64, 0); }
        };
        if entry.is_dir {
            serial::write_str("[EXEC] is a directory\n");
            return SyscallResult(-1i64 as u64, 0);
        }
        let size = entry.size as usize;
        let mut data = alloc::vec![0u8; size];
        if vol.read_file(&entry, &mut data).is_err() {
            serial::write_str("[EXEC] read failed\n");
            return SyscallResult(-1i64 as u64, 0);
        }
        data
    };

    // 3. Validate and parse ELF
    let info = match crate::elf::elf_load_raw(&file_data) {
        Ok(i) => i,
        Err(e) => { serial::write_str("[EXEC] "); serial::write_str(e); serial::write_str("\n"); return SyscallResult(-1i64 as u64, 0); }
    };

    // 4. Create new address space
    let new_cr3 = match paging::new_address_space() {
        Some(c) => c,
        None => { serial::write_str("[EXEC] OOM: new_address_space\n"); return SyscallResult(-1i64 as u64, 0); }
    };

    // 5. Load ELF segments into new CR3
    if let Err(e) = crate::elf::load_segments_into_cr3(new_cr3, &file_data, &info) {
        serial::write_str("[EXEC] segment load failed: ");
        serial::write_str(e);
        serial::write_str("\n");
        // Don't leak new_cr3 on failure, but this is best-effort during error
        return SyscallResult(-1i64 as u64, 0);
    }

    // 6. Copy argv/envp from old userspace
    let argv_strs = unsafe { copy_user_str_array(argv) };
    let envp_strs = unsafe { copy_user_str_array(envp) };

    // 7. Allocate and set up new user stack
    let us_size = crate::process::USER_STACK_SIZE;
    let us_layout = match core::alloc::Layout::from_size_align(us_size, crate::mem::paging::PAGE_SIZE) {
        Ok(layout) => layout,
        Err(_) => {
            serial::write_str("[EXEC] CRITICAL: invalid Layout for user stack\n");
            return SyscallResult(-1i64 as u64, 0);
        }
    };
    let us_ptr = unsafe { alloc::alloc::alloc_zeroed(us_layout) };
    if us_ptr.is_null() {
        serial::write_str("[EXEC] OOM: user stack\n");
        return SyscallResult(-1i64 as u64, 0);
    }
    let us_base = us_ptr as usize;
    let us_vaddr = crate::process::USER_STACK_TOP - us_size;

    // Map user stack in new CR3
    for i in 0..(us_size / crate::mem::paging::PAGE_SIZE) {
        if paging::map_page_user(new_cr3, us_vaddr + i * crate::mem::paging::PAGE_SIZE, us_base + i * crate::mem::paging::PAGE_SIZE).is_err() {
            serial::write_str("[EXEC] stack map failed\n");
            return SyscallResult(-1i64 as u64, 0);
        }
    }

    // Build user stack in kernel heap
    let stack_slice = unsafe { core::slice::from_raw_parts_mut(us_ptr as *mut u8, us_size) };
    let mut sp = us_size;

    macro_rules! push_u64 {
        ($val:expr) => {{
            let bytes = $val.to_le_bytes();
            sp = sp.wrapping_sub(8);
            stack_slice[sp..sp + 8].copy_from_slice(&bytes);
        }};
    }
    
    // ── Phase 1: Place all strings ──
    // Envp strings (preserve order)
    let mut envp_str_addrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    for s in &envp_strs {
        let total_len = s.len() + 1; // +1 null
        sp = sp.wrapping_sub(total_len);
        stack_slice[sp..sp + s.len()].copy_from_slice(s);
        stack_slice[sp + s.len()] = 0;
        envp_str_addrs.push((us_vaddr + sp) as u64);
    }
    
    // Argv strings (preserve order)
    let mut argv_str_addrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    for s in &argv_strs {
        let total_len = s.len() + 1;
        sp = sp.wrapping_sub(total_len);
        stack_slice[sp..sp + s.len()].copy_from_slice(s);
        stack_slice[sp + s.len()] = 0;
        argv_str_addrs.push((us_vaddr + sp) as u64);
    }
    
    // Align sp to 8 bytes
    sp &= !7;
    
    // ── Phase 2: Push envp pointers (NULL terminated) ──
    // Push NULL first (highest address), then envp[n-1]..envp[0] so that
    // envp[0] ends up at the lowest address (where crt0.s expects it).
    push_u64!(0u64); // envp NULL terminator
    for addr in envp_str_addrs.iter().rev() {
        push_u64!(*addr);
    }
    
    // ── Phase 3: Push argv pointers (NULL terminated) ──
    push_u64!(0u64); // argv NULL terminator
    for addr in argv_str_addrs.iter().rev() {
        push_u64!(*addr);
    }
    
    // ── Phase 4: Push argc ──
    let argc = argv_strs.len() as u64;
    push_u64!(argc);
    
    let user_rsp = (us_vaddr + sp) as u64;

    // 8. Free old address space and user stack
    let proc = match process::current_process() {
        Some(p) => p,
        None => { serial::write_str("[EXEC] no current process\n"); return SyscallResult(-1i64 as u64, 0); }
    };

    let old_cr3 = proc.cr3;
    let old_us_phys = proc.user_stack_phys;

    // Close all FDs except stdin/stdout/stderr
    for i in 3..process::MAX_FDS {
        proc.fds[i] = None;
    }

    // Update process struct
    proc.cr3 = new_cr3;
    proc.user_rip = info.entry;
    proc.user_rsp = user_rsp;
    proc.user_stack_phys = us_base;
    proc.program_break = process::PROGRAM_BREAK_BASE;
    proc.program_break_end = process::PROGRAM_BREAK_BASE;

    // Update name
    let name_bytes = path_buf[..actual_len.min(31)].to_vec(); // already have path in buffer
    proc.name_len = actual_len.min(31);
    proc.name[..proc.name_len].copy_from_slice(&name_bytes);

    // Free old resources
    if old_us_phys != 0 {
        let old_layout = match core::alloc::Layout::from_size_align(us_size, crate::mem::paging::PAGE_SIZE) {
            Ok(layout) => layout,
            Err(_) => {
                serial::write_str("[EXEC] CRITICAL: invalid Layout for old user stack dealloc\n");
                return SyscallResult(-1i64 as u64, 0);
            }
        };
        unsafe { alloc::alloc::dealloc(old_us_phys as *mut u8, old_layout); }
    }
    if old_cr3 != 0 && old_cr3 != paging::read_cr3() {
        paging::free_address_space(old_cr3);
    }

    // 9. Modify IRET frame on current kernel stack:
    //    After PUSH_REGS (15 regs × 8 = 120 bytes), the IRET frame:
    //    [rsp+120] = RIP, [rsp+128] = CS, [rsp+136] = RFLAGS, [rsp+144] = RSP, [rsp+152] = SS
    unsafe {
        let iret_rip  = (current_rsp as *mut u64).add(120 / 8);
        let iret_rsp  = (current_rsp as *mut u64).add(144 / 8);
        let iret_cs   = (current_rsp as *mut u64).add(128 / 8);
        let iret_ss   = (current_rsp as *mut u64).add(152 / 8);
        let iret_rflags = (current_rsp as *mut u64).add(136 / 8);
        *iret_rip = info.entry;
        *iret_rsp = user_rsp;
        *iret_cs = 0x23u64;    // USER_CS | 3
        *iret_ss = 0x1Bu64;    // USER_DS | 3
        *iret_rflags = 0x202u64; // IF=1
    }

    // 10. Switch to new CR3
    let current_cr3 = paging::read_cr3();
    if new_cr3 != current_cr3 {
        paging::write_cr3(new_cr3);
    }

    serial::write_str("[SYS] EXECVE done: entry=");
    serial::write_hex(info.entry as usize);
    serial::write_str(" rsp=");
    serial::write_hex(user_rsp as usize);
    serial::write_str(" argc=");
    serial::write_usize(argc as usize);
    serial::write_str("\n");

    // Return (0, 0): no context switch, but modified IRET frame redirects to new program
    SyscallResult(0, 0)
}

// ── SYS_SEND ───────────────────────────────────────────────────────────────

fn sys_send(dst_pid: u64, msg_type: u64, data_ptr: u64, data_len: u64) -> i64 {
    let src_pid = match process::current_process() {
        Some(p) => p.pid,
        None => return -1,
    };

    let len = (data_len as usize).min(crate::ipc::IPC_DATA_SIZE);
    let mut kbuf = [0u8; crate::ipc::IPC_DATA_SIZE];
    if len > 0 {
        if paging::copy_from_user(&mut kbuf[..len], data_ptr as usize, len).is_err() {
            return -1;
        }
    }

    crate::ipc::send(src_pid, dst_pid, msg_type, &kbuf[..len])
}

// ── SYS_RECV ───────────────────────────────────────────────────────────────

fn sys_recv(buf: usize, len: usize, current_rsp: u64) -> (i64, u64) {
    let pid = match process::current_process() {
        Some(p) => p.pid,
        None => return (-1, 0),
    };

    let mut kbuf = [0u8; 64];
    let res = crate::ipc::recv(pid, &mut kbuf);
    match res {
        0 => {
            let copy_len = len.min(64);
            if paging::copy_to_user(buf, &kbuf[..copy_len]).is_err() {
                return (-1, 0);
            }
            (copy_len as i64, 0)
        }
        1 => {
            if let Some(proc) = process::current_process() {
                proc.state = process::ProcessState::Blocked;
                proc.sleep_until = 1;
            }
            let cs = saved_cs_from_rsp(current_rsp);
            let new_rsp = process::schedule_tick(current_rsp, cs);
            if new_rsp != 0 {
                return (0, new_rsp);
            }
            unsafe { core::arch::asm!("sti", options(nostack, nomem)); }
            unsafe { core::arch::asm!("hlt", options(nostack, nomem)); }
            unsafe { core::arch::asm!("cli", options(nostack, nomem)); }
            (0, 0)
        }
        _ => (-1, 0),
    }
}

// ── SYS_REG_IRQ ────────────────────────────────────────────────────────────

fn sys_reg_irq(irq: u64, pid: u64) -> i64 {
    crate::ipc::register_irq(irq as usize, pid)
}

// ── SYS_BLOCK_READ ──────────────────────────────────────────────────────────

fn sys_block_read(dev_id: i32, lba: u64, count: u64, buf: u64) -> i64 {
    if buf == 0 || count == 0 { return -1; }
    let total_bytes = (count as usize) * 512;
    if total_bytes > 65536 { return -1; }

    let mut kbuf = alloc::vec![0u8; total_bytes];
    let drive = match crate::drivers::storage::registry::get_device(dev_id as usize) {
        Some(d) => d,
        None => {
            serial::write_str("[BLK] dev not found\n");
            return -1;
        }
    };

    let n = (count as usize).min(drive.total_sectors() as usize);
    let actual_bytes = n * 512;
    kbuf.resize(actual_bytes, 0);

    if drive.read_sectors(lba, n, &mut kbuf).is_err() {
        serial::write_str("[BLK] read error\n");
        return -1;
    }

    if paging::copy_to_user(buf as usize, &kbuf[..actual_bytes]).is_err() {
        return -1;
    }

    serial::write_str("[BLK] ata");
    serial::write_usize(dev_id as usize);
    serial::write_str(": read LBA=");
    serial::write_usize(lba as usize);
    serial::write_str(" count=");
    serial::write_usize(n);
    serial::write_str(" -> ");
    serial::write_usize(actual_bytes);
    serial::write_str(" bytes\n");

    actual_bytes as i64
}

// ── SYS_IOPORT ───────────────────────────────────────────────────────────

/// Ports that DRIVERS are allowed to register (whitelist).
/// Sensitive kernel ports (PIC 0x20/0xA0, PIT 0x40, CMOS 0x70) are DENIED.
const DENY_PORTS: &[u16] = &[0x20, 0x21, 0xA0, 0xA1, 0x40, 0x43, 0x70, 0x71];

fn sys_ioport(port: u16, enable: u8) -> i64 {
    if DENY_PORTS.contains(&port) {
        serial::write_str("[IOPORT] DENIED port 0x");
        serial::write_hex(port as usize);
        serial::write_str("\n");
        return -1; // -EPERM
    }
    let proc = match process::current_process() {
        Some(p) => p,
        None => return -1,
    };
    if enable != 0 {
        if proc.registered_port_count >= 16 {
            return -1;
        }
        for i in 0..proc.registered_port_count {
            if proc.registered_ports[i] == port {
                return 0; // already registered
            }
        }
        proc.registered_ports[proc.registered_port_count] = port;
        proc.registered_port_count += 1;
    } else {
        for i in 0..proc.registered_port_count {
            if proc.registered_ports[i] == port {
                proc.registered_ports[i] = proc.registered_ports[proc.registered_port_count - 1];
                proc.registered_port_count -= 1;
                break;
            }
        }
    }
    serial::write_str("[IOPORT] PID ");
    serial::write_usize(proc.pid as usize);
    serial::write_str(" port 0x");
    serial::write_hex(port as usize);
    serial::write_str(" enable=");
    serial::write_usize(enable as usize);
    serial::write_str("\n");
    0
}

fn port_is_registered(port: u16) -> bool {
    match process::current_process() {
        Some(proc) => {
            for i in 0..proc.registered_port_count {
                if proc.registered_ports[i] == port {
                    return true;
                }
            }
            false
        }
        None => false,
    }
}

// ── SYS_IOREAD ───────────────────────────────────────────────────────────

fn sys_ioread(port: u16) -> i64 {
    if !port_is_registered(port) {
        serial::write_str("[IOREAD] DENIED port 0x");
        serial::write_hex(port as usize);
        serial::write_str("\n");
        return -1;
    }
    let val: u8;
    unsafe {
        core::arch::asm!("in al, dx", out("al") val, in("dx") port, options(nostack, nomem));
    }
    serial::write_str("[IOREAD] port 0x");
    serial::write_hex(port as usize);
    serial::write_str(" -> 0x");
    serial::write_hex(val as usize);
    serial::write_str("\n");
    val as i64
}

// ── SYS_IOWRITE ──────────────────────────────────────────────────────────

fn sys_iowrite(port: u16, value: u8) -> i64 {
    if !port_is_registered(port) {
        serial::write_str("[IOWRITE] DENIED port 0x");
        serial::write_hex(port as usize);
        serial::write_str("\n");
        return -1;
    }
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nostack, nomem));
    }
    serial::write_str("[IOWRITE] port 0x");
    serial::write_hex(port as usize);
    serial::write_str(" <- 0x");
    serial::write_hex(value as usize);
    serial::write_str("\n");
    0
}

// ── SYS_MMAP_DEVICE ─────────────────────────────────────────────────────

fn sys_mmap_device(phys: usize, size: usize) -> i64 {
    if phys == 0 || size == 0 || size > 0x100_0000 {
        serial::write_str("[MMAP_DEV] invalid params\n");
        return -1;
    }
    // Only allow device memory regions:
    //   - VGA: 0xA0000-0xBFFFF
    //   - Framebuffer/PCI MMIO: >= 0xE000_0000
    //   - Also allow framebuffer at 0xFD00_0000 (QEMU stdvga)
    let allow = (phys >= 0xA0000 && phys < 0xC0000)
             || (phys >= 0xE000_0000)
             || (phys >= 0xFC00_0000 && phys < 0xFE00_0000);
    if !allow {
        serial::write_str("[MMAP_DEV] DENIED phys 0x");
        serial::write_hex(phys);
        serial::write_str(" (not device region)\n");
        return -1;
    }

    let cr3 = match process::current_process() {
        Some(p) => p.cr3,
        None => return -1,
    };

    let pages = (size + paging::PAGE_SIZE - 1) / paging::PAGE_SIZE;
    let vaddr = 0x4000_0000_0000usize; // fixed device mapping region

    for i in 0..pages {
        let vaddr_i = vaddr + i * paging::PAGE_SIZE;
        let paddr_i = (phys + i * paging::PAGE_SIZE) as u64;
        // Map with no-execute to prevent code execution from device memory
        if paging::map_page(cr3, vaddr_i, paddr_i as usize,
            paging::PRESENT | paging::WRITABLE | paging::USER | paging::ACCESSED | paging::NO_EXECUTE).is_err() {
            serial::write_str("[MMAP_DEV] map failed\n");
            return -1;
        }
    }

    serial::write_str("[MMAP_DEV] phys 0x");
    serial::write_hex(phys);
    serial::write_str(" -> vaddr 0x");
    serial::write_hex(vaddr);
    serial::write_str(" pages=");
    serial::write_usize(pages);
    serial::write_str("\n");

    vaddr as i64
}

// ── Device file read (for /dev/kbd, /dev/sda0) ─────────────────────────

fn sys_read_device(fd: i32, buf: usize, count: usize, info: &process::DeviceInfo) -> (i64, u64) {
    match info.dev_type {
        process::DeviceType::Null => (0, 0),
        process::DeviceType::Kbd => {
            // Read scancodes from the /dev/kbd ring buffer
            let mut copied = 0usize;
            let mut temp = [0u8; 64];
            while copied < count.min(64) {
                match kbd_buf_read() {
                    Some(b) => {
                        temp[copied] = b;
                        copied += 1;
                    }
                    None => break,
                }
            }
            if copied > 0 {
                if paging::copy_to_user(buf, &temp[..copied]).is_err() {
                    return (-1, 0);
                }
            }
            serial::write_str("[DEV] /dev/kbd read ");
            serial::write_usize(copied);
            serial::write_str(" scancodes\n");
            (copied as i64, 0)
        }
        process::DeviceType::Sda => {
            // Block device read via the existing block device
            let pos = info.pos;
            let lba = pos / 512;
            let count_sectors = ((count + 511) / 512).min(128);
            let total_bytes = count_sectors * 512;
            let mut kbuf = alloc::vec![0u8; total_bytes];
            // Use the internal block device directly
            if let Some(drive) = crate::drivers::storage::registry::get_device(0) {
                if drive.read_sectors(lba as u64, count_sectors, &mut kbuf).is_err() {
                    return (-1, 0);
                }
                let offset = (pos % 512) as usize;
                let to_copy = (total_bytes - offset).min(count);
                if paging::copy_to_user(buf, &kbuf[offset..offset + to_copy]).is_err() {
                    return (-1, 0);
                }
                // Update position
                if let Some(proc) = process::current_process() {
                    if let Some(fde) = process::fd_get_mut(proc, fd as usize) {
                        if let process::FdType::Device(ref mut di) = fde.fd_type {
                            di.pos += to_copy as u32;
                        }
                    }
                }
                serial::write_str("[DEV] /dev/sda0 read LBA=");
                serial::write_usize(lba as usize);
                serial::write_str(" bytes=");
                serial::write_usize(to_copy);
                serial::write_str("\n");
                (to_copy as i64, 0)
            } else {
                (-1, 0)
            }
        }
        process::DeviceType::Fb => {
            // Reading from framebuffer: return 0 bytes
            (0, 0)
        }
    }
}