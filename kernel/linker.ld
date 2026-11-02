/* kernel/linker.ld - PORTIX v4 */
OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

/* ── Mitad superior (ciclo 49, autorizado) ─────────────────────────────────
 * El núcleo se ENLAZA en 0xFFFFFFFF80200000 (los 2 GiB superiores del espacio
 * canónico, modelo de código "kernel") pero se CARGA físicamente en 0x200000,
 * exactamente como antes: stage2/UEFI no cambian. `_start` (kernel/src/main.rs)
 * construye el mapa 0xFFFFFFFF80000000 -> físico 0 (1 GiB, páginas de 2 MiB)
 * en PML4[511] y salta a la dirección alta antes de ejecutar una sola línea de
 * Rust. Así la ventana baja 0x400000.. queda libre para binarios no-PIE.
 * KERNEL_VMA_OFFSET en kernel/src/mem/paging.rs debe coincidir con esta base.
 * ─────────────────────────────────────────────────────────────────────────── */
KERNEL_VMA_OFFSET = 0xFFFFFFFF80000000;
KERNEL_PHYS_BASE  = 0x200000;

SECTIONS
{
    . = KERNEL_VMA_OFFSET + KERNEL_PHYS_BASE;

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
