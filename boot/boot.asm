; boot/boot.asm - Stage1 PORTIX v6.0
; CORRECCIONES:
;   - boot_drive se guarda como PRIMERA instrucción (DL válido al entrar)
;   - LBA: primero boot_drive ORIGINAL, luego 0x80 como fallback (no al revés)
;   - boot_drive se pasa a stage2 vía DL justo antes del jmp
;   - CHS: geometría HD (63 sec/pista, 255 cabezas), no floppy
;   - Retry ×3 con reset en CHS
;   - Stage2 se carga en 0x0800:0x0000 = físico 0x8000

BITS 16
ORG 0x7C00

STAGE2_SECTORS equ 64
STAGE2_SEG     equ 0x0800    ; 0x0800:0x0000 = 0x8000 físico

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti

    ; ── CRÍTICO: guardar DL ANTES de cualquier otra instrucción ───────────
    mov [boot_drive], dl

    mov si, msg_boot
    call print_string

    ; Reset disco — ignorar error (QEMU IDE puede fallar aquí)
    xor ah, ah
    mov dl, [boot_drive]
    int 0x13

    mov si, msg_loading
    call print_string

    ; ── A) LBA con boot_drive ORIGINAL (puede ser 0x9F en óptico, 0x80 en HDD) ──
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, [boot_drive]
    int 0x13
    jc  .try_lba_80
    cmp bx, 0xAA55
    jne .try_lba_80
    ; Extensiones disponibles en drive original
    jmp .do_lba

    ; ── B) LBA con 0x80 como fallback (por si el BIOS da DL raro) ────────
.try_lba_80:
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, 0x80
    int 0x13
    jc  .use_chs
    cmp bx, 0xAA55
    jne .use_chs
    mov byte [boot_drive], 0x80   ; actualizar solo si realmente funciona

.do_lba:
    mov word  [dap_sectors],  STAGE2_SECTORS
    mov word  [dap_offset],   0x0000
    mov word  [dap_segment],  STAGE2_SEG
    mov dword [dap_lba_lo],   1        ; stage2 empieza en LBA 1
    mov dword [dap_lba_hi],   0

    mov si, dap
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    jnc .loaded
    ; LBA falló — caer a CHS
    mov si, msg_chs
    call print_string

    ; ── C) CHS clásico ────────────────────────────────────────────────────
.use_chs:
    mov ax, STAGE2_SEG
    mov es, ax
    xor bx, bx
    mov word [current_lba], 1
    mov cx, STAGE2_SECTORS

.read_loop_chs:
    push cx
    push bx
    push es

    ; Calcular CHS del sector actual
    mov ax, [current_lba]
    call lba_to_chs_hd       ; ch=cilindro, cl=sector|hi_cil, dh=cabeza

    ; Intentar 3 veces con reset
    mov cx, 3
.chs_retry:
    push cx
    mov  ah, 0x02
    mov  al, 1
    mov  dl, [boot_drive]
    int  0x13
    pop  cx
    jnc  .chs_ok
    ; Reset antes de reintentar
    push cx
    xor  ah, ah
    mov  dl, [boot_drive]
    int  0x13
    ; Recalcular CHS (registros destruidos por reset)
    mov  ax, [current_lba]
    call lba_to_chs_hd
    pop  cx
    loop .chs_retry
    jmp  disk_error

.chs_ok:
    pop es
    pop bx
    pop cx

    ; Avanzar destino: +512 bytes
    add bx, 512
    jnc .no_wrap
    mov ax, es
    add ax, 0x1000
    mov es, ax
    xor bx, bx
.no_wrap:
    inc word [current_lba]
    loop .read_loop_chs

.loaded:
    mov si, msg_jump
    call print_string

    ; Pasar boot_drive a stage2 en DL (stage2 lo lee como primera instrucción)
    mov dl, [boot_drive]
    jmp 0x0000:0x8000

; ── LBA→CHS HD (63 sec/track, 255 heads) ─────────────────────────────────────
; Entrada: AX = LBA  |  Salida: CH=cil, CL=sec|hi_cil, DH=cabeza
; Destruye: AX, BX, DX  (¡no necesita preservar: caller no depende de ellos!)
lba_to_chs_hd:
    push si
    mov  si, ax           ; si = LBA

    ; sector = (LBA mod 63) + 1
    xor  dx, dx
    mov  ax, si
    mov  bx, 63
    div  bx               ; ax = LBA/63,  dx = LBA%63
    inc  dx               ; 1-based
    mov  cl, dl           ; CL = sector

    ; cabeza y cilindro
    xor  dx, dx
    mov  bx, 255
    div  bx               ; ax = cilindro,  dx = cabeza
    mov  dh, dl           ; DH = cabeza
    mov  ch, al           ; CH = cilindro bits 7:0
    shl  ah, 6
    or   cl, ah           ; CL bits 7:6 = cilindro bits 9:8

    pop  si
    ret

; ── print_string ──────────────────────────────────────────────────────────────
print_string:
    pusha
.loop:
    lodsb
    test al, al
    jz   .done
    mov  ah, 0x0E
    mov  bh, 0
    mov  bl, 7
    int  0x10
    jmp  .loop
.done:
    popa
    ret

disk_error:
    mov si, msg_error
    call print_string
    cli
    hlt

; ── Datos ──────────────────────────────────────────────────────────────────────
msg_boot    db "PORTIX v0.6", 13, 10, 0
msg_loading db "Loading...", 13, 10, 0
msg_chs     db "CHS fallback", 13, 10, 0
msg_jump    db "Jumpaaaaaing!", 13, 10, 0
msg_error   db "DISK ERROR!", 13, 10, 0

boot_drive  db 0x80
current_lba dw 0

align 4
dap:
    db 0x10, 0x00
dap_sectors: dw 0
dap_offset:  dw 0
dap_segment: dw 0
dap_lba_lo:  dd 0
dap_lba_hi:  dd 0

times 510-($-$$) db 0
dw 0xAA55