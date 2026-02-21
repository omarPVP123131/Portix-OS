// kernel/src/main.rs
#![no_std]
#![no_main]

mod halt;
mod idt;
mod framebuffer; // Cambio aquí: Ya no usamos vga
mod font;        // Importamos nuestro archivo de fuente

use core::arch::global_asm;
use core::panic::PanicInfo;
use halt::halt_loop;
use framebuffer::{Console, Color}; // Importamos nuestras nuevas estructuras

extern "C" {
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}

global_asm!(
    ".section .text._start, \"ax\"",
    ".global _start",
    ".code32",
    "",
    "_start:",
    "    cli",
    "    cld",
    "    mov edi, __bss_start",
    "    mov ecx, __bss_end",
    "    sub ecx, edi",
    "    xor eax, eax",
    "    rep stosb",
    "    call rust_main",
    "1:  cli",
    "    hlt",
    "    jmp 1b"
);

fn leer_ram_mb() -> u32 {
    unsafe {
        let ram_bytes = *(0x9000 as *const u32);
        if ram_bytes == 0 { 0 } else { ram_bytes / (1024 * 1024) }
    }
}

fn itoa(mut num: u32, buf: &mut [u8]) -> &str {
    if num == 0 {
        buf[0] = b'0';
        return core::str::from_utf8(&buf[..1]).unwrap();
    }
    let mut i = 0;
    while num > 0 {
        buf[i] = b'0' + (num % 10) as u8;
        num /= 10;
        i += 1;
    }
    buf[..i].reverse();
    core::str::from_utf8(&buf[..i]).unwrap()
}

#[no_mangle]
extern "C" fn rust_main() -> ! {
    let mut console = Console::new();

    // Limpiamos la pantalla gráfica con el Azul Portix
    console.clear(Color::PORTIX_BLUE);

    // =========================
    // HEADER SUPERIOR (Píxeles)
    // =========================
    // Dibujamos un banner superior elegante (X, Y, Ancho, Alto)
    console.fill_rect(0, 0, 1024, 40, Color::DARK_GRAY);
    console.write_at(" PORTIX KERNEL v0.3 (VESA EDITION) ", 380, 15, Color::PORTIX_GOLD);

    // =========================
    // ÁREA DE MENSAJES
    // =========================
    console.set_position(20, 60);

    console.write("[ OK ] Subsistema VESA Framebuffer Inicializado!\n", Color::GREEN);
    console.write("[ OK ] Modo protegido activo y graficos a 32-bit\n", Color::GREEN);

    unsafe { idt::init_idt(); }
    
    console.write("[ OK ] Tabla de interrupciones (IDT) cargada\n", Color::GREEN);
    
    console.write("\nSistema inicializado correctamente en Alta Resolucion.\n", Color::WHITE);
    console.write("Nucleo estable y dibujando pixeles a maxima velocidad.\n", Color::LIGHT_GRAY);

    // =========================
    // BARRA INFERIOR DE ESTADO
    // =========================
    console.fill_rect(0, 728, 1024, 40, Color::GRAY);
    
    let ram_mb = leer_ram_mb();
    let mut buf = [0u8; 16];
    let ram_str = itoa(ram_mb, &mut buf);

    console.write_at("ESTADO: SISTEMA EN EJECUCION", 20, 740, Color::BLACK);
    console.write_at("RAM DETECTADA: ", 800, 740, Color::BLACK);
    console.write_at(ram_str, 935, 740, Color::PORTIX_BLUE);
    console.write_at("MB", 965, 740, Color::BLACK);

    halt_loop();
}

// ---------------------------------------------------------
// RUTINAS DE INTERRUPCIÓN (Actualizadas para usar gráficos)
// ---------------------------------------------------------

#[no_mangle]
extern "C" fn isr_divide_by_zero() {
    let mut console = Console::new();
    console.clear(Color::RED);
    console.draw_rect(200, 300, 600, 100, Color::WHITE);
    console.write_at("EXCEPTION: DIVIDE BY ZERO (#DE)", 350, 340, Color::WHITE);
    halt_loop();
}

#[no_mangle]
extern "C" fn isr_bound_range() {
    let mut console = Console::new();
    console.clear(Color::RED);
    console.write_at("EXCEPTION: BOUND RANGE EXCEEDED (#BR)", 350, 340, Color::WHITE);
    halt_loop();
}

#[no_mangle]
extern "C" fn isr_ud_handler() {
    let mut console = Console::new();
    console.clear(Color::new(100, 0, 100)); // Morado Panic
    console.fill_rect(200, 250, 624, 200, Color::DARK_GRAY);
    console.write_at("EXCEPTION HANDLER WORKS!", 400, 280, Color::GREEN);
    console.write_at("Invalid Opcode (#UD) caught successfully in Graphics Mode.", 250, 330, Color::WHITE);
    console.write_at("System halted.", 450, 400, Color::PORTIX_GOLD);
    halt_loop();
}

#[no_mangle]
extern "C" fn isr_double_fault() {
    let mut console = Console::new();
    console.clear(Color::RED);
    console.write_at("EXCEPTION: DOUBLE FAULT (#DF) - SYSTEM HALTED", 300, 340, Color::WHITE);
    halt_loop();
}

#[no_mangle]
extern "C" fn isr_gp_handler() {
    let mut console = Console::new();
    console.clear(Color::RED);
    console.write_at("EXCEPTION: GENERAL PROTECTION FAULT (#GP)", 300, 340, Color::WHITE);
    halt_loop();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let mut console = Console::new();
    console.clear(Color::RED);
    console.fill_rect(300, 300, 400, 100, Color::BLACK);
    console.write_at("*** KERNEL PANIC ***", 430, 340, Color::RED);
    halt_loop();
}

// --- Soporte nativo ---
#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n { *s.add(i) = c as u8; i += 1; }
    s
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n { *dest.add(i) = *src.add(i); i += 1; }
    dest
}