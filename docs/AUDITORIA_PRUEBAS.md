# Auditoría estricta de pruebas y errno (ciclo 50)

Regla del proyecto: **una prueba que no puede fallar no es una prueba** y **un
error genérico no demuestra nada**. Esta auditoría se disparó al descubrir en
el ciclo 49 que `getdents64` "pasaba" exigiendo sólo `n > 0`, y que varias
pruebas aceptaban cualquier `-1` como si fuera `ENOENT`/`EBADF`.

## 1. Herramienta: `scripts/auditoria_pruebas.py`

Recorre los 4 904 bloques `[TEST n]` de `userland/bin/test_posix/src/main.rs`
y clasifica cada uno:

| Categoría | Significado |
|---|---|
| `TAUTOLOGIA` | imprime el veredicto `verified` sin condición, o lo imprime **en ambas ramas** del `if` |
| `ERROR-GENERICO` | la rama de éxito acepta `== -1`, `< 0`, `!= 0`, `<= 0` sobre el resultado de un syscall |
| `SIN-RAMA-FALLO` | hay condición pero ninguna línea de fallo |
| `PENDIENTE` | retirada en el ciclo 46 (`PENDIENTE`, `RESUMEN-DE-EPOCA`, `SIN-SUSTRATO`): no cuenta como PASS |
| `OK` | condición discriminante con rama de fallo |

Excepciones revisadas a mano (con justificación en el propio guion): `1241`,
`1296`, `2296`, `2740`, `2759`. `== -1` se admite únicamente cuando la prueba
declara `EPERM` (porque `-EPERM == -1` y el núcleo ya no devuelve `-1`
genérico).

`python3 scripts/auditoria_pruebas.py --estricto` devuelve código 1 si hay
hallazgos; `recertify.py` lo ejecuta como paso 0 y **no certifica** si falla.

## 2. Resultado inicial y corrección

| Hallazgo | Antes | Después |
|---|---|---|
| Tautologías reales (sin condición / ambas ramas) | 11 | 0 |
| Éxito con error genérico | 58 | 0 |
| Sin rama de fallo | 20 (falsos positivos del clasificador, afinado) | 0 |
| `execute_and_verify(..., "")` en la suite headless (patrón vacío = siempre pasa) | 8 | 0 (+ prohibido por código) |

Pruebas reescritas con comprobación real: 1291 y 2346 (`getppid` visto desde
un hijo), 2297 (`sched_yield` devuelve 0), 3172 (`poll` real sobre tubería
vacía/llena), 3147 (`mincore` residencia tras tocar la página), 3148
(`madvise` con consejo inválido → `EINVAL`), 3149 (`msync` alineado/no
alineado), 3152 (`mremap` con `MREMAP_MAYMOVE` conservando el contenido),
3139 y 3156 (rama de fallo real), 2296 (`poll_key` drenado hasta el centinela).

Las 8 pruebas de la suite headless con patrón vacío ahora exigen el código de
salida real del proceso (`name='cmp' code=0`, `code=1` para ficheros
distintos, `wc -c` = 10 tras `truncate`, `wc` falla tras `unlink`, ...).

## 3. El núcleo ya no devuelve `-1` genérico

Se revisaron **una a una** las 198 + 20 + 11 apariciones de `-1`/`(-1, 0)`
en `kernel/src/syscall/mod.rs` y las de `process/`, `ipc.rs`, `lkm/ksyms.rs`,
`net/socket.rs` y `drivers/audio`. Cada una devuelve ahora el errno POSIX que
corresponde a su causa:

| Causa | errno |
|---|---|
| puntero/cadena de usuario inválidos, `copy_*_user` falla | `-EFAULT` (14) |
| fd fuera de rango, no abierto, no legible/escribible | `-EBADF` (9) |
| sin proceso actual | `-ESRCH` (3) |
| ruta/componente inexistente, módulo no cargado | `-ENOENT` (2) |
| ya existe (`mkdir` sobre `/` o ruta existente) | `-EEXIST` (17) |
| parámetros inválidos (alineación, longitud, whence, flags, dominio…) | `-EINVAL` (22) / `-EAFNOSUPPORT` (97) |
| sin fd libre / tabla de sockets llena | `-EMFILE` (24) / `-ENFILE` (23) |
| sin memoria, mapeo imposible | `-ENOMEM` (12) |
| sin dispositivo / montaje fallido / E-S | `-ENODEV` (19) / `-EIO` (5) |
| `lseek` sobre tubería, `getcwd` corto, `unlink` de directorio, `chdir` a fichero | `-ESPIPE` (29), `-ERANGE` (34), `-EISDIR` (21), `-ENOTDIR` (20) |
| tubería llena/sin lector, `fork` sin hueco, cola IPC llena | `-EAGAIN` (11) |
| `waitpid` sin hijos | `-ECHILD` (10) |
| `execve` con ELF inválido | `-ENOEXEC` (8) |
| operación de privilegio | `-EPERM` (1), comentado explícitamente |

Consecuencia visible: 75 pruebas de `test_posix` que "pasaban" con `-1`
tuvieron que ser endurecidas para exigir el errno exacto — y `busybox`
(`rm -rf`, `mkdir -p`) empezó a funcionar porque distingue `ENOENT` de
`EPERM`.

## 4. Zero `unwrap/expect/panic` en el núcleo

Quedan sólo: el `panic!` de OOM del asignador global (fallo fatal legítimo), el
`panic!` de "RAMFS no montado" en la inicialización, las macros de aserción de
`serial.rs` (sólo en depuración), y `unreachable!()` tras un `loop` sin
salida. Se eliminaron el `.expect()` de `arch/ring3.rs` (ahora registra
`[R3-FALLO]` y detiene la CPU) y el `.unwrap()` de `procfs.rs`.
