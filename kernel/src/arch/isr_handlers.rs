// arch/isr_handlers.rs — Manejadores de excepciones de CPU, panic y funciones mem*
//
// Todos los símbolos aquí son `#[no_mangle]` y enlazados directamente desde
// la IDT configurada en arch/idt.rs.

use core::panic::PanicInfo;
use crate::graphics::driver::framebuffer::{Color, Console};
use crate::arch::halt::halt_loop;
use crate::ui::exception::draw_exception;
use crate::util::fmt::{fmt_u32, fmt_hex};

// ── Excepciones de CPU ────────────────────────────────────────────────────────

#[no_mangle]
extern "C" fn isr_divide_by_zero() {
    let mut c = Console::new();
    draw_exception(&mut c,
        "#DE  DIVISION POR CERO",
        "Division entre cero o desbordamiento DIV/IDIV.");
    halt_loop()
}

#[no_mangle]
extern "C" fn isr_bound_range() {
    let mut c = Console::new();
    draw_exception(&mut c, "#BR  RANGO EXCEDIDO", "Indice fuera de rango.");
    halt_loop()
}

#[no_mangle]
extern "C" fn isr_ud_handler() {
    let mut c = Console::new();
    draw_exception(&mut c, "#UD  OPCODE INVALIDO", "Instruccion no definida.");
    halt_loop()
}

#[no_mangle]
extern "C" fn isr_double_fault() {
    // Doble fallo: no podemos usar el framebuffer de forma segura,
    // caemos al VGA de texto en modo legacy.
    unsafe {
        let v = 0xB8000usize as *mut u16;
        for i in 0..80 {
            core::ptr::write_volatile(v.add(i), 0x4F20);
        }
        for (i, &b) in b"#DF DOBLE FALLO -- SISTEMA DETENIDO".iter().enumerate() {
            core::ptr::write_volatile(v.add(i), 0x4F00 | b as u16);
        }
    }
    halt_loop()
}

#[no_mangle]
extern "C" fn isr_gp_handler(ec: u64) {
    let mut c  = Console::new();
    let w = c.width(); let h = c.height();
    c.fill_rect(0, 0, w, h, Color::new(0, 0, 60));
    c.fill_rect(0, 0, w, 4, Color::RED);
    c.fill_rect(0, h - 4, w, 4, Color::RED);
    c.write_at("#GP  FALLO DE PROTECCION GENERAL", 60, 64, Color::WHITE);
    let mut buf = [0u8; 18];
    c.write_at("Codigo de error:", 60,  84, Color::GRAY);
    c.write_at(fmt_hex(ec, &mut buf),  200, 84, Color::YELLOW);
    c.present();
    halt_loop()
}

#[no_mangle]
extern "C" fn isr_page_fault(ec: u64) {
    let cr2: u64;
    unsafe {
        core::arch::asm!("mov {r}, cr2", r = out(reg) cr2,
                         options(nostack, preserves_flags));
    }
    let mut c = Console::new();
    let w = c.width(); let h = c.height();
    c.fill_rect(0, 0, w, h, Color::new(0, 0, 60));
    c.fill_rect(0, 0, w, 4, Color::RED);
    c.fill_rect(0, h - 4, w, 4, Color::RED);
    c.write_at("#PF  FALLO DE PAGINA", 60, 64, Color::WHITE);
    let mut ba = [0u8; 18]; let mut be = [0u8; 18];
    c.write_at("CR2:", 60,  84, Color::GRAY); c.write_at(fmt_hex(cr2, &mut ba), 100, 84, Color::YELLOW);
    c.write_at("Cod:", 60, 104, Color::GRAY); c.write_at(fmt_hex(ec,  &mut be),  96, 104, Color::YELLOW);
    c.present();
    halt_loop()
}

#[no_mangle]
extern "C" fn isr_generic_handler() {
    let mut c = Console::new();
    draw_exception(&mut c, "FALLO DE CPU", "Excepcion no manejada.");
    halt_loop()
}

// ── Panic handler ─────────────────────────────────────────────────────────────

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut c = Console::new();
    let w = c.width(); let h = c.height();
    c.fill_rect(0, 0, w, h, Color::new(50, 0, 0));
    c.fill_rect(0, 0, w, 4, Color::RED);
    c.fill_rect(0, h - 4, w, 4, Color::RED);
    c.write_at("*** PANIC DE KERNEL ***", w / 2 - 110, 16, Color::RED);

    if let Some(loc) = info.location() {
        c.write_at("Archivo:", 60, 64, Color::GRAY);
        c.write_at(loc.file(),  130, 64, Color::YELLOW);
        let mut lb = [0u8; 16];
        c.write_at("Linea:", 60, 84, Color::GRAY);
        c.write_at(fmt_u32(loc.line(), &mut lb), 110, 84, Color::YELLOW);
    }
    c.write_at("Error irrecuperable — sistema detenido.", 60, 120, Color::WHITE);
    c.present();
    halt_loop()
}

// ── Funciones intrínsecas de memoria ─────────────────────────────────────────
// Necesarias porque el compilador puede generar llamadas a estas incluso en
// código `no_std`. Se implementan con write/read volatile para evitar que el
// optimizador las elimine en contextos de hardware real.

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut u8, cv: i32, n: usize) -> *mut u8 {
    for i in 0..n { core::ptr::write_volatile(s.add(i), cv as u8); }
    s
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i)));
    }
    d
}

#[no_mangle]
pub unsafe extern "C" fn memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    if (d as usize) <= (s as usize) {
        memcpy(d, s, n)
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i)));
        }
        d
    }
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        let d = *a.add(i) as i32 - *b.add(i) as i32;
        if d != 0 { return d; }
    }
    0
}
