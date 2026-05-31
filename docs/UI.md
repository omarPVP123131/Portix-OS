# Portix OS — Interfaz de Usuario

## Layout

Toda la geometría se calcula a partir de las dimensiones del framebuffer
`(fw, fh)` en `graphics/driver/framebuffer.rs`. Sin valores de resolución
hardcodeados — la UI es completamente adaptable.

```rust
Layout {
    fw, fh             // dimensiones del framebuffer
    header_h           // barra de cabecera (proporcional a fh)
    gold_h             // línea separadora de 4 px
    tab_h              // altura de la barra de pestañas
    tab_y              // header_h + gold_h
    tab_w              // fw / 5  (ancho de cada pestaña)
    content_y          // tab_y + tab_h  (inicio del área de contenido)
    status_h           // barra de estado inferior
    bottom_y           // fh - status_h
    pad                // margen lateral (proporcional a fw)
    col_div            // divisor de columna izquierda (fw * 5 / 12)
    right_x            // col_div + pad + 4
    line_h             // font_h + font_h / 2
    font_w = 8
    font_h = 8
}
```

---

## Pestañas

| Tecla | Pestaña    | Descripción                                                         |
|-------|------------|---------------------------------------------------------------------|
| F1    | Sistema    | Versión, uptime, estadísticas del heap, CPU, disco, log serial     |
| F2    | Terminal   | Emulador de terminal completo con comandos                          |
| F3    | Dispositivos | Árbol de dispositivos PCI, unidades ATA, estado del ratón         |
| F4    | IDE        | Editor de código/texto integrado con soporte de ratón              |
| F5    | Explorador | Navegador de archivos FAT32                                         |
| Tab   | Ciclar     | Siguiente pestaña (sin Ctrl)                                        |

---

## Comandos del terminal

| Comando    | Descripción                                        |
|------------|----------------------------------------------------|
| `ayuda`    | Lista los comandos disponibles                     |
| `clear`    | Limpia la pantalla del terminal                    |
| `echo`     | Imprime los argumentos                             |
| `ls`       | Lista el contenido de un directorio                |
| `cat`      | Muestra el contenido de un archivo                 |
| `nano`     | Abre el editor de texto en línea de comandos       |
| `sysinfo`  | Información del sistema (CPU, RAM, disco)          |
| `meminfo`  | Estadísticas del buddy allocator                   |
| `pci`      | Lista los dispositivos PCI detectados              |
| `reboot`   | Reinicia el sistema                                |
| `apagado`  | Apaga el sistema vía ACPI (puerto 0x604)           |

> **`apagado`**: implementado en `drivers/bus/acpi.rs`. Envía el comando
> de apagado ACPI S5 escribiendo `0x2000` al puerto `0x604`
> (método compatible con QEMU).

---

## IDE — Editor integrado

- Pestañas de archivo, números de línea, barra de estado inferior
- Barra de menú: `Archivo`, `Editar`, `Ver`, `Ejecutar`, `Ayuda`
- Atajos de teclado: `Ctrl+S` guardar, `Ctrl+W` cerrar, `Ctrl+N` nuevo archivo
- Menús desplegables con detección de clic de ratón
- El estado del IDE (`IdeState`) se almacena en BSS estático (no en el stack)
  para evitar desbordamiento de pila con archivos grandes

---

## Explorador de archivos

- Árbol de directorios con lista de archivos
- Barra de herramientas con actualización y navegación
- Menú contextual al clic derecho (Abrir, Renombrar, Eliminar)
- Panel de ayuda alternado con botón `[?]`
- La selección de un archivo de código y la pulsación de Enter lo abre
  directamente en el IDE

---

## Fuente

- Fuente bitmap 8×8 en `graphics/render/font.rs` (`FONT_8X8`)
- Cada glifo: 8 filas de 8 bits (1 byte por fila), cubre ASCII 32–127
- `write_at(s, x, y, color)` — renderiza texto en cualquier posición (x, y)
- `write_at_tall(s, x, y, color)` — texto en doble altura (cada carácter
  ocupa 2 líneas verticales)

---

## Pipeline de renderizado

```
Loop principal (30 FPS):
  │
  ├── Poll PS/2 (bytes de teclado + ratón)
  ├── Procesado de cola de teclado
  │     └── Teclas de función, Terminal, IDE, Explorador
  ├── Procesado de cola de ratón
  │     └── Clic izquierdo, clic derecho, scroll, arrastre de scrollbar
  ├── needs_draw = true → renderizar:
  │     ├── draw_chrome()          // cabecera, línea dorada, barra de pestañas
  │     ├── draw_system_tab()      // o
  │     ├── draw_terminal_tab()    // o
  │     ├── draw_devices_tab()     // o
  │     ├── draw_ide_tab()         // o
  │     └── draw_explorer_tab()
  │           └── draw_cursor()   // cursor del ratón PS/2
  └── needs_present + tick de render → c.present()
        └── Vuelca región sucia → LFB hardware
```

El cursor parpadea a ~1 Hz (toggle cada 50 ticks a 100 Hz del PIT).

---

## Entrada

- **Teclado**: IRQ1 → byte PS/2 → `KeyboardState::feed_byte()` → enum `Key`
- **Ratón**: IRQ12 → `MouseState::feed()` → posición absoluta del cursor
- Ambos dispositivos se leen de forma unificada desde el puerto PS/2
  (`0x60`/`0x64`) al inicio de cada iteración del loop principal
- Sin soporte USB en esta versión

---

## Paleta de colores

| Constante       | Hex       | Uso                          |
|-----------------|-----------|------------------------------|
| `PORTIX_BG`     | `#01080F` | Fondo principal              |
| `PORTIX_PANEL`  | `#030C18` | Fondo del panel              |
| `PORTIX_GOLD`   | `#FFD700` | Acentos dorados              |
| `PORTIX_AMBER`  | `#FFAA00` | Acento secundario            |
| `WHITE`         | `#FFFFFF` | Texto principal              |
| `GREEN`         | `#00CC44` | Éxito / OK                   |
| `RED`           | `#EE2222` | Error                        |
| `BLUE`          | `#0055FF` | Información                  |
| `YELLOW`        | `#FFFF00` | Advertencia                  |

---

## Trabajo futuro

- Escalado de fuente (8×8 → 16×16, TrueType)
- Gestor de ventanas (mover/redimensionar)
- Renderizado acelerado por GPU (virtio-gpu, VMware SVGA)
- Transiciones animadas entre pestañas
- Soporte de portapapeles
- Soporte USB (teclado/ratón HID)
