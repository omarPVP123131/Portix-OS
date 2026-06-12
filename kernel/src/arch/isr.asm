; =============================================================================
; kernel/src/arch/isr.asm — PORTIX OS  isr/irq stubs
; =============================================================================
;
; Responsabilidades:
;   • Capturar CrashFrame antes de tocar ningún registro (CAPTURE_FRAME).
;   • Salvar/restaurar los 15 GPRs vía PUSH_REGS / POP_REGS.
;   • Despachar cada excepción CPU a su handler Rust correspondiente.
;   • Soportar context-switch: los handlers devuelven 0 (continuar) o un
;     nuevo RSP (cambiar al proceso cuyo kernel-stack tiene ese RSP).
;   • IRQ0 (PIT): EOI temprano + pit_tick + schedule_tick.
;   • IRQ1 (teclado PS/2): leer 0x60, llamar irq1_handler_rust, EOI.
;   • IRQ stubs genéricos para IRQs 2-7 (master) y 8-15 (slave).
;   • int 0x80 y SYSCALL MSR como puertas a ring-3.
;   • ISR_NOERR_FATAL para excepciones que NUNCA pueden reanudar en ring-0
;     (evita el loop #UD -> #GP -> #DF que colapsaba el sistema).
;
; Tres macros de excepcion:
;   ISR_NOERR       - sin error code, puede reanudar (ring-3 recovery via
;                     try_recover_ring3, o iretq si handler devuelve 0).
;   ISR_NOERR_FATAL - sin error code, ring-0 FATAL: si el handler devuelve 0
;                     entra en cli+hlt en lugar de iretq (evita loop).
;                     Si devuelve != 0 es un RSP de context-switch -> iretq.
;   ISR_ERR         - con error code (pop rdi antes de PUSH_REGS).
;
; Layout del stack al entrar a una excepcion SIN error code:
;   [RSP+ 0] = RIP   del codigo faulting
;   [RSP+ 8] = CS
;   [RSP+16] = RFLAGS
;   [RSP+24] = RSP   del codigo faulting
;   [RSP+32] = SS
;
; Layout CON error code:
;   [RSP+ 0] = Error Code
;   [RSP+ 8] = RIP
;   [RSP+16] = CS
;   [RSP+24] = RFLAGS
;   [RSP+32] = RSP
;   [RSP+40] = SS
;
; Tras PUSH_REGS (15 x 8 = 120 bytes) el IRET frame queda desplazado:
;   ISR_NOERR : CS en [RSP + 120 + 8]  = [RSP + 128]
;   ISR_ERR   : CS en [RSP + 120 + 8]  = [RSP + 128]
;               (el pop rdi ocurrio ANTES de PUSH_REGS, mismo offset)
;
; CrashFrame offsets (ver isr_handlers.rs struct CrashFrame):
;   +0   rip    +8  rsp   +16 rflags  +24 cr3
;   +32  rax    +40 rbx   +48 rcx     +56 rdx
;   +64  rsi    +72 rdi   +80 r8      +88 r9
;   +96  r10   +104 r11  +112 r12    +120 r13
;  +128  r14   +136 r15  +144 rbp    +152 valid (u8)
; =============================================================================

BITS 64
[default abs]

; -- Externos Rust -------------------------------------------------------------
extern pit_tick
extern schedule_tick
extern irq1_handler_rust
extern irq12_handler_rust
extern isr_divide_by_zero
extern isr_bound_range
extern isr_ud_handler
extern isr_double_fault
extern isr_gp_handler
extern isr_page_fault
extern isr_generic_handler
extern syscall_dispatch
extern enter_ring3_setup
extern process_save_ring3_ret_addr
extern process_exit_trampoline
extern crash_frame
extern exception_cs
extern __stack_top
extern ipc_notify_irq_handler

; -- Globales para Rust / linker -----------------------------------------------
global irq0_handler
global irq1_handler
global irq12_handler
global irq_stub_master
global irq_stub_slave
global reload_segments
global syscall_entry
global int80_handler
global ring3_exit_trampoline
global enter_ring3_asm
global isr_0,  isr_1,  isr_2,  isr_3,  isr_4,  isr_5,  isr_6,  isr_7
global isr_8,  isr_10, isr_11, isr_12, isr_13, isr_14
global isr_16, isr_17, isr_18, isr_19

; =============================================================================
; .data
; =============================================================================
section .data
global ring3_ret_rsp
ring3_ret_rsp: dq 0

; =============================================================================
; CAPTURE_FRAME  arg: byte-offset al RIP dentro del IRET frame del CPU
; =============================================================================
%macro CAPTURE_FRAME 1
    push    rax
    push    rcx
    push    rdx

    mov     rax, [rsp + 24 + %1]
    mov     [crash_frame + 0], rax

    mov     rax, [rsp + 24 + %1 + 24]
    mov     [crash_frame + 8], rax

    mov     rax, [rsp + 24 + %1 + 16]
    mov     [crash_frame + 16], rax

    mov     rax, cr3
    mov     [crash_frame + 24], rax

    mov     rax, [rsp + 16]
    mov     [crash_frame + 32], rax
    mov     rax, [rsp + 8]
    mov     [crash_frame + 48], rax
    mov     rax, [rsp + 0]
    mov     [crash_frame + 56], rax

    mov     [crash_frame +  40], rbx
    mov     [crash_frame +  64], rsi
    mov     [crash_frame +  72], rdi
    mov     [crash_frame +  80], r8
    mov     [crash_frame +  88], r9
    mov     [crash_frame +  96], r10
    mov     [crash_frame + 104], r11
    mov     [crash_frame + 112], r12
    mov     [crash_frame + 120], r13
    mov     [crash_frame + 128], r14
    mov     [crash_frame + 136], r15
    mov     [crash_frame + 144], rbp

    mov     byte [crash_frame + 152], 1

    pop     rdx
    pop     rcx
    pop     rax
%endmacro

; =============================================================================
; PUSH_REGS / POP_REGS
; =============================================================================
%macro PUSH_REGS 0
    push    r15
    push    r14
    push    r13
    push    r12
    push    rbp
    push    rbx
    push    r11
    push    r10
    push    r9
    push    r8
    push    rdi
    push    rsi
    push    rdx
    push    rcx
    push    rax
    ; Guardar registros XMM (SSE) — 16 bytes c/u, necesita alineación 16
    sub     rsp, 256        ; espacio para xmm0-xmm15 (16 regs × 16 bytes)
    movups  [rsp + 0],   xmm0
    movups  [rsp + 16],  xmm1
    movups  [rsp + 32],  xmm2
    movups  [rsp + 48],  xmm3
    movups  [rsp + 64],  xmm4
    movups  [rsp + 80],  xmm5
    movups  [rsp + 96],  xmm6
    movups  [rsp + 112], xmm7
    movups  [rsp + 128], xmm8
    movups  [rsp + 144], xmm9
    movups  [rsp + 160], xmm10
    movups  [rsp + 176], xmm11
    movups  [rsp + 192], xmm12
    movups  [rsp + 208], xmm13
    movups  [rsp + 224], xmm14
    movups  [rsp + 240], xmm15
%endmacro

%macro POP_REGS 0
    ; Restaurar registros XMM
    movups  xmm0,  [rsp + 0]
    movups  xmm1,  [rsp + 16]
    movups  xmm2,  [rsp + 32]
    movups  xmm3,  [rsp + 48]
    movups  xmm4,  [rsp + 64]
    movups  xmm5,  [rsp + 80]
    movups  xmm6,  [rsp + 96]
    movups  xmm7,  [rsp + 112]
    movups  xmm8,  [rsp + 128]
    movups  xmm9,  [rsp + 144]
    movups  xmm10, [rsp + 160]
    movups  xmm11, [rsp + 176]
    movups  xmm12, [rsp + 192]
    movups  xmm13, [rsp + 208]
    movups  xmm14, [rsp + 224]
    movups  xmm15, [rsp + 240]
    add     rsp, 256
    ; Restaurar GPRs
    pop     rax
    pop     rcx
    pop     rdx
    pop     rsi
    pop     rdi
    pop     r8
    pop     r9
    pop     r10
    pop     r11
    pop     rbx
    pop     rbp
    pop     r12
    pop     r13
    pop     r14
    pop     r15
%endmacro
; =============================================================================
; ISR_NOERR -- excepcion SIN error code, recuperable
; =============================================================================
%macro ISR_NOERR 2
isr_%1:
    CAPTURE_FRAME 0
    PUSH_REGS
    mov     rax, [rsp + 384]
    mov     [exception_cs], rax
    call    %2
    test    rax, rax
    jnz     .ctx_%1
    POP_REGS
    iretq
.ctx_%1:
    mov     rsp, rax
    POP_REGS
    iretq
%endmacro

; =============================================================================
; ISR_NOERR_FATAL -- excepcion SIN error code que NO puede reanudar en ring-0
;
; Si handler devuelve 0 desde ring-0: cli + hlt permanente.
; Si handler devuelve != 0: context-switch normal -> iretq.
;
; Uso obligatorio para #UD (vector 6): hacer iretq al mismo RIP faulting
; volveria a disparar #UD infinitamente hasta agotar el stack -> #DF.
; =============================================================================
%macro ISR_NOERR_FATAL 2
isr_%1:
    CAPTURE_FRAME 0
    PUSH_REGS
    mov     rax, [rsp + 384]
    mov     [exception_cs], rax
    call    %2
    test    rax, rax
    jnz     .ctx_%1
    cli
.hlt_%1:
    hlt
    jmp     .hlt_%1
.ctx_%1:
    mov     rsp, rax
    POP_REGS
    iretq
%endmacro

; =============================================================================
; ISR_ERR -- excepcion CON error code
; =============================================================================
%macro ISR_ERR 2
isr_%1:
    CAPTURE_FRAME 8
    pop     rdi
    PUSH_REGS
    mov     rax, [rsp + 384]
    mov     [exception_cs], rax
    call    %2
    test    rax, rax
    jnz     .ctx_%1
    POP_REGS
    iretq
.ctx_%1:
    mov     rsp, rax
    POP_REGS
    iretq
%endmacro

; =============================================================================
; Tabla de stubs de excepcion CPU (vectores 0-19)
; =============================================================================
ISR_NOERR        0, isr_divide_by_zero   ; #DE  Division por cero
ISR_NOERR        1, isr_generic_handler  ; #DB  Debug
ISR_NOERR        2, isr_generic_handler  ; NMI  Non-Maskable Interrupt
ISR_NOERR        3, isr_generic_handler  ; #BP  Breakpoint (int3)
ISR_NOERR        4, isr_generic_handler  ; #OF  Overflow (into)
ISR_NOERR        5, isr_bound_range      ; #BR  Bound Range Exceeded
ISR_NOERR_FATAL  6, isr_ud_handler       ; #UD  Invalid Opcode - FATAL en ring-0
ISR_NOERR        7, isr_generic_handler  ; #NM  Device Not Available (FPU ausente)
ISR_ERR          8, isr_double_fault     ; #DF  Double Fault (EC siempre 0, IST1)
ISR_ERR         10, isr_generic_handler  ; #TS  Invalid TSS
ISR_ERR         11, isr_generic_handler  ; #NP  Segment Not Present
ISR_ERR         12, isr_generic_handler  ; #SS  Stack-Segment Fault
ISR_ERR         13, isr_gp_handler       ; #GP  General Protection Fault
ISR_ERR         14, isr_page_fault       ; #PF  Page Fault
ISR_NOERR       16, isr_generic_handler  ; #MF  x87 FPU Floating-Point Exception
ISR_ERR         17, isr_generic_handler  ; #AC  Alignment Check
ISR_NOERR       18, isr_generic_handler  ; #MC  Machine Check
ISR_NOERR       19, isr_generic_handler  ; #XM  SIMD FP Exception

; =============================================================================
; IRQ0 -- PIT Timer (100 Hz) + Scheduler
; =============================================================================
irq0_handler:
    PUSH_REGS
    mov     al, 0x20
    out     0x20, al                ; EOI temprano al PIC maestro
    call    pit_tick
    mov     rdi, rsp                ; arg0: RSP actual (= kernel_rsp del proc)
    mov     rsi, [rsp + 384]        ; arg1: CS del IRET frame (CPL detection)
    call    schedule_tick
    test    rax, rax
    jz      .no_switch
    mov     rsp, rax
.no_switch:
    POP_REGS
    iretq

; =============================================================================
; IRQ1 -- Teclado PS/2
; =============================================================================
; Lee el scancode del puerto 0x60 y lo envia a irq1_handler_rust que lo
; mete en SCANCODE_BUF. El loop de main drena ese buffer via pop_scancode().
;
; REGLA: nadie mas puede leer 0x60. Cualquier lectura fuera de aqui roba
; el byte del FIFO del 8042 y lo hace invisible para este handler.
; break-codes robados dejan modificadores "presionados para siempre",
; generando scancodes invalidos que causan #UD -> #GP -> #DF en cascada.
;
; [FIX-REG-CORRUPTION] Ahora usa PUSH_REGS/POP_REGS para preservar
; TODOS los registros (incluyendo rdi, rsi, r8-r11 que extern "C" clobberea).
; =============================================================================
irq1_handler:
    PUSH_REGS
    xor     eax, eax
    in      al, 0x64
    test    al, 0x01
    jz      .eoi1
    test    al, 0x20
    jnz     .eoi1
    in      al, 0x60
    mov     edi, eax
    call    irq1_handler_rust

.eoi1:
    mov     al, 0x20
    out     0x20, al                ; EOI master PIC
    POP_REGS
    iretq

; =============================================================================
; IRQ12 -- Raton PS/2
; =============================================================================
; IRQ12 es el IRQ del raton en el PIC esclavo (IRQ 4 del esclavo = vector 0x2C).
; Lee el byte del puerto 0x60 SOLO si AUXB (bit5 del status) esta activo.
; Si AUXB=0 el byte es de teclado y NO debemos tocarlo — IRQ1 lo manejara.
;
; REGLA: Nunca leer 0x60 sin verificar AUXB primero. El 8042 mezcla bytes
; de teclado y raton en el mismo FIFO, y leer el byte incorrecto rompe
; ambos flujos de datos (scancodes huerfanos, paquetes de raton desync).
;
; [FIX-REG-CORRUPTION] Ahora usa PUSH_REGS/POP_REGS para preservar
; TODOS los registros (incluyendo rdi, rsi, r8-r11 que extern "C" clobberea).
; =============================================================================
irq12_handler:
    PUSH_REGS
    xor     eax, eax
    in      al, 0x64                ; leer status register
    test    al, 0x20                ; AUXB = bit 5
    jz      .eoi12                  ; no es raton -> EOI sin leer
    in      al, 0x60                ; leer byte de raton
    mov     edi, eax
    call    irq12_handler_rust
.eoi12:
    mov     al, 0x20
    out     0xA0, al                ; EOI slave  PIC
    out     0x20, al                ; EOI master PIC (cascade)
    POP_REGS
    iretq

; =============================================================================
; IRQ stubs genericos
; =============================================================================
irq_stub_master:
    push    rax
    mov     al, 0x20
    out     0x20, al
    pop     rax
    iretq

irq_stub_slave:
    push    rax
    mov     al, 0x20
    out     0xA0, al                ; EOI slave PIC
    out     0x20, al                ; EOI master PIC (cascade)
    pop     rax
    iretq

; =============================================================================
; IRQ14 -- ATA Primary Channel
; =============================================================================
irq14_handler:
    PUSH_REGS
    mov     edi, 14
    call    ipc_notify_irq_handler
    mov     al, 0x20
    out     0xA0, al                ; EOI slave
    out     0x20, al                ; EOI master (cascade)
    POP_REGS
    iretq

; =============================================================================
; IRQ15 -- ATA Secondary Channel
; =============================================================================
irq15_handler:
    PUSH_REGS
    mov     edi, 15
    call    ipc_notify_irq_handler
    mov     al, 0x20
    out     0xA0, al                ; EOI slave
    out     0x20, al                ; EOI master (cascade)
    POP_REGS
    iretq

; =============================================================================
; reload_segments -- recargar CS mediante far return
; =============================================================================
reload_segments:
    pop     rax
    push    qword 0x08
    push    rax
    retfq

; =============================================================================
; int80_handler -- syscall via int 0x80 (ring-3 -> ring-0)
; =============================================================================
; Al entrar (CPU cambio al kernel-stack via TSS.RSP0):
;   RAX=num_syscall  RDI=a1 RSI=a2 RDX=a3 R10=a4 R8=a5 R9=a6
;   Stack: [RIP][CS][RFLAGS][RSP_user][SS]
;
; syscall_dispatch devuelve SyscallResult { result:RAX, new_rsp:RDX }.
;   RDX=0   -> continuar proceso actual
;   RDX!=0  -> context-switch al proceso con ese kernel-RSP
;
; El resultado se escribe en el RAX guardado [RSP+0] para que POP_REGS
; lo restaure al volver a usuario.
; =============================================================================
int80_handler:
    PUSH_REGS
    mov     r9,  r8                 ; reordenar args para syscall_dispatch
    mov     r8,  r10
    mov     rcx, rdx
    mov     rdx, rsi
    mov     rsi, rdi
    mov     rdi, rax
    mov     rbx, rsp                ; rbx = current_rsp
    sub     rsp, 8                  ; alinear stack a 16 bytes
    push    rbx                     ; 7o arg: current_rsp
    call    syscall_dispatch
    add     rsp, 16                 ; limpiar alineacion(8) + push(8)
    mov     [rsp], rax              ; resultado -> RAX guardado
    test    rdx, rdx
    jz      .cont80
    mov     rsp, rdx
.cont80:
    POP_REGS
    iretq

; =============================================================================
; syscall_entry -- SYSCALL MSR (ring-3 -> ring-0)
; =============================================================================
; Tras SYSCALL:
;   RAX=num  RCX=RIP_usuario  R11=RFLAGS_usuario
;   RDI RSI RDX R10 R8 R9 = args 1-6
;   RSP = RSP de usuario (sin cambiar por CPU)
;
; Construimos IRET frame sintetico para que el context-switch funcione
; igual que con int 0x80.
; =============================================================================
syscall_entry:
    swapgs
    mov     r15, rsp                ; RSP usuario
    mov     r14, r11                ; RFLAGS usuario
    mov     r13, rcx                ; RIP   usuario
    lea     rsp, [rel __stack_top]  ; cambiar al kernel-stack estatico
    push    0x1B                    ; SS  = USER_DS|3
    push    r15                     ; RSP usuario
    push    r14                     ; RFLAGS usuario
    push    0x23                    ; CS  = USER_CS|3
    push    r13                     ; RIP usuario
    PUSH_REGS
    mov     r9,  r8
    mov     r8,  r10
    mov     rcx, rdx
    mov     rdx, rsi
    mov     rsi, rdi
    mov     rdi, rax
    mov     rbx, rsp
    sub     rsp, 8
    push    rbx
    call    syscall_dispatch
    add     rsp, 16
    mov     [rsp], rax
    test    rdx, rdx
    jz      .cont_sc
    mov     rsp, rdx
.cont_sc:
    POP_REGS
    swapgs
    iretq

; =============================================================================
; enter_ring3_asm -- transicion al primer proceso de usuario
; =============================================================================
enter_ring3_asm:
    pop     rax                     ; dir. retorno a rust_main
    mov     rdi, rax
    call    process_save_ring3_ret_addr
    push    rax
    call    enter_ring3_setup       ; hace IRETQ -> nunca retorna
.dead:
    cli
    hlt
    jmp     .dead

; =============================================================================
; ring3_exit_trampoline -- salida limpia de proceso de ring-3
; =============================================================================
ring3_exit_trampoline:
    call    process_exit_trampoline ; nunca retorna
.dead:
    cli
    hlt
    jmp     .dead