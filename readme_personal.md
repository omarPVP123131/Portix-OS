# Portix OS — Notas Personales

**Version:** 0.8.0  
**Estado:** Arranque dual BIOS + UEFI funcional

## ¿Qué es Portix?

Un sistema operativo desde cero hecho en Rust. Kernel freestanding, sin libc,
sin linux, sin andamios. Arranca en BIOS legacy **y** en UEFI (OVMF, hardware
real) — ese es el gran logro de esta versión.

## Boot

**BIOS:** MBR → stage2 (32 KB) → VESA 1024×768 → kernel  
**UEFI:** GPT ESP → BOOTX64.EFI (Rust) → Block I/O lee FAT32 → PortixBootInfo
→ ExitBootServices → kernel

El loader UEFI está en `boot/efi/src/main.rs`. Es Rust puro, sin binding a
tiano-rs ni nada — llamadas directas a la boot table con la convención
`extern "efiapi"` (Microsoft x64: RCX, RDX, R8, R9).

### GOP

El UEFI spec dice que hay que usar Graphics Output Protocol para el
framebuffer. En OVMF de Windows **no funciona** (LocateProtocol devuelve
EFI_NOT_FOUND para todo excepto LoadedImageProtocol). El kernel tiene un
fallback: escanea PCI, encuentra el BAR de la VGA, y si el Bochs VBE
interface no está activo, lo inicializa a 1024×768×32. Así el display
siempre se ve bien sin importar lo que haya dejado el firmware.

### Serial Debug

Todo el debug es por COM1 (puerto 0x3F8). El loader UEFI imprime cada paso:
protocolos, GOP, memoria, ExitBS. El kernel imprime drivers, VBE, filesystem.
Conectas QEMU con `-serial stdio` y ves todo.

## Drivers

| Driver     | Estado     | Notas                                |
|------------|------------|--------------------------------------|
| ATA PIO    | Funcional  | LBA48, cache de sectores             |
| FAT32      | Funcional  | Lectura/escritura, cluster chain     |
| PCI        | Funcional  | Escaneo de bus, BAR parsing          |
| PS/2 tec.  | Funcional  | Keyboard IRQ1                        |
| PIT        | Funcional  | 100 Hz tick                          |
| Serial     | Funcional  | COM1, 38400 8N1, log levels          |

## UI

La UI es **proporcional** — todas las coordenadas se calculan como fracciones
del ancho/alto del framebuffer. No hay píxeles duros. El Layout se adapta a
cualquier resolución.

Tabs: System, Terminal, Devices, IDE, Explorer.

## Build

```sh
python scripts/build.py --mode=raw     # BIOS
python scripts/build.py --mode=uefi    # UEFI (necesita OVMF)
python scripts/build.py --vga=virtio   # Probar con virtio-vga
```

El build produce: .img (BIOS), .iso (El Torito), .img UEFI (GPT+ESP), .vdi,
.vmdk.

## Lo que falta (pero ya se puede escalar)

- [ ] Paginación virtual (ahora se usa el identity map que deja UEFI)
- [ ] USB (solo PS/2 por ahora)
- [ ] TCP/IP stack
- [ ] Soporte SMP (múltiples CPUs)
- [ ] Modo usuario / ring 3
- [ ] VirtIO-GPU para aceleración 2D/3D
- [ ] Sistema de ventanas (mover, redimensionar)
- [ ] Drivers NVMe, AHCI
- [ ] Ext2/4 read support

## Historia técnica rápida

Portix empezó como un kernel BIOS-only. La meta de 0.8.0 era UEFI. El problema
fue que OVMF en Windows no expone GOP, el kernel se quedaba sin framebuffer,
y el display salía distorsionado. La solución fue:
1. Loader UEFI que pasa PortixBootInfo con lo que pueda (GOP o no)
2. Kernel que detecta la falta de framebuffer y escanea PCI directamente
3. VBE init forzado a 1024×768 si el firmware no lo configuró

Todo eso ya está funcionando. El siguiente paso es limpiar, documentar, y
empezar con paginación y ring 3.
