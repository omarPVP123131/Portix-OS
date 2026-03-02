; kernel/src/isr.asm — PORTIX v7 — ISR stubs con captura de CrashFrame
;
; NUEVO: Antes de llamar a cada handler Rust, llenamos la estructura
;        crash_frame (definida en isr_handlers.rs como #[no_mangle] static mut)
;        con RIP, RSP, RFLAGS, CR3, RAX, RBX, RCX, RDX, RSI, RDI, R8-R15.
;
; Layout del stack al entrar a una excepción SIN error code (el CPU empuja):
;   [RSP+0]  = RIP del faulting code
;   [RSP+8]  = CS
;   [RSP+16] = RFLAGS
;   [RSP+24] = RSP del faulting code
;   [RSP+32] = SS
;
; Layout CON error code (el CPU empuja EC antes del RIP):
;   [RSP+0]  = Error Code
;   [RSP+8]  = RIP
;   [RSP+16] = CS
;   [RSP+24] = RFLAGS
;   [RSP+32] = RSP
;   [RSP+40] = SS

BITS 64

extern pit_tick
extern isr_divide_by_zero
extern isr_bound_range
extern isr_ud_handler
extern isr_double_fault
extern isr_gp_handler
extern isr_page_fault
extern isr_generic_handler

; CrashFrame exportada desde Rust como #[no_mangle] static mut
; Offsets (ver struct CrashFrame en isr_handlers.rs):
;   +0   rip
;   +8   rsp
;   +16  rflags
;   +24  cr3
;   +32  rax
;   +40  rbx
;   +48  rcx
;   +56  rdx
;   +64  rsi
;   +72  rdi
;   +80  r8
;   +88  r9
;   +96  r10
;   +104 r11
;   +112 r12
;   +120 r13
;   +128 r14
;   +136 r15
;   +144 valid (u8)
extern crash_frame

global irq0_handler
global irq_stub_master
global irq_stub_slave
global reload_segments

; ─── Macro: llenar crash_frame ANTES de tocar los registros ─────────────────
; Se llama al inicio del stub, cuando el stack aún tiene el frame original.
; rip_offset = offset al RIP en el frame del CPU (0 sin EC, 8 con EC).
%macro CAPTURE_FRAME 1      ; arg: offset_to_rip_on_stack
    ; Usar scratch: rax, rcx, rdx — luego los restauramos del frame
    push    rax
    push    rcx
    push    rdx

    ; RIP del faulting code
    mov     rax, [rsp + 24 + %1]    ; +24 para saltar los 3 push de arriba
    mov     [crash_frame + 0], rax

    ; RSP del faulting code
    mov     rax, [rsp + 24 + %1 + 24]   ; RSP está 3 qwords después de RIP
    mov     [crash_frame + 8], rax

    ; RFLAGS
    mov     rax, [rsp + 24 + %1 + 16]
    mov     [crash_frame + 16], rax

    ; CR3
    mov     rax, cr3
    mov     [crash_frame + 24], rax

    ; Registros de propósito general (de los registros actuales, antes de corromper)
    ; rax, rcx, rdx están en el stack (los salvamos arriba), recupéralos
    mov     rax, [rsp + 16]         ; rax original (guardado 3ro)
    mov     [crash_frame + 32], rax
    mov     rax, [rsp + 8]          ; rcx original
    mov     [crash_frame + 48], rax
    mov     rax, [rsp + 0]          ; rdx original
    mov     [crash_frame + 56], rax

    ; El resto de los registros no han sido tocados todavía
    mov     [crash_frame + 40], rbx
    mov     [crash_frame + 64], rsi
    mov     [crash_frame + 72], rdi
    mov     [crash_frame + 80], r8
    mov     [crash_frame + 88], r9
    mov     [crash_frame + 96], r10
    mov     [crash_frame + 104], r11
    mov     [crash_frame + 112], r12
    mov     [crash_frame + 120], r13
    mov     [crash_frame + 128], r14
    mov     [crash_frame + 136], r15

    ; Marcar como válido
    mov     byte [crash_frame + 144], 1

    pop     rdx
    pop     rcx
    pop     rax
%endmacro

; ─── Macro: save/restore caller-saved regs (System V AMD64 ABI) ─────────────
%macro PUSH_REGS 0
    push rax
    push rcx
    push rdx
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
%endmacro

%macro POP_REGS 0
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rax
%endmacro

; ─── IRQ0: PIT tick ──────────────────────────────────────────────────────────
irq0_handler:
    PUSH_REGS
    push    rax                 ; dummy align
    call    pit_tick
    pop     rax
    mov     al, 0x20
    out     0x20, al
    POP_REGS
    iretq

; ─── IRQ 0x21-0x27: generic master PIC stub ─────────────────────────────────
irq_stub_master:
    push    rax
    mov     al, 0x20
    out     0x20, al
    pop     rax
    iretq

; ─── IRQ 0x28-0x2F: generic slave PIC stub ──────────────────────────────────
irq_stub_slave:
    push    rax
    mov     al, 0x20
    out     0xA0, al
    out     0x20, al
    pop     rax
    iretq

; ─── Reload CS via far return ────────────────────────────────────────────────
reload_segments:
    pop     rax
    push    qword 0x08
    push    rax
    retfq

; ─── CPU exception stubs ─────────────────────────────────────────────────────
global isr_0, isr_1, isr_2, isr_3, isr_4, isr_5, isr_6, isr_7
global isr_8, isr_10, isr_11, isr_12, isr_13, isr_14
global isr_16, isr_17, isr_18, isr_19

; Excepción SIN error code
; Al entrar: [RSP+0]=RIP, [RSP+8]=CS, [RSP+16]=RFLAGS, [RSP+24]=RSP, [RSP+32]=SS
%macro ISR_NOERR 2
isr_%1:
    CAPTURE_FRAME 0         ; RIP en [RSP+0] (antes de cualquier push)
    PUSH_REGS
    push rax                ; align
    call %2
    pop  rax
    POP_REGS
    iretq
%endmacro

; Excepción CON error code
; Al entrar: [RSP+0]=EC, [RSP+8]=RIP, [RSP+16]=CS, [RSP+24]=RFLAGS, [RSP+32]=RSP
%macro ISR_ERR 2
isr_%1:
    CAPTURE_FRAME 8         ; RIP en [RSP+8] (EC está en [RSP+0])
    pop     rdi             ; error code → RDI (primer arg Rust)
    PUSH_REGS
    push    rax             ; align
    call    %2
    pop     rax
    POP_REGS
    iretq
%endmacro

ISR_NOERR  0, isr_divide_by_zero
ISR_NOERR  1, isr_generic_handler
ISR_NOERR  2, isr_generic_handler
ISR_NOERR  3, isr_generic_handler
ISR_NOERR  4, isr_generic_handler
ISR_NOERR  5, isr_bound_range
ISR_NOERR  6, isr_ud_handler
ISR_NOERR  7, isr_generic_handler
ISR_NOERR  8, isr_double_fault      ; #DF error code siempre 0, tratado como NOERR
ISR_ERR   10, isr_generic_handler   ; #TS
ISR_ERR   11, isr_generic_handler   ; #NP
ISR_ERR   12, isr_generic_handler   ; #SS
ISR_ERR   13, isr_gp_handler        ; #GP
ISR_ERR   14, isr_page_fault        ; #PF
ISR_NOERR 16, isr_generic_handler   ; #MF
ISR_ERR   17, isr_generic_handler   ; #AC
ISR_NOERR 18, isr_generic_handler   ; #MC
ISR_NOERR 19, isr_generic_handler   ; #XM