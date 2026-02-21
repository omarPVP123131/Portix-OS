; boot/stage2.asm - PORTIX v7 - FIXED (corregido lectura LFB en long mode)
; nasm -f bin stage2.asm -o stage2.bin

BITS 16
ORG 0x8000

KERNEL_SECTORS   equ 256            ; ← subido de 192 (kernel actual ~225 sec)
KERNEL_LOAD_SEG  equ 0x1000
KERNEL_PHYS_ADDR equ 0x10000

section .text

start2:
    mov [boot_drive], dl
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti

    mov si, msg_stage2
    call print

    ; ── 1. A20 ───────────────────────────────────────────────────────────────
    in  al, 0x92
    or  al, 0x02
    and al, 0xFE
    out 0x92, al

; ── 2. E820 RAM (Construcción de Tabla) ──────────────────────────────────
    mov di, 0x9102          ; Las entradas empiezan en 0x9102
    xor ebx, ebx            ; EBX debe ser 0 para empezar
    xor bp, bp              ; Usaremos BP como contador de entradas
.e820_loop:
    mov eax, 0xE820
    mov ecx, 24             ; Pedir 24 bytes
    mov edx, 0x534D4150     ; 'SMAP'
    int 0x15
    jc .e820_done
    cmp eax, 0x534D4150
    jne .e820_done
    
    test ecx, ecx           ; ¿BIOS retornó 0 bytes?
    jz .e820_next
    
    add di, 20              ; Avanzar al siguiente slot de la tabla
    inc bp                  ; Incrementar contador
    
.e820_next:
    test ebx, ebx           ; Si EBX es 0, terminó el mapa
    jnz .e820_loop
.e820_done:
    mov [0x9100], bp        ; Guardar el número total de entradas en 0x9100
    

    ; ── 3. Deshabilitar nIEN (IDE IRQ) ────────────────────────────────────────
    mov al, 0x02
    mov dx, 0x3F6
    out dx, al
    out 0x80, al
    mov dx, 0x376
    out dx, al
    out 0x80, al

    ; ── 4. Cargar kernel ──────────────────────────────────────────────────────
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, 0x80
    int 0x13
    jc  .try_ext_orig
    cmp bx, 0xAA55
    jne .try_ext_orig
    mov byte [boot_drive], 0x80
    jmp .do_lba_read

.try_ext_orig:
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, [boot_drive]
    int 0x13
    jc  .try_chs
    cmp bx, 0xAA55
    jne .try_chs
    jmp .do_lba_read

.do_lba_read:
    mov word  [dap_count],   1
    mov word  [dap_offset],  0
    mov word  [dap_segment], KERNEL_LOAD_SEG
    mov dword [dap_lba_lo],  65
    mov dword [dap_lba_hi],  0
    mov cx, KERNEL_SECTORS

.lba_loop:
    mov si, dap
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    jc  .lba_error
    inc dword [dap_lba_lo]
    mov ax, [dap_segment]
    add ax, 0x20
    mov [dap_segment], ax
    loop .lba_loop
    jmp .kernel_loaded

.lba_error:
    jmp .try_chs

.try_chs:
    mov byte [boot_drive], 0x80
    mov ax, KERNEL_LOAD_SEG
    mov es, ax
    xor bx, bx
    mov word [current_lba], 65
    mov cx, KERNEL_SECTORS

.chs_loop:
    push cx
    push bx
    push es
    mov ax, [current_lba]
    call lba_to_chs_hd
    mov ah, 0x02
    mov al, 1
    mov dl, [boot_drive]
    int 0x13
    jc  .chs_fail
    pop es
    pop bx
    pop cx
    mov ax, es
    add ax, 0x20
    mov es, ax
    inc word [current_lba]
    loop .chs_loop
    jmp .kernel_loaded

.chs_fail:
    pop es
    pop bx
    pop cx
    jmp error_disk

.kernel_loaded:
    mov si, msg_kernel_ok
    call print

; ── 5. VESA ───────────────────────────────────────────────────────────────
    xor ax, ax
    mov es, ax
    mov ax, 0x4F01
    mov cx, 0x118
    mov di, 0x8500
    int 0x10
    cmp ax, 0x004F
    jne .vesa_skip

    mov eax, [0x8528]
    mov [0x9004], eax
    mov ax, [0x8512]
    mov [0x9008], ax
    mov ax, [0x8514]
    mov [0x900A], ax
    mov ax, [0x8510]
    mov [0x900C], ax
    mov al, [0x8519]
    mov [0x900E], al

    mov ax, 0x4F02
    mov bx, 0x4118
    int 0x10
    jmp .vesa_done
    
.try_modo_112:
    mov ax, 0x4F01
    mov cx, 0x112
    mov di, 0x8500
    int 0x10
    cmp ax, 0x004F
    jne .vesa_skip

    mov eax, [0x8528]
    mov [0x9004], eax
    mov ax,  [0x8512]
    mov [0x9008], ax
    mov ax,  [0x8514]
    mov [0x900A], ax
    mov ax,  [0x8510]
    mov [0x900C], ax
    mov al,  [0x8519]
    mov [0x900E], al

    mov ax, 0x4F02
    mov bx, 0x4112
    int 0x10
    jmp .vesa_done

.vesa_skip:
    xor eax, eax
    mov [0x9004], eax
    mov word [0x9008], 640
    mov word [0x900A], 480
    mov word [0x900C], 2560
    mov byte [0x900E], 32
.vesa_done:

    ; ── 6. Tablas de paginación (identity map 0-1GB + LFB) ───────────────────
    mov edi, 0x1000
    xor eax, eax
    mov ecx, (0x5000 / 4)
    rep stosd

    mov dword [0x1000], 0x2003
    mov dword [0x1004], 0

    mov dword [0x2000], 0x3003
    mov dword [0x2004], 0

    mov edi, 0x3000
    mov eax, 0x00000083
    mov ecx, 512
.fill_pd:
    mov [edi],   eax
    mov dword [edi+4], 0
    add eax, 0x200000
    add edi, 8
    loop .fill_pd

    mov ebx, [0x9004]
    test ebx, ebx
    jz  .paging_done
    mov eax, ebx
    shr eax, 30
    and eax, 0x1FF
    cmp eax, 0
    je  .paging_done
    shl eax, 3
    mov dword [0x2000 + eax], 0x4003
    mov dword [0x2004 + eax], 0
    mov edi, 0x4000
    mov eax, ebx
    and eax, 0xC0000000
    or  eax, 0x83
    mov ecx, 512
.fill_lfb:
    mov [edi],   eax
    mov dword [edi+4], 0
    add eax, 0x200000
    add edi, 8
    loop .fill_lfb
.paging_done:

    ; ── 7. PIC remap + enmascarar TODO ───────────────────────────────────────
    cli
    mov al, 0x11
    out 0x20, al
    out 0x80, al
    out 0xA0, al
    out 0x80, al
    mov al, 0x20
    out 0x21, al
    out 0x80, al
    mov al, 0x28
    out 0xA1, al
    out 0x80, al
    mov al, 0x04
    out 0x21, al
    out 0x80, al
    mov al, 0x02
    out 0xA1, al
    out 0x80, al
    mov al, 0x01
    out 0x21, al
    out 0x80, al
    out 0xA1, al
    out 0x80, al
    mov al, 0xFF
    out 0x21, al
    out 0x80, al
    out 0xA1, al
    out 0x80, al

    ; ── 8. Long Mode ──────────────────────────────────────────────────────────
    lgdt [gdt64_desc]
    lidt [idt_null_desc]

    mov eax, cr4
    or  eax, (1 << 5)
    mov cr4, eax

    mov eax, 0x1000
    mov cr3, eax

    mov ecx, 0xC0000080
    rdmsr
    or  eax, (1 << 8)
    xor edx, edx
    wrmsr

    mov eax, cr0
    or  eax, (1 << 31) | (1 << 0)
    mov cr0, eax

    o32 jmp far [far_jump_ptr]

; ── Funciones ─────────────────────────────────────────────────────────────────
lba_to_chs_hd:
    push bx
    push ax
    mov ax, dx
    xor dx, dx
    mov bx, 63
    div bx
    inc dx
    mov cl, dl
    xor dx, dx
    mov bx, 255
    div bx
    mov ch, al
    shl ah, 6
    or cl, ah
    mov dh, dl
    pop ax
    pop bx
    ret

print:
    pusha
.pl: lodsb
    or al, al
    jz .pd
    mov ah, 0x0E
    int 0x10
    jmp .pl
.pd: popa
    ret

error_disk:
    mov si, msg_err_disk
    call print
    cli
    hlt

; ── Datos ──────────────────────────────────────────────────────────────────────
msg_stage2    db "S2 OK", 13, 10, 0
msg_kernel_ok db "K  OK", 13, 10, 0
msg_err_disk  db "DISK ERR", 13, 10, 0
boot_drive    db 0
current_lba   dw 0

align 4
dap:
    db 0x10, 0x00
dap_count:   dw 1
dap_offset:  dw 0
dap_segment: dw KERNEL_LOAD_SEG
dap_lba_lo:  dd 65
dap_lba_hi:  dd 0

align 8
gdt64:
    dq 0x0000000000000000
    dq 0x00AF9A000000FFFF
    dq 0x00CF92000000FFFF
gdt64_end:

gdt64_desc:
    dw gdt64_end - gdt64 - 1
    dd gdt64

idt_null_desc:
    dw 0x0000
    dd 0x00000000

align 4
far_jump_ptr:
    dd long_mode_entry
    dw 0x08

; ── 64-bit entry ──────────────────────────────────────────────────────────────
BITS 64
long_mode_entry:
    cli
    
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    xor ax, ax
    mov fs, ax
    mov gs, ax
    mov rsp, 0x7FF00
    xor rbp, rbp

    ; Habilitar FPU y SSE
    mov rax, cr0
    and ax, 0xFFFB
    or  ax, 0x0002
    mov cr0, rax
    mov rax, cr4
    or  ax, 3 << 9
    mov cr4, rax

    xor rdi, rdi
    mov edi, dword [0x9004]
    test rdi, rdi
    jz .no_debug_pixel
    mov eax, 0xFFFFFFFF
    mov ecx, 8
    mov ebx, dword [0x900C]

.loop_y:
    push rcx
    mov ecx, 8
.loop_x:
    mov [rdi], eax
    add rdi, 4
    loop .loop_x
    add rdi, rbx
    sub rdi, 32
    pop rcx
    loop .loop_y
    
.no_debug_pixel:
    mov rax, KERNEL_PHYS_ADDR
    jmp rax

; Pad to exactly 64 sectors (512*64 bytes) — use DEFAULT ABS explicitly
DEFAULT ABS
times (512*64)-($-$$) db 0