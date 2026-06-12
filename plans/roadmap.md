# PORTIX — Roadmap Completo (40+ Fases)

Evolución de Portix de demo bare-metal a SO completo, multiproceso,
multi-usuario, auto-suficiente, con soporte de red, gráficos acelerados,
virtualización, contenedores y seguridad de nivel producción.

---

## Dependency Graph Principal (Fases 0–20)

```
Fase 0 (Paging)
  ↓
Fase 1 (User Memory) ──────────────────────────────→ Fase 5 (Exceptions)
  ↓                                                        ↑
Fase 2 (Process) ──────────────────────→ Fase 4 (Scheduler)
  ↓
Fase 3 (ELF) → Fase 6 (Syscalls) → Fase 7 (libportix C+Rust)
                      ↓
               Fase 8 (Init/Shell)
                      ↓
        Fase 11 (IPC) → Fase 9 (Userspace Drivers)
                              ↓
                        Fase 10 (VFS)
                              ↓
               Fase 12 (Drivers) → Fase 13 (Networking)
                              ↓
                        Fase 14 (Multi-User)
                              ↓
               Fase 15 (SMP) → Fase 16 (Dyn Linker)
                              ↓
               Fase 17 (ACPI) → Fase 18 (Init System)
                              ↓
                        Fase 19 (Self-Hosting)
                              ↓
                        Fase 20 (POSIX)
```

## Dependency Graph Extendido (Fases 21–40+)

```
Fase 20 (POSIX)
  ↓
Fase 21 (Graphics/DRM) → Fase 22 (Audio)
  ↓
Fase 23 (USB) → Fase 24 (Containers)
  ↓
Fase 25 (Package Manager) → Fase 26 (Crypto)
  ↓
Fase 27 (Virtualization/KVM) → Fase 28 (Security/SELinux)
  ↓
Fase 29 (Filesystems Avanzados) → Fase 30 (RAID)
  ↓
Fase 31 (Bluetooth) → Fase 32 (Thunderbolt/PCIe hotplug)
  ↓
Fase 33 (GPU Compute/Vulkan) → Fase 34 (NVMe)
  ↓
Fase 35 (Profiling/Tracing) → Fase 36 (Live Patching)
  ↓
Fase 37 (Distributed FS) → Fase 38 (Formal Verification)
  ↓
Fase 39 (RISC-V port) → Fase 40 (ARM64 port)
  ↓
Fase 41 (Embedded Profile) → Fase 42 (RTOS Mode)
```

---

## Progreso por Fase

| Fase | Nombre | Estado | Progreso | Última actualización |
|------|--------|--------|----------|---------------------|
| 0 | Page Table Infrastructure | ✅ Completado | ████████████████████ 100% | 2026-06-11 |
| 1 | Safe User Memory Access | ✅ Completado | ████████████████████ 100% | 2026-06-11 |
| 2 | Process Model | ✅ Completado | ████████████████████ 100% | 2026-06-10 |
| 3 | ELF64 Loader | ✅ Completado | ████████████████████ 100% | 2026-06-10 |
| 4 | Preemptive Scheduler | ✅ Completado | ████████████████████ 100% | 2026-06-10 |
| 5 | Ring-3 Exception Handling | ✅ Completado | ████████████████████ 100% | 2026-06-10 |
| 6 | System Calls Completo | ✅ Completado | ████████████████████ 100% | 2026-06-11 |
| 7 | libportix (C + Rust Runtime) | ✅ Completado | ████████████████████ 100% | 2026-06-11 |
| 8 | Init + Shell + User Programs | ✅ Completado | ████████████████████ 100% | 2026-06-11 |
| 9 | FAT32 Userspace Driver | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 10 | VFS + Mount + Multiple FS | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 11 | IPC System | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 12 | Userspace Drivers | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 13 | Networking Stack | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 14 | Multi-User + Security | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 15 | SMP + Multi-Core | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 16 | Dynamic Linker | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 17 | Power Management + ACPI | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 18 | Init System + Service Manager | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 19 | Self-Hosting | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 20 | POSIX Compatibility | ⏳ Pendiente | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 21 | Graphics Acceleration (DRM/KMS) | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 22 | Audio Stack | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 23 | USB Stack | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 24 | Containers + Namespaces | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 25 | Package Management | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 26 | Crypto Stack | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 27 | Virtualization (KVM-style) | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 28 | Mandatory Access Control | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 29 | Filesystems Avanzados | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 30 | RAID + Volume Manager | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 31 | Bluetooth Stack | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 32 | PCIe Hotplug + Thunderbolt | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 33 | GPU Compute + Vulkan ICD | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 34 | NVMe + AHCI | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 35 | Profiling + Tracing (perf-like) | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 36 | Live Kernel Patching | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 37 | Distributed Filesystem | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 38 | Formal Verification | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 39 | RISC-V Port | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 40 | ARM64 Port (AArch64) | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 41 | Embedded/IoT Profile | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |
| 42 | Hard RTOS Mode | 📋 Planificado | ░░░░░░░░░░░░░░░░░░░░ 0% | — |

**Progreso total**: 9 / 43 fases completadas (21%)
**Tiempo restante estimado**: ~115 semanas (~2.2 años) con 1 persona

---

## Fase 0 — Page Table Infrastructure
> **Badge**: ✅ `COMPLETADO` · 100% · Actualizado 2026-06-11

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

### Criterios de aceptación

- [x] `translate(cr3, vaddr)` devuelve dirección física correcta para direcciones mapeadas
- [x] `translate` devuelve `None` para direcciones no mapeadas
- [x] `map_page` y `unmap_page` imprimen log correcto por serial
- [x] `new_address_space()` crea PML4 sin corromper el espacio del kernel
- [x] `free_address_space` libera todas las tablas sin memory leaks (verificado con frame counter)
- [x] `dump_page_tables()` produce output legible sin panic
- [x] Tests: round-trip map → translate → unmap → translate=None

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| TLB stale entries tras unmap | Media | Crítico | Siempre `invlpg` después de cada unmap |
| Corrupción de kernel mappings en `new_address_space` | Media | Crítico | Copiar solo entradas de nivel alto; nunca USER en rangos kernel |
| Fuga de frames en estructuras intermedias | Alta | Medio | Contador de frames; assert en teardown |

---

## Fase 1 — Safe User Memory Access
> **Badge**: ✅ `COMPLETADO` · 100% · Actualizado 2026-06-11

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

### Criterios de aceptación

- [x] `copy_from_user` con puntero NULL retorna error, NO panic
- [x] `copy_from_user` con puntero a kernel space retorna error
- [x] `copy_to_user` escribe datos correctos verificados desde ring-3
- [x] `probe_readable` detecta correctamente rangos inaccesibles
- [x] `sys_write` usa `copy_from_user`; buffer inválido no cuelga el kernel
- [x] Flag `expect_user_fault` per-CPU resetea correctamente después de cada uso
- [x] Serial log muestra `OK`/`FAIL` en cada operación

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Race condition en flag `expect_user_fault` en SMP | Alta | Crítico | Flag per-CPU en estructura GS-based; desactivar interrupciones durante copy |
| Puntero ring-3 que apunta a MMIO del kernel | Baja | Crítico | Verificar rango de dirección < `USER_MAX` antes de copiar |
| Overflow de `count` que cruza límite de página | Media | Alto | Verificar page-by-page en `probe_readable` |

---

## Fase 2 — Process Model
> **Badge**: ✅ `COMPLETADO` · 100% · Actualizado 2026-06-10

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

### Criterios de aceptación

- [x] `process_create` asigna PIDs únicos; no reutiliza PID hasta que slot sea liberado
- [x] IRETQ frame correctamente configurado: RIP=entry, CS=user CS, RFLAGS=IF, RSP=user stack top
- [x] `TSS.RSP0` apunta al kernel stack del proceso actual tras cada switch
- [x] `process_exit` libera 100% de frames (kernel stack + user stack + page tables)
- [x] Process table tolera 64 procesos simultáneos sin corrupción
- [x] `current_process()` nunca retorna proceso en estado Zombie
- [x] Serial log muestra create/exit con PID, nombre y entry point correctos

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Double-free de kernel stack si `process_exit` se llama dos veces | Media | Crítico | State machine: solo se puede hacer exit desde `Running`/`Ready` |
| PID reuse demasiado rápido (wraparound en stress test) | Baja | Medio | Generational PID (epoch + slot) |
| `current_process()` desactualizado en SMP | Alta (futuro) | Crítico | Placeholder; arreglar en Fase 15 con per-CPU pointer |

---

## Fase 3 — ELF64 Loader
> **Badge**: ✅ `COMPLETADO` · 100% · Actualizado 2026-06-10

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

### Criterios de aceptación

- [x] ELF con magic incorrecto retorna `Err`, no panic
- [x] Segmentos `PT_LOAD` se mapean en las direcciones exactas de `p_vaddr`
- [x] BSS region es cero tras load (verificar con `SYS_READ` desde ring-3)
- [x] argv/envp correctamente alineados en stack; `argc` en RSI, `argv` en RDI al entry
- [x] Segmento de solo lectura (R) no es writable (verificar #PF al escribir)
- [x] `elf_load_raw` produce mismo resultado que `elf_load` para mismo binario
- [x] Serial log muestra entry point, número de segmentos y tamaño de stack

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| ELF malicioso con `p_vaddr` en espacio kernel | Alta | Crítico | Rechazar cualquier segmento con vaddr ≥ `KERNEL_BASE` |
| Overlap de segmentos ELF | Baja | Alto | Verificar rangos antes de mapear |
| Stack overflow en binario con muchos argv | Media | Medio | Limitar argv total a 4 KB; retornar E2BIG |

---

## Fase 4 — Preemptive Scheduler
> **Badge**: ✅ `COMPLETADO` · 100% · Actualizado 2026-06-10

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

El scheduler corre dentro del handler IRQ0. El stack en ese momento:
`[POP_REGS] [IRET frame]`. El scheduler debe reemplazar los registros
guardados en el stack por los del nuevo proceso, y modificar el RIP del
IRET frame para que `iretq` salte al nuevo proceso.

### Logging

`[SCHED] switch: PID 1 (demo) → PID 2 (shell)  ticks=142`

### Criterios de aceptación

- [x] 2+ procesos corren simultáneamente sin starvation
- [x] Time-slice de 10ms medido con PIT (tolerancia ±1ms)
- [x] Context switch < 5μs (benchmark con TSC antes/después de `switch_to`)
- [x] `SYS_YIELD` cede CPU inmediatamente; siguiente proceso corre antes del próximo tick
- [x] `SYS_SLEEP(N)` bloquea exactamente N ticks ±1 tick
- [x] No deadlocks en stress test con 64 procesos activos durante 60 segundos
- [x] Serial log muestra switch cada ~10ms con PIDs correctos

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Stack kernel corrompido en context switch | Alta | Crítico | Verificar RSP alineación a 16 bytes; canary en top del kernel stack |
| CR3 incorrecto tras switch → TLB con address space equivocado | Media | Crítico | Flush TLB completo en cada switch (write CR3) |
| IRQ0 reentrant si switch tarda más de 10ms | Baja | Alto | Deshabilitar IRQ0 durante `schedule()`; reactivar al final |

---

## Fase 5 — Ring-3 Exception Handling
> **Badge**: ✅ `COMPLETADO` · 100% · Actualizado 2026-06-10

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

- `#PF` (page fault): null pointer, invalid access, COW
- `#GP` (general protection): instruccion privilegiada en ring-3
- `#UD` (undefined): instruccion invalida
- `#DE` (divide error): division por cero
- `#NM`: FPU no disponible

### Criterios de aceptación

- [x] Proceso ring-3 que hace `mov cr3, rax` muere con #GP; kernel sigue vivo
- [x] Proceso ring-3 con null pointer dereference muere con #PF; kernel sigue vivo
- [x] División por cero en ring-3 mata el proceso, no el kernel
- [x] `copy_from_user` con puntero inválido retorna error sin matar el proceso
- [x] Después de matar proceso por excepción, scheduler elige siguiente proceso Ready
- [x] Serial log imprime RIP, CR2 (en #PF), y razón de muerte para cada excepción
- [x] Todas las 5 excepciones listadas producen kill limpio, no triple fault

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Exception handler en ring-0 causa double fault | Media | Crítico | NMI stack separado (IST); guard page en kernel stack |
| `kill_current_process` llamado sin proceso activo (idle) | Baja | Alto | Guard: si `current_pid == IDLE_PID`, panic (es bug del kernel) |
| #NM sin soporte FXSAVE → estado FPU corrompido entre procesos | Media | Medio | Habilitar TS bit en CR0; lazy FPU save/restore |

---

## Fase 6 — System Calls Completo
> **Badge**: ✅ `COMPLETADO` · 100% · Actualizado 2026-06-11

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
| 14 | `SYS_UPTIME` | `()` | Retorna ticks de PIT desde boot |

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

### Criterios de aceptación

- [x] Número de syscall inválido retorna `-ENOSYS`, no crash
- [x] `SYS_OPEN` + `SYS_READFILE` + `SYS_CLOSE`: round-trip lee contenido correcto de archivo FAT32
- [x] `SYS_BRK(0)` retorna break actual; sucesivas llamadas expanden heap sin gaps
- [x] `SYS_EXECVE` reemplaza address space completamente; no leaks de address space anterior
- [x] `SYS_DUP2(0, 1)` redirige stdout a stdin (prueba con `cat < file`)
- [x] FD table por proceso: máx 64 FDs; abrir más retorna `-EMFILE`
- [x] Serial log muestra cada syscall con PID, nombre y resultado

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| `sys_execve` falla a mitad → proceso en estado inconsistente | Media | Crítico | Cargar nuevo ELF completamente antes de descartar address space viejo |
| FD leak si proceso muere sin cerrar FDs | Alta | Medio | `process_exit` itera y cierra todos los FDs activos |
| `sys_mmap` con addr=0 y len enorme → agota memoria kernel | Media | Alto | Limitar mmap anónimo a 256 MB por proceso |

---

## Fase 7 — libportix (C + Rust Runtime Ring-3)
> **Badge**: ✅ `COMPLETADO` · 100% · Actualizado 2026-06-11

**Objetivo**: proveer a los programas ring-3 un runtime completo para escribir
en C **y en Rust** sin dependencias del sistema host. Dos superficies de API
paralelas — `libportix.a` (C) y `libportix.rlib` (Rust `no_std`) — que se
sintetizan sobre los mismos syscalls.

---

### 7A — Runtime C

#### Componentes C

| Componente | Descripcion |
|------------|-------------|
| `crt0.s` | `_start` en asm: llama `_init`, `main(argc, argv, envp)`, `exit()` |
| `stdio.c` | `printf`, `puts`, `fgets`, `fputs`, `sprintf`, `vprintf` |
| `stdlib.c` | `malloc`, `free`, `calloc`, `realloc` (via SYS_BRK + free-list coalescing) |
| `file.c` | `fopen`, `fread`, `fwrite`, `fclose`, `fseek`, `ftell`, `rewind` (via syscalls) |
| `string.c` | `memcpy`, `memset`, `memmove`, `strlen`, `strcmp`, `strcpy`, `strncpy`, `strcat`, `strstr` |
| `math.c` | `abs`, `atoi`, `atol`, `strtol`, `strtoul` |
| `portix.h` | Header principal con todas las declaraciones |
| `libportix.a` | Libreria estatica para linkear con `-lportix` |

#### Toolchain C

Script `scripts/ring3-toolchain.sh`:

```bash
# Compila libportix.a con cross-gcc x86_64-elf
x86_64-elf-gcc -ffreestanding -nostdlib -c src/*.c
x86_64-elf-ar rcs libportix.a *.o

# Compilar programa de usuario
x86_64-elf-gcc -ffreestanding -nostdlib -static \
    -L. -lportix -o hello.elf hello.c
```

#### Criterios de aceptación (C)

- [x] `hello.c` compila con `-lportix` y corre en Portix mostrando output
- [x] `malloc` + `free` sin leaks en 1000 iteraciones (verificar break no crece indefinidamente)
- [x] `printf` maneja `%s`, `%d`, `%x`, `%p`, `%u`, `%c`, `%ld`, `%lu` correctamente
- [x] `fopen` / `fread` / `fclose` leen archivo FAT32 completo correctamente
- [x] `strcmp` / `memcpy` / `strlen` / `memmove` pasan test suite estándar
- [x] `free-list` con coalescing: heap no fragmenta con alloc/free alternados de tamaños variados
- [x] `libportix.a` compilable con script `ring3-toolchain.sh` sin intervención manual

---

### 7B — Runtime Rust (`no_std`)

#### Componentes Rust

| Componente | Descripción |
|------------|-------------|
| `libportix/rust/src/lib.rs` | Crate raíz `no_std`; re-exporta módulos |
| `portix_rt/src/entry.rs` | Entry point en Rust: `#[no_mangle] extern "C" fn _start()` llama `main()` |
| `portix_rt/src/panic.rs` | `#[panic_handler]` → `SYS_EXIT(1)` + serial log |
| `portix_rt/src/alloc.rs` | `GlobalAllocator` custom via `SYS_BRK`; implementa `alloc::GlobalAlloc` |
| `portix_rt/src/io.rs` | `write!` / `writeln!` macro support via `SYS_WRITE` |
| `portix_rt/src/fs.rs` | `File::open()`, `File::read()`, `File::write()`, `File::close()` via syscalls |
| `portix_rt/src/process.rs` | `getpid()`, `exit()`, `yield_cpu()`, `sleep()` wrappers |
| `portix_rt/src/syscall.rs` | `syscall!(nr, a0, a1, ...)` macro de bajo nivel con `asm!` |
| `portix.rlib` | Artefacto compilado para `x86_64-unknown-none` |

#### Target custom Rust

```json
// x86_64-portix-none.json
{
  "llvm-target": "x86_64-unknown-none",
  "data-layout": "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128",
  "arch": "x86_64",
  "os": "none",
  "linker-flavor": "ld.lld",
  "linker": "rust-lld",
  "panic-strategy": "abort",
  "disable-redzone": true,
  "features": "-mmx,-sse,+soft-float"
}
```

#### Toolchain Rust

```bash
# Compilar runtime
cargo build --target x86_64-portix-none --release \
    -p portix_rt

# Compilar programa de usuario Rust
cargo build --target x86_64-portix-none --release \
    -p my_app
# Requiere en Cargo.toml:
#   [dependencies]
#   portix_rt = { path = "../../libportix/rust" }
```

#### Programa de ejemplo `hello.rs`

```rust
#![no_std]
#![no_main]

use portix_rt::{println, process};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    println!("Hello from Rust ring-3 on PORTIX!");
    let pid = process::getpid();
    println!("PID = {}", pid);
    0
}
```

#### `alloc` crate (heap Rust)

```rust
// portix_rt/src/alloc.rs
use core::alloc::{GlobalAlloc, Layout};
use crate::syscall::sys_brk;

pub struct PortixAllocator;

unsafe impl GlobalAlloc for PortixAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Expande program break, retorna puntero alineado
        sys_brk_alloc(layout.size(), layout.align())
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Free-list interno; no contrae el break en MVP
    }
}

#[global_allocator]
static A: PortixAllocator = PortixAllocator;
```

Esto permite usar `alloc::vec::Vec`, `alloc::string::String`, `alloc::boxed::Box`
en cualquier programa ring-3 Rust sin tocar el kernel.

#### Criterios de aceptación (Rust)

- [x] Programa `hello.rs` compila para `x86_64-portix-none` y corre en Portix sin error
- [x] `#[panic_handler]` dispara `SYS_EXIT(1)` y log serial antes de terminar; no triple fault
- [x] `GlobalAllocator` permite `Vec::new()` y `push()` en loop de 1000 elementos sin corrupción
- [x] `println!` macro produce output idéntico al `printf` del runtime C para los mismos valores
- [x] `File::open()` + `File::read()` + `File::close()` en Rust leen archivo FAT32 correcto
- [x] `process::sleep(10)` bloquea exactamente 10 ticks (verificado contra PIT)
- [x] `cargo build --target x86_64-portix-none --release` compila sin warnings con `deny(warnings)`
- [x] Binario Rust sin `alloc` (solo `core`) tiene tamaño < 4 KB stripped
- [x] Binario Rust con `alloc` (Vec + String) tiene tamaño < 16 KB stripped

#### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| `GlobalAllocator` no es thread-safe → UB en SMP | Alta | Crítico | Añadir spinlock en alloc/dealloc; documentar como "single-thread" hasta Fase 15 |
| ABI mismatch entre crt0 de C y entry Rust en mismo binario | Media | Alto | Usar solo uno por binario; no mezclar runtimes en mismo ejecutable |
| `alloc` crate requiere `__rust_alloc_error_handler` | Media | Medio | Proveer `#[alloc_error_handler]` que llama `SYS_EXIT(ENOMEM)` |
| `no_std` + target custom → errores crípticos de LLVM | Alta | Bajo | Documentar flags obligatorias; script de setup en `scripts/rust-setup.sh` |
| Heap fragmentación en `Vec` con grow + shrink | Media | Medio | Implementar coalescing completo antes de marcar criterio como completado |

---

## Fase 8 — Init + Shell + User Programs
> **Badge**: ✅ `COMPLETADO` · 100% · Actualizado 2026-06-11

**Objetivo**: el sistema arranca con un init ring-3 que lanza un shell.
El usuario puede ejecutar comandos, navegar el FS, editar archivos.

### Componentes

| Programa | Descripcion |
|----------|-------------|
| `/bin/init` | Primer proceso al boot. Lanza `/bin/sh` via execve |
| `/bin/sh` | Shell ring-3 minimal: prompt `portix$ `, ejecuta programas con PATH |
| `/bin/ls` | Lista directorio con `SYS_GETDIRENTS` |
| `/bin/cat` | Concatena archivos con `SYS_READ` |
| `/bin/echo` | Imprime argumentos |
| `/bin/clear` | Limpia terminal (escape codes ANSI) |
| `/bin/help` | Lista comandos disponibles |
| `/bin/hello` | Demo "Hello from Ring 3!" |
| `/bin/uptime` | Muestra tiempo desde boot via `SYS_UPTIME` + PIT ticks |

### Boot sequence

```
kernel init → FAT32 mount → find /bin/init → process_create(init) → shell prompt
```

### Logging

`[INIT] starting /bin/sh on terminal`

### Criterios de aceptación

- [x] Sistema arranca hasta prompt `portix$ ` sin intervención manual
- [x] `ls /` muestra contenido del directorio raíz correctamente
- [x] `cat /etc/inittab` imprime contenido del archivo
- [x] `echo hello world` imprime `hello world` con newline
- [x] Shell ejecuta binario en PATH correctamente con `SYS_EXECVE`
- [x] `clear` limpia la pantalla con secuencias ANSI correctas
- [x] `uptime` muestra tiempo desde boot en formato legible
- [x] Ctrl+C en shell mata proceso hijo sin matar el shell

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Shell no recibe input si teclado IRQ no está ruteado | Alta | Crítico | Verificar IRQ1 activo antes de lanzar shell; fallback a polling |
| `/bin/init` no existe en FAT32 → boot falla sin mensaje claro | Media | Alto | Kernel verifica existencia de `/bin/init` y panic con mensaje descriptivo |
| `SYS_EXECVE` en shell deja zombie si hijo no hace `SYS_EXIT` | Media | Medio | Shell hace wait implícito (busy-poll en process table) hasta hijo Zombie |

---

## Fase 9 — FAT32 Userspace Driver
> **Badge**: ⏳ `PENDIENTE` · 0%

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

### Criterios de aceptación

- [ ] `ata_driver` lee sector 0 (MBR) correctamente desde ring-3
- [ ] `fat32_driver` monta partición y lista raíz vía IPC al kernel
- [ ] `SYS_OPEN` en kernel delega a `fat32_driver` sin acceder disco directamente
- [ ] IRQ14 llega al `ata_driver` como mensaje IPC; driver responde en < 1ms
- [ ] `cat /bin/hello` desde shell funciona a través del nuevo stack de drivers
- [ ] ATA driver tolera disco lento (500ms timeout antes de error)
- [ ] Serial log muestra LBA, count y resultado de cada operación de bloque

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Latencia IPC introduce regresión vs driver en ring-0 | Alta | Medio | Cache de bloques en `fat32_driver`; medir con benchmark antes/después |
| `ata_driver` crash → sistema de archivos inaccesible | Alta | Crítico | Kernel mantiene driver ATA mínimo de emergencia para recovery |
| IRQ14 se pierde si `ata_driver` no está en `RECV` | Media | Alto | Cola IPC con buffer; driver siempre en RECV o procesando |

### Alternativas

- **Opción A**: Driver ATA completo en ring-3 desde el inicio (3 semanas)
- **Opción B**: Wrapper thin en ring-0 con cache, driver real después ← Recomendado para MVP
- **Opción C**: Usar virtio-blk en QEMU para simplificar (1 semana)

---

## Fase 10 — VFS + Mount + Multiple FS
> **Badge**: ⏳ `PENDIENTE` · 0%

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

### Criterios de aceptación

- [ ] `mount ramfs /tmp` funciona; archivos en `/tmp` sobreviven hasta reboot
- [ ] `mount fat32 /dev/sda0 /home` redirige correctamente operaciones de archivo
- [ ] Path resolution de `/home/user/file.txt` atraviesa mount point correctamente
- [ ] `VnodeOps` implementado para FAT32 y ramfs con misma interfaz
- [ ] `open /proc/1/status` retorna info del proceso PID 1 via ramfs virtual
- [ ] Unmount falla si hay FDs abiertos en ese mount point
- [ ] Serial log muestra cada mount/unmount con path y tipo de FS

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Path traversal en `..` cruza límites de mount point | Alta | Crítico | Verificar que `..` en root de mount apunta al directorio del mount en parent FS |
| Vnode leak si `open` no tiene `close` correspondiente | Alta | Medio | Reference counting en vnodes; assert en unmount que refcount = 0 |
| Deadlock en VFS lock si driver IPC bloquea | Media | Alto | Timeout en operaciones VFS; retornar `-ETIME` si driver no responde |

---

## Fase 11 — IPC System
> **Badge**: ✅ `COMPLETADO` · 100%

**Objetivo**: sistema de mensajes entre procesos para permitir arquitectura
microkernel (drivers en ring-3, servicios en ring-3).

### API

| Syscall | Args | Descripcion |
|---------|------|-------------|
| `SYS_SEND` (14) | `(pid_dest, type, data_ptr, data_len)` | Enviar mensaje a proceso destino |
| `SYS_RECV` (15) | `(buf, len)` | Recibir mensaje (bloqueante si no hay) |
| `SYS_REG_IRQ` (16) | `(irq, pid)` | Registrar un proceso como handler de una IRQ |

### Diseño

- Mensajes de tamaño fijo (64 bytes: 24 header + 40 data) para simplicidad
- Per-process mailbox circular: 16 mensajes por proceso, 64KB total en BSS
- SYS_SEND despierta proceso destino si está Blocked
- IRQ routing table `[Option<u64>; 16]` para IRQ0-IRQ15
- Timeout: ~10s via scheduler wake_blocked (sleep_until = 1)

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 11.1 | `IpcMessage` struct | `ipc::IpcMessage` en `kernel/src/ipc.rs` — 64 bytes total |
| 11.2 | Kernel queues | `PerProcQueue` con array fijo `[IpcMessage; 16]` por slot de proceso |
| 11.3 | `SYS_SEND` | `fn sys_send(dst_pid, msg_type, data_ptr, data_len)` — copy_from_user + enqueue |
| 11.4 | `SYS_RECV` | `fn sys_recv(buf, len)` — dequeue + copy_to_user. Blockea si empty |
| 11.5 | IRQ registration | `sys_reg_irq(irq, pid)` + `ipc::notify_irq(irq)` para enviar desde handler |

### Logging

`[IPC] PID 1 → PID 2: msg type=1 size=24`, `[IPC] IRQ14 → ata_driver (PID 3)`

### Criterios de aceptación

- [x] `SYS_SEND` → `SYS_RECV` en dos procesos: mensaje llega íntegro
- [x] `SYS_RECV` bloquea proceso hasta que llegue mensaje (no busy-wait)
- [ ] IRQ14 dispara mensaje IPC al proceso registrado con `SYS_REG_IRQ`
- [ ] Cola llena → `SYS_SEND` retorna -1 (sin bloqueo por ahora)
- [x] Proceso destino muerto → `SYS_SEND` retorna -1
- [ ] 1000 mensajes/segundo entre 2 procesos sin pérdida (benchmark)
- [x] Serial log muestra src, dst, tipo y tamaño para cada mensaje

### Riesgos (mitigados)

- Deadlock: timeout de ~10s en SYS_RECV vía scheduler wake_blocked
- IRQ handler: mailbox lleno → mensaje se descarta (se mejora con cola separada en Fase 12)
- Payload 40 bytes: suficiente para mensajes de control (IRQ notifications, block requests)

---

## Fase 12 — Userspace Drivers
> **Badge**: ⏳ `PENDIENTE` · 0%

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

### Criterios de aceptación

- [ ] Tecla presionada llega como IPC `KBD_EVENT` al `kbd_drv` y luego al shell
- [ ] `fb_drv` puede escribir píxeles en framebuffer desde ring-3
- [ ] `pci_drv` lista dispositivos PCI correctamente desde ring-3
- [ ] `SYS_IOPORT` deniega acceso a puertos no permitidos (retorna `-EPERM`)
- [ ] Driver que crashea no mata el kernel; puede ser reiniciado por init
- [ ] `/dev/kbd`, `/dev/fb0`, `/dev/sda0` accesibles como FDs normales
- [ ] Serial log muestra IRQ registrado y PID del driver para cada dispositivo

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Driver malicioso usa `SYS_IOPORT` para acceder puertos kernel (ej. 0x60 PIC) | Media | Crítico | Whitelist de puertos permitidos por driver; kernel valida en `SYS_IOPORT` |
| Latencia de teclado aumenta con IPC indirecto | Media | Medio | `kbd_drv` con prioridad alta en scheduler; medir latencia < 5ms |
| MMIO mapping incorrecto → driver escribe en RAM del kernel | Baja | Crítico | Verificar que rango físico es BAR PCI válido; no permitir < 1MB físico |

---

## Fase 13 — Networking Stack
> **Badge**: ⏳ `PENDIENTE` · 0%

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
| 13.8 | WiFi driver | Intel PRO/Wireless o Atheros |
| 13.9 | WPA2 supplicant | Autenticación WiFi |
| 13.10 | NetworkManager | CLI para gestionar conexiones |

### Logging

`[NET] eth0: MAC 52:54:00:12:34:56 UP`, `[NET] DHCP: lease 10.0.2.15`

### Bonus: HTTP server

Programa ring-3 `httpd` que sirve archivos via HTTP.

### Criterios de aceptación

- [ ] `ping 8.8.8.8` desde Portix recibe ICMP reply (en QEMU con SLIRP o bridge)
- [ ] DHCP asigna IP correctamente al boot; IP visible con `ifconfig`
- [ ] `nc -u` envía y recibe datos UDP
- [ ] TCP 3-way handshake completo verificado con Wireshark/tcpdump externo
- [ ] `httpd` sirve archivo estático y cliente externo recibe response 200 OK
- [ ] ARP cache se puebla con primer packet; segundo packet no genera ARP request
- [ ] Stack tolera 100 conexiones simultáneas TCP sin pérdida de datos

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| TCP state machine bugs (TIME_WAIT, FIN_WAIT2) | Alta | Crítico | Implementar con state machine formal; fuzz testing con packetdrill |
| NIC driver incompatibilidad con hardware real | Media | Alto | Soportar solo RTL8139 inicialmente; e1000 después |
| Checksum incorrecto → paquetes descartados silenciosamente | Alta | Alto | Verificar con Wireshark; implementar offload solo después de validar SW |
| Security: stack smashing via paquetes malformados | Alta | Crítico | Sanitizar todos los campos de longitud antes de parsear |

### Alternativas

- **Opción A**: Stack TCP/IP completo propio (6 semanas)
- **Opción B**: Port lwIP a ring-3 (2 semanas) ← Recomendado para MVP
- **Opción C**: Solo UDP primero, TCP después (4 semanas)

---

## Fase 14 — Multi-User + Security
> **Badge**: ⏳ `PENDIENTE` · 0%

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

### Criterios de aceptación

- [ ] Usuario sin privilegios no puede leer `/etc/shadow` (retorna `-EACCES`)
- [ ] `login` con password incorrecto rechaza acceso; 3 intentos → delay de 5s
- [ ] `chmod 600 file` impide lectura por otro usuario (verificar con `SYS_OPEN`)
- [ ] SUID binary corre con euid del owner, no del caller
- [ ] Proceso sin `CAP_NET_RAW` no puede usar `socket(AF_PACKET)` (retorna `-EPERM`)
- [ ] `root` (UID=0) pasa todos los checks de permiso
- [ ] Serial log muestra UID, GID y capabilities en cada `process_create`

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| SUID race condition (TOCTOU en execve) | Media | Crítico | Fijar euid antes de cargar ELF; no re-stat el archivo |
| Hash de password débil en `/etc/shadow` | Alta | Alto | Usar SHA-512 con salt aleatorio de 16 bytes mínimo |
| Capability leak tras fork/exec | Media | Alto | Definir política clara: exec dropa capabilities no heredables |

---

## Fase 15 — SMP + Multi-Core
> **Badge**: ⏳ `PENDIENTE` · 0%

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
| 15.7 | NUMA detection | Parse SRAT table |
| 15.8 | NUMA-aware allocator | Allocar memoria cerca del CPU |
| 15.9 | Memory migration | Mover páginas entre nodos |

### Logging

`[SMP] APIC ID 0 (BSP)`, `[SMP] APIC ID 1 (AP) started`, `[SMP] 2 CPUs online`

### Criterios de aceptación

- [ ] 2 CPUs detectados y activos en QEMU `-smp 2`
- [ ] Proceso puede migrar entre CPUs sin corrupción de estado
- [ ] Spinlock previene race condition en process table (verificar con `lockdep`)
- [ ] IRQ0 (PIT) llega solo a BSP; IRQ balancing distribuye el resto
- [ ] Throughput con 2 CPUs > 1.8x throughput con 1 CPU (benchmark de procesos CPU-bound)
- [ ] No deadlocks en stress test con 32 procesos en 2 CPUs durante 5 minutos
- [ ] Serial log confirma ID de APIC y estado de cada CPU al boot

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| AP no inicia (SIPI mal dirigido) | Media | Alto | Verificar dirección de trampoline code < 1MB; usar QEMU para debugging |
| Spinlock con `sti` → deadlock si IRQ handler intenta tomar mismo lock | Alta | Crítico | `spin_lock_irqsave` + `spin_unlock_irqrestore` obligatorio en IRQ paths |
| TLB shootdown no implementado → CPU2 usa mapping obsoleto | Alta | Crítico | IPI TLB shootdown antes de cualquier `unmap_page` en sistema multicore |

---

## Fase 16 — Dynamic Linker
> **Badge**: ⏳ `PENDIENTE` · 0%

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

### Criterios de aceptación

- [ ] ELF dinámico carga y ejecuta `printf` desde `libc.so` compartida
- [ ] Dos procesos que usan `libc.so` comparten las mismas páginas físicas (verificar con page frame counter)
- [ ] `dlopen("libm.so")` + `dlsym("sin")` retorna puntero funcional
- [ ] Lazy binding: GOT entry se resuelve solo en primera llamada
- [ ] `ld-portix.so` no está disponible → error claro, no crash silencioso
- [ ] Actualizar `libc.so` en disco + reiniciar proceso → usa nueva versión
- [ ] Serial log muestra cada librería cargada y símbolos resueltos

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| ASLR + PIE complica resolución de GOT | Alta | Alto | Implementar ASLR después del dynamic linker; primero sin ASLR |
| Symbol collision entre librerías (mismo nombre, diferente implementación) | Media | Medio | Namespacing de librerías; orden de búsqueda explícito |
| Librería desactualizada en cache → proceso usa versión vieja | Media | Medio | Versioning en nombre de librería (`libc.so.1`); no cache agresivo |

---

## Fase 17 — Power Management + ACPI
> **Badge**: ⏳ `PENDIENTE` · 0%

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

### Criterios de aceptación

- [ ] RSDP encontrado y validado con checksum correcto
- [ ] S3 suspend + resume restaura estado de pantalla y procesos activos
- [ ] P-state más bajo reduce consumo medible (verificar con `powertop` externo en hardware real)
- [ ] Battery percentage actualiza cada 10 segundos vía ACPI `_BST`
- [ ] ACPI poweroff (`shutdown -h`) apaga la máquina limpiamente
- [ ] Tables corruptas → warning en serial, no panic
- [ ] Serial log muestra cada tabla ACPI encontrada con dirección y longitud

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| ACPI AML interpreter complejo de implementar | Alta | Alto | Usar subset mínimo de AML; no implementar intérprete completo en MVP |
| S3 resume rompe estado de drivers ring-3 | Alta | Crítico | Notificar a todos los drivers via IPC antes de S3; reinicializar en resume |
| Frecuencia incorrecta deja CPU en estado inestable | Baja | Crítico | Solo cambiar P-states dentro del rango reportado por `_PSS` |

---

## Fase 18 — Init System + Service Manager
> **Badge**: ⏳ `PENDIENTE` · 0%

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

### Criterios de aceptación

- [ ] Todos los servicios sin dependencias mutuas arrancan en paralelo
- [ ] Servicio que falla 3 veces seguidas queda en estado `failed` sin restart loop
- [ ] Watchdog reinicia servicio que no envía heartbeat en 30 segundos
- [ ] Socket activation: conexión a puerto llega antes que `httpd` → se guarda, `httpd` la recibe al arrancar
- [ ] Tiempo de boot total medido < 1 segundo en QEMU (sin disco lento)
- [ ] `journal` guarda logs de todos los servicios con timestamp
- [ ] Dependencia circular detectada al parsear units → error claro, no deadlock en boot

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Dependencia circular entre servicios → boot cuelga | Media | Crítico | Detección de ciclos con DFS antes de iniciar ningún servicio |
| PID 1 que crashea → kernel panic inevitable | Baja | Crítico | Init tiene supervisor separado en ring-0 de último recurso (emergency shell) |
| Socket activation con backlog grande consume toda la memoria | Baja | Medio | Límite de 128 conexiones en espera por socket activado |

---

## Fase 19 — Self-Hosting
> **Badge**: ⏳ `PENDIENTE` · 0%

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

### Criterios de aceptación

- [ ] `gcc --version` corre nativamente en Portix sin errores
- [ ] `hello.c` compila nativamente con GCC portado; binario corre correctamente
- [ ] `make` ejecuta todas las reglas del Makefile del kernel sin errores
- [ ] Kernel compilado nativamente arranca y pasa boot hasta prompt del shell
- [ ] `git clone` trae repositorio completo (requiere Fase 13 completa)
- [ ] Kernel auto-compilado tiene tamaño similar al cross-compilado (±10%)
- [ ] Serial log del proceso de compilación muestra cada archivo compilado

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| GCC requiere más RAM de la disponible | Alta | Alto | Implementar swap o aumentar RAM de QEMU a 512MB+ para compilación |
| Bootstrapping circular (GCC necesita libportix, libportix necesita GCC) | Alta | Crítico | Usar binario GCC pre-compilado para construir el GCC nativo (stage 1) |
| Kernel auto-compilado tiene bug regresivo no detectado | Media | Alto | Suite de smoke tests que corren automáticamente después de cada build |

### Alternativas

- **Opción A**: GCC completo (8 semanas de porting)
- **Opción B**: TinyCC (tcc) como compilador C mínimo (2 semanas) ← Recomendado para MVP
- **Opción C**: Rustc cross-compilado para Portix target (4 semanas)

---

## Fase 20 — POSIX Compatibility
> **Badge**: ⏳ `PENDIENTE` · 0%

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

### Criterios de aceptación

- [ ] `fork()` + `exec()` correcto: hijo corre nuevo binario, padre obtiene PID hijo
- [ ] COW: escribir en página compartida post-fork genera nueva copia sin afectar padre
- [ ] `SIGINT` (Ctrl+C) mata proceso foreground del shell; shell sobrevive
- [ ] `pipe()`: datos escritos en un extremo leídos en otro sin pérdida
- [ ] `select()` con timeout retorna correctamente cuando FD tiene datos
- [ ] DOOM arranca y muestra pantalla de título en framebuffer (30+ FPS en QEMU)
- [ ] `gdb` remoto puede poner breakpoint y step en proceso ring-3

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| COW incorrecto → padre e hijo comparten estado mutable | Alta | Crítico | Test: escribir en array post-fork y verificar que valores divergen |
| DOOM requiere `SDL2` o framebuffer específico | Alta | Alto | Usar port DOOM que soporte framebuffer raw (chocolate-doom o similar) |
| Signal delivery en momento incorrecto corrompe stack ring-3 | Alta | Crítico | Señales solo entregadas en transición ring-0 → ring-3; never mid-instruction |
| `select()` con muchos FDs tiene complejidad O(n) | Media | Medio | Limitar a 256 FDs en `select`; implementar `epoll` después |

---

## Fase 21 — Graphics Acceleration (DRM/KMS + Mesa)
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: stack de gráficos moderno con aceleración por GPU, compositor
Wayland-style, y font rendering con FreeType. Sustituye el framebuffer lineal.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 21.1 | DRM core ring-3 | API `/dev/dri/card0`: `drmModeGetResources`, `drmModeSetCrtc`, `drmModePageFlip` |
| 21.2 | GEM buffer manager | `GEM_CREATE`, `GEM_MMAP`, `GEM_CLOSE` — gestión de buffers de GPU |
| 21.3 | KMS atomic commit | Planos, CRTCs, encoders, connectors — evitar tearing via page flip sync |
| 21.4 | Mesa Gallium port | Adaptar `softpipe` (software rasterizer) como ICD Vulkan/OpenGL stub |
| 21.5 | Wayland-style compositor | Servidor de composición IPC-based; ventanas como superficies con doble buffer |
| 21.6 | FreeType port | Rasterización de fuentes TTF/OTF en ring-3; atlas de glifos en GEM buffer |
| 21.7 | 2D accel via Vulkan | Primitivas de rect/blit via descriptor sets, shaders SPIR-V mínimos |
| 21.8 | Window manager | Ventanas decoradas, foco, `Alt+Tab`, resize drag |

### Logging

`[DRM] card0: 1920x1080@60Hz connector HDMI-A-1 enabled`
`[WM] surface PID 5 mapped at (100,100) 800x600`

### Criterios de aceptación

- [ ] `drmModeSetCrtc` cambia resolución a 1920x1080 sin artefactos
- [ ] Page flip elimina tearing visible (verificar con patrón de franjas en movimiento)
- [ ] FreeType renderiza texto Unicode con anti-aliasing en pantalla
- [ ] Compositor maneja 3+ ventanas simultáneas a 60 FPS
- [ ] Mesa softpipe pasa `glxinfo` con `GL_VERSION >= 3.3`
- [ ] `Alt+Tab` alterna foco entre ventanas correctamente
- [ ] Aplicación que hace `mmap` de GEM buffer puede escribir píxeles y verlos en pantalla

### Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Mesa requiere libc completa y `pthread` | Alta | Alto | Stub mínimo de `pthread` para inicialización; Gallium softpipe primero |
| Tearing en modo VESA sin KMS atomic | Alta | Medio | Implementar doble buffer en FB driver antes de DRM real |
| FreeType usa `malloc` masivo al cargar fuente grande | Media | Medio | Limitar atlas a fuentes monocromáticas 8px–24px; bitmap fonts como fallback |

---

## Fase 22 — Audio Stack
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: HDA driver, PCM interface, servidor de audio con mixing,
soporte de codecs MP3/WAV/OGG.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 22.1 | Intel HDA driver ring-3 | Enumerar widgets, configurar verbos CORB/RIRB, DMA ring buffers |
| 22.2 | PCM interface `/dev/dsp` | `open`, `write` PCM s16le, `ioctl` para sample rate / channels |
| 22.3 | Audio server `portix-audio` | Mezcla streams de múltiples clientes; latencia < 20ms |
| 22.4 | WAV decoder | Parsear RIFF header, decodificar PCM sin compresión |
| 22.5 | MP3 decoder | Port de `minimp3` (single-header) a ring-3 |
| 22.6 | OGG/Vorbis decoder | Port de `stb_vorbis` a ring-3 |
| 22.7 | ALSA-compat API | Subset de ALSA `snd_pcm_open`, `snd_pcm_writei` para portar software |
| 22.8 | MIDI sequencer | Parser de archivos MIDI; síntesis de onda simple |

### Criterios de aceptación

- [ ] `aplay sample.wav` reproduce audio correcto sin clicks ni dropout
- [ ] Dos procesos reproduciendo audio simultáneamente → mezcla correcta sin artefactos
- [ ] `mp3play song.mp3` decodifica y reproduce a 44100Hz stereo
- [ ] Latencia de playback < 20ms medida con loopback
- [ ] Cambio de volumen en runtime vía `ioctl` sin reiniciar stream

---

## Fase 23 — USB Stack
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: xHCI, enumeración USB, HID class driver, Mass Storage, hotplug.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 23.1 | xHCI controller driver | MMIO init, command/event rings, port reset, slot enable |
| 23.2 | USB enumeration | `GET_DESCRIPTOR`, `SET_ADDRESS`, `SET_CONFIGURATION` |
| 23.3 | HID class driver | Teclado + mouse USB; mapeo de report descriptor a eventos |
| 23.4 | Mass Storage (BOT) | Bulk-Only Transport; exponer como `/dev/usb0` bloque device |
| 23.5 | Hotplug daemon `udev-portix` | Detectar connect/disconnect via xHCI events; crear/borrar `/dev/` nodes |
| 23.6 | USB Audio class | Altavoz USB via UAC1 protocol |
| 23.7 | CDC-ECM (USB Ethernet) | NIC via USB para hardware sin NIC PCI |

### Criterios de aceptación

- [ ] Teclado USB es funcional tras boot sin PS/2 disponible
- [ ] USB flash drive monta en `/mnt/usb0` automáticamente al conectar
- [ ] Desconectar USB flash → `/mnt/usb0` desmonta limpiamente; FDs abiertos retornan error
- [ ] `hotplug` notifica a `init` de cada evento connect/disconnect en < 100ms

---

## Fase 24 — Containers + Namespaces
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: PID/network/mount namespaces, cgroups básicos, overlay FS,
runtime CLI tipo `docker run`.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 24.1 | PID namespace | `clone(CLONE_NEWPID)` — proceso ve su propio PID 1 |
| 24.2 | Mount namespace | `clone(CLONE_NEWNS)` — vistas de FS aisladas |
| 24.3 | Network namespace | Stack TCP/IP separado por namespace; veth pairs |
| 24.4 | cgroups v1 | Limitar CPU time y memoria por grupo de procesos; `/sys/fs/cgroup/` |
| 24.5 | Overlay FS | Capas read-only + capa write-on-top para container images |
| 24.6 | Container runtime CLI | `portix-run image cmd` — crea namespaces, monta overlay, lanza proceso |
| 24.7 | OCI image format | Parsear `config.json` + `layers.tar` de imagen OCI |
| 24.8 | `portix-build` | Dockerfile-like build de imágenes nativas |

### Criterios de aceptación

- [ ] `portix-run alpine sh` lanza shell aislado con PID=1 y FS limpio
- [ ] Proceso en container no puede ver procesos del host (PID namespace)
- [ ] Límite de 64 MB de RAM en cgroup se enforce sin OOM del host
- [ ] Overlay FS: modificaciones en container no afectan capa base
- [ ] `portix-build` produce imagen OCI válida desde Portixfile

---

## Fase 25 — Package Management
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: Formato de paquete `.pxp`, repositorio HTTP, resolver de
dependencias SAT, firmas GPG.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 25.1 | Formato `.pxp` | TAR+Zstd con `PKGINFO` (name, version, deps, conflicts), pre/post scripts |
| 25.2 | `pxp install pkg` | Descargar, verificar firma, extraer a `/`, correr post-install |
| 25.3 | `pxp remove pkg` | Revertir archivos instalados, correr pre-remove |
| 25.4 | Dependency resolver | SAT solver mínimo (PubGrub algorithm) para deps con versiones |
| 25.5 | Repositorio HTTP | `repoindex.json` con lista de paquetes + checksums; CDN-ready |
| 25.6 | Firmas GPG | Cada paquete firmado con clave del mantenedor; `pxp verify` |
| 25.7 | `pxp build` | Compilar paquete desde `PKGBUILD` (similar a Arch Linux) |
| 25.8 | `pxp search` | Buscar en índice de repositorio por nombre o descripción |

### Criterios de aceptación

- [ ] `pxp install gcc` descarga, verifica firma, instala sin conflictos
- [ ] Dependencia circular detectada antes de instalar (SAT solver)
- [ ] `pxp remove gcc` desinstala sin borrar archivos compartidos con otro paquete
- [ ] Repositorio HTTP sirve índice y paquetes; `pxp update` refresca índice

---

## Fase 26 — Crypto Stack
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: CSPRNG, AES-NI, SHA-256/512, ChaCha20-Poly1305, disk encryption, TLS 1.3.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 26.1 | CSPRNG | `/dev/urandom` via RDRAND + Fortuna PRNG con entropy pool del kernel |
| 26.2 | AES-NI | Usar instrucciones `AESENC`/`AESDEC` para AES-128/256-GCM |
| 26.3 | SHA-256 / SHA-512 | Implementación con SHA-NI (`SHA256RNDS2`) |
| 26.4 | ChaCha20-Poly1305 | Stream cipher + AEAD para TLS 1.3 |
| 26.5 | X25519 + Ed25519 | ECDH y firma digital para key exchange |
| 26.6 | Disk encryption `portix-crypt` | LUKS2-compatible: header + master key + sector cipher AES-XTS |
| 26.7 | TLS 1.3 | Librería `libtls-portix`: handshake, record layer, cert validation |
| 26.8 | `ssh` client/server | Basado en `libtls-portix`; acceso remoto a Portix |

### Criterios de aceptación

- [ ] `dd if=/dev/urandom count=1` produce output estadísticamente uniforme (NIST SP 800-22)
- [ ] AES-256-GCM encrypt/decrypt round-trip de 1MB < 5ms (con AES-NI)
- [ ] Disco cifrado con `portix-crypt` monta correctamente con passphrase correcta; datos ilegibles sin clave
- [ ] TLS 1.3 handshake completo con `curl` externo (verificar con Wireshark)
- [ ] `ssh portix@localhost` autentica con key pair Ed25519

---

## Fase 27 — Virtualization (VMX / KVM-style)
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: Portix como hipervisor tipo-2; correr VMs con VMX (Intel VT-x)
y EPT (Extended Page Tables).

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 27.1 | CPUID check | Verificar `VMX` bit en CPUID ECX; abortar si ausente |
| 27.2 | VMXON | Habilitar VMX per-CPU; allocar VMXON region alineada a 4KB |
| 27.3 | VMCS setup | Configurar VMCS: guest CS/SS/DS/ES, RFLAGS, CR0/CR3/CR4, RIP, RSP |
| 27.4 | EPT (Extended Page Tables) | Mapeo de guest-physical → host-physical; shadow page tables como fallback |
| 27.5 | VM entry/exit handlers | Manejar VM exits: CPUID, I/O port, MMIO, IRQ injection, HLT |
| 27.6 | Device emulation | VirtIO blk + net emulados en ring-3; e8259 PIC, HPET stub |
| 27.7 | `portix-vm` CLI | `portix-vm create --disk img.qcow2 --mem 512M` |
| 27.8 | VMCS save/restore | Para migración de VM y suspend/resume |

### Criterios de aceptación

- [ ] Boot de Linux kernel minimal en VM sobre Portix hasta prompt
- [ ] Disco VirtIO-blk funcional: guest puede montar y escribir FS
- [ ] VM exit por HLT devuelve control al scheduler del host correctamente
- [ ] Nested paging (EPT) operativo: guest no puede acceder a memoria del host
- [ ] `portix-vm list` muestra VMs activas con uso de CPU y memoria

---

## Fase 28 — Mandatory Access Control (MAC)
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: política MAC tipo SELinux/AppArmor: cada proceso tiene un label,
las operaciones son permitidas/denegadas por política, no por UID/GID.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 28.1 | Security labels | Cada proceso, archivo y socket tiene un `security_label: u64` |
| 28.2 | Hook points | LSM-style hooks en `sys_open`, `sys_connect`, `execve`, `mmap` |
| 28.3 | Policy engine | Archivo `/etc/mac_policy` parseado al boot; tabla allow/deny |
| 28.4 | Transition rules | `exec httpd → httpd_t`, `fork init → child_t` |
| 28.5 | `portix-audit` | Log de cada decisión MAC con label src/dst + operación |
| 28.6 | Confined domains | Dominios predefinidos: `web_t`, `ssh_t`, `container_t`, `untrusted_t` |

### Criterios de aceptación

- [ ] `httpd` en dominio `web_t` no puede leer `/etc/shadow` aunque UID=root
- [ ] Violación de política genera entrada en audit log con label y syscall
- [ ] Policy reload sin reboot aplica nuevas reglas en < 100ms
- [ ] Proceso en `untrusted_t` solo puede escribir a `/tmp` y leer `/bin`

---

## Fase 29 — Filesystems Avanzados
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: ext4, Btrfs-lite, journaling, snapshots, compresión inline.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 29.1 | ext4 driver | Extent tree, htree directories, extents, journal (JBD2 subset) |
| 29.2 | Journal (write-ahead log) | Atomic commits; recovery tras crash sin `fsck` |
| 29.3 | Btrfs-lite | Copy-on-Write tree (B-tree), checksums per-block, subvolumes |
| 29.4 | Snapshots | `btrfs subvolume snapshot` — O(1) fork de subvolumen |
| 29.5 | Compresión inline | LZ4 / Zstd transparent por bloque en Btrfs-lite |
| 29.6 | `fsck.portix` | Checker y reparador de ext4 y Btrfs-lite |
| 29.7 | Online resize | Extender partición sin desmontar |

### Criterios de aceptación

- [ ] ext4 sobrevive power-off abrupto sin corrupción (test con QEMU + kill -9)
- [ ] Snapshot de 4GB subvolume toma < 1 segundo
- [ ] Compresión Zstd reduce archivo de texto a < 30% tamaño original inline
- [ ] `fsck` detecta y repara inodo corrupto manualmente inyectado

---

## Fase 30 — RAID + Volume Manager
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: RAID 0/1/5/6 por software, LVM-style volume manager,
thin provisioning.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 30.1 | RAID 0 (stripe) | Distribución de bloques entre N discos; throughput N×disco |
| 30.2 | RAID 1 (mirror) | Escritura doble; lectura balanceada; rebuild automático |
| 30.3 | RAID 5 | Paridad rotante; tolera 1 disco fallido; algoritmo GF(2^8) |
| 30.4 | RAID 6 | Doble paridad; tolera 2 discos fallidos simultáneos |
| 30.5 | Volume manager | Physical volumes → volume groups → logical volumes |
| 30.6 | Thin provisioning | Logical volumes de tamaño virtual > espacio físico; allocate on write |
| 30.7 | `portix-raid` CLI | `portix-raid create --level 5 /dev/sda /dev/sdb /dev/sdc` |
| 30.8 | Rebuild online | Sustituir disco fallido sin parar el sistema |

### Criterios de aceptación

- [ ] RAID 1 con 2 discos: apagar un disco → sistema sigue operativo; rebuild automático al reconectar
- [ ] RAID 5 con 3 discos: throughput lectura > 1.8× disco individual
- [ ] Thin provisioned volume de 100 GB en disco de 20 GB funciona mientras datos reales < 20 GB
- [ ] `portix-raid status` muestra health de cada miembro y progreso de rebuild

---

## Fase 31 — Bluetooth Stack
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: stack Bluetooth 5.x en ring-3: HCI, L2CAP, profiles A2DP/HID/PAN.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 31.1 | HCI transport | USB HCI via URB o UART HCI para módulos serial |
| 31.2 | L2CAP | Logical Link Control and Adaptation Protocol |
| 31.3 | SDP / GATT | Service Discovery Protocol; GATT para BLE |
| 31.4 | HID profile | Teclado y mouse Bluetooth |
| 31.5 | A2DP profile | Audio streaming a auriculares Bluetooth (SBC codec) |
| 31.6 | PAN profile | Red IP sobre Bluetooth (BNEP) |
| 31.7 | BLE scan/connect | Advertising, scanning, pairing con dispositivos BLE |
| 31.8 | `btctl` CLI | `btctl scan`, `btctl pair`, `btctl connect` |

### Criterios de aceptación

- [ ] Teclado Bluetooth parea y envía input al shell correctamente
- [ ] Audio A2DP reproduce WAV a auriculares Bluetooth sin dropout
- [ ] `btctl scan` lista dispositivos cercanos con RSSI y nombre

---

## Fase 32 — PCIe Hotplug + Thunderbolt
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: agregar y retirar dispositivos PCIe en caliente; soporte
Thunderbolt 3/4 con seguridad por autorización.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 32.1 | PCIe hotplug daemon | Monitorear `Hot-Plug Capable` slots vía ACPI eventos; allocar BARs dinámicamente |
| 32.2 | PCIe AER | Advanced Error Reporting; recuperación de errores de bus sin reboot |
| 32.3 | Thunderbolt security | Usuario autoriza nuevos dispositivos TB; deny-by-default |
| 32.4 | TB tunneling | PCIe tunnel + DisplayPort tunnel sobre TB fabric |
| 32.5 | eGPU support | Enumerar GPU externa TB; reinicializar DRM stack con nuevo dispositivo |

### Criterios de aceptación

- [ ] Insertar NIC PCIe en slot hotplug → `/dev/eth1` aparece en < 2 segundos
- [ ] Extraer NIC PCIe en caliente → conexiones activas se cierran limpiamente
- [ ] Dispositivo Thunderbolt desconocido bloqueado hasta autorización del usuario
- [ ] eGPU conectada via TB enumerable por `pci_drv`

---

## Fase 33 — GPU Compute + Vulkan ICD
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: compute shaders via Vulkan, ray tracing stub, soporte de GPU
discretas NVIDIA/AMD via driver open source.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 33.1 | Vulkan ICD loader | `portix-vulkan.so`: dispatch table desde `VkInstance` a driver backend |
| 33.2 | AMDGPU driver | AMDGPU GFX IP init, command buffer submission via ring, fence sync |
| 33.3 | nouveau stub | NV50/Fermi init basic: modo gráfico sin aceleración completa |
| 33.4 | SPIR-V compiler (Mesa NIR) | SPIR-V → NIR → backend ISA para AMDGPU |
| 33.5 | Compute pipelines | `vkCreateComputePipeline`, `vkCmdDispatch`; GPGPU básico |
| 33.6 | VK_KHR_ray_tracing | BVH builder; ray-triangle intersection acelerado por HW (RDNA2+) |
| 33.7 | GPU memory allocator | VMA (Vulkan Memory Allocator) portado a ring-3 |

### Criterios de aceptación

- [ ] `vulkaninfo` muestra dispositivo físico con extensiones básicas
- [ ] Compute shader suma 1M de floats < 1ms en GPU
- [ ] `glxgears` equivalente Vulkan corre a > 200 FPS en resolución 1080p

---

## Fase 34 — NVMe + AHCI
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: reemplazar ATA PIO por NVMe (PCIe) y AHCI (SATA) para
rendimiento de almacenamiento moderno.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 34.1 | NVMe controller init | PCI BAR0 MMIO; Admin Queue; I/O Queue pairs |
| 34.2 | NVMe namespaces | Enumerar namespaces; `IDENTIFY NAMESPACE`; LBA size |
| 34.3 | NVMe command submission | SQ/CQ doorbell; PRPs para scatter-gather |
| 34.4 | AHCI controller | MMIO init; Port Command List + FIS receive area |
| 34.5 | AHCI DMA | PRDT (Physical Region Descriptor Table); 64-bit DMA |
| 34.6 | Trim/Discard | `NVM_CMD_DATASET_MANAGEMENT` para SSD longevity |
| 34.7 | I/O scheduler | Deadline scheduler; merge de requests adyacentes |

### Criterios de aceptación

- [ ] NVMe drive alcanza throughput > 1 GB/s en lectura secuencial (QEMU NVMe)
- [ ] AHCI SATA drive reemplaza ATA PIO; mismo FS FAT32 accesible
- [ ] TRIM enviado automáticamente tras `unlink` en Btrfs-lite
- [ ] I/O scheduler reduce latencia promedio de random reads en 30%

---

## Fase 35 — Profiling + Tracing (perf-like)
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: instrumentación del kernel para profiling de performance,
tracepoints, eBPF-lite, flame graphs.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 35.1 | PMU sampling | Usar LAPIC Performance Monitoring Counters; `PERF_COUNT_HW_CPU_CYCLES` |
| 35.2 | Tracepoints | Macros `TRACE_EVENT()` en código crítico del kernel (scheduler, VFS, IPC) |
| 35.3 | Ring buffer de eventos | Lock-free per-CPU ring buffer para trace events; exportado vía `/dev/trace` |
| 35.4 | `portix-perf` CLI | `portix-perf record cmd`, `portix-perf report` — flame graph ASCII |
| 35.5 | eBPF-lite VM | Bytecode verificable para filtros de trace; `maps` tipo hash/array |
| 35.6 | `strace` port | Intercepta syscalls via ptrace-like mecanismo; log de cada syscall con timing |
| 35.7 | Kernel lockdep | Detector de lock ordering violations en spinlocks/mutexes |
| 35.8 | KASAN lite | Kernel Address Sanitizer: shadow memory para detectar use-after-free |

### Criterios de aceptación

- [ ] `portix-perf record ls /` produce flame graph legible con hotspots
- [ ] Tracepoint `vfs:open` captura todos los `sys_open` con path y latencia
- [ ] eBPF programa que cuenta syscalls por PID corre sin crash en 60 segundos
- [ ] `lockdep` detecta inversión de lock introducida deliberadamente en test
- [ ] KASAN detecta use-after-free plantado en test unitario del allocator

---

## Fase 36 — Live Kernel Patching
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: aplicar parches de seguridad al kernel en ejecución sin reboot,
estilo `kpatch`/`livepatch`.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 36.1 | Patch ELF format | `.livepatch` section en ELF: lista de funciones a reemplazar |
| 36.2 | Function redirect | Sobreescribir primeros 5 bytes de función con JMP al parche |
| 36.3 | Consistency model | Esperar a que ningún CPU esté en mid-function antes de parchear (stop-machine) |
| 36.4 | `portix-patch apply` | Cargar `.ko`-like patch module; verificar firma; aplicar |
| 36.5 | Patch rollback | Restaurar bytes originales; descargar módulo de parche |
| 36.6 | Patch metadata | Registro de parches activos en `/proc/livepatches` |

### Criterios de aceptación

- [ ] Bug de kernel introducido en función de prueba corregido en caliente sin reboot
- [ ] Parche con firma inválida rechazado antes de aplicar
- [ ] Rollback de parche restaura comportamiento original en < 100ms
- [ ] `portix-patch list` muestra todos los parches activos con version y fecha

---

## Fase 37 — Distributed Filesystem
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: acceder a filesystems sobre red; NFS client, 9P server,
FUSE equivalente para drivers de FS en ring-3.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 37.1 | NFSv3 client | `mount -t nfs server:/export /mnt`; RPC sobre UDP/TCP |
| 37.2 | 9P server | Protocolo 9P/2000.L para compartir directorios con VMs y QEMU virtfs |
| 37.3 | FUSE-portix | Interfaz para escribir drivers de FS en ring-3 en C o Rust |
| 37.4 | SSHFS | Montar directorio remoto sobre SSH vía `FUSE-portix` |
| 37.5 | Cache coherency | Invalidar cache VFS al detectar cambios remotos (NFSv4 delegations) |
| 37.6 | `portix-net-fs` | CLI para gestionar mounts remotos con retry automático |

### Criterios de aceptación

- [ ] NFS mount de export Linux accesible en Portix: `ls`, `cat`, `cp` funcionales
- [ ] QEMU virtfs accesible desde Portix guest vía 9P
- [ ] FUSE driver en Rust que implementa FS de solo lectura desde un ZIP
- [ ] SSHFS monta directorio remoto sobre tunnel Ed25519

---

## Fase 38 — Formal Verification
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: verificar formalmente componentes críticos del kernel usando
`kani` (Rust model checker) y anotaciones de Hoare logic.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 38.1 | Kani harnesses | Probar `translate()`, `map_page()`, `copy_from_user()` con Kani |
| 38.2 | Pre/post conditions | Anotaciones `#[requires]` / `#[ensures]` en funciones críticas |
| 38.3 | Memory safety proofs | Verificar ausencia de use-after-free en buddy allocator con Kani |
| 38.4 | IPC deadlock freedom | Model checking del protocolo IPC con `spin` o equivalente |
| 38.5 | Scheduler liveness | Verificar que todo proceso Ready eventualmente ejecuta (no starvation) |
| 38.6 | CI integration | Kani corre en CI en cada PR; falla si propiedad violada |

### Criterios de aceptación

- [ ] `kani translate()` verifica que toda dirección mapeada tiene translate invertible
- [ ] `kani buddy_alloc()` no encuentra use-after-free en 100K iteraciones simbólicas
- [ ] IPC model checker no encuentra deadlock en protocolo SEND/RECV de 4 procesos
- [ ] CI bloquea merge si verificación falla

---

## Fase 39 — RISC-V Port
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: portar PORTIX a arquitectura RISC-V 64 (RV64GC); kernel
idéntico, HAL abstracto, bootloader SBI.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 39.1 | HAL abstraction | Trait `Arch` en Rust: `flush_tlb()`, `read_cr3()` → métodos abstractos |
| 39.2 | RISC-V boot | SBI firmware call para console; M-mode → S-mode handoff |
| 39.3 | Sv39/Sv48 paging | Port de `paging.rs` a RISC-V page tables |
| 39.4 | PLIC interrupt controller | Port de APIC → PLIC para IRQ routing |
| 39.5 | RISC-V trap handlers | `scause`, `sepc`, `stval`; port de ISR |
| 39.6 | QEMU `virt` machine | Boot completo hasta shell en QEMU `-machine virt -cpu rv64` |
| 39.7 | Cross-toolchain | `riscv64-unknown-none-elf` target para kernel y userspace |

### Criterios de aceptación

- [ ] Kernel PORTIX arranca en QEMU RISC-V hasta prompt `portix$`
- [ ] Process scheduler + context switch funciona en RISC-V
- [ ] Syscall path completo (`ecall` → handler → iret) funcional
- [ ] `hello` ring-3 corre nativamente en RISC-V port

---

## Fase 40 — ARM64 Port (AArch64)
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: portar PORTIX a AArch64; objetivo primario: Raspberry Pi 4/5
y Apple Silicon (vía QEMU).

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 40.1 | AArch64 exception levels | EL0/EL1/EL2; boot en EL2 → drop a EL1 |
| 40.2 | ARMv8 MMU | Tablas de páginas 4KB; TCR_EL1, MAIR_EL1, TTBR0/TTBR1 |
| 40.3 | GIC-v3 | Generic Interrupt Controller; LPI + SPI + PPI routing |
| 40.4 | PSCI | Power State Coordination Interface para SMP core bring-up |
| 40.5 | Device Tree | Parse DTB para descubrir memoria, UARTs, interrupciones |
| 40.6 | Raspberry Pi 4 BSP | PL011 UART, VideoCore mailbox, GPIO, SD/MMC |
| 40.7 | Apple Silicon stub | M1/M2 con QEMU `-machine virt`; no acceso a hardware propietario |

### Criterios de aceptación

- [ ] Boot en Raspberry Pi 4 real hasta prompt `portix$` por UART
- [ ] Multitarea en 4 cores ARM (PSCI SIPI equivalente)
- [ ] SD card accesible; FAT32 montable en RPi4
- [ ] `hello` Rust ring-3 corre en RPi4 con `println!` funcional

---

## Fase 41 — Embedded / IoT Profile
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: variante de PORTIX para microcontroladores ARM Cortex-M4/M7
y RISC-V RV32; footprint < 64 KB de ROM, < 8 KB de RAM.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 41.1 | `portix-embedded` crate | Subset del kernel sin MMU; scheduler cooperativo |
| 41.2 | Cortex-M4 HAL | NVIC, SysTick, MPU regions, FPU |
| 41.3 | No-MMU process model | Procesos en regiones MPU fijas; no virtual memory |
| 41.4 | Minimal RTOS primitives | Semáforos, mutex, message queues en < 2 KB ROM |
| 41.5 | `portix-flash` | Tool para flashear imagen PORTIX-embedded a MCU via SWD/JTAG |
| 41.6 | Sensor drivers | I2C, SPI, UART drivers para sensores BME280, MPU6050, etc |
| 41.7 | MQTT client | Conectar sensor data a broker MQTT via WiFi (ESP32 companion) |

### Criterios de aceptación

- [ ] Kernel PORTIX-embedded arranca en STM32F4 (QEMU `-machine netduino2`) en < 10ms
- [ ] 8 tareas cooperativas con message queues sin starvation
- [ ] Footprint: ROM < 48 KB, RAM < 6 KB para kernel base
- [ ] `portix-flash` programa imagen en STM32F4 real via OpenOCD

---

## Fase 42 — Hard RTOS Mode
> **Badge**: 📋 `PLANIFICADO` · 0%

**Objetivo**: Portix con garantías de tiempo real duro (WCET bounded);
scheduler EDF, preempción total, latencia de IRQ < 10μs.

### Tareas

| # | Tarea | Descripcion |
|---|-------|-------------|
| 42.1 | EDF Scheduler | Earliest Deadline First; cada tarea tiene `deadline` y `period` |
| 42.2 | Fully preemptible kernel | Spinlocks con `preempt_disable`; secciones críticas mínimas |
| 42.3 | IRQ latency budget | Medir con TSC en cada IRQ entry/exit; panic si > 10μs budget |
| 42.4 | WCET analysis | Herramienta `portix-wcet` que analiza binarios con `aiT`-style |
| 42.5 | Priority inheritance | Mutex con PI para evitar priority inversion |
| 42.6 | Temporal isolation | Cgroups de tiempo real: proceso `realtime_t` no bloqueado por `best_effort_t` |
| 42.7 | `SCHED_DEADLINE` syscall | Configurar `runtime`, `deadline`, `period` por proceso |
| 42.8 | Certification artifacts | Documentación para DO-178C nivel C (aviación) |

### Criterios de aceptación

- [ ] IRQ latencia < 10μs en 99.99% de casos bajo carga de 64 procesos (benchmark 24h)
- [ ] EDF no pierde deadline en set de tareas sintéticas con utilización < 80%
- [ ] Priority inheritance resuelve inversión de prioridad clásica (Mars Pathfinder scenario)
- [ ] `portix-wcet` calcula WCET de función de prueba con ±5% exactitud vs medición real
- [ ] `SCHED_DEADLINE` proceso con `period=1ms, runtime=500μs` corre sin miss durante 60 segundos

---

## Timeline Estimado (Fases 0–42)

```
Fase  0 ████████████████████████████  ✅ COMPLETADO
Fase  1 ████████████████████████████  ✅ COMPLETADO
Fase  2 ████████████████████████████  ✅ COMPLETADO
Fase  3 ████████████████████████████  ✅ COMPLETADO
Fase  4 ████████████████████████████  ✅ COMPLETADO
Fase  5 ████████████████████████████  ✅ COMPLETADO
Fase  6 ████████████████████████████  ✅ COMPLETADO
Fase  7 ████████████████████████████  ✅ COMPLETADO
Fase  8 ████████████████████████████  ✅ COMPLETADO
Fase  9 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~3 semanas
Fase 10 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~3 semanas
Fase 11 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~2 semanas
Fase 12 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 13 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~6 semanas
Fase 14 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~2 semanas
Fase 15 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 16 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 17 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 18 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 19 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~8 semanas
Fase 20 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~8 semanas
Fase 21 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~6 semanas
Fase 22 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 23 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~6 semanas
Fase 24 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~5 semanas
Fase 25 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 26 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~5 semanas
Fase 27 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~8 semanas
Fase 28 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 29 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~6 semanas
Fase 30 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~5 semanas
Fase 31 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~6 semanas
Fase 32 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 33 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~10 semanas
Fase 34 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 35 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~5 semanas
Fase 36 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~3 semanas
Fase 37 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~4 semanas
Fase 38 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~6 semanas
Fase 39 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~6 semanas
Fase 40 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~8 semanas
Fase 41 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~5 semanas
Fase 42 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ~8 semanas

Progreso: 8 / 43 fases (21%)
Tiempo restante estimado: ~195 semanas (~4 años) con 1 persona a tiempo parcial
                          ~80 semanas  (~1.5 años) con dedicación full-time
```

---

## Árbol de Directorios Final (Proyectado)

```
portix/
├── boot/                   # Stage1 + Stage2 asm, UEFI loader
├── kernel/                 # Kernel ring-0 mínimo
│   ├── src/
│   │   ├── arch/
│   │   │   ├── x86_64/     # isr.asm, idt.rs, gdt.rs, halt, vmx
│   │   │   ├── riscv64/    # trap.rs, plic.rs, sv39.rs
│   │   │   └── aarch64/    # exceptions.rs, gic.rs, mmu.rs
│   │   ├── mem/            # paging.rs, allocator.rs, buddy.rs
│   │   ├── process.rs      # scheduler + process table
│   │   ├── ipc.rs          # IPC core + mailboxes
│   │   ├── syscall.rs      # dispatch table
│   │   ├── mac.rs          # Mandatory Access Control hooks
│   │   └── rtos.rs         # EDF scheduler, WCET tracking
├── drivers/                # Drivers ring-3
│   ├── ata/                # ATA PIO driver
│   ├── nvme/               # NVMe PCIe driver
│   ├── ahci/               # AHCI SATA driver
│   ├── fat32/              # FAT32 driver
│   ├── ext4/               # ext4 driver
│   ├── btrfs/              # Btrfs-lite driver
│   ├── kbd/                # PS/2 + USB keyboard
│   ├── mouse/              # PS/2 + USB mouse
│   ├── fb/                 # Framebuffer/VESA
│   ├── drm/                # DRM/KMS core + AMDGPU
│   ├── hda/                # Intel HDA audio
│   ├── pci/                # PCI/PCIe bus + hotplug
│   ├── usb/                # xHCI + HID + MSC + Audio
│   ├── serial/             # COM1 driver
│   ├── net/                # RTL8139, e1000, virtio-net
│   ├── bt/                 # Bluetooth HCI stack
│   ├── nvme_sched/         # I/O deadline scheduler
│   └── tb/                 # Thunderbolt driver
├── lib/                    # Librerías compartidas
│   ├── libc/               # libportix.a (C runtime)
│   ├── libportix-rs/       # libportix.rlib (Rust no_std runtime)
│   ├── libm/               # Math library
│   ├── libtls/             # TLS 1.3
│   ├── libvulkan/          # Vulkan ICD loader
│   └── ld/                 # Dynamic linker ld-portix.so
├── usr/                    # Programas ring-3
│   ├── init/               # PID 1 + service manager
│   ├── shell/              # /bin/sh
│   ├── coreutils/          # ls, cat, echo, cp, mv, rm, find, grep, sed, awk
│   ├── net/                # ifconfig, ping, nc, curl, wget, ssh, httpd
│   ├── crypto/             # portix-crypt, gpg-lite
│   ├── vm/                 # portix-vm hypervisor CLI
│   ├── pkg/                # pxp package manager
│   ├── perf/               # portix-perf, strace, portix-wcet
│   ├── patch/              # portix-patch live patching tool
│   ├── bt/                 # btctl Bluetooth CLI
│   ├── audio/              # aplay, mp3play, audio server
│   ├── raid/               # portix-raid volume manager
│   └── servers/            # httpd, sshd, ntpd, ftpd
├── embedded/               # portix-embedded MCU profile
│   ├── cortex-m4/          # STM32F4 HAL
│   └── rv32/               # RV32 HAL
├── verify/                 # Kani harnesses + formal specs
├── scripts/                # Build + toolchain + flash tools
│   ├── ring3-toolchain.sh  # C cross-toolchain
│   ├── rust-setup.sh       # Rust target setup
│   ├── build.py            # Main build orchestrator
│   └── portix-flash        # MCU flash tool
├── plans/                  # Roadmap y documentación técnica
│   ├── roadmap.md          # Este documento
│   ├── team.md          # Este documento
├── docs/  
│   ├── ARCHITECTURE.md
│   ├── BOOT.md
│   ├── MEMORY.md
│   ├── GRAPHICS.md
│   ├── STORAGE.md
│   ├── NETWORK.md
│   └── SECURITY.md
└── tests/                  # Suite de pruebas automatizadas
    ├── unit/               # Tests por módulo
    ├── integration/        # Boot + syscall tests en QEMU
    ├── kani/               # Verificación formal
    └── fuzz/               # Fuzzing de parsers (ELF, FAT32, TCP, USB)
```

---

*Última actualización: 2026-06-11 — PORTIX OS by Omar*