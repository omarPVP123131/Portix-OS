# ESTADO REAL Y VERIFICADO DE PORTIX OS (AUDITORIA TECNICA)

> Documento de auditoria y transparencia tecnica.
> Version del sistema: Portix OS v1.0.0
> Filosofia: transparencia absoluta, rigor de ingenieria de sistemas y cero mitos.

---

## 1. RESUMEN EJECUTIVO

Portix OS es un sistema operativo independiente de 64 bits disenado desde cero en
Rust puro freestanding (no_std), sin recurrir a bibliotecas de lenguaje C estandar
en el nucleo, sin GRUB y con cero dependencias externas en el kernel
([dependencies] vacio en kernel/Cargo.toml).

Implementa una arquitectura hibrida desacoplada inspirada en los principios de
microkernel de seL4 y Zircon, combinada con un subsistema de compatibilidad
binaria directa para ejecutables Linux (ELF64) en espacio de usuario (Ring 3).

Este documento expone con rigor lo que el sistema efectivamente implementa y
verifica, distinguiendo con honestidad tecnica sus capacidades reales frente a
sus limitaciones actuales.

---

## 2. LO QUE ESTA IMPLEMENTADO Y VERIFICADO

### 2.1. Cargador de arranque dual (Bootloader)

- BIOS Legacy (x86_64): sector MBR de 512 bytes en NASM, Stage 2 (64 sectores),
  modo protegido, modo largo, carga del kernel en 0x200000.
- UEFI nativo: cargador en Rust puro (BOOTX64.EFI), negocia servicios UEFI,
  asigna memoria GOP, lee kernel ELF de la ESP FAT32.
- Imagenes ISO hibridas dual-boot (xorriso con El Torito).

### 2.2. Gestion de memoria y paginacion

- Buddy allocator: 64 MiB, ordenes 0 a 11 (4 KiB a 4 MiB), listas libres intrusivas.
- Paginador de 4 niveles x86_64: PML4, PDPT, PD, PT. Proteccion de paginas,
  supervisor/usuario, bit NX.
- Paginas gigantes (2 MB, 1 GB).
- Copy-On-Write en fork con conteo de referencias por marco fisico.
- Asignador de marcos fisicos a partir del mapa de memoria del firmware.

### 2.3. Microkernel hibrido, planificacion e IPC

- Planificador CFS con vruntime, pesos nice, penalizaciones de latencia.
- Fast-IPC hibrido: mensajeria sincronica en registros, asincronica con VMOs.
- SMP real: segundo nucleo arranca y planifica (AP con GDT/TSS/LAPIC propios).

### 2.4. Almacenamiento, VFS y sistemas de archivos

- Controlador ATA/IDE (LBA48).
- VFS con puntos de montaje dinamicos.
- FAT32: lectura y escritura.
- RamFS: volatil, montado en /tmp.
- ext2: implementacion completa RW (superbloque, bitmaps, directorios, indirectos).
- procfs: /proc con estado del kernel y procesos.
- FIFOs con nombre en ext2.

### 2.5. Red, protocolos y cifrado

- Controlador Intel e1000 Gigabit (PCI nativo, buffers RX/TX circulares).
- Pila de red: ARP, ICMP (ping), DHCP, UDP, TCP, DNS.
- IPv6: SLAAC, NDP, ICMPv6, TCP/UDP sobre IPv6.
- TLS 1.3 real en Ring 3 (X25519, HKDF, ChaCha20-Poly1305, RSA, X.509).
- WireGuard (modulo, no verificado en QEMU como tunel activo).

### 2.6. Espacio de usuario (Ring 3)

- Runtime propio portix-rt (no_std, traduce syscalls directamente).
- 70+ utilidades (ls, cat, grep, make, curl, httpd, python, etc.).
- Compilacion nativa de binarios C (hello_c, calc_c) con cabeceras POSIX.
- Interprete Python embebido (/bin/python).
- Compatibilidad Linux ABI: 452 syscalls traducidas (verificable con /bin/abi-linux).
- Compatibilidad Windows PE32+ (portix-win32): cargador PE/COFF Ring-3 con emulacion de kernel32.dll.

### 2.7. Interfaz grafica

- Framebuffer VBE/GOP (1024x768, 32 bpp).
- Compositor Wayland en Ring 3 (portix-wm) con tiling y workspaces.
- Terminal interactiva, centro de control, dock.

---

## 3. COMPARATIVA TECNICA HONESTA

Comparaciones con otros sistemas operativos no se pueden medir en igualdad de
condiciones en este entorno (QEMU TCG, sin KVM, sin silicio real). Se listan
unicamente las metricas medidas de Portix en QEMU.

| Dimension | Portix OS (medido en QEMU) | Linux / Windows 11 |
| :--- | :--- | :--- |
| Lenguaje del nucleo | 100% Rust (no_std), cero dependencias externas | Linux: 95% C; Windows: C/C++ |
| Tiempo de arranque | ~9-12 s en QEMU/SeaBIOS (incluye firmware) | No medido en este entorno |
| Consumo de memoria base | ~85 MB fijos (monticulo 64 MiB + back buffer) + usuario | No medido en este entorno |
| Huella en disco | ~72 MB (imagen completa con userland) | Linux: 1.5-10 GB; Windows: 20-64 GB |
| Telemetria y privacidad | Cero telemetria por diseno (codigo auditable) | Linux: configurable; Windows: obligatoria |
| Latencia de syscall | No medida (portix-bench mide ciclos TSC) | No medido en este entorno |
| Ecosistema de drivers | Limitado a hardware estandar (virtio, e1000, AC97) | Exhaustivo |
| Aceleracion 3D | VirtIO GPU 3D Venus / en desarrollo bare-metal | Excelente / Nativo |
| Catalogo de software | Binarios estaticos, Python, C y Win32 livianos | Masivo / Universal |
| Anti-cheats de nivel kernel | No soportados (filosofia microkernel segura) | Parcial / Nativo |

---

## 4. ANALISIS DE FORTALEZAS Y DEBILIDADES

### 4.1. Fortalezas

1. Seguridad por el compilador: Rust elimina use-after-free, double-free, desbordamientos de buffer.
2. Determinismo y rendimiento: huella de memoria pequena, arranque rapido en QEMU.
3. Soberania y transparencia: cero telemetria, codigo auditable.
4. Arquitectura limpia: separacion estricta Ring 0 / Ring 3.

### 4.2. Debilidades

1. Hardware fisico exotico: falta de drivers para perifericos antiguos/exoticos.
2. GPUs propietarias de ultima generacion: no hay drivers firmados para ray tracing bare-metal.
3. Bibliotecas dinamicas complejas: no implementa .NET/WinRT ni todas las peculiaridades de glibc.
4. Verificacion en QEMU (TCG), no en silicio real.

---

## 5. PRUEBAS AUTOMATIZADAS

- Suite headless CI (scripts/headless_runner.py): pruebas de arranque, subsistemas, POSIX, red, TLS, etc.
- Suite interactiva (scripts/interactive_runner.py): pruebas de escritorio real en QEMU.
- Recertificacion (scripts/recertify.py): reconstruccion + suites + verificacion.

La suite exige dos pasadas consecutivas con 0 fallos + recertificacion para cerrar un ciclo.

---

## 6. CONCLUSION

Portix OS es un sistema operativo independiente en Rust que demuestra que es
tecnicamente viable construir un sistema operativo contemporaneo, eficiente,
seguro y soberano. Sus capacidades estan probadas y verificadas en QEMU.
Sus limitaciones estan identificadas y delimitadas.