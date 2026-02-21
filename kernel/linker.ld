    /* kernel/linker.ld - PORTIX v3 */
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

        /DISCARD/ : {
            *(.eh_frame)
            *(.eh_frame_hdr)
            *(.note .note.*)
            *(.comment)
            *(.gcc_except_table)
        }
    }
