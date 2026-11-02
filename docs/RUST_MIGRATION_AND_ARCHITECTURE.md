# PORTIX OS — Migración Integral a 100% Rust y Arquitectura Modular Senior

## 1. Visión General de la Transformación

Siguiendo las directrices de ingeniería y arquitectura de sistemas operativos Senior, **Portix OS** ha completado la transición estructural para erradicar el código C heredado y consolidar un ecosistema unificado en **Rust puro bare-metal (`no_std`)**:

- **Kernel (`kernel/`)**: Núcleo monolítico modular en Rust 100% bare-metal sin dependencias externas (`[dependencies]` vacío). Manejo estricto de memoria, paginación x86_64 multinivel, planificador preemptivo, subsistema VFS/FAT32, pila de red e1000/TCP/UDP/DHCP, y subsistema LKM (Loadable Kernel Modules).
- **Userland Workspace (`userland/`)**: Nuevo espacio de usuario orquestado mediante un `Cargo.toml` workspace profesional. Contiene el runtime nativo `portix-rt` y 15 binarios independientes compilados como ejecutables ELF x86_64 nativos para Ring 3 (`ET_EXEC` alineados a páginas de 4096 bytes con punto de entrada en `0x20000000`).
- **Módulos de Kernel LKM (`modules/`)**: Módulos reubicables ELF (`ET_REL`) compilados directamente en Rust para inyección y descarga dinámica en tiempo de ejecución en Ring 0 sin dependencias en C.
- **Servidor Gráfico y Compositor Ring 3 (`userland/bin/portix-wm`)**: Implementación de la **Ruta 1 (Gráficos Desacoplados y Protocolo de Ventanas en Ring 3)**. El compositor corre en espacio de usuario aislado, mapea el framebuffer físico mediante `sys_mmap_device`, renderiza con doble búfer y fuentes bitmap, y permite alternar o finalizar devolviendo el control al modo consola sin colapsar el kernel.
- **Archivo Histórico de C (`legacy/c_archive/`)**: El código original en C (ejemplos, libc básica y módulos C) fue retirado del árbol activo de compilación y archivado con fines históricos.

---

## 2. Nueva Jerarquía de Archivos y Responsabilidades

```
portix/
├── boot/                      # Cargador de arranque dual BIOS (16/32/64 bits)
│   ├── boot.asm              # MBR LBA0 — arranque en modo real 16-bit
│   └── stage2.asm            # Transición a Modo Protegido y Modo Largo 64-bit
├── kernel/                    # Núcleo 100% Rust bare-metal (`no_std`, zero crates)
│   ├── src/
│   │   ├── arch/             # IDT, GDT, TSS, PIT, ISRs x86_64
│   │   ├── mem/              # Buddy Allocator (64 MB), Paginación x86_64 (CR3)
│   │   ├── fs/               # VFS abstracto, FAT32 driver con particionado MBR
│   │   ├── net/              # Intel e1000 NIC driver, ARP, IPv4, UDP, DHCP
│   │   ├── process/          # Procesos Ring 3, fork, exec, waitpid, scheduler
│   │   ├── lkm/              # Subsistema de módulos dinámicos (ELF rel, ksyms)
│   │   ├── elf.rs            # Cargador de binarios ELF de usuario
│   │   ├── syscall.rs        # Despachador de syscalls POSIX, red, LKM y gráficos
│   │   └── main.rs           # Inicialización y consola interactiva
│   ├── x86_64-portix.json    # Target bare-metal del kernel
│   └── Cargo.toml
├── userland/                  # Workspace Profesional de Espacio de Usuario en Rust
│   ├── Cargo.toml            # Orquestador del workspace (portix-rt + 15 crates)
│   ├── x86_64-userland.json  # Target bare-metal para ejecutables Ring 3
│   ├── linker.ld             # Linker script Ring 3 (ALIGN 4KB, vaddr 0x20000000)
│   ├── crates/
│   │   └── portix-rt/        # Runtime estándar de Portix en puro Rust
│   │       └── src/
│   │           ├── syscall.rs    # Invocaciones asm `syscall` x86_64 Ring 3
│   │           ├── entry.rs      # Punto de entrada `_start` -> `main`
│   │           ├── io.rs         # Macros `print!`, `println!`
│   │           ├── allocator.rs # GlobalAlloc en Ring 3 respaldado por `sys_brk`
│   │           ├── panic.rs      # Panic handler con volcado a COM1 / stdout
│   │           ├── mem.rs        # memcpy, memset, memcmp, memmove, strlen en Rust
│   │           ├── fs.rs         # Abstracción `File`, `OpenOptions`, `Stat`
│   │           ├── net.rs        # Abstracción `UdpSocket`, sockets BSD
│   │           ├── process.rs    # `fork()`, `waitpid()`, `exit()`, `getpid()`
│   │           ├── lkm.rs        # `init_module()`, `delete_module()`, `query_module()`
│   │           └── gfx.rs        # `mmap_device()`, primitivas de dibujo y buffer
│   └── bin/                  # Binarios de usuario en Rust independiente
│       ├── hello/            # "Hola Mundo" desde Ring 3
│       ├── uptime/           # Consulta de ticks y tiempo del sistema
│       ├── echo/             # Utilidad de impresión de argumentos
│       ├── clear/            # Limpieza de pantalla VT100
│       ├── cat/              # Lector y visualizador de archivos VFS
│       ├── ls/               # Listador de directorios VFS/FAT32
│       ├── mkdir/            # Creador de directorios
│       ├── curl/             # Cliente HTTP / UDP para pruebas de red
│       ├── lsmod/            # Consulta de módulos LKM cargados en Ring 0
│       ├── insmod/           # Inyección dinámica de módulos ELF reubicables
│       ├── rmmod/            # Descarga y liberación de módulos LKM
│       ├── sh/               # Intérprete de comandos de Ring 3
│       ├── init/             # Proceso init (PID 1)
│       ├── test_posix/       # Suite de 9 pruebas de conformidad POSIX
│       └── portix-wm/        # Servidor de ventanas y compositor gráfico Ring 3
├── modules/                   # Módulos de Kernel LKM
│   └── demo_mod/             # Módulo en puro Rust (`no_std`)
│       ├── src/lib.rs        # Símbolos `init_module`, `cleanup_module`
│       └── Cargo.toml
├── legacy/                    # Archivo histórico (código C retirado)
│   └── c_archive/
├── scripts/                   # Automatización de Construcción y Testing CI
│   ├── build.py              # Compilación, empaquetado ISO/IMG/VDI/VMDK
│   └── headless_runner.py    # Suite de pruebas headless automatizada (COM1)
```

---

## 3. Resolución de Desafíos de Ingeniería de Sistemas

Durante el proceso de migración y profesionalización se identificaron y solucionaron aspectos arquitectónicos críticos de bajo nivel:

1. **Alineación de Segmentos ELF y Aislamiento de Páginas (4KB Boundaries)**:
   - *Problema*: Los scripts de enlace previos utilizaban alineaciones pequeñas (`ALIGN(16)`), causando que los segmentos `PT_LOAD` `.text` (R-X) y `.rodata`/`.data` (RW-) compartieran la misma página virtual de 4KB. Al mapear el segundo segmento en `kernel/src/elf.rs`, se sobreescribía la entrada de la tabla de páginas, corrompiendo el código binario.
   - *Solución*: Se actualizaron `userland/linker.ld`, `build/linker.ld` y el cargador `elf.rs` con `ALIGN(0x1000)`. Se añadió lógica en `elf.rs` para reutilizar páginas ya asignadas y combinar banderas de protección de forma no destructiva.

2. **Acceso a Memoria de Dispositivos (VBE) y el Bit No-Execute (NX)**:
   - *Problema*: Al invocar `sys_mmap_device` desde el servidor de ventanas de Ring 3 (`portix-wm`), la CPU disparaba un `#PF` con código de error `EC=0x0E` (violación de bit reservado) al escribir en el framebuffer mapped en `0x4000_0000_0000`.
   - *Solución*: Se determinó que el flag `NO_EXECUTE` (bit 63) estaba siendo establecido en las entradas de la tabla de páginas sin que el bit `EFER.NXE` estuviese habilitado en el registro MSR 0xC0000080. Al corregir las banderas a `PRESENT | WRITABLE | USER | ACCESSED`, el mapeo de hardware funcionó inmediatamente a máxima velocidad sin excepciones.

3. **Arquitectura Gráfica Desacoplada (Ruta 1 - Ring 3)**:
   - Se diseñó el compositor `portix-wm` en Ring 3. El kernel únicamente expone `sys_mmap_device`, permitiendo al compositor gestionar su propia memoria de video, renderizar ventanas con sombras, barras de títulos y tipografía, y cerrarse limpiamente devolviendo la pantalla al modo texto sin comprometer la estabilidad del sistema operativo.

---

## 4. Resultados de Verificación y Validación (CI Suite)

La suite de validación automatizada `scripts/headless_runner.py` ejecutó 40 pruebas de integración de extremo a extremo con un resultado de **40/40 superadas (100% PASS, 0 fallos)**:

- Inicialización de hardware y subsistemas del kernel: Buddy Allocator, Paging, PIT, VBE, ATA, FAT32, e1000, DHCP, LKM.
- Comandos interactivos de consola y persistencia en sistema de archivos FAT32.
- Inyección, consulta, ejecución y descarga de módulos dinámicos LKM en Rust tanto desde la shell como mediante binarios Ring 3 (`insmod`, `lsmod`, `rmmod`).
- Servidor de ventanas desacoplado Ring 3 (`portix-wm`) con mapeo de framebuffer.
- Suite de conformidad POSIX completa (`/bin/test_posix`): `getpid`, `time`, `getcwd`, `chdir`, `pipe`, `stat`, `fstat`, `lseek`, y `fork` + `waitpid` sincronizado entre procesos padre e hijo.
