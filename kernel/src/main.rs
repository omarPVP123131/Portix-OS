#![no_std]
#![no_main]

mod halt;
mod idt;
mod vga;

use core::arch::global_asm;
use core::panic::PanicInfo;
use halt::halt_loop;
use vga::Vga;

extern "C" {
    /* Símbolos provistos por linker.ld */
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}

/* Start en ensamblador (32-bit). Limpia BSS usando los símbolos definidos en linker.ld */
global_asm!(
    ".section .text._start, \"ax\"",
    ".global _start",
    ".code32",
    "",
    "_start:",
    "    cli",
    "    cld",
    /* Limpiar BSS: EDI = __bss_start, ECX = __bss_end - __bss_start, AL = 0 */
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
        if ram_bytes == 0 {
            0
        } else {
            ram_bytes / (1024 * 1024)
        }
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
    let mut vga = Vga::new();

    // Fondo azul
    vga.clear(0x1F);

    // =========================
    // HEADER SUPERIOR
    // =========================
    vga.draw_box(0, 0, 80, 3, 0x1F);
    vga.write_at(" PORTIX KERNEL v0.2 ", 1, 29, 0x1F);

    // =========================
    // ÁREA DE MENSAJES
    // =========================
    vga.set_position(4, 2);

    vga.write("[ OK ] Subsistema VGA inicializado\n", 0x0A);
    vga.write("[ OK ] Modo protegido activo\n", 0x0A);

    // Cargar IDT
    unsafe {
        idt::init_idt();
    }
    vga.write("[ OK ] Tabla de interrupciones (IDT) cargada\n", 0x0A);

    vga.write("\n", 0x0F);
    vga.write("Sistema inicializado correctamente.\n", 0x0E);
    vga.write("Nucleo estable y en ejecucion.\n", 0x0E);

    vga.write("\n", 0x0F);
    vga.write("Para probar excepciones:\n", 0x07);
    vga.write("Descomenta la instruccion UD2 en main.rs\n", 0x07);

    // =========================
    // BARRA INFERIOR
    // =========================
    vga.draw_hline(22, 0, 80, 0x1F);
    vga.fill_rect(23, 0, 80, 2, b' ', 0x2E);

    // Leer RAM
    let ram_mb = leer_ram_mb();
    let mut buf = [0u8; 16];
    let ram_str = itoa(ram_mb, &mut buf);

    // Construir texto del footer
    vga.write_at("Estado: SISTEMA EN EJECUCION", 24, 5, 0x2E);
    vga.write_at("RAM:", 24, 45, 0x2E);
    vga.write_at(ram_str, 24, 50, 0x2E);
    vga.write_at("MB", 24, 55, 0x2E);

    // Cursor
    vga.set_position(10, 2);
    vga.enable_hardware_cursor(0, 15);
    vga.update_hardware_cursor();

    halt_loop();
}

#[no_mangle]
extern "C" fn isr_divide_by_zero() {
    let mut vga = Vga::new();
    vga.clear(0x4F);
    vga.draw_box(8, 15, 50, 7, 0x4F);
    vga.write_at("EXCEPTION: DIVIDE BY ZERO (#DE)", 9, 24, 0x4F);
    vga.write_at("System halted.", 13, 29, 0x4E);
    halt_loop();
}

#[no_mangle]
extern "C" fn isr_bound_range() {
    let mut vga = Vga::new();
    vga.clear(0x4E);
    vga.draw_box(8, 15, 50, 7, 0x4E);
    vga.write_at("EXCEPTION: BOUND RANGE EXCEEDED (#BR)", 9, 21, 0x4E);
    vga.write_at("System halted.", 13, 29, 0x4C);
    halt_loop();
}

#[no_mangle]
extern "C" fn isr_ud_handler() {
    let mut vga = Vga::new();
    vga.clear(0x2F);

    vga.draw_box(8, 15, 50, 9, 0x2F);
    vga.write_at("EXCEPTION HANDLER WORKS!", 10, 27, 0x2E);
    vga.write_at("", 11, 0, 0x2F);
    vga.write_at("Invalid Opcode (#UD) caught successfully.", 12, 15, 0x0F);
    vga.write_at("", 13, 0, 0x0F);
    vga.write_at("The IDT is working correctly!", 14, 21, 0x0A);
    vga.write_at("System halted.", 15, 29, 0x0E);

    halt_loop();
}

#[no_mangle]
extern "C" fn isr_double_fault() {
    let mut vga = Vga::new();
    vga.clear(0xCE);
    vga.draw_box(8, 15, 50, 7, 0xCE);
    vga.write_at("EXCEPTION: DOUBLE FAULT (#DF)", 9, 25, 0xCE);
    vga.write_at("System halted.", 13, 29, 0xCC);
    halt_loop();
}

#[no_mangle]
extern "C" fn isr_gp_handler() {
    let mut vga = Vga::new();
    vga.clear(0x4F);

    vga.draw_box(8, 12, 56, 7, 0x4F);
    vga.write_at("EXCEPTION: GENERAL PROTECTION FAULT (#GP)", 9, 19, 0x4F);
    vga.write_at("Memory protection violation.", 11, 22, 0x0F);
    vga.write_at("System halted.", 13, 29, 0x4E);

    halt_loop();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let mut vga = Vga::new();
    vga.clear(0xCF);
    vga.draw_box(10, 20, 40, 6, 0xCF);
    vga.write_at("*** KERNEL PANIC ***", 11, 29, 0xCF);
    halt_loop();
}

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *s.add(i) = c as u8;
        i += 1;
    }
    s
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *dest.add(i) = *src.add(i);
        i += 1;
    }
    dest
}
