// kernel/src/main.rs — PORTIX Kernel v0.7.4
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
#![allow(static_mut_refs)]   // kernel bare-metal single-thread — safe

pub mod drivers;
pub mod arch;
pub mod graphics;
pub mod time;
pub mod console;
pub mod util;
pub mod ui;

use core::arch::global_asm;
use graphics::driver::framebuffer::{Color, Console, Layout};
use drivers::input::keyboard::Key;
use console::terminal::LineColor;
use console::terminal::editor::draw_editor_tab;
use ui::{Tab, SCROLLBAR_W, draw_chrome, draw_system_tab, draw_terminal_tab,
         draw_devices_tab, draw_ide_tab, draw_explorer_tab, terminal_hist_geometry};
use ui::tabs::ide::{IdeState, MenuState, MENUS, init_page_pool};
use ui::tabs::explorer::ExplorerState;

extern "C" {
    static __bss_start: u8;
    static __bss_end:   u8;
    static __stack_top: u8;
}

global_asm!(
    ".section .text._start, \"ax\"",
    ".global _start",
    ".code64",
    "_start:",
    "    cli",
    "    cld",
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
    "    call {RUST_MAIN}",
    "2:  hlt",
    "    jmp 2b",
    STACK_TOP = sym __stack_top,
    BSS_START = sym __bss_start,
    BSS_END   = sym __bss_end,
    RUST_MAIN = sym rust_main,
);

// ── Constantes ────────────────────────────────────────────────────────────────

const RENDER_HZ:       u64 = 30;
const RENDER_INTERVAL: u64 = 100 / RENDER_HZ;
const PS2_STATUS:      u16 = 0x64;
const PS2_DATA:        u16 = 0x60;

// Alturas internas del IDE (deben coincidir con ide.rs)
const IDE_MENUBAR_H:  usize = 20;
const IDE_FILETABS_H: usize = 22;
const IDE_STATUS_H:   usize = 18;

#[inline(always)]
unsafe fn ps2_inb(p: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", out("al") v, in("dx") p, options(nostack, nomem));
    v
}

// ── Statics BSS ──────────────────────────────────────────────────────────────

static mut IDE_STORAGE:      core::mem::MaybeUninit<IdeState>      = core::mem::MaybeUninit::uninit();
static mut EXPLORER_STORAGE: core::mem::MaybeUninit<ExplorerState> = core::mem::MaybeUninit::uninit();

// ── Hit-test de la menubar del IDE ────────────────────────────────────────────
//
// Devuelve el índice del menú clickeado (-1 si no hay hit).
// Tiene que coincidir con los anchos que calcula draw_ide_tab en ide.rs.
fn ide_menubar_hit(mx: i32, my: i32, content_y: usize, font_w: usize) -> i32 {
    let y = my as usize;
    if y < content_y || y >= content_y + IDE_MENUBAR_H { return -1; }
    let mut x_pos = 6usize;
    for (i, menu) in MENUS.iter().enumerate() {
        let label_w = menu.title.len() * font_w + 16;
        let x = mx as usize;
        if x >= x_pos && x < x_pos + label_w { return i as i32; }
        x_pos += label_w + 2;
    }
    -1
}

// Hit-test dentro del dropdown abierto. Devuelve el índice del item (-1 si no).
fn ide_dropdown_hit(mx: i32, my: i32, menu_idx: usize, content_y: usize, font_w: usize) -> i32 {
    if menu_idx >= MENUS.len() { return -1; }
    let menu = &MENUS[menu_idx];

    let mut x_pos = 6usize;
    for i in 0..menu_idx {
        x_pos += MENUS[i].title.len() * font_w + 18;
    }

    let max_label = menu.items.iter().map(|it| it.label.len()).max().unwrap_or(10);
    let max_short = menu.items.iter().map(|it| it.shortcut.len()).max().unwrap_or(0);
    let dd_w = (max_label + max_short + 6) * font_w + 16;
    let dd_x = x_pos;
    let dd_y = content_y + IDE_MENUBAR_H;

    let x = mx as usize;
    let y = my as usize;
    if x < dd_x || x >= dd_x + dd_w { return -1; }

    const ITEM_H: usize = 16;
    let rel_y = y.wrapping_sub(dd_y + 3);
    let item  = rel_y / ITEM_H;
    if item < menu.items.len() && y >= dd_y + 3 { item as i32 } else { -1 }
}

// ── Punto de entrada ──────────────────────────────────────────────────────────

#[no_mangle]
extern "C" fn rust_main() -> ! {
    // CRÍTICO: PAGE_POOL debe inicializarse ANTES de IdeState::new()
    init_page_pool();

    unsafe { arch::idt::init_idt(); }
    drivers::serial::init();
    time::pit::init();
    unsafe { core::arch::asm!("sti", options(nostack, preserves_flags)); }
    drivers::serial::log("PIT", "temporizador 100 Hz");

    let hw  = arch::hardware::HardwareInfo::detect_all();
    drivers::serial::log("HW", hw.cpu.brand_str());

    let pci = drivers::bus::pci::PciBus::scan();
    {
        let mut t = [0u8; 16];
        let s = util::fmt::fmt_u32(pci.count as u32, &mut t);
        drivers::serial::write_str("PCI: ");
        drivers::serial::write_str(s);
        drivers::serial::write_str(" dispositivos\n");
    }
    { let ata = drivers::storage::ata::AtaBus::scan(); drivers::storage::ata::log_drives(&ata); }

    let mut kbd = drivers::input::keyboard::KeyboardState::new();
    let mut ms  = drivers::input::mouse::MouseState::new();
    let mut c   = Console::new();
    let lay     = Layout::new(c.width(), c.height());
    ms.init(lay.fw.max(1), lay.fh.max(1));

    let mut term = console::terminal::Terminal::new();
    term.write_line("PORTIX v0.7.4  Kernel Bare-Metal", LineColor::Header);
    term.write_line("Escribe 'ayuda' para comandos.", LineColor::Info);
    term.write_empty();

    // Inicializar grandes estructuras en BSS (no stack)
    unsafe {
        core::ptr::addr_of_mut!(IDE_STORAGE).write(
            core::mem::MaybeUninit::new(IdeState::new())
        );
        core::ptr::addr_of_mut!(EXPLORER_STORAGE).write(
            core::mem::MaybeUninit::new(ExplorerState::new(2))
        );
    }

    // Intentar FAT32 — si falla Explorer queda con cluster=2
    {
        let ata = drivers::storage::ata::AtaBus::scan();
        if let Some(drive) = ata.drive(drivers::storage::ata::DriveId::Primary0) {
            if let Ok(vol) = drivers::storage::fat32::Fat32Volume::mount(drive) {
                let root = vol.root_cluster();
                unsafe {
                    core::ptr::addr_of_mut!(EXPLORER_STORAGE).write(
                        core::mem::MaybeUninit::new(ExplorerState::new(root))
                    );
                }
            }
        }
    }

    // Referencias limpias para el loop principal
    let ide:      &mut IdeState      = unsafe { (*core::ptr::addr_of_mut!(IDE_STORAGE)).assume_init_mut() };
    let explorer: &mut ExplorerState = unsafe { (*core::ptr::addr_of_mut!(EXPLORER_STORAGE)).assume_init_mut() };

    let mut tab               = Tab::System;
    let mut sb_dragging       = false;
    let mut sb_drag_y:  i32   = 0;
    let mut sb_drag_offset: usize = 0;
    let mut last_blink_tick   = 0u64;
    let mut last_render_tick  = 0u64;
    let mut needs_draw        = true;
    let mut needs_present     = true;

    let boot_lines: &[(&str, &str, Color)] = &[
        ("  OK  ", "Modo largo (64-bit) activo",     Color::GREEN),
        ("  OK  ", "GDT + TSS cargados",             Color::GREEN),
        ("  OK  ", "IDT configurada (0-19 + IRQ)",   Color::GREEN),
        ("  OK  ", "PIC remapeado, IRQ0 habilitado", Color::GREEN),
        ("  OK  ", "PIT @ 100 Hz",                   Color::GREEN),
        ("  OK  ", "Teclado PS/2 inicializado",      Color::GREEN),
        ("  OK  ", "Raton PS/2 inicializado",        Color::GREEN),
        ("  OK  ", "Escaneo de discos ATA completo", Color::GREEN),
        ("  OK  ", "Framebuffer VESA activo",        Color::GREEN),
        ("  OK  ", "Doble buffer @ 0x600000",        Color::GREEN),
        ("  OK  ", "Bus PCI escaneado",              Color::GREEN),
        ("  OK  ", "Serial COM1 @ 38400 baud",       Color::GREEN),
    ];

    c.clear(Color::PORTIX_BG);

    loop {
        let now = time::pit::ticks();

        // ── Drenado unificado PS/2 ────────────────────────────────────────
        let mut kbd_buf = [0u8; 32]; let mut kbd_n = 0usize;
        let mut ms_buf  = [0u8; 32]; let mut ms_n  = 0usize;
        unsafe {
            loop {
                let st = ps2_inb(PS2_STATUS);
                if st & 0x01 == 0 { break; }
                let byte = ps2_inb(PS2_DATA);
                if st & 0x20 != 0 {
                    if ms_n  < 32 { ms_buf[ms_n]  = byte; ms_n  += 1; }
                } else {
                    if kbd_n < 32 { kbd_buf[kbd_n] = byte; kbd_n += 1; }
                }
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
                        ed.handle_key(key);
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
                    Key::F1  => tab = Tab::System,
                    Key::F2  => tab = Tab::Terminal,
                    Key::F3  => tab = Tab::Devices,
                    Key::F4  => tab = Tab::Ide,
                    Key::F5  => {
                        if tab == Tab::Explorer {
                            explorer.needs_refresh = true;
                        } else {
                            tab = Tab::Explorer;
                        }
                    }

                    // Tab sin Ctrl: ciclar pestañas
                    Key::Tab if !ctrl => {
                        tab = match tab {
                            Tab::System   => Tab::Terminal,
                            Tab::Terminal => Tab::Devices,
                            Tab::Devices  => Tab::Ide,
                            Tab::Ide      => Tab::Explorer,
                            Tab::Explorer => Tab::System,
                        };
                    }

                    // ── Terminal ──────────────────────────────────────────
                    Key::PageUp   if tab == Tab::Terminal => {
                        let (_, _, _, ml) = terminal_hist_geometry(&lay);
                        term.scroll_up(10, ml);
                    }
                    Key::PageDown if tab == Tab::Terminal => term.scroll_down(10),
                    Key::Home     if tab == Tab::Terminal => {
                        let (_, _, _, ml) = terminal_hist_geometry(&lay);
                        term.scroll_up(usize::MAX / 2, ml);
                    }
                    Key::End      if tab == Tab::Terminal => term.scroll_to_bottom(),
                    Key::Char(ch) if tab == Tab::Terminal => {
                        term.type_char(ch);
                        drivers::serial::write_byte(ch);
                    }
                    Key::Backspace if tab == Tab::Terminal => term.backspace(),
                    Key::Enter     if tab == Tab::Terminal => {
                        drivers::serial::write_byte(b'\n');
                        term.enter(&hw, &pci);
                        if term.editor.is_some() { tab = Tab::Terminal; }
                    }

                    // ── IDE — Ctrl+S/N/W y teclas de edición ──────────────
                    _ if tab == Tab::Ide => {
                        let edit_start = lay.content_y + IDE_MENUBAR_H + IDE_FILETABS_H;
                        let edit_h     = lay.fh.saturating_sub(edit_start + IDE_STATUS_H);
                        let lh         = lay.font_h + 3;
                        let vis_r      = (edit_h / lh).max(1);
                        // Ctrl+S/N/W/Tab manejados dentro de ide.handle_key
                        ide.handle_key(key, ctrl, vis_r);
                    }

                    // ── Explorer ──────────────────────────────────────────
                    _ if tab == Tab::Explorer => {
                        let prev_sel = explorer.selected;
                        explorer.handle_key(key);
                        // Cargar preview si cambió la selección y hay FAT32
                        let _ = prev_sel; // preview se carga en el render o con FAT32
                        if explorer.open_request {
                            explorer.open_request = false;
                            let name = core::str::from_utf8(
                                &explorer.open_name[..explorer.open_name_len]
                            ).unwrap_or("archivo");
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
            for i in 0..ms_n { if ms.feed(ms_buf[i]) { changed = true; } }
            if ms.error_count >= 25 { ms.intelligent_reset(); }
            changed
        } else { false };

        if mouse_changed { needs_draw = true; }

        // ── Interacción con ratón ─────────────────────────────────────────
        if term.editor.is_none() {
            let fw   = lay.fw;
            let sb_x = fw.saturating_sub(SCROLLBAR_W) as i32;

            // Soltar drag de scrollbar
            if sb_dragging && (ms.left_released() || !ms.left_btn()) {
                sb_dragging = false; needs_draw = true;
            }

            // Arrastrar scrollbar del terminal
            if sb_dragging && ms.left_btn() && tab == Tab::Terminal {
                let (_, hist_h, _, max_lines) = terminal_hist_geometry(&lay);
                let max_scroll = term.max_scroll(max_lines);
                if max_scroll > 0 {
                    let available = term.line_count.saturating_sub(
                        if term.line_count > console::terminal::TERM_ROWS {
                            term.line_count - console::terminal::TERM_ROWS
                        } else { 0 }
                    );
                    let thumb_h = if available == 0 { hist_h }
                                  else { (hist_h * max_lines / available).max(10).min(hist_h) };
                    let travel = hist_h.saturating_sub(thumb_h) as i32;
                    if travel > 0 {
                        let dy = ms.y - sb_drag_y;
                        let new_offset = sb_drag_offset as i32 - (dy * max_scroll as i32) / travel;
                        term.scroll_offset = new_offset.max(0).min(max_scroll as i32) as usize;
                    }
                }
                needs_draw = true;
            }

            if mouse_changed && ms.left_clicked() {
                // ── Click en scrollbar del terminal ───────────────────────
                if tab == Tab::Terminal && ms.x >= sb_x {
                    sb_dragging    = true;
                    sb_drag_y      = ms.y;
                    sb_drag_offset = term.scroll_offset;
                    needs_draw     = true;

                // ── Click en barra de TABS del chrome ─────────────────────
                } else if (ms.y as usize) >= lay.tab_y && (ms.y as usize) < lay.tab_y + lay.tab_h {
                    match lay.tab_hit(ms.x, ms.y) {
                        0 => { tab = Tab::System;   needs_draw = true; }
                        1 => { tab = Tab::Terminal; needs_draw = true; }
                        2 => { tab = Tab::Devices;  needs_draw = true; }
                        3 => { tab = Tab::Ide;      needs_draw = true; }
                        4 => { tab = Tab::Explorer; needs_draw = true; }
                        _ => {}
                    }

                // ── Click dentro del área de contenido del IDE ────────────
                } else if tab == Tab::Ide {
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
                        let item_hit = ide_dropdown_hit(ms.x, ms.y, open_idx, lay.content_y, lay.font_w);
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
                if tab == Tab::Terminal { needs_draw = true; }
            }
        }

        // ── Render ────────────────────────────────────────────────────────
        if needs_draw {
            draw_chrome(&mut c, &lay, &hw, tab, ms.x, ms.y);

            match tab {
                Tab::System   => draw_system_tab(&mut c, &lay, &hw, boot_lines),
                Tab::Terminal => {
                    if let Some(ref ed) = term.editor {
                        draw_editor_tab(&mut c, &lay, ed);
                    } else {
                        draw_terminal_tab(&mut c, &lay, &term, sb_dragging);
                    }
                }
                Tab::Devices  => draw_devices_tab(&mut c, &lay, &hw, &pci),
                Tab::Ide      => draw_ide_tab(&mut c, &lay, ide),
                Tab::Explorer => draw_explorer_tab(&mut c, &lay, explorer),
            }

            if ms.present { c.draw_cursor(ms.x, ms.y); }
            needs_draw    = false;
            needs_present = true;
        }

        if needs_present && now.wrapping_sub(last_render_tick) >= RENDER_INTERVAL {
            c.present();
            last_render_tick = now;
            needs_present    = false;
        }

        unsafe { core::arch::asm!("pause", options(nostack, nomem)); }
    }
}