# 🖥️ Guía Maestra de Despliegue en Hardware Real Bare-Metal — Portix OS v4.0 'Cosmos'

> **Fecha de Publicación**: Septiembre de 2026  
> **Versión del Sistema**: Portix OS v4.0.0 "Cosmos"  
> **Arquitectura Objetivo**: x86_64 (Intel 64 / AMD64) & Preparación ARM64  
> **Tiempo de Arranque Típico**: ~450 ms en frío  

---

## 📑 Índice de Contenidos

1. [Requisitos de Hardware y Matriz de Compatibilidad](#1-requisitos-de-hardware-y-matriz-de-compatibilidad)
2. [Preparación del Medio USB Booteable](#2-preparación-del-medio-usb-booteable)
   - [Opción A: Herramienta Oficial `scripts/flash_usb.py` (Linux / macOS)](#opción-a-herramienta-oficial-scriptsflash_usbpy-linux--macos)
   - [Opción B: Despliegue con Ventoy (Recomendado para Multi-Boot)](#opción-b-despliegue-con-ventoy-recomendado-para-multi-boot)
   - [Opción C: Grabación con Rufus (Windows)](#opción-c-grabación-con-rufus-windows)
   - [Opción D: Volcado Directo con `dd` (UNIX CLI)](#opción-d-volcado-directo-con-dd-unix-cli)
3. [Configuración del Firmware de la Placa Madre (BIOS / UEFI)](#3-configuración-del-firmware-de-la-placa-madre-bios--uefi)
4. [Proceso de Arranque y Primer Contacto (Bare-Metal OOBE)](#4-proceso-de-arranque-y-primer-contacto-bare-metal-oobe)
5. [Estrategias de Coexistencia y Dual-Boot](#5-estrategias-de-coexistencia-y-dual-boot)
   - [Dual-Boot con Windows 11](#dual-boot-con-windows-11)
   - [Dual-Boot con Distribuciones Linux](#dual-boot-con-distribuciones-linux)
6. [Diagnóstico de Hardware y Resolución de Problemas (Troubleshooting)](#6-diagnóstico-de-hardware-y-resolución-de-problemas-troubleshooting)

---

## 1. Requisitos de Hardware y Matriz de Compatibilidad

Portix OS v4.0 'Cosmos' ha sido diseñado para operar con una eficiencia extrema tanto en equipos modernos de alta gama como en ordenadores portátiles y de sobremesa de generaciones anteriores:

| Componente | Requisito Mínimo | Configuración Recomendada | Notas de Compatibilidad |
|---|---|---|---|
| **Procesador (CPU)** | Intel Core 2 Duo / AMD Athlon 64 (x86_64) | Intel Core (10ª a 14ª gen) o AMD Ryzen (Serie 3000 a 8000) | Soporte de virtualización por hardware (VT-x / AMD-V) para MicroVMs. |
| **Memoria RAM** | 256 MB | 4 GB – 16 GB DDR4/DDR5 | El sistema base consume apenas 14 a 16 MB en reposo. |
| **Almacenamiento** | 128 MB (arranque directo) / 2 GB (persistente) | SSD NVMe M.2 (PCIe Gen 3/4/5) o SSD SATA AHCI | Soporte nativo para ZNS, NVMe y particionado GPT/MBR. |
| **Pantalla y GPU** | VESA / GOP Framebuffer (1024×768×32) | Intel UHD/Iris Xe, AMD Radeon Graphics (RDNA), NVIDIA | Direct Scanout y búfer doble acelerado en RAM. |
| **Teclado / Ratón** | PS/2 nativo o USB con emulación BIOS Legacy | Teclado y ratón USB estándar | Se apoya en el hand-off xHCI/EHCI del firmware. |
| **Red** | Sin conexión requerida | Intel Gigabit (e1000/e1000e), Realtek r8169, Wi-Fi 6E | Soporte DHCP automático y cifrado WireGuard Ring-0. |

---

## 2. Preparación del Medio USB Booteable

Las imágenes oficiales de Portix OS se encuentran ubicadas en `build/dist/`:
- **`portix-dual.img`**: Imagen híbrida con tabla de particiones MBR + GPT booteable tanto en máquinas BIOS Legacy antiguas como en plataformas UEFI modernas.
- **`portix-uefi.iso`**: Imagen ISO El Torito optimizada para UEFI GOP y herramientas multi-arranque.

### Opción A: Herramienta Oficial `scripts/flash_usb.py` (Linux / macOS)
El repositorio incluye una herramienta inteligente que verifica la integridad criptográfica de la imagen y evita sobreescribir discos de sistema por error:

```bash
# 1. Listar dispositivos extraíbles conectados:
python3 scripts/flash_usb.py --list

# 2. Simulación segura previa (Dry Run):
python3 scripts/flash_usb.py --device /dev/sdX --dry-run

# 3. Grabación definitiva en la unidad USB seleccionada:
python3 scripts/flash_usb.py --device /dev/sdX
```

### Opción B: Despliegue con Ventoy (Recomendado para Multi-Boot)
Si utilizas una unidad USB formateada con [Ventoy](https://www.ventoy.net/):
1. Conecta tu pendrive Ventoy a tu equipo.
2. Copia directamente el archivo `build/dist/portix-dual.img` o `build/dist/portix-uefi.iso` en la partición de datos de Ventoy.
3. Desmonta y extrae la unidad con seguridad. Al encender tu PC, selecciona Portix en el menú interactivo de Ventoy.

### Opción C: Grabación con Rufus (Windows)
1. Abre Rufus en Windows.
2. Selecciona tu unidad USB en el campo **Dispositivo**.
3. En **Elección de arranque**, haz clic en *Seleccionar* y elige `portix-dual.img` (asegúrate de mostrar "Todos los archivos `*.*`").
4. Si Rufus solicita el modo de grabación, selecciona **"Escribir en modo imagen DD"**.
5. Pulsa **Empezar**.

### Opción D: Volcado Directo con `dd` (UNIX CLI)
```bash
# Asegúrate de reemplazar /dev/sdX por tu pendrive USB (¡NO selecciones tu disco del sistema!)
sudo dd if=build/dist/portix-dual.img of=/dev/sdX bs=4M status=progress conv=fsync
sudo sync
```

---

## 3. Configuración del Firmware de la Placa Madre (BIOS / UEFI)

Para garantizar un arranque inmediato y sin bloqueos de seguridad del fabricante, ingresa a la BIOS de tu equipo pulsando la tecla correspondiente al encender (Lenovo: `F1` o `Enter`; Dell: `F2`; HP: `F10` o `Esc`; ASUS: `F2` o `Del`; MSI: `Del`; Framework: `F12`):

1. **Secure Boot**:
   - Ajuste: **Disabled** (Desactivado).
   - *Razón*: Portix OS utiliza su propio cargador de arranque en Rust puro (`BOOTX64.EFI`) y no depende de claves shim firmadas por Microsoft.
2. **SATA Operation / Storage Configuration**:
   - Ajuste: **AHCI** o **NVMe Native**.
   - ⚠️ **Importante**: Desactiva configuraciones como *Intel RST (Rapid Storage Technology)*, *RAID On* o *Intel VMD (Volume Management Device)*, ya que ocultan los discos PCIe tras controladores propietarios no estándar.
3. **Boot Mode**:
   - Ajuste: **UEFI Only** (recomendado para laptops modernas) o **Dual / UEFI with CSM**.
4. **Fast Boot / Quick Boot**:
   - Ajuste: **Disabled** (Desactivado).
   - Permite que el firmware inicialice correctamente los controladores USB y las controladoras PCI al encender.
5. **Virtualización por Hardware (Intel VT-x / AMD SVM)**:
   - Ajuste: **Enabled** (Activado).
   - Necesario para aprovechar las MicroVMs instantáneas del Hipervisor Tipo-1 de Portix OS.
6. **USB Legacy Support**:
   - Ajuste: **Enabled** (Activado).
   - Asegura la captura de eventos de teclado y ratón durante las fases tempranas de inicio.

---

## 4. Proceso de Arranque y Primer Contacto (Bare-Metal OOBE)

1. Conecta la unidad USB preparada en un puerto USB directo del equipo (preferentemente USB 3.0 en el panel trasero o lateral principal).
2. Enciende el equipo y pulsa reiteradamente la tecla del menú de arranque rápido (**Boot Menu**):
   - **Lenovo ThinkPad / IdeaPad**: `F12`
   - **Dell**: `F12`
   - **HP**: `F9` o `Esc`
   - **ASUS**: `F8` o `Esc`
   - **MSI / ASRock**: `F11`
   - **Gigabyte**: `F12`
3. Selecciona la entrada que corresponde a la unidad USB (por ejemplo: `UEFI: Kingston DataTraveler` o `Portix OS Bootloader`).
4. **En menos de 500 milisegundos**, la pantalla presentará la inicialización gráfica en resolución 1024×768 a 32 bits por píxel con el escritorio interactivo Cosmos v4.0:
   - Notificación de bienvenida: `❖ PORTIX OS v4.0 'COSMOS'`.
   - Barra superior con telemetría de silicio en vivo: NPU, Malla WireGuard, latencia de audio y reloj 60 FPS.
   - Terminal Starship lista para recibir comandos.
   - Centro de Control con monitoreo de temperatura de CPU mediante MSRs, interfaz e1000 y asignación de memoria física Buddy Allocator.

---

## 5. Estrategias de Coexistencia y Dual-Boot

### Dual-Boot con Windows 11
Portix OS puede convivir en la misma máquina que Windows 11 sin alterar la partición NTFS del sistema operativo de Microsoft:

1. **Liberar espacio no asignado**:
   - En Windows 11, pulsa `Win + X` y selecciona **Administración de discos** (`diskmgmt.msc`).
   - Haz clic derecho sobre tu volumen principal `C:` y selecciona **Reducir volumen**.
   - Reserva al menos 4 GB de espacio libre sin particionar.
2. **Copia de seguridad de la clave BitLocker**:
   - Si tienes cifrado BitLocker activo, suspéndelo temporalmente o respalda tu clave de recuperación de 48 dígitos.
3. **Configurar el arranque**:
   - Puedes arrancar Portix OS conectando tu USB cuando desees utilizarlo (pulsando `F12`), o bien copiar el cargador `BOOTX64.EFI` a una subcarpeta `/EFI/PORTIX/` dentro de la partición ESP del sistema (`EFI System Partition`) y añadir la entrada mediante herramientas como `efibootmgr` o `EasyUEFI`.

### Dual-Boot con Distribuciones Linux
Si ya cuentas con una instalación de Linux (Ubuntu, Fedora, Debian, Arch Linux) administrada por GRUB 2:

1. Monta la partición ESP (generalmente en `/boot/efi`).
2. Copia el archivo `boot/efi/BOOTX64.EFI` a `/boot/efi/EFI/portix/bootx64.efi`.
3. Añade una entrada personalizada en `/etc/grub.d/40_custom`:
   ```bash
   menuentry "Portix OS v4.0 'Cosmos' (Bare-Metal)" {
       insmod fat
       insmod chain
       search --no-floppy --set=root --file /EFI/portix/bootx64.efi
       chainloader /EFI/portix/bootx64.efi
   }
   ```
4. Actualiza GRUB ejecutando `sudo update-grub` o `sudo grub-mkconfig -o /boot/grub/grub.cfg`.

---

## 6. Diagnóstico de Hardware y Resolución de Problemas (Troubleshooting)

### A. Pantalla en negro tras seleccionar el arranque
- **Causa**: El firmware UEFI de la máquina no expuso el protocolo GOP en el modo nativo seleccionado.
- **Solución**: En el menú de la BIOS, activa el soporte CSM (*Compatibility Support Module*) o arranca en modo BIOS Legacy seleccionando la entrada no-UEFI de la unidad USB. Portix conmutará automáticamente a su driver VESA BIOS Extensions a 1024×768.

### B. El teclado o ratón USB no responden en el escritorio
- **Causa**: La controladora xHCI no realizó el traspaso de emulación PS/2 legacy al kernel.
- **Solución**:
  - Ingresa a la BIOS y asegúrate de activar **"Legacy USB Emulation"** o **"Port 60/64 Emulation"**.
  - Conecta el periférico a un puerto USB 2.0 (habitualmente de color negro) en lugar de un puerto USB 3.2 Gen 2 / Tipo-C.

### C. No se detecta el disco SSD NVMe
- **Causa**: La controladora de almacenamiento está configurada en modo Intel VMD o Intel RST.
- **Solución**: Ingresa al menú de la placa madre y cambia el modo de almacenamiento a **AHCI / NVMe Direct**.

### D. Captura de registros seriales de depuración en vivo
Portix OS envía telemetría de diagnóstico continua a través del puerto serie estándar **COM1 (UART 16550) a 38400 baudios, 8 bits de datos, sin paridad y 1 bit de parada (38400 8N1)**.
- En placas base de sobremesa con cabezal serie o mediante un cable adaptador USB-UART FTDI conectado a los pines TX/RX/GND, puedes capturar la traza completa de arranque con PuTTY, Minicom o screen:
  ```bash
  screen /dev/ttyUSB0 38400
  ```

---

<div align="center">

**Portix OS v4.0 'Cosmos'**  
*Ingeniería soberana en Rust directo al silicio.*

</div>
