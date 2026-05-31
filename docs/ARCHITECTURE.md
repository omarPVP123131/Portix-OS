# Portix OS — Arquitectura

## Objetivo del sistema

| Campo         | Valor                                        |
|---------------|----------------------------------------------|
| Arquitectura  | x86\_64 (IA-32e, modo largo de 64 bits)      |
| Arranque      | BIOS (ISO El Torito) + UEFI (GPT + ESP)      |
| Lenguaje      | Rust nightly, `no_std`, `no_main`            |
| Enlazador     | `rust-lld` + script personalizado `linker.ld`|
| Build         | Python (`build.py` v5.0)                     |

### Especificación del target del kernel (`x86_64-portix.json`)

```json
{
  "llvm-target": "x86_64-unknown-none-elf",
  "panic-strategy": "abort",
  "disable-redzone": true,
  "features": "-mmx,-sse,+soft-float",
  "pre-link-args": {"ld.lld": ["-Tlinker.ld", "-n", "--gc-sections"]}
}
```

> **Nota sobre SSE**: el target deshabilita MMX y SSE (`-mmx,-sse`) y activa
> aritmética de punto flotante por software (`+soft-float`). La función
> `init_cpu_features()` configura `EM=0` y `MP=1` en CR0 y habilita
> `OSFXSR`/`OSXMMEXCPT` en CR4 para permitir `FXSAVE`/`FXRSTOR`, pero
> **no** habilita instrucciones SSE en el kernel.

---

## Árbol de módulos

```
kernel/src/
  arch/
    hardware.rs       Detección de CPU (CPUID), RAM, ATA IDENTIFY, display
    idt.rs            Entradas IDT (vectores 0-19 + IRQ 0-15), recarga GDT
    isr.asm           Stubs ISR en NASM elf64; double fault usa IST1
    isr_handlers.rs   Manejadores de excepciones, pantalla de pánico gráfica
    halt.rs           halt_loop()
    mod.rs

  drivers/
    serial.rs         COM1 38400 8N1, niveles de log, write_usize/hex
    mod.rs
    bus/
      acpi.rs         Soporte básico ACPI (apagado vía puerto 0x604)
      pci.rs          Enumeración PCI por puertos CF8/CFC, búsqueda de BAR VGA
      mod.rs
    input/
      keyboard.rs     Driver teclado PS/2 (IRQ1)
      mouse.rs        Driver ratón PS/2 (IRQ12)
      mod.rs
    storage/
      ata.rs          ATA PIO LBA48 con caché de sectores
      fat32.rs        Lectura/escritura FAT32, cadena de clústeres
      mkfs.rs         Formato FAT32 en primer arranque
      vfs.rs          Trait Virtual File System
      mod.rs

  graphics/
    driver/
      framebuffer.rs  LFB + backbuffer, init VBE, Layout, Color, Console
      vga.rs          Escritura directa en modo texto VGA (0xB8000)
      mod.rs
    render/
      font.rs         Fuente bitmap 8×8 (FONT_8X8)
      mod.rs
    mod.rs

  mem/
    allocator.rs      Buddy allocator con listas libres intrusivas
    mod.rs

  time/
    pit.rs            Temporizador PIT a 100 Hz
    mod.rs

  ui/
    chrome.rs         Barra de pestañas, panel lateral, dibuja todas las pestañas
    exception.rs      Primitivas gráficas para pantallas de error
    input.rs          Enrutamiento de entrada
    mod.rs
    tabs/
      system.rs       Pestaña Sistema
      terminal.rs     Pestaña Terminal
      devices.rs      Pestaña Dispositivos
      ide.rs          Editor de código integrado
      explorer.rs     Explorador de archivos
      mod.rs

  console/
    terminal/
      terminal.rs     Emulador de terminal con historial y scrollback
      editor.rs       Editor de texto en línea de comandos (nano)
      fmt.rs          Formateo interno del terminal
      mod.rs
      commands/
        system.rs     Comandos: ayuda, sysinfo, meminfo, pci, reboot, apagado
        disk.rs       Comandos: ls, cat, nano
        debug.rs      Comandos de depuración
        convert.rs    Comandos de conversión
        fun.rs        Comandos recreativos
        mod.rs
    mod.rs

  util/
    fmt.rs            fmt_u32(), fmt_hex() — formateo numérico sin std
    mod.rs

  bootinfo.rs         Struct PortixBootInfo (parser + validación)
  main.rs             Punto de entrada del kernel, loop principal
```

---

## Secuencia de arranque (lado del kernel)

```
_start  (entrada, RDI = dirección de BootInfo)
  cli, cld
  Guardar RDI → R12
  RSP = __stack_top  (LEA relativa a RIP, segura con PIE)
  Poner a cero BSS (__bss_start → __bss_end con rep stosb)
  Llamar rust_main(RDI)

rust_main(boot_info):
  bootinfo::init(boot_info)       // parsear y validar PortixBootInfo
  init_cpu_features()             // CR0: EM=0, MP=1 | CR4: OSFXSR, OSXMMEXCPT
  arch::idt::init_idt()           // GDT + TSS + IDT, remapeo PIC, STI
  ALLOCATOR.init()                // Buddy allocator en heap @ 0x500000
  init_page_pool()                // Pool de páginas para el IDE
  serial::init()                  // COM1, test de loopback
  time::pit::init()               // PIT a 100 Hz
  HardwareInfo::detect_all()      // CPUID, RAM, ATA IDENTIFY, display
  PciBus::scan()                  // Escaneo completo del bus PCI
  Console::new()                  // Init framebuffer (BIOS/GOP/PCI+VBE)
  Layout::new()                   // Geometría de UI proporcional a la resolución
  KeyboardState::new()            // Driver teclado PS/2
  MouseState::new() + init()      // Driver ratón PS/2
  Terminal::new()                 // Emulador de terminal
  IdeState::new()                 // Editor IDE (en BSS, no en stack)
  ExplorerState::new()            // Explorador (en BSS)
  AtaBus::scan() → FAT32::mount() // Detección ATA y montaje FAT32
  Loop principal  (render @ 30 FPS, poll PS/2, despacho de entrada)
```

---

## Flujo de interrupciones

```
Excepción de CPU / IRQ
  └→ stub en isr.asm
       ├ Guarda registros en crash_frame
       ├ Establece crash_frame.valid = 1
       └→ isr_handlers.rs — manejador específico
            ├→ Comprueba crash_frame.valid
            │    0 → fallback texto VGA en 0xB8000 + hlt
            │        (evita cascada #GP → #DF)
            │    1 → despacho normal
            ├→ #DE (0x00) → pantalla "divide by zero"
            ├→ #UD (0x06) → pantalla "invalid opcode"
            ├→ #BR (0x05) → pantalla "bound range"
            ├→ #PF (0x0E) → pantalla page fault (CR2, bits del error code)
            ├→ #GP (0x0D) → pantalla general protection (análisis de selector)
            ├→ #DF (0x08) → double fault vía IST1, solo texto VGA
            ├→ IRQ0 (PIT) → timer_tick + EOI (out 0x20, 0x20)
            ├→ IRQ1       → byte PS/2 → KeyboardState
            ├→ IRQ12      → byte PS/2 → MouseState
            └→ resto      → pantalla de excepción genérica
```

> **Guardia `crash_frame.valid`**: si el frame no fue capturado (stack
> potencialmente corrupto o framebuffer no accesible), todos los ISR caen al
> buffer de texto VGA en `0xB8000`, rompiendo la cadena #GP→#GP→#DF.
> El #DF usa además un stack dedicado de 16 KB vía IST1.

---

## Mapa de memoria física

| Rango                   | Propósito                                          |
|-------------------------|----------------------------------------------------|
| `0x000000–0x00FFFF`     | IVT, BDA, EBDA (primer 1 MB — low memory)          |
| `0x010000–0x01FFFF`     | Datos del cargador BIOS / stack stage2             |
| `0x100000–0x101FFF`     | Datos/stack del cargador EFI (modo UEFI)           |
| **`0x200000`**          | **Kernel** (`.text`, `.rodata`, `.data`, `.bss`)   |
| `0x500000`              | Heap — buddy allocator                             |
| **`0x600000`**          | **PortixBootInfo** (6 656 bytes, magic + checksum) |
| **`0x5000000`**         | **Backbuffer** (copia software del framebuffer)    |
| `0x1000000+` (variable) | Framebuffer LFB — dirección del BAR de VRAM        |

> El mapa de páginas es el identity map dejado por el firmware/cargador.
> Todas las direcciones físicas son iguales a las virtuales. No hay gestión
> propia de tablas de páginas en esta versión.

---

## Decisiones de diseño principales

**1. Sin tablas de página propias en arranque.**
El kernel usa el identity map dejado por el firmware. Dirección física = virtual
en todo momento. Trabajo futuro: espacio de usuario con aislamiento de páginas.

**2. Framebuffer de doble buffer.**
El backbuffer vive en `0x5000000`. Todas las operaciones de píxel escriben ahí.
`present()` solo vuelca las regiones marcadas como sucias (`DirtyRegion`) al LFB
hardware, usando `rep movsd` para velocidad máxima. Sin tearing, permite
composición por software.

**3. Buddy allocator con listas libres intrusivas.**
Los bloques libres almacenan sus punteros `next`/`prev` en su propia carga útil.
Bloque mínimo: 64 bytes (`MIN_ORDER`). Pool fijo en `~0x500000`. Expone
contadores atómicos en `ALLOC_STATS` visibles desde la UI sin bloqueo.

**4. PortixBootInfo estandarizado.**
Estructura a `0x600000` producida tanto por el stage2 BIOS como por el cargador
UEFI. Contiene: framebuffer, mapa de memoria, rangos reservados y tablas de
firmware. Validado por magic `0x50525458424F4F54` ("PRTXBOOT") y checksum de 32
bits. Permite que el kernel sea agnóstico al método de arranque.

**5. Debug solo por serial.**
Todos los diagnósticos de arranque van a COM1 (`0x3F8`) a 38 400 baudios. El
framebuffer solo se inicializa después de que serial esté listo.

**6. Guardia `CrashFrame.valid`.**
Cada ISR comprueba `crash_frame.valid` antes de llamar a `Console::new()`.
Si `valid == 0` (frame no capturado o framebuffer inaccesible), cae al buffer
de texto VGA en `0xB8000`, impidiendo la cascada #GP → #DF.
