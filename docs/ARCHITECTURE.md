# Portix OS — Arquitectura


## Objetivo del sistema

| Campo         | Valor                                        |
|---------------|----------------------------------------------|
| Arquitectura  | x86\_64 (IA-32e, modo largo de 64 bits)      |
| Arranque      | BIOS (ISO El Torito) + UEFI (GPT + ESP)      |
| Lenguaje      | Rust nightly, `no_std`, `no_main`            |
| Enlazador     | `rust-lld` + script personalizado `linker.ld`|
| Build         | Python (`build.py` v1.0.0)                     |

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
    serial.rs         COM1 38400 8N1, niveles de log
    mod.rs
    bus/
      acpi.rs         Soporte básico ACPI
      pci.rs          Enumeración PCI
      mod.rs
    input/
      keyboard.rs     Driver teclado PS/2 (IRQ1)
      mouse.rs        Driver ratón PS/2 (IRQ12)
      mod.rs
    storage/
      ata.rs          ATA PIO LBA48
      fat32.rs        Lectura/escritura FAT32
      fat_adaptador.rs Adaptador FAT solo lectura (/boot)
      ext2.rs         ext2 RW (raíz /)
      ramfs_adaptador.rs Adaptador ramfs
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

  fs/
    mod.rs            Contrato SistemaDeFicheros + errno
    ext2.rs           ext2 RW (bitmaps, indirectos, directorios)
    ramfs_adaptador.rs Adaptador ramfs
    fat_adaptador.rs  Adaptador FAT solo lectura

  mem/
    allocator.rs      Buddy allocator (64 MiB, órdenes 0-11)
    marcos.rs         Asignador de marcos físicos (mapa de bits, E820/UEFI)
    paging.rs         Paginación 4 niveles, COW, crecer_pila
    mod.rs

  net/
    mod.rs            Ethernet, ARP, IPv4, ICMP, loopback
    tcp.rs            TCP (retransmisión, SACK, ventana, RTO)
    ipv6.rs           IPv6 (SLAAC, NDP, ICMPv6)
    socket.rs         Sockets (AF_INET, AF_INET6, AF_UNIX, socketpair)
    mod.rs

  process/
    mod.rs            Procesos, fork, exec, exit, pipes, FIFOs
    types.rs          Process, ProcessState, FdType, etc.
    signals.rs        Señales POSIX (sigaction, sigreturn, entrega)
    scheduler.rs      Planificador, SMP, wait_hlt_preemptible
    mod.rs

  syscall/
    mod.rs            Despachador nativo + traductor ABI Linux
    futex.rs          Futex real con cola de espera
    numbers.rs        Números de syscall
    types.rs          Tipos ABI

  system/
    devsuite.rs       Trazador TSC, profiler, symbols
    consagracion.rs   ABI freeze, certificación bare-metal
    subsystem_suite.rs Sumarios de subsistemas
    symbols.rs        Símbolos ELF por proceso
    mod.rs

  arch/
    smp.rs            SMP real (RSDP, MADT, IPI)
    percpu.rs         Estado por CPU (gs:)
    debug_regs.rs     DR0-DR3, breakpoints HW
    isr_handlers.rs   Manejadores de excepciones
    isr.asm           Stubs ISR en NASM
    mod.rs

  time/
    pit.rs            Temporizador PIT a 100 Hz
    rtc.rs            RTC (CMOS)
    mod.rs

  ui/
    (compositor de ventanas, terminal gráfica, verificación)

  console/
    terminal/         Emulador de terminal, comandos
    mod.rs

  util/
    fmt.rs            Formateo numérico
    mod.rs

  bootinfo.rs         Struct PortixBootInfo (parser + validación)
  main.rs             Punto de entrada del kernel, loop principal
  version.rs          Versión 1.0.0
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
| **`0xE00000`**          | **PortixBootInfo** (magic + checksum)              |
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
Estructura a `0xE00000` (BIOS) producida tanto por el stage2 BIOS como por el cargador
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

---

## Extensiones Arquitectónicas Modernas (v0.9.5)

### 1. Pila de Red Dual IPv4/IPv6, WireGuard VPN & eBPF/XDP
- **IPv6 y NDP**: Cabeceras fijas de 40 bytes (`0x86DD`), generación EUI-64 link-local (`fe80::`), Neighbor Discovery Protocol (NDP) con resolución de direcciones físicas por solicitudes/anuncios ICMPv6 y tabla de rutas con gateway por defecto.
- **WireGuard VPN**: Protocolo de túnel seguro implementado en Rust puro `no_std`. Emplea Curve25519 (X25519) para el intercambio de claves efímeras bajo el Noise Protocol Framework, ChaCha20-Poly1305 AEAD para encapsulación de paquetes y protección anti-replay mediante contadores de secuencia de 64 bits.
- **eBPF / XDP L2 Engine**: Gancho de procesamiento ultrarrápido a velocidad de línea en la capa de enlace del controlador Intel e1000. Descarte (`XDP_DROP`) y reenvío (`XDP_TX`/`XDP_PASS`) en cero copias de memoria para mitigación inmediata de ataques de denegación de servicio.

### 2. Pila Gráfica Vulkan 1.3 ICD, SPIR-V & Efectos Wayland
- **Vulkan 1.3 ICD**: Driver cliente instalable conforme a Khronos con soporte para pipelines gráficos, swapchains KHR, enumeración de dispositivos VirtIO-GPU Venus 3D y rasterizador multi-core de CPU.
- **Validador SPIR-V**: Parseo e interpretación de flujos de bytecode SPIR-V 1.3 (`0x07230203`), mapeando instrucciones de transformación matricial (`OpMatrixTimesVector`) y modelos de memoria.
- **Efectos Wayland**: Filtro de convolución 1D separable para Desenfoque Gaussiano (kernel 5x5), sombras proyectadas paramétricas en ventanas `xdg_toplevel`, y animaciones fluidas a 60 FPS con curva cúbica ease-in-out.

### 3. Microkernel Modular con Fast-IPC de Capacidades (<100 ciclos)
- **Seguridad por Capacidades**: Estructura *CSpace* con badges criptográficos inviolables y máscara de derechos de acceso (`CAP_READ`, `CAP_WRITE`, `CAP_CALL`, `CAP_GRANT`).
- **Fast-IPC en Registros**: Transferencia directa de mensajes síncronos (Rendezvous) utilizando registros de CPU (`RDX`, `RSI`, `RDI`, `R8`, `R9`, `R10`) completando llamadas en ~78 ciclos de reloj.
- **Servicios Aislados en Ring-3 y Auto-sanación**: Drivers y servicios (`vfs-server`, `net-server`, `audio-server`, `display-server`) confinados con reinicio en caliente (*Crash & Respawn*) transparente.

### 4. Autohospedaje (Self-Hosting) y Gestor Tiling Sway/i3
- **Self-Hosting Soberano**: Cadena de compilación y enlace nativa dentro de Portix OS (`/bin/portix-cc`, `/bin/ld-portix`, `/bin/ar`) capaz de generar ejecutables ELF64 en el VFS de manera totalmente autónoma.
- **Gestor Tiling BSP (Sway/i3)**: Partición espacial binaria automática sin solapamiento (*zero overlap*), soporte multi-workspace y barra integrada *Portix-Bar* con telemetría en tiempo real.

