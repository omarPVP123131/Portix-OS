/* kernel/linker.ld - PORTIX v4 */
OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

SECTIONS
{
    . = 0x10000;

    .text : ALIGN(16) {
        KEEP(*(.text._start))
        *(.text .text.*)
    }

    .rodata : ALIGN(16) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(16) {
        *(.data .data.*)
    }

    .bss : ALIGN(16) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    __kernel_end = .;

    /* ── Stack dedicado (512 KB) ────────────────────────────────────────────
     * Se coloca DESPUÉS del .bss para que nunca colisione con datos del kernel.
     * NOLOAD = no ocupa espacio en el binario (como .bss).
     * El stack crece hacia abajo desde __stack_top.
     * _start usa: lea rsp, [rip + __stack_top]
     * ──────────────────────────────────────────────────────────────────── */
    .stack (NOLOAD) : ALIGN(4096) {
        __stack_bottom = .;
        . += 512K;
        . = ALIGN(16);
        __stack_top = .;
    }

    /DISCARD/ : {
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.note .note.*)
        *(.comment)
        *(.gcc_except_table)
    }
}