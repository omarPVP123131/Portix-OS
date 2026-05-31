; boot/stage2.asm — PORTIX Stage-2 v9.8
; nasm -f bin -DKERNEL_SECTORS=N [-DKERNEL_LBA=N] stage2.asm -o stage2.bin
;
; ══════════════════════════════════════════════════════════════════════════════
; CAMBIOS vs v9.7
; ══════════════════════════════════════════════════════════════════════════════
;
;   [FIX-PAGING-CRASH]  El triple fault en la activación de paginación se
;                       debía a que PML4/PDPT/PD estaban en 0x800000..0x803000,
;                       pero el identity map solo cubría 0x000000..0x7FFFFF
;                       (4 entradas × 2 MB). Las page tables caían exactamente
;                       1 byte fuera del rango mapeado → #PF al activar CR0.PG
;                       → double fault → triple fault.
;
;                       SOLUCIÓN: page tables movidas al primer MB libre
;                       (0x1000..0x4FFF), zona siempre cubierta por la entrada
;                       0 del identity map (0x000000..0x1FFFFF).
;
;                       Layout del primer MB con este fix:
;                         0x0000..0x03FF  IVT (BIOS)
;                         0x0400..0x04FF  BDA (BIOS)
;                         0x0500..0x0FFF  libre
;                         0x1000..0x1FFF  PML4  ← (antes 0x800000)
;                         0x2000..0x2FFF  PDPT  ← (antes 0x801000)
;                         0x3000..0x3FFF  PD_IDENT ← (antes 0x802000)
;                         0x4000..0x4FFF  PD_LFB   ← (antes 0x803000)
;                         0x5000..0x7BFF  libre
;                         0x7BF0          CANARY
;                         0x7C00          boot.bin (512 B)
;                         0x7E00          base_lba (dword, escrito por boot.asm)
;                         0x8000..0xFFFF  stage2.bin (este archivo, 32 KB)
;                         0x10000..0x9FFFF staging kernel
;
;                       Identity map: 5 entradas × 2 MB = 10 MB (0..0x9FFFFF)
;                       cubre: IVT, BDA, page tables, stack, BIOS data, stage2,
;                       staging y las primeras instrucciones del kernel.
;                       El kernel vive en 0x200000..N dentro de ese rango.
;
;   [FIX-ISO-HDEMU]   (de v9.7) ISO usa El Torito no-emul. detect_cdrom()
;                     devuelve is_cdrom=0 con HD-emul. do_cdrom_load() es
;                     fallback para CD-ROM físico no-emul futuro.
;
;   [FIX-BIT-REMOVE]  (de v9.7) Eliminada lectura del BIT para HD-emul.
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
CANARY_ADDR      equ 0x7BF0
CANARY_VAL       equ 0xDEAD

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

; ── [FIX-PAGING-CRASH] Page tables en el primer MB ──────────────────────────
; Zona 0x0500..0x7BEF está libre (no la usa BIOS, BDA ni el stack de stage2).
; Estas direcciones siempre quedan cubiertas por la primera entrada 2 MB
; del identity map (0x000000..0x1FFFFF) — se mapean a sí mismas antes de
; que CR0.PG se active, así que el MMU puede leerlas sin #PF.
PML4_ADDR     equ 0x1000   ; era 0x800000 → fuera del identity map → CRASH
PDPT_ADDR     equ 0x2000   ; era 0x801000
PD_IDENT_ADDR equ 0x3000   ; era 0x802000
PD_LFB_ADDR   equ 0x4000   ; era 0x803000

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

    mov  word [CANARY_ADDR], CANARY_VAL

    mov  eax, [BASE_LBA_ADDR]
    mov  [base_lba], eax

    mov  si, msg_stage2
    call print

    call detect_cdrom
    call check_long_mode
    call enable_a20
    call probe_e820
    call load_kernel
    jnc  .kernel_loaded
    jmp  error_disk

.kernel_loaded:
    mov  si, msg_kernel_ok
    call print

    call verify_kernel
    jnc  .kernel_ok
    mov  si, msg_kern_warn
    call print
.kernel_ok:

    cmp  word [CANARY_ADDR], CANARY_VAL
    je   .canary_ok
    mov  si, msg_stack_smash
    call print
.canary_ok:

    call setup_vesa
    call setup_paging

    ; Deshabilitar IRQ IDE
    mov  al, 0x02
    mov  dx, 0x3F6
    out  dx, al
    out  0x80, al
    mov  dx, 0x376
    out  dx, al
    out  0x80, al

    call remap_pic
    call enter_long_mode
    cli
    hlt

; ══════════════════════════════════════════════════════════════════════════════
; detect_cdrom
; ══════════════════════════════════════════════════════════════════════════════
detect_cdrom:
    push ax
    push bx
    push cx
    push dx
    push es
    push di

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
;
; [FIX-PAGING-CRASH] Las page tables ahora están en 0x1000..0x4FFF,
; dentro del primer MB, siempre cubiertos por la entrada 0 del identity map
; (0x000000..0x1FFFFF). El identity map se amplía de 4 a 5 entradas de 2 MB
; para cubrir también 0x800000..0x9FFFFF donde puede vivir el LFB en QEMU.
; ══════════════════════════════════════════════════════════════════════════════
setup_paging:
    ; Limpiar 4 tablas × 4 KB = 16 KB en 0x1000..0x4FFF
    ; Usar edi como puntero (32 bits OK en modo protegido antes de long mode)
    mov  edi, PML4_ADDR
    xor  eax, eax
    mov  ecx, 0x4000 / 4
    rep  stosd

    ; PML4[0] → PDPT (bit 0=Present, bit 1=R/W)
    mov  dword [dword PML4_ADDR],     PDPT_ADDR | 0x03
    mov  dword [dword PML4_ADDR + 4], 0

    ; PDPT[0] → PD_IDENT (primer GiB)
    mov  dword [dword PDPT_ADDR],     PD_IDENT_ADDR | 0x03
    mov  dword [dword PDPT_ADDR + 4], 0

    ; PD_IDENT: identity map 0..96 MB (48 entradas × 2 MB)
    ; Cubre: IVT/BDA, page tables (0x1000..0x4FFF), stage2 (0x8000..0xFFFF),
    ;        staging (0x10000..0x9FFFF), kernel (0x200000..N),
    ;        BOOTINFO (0x600000..0x61AFFF), BOOT_STACK_TOP (0x508000),
    ;        HEAP (0x1000000..0x4FFFFFF), BACKBUF (0x5000000..0x57FFFFF).
    mov  edi, PD_IDENT_ADDR
    mov  eax, 0x00000083        ; Present + R/W + PS (2 MB page)
    mov  ecx, 48                ; 48 × 2MB = 96 MB (cubre heap + backbuffer)
.fill_pd:
    mov  [edi],         eax
    mov  dword [edi+4], 0
    add  eax, 0x200000
    add  edi, 8
    loop .fill_pd

    ; Mapear LFB si está fuera del primer GiB
    mov  ebx, [BINFO_LFB]
    test ebx, ebx
    jz   .done

    mov  eax, ebx
    shr  eax, 30
    and  eax, 0x1FF
    jz   .map_lfb_low           ; LFB dentro del primer GiB → mapear abajo

    shl  eax, 3
    cmp  dword [PDPT_ADDR + eax], 0
    jne  .done

    mov  dword [PDPT_ADDR + eax],     PD_LFB_ADDR | 0x03
    mov  dword [PDPT_ADDR + eax + 4], 0

    mov  edi, PD_LFB_ADDR
    mov  eax, ebx
    and  eax, 0xC0000000
    or   eax, 0x83
    mov  ecx, 512
.fill_lfb:
    mov  [edi],         eax
    mov  dword [edi+4], 0
    add  eax, 0x200000
    add  edi, 8
    loop .fill_lfb
    jmp  .done

.map_lfb_low:
    ; LFB dentro del primer GiB pero fuera de la identidad mínima
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

    ; CR4.PAE = 1
    mov  eax, cr4
    or   eax, (1 << 5)
    mov  cr4, eax

    ; CR3 = PML4 (ahora en 0x1000, siempre mapeado)
    mov  eax, PML4_ADDR
    mov  cr3, eax

    ; EFER.LME = 1
    mov  ecx, 0xC0000080
    rdmsr
    or   eax, (1 << 8)
    xor  edx, edx
    wrmsr

    ; CR0.PG + CR0.PE = 1
    ; Ahora que CR3 apunta a tablas dentro del identity map,
    ; el primer acceso del MMU (PML4 en 0x1000) está mapeado → sin #PF.
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
; do_cdrom_load
; ══════════════════════════════════════════════════════════════════════════════
do_cdrom_load:
    pusha

    mov  eax, [base_lba]
    add  eax, KERNEL_LBA
    shr  eax, 2
    mov  [cdap_lba_lo], eax
    mov  dword [cdap_lba_hi], 0

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
msg_stage2       db "S2 v9.8 OK", 13, 10, 0
msg_kernel_ok    db "Kernel OK", 13, 10, 0
msg_err_disk     db "DISK ERR ", 0
msg_a20_warn     db "A20 WARN", 13, 10, 0
msg_kern_warn    db "KERN EMPTY?", 13, 10, 0
msg_no_cpuid     db "NO CPUID!", 13, 10, 0
msg_no_lm        db "NO LM CPU!", 13, 10, 0
msg_stack_smash  db "STACK WARN", 13, 10, 0
msg_cdrom_native db "CD NATIVE", 13, 10, 0
msg_crlf         db 13, 10, 0
str_0x           db "0x", 0

boot_drive      db 0
lba_drive       db 0x80
chs_drive       db 0x80
is_cdrom        db 0
base_lba        dd 0
lba_remain      dw 0
cd_remain       dw 0
chs_cur_lba     dw 0
chs_dest_seg    dw KERNEL_LOAD_SEG
disk_err_code   db 0
vesa_mode       dw 0
spt             dw 63
heads           dw 255

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
    dq 0x00AF9A000000FFFF   ; Code 64-bit
    dq 0x00CF92000000FFFF   ; Data 32/64-bit
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

    ; Copiar kernel desde staging (< 1 MB) a dirección física final
    mov  rsi, KERNEL_STAGING
    mov  rdi, KERNEL_PHYS_ADDR
    mov  rcx, (KERNEL_SECTORS * 512) / 8
    rep  movsq

    call build_bootinfo

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