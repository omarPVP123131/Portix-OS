; boot/stage2.asm - PORTIX v7.1 - Robust Multi-Mode VESA + Better Compatibility
; nasm -f bin -DKERNEL_SECTORS=N stage2.asm -o stage2.bin
;
; MEJORAS vs v7:
;   - Cadena de modos VESA: intenta 0x118(1024x768x24), 0x11B(1280x1024x24),
;     0x115(800x600x24), 0x112(800x600x15), texto como último recurso
;   - Verifica que el LFB exista (bit 7 del campo attributes) antes de activar
;   - Guarda también ancho/alto reales del modo elegido en 0x9008/0x900A
;   - A20: primero BIOS INT 15h, fallback a puerto 0x92 (más compatible)
;   - PIC remap idéntico al original (no se toca)
;   - Debug pixel removido del stage2 (el kernel lo hace si quiere)

BITS 16
ORG 0x8000

%ifndef KERNEL_SECTORS
  %error "KERNEL_SECTORS no definido - ejecuta a traves de build.py"
%endif

KERNEL_LOAD_SEG  equ 0x1000
KERNEL_PHYS_ADDR equ 0x10000

; ── Offsets en la estructura VBE Mode Info (Int 10h / AX=4F01h) ───────────────
; Offset 0x00 = ModeAttributes (word)  bit7 = LFB disponible
; Offset 0x10 = BytesPerScanLine (word)  = pitch
; Offset 0x12 = XResolution (word)
; Offset 0x14 = YResolution (word)
; Offset 0x19 = BitsPerPixel (byte)
; Offset 0x28 = PhysBasePtr (dword)     = dirección física del LFB

VESA_BUF equ 0x8500   ; buffer para VBE Mode Info (256 bytes, seguro en 0x8500)

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

    ; ── 1. A20 — primero BIOS, luego port 0x92 ───────────────────────────────
    ; Método 1: INT 15h AX=2401h (BIOS A20 enable) — más limpio en sistemas modernos
    mov ax, 0x2401
    int 0x15
    jnc .a20_done          ; si no hubo carry, A20 activado por BIOS

    ; Método 2: Puerto 0x92 (Fast A20) — compatible con la mayoría de hardware
    in  al, 0x92
    or  al, 0x02
    and al, 0xFE
    out 0x92, al

.a20_done:

    ; ── 2. E820 RAM map ───────────────────────────────────────────────────────
    mov di, 0x9102
    xor ebx, ebx
    xor bp, bp
.e820_loop:
    mov eax, 0xE820
    mov ecx, 24
    mov edx, 0x534D4150     ; 'SMAP'
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

    ; ── 3. Deshabilitar nIEN (IDE IRQ) ───────────────────────────────────────
    mov al, 0x02
    mov dx, 0x3F6
    out dx, al
    out 0x80, al
    mov dx, 0x376
    out dx, al
    out 0x80, al

    ; ── 4. Cargar kernel desde disco ─────────────────────────────────────────
    ; Intentar LBA extendido con drive 0x80 primero
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

; ── 5. VESA — cadena de modos con validación de LFB ──────────────────────────
;
; Estrategia:
;   1) Intentar 0x118  → 1024x768  × 24bpp
;   2) Intentar 0x11B  → 1280x1024 × 24bpp
;   3) Intentar 0x115  → 800x600   × 24bpp
;   4) Intentar 0x112  → 800x600   × 15bpp  (mínimo aceptable)
;   5) Fallback texto  → LFB=0, kernel decide
;
; Para cada modo verificamos:
;   a) INT 10h 4F01h retorna AX=004Fh
;   b) ModeAttributes bit 7 = 1 (LFB soportado)
;   c) PhysBasePtr (0x28) != 0

    xor ax, ax
    mov es, ax

    ; ── Intento 1: Modo 0x118 — 1024×768×24bpp ───────────────────────────────
    mov ax, 0x4F01
    mov cx, 0x118
    mov di, VESA_BUF
    int 0x10
    cmp ax, 0x004F
    jne .try_11B
    call vesa_lfb_ok
    jz  .try_11B           ; ZF=1 → LFB no válido, probar siguiente
    mov word [vesa_mode], 0x4118
    jmp .vesa_activate

    ; ── Intento 2: Modo 0x11B — 1280×1024×24bpp ──────────────────────────────
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

    ; ── Intento 3: Modo 0x115 — 800×600×24bpp ────────────────────────────────
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

    ; ── Intento 4: Modo 0x112 — 800×600×15bpp ────────────────────────────────
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

    ; ── Sin VESA usable — llenar con zeros / fallback texto ──────────────────
.vesa_skip:
    xor eax, eax
    mov [0x9004], eax       ; LFB=0 → kernel usará VGA texto si puede
    mov word [0x9008], 0
    mov word [0x900A], 0
    mov word [0x900C], 0
    mov byte [0x900E], 0
    jmp .vesa_done

    ; ── Activar el modo elegido ───────────────────────────────────────────────
.vesa_activate:
    ; Volver a leer la info del modo que vamos a activar (vesa_mode sin bit LFB)
    mov ax, 0x4F01
    mov cx, [vesa_mode]
    and cx, 0x01FF          ; quitar bit 14 (LFB flag) para leer info
    mov di, VESA_BUF
    int 0x10

    ; Guardar parámetros en memoria baja para el kernel
    mov eax, [VESA_BUF + 0x28]  ; PhysBasePtr
    mov [0x9004], eax
    mov ax,  [VESA_BUF + 0x12]  ; XResolution
    mov [0x9008], ax
    mov ax,  [VESA_BUF + 0x14]  ; YResolution
    mov [0x900A], ax
    mov ax,  [VESA_BUF + 0x10]  ; BytesPerScanLine
    mov [0x900C], ax
    mov al,  [VESA_BUF + 0x19]  ; BitsPerPixel
    mov [0x900E], al

    ; Activar el modo con LFB (bit 14 set)
    mov ax, 0x4F02
    mov bx, [vesa_mode]
    int 0x10

    ; Si la activación falló (AX != 004Fh), borrar LFB addr
    cmp ax, 0x004F
    je  .vesa_done
    xor eax, eax
    mov [0x9004], eax

.vesa_done:

    ; ── 6. Tablas de paginación (identity map 0–1GB + LFB region) ───────────
    mov edi, 0x1000
    xor eax, eax
    mov ecx, (0x5000 / 4)
    rep stosd

    ; PML4[0] → PDPT en 0x2000
    mov dword [0x1000], 0x2003
    mov dword [0x1004], 0

    ; PDPT[0] → PD en 0x3000  (cubre 0–1GB)
    mov dword [0x2000], 0x3003
    mov dword [0x2004], 0

    ; PD[0..511] → 2MB huge pages, 0–1GB, WB
    mov edi, 0x3000
    mov eax, 0x00000083     ; presente | writable | huge (PS)
    mov ecx, 512
.fill_pd:
    mov  [edi],   eax
    mov dword [edi+4], 0
    add eax, 0x200000
    add edi, 8
    loop .fill_pd

    ; ── Mapear LFB si está arriba de 1GB ─────────────────────────────────────
    ; Calculamos el índice PDPT del LFB: (LFB_addr >> 30) & 0x1FF
    ; Si índice == 0 ya está cubierto por el mapa de 0–1GB.
    ; Si está en el rango 1–4GB montamos una PD extra en 0x4000.
    mov ebx, [0x9004]       ; LFB base address
    test ebx, ebx
    jz  .paging_done        ; LFB = 0 → nada que mapear

    mov eax, ebx
    shr eax, 30             ; bits 31:30 = índice PDPT (0..3 para 0..4GB)
    and eax, 0x1FF
    cmp eax, 0              ; índice 0 → ya mapeado en 0x3000
    je  .paging_done

    ; Apuntar PDPT[idx] → PD en 0x4000
    shl eax, 3              ; × 8 bytes por entrada
    mov dword [0x2000 + eax], 0x4003
    mov dword [0x2004 + eax], 0

    ; Llenar PD en 0x4000 con huge pages alineadas al GB del LFB
    mov edi, 0x4000
    mov eax, ebx
    and eax, 0xC0000000    ; base alineada al GB
    or  eax, 0x83          ; presente | writable | huge
    mov ecx, 512
.fill_lfb:
    mov  [edi],   eax
    mov dword [edi+4], 0
    add eax, 0x200000
    add edi, 8
    loop .fill_lfb

.paging_done:

    ; ── 7. PIC remap + enmascarar TODO (idéntico al original) ────────────────
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

    ; ── 8. Entrar a Long Mode ─────────────────────────────────────────────────
    lgdt [gdt64_desc]
    lidt [idt_null_desc]

    mov eax, cr4
    or  eax, (1 << 5)       ; PAE
    mov cr4, eax

    mov eax, 0x1000
    mov cr3, eax

    mov ecx, 0xC0000080     ; EFER MSR
    rdmsr
    or  eax, (1 << 8)       ; LME
    xor edx, edx
    wrmsr

    mov eax, cr0
    or  eax, (1 << 31) | (1 << 0)  ; PG + PE
    mov cr0, eax

    o32 jmp far [far_jump_ptr]

; ── Funciones ─────────────────────────────────────────────────────────────────

; vesa_lfb_ok — verifica que el modo en VESA_BUF tenga LFB válido
; Entrada: VESA_BUF recién llenado por INT 10h 4F01h
; Salida:  ZF=0 → OK (LFB válido)   ZF=1 → no válido
; Destruye: eax
vesa_lfb_ok:
    ; Verificar bit 7 de ModeAttributes (LFB disponible)
    test byte [VESA_BUF + 0x00], 0x80
    jz  .bad                ; bit 7 clear → sin LFB
    ; Verificar que PhysBasePtr != 0
    mov eax, [VESA_BUF + 0x28]
    test eax, eax
    jz  .bad
    ; Todo OK: forzar ZF=0 devolviendo NZ
    or  eax, eax            ; eax != 0 → ZF=0
    ret
.bad:
    xor eax, eax            ; ZF=1
    ret

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

; ── Datos ─────────────────────────────────────────────────────────────────────
msg_stage2    db "S2 OK", 13, 10, 0
msg_kernel_ok db "K  OK", 13, 10, 0
msg_err_disk  db "DISK ERR", 13, 10, 0
boot_drive    db 0
current_lba   dw 0
vesa_mode     dw 0          ; modo VESA elegido (con bit LFB 0x4xxx)

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
    dq 0x0000000000000000   ; null
    dq 0x00AF9A000000FFFF   ; CS64: code, DPL0, 64-bit
    dq 0x00CF92000000FFFF   ; DS:   data, DPL0, 32/64-bit
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

; ── 64-bit entry point ────────────────────────────────────────────────────────
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

    ; Habilitar FPU y SSE (necesario para no_std Rust)
    mov rax, cr0
    and ax, 0xFFFB          ; clear EM (no emular FPU)
    or  ax, 0x0002          ; set MP
    mov cr0, rax
    mov rax, cr4
    or  ax, (1 << 9) | (1 << 10)   ; OSFXSR + OSXMMEXCPT
    mov cr4, rax

    ; Saltar al kernel
    mov rax, KERNEL_PHYS_ADDR
    jmp rax

; ── Padding a exactamente 64 sectores ────────────────────────────────────────
DEFAULT ABS
times (512*64)-($-$$) db 0