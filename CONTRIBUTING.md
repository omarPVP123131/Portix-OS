# Contribuir a Portix OS

Portix es un kernel x86_64 escrito en Rust freestanding — sin libc, sin Linux, sin abstracciones innecesarias.
Contribuir aquí significa trabajar directamente sobre el hardware: bootloaders en ASM, drivers en Rust `no_std`, gestión de memoria manual.

Es exigente. También es de los proyectos más interesantes en los que puedes trabajar.

---

## Tabla de contenidos

- [Contribuir a Portix OS](#contribuir-a-portix-os)
  - [Tabla de contenidos](#tabla-de-contenidos)
  - [Antes de empezar](#antes-de-empezar)
  - [Primeros pasos (good first issues)](#primeros-pasos-good-first-issues)
  - [Configuración del entorno](#configuración-del-entorno)
    - [Dependencias](#dependencias)
    - [Compilar y probar](#compilar-y-probar)
  - [Flujo de Pull Request](#flujo-de-pull-request)
    - [1. Fork y rama](#1-fork-y-rama)
    - [2. Desarrolla y prueba](#2-desarrolla-y-prueba)
    - [3. Abre el Pull Request](#3-abre-el-pull-request)
    - [4. Revisión](#4-revisión)
  - [Convenciones de commits](#convenciones-de-commits)
    - [Formato](#formato)
    - [Tipos](#tipos)
    - [Ejemplos](#ejemplos)
  - [Reglas de código](#reglas-de-código)
    - [`no_std` obligatorio](#no_std-obligatorio)
    - [`unsafe` — documentar siempre](#unsafe--documentar-siempre)
    - [Formato y estilo](#formato-y-estilo)
  - [Reglas de arquitectura — qué NO tocar](#reglas-de-arquitectura--qué-no-tocar)
    - [🔴 Crítico — requiere Issue + aprobación previa](#-crítico--requiere-issue--aprobación-previa)
    - [🟡 Sensible — discutir antes si el cambio es estructural](#-sensible--discutir-antes-si-el-cambio-es-estructural)
  - [Áreas de interés](#áreas-de-interés)
  - [Reportar bugs](#reportar-bugs)
  - [Comunicación](#comunicación)

---

## Antes de empezar

Antes de abrir un PR, confirma que:

- [ ] Leíste este documento completo
- [ ] Tu cambio compila sin errores con `python scripts/build.py`
- [ ] Probaste en QEMU (`--mode=bios o `--mode=`uefi`)
- [ ] Los mensajes de tus commits siguen el formato definido abajo
- [ ] No modificaste zonas de arquitectura crítica sin abrir un Issue primero

Si tienes dudas sobre si un cambio encaja con el proyecto, **abre un Issue antes de escribir código**.
Es mejor discutir el enfoque primero que reescribir un PR completo.

---

## Primeros pasos (good first issues)

Si es tu primera contribución a Portix, estas áreas son el mejor punto de entrada:

| Área | Dificultad | Dónde |
|------|-----------|-------|
| Nuevos comandos de terminal | 🟢 Baja | `kernel/src/console/terminal/commands/` |
| Mejoras en mensajes de error del kernel | 🟢 Baja | `kernel/src/util/` |
| Documentación de funciones existentes | 🟢 Baja | cualquier módulo |
| Optimización del renderizado de fuentes | 🟡 Media | `kernel/src/graphics/render/` |
| Nuevos comandos de disco | 🟡 Media | `kernel/src/console/terminal/commands/` |
| Mejoras en el manejo de interrupciones | 🔴 Alta | `kernel/src/arch/` |
| Soporte ACPI básico | 🔴 Alta | `kernel/src/drivers/bus/` |

Busca issues etiquetados con `good first issue` en el repositorio para tareas concretas y acotadas.

---

## Configuración del entorno

### Dependencias

Consulta [`PREREQUISITES.md`](PREREQUISITES.md) para la guía completa por sistema operativo.
Resumen rápido:

```sh
# Rust nightly
rustup toolchain install nightly
rustup default nightly
rustup component add rust-src --toolchain nightly

# Herramientas (Debian/Ubuntu)
sudo apt install nasm qemu-system-x86 binutils xorriso ovmf

# pyfatfs (solo UEFI)
pip3 install pyfatfs
```

### Compilar y probar

```sh
# Compilar y lanzar en modo BIOS
python scripts/build.py --mode=iso

# Compilar y lanzar en modo UEFI
python scripts/build.py --mode=uefi

# Limpiar artefactos
python scripts/build.py --clean
```

La salida serial de debug aparece en la terminal via `-serial stdio`.

---

## Flujo de Pull Request

### 1. Fork y rama

```sh
# Clona tu fork
git clone https://github.com/<tu-usuario>/portix-os.git
cd portix-os

# Crea una rama descriptiva para tu cambio
git checkout -b feat/comando-ls-en-terminal
# o
git checkout -b fix/kernel-panic-en-fat32-write
```

Nombra las ramas igual que los commits: `tipo/descripcion-breve`.

### 2. Desarrolla y prueba

- Haz commits atómicos — un commit, una cosa
- Prueba en QEMU antes de subir (`--mode=iso` mínimo, `--mode=uefi` si tocaste el loader)
- Si tu cambio afecta memoria o drivers, prueba también con `-serial stdio` para revisar la salida de debug

### 3. Abre el Pull Request

Al abrir el PR incluye:

```
## ¿Qué hace este PR?
Descripción clara del cambio.

## ¿Por qué es necesario?
Contexto del problema que resuelve o la función que añade.

## ¿Cómo se probó?
- [ ] Compila sin errores
- [ ] Probado en QEMU --mode=iso
- [ ] Probado en QEMU --mode=uefi
- [ ] No rompe funcionalidad existente

## Notas adicionales
Decisiones de diseño, limitaciones conocidas, etc.
```

### 4. Revisión

- Los PRs se revisan por Omar Palomares
- Puede haber rondas de feedback — es normal y parte del proceso
- Un PR que pase la revisión se integra a `master`

---

## Convenciones de commits

### Formato

```
tipo(alcance): descripción breve en minúsculas
```

- Máximo **50 caracteres** en el título
- Sin punto final
- Todo en **minúsculas**
- Si necesitas más contexto, añade un cuerpo separado por una línea en blanco

### Tipos

| Tipo | Cuándo usarlo |
|------|---------------|
| `feat` | Nueva funcionalidad (driver, comando, módulo) |
| `fix` | Corrección de bug (kernel panic, comportamiento incorrecto) |
| `docs` | Solo cambios en documentación |
| `style` | Formateo, espacios — sin cambio de lógica |
| `refactor` | Reescritura de código sin cambiar comportamiento externo |
| `arch` | Cambios en arranque o ensamblador (`boot/`, ISRs, GDT, IDT) |
| `perf` | Optimización de rendimiento |
| `test` | Añadir o corregir pruebas |

### Ejemplos

```sh
feat(drivers): añadir soporte inicial para ratón ps/2
fix(mem): corregir desbordamiento en el buddy allocator
docs(readme): actualizar guía de ejecución con qemu
arch(boot): migrar stage2 a modo largo 64-bit
feat(cmd): añadir comando 'ls' para listar directorio actual
fix(fat32): corregir lectura de cluster chain en archivos grandes
refactor(graphics): separar lógica de blit del módulo framebuffer
perf(ata): reducir latencia en lecturas con prefetch de sectores
```

---

## Reglas de código

### `no_std` obligatorio

Todo el código del kernel debe funcionar sin la librería estándar de Rust.
No uses `std::`, `println!` (fuera del módulo serial), ni ningún tipo que dependa del sistema operativo host.

```rust
// ❌ Prohibido
use std::vec::Vec;
use std::string::String;

// ✅ Correcto
use alloc::vec::Vec;         // Solo si el allocator está inicializado
use core::fmt::Write;
```

### `unsafe` — documentar siempre

`unsafe` en un kernel es inevitable. Lo que no es aceptable es `unsafe` sin explicación.

```rust
// ❌ Inaceptable
unsafe {
    ptr::write(addr as *mut u32, value);
}

// ✅ Correcto
// SAFETY: `addr` es una dirección de framebuffer mapeada en PortixBootInfo,
// garantizada como válida y alineada a 4 bytes antes de llamar esta función.
unsafe {
    ptr::write(addr as *mut u32, value);
}
```

### Formato y estilo

- Sigue la estructura de módulos existente — no crees nuevos módulos raíz sin discutirlo
- Nombres descriptivos: `framebuffer_blit_region` no `fbr`
- Sin código comentado en los PRs — si es experimental, usa una rama separada
- Sin `unwrap()` ni `expect()` en rutas de código crítico del kernel

---

## Reglas de arquitectura — qué NO tocar

Estas áreas afectan la estabilidad fundamental del kernel.
**Cualquier cambio aquí requiere abrir un Issue primero** y esperar aprobación antes de escribir código.

### 🔴 Crítico — requiere Issue + aprobación previa

| Área | Motivo |
|------|--------|
| `boot/boot.asm` y `boot/stage2.asm` | Cambiar el MBR o la secuencia de arranque puede romper todos los modos de boot |
| `kernel/linker.ld` | Modificar el layout de memoria puede corromper el heap, el stack o el framebuffer |
| `kernel/src/arch/` (GDT, IDT, ISR) | Errores aquí producen triple faults silenciosos difíciles de depurar |
| `kernel/src/mem/` (Buddy Allocator) | El allocator afecta a todo el kernel — un bug aquí produce corrupción de memoria global |
| Mapa de memoria (`0x100000`, `0x200000`) | Las direcciones del backbuffer y del kernel están hardcodeadas en múltiples sitios |
| `boot/efi/src/main.rs` (loader UEFI) | Cambios incorrectos aquí rompen el arranque UEFI completamente |

### 🟡 Sensible — discutir antes si el cambio es estructural

- `kernel/src/drivers/storage/` — el VFS y FAT32 tienen dependencias cruzadas
- `kernel/src/graphics/driver/` — el framebuffer es compartido por toda la UI
- `scripts/build.py` — cambios en el pipeline de build afectan a todos los modos

---

## Áreas de interés

Si buscas algo concreto donde contribuir:

**Terminal y comandos**
`kernel/src/console/terminal/commands/`
Añadir nuevos comandos (`ls`, `cat`, `cp`, `mv`, etc.) siguiendo el patrón de los existentes.

**Renderizado de fuentes**
`kernel/src/graphics/render/`
Optimización del blit de glifos, soporte para fuentes de mayor resolución, antialiasing básico.

**Drivers de entrada**
`kernel/src/drivers/input/`
Mejoras en el protocolo PS/2, soporte para más scancodes, buffers de entrada más robustos.

**Interrupciones y ACPI**
`kernel/src/arch/` y `kernel/src/drivers/bus/`
Soporte para APIC, mejoras en el manejo de IRQs, parsing básico de tablas ACPI.

---

## Reportar bugs

Antes de abrir un issue de bug, comprueba que no existe ya uno similar abierto.

Usa esta plantilla al reportar:

```
**Versión de Portix:** (ej. v0.8.0)
**Sistema host:** (ej. Ubuntu 22.04 / Windows 11)
**Modo de arranque:** (BIOS / UEFI / ambos)

**Descripción del problema**
Qué ocurre y qué se esperaba que ocurriera.

**Pasos para reproducir**
1. Compilar con `python scripts/build.py --mode=iso`
2. ...
3. El kernel hace panic en ...

**Salida serial (si está disponible)**
Pega aquí la salida de `-serial stdio`.

**Notas adicionales**
Cualquier contexto relevante: si ocurre solo en QEMU, solo en hardware real, etc.
```

Etiqueta el issue con `bug`. Si el bug produce un kernel panic, añade también `kernel-panic`.

---

## Comunicación

- **Issues** — para bugs, propuestas de features y preguntas técnicas (usa el tag `question`)
- **Pull Requests** — para cambios concretos con código

Toda contribución, por pequeña que sea, es bienvenida.
Portix es un proyecto de largo aliento — la 1.0 será perfecta, y cada PR cuenta.

---

**Portix OS** · Desarrollado por Omar Palomares Velasco · Licencia GPL v3
