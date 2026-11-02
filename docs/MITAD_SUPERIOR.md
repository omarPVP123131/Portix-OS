# Núcleo en la mitad superior — diseño, fases y verificación

Documento de ingeniería del ciclo 49 (opción 1 aprobada por el usuario:
"Trabajemos la opción 1 pero con ultra cuidado").

## 1. El problema

Hasta el ciclo 48 el núcleo de Portix se enlazaba **y** ejecutaba en la
dirección física 0x200000 con identidad plana (virtual = físico):

```
0x000000 ┬ IVT/BDA, stage2, tablas de páginas de arranque (0x1000..0x5000), staging
0x200000 ├ ███ KERNEL (.text .rodata .data .bss .stack)  →  ~0x580000
0x500000 ├ pila de arranque (0x508000), PortixBootInfo (0x600000), datos UEFI (0x700000)
0x1000000├ ███ HEAP del núcleo (64 MiB, identidad)
0x5000000┤
0x20000000 ─ programas nativos de Portix (los enlaza nuestro propio toolchain aquí)
```

Todo binario **estático no-PIE** de una distribución Linux (busybox y tcc de
Alpine, cualquier `gcc -static`) está enlazado para vivir en **0x400000** con
direcciones absolutas. Cargarlo exigía escribir sus páginas sobre el `.data`
del núcleo: triple fallo inmediato (observado en la sonda T1, ciclo 49).

## 2. Diseño elegido: imagen en los 2 GiB superiores, identidad conservada

* El núcleo se **enlaza** en `0xFFFFFFFF80200000` (`kernel/linker.ld`,
  `KERNEL_VMA_OFFSET = 0xFFFFFFFF80000000`) con el modelo de código `kernel`
  (`kernel/x86_64-portix.json`, también la plantilla de `scripts/build.py`):
  igual que Linux, las referencias a símbolos caben en 32 bits con signo.
* Se sigue **cargando** físicamente en 0x200000: `boot/stage2.asm` y el
  cargador UEFI **no cambian** (zonas protegidas intactas salvo el `linker.ld`,
  cuyo cambio autorizó el usuario).
* `_start` (`kernel/src/main.rs`, `global_asm!`) es un trampolín que, usando
  sólo direccionamiento relativo a RIP (válido ejecutando en la dirección
  física), (1) limpia `.bss`, (2) rellena `HH_PD` con 512 páginas de 2 MiB
  (físico 0..1 GiB), (3) cuelga `HH_PDPT[510] -> HH_PD` y `PML4[511] -> HH_PDPT`
  —**sólo si PML4[511] está libre**; si no, escribe `HH!` por COM1 y se
  detiene—, (4) recarga CR3 y salta a la dirección alta, donde fija la pila
  (`__stack_top`, ya alta) y llama a `rust_main`.
* `rust_main` llama inmediatamente a `verificar_mitad_superior()`: comprueba
  RIP y RSP altos, `virt_to_phys(rust_main)` dentro de la imagen física,
  `translate(cr3, rust_main) == virt_to_phys(rust_main)` y lo mismo para la
  tabla `HH_PD`. Imprime `[HH] nucleo en mitad superior: rip=... fisico=...`
  o `[HH-FALLO]` y para la máquina. La suite headless y la sonda UEFI exigen
  la línea `[HH]`.
* `new_address_space()` copia todas las entradas presentes del PML4 maestro,
  así que **cada proceso hereda PML4[511]**: el núcleo es visible sobre
  cualquier CR3 de usuario (necesario para syscalls e interrupciones).

### Qué sigue en identidad (Fase A) y por qué no rompe

| Qué | Dirección | Acceso del núcleo |
|---|---|---|
| Tablas de páginas (todas) | físicas | `read_entry(phys)` por identidad de PML4[0] |
| Montículo del núcleo | 0x1000000–0x5000000 | identidad |
| PortixBootInfo, pila de arranque, datos UEFI | 0x500000–0x800000 | identidad |
| LFB, BARs PCI, APIC, ACPI | < 4 GiB altos | identidad |

Para que un mapeo de usuario no pueda **ocultar** esas zonas al núcleo
mientras corre sobre el CR3 del proceso, `paging::ZONAS_IDENTIDAD_RESERVADAS`
las enumera y `paging::rango_usuario_permitido()` las prohíbe en el cargador
ELF (`load_segments_into_cr3`) y en `mmap` con pista. La ventana que queda
libre para binarios no-PIE es `0x200000–0x500000` y `0x800000–0x1000000`:
suficiente para busybox (1 MiB), tcc y la mayoría de estáticos; la Fase B
la amplía al espacio completo.

### Supuestos "virtual = físico" auditados y corregidos

* `drivers/net/e1000.rs`: anillos y búferes de descriptores son `static` de la
  imagen → ahora se programan con `paging::virt_to_phys(...)`.
* NVMe/AHCI/xHCI/HDA ya usaban `mmio::dma_alloc` (`translate()` sobre el
  montículo): sin cambios.
* `console/terminal/commands/install.rs`: el tamaño del núcleo se mide en
  físico (`virt_to_phys(__kernel_end) - 0x200000`).
* `elf.rs::load_segments_into_cr3`: una traducción heredada de la identidad
  (marco físico no dinámico) **no se reutiliza**; se mapea un marco nuevo y
  `map_page` divide la página gigante en la copia privada del proceso.
* IDT/GDT/TSS/LSTAR ya escribían los 64 bits de las direcciones: sin cambios.
* `lkm/mod.rs` (**destapado por la suite, no por la revisión**): un módulo
  `.ko` cargado en el montículo (dirección baja) no puede alcanzar el núcleo
  alto con `PC32/PLT32` (desplazamiento > 2 GiB). El módulo se ejecuta ahora
  por el alias alto del mismo marco (`montículo + KERNEL_VMA_OFFSET`, que
  `HH_PD` cubre) y toda reubicación de 32 bits fuera de rango se rechaza con
  error en lugar de truncarse.
* `isr.asm` (zona protegida) no contiene referencias absolutas de 32 bits:
  sin cambios.

## 3. Lo que el nuevo espacio destapó (T1, busybox de Alpine)

Con el núcleo fuera del camino, el binario corrió y fue revelando defectos
reales del traductor Linux, todos corregidos y cubiertos por pruebas:

1. `AT_PHDR` se pasaba como `e_phoff` (0x40) cuando no hay `PT_PHDR`; ahora
   `p_vaddr - p_offset + e_phoff` del `PT_LOAD` que contiene las cabeceras.
2. Las rutas pasadas en `argv` viven junto al tope de la pila:
   `copy_from_user(.., 256)` exigía 256 bytes mapeados → `EPERM`. Nuevo
   `paging::copy_cstr_from_user` valida página a página hasta el NUL.
3. `getdents64` no tenía cursor: devolvía siempre el listado completo y nunca
   0 → `ls` de busybox giraba sin fin. Cursor real en `pos` del descriptor,
   `d_ino ≠ 0`, `d_reclen` alineado a 8, `DT_DIR/DT_REG`. La prueba 532 de
   `test_posix` ahora recorre hasta 0 con búfer pequeño y detecta repetidos.
4. Faltaban en la tabla Linux: `lstat` (6), `dup` (32), `dup2` (33),
   `statx` (332) y `fcntl(F_DUPFD_CLOEXEC)`.
5. `execve` no empujaba **ningún** auxv: ahora el mismo vector que la ruta de
   creación (`AT_PHDR/PHENT/PHNUM/PAGESZ/ENTRY/RANDOM/EXECFN/...`).
6. `fork` sólo copiaba la imagen en `0x20000000..0x21000000`: el hijo de un
   no-PIE salía sin texto (`CR2 == RIP`). `process::ventana_imagen(image_end)`
   deduce la ventana real (nativa, no-PIE o PIE).

## 4. Verificación

* `scripts/headless_runner.py`: `[HH]` en el arranque, `fisico=0x2…`,
  `pkg install busybox-static` (SHA-256), `uname -m`, `cat`, `ls`, y el guion
  `/usr/src/prueba_t1.sh` (tuberías, subshells, bucles, redirección,
  `sh -c` anidado) con `[T1-PASS]` y 6 líneas numéricas.
* `scripts/uefi_probe.py` (nuevo, integrado en `recertify.py`): arranque real
  con OVMF: `EFI:entry` → `[HH]` → `present done`.
* `/proc/certification`: nueva comprobación "Nucleo en la mitad superior".
* `scripts/fetch_distro.py`: descarga el `.apk` oficial de Alpine, verifica el
  SHA-256 del paquete y del binario (valores fijados) y lo publica como
  `busybox-static.pkg`. Sin red y sin copia verificada, la prueba T1 falla.

## 5. Fase B (pendiente, planificada antes de T3)

Mapa físico directo en la mitad superior (`0xFFFF888000000000 + p`, como
Linux): `read_entry`, el montículo, `dma_alloc`, MMIO, bootinfo y LFB pasan a
usarse por ese mapa; PML4[0] deja de copiarse a los procesos y
`ZONAS_IDENTIDAD_RESERVADAS` desaparece: todo el espacio bajo (0–128 TiB)
queda para el usuario. Requiere tocar `kernel/src/mem/allocator.rs` (zona
protegida) sólo en la base virtual del montículo; se pedirá autorización
explícita en su momento.
