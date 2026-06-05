; boot/boot.asm  -  PORTIX Stage-1  v9.15
; nasm -f bin boot.asm -o boot.bin
;
; FIXES vs v9.14:
;
;   [FIX-XORRISO-ZERO]  xorriso -boot-info-table pone a cero bytes 24-63 del
;                        primer sector (no documentado). start_real movido
;                        de offset 0x18 a 0x40 (byte 64) para evitarlo.
;
; Heredado de v9.14:
;   [FIX-CD-RAM-COPY]   CD path copia stage2 de RAM (0x7E00→0x8000), no usa INT 13h.
;   [FIX-BIT-SAFE]      Área BIT expandida a bytes 2-23.
;   [REMOVE-CHS]        CHS fallback eliminado.

BITS 16
ORG 0x7C00

STAGE2_SECTORS equ 64
STAGE2_SEG     equ 0x0800
BASE_LBA_ADDR  equ 0x7E00

; ── Offset 0x00: jmp short sobre BIT + zona zero de xorriso ──────────
; xorriso -boot-info-table parcha bytes 8-23 del boot image:
;   0x7C08  PVD LBA    (u32)
;   0x7C0C  File LBA   (u32)  ← BIT_BOOT_LBA, usado por stage2
;   0x7C10  Image len  (u32)
;   0x7C14  Checksum   (u32)
;
; IMPORTANTE: xorriso 1.5.6 con -boot-info-table también pone a cero
; bytes 24-63 del primer sector (no solo bytes 8-23). start_real debe
; estar en offset ≥64 (0x40) para evitar que el código sea destruido.
    jmp  short start_real
    nop
    times 61 db 0

; ══════════════════════════════════════════════════════════════════════════════
; start_real — Entry point real (offset 0x40 = 0x7C40)
; ══════════════════════════════════════════════════════════════════════════════
start_real:
    cli
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    mov  ss, ax
    mov  sp, 0x7C00
    sti
    mov  [boot_drive_orig], dl    ; DL = boot drive from BIOS
    mov  [boot_drive],      dl
    mov  di, si

    ; ══════════════════════════════════════════════════════════════════════════
    ; Detectar CD-ROM: DL = 0xE0-0xEF (El Torito estándar)
    ; ══════════════════════════════════════════════════════════════════════════
    ; En CD, el BIOS ya cargó la boot image completa en RAM a 0x7C00
    ; (con -boot-load-size <N>). Stage2 está en 0x7E00. Solo copiamos de RAM.
    cmp  dl, 0xE0
    jae  .cd_fallback_copy

    ; ── Path HDD/USB (DL=0x00-0x9F) ──────────────────────────────────────
    ; Leer stage2 del disco a 0x8000 mediante INT 13h AH=0x42

    ; base_lba desde entrada de partición en DI (SI original)
    ; Si no hay partición (DI=0), usa LBA=1
    xor  eax, eax
    test di, di
    jz   .hdd_lba_set
    mov  eax, [di + 8]           ; LBA de inicio de partición
    test eax, eax
    jnz  .hdd_lba_set
    xor  eax, eax
.hdd_lba_set:
    inc  eax                     ; stage2 en LBA 1 (siguiente sector)

    ; ══════════════════════════════════════════════════════════════════════════
    ; .read_prep — Preparar DAP y leer stage2
    ;
    ; Entrada: eax = LBA de stage2 (sectores 512B)
    ; ══════════════════════════════════════════════════════════════════════════
.read_prep:
    mov  [dap_lba_lo],  eax
    xor  eax, eax
    mov  [dap_lba_hi],  eax
    mov  word [dap_segment], STAGE2_SEG
    mov  word [dap_offset],  0
    mov  word [dap_count],   STAGE2_SECTORS
    mov  word [remaining],   STAGE2_SECTORS

    ; ── .lba_blk — Lector LBA con reintentos ─────────────────────────────
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
    jmp  disk_error

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

.loaded:
    mov  dl, [boot_drive_orig]
    jmp  0x0000:0x8000

; ══════════════════════════════════════════════════════════════════════════════
; .cd_fallback_copy — Copia stage2 de RAM (El Torito carga boot image a 0x7C00)
;
; El BIOS ya cargó la boot image completa en RAM a 0x7C00 (incluyendo stage2
; en 0x7E00). Solo copiamos a 0x8000 donde stage2 espera estar por ORG.
; ══════════════════════════════════════════════════════════════════════════════
.cd_fallback_copy:
    std                             ; DF=1 → backward (evita overlap)
    mov  si, 0x7E00 + STAGE2_SECTORS * 512 - 2
    mov  di, 0x8000 + STAGE2_SECTORS * 512 - 2
    mov  cx, STAGE2_SECTORS * 512 / 2
    rep  movsw
    cld
    mov  dl, [boot_drive_orig]
    jmp  0x0000:0x8000

disk_error:
    mov  ah, 0x0E
    mov  al, 'X'
    xor  bx, bx
    int  0x10
    cli
    hlt

; === Datos ===
boot_drive_orig db 0x80
boot_drive      db 0x80
remaining       dw 0

dap:
    db 0x10, 0x00
dap_count:   dw 0
dap_offset:  dw 0
dap_segment: dw 0
dap_lba_lo:  dd 0
dap_lba_hi:  dd 0

times 510-($-$$) db 0
dw 0xAA55
