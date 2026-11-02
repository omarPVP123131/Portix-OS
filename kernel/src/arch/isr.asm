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
extern liberar_contexto_saliente
extern irq1_handler_rust
extern irq12_handler_rust
extern isr_divide_by_zero
extern isr_dbg_handler
extern isr_bp_handler
extern isr_bound_range
extern isr_ud_handler
extern isr_double_fault
extern isr_gp_handler
extern isr_page_fault
extern isr_generic_handler
extern syscall_dispatch
extern linux_syscall_dispatch
extern enter_ring3_setup
extern process_save_ring3_ret_addr
extern process_exit_trampoline
extern crash_frame
extern __stack_top
extern ipc_notify_irq_handler
extern is_irq_routed
extern ipi_handler_rust
extern ap_tick_rust          ; T-025 SMP: manejador de IPI (Rust)

; -- Globales para Rust / linker -----------------------------------------------
global irq0_handler
global irq1_handler
global irq12_handler
global irq14_handler
global irq15_handler
global irq_stub_master
global irq_stub_slave
global reload_segments
global syscall_entry
global int80_handler
global ring3_exit_trampoline
global enter_ring3_asm
global switch_to_process_asm
global isr_0,  isr_1,  isr_2,  isr_3,  isr_4,  isr_5,  isr_6,  isr_7
global isr_8,  isr_10, isr_11, isr_12, isr_13, isr_14
global isr_16, isr_17, isr_18, isr_19

; =============================================================================
; .data
; =============================================================================
section .data
global syscall_user_rsp
syscall_user_rsp: dq 0
; [CICLO v6.x] Cima de la pila de kernel del PROCESO EN EJECUCION. La puerta
; `syscall` construye su frame aqui (no en una pila estatica compartida): si
; todos los procesos usaran la misma pila, el frame de un proceso BLOQUEADO en
; un syscall (waitpid, read de tuberia...) seria sobrescrito por los syscalls de
; cualquier otro proceso y al reanudar restauraria un contexto basura.
global current_kstack_top
current_kstack_top: dq 0

section .text

; =============================================================================
; CAPTURE_FRAME  arg: byte-offset al RIP dentro del IRET frame del CPU
; =============================================================================
%macro CAPTURE_FRAME 1
    ; [E3d] por CPU: PerCpu.crash_frame en gs:104 (OFF_CRASH_FRAME); GS ya es el del nucleo
    push    rax
    push    rcx
    push    rdx

    mov     rax, [rsp + 24 + %1]
    mov     [gs:104], rax

    mov     rax, [rsp + 24 + %1 + 24]
    mov     [gs:112], rax

    mov     rax, [rsp + 24 + %1 + 16]
    mov     [gs:120], rax

    mov     rax, cr3
    mov     [gs:128], rax

    mov     rax, [rsp + 16]
    mov     [gs:136], rax
    mov     rax, [rsp + 8]
    mov     [gs:152], rax
    mov     rax, [rsp + 0]
    mov     [gs:160], rax

    mov     [gs:144], rbx
    mov     [gs:168], rsi
    mov     [gs:176], rdi
    mov     [gs:184], r8
    mov     [gs:192], r9
    mov     [gs:200], r10
    mov     [gs:208], r11
    mov     [gs:216], r12
    mov     [gs:224], r13
    mov     [gs:232], r14
    mov     [gs:240], r15
    mov     [gs:248], rbp

    mov     byte [gs:256], 1

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
; [T-025 E3a] GS por CPU. En el nucleo GS_BASE apunta al bloque PerCpu de la
; CPU actual (kernel/src/arch/percpu.rs: +0 puntero propio, +8 cima de la pila
; de nucleo del proceso en ejecucion, +16 RSP de usuario de la syscall en
; curso). En Ring 3 GS_BASE es el del usuario (0) y KERNEL_GS_BASE guarda el
; del nucleo: `swapgs` los intercambia. Invariante: toda entrada DESDE Ring 3
; hace swapgs y toda salida HACIA Ring 3 lo deshace; las entradas desde Ring 0
; (IRQ en el nucleo, frames expropiados) no tocan GS.
; =============================================================================
%macro SWAPGS_ENTRADA 1             ; %1 = desplazamiento del CS del frame
    test    qword [rsp + %1], 3
    jz      %%nucleo
    swapgs
%%nucleo:
%endmacro

; [T-025 E3d] Tras `mov rsp, <pila del entrante>`: liberar el contexto saliente
; (ver percpu.saliente). rbx se repone con POP_REGS; la pila se alinea a 16.
%macro LIBERAR_SALIENTE 0
    mov     rbx, rsp
    and     rsp, -16
    call    liberar_contexto_saliente
    mov     rsp, rbx
%endmacro

%macro SWAPGS_SALIDA 0              ; antes de POP_REGS: CS en [rsp+384]
    cli                             ; ventana swapgs..iretq sin IRQ: una IRQ
                                    ; aqui veria CS=0x08 y no repondria el GS
                                    ; del nucleo (iretq restaura el IF del frame)
    test    qword [rsp + 384], 3
    jz      %%nucleo
    swapgs
%%nucleo:
%endmacro

; =============================================================================
; ISR_NOERR -- excepcion SIN error code, recuperable
; =============================================================================
%macro ISR_NOERR 2
isr_%1:
    SWAPGS_ENTRADA 8
    CAPTURE_FRAME 0
    PUSH_REGS
    mov     rax, [rsp + 384]
    mov     [gs:48], rax            ; [E3b] PerCpu.exception_cs (GS ya intercambiado)
    call    %2
    test    rax, rax
    jnz     .ctx_%1
    SWAPGS_SALIDA
    POP_REGS
    iretq
.ctx_%1:
    mov     rsp, rax
    SWAPGS_SALIDA
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
    SWAPGS_ENTRADA 8
    CAPTURE_FRAME 0
    PUSH_REGS
    mov     rax, [rsp + 384]
    mov     [gs:48], rax            ; [E3b] PerCpu.exception_cs (GS ya intercambiado)
    call    %2
    test    rax, rax
    jnz     .ctx_%1
    cli
.hlt_%1:
    hlt
    jmp     .hlt_%1
.ctx_%1:
    mov     rsp, rax
    SWAPGS_SALIDA
    POP_REGS
    iretq
%endmacro

; =============================================================================
; ISR_ERR -- excepcion CON error code
; =============================================================================
%macro ISR_ERR 2
isr_%1:
    SWAPGS_ENTRADA 16
    mov     [gs:72], rdi            ; [E3c] RDI original -> PerCpu.rdi_excepcion (por CPU, sin carrera)
    CAPTURE_FRAME 8
    pop     rdi
    PUSH_REGS
    ; [CICLO 47] Correccion: `pop rdi` carga el codigo de error ANTES de
    ; PUSH_REGS, asi que la ranura RDI del frame (+288) contenia el codigo de
    ; error y POP_REGS devolvia al usuario un RDI corrupto tras cualquier #PF
    ; resuelto (COW, paginacion por demanda). CAPTURE_FRAME ya guardo el RDI
    ; original en crash_frame+72: se restaura en la ranura del frame. El
    ; manejador en Rust sigue recibiendo el codigo de error en RDI.
    mov     rax, [gs:72]            ; [E3c] antes crash_frame+72 (global: dos CPUs con #PF a la vez se pisaban)
    mov     [rsp + 288], rax
    mov     rax, [rsp + 384]
    mov     [gs:48], rax            ; [E3b] PerCpu.exception_cs (GS ya intercambiado)
    call    %2
    test    rax, rax
    jnz     .ctx_%1
    SWAPGS_SALIDA
    POP_REGS
    iretq
.ctx_%1:
    mov     rsp, rax
    SWAPGS_SALIDA
    POP_REGS
    iretq
%endmacro

; =============================================================================
; Tabla de stubs de excepcion CPU (vectores 0-19)
; =============================================================================
ISR_NOERR        0, isr_divide_by_zero   ; #DE  Division por cero
ISR_NOERR        1, isr_dbg_handler      ; #DB  Debug (watchpoints DR0-DR3 / single-step)
ISR_NOERR        2, isr_generic_handler  ; NMI  Non-Maskable Interrupt
ISR_NOERR        3, isr_bp_handler       ; #BP  Breakpoint (int3) del depurador
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
    SWAPGS_ENTRADA 8
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
    LIBERAR_SALIENTE
.no_switch:
    SWAPGS_SALIDA
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
    SWAPGS_ENTRADA 8
    PUSH_REGS
    mov     edi, 1
    call    is_irq_routed
    test    al, al
    jnz     .irq1_ipc               ; driver registered: forward via IPC
    xor     eax, eax
    in      al, 0x64
    test    al, 0x01
    jz      .eoi1
    test    al, 0x20
    jnz     .eoi1
    in      al, 0x60
    mov     edi, eax
    call    irq1_handler_rust
    jmp     .eoi1
.irq1_ipc:
    mov     edi, 1
    call    ipc_notify_irq_handler

.eoi1:
    mov     al, 0x20
    out     0x20, al                ; EOI master PIC
    SWAPGS_SALIDA
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
    SWAPGS_ENTRADA 8
    PUSH_REGS
    mov     edi, 12
    call    is_irq_routed
    test    al, al
    jnz     .irq12_ipc              ; driver registered: forward via IPC
    xor     eax, eax
    in      al, 0x64                ; leer status register
    test    al, 0x01                ; OBF = bit 0 (Output Buffer Full)
    jz      .eoi12                  ; buffer vacio -> EOI sin leer
    test    al, 0x20                ; AUXB = bit 5 (Auxiliary mouse byte)
    jz      .eoi12                  ; no es raton -> EOI sin leer
    in      al, 0x60                ; leer byte de raton
    mov     edi, eax
    call    irq12_handler_rust
    jmp     .eoi12
.irq12_ipc:
    mov     edi, 12
    call    ipc_notify_irq_handler
.eoi12:
    mov     al, 0x20
    out     0xA0, al                ; EOI slave  PIC
    out     0x20, al                ; EOI master PIC (cascade)
    SWAPGS_SALIDA
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
    SWAPGS_ENTRADA 8
    PUSH_REGS
    mov     edi, 14
    call    ipc_notify_irq_handler
    mov     al, 0x20
    out     0xA0, al                ; EOI slave
    out     0x20, al                ; EOI master (cascade)
    SWAPGS_SALIDA
    POP_REGS
    iretq

; =============================================================================
; IRQ15 -- ATA Secondary Channel
; =============================================================================
irq15_handler:
    SWAPGS_ENTRADA 8
    PUSH_REGS
    mov     edi, 15
    call    ipc_notify_irq_handler
    mov     al, 0x20
    out     0xA0, al                ; EOI slave
    out     0x20, al                ; EOI master (cascade)
    SWAPGS_SALIDA
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
    SWAPGS_ENTRADA 8
    ; [T-025 E3a] La puerta 0x80 pasa a ser de interrupcion (IF=0 al entrar)
    ; para que ninguna IRQ se cuele antes del swapgs; una vez intercambiado GS
    ; se reabre IF: la ABI nativa (int 0x80) siempre corrio con IF=1 (p. ej.
    ; `certify` espera ticks del PIT dentro de la propia syscall).
    sti
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
    mov     [rsp + 256], rax        ; resultado -> RAX guardado (tras 256B XMM)
    test    rdx, rdx
    jz      .cont80
    mov     rsp, rdx
    LIBERAR_SALIENTE
.cont80:
    SWAPGS_SALIDA
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
    swapgs                          ; [E3a] SYSCALL siempre llega desde Ring 3
    mov     [gs:16], rsp            ; RSP de usuario -> PerCpu.rsp_usuario
    mov     rsp, [gs:8]             ; PerCpu.pila_nucleo: pila del proceso en ESTA CPU
    test    rsp, rsp
    jnz     .stack_ok
    lea     rsp, [rel __stack_top]          ; respaldo durante el arranque temprano
.stack_ok:
    push    0x1B                    ; SS  = USER_DS|3
    push    qword [gs:16]           ; RSP usuario
    push    r11                     ; RFLAGS usuario
    push    0x23                    ; CS  = USER_CS|3
    push    rcx                     ; RIP usuario
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
    call    linux_syscall_dispatch
    add     rsp, 16
    mov     [rsp + 256], rax        ; resultado -> RAX guardado (tras 256B XMM)
    test    rdx, rdx
    jz      .cont_sc
    mov     rsp, rdx
    LIBERAR_SALIENTE
.cont_sc:
    SWAPGS_SALIDA
    POP_REGS
    iretq

; =============================================================================
; enter_ring3_asm -- transicion al primer proceso de usuario
; =============================================================================
; [T-025 E3a] Es una funcion `extern "C"` que "retorna" cuando el proceso
; termina (process_exit_trampoline hace `mov rsp,[ring3_ret_rsp]; jmp ret_addr`).
; Antes volvia directamente al invocador con RBX/RBP/R12-R15 basura (los del
; proceso muerto): el compilador los da por conservados y el fallo dependia de
; que variable viviera en ellos ("Proceso PID 17294103305076670291", reinicios
; al cargar el siguiente binario). Ahora se conservan en la pila del invocador
; y la vuelta pasa por `.volver`, que los restaura antes del `ret`.
enter_ring3_asm:
    push    rbp
    push    rbx
    push    r12
    push    r13
    push    r14
    push    r15
    mov     [gs:56], rsp            ; [E3b] PerCpu.ring3_ret_rsp: pila del invocador (con los 6 salvados)
    lea     rdi, [rel .volver]
    call    process_save_ring3_ret_addr
    call    enter_ring3_setup       ; hace IRETQ -> nunca retorna
.dead:
    cli
    hlt
    jmp     .dead
.volver:                            ; rsp == ring3_ret_rsp (desde el tramp. de salida)
    pop     r15
    pop     r14
    pop     r13
    pop     r12
    pop     rbx
    pop     rbp
    ret

; =============================================================================
; ring3_exit_trampoline -- salida limpia de proceso de ring-3
; =============================================================================
ring3_exit_trampoline:
    ; [T-025 E3d] Alineación ABI: se llega aquí con `call` desde Rust (rsp ≡ 8
    ; mod 16) y el segundo `call` dejaba a process_exit_trampoline con rsp ≡ 0
    ; mod 16 (la ABI exige 8): un `movdqa` sobre su pila (net::poll → handle_ipv6
    ; desde el bucle de espera de salida) daba #GP de núcleo. Nunca retorna, así
    ; que realinear es gratis.
    and     rsp, -16
    call    process_exit_trampoline ; nunca retorna
.dead:
    cli
    hlt
    jmp     .dead

; =============================================================================
; switch_to_process_asm -- reanudar proceso en ring-3 via IRETQ
; rdi = kernel_rsp
; =============================================================================
switch_to_process_asm:
    mov     rsp, rdi
    LIBERAR_SALIENTE
    SWAPGS_SALIDA
    POP_REGS
    iretq
.dead:
    cli
    hlt
    jmp     .dead

; =============================================================================
; T-025 SMP — Interrupciones entre procesadores (IPI). SOLO AÑADIDOS.
; Vectores 0xF0 (prueba/contador), 0xF1 (TLB shootdown), 0xF2 (replanificar),
; 0xFF (espurio de la LAPIC: no lleva EOI). Cada stub salva registros, llama a
; `ipi_handler_rust(vector)` (que escribe el EOI en la LAPIC) y retorna.
; =============================================================================
global ipi_stub_f0, ipi_stub_f1, ipi_stub_f2, ap_timer_handler, ipi_stub_espurio

%macro IPI_STUB 2
%1:
    SWAPGS_ENTRADA 8
    PUSH_REGS
    mov     edi, %2
    call    ipi_handler_rust
    SWAPGS_SALIDA
    POP_REGS
    iretq
%endmacro

IPI_STUB ipi_stub_f0, 0xF0
IPI_STUB ipi_stub_f1, 0xF1
IPI_STUB ipi_stub_f2, 0xF2

; =============================================================================
; [E3c] Temporizador LAPIC de un AP (vector 0xF3): como irq0_handler pero sin
; PIC; `ap_tick_rust` hace el EOI en la LAPIC y puede devolver otro RSP
; (conmutacion de proceso en el AP o arranque de uno desde el bucle ocioso).
; =============================================================================
ap_timer_handler:
    SWAPGS_ENTRADA 8
    PUSH_REGS
    mov     rdi, rsp
    mov     rsi, [rsp + 384]
    call    ap_tick_rust
    test    rax, rax
    jz      .ap_no_switch
    mov     rsp, rax
    LIBERAR_SALIENTE
.ap_no_switch:
    SWAPGS_SALIDA
    POP_REGS
    iretq

ipi_stub_espurio:
    iretq
