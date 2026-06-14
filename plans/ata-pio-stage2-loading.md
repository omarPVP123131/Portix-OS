# ATA PIO en Stage2 — Carga escalable del kernel (plan futuro)

## Problema

INT 13h en modo real tiene dos límites fatales para kernels grandes:

| Límite | Causa | Máximo |
|--------|-------|--------|
| **VGA/BIOS hole** | El staging `0x10000` colisiona con framebuffer VGA (`0xA0000`), VGA BIOS (`0xC0000`) y system BIOS (`0xF0000`) | ~576 KB |
| **Segment overflow** | `add word [dap_segment], ax` trunca a 16 bits cuando > `0xFFFF` | ~928 KB (1856 sectores) |

Si el kernel crece, el boot se rompe. Con el fix actual (reducir `MAX_PROCS`/`MAX_FDS`) apenas rozamos 601 KB. Escalar a 2 MB+ es imposible sin cambiar la arquitectura.

## Solución

Dividir la carga del kernel en **dos fases**:

| Fase | Modo | Método | Destino | Límite |
|------|------|--------|---------|--------|
| 1 | Real (16-bit) | INT 13h LBA | `0x10000` | 576 KB (hasta `0xA0000`) |
| 2 | Protegido (32-bit) | ATA PIO | `0x100000` (1 MB) | Ilimitado (32-bit addr) |

## Flujo de boot modificado

```
Real mode:
  ┌─ detect_cdrom_bit, check_long_mode, enable_a20, probe_e820
  ├─ setup_vesa ──────────────────────────── BIOS intacto, sin kernel cargado
  ├─ pci_fallback_fb
  │
  ├─ load_kernel_part1 (INT 13h)
  │   └─ Lee min(1152, KERNEL_SECTORS) sectores a staging 0x10000
  │      └─ Siempre cabe antes de VGA framebuffer (0xA0000)
  │
  ├─ verify_kernel, canary
  │
Protected mode 32-bit:
  │
  ├─ lgdt [gdt32] ────────────────────────── GDT con code segment 32-bit
  ├─ mov eax, cr0 | 1; mov cr0, eax ──────── PE=1
  ├─ far jmp 0x18:pm32_entry ─────────────── Salto a código 32-bit
  │
  ├─ load_kernel_part2 (ATA PIO)
  │   └─ Lee sectores restantes (si KERNEL_SECTORS > 1152) a 0x100000+
  │      └─ Usa puertos ATA 0x1F0-0x1F7, direcciones 32-bit
  │
  ├─ setup_paging ────────────────────────── Page tables (0x1000-0x4FFF)
  ├─ remap_pic
  │
  ├─ set EFER.LME ────────────────────────── Long mode enable
  ├─ mov cr0, PG | PE ────────────────────── Paging on
  │
Long mode 64-bit:
  │
  ├─ far jmp 0x08:lm64_entry ─────────────── Salto a código 64-bit
  │
  ├─ copy kernel a destino (0x200000)
  │   ├─ rep movsq desde 0x10000 (parte 1)
  │   └─ rep movsq desde 0x100000 (parte 2, si existe)
  │
  ├─ build_bootinfo
  ├─ jmp KERNEL_PHYS_ADDR ────────────────── Salto al kernel
```

## Cambios necesarios en `boot/stage2.asm`

### 1. Agregar entry 32-bit al GDT

```asm
gdt32:
    dq 0x0000000000000000       ; 0x00 null
    dq 0x00AF9A000000FFFF       ; 0x08 64-bit code
    dq 0x00CF92000000FFFF       ; 0x10 64/32-bit data
    dq 0x00CF9A000000FFFF       ; 0x18 32-bit code (nuevo)
gdt32_end:
```

### 2. Función ATA PIO (32-bit protected mode)

```asm
BITS 32
pm32_entry:
    ; ── Configurar segmentos planos ──
    mov  ax, 0x10
    mov  ds, ax
    mov  es, ax
    mov  ss, ax
    mov  sp, 0x7C00              ; stack temporal

    ; ── Cargar parte 2 del kernel ──
    mov  eax, [part1_sectors]    ; sectores ya cargados en fase 1
    mov  ebx, KERNEL_SECTORS
    cmp  eax, ebx
    jae  .done                   ; si no hay parte 2, saltear

    sub  ebx, eax                ; sectores restantes
    mov  ecx, ebx
    mov  eax, KERNEL_LBA
    add  eax, [part1_sectors]    ; LBA inicial de parte 2
    mov  edx, 0x100000           ; destino = 1 MB
    call ata_pio_read

.done:
    ; ── Continuar a long mode ──
    ; setup_paging + enter_long_mode
    jmp  setup_and_enter_lm


; ──────────────────────────────────────────────────────────────────────────────
; ata_pio_read — Lee sectores de disco ATA via PIO en 32-bit PM
; Input:
;   eax = LBA inicial (28-bit)
;   ecx = número de sectores
;   edx = destino (dirección física 32-bit)
; ──────────────────────────────────────────────────────────────────────────────
ata_pio_read:
    push eax
    push ecx
    push edx
    push ebp

    mov  ebp, ecx                ; sector count
    mov  ecx, eax                ; LBA

.loop:
    test ebp, ebp
    jz   .done

    ; Esperar que el controlador esté listo
    mov  dx, 0x1F7
.wait_bsy:
    in   al, dx
    test al, 0x80                ; BSY
    jnz  .wait_bsy
.wait_drq:
    in   al, dx
    test al, 0x40                ; DRDY
    jz   .wait_drq

    ; Seleccionar drive + LBA high nibble
    mov  dx, 0x1F6
    mov  al, 0xE0                ; LBA mode, master
    shl  ecx, 24                 ; LBA bits 24-27 → AL high nibble
    and  al, 0x0F
    or   al, cl
    out  dx, al
    shr  ecx, 24                 ; restore ECX = LBA (low 24 bits)

    ; Sector count = 1
    mov  dx, 0x1F2
    mov  al, 1
    out  dx, al

    ; LBA low 24 bits (3 puertos: 0x1F3, 0x1F4, 0x1F5)
    mov  dx, 0x1F3
    mov  al, cl
    out  dx, al
    mov  dx, 0x1F4
    mov  al, ch
    out  dx, al
    shr  ecx, 16
    mov  dx, 0x1F5
    mov  al, cl
    out  dx, al

    ; Comando READ SECTORS
    mov  dx, 0x1F7
    mov  al, 0x20
    out  dx, al

    ; Esperar DRQ
.wait:
    in   al, dx
    test al, 0x80                ; BSY
    jnz  .wait
    test al, 0x01                ; ERR
    jnz  .error
    test al, 0x08                ; DRQ
    jz   .wait

    ; Leer 256 words (512 bytes)
    mov  dx, 0x1F0
    mov  cx, 256
    rep  insw

    ; Avanzar a siguiente sector
    inc  ecx                     ; LBA++
    add  edx, 512                ; dest += 512
    dec  ebp
    jmp  .loop

.done:
    pop  ebp
    pop  edx
    pop  ecx
    pop  eax
    ret

.error:
    ; Si hay error, reintentar o abortar
    ; Por simplicidad: loop infinito con pánico
    cli
    hlt
    jmp  .error
```

### 3. Copia en 2 fases (64-bit)

```asm
BITS 64
long_mode_entry:
    cli
    mov  ax, 0x10
    mov  ds, ax
    mov  es, ax
    mov  ss, ax

    ; ── Copiar parte 1 (desde staging clásico) ──
    mov  rsi, KERNEL_STAGING          ; 0x10000
    mov  rdi, KERNEL_PHYS_ADDR        ; 0x200000
    mov  rax, [part1_sectors]
    imul rax, 512
    mov  rcx, rax
    shr  rcx, 3
    rep  movsq

    ; ── Copiar parte 2 (desde 1 MB, si existe) ──
    mov  rax, KERNEL_SECTORS
    sub  rax, [part1_sectors]
    jz   .copy_done                   ; no part2

    mov  rsi, 0x100000
    mov  rdi, KERNEL_PHYS_ADDR
    mov  rax, [part1_sectors]
    imul rax, 512
    add  rdi, rax                      ; dest = KERNEL_PHYS_ADDR + part1_size

    mov  rax, KERNEL_SECTORS
    sub  rax, [part1_sectors]
    imul rax, 512
    mov  rcx, rax
    shr  rcx, 3
    rep  movsq

.copy_done:
    call build_bootinfo
    jmp  KERNEL_PHYS_ADDR
```

### 4. Variables nuevas

```asm
part1_sectors:   dw 0    ; se calcula en fase 1
```

## Condicional: kernel pequeño saltea ATA PIO

```asm
; En fase 1 (modo real):
mov  ax, KERNEL_SECTORS
cmp  ax, 1152               ; max sectores antes de VGA hole
jbe  .load_all
    mov  [part1_sectors], 1152
    jmp  .load_part1
.load_all:
    mov  [part1_sectors], ax
.load_part1:
    ; cargar [part1_sectors] sectores via INT 13h
```

Si `KERNEL_SECTORS <= 1152`, la fase 2 se saltea completamente y el boot es idéntico al actual.

## Ventajas

- **Sin límite de tamaño** — kernel puede ser de varios MB
- **Sin segment overflow** — ATA PIO usa `ecx`/`edx` 32-bit
- **Sin dependencia de VGA/BIOS** — la fase 2 solo toca puertos ATA
- **Backwards compatible** — CD-ROM sigue su propia ruta (INT 13h)
- **Zero overhead** — si kernel ≤ 576 KB, el flujo es idéntico al actual
- **Código mínimo** — ~100 líneas de asm nuevo

## Consideraciones

- **Asume ATA legacy** (puertos 0x1F0-0x1F7, IRQ 14) — funciona en QEMU, VirtualBox, hardware real con IDE
- **SATA AHCI** requeriría controladora diferente (implementación futura)
- **CD-ROM** no usa ATA PIO — sigue con INT 13h/DAP (sectores de 2048 bytes)
- **Timeout/error handling** — la implementación básica hace busy-wait con reintentos

## Implementación futura

Cuando se implemente esto:

1. **Revisar `KERNEL_STAGING`** — puede permanecer en `0x10000` o moverse a `0x100000` para simplificar
2. **Agregar `gdt32`** con entry 32-bit code (`0x18`)
3. **Insertar `pm32_entry`** entre VESA setup y `enter_long_mode`
4. **Modificar copia en 64-bit** para manejar 2 fuentes
5. **Probar con kernel de 2 MB+** (aumentar `MAX_PROCS` a 256 para verificar)

## Referencias

- ATA/ATAPI Specification (ANSI NCITS 317-1998)
- OSDev Wiki: [ATA PIO Mode](https://wiki.osdev.org/ATA_PIO_Mode)
- OSDev Wiki: [Protected Mode](https://wiki.osdev.org/Protected_Mode)
- `boot/stage2.asm` — código actual del bootloader
