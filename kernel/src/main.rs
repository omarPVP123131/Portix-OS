// kernel/src/main.rs — PORTIX Kernel v0.14.0
//
// FIXES:
//   - ctrl leído desde kbd.ctrl() en lugar de hardcoded false
//   - ide_handle_click() integrado para clicks en menubar del IDE
//   - explorer: preview se carga tras mover selección
//   - variables ide/explorer: binding único sin doble-shadow
//   - unused_unsafe: init_page_pool ya es safe (el unsafe está dentro)

#![no_std]
#![no_main]
#![allow(dead_code)]
#![allow(static_mut_refs)]
#![feature(alloc_error_handler)]
#![feature(allocator_api)]

extern crate alloc;

pub mod arch;
pub mod bootinfo;
pub mod console;
pub mod drivers;
pub mod graphics;
pub mod elf;
pub mod mem;
pub mod process;
pub mod syscall;
pub mod time;
pub mod ui;
pub mod util;

use console::terminal::editor::draw_editor_tab;
use console::terminal::LineColor;
use core::arch::global_asm;
use drivers::input::keyboard::Key;
use alloc::boxed::Box;
use crate::drivers::serial;
use drivers::storage::traits::BlockDevice;
use drivers::storage::ata::DriveType;
use drivers::storage::{ata, atapi, fat32, mkfs, registry};
use graphics::driver::framebuffer::{Color, Console, Layout};
use mem::allocator::BuddyAllocator;
use ui::tabs::explorer::ExplorerState;
use ui::tabs::ide::{init_page_pool, IdeState, MenuState, MENUS};
use ui::tabs::ide::{MENU_H as IDE_MENU_H, STATUS_H as IDE_STATUS_H, TABS_H as IDE_TABS_H};
use ui::{
    draw_chrome, draw_devices_tab, draw_explorer_tab, draw_ide_tab, draw_system_tab,
    draw_terminal_tab, terminal_hist_geometry, Tab, SCROLLBAR_W,
};

#[no_mangle]
extern "Rust" fn __rust_alloc_error_handler(size: usize, align: usize) -> ! {
    panic!("OOM: size={} align={}", size, align);
}

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
    pub static __stack_top: u8;
}

global_asm!(
    ".section .text._start, \"ax\"",
    ".global _start",
    ".code64",
    "_start:",
    "    cli",
    "    cld",
    "    mov r12, rdi",
    "    lea rsp, [rip + {STACK_TOP}]",
    "    xor rbp, rbp",
    "    lea rdi, [rip + {BSS_START}]",
    "    lea rcx, [rip + {BSS_END}]",
    "    sub rcx, rdi",
    "    jz 1f",
    "    test rcx, rcx",
    "    js  1f",
    "    xor eax, eax",
    "    rep stosb",
    "1:",
    "    mov rdi, r12",
    "    call {RUST_MAIN}",
    "2:  hlt",
    "    jmp 2b",
    STACK_TOP = sym __stack_top,
    BSS_START = sym __bss_start,
    BSS_END   = sym __bss_end,
    RUST_MAIN = sym rust_main,
);

#[global_allocator]
static ALLOCATOR: BuddyAllocator = BuddyAllocator::new();

// ── Constantes ────────────────────────────────────────────────────────────────

const RENDER_HZ: u64 = 30;
const RENDER_INTERVAL: u64 = 100 / RENDER_HZ;

unsafe fn init_cpu_features() {
    let mut cr0: u64;
    core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nostack, nomem));
    cr0 &= !(1 << 2); // EM=0
    cr0 |= 1 << 1;    // MP=1
    core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nostack, nomem));

    let mut cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nostack, nomem));
    cr4 |= (1 << 9) | (1 << 10); // OSFXSR + OSXMMEXCPT
    core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nostack, nomem));
}


// ── Statics BSS ──────────────────────────────────────────────────────────────

static mut IDE_STORAGE: core::mem::MaybeUninit<IdeState> = core::mem::MaybeUninit::uninit();
static mut EXPLORER_STORAGE: core::mem::MaybeUninit<ExplorerState> =
    core::mem::MaybeUninit::uninit();

// ── Hit-test de la menubar del IDE ────────────────────────────────────────────
//
// Devuelve el índice del menú clickeado (-1 si no hay hit).
// Tiene que coincidir con los anchos que calcula draw_ide_tab en ide.rs.
fn ide_menubar_hit(mx: i32, my: i32, content_y: usize, font_w: usize) -> i32 {
    let y = my as usize;
    if y < content_y || y >= content_y + IDE_MENU_H {
        return -1;
    }
    let mut x_pos = 8usize; // cambió de 6 a 8
    for (i, menu) in MENUS.iter().enumerate() {
        let label_w = menu.title.len() * font_w + 14; // cambió de 16 a 14
        let x = mx as usize;
        if x >= x_pos && x < x_pos + label_w {
            return i as i32;
        }
        x_pos += label_w + 2;
    }
    -1
}
fn ide_help_btn_hit(mx: i32, my: i32, content_y: usize, fw: usize, font_w: usize) -> bool {
    let y = my as usize;
    if y < content_y || y >= content_y + IDE_MENU_H {
        return false;
    }
    let help_x = fw.saturating_sub(font_w * 3 + 12);
    let x = mx as usize;
    x >= help_x && x < fw
}

fn exp_help_btn_hit(mx: i32, my: i32, content_y: usize, fw: usize, font_w: usize) -> bool {
    use ui::tabs::explorer::TOOLBAR_H;
    let y = my as usize;
    let x = mx as usize;
    if y < content_y || y >= content_y + TOOLBAR_H {
        return false;
    }
    let hx = fw.saturating_sub(font_w * 2 + 14);
    x >= hx && x < fw
}

// Hit-test dentro del dropdown abierto. Devuelve el índice del item (-1 si no).
fn ide_dropdown_hit(mx: i32, my: i32, menu_idx: usize, content_y: usize, font_w: usize) -> i32 {
    if menu_idx >= MENUS.len() {
        return -1;
    }
    let menu = &MENUS[menu_idx];

    let mut x_pos = 6usize;
    for i in 0..menu_idx {
        x_pos += MENUS[i].title.len() * font_w + 18;
    }

    let max_label = menu
        .items
        .iter()
        .map(|it| it.label.len())
        .max()
        .unwrap_or(10);
    let max_short = menu
        .items
        .iter()
        .map(|it| it.shortcut.len())
        .max()
        .unwrap_or(0);
    let dd_w = (max_label + max_short + 6) * font_w + 16;
    let dd_x = x_pos;
    let dd_y = content_y + IDE_MENU_H;

    let x = mx as usize;
    let y = my as usize;
    if x < dd_x || x >= dd_x + dd_w {
        return -1;
    }

    const ITEM_H: usize = 16;
    let rel_y = y.wrapping_sub(dd_y + 3);
    let item = rel_y / ITEM_H;
    if item < menu.items.len() && y >= dd_y + 3 {
        item as i32
    } else {
        -1
    }
}

// ── Punto de entrada ──────────────────────────────────────────────────────────

#[no_mangle]
extern "C" fn rust_main(boot_info: *const bootinfo::PortixBootInfo) -> ! {
    unsafe { bootinfo::init(boot_info); }
    unsafe { init_cpu_features(); }

    // Verificar que SSE está habilitado correctamente
unsafe {
    let cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nostack, nomem));
    drivers::serial::write_str("[CPU] CR4=");
    drivers::serial::write_hex(cr4 as usize);
    drivers::serial::write_str("\n");
    // Debe tener bits 9 y 10 (OSFXSR=0x200, OSXMMEXCPT=0x400)
    if cr4 & 0x600 != 0x600 {
        drivers::serial::write_str("[CPU] WARN: SSE no habilitado correctamente\n");
    }
}

    drivers::serial::init();  // serial antes que nada para debug

    // IDT primero — sin esto cualquier excepción = triple fault opaco
    unsafe { arch::idt::init_idt(); }  // instala GDT+TSS+IDT y hace STI

    unsafe { ALLOCATOR.init(); }
    mem::paging::init();
    process::init();
    init_page_pool();
    time::pit::init();
    drivers::serial::log("PIT", "temporizador 100 Hz");

    let hw = arch::hardware::HardwareInfo::detect_all();
    drivers::serial::log("HW", hw.cpu.brand_str());

    let pci = drivers::bus::pci::PciBus::scan();
    {
        let mut t = [0u8; 16];
        let s = util::fmt::fmt_u32(pci.count as u32, &mut t);
        drivers::serial::write_str("PCI: ");
        drivers::serial::write_str(s);
        drivers::serial::write_str(" dispositivos\n");
    }

    let mut kbd = drivers::input::keyboard::KeyboardState::new();
    let mut ms = drivers::input::mouse::MouseState::new();
    let mut c = Console::new();
    let lay = Layout::new(c.width(), c.height());
    ms.init(lay.fw.max(1), lay.fh.max(1));

    // ── Drenar FIFO PS/2 ──────────────────────────────────────────────
    // Los bytes basura en el FIFO del 8042 (p.ej. BAT completion del
    // teclado durante el boot) bloquean la generacion de nuevas IRQs: el
    // 8042 solo genera IRQ1/IRQ12 en la transicion vacio→no-vacio.
    // Si el FIFO contiene bytes viejos, las IRQs nunca se disparan.
    unsafe {
        let mut drained = 0u32;
        for _ in 0..256 {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x01 == 0 { break; }
            let _byte: u8;
            core::arch::asm!("in al, dx", out("al") _byte, in("dx") 0x60u16, options(nostack));
            drained += 1;
        }
        if drained > 0 {
            crate::drivers::serial::write_str("[PS2] drained ");
            crate::drivers::serial::write_usize(drained as usize);
            crate::drivers::serial::write_str(" stale bytes\n");
        }

        // Segunda fase: habilitar reloj del teclado (bit4=0) + traduccion (bit6=1)
        // La primera fase en mouse.rs deshabilito el reloj para evitar interferencia.
        loop {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x02 == 0 { break; }
            core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
        }
        core::arch::asm!("out dx, al", in("dx") 0x64u16, in("al") 0x60u8, options(nostack));
        core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
        loop {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x02 == 0 { break; }
            core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
        }
        // 0x43 = IRQ1+IRQ12+translation ON, keyboard+mouse clock enabled
        core::arch::asm!("out dx, al", in("dx") 0x60u16, in("al") 0x43u8, options(nostack));
        crate::drivers::serial::log("PS2", "config 0x43 (kbd clock + translation ON)");

        // Habilitar scanning del teclado (0xF4)
        // El ACK (0xFA) llega via IRQ1 → SCANCODE_BUF (se ignora)
        loop {
            let st: u8;
            core::arch::asm!("in al, dx", out("al") st, in("dx") 0x64u16, options(nostack));
            if st & 0x02 == 0 { break; }
            core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
        }
        core::arch::asm!("out dx, al", in("dx") 0x60u16, in("al") 0xF4u8, options(nostack, nomem));
        crate::drivers::serial::log("KBD", "enable cmd sent (0xF4)");
    }

    let mut term = console::terminal::Terminal::new();
    term.write_line("PORTIX v0.7.4  Kernel Bare-Metal", LineColor::Header);
    term.write_line("Escribe 'ayuda' para comandos.", LineColor::Info);
    term.write_empty();

    // Inicializar grandes estructuras en BSS (no stack)
    unsafe {
        core::ptr::addr_of_mut!(IDE_STORAGE).write(core::mem::MaybeUninit::new(IdeState::new()));
        core::ptr::addr_of_mut!(EXPLORER_STORAGE)
            .write(core::mem::MaybeUninit::new(ExplorerState::new(2)));
    }

    let mut mount_ok = false;
    let mut root_cluster = 2u32;

    {
        let bus = ata::AtaBus::scan();
        ata::log_drives(&bus);

        // Cachear info del Primary0 para comandos sin re-escanear hardware
        if let Some(info) = bus.info(ata::DriveId::Primary0) {
            ata::store_primary_drive_info(*info);
        }

        // Registrar todos los drives en el DeviceRegistry
        for info in bus.iter() {
            let dev: Box<dyn BlockDevice> = if info.kind == DriveType::Atapi {
                Box::new(atapi::AtapiDrive::new(*info))
            } else {
                Box::new(ata::AtaDrive::from_info(*info))
            };
            registry::register_device(dev);
        }

        // Montar FAT32 o formatear si no existe (usa device 0 = Primary0)
        let dev0_kind = registry::get_device(0).map(|d| d.device_info().kind);

        // Si device 0 es ATAPI (CD-ROM), mostrar guía en vez de montar/formatear
        if let Some(DriveType::Atapi) = dev0_kind {
            serial::log_level(serial::Level::Info, "BOOT",
                "Boot desde CD-ROM detectado. Ejecuta 'install' para instalar PORTIX en disco duro.");
        }

        serial::write_str("[FS] dev0_kind=");
        serial::write_usize(match dev0_kind { Some(DriveType::Ata) => 1, Some(DriveType::Atapi) => 2, None => 0 });
        serial::write_str("\n");
        if let Some(DriveType::Ata) = dev0_kind {
            serial::write_str("[FS] Attempting FAT32 mount...\n");
            if let Some(drive) = registry::get_device(0) {
                serial::write_str("[FS] Got drive, mounting...\n");
                let mut mbr = [0u8; 512];
                if drive.read_sectors(0, 1, &mut mbr).is_ok() {
                    serial::write_str("[FS] MBR read OK, sig=");
                    serial::write_hex(mbr[510] as usize);
                    serial::write_hex(mbr[511] as usize);
                    serial::write_str("\n");
                    serial::write_str("[FS] P1 type=");
                    serial::write_hex(mbr[0x1BE+4] as usize);
                    serial::write_str(" P2 type=");
                    serial::write_hex(mbr[0x1CE+4] as usize);
                    serial::write_str("\n");
                }
                if let Ok(vol) = fat32::Fat32Volume::mount(drive) {
                    root_cluster = vol.root_cluster();
                    mount_ok = true;
                    serial::log("FS", "FAT32 montado correctamente");
                } else {
                    serial::log("FS", "FAT32 no encontrado en particion");
                }
            }
            serial::write_str("[FS] mount_ok=");
            serial::write_usize(mount_ok as usize);
            serial::write_str("\n");
        }
        serial::write_str("[FS] After mount block, mount_ok=");
        serial::write_usize(mount_ok as usize);
        serial::write_str("\n");

        if !mount_ok {
            if let Some(drive) = registry::get_device(0) {
                let total_secs = drive.total_sectors();
                if mkfs::auto_format(drive, total_secs).is_some() {
                    if let Some(drive) = registry::get_device(0) {
                        if let Ok(vol) = fat32::Fat32Volume::mount(drive) {
                            root_cluster = vol.root_cluster();
                        }
                    }
                }
            }
        }

        // Quick FAT32 test: list directories
        if mount_ok {
            if let Some(drive) = registry::get_device(0) {
                if let Ok(mut vol) = fat32::Fat32Volume::mount(drive) {
                    serial::write_str("[FS] mount_ok, testing list...\n");
                    fat32_list_dir(&mut vol, "bin");
                    fat32_list_dir(&mut vol, "home");
                    fat32_list_dir(&mut vol, "etc");
                }
            }
        }

        unsafe {
            core::ptr::addr_of_mut!(EXPLORER_STORAGE)
                .write(core::mem::MaybeUninit::new(ExplorerState::new(root_cluster)));
        }
    }

fn fat32_list_dir(vol: &mut fat32::Fat32Volume, dir_name: &str) {
    let root = vol.root_cluster();
    if let Ok(entry) = vol.find_entry(root, dir_name) {
        if entry.is_dir {
            serial::write_str("[FS] /");
            serial::write_str(dir_name);
            serial::write_str("/ contents:\n");
            let _ = vol.list_dir(entry.cluster, |e| {
                serial::write_str("      ");
                if e.is_dir { serial::write_str("[DIR] "); }
                else { serial::write_str("[FILE] "); }
                serial::write_str(e.name_str());
                serial::write_str("  (");
                serial::write_usize(e.size as usize);
                serial::write_str(" bytes)\n");
            });
        }
    }
}

fn load_ring3_init(vol: &mut fat32::Fat32Volume, root: u32) -> bool {
    let bin_entry = match vol.find_entry(root, "bin") {
        Ok(e) if e.is_dir => e,
        _ => { serial::log("FS", "/bin not found"); return false; }
    };

    let candidates = ["init", "sh"];
    let mut loaded = false;

    for name in &candidates {
        let entry = match vol.find_entry(bin_entry.cluster, name) {
            Ok(e) if !e.is_dir => e,
            _ => { continue; }
        };
        let size = entry.size as usize;
        let mut buf = alloc::vec![0u8; size];
        if vol.read_file(&entry, &mut buf).is_err() {
            serial::log("FS", "read /bin/");
            serial::write_str(name);
            serial::write_str(" failed\n");
            continue;
        }
        serial::write_str("[FS] /bin/");
        serial::write_str(name);
        serial::write_str(" loaded (");
        serial::write_usize(size);
        serial::write_str(" bytes)\n");
        if let Some(pid1) = elf::elf_load_and_create_process(&buf, name) {
            if let Some(proc) = process::process_by_pid(pid1) {
                proc.state = process::ProcessState::Ready;
            }
            let stack_top = core::ptr::addr_of!(__stack_top) as u64;
            process::set_tss_rsp0(stack_top);
            serial::write_str("[R3] ");
            serial::write_str(name);
            serial::write_str(" process PID=1 ready (awaiting scheduler)\n");
            loaded = true;
            break;
        } else {
            serial::log("FS", "elf_load_and_create_process failed for /bin/");
            serial::write_str(name);
            serial::write_str("\n");
        }
    }

    loaded
}

    // Referencias limpias para el loop principal
    let ide: &mut IdeState = unsafe { (*core::ptr::addr_of_mut!(IDE_STORAGE)).assume_init_mut() };
    let explorer: &mut ExplorerState =
        unsafe { (*core::ptr::addr_of_mut!(EXPLORER_STORAGE)).assume_init_mut() };

    let mut tab = Tab::System;
    let mut sb_dragging = false;
    let mut sb_drag_y: i32 = 0;
    let mut sb_drag_offset: usize = 0;
    let mut last_blink_tick = 0u64;
    let mut last_render_tick = 0u64;
    let mut needs_draw = true;
    let mut needs_present = true;

    let boot_lines: &[(&str, &str, Color)] = &[
        ("  OK  ", "Modo largo (64-bit) activo", Color::GREEN),
        ("  OK  ", "GDT + TSS cargados", Color::GREEN),
        ("  OK  ", "IDT configurada (0-19 + IRQ)", Color::GREEN),
        ("  OK  ", "PIC remapeado, IRQ0 habilitado", Color::GREEN),
        ("  OK  ", "PIT @ 100 Hz", Color::GREEN),
        ("  OK  ", "Teclado PS/2 inicializado", Color::GREEN),
        ("  OK  ", "Raton PS/2 inicializado", Color::GREEN),
        ("  OK  ", "Escaneo de discos ATA completo", Color::GREEN),
        ("  OK  ", "Framebuffer VESA activo", Color::GREEN),
        ("  OK  ", "Doble buffer @ 0x5000000", Color::GREEN),
        ("  OK  ", "Bus PCI escaneado", Color::GREEN),
        ("  OK  ", "Serial COM1 @ 38400 baud", Color::GREEN),
    ];

    c.clear(Color::PORTIX_BG);

    // Draw initial UI before ring-3 demo
    draw_chrome(&mut c, &lay, &hw, Tab::System, 0, 0);
    draw_system_tab(&mut c, &lay, &hw, boot_lines);
    drivers::serial::log("FB", "presenting initial UI...");
    c.present_full();
    drivers::serial::log("FB", "present done");

    // ── Ring 3 — init process (scheduler-managed) ────────────────────────
    // Create PID 1 in Ready state. Process is NOT auto-executed — it enters
    // the scheduler's ready pool for preemptive multitasking (future).
    // The kernel boots directly into the main loop UI.
    if mount_ok {
        if let Some(drive) = registry::get_device(0) {
            if let Ok(mut vol) = fat32::Fat32Volume::mount(drive) {
                let root = vol.root_cluster();
                let _ = load_ring3_init(&mut vol, root);
            }
        }
    } else {
        let hello_elf = include_bytes!("../../build/hello.elf");
        if let Some(pid1) = elf::elf_load_and_create_process(hello_elf, "shell") {
            if let Some(proc) = process::process_by_pid(pid1) {
                proc.state = process::ProcessState::Ready;
            }
            let stack_top = core::ptr::addr_of!(__stack_top) as u64;
            process::set_tss_rsp0(stack_top);
            drivers::serial::log("R3", "Hello process PID=1 ready (awaiting scheduler)");
        } else {
            drivers::serial::log("R3", "ERROR: failed to load ELF or create process");
        }
    }

    loop {
        let now = time::pit::ticks();

        // ── Drenado IRQ1 teclado ─────────────────────────────────────────
        // IRQ1 handler buffers scancodes into SCANCODE_BUF.
        // IRQ12 (mouse) buffers mouse bytes into MOUSE_BUF.
        let mut kbd_buf = [0u8; 32];
        let mut kbd_n = 0usize;
        let mut ms_buf = [0u8; 32];
        let mut ms_n = 0usize;
        // Drain IRQ1 scancode buffer
        while kbd_n < 32 {
            match crate::arch::isr_handlers::pop_scancode() {
                Some(sc) => { kbd_buf[kbd_n] = sc; kbd_n += 1; }
                None => break,
            }
        }
        // Drain IRQ12 mouse buffer
        while ms_n < 32 {
            match crate::arch::isr_handlers::pop_mouse_byte() {
                Some(byte) => { ms_buf[ms_n] = byte; ms_n += 1; }
                None => break,
            }
        }





        // ── Poll mouse directly (fallback if IRQ12 doesn't fire) ──────────
        // IRQ1 handler now skips AUXB=1 bytes, so mouse data stays in the
        // controller buffer until consumed here or by IRQ12.
        while ms_n < 32 {
            match unsafe { drivers::input::mouse::poll_aux() } {
                Some(byte) => {
                    if drivers::input::mouse::init_done() < 3 {
                        crate::drivers::serial::write_str("[MS] byte[");
                        crate::drivers::serial::write_usize(drivers::input::mouse::init_done() as usize);
                        crate::drivers::serial::write_str("]=0x");
                        crate::drivers::serial::write_hex(byte as usize);
                        crate::drivers::serial::write_str("\n");
                    }
                    ms_buf[ms_n] = byte; ms_n += 1;
                }
                None => break,
            }
        }

        // ── Cola de teclado ───────────────────────────────────────────────
        for i in 0..kbd_n {
            if let Some(key) = kbd.feed_byte(kbd_buf[i]) {
                needs_draw = true;

                // Editor de texto del terminal (modo especial)
               if term.editor.is_some() {
    let should_exit = {
        let ed = term.editor.as_mut().unwrap();
        let ctrl = kbd.ctrl();
        ed.handle_key(key, ctrl);  // ← ctrl ahora se pasa
        ed.exit
    };
                    if should_exit {
                        term.editor = None;
                        term.write_line("  Editor cerrado.", LineColor::Info);
                        tab = Tab::Terminal;
                    }
                    continue;
                }

                // ► LECTURA REAL DEL ESTADO CTRL ◄
                let ctrl = kbd.ctrl();

                // Escape cierra menú IDE o limpia terminal
                if key == Key::Escape {
                    if ide.menu != MenuState::Closed {
                        ide.menu = MenuState::Closed;
                        continue;
                    }
                    if tab == Tab::Terminal {
                        term.clear_history();
                        term.clear_input();
                    }
                    sb_dragging = false;
                    continue;
                }

                match key {
                    // Teclas de función — siempre cambian tab
                    Key::F1 => tab = Tab::System,
                    Key::F2 => tab = Tab::Terminal,
                    Key::F3 => tab = Tab::Devices,
                    Key::F4 => tab = Tab::Ide,
                    Key::F5 => {
                        if tab == Tab::Explorer {
                            explorer.needs_refresh = true;
                        } else {
                            tab = Tab::Explorer;
                        }
                    }

                    // Tab sin Ctrl: ciclar pestañas
                    Key::Tab if !ctrl => {
                        tab = match tab {
                            Tab::System => Tab::Terminal,
                            Tab::Terminal => Tab::Devices,
                            Tab::Devices => Tab::Ide,
                            Tab::Ide => Tab::Explorer,
                            Tab::Explorer => Tab::System,
                        };
                    }

                    // ── Terminal ──────────────────────────────────────────
                    Key::PageUp if tab == Tab::Terminal => {
                        let (_, _, _, ml) = terminal_hist_geometry(&lay);
                        term.scroll_up(10, ml);
                    }
                    Key::PageDown if tab == Tab::Terminal => term.scroll_down(10),
                    Key::Home if tab == Tab::Terminal => {
                        let (_, _, _, ml) = terminal_hist_geometry(&lay);
                        term.scroll_up(usize::MAX / 2, ml);
                    }
                    Key::End if tab == Tab::Terminal => term.scroll_to_bottom(),
                    Key::Char(ch) if tab == Tab::Terminal => {
                        term.type_char(ch);
                        drivers::serial::write_byte(ch);
                    }
                    Key::Backspace if tab == Tab::Terminal => term.backspace(),
                    Key::Enter if tab == Tab::Terminal => {
                        drivers::serial::write_byte(b'\n');
                        term.enter(&hw, &pci);
                        if term.editor.is_some() {
                            tab = Tab::Terminal;
                        }
                    }

                    // ── IDE — Ctrl+S/N/W y teclas de edición ──────────────
                    _ if tab == Tab::Ide => {
                        let edit_start = lay.content_y + IDE_MENU_H + IDE_TABS_H;
                        let edit_h = lay.fh.saturating_sub(edit_start + IDE_STATUS_H);
                        let lh = lay.font_h + 3;
                        let vis_r = (edit_h / lh).max(1);

                        // Ctrl+S/N/W/Tab manejados dentro de ide.handle_key
                        ide.handle_key(key, ctrl, vis_r);
                    }

                    // ── Explorer ──────────────────────────────────────────
                    _ if tab == Tab::Explorer => {
                        explorer.handle_key(key);
                        if explorer.open_request {
                            explorer.open_request = false;
                            let name =
                                core::str::from_utf8(&explorer.open_name[..explorer.open_name_len])
                                    .unwrap_or("archivo");
                            ide.open_new(name);
                            tab = Tab::Ide;
                        }
                    }

                    _ => {}
                }
            }
        }

        // ── Cola de ratón ─────────────────────────────────────────────────
        let mouse_changed = if ms.present && ms_n > 0 {
            ms.begin_frame();
            let mut changed = false;
            for i in 0..ms_n {
                if ms.feed(ms_buf[i]) {
                    changed = true;
                }
            }
            if ms.error_count >= 25 {
                ms.intelligent_reset();
            }
            if changed || ms.error_count > 0 {
                crate::drivers::serial::write_str("[MS] feed ");
                crate::drivers::serial::write_usize(ms_n);
                crate::drivers::serial::write_str("B xy=0x");
                crate::drivers::serial::write_hex(ms.x as usize);
                crate::drivers::serial::write_str(",0x");
                crate::drivers::serial::write_hex(ms.y as usize);
                crate::drivers::serial::write_str(" err=");
                crate::drivers::serial::write_usize(ms.error_count as usize);
                crate::drivers::serial::write_str("\n");
            }
            changed
        } else {
            false
        };

        if mouse_changed {
            needs_draw = true;
        }

        // ── Interacción con ratón ─────────────────────────────────────────
        if term.editor.is_none() {
            let fw = lay.fw;
            let sb_x = fw.saturating_sub(SCROLLBAR_W) as i32;

            // Soltar drag de scrollbar
            if sb_dragging && (ms.left_released() || !ms.left_btn()) {
                sb_dragging = false;
                needs_draw = true;
            }

            // Arrastrar scrollbar del terminal
            if sb_dragging && ms.left_btn() && tab == Tab::Terminal {
                let (_, hist_h, _, max_lines) = terminal_hist_geometry(&lay);
                let max_scroll = term.max_scroll(max_lines);
                if max_scroll > 0 {
                    let available = term.line_count.saturating_sub(
                        if term.line_count > console::terminal::TERM_ROWS {
                            term.line_count - console::terminal::TERM_ROWS
                        } else {
                            0
                        },
                    );
                    let thumb_h = if available == 0 {
                        hist_h
                    } else {
                        (hist_h * max_lines / available).max(10).min(hist_h)
                    };
                    let travel = hist_h.saturating_sub(thumb_h) as i32;
                    if travel > 0 {
                        let dy = ms.y - sb_drag_y;
                        let new_offset = sb_drag_offset as i32 - (dy * max_scroll as i32) / travel;
                        term.scroll_offset = new_offset.max(0).min(max_scroll as i32) as usize;
                    }
                }
                needs_draw = true;
            }

            if mouse_changed && ms.right_clicked() && tab == Tab::Explorer {
                explorer.handle_right_click(ms.x as usize, ms.y as usize, lay.content_y, lay.fw);
                needs_draw = true;
            }
            if mouse_changed && ms.left_clicked() {
                // ── Click en scrollbar del terminal ───────────────────────
                if tab == Tab::Terminal && ms.x >= sb_x {
                    sb_dragging = true;
                    sb_drag_y = ms.y;
                    sb_drag_offset = term.scroll_offset;
                    needs_draw = true;

                // ── Click en barra de TABS del chrome ─────────────────────
                } else if (ms.y as usize) >= lay.tab_y && (ms.y as usize) < lay.tab_y + lay.tab_h {
                    match lay.tab_hit(ms.x, ms.y) {
                        0 => {
                            tab = Tab::System;
                            needs_draw = true;
                        }
                        1 => {
                            tab = Tab::Terminal;
                            needs_draw = true;
                        }
                        2 => {
                            tab = Tab::Devices;
                            needs_draw = true;
                        }
                        3 => {
                            tab = Tab::Ide;
                            needs_draw = true;
                        }
                        4 => {
                            tab = Tab::Explorer;
                            needs_draw = true;
                        }
                        _ => {}
                    }

                // ── Click dentro del área de contenido del IDE ────────────
                } else if tab == Tab::Ide {
                    if ide_help_btn_hit(ms.x, ms.y, lay.content_y, lay.fw, lay.font_w) {
                        ide.show_help = !ide.show_help;
                        needs_draw = true;
                    }
                    if explorer.context.visible {
                        // ¿Hit en algún item del menú?
                        let cx = explorer.context.x;
                        let cy = explorer.context.y;
                        let mw = explorer.context.width(lay.font_w);
                        let x = ms.x as usize;
                        let y = ms.y as usize;
                        if x >= cx && x < cx + mw && y >= cy {
                            let item_idx = (y.saturating_sub(cy + 2)) / 18;
                            explorer.execute_context(item_idx);
                        } else {
                            explorer.context.close();
                        }
                        needs_draw = true;
                    }
                    // Botón [?] de help en toolbar del explorer
                    else if exp_help_btn_hit(ms.x, ms.y, lay.content_y, lay.fw, lay.font_w) {
                        explorer.show_help = !explorer.show_help;
                        needs_draw = true;
                    }
                    let hit_menu = ide_menubar_hit(ms.x, ms.y, lay.content_y, lay.font_w);
                    if hit_menu >= 0 {
                        // Abrir/cerrar menú
                        let idx = hit_menu as usize;
                        ide.menu = if ide.menu == MenuState::Open(idx) {
                            MenuState::Closed
                        } else {
                            MenuState::Open(idx)
                        };
                        needs_draw = true;
                    } else if let MenuState::Open(open_idx) = ide.menu {
                        // Click dentro del dropdown
                        let item_hit =
                            ide_dropdown_hit(ms.x, ms.y, open_idx, lay.content_y, lay.font_w);
                        if item_hit >= 0 {
                            let action = MENUS[open_idx].items[item_hit as usize].action;
                            ide.execute_menu(action);
                            needs_draw = true;
                        } else {
                            // Click fuera del dropdown → cerrar
                            ide.menu = MenuState::Closed;
                            needs_draw = true;
                        }
                    } else {
                        // Click en editor — cerrar cualquier menú abierto
                        if ide.menu != MenuState::Closed {
                            ide.menu = MenuState::Closed;
                            needs_draw = true;
                        }
                    }

                // ── Click fuera del IDE con menú abierto → cerrarlo ───────
                } else if ide.menu != MenuState::Closed {
                    ide.menu = MenuState::Closed;
                    needs_draw = true;
                }
            }

            // Scroll del ratón en terminal
            if mouse_changed && ms.scroll_delta != 0 && tab == Tab::Terminal && !sb_dragging {
                let (_, _, _, ml) = terminal_hist_geometry(&lay);
                if ms.scroll_delta > 0 {
                    term.scroll_up(console::terminal::SCROLL_STEP, ml);
                } else {
                    term.scroll_down(console::terminal::SCROLL_STEP);
                }
                needs_draw = true;
            }
        }

        // ── Cursor parpadeante ────────────────────────────────────────────
        if term.editor.is_none() {
            if now.wrapping_sub(last_blink_tick) >= 50 {
                last_blink_tick = now;
                term.cursor_vis = !term.cursor_vis;
                if tab == Tab::Terminal {
                    needs_draw = true;
                }
            }
        }

        // ── Render ────────────────────────────────────────────────────────
        if needs_draw {
            draw_chrome(&mut c, &lay, &hw, tab, ms.x, ms.y);

            match tab {
                Tab::System => draw_system_tab(&mut c, &lay, &hw, boot_lines),
                Tab::Terminal => {
                    if let Some(ref ed) = term.editor {
                        draw_editor_tab(&mut c, &lay, ed);
                    } else {
                        draw_terminal_tab(&mut c, &lay, &term, sb_dragging);
                    }
                }
                Tab::Devices => draw_devices_tab(&mut c, &lay, &hw, &pci),
                Tab::Ide => draw_ide_tab(&mut c, &lay, ide),
                Tab::Explorer => draw_explorer_tab(&mut c, &lay, explorer),
            }

// v0.7.4 (correcto):
if ms.present {
    c.draw_cursor(ms.x, ms.y);
}            needs_draw = false;
            needs_present = true;
        }

        if needs_present && now.wrapping_sub(last_render_tick) >= RENDER_INTERVAL {
            c.present();
            last_render_tick = now;
            needs_present = false;
        }

        unsafe {
            core::arch::asm!("pause", options(nostack, nomem));
        }
    }
}
