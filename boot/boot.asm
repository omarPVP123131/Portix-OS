; boot/boot.asm  -  PORTIX Stage-1  v9.5
; nasm -f bin boot.asm -o boot.bin
;
; CORRECCIONES vs v9.4:
;
;   [FIX-NO-EMUL]    Deteccion de CD-ROM no-emul via INT 13h/48h.
;
;                    PROBLEMA:
;                      Con El Torito -no-emul-boot + boot-load-size=65,
;                      el BIOS carga 65*512=33280 bytes en 0x7C00:
;                        0x7C00: boot.bin (512B)
;                        0x7E00: stage2  (32768B)  <- BIOS lo pone aqui
;                      Pero stage2 tiene ORG 0x8000 (necesita estar ahi).
;                      Ademas, INT 13h/42h en modo CD usa sectores de 2048B;
;                      boot.asm pide LBA 1 esperando 512B -> lee bytes
;                      2048..4095 del boot image -> datos incorrectos en 0x8000
;                      -> salto a basura -> pantalla negra / cuelgue.
;
;                    SOLUCION:
;                      Detectar CD via INT 13h/48h (bytes_per_sector==2048).
;                      Si es CD: backward copy 32768B de 0x7E00 a 0x8000
;                      (backward para evitar solapamiento de regiones),
;                      luego saltar a 0x8000 directamente sin recargar nada.
;                      stage2 usa BIT [0x7C0C] (parchado por xorriso) para
;                      localizar el kernel en el ISO.
;
;                    Por que -no-emul-boot (NO -hard-disk-emul):
;                      -no-emul-boot es el modo El Torito estandar soportado
;                      por VirtualBox, VMware, QEMU y hardware fisico.
;                      -hard-disk-emul NO esta implementado en el BIOS de
;                      VirtualBox -> "No bootable medium found".
;
;   [CUT-CANDIDATES] Se eliminan los candidatos 0x81, 0x82, 0x9F, 0xE0 del
;                    scan en boot.asm (liberan ~50 bytes para el fix anterior).
;                    Estos drives raros se siguen probando en stage2.asm, que
;                    tiene mas espacio y solo los necesita para el kernel.
;
;   [CUT-SI-SIMPLE]  Simplificada la lectura de base_lba desde SI:
;                    se elimina la validacion de alineacion/tipo de entrada,
;                    solo se lee [SI+8] (LBA de inicio de particion).
;
;   [CUT-RESET]      Eliminado el reset de disco previo al scan LBA;
;                    stage2 hace su propio reset antes de cargar el kernel.
;
; PROTOCOLO AL SALTAR A STAGE2:
;   DL       = boot_drive_orig
;   [0x7E00] = base_lba dword (0 en ISO no-emul, LBA particion en disco)

BITS 16
ORG 0x7C00

STAGE2_SECTORS equ 64
STAGE2_SEG     equ 0x0800
BASE_LBA_ADDR  equ 0x7E00

start:
    cli
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    mov  ss, ax
    mov  sp, 0x7C00
    sti
    mov  [boot_drive_orig], dl
    mov  [boot_drive],      dl
    mov  di, si                  ; [CUT-SI-SIMPLE] Guardar SI en DI antes de INT 13h/08h

    ; Geometria CHS dinamica (INT 13h/08h puede clobber SI)
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

    ; base_lba desde entrada de particion en DI (SI original), o 0
    xor  eax, eax
    mov  [BASE_LBA_ADDR], eax
    test di, di
    jz   .base_done
    mov  eax, [di + 8]           ; LBA de inicio de particion (offset 8)
    test eax, eax
    jz   .base_done
    mov  [BASE_LBA_ADDR], eax
.base_done:

    ; ══════════════════════════════════════════════════════════════════════
    ; [FIX-NO-EMUL]  Deteccion de CD-ROM no-emul
    ; ══════════════════════════════════════════════════════════════════════
    ; Reutilizamos el area de 'dap' como buffer para INT 13h/48h.
    ; Necesitamos 26 bytes minimo (bytes_per_sector en offset 0x18=24).
    ; dap = 16 bytes + los bytes cero de padding hasta 0x7DFF son suficientes.
    ; ──────────────────────────────────────────────────────────────────────
    mov  word [dap], 26          ; Tamano minimo del buffer EDD params
    mov  ah, 0x48
    mov  dl, [boot_drive]
    mov  si, dap
    int  0x13
    jc   .no_cd
    cmp  word [dap + 24], 2048   ; bytes_per_sector @ offset 0x18
    jne  .no_cd

    ; ── CD-ROM no-emul confirmado ──────────────────────────────────────
    ; El BIOS cargo 65*512=33280 bytes en 0x7C00:
    ;   0x7C00..0x7DFF = boot.bin (512B)
    ;   0x7E00..0xFDFF = stage2  (32768B)
    ; Necesitamos stage2 en 0x8000 (ORG). Backward copy (obligatorio por
    ; solapamiento: src 0x7E00..0xFDFF, dst 0x8000..0xFFFF se solapan
    ; en 0x8000..0xFDFF -> forward copy corromperia la fuente).
    ; ──────────────────────────────────────────────────────────────────────
    std                          ; DF=1: SI/DI decrementan
    mov  si, 0xFDFE              ; Ultimo word fuente  (0x7E00 + 0x8000 - 2)
    mov  di, 0xFFFE              ; Ultimo word destino (0x8000 + 0x8000 - 2)
    mov  cx, 0x4000              ; 16384 words = 32768 bytes
    rep  movsw                   ; DS:SI -> ES:DI (DS=ES=0)
    cld                          ; Restaurar DF=0

    ; base_lba = 0; stage2.detect_cdrom() leera BIT [0x7C0C] (xorriso lo
    ; parcha con el CD-sector del boot image en el ISO) y calculara la
    ; posicion correcta del kernel.
    xor  eax, eax
    mov  [BASE_LBA_ADDR], eax

    mov  dl, [boot_drive_orig]
    jmp  0x0000:0x8000           ; stage2 ya esta en su lugar

    ; ══════════════════════════════════════════════════════════════════════
.no_cd:
    ; Restaurar cabecera dap (INT 13h/48h puede haberla sobreescrito)
    mov  byte [dap],   0x10
    mov  byte [dap+1], 0x00

    ; ── Path normal: disco HDD/USB ─────────────────────────────────────
    ; [CUT-CANDIDATES] Solo se prueban: boot_drive_orig y 0x80.
    ; Los drives 0x81, 0x82, 0x9F, 0xE0 se prueban en stage2.asm
    ; que tiene mas espacio disponible.
    ; ──────────────────────────────────────────────────────────────────────
    mov  byte [drive_idx], 0

.pick_drive:
    mov  al, [drive_idx]
    cmp  al, 0
    je   .pick_orig
    cmp  al, 2
    jae  .use_chs
    ; idx=1: probar 0x80
    mov  dl, 0x80
    cmp  dl, [boot_drive_orig]
    je   .pick_next              ; Ya se probo como orig, saltar
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
    ; Sin carry: stage2 esta en LBA < 65 siempre
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