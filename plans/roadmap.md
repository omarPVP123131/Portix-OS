# PORTIX — Roadmap Ring-3 (20 Fases)

Evolución de Portix de demo bare-metal a SO completo, multiproceso,
multi-usuario, auto-suficiente y con soporte de red.

---

## Fase 0 — Page Table Infrastructure

**Objetivo**: abstraer la manipulacion de tablas de paginacion. El codigo actual
modifica PDE/PTE a mano con punteros volatiles. Necesitamos una capa portable.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 0.1 | `paging.rs` modulo base | Archivo `kernel/src/mem/paging.rs` con indices PML4/PDPT/PD/PT, constantes de flags |
| 0.2 | `read_cr3()` / `write_cr3()` | Leer/escribir CR3 en asm inline |
| 0.3 | `flush_tlb()` / `flush_tlb_page(vaddr)` | Invalidar TLB via `mov cr3` o `invlpg` |
| 0.4 | `translate(cr3, vaddr)` | Walk the page table: devuelve direccion fisica o None |
| 0.5 | `map_page(cr3, vaddr, paddr, flags)` | Recorrer levels, crear PT intermedios si no existen, escribir PTE |
| 0.6 | `unmap_page(cr3, vaddr)` | Limpiar PTE, invalidar TLB |
| 0.7 | `new_address_space()` | Allocar una copia de la PML4 actual para un proceso ring-3 |
| 0.8 | `free_address_space(cr3)` | Recorrer y liberar todas las tablas intermedias |
| 0.9 | Debug: `dump_page_tables()` | Imprimir estructura de paginas via serial |

### Flags definidas

```
PRESENT=1, WRITABLE=2, USER=4, WRITE_THROUGH=8, CACHE_DISABLE=16,
ACCESSED=32, DIRTY=64, HUGE_PAGE=128, GLOBAL=256, NO_EXECUTE=1<<63
```

### Debug serial

Cada `map_page` y `unmap_page` imprime por serial la operacion, direccion y flags.

---

## Fase 1 — Safe User Memory Access

**Objetivo**: el kernel pueda leer/escribir memoria de ring-3 sin risk de
#PF que mate el kernel. Necesario para syscalls como `SYS_WRITE` donde el
usuario pasa un puntero a su buffer.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 1.1 | `copy_from_user(dst, src, count)` | Copiar `count` bytes desde `*src` ring-3 a `*dst` kernel. Protegido contra #PF mediante probe. |
| 1.2 | `copy_to_user(dst, src, count)` | Analog para escribir a ring-3 desde kernel |
| 1.3 | `probe_readable(ptr, len)` | Verificar todo el rango es accesible ring-3 sin fault |
| 1.4 | Mecanismo #PF recovery | Cuando `copy_from_user` falla, el #PF handler reconoce fault "esperado" y retorna error en vez de panic. Flag per-CPU `expect_user_fault`. |
| 1.5 | Refactor `sys_write` | Usar `copy_from_user` en vez de `slice::from_raw_parts` directo |

### Logging

Cada copia imprime: `[PAGING] copy_from_user: addr=0x... count=N OK/FAIL`.

---

## Fase 2 — Process Model

**Objetivo**: estructura de datos para representar un proceso ring-3. Crear,
terminar, enumerar. Integracion con TSS para switch de pila ring-3→ring-0.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 2.1 | `Process` struct | `pid, state, name, cr3, kernel_rsp, user_rsp, user_rip, kernel_stack, user_stack_base, exit_code` |
| 2.2 | `ProcessState` enum | `Ready, Running, Blocked, Zombie` |
| 2.3 | `process_create(entry, name)` | Asigna PID, allocate kernel stack + user stack, crea address space, prepara frame IRETQ |
| 2.4 | `process_exit(pid, code)` | Libera paginas, kernel stack, quita de la tabla |
| 2.5 | Process table | Array fijo de `MAX_PROCS` (64) con slot bitmap |
| 2.6 | `current_process()` | Retorna `&mut Process` (puntero al running) |
| 2.7 | TSS sync | Al cambiar proceso, actualizar `TSS.RSP0` al `kernel_rsp` del proceso |

### Debug serial

Al crear proceso: `[PROC] create PID=1 name='demo' entry=0x... cr3=0x...`
Al exit: `[PROC] exit PID=1 code=0`

---

## Fase 3 — ELF64 Loader

**Objetivo**: cargar binarios ELF64 desde FAT32 a memoria de usuario, parsear
headers, mapear segmentos, setear stack con argv.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 3.1 | `ElfHeader` / `ProgramHeader` parsing | Validar magic (0x7F ELF), class=64-bit, endian=LE, type=ET_EXEC, machine=x86-64 |
| 3.2 | Segment loading | Por cada `PT_LOAD`: `map_page` desde `p_vaddr` con datos del offset del archivo, flags segun `p_flags` (R/W/X) |
| 3.3 | BSS zeroing | Si `p_memsz > p_filesz`, mapear paginas extra y zero-fill |
| 3.4 | User stack setup | Mapear stack de usuario (ej: 64 KB en 0x7FFF_0000_0000...), empujar argv, envp, auxv |
| 3.5 | `elf_load(path) -> Result<ElfLoader>` | Abrir archivo FAT32, parsear, devolver entry point + total_size + stack_size |
| 3.6 | `elf_load_raw(data) -> Result<ElfLoader>` | Cargar desde slice en memoria (debugging) |

### Logging

`[ELF] loading /bin/hello: entry=0x... segments=3 stack=64K`

---

## Fase 4 — Preemptive Scheduler

**Objetivo**: Round-Robin scheduler con time-slice de 10ms (100 Hz PIT).
Context switch real entre procesos.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 4.1 | Quantum | `TIME_SLICE = 1` tick (10 ms) por proceso |
| 4.2 | `switch_to(next_pid)` | Guardar RSP actual en `current.kernel_rsp`, restaurar `next.kernel_rsp`, cambiar CR3, actualizar TSS.RSP0, `ret` al contexto next |
| 4.3 | `schedule()` | Elegir next proceso Ready, llamar `switch_to` |
| 4.4 | IRQ0 hook | En `irq0_handler`: tick PIT → si proceso agoto quantum, `schedule()` |
| 4.5 | `SYS_YIELD` | Syscall que cede time slice voluntariamente |
| 4.6 | `SYS_SLEEP(ticks)` | Marcar proceso como Blocked, despertar despues de N ticks |

### Context switch detalle

El scheduler RUNEA dentro del handler IRQ0. El stack en ese momento:
`[POP_REGS] [IRET frame]`. El scheduler debe reemplazar los registros
guardados en el stack por los del nuevo proceso, y modificar el RIP del
IRET frame para que `iretq` salte al nuevo proceso.

### Logging

`[SCHED] switch: PID 1 (demo) → PID 2 (shell)  ticks=142`

---

## Fase 5 — Ring-3 Exception Handling

**Objetivo**: cuando un proceso ring-3 causa #PF, #GP, #UD, etc., el kernel
NO debe panickear. Debe matar el proceso y seguir.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 5.1 | Detectar ring-3 en handlers | En cada handler, chequear `CS & 3 == 3` desde el stack de interrupcion |
| 5.2 | `kill_current_process(reason)` | Imprime diagnostic, llama `process_exit`, luego `schedule()` |
| 5.3 | Page fault recovery | Si el fault ocurrio durante `copy_from_user` (flag per-CPU `expect_user_fault`), NO kill — solo retorna error |
| 5.4 | Mensaje de muerte | `[EXCEPTION] PID 1 killed: #PF at RIP=0x... CR2=0x...` |

### Excepciones manejadas

- #PF (page fault): null pointer, invalid access, COW
- #GP (general protection): instruccion privilegiada en ring-3
- #UD (undefined): instruccion invalida
- #DE (divide error): division por cero
- #NM: FPU no disponible

---

## Fase 6 — System Calls Completo

**Objetivo**: set completo de syscalls para programas ring-3 utiles.

### Tabla de syscalls

| # | Nombre | Args | Descripcion |
|---|--------|------|-------------|
| 0 | `SYS_EXIT` | `(code)` | Termina proceso con codigo de salida |
| 1 | `SYS_WRITE` | `(fd, buf, len)` | Escribir a fd (1=stdout, 2=stderr) |
| 2 | `SYS_GETPID` | `()` | Retorna PID del proceso actual |
| 3 | `SYS_READ` | `(fd, buf, len)` | Leer desde fd (stdin = teclado) |
| 4 | `SYS_OPEN` | `(path, flags)` | Abrir archivo FAT32, retorna fd |
| 5 | `SYS_READFILE` | `(fd, buf, len)` | Leer de fd abierto |
| 6 | `SYS_BRK` | `(addr)` | Heap expansion (sbrk) |
| 7 | `SYS_MMAP` | `(addr, len, prot, flags)` | Mapear memoria anonima o device |
| 8 | `SYS_YIELD` | `()` | Ceder CPU |
| 9 | `SYS_SLEEP` | `(ticks)` | Dormir N ticks |
| 10 | `SYS_CLOSE` | `(fd)` | Cerrar fd |
| 11 | `SYS_GETDIRENTS` | `(fd, buf, len)` | Leer entrada de directorio |
| 12 | `SYS_EXECVE` | `(path, argv, envp)` | Reemplazar proceso con otro binario |
| 13 | `SYS_DUP2` | `(oldfd, newfd)` | Duplicar fd |

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 6.1 | Expand dispatch | Array de function pointers indexado por numero de syscall |
| 6.2 | File descriptor table | Por proceso: Vec<(path, offset, mode)> |
| 6.3 | `sys_open` | Resolver path en FAT32, guardar referencia al file + offset |
| 6.4 | `sys_brk` | Administrar heap: mapear/desmapear paginas en zona program break |
| 6.5 | `sys_mmap` | Mapear paginas anonimas en el addr solicitado |
| 6.6 | `sys_execve` | Cerrar fd viejos, `elf_load(path)`, reemplazar address space + regs |

### Logging

`[SYSCALL] PID 1: open('/home/user/test.txt', 0) → fd=3`

---

## Fase 7 — libportix (C Runtime Ring-3)

**Objetivo**: proveer a los programas ring-3 un runtime minimo para escribir
en C o Rust sin dependencias del kernel.

### Componentes

| Componente | Descripcion |
|------------|-------------|
| `crt0.s` | `_start` en asm: llama `_init`, `main(argc, argv, envp)`, `exit()` |
| `stdio.c` | `printf`, `puts`, `fgets`, `fputs`, `sprintf` |
| `stdlib.c` | `malloc`, `free`, `calloc`, `realloc` (via SYS_BRK) |
| `file.c` | `fopen`, `fread`, `fwrite`, `fclose`, `fseek` (via syscalls) |
| `string.c` | `memcpy`, `memset`, `strlen`, `strcmp`, `strcpy` |
| `portix.h` | Header principal con todas las declaraciones |
| `libportix.a` | Libreria estatica para linkear con `-lportix` |

### Toolchain

Script `scripts/ring3-toolchain.sh` que:
1. Compila `libportix.a` con cross-gcc x86_64-elf
2. Ensambla `crt0.s`
3. Programa ejemplo: `x86_64-elf-gcc -ffreestanding -nostdlib -static -lportix -o hello.elf hello.c`

### Logging

`[PORTS] building libportix v1.0 — crt0 stdio stdlib file string`

---

## Fase 8 — Init + Shell + User Programs

**Objetivo**: el sistema arranca con un init ring-3 que lanza un shell.
El usuario puede ejecutar comandos, navegar el FS, editar archivos.

### Componentes

| Programa | Descripcion |
|----------|-------------|
| `/bin/init` | Primer proceso al boot. Lee `/etc/inittab`, lanza shell en terminal |
| `/bin/sh` | Shell ring-3 minimal: prompt `portix$ `, ejecuta programas con PATH |
| `/bin/ls` | Lista directorio con `SYS_GETDIRENTS` |
| `/bin/cat` | Concatena archivos con `SYS_READFILE` |
| `/bin/echo` | Imprime argumentos |
| `/bin/clear` | Limpia terminal (escape codes) |
| `/bin/help` | Lista comandos disponibles |
| `/bin/hello` | Demo "Hello from Ring 3!" |
| `/bin/uptime` | Muestra tiempo desde boot via `SYS_SLEEP(0)` + PIT ticks |

### Boot sequence

```
kernel init → FAT32 mount → find /bin/init → process_create(init) → shell prompt
```

### Logging

`[INIT] starting /bin/sh on terminal`

---

## Fase 9 — FAT32 Userspace Driver

**Objetivo**: mover ATA + FAT32 a ring-3. El kernel deja de hablarle al disco
directamente. Delega toda IO de bloques a un proceso driver ring-3.

### Arquitectura

```
Kernel ring-0:          Driver ring-3:
  block_request(pid)  ←  [ata_driver]
  block_response()    →  [ata_driver]
                        ╰→ ATA PIO DMA commands
                        ╰→ FAT32 parse + cache
                        ╰→ /dev/sda0 → block device interface via IPC
```

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 9.1 | `block_request()` syscall | Kernel: `SYS_BLOCK_READ(dev, lba, count, buf)` — driver ring-3 recibe y ejecuta |
| 9.2 | `ata_driver` ring-3 | Proceso que maneja ATA PIO, registra IRQ14/15, responde peticiones block |
| 9.3 | `fat32_driver` ring-3 | Proceso que monta FAT32 sobre block device, responde `open/read/write` |
| 9.4 | IRQ forwarding | Kernel reenvia IRQ14/15 al driver ATA via mensaje IPC |

### Logging

`[BLK] ata0: read LBA=100 count=4 → 2048 bytes`

---

## Fase 10 — VFS + Mount + Multiple FS

**Objetivo**: Virtual Filesystem layer que abstrae diferentes sistemas de
archivos bajo una misma API.

### Arquitectura

```
syscall open/read/write/getdents
        ↓
    VFS layer (ring-0 o ring-3)
        ↓
  ┌─────┼─────┐
FAT32  ext2  ramfs  (todos drivers ring-3)
```

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 10.1 | `vnode` abstraction | `{ type: File|Dir|Mount, ops: *VnodeOps, private: *mut () }` |
| 10.2 | `mount(dev, path, fstype)` | Registrar un filesystem en un punto de montaje |
| 10.3 | `VnodeOps` trait | `open`, `read`, `write`, `getdents`, `truncate` |
| 10.4 | Ramfs driver | FS simple en memoria para `/tmp`, `/dev`, `/proc` |
| 10.5 | ext2 driver (opcional) | Segundo FS real para contrastar con FAT32 |
| 10.6 | Path resolution | `/home/user/file.txt` → walk vnodes atravesando mounts |

### Logging

`[VFS] mount /dev/sda0 → /home (fat32)`, `[VFS] mount ramfs → /tmp (ramfs)`

---

## Fase 11 — IPC System

**Objetivo**: sistema de mensajes entre procesos para permitir arquitectura
microkernel (drivers en ring-3, servicios en ring-3).

### API

| Syscall | Args | Descripcion |
|---------|------|-------------|
| `SYS_SEND` | `(pid_dest, buf, len)` | Enviar mensaje a proceso destino (bloqueante si no hay buffer) |
| `SYS_RECV` | `(buf, len)` | Recibir mensaje (bloqueante si no hay) |
| `SYS_REG_IRQ` | `(irq, pid)` | Registrar un proceso como handler de una IRQ |

### Diseno

- Mensajes de tamaño fijo (64 bytes) para simplicidad
- Kernel mantiene cola circular por par (sender, receiver)
- Notificacion de IRQ: kernel envia mensaje especial al driver registrado
- Timeout opcional

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 11.1 | `IpcMessage` struct | `{ src_pid, dst_pid, type, data[56] }` |
| 11.2 | Kernel queues | Per-process mailbox: `VecDeque<IpcMessage>` |
| 11.3 | `SYS_SEND` implementation | Copy from user via `copy_from_user`, enqueue al destino |
| 11.4 | `SYS_RECV` implementation | Dequeue del mailbox, copy to user via `copy_to_user`. Bloquear si empty. |
| 11.5 | IRQ registration | Map IRQ number → handler PID. Enviar mensaje en irq_handler. |

### Logging

`[IPC] PID 1 → PID 2: msg type=1 size=24`, `[IPC] IRQ14 → ata_driver (PID 3)`

---

## Fase 12 — Userspace Drivers

**Objetivo**: migrar los drivers de dispositivo actuales a procesos ring-3
independientes. Cada driver = proceso separado, comunicado por IPC.

### Drivers a migrar

| Driver | Proceso | IPC messages | IRQs |
|--------|---------|-------------|------|
| ATA/ATAPI | `ata_drv` | BLOCK_READ, BLOCK_WRITE | 14, 15 |
| PS/2 Keyboard | `kbd_drv` | KBD_EVENT (scancode) | 1 (IRQ1) |
| PS/2 Mouse | `mouse_drv` | MOUSE_EVENT (dx,dy,buttons) | 12 (IRQ12) |
| Framebuffer/VESA | `fb_drv` | FB_WRITE, FB_BLIT, FB_INFO | — |
| PCI | `pci_drv` | PCI_SCAN, PCI_READ_CONFIG | — |
| Serial COM1 | `serial_drv` | SERIAL_WRITE, SERIAL_READ | 4 (IRQ4) |

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 12.1 | Early boot: kernel init drivers minimo | El kernel mantiene solo serial + PIT para debug. Todo lo demas se delega. |
| 12.2 | `IOPort` permission | `SYS_IOPORT(enable, port)` — permitir a driver ring-3 usar `in`/`out` | 
| 12.3 | MMIO mapping | `SYS_MMAP_DEVICE(phys, size)` — mapear framebuffer/BARs PCI en ring-3 |
| 12.4 | Driver lifecycle | `probe() → init() → handle_irq() → ioctl()` estandar |
| 12.5 | `/dev/` filesystem | Cada driver expone device node: `/dev/kbd`, `/dev/mouse`, `/dev/fb0`, `/dev/sda0` |

### Logging

`[DRV] ata_drv (PID 3): registered IRQ14`, `[DRV] kbd_drv (PID 4): registered IRQ1`

---

## Fase 13 — Networking Stack

**Objetivo**: pila TCP/IP en ring-3 para que Portix hable por la red.

### Stack components

| Componente | Descripcion |
|------------|-------------|
| NIC driver | RTL8139 o e1000 en ring-3 (MMIO + PCI BAR) |
| Ethernet layer | ARP, MAC resolution |
| IP layer | IPv4, routing table |
| Transport | TCP, UDP |
| Socket API | `socket()`, `bind()`, `listen()`, `accept()`, `connect()`, `send()`, `recv()` |

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 13.1 | NIC driver ring-3 | RTL8139 PCI device, init MAC, TX/RX rings, IRQ forwarding |
| 13.2 | Ethernet + ARP | Parse/send ethernet frames, ARP cache |
| 13.3 | IPv4 + ICMP | Packet send/receive, ping reply, checksum |
| 13.4 | UDP | Connectionless datagrams |
| 13.5 | TCP | State machine, retransmit, windowing |
| 13.6 | Socket syscalls | 6 new syscalls: `socket/bind/listen/accept/connect/close` |
| 13.7 | DHCP client | Obtener IP automaticamente al boot |

### Logging

`[NET] eth0: MAC 52:54:00:12:34:56 UP`, `[NET] DHCP: lease 10.0.2.15`

### Bonus: HTTP server

Programa ring-3 `httpd` que sirve archivos via HTTP.

---

## Fase 14 — Multi-User + Security

**Objetivo**: Portix se vuelve un SO multi-usuario con permisos, login,
y capabilities.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 14.1 | UID/GID | Cada proceso tiene `uid, gid, euid, egid` |
| 14.2 | `/etc/passwd` | Archivo con usuarios, home dirs, shell |
| 14.3 | `/etc/shadow` | Hash de passwords (SHA-512) |
| 14.4 | `login` program | Prompt user/pass, spawn shell con UID del usuario |
| 14.5 | `chmod` / `chown` | Permisos rwx por owner/group/other |
| 14.6 | `execve` setuid | Permitir SUID binaries |
| 14.7 | Capabilities | `CAP_NET_RAW`, `CAP_SYS_ADMIN`, `CAP_DAC_OVERRIDE` |

### Logging

`[AUTH] login: omar OK (UID=1000)`, `[SEC] PID 5 (httpd) dropped CAP_NET_RAW`

---

## Fase 15 — SMP + Multi-Core

**Objetivo**: soporte para multiples CPUs/core via ACPI MADT + SIPI.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 15.1 | Parse MADT (APIC) | Local APIC base, I/O APIC, CPU entries |
| 15.2 | BSP init APs | Send INIT-SIPI-SIPI IPI sequence |
| 15.3 | Per-CPU struct | `{ id, current_process, kernel_stack, idle_process }` |
| 15.4 | Spinlocks | `spin_lock` / `spin_unlock` with `xchg` / `cmpxchg` |
| 15.5 | Per-CPU scheduler | Cada CPU tiene su propia cola de procesos ready |
| 15.6 | IRQ balancing | Distribuir IRQs entre CPUs via I/O APIC redirection |

### Logging

`[SMP] APIC ID 0 (BSP)`, `[SMP] APIC ID 1 (AP) started`, `[SMP] 2 CPUs online`

---

## Fase 16 — Dynamic Linker

**Objetivo**: programas ring-3 pueden compartir `.so` en vez de ser
statically linked. Reduce tamano de binarios y permite actualizar librerias.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 16.1 | `ld-portix.so` | Linker en ring-3: se carga al iniciar un ELF dinámico |
| 16.2 | PLT/GOT | Procedure Linkage Table + Global Offset Table |
| 16.3 | Lazy binding | Resolver symbolos solo cuando se llaman (first call) |
| 16.4 | `dlopen` / `dlsym` | Cargar librerias en runtime |
| 16.5 | Shared lib path | `/usr/lib/libc.so`, `/usr/lib/libm.so` |

### Logging

`[LINK] loading /usr/lib/libc.so (PID 1)`, `[LINK] resolved printf → 0x7F00_1234`

---

## Fase 17 — Power Management + ACPI

**Objetivo**: suspender, reanudar, escalar frecuencia, monitorear bateria.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 17.1 | Parse RSDP, RSDT/XSDT | Root pointer via EBDA o UEFI config table |
| 17.2 | DSDT/SSDT tables | Differentiated System Description Table |
| 17.3 | S3 sleep | Suspend-to-RAM: save CPU state, enter ACPI S3 |
| 17.4 | CPU frequency scaling | P-states via ACPI `_PSS` / `_PPC` |
| 17.5 | Battery monitoring | ACPI `_BIF`, `_BST` — percentage, rate, voltage |

### Logging

`[ACPI] S3 wake: resume from 0x...`, `[PM] CPU freq: 2.4 GHz → 1.2 GHz (powersave)`

---

## Fase 18 — Init System + Service Manager

**Objetivo**: sistema de inicio moderno tipo systemd/s6: unidades, dependencias,
parallel startup, watchdog, socket activation.

### Componentes

| Componente | Descripcion |
|------------|-------------|
| `/sbin/init` | PID 1. Lee `/etc/services/`, resuelve dependencias, arranca en orden |
| Service units | Archivos TOML: `[service] name="httpd" exec="/usr/sbin/httpd" depends=["net"]` |
| Parallel startup | Servicios sin dependencias mutuas arrancan simultaneamente |
| Watchdog | Si un servicio no responde heartbeat, reiniciarlo |
| Socket activation | Escuchar socket antes de que el servicio exista |
| Logging | `journal` en `/var/log/` |

### Logging

`[INIT] starting httpd (dep: net, fs)`, `[INIT] all 12 services up in 0.34s`

---

## Fase 19 — Self-Hosting

**Objetivo**: Portix puede compilar su propio kernel. Toolchain completa
corriendo nativamente.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 19.1 | Port GCC | Cross-compilar GCC a target `x86_64-portix` |
| 19.2 | Port Binutils | as, ld, objcopy corriendo en Portix |
| 19.3 | `make` port | Build system para compilar el kernel |
| 19.4 | `git` port | Clonar repo desde github en Portix |
| 19.5 | Compilar kernel nativo | `cd /src/portix && make && cp kernel.bin /boot/` |
| 19.6 | Reboot con nuevo kernel | `shutdown -r` → bootea el kernel recien compilado |

### Logging

`[SELF] building kernel from /src/portix...`, `[SELF] kernel.bin: 485 KB OK`

---

## Fase 20 — POSIX Compatibility

**Objetivo**: suficiente POSIX para correr programas reales (DOOM, busybox,
web server, gdb stub).

### Componentes

| Componente | Descripcion |
|------------|-------------|
| `fork()` | Copy-on-Write via page fault |
| `signal()` | Syscall `SYS_KILL`, `SYS_SIGNAL`, signal handlers en ring-3 |
| `pipe()` | Pipe syscall con buffer en kernel |
| `select()` / `poll()` | I/O multiplexing sobre fds |
| `termios` | Terminal I/O control, raw mode, echo on/off |
| `pty` | Pseudoterminal para ssh |
| `gdb stub` | Remote serial protocol para debuggear programas ring-3 |

### Ports targets

| Program | Estado esperado |
|---------|----------------|
| DOOM (1993) | Corriendo en framebuffer ring-3 |
| busybox | Shell + coreutils + networking |
| micro_httpd | Servidor web minimal |
| gdb stub | Debug remoto desde QEMU |

### Logging

`[POSIX] fork: PID 10 → PID 11 (COW)`, `[GDB] stub ready on serial'

---

## Timeline estimado (fases 0-20)

```
Fase  0: ████████░░░░░░░░░░░░░░░░░░░░  1-2 semanas
Fase  1: ██████████░░░░░░░░░░░░░░░░░░  1 semana
Fase  2: ██████████████░░░░░░░░░░░░░░  2 semanas
Fase  3: ██████████████████░░░░░░░░░░  2 semanas
Fase  4: ██████████████████████░░░░░░  2 semanas
Fase  5: ████████████████████████░░░░  1 semana
Fase  6: ████████████████████████████  3 semanas
Fase  7: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  3 semanas
Fase  8: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  3 semanas
Fase  9: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  3 semanas
Fase 10: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  3 semanas
Fase 11: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  2 semanas
Fase 12: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  4 semanas
Fase 13: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  6 semanas
Fase 14: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  2 semanas
Fase 15: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  4 semanas
Fase 16: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  4 semanas
Fase 17: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  4 semanas
Fase 18: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  4 semanas
Fase 19: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  8 semanas
Fase 20: ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  8 semanas

Total estimado: ~60 semanas (14 meses) con 1 persona
```

---

## Arbol de directorios final (proyectado)

```
portix/
├── boot/               # Stage1 + Stage2 asm
├── kernel/             # Kernel ring-0 minimo
│   ├── src/
│   │   ├── arch/       # isr, idt, gdt, halt
│   │   ├── mem/        # paging.rs, allocator.rs
│   │   ├── process.rs  # scheduler + process table
│   │   ├── ipc.rs      # IPC core
│   │   └── syscall.rs  # dispatch table
├── drivers/            # Drivers ring-3
│   ├── ata/            # ATA PIO driver
│   ├── fat32/          # FAT32 driver
│   ├── kbd/            # PS/2 keyboard
│   ├── mouse/          # PS/2 mouse
│   ├── fb/             # Framebuffer
│   ├── pci/            # PCI bus
│   ├── serial/         # COM1
│   └── net/            # RTL8139/E1000 NIC
├── lib/                # librerias compartidas
│   ├── libc/           # libportix
│   ├── libm/           # math library
│   └── ld/             # dynamic linker
├── usr/                # Programas ring-3
│   ├── init/           # PID 1
│   ├── shell/          # /bin/sh
│   ├── coreutils/      # ls, cat, echo, etc
│   └── servers/        # httpd, sshd
├── scripts/            # Build + toolchain
└── plans/              # Roadmap y docs tecnicas
```

---

> Documento generado el 2026-06-07. Proximo update: al completar cada fase.
