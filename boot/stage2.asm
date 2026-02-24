; boot/stage2.asm - PORTIX v7.3 - Boot desde cualquier medio
; nasm -f bin -DKERNEL_SECTORS=N [-DKERNEL_LBA=N] stage2.asm -o stage2.bin
;
; CORRECCIONES vs v7.2:
;   - boot_drive tomado de DL al entrar (pasado por stage1)
;   - Estrategia de carga: SOLO usa el boot_drive real primero.
;     Solo cae a 0x80 si el boot_drive original falla Y 0x80 es diferente.
;   - Si boot_drive == 0x80, no hace el intento duplicado
;   - CHS lba_to_chs_hd: preserva AX/BX/DX correctamente
;   - Retry x3 con reset de disco en LBA y CHS
;   - KERNEL_LBA configurable desde build.py (defecto 65)

BITS 16
ORG 0x8000

%ifndef KERNEL_SECTORS
  %error "KERNEL_SECTORS no definido"
%endif
%ifndef KERNEL_LBA
  %define KERNEL_LBA 65
%endif

KERNEL_LOAD_SEG  equ 0x1000
KERNEL_PHYS_ADDR equ 0x10000
VESA_BUF         equ 0x8500

section .text

start2:
    ; DL = boot_drive pasado por stage1
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

    ; 1. A20
    mov ax, 0x2401
    int 0x15
    jnc .a20_done
    in  al, 0x92
    or  al, 0x02
    and al, 0xFE
    out 0x92, al
.a20_done:

    ; 2. E820
    mov di, 0x9102
    xor ebx, ebx
    xor bp, bp
.e820_loop:
    mov eax, 0xE820
    mov ecx, 24
    mov edx, 0x534D4150
    int 0x15
    jc  .e820_done
    cmp eax, 0x534D4150
    jne .e820_done
    test ecx, ecx
    jz  .e820_next
    add di, 20
    inc bp
.e820_next:
    test ebx, ebx
    jnz .e820_loop
.e820_done:
    mov [0x9100], bp

    ; 3. Deshabilitar IDE IRQ
    mov al, 0x02
    mov dx, 0x3F6
    out dx, al
    out 0x80, al
    mov dx, 0x376
    out dx, al
    out 0x80, al

    ; 4. Cargar kernel
    ; Orden: A) LBA en boot_drive  B) LBA en 0x80 (si distinto)
    ;        C) CHS en boot_drive  D) CHS en 0x80 (si distinto)

    ; A) LBA en boot_drive
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, [boot_drive]
    int 0x13
    jc  .lba_orig_no
    cmp bx, 0xAA55
    jne .lba_orig_no
    mov al, [boot_drive]
    mov [lba_drive], al
    call do_lba_load
    jnc .kernel_loaded

.lba_orig_no:
    ; B) LBA en 0x80 (solo si boot_drive != 0x80)
    cmp byte [boot_drive], 0x80
    je  .try_chs_orig
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, 0x80
    int 0x13
    jc  .try_chs_orig
    cmp bx, 0xAA55
    jne .try_chs_orig
    mov byte [lba_drive], 0x80
    call do_lba_load
    jnc .kernel_loaded

    ; C) CHS en boot_drive
.try_chs_orig:
    mov al, [boot_drive]
    mov [chs_drive], al
    call do_chs_load
    jnc .kernel_loaded

    ; D) CHS en 0x80 (solo si boot_drive != 0x80)
    cmp byte [boot_drive], 0x80
    je  .disk_fail
    mov byte [chs_drive], 0x80
    call do_chs_load
    jnc .kernel_loaded

.disk_fail:
    jmp error_disk

.kernel_loaded:
    mov si, msg_kernel_ok
    call print

; 5. VESA
    xor ax, ax
    mov es, ax

    mov ax, 0x4F01
    mov cx, 0x118
    mov di, VESA_BUF
    int 0x10
    cmp ax, 0x004F
    jne .try_11B
    call vesa_lfb_ok
    jz  .try_11B
    mov word [vesa_mode], 0x4118
    jmp .vesa_activate

.try_11B:
    mov ax, 0x4F01
    mov cx, 0x11B
    mov di, VESA_BUF
    int 0x10
    cmp ax, 0x004F
    jne .try_115
    call vesa_lfb_ok
    jz  .try_115
    mov word [vesa_mode], 0x411B
    jmp .vesa_activate

.try_115:
    mov ax, 0x4F01
    mov cx, 0x115
    mov di, VESA_BUF
    int 0x10
    cmp ax, 0x004F
    jne .try_112
    call vesa_lfb_ok
    jz  .try_112
    mov word [vesa_mode], 0x4115
    jmp .vesa_activate

.try_112:
    mov ax, 0x4F01
    mov cx, 0x112
    mov di, VESA_BUF
    int 0x10
    cmp ax, 0x004F
    jne .vesa_skip
    call vesa_lfb_ok
    jz  .vesa_skip
    mov word [vesa_mode], 0x4112
    jmp .vesa_activate

.vesa_skip:
    xor eax, eax
    mov [0x9004], eax
    mov word [0x9008], 0
    mov word [0x900A], 0
    mov word [0x900C], 0
    mov byte [0x900E], 0
    jmp .vesa_done

.vesa_activate:
    mov ax, 0x4F01
    mov cx, [vesa_mode]
    and cx, 0x01FF
    mov di, VESA_BUF
    int 0x10
    mov eax, [VESA_BUF + 0x28]
    mov [0x9004], eax
    mov ax,  [VESA_BUF + 0x12]
    mov [0x9008], ax
    mov ax,  [VESA_BUF + 0x14]
    mov [0x900A], ax
    mov ax,  [VESA_BUF + 0x10]
    mov [0x900C], ax
    mov al,  [VESA_BUF + 0x19]
    mov [0x900E], al
    mov ax, 0x4F02
    mov bx, [vesa_mode]
    int 0x10
    cmp ax, 0x004F
    je  .vesa_done
    xor eax, eax
    mov [0x9004], eax
.vesa_done:

; 6. Paginacion: identity map 0-1GB + LFB
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
    mov  [edi], eax
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
    mov  [edi], eax
    mov dword [edi+4], 0
    add eax, 0x200000
    add edi, 8
    loop .fill_lfb
.paging_done:

; 7. PIC remap
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

; 8. Long Mode
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

; ==============================================================================
; do_lba_load: carga kernel via INT 13h/42h usando [lba_drive]
; CF=0 OK, CF=1 error
; ==============================================================================
do_lba_load:
    pusha
    mov dword [dap_lba_lo], KERNEL_LBA
    mov dword [dap_lba_hi], 0
    mov word  [dap_segment], KERNEL_LOAD_SEG
    mov word  [dap_offset],  0
    mov cx, KERNEL_SECTORS
.loop:
    push cx
    mov  cx, 3
.retry:
    push cx
    mov  si, dap
    mov  ah, 0x42
    mov  dl, [lba_drive]
    int  0x13
    pop  cx
    jnc  .ok
    push cx
    xor  ah, ah
    mov  dl, [lba_drive]
    int  0x13
    pop  cx
    loop .retry
    pop  cx
    popa
    stc
    ret
.ok:
    pop cx
    inc dword [dap_lba_lo]
    mov ax, [dap_segment]
    add ax, 0x20
    mov [dap_segment], ax
    loop .loop
    popa
    clc
    ret

; ==============================================================================
; do_chs_load: carga kernel via INT 13h/02h usando [chs_drive]
; CF=0 OK, CF=1 error
; ==============================================================================
do_chs_load:
    pusha
    mov word [current_lba], KERNEL_LBA
    mov ax, KERNEL_LOAD_SEG
    mov es, ax
    xor bx, bx
    mov cx, KERNEL_SECTORS
.outer:
    push cx
    push bx
    push es
    mov  ax, [current_lba]
    call lba_to_chs_hd
    mov  cx, 3
.inner:
    push cx
    mov  ah, 0x02
    mov  al, 1
    mov  dl, [chs_drive]
    int  0x13
    pop  cx
    jnc  .sec_ok
    push cx
    xor  ah, ah
    mov  dl, [chs_drive]
    int  0x13
    mov  ax, [current_lba]
    call lba_to_chs_hd
    pop  cx
    loop .inner
    pop es
    pop bx
    pop cx
    popa
    stc
    ret
.sec_ok:
    pop es
    pop bx
    pop cx
    mov ax, es
    add ax, 0x20
    mov es, ax
    inc word [current_lba]
    loop .outer
    popa
    clc
    ret

; ==============================================================================
; lba_to_chs_hd
; Entrada: AX = LBA (16-bit)
; Salida:  CH = cilindro[7:0], CL = sector[5:0]|cil[9:8] en [7:6], DH = cabeza
; Preserva: AX, BX, todos los demas (solo modifica CH, CL, DH via push/pop)
; ==============================================================================
lba_to_chs_hd:
    push ax
    push bx
    push dx

    xor  dx, dx
    mov  bx, 63
    div  bx             ; ax = LBA/63,  dx = LBA%63
    inc  dx
    mov  cl, dl         ; sector 1-based en CL

    xor  dx, dx
    mov  bx, 255
    div  bx             ; ax = cilindro,  dx = cabeza
    mov  dh, dl
    mov  ch, al
    shl  ah, 6
    or   cl, ah

    pop  dx
    pop  bx
    pop  ax
    ret

; ==============================================================================
; vesa_lfb_ok: ZF=0 si LFB valido, ZF=1 si no
; ==============================================================================
vesa_lfb_ok:
    test byte [VESA_BUF], 0x80
    jz  .bad
    mov eax, [VESA_BUF + 0x28]
    test eax, eax
    jz  .bad
    or  eax, eax
    ret
.bad:
    xor eax, eax
    ret

; ==============================================================================
; print
; ==============================================================================
print:
    pusha
.l: lodsb
    or  al, al
    jz  .d
    mov ah, 0x0E
    int 0x10
    jmp .l
.d: popa
    ret

error_disk:
    mov si, msg_err_disk
    call print
    cli
    hlt

; Datos
msg_stage2    db "S2 OK", 13, 10, 0
msg_kernel_ok db "K  OK", 13, 10, 0
msg_err_disk  db "DISK ERR", 13, 10, 0

boot_drive  db 0
lba_drive   db 0x80
chs_drive   db 0x80
current_lba dw 0
vesa_mode   dw 0

align 4
dap:
    db 0x10, 0x00
dap_count:   dw 1
dap_offset:  dw 0
dap_segment: dw KERNEL_LOAD_SEG
dap_lba_lo:  dd KERNEL_LBA
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

    mov rax, cr0
    and ax, 0xFFFB
    or  ax, 0x0002
    mov cr0, rax
    mov rax, cr4
    or  ax, (1 << 9) | (1 << 10)
    mov cr4, rax

    mov rax, KERNEL_PHYS_ADDR
    jmp rax

DEFAULT ABS
times (512*64)-($-$$) db 0