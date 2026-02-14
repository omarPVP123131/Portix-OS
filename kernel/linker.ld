/* linker.ld - Linker script corregido para Portix */
ENTRY(_start)

SECTIONS
{
    /* Cargar el kernel en 0x10000 (como lo espera stage2.asm) */
    . = 0x10000;

    /* .text: código (incluye _start) */
    .text : ALIGN(4) {
        PROVIDE(__text_start = .);
        *(.text._start)
        *(.text._start.*)
        *(.text*)
        PROVIDE(__text_end = .);
    }

    /* .rodata: datos constantes */
    .rodata : ALIGN(4) {
        *(.rodata*)
        *(.rodata.*)
    }

    /* .data: variables inicializadas */
    .data : ALIGN(4) {
        PROVIDE(__data_start = .);
        *(.data*)
        *(.data.*)
        PROVIDE(__data_end = .);
    }

    /* .bss: variables no inicializadas */
    .bss (NOLOAD) : ALIGN(4) {
        PROVIDE(__bss_start = .);
        *(.bss*)
        *(COMMON)
        PROVIDE(__bss_end = .);
    }

    /* Evitar que se pierdan secciones de excepción de unwinding que no necesitamos */
    /DISCARD/ : { *(.eh_frame) }
}
