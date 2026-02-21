; boot/boot.asm - Stage1 PORTIX v5 - FIXED
; BUGS CORREGIDOS:
;   1. Reset de disco: fallo no fatal (en QEMU IDE puede fallar con CF=1)
;   2. CHS: geometría de HD (63 sec/pista, 255 cabezas) en vez de floppy
;   3. LBA: probar primero con 0x80, luego con boot_drive guardado
;   4. Buffer CHS: usar ES:BX relativo correctamente para 64 sectores
; nasm -f bin boot.asm -o boot.bin

BITS 16
ORG 0x7C00

STAGE2_SECTORS equ 64
STAGE2_SEG     equ 0x0800    ; 0x0800:0x0000 = dirección física 0x8000

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti

    mov [boot_drive], dl

    ; Imprimir banner ANTES de cualquier disco (para saber si el bootloader arranca)
    mov si, msg_boot
    call print_string

    ; Reset disco — NO fatal si falla (QEMU IDE retorna error en reset a veces)
    mov ah, 0x00
    mov dl, 0x80
    int 0x13
    ; Ignorar CF — continuar de todas formas

    mov si, msg_loading
    call print_string

    ; ── Intentar LBA extendido con drive 0x80 ─────────────────────────────
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, 0x80
    int 0x13
    jc  .try_lba_orig
    cmp bx, 0xAA55
    jne .try_lba_orig

    mov byte [boot_drive], 0x80
    jmp .do_lba

.try_lba_orig:
    ; Intentar con el boot drive original
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, [boot_drive]
    int 0x13
    jc  .use_chs
    cmp bx, 0xAA55
    jne .use_chs

.do_lba:
    ; LBA extendido disponible
    mov word [dap_sectors],  STAGE2_SECTORS
    mov word [dap_offset],   0x0000
    mov word [dap_segment],  STAGE2_SEG
    mov dword [dap_lba_lo],  1
    mov dword [dap_lba_hi],  0

    mov si, dap
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    jnc .loaded
    ; LBA falló — caer a CHS
    mov si, msg_chs
    call print_string

.use_chs:
    ; CHS con geometría de HD: 63 sec/pista, 255 cabezas (igual que stage2)
    mov byte [boot_drive], 0x80
    mov word [current_lba], 1
    ; Leer 64 sectores de 512 bytes = 32KB a 0x0800:0000 = 0x8000 físico
    mov ax, STAGE2_SEG
    mov es, ax
    xor bx, bx
    mov cx, STAGE2_SECTORS

.read_loop_chs:
    push cx
    push bx
    push es
    mov ax, [current_lba]
    call lba_to_chs_hd
    mov ah, 0x02
    mov al, 0x01
    mov dl, [boot_drive]
    int 0x13
    jc  disk_error
    pop es
    pop bx
    pop cx
    ; Avanzar puntero: +512 bytes
    add bx, 512
    jnc .no_wrap
    ; BX wrapaó (pasó 64KB), ajustar ES
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
    mov dl, [boot_drive]
    jmp 0x0000:0x8000

; ── LBA → CHS para HD (63 sec/pista, 255 cabezas) ────────────────────────────
; Entrada: AX = LBA
; Salida: CH = cilindro, CL = sector (1-63), DH = cabeza
lba_to_chs_hd:
    push ax
    push bx
    push dx
    xor dx, dx
    mov bx, 63
    div bx          ; AX = LBA/63 (pista), DX = LBA%63 (sector 0-based)
    inc dx
    mov cl, dl      ; CL = sector 1-based
    and cl, 0x3F
    xor dx, dx
    mov bx, 255
    div bx          ; AX = cilindro, DX = cabeza
    mov ch, al      ; CH = cilindro (bits 7:0)
    mov dh, dl      ; DH = cabeza
    ; Bits 9:8 del cilindro van en CL bits 7:6 (ignoramos cilindros > 255 aquí)
    pop dx
    pop bx
    pop ax
    ret

; ── Print ──────────────────────────────────────────────────────────────────────
print_string:
    pusha
.loop:
    lodsb
    test al, al
    jz .done
    mov ah, 0x0E
    mov bh, 0
    mov bl, 7
    int 0x10
    jmp .loop
.done:
    popa
    ret

disk_error:
    mov si, msg_error
    call print_string
    cli
    hlt

; ── Datos ──────────────────────────────────────────────────────────────────────
msg_boot    db "PORTIX v0.5", 13, 10, 0
msg_loading db "Loading...", 13, 10, 0
msg_chs     db "CHS fallback", 13, 10, 0
msg_jump    db "Jumping!", 13, 10, 0
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