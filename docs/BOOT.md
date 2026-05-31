# Portix OS — Cadena de Arranque

## Visión general

Portix soporta dos rutas de arranque. Ambas producen la misma estructura
`PortixBootInfo` en la dirección física `0x600000` y saltan al kernel en
`0x200000`.

| Ruta  | Imagen                          | Cargador                       | Firmware                      |
|-------|---------------------------------|--------------------------------|-------------------------------|
| BIOS  | `portix.iso` (ISO9660 El Torito)| `stage2.asm`                   | BIOS legacy vía INT 13h       |
| BIOS  | `portix.img` (MBR raw)          | `boot.asm` + `stage2.asm`      | BIOS legacy vía INT 13h       |
| UEFI  | `portix-uefi.img` (GPT + ESP)   | `BOOTX64.EFI` (Rust)           | UEFI vía OVMF / hardware real |
| Mixta | `portix-dual.iso` (híbrida)     | `stage2.asm` + `BOOTX64.EFI`   | BIOS El Torito + UEFI El Torito|

---

## Arranque BIOS (ISO / MBR)

### `boot.asm` (LBA 0)

- MBR estándar con tabla de particiones
- Carga stage2 desde LBA 1–67 a `0x7E00` vía INT 13h/42h
- Salta a `0x7E00`

### `stage2.asm` (32 KB)

- **Detección de CD**: usa INT 13h/48h para detectar CD/DVD (sectores de
  2 048 bytes). Copia hacia atrás de `0x7E00` a `0x8000` al arrancar
  desde El Torito.
- **Carga del kernel**: lee `kernel.bin` a `0x200000` vía INT 13h/42h (LBA48).
- **Puerta A20**: habilitada vía puerto `0x92`.
- **VESA**: INT 10h/4F01 (info de modo), INT 10h/4F02 (seleccionar modo
  1024×768×32). Fallback a 640×480×32 si el modo deseado no está disponible.
- **E820**: INT 15h/E820 para mapa de memoria en `0x9100`.
- **BootInfo**: escribe `PortixBootInfo` en `0x600000` con magic
  `0x50525458424F4F54` ("PRTXBOOT").
- **Salto**: far jump al kernel en `0x200000` (RDI = `0x600000`).

---

## Arranque UEFI

### Cargador UEFI (`boot/efi/src/main.rs`)

Compilado con:
```
cargo +nightly build --release -Z build-std=core --target x86_64-unknown-uefi
```
Usa la convención de llamada `extern "efiapi"` (Microsoft x64: RCX, RDX, R8, R9).

**Secuencia**:

1. `LocateHandleBuffer(312)` → buscar handles con protocolo GOP
2. Fallback a `ConsoleOutHandle` (`SystemTable + 0x38`) si no hay handles
3. `OpenProtocol(280)` → abrir GOP en el handle candidato
4. Leer modo GOP: `interface+8 → mode; mode+24 → fb_base; mode+32 → fb_size`
5. Leer info de modo: `mode+8 → info; info+4 → width; info+12 → pixel_format; info+32 → pixels_per_scanline`
6. Abrir Block I/O en el handle del dispositivo → leer partición FAT32
7. Parsear VBR FAT32 → recorrer clústeres hasta `/PORTIX/KERNEL.BIN`
8. Reservar páginas en `0x200000` vía `AllocatePages(40)`
9. Leer clústeres del kernel en memoria
10. Construir `PortixBootInfo` en `0x600000`
11. `ExitBootServices(232)` → saltar al kernel

**Si GOP no está disponible** (OVMF en Windows): el cargador EFI establece
`fb_ok = false`. El fallback PCI + VBE del kernel se encarga de descubrir el
framebuffer.

### Offsets de Boot Services (UEFI x86\_64)

| Offset | Servicio             |
|--------|----------------------|
| 40     | AllocatePages        |
| 56     | GetMemoryMap         |
| 64     | AllocatePool         |
| 72     | FreePool             |
| 280    | OpenProtocol         |
| 312    | LocateHandleBuffer   |

---

## Formato de PortixBootInfo

Ubicada en `0x600000`, tamaño `0x1A00` (6 656 bytes). Validada por el magic
`0x50525458424F4F54` y un checksum de 32 bits (suma de todas las palabras
de la cabecera debe ser 0).

| Offset | Tamaño | Campo               | Descripción                                |
|--------|--------|---------------------|--------------------------------------------|
| 0x00   | 8      | `magic`             | `0x50525458424F4F54` ("PRTXBOOT")          |
| 0x08   | 4      | `abi_version`       | 1                                          |
| 0x0C   | 4      | `arch`              | 1 = x86\_64                                |
| 0x10   | 4      | `endian`            | 1 = little-endian                          |
| 0x14   | 4      | `header_size`       | Tamaño de la cabecera                      |
| 0x18   | 4      | `total_size`        | Tamaño total de la estructura              |
| 0x1C   | 4      | `checksum`          | Checksum 32 bits (suma = 0)                |
| 0x20   | 8      | `flags`             | bit 0 = fb válido, bit 1 = mapa de mem.   |
| 0x40   | 4      | `boot_source`       | 1 = BIOS, 2 = UEFI                         |
| 0x44   | 4      | `boot_protocol`     | 1 = stage2, 2 = UEFI native               |
| 0x50   | 8      | `fb.base`           | Dirección física del framebuffer           |
| 0x58   | 8      | `fb.size`           | Tamaño del framebuffer en bytes            |
| 0x60   | 4      | `fb.width`          | Resolución horizontal                      |
| 0x64   | 4      | `fb.height`         | Resolución vertical                        |
| 0x68   | 4      | `fb.pitch`          | Bytes por línea de pantalla                |
| 0x6C   | 4      | `fb.bpp`            | 32 (bits por píxel)                        |
| 0x70   | 4      | `fb.source`         | 1 = VESA, 2 = EFI GOP                     |
| 0x74   | 4      | `fb.canonical_format`| 1 = XRGB8888                             |
| 0x98   | 8      | `kernel_base`       | `0x200000`                                 |
| 0xA0   | 8      | `kernel_size`       | Tamaño del kernel en bytes                 |
| 0xA8+  | var.   | `memory_map`        | Array de `PortixMemoryRegion` (máx. 128)  |
| 0x1900+| var.   | `reserved_ranges`   | Array de `ReservedRange` (máx. 64)        |
| 0x1A00+| var.   | `firmware_tables`   | Array de `FirmwareTableEntry` (máx. 16)  |

---

## Problemas conocidos

### VirtualBox BIOS — cursor parpadeante tras "S2 v9.8 OK"

**Síntoma**: VBox BIOS arranca la ISO, muestra "S2 v9.8 OK" / "CD NATIVE" /
"Kernel OK", luego la pantalla muestra un cursor parpadeante. Sin salida por
framebuffer.

**Causa raíz**: `stage2.asm` → `do_cdrom_load` lee el kernel desde LBA de CD
`(base_lba + KERNEL_LBA) / 4 = 17` (sectores CD de 2 048 bytes). En
QEMU/SeaBIOS la unidad virtual El Torito mapea LBA 0 al inicio de la imagen,
así que LBA 17 = byte 34 816 = datos del kernel. En VBox BIOS (basado en Bochs
BIOS) la unidad virtual usa LBAs absolutos de CD o sectores de 512 bytes, por
lo que stage2 lee datos incorrectos → basura/crash → el kernel nunca entra en
modo largo.

**Corrección necesaria** (`boot/stage2.asm`): en `detect_cdrom`, leer el LBA
de la imagen de arranque BIT de `[0x7DFC]` (parcheado por xorriso con
`-boot-info-table`) y sumarlo a `base_lba`. Alternativamente, en
`do_cdrom_load`, usar el LBA absoluto de CD directamente:
`CD_LBA = BIT_resp + KERNEL_LBA / 4`.

**Solución temporal**: usar `portix-dual.iso` con UEFI habilitado en VBox
(Configuración → Sistema → Placa base → Habilitar EFI). La ISO híbrida contiene
entradas El Torito tanto para BIOS como para UEFI; el firmware UEFI de VBox
arrancará el cargador EFI correctamente.

### VirtualBox UEFI — "Not Found" al arrancar

**Síntoma**: VBox UEFI intenta arrancar desde CD pero muestra "Not Found".

**Causa raíz**: flags incorrectos de xorriso en `create_uefi_iso()`. Corregido
en v0.8.0: se usan `-e esp.img -no-emul-boot -isohybrid-gpt-basdat
--efi-boot-part` en lugar de `--efi-boot`.
