# ⚡ Guía de Arquitectura y Portabilidad Nativa ARM64 (AArch64) — Portix OS v4.0 'Cosmos'

> **Fecha de Publicación**: Septiembre de 2026  
> **Versión del Sistema**: Portix OS v4.0.0 "Cosmos"  
> **Arquitectura**: ARMv8-A / ARMv9-A (AArch64 de 64 bits)  
> **Plataformas Objetivo**: QEMU Virt, Raspberry Pi 5 y Apple Silicon (M1/M2/M3)  

---

## 📑 Índice de Contenidos

1. [Visión General de la Arquitectura AArch64](#1-visión-general-de-la-arquitectura-aarch64)
2. [Modelo de Excepción y Aislamiento de Privilegios](#2-modelo-de-excepción-y-aislamiento-de-privilegios)
3. [Unidad de Gestión de Memoria (MMU de 4 Niveles)](#3-unidad-de-gestión-de-memoria-mmu-de-4-niveles)
4. [Controlador Genérico de Interrupciones (GICv2 / GICv3)](#4-controlador-genérico-de-interrupciones-gicv2--gicv3)
5. [Interfaz Binaria de Llamadas al Sistema (AArch64 ABI)](#5-interfaz-binaria-de-llamadas-al-sistema-aarch64-abi)
6. [Plataformas de Referencia y Arranque](#6-plataformas-de-referencia-y-arranque)
   - [A. QEMU Virt (Cortex-A76 / A72)](#a-qemu-virt-cortex-a76--a72)
   - [B. Raspberry Pi 5 (Broadcom BCM2712)](#b-raspberry-pi-5-broadcom-bcm2712)
   - [C. Apple Silicon M1 / M2 / M3 (vía Asahi m1n1)](#c-apple-silicon-m1--m2--m3-vía-asahi-m1n1)
   - [D. Arranque UEFI Estándar (BOOTAA64.EFI)](#d-arranque-uefi-estándar-bootaa64efi)
7. [Compilación Cruzada y Verificación](#7-compilación-cruzada-y-verificación)

---

## 1. Visión General de la Arquitectura AArch64

Portix OS extiende su microkernel soberano desde x86_64 hacia **ARM64 (AArch64)** manteniendo una paridad arquitectónica total:
- **Cero dependencias externas y 100% Rust (`no_std`)**: el núcleo y los controladores de periféricos ARM se compilan de forma limpia sin código C ni ensamblador inline innecesario.
- **Banco de Registros Ortogonal**: 31 registros de propósito general de 64 bits (`X0` a `X30`), donde `X29` actúa como puntero de marco de pila (*Frame Pointer*) y `X30` como registro de retorno de llamada (*Link Register* `LR`).
- **Punteros de Pila Dedicados**: separación estricta entre `SP_EL0` (espacio de usuario) y `SP_EL1` (espacio de kernel).

---

## 2. Modelo de Excepción y Aislamiento de Privilegios

La arquitectura ARMv8-A implementa cuatro Niveles de Excepción (*Exception Levels*):

```
┌────────────────────────────────────────────────────────┐
│ EL0 — Espacio de Usuario (Ring 3 equivalente)           │
│       Binarios ELF64, Servidor Wayland, portix-ai      │
├────────────────────────────────────────────────────────┤
│ EL1 — Espacio de Kernel (Ring 0 equivalente)           │
│       Microkernel Portix, Planificador CFS, Drivers    │
├────────────────────────────────────────────────────────┤
│ EL2 — Hipervisor y Virtualización                       │
│       MicroVMs Instantáneas de Portix OS               │
├────────────────────────────────────────────────────────┤
│ EL3 — Monitor Seguro / TrustZone                       │
│       Raíz de Confianza Criptográfica y TPM            │
└────────────────────────────────────────────────────────┘
```

El kernel de Portix toma el control en **EL1h** (usando la pila `SP_EL1`), mientras que las aplicaciones de usuario se despachan en **EL0t**. Las transiciones se efectúan mediante la instrucción `SVC #0` para llamadas al sistema y la instrucción `ERET` para el retorno atómico de interrupciones y excepciones.

---

## 3. Unidad de Gestión de Memoria (MMU de 4 Niveles)

Portix OS configura la MMU de AArch64 con direccionamiento virtual de 48 bits y tamaño de gránulo de página de 4 KB (esquema L0 → L1 → L2 → L3):

- **Registro TCR_EL1 (Translation Control Register)**:
  - `T0SZ = 16`: Espacio virtual inferior para espacio de usuario (`0x0000_0000_0000_0000` a `0x0000_FFFF_FFFF_FFFF`).
  - `T1SZ = 16`: Espacio virtual superior para kernel (`0xFFFF_0000_0000_0000` a `0xFFFF_FFFF_FFFF_FFFF`).
  - `TG0 = 0b00`, `TG1 = 0b10`: Gránulo de 4 KB en ambos espacios.
- **Registro MAIR_EL1 (Memory Attribute Indirection Register)**:
  - `Attr 0 (0x44)`: Memoria Normal, No-Cacheable (para buffers de DMA y video).
  - `Attr 1 (0xFF)`: Memoria Normal, Write-Back Cacheable (para código, datos y heap).
  - `Attr 2 (0x04)`: Memoria de Dispositivo `Device-nGnRE` (para registros MMIO de periféricos).

---

## 4. Controlador Genérico de Interrupciones (GICv2 / GICv3)

El soporte de interrupciones se gestiona a través del controlador canónico de ARM:
- **SGI (Software Generated Interrupts - IRQ 0 a 15)**: Utilizadas para IPIs (*Inter-Processor Interrupts*) en el planificador multinúcleo SMP.
- **PPI (Private Peripheral Interrupts - IRQ 16 a 31)**: Reservadas para temporizadores locales de CPU (ARM Generic Timer, IRQ 30).
- **SPI (Shared Peripheral Interrupts - IRQ 32 a 1019)**: Asignadas a periféricos compartidos como UARTs PL011, controladoras PCIe y tarjetas de red.

---

## 5. Interfaz Binaria de Llamadas al Sistema (AArch64 ABI)

A diferencia de x86_64 (donde el número de syscall se pasa en `RAX` y se invoca con `syscall`), en AArch64 se sigue el estándar POSIX/Linux para ARM64:
- **Número de llamada al sistema**: Se carga en el registro **`X8`**.
- **Parámetros de entrada**: Se transmiten en los registros **`X0`, `X1`, `X2`, `X3`, `X4`, `X5`**.
- **Valor de retorno**: Se devuelve en el registro **`X0`**.
- **Instrucción de disparo**: **`SVC #0`** (*Supervisor Call*).
- **Decodificación en Kernel**: El registro `ESR_EL1` (*Exception Syndrome Register*) reporta `EC = 0x15` indicando ejecución de instrucción `SVC` en estado AArch64.

---

## 6. Plataformas de Referencia y Arranque

### A. QEMU Virt (Cortex-A76 / A72)
Arranque para desarrollo rápido sin hardware físico:
```bash
qemu-system-aarch64 \
  -M virt \
  -cpu cortex-a76 \
  -smp 4 \
  -m 1G \
  -kernel build/dist/portix-aarch64.img \
  -serial stdio
```

### B. Raspberry Pi 5 (Broadcom BCM2712)
El procesador BCM2712 incluye cuatro núcleos ARM Cortex-A76 a 2.4 GHz y el puente de E/S RP1.
1. Formatea una tarjeta MicroSD en FAT32 con tabla MBR.
2. Copia la imagen `portix-rpi5-sd.img` directamente a la tarjeta SD mediante `dd`:
   ```bash
   sudo dd if=build/dist/portix-rpi5-sd.img of=/dev/sdX bs=4M conv=fsync
   ```
3. El archivo `config.txt` precargado configura `arm_64bit=1`, `kernel=kernel8.img`, activa la consola serie en GPIO 14/15 a 115200 baudios y habilita la GPU VideoCore VII con aceleración KMS DRM.

### C. Apple Silicon M1 / M2 / M3 (vía Asahi m1n1)
En ordenadores Mac con chips de la serie M:
1. Se utiliza el cargador de arranque de código abierto `m1n1` para inicializar el silicio de Apple y configurar los controladores de interrupción AIC.
2. Portix OS se encadena como payload directo (`portix-apple-silicon.m1n1`) tras el Device Tree entregado por `m1n1`.
3. El planificador toma ventaja inmediata de la microarquitectura de núcleos de alto rendimiento (Firestorm / Avalanche) y de alta eficiencia (Icestorm / Blizzard).

### D. Arranque UEFI Estándar (BOOTAA64.EFI)
Para servidores ARM64 empresariales (Ampere Altra, AWS Graviton) o laptops con Snapdragon X Elite:
- El artefacto oficial `build/dist/BOOTAA64.EFI` se instala en la partición `/EFI/BOOT/BOOTAA64.EFI`, cargando la tabla GOP del firmware para renderizar de inmediato el escritorio Cosmos v4.0.

---

## 7. Compilación Cruzada y Verificación

Para generar todos los binarios y comprobar la conformidad arquitectónica:
```bash
# Generar todos los artefactos ARM64 oficiales
python3 scripts/build_aarch64.py

# Ejecutar la suite de verificación arquitectónica
python3 scripts/arm64_runner.py
```

---

<div align="center">

**Portix OS v4.0 'Cosmos'**  
*Una base de software universal desde x86_64 hasta ARM64.*

</div>
