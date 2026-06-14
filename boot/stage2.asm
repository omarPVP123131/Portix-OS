; boot/stage2.asm — PORTIX Stage-2 v9.11
; nasm -f bin -DKERNEL_SECTORS=N [-DKERNEL_LBA=N] stage2.asm -o stage2.bin
;
; ══════════════════════════════════════════════════════════════════════════════
; CAMBIOS vs v9.9
; ══════════════════════════════════════════════════════════════════════════════
;
;   [FIX-BIT-OFFSETS]  Los defines BIT_* estaban desplazados 8 bytes.
;
;                      El Torito Boot Information Table (spec):
;                        xorriso con -boot-info-table parcha el boot image
;                        a partir del byte 0 del boot image (= 0x7C00 en RAM).
;
;                        offset +0x00  bi_pvd    LBA del PVD         (u32 LE)
;                        offset +0x04  bi_file   LBA del boot image  (u32 LE)  ← el que importa
;                        offset +0x08  bi_length longitud en bytes   (u32 LE)
;                        offset +0x0C  bi_csum   checksum            (u32 LE)
;
;                      Antes (v9.9) — INCORRECTO:
;                        BIT_PVD_LBA   equ 0x7C08  ← era bi_length, siempre != 0
;                        BIT_BOOT_LBA  equ 0x7C0C  ← era bi_csum, valor basura
;                        BIT_IMAGE_LEN equ 0x7C10  ← fuera del BIT
;                        BIT_CHECKSUM  equ 0x7C14  ← fuera del BIT
;
;                      Ahora (v9.10) — CORRECTO:
;                        BIT_PVD_LBA   equ 0x7C00
;                        BIT_BOOT_LBA  equ 0x7C04  ← LBA real del boot image
;                        BIT_IMAGE_LEN equ 0x7C08
;                        BIT_CHECKSUM  equ 0x7C0C
;
;                      Consecuencia del bug anterior:
;                        - [0x7C0C] = bi_csum podía ser != 0 → bit_valid=1
;                        - cdap_lba = bi_csum + 17  → LBA completamente incorrecto
;                        - Kernel leído de posición basura → pantalla negra
;                        - VDI/VMDK funcionaban porque no usan esta ruta
;
; Heredado de v9.9:
;   [FIX-VBOX-CD-BIOS]  Detección de CD por BIT, no INT 13h/48h.
;   [FIX-VBOX-UEFI]     BOOTX64.EFI para la entrada EFI de la ISO.
;   [FIX-PAGING-CRASH]  Page tables en 0x1000..0x4FFF.
;   [FIX-ISO-HDEMU]     ISO usa El Torito no-emul.
;
; ══════════════════════════════════════════════════════════════════════════════

BITS 16
ORG 0x8000

; ──────────────────────────────────────────────────────────────────────────────
; Parámetros de build
; ──────────────────────────────────────────────────────────────────────────────
%ifndef KERNEL_SECTORS
  %error "KERNEL_SECTORS no definido. Usar: nasm -DKERNEL_SECTORS=N"
%endif
%ifndef KERNEL_LBA
  %define KERNEL_LBA 68
%endif

%if (KERNEL_LBA % 4) != 0
  %error "KERNEL_LBA debe ser múltiplo de 4"
%endif

; ──────────────────────────────────────────────────────────────────────────────
; Constantes de layout de memoria
; ──────────────────────────────────────────────────────────────────────────────
KERNEL_LOAD_SEG  equ 0x1000
KERNEL_STAGING   equ 0x10000
KERNEL_PHYS_ADDR equ 0x200000
VESA_BUF         equ 0x6000
BASE_LBA_ADDR    equ 0x7E00
STACK_TOP        equ 0x7C00
CANARY_ADDR      equ 0x7800
CANARY_VAL       equ 0xDEAD

; ──────────────────────────────────────────────────────────────────────────────
; BIT (Boot Information Table) — offsets VERIFICADOS contra ISO real
;
; xorriso -boot-info-table parcha el boot image en bytes 8-23 (NO byte 0).
; El boot image se carga en 0x7C00. Verificado empíricamente: xorriso 1.5.6
; escribe BIT en bytes 8-23 del boot image, por tanto en RAM:
;
;   0x7C08  bi_pvd    LBA del PVD en el CD       (u32 LE)
;   0x7C0C  bi_file   LBA absoluto del boot image (u32 LE, sectores 2048B)
;   0x7C10  bi_length longitud del boot image     (u32 LE, bytes)
;   0x7C14  bi_csum   checksum del boot image     (u32 LE)
;
; IMPORTANTE: v9.10 "fix" cambió estos valores a 0x7C00/0x7C04/0x7C08/0x7C0C,
; pero eso fue INCORRECTO. El BIT REAL está en bytes 8-23 (0x7C08-0x7C17).
; ──────────────────────────────────────────────────────────────────────────────
BIT_PVD_LBA   equ 0x7C08   ; bi_pvd
BIT_BOOT_LBA  equ 0x7C0C   ; bi_file  (LBA del boot image en CD)
BIT_IMAGE_LEN equ 0x7C10   ; bi_length
BIT_CHECKSUM  equ 0x7C14   ; bi_csum

BINFO_BASE    equ 0x9000
BINFO_E820CNT equ BINFO_BASE + 0x00
BINFO_FLAGS   equ BINFO_BASE + 0x02
BINFO_LFB     equ BINFO_BASE + 0x04
BINFO_WIDTH   equ BINFO_BASE + 0x08
BINFO_HEIGHT  equ BINFO_BASE + 0x0A
BINFO_PITCH   equ BINFO_BASE + 0x0C
BINFO_BPP     equ BINFO_BASE + 0x0E
BINFO_E820    equ 0x9100

BOOTINFO_BASE            equ 0x600000
BOOTINFO_HDR_SIZE        equ 0xE0
BOOTINFO_MEMMAP_OFFSET   equ 0x100
BOOTINFO_MEMMAP_ENTRY_SIZE equ 48
BOOTINFO_MEMMAP_MAX      equ 128
BOOTINFO_RANGES_OFFSET   equ 0x1900
BOOTINFO_RANGE_ENTRY_SIZE equ 32
BOOTINFO_RANGES_COUNT    equ 6
BOOTINFO_FW_OFFSET       equ 0x1A00
BOOTINFO_FW_ENTRY_SIZE   equ 24
BOOTINFO_FW_COUNT        equ 0
BOOTINFO_TOTAL_SIZE      equ 0x1A00

BOOT_MAGIC_LO equ 0x424F4F54
BOOT_MAGIC_HI equ 0x50525458

BI_FLAG_FB_VALID  equ 0x00000001
BI_FLAG_MEM_VALID equ 0x00000002

MEM_USABLE_MAPPED   equ 1
MEM_USABLE_UNMAPPED equ 2
MEM_RESERVED        equ 3
MEM_ACPI_RECLAIM    equ 4
MEM_ACPI_NVS        equ 5
MEM_FRAMEBUFFER     equ 7
MEM_KERNEL          equ 8
MEM_KERNEL_STACK    equ 9
MEM_PAGE_TABLES     equ 10
MEM_LOADER_CODE     equ 11
MEM_LOADER_DATA     equ 12
MEM_LOADER_STACK    equ 13
MEM_BAD_MEMORY      equ 16

OWNER_FIRMWARE equ 1
OWNER_LOADER   equ 2
OWNER_KERNEL   equ 3
OWNER_DEVICE   equ 4
OWNER_RESERVED equ 5

RECLAIM_NEVER                   equ 0
RECLAIM_AFTER_KERNEL_INIT       equ 1
RECLAIM_AFTER_PAGING_TRANSITION equ 2
RECLAIM_AFTER_ACPI_INIT         equ 3

CACHE_UNKNOWN equ 0
CACHE_UC      equ 1
CACHE_WB      equ 4

PML4_ADDR     equ 0x1000
PDPT_ADDR     equ 0x2000
PD_IDENT_ADDR equ 0x3000
PD_LFB_ADDR   equ 0x4000

BOOT_STACK_TOP equ 0x508000

section .text

; ══════════════════════════════════════════════════════════════════════════════
; PUNTO DE ENTRADA
; ══════════════════════════════════════════════════════════════════════════════
start2:
    mov  [boot_drive], dl

    cli
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    mov  ss, ax
    mov  sp, STACK_TOP
    sti

    ; Debug: '0' = stage2 entered
    mov  ah, 0x0E
    mov  al, '0'
    xor  bx, bx
    int  0x10

    mov  word [CANARY_ADDR], CANARY_VAL

    mov  eax, [BASE_LBA_ADDR]
    mov  [base_lba], eax

    ; Debug: '1' = about to print title
    mov  ah, 0x0E
    mov  al, '1'
    xor  bx, bx
    int  0x10

    mov  si, msg_stage2
    call print

    call detect_cdrom_bit
    mov  al, 'A'
    call print_char

    ; Debug: 'L' = about to check long mode
    mov  al, 'L'
    call print_char
    call check_long_mode
    ; Debug: 'M' = long mode OK
    mov  al, 'M'
    call print_char
    call enable_a20
    ; Debug: 'N' = A20 done
    mov  al, 'N'
    call print_char
    call probe_e820
    ; Debug: 'O' = E820 done
    mov  al, 'O'
    call print_char
    ; Debug: 'P' = about to set up VESA
    mov  al, 'P'
    call print_char
    call setup_vesa
    ; Debug: 'Q' = VESA done (may display 'D' if failed)
    mov  al, 'Q'
    call print_char

    call pci_fallback_fb

    ; Debug: 'R' = about to load kernel
    mov  al, 'R'
    call print_char
    call load_kernel
    jnc  .kernel_loaded
    jmp  error_disk

.kernel_loaded:
    mov  al, 'B'
    call print_char
    mov  si, msg_kernel_ok
    call print

    call verify_kernel
    jnc  .kernel_ok
    mov  si, msg_kern_warn
    call print
.kernel_ok:
    mov  al, 'C'
    call print_char

    cmp  word [CANARY_ADDR], CANARY_VAL
    je   .canary_ok
    mov  si, msg_stack_smash
    call print
.canary_ok:
    call setup_paging
    mov  al, 'G'
    call print_char

    mov  al, 0x02
    mov  dx, 0x3F6
    out  dx, al
    out  0x80, al
    mov  dx, 0x376
    out  dx, al
    out  0x80, al

    call remap_pic
    mov  al, 'H'
    call print_char
    call enter_long_mode
    cli
    hlt

; ══════════════════════════════════════════════════════════════════════════════
; detect_cdrom_bit
;
; Lee [BIT_BOOT_LBA] = [0x7C0C] = bi_file (LBA absoluto del boot image).
;
; xorriso -boot-info-table escribe bi_file en byte 12 del boot image
; (= RAM 0x7C0C). Si != 0 → CD no-emul con BIT parchado.
;
; NOTA HISTÓRICA:
;   v9.9 leía [0x7C0C] = bi_csum (checksum) por error → siempre != 0,
;   bit_valid=1 con LBA basura → kernel cargado de posición incorrecta.
;   v9.10 "fix" cambió BIT_BOOT_LBA a 0x7C04 pero eso también era
;   incorrecto (leyó padding). v9.13 finalmente usa 0x7C0C que es
;   donde xorriso realmente escribe bi_file.
; ══════════════════════════════════════════════════════════════════════════════
detect_cdrom_bit:
    push ax
    push bx
    push cx
    push dx
    push es
    push di

    xor  ax, ax
    mov  ds, ax

    ; Leer bi_file = LBA absoluto del boot image (offset +0x04 del boot image)
    ; El boot image está en 0x7C00, por tanto bi_file está en 0x7C04
    mov  eax, [BIT_BOOT_LBA]    ; = [0x7C04]
    test eax, eax
    jz   .try_int13

    ; bi_file != 0 → BIT fue parchado por xorriso → es CD no-emul
    mov  byte [is_cdrom], 1
    mov  byte [bit_valid], 1
    mov  si, msg_cdrom_bit
    call print
    jmp  .done

.try_int13:
    ; Fallback: INT 13h/48h (para QEMU SeaBIOS que no tiene BIT,
    ; o hardware donde xorriso no parchó el BIT por alguna razón)
    xor  ax, ax
    mov  es, ax

    mov  di, drive_params_buf
    mov  cx, 33
    rep  stosw

    mov  word [drive_params_buf], 0x0042

    mov  ah, 0x48
    mov  dl, [boot_drive]
    int  0x13
    jc   .not_cdrom

    xor  ax, ax
    mov  ds, ax

    cmp  word [drive_params_buf + 0x18], 2048
    jne  .not_cdrom

    mov  byte [is_cdrom], 1
    mov  si, msg_cdrom_native
    call print
    jmp  .done

.not_cdrom:
    mov  byte [is_cdrom], 0

.done:
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    pop  di
    pop  es
    pop  dx
    pop  cx
    pop  bx
    pop  ax
    ret

; ══════════════════════════════════════════════════════════════════════════════
; enable_a20
; ══════════════════════════════════════════════════════════════════════════════
enable_a20:
    call check_a20
    jnz  .done

    mov  ax, 0x2401
    int  0x15
    jnc  .verify

    in   al, 0x92
    test al, 0x02
    jnz  .verify
    or   al, 0x02
    and  al, 0xFE
    out  0x92, al

.verify:
    xor  cx, cx
.wait1:
    loop .wait1
    call check_a20
    jnz  .done

    call a20_via_kbc
    xor  cx, cx
.wait2:
    loop .wait2
    call check_a20
    jnz  .done

    mov  si, msg_a20_warn
    call print
    or   word [BINFO_FLAGS], 0x0002

.done:
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    ret

; ══════════════════════════════════════════════════════════════════════════════
; probe_e820
; ══════════════════════════════════════════════════════════════════════════════
probe_e820:
    xor  ax, ax
    mov  es, ax
    mov  di, BINFO_BASE
    mov  cx, (BINFO_E820 - BINFO_BASE) / 2
    rep  stosw

    mov  di, BINFO_E820
    xor  ebx, ebx
    xor  bp, bp

.loop:
    mov  eax, 0xE820
    mov  ecx, 24
    mov  edx, 0x534D4150
    int  0x15
    push ax
    xor  ax, ax
    mov  es, ax
    pop  ax
    jc   .done
    cmp  eax, 0x534D4150
    jne  .done
    cmp  ecx, 20
    jb   .next
    mov  eax, [di]
    or   eax, [di+4]
    or   eax, [di+8]
    or   eax, [di+12]
    jz   .next
    cmp  bp, 127
    jae  .done
    add  di, 24
    inc  bp
.next:
    test ebx, ebx
    jnz  .loop
.done:
    mov  [BINFO_E820CNT], bp
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    ret

; ══════════════════════════════════════════════════════════════════════════════
; load_kernel
; ══════════════════════════════════════════════════════════════════════════════
load_kernel:
    cmp  byte [is_cdrom], 0
    je   .skip_cd
    call do_cdrom_load
    jnc  .ok
.skip_cd:

    mov  dl, [boot_drive]
    call try_lba_on_drive
    jnc  .ok

    mov  dl, 0x80
.scan_drives:
    cmp  dl, [boot_drive]
    je   .scan_next
    call try_lba_on_drive
    jnc  .ok
.scan_next:
    inc  dl
    cmp  dl, 0x83
    jb   .scan_drives

    mov  dl, 0x9F
    cmp  dl, [boot_drive]
    je   .skip_9F
    call try_lba_on_drive
    jnc  .ok
.skip_9F:

    mov  dl, 0xE0
    cmp  dl, [boot_drive]
    je   .skip_E0
    call try_lba_on_drive
    jnc  .ok
.skip_E0:

    mov  byte [lba_drive], 0x80
    call do_lba_load
    jnc  .ok

    mov  al, [boot_drive]
    mov  [chs_drive], al
    call do_chs_load
    jnc  .ok

    mov  byte [chs_drive], 0x80
    call do_chs_load
    jnc  .ok

    stc
    ret
.ok:
    clc
    ret

; ══════════════════════════════════════════════════════════════════════════════
; verify_kernel
; ══════════════════════════════════════════════════════════════════════════════
verify_kernel:
    push ax
    push cx
    push si
    push ds
    mov  ax, KERNEL_LOAD_SEG
    mov  ds, ax
    xor  si, si
    mov  cx, 8
    xor  ax, ax
.chk:
    or   ax, [si]
    add  si, 2
    loop .chk
    pop  ds
    pop  si
    pop  cx
    pop  ax
    test ax, ax
    jnz  .ok
    stc
    ret
.ok:
    clc
    ret

; ══════════════════════════════════════════════════════════════════════════════
; setup_vesa
; ══════════════════════════════════════════════════════════════════════════════
setup_vesa:
    xor  ax, ax
    mov  es, ax

    mov  word [vesa_mode], 0x4000 | 0x118
    call try_vesa_mode
    jnz  .activate
    mov  word [vesa_mode], 0x4000 | 0x11B
    call try_vesa_mode
    jnz  .activate
    mov  word [vesa_mode], 0x4000 | 0x115
    call try_vesa_mode
    jnz  .activate
    mov  word [vesa_mode], 0x4000 | 0x112
    call try_vesa_mode
    jnz  .activate
    mov  word [vesa_mode], 0x4000 | 0x111
    call try_vesa_mode
    jnz  .activate

    xor  eax, eax
    mov  [BINFO_LFB],    eax
    mov  [BINFO_WIDTH],  ax
    mov  [BINFO_HEIGHT], ax
    mov  [BINFO_PITCH],  ax
    mov  byte [BINFO_BPP], 0
    mov  word [BINFO_FLAGS], 0
    mov  al, 'D'
    call print_char
    ret

.activate:
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
    ret
.vesa_fail:
    xor  eax, eax
    mov  [BINFO_LFB], eax
    and  word [BINFO_FLAGS], ~0x0001
    ret

; ══════════════════════════════════════════════════════════════════════════════
; setup_paging
; ══════════════════════════════════════════════════════════════════════════════
setup_paging:
    mov  edi, PML4_ADDR
    xor  eax, eax
    mov  ecx, 0x4000 / 4
    rep  stosd

    mov  dword [dword PML4_ADDR],     PDPT_ADDR | 0x03
    mov  dword [dword PML4_ADDR + 4], 0

    mov  dword [dword PDPT_ADDR],     PD_IDENT_ADDR | 0x03
    mov  dword [dword PDPT_ADDR + 4], 0

    mov  edi, PD_IDENT_ADDR
    mov  eax, 0x00000083
    mov  ecx, 48
.fill_pd:
    mov  [edi],         eax
    mov  dword [edi+4], 0
    add  eax, 0x200000
    add  edi, 8
    loop .fill_pd

    mov  ebx, [BINFO_LFB]
    test ebx, ebx
    jz   .done

    mov  eax, ebx
    shr  eax, 30
    and  eax, 0x1FF
    jz   .map_lfb_low

    shl  eax, 3
    cmp  dword [PDPT_ADDR + eax], 0
    jne  .done

    mov  dword [PDPT_ADDR + eax],     PD_LFB_ADDR | 0x03
    mov  dword [PDPT_ADDR + eax + 4], 0

    ; Map LFB in PD_LFB table at correct PD index
    mov  eax, ebx
    and  eax, 0xFFE00000              ; 2MB-aligned physical base
    or   eax, 0x83                     ; PRESENT | WRITABLE | HUGE
    mov  edi, ebx
    shr  edi, 21
    and  edi, 0x1FF                    ; PD index for LFB virtual address
    shl  edi, 3
    add  edi, PD_LFB_ADDR             ; PDE address in PD_LFB table
    mov  ecx, 8                        ; map 16MB (8 × 2MB huge pages)
.fill_lfb_high:
    cmp  edi, PD_LFB_ADDR + 512*8
    jae  .done
    mov  [edi],         eax
    mov  dword [edi+4], 0
    add  eax, 0x200000
    add  edi, 8
    loop .fill_lfb_high
    jmp  .done

.map_lfb_low:
    mov  eax, ebx
    and  eax, 0xFFE00000
    mov  edi, ebx
    shr  edi, 21
    and  edi, 0x1FF
    shl  edi, 3
    add  edi, PD_IDENT_ADDR
    or   eax, 0x83
    mov  ecx, 8
.fill_lfb_low:
    cmp  edi, PD_IDENT_ADDR + 512*8
    jae  .done
    mov  [edi], eax
    mov  dword [edi+4], 0
    add  eax, 0x200000
    add  edi, 8
    loop .fill_lfb_low

.done:
    ret

; ══════════════════════════════════════════════════════════════════════════════
; remap_pic
; ══════════════════════════════════════════════════════════════════════════════
remap_pic:
    cli
    mov  al, 0x11
    out  0x20, al
    out  0xA0, al
    out  0x80, al
    mov  al, 0x20
    out  0x21, al
    out  0x80, al
    mov  al, 0x28
    out  0xA1, al
    out  0x80, al
    mov  al, 0x04
    out  0x21, al
    out  0x80, al
    mov  al, 0x02
    out  0xA1, al
    out  0x80, al
    mov  al, 0x01
    out  0x21, al
    out  0xA1, al
    out  0x80, al
    mov  al, 0xFF
    out  0x21, al
    out  0xA1, al
    ret

; ══════════════════════════════════════════════════════════════════════════════
; enter_long_mode
; ══════════════════════════════════════════════════════════════════════════════
enter_long_mode:
    lgdt [gdt64_desc]
    lidt [idt_null_desc]

    mov  eax, cr4
    or   eax, (1 << 5)
    mov  cr4, eax

    mov  eax, PML4_ADDR
    mov  cr3, eax

    mov  ecx, 0xC0000080
    rdmsr
    or   eax, (1 << 8)
    xor  edx, edx
    wrmsr

    mov  eax, cr0
    or   eax, (1 << 31) | (1 << 0)
    mov  cr0, eax

    o32 jmp far [far_jump_ptr]

; ══════════════════════════════════════════════════════════════════════════════
; try_lba_on_drive
; ══════════════════════════════════════════════════════════════════════════════
try_lba_on_drive:
    push ax
    push bx
    push dx
    mov  ah, 0x41
    mov  bx, 0x55AA
    int  0x13
    pop  dx
    push dx
    mov  [lba_drive], dl
    pop  dx
    pop  bx
    pop  ax
    call do_lba_load
    ret

; ══════════════════════════════════════════════════════════════════════════════
; check_long_mode
; ══════════════════════════════════════════════════════════════════════════════
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
    jnz  .ok
    mov  si, msg_no_lm
    call print
    cli
    hlt
.ok:
    ret

; ══════════════════════════════════════════════════════════════════════════════
; check_a20
; ══════════════════════════════════════════════════════════════════════════════
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

; ══════════════════════════════════════════════════════════════════════════════
; a20_via_kbc
; ══════════════════════════════════════════════════════════════════════════════
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

; ══════════════════════════════════════════════════════════════════════════════
; try_vesa_mode
; ══════════════════════════════════════════════════════════════════════════════
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

; ══════════════════════════════════════════════════════════════════════════════
; pci_fallback_fb
;
; Fallback cuando VESA no encuentra framebuffer.
; Escanea PCI (I/O ports 0xCF8/0xCFC) buscando controlador VGA (class 0x0300),
; luego itera los 6 BARs (0x10..0x24) para hallar el primer MMIO BAR.
; En VirtualBox el framebuffer está en BAR2 (BAR0 es I/O, BAR1 es VBE I/O).
; Configura BINFO con defaults 1024x768x32.
; Solo modifica BINFO si BINFO_LFB == 0.
; ══════════════════════════════════════════════════════════════════════════════
pci_fallback_fb:
    cmp  dword [BINFO_LFB], 0
    jne  .done

    pusha
    mov  al, 'E'
    call print_char

    mov  byte [pci_bus], 0
.bus_loop:
    mov  byte [pci_dev], 0
.dev_loop:
    xor  ax, ax
    mov  al, [pci_bus]
    mov  cl, [pci_dev]
    xor  ch, ch
    xor  dl, dl
    call pci_read32
    mov  bx, ax
    cmp  bx, 0xFFFF
    je   .next_dev

    mov  al, [pci_bus]
    mov  cl, [pci_dev]
    xor  ch, ch
    mov  dl, 8
    call pci_read32
    shr  eax, 16
    cmp  ax, 0x0300
    jne  .next_dev

    ; Found VGA controller — scan BARs 0-5 (offsets 0x10..0x24)
    mov  byte [pci_bar], 0x10
.bar_loop:
    mov  al, [pci_bus]
    mov  cl, [pci_dev]
    xor  ch, ch
    mov  dl, [pci_bar]
    call pci_read32
    test al, 1           ; bit 0 = 1 → I/O BAR, skip
    jnz  .next_bar
    and  eax, 0xFFFFFFF0
    jz   .next_bar       ; zero address, skip

    ; Valid MMIO BAR found
    mov  [BINFO_LFB], eax
    mov  al, 'F'
    call print_char
    mov  word [BINFO_WIDTH], 1024
    mov  word [BINFO_HEIGHT], 768
    mov  word [BINFO_PITCH], 4096
    mov  byte [BINFO_BPP], 32
    mov  word [BINFO_FLAGS], 2

    popa
    ret

.next_bar:
    add  byte [pci_bar], 4
    cmp  byte [pci_bar], 0x24
    jbe  .bar_loop

.next_dev:
    inc  byte [pci_dev]
    cmp  byte [pci_dev], 32
    jb   .dev_loop
.next_bus:
    inc  byte [pci_bus]
    cmp  byte [pci_bus], 2
    jb   .bus_loop

    popa
.done:
    ret

; ── pci_read32 ──
; Lee un DWORD del espacio de configuración PCI (I/O 0xCF8/0xCFC).
; Input:  al = bus, cl = device, ch = function, dl = register
; Output: eax = valor 32-bit
pci_read32:
    push bx
    push cx

    xor  ebx, ebx
    mov  bl, al
    shl  ebx, 16

    xor  eax, eax
    mov  al, cl
    shl  eax, 11
    or   ebx, eax

    xor  eax, eax
    mov  al, ch
    shl  eax, 8
    or   ebx, eax

    xor  eax, eax
    mov  al, dl
    and  al, 0xFC
    or   ebx, eax

    or   ebx, 0x80000000

    mov  dx, 0xCF8
    mov  eax, ebx
    out  dx, eax

    mov  dx, 0xCFC
    in   eax, dx

    pop  cx
    pop  bx
    ret

; ══════════════════════════════════════════════════════════════════════════════
; do_cdrom_load
;
; Calcula el LBA del kernel en el CD.
;
; Modo BIT (bit_valid=1 — VirtualBox, hardware real):
;   bi_file = [0x7C04] = LBA absoluto del boot image en sectores CD (2048B)
;   kernel  = bi_file + KERNEL_LBA/4
;
;   Por qué KERNEL_LBA/4:
;     boot image = 65 sectores×512B = 33280 bytes
;     ceil(33280 / 2048) = 17 sectores CD
;     KERNEL_LBA/4 = 68/4 = 17  → coincide exactamente
;
; Modo SeaBIOS (bit_valid=0 — QEMU):
;   SeaBIOS virtualiza el acceso: LBA 0 en INT 13h/42h = boot image[0]
;   kernel = 0 + KERNEL_LBA/4 = 17
; ══════════════════════════════════════════════════════════════════════════════
do_cdrom_load:
    pusha

    cmp  byte [bit_valid], 1
    jne  .use_seabios

    ; Modo BIT: bi_file está en [0x7C04] (corregido de 0x7C0C en v9.9)
    mov  eax, [BIT_BOOT_LBA]          ; = [0x7C04] = bi_file
    mov  ecx, KERNEL_LBA
    shr  ecx, 2                        ; KERNEL_LBA/4 = sectores CD
    add  eax, ecx                      ; LBA CD absoluto del kernel
    mov  [cdap_lba_lo], eax
    mov  dword [cdap_lba_hi], 0
    jmp  .load

.use_seabios:
    ; Modo SeaBIOS: LBA relativo al boot image
    mov  eax, KERNEL_LBA
    shr  eax, 2
    mov  [cdap_lba_lo], eax
    mov  dword [cdap_lba_hi], 0

.load:
    mov  eax, KERNEL_SECTORS
    add  eax, 3
    shr  eax, 2
    mov  [cd_remain], ax

    mov  word [cdap_offset],  0x0000
    mov  word [cdap_segment], KERNEL_LOAD_SEG

.block:
    mov  ax, [cd_remain]
    test ax, ax
    jz   .ok
    cmp  ax, 16
    jbe  .set_cnt
    mov  ax, 16
.set_cnt:
    mov  [cdap_count], ax

    mov  cx, 3
.retry:
    push cx
    mov  si, cdap
    mov  ah, 0x42
    mov  dl, [boot_drive]
    int  0x13
    pop  cx
    jnc  .blk_ok
    mov  [disk_err_code], ah
    push cx
    xor  ah, ah
    mov  dl, [boot_drive]
    int  0x13
    pop  cx
    loop .retry

    popa
    stc
    ret

.blk_ok:
    movzx eax, word [cdap_count]
    add   [cdap_lba_lo], eax
    movzx eax, word [cdap_count]
    shl   eax, 7
    add   [cdap_segment], ax
    mov   ax, [cdap_count]
    sub   [cd_remain], ax
    jmp   .block

.ok:
    popa
    clc
    ret

; ══════════════════════════════════════════════════════════════════════════════
; do_lba_load
; ══════════════════════════════════════════════════════════════════════════════
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
    cmp  ax, 64
    jbe  .set_count
    mov  ax, 64
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
    movzx eax, word [dap_count]
    add   [dap_lba_lo], eax
    jnc   .no_carry
    inc   dword [dap_lba_hi]
.no_carry:
    mov   ax, [dap_count]
    shl   ax, 5
    add   word [dap_segment], ax
    mov   ax, [dap_count]
    sub   word [lba_remain], ax
    jmp   .block

.ok:
    popa
    clc
    ret

; ══════════════════════════════════════════════════════════════════════════════
; do_chs_load
; ══════════════════════════════════════════════════════════════════════════════
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
    jz   .skip_geom
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

; ══════════════════════════════════════════════════════════════════════════════
; lba_to_chs_hd
; ══════════════════════════════════════════════════════════════════════════════
lba_to_chs_hd:
    push ax
    push bx

    mov  bx, [spt]
    test bx, bx
    jz   .default
    xor  dx, dx
    div  bx
    inc  dx
    mov  cl, dl

    mov  bx, [heads]
    test bx, bx
    jz   .default
    xor  dx, dx
    div  bx
    mov  dh, dl
    mov  ch, al
    shl  ah, 6
    or   cl, ah

    pop  bx
    pop  ax
    ret

.default:
    mov  cl, 1
    xor  ch, ch
    xor  dh, dh
    pop  bx
    pop  ax
    ret

; ══════════════════════════════════════════════════════════════════════════════
; print / print_err_code / error_disk
; ══════════════════════════════════════════════════════════════════════════════
print:
    pusha
.lp:
    lodsb
    or   al, al
    jz   .dn
    mov  ah, 0x0E
    mov  bh, 0
    int  0x10
    jmp  .lp
.dn:
    popa
    ret

; ── print_char ──
; Imprime un carácter en AL (AH=0x0E, INT 0x10)
print_char:
    pusha
    mov  ah, 0x0E
    mov  bh, 0
    int  0x10
    popa
    ret

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
    mov  bh, 0
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

; ══════════════════════════════════════════════════════════════════════════════
; DATOS
; ══════════════════════════════════════════════════════════════════════════════
msg_stage2       db "S2 v9.11 OK", 13, 10, 0
msg_kernel_ok    db "Kernel OK", 13, 10, 0
msg_err_disk     db "DISK ERR ", 0
msg_a20_warn     db "A20 WARN", 13, 10, 0
msg_kern_warn    db "KERN EMPTY?", 13, 10, 0
msg_no_cpuid     db "NO CPUID!", 13, 10, 0
msg_no_lm        db "NO LM CPU!", 13, 10, 0
msg_stack_smash  db "STACK WARN", 13, 10, 0
msg_cdrom_native db "CD INT13", 13, 10, 0
msg_cdrom_bit    db "CD BIT OK", 13, 10, 0
msg_crlf         db 13, 10, 0
str_0x           db "0x", 0

boot_drive      db 0
lba_drive       db 0x80
chs_drive       db 0x80
is_cdrom        db 0
bit_valid       db 0
base_lba        dd 0
lba_remain      dw 0
cd_remain       dw 0
chs_cur_lba     dw 0
chs_dest_seg    dw KERNEL_LOAD_SEG
disk_err_code   db 0
vesa_mode       dw 0
spt             dw 63
heads           dw 255
pci_bus         db 0
pci_dev         db 0
pci_bar         db 0

align 4
dap:
    db 0x10, 0x00
dap_count:   dw 1
dap_offset:  dw 0
dap_segment: dw KERNEL_LOAD_SEG
dap_lba_lo:  dd KERNEL_LBA
dap_lba_hi:  dd 0

align 4
cdap:
    db 0x10, 0x00
cdap_count:   dw 1
cdap_offset:  dw 0
cdap_segment: dw KERNEL_LOAD_SEG
cdap_lba_lo:  dd 0
cdap_lba_hi:  dd 0

align 4
drive_params_buf: times 66 db 0

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

; ══════════════════════════════════════════════════════════════════════════════
; LONG MODE ENTRY — 64 bits
; ══════════════════════════════════════════════════════════════════════════════
BITS 64
long_mode_entry:
    cli

    mov  ax, 0x10
    mov  ds, ax
    mov  es, ax
    mov  ss, ax
    xor  ax, ax
    mov  fs, ax
    mov  gs, ax

    mov  rsp, BOOT_STACK_TOP
    xor  rbp, rbp

    mov  rsi, KERNEL_STAGING
    mov  rdi, KERNEL_PHYS_ADDR
    mov  rcx, (KERNEL_SECTORS * 512) / 8
    rep  movsq

    call build_bootinfo

    ; Red pixel on LFB to confirm kernel entry
    mov  edi, [dword BINFO_LFB]
    test edi, edi
    jz   .no_lfb
    mov  dword [edi], 0x00FF0000
.no_lfb:

    mov  rdi, BOOTINFO_BASE
    mov  rax, KERNEL_PHYS_ADDR
    jmp  rax

; ══════════════════════════════════════════════════════════════════════════════
; build_bootinfo
; ══════════════════════════════════════════════════════════════════════════════
build_bootinfo:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11

    mov  rdi, BOOTINFO_BASE
    xor  eax, eax
    mov  rcx, BOOTINFO_TOTAL_SIZE / 8
    rep  stosq

    mov  dword [BOOTINFO_BASE + 0x00], BOOT_MAGIC_LO
    mov  dword [BOOTINFO_BASE + 0x04], BOOT_MAGIC_HI
    mov  dword [BOOTINFO_BASE + 0x08], 1
    mov  dword [BOOTINFO_BASE + 0x0C], 1
    mov  dword [BOOTINFO_BASE + 0x10], 1
    mov  dword [BOOTINFO_BASE + 0x14], BOOTINFO_HDR_SIZE
    mov  dword [BOOTINFO_BASE + 0x18], BOOTINFO_TOTAL_SIZE
    mov  dword [BOOTINFO_BASE + 0x1C], 0
    mov  qword [BOOTINFO_BASE + 0x20], BI_FLAG_MEM_VALID
    test word [BINFO_FLAGS], 0x0001
    jz   .flags_done
    or   qword [BOOTINFO_BASE + 0x20], BI_FLAG_FB_VALID
.flags_done:
    mov  qword [BOOTINFO_BASE + 0x28], 0
    mov  qword [BOOTINFO_BASE + 0x30], 0
    mov  qword [BOOTINFO_BASE + 0x38], 0
    mov  dword [BOOTINFO_BASE + 0x40], 1
    mov  dword [BOOTINFO_BASE + 0x44], 1
    mov  qword [BOOTINFO_BASE + 0x48], 0

    xor  rax, rax
    mov  eax, [BINFO_LFB]
    mov  [BOOTINFO_BASE + 0x50], rax
    movzx rax, word [BINFO_PITCH]
    movzx rbx, word [BINFO_HEIGHT]
    mul  rbx
    mov  [BOOTINFO_BASE + 0x58], rax
    movzx eax, word [BINFO_WIDTH]
    mov  [BOOTINFO_BASE + 0x60], eax
    movzx eax, word [BINFO_HEIGHT]
    mov  [BOOTINFO_BASE + 0x64], eax
    movzx eax, word [BINFO_PITCH]
    mov  [BOOTINFO_BASE + 0x68], eax
    movzx eax, byte [BINFO_BPP]
    mov  [BOOTINFO_BASE + 0x6C], eax
    mov  dword [BOOTINFO_BASE + 0x70], 1
    test word [BINFO_FLAGS], 2
    jz   .fb_src_done
    mov  dword [BOOTINFO_BASE + 0x70], 2
.fb_src_done:
    mov  dword [BOOTINFO_BASE + 0x74], 1
    cmp  eax, 16
    jne  .fb_not_565
    mov  dword [BOOTINFO_BASE + 0x74], 4
.fb_not_565:
    cmp  eax, 24
    jne  .fb_not_888
    mov  dword [BOOTINFO_BASE + 0x74], 5
.fb_not_888:
    mov  dword [BOOTINFO_BASE + 0x78], 0
    movzx eax, word [BINFO_PITCH]
    movzx ebx, byte [BINFO_BPP]
    add  ebx, 7
    shr  ebx, 3
    test ebx, ebx
    jz   .pps_done
    xor  edx, edx
    div  ebx
    mov  [BOOTINFO_BASE + 0x7C], eax
.pps_done:
    mov  dword [BOOTINFO_BASE + 0x80], CACHE_UNKNOWN

    mov  qword [BOOTINFO_BASE + 0x98], KERNEL_PHYS_ADDR
    mov  qword [BOOTINFO_BASE + 0xA0], KERNEL_SECTORS * 512

    mov  dword [BOOTINFO_BASE + 0xA8], BOOTINFO_MEMMAP_OFFSET
    movzx eax, word [BINFO_E820CNT]
    cmp  eax, BOOTINFO_MEMMAP_MAX
    jbe  .mm_count_ok
    mov  eax, BOOTINFO_MEMMAP_MAX
.mm_count_ok:
    mov  [BOOTINFO_BASE + 0xAC], eax
    mov  dword [BOOTINFO_BASE + 0xB0], BOOTINFO_MEMMAP_ENTRY_SIZE
    mov  dword [BOOTINFO_BASE + 0xB4], BOOTINFO_RANGES_OFFSET
    mov  dword [BOOTINFO_BASE + 0xB8], BOOTINFO_RANGES_COUNT
    mov  dword [BOOTINFO_BASE + 0xBC], BOOTINFO_RANGE_ENTRY_SIZE
    mov  dword [BOOTINFO_BASE + 0xC0], BOOTINFO_FW_OFFSET
    mov  dword [BOOTINFO_BASE + 0xC4], BOOTINFO_FW_COUNT
    mov  dword [BOOTINFO_BASE + 0xC8], BOOTINFO_FW_ENTRY_SIZE

    call build_memory_map
    call build_reserved_ranges
    call bootinfo_checksum

    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    ret

build_memory_map:
    movzx ecx, word [BINFO_E820CNT]
    cmp  ecx, BOOTINFO_MEMMAP_MAX
    jbe  .count_ok
    mov  ecx, BOOTINFO_MEMMAP_MAX
.count_ok:
    mov  rsi, BINFO_E820
    mov  rdi, BOOTINFO_BASE + BOOTINFO_MEMMAP_OFFSET
    test ecx, ecx
    jz   .done
.loop:
    mov  rax, [rsi + 0]
    mov  [rdi + 0], rax
    mov  rax, [rsi + 8]
    mov  [rdi + 8], rax
    mov  eax, [rsi + 16]
    mov  [rdi + 32], eax
    mov  qword [rdi + 40], 0
    mov  dword [rdi + 24], RECLAIM_NEVER
    mov  dword [rdi + 28], CACHE_UNKNOWN
    cmp  eax, 1
    je   .usable
    cmp  eax, 3
    je   .acpi_reclaim
    cmp  eax, 4
    je   .acpi_nvs
    cmp  eax, 5
    je   .bad
    mov  dword [rdi + 16], MEM_RESERVED
    mov  dword [rdi + 20], OWNER_FIRMWARE
    jmp  .next
.usable:
    mov  dword [rdi + 16], MEM_USABLE_UNMAPPED
    mov  dword [rdi + 20], OWNER_FIRMWARE
    mov  dword [rdi + 28], CACHE_WB
    mov  rax, [rsi + 0]
    mov  rbx, [rsi + 8]
    add  rbx, rax
    cmp  rbx, 0x800000
    ja   .next
    mov  dword [rdi + 16], MEM_USABLE_MAPPED
    jmp  .next
.acpi_reclaim:
    mov  dword [rdi + 16], MEM_ACPI_RECLAIM
    mov  dword [rdi + 20], OWNER_FIRMWARE
    mov  dword [rdi + 24], RECLAIM_AFTER_ACPI_INIT
    jmp  .next
.acpi_nvs:
    mov  dword [rdi + 16], MEM_ACPI_NVS
    mov  dword [rdi + 20], OWNER_FIRMWARE
    jmp  .next
.bad:
    mov  dword [rdi + 16], MEM_BAD_MEMORY
    mov  dword [rdi + 20], OWNER_RESERVED
.next:
    add  rsi, 24
    add  rdi, BOOTINFO_MEMMAP_ENTRY_SIZE
    dec  ecx
    jnz  .loop
.done:
    ret

%macro RANGE 6
    mov qword [rdi + 0], %1
    mov qword [rdi + 8], %2
    mov dword [rdi + 16], %3
    mov dword [rdi + 20], %4
    mov dword [rdi + 24], %5
    mov dword [rdi + 28], %6
    add rdi, BOOTINFO_RANGE_ENTRY_SIZE
%endmacro

build_reserved_ranges:
    mov  rdi, BOOTINFO_BASE + BOOTINFO_RANGES_OFFSET
    RANGE KERNEL_PHYS_ADDR,  KERNEL_SECTORS * 512, MEM_KERNEL,       OWNER_KERNEL,  RECLAIM_NEVER,                   CACHE_WB
    RANGE BOOTINFO_BASE,     BOOTINFO_TOTAL_SIZE,  MEM_LOADER_DATA,  OWNER_LOADER,  RECLAIM_NEVER,                   CACHE_WB
    RANGE PML4_ADDR,         0x4000,               MEM_PAGE_TABLES,  OWNER_LOADER,  RECLAIM_AFTER_PAGING_TRANSITION, CACHE_WB
    RANGE 0x8000,            0x8000,               MEM_LOADER_CODE,  OWNER_LOADER,  RECLAIM_AFTER_KERNEL_INIT,       CACHE_WB
    RANGE KERNEL_STAGING,    KERNEL_SECTORS * 512, MEM_LOADER_DATA,  OWNER_LOADER,  RECLAIM_AFTER_KERNEL_INIT,       CACHE_WB
    RANGE BOOT_STACK_TOP - 0x8000, 0x8000,         MEM_LOADER_STACK, OWNER_LOADER,  RECLAIM_AFTER_KERNEL_INIT,       CACHE_WB
    ret

bootinfo_checksum:
    mov  dword [BOOTINFO_BASE + 0x1C], 0
    xor  eax, eax
    mov  rsi, BOOTINFO_BASE
    mov  ecx, BOOTINFO_TOTAL_SIZE / 4
.sum:
    mov  edx, [rsi]
    add  eax, edx
    add  rsi, 4
    loop .sum
    neg  eax
    mov  [BOOTINFO_BASE + 0x1C], eax
    ret

DEFAULT ABS
times (512*64)-($-$$) db 0