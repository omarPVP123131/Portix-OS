; boot/stage2.asm - CORREGIDO PARA NASM
BITS 16
org 0x8000

KERNEL_SECTORS  equ 64
KERNEL_LOAD_SEG equ 0x1000
KERNEL_START_LBA equ 65

start2:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti

    mov si, msg_stage2
    call print
    
    ; Habilitar A20
    call check_a20
    test ax, ax
    jz .enable_a20
    mov si, msg_a20_enabled
    call print
    jmp .a20_ok
    
.enable_a20:
    mov si, msg_a20_disabled
    call print
    in al, 0x92
    or al, 2
    out 0x92, al
    call check_a20
    test ax, ax
    jz a20_error
    
.a20_ok:
    mov si, msg_loading_k
    call print

    ; Cargar kernel
    mov ax, KERNEL_LOAD_SEG
    mov es, ax
    xor bx, bx
    mov word [current_lba], KERNEL_START_LBA
    mov cx, KERNEL_SECTORS
    mov word [sectors_loaded], 0

load_kernel_loop:
    push cx
    mov ax, [current_lba]
    call lba_to_chs
    mov ah, 0x02
    mov al, 1
    mov dl, 0
    int 0x13
    jc disk_error
    inc word [sectors_loaded]
    pop cx
    add bx, 512
    inc word [current_lba]
    loop load_kernel_loop
    
    mov si, msg_load_ok
    call print

    ; Verificar kernel
    mov si, msg_verify_kernel
    call print
    mov ax, KERNEL_LOAD_SEG
    mov es, ax
    mov eax, [es:0]
    test eax, eax
    jz kernel_corrupt
    cmp eax, 0xFFFFFFFF
    je kernel_corrupt
    mov si, msg_kernel_ok
    call print

    ; ==========================
    ; Detectar RAM con E820
    ; Resultado final en 0x9000 (bytes)
    ; ==========================
    mov si, msg_detect_ram
    call print

xor ebx, ebx

mov ax, 0x0000
mov es, ax
mov di, 0x9100        ; buffer E820 en 0x9100

; total RAM = 0 en 0x9000
mov dword [0x9000], 0


detect_loop:
    mov eax, 0xE820
    mov edx, 0x534D4150   ; "SMAP"
    mov ecx, 24
    int 0x15
    jc detect_done
    cmp eax, 0x534D4150
    jne detect_done

    ; Tipo de memoria usable = 1
    cmp dword [es:di+16], 1
    jne .next

    ; Sumar tamaño (parte baja del tamaño)
    mov eax, [es:di+8]
    add [0x9000], eax

.next:
    add di, 24
    test ebx, ebx
    jnz detect_loop

detect_done:
    mov si, msg_ram_ok
    call print


    mov si, msg_setup_pm
    call print
    cli
    lgdt [gdt_desc]
    mov si, msg_gdt_loaded
    call print
    mov si, msg_enter_pm
    call print

    ; Activar protected mode
    mov eax, cr0
    or eax, 1
    mov cr0, eax
    jmp 0x08:pm_entry

a20_error:
    mov si, msg_a20_fail
    call print
    cli
    hlt

disk_error:
    mov si, msg_disk
    call print
    cli
    hlt

kernel_corrupt:
    mov si, msg_kernel_bad
    call print
    cli
    hlt

check_a20:
    push es
    push ds
    xor ax, ax
    mov es, ax
    mov di, 0x0500
    mov ax, 0xFFFF
    mov ds, ax
    mov si, 0x0510
    mov al, [es:di]
    push ax
    mov al, [ds:si]
    push ax
    mov byte [es:di], 0x00
    mov byte [ds:si], 0xFF
    cmp byte [es:di], 0xFF
    pop ax
    mov [ds:si], al
    pop ax
    mov [es:di], al
    mov ax, 0
    je .disabled
    mov ax, 1
.disabled:
    pop ds
    pop es
    ret

lba_to_chs:
    xor dx, dx
    mov cx, 18
    div cx
    inc dx
    mov [temp_sector], dl
    xor dx, dx
    mov cx, 2
    div cx
    mov ch, al
    mov dh, dl
    mov cl, [temp_sector]
    ret

print:
    pusha
.next:
    lodsb
    or al, al
    jz .done
    mov ah, 0x0E
    int 0x10
    jmp .next
.done:
    popa
    ret

temp_sector db 0
current_lba dw 0
sectors_loaded dw 0

msg_stage2      db 13,10,"[Stage2] Loaded",13,10,0
msg_a20_enabled db "[A20] Enabled",13,10,0
msg_a20_disabled db "[A20] Enabling...",13,10,0
msg_a20_fail    db "[A20] FAILED!",13,10,0
msg_loading_k   db "[KERNEL] Loading...",13,10,0
msg_load_ok     db "[KERNEL] Loaded",13,10,0
msg_verify_kernel db "[VERIFY] Checking...",0
msg_kernel_ok   db "OK",13,10,0
msg_kernel_bad  db "INVALID!",13,10,0
msg_setup_pm    db "[PM] Setup...",13,10,0
msg_gdt_loaded  db "[GDT] Loaded",13,10,0
msg_enter_pm    db "[PM] Entering...",13,10,0
msg_disk        db "[ERROR] Disk!",13,10,0
msg_detect_ram db "[RAM] Detecting...",13,10,0
msg_ram_ok     db "[RAM] OK",13,10,0

; ==========================================
; GDT - SIMPLIFICADA
; ==========================================
align 8
gdt_start:
    ; Null descriptor (0x00)
    dq 0x0000000000000000

    ; Code segment (0x08)
    dw 0xFFFF           ; Limit
    dw 0x0000           ; Base 0:15
    db 0x00             ; Base 16:23
    db 10011011b        ; Access: Present, Ring 0, Code, Exec/Read
    db 11001111b        ; Flags: 4KB, 32-bit
    db 0x00             ; Base 24:31

    ; Data segment (0x10)
    dw 0xFFFF
    dw 0x0000
    db 0x00
    db 10010011b        ; Access: Present, Ring 0, Data, Read/Write
    db 11001111b
    db 0x00

    ; TSS descriptor (0x18)
    ; La base se calcula como: (gdt_start - 0x8000) + offset_de_tss
    ; Que es: tss_location = 0x8000 + (tss - $$)
tss_descriptor:
    dw 103              ; Limit (104 bytes - 1)
    dw (tss_location & 0xFFFF)     ; Base 0:15
    db ((tss_location >> 16) & 0xFF)  ; Base 16:23
    db 10001001b        ; Access: Present, Ring 0, TSS Available (0x89)
    db 00000000b        ; Flags: Byte granularity
    db ((tss_location >> 24) & 0xFF)  ; Base 24:31
gdt_end:

gdt_desc:
    dw gdt_end - gdt_start - 1
    dd gdt_start

; ==========================================
; TSS - En memoria después de la GDT
; ==========================================
align 4
tss_location equ 0x8000 + (tss_data - $$)

tss_data:
    dd 0                ; Link (unused)
    dd 0x9000           ; ESP0 - Stack para ring 0
    dd 0x10             ; SS0 - Data segment
    times 23 dd 0       ; Resto del TSS (zeros)
tss_data_end:

; ==========================================
; PROTECTED MODE
; ==========================================
[BITS 32]
pm_entry:
    ; Configurar segmentos
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov esp, 0x90000
    mov ebp, esp

    ; Cargar TSS - IMPORTANTE PARA EXCEPCIONES
    mov ax, 0x18
    ltr ax

    ; Habilitar SSE
    mov eax, cr0
    and eax, 0xFFFFFFFB
    or eax, 0x00000002
    mov cr0, eax
    mov eax, cr4
    or eax, 0x00000600
    mov cr4, eax

    ; Verificar kernel
    mov eax, [0x10000]
    test eax, eax
    jz kernel_missing

    ; Saltar al kernel
    jmp 0x08:0x10000

kernel_missing:
    mov edi, 0xB8000
    mov esi, error_no_kernel
    mov ah, 0x4F
.loop:
    lodsb
    test al, al
    jz .hang
    stosw
    jmp .loop
.hang:
    cli
    hlt
    jmp .hang

error_no_kernel db "NO KERNEL!", 0

times (512*64)-($-$$) db 0