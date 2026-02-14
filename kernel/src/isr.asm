; kernel/src/isr.asm - CORREGIDO SIN STI
BITS 32

; Exportar los ISR
global isr_0
global isr_5
global isr_6
global isr_8
global isr_13

; Importar handlers de Rust
extern isr_divide_by_zero
extern isr_bound_range
extern isr_ud_handler
extern isr_double_fault
extern isr_gp_handler

; ============================================
; MACRO PARA ISR SIN ERROR CODE
; ============================================
%macro ISR_NOERRCODE 2
isr_%1:
    cli                     ; Deshabilitar interrupciones
    pusha                   ; Guardar registros (EAX, ECX, EDX, EBX, ESP, EBP, ESI, EDI)
    push ds                 ; Guardar segmentos de datos
    push es
    push fs
    push gs
    
    ; Cargar segmentos del kernel
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    
    ; Llamar al handler de Rust
    call %2
    
    ; Restaurar segmentos
    pop gs
    pop fs
    pop es
    pop ds
    
    ; Restaurar registros
    popa
    
    ; NO usar sti aquí - los handlers NO retornan
    ; El handler de Rust hace halt_loop() y nunca vuelve
    iret
%endmacro

; ============================================
; MACRO PARA ISR CON ERROR CODE
; ============================================
%macro ISR_ERRCODE 2
isr_%1:
    cli                     ; Deshabilitar interrupciones
    
    ; El CPU ya puso el error code en el stack
    ; Stack: [ERROR_CODE] [EIP] [CS] [EFLAGS] [ESP] [SS]
    
    pusha                   ; Guardar registros
    push ds
    push es
    push fs
    push gs
    
    ; Cargar segmentos del kernel
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    
    ; Llamar al handler de Rust
    call %2
    
    ; Restaurar segmentos
    pop gs
    pop fs
    pop es
    pop ds
    
    ; Restaurar registros
    popa
    
    ; Remover el error code del stack
    add esp, 4
    
    ; NO usar sti - el handler hace halt y nunca retorna
    iret
%endmacro

; ============================================
; DEFINIR TODOS LOS ISR
; ============================================

; ISR 0: Divide by Zero (sin error code)
ISR_NOERRCODE 0, isr_divide_by_zero

; ISR 5: Bound Range Exceeded (sin error code)
ISR_NOERRCODE 5, isr_bound_range

; ISR 6: Invalid Opcode (sin error code)
ISR_NOERRCODE 6, isr_ud_handler

; ISR 8: Double Fault (con error code - siempre es 0)
ISR_ERRCODE 8, isr_double_fault

; ISR 13: General Protection Fault (con error code)
ISR_ERRCODE 13, isr_gp_handler