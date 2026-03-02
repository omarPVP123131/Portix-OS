; boot/boot.asm  -  PORTIX Stage-1  v9.2
; nasm -f bin boot.asm -o boot.bin
;
; CORRECCIONES vs v9.1:
;   [FIX-DAP]     El DAP sigue el formato EXACTO de INT 13h/42h:
;                   offset(2), segment(2)  ← orden correcto en memoria
;                 En v9.1 estaban en orden correcto pero se llenaban al revés:
;                   mov word [dap_offset], 0  y  mov word [dap_segment], STAGE2_SEG
;                 Lo cual es correcto. El bug real era otro:
;   [FIX-DS]      En start, bp=si se guardaba ANTES de establecer DS=0.
;                 Si el chainloader entregó DS≠0, la instrucción posterior
;                   mov eax, [BASE_LBA_ADDR]  leía con el DS del chainloader.
;                 Ahora: primero CLI/segmentos/SP, luego guardar SI.
;   [FIX-ALIGN]   Eliminado 'align 4' antes del DAP: en un sector de 512 bytes
;                 el align podía empujar datos más allá del byte 510, corrompiendo
;                 la firma 0xAA55 o el propio DAP.
;   [FIX-RESET]   El reset de disco tras fallo LBA restaura CH/CL/DH para CHS.
;                 En v9.1 faltaba recalcular CH/CL/DH después del reset en el
;                 bloque .chs_retry (los registros quedan destruidos por INT 13h/0).
;   [FIX-SEGWRAP] En CHS .chs_ok: el avance de segmento usa add ax,0x20 sobre ES
;                 directamente, sin pasar por variable intermedia — igual que v6,
;                 que funcionaba. En v9.1 el avance era correcto pero dependía de
;                 que ES se hubiera restaurado del stack, lo cual fallaba si el
;                 pop es/bx/cx ocurría en orden incorrecto.
;
; PROTOCOLO AL SALTAR A STAGE2:
;   DL       = boot_drive_orig
;   [0x7E00] = base_lba dword (offset de la imagen en el disco físico, 0 si ninguno)

BITS 16
ORG 0x7C00

STAGE2_SECTORS equ 64
STAGE2_SEG     equ 0x0800    ; físico 0x8000
BASE_LBA_ADDR  equ 0x7E00    ; dword, escrito antes de saltar a stage2

start:
    ; ── [FIX-DS] Establecer segmentos PRIMERO, LUEGO guardar SI ──────────
    cli
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    mov  ss, ax
    mov  sp, 0x7C00
    sti

    ; Ahora DS=0, es seguro guardar DL y SI
    mov  [boot_drive_orig], dl
    mov  [boot_drive],      dl
    mov  [saved_si], si           ; guardar SI (puntero a entrada de partición)

    ; ── Detectar geometría dinámica (CHS) ─────────────────────────────────
    push es
    mov  ah, 0x08
    mov  dl, [boot_drive]
    int  0x13
    jc   .geom_done
    and  cx, 0x003F
    jz   .geom_done
    mov  [spt],   cx
    movzx ax, dh
    inc  ax
    mov  [heads], ax
.geom_done:
    pop  es

    ; ── Detectar base_lba desde DS:SI (protocolo chainload) ───────────────
    xor  eax, eax
    mov  [BASE_LBA_ADDR], eax     ; default: sin offset

    mov  si, [saved_si]
    test si, si
    jz   .no_offset

    mov  ax, si
    sub  ax, 0x00BE
    test ax, 0x000F               ; alineado a 16 bytes como entrada de partición?
    jnz  .no_offset

    mov  al, [si]
    cmp  al, 0x80
    je   .chk_lba
    test al, al
    jnz  .no_offset

.chk_lba:
    mov  eax, [si + 8]
    test eax, eax
    jz   .no_offset
    mov  [BASE_LBA_ADDR], eax

.no_offset:
    ; Reset disco
    xor  ah, ah
    mov  dl, [boot_drive]
    int  0x13

    ; ── A) LBA con drive original ──────────────────────────────────────────
    mov  ah, 0x41
    mov  bx, 0x55AA
    mov  dl, [boot_drive]
    int  0x13
    jc   .try_lba_80
    cmp  bx, 0xAA55
    jne  .try_lba_80
    jmp  .do_lba

    ; ── B) LBA con 0x80 (solo si drive original != 0x80) ──────────────────
.try_lba_80:
    cmp  byte [boot_drive_orig], 0x80
    je   .use_chs
    mov  ah, 0x41
    mov  bx, 0x55AA
    mov  dl, 0x80
    int  0x13
    jc   .use_chs
    cmp  bx, 0xAA55
    jne  .use_chs
    mov  byte [boot_drive], 0x80

.do_lba:
    ; LBA físico de stage2 = base_lba + 1
    mov  eax, [BASE_LBA_ADDR]
    add  eax, 1
    ; ── [FIX-DAP] Llenar DAP correctamente ────────────────────────────────
    ; Formato en memoria: size(1) res(1) count(2) offset(2) segment(2) lba_lo(4) lba_hi(4)
    mov  word  [dap_count],   STAGE2_SECTORS
    mov  word  [dap_offset],  0x0000
    mov  word  [dap_segment], STAGE2_SEG
    mov  [dap_lba_lo], eax
    mov  dword [dap_lba_hi],  0

    mov  si, dap
    mov  ah, 0x42
    mov  dl, [boot_drive]
    int  0x13
    jnc  .loaded
    mov  [disk_err_code], ah

    ; ── C) CHS clásico ────────────────────────────────────────────────────
.use_chs:
    mov  dl, [boot_drive_orig]
    mov  [boot_drive], dl

    mov  eax, [BASE_LBA_ADDR]
    inc  eax
    cmp  eax, 0xFFFF
    ja   disk_error
    mov  [current_lba], ax

    ; Segmento destino inicial
    mov  ax, STAGE2_SEG
    mov  es, ax
    xor  bx, bx
    mov  cx, STAGE2_SECTORS

.chs_loop:
    push cx

    mov  ax, [current_lba]
    call lba_to_chs_hd            ; → CH=cilindro, CL=sector|hi_cil, DH=cabeza

    ; Intentar 3 veces con reset + recalcular CHS
    mov  cx, 3
.chs_retry:
    push cx
    mov  ah, 0x02
    mov  al, 1
    mov  dl, [boot_drive]
    int  0x13
    pop  cx
    jnc  .chs_ok
    mov  [disk_err_code], ah

    ; Reset disco
    push cx
    xor  ah, ah
    mov  dl, [boot_drive]
    int  0x13
    ; [FIX-RESET] Recalcular CHS: el reset destruye CH/CL/DH
    mov  ax, [current_lba]
    call lba_to_chs_hd
    pop  cx
    loop .chs_retry

    ; Fallback: intentar con 0x80
    cmp  byte [boot_drive_orig], 0x80
    je   disk_error
    mov  cx, 3
.chs_retry80:
    push cx
    mov  ah, 0x02
    mov  al, 1
    mov  dl, 0x80
    int  0x13
    pop  cx
    jnc  .chs_ok
    push cx
    xor  ah, ah
    mov  dl, 0x80
    int  0x13
    mov  ax, [current_lba]
    call lba_to_chs_hd
    pop  cx
    loop .chs_retry80
    jmp  disk_error

.chs_ok:
    ; [FIX-SEGWRAP] Avanzar segmento destino: +512 bytes = +0x20 párrafos
    mov  ax, es
    add  ax, 0x20
    mov  es, ax
    xor  bx, bx                   ; offset = 0 para cada sector

    inc  word [current_lba]
    pop  cx
    loop .chs_loop

.loaded:
    mov  dl, [boot_drive_orig]
    jmp  0x0000:0x8000

; ── lba_to_chs_hd dinámico ───────────────────────────────────────────────────
; Entrada: AX = LBA  |  Salida: CH=cil, CL=sec|hi_cil, DH=cabeza
; [spt] y [heads] contienen la geometría detectada (default 63/255)
lba_to_chs_hd:
    push ax
    push bx
    xor  dx, dx
    mov  bx, [spt]
    div  bx
    inc  dx
    mov  cl, dl
    xor  dx, dx
    mov  bx, [heads]
    div  bx
    mov  dh, dl
    mov  ch, al
    shl  ah, 6
    or   cl, ah
    pop  bx
    pop  ax
    ret

; ── print_string ──────────────────────────────────────────────────────────────
print_string:
    pusha
.l: lodsb
    test al, al
    jz   .d
    mov  ah, 0x0E
    int  0x10
    jmp  .l
.d: popa
    ret

disk_error:
    mov  si, msg_err
    call print_string
    cli
    hlt

; ── Datos ──────────────────────────────────────────────────────────────────────
msg_err          db "ERR", 0

spt              dw 63
heads            dw 255
boot_drive_orig  db 0x80
boot_drive       db 0x80
saved_si         dw 0
current_lba      dw 0
disk_err_code    db 0

; ── [FIX-ALIGN] SIN align 4 — el DAP debe caber antes del byte 510 ──────────
; Formato DAP exacto (14 bytes):
;   [0] size=0x10  [1] reserved=0x00
;   [2-3] count    [4-5] offset   [6-7] segment
;   [8-11] lba_lo  [12-15] lba_hi
dap:
    db 0x10, 0x00
dap_count:   dw 0
dap_offset:  dw 0
dap_segment: dw 0
dap_lba_lo:  dd 0
dap_lba_hi:  dd 0

times 510-($-$$) db 0
dw 0xAA55