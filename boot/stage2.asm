; boot/stage2.asm  -  PORTIX Stage-2  v9.3
; nasm -f bin -DKERNEL_SECTORS=N [-DKERNEL_LBA=N] stage2.asm -o stage2.bin
;
; CORRECCIONES vs v9.2 — EL BUG REAL DEL 0x01 EN ISO:
;
;   [FIX-IDE-ORDER]  El bloque "Deshabilitar IRQ IDE" (OUT 0x3F6/0x376) se
;                    movía ANTES de cargar el kernel. En VirtualBox con ISO
;                    en modo IDE, el BIOS usa el controlador IDE para servir
;                    las llamadas INT 13h. Deshabilitar el IDE antes de que
;                    el BIOS termine de leer el kernel causa que todas las
;                    llamadas INT 13h retornen 0x01 (invalid command).
;                    SOLUCIÓN: mover el bloque IDE disable al paso 7, justo
;                    antes de PIC remap, DESPUÉS de que el kernel ya cargó.
;
;   [FIX-ES-E820]    Después del bucle E820, restaurar ES=0 explícitamente.
;                    El BIOS de VirtualBox/SeaBIOS puede modificar ES durante
;                    INT 0x15/E820. Si ES queda sucio, las escrituras del
;                    DAP (que usa DS:SI donde DS=0) son correctas, pero las
;                    lecturas/escrituras que asumen ES=0 fallarían.
;
;   [FIX-DS-KERNEL]  Después de verificar el magic del kernel (que cambia DS
;                    temporalmente), restaurar DS=0 explícitamente aunque
;                    el código ya lo hacía — se hace más explícito y seguro.
;
; HEREDADO DE v9.2:
;   [FIX-ISO-LBA]  Fallback incondicional a 0x80 (paso C)
;   [FIX-HEADS]    heads dw 255
;   [FIX-CHS-ES]   ES:BX inicializados desde chs_dest_seg
;   [FIX-CHS-INIT] chs_dest_seg reinicializado al entrar

BITS 16
ORG 0x8000

%ifndef KERNEL_SECTORS
  %error "KERNEL_SECTORS no definido. Usar: nasm -DKERNEL_SECTORS=N"
%endif
%ifndef KERNEL_LBA
  %define KERNEL_LBA 65
%endif

KERNEL_LOAD_SEG  equ 0x1000
KERNEL_PHYS_ADDR equ 0x10000
VESA_BUF         equ 0x6000
BASE_LBA_ADDR    equ 0x7E00

BINFO_BASE    equ 0x9000
BINFO_E820CNT equ BINFO_BASE + 0x00
BINFO_FLAGS   equ BINFO_BASE + 0x02
BINFO_LFB     equ BINFO_BASE + 0x04
BINFO_WIDTH   equ BINFO_BASE + 0x08
BINFO_HEIGHT  equ BINFO_BASE + 0x0A
BINFO_PITCH   equ BINFO_BASE + 0x0C
BINFO_BPP     equ BINFO_BASE + 0x0E
BINFO_E820    equ 0x9100

PML4_ADDR     equ 0x1000
PDPT_ADDR     equ 0x2000
PD_IDENT_ADDR equ 0x3000
PD_LFB_ADDR   equ 0x4000

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

    mov  eax, [BASE_LBA_ADDR]
    mov  [base_lba], eax

    mov si, msg_stage2
    call print

    ; ── 1. Long Mode check ────────────────────────────────────────────────
    call check_long_mode

    ; ── 2. A20 ────────────────────────────────────────────────────────────
    call check_a20
    jnz .a20_done

    mov ax, 0x2401
    int 0x15
    jnc .a20_verify

.a20_port92:
    in   al, 0x92
    test al, 0x02
    jnz  .a20_verify
    or   al, 0x02
    and  al, 0xFE
    out  0x92, al

.a20_verify:
    xor  cx, cx
.a20_wait1:
    loop .a20_wait1
    call check_a20
    jnz .a20_done

    call a20_via_kbc
    xor  cx, cx
.a20_wait2:
    loop .a20_wait2
    call check_a20
    jnz .a20_done

    mov si, msg_a20_warn
    call print
    or word [BINFO_FLAGS], 0x0002

.a20_done:
    ; Restaurar DS=ES=0 por si alguna rutina de A20 los modificó
    xor ax, ax
    mov ds, ax
    mov es, ax

    ; ── 3. E820 ───────────────────────────────────────────────────────────
    mov  di, BINFO_BASE
    mov  cx, (BINFO_E820 - BINFO_BASE) / 2
    xor  ax, ax
    rep  stosw

    xor  ax, ax
    mov  es, ax                 ; [FIX-ES-E820] Asegurar ES=0 antes del bucle

    mov  di, BINFO_E820
    xor  ebx, ebx
    xor  bp, bp

.e820_loop:
    mov  eax, 0xE820
    mov  ecx, 24
    mov  edx, 0x534D4150
    int  0x15
    ; [FIX-ES-E820] Restaurar ES=0 después de cada INT 0x15 (el BIOS puede cambiarlo)
    push ax
    xor  ax, ax
    mov  es, ax
    pop  ax
    jc   .e820_done
    cmp  eax, 0x534D4150
    jne  .e820_done
    cmp  ecx, 20
    jb   .e820_next
    mov  eax, [di]
    or   eax, [di+4]
    or   eax, [di+8]
    or   eax, [di+12]
    jz   .e820_next
    add  di, 24
    inc  bp
.e820_next:
    test ebx, ebx
    jnz  .e820_loop
.e820_done:
    mov  [BINFO_E820CNT], bp

    ; [FIX-ES-E820] Restaurar DS=ES=0 explícitamente tras E820
    xor  ax, ax
    mov  ds, ax
    mov  es, ax

    ; ── 4. Cargar kernel ──────────────────────────────────────────────────
    ; [FIX-IDE-ORDER] El bloque IDE disable se mueve al paso 7 (post-kernel).
    ; NO deshabilitar IDE aquí — el BIOS lo necesita para INT 13h.
    ;
    ; Estrategia A→B→C→D→E:
    ; A) LBA con boot_drive original
    ; B) LBA con 0x80 (si distinto)
    ; C) LBA con 0x80 INCONDICIONAL (captura ISO/VBox)
    ; D) CHS con boot_drive original
    ; E) CHS con 0x80

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
    ; B) LBA con 0x80 (solo si drive != 0x80)
    cmp byte [boot_drive], 0x80
    je  .lba_force_80
    mov ah, 0x41
    mov bx, 0x55AA
    mov dl, 0x80
    int 0x13
    jc  .lba_force_80
    cmp bx, 0xAA55
    jne .lba_force_80
    mov byte [lba_drive], 0x80
    call do_lba_load
    jnc .kernel_loaded

    ; C) Forzar LBA con 0x80 sin verificar extensiones
.lba_force_80:
    mov byte [lba_drive], 0x80
    call do_lba_load
    jnc .kernel_loaded

    ; D) CHS en boot_drive
    mov al, [boot_drive]
    mov [chs_drive], al
    call do_chs_load
    jnc .kernel_loaded

    ; E) CHS con 0x80
    mov byte [chs_drive], 0x80
    call do_chs_load
    jnc .kernel_loaded

    jmp error_disk

.kernel_loaded:
    mov si, msg_kernel_ok
    call print

    ; Verificar magic del kernel
    mov  ax, KERNEL_LOAD_SEG
    mov  ds, ax
    xor  si, si
    mov  eax, [si]
    xor  ax, ax
    mov  ds, ax                 ; [FIX-DS-KERNEL] Restaurar DS=0 explícitamente
    mov  es, ax
    test eax, eax
    jnz  .kernel_magic_ok
    mov  si, msg_kern_warn
    call print
.kernel_magic_ok:

    ; ── 5. VESA ───────────────────────────────────────────────────────────
    xor ax, ax
    mov es, ax

    %macro try_mode 1
        mov word [vesa_mode], 0x4000 | %1
        call try_vesa_mode
        jnz .vesa_activate
    %endmacro

    try_mode 0x118
    try_mode 0x11B
    try_mode 0x115
    try_mode 0x112
    try_mode 0x111

    xor  eax, eax
    mov  [BINFO_LFB],    eax
    mov  [BINFO_WIDTH],  ax
    mov  [BINFO_HEIGHT], ax
    mov  [BINFO_PITCH],  ax
    mov  byte [BINFO_BPP], 0
    mov  word [BINFO_FLAGS], 0
    jmp  .vesa_done

.vesa_activate:
    mov  ax, 0x4F01
    mov  cx, [vesa_mode]
    and  cx, 0x01FF
    mov  di, VESA_BUF
    int  0x10
    mov  eax, [VESA_BUF + 0x28]
    mov  [BINFO_LFB], eax
    mov  ax, [VESA_BUF + 0x12]
    mov  [BINFO_WIDTH], ax
    mov  ax, [VESA_BUF + 0x14]
    mov  [BINFO_HEIGHT], ax
    mov  ax, [VESA_BUF + 0x10]
    mov  [BINFO_PITCH], ax
    mov  al, [VESA_BUF + 0x19]
    mov  [BINFO_BPP], al
    mov  ax, 0x4F02
    mov  bx, [vesa_mode]
    int  0x10
    cmp  ax, 0x004F
    jne  .vesa_fail
    or   word [BINFO_FLAGS], 0x0001
    jmp  .vesa_done
.vesa_fail:
    xor  eax, eax
    mov  [BINFO_LFB], eax
    and  word [BINFO_FLAGS], ~0x0001
.vesa_done:

    ; ── 6. Paginación ─────────────────────────────────────────────────────
    mov  edi, PML4_ADDR
    xor  eax, eax
    mov  ecx, (0x5000 / 4)
    rep  stosd

    mov  dword [PML4_ADDR],     PDPT_ADDR | 0x03
    mov  dword [PML4_ADDR + 4], 0
    mov  dword [PDPT_ADDR],     PD_IDENT_ADDR | 0x03
    mov  dword [PDPT_ADDR + 4], 0

    mov  edi, PD_IDENT_ADDR
    mov  eax, 0x00000083
    mov  ecx, 512
.fill_pd:
    mov  [edi],   eax
    mov  dword [edi+4], 0
    add  eax, 0x200000
    add  edi, 8
    loop .fill_pd

    mov  ebx, [BINFO_LFB]
    test ebx, ebx
    jz   .paging_done
    mov  eax, ebx
    shr  eax, 30
    and  eax, 0x1FF
    jz   .paging_done
    shl  eax, 3
    mov  dword [PDPT_ADDR + eax],     PD_LFB_ADDR | 0x03
    mov  dword [PDPT_ADDR + eax + 4], 0
    mov  edi, PD_LFB_ADDR
    mov  eax, ebx
    and  eax, 0xC0000000
    or   eax, 0x83
    mov  ecx, 512
.fill_lfb:
    mov  [edi],   eax
    mov  dword [edi+4], 0
    add  eax, 0x200000
    add  edi, 8
    loop .fill_lfb
.paging_done:

    ; ── 7. Deshabilitar IRQ IDE ───────────────────────────────────────────
    ; [FIX-IDE-ORDER] AHORA, después de cargar el kernel y VESA.
    ; El BIOS ya no necesita el IDE. Es seguro deshabilitarlo.
    mov al, 0x02
    mov dx, 0x3F6
    out dx, al
    out 0x80, al
    mov dx, 0x376
    out dx, al
    out 0x80, al

    ; ── 8. PIC remap ──────────────────────────────────────────────────────
    cli
    mov al, 0x11
    out 0x20, al
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
    out 0xA1, al
    out 0x80, al
    mov al, 0xFF
    out 0x21, al
    out 0xA1, al

    ; ── 9. Long Mode ──────────────────────────────────────────────────────
    lgdt [gdt64_desc]
    lidt [idt_null_desc]

    mov eax, cr4
    or  eax, (1 << 5)
    mov cr4, eax

    mov eax, PML4_ADDR
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
; check_long_mode
; ==============================================================================
check_long_mode:
    pushfd
    pop  eax
    mov  ecx, eax
    xor  eax, (1 << 21)
    push eax
    popfd
    pushfd
    pop  eax
    push ecx
    popfd
    xor  eax, ecx
    jnz  .has_cpuid
    mov  si, msg_no_cpuid
    call print
    cli
    hlt
.has_cpuid:
    mov  eax, 0x80000000
    cpuid
    cmp  eax, 0x80000001
    jae  .has_ext
    mov  si, msg_no_lm
    call print
    cli
    hlt
.has_ext:
    mov  eax, 0x80000001
    cpuid
    test edx, (1 << 29)
    jnz  .lm_ok
    mov  si, msg_no_lm
    call print
    cli
    hlt
.lm_ok:
    ret

; ==============================================================================
; check_a20
; ==============================================================================
check_a20:
    push ds
    push es
    push si
    push di
    push ax
    xor  ax, ax
    mov  ds, ax
    mov  si, 0x0500
    mov  ax, 0xFFFF
    mov  es, ax
    mov  di, 0x0510
    mov  al, [ds:si]
    push ax
    mov  al, [es:di]
    push ax
    mov  byte [ds:si], 0xAA
    mov  byte [es:di], 0x55
    cmp  byte [ds:si], 0x55
    pop  ax
    mov  byte [es:di], al
    pop  ax
    mov  byte [ds:si], al
    pop  ax
    pop  di
    pop  si
    pop  es
    pop  ds
    ret

; ==============================================================================
; a20_via_kbc
; ==============================================================================
a20_via_kbc:
    push ax
    call .wait_w
    mov  al, 0xAD
    out  0x64, al
    call .wait_w
    mov  al, 0xD0
    out  0x64, al
    call .wait_r
    in   al, 0x60
    push ax
    call .wait_w
    mov  al, 0xD1
    out  0x64, al
    call .wait_w
    pop  ax
    or   al, 0x02
    out  0x60, al
    call .wait_w
    mov  al, 0xAE
    out  0x64, al
    call .wait_w
    pop  ax
    ret
.wait_w:
    in   al, 0x64
    test al, 0x02
    jnz  .wait_w
    ret
.wait_r:
    in   al, 0x64
    test al, 0x01
    jz   .wait_r
    ret

; ==============================================================================
; try_vesa_mode
; ==============================================================================
try_vesa_mode:
    push ax
    push cx
    push di
    mov  ax, 0x4F01
    mov  cx, [vesa_mode]
    and  cx, 0x01FF
    mov  di, VESA_BUF
    int  0x10
    cmp  ax, 0x004F
    jne  .fail
    test byte [VESA_BUF], 0x80
    jz   .fail
    cmp  dword [VESA_BUF + 0x28], 0
    je   .fail
    or   ax, 1
    jmp  .done
.fail:
    xor  ax, ax
.done:
    pop  di
    pop  cx
    pop  ax
    ret

; ==============================================================================
; do_lba_load — carga kernel via INT 13h/42h usando [lba_drive]
; CF=0 éxito, CF=1 error
; ==============================================================================
do_lba_load:
    pusha

    mov  eax, [base_lba]
    add  eax, KERNEL_LBA
    mov  [dap_lba_lo], eax
    mov  dword [dap_lba_hi], 0
    mov  word [dap_segment], KERNEL_LOAD_SEG
    mov  word [dap_offset],  0
    mov  word [lba_remain],  KERNEL_SECTORS

.block:
    mov  ax, [lba_remain]
    test ax, ax
    jz   .ok
    cmp  ax, 127
    jbe  .set_count
    mov  ax, 127
.set_count:
    mov  [dap_count], ax

    mov  cx, 3
.retry:
    push cx
    mov  si, dap
    mov  ah, 0x42
    mov  dl, [lba_drive]
    int  0x13
    pop  cx
    jnc  .block_ok
    mov  [disk_err_code], ah
    push cx
    xor  ah, ah
    mov  dl, [lba_drive]
    int  0x13
    pop  cx
    loop .retry

    popa
    stc
    ret

.block_ok:
    mov  ax, [dap_count]
    movzx eax, ax
    add  [dap_lba_lo], eax
    jnc  .no_carry
    inc  dword [dap_lba_hi]
.no_carry:
    mov  ax, [dap_count]
    shl  ax, 5
    add  word [dap_segment], ax
    mov  ax, [dap_count]
    sub  word [lba_remain], ax
    jmp  .block

.ok:
    popa
    clc
    ret

; ==============================================================================
; do_chs_load — carga kernel via INT 13h/02h usando [chs_drive]
; CF=0 éxito, CF=1 error
; ==============================================================================
do_chs_load:
    pusha

    push es
    mov  ah, 0x08
    mov  dl, [chs_drive]
    int  0x13
    jc   .skip_geom
    and  cx, 0x003F
    jz   .skip_geom
    mov  [spt], cx
    movzx ax, dh
    inc  ax
    mov  [heads], ax
.skip_geom:
    pop  es

    mov  word [chs_dest_seg], KERNEL_LOAD_SEG

    mov  eax, [base_lba]
    add  eax, KERNEL_LBA
    cmp  eax, 0x0000FFFF
    ja   .chs_too_far
    mov  [chs_cur_lba], ax

    mov  cx, KERNEL_SECTORS

.outer:
    push cx

    mov  ax, [chs_cur_lba]
    call lba_to_chs_hd

    mov  ax, [chs_dest_seg]
    mov  es, ax
    xor  bx, bx

    mov  cx, 3
.inner:
    push cx
    mov  ah, 0x02
    mov  al, 1
    mov  dl, [chs_drive]
    int  0x13
    pop  cx
    jnc  .sec_ok
    mov  [disk_err_code], ah
    push cx
    xor  ah, ah
    mov  dl, [chs_drive]
    int  0x13
    mov  ax, [chs_dest_seg]
    mov  es, ax
    xor  bx, bx
    mov  ax, [chs_cur_lba]
    call lba_to_chs_hd
    pop  cx
    loop .inner

    pop  cx
    popa
    stc
    ret

.sec_ok:
    mov  ax, [chs_dest_seg]
    add  ax, 0x20
    mov  [chs_dest_seg], ax
    inc  word [chs_cur_lba]
    pop  cx
    loop .outer
    popa
    clc
    ret

.chs_too_far:
    popa
    stc
    ret

; ==============================================================================
; lba_to_chs_hd
; ==============================================================================
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

; ==============================================================================
; print
; ==============================================================================
print:
    pusha
.lp:
    lodsb
    or   al, al
    jz   .dn
    mov  ah, 0x0E
    int  0x10
    jmp  .lp
.dn:
    popa
    ret

; ==============================================================================
; print_err_code
; ==============================================================================
print_err_code:
    pusha
    mov  si, str_0x
    call print
    mov  al, [disk_err_code]
    mov  bl, al
    shr  al, 4
    call .nib
    mov  al, bl
    and  al, 0x0F
    call .nib
    popa
    ret
.nib:
    add  al, '0'
    cmp  al, '9'
    jbe  .em
    add  al, 7
.em:
    mov  ah, 0x0E
    int  0x10
    ret

error_disk:
    mov  si, msg_err_disk
    call print
    call print_err_code
    mov  si, msg_crlf
    call print
    cli
    hlt

; ==============================================================================
; Datos
; ==============================================================================
msg_stage2    db "S2 v9.3 OK", 13, 10, 0
msg_kernel_ok db "Kernel OK", 13, 10, 0
msg_err_disk  db "DISK ERR ", 0
msg_a20_warn  db "A20 WARN", 13, 10, 0
msg_kern_warn db "KERN?0", 13, 10, 0
msg_no_cpuid  db "NO CPUID!", 13, 10, 0
msg_no_lm     db "NO LM CPU!", 13, 10, 0
msg_crlf      db 13, 10, 0
str_0x        db "0x", 0

boot_drive    db 0
lba_drive     db 0x80
chs_drive     db 0x80
base_lba      dd 0
lba_remain    dw 0
chs_cur_lba   dw 0
chs_dest_seg  dw KERNEL_LOAD_SEG
disk_err_code db 0
vesa_mode     dw 0
spt           dw 63
heads         dw 255

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

; ==============================================================================
; long_mode_entry
; ==============================================================================
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
    mov rsp, 0x8FF00
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