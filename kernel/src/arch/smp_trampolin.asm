; =============================================================================
; smp_trampolin.asm — Trampolín de arranque de procesadores de aplicación (AP)
; =============================================================================
; T-025 (ROADMAP 5b). Este código se ensambla como binario plano (`nasm -f bin`)
; y el núcleo lo copia a la dirección física TRAMPOLIN_BASE (0x8000, < 1 MiB,
; alineada a página) antes de enviar INIT-SIPI-SIPI por el ICR de la LAPIC.
;
; Un AP arranca en modo real en CS:IP = (vector SIPI << 8):0 → 0x8000.
; Secuencia: modo real → protegido (GDT32 propia) → PAE + CR3 del núcleo +
; EFER del BSP → modo largo → salto a la entrada Rust `ap_entrada_rust(idx)`
; con la pila que el BSP preparó.
;
; Bloque de datos (rellenado por el BSP, offsets fijos respecto a 0x8000):
;   0xF00  cr3     (u64)  PML4 maestro del núcleo
;   0xF08  pila    (u64)  cima de la pila del AP (dirección virtual alta)
;   0xF10  entrada (u64)  dirección de `ap_entrada_rust`
;   0xF18  indice  (u64)  índice lógico del AP (argumento RDI)
;   0xF20  cr4     (u64)  CR4 del BSP (PAE, OSFXSR, OSXMMEXCPT, PGE…)
;   0xF28  efer    (u64)  EFER del BSP (LME, NXE, SCE)
;   0xF30  sello   (u64)  el AP escribe 0x5041 ("AP") al llegar a 32 bits y
;                         0x5058 al llegar a 64 bits (diagnóstico del BSP)
; =============================================================================
BITS 16
ORG 0x8000

TRAMPOLIN_BASE  equ 0x8000
D_CR3           equ TRAMPOLIN_BASE + 0xF00
D_PILA          equ TRAMPOLIN_BASE + 0xF08
D_ENTRADA       equ TRAMPOLIN_BASE + 0xF10
D_INDICE        equ TRAMPOLIN_BASE + 0xF18
D_CR4           equ TRAMPOLIN_BASE + 0xF20
D_EFER          equ TRAMPOLIN_BASE + 0xF28
D_SELLO         equ TRAMPOLIN_BASE + 0xF30

inicio16:
    cli
    cld
    xor     ax, ax
    mov     ds, ax
    mov     es, ax
    mov     ss, ax
    mov     sp, 0x7C00              ; pila temporal de modo real (no se usa)

    lgdt    [gdt_ptr]

    mov     eax, cr0
    or      eax, 1                  ; PE
    mov     cr0, eax
    jmp     0x08:inicio32

BITS 32
inicio32:
    mov     ax, 0x10
    mov     ds, ax
    mov     es, ax
    mov     ss, ax
    mov     fs, ax
    mov     gs, ax
    mov     esp, 0x7C00

    mov     dword [D_SELLO], 0x5041 ; "AP": llegó a 32 bits

    ; CR4 del BSP (incluye PAE, OSFXSR, OSXMMEXCPT)
    mov     eax, [D_CR4]
    or      eax, (1 << 5)           ; PAE obligatorio
    mov     cr4, eax

    ; CR3 = PML4 maestro del núcleo (< 4 GiB)
    mov     eax, [D_CR3]
    mov     cr3, eax

    ; EFER del BSP (LME, NXE, SCE)
    mov     ecx, 0xC0000080
    rdmsr
    or      eax, [D_EFER]
    or      eax, (1 << 8)           ; LME obligatorio
    wrmsr

    ; Activar paginación → modo largo (compatibilidad)
    mov     eax, cr0
    or      eax, 0x80000001         ; PG | PE
    and     eax, ~(1 << 2)          ; EM=0 (FPU presente)
    or      eax, (1 << 1)           ; MP
    mov     cr0, eax

    jmp     0x18:inicio64

BITS 64
inicio64:
    mov     ax, 0x20
    mov     ds, ax
    mov     es, ax
    mov     ss, ax
    mov     fs, ax
    mov     gs, ax

    mov     dword [abs D_SELLO], 0x5058 ; "PX": llegó a 64 bits

    mov     rsp, [abs D_PILA]
    and     rsp, ~0xF
    mov     rdi, [abs D_INDICE]
    mov     rax, [abs D_ENTRADA]
    xor     rbp, rbp
    call    rax
.muerto:
    cli
    hlt
    jmp     .muerto

; ── GDT temporal del trampolín (código/datos 32 y 64 bits) ──────────────────
ALIGN 16
gdt_inicio:
    dq 0x0000000000000000           ; 0x00 nulo
    dq 0x00CF9A000000FFFF           ; 0x08 código 32 bits, base 0, límite 4 GiB
    dq 0x00CF92000000FFFF           ; 0x10 datos 32 bits
    dq 0x00AF9A000000FFFF           ; 0x18 código 64 bits (L=1)
    dq 0x00AF92000000FFFF           ; 0x20 datos 64 bits
gdt_fin:
gdt_ptr:
    dw gdt_fin - gdt_inicio - 1
    dd gdt_inicio

; Relleno hasta el bloque de datos (0xF00) para que el binario tenga tamaño fijo
times 0xF00 - ($ - $$) db 0
    dq 0                            ; 0xF00 cr3
    dq 0                            ; 0xF08 pila
    dq 0                            ; 0xF10 entrada
    dq 0                            ; 0xF18 indice
    dq 0                            ; 0xF20 cr4
    dq 0                            ; 0xF28 efer
    dq 0                            ; 0xF30 sello
times 0x1000 - ($ - $$) db 0
