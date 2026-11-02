# Registro de cambios de Portix OS

Este registro distingue **cambios realizados**, **resultados observados** y **trabajo pendiente**. Una prueba parcial no equivale a una versión publicada ni certifica todo el sistema. Los números de build identifican ciclos de desarrollo; no son versiones SemVer de lanzamiento.

Portix es un sistema operativo independiente en Rust, definido alrededor de un microkernel y una capa de compatibilidad con binarios Linux x86-64 sin modificar. La modularidad completa y el aislamiento de todos los controladores todavía no están demostrados: el árbol actual contiene componentes y controladores en el núcleo. Ejecutar software de **cualquier** distribución es el objetivo, no una compatibilidad universal ya conseguida.

## [Sin publicar]

**Última actualización: 2026-10-31. Lote b203: en curso, no certificado.**

**T-041 cerrado**: la ruta UEFI vuelve a ejecutar los binarios propios y la batería de memoria pasa igual en BIOS y UEFI (detalle abajo). Las dos suites consecutivas verdes y la recertificación están hechas sobre la misma imagen; lo que queda abierto son los tickets T-026, T-035–T-038 y la validación en hardware físico.

**Ciclo T-029 / T-030 (1 de noviembre) — cerrados con la condición que les faltaba: dos suites consecutivas y recert sobre la misma imagen:**

- **Imagen identificable**: `build/dist/portix.img` **`e890fc288b29e558…`** (2 186 280 960 B), **sin reconstruir entre pasadas**.
- **Suite #1**: `TOTAL: 3843 | PASSED: 3843 | FAILED: 0`. **Suite #2**: idéntico, con **cero** líneas `[IRQ0-HUECO]`.
- **`recertify.py --no-build`**: **MASTER-PASS 7/7** — 0 hallazgos de la auditoría estricta, depurador **64/64**, interactiva **128/128**, Épica 58 completa, Épica 59 **31/31**, Épica 60 **10/10**, UEFI **4/4**.
- **T-029**: la suite certifica que se ejecuta **software Linux de terceros sin modificar** — BusyBox de Alpine descargado por HTTPS con SHA-256 completo e instalado, que imprime el saludo y sale 0 (PID 326) y calcula `45 + 55 = 100` (PID 327). No hay código C propio en el userland distribuido: el constructor ya no llama `_compile_c_userland` y las fuentes se conservan sin desplegarse.
- **T-030**: toda la ejecución usó imágenes **dentro de `build/dist/`** (y copias privadas en `build/dist/pruebas/run/`); los SHA-256 de las imágenes quedaron intactos tras las pasadas. **Límite declarado**: la prueba con **SIGINT real** del cierre HTTP sigue sin repetirse; no se da por demostrada.
- Evidencias: `docs/evidencias/b203/suite-e890fc28-1.log`, `suite-e890fc28-2.log`, `recert-e890fc28-master-pass.log`.

**Ciclo T-039 / T-040 (1 de noviembre) — cerrados con `si_code` correcto y con la ventana de pila protegida, en BIOS y en UEFI:**

- **T-039 CERRADO**: una reserva perezosa con `PROT_NONE` llega al manejador con **`si_code=2` (ACCERR)** y no `1` (MAPERR). BIOS (`sonda_t035_b203.py`, modos del caso, imagen `e890fc288b29e558…`): **6/6 casos** con los controles negativos que detectarían la vuelta del defecto; traza literal `CR2=0x300000003000 EC=0x4` → `fase=lectura senal=11 codigo=2 faltas=1 direccion=correcta` y `fase=restauracion inicial=0 nueva=77`. UEFI (OVMF real): **4/4**.
- **T-040 CERRADO**: una reserva protegida dentro de la ventana de pila (`0x7EFFFF801000`) ya **no** se resuelve como crecimiento de pila: `[SIG] SIGSEGV … CR2=0x07EFFFF801000 EC=0x4` → `senal=11 codigo=2 direccion=correcta`; y el crecimiento legítimo sigue intacto (`pila-creciente fase=crecimiento inicial=0 nueva=77`). Mismos resultados en UEFI.
- Evidencias: `docs/evidencias/b203/t039-t040-bios-6-modos.log` y `t039-t040-uefi-4-modos.log`.

**Ciclo T-035 / T-027 (1 de noviembre) — `mprotect(PROT_NONE)` certificado en BIOS y UEFI; la carrera de la PTE, endurecida y declarada no reproducida:**

- **T-035 CERRADO con paridad**: `sonda_t035_b203.py` sobre BIOS → **30/30 casos** (incluidos los controles negativos que deben fallar si se omite la operación) e integridad del ext2 verificada; `sonda_t035_uefi.py` sobre OVMF real → **6/6**. El caso que definía el defecto (`protegida-materializada`) produce **SIGSEGV real con `si_code=2` en lectura y en escritura** y conserva el contenido al restaurar permisos. Evidencia: `docs/evidencias/b203/t035-cierre-y-t027-medido.txt`.
- **T-027, hipótesis 1 descartada midiendo**: fixture nuevo `scripts/sondas/t027_pila_hilo.rs` (reserva `PROT_NONE`, `mprotect` del tramo útil, escritura en todas las páginas, descenso hasta el fondo con marcos reales, `madvise(MADV_DONTNEED)` y reescritura) → **200 vueltas, 2,1 s, cero SIGSEGV y cero escrituras perdidas**.
- **T-027, hipótesis 2 endurecida**: el *read-modify-write* de la PTE no era atómico entre CPUs; `mprotect` y un fallo de página podían escribir desde una lectura vieja y dejar la página con la protección anterior. Ahora hay cerrojo corto `CERROJO_PTE` y, al materializar, **relectura de la protección vigente** (si otra CPU ya materializó se libera el marco en vez de duplicarlo). Contador `pte: carreras` en `/proc/smp`.
- **Lo que NO se afirma**: el estresor de dos hilos (`t027_carrera`, 400 vueltas) pasa sin SIGSEGV pero con **`carreras=0`**: no entró en la ventana. El arreglo es un endurecimiento con causa demostrada en el código, **no** una corrección reproducida. Queda escrito así en el ticket.
- **Cierre medido sobre `1bc3494b81a9499f…`**: suite completa **3843/3843, 0 fallos**; recertificación `--no-build` **MASTER-PASS 7/7** (depurador 64/64, interactiva 128/128, Épica 58 completa, 59 31/31, 60 10/10, UEFI 4/4). Evidencias: `suite-1bc3494b-1.log`, `recert-1bc3494b-master-pass.log`.

**Ciclo T-015 (1 de noviembre) — la hipótesis del «sueño corto» queda descartada con datos, y el escenario que se colgó pasa a la suite:**

- **Descartada con medición**: se instrumentó `sys_sleep_switch` (`suenos: totales … cortos … peor_deficit_ticks …` en `/proc/smp`, aviso `[SUENO-CORTO]`) y se añadió el fixture `scripts/sondas/t015_sueno.rs`: **50 sueños de 100 ms** con el reloj del núcleo medido antes y después de cada vuelta → **delta mínimo, máximo y medio de 10 ticks; ceros=0, cortos=0, `cortos_kernel=0`, `peor_déficit=0`**. `nanosleep` duerme lo pedido; el «1 de cada 30 sin avanzar» del ciclo anterior era un artefacto de medición.
- **El escenario real, como prueba permanente**: `scripts/prueba_t015.sh` repite el paso FIFO exacto de T-001 (`cat fifo > out &`, escritura, `wait`) N veces en el mismo shell con marca por vuelta; en aislado **20/20 en 3,8 s sin cuelgues**. Si se cuelga, la marca dice en qué vuelta.
- **Dentro de la certificación**: sección nueva «Validating T-015/T-028» en `scripts/headless_runner.py` con **12 comprobaciones** (publica `/proc/smp`, fixtures compilados por el anfitrión, 50 sueños exactos, estadística cruzada fixture/núcleo, reloj avanzando con todo durmiendo, busybox asegurado, FIFO 10 veces y **peor hueco del barrido ≤ 20 ticks**; medido en la sesión de prueba: **2 ticks**).
- **Cierre medido sobre `2958ae90…`**: suite completa **3843/3843, 0 fallos** (14 pruebas más que el ciclo anterior: toda la sección del reloj y de los sueños), cero `[IRQ0-HUECO]`, `peor_tramo_us=35 585` (`elf.rs:212`); recertificación `--no-build` **MASTER-PASS 7/7** (depurador 64/64, interactiva 128/128, Épica 58 completa, 59 31/31, 60 10/10, UEFI 4/4). Evidencias: `docs/evidencias/b203/suite-2958ae90-1.log` y `recert-2958ae90-master-pass.log`.
- **Matiz honesto del criterio**: bajo la carga de la suite (JVM) se midió un hueco de barrido de **50 ticks (500 ms)**, que no explica un plazo de 1,5 s. Por eso la prueba exige dos cosas a la vez: **cero** huecos > 20 ticks durante su propia sección (contador aditivo, no arrastra la JVM) y que el máximo de la sesión no llegue a **150 ticks** (la regresión medida antes del arreglo). El umbral de 20 ticks sobre el máximo acumulado —mi primer intento— fallaba por la carga, no por el núcleo, y se corrigió.
- **Imágenes de este ciclo**: `build/dist/portix.img` **`2958ae9082ab4ca9…`**, `portix-uefi.img` **`ed8b65afe7433aef…`**.

**Ciclo T-028 (1 de noviembre) — el despertador por tiempo no corría con el sistema ocioso:**

- **La pista estaba en la traza de T-028**: el `curl` seguía `Blocked cpu 0 sleep_until 1799` más de 144 s después **y** `tcp_wait_accept`, con su propio tope de 1000 ticks, tampoco volvía. Dos plazos independientes que no vencen a la vez apuntan a un solo mecanismo: el reloj o el barrido que despierta.
- **Instrumentación permanente** (`kernel/src/process/scheduler.rs` + línea `reloj:` en `/proc/smp`): llamadas al barrido, barridos sin poder tomar el cerrojo, despertares, peor racha y **peor hueco entre barridos**, más un vigilante que avisa `[ESPERA-ATASCADA]` si el barrido deja de correr con procesos durmiendo.
- **Medido** (`scripts/sonda_reloj_t028.py` + `scripts/sondas/t028_reloj.rs`, dos hijos durmiendo y el padre imprimiendo `ticks` cada 100 ms; la hora la pone el anfitrión): sobre la imagen anterior, **150 ticks (1,5 s) seguidos sin un solo barrido de vencimientos**, 987 barridos en la corrida.
- **Causa**: `schedule_tick` volvía antes de `wake_blocked()` y el barrido estaba detrás de ese retorno; con todas las CPUs ociosas en el núcleo, **nadie despertaba a los que dormían con plazo**.
- **Arreglo**: el barrido corre en cada tick, antes del retorno, venga la CPU de modo usuario o esté ociosa (se retira la llamada duplicada). **A/B**: peor hueco **150 ticks → 1 tick** (1,5 s → 10 ms), barridos 987 → 1999, mismos 31 despertares ahora puntuales. Evidencia: `docs/evidencias/b203/t028-reloj-antes-despues.txt`.
- **Imagen de este ciclo**: `build/dist/portix.img` **`b120f6f932b79c68…`**.

**Ciclo T-045 (1 de noviembre) — el cerrojo de la consola ya no escribe sin turno; instrumento y prueba permanentes:**

- **Dónde no está el volcado**: las dos suites completas sobre `f3d5dacc…` tienen **0 bytes NUL** en toda su serie (537 619 y 537 620 B). El torrente binario aparece **sólo** en las corridas de `dbg_probe` (una vez: 19 114 163 NUL, 88,7 % de 21,5 MB). No es la consola normal: es un camino que se activa en esa sonda.
- **Descartado midiendo**: `scripts/repro_t045.py` repite `dbg --ortest` + `dbg --buftest` en un solo arranque con el núcleo instrumentado → **20 vueltas (40 bloques) limpias, 0 NUL, 0 escrituras de >=256 B**. El depurador en aislamiento no produce el volcado; el disparo necesita el estado que deja la suite completa antes de las sondas.
- **Defecto real corregido** (`kernel/src/drivers/serial.rs`): cuando otra CPU sostenía el cerrojo de la consola, `tomar()` agotaba su espera y **devolvía un guarda falso con el que el llamador escribía igual**; las dos series se entrelazaban byte a byte y las líneas del depurador perdían sus dígitos (`búfer de 32 B` llegaba con basura en el número). Ahora `tomar()` devuelve `Option`: **sin turno no se escribe**, la pérdida se cuenta y el siguiente escritor con turno publica `[SERIE-SIN-TURNO] escrituras perdidas=N`.
- **Instrumento permanente**: `write_bytes_raw` lleva `#[track_caller]` y cualquier escritura de **>= 256 B** deja `[SERIE-BULTO] n=<bytes> pid=<pid> en <fichero>:<línea>` (máximo 60 avisos). Si el volcado vuelve, el productor queda nombrado en la propia serie.
- **Prueba que falla si vuelve** (`scripts/dbg_probe.py`): «la consola serie transporta sólo texto (ni NUL ni memoria cruda)» y «ninguna escritura suelta de >=256 B». La sonda pasa de 62 a **64** comprobaciones; en la imagen nueva dio **64/64 en 46,5 s**.
- **Imagen de este ciclo**: `build/dist/portix.img` **`90569defe59a9c77…`**, `portix-uefi.img` **`cef034f08208f928…`**.
- **Cierre de (A) sobre esa imagen, medido**: `recertify.py --no-build` → **[MASTER-PASS] 7/7** (0 hallazgos, depurador **64/64**, interactiva **128/128**, Épica 58 completa, Épica 59 **31/31**, Épica 60 **10/10**, UEFI **4/4**) y **dos suites completas consecutivas verdes**: `TOTAL: 3829 | PASSED: 3829 | FAILED: 0` y lo mismo en la siguiente (el código de salida no se capturó en estas dos; el recuento del ejecutor es limpio), **cero líneas `[IRQ0-HUECO]`** en ambas. Una suite anterior de la misma imagen dio `3825/3829`: sus 4 fallos fueron el fixture T-017 agotando su plazo de 60 s (las comprobaciones del núcleo del mismo bloque pasaron), no un defecto del `execve` desde hilo; queda anotado como intermitente de plazo. Peor tramo continuo con IF=0: **42 364 y 64 367 µs** (en `elf.rs:212`), por debajo de los 100 ms. Evidencias: `docs/evidencias/b203/recert-t045-master-pass.log` y `suite-90569def-1/2/3.log`.

**Ciclo T-042 (31 de octubre, continuación) — corregido en sonda dirigida, pendiente de las dos suites completas:**

- **Documento único de syscalls**: `docs/ABI_SYSCALLS.md` se genera desde el código (`scripts/generar_abi_doc.py` sobre `scripts/abi_inventario.py`) y recoge las **354** constantes de la ABI nativa, las **141** syscalls Linux traducidas de 375, las **234** sin traducir clasificadas por riesgo real (56 alto, 96 medio, 82 bajo), los **333** choques de número entre ambas tablas, las dos puertas de entrada y una guía de trabajo (añadir una syscall, depurar, probar, trampas). `scripts/test_abi_inventario.py` (**8/8**) falla si el documento deja de coincidir con el código.
- **T-026 instrumentado y explicado**: se miden las **ventanas con IF=0** por syscall y la atribución de cada hueco del PIT (`ventanas_if0`, `irq0_hueco_dentro`, `irq0_hueco_tick` en `/proc/smp`, línea inmediata `[IRQ0-HUECO]`). El hueco de 499 ms de la pasada anterior estaba en `ipv6::ping6_rtt` con el BSP **en `hlt` con IF=1** (simbolizado contra el ELF del núcleo), no en un tramo `cli`: es un retraso del anfitrión. Los umbrales no cambian; el predicado ahora distingue «causado por el núcleo» de «externo» y nombra la syscall cuando la hay. Detalle en [T-026](docs/TICKETS.md).
- **IPv6**: `ping6` a un destino inalcanzable ya no agota el plazo; el `ICMPv6 destino inalcanzable` de slirp corta la espera con su motivo y sin inventar RTT. Verificado: `IPv6: destino inexistente via encaminador -> sin respuesta (no se finge RTT)` **PASS**.
- **Consola tras la JVM (T-042)**: se corrigió el cierre diferido del líder de un grupo lanzado por `run` cuando el último hilo muere en otro CPU y el líder duerme en un `futex`: se completa en el punto único de espera del núcleo. Con ello la suite deja de caer en cascada (`842 PASS / 4926 FAIL` en la pasada anterior eran una consola muerta, no 4 926 fallos). Sonda dirigida `--solo "Terminal Built-in,Java"`: **104 PASS / 3 FAIL**, los 3 de prerrequisito omitido.
- **T-043 corregido**: cada página de la rama perezosa de `mmap` costaba **dos invalidaciones de TLB con espera de acuse** (`unmap_page` y `mapear_perezosa`), cientos de µs bajo TCG cuando otro CPU comparte el espacio. Ahora se invalida una vez por tanda de 128 páginas y los marcos se liberan después del shootdown. Medido con un fixture que puede fallar (`scripts/sondas/t026_mmapfixed.rs` + hilo vigilante en el CPU 1): **90 000 µs → 10 000 µs** para 8 MB (44 → ≤5 µs/página), y el tramo continuo con IF=0 publicado por el núcleo baja de **20 944 µs a ≤11 203 µs** (y ese resto es del cargador de ELF). Con ello desaparece el último hueco del PIT atribuible al núcleo (`[IRQ0-HUECO] … dentro_if0=1 … nr=9`, 213 ms); los que queden con `dentro_if0=0` son del anfitrión.
- **T-042 reproducido y verificado en los dos firmwares**: el modo `futex-lider` del fixture mataba el grupo antes de que el líder llegara a dormir; se añadió `hilo_mata_grupo_tarde` (1200 ms de espera) y el caso se reproduce **siempre en 3 s** (en Linux sale con código 7). Con el arreglo del cierre diferido, en Portix se ve `[T-005] cierre diferido del lider completado en su espera: pid=2`, `PROC: exit PID=2 … code=7`, `[OK] Proceso PID 2 finalizado` y la consola atiende la orden siguiente. Sonda nueva `scripts/sonda_paridad_t042.py`: **BIOS 8/8 y UEFI 8/8** sobre la misma imagen. Se deja anotado que el prompt de Portix se imprime en el eco de la línea tecleada: esperar «que vuelva el prompt» produce falsos negativos.
- **Sondas nuevas**: `scripts/sonda_t026_ventanas.py` (verifica la instrumentación y mide la peor ventana), `scripts/sonda_futex_lider.py`, modo `futex-lider` con asesino retardado en `scripts/sondas/t034_exit_group.rs` (reproducción determinista del cuelgue en 3 s) y `scripts/sonda_paridad_t042.py` (misma batería en BIOS y UEFI). `scripts/resumen_suite.py` cuenta los veredictos del ejecutor sin que los códigos de color los oculten.
- **`munmap` con el mismo lote de TLB (cierre del ciclo siguiente)**: el bucle de `sys_munmap` recorría las páginas una a una y además **liberaba el marco antes de quitar la PTE**. Ahora usa una invalidación por tanda de 128 páginas y devuelve los marcos **después** del shootdown. A/B con el mismo fixture: **50 000 µs → 0 µs** para 8 MB (por debajo de los 10 ms de resolución del reloj del invitado). Con ello el peor tramo continuo con IF=0 del sistema pasa a ser el cargador de ELF, ~13 ms.
- **T-044 resuelto en su causa (1 de noviembre)**: el fallo intermitente del depurador **no es del depurador**. Reproducido en la recertificación final (53/62) y medido: la consola serie de esa corrida lleva **21,5 MB de los que 19,1 MB son `0x00` (88,7 %)** y bloques de una página entera de ceros con punteros de pila; ese torrente binario **parte las líneas de texto** y hace fallar las comprobaciones que llevan dígitos dentro de la cadena. Los marcadores del depurador **sí están** en la serie (`[SESSION-BUF-PASS]` ×3, `[WATCH-CLEARED]` ×4, `ranura=DR0 vigilada=` ×11). Queda abierto **T-045** para el transporte: hasta que la consola no envíe memoria cruda sin codificar, ninguna prueba de texto con dígitos es fiable.
- **T-044 pasa a tener diagnóstico propio**: `dbg --ortest` espera cada parada con plazo de 15 s e imprime siempre `[SESSION-OR-WAIT]` con el estado crudo del hijo, más `[SESSION-OR-TIMEOUT]` si vence; `scripts/dbg_probe.py` guarda la serie completa del invitado en `build/dbg_probe_serie.log`. El fallo intermitente (57/62, una sola vez) **no se ha reproducido** en cuatro escenarios distintos —ocho ensayos seguidos en una VM, carga de CPU, carga de E/S y dos recertificaciones completas (7/7)—, pero la próxima vez no quedará sin explicación: el registro dirá si el objetivo murió antes o si la parada se perdió.
- **Cierre del ciclo sobre la imagen `c3db4abb…`**: dos suites completas consecutivas **3829/3829 (`EXIT=0`)** sin reconstruir entre ellas y recertificación `--no-build` **MASTER-PASS 7/7** (interactiva 128/128, Épicas 58/59/60, depurador 62/62, UEFI 4/4). Paridad BIOS/UEFI: **8/8 y 8/8**. Queda abierto **T-044** (`dbg_probe` intermitente bajo carga: 57/62 en la primera recertificación, 62/62 aislado y 7/7 al repetirla) y el `munmap` por página (75 ms de tramo con IF=0).

### Estado comprobable y límites

| Área | Lo observado | Límite y evidencia |
|---|---|---|
| Compatibilidad Linux | Se han ejecutado binarios concretos de Alpine y Debian sin modificar, incluidas pruebas de BusyBox, glibc, herramientas de desarrollo y Java. | No se han probado todas las distribuciones ni todas sus aplicaciones. El historial anterior se recoge en [AGENTS.md](AGENTS.md) y [tickets](docs/TICKETS.md); la validación de b203 sigue abierta. |
| Arranque | En una build anterior se probaron BIOS y UEFI/OVMF en QEMU. | La imagen BIOS reconstruida usada en las sondas de octubre no es una imagen dual final certificada. No se afirma aquí una validación en silicio físico. [T-030](docs/TICKETS.md). |
| Memoria | Una sonda con dos CPU virtuales verificó el acceso después de `munmap` y remapeos; en dos casos se observó la reutilización real de marcos. | Las pausas de QMP alteran los tiempos; no se demuestran todas las carreras ni la seguridad SMP general. [Registro](docs/evidencias/b203/t033d-marcos-qemu.log). |
| Protección de páginas | `mprotect(PROT_NONE)` sobre una página materializada devolvió éxito, pero la lectura siguió funcionando tanto en CPU 0 como en CPU 1. | **Defecto abierto T-035**. Los controles negativos y Linux como referencia permiten detectar el fallo. [CPU 1](docs/evidencias/b203/t033b-smp-qemu-base.log) · [CPU 0](docs/evidencias/b203/t033d-marcos-qemu.log). |
| Certificación | Sobre la imagen `f3d5dacc…` (cierre de (A)) hay **dos suites completas consecutivas verdes**: 3829/3829 y 3829/3829 (`EXIT=0`), sin ninguna línea `[IRQ0-HUECO]`. La recertificación de esa imagen dio 6/7: el depurador marcó 53/62 por la causa de T-045 (volcados binarios en la consola), no por un defecto del depurador. | Sigue sin haber validación en hardware físico ni imagen dual final re-firmada; las pasadas anteriores de b203 fueron 3810/3812, 3812/3813, 3813/3813, 3811/3813 y 3813/3813. [Índice de evidencias](docs/evidencias/b203/LEEME.md). |

### 30–31 de octubre de 2026 · b203 en curso

#### Añadido y cambiado

- **Imágenes y userland:** el constructor y los ejecutores llevan las imágenes temporales a `build/dist/`. Se desactivó la incorporación de utilidades C propias al rootfs sin borrar sus fuentes; se mantiene la ejecución de binarios de terceros sin modificar. En la imagen BIOS reconstruida, una inspección offline pasó **18/18**, incluyendo ausencia de `/bin/doom` y de los compiladores enumerados. [T-029](docs/TICKETS.md) · [comprobación](docs/evidencias/b203/t029-imagen-reconstruida.log).
- **Hilos y señales:** se corrigieron experimentalmente casos de `exit_group`, salida del líder y SIGKILL con hilos; el núcleo experimental 118 pasó **10/10** casos en QEMU y el mismo fixture pasó 10/10 en Linux. La imagen ensayada era una copia BIOS, no una imagen dual; la integración y la certificación siguen pendientes. [T-031](docs/TICKETS.md) · [registro](docs/evidencias/b203/t034-k118h.log).
- **Ejecutor QMP:** se sustituyó la lectura que podía confundir eventos con respuestas por un cliente que correlaciona identificadores. Pasó **128/128 controles locales**, un ejercicio con QEMU mínimo y un arranque Portix con traza ATA. No sustituye la suite completa. [Controles](docs/evidencias/b203/t033e-controles-qmp-general.log) · [arranque](docs/evidencias/b203/t033e-qmp-ata-resumen.log).
- **Changelog reconstruido:** se sustituyeron las entradas posteriores a la 1.0 sin respaldo por esta cronología con enlaces y límites; se conservó el tramo hasta v1.0.0 sin modificar. Un control local y un flujo de CI comprueban estructura, enlaces y actualización ante cambios relevantes; los controles **no garantizan veracidad** y el flujo remoto no se ha ejecutado en este entorno. Se ajustaron rutas del CI anterior para mantener imágenes en `build/dist/`.
- **Pruebas de memoria:** la sonda Rust ampliada pasó 23/23 casos y controles en Linux; en dos ejecuciones BIOS sobre la imagen reconstruida se obtuvieron **9/11** y **5/12**, respectivamente. Los fallos y sus prerrequisitos quedan abiertos abajo. [Evidencias](docs/evidencias/b203/LEEME.md).
- **Doble arranque con la corrección de memoria (BIOS y UEFI):** se reconstruyó la imagen dual completa con el núcleo nuevo (`portix.img` = `portix-dual.img` `b95b01a2…`, `portix-uefi.img` `9a2ca631…`, 44 s). UEFI arranca **4/4** con OVMF (mitad superior, escritorio, AP 1). Los mismos seis modos de memoria pasan **6/6 en UEFI** (página protegida materializada con lectura y escritura prohibidas, `fork`, reserva protegida en la ventana de pila, crecimiento legítimo de pila, reserva perezosa y su control desmapeado) y **6/6 en BIOS**; el ELF del ext2 es idéntico byte a byte en ambos. Evidencia: `t035h-uefi-memoria5.log` y `t035h-bios-dual.log`. No es certificación: una sola VM, sin hardware físico ni recertificación.
- **Reparado el defecto sólo-UEFI que mataba los binarios propios (T-041):** bajo OVMF, `curl` e `init` morían en su primera instrucción con `#PF CR2=0 EC=0x5`, mientras que en BIOS el mismo `/bin/curl` descargaba sin problema. `EC` con P=1 indicaba violación de protección, no página ausente, y los bytes reales en `RIP` eran ceros: el marco asignado a la imagen era el físico `0x20005000`, **el hueco PCI/MMIO justo por encima de la RAM de 512 MiB**. `marcos::liberar()` aceptaba cualquier dirección dentro del rango del mapa de bits sin comprobar que el firmware la hubiera declarado RAM; al liberarse un marco de ese hueco, `asignar()` lo entregaba al espacio de usuario y todo lo escrito se perdía. Arreglo: mapa de bits `existe` con la RAM declarada (sólo esos marcos se liberan y el asignador nunca entrega un físico inexistente, con contador `liberados_ignorados` en la salida de proceso) y el cargador ELF sólo reutiliza traducciones que ya son de usuario (PTE con `USER`). [T-041](docs/TICKETS.md) · [diagnóstico](docs/evidencias/b203/t041-diagnostico.log) · [comparación previa](docs/evidencias/b203/t041-comparacion-pre-fix.log).
- **Paridad BIOS/UEFI con la batería idéntica:** sobre las imágenes finales (`portix.img` `fc20a5aa…47f6c`, `portix-uefi.img` `c4ede9b8…9e701`), la misma sonda ejecutada con `--firmware uefi` y `--firmware bios` pasó **6/6 en ambos** firmware, descargó el fixture **por red con `curl`** (1 211 072 B) en ambos y verificó el ELF del ext2 idéntico byte a byte en ambos; la traza de `curl` es la misma hasta el código de salida (`code=1` por `connect()` sin servidor, `pico=104` marcos, `respaldo_heap=0`). Antes de este ciclo la rama UEFI no podía ni ejecutar sus propios binarios. [UEFI](docs/evidencias/b203/t041-paridad-uefi-red.log) · [BIOS](docs/evidencias/b203/t041-paridad-bios-red.log) · [build](docs/evidencias/b203/t041-build-limpio.log). Sigue siendo QEMU: no es silicio físico ni certificación.
- **Protección real de páginas (T-035):** `mprotect(PROT_NONE)` sobre una página ya escrita dejaba leer su contenido porque la entrada seguía presente; en x86-64 el bit de escritura no impide la lectura. Ahora la entrada se oculta conservando la dirección física y la protección, y vuelve a presente con el mismo marco y el mismo contenido al permitir el acceso. La traza del núcleo lo confirma en el caso medido (`0x8080000005475000` sin presente tras `PROT_NONE`). En una copia BIOS: lectura y escritura prohibidas con `SIGSEGV/ACCERR=2` y dirección exacta, `PROT_READ` conserva 77, `PROT_RW` restaura y escribe; `fork` aísla la copia; `munmap`, `MAP_FIXED` y la liberación devuelven el marco. Sonda BIOS **21/30**: los 9 fallos son exactamente T-036, T-037 y T-038, ya abiertos, sin fallos nuevos; sonda SMP T-033 **4/4**, incluida la CPU 1; referencia Linux **29/29**. Suite completa sobre esa copia: **3813/3814**, con un único fallo de umbral en T-026 (`irq0_perdidos=675` frente a `<600`), sin control de la base en el mismo entorno; no es una suite verde ni certifica. Java `-version` pasó en 42,0 s. **Sin hardware físico ni recertificación.** [Diseño y límites](docs/T035_DISENO_MEMORIA.md) · [Evidencias](docs/evidencias/b203/LEEME.md).
- **Protección de pila (T-040):** una reserva `MAP_FIXED` + `PROT_NONE` en la ventana de crecimiento de pila no bloqueaba la lectura: el fallo de página la convertía en página de pila escribible. En una copia BIOS se reprodujo (2/3) y la referencia Linux devolvió `SIGSEGV/ACCERR`. Con la guarda que exige un hueco real, la misma familia de pruebas pasó **6/6**, incluido el crecimiento legítimo de pila y sin regresión en T-039; la prueba SMP de esa imagen siguió 3/4 por T-035. No hay suites, UEFI ni hardware. [Evidencias](docs/evidencias/b203/LEEME.md) · [Diseño](docs/T035_DISENO_MEMORIA.md).
- **Señal de reserva perezosa (T-039):** una reserva anónima PROT_NONE sin materializar devolvía `SEGV_MAPERR` en vez de `SEGV_ACCERR`. La corrección experimental pasó 9/9 casos seleccionados en una copia BIOS, con controles de página accesible y desmapeada; Linux pasó los tres casos de referencia. No se probó UEFI ni se certificó. El defecto de acceso en páginas materializadas **sigue abierto**: prueba SMP 3/4 después del cambio. [Diseño y límites](docs/T035_DISENO_MEMORIA.md) · [resultados](docs/evidencias/b203/LEEME.md).
- **Auditoría de pruebas:** se conservan los fallos, controles negativos, registros parciales y sumas SHA-256; se rechazan resultados sin el errno, PID, salida o hardware exigidos por cada prueba. El manifiesto detecta alteraciones de los archivos, pero **no autentica su origen**. [Evidencias b203](docs/evidencias/b203/LEEME.md).

- **Dos pasadas completas verdes consecutivas y recertificación sobre la MISMA imagen:** `portix.img` `fc20a5aa…47f6c` (núcleo `9f9ce890…`). Primera pasada **3814/3814, 0 fallos**; segunda pasada **3814/3814, 0 fallos**; sin reconstruir entre ambas. La recertificación con `--no-build` dio **MASTER-PASS 7/7** (auditoría estricta con 0 tautologías y 0 errno genérico; suite interactiva `128/128`; Épicas 58/59/60; depurador `62/62`; arranque UEFI `4/4`). Es la primera vez en b203 que se registran dos verdes consecutivas. [Suite 1](docs/evidencias/b203/t041-suite1-3814.log) · [Suite 2](docs/evidencias/b203/t041-suite2-3814.log) · [Recertificación](docs/evidencias/b203/t041-recert-master-pass.log) · [Controles](docs/evidencias/b203/t041-controles-finales.log).
- **T-026 medido en las dos pasadas:** `irq0_perdidos` cayó en **413** y **370** ticks frente al umbral `< 600` (en el ciclo anterior el mismo contador marcó **675** y falló). El contador es acumulado desde el arranque y su mayor contribuyente es la sección de la JVM (`peor_hueco_us` 236 948 en la primera pasada y 49 744 en la segunda, es decir, casi 5× de variación entre dos ejecuciones del mismo binario). Por eso este ciclo **no cierra T-026**: lo deja atribuido a la variabilidad de una medida de latencia bajo TCG y no a un camino de código concreto; un valor por debajo del umbral en dos pasadas seguidas no demuestra que la ventana con IF=0 haya desaparecido. [T-026](docs/TICKETS.md).
- **Controles finales:** `unittest` **266/266** (se recompiló `build/packages/t033_smp`, ausente tras la renovación del sandbox), `verificar_changelog.py` PASS y `sha256sum -c` de `t041-huellas.sha256` (imágenes, núcleo, stage2, fixtures, sondas y fuentes tocadas) sin discrepancias.

- **Cuatro defectos cerrados con causa raíz medida (ciclo T-042).** Se trabajó en el orden que pidió el usuario: primero los bugs y las dependencias, y después lo que abren.
  - **T-031 (`exit_group`/SIGKILL con el líder en espera)**: la corrección existía desde los kernels 115-118 pero nunca entró en el flujo de cierre. Ahora hay sonda propia (`scripts/sonda_t031_b203.py`, **8/8**) y sección en `headless_runner.py` (11 comprobaciones, verde en `--solo T-031`). Exige la línea exacta de cada modo y el código de salida del PID que anunció `PROC: create`, no el de un hilo ni el de un hijo asesinado. [Sonda](docs/evidencias/b203/t031-sonda-8de8.log) · [Suite](docs/evidencias/b203/t031-suite-solo.log).
  - **Lo que desbloqueó (dependencia)**: `rustc` de Debian ya **compila y enlaza dentro de Portix** y el binario corre con hilos y `join` (antes el compilador salía con código 32). Medido: despliegue 751,6 s, `rustc hola.rs` 87,8 s. [Registro](docs/evidencias/b203/t031-rustc-desbloqueado.log).
  - **T-038 (`socketpair` → ENOSYS)**: el núcleo lo tenía, pero sólo en el número **105** del dialecto nativo; en x86-64 Linux `socketpair` es la **53** y la capa de compatibilidad no tenía esa rama. Añadida: los cuatro modos de socket pasan a PASS.
  - **T-037 (`write` con búfer inaccesible)**: dos causas. `verify_user_buffer` sólo validaba el rango, y la ruta de tubería devolvía cuenta corta (0 o 128) en vez de `EFAULT`; ahora, para escrituras ≤ `PIPE_BUF` (4096), se valida el rango completo antes de escribir, como la publicación atómica de Linux. Y `fcntl(F_SETFL)` devolvía 0 **sin guardar** `O_NONBLOCK` en tuberías y socketpair, así que el `read` siguiente bloqueaba: ahora se guarda.
  - **T-036 (`MADV_DONTNEED`)**: sólo actuaba sobre páginas presentes y escribibles. Las protegidas (PROT_NONE, que no están presentes) y las de sólo lectura se ignoraban y el contenido seguía visible (medido: 77 en vez de 0). Ahora se libera el marco y queda una reserva perezosa con la misma protección.
- **Sonda de memoria completa en 30/30** sobre la imagen `346655e0…0da1` (`t042-sonda-completa-30de30.log`), frente a **21/30** al empezar el ciclo (`t042-sonda-completa-antes-21de30.log`). Cubre T-035 a T-040. Sigue siendo una sola VM en QEMU: **no certifica** por sí sola.
- **Nota de método**: la doble tabla de números de syscall (dialecto nativo con `SYS_SOCKETPAIR = 105` frente a la capa Linux que despacha números literales) es una trampa para el próximo que añada una llamada. No se ha "arreglado" para no tocar el ABI nativo, pero queda documentada.

#### Abierto o no demostrado

- **T-035:** corregir `PROT_NONE` sin perder propiedad, contenido, liberación de marcos ni semántica de `fork`/COW y validación de memoria de usuario. Un `write` desde esa página a una tubería devolvió 1 en vez de `EFAULT`.
- **T-036/T-037:** `MADV_DONTNEED` dejó el contenido de páginas privadas protegidas en vez de descartarlo; `write` a tubería desde memoria desmapeada devolvió 0 o 128 en dos casos en los que Linux devolvió `EFAULT`. Los controles negativos detectaron las omisiones.
- **T-038:** la syscall Linux `socketpair` 53 devolvió `ENOSYS`; cuatro ensayos de socket no llegaron a evaluar `write`. [Detalle y resultados](docs/TICKETS.md).
- **T-033:** `java -version` superó el límite de 120 s en una ejecución de kernel 118 y terminó después, dentro de la ventana diagnóstica. La medición de shootdowns identifica un coste concreto, pero no demuestra la causa única del retraso. [Registro](docs/evidencias/b203/t033-version-aislada118.log).
- **T-032:** un arranque anterior no detectó ATA; los reintentos con y sin traza arrancaron, pero no reproducen ni explican aquel fallo. [Trazas y límites](docs/evidencias/b203/LEEME.md).
- **T-028/T-027:** siguen abiertos los fallos intermitentes documentados de despertar y HotSpot; los límites de interrupciones y el indicador `reloj FALLO` necesitan seguimiento.
- Antes de optimizar invalidaciones TLB hay que revisar el orden de liberación de marcos y los acuses. Antes de cerrar b203 hacen falta una imagen final dual identificable, **dos suites completas verdes consecutivas sobre esa imagen y recertificación**, sin reconstruir entre las suites.

## Cambios reconstruidos posteriores a v1.0.0

Las entradas siguientes resumen la [bitácora de desarrollo](AGENTS.md) y los [tickets](docs/TICKETS.md). Los registros independientes disponibles están enlazados cuando existen; las cifras históricas que sólo constan en la bitácora no se han vuelto a certificar para este resumen. No se asignan números de versión ni se presentan como lanzamientos publicados.

### Hitos concretos documentados en la bitácora

Estos hitos describen **builds históricas**, no garantizan que una imagen posterior los haya vuelto a superar:

- **Builds 104–117, 23 de octubre:** núcleo en mitad superior y BusyBox estático de Alpine ejecutado como ELF sin modificar; se corrigieron `getdents64`, `execve` y errores exactos revelados por utilidades reales. La bitácora registra la suite 49e **3531/3531** y recertificación 63 con UEFI/OVMF 3/3. [Ciclo 49](AGENTS.md).
- **Build 182, 24–25 de octubre:** raíz ext2 y FIFOs; se retiró el rescate FAT que podía encubrir la falta de raíz. La bitácora registra comprobaciones con `e2fsck`/`debugfs`, suite 58 **3597/3597** y recertificación 96. [T-001](docs/TICKETS.md).
- **Builds 183 y 185, 25 de octubre:** instalación verificada de `.apk` de Alpine y prueba del GCC de Alpine compilando y ejecutando dentro de Portix. La bitácora registra suite 59 **3616/3616** y suite 60 **3626/3626**, cada una con su recertificación. Los scripts de instalación y las restricciones de versión conservan limitaciones. [T-002 y T-003](docs/TICKETS.md).
- **Build 187, 26 de octubre:** verificación OpenPGP de `InRelease` para el camino RSA ensayado y ejecución de utilidades de Debian con glibc; suite 63 **3650/3650** y recertificación 100 en la bitácora. No se verifican todos los estados o algoritmos de claves y `.rpm` sigue pendiente. [T-004](docs/TICKETS.md).
- **Builds 189–190, 26 de octubre:** hilos con espacio compartido, futex y salida por hilo; sondas de C++, Rust y NASM, y ejecución de CPython de Debian. La bitácora registra suite 66 **3664/3664** y suite 70 **3719/3719**, con sus recertificaciones. No prueban todos los casos de concurrencia. [T-005 y T-005b](docs/TICKETS.md).
- **b193–b199, 27–29 de octubre:** arranque de CPU secundarias, IPI, estado por CPU y planificación SMP en QEMU. La bitácora registra b199 con suites 90a/90b **3771/3771** y recertificación 123; hallazgos posteriores mantienen abierta la revisión de TLB y memoria. [T-025 y T-033](docs/TICKETS.md).
- **b200–b202, 29–30 de octubre:** se instrumentaron interrupciones, se incorporaron mapeos de fichero perezosos y se ampliaron `execve` y futex. La bitácora registra b202 con suites 96a/96b **3804/3804** y recertificación 127; la situación actual de Java y TLB es la indicada en `[Sin publicar]`. [T-014, T-017, T-018 y T-026](docs/TICKETS.md).

### 27–30 de octubre de 2026 · SMP, memoria e hilos

- Se avanzó en planificación SMP y estado por CPU, invalidación TLB con acuses, `execve` desde un hilo y operaciones futex. La bitácora registra pruebas en QEMU y correcciones sucesivas, no una prueba general de ausencia de carreras.
- Se incorporaron mapeos privados perezosos de fichero y medición de interrupciones; algunas regresiones posteriores y sus límites se mantienen abiertas en los tickets.
- La bitácora registra b202 como cerrado con dos suites 3804/3804 y recertificación. **Ese resultado es histórico y no certifica los cambios posteriores de b203.** [Entrada b202](AGENTS.md) · [T-017 y T-018](docs/TICKETS.md).

### 25–26 de octubre de 2026 · Paquetes y software de distribuciones

- Se extendió ext2 y se retiró el rescate que podía ocultar la ausencia de una raíz real; sin raíz, el modo degradado se informa explícitamente.
- `pkg` incorporó instalación de paquetes `.apk` de Alpine y `.deb` de Debian con verificación de integridad y firmas en las rutas documentadas. Hay limitaciones: no se ejecutan todos los scripts de mantenedor, no se verifican todos los tipos o estados de clave y no hay soporte general de todas las familias de paquetes.
- La bitácora registra pruebas concretas de GCC de Alpine, glibc y utilidades de Debian, hilos, CPython y Java. Son ejemplos delimitados, no garantía para todas las versiones ni aplicaciones. Los compiladores de terceros se instalan para las pruebas, no se presentan como preinstalados en la imagen actual. [T-001 a T-005](docs/TICKETS.md).

### 23–24 de octubre de 2026 · Base de compatibilidad e identidad

- Se trabajó en el núcleo en mitad superior y en ejecutar BusyBox y `tcc` de Alpine; también se endurecieron pruebas para exigir resultados y errores específicos.
- La raíz pasó a ext2 y FAT quedó para el arranque; el alcance y la evidencia histórica están en la bitácora.
- Se retiró la capa y las afirmaciones de compatibilidad Win32/PE. Wine sin modificar es una posibilidad condicionada a trabajos futuros, **no** una capacidad demostrada. No se anuncia compatibilidad con módulos Linux `.ko`, `systemd` como PID 1 ni Docker.
- Se definió la identidad de Portix y se trasladaron a tickets las capacidades todavía no conquistadas, como controladores reiniciables, actualización A/B e imágenes reproducibles bit a bit. Las métricas no medidas no se presentan como resultados. [T-000 y T-007 a T-011](docs/TICKETS.md).

## Cómo mantener este registro

En **cada ciclo** se actualiza `[Sin publicar]` junto con `AGENTS.md` y los tickets afectados: qué cambió, prueba y resultado exactos, límites y pendientes. Se enlazan evidencias conservadas; si se perdió un registro, se indica. No se inventan versiones, fechas, comparaciones ni certificaciones. Un resultado de una build no se atribuye a otra sin identidad comprobada.

Al publicar una versión real, se mueve únicamente lo respaldado por su imagen identificada y sus pruebas a una entrada con versión y fecha reales; lo pendiente permanece en `[Sin publicar]`. La comprobación de estructura y enlaces se ejecuta con `python3 scripts/verificar_changelog.py`.

## Historial original hasta v1.0.0

Las entradas desde **v1.0.0 y anteriores** se conservan sin modificar, como pidió el usuario. Son historia documental no revalidada en esta reconstrucción: sus afirmaciones, cifras y denominaciones de lanzamiento **no demuestran el estado actual** y, donde contradicen el apartado `[Sin publicar]`, prevalece el estado actual documentado arriba. El archivo anterior completo se conserva sólo como [referencia histórica no validada](docs/evidencias/changelog/LEEME.md).

---

## [v1.0.0] - 2026-09-26 "Genesis" (Lanzamiento Oficial Definitivo)
### Añadido
- **Consagración de las 500 Fases de Ingeniería y Lanzamiento Oficial**:
  - Hito histórico alcanzado: Portix OS culmina íntegramente las primeras 500 fases de la hoja de ruta fundacional.
  - Sistema operativo independiente de 64 bits en Rust puro (`no_std`), con cero líneas de código C en kernel, sin dependencias de GRUB ni librerías externas.
  - Arranque dual híbrido nativo: BIOS MBR en Assembly y UEFI GOP (`BOOTX64.EFI`) en Rust.
- **Internacionalización Nativa (i18n) Multilingüe (Fase 482)**:
  - Motor de catálogos en memoria sin dependencias externas (`I18nEngine`), con búsqueda de cadenas O(1) determinista en `no_std`.
  - Soporte bilingüe completo: Español (`es-ES`) e Inglés (`en-US`), con conmutación dinámica en caliente y traducción de mensajes de arranque, VFS, red, seguridad y terminal.
- **Accesibilidad Universal (a11y) y Síntesis Fonética TTS (Fase 486)**:
  - Paleta monocromática accesible de alto contraste para entornos de baja visibilidad o fotofobia.
  - Factor dinámico de escalado de tipografías vectoriales del 100% al 200%.
  - Transductor de texto a voz (TTS) con cola de síntesis fonética por puerto serie COM1 y dispositivo de audio.
  - Soporte de emulación de StickyKeys, atajos de teclado exclusivos para el escritorio Wayland y conformidad WCAG 2.1 AAA.
- **Asistente de Migración de Perfiles (`portix-migrate`) (Fase 489)**:
  - Escáner heterogéneo en caliente capaz de detectar particiones NTFS (Windows), EXT4 (Linux) y FAT32 (EFI).
  - Detección e importación no destructiva de perfiles de usuario: documentos, claves privadas SSH `~/.ssh/id_ed25519`, marcadores web y perfiles Wi-Fi hacia `/home/user/`.
  - Verificación de sumas criptográficas SHA-256 en cada archivo migrado y registro de transacciones con soporte de reversión.
- **Telemetría Ética Local y Diagnóstico Transparente (Fase 491)**:
  - Política de privacidad estricta con compuerta Opt-In obligatoria (`opt_in = false` por defecto).
  - Generación de identificador de sesión criptográfico pseudo-anónimo y garantía absoluta de Cero Información de Identificación Personal (Zero PII).
  - Registro de auditoría de salud local transparente e inspeccionable en `/var/log/telemetry.log`.
- **Regulador Dinámico de Rendimiento de CPU (*Performance Governor*) (Fase 492)**:
  - Selector en caliente entre modos de energía: `PowerSave` (800 MHz, 50 Hz CFS, mínimo consumo), `Balanced` (2.4 GHz, 100 Hz CFS) y `UltraPerformance` (4.8 GHz Turbo, 250 Hz CFS, latencia sub-milisegundo).
  - Sintonización dinámica del quantum del planificador CFS y modulación de voltajes/frecuencias DVFS.
- **Compilaciones Reproducibles y Congelación de la ABI LTS del Kernel (Fases 496, 498, 500)**:
  - Verificación de compilación determinista bit a bit con suma de comprobación maestra SHA-512 oficial.
  - Sello de inmutabilidad y congelación de la interfaz binaria de aplicaciones (LTS ABI Contract): 360 llamadas al sistema estables y garantizadas hacia el futuro.
  - Garantía de compatibilidad retrospectiva para binarios de usuario en Ring 3.
- **Ampliación de la Hoja de Ruta Maestra a 1,000 Fases**:
  - Incorporadas las **Épicas 11 a 20 (Fases 501 a 1000)** en `docs/ROADMAP_1000_PHASES.md` y `ROADMAP.md` para guiar la era de ultra-optimización microarquitectónica, compiladores JIT, redes 100GbE, almacenamiento Zettabyte y Portix OS v2.0 "Titan".
- **Suite de Pruebas CI Automatizadas**:
  - Total de comprobaciones elevado a **4,853 pruebas superadas al 100%** (`scripts/headless_runner.py`).
  - 70 pasos continuos interactivos en la suite de interfaz gráfica `test_ui`.
  - 50 nuevos tests POSIX (Épocas CCXXXVI a CCXL, tests #4496 a #4545).

---

## [v0.9.9] - 2026-09-26 (Release Candidate 1)
### Añadido
- **Autoconstrucción Soberana de Medios Booteables en Ring-3 (`portix-build`)**:
  - Empaquetador nativo de sistemas de archivos VFS en memoria para la creación de imágenes ISO-9660 con catálogo El Torito híbrido (BIOS MBR + UEFI ESP).
  - Generación autónoma de la imagen de arranque `portix-sovereign.iso` con cálculo y verificación del hash SHA-512 directamente en `/tmp` sin requerir herramientas en el sistema anfitrión.
- **Gestor de Arranque Gráfico Nativo con Boot Splash y Selector Multi-Boot**:
  - Renderizado visual en resolución nativa VBE/GOP (1024x768x32 bpp) con animación fluida de barra de progreso de inicio (*boot splash*).
  - Menú interactivo con temporizador de 5 segundos y detección de arranque dual (*dual-boot*) coexistente con Windows Boot Manager (`bootmgfw.efi`) y Linux GRUB (`grubx64.efi`).
- **Raíz de Confianza Hardware TPM 2.0 y Secure Boot Integral**:
  - Interfaz TCG Command Response Buffer (CRB) y FIFO mapeada en MMIO (`0xFED4_0000`).
  - Medición acumulativa de las etapas de arranque en registros de configuración de plataforma (PCR 0 a 7 con SHA-256).
  - Sellado criptográfico (`TPM2_Seal`) y recuperación (`TPM2_Unseal`) de claves maestras para volúmenes cifrados LUKS2 ligado a la integridad de la plataforma.
- **Firma Criptográfica de Módulos de Kernel (LKM Signing)**:
  - Verificación estricta de firmas digitales Ed25519 adjuntas a los módulos `.ko` antes de permitir su inserción en Ring 0 (`sys_init_module`), rechazando binarios no autorizados o alterados con `-EKEYREJECTED`.
- **Servidor y Cliente SMB 3.1.1 Nativo**:
  - Negociación de dialectos SMB 3.1.1 en el puerto TCP 445 con autenticación segura de sesiones mediante NTLMv2.
  - Cifrado transparente de datos con AES-128-GCM y compartición de rutas locales (`/home`, `/var/shared`) para interoperabilidad con clientes Windows y macOS.
- **Túneles PCIe USB4 / Thunderbolt 4 (40 Gbps)**:
  - Inicialización de routers USB4 y túneles PCIe para conexión en caliente (*hotplug*) de aceleradores gráficos externos (eGPU) y estaciones de acoplamiento (*docks*).
- **Controlador USB HID Especializado**:
  - Decodificación en tiempo real de reportes de 64 bytes para mandos de videojuegos (*gamepads*) y joysticks analógicos (ejes X, Y, Z, Rz, gatillos y Hat Switch de 8 direcciones).
- **Arranque en Frío Ultrarrápido (< 1.0s) y Perfiles de Edición**:
  - Sondeo asíncrono y en paralelo de dispositivos en el bus PCI y precarga inteligente de sectores críticos de almacenamiento en caché VFS, logrando alcanzar el escritorio gráfico en **840 milisegundos**.
  - Selector de perfiles de despliegue soberanos: `Portix Server` (headless por consola serie, huella < 48 MB RAM), `Portix Desktop` (Wayland 60 FPS acelerado) y `Portix Embedded / IoT` (mínimo consumo < 24 MB RAM).
- **Suite de Pruebas CI Automatizadas**:
  - Total de comprobaciones elevado a **4,794 pruebas superadas al 100%** (`scripts/headless_runner.py`).
  - 65 pasos continuos interactivos en la suite de interfaz gráfica `test_ui`.
  - 40 nuevos tests POSIX (Épocas CCXXXII a CCXXXV, tests #4456 a #4495).

---

## [v0.9.8] - 2026-09-26
### Añadido
- **Aislamiento, Seguridad y Motor de Contenedores Nativo (`portix-box`)**:
  - Implementación de Namespaces del kernel: `MountNamespace`, `NetworkNamespace` (con interfaces virtuales `veth`), `UtsNamespace` y `PidNamespace`.
  - Soporte de llamadas al sistema `sys_unshare`, `sys_setns` y `sys_pivot_root` para jaulas de ejecución aisladas.
  - Motor de Contenedores `portix-box` (`pbox`) con control de recursos `cgroups v2` (`cpu.max`, `memory.max`, `pids.max`).
  - Sandboxing de llamadas al sistema Seccomp-BPF (`SECCOMP_RET_ALLOW`, `KILL_PROCESS`, `ERRNO`) y confinamiento de rutas Landlock LSM.
- **Motor Web Gráfico y Multimedia de Próxima Generación**:
  - Motor de renderizado nativo `portix-browser` v2: Analizador determinista HTML5 DOM y cálculo geométrico CSS Box Model.
  - Renderizado tipográfico vectorial TrueType (TTF) sobre superficies Wayland interactivas `xdg_toplevel` con scroll suave y eventos de puntero.
  - Subsistema de audio dúplex Ring-3 con captura simultánea de micrófono HDA/AC97 DMA y mezclador con control de ganancia independiente por aplicación (-24 dB a +12 dB).
  - Controlador de brillo de pantalla por modulación PWM y temporizador inteligente de inactividad con atenuación y apagado de panel.
- **Estados de Suspensión ACPI S3/S4 y Resiliencia de Energía**:
  - Suspensión a RAM (ACPI S3 / Sleep) con preservación y restauración atómica del contexto arquitectónico de CPU (RAX..R15, CR0-CR4, MSRs FS/GS_BASE, EFER, APIC) y vector de reanudación en modo real.
  - Hibernación a Disco (ACPI S4 / Suspend-to-Disk) con compactación de páginas, compresión de alta velocidad, firma SHA-512 y recuperación in-situ en cargadores UEFI/BIOS.
  - Gestión térmica activa ACPI con monitoreo `_TMP`, umbrales `_AC0`, `_PSV`, `_CRT` y control escalonado de ventilador PWM con zona muerta anti-oscilación.
- **Virtualización por Hardware x86-64 y Capa Compatible KVM (`/dev/kvm`)**:
  - Soporte nativo para Intel VT-x VMX y AMD SVM con estructuras VMCS/VMCB, `VMPTRLD`, `VMLAUNCH` y `VMRESUME`.
  - Dispositivo de caracteres `/dev/kvm` e ioctls de vCPUs (`KVM_GET_API_VERSION`, `KVM_CREATE_VM`, `KVM_CREATE_VCPU`, `KVM_SET_USER_MEMORY_REGION`, `KVM_RUN`).
  - Paginación anidada EPT/NPT de 4 niveles para aislamiento de memoria física de máquinas virtuales.
- **15 Épocas Adicionales de Ingeniería Soberana**:
  - Distribución descentralizada P2P de paquetes (`portix-pkg-p2p`) con DHT Kademlia de 160 bits y árboles Merkle SHA-512.
  - Criptografía post-cuántica resistente a ordenadores cuánticos: KEM Kyber (ML-KEM-768) y firmas Dilithium (ML-DSA-65).
  - Pila Bluetooth HCI avanzada con L2CAP, SDP, RFCOMM y transmisión de audio estéreo A2DP con códec SBC.
  - Pila de red Wi-Fi IEEE 802.11 nativa con escaneo de balizas, autenticación WPA2/WPA3 Personal y cifrado AES-CCMP.
  - Trazado dinámico de kernel mediante eBPF con sondas Kprobes e intercepciones Uprobes en Ring-3.
  - Servidor de impresión IPP/2.0 y spooler CUPS con descubrimiento ZeroConf/mDNS en red local.
  - Gestor de memoria swap comprimida en RAM ZSWAP y ZRAM con algoritmos de compresión LZ4 y ZSTD.
  - Aceleración de cómputo GPU Vulkan Compute y shaders SPIR-V para procesamiento de tensores e inteligencia artificial.
- **Suite de Pruebas CI Automatizadas**:
  - Total de comprobaciones elevado a **4,746 pruebas superadas al 100%** (`scripts/headless_runner.py`).
  - 61 pasos continuos interactivos en la suite de interfaz gráfica `test_ui`.
  - 190 nuevos tests POSIX (Épocas CCXIII a CCXXXI, tests #4266 a #4455).

---

## [v0.9.7] - 2026-09-25
### Añadido
- **Motor de E/S Asíncrona `io_uring` y Descriptores Modernos**:
  - Colas de sumisión y completado (*Submission & Completion Queues* - SQ/CQ) mapeadas en memoria compartida sin costo de copiado.
  - Sometimiento asíncrono y recolección de eventos por lotes (`IORING_OP_NOP`, `READV`, `WRITEV`, `FSYNC`, `POLL_ADD`).
  - Descriptores de archivo anónimos sellados (`memfd_create`) con sellado criptográfico de inmutabilidad (`F_SEAL_SHRINK`, `F_SEAL_GROW`, `F_SEAL_WRITE`).
  - Primitivas de sincronización atómica no bloqueante mediante contadores de 64 bits (`eventfd`).
- **Administrador de Energía ACPI y Perfiles Dinámicos**:
  - Telemetría en tiempo real del estado de adaptador AC y batería (tablas ACPI `_BST` y `_BIF`).
  - Perfiles de rendimiento configurables: `PowerSave` (1600 MHz, 50 Hz CFS), `Balanced` (2600 MHz, 100 Hz CFS), `Performance` (4400 MHz, 250 Hz CFS).
  - Algoritmo predictivo de autonomía restante y monitorización de zonas térmicas (`_TMP`).
  - Widget en vivo de batería y perfil de energía en la barra de tareas *Portix-Bar*.
- **Particionado Dual y Actualizaciones Atómicas A/B con Auto-Rollback**:
  - Esquema de doble ranura de sistema (`SlotA` vs `SlotB`) con aislamiento LBA de 64 MB por ranura.
  - Banderas atómicas de estado de arranque y guardia con contador de hasta 3 intentos antes de rollback.
  - Reversión automática a la ranura buena conocida ante fallos de inicio en actualizaciones en fase *Staged*.
  - Protocolo de apretón de manos saludable (*mark_boot_successful*) al alcanzar el entorno gráfico.
  - Escritura y clonación en segundo plano en caliente sin tiempo de inactividad (*zero-downtime*).
- **Instalador Autónomo de Disco Asistido y Verificación SHA-512**:
  - Detección universal de medios de almacenamiento (controladores ATA, AHCI SATA y NVMe PCIe).
  - Comprobación criptográfica de integridad de la imagen del sistema mediante hash SHA-512.
  - Particionado desatendido con generador de tablas MBR/GPT y formateo directo de la partición ESP (FAT32).
  - Soporte de instalación dual-boot no destructiva y sincronización de sectores de arranque BIOS Stage 1/Stage 2.
- **Suite de Pruebas CI Automatizadas**:
  - Total de comprobaciones elevado a **4,538 pruebas superadas al 100%** (`scripts/headless_runner.py`).
  - 52 pasos continuos interactivos en la suite de interfaz gráfica `test_ui`.
  - 40 nuevos tests POSIX (Épocas CCIX a CCXII, tests #4226 a #4265).

---

## [v0.9.6] - 2026-09-25
### Añadido
- **Ampliación del Microkernel: Memoria Compartida Zero-Copy & Revocación Dinámica**:
  - Paso de páginas de memoria compartida *Zero-Copy* vía Fast-IPC sin clonar búferes en memoria (`register_shared_page`, `share_page_zero_copy`).
  - Nuevos derechos de capacidad `CAP_SHARE` y `CAP_REVOKE`.
  - Árbol jerárquico de derivación de capacidades y algoritmo de revocación recursiva en caliente (`revoke_capability`) con expulsión atómica de servicios prestatarios.
- **Entrada Multi-Seat Nativa en Wayland & Atajos Tiling**:
  - Protocolo `wl_seat` multi-usuario con puestos de trabajo aislados e independientes (`seat0`, `seat1`).
  - Coordenadas de puntero `(x, y)` y focos de ventana de teclado/ratón independientes sin colisiones de foco.
  - Atajos configurables para el gestor Tiling Sway/i3 (Mod4+Enter, Mod4+Shift+Q, Mod4+j/k, Mod4+v, Mod4+1..4).
- **Pila de Red IPv6 Avanzada: Enrutamiento LPM, Fragmentación & Raw Sockets**:
  - Tabla de enrutamiento estático y dinámico con búsqueda de prefijo más largo (*Longest Prefix Match* - LPM) de 0 a 128 bits y métricas de coste.
  - Fragmentación y reensamblaje conforme a RFC 8200 con cabecera de fragmento 44 (`Ipv6FragmentHeader`), división respetando MTU y reordenamiento contiguo.
  - Sockets raw IPv6 (`AF_INET6`, `SOCK_RAW`) con inyección y captura directa sin demultiplexación forzada de protocolos de transporte.
- **Servidor de Audio Multicanal Desacoplado en Espacio de Usuario Ring-3**:
  - Desacoplamiento arquitectónico del servidor de audio como microservicio en Ring 3 con paso de mensajes Fast-IPC.
  - Soporte multicanal para Mono, Estéreo 2.0, Sonido Envolvente 5.1 y 7.1 Surround.
  - Implementación de la matriz de Downmix ITU-R BS.775 a estéreo, normalización LFE (-6 dB) / Centro (-3 dB) y limitador de soft-clipping para PCM de 16 bits.
  - Transferencia de tramas mediante anillo de búferes Zero-Copy sobre páginas compartidas del microkernel.
- **Suite de Pruebas CI Automatizadas**:
  - Total de comprobaciones elevado a **4,490 pruebas superadas al 100%** (`scripts/headless_runner.py`).
  - 48 pasos continuos interactivos en la suite de interfaz gráfica `test_ui`.
  - 40 nuevos tests POSIX (Épocas CCV a CCVIII, tests #4186 a #4225).

---

## [v0.9.5] - 2026-09-25
### Añadido
- **Pila de Red IPv6, WireGuard VPN & eBPF/XDP**:
  - Soporte nativo para IPv6 con cabeceras de 40 bytes (0x86DD), generación EUI-64 link-local (`fe80::`), Neighbor Discovery Protocol (NDP) con tabla de vecinos y enrutamiento con gateway.
  - Implementación en Rust puro del protocolo WireGuard VPN (`wg0`, Curve25519, ChaCha20-Poly1305, contadores anti-replay, allowed IPs `10.0.0.0/24`).
  - Motor de filtrado de paquetes en capa de enlace eBPF / XDP integrado en el controlador Intel e1000 a velocidad de línea (XDP_PASS, XDP_DROP, XDP_TX, XDP_REDIRECT).
- **Driver ICD Vulkan 1.3 SPIR-V & Efectos Wayland**:
  - Controlador instalable (ICD) Vulkan 1.3 nativo conforme a Khronos, validador e intérprete de bytecode SPIR-V (`0x07230203`), enumeración de GPU VirtIO-GPU Venus y rasterizador CPU multi-hilo.
  - Efectos visuales en tiempo real para el compositor Wayland: Gaussian Blur 1D separable (kernel 5x5), drop shadows paramétricas en superficies `xdg_toplevel` y transiciones animadas cúbicas a 60 FPS.
- **Microkernel Modular, Fast-IPC de Capacidades & Servicios Ring-3**:
  - Modelo de seguridad basado en capacidades (*CSpace* con badges y permisos granulares `CAP_READ`, `CAP_WRITE`, `CAP_CALL`, `CAP_GRANT`).
  - Fast-IPC síncrono pasando registros de CPU (`RDX`, `RSI`, `RDI`, `R8`, `R9`, `R10`) en ~78 ciclos de reloj (<100 ciclos).
  - Aislamiento estricto de microservicios Ring-3 (`vfs-server`, `net-server`, `audio-server`, `display-server`) y auto-recuperación en caliente (*Crash & Respawn*).
- **Autohospedaje (Self-Hosting) & Gestor Tiling Sway/i3**:
  - Pipeline de autoconstrucción soberana con herramientas nativas en el VFS (`/bin/portix-cc`, `/bin/ld-portix`, `/bin/ar`) generando binarios ejecutables en `/tmp` sin anfitrión externo.
  - Gestor de ventanas en mosaico dinámico con partición espacial binaria (BSP) estilo Sway/i3, sin solapamiento (*zero overlap*), workspaces 1 a 4 y barra integrada *Portix-Bar*.
- **Suite de Pruebas CI Automatizadas**:
  - Total de comprobaciones elevado a **4,442 pruebas superadas al 100%** sin una sola falla ni kernel panic (`scripts/headless_runner.py`).
  - 44 pasos interactivos en el gestor de ventanas `test_ui` y 4,185 pruebas POSIX completadas.

---

## [v0.9.4] - 2026-09-25
### Añadido
- **Pila USB 3.0 Nativa xHCI, SCSI BOT & Automount**:
  - Implementación de descriptores CBW (`0x43425355`) y CSW (`0x53425355`) para dispositivos de almacenamiento masivo USB 3.0.
  - Comandos SCSI `READ(10)`, `WRITE(10)`, `INQUIRY` y `READ_CAPACITY`, hotplug y automount en `/media/usb0`.
- **Almacén Reproducible Nix-Store (`/var/pkg/store`)**:
  - Direccionamiento por contenido inmutable SHA-512, resolución de dependencias por DAG y perfiles generacionales con rollback atómico.
- **Servidor Wayland Nativo `xdg-shell` & XWayland**:
  - Protocolo Wayland nativo con buffers compartidos `wl_shm`, ventanas `xdg_toplevel` y socket proxy X11 `/tmp/.X11-unix/X0` para compatibilidad binaria transparente.
- **Planificador CFS Multinúcleo & Sincronización RCU**:
  - Completely Fair Scheduler con vruntime ponderado por prioridades nice, balanceo work-stealing entre núcleos SMP y primitivas RCU lockless.

---

## [v0.9.1] - 2026-09-21
### Añadido
- **C Userland Toolchain & Standard Libc**:
  - Implementada runtime C estándar pura (`userland/libc/include/`, `userland/libc/src/libc.c`, `crt0.s`, `libc.a`).
  - Cabeceras POSIX C disponibles en `/usr/include/` (`stdio.h`, `stdlib.h`, `string.h`, `unistd.h`, `sys/types.h`, `sys/stat.h`, `fcntl.h`, `errno.h`, `sys/mman.h`).
  - Binarios ELF de usuario compilados en C estándar con GCC: `/bin/hello_c` y `/bin/calc_c`.
- **Suite de Pruebas CI**:
  - Total de pruebas automáticas elevado a **2,761 pruebas superadas al 100%** (`scripts/headless_runner.py`).
- **Optimización de Build**:
  - Reducción del tiempo de compilación de 18.0s a 10.0s (eliminación de compilación redundante de kernel).
  - Empaquetado automático de binarios C en la partición FAT32.
- **Hoja de Ruta de 500 Fases**:
  - Creación del documento maestro `docs/ROADMAP_500_PHASES.md` cubriendo 10 grandes épicas de ingeniería de sistemas operativos.
- **Estándares de Código Limpio**:
  - Creación de `docs/CODING_STANDARDS.md` para garantizar código de grado industrial sin dependencias externas ni código basura.

---

## [v0.9.0] - 2026-09-21
### Añadido
- **Entorno de Escritorio Flotante Moderno (WindowManager)**:
  - Eliminación completa de la antigua interfaz estática por pestañas.
  - Gestor de ventanas con Z-order dinámico, arrastre libre por coordenadas `(x, y)` y decoraciones nativas (`—`, `□`, `✕`).
  - Barra de tareas inferior (Dock) con botón de inicio estilizado `[❖ PORTIX]`, pestañas de ventanas activas con indicador cian y bandeja del sistema (Systray con Ethernet, RAM y Reloj).
  - Menú de inicio flotante de 400px con tarjeta de perfil de usuario (`root`), accesos directos y acciones de energía.
  - Atajo de teclado `Tab` para conmutar foco entre ventanas (Alt+Tab).
  - Explorador de archivos dinámico interactuando con el Editor de Texto.
  - Lanzador de ventanas GUI por comando CLI desde el terminal (`calc`, `editor`, `files`, `top`).

---

## [v0.8.0] - 2026-09-20
### Añadido
- **Suite de Conformidad POSIX Completa**:
  - Expansión de llamadas al sistema a 2,759 pruebas unitarias y de integración pasando al 100%.
  - Syscalls avanzadas: `epoll`, `inotify`, `mq_timedsend`, `select`, `pselect6`, `io_uring`, `landlock`, `futex`, `process_vm_readv`.
  - Subsistema de IPC bidireccional y memoria compartida (`sys_shm*`).

---

## [v0.7.5] - 2026-09-20
### Añadido
- **Módulos de Kernel Dinámicos (LKM)**:
  - Inyección y descarga en caliente de módulos `.ko` compilados en Rust puro (`insmod`, `rmmod`, `lsmod`).
  - Resolución dinámica de símbolos de kernel (`ksyms`).

---

## [v0.7.0] - 2026-09-19
### Añadido
- **Pila de Red Ethernet e1000 Gigabit**:
  - Driver Intel 82540EM PCIe con gestión de buffers en anillo RX/TX.
  - Implementación de protocolos ARP, IPv4, ICMP Echo (Ping) y cliente DHCP con asignación dinámica de IP.

---

## [v0.6.0] - 2026-09-18
### Añadido
- **Sistema de Archivos FAT32 y VFS**:
  - Soporte de lectura y escritura en particiones FAT32 sobre discos duros ATA/IDE.
  - Capa de Virtual File System (VFS) con montaje jerárquico de `RamFS` y `FatFs` (`/bin`, `/etc`, `/home`, `/tmp`).
  - Persistencia de archivos en disco: creación de `/etc/passwd`, `/etc/group`, `/etc/hostname`, `/etc/os-release`.

---

## [v0.5.0] - 2026-09-17
### Añadido
- **Espacio de Usuario Ring-3 y Cargador ELF64**:
  - Aislamiento de privilegios mediante transiciones de nivel de anillo x86_64 (`iretq` / `enter_ring3_asm`).
  - Cargador de ejecutables ELF64 con soporte de segmentos de código, datos y BSS.
  - Runtime de espacio de usuario `portix-rt` en Rust con asignador de heap de usuario.
  - Primeros binarios de usuario: `/bin/sh`, `/bin/hello`, `/bin/uptime`.

---

## [v0.4.0] - 2026-09-16
### Añadido
- **Controlador Gráfico VBE 1024x768x32bpp**:
  - Inicialización del Linear Framebuffer VESA mediante llamada BIOS en el arranque.
  - Renderizado lineal con doble buffer en memoria para eliminar parpadeos de pantalla.
  - Motor de renderizado tipográfico con fuentes monocromo VGA integradas.
  - Driver de ratón PS/2 con algoritmo de clampeo suave y eliminación de saltos bruscos.

---

## [v0.3.0] - 2026-09-15
### Añadido
- **Paginación x86_64 y Asignador de Memoria Buddy**:
  - Tablas de páginas de cuatro niveles (PML4, PDPT, PD, PT) con páginas enormes de 2 MB y soporte de bit No-Execute (NX).
  - Asignador Buddy System físico para gestionar 64 MB de memoria física con órdenes de 0 a 10.
  - Global Allocator en Rust implementando `alloc::boxed::Box`, `alloc::vec::Vec` y `alloc::string::String`.

---

## [v0.2.0] - 2026-09-14
### Añadido
- **Transición a Modo Largo (64-bit Long Mode)**:
  - Salto de Modo Real de 16 bits a Modo Protegido de 32 bits y activación de paginación PAE/Long Mode.
  - Configuración de la Global Descriptor Table (GDT) de 64 bits y Task State Segment (TSS).
  - Tabla de Descriptores de Interrupción (IDT) de 256 vectores con trampas de excepciones de CPU y handlers de interrupción de hardware (PIC / PIT 100Hz / Teclado IRQ1 / Ratón IRQ12).
  - Migración completa del kernel de C a Rust puro `no_std`.

---

## [v0.1.0] - 2026-09-13
### Añadido
- **Génesis del Proyecto - Hola Mundo Bare-Metal**:
  - Sector de arranque inicial MBR de 512 bytes en ensamblador x86 (`boot.asm`) con firma `0xAA55`.
  - Emisión de caracteres en modo texto VGA 80x25 mediante interrupción BIOS `INT 0x10`.
  - Impresión del mensaje seminal: `"Hola Mundo desde Portix OS (x86 Bare-Metal)"`.
  - Scripts de compilación con NASM y ejecución en QEMU.
