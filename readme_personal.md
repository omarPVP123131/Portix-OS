# Portix OS — Notas de Desarrollo

**Versión actual:** `0.8.0`  
**Estado:** ✅ Arranque BIOS + UEFI dual-boot funcional  
**Lenguaje:** Rust (nightly) — freestanding, sin libc, sin Linux

---

## Tabla de contenidos

- [¿Qué es Portix?](#qué-es-portix)
- [Arquitectura de boot](#arquitectura-de-boot)
- [Drivers](#drivers)
- [Display](#display)
- [Build](#build)
- [Decisiones de diseño](#decisiones-de-diseño)
- [Changelog](#changelog)
- [Roadmap](#roadmap)

---

## ¿Qué es Portix?

Un sistema operativo desde cero en Rust. Kernel freestanding, sin libc,
sin Linux, sin andamios. Arranca en BIOS y UEFI — el logro central de la v0.8.0.

---

## Arquitectura de boot

```
┌─────────────────────────────────────────────────────────────┐
│  BIOS                                                       │
│  MBR → stage2 (32 KB) → VESA 1024×768 → kernel @ 0x200000 │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  UEFI                                                       │
│  GPT ESP → BOOTX64.EFI (Rust) → Block I/O FAT32            │
│         → PortixBootInfo → ExitBootServices                 │
│         → kernel @ 0x200000                                 │
└─────────────────────────────────────────────────────────────┘
```

El loader UEFI vive en `boot/efi/src/main.rs`. Rust puro sin bindings,
llama a la UEFI boot table directamente con `extern "efiapi"` (convención
Microsoft x64: RCX, RDX, R8, R9).

### GOP y PCI Fallback

OVMF en Windows no siempre expone el protocolo GOP. Para manejarlo de forma robusta:

1. El loader UEFI intenta obtener el framebuffer vía **GOP protocol**
2. Si GOP no está disponible → pasa `fb_ok=false` en `PortixBootInfo`
3. El kernel detecta `lfb == 0` y escanea **PCI** directamente
4. Si VBE no está activo → lo inicializa a **1024×768×32**

El display siempre funciona sin importar el firmware.

---

## Drivers

| Driver         | Estado     | Detalles                              |
|----------------|------------|---------------------------------------|
| ATA PIO        | ✅ Funcional | LBA48, sector cache                  |
| FAT32          | ✅ Funcional | Read/write, cluster chain            |
| PCI            | ✅ Funcional | Escaneo de bus completo              |
| PS/2 Teclado   | ✅ Funcional | IRQ1                                 |
| PS/2 Mouse     | ✅ Funcional | IRQ12                                |
| PIT            | ✅ Funcional | 100 Hz tick                          |
| Serial         | ✅ Funcional | COM1 38400 8N1, log levels           |

---

## Display

- Framebuffer vía `PortixBootInfo`, VESA, GOP o PCI+VBE fallback
- VBE forzado a **1024×768×32** si no está activo
- Doble buffer (backbuffer @ `0x100000`, dirty region blit)
- Layout proporcional — sin coordenadas de píxel hardcodeadas
- Alfa blending para rectángulos

### UI — 5 pestañas

| Pestaña    | Función                              |
|------------|--------------------------------------|
| System     | Monitoreo de heap y telemetría       |
| Terminal   | Consola + comandos de disco          |
| Devices    | Listado de dispositivos PCI/ATA      |
| IDE        | Editor de texto integrado (nano-like)|
| Explorer   | Navegador de archivos FAT32          |

---

## Build

```sh
# BIOS — genera imagen ISO El Torito
python scripts/build.py --mode=iso

# UEFI — GPT + ESP, requiere OVMF + pyfatfs
python scripts/build.py --mode=uefi

# Ambos modos en un solo paso
python scripts/build.py --mode=dual
```

La salida serial de debug aparece en la terminal vía `-serial stdio`.

---

## Decisiones de diseño

### ¿Por qué Rust freestanding?

El objetivo era un kernel sin ninguna capa externa. Sin libc implica control
total del layout de memoria, sin sorpresas en el heap, y el borrow checker
como red de seguridad para las estructuras del kernel.

### ¿Por qué `extern "efiapi"` en lugar de un crate UEFI?

Cero dependencias externas en el loader. UEFI usa la convención Microsoft x64
(distinta a System V AMD64), basta con declararla explícitamente en Rust.
Añadir un crate como `uefi-rs` habría complicado el linkado freestanding
sin beneficio real para este scope.

### ¿Por qué el doble buffer en `0x100000`?

La dirección `0x100000` (1 MiB) está por encima del primer megabyte
reservado por BIOS y por debajo del kernel en `0x200000`. Es espacio libre
conveniente y alineado que no requiere ningún allocator.

### ¿Por qué identity map en lugar de paginación virtual?

La paginación virtual es el siguiente gran paso (ver roadmap). Por ahora,
mantener el mapa de identidad del bootloader elimina complejidad y permite
iterar rápido sobre drivers y UI. El precio es que el kernel corre en ring 0
con acceso directo a memoria física — aceptable para esta etapa.

### ATA PIO con caché de sectores

Las lecturas ATA PIO son lentas (polling). La caché evita resets de bus
repetitivos y reduce la latencia en operaciones FAT32 con acceso secuencial
al cluster chain.

### Buddy system allocator

Se eligió buddy system por ser determinístico en latencia y simple de
implementar con listas intrusivas en Rust. El heap se monitorea en tiempo
real desde la pestaña System.

---

## Changelog

### v0.8.0 — UEFI + BIOS Dual Boot *(actual)*
- ✅ Loader UEFI completo en Rust puro (`extern "efiapi"`)
- ✅ GOP fallback → PCI + Bochs VBE init
- ✅ Generación de imagen `.iso` ISO9660 estable (`build.py --mode=iso`)
- ✅ Modo dual: BIOS y UEFI desde un mismo artefacto

### v0.7.7-stable
- ✅ Creación correcta de archivo `.iso` usando ISO9660 (El Torito)
- 🐛 Fix renderizado visual en el editor IDE
- 🐛 Fix combinaciones de teclas en el editor
- 🐛 Fix bug de terminal en el comando `nano`
- 🐛 Consolidar secuencia de boot de almacenamiento

### v0.7.6-beta
- ✅ Driver ATA con sistema de caché para evitar resets de bus
- ✅ Sistema de archivos FAT32 read/write con cluster chain
- ✅ VFS (Virtual File System) unificado
- ✅ Comandos de disco optimizados en consola
- ✅ IDT/ISR robustecidos — manejo avanzado de excepciones del CPU
- 🐛 Fix linker — estabilidad de `main` para evitar kernel panic

### v0.7.5
- ✅ Buddy system allocator con listas intrusivas
- ✅ Monitoreo de heap y telemetría en pestaña System
- ✅ Driver serial: niveles de log, volcado hexadecimal y self-test
- ✅ Rediseño de interfaz chrome — explorador e IDE robustecidos
- ✅ Alfa blending para rectángulos (`feat(graphics)`)
- ✅ Sistema de input unificado
- ✅ Refactorización de scripts de construcción

### v0.7.4
- ✅ Optimización del repositorio y blindaje de `.gitignore`

### v0.7.3
- 🐛 Fix driver PS/2 mouse

### v0.7.2
- ✅ Soporte de arranque para múltiples formatos

### v0.7.1
- ✅ Doble buffer de video (backbuffer + dirty region blit)

### v0.6
- ✅ Migración a VESA
- ✅ Drivers básicos (PIT, PS/2, Serial)

### v0.1
- 🌱 Primer experimento de kernel en Rust

---

## Roadmap

### Inmediato

- [ ] **Paginación virtual** — reemplazar identity map por page tables propias (4-level, x86_64)
- [ ] **Modo usuario / ring 3** — syscall interface, separación kernel/user space

### Medio plazo

- [ ] **SMP** — soporte para múltiples CPUs (APIC, spinlocks, per-CPU data)
- [ ] **USB** — reemplazar PS/2 (xHCI o EHCI)
- [ ] **TCP/IP** — stack de red básico (RTL8139 o virtio-net)

### Largo plazo

- [ ] **VirtIO-GPU** — aceleración 2D para QEMU
- [ ] **NVMe / AHCI** — reemplazar ATA PIO
- [ ] **Ext2/4** — sistema de archivos alternativo a FAT32

---

*Última actualización: v0.8.0 — dual boot BIOS + UEFI funcional.*