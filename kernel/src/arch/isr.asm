; kernel/src/isr.asm — PORTIX v6 ISR stubs
; IRQ0 → calls pit_tick() then EOI
; All other IRQs → generic EOI stub
; CPU exceptions → call Rust handlers
BITS 64

extern pit_tick
extern isr_divide_by_zero
extern isr_bound_range
extern isr_ud_handler
extern isr_double_fault
extern isr_gp_handler
extern isr_page_fault
extern isr_generic_handler

global irq0_handler
global irq_stub_master
global irq_stub_slave
global reload_segments

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
    ; Align stack to 16 bytes (required by System V ABI for function calls)
    ; At entry RSP is 8-byte aligned (return addr pushed by CPU).
    ; After 9 pushes (72 bytes) + original 8 = 80 → not 16-aligned.
    ; Push a dummy to align.
    push    rax                 ; dummy align (RSP now 16-aligned)
    call    pit_tick
    pop     rax                 ; discard dummy
    ; EOI to PIC master
    mov     al, 0x20
    out     0x20, al
    POP_REGS
    iretq

; ─── IRQ 0x21-0x27: generic master PIC stub ─────────────────────────────────
irq_stub_master:
    push    rax
    mov     al, 0x20
    out     0x20, al            ; EOI master
    pop     rax
    iretq

; ─── IRQ 0x28-0x2F: generic slave PIC stub ──────────────────────────────────
irq_stub_slave:
    push    rax
    mov     al, 0x20
    out     0xA0, al            ; EOI slave
    out     0x20, al            ; EOI master
    pop     rax
    iretq

; ─── Reload CS via far return ────────────────────────────────────────────────
reload_segments:
    ; Build a 64-bit far return: [rip_target, 0x08] on stack
    pop     rax                 ; return address
    push    qword 0x08          ; new CS
    push    rax                 ; return RIP
    retfq

; ─── CPU exception stubs ─────────────────────────────────────────────────────
; Helpers that call the Rust #[no_mangle] handlers

global isr_0, isr_1, isr_2, isr_3, isr_4, isr_5, isr_6, isr_7
global isr_8, isr_10, isr_11, isr_12, isr_13, isr_14
global isr_16, isr_17, isr_18, isr_19

; Exceptions WITHOUT error code
%macro ISR_NOERR 2
isr_%1:
    PUSH_REGS
    push rax                    ; align
    call %2
    pop  rax
    POP_REGS
    iretq
%endmacro

; Exceptions WITH error code (already on stack by CPU)
%macro ISR_ERR 2
isr_%1:
    ; Error code is already on stack (below return addr).
    ; Pop it into RDI (first arg) before calling handler.
    pop     rdi
    PUSH_REGS
    push    rax                 ; align
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
; #DF has error code (always 0)
ISR_NOERR  8, isr_double_fault
; 9 = reserved (coprocessor segment overrun, not used)
ISR_ERR   10, isr_generic_handler   ; #TS invalid TSS
ISR_ERR   11, isr_generic_handler   ; #NP segment not present
ISR_ERR   12, isr_generic_handler   ; #SS stack fault
ISR_ERR   13, isr_gp_handler        ; #GP with error code in RDI
ISR_ERR   14, isr_page_fault        ; #PF with error code in RDI
ISR_NOERR 16, isr_generic_handler   ; #MF x87 FPE
ISR_ERR   17, isr_generic_handler   ; #AC alignment check
ISR_NOERR 18, isr_generic_handler   ; #MC machine check
ISR_NOERR 19, isr_generic_handler   ; #XM SIMD FPE