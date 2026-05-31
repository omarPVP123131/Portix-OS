# Portix OS — Subsistema Gráfico

## Visión general

```
PortixBootInfo.framebuffer
  │
  └── fb.base → Linear Framebuffer (LFB) — VRAM hardware en dirección del BAR
        │
        └── Framebuffer (struct en kernel/src/graphics/driver/framebuffer.rs)
              ├── lfb          dirección física de la VRAM (p. ej. 0x01000000+)
              ├── backbuf      buffer software en 0x5000000
              ├── width        resolución horizontal (píxeles)
              ├── height       resolución vertical (píxeles)
              ├── lfb_pitch    bytes por línea en la VRAM
              ├── bpp          bits por píxel (32 en funcionamiento normal)
              ├── back_pitch   bytes por línea en el backbuffer (width × 4)
              └── dirty        DirtyRegion — zona marcada pendiente de blit
```

---

## Inicialización del framebuffer (orden de prioridad)

El kernel intenta obtener el framebuffer en este orden, usando el primero
que devuelva datos válidos:

**1. PortixBootInfo** — si `fb.flags & 1` está activo, se usan directamente
`base`, `width`, `height`, `pitch` y `bpp` de la estructura. Cubre las rutas
BIOS VESA y EFI GOP.

**2. Punteros heredados** — si no hay bootinfo, se leen las direcciones
hardcodeadas en `0x9004+` (escritas por `stage2.asm` en el arranque BIOS).

**3. Fallback PCI + init VBE** — si el bootinfo no tiene framebuffer (p. ej.
GOP fallido en OVMF/VirtualBox):
- `pci::pci_find_vga_framebuffer()` escanea PCI clase `0x0300` (controlador VGA)
- Selecciona el BAR correcto según vendor:
  - virtio-vga (1AF4) → BAR1, fallback BAR0
  - Bochs/QEMU (1234) → BAR0, fallback BAR1
  - VMware (15AD), VirtualBox (80EE) → BAR0
- Gestiona BARs MMIO de 32 y 64 bits
- Inicializa Bochs VBE a 1024×768×32 si no está activo

---

## Interfaz Bochs VBE DISPI

| Puerto | Dirección | Función              |
|--------|-----------|----------------------|
| 0x1CE  | Escritura | Índice de registro   |
| 0x1CF  | R/W       | Dato del registro    |

### Registros

| Índice | Nombre      | Descripción                        |
|--------|-------------|------------------------------------|
| 0      | ID          | Debe ser 0xB0C0–0xB0C4             |
| 1      | XRES        | Resolución horizontal              |
| 2      | YRES        | Resolución vertical                |
| 3      | BPP         | Bits por píxel                     |
| 4      | ENABLE      | 0x41 = habilitar + LFB             |
| 6      | VIRT\_WIDTH | Píxeles por línea (virtual stride) |

### Secuencia de init (`bochs_vbe_setup`)

```rust
vbe_outw(0, 0xB0C4);   // ID
vbe_outw(4, 0);         // deshabilitar
vbe_outw(1, 1024);      // XRES
vbe_outw(2, 768);       // YRES
vbe_outw(3, 32);        // BPP
vbe_outw(6, 1024);      // virtual width
vbe_outw(4, 0x41);      // habilitar + LFB
```

Si VBE ya estaba activo se leen los valores actuales sin reinicializar.

---

## Doble buffer

- **LFB**: VRAM hardware. Solo se escribe durante `present()`.
- **Backbuffer**: `0x5000000`. Todas las operaciones de píxel escriben aquí.
- **Alpha LUT**: tabla `ALPHA_LUT[256][256]` calculada al inicio para
  multiplicación de alpha sin divisiones en caliente (`alpha_mul()`).

### `present()` — volcado de región sucia

`present()` solo copia las filas y columnas marcadas en `DirtyRegion`.
Usa `rep movsd` (x86\_64) para velocidad máxima. Soporta BPP 16, 24 y 32.
`present_full()` fuerza el blit completo de toda la pantalla.

### Funciones principales de `Framebuffer`

| Función                  | Descripción                                        |
|--------------------------|----------------------------------------------------|
| `draw_pixel(x, y, c)`    | Escribe un píxel en el backbuffer                  |
| `fill_rect()`            | Relleno de rectángulo con `rep stosd`              |
| `fill_rounded()`         | Rectángulo con esquinas redondeadas                |
| `fill_rect_alpha_fast()` | Relleno con alpha blending vía LUT                 |
| `fill_gradient_dither()` | Degradado horizontal con dithering Bayer 4×4       |
| `draw_line()`            | Línea de Bresenham                                 |
| `fill_circle()`          | Círculo relleno (algoritmo del punto medio)        |
| `scroll_region_up()`     | Desplazamiento vertical de una región con `rep movsd` |
| `blit_sprite()`          | Copia de sprite con color-key transparente         |
| `present()`              | Vuelca región sucia → LFB                          |
| `present_full()`         | Blit forzado de toda la pantalla                   |
| `clear()`                | Limpia backbuffer con `rep stosd`                  |

---

## Sistema de Layout (proporcional)

Toda la geometría se calcula a partir de `(fw, fh)` — las dimensiones del
framebuffer. Sin constantes de resolución hardcodeadas. Compatible con
cualquier modo VESA.

```rust
Layout {
    fw, fh             // dimensiones del framebuffer
    header_h           // fh * 65 / 1000, mín 38, máx 60
    gold_h             // 4 px (línea separadora dorada)
    tab_h              // fh * 35 / 1000, mín 22, máx 32
    tab_y              // header_h + gold_h
    tab_w              // fw / 5
    content_y          // tab_y + tab_h
    status_h           // fh * 28 / 1000, mín 18, máx 24
    bottom_y           // fh - status_h
    pad                // fw / 80, mín 8, máx 18
    col_div            // fw * 5 / 12  (divisor de columna izquierda)
    right_x            // col_div + pad + 4
    line_h             // font_h + font_h / 2
    font_w = 8
    font_h = 8
}
```

---

## Offsets GOP (cargador UEFI)

Implementado en `boot/efi/src/main.rs` mediante acceso raw a punteros:

```
EFI_GRAPHICS_OUTPUT_PROTOCOL
  +0   QueryMode  (fn ptr)
  +8   SetMode    (fn ptr)
  +16  Blt        (fn ptr)
  +24  Mode       (*mut EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE)
         Mode:
           +0   MaxMode           (u32)
           +4   Mode              (u32)
           +8   Info              (*mut EFI_GRAPHICS_OUTPUT_MODE_INFORMATION)
           +16  SizeOfInfo        (usize)
           +24  FrameBufferBase   (u64)
           +32  FrameBufferSize   (u64)
           Info:
             +0   Version               (u32)
             +4   HorizontalResolution  (u32)
             +8   VerticalResolution    (u32)
             +12  PixelFormat           (u32)
             +32  PixelsPerScanline     (u32)
```

---

## Paleta de colores del sistema

| Constante       | Hex       | Uso                          |
|-----------------|-----------|------------------------------|
| `PORTIX_BG`     | `#01080F` | Fondo principal              |
| `PORTIX_PANEL`  | `#030C18` | Fondo del panel lateral      |
| `PORTIX_GOLD`   | `#FFD700` | Acentos dorados              |
| `PORTIX_AMBER`  | `#FFAA00` | Acento secundario            |
| `WHITE`         | `#FFFFFF` | Texto principal              |
| `GREEN`         | `#00CC44` | Éxito / OK                   |
| `RED`           | `#EE2222` | Error                        |
| `BLUE`          | `#0055FF` | Información                  |
| `YELLOW`        | `#FFFF00` | Advertencia                  |
| `CYAN`          | `#00CCEE` | Acento terciario             |
| `NEON_GREEN`    | `#00FF88` | Indicadores de actividad     |
| `MAGENTA`       | `#FF00CC` | Excepciones #GP              |

---

## Fuente

- Fuente bitmap 8×8 en `graphics/render/font.rs` (`FONT_8X8`)
- Cada glifo: 8 filas × 8 bits (1 byte por fila), cubre ASCII 32–127
- `write_at(s, x, y, color)` — texto a posición arbitraria
- `write_at_tall(s, x, y, color)` — texto en doble altura (caracteres 8×16)
- `draw_char_tall()` — renderiza cada carácter ocupando 2 líneas verticales
