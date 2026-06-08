; lib/src/crt0.s — Ring-3 _start entry point for PORTIX
;
; Called by the kernel's IRETQ to ring 3.
; Sets up stack, calls _init, then main(), then exit().
; The kernel passes no arguments for now (no argv/envp).

BITS 64

extern main
extern exit
extern _init

global _start

section .text

_start:
    ; Clear frame pointer (no stack trace needed)
    xor     rbp, rbp

    ; Call global constructors (if any)
    call    _init

    ; main(argc, argv, envp) — all zero for now
    xor     edi, edi        ; argc = 0
    xor     esi, esi        ; argv = NULL
    xor     edx, edx        ; envp = NULL
    call    main

    ; exit(code)
    mov     edi, eax
    call    exit

    ; Should never reach here
    hlt
