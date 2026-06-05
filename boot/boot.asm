; boot/boot.asm  -  PORTIX Stage-1  v9.8
; nasm -f bin boot.asm -o boot.bin
;
; CORRECCIONES vs v9.7:
;
;   [FIX-BIT-PATCH-AREA]  xorriso -boot-info-table parcha el boot image
;                         empezando en offset +2 (tras el jmp/nop inicial).
;                         En v9.7 los primeros bytes eran cli/xor/etc.,
;                         que quedaban corrompidos → crash inmediato en CD.
;
;                         SOLUCIÓN: jmp short al inicio que salta a
;                         start_real en offset +0x10, dejando 13 bytes
;                         libres para el BIT entre offset +0x03 y +0x0F.
;
;   [FIX-NO-CD-DETECT]    Boot.asm ya no intenta detectar CD (evita toda
;                         la lógica BIT / INT 13h/48h que era frágil).
;                         En su lugar, comprueba si stage2 ya está en RAM
;                         buscando el magic dword STAGE2_MAGIC en 0x8000.
;
;                         Con boot-load-size=65, El Torito carga 65×512 B
;                         que incluye boot.bin (512 B) + stage2.bin (64×512 B).
;                         stage2.bin se carga en 0x7E00..0xA1FF; su ORG
;                         es 0x8000, así que los primeros 4 bytes en 0x8000
;                         son el magic "ST92" (0x32395453 LE).
;
;                         En HDD/USB solo se carga 1 sector (512 B =
;                         boot.bin). [0x8000] contiene RAM sin inicializar
;                         o datos del BIOS; la probabilidad de un falso
;                         positivo con el magic es despreciable.
;
;                         REQUISITO: stage2.asm debe exportar el magic en
;                         sus primeros 4 bytes (ver stage2.asm v9.11).
;
; Heredado de v9.6/v9.7:
;   [FIX-DAP-CLOBBER]   Buffer edd_buf eliminado (ya no se usa INT 13h/48h
;                       en boot.asm). DAP limpio antes de su primer uso.
;   Path HDD/USB LBA+CHS sin cambios.

BITS 16
ORG 0x7C00

STAGE2_SECTORS equ 64
STAGE2_SEG     equ 0x0800
BASE_LBA_ADDR  equ 0x7E00

; Magic que stage2.asm pone en sus primeros 4 bytes (en 0x8000).
; Valor: "ST92" en little-endian = 53 54 39 32.
STAGE2_MAGIC   equ 0x32395453

; ── Offset 0x00: jmp short sobre el área BIT ──────────────────────────
; xorriso parcha el boot image desde offset +2 (tras el jmp/nop).
; El código real empieza en start_real (offset 0x10).
    jmp  short start_real
    nop
    ; Offsets 0x03..0x0F: área BIT (13 bytes, rellenada por xorriso con
    ; bi_pvd, bi_file, bi_length, bi_csum). No ejecutada.
    times 13 db 0

start_real:                 ; offset 0x10 desde ORG = 0x7C10
    cli
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    mov  ss, ax
    mov  sp, 0x7C00
    sti
    mov  [boot_drive_orig], dl
    mov  [boot_drive],      dl
    mov  di, si

    ; Geometría CHS dinámica
    push es
    mov  ah, 0x08
    mov  dl, [boot_drive]
    int  0x13
    jc   .geom_done
    and  cx, 0x003F
    jz   .geom_done
    mov  [spt], cx
    movzx ax, dh
    inc  ax
    mov  [heads], ax
.geom_done:
    pop  es
    xor  ax, ax
    mov  ds, ax
    mov  es, ax

    ; base_lba desde entrada de partición en DI (SI original)
    xor  eax, eax
    mov  [BASE_LBA_ADDR], eax
    test di, di
    jz   .base_done
    mov  eax, [di + 8]
    test eax, eax
    jz   .base_done
    mov  [BASE_LBA_ADDR], eax
.base_done:

    ; ══════════════════════════════════════════════════════════════════════
    ; Detectar si stage2 ya está en RAM (arrancó desde CD con
    ; boot-load-size=65). Comprobar el magic en 0x8000.
    ; ══════════════════════════════════════════════════════════════════════
    mov  eax, [0x8000]
    cmp  eax, STAGE2_MAGIC
    jne  .load_stage2

    ; Stage2 ya en RAM → es CD. Limpiar base_lba y saltar.
    xor  eax, eax
    mov  [BASE_LBA_ADDR], eax
    mov  dl, [boot_drive_orig]
    jmp  0x0000:0x8000

    ; ══════════════════════════════════════════════════════════════════════
.load_stage2:
    ; ── Path HDD/USB: leer stage2 del disco ───────────────────────────
    mov  byte [drive_idx], 0

.pick_drive:
    mov  al, [drive_idx]
    cmp  al, 0
    je   .pick_orig
    cmp  al, 2
    jae  .use_chs
    mov  dl, 0x80
    cmp  dl, [boot_drive_orig]
    je   .pick_next
    jmp  .do_lba
.pick_orig:
    mov  dl, [boot_drive_orig]
.do_lba:
    mov  [boot_drive], dl

    mov  eax, [BASE_LBA_ADDR]
    inc  eax
    mov  [dap_lba_lo],  eax
    mov  dword [dap_lba_hi], 0
    mov  word [dap_segment], STAGE2_SEG
    mov  word [dap_offset],  0
    mov  word [remaining],   STAGE2_SECTORS

.lba_blk:
    mov  ax, [remaining]
    test ax, ax
    jz   .loaded
    cmp  ax, 64
    jbe  .setcnt
    mov  ax, 64
.setcnt:
    mov  [dap_count], ax
    mov  cx, 3
.lba_try:
    push cx
    mov  si, dap
    mov  ah, 0x42
    mov  dl, [boot_drive]
    int  0x13
    pop  cx
    jnc  .lba_ok
    push cx
    xor  ah, ah
    mov  dl, [boot_drive]
    int  0x13
    pop  cx
    loop .lba_try
.pick_next:
    inc  byte [drive_idx]
    jmp  .pick_drive

.lba_ok:
    mov  ax, [dap_count]
    movzx eax, ax
    add  [dap_lba_lo], eax
    mov  ax, [dap_count]
    shl  ax, 5
    add  word [dap_segment], ax
    mov  ax, [dap_count]
    sub  word [remaining], ax
    jmp  .lba_blk

.use_chs:
    mov  dl, [boot_drive_orig]
    mov  [boot_drive], dl
    mov  eax, [BASE_LBA_ADDR]
    inc  eax
    cmp  eax, 0x0000FFFF
    ja   disk_error
    mov  [current_lba], ax
    mov  ax, STAGE2_SEG
    mov  es, ax
    xor  bx, bx
    mov  cx, STAGE2_SECTORS
.chs_loop:
    push cx
    mov  ax, [current_lba]
    call lba_to_chs_hd
    mov  cx, 3
.chs_try:
    push cx
    mov  ah, 0x02
    mov  al, 1
    mov  dl, [boot_drive]
    int  0x13
    pop  cx
    jnc  .chs_ok
    push cx
    xor  ah, ah
    mov  dl, [boot_drive]
    int  0x13
    mov  ax, [current_lba]
    call lba_to_chs_hd
    pop  cx
    loop .chs_try
    jmp  disk_error
.chs_ok:
    mov  ax, es
    add  ax, 0x20
    mov  es, ax
    xor  bx, bx
    inc  word [current_lba]
    pop  cx
    loop .chs_loop

.loaded:
    mov  dl, [boot_drive_orig]
    jmp  0x0000:0x8000

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

disk_error:
    mov  ah, 0x0E
    mov  al, 'X'
    xor  bx, bx
    int  0x10
    cli
    hlt

; === Datos ===
spt             dw 63
heads           dw 255
boot_drive_orig db 0x80
boot_drive      db 0x80
current_lba     dw 0
remaining       dw 0
drive_idx       db 0

dap:
    db 0x10, 0x00
dap_count:   dw 0
dap_offset:  dw 0
dap_segment: dw 0
dap_lba_lo:  dd 0
dap_lba_hi:  dd 0

times 510-($-$$) db 0
dw 0xAA55