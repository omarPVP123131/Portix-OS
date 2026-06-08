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
[default abs]

extern pit_tick
extern isr_divide_by_zero
extern isr_bound_range
extern isr_ud_handler
extern isr_double_fault
extern isr_gp_handler
extern isr_page_fault
extern isr_generic_handler
extern syscall_dispatch
extern enter_ring3_setup
extern __stack_top
extern schedule_tick
extern process_save_ring3_ret_addr
extern process_save_ring3_ret_rsp
extern process_exit_trampoline

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
extern exception_cs

global irq0_handler
global irq1_handler
global irq_stub_master
global irq_stub_slave
global reload_segments
global syscall_entry
global int80_handler
global ring3_exit_trampoline

; ─── Guardar RSP de usuario (syscall_entry) ──────────────────────────────
section .data
syscall_count: dq 0
user_rsp_save: dq 0
user_rflags_save: dq 0   ; RFLAGS original (syscall los guarda en R11)
user_rip_save: dq 0       ; RIP original (syscall lo guarda en RCX)
global ring3_ret_rsp
ring3_ret_rsp: dq 0       ; kernel RSP to restore on ring-3 exit (points to resume fn address)

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
    mov     byte [crash_frame + 152], 1

    pop     rdx
    pop     rcx
    pop     rax
%endmacro

; ─── Macro: save/restore caller-saved regs (System V AMD64 ABI) ─────────────
; Save/restore ALL 15 general-purpose registers (RSP saved via IRET frame).
; Context switch between processes requires full register preservation.
%macro PUSH_REGS 0
    push r15
    push r14
    push r13
    push r12
    push rbp
    push rbx
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rdx
    push rcx
    push rax
%endmacro

%macro POP_REGS 0
    pop rax
    pop rcx
    pop rdx
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop rbx
    pop rbp
    pop r12
    pop r13
    pop r14
    pop r15
%endmacro

; ─── IRQ0: PIT tick + scheduler ─────────────────────────────────────────────
; On entry (ring 3 → ring 0):
;   Stack: [PUSH_REGS] [IRET frame: RIP, CS, RFLAGS, RSP_user, SS]
;   RSP after PUSH_REGS points to saved r11.
;
; schedule_tick(current_rsp, saved_cs) returns new RSP (0 = no switch).
irq0_handler:
    PUSH_REGS
    mov     al, 0x20
    out     0x20, al                    ; EOI early
    call    pit_tick
    mov     rdi, rsp                    ; arg0 = current RSP (after PUSH_REGS)
    mov     rsi, [rsp + 128]            ; arg1 = saved CS (15 pushes × 8 = 120, CS at +8 in IRET)
    call    schedule_tick
    test    rax, rax
    jz      .no_switch
    mov     rsp, rax                    ; switch to new process's kernel stack
.no_switch:
    POP_REGS
    iretq

; ─── IRQ1 (keyboard) handler ─────────────────────────────────────────────
; Reads scancode from PS/2 data port, calls Rust irq1_handler_rust, sends EOI.
extern irq1_handler_rust
irq1_handler:
    push    rax
    push    rdx
    xor     eax, eax
    in      al, 0x60                ; read scancode
    mov     edi, eax                ; arg0 = scancode
    call    irq1_handler_rust
    mov     al, 0x20
    out     0x20, al                ; EOI master PIC
    pop     rdx
    pop     rax
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
; El handler devuelve 0 (continuar) o nuevo RSP (cambio de contexto).
%macro ISR_NOERR 2
isr_%1:
    CAPTURE_FRAME 0         ; RIP en [RSP+0] (antes de cualquier push)
    PUSH_REGS
    mov     rax, [rsp + 128] ; CS del IRET frame (15 pushes × 8 = 120, CS en +8)
    mov     [exception_cs], rax
    call    %2
    test    rax, rax
    jnz     .switch_%1
    POP_REGS
    iretq
.switch_%1:
    mov     rsp, rax
    POP_REGS
    iretq
%endmacro

; Excepción CON error code
; Al entrar: [RSP+0]=EC, [RSP+8]=RIP, [RSP+16]=CS, [RSP+24]=RFLAGS, [RSP+32]=RSP
; El handler devuelve 0 (continuar) o nuevo RSP (cambio de contexto).
%macro ISR_ERR 2
isr_%1:
    CAPTURE_FRAME 8         ; RIP en [RSP+8] (EC está en [RSP+0])
    pop     rdi             ; error code → RDI (primer arg Rust)
    PUSH_REGS
    mov     rax, [rsp + 128] ; CS del IRET frame (15 pushes × 8 = 120, CS en +8)
    mov     [exception_cs], rax
    call    %2
    test    rax, rax
    jnz     .switch_%1
    POP_REGS
    iretq
.switch_%1:
    mov     rsp, rax
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
ISR_ERR  8, isr_double_fault        ; #DF error code siempre 0
ISR_ERR   10, isr_generic_handler   ; #TS
ISR_ERR   11, isr_generic_handler   ; #NP
ISR_ERR   12, isr_generic_handler   ; #SS
ISR_ERR   13, isr_gp_handler        ; #GP
ISR_ERR   14, isr_page_fault        ; #PF
ISR_NOERR 16, isr_generic_handler   ; #MF
ISR_ERR   17, isr_generic_handler   ; #AC
ISR_NOERR 18, isr_generic_handler   ; #MC
ISR_NOERR 19, isr_generic_handler   ; #XM

; ─── int 0x80 handler (ring 3 → ring 0 via int 0x80) ────────────────────────
; On entry (CPU has already switched to ring-0 stack via TSS.RSP0):
;   Stack: [RSP+0]=RIP, [RSP+8]=CS, [RSP+16]=RFLAGS, [RSP+24]=RSP, [RSP+32]=SS
;   RAX = syscall number
;   RDI, RSI, RDX, R10, R8, R9 = args 1-6
;   (int preserves all registers)
; int80_handler — supports context switch:
;   syscall_dispatch returns SyscallResult(result, new_rsp) in RAX:RDX.
;   If RDX != 0, load it as new RSP and POP_REGS + IRETQ to a different process.
;   Stack alignment: PUSH_REGS (72) + IRET frame (40) = 112 = 0 mod 16.
;   We allocate 16 more bytes to reach 128 = 0 mod 16 before call.
int80_handler:
    PUSH_REGS
    ; Reorder args for syscall_dispatch(rdi=num, rsi=a1, rdx=a2, rcx=a3, r8=a4, r9=a5, [rsp+8]=a6=current_rsp)
    mov     r9, r8
    mov     r8, r10
    mov     rcx, rdx
    mov     rdx, rsi
    mov     rsi, rdi
    mov     rdi, rax
    mov     rbx, rsp                ; rbx = RSP after PUSH_REGS (= current_rsp)
    sub     rsp, 8                  ; alignment padding
    push    rbx                     ; 7th arg = current_rsp (pushed last, on top)
    call    syscall_dispatch
    add     rsp, 16                 ; clean up: sub(8) + push(8) = 16, RSP back to PUSH_REGS level
    ; RAX = result, RDX = new_rsp (0 = continue, non-zero = switch)
    ; Saved RAX is at [rsp + 0] (pushed last, closest to RSP)
    mov     [rsp], rax              ; store result in saved RAX on stack
    test    rdx, rdx
    jz      .cont
    mov     rsp, rdx                ; switch to new process kernel stack
.cont:
    POP_REGS
    iretq

; ─── enter_ring3_asm — ring-3 transition with ret-addr save ───────────
; Called from rust_main.  Saves the return address before calling
; the Rust enter_ring3_setup which does IRETQ to ring 3.
; At entry: [rsp] = return address to rust_main (pushed by call)
global enter_ring3_asm
enter_ring3_asm:
    pop     rax                     ; RAX = return addr to rust_main
    mov     rdi, rax                ; arg0 = return address
    call    process_save_ring3_ret_addr
    push    rax                     ; push it back on stack
    call    enter_ring3_setup
    ; Never reached — setup does IRETQ.  Trampoline restores RSP and rets.
    hlt
    jmp     enter_ring3_asm

; ─── ring3_exit_trampoline — return from ring 3 to kernel ────────────────
; Called from sys_exit.
; Delegates to Rust process_exit_trampoline which handles per-process RSP.
ring3_exit_trampoline:
    call    process_exit_trampoline
    ; never reached
    hlt
    jmp     ring3_exit_trampoline

; ─── syscall_entry (SYSCALL de ring 3 a ring 0) ─────────────────────────────
; On entry (after SYSCALL):
;   RAX = syscall number
;   RCX = user RIP (poisoned by SYSCALL)
;   R11 = user RFLAGS (poisoned by SYSCALL)
;   RDI, RSI, RDX, R10, R8, R9 = args 1-6
;   RSP = user RSP (unchanged)
;   SS  = set to STAR[47:32]+8 by CPU (SS.DPL forced to 0)
;   CS  = KERNEL_CS
;
; Registers preserved across syscall: RBX, RBP, R12-R15
syscall_entry:
    ; DEBUG: print 'E' BEFORE swapgs to confirm SYSCALL entry
    push    rax
    push    rdx
    mov     al, 'E'
    mov     dx, 0x3F8
    out     dx, al
    pop     rdx
    pop     rax

    swapgs

    ; Save user context in callee-saved registers (preserved by C ABI)
    mov     r12, rsp        ; user RSP
    mov     r13, r11        ; user RFLAGS
    mov     r14, rcx        ; user RIP

    lea     rsp, [rel __stack_top]

    ; Reorder: user RAX→RDI, RDI→RSI, RSI→RDX, RDX→RCX, R10→R8, R8→R9
    mov     r9, r8          ; a5
    mov     r8, r10         ; a4
    mov     rcx, rdx        ; a3
    mov     rdx, rsi        ; a2
    mov     rsi, rdi        ; a1
    mov     rdi, rax        ; syscall number

    call    syscall_dispatch

    ; Restore user RSP, RFLAGS(R11), RIP(RCX)
    mov     rsp, r12
    mov     r11, r13
    mov     rcx, r14
    swapgs
    sysret
