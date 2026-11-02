# T-035: diseño de memoria protegida y seguridad SMP

**Estado: corrección experimental medida en BIOS, sin certificar.** Este documento distingue hechos medidos de condiciones de diseño. La protección de páginas materializadas ya no falla en las pruebas acotadas descritas abajo; siguen abiertos T-036 a T-038, las carreras de TLB, UEFI, el hardware físico y la certificación. No se autoriza ninguna optimización de TLB.

## Evidencia actual

- En la BIOS reconstruida, una página materializada conservó su valor legible 77 después de `mprotect(PROT_NONE)`, tanto en CPU 0 como en CPU 1: T-035.
- En una reserva anónima no materializada, `PROT_NONE` provocaba una señal en la dirección correcta pero `si_code=SEGV_MAPERR=1`, no `SEGV_ACCERR=2`. Los controles reales diferenciaron una página accesible (valor 0, sin señal) y una desmapeada (MAPERR). Linux aprobó los tres casos; QEMU base aprobó 2/3.
- Un cambio acotado en `isr_handlers.rs` reclasifica únicamente una PTE perezosa con protección cero para fallos de usuario. En copia BIOS experimental se aprobaron 9/9 casos seleccionados, incluyendo los tres anteriores y restauración a RW; el ensayo SMP de páginas presentes sigue 3/4, con el fallo T-035 sin corregir. Véanse `docs/evidencias/b203/t035c-qemu-base.log`, `t035d-qemu.log` y `t035d-smp-qemu.log`.
- **T-040, hallado el mismo día:** una reserva explícita `MAP_FIXED` + `PROT_NONE` dentro de la ventana de crecimiento de pila no protegía nada: el #PF de lectura la convertía en página de pila escribible con valor cero. La prueba BIOS lo reprodujo (`t035e-pila-qemu.log`, 2/3) y la referencia Linux pasó el caso con `SIGSEGV/ACCERR`. La condición de `crecer_pila` en `isr_handlers.rs` exige ahora un hueco real (PTE ausente o cero; nunca una reserva perezosa), y la copia BIOS experimental dio 6/6 incluyendo el crecimiento legítimo de pila, la reserva protegida, los controles y la regresión de T-039 (`t035f-qemu.log`). No se ejecutaron suites completas, UEFI ni pruebas de hardware.
- **Corrección de la representación (`t035g`, 31-10):** `mprotect(PROT_NONE)` deja la entrada
  sin `PRESENT` conservando marco y protección. Sonda BIOS completa **21/30**: pasan los 21 casos
  que no dependen de los defectos abiertos —incluidos `fork`, `munmap`, `MAP_FIXED`, las dos
  reservas perezosas, las tres pruebas de pila y `protegida-materializada`— y fallan exactamente
  los 9 de T-036 (`madvise` NONE/RO), T-037 (`usuario`, `pipe-invalida`, `pipe-parcial`) y T-038
  (los cuatro de `socketpair`, dos de ellos por prerrequisito ENOSYS). La sonda SMP T-033 pasó
  **4/4**, incluida la protección observada desde la CPU 1. Referencia Linux del fixture: **29/29**
  (se omite `pila-creciente`, mecanismo propio de Portix).
- **Doble arranque (31-10):** imagen dual reconstruida (`portix.img`=`portix-dual.img`
  `b95b01a2…`, `portix-uefi.img` `9a2ca631…`). Con los mismos seis modos, **6/6 en BIOS** y
  **6/6 en UEFI** (OVMF), ELF del ext2 idéntico; arranque UEFI 4/4. La sonda UEFI
  (`scripts/sonda_t035_uefi.py`) inyecta el ELF en la partición GPT+ext2 de una copia privada
  porque `curl` falla al arrancar bajo UEFI (T-041). Sigue sin haber hardware físico, ejecución
  bajo `PROT_NONE` ni recertificación.
- La observación física de dos marcos reutilizados es puntual; las pausas QMP alteran la planificación y no prueban todas las carreras. Las sondas no constituyen certificación ni prueba de silicio real.

## Causa raíz de T-035 (confirmada por la traza del propio núcleo)

`mprotect_page` conservaba el bit `PRESENT` para una página ya materializada: con `PROT_NONE`
sólo limpiaba `WRITABLE`, añadía NX y reescribía la entrada. En x86-64 no existe una página
presente sin permiso de lectura para Ring 3 —el bit R/W sólo limita la escritura—, así que la
lectura seguía funcionando. La traza de la corrección lo confirma en la misma dirección del
caso medido: antes de proteger, la PTE era presente; tras `mprotect(PROT_NONE)` el núcleo
escribe `0x8080000005475000` (sin presente, marco `0x5475000`, bit de página protegida y NX) y la
lectura pasa a entregar `SIGSEGV/ACCERR`; tras `mprotect(PROT_READ)` escribe
`0x8000000005475025` (presente, usuario, NX, sin escritura, contenido 77 intacto) y la escritura
da `ACCERR`.

## Representación implementada (experimental)

- `PROTEGIDA_FLAG = 1<<55`; la protección se guarda en los bits 52..54, igual que en las
  reservas perezosas. La entrada NO es presente y conserva su dirección física y, si la página
  era COW, la marca COW (una restauración con escritura no concede escritura sin copiar).
- `mprotect(PROT_NONE)` oculta la página; cualquier otra protección la vuelve presente con el
  MISMO marco, sin asignar memoria.
- El fallo de página de una oculta se resuelve restaurándola si la protección permite el acceso;
  si no, señal `ACCERR`. La ruta es la misma para Ring 3 y para el núcleo, de modo que una
  syscall no puede saltarse la protección por la dirección de usuario.
- `for_each_user_page`, `pagina_del_proceso`, `unmap_page`, `munmap`, la liberación del espacio y
  el `MAP_FIXED` de `mmap` reconocen la entrada oculta y devuelven su marco.
- `fork` copia en profundidad las páginas ocultas con la misma protección (`copiar_protegidas`),
  en lugar de compartirlas: compartir obligaría a tratar como COW cualquier restauración.

## Lo que sigue abierto dentro de T-035

- No se ha probado **ejecución** de una página protegida ni reservas con respaldo de fichero.
- `madvise(MADV_DONTNEED)` no ve la página oculta (`translate` no la devuelve), así que T-036
  sigue abierto: `madvise` sobre NONE/RO conserva 77.
- `mremap` no mueve entradas perezosas ni, por tanto, protegidas; no hay prueba de ello.
- No hay prueba de carrera entre un ocultado y un acceso desde la otra CPU; el shootdown se
  mantiene con el protocolo anterior y su resultado se sigue ignorando en los envoltorios.
- `verify_user_buffer` comprueba el rango, no la PTE; no se ha auditado cada syscall que copia
  desde usuario para confirmar que todas devuelven `EFAULT` con una página oculta.
- La liberación se apoya en que todos los recorridos ven la entrada; se midió
  `marcos_libres=109409` constante en 33 procesos de la sonda completa, pero no es una prueba de
  estrés.
- Falta hardware físico, dos suites verdes consecutivas sobre la imagen reconstruida y
  recertificación. La ruta de red bajo UEFI está rota (T-041), ajena a esta corrección.

## Invariantes antes de reparar páginas materializadas

1. **Propiedad frente a accesibilidad.** Una página protegida sigue perteneciendo al proceso y ocupando su dirección, aunque el usuario no pueda leerla ni escribirla. Los recorridos de `fork`, `exec`, salida y liberación no deben confundirla con un hueco ni con memoria del núcleo. Cualquier marcador elegido debe distinguir las entradas heredadas de supervisor y no colisionar con `COW_FLAG`, `DEMAND_PAGING_FLAG` o `SWAP_FLAG`. No se ha elegido todavía una codificación definitiva.
2. **Contenido y permisos.** Reproteger `RW → NONE → RW` conserva datos y permisos aplicables, incluyendo COW; un primer acceso a una reserva perezosa no debe crear un marco cuando la protección lo prohíbe. La semántica de lectura, escritura y ejecución, NX y `siginfo` debe contrastarse con Linux.
3. **Retiro y reclamación.** `munmap`, `MAP_FIXED` y `MADV_DONTNEED` deben reconocer páginas protegidas, quitar la entrada adecuada, invalidar todas las traducciones pertinentes y **sólo después de confirmar** que no pueden usarse devolver su marco o su referencia. La ruta de error no puede continuar como si hubiera acuse; también deben auditarse `free_user_heap_pages`, `free_user_pages_dispersos`, `free_address_space` y los caminos COW.
4. **Acceso desde el núcleo.** Las copias hacia o desde búferes de usuario deben respetar permisos y devolver el error exacto. `verify_user_buffer` comprueba actualmente rango aritmético, no la PTE; las ramas que acceden directamente o ignoran fallos de copia requieren auditoría individual. Una PTE presente de supervisor no debe permitir que una syscall eluda `PROT_NONE` mediante su dirección de usuario.
5. **`fork` y vida del espacio.** El hijo debe heredar protección y contenido, sin compartir escrituras privadas incorrectamente ni perder referencias. Auditar imagen, intérprete, pila, `brk` y ventanas `mmap`/MAP_FIXED; tanto recorridos dispersos como bucles con `translate` filtran hoy por `PRESENT|USER`.
6. **Fallos de asignación y de invalidación.** `mprotect_range` ignora actualmente los errores de `mprotect_page`; una implementación nueva no debe devolver éxito si no ha aplicado la protección. Debe definir explícitamente qué ocurre ante fallo de asignación, cambios parciales y falta de acuse, manteniendo cerrados los lotes TLB.

## Observaciones estáticas que requieren prueba

- En `sys_munmap`, el `mmap` perezoso MAP_FIXED, `madvise` y algunos recorridos de salida aparecen liberaciones antes de `unmap_page`; el resultado del shootdown se ignora en los envoltorios. Es un **riesgo de orden lógico**, no una explotación reproducida.
- `tlb_shootdown` compara un contador global de acuses antes y después, con envíos serializados. Hay que investigar si un acuse tardío tras un timeout, o un envío por otra ruta, podría confundirse con el de una solicitud posterior; no se ha reproducido ni demostrado esa carrera. La corrección debe identificar generación y destinatario o demostrar una alternativa equivalente, y cubrir la falta de acuse sin liberar marcos potencialmente visibles.
- `crecer_pila` se invocaba tras fallar la materialización de una página no presente. **Reproducido (T-040)** y corregido experimentalmente: la reserva protegida en la ventana de pila se convertía en página escribible. La guarda nueva exige que la PTE sea un hueco y no una reserva perezosa; faltan las pruebas de concurrencia, de reserva de fichero en esa ventana y de hardware.

## Matriz mínima de aceptación

- Controles negativos que omitan la protección, el desmapeo y el acuse, con errno/si_code/dirección exactos.
- Página presente y perezosa: lectura/escritura/ejecución, restauración y contenido, desde CPU 0 y CPU 1; referencias Linux.
- `munmap`, `MAP_FIXED`, `madvise`, `fork`/COW, `exec` y salida: verificar propiedad, datos y liberación; pruebas de agotamiento y limpieza.
- TLB: escritura previa en ambas CPU, cambio de permiso, acuse correlacionado, remapeo y reutilización física real; inyección controlada de acuse tardío/ausente y comprobación de que no se libera prematuramente.
- Una imagen identificada para BIOS y UEFI; después dos suites completas consecutivas y recertificación sobre la imagen exigida para cerrar b203. La evidencia parcial BIOS no reemplaza esos pasos.
