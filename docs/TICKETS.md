# Tickets abiertos de Portix OS

Registro de trabajo pendiente acordado con el usuario. Cada ticket se cierra
con un lote certificado (build + suite headless + recertificación) y una
entrada en `AGENTS.md`. Estado al 24-10-2026 (build 173, suite 51c 3579/3579,
recert 87 MASTER-PASS).

---

## T-000 · Ronda de definición: qué hace especial a Portix  — **CERRADO (24-10-2026, decidido con el usuario)**

**Titular (una línea)** — combina lo que YA es cierto y verificable:
> **Portix OS: microkernel en Rust que ejecuta binarios Linux sin modificar, y cuyas afirmaciones se pueden verificar desde fuera.**

Tres pilares, cada uno con su prueba que puede fallar:
1. **Arquitectura**: núcleo Rust `no_std` sin dependencias externas; WM, gestor de paquetes y servicios en Ring 3; Fast-IPC. (Pruebas: suite headless, recert, `cargo tree` vacío.)
2. **Compatibilidad como traductor**: ABI Linux x86-64 como capa; binarios de distro con el SHA-256 del fabricante (busybox, musl, tcc hoy; glibc/cualquier distro = T-004).
3. **Verificabilidad externa**: `e2fsck` ajeno, OVMF, SHA-256 de terceros, errno exacto, imágenes reproducibles desde cero. *Lo que no se pueda verificar así, no se afirma.*

**Público: "todo junto", por fases** (decisión del usuario). Orden en que cada público recibe valor real:
- Fase 1 — **desarrolladores y sistemas** (terminal, toolchains, servicios): es donde la compatibilidad aterriza primero (T-001…T-005).
- Fase 2 — **escritorio** (Wayland, GTK/Qt, navegador): T-006.
- **Embebido/educación e investigación** se benefician de la arquitectura desde ya (imagen mínima, documentación, mediciones) y de T-008…T-010 sin trabajo específico.

**Rasgos aún no conquistados → TODOS al roadmap** (decisión del usuario) como tickets T-008, T-009, T-010. No se afirman en UI/terminal hasta que su prueba pase.

**Textos no medidos → "medir primero"** (decisión del usuario): ticket T-011. Cada cifra se sustituye por la medición real de Portix en QEMU con su prueba; las comparativas con Linux/Windows se eliminan (no se pueden medir aquí en igualdad de condiciones).

---

## Plan técnico acordado (orden vigente: T-001 → T-002 (T-011 cerrado) → … ; T-008/T-009/T-010 se intercalan cuando el usuario lo pida)

### T-001 · ext2 completo y rápido  — **CERRADO (24/25-10-2026, build 182; suite 58 3597/3597, recert 96 MASTER-PASS)**
Hecho y probado (suite `T-001:*` + comprobación del anfitrión con `debugfs`/`e2fsck`):
- **Caché de bloques write-through** (`CacheBloques` en `ext2.rs`): arena de 2 MiB, correspondencia directa; toda escritura va al disco en el acto y refresca la ranura; `escribir_bytes` (superbloque) invalida; `descartar_cache_bloques()` para medir en frío. Medido: lectura de `/bin/test_posix` 4,3 MiB/s frío → **35 MiB/s caliente (8×)**; `portix-fuzz` 10–11 s → **6,2–6,9 s** (límite de la suite bajado a 15 s). `bench` publica aciertos/fallos y la suite exige caliente ≥ 3× frío (frío = tras descartar la caché).
- **Indirecto triple**: `geometria()` + mapeo/asignación/liberación genéricos por niveles (`mapear`, `mapear_cacheado`, `mapear_o_asignar`, `liberar_subarbol`); `verificar()` ya recorre todos los bloques del inodo. `FsFileInfo.pos/size` pasan a **u64** y `lseek` pierde el tope de 4 GiB. Prueba: fichero disperso de 4 831 838 214 bytes con **4 bloques** (TIND/DIND/IND/dato, Blockcount 32), lectura `TRIPLE` a través del triple, hueco leído como ceros, `truncate` libera el árbol entero (Blockcount 0); `debugfs` del anfitrión ve `(TIND)`.
- **`link`/`chown`** en el contrato (`enlazar`, `cambiar_propietario`): ext2 real (nlink, ctime, EPERM en directorios, EMLINK, EEXIST), ramfs por su adaptador, FAT RO → EROFS/EPERM. `sys_link` da **EXDEV** entre sistemas distintos (`vfs::mismo_sistema`); `sys_chown` ya no devuelve 0 sin hacer nada; `lchown` sobre enlace → EOPNOTSUPP. `stat`/`fstat` devuelven nlink, inodo y **bloques reales** (dispersos) del contrato.
- **`fsync`/`fdatasync`** reales: `sincronizar()` del sistema (superbloque + GDT + ATA FLUSH CACHE); antes devolvían 0.
- **Tabla ABI Linux**: faltaban 23 syscalls de ficheros (74 fsync, 75, 76 truncate, 77, 82 rename, 85 creat, 86 link, 90 chmod, 91, 92 chown, 93, 94, 133 mknod, 162 sync, 258 mkdirat, 259, 260 fchownat, 263 unlinkat, 264 renameat, 265 linkat, 266 symlinkat, 268 fchmodat, 306 syncfs) → busybox recibía ENOSYS; conectados a las nativas.
- `mknod` de fichero regular en ext2 por `crear`; FIFOs/dispositivos fuera de ramfs → EPERM explícito.
- `chown` con regla POSIX (root, o dueño → su uid/egid); ext2 asigna uid/gid del creador; 5 pruebas de `test_posix` corregidas a la semántica Linux real.
- **FAT de rescate podada (build 181)**: fuera `FdType::File(OpenFileInfo)`, 19 bloques FAT de `syscall/mod.rs`, el rescate de `elf::read_binary_file`, `load_ring3_init`/`fat32_list_dir`, el **`mkfs::auto_format` del disco 0** cuando la FAT no montaba (destructivo), el `hello.elf` embebido como PID 1 ficticio y el listado inventado del explorador. Sin raíz: `[MODO DEGRADADO]`, sin PID 1, consola del núcleo viva, syscalls de ficheros → **ENODEV**. Prueba de la suite: arranque con el superbloque ext2 de P3 destruido.
- **FIFOs en ext2 (build 182)**: `crear(ruta, S_IFIFO|perm)` crea el inodo real; `open` lo enruta a la tubería con nombre; `busybox mkfifo` + `cat fifo > out &` + `wait` transportan datos; `debugfs` ve `Type: FIFO`. Dispositivos (char/block) en ext2 → EPERM (sin `rdev`; fuera de alcance).
- Al probarlo se descubrió y arregló que **`wait` de busybox sh nunca volvía**: SIGCHLD sólo al reap y `rt_sigsuspend`/`pause` devolvían -EINTR sin esperar. Ahora son reales (máscara temporal, espera, restauración POSIX); Linux 34 `pause` añadido. Pruebas 218/1594/2002 de `test_posix` reescritas para exigirlo.
- `pkg install musl-dev` se mide en la suite (tiempo de pared impreso como `[T-001] medido:`); no se fija umbral inventado.

### T-002 · H1 — `.apk` real desde Alpine  — CERRADO (build 183: suite 59 3616/3616, recert 97 MASTER-PASS)
- **Hecho**: `pkg add <paquete>…` descarga `APKINDEX.tar.gz` de los repositorios de `/etc/apk/repositories` (v3.22 main + community, `dl-cdn.alpinelinux.org`, HTTPS real con el `curl` de Portix), verifica la firma RSA-4096 (PKCS#1 v1.5/SHA-1 sobre el gzip firmado) con la clave de `/etc/apk/keys/`, parsea el índice (gzip + tar + PAX en Rust propio), resuelve dependencias (`D:` con `so:`/`cmd:` vía `p:`), descarga cada `.apk`, verifica firma + `C:Q1` + `datahash` (SHA-256) y despliega sobre la raíz ext2 (`mkdir -p`, symlink, hardlink, chmod). `pkg del`, `pkg apk-list`, `pkg apk-update`.
- **Prueba (suite, 22 comprobaciones)**: `pkg add tcc tcc-libs-static musl-dev` contra el espejo oficial sin espejo propio; el tcc instalado compila `prueba_tcc.c` y el binario corre; paquete inexistente falla; `pkg del tcc-libs` rompe `tcc -v` de verdad y `pkg add tcc-libs` lo repara.
- **Hallazgos colaterales**: (a) `write()` de 9 MiB desde Ring 3 tiraba el núcleo (`vec![0; count]`); corregido con trozos de 256 KiB. (b) el ext2 no seguía enlaces simbólicos (`/usr/lib/libc.so -> ../../lib/ld-musl…` se leía como fichero); ahora `open`/`stat`/`exec` los siguen y `lstat`/`readlink`/`unlink`/`link` no (ELOOP a los 40 saltos).
- **Pendiente dentro del ticket**: restricciones de versión (`>=`, `<`) no se evalúan (se instala la versión única del índice); scripts `.pre/.post-install` y triggers no se ejecutan (se avisa); sin `pkg upgrade`; una sola arquitectura (x86_64).

### T-003 · H2 — gcc de Alpine (musl) compila dentro de Portix  — CERRADO (build 185)
- Prueba de aceptación: `scripts/prueba_t003.sh` (`/usr/src/prueba_t003.sh` en la
  imagen) → `[T003-PASS]`: `gcc -v -O2 -c` (cpp→cc1 reales), `as`, `ld`/`collect2`,
  el binario dinámico corre (`[TCC-RUN] fib(20)=6765`), y `gcc -static` también.
  Verificado en el flujo limpio (imagen recién construida → `pkg install
  busybox-static` → `pkg add gcc musl-dev` desde el espejo → compilar): 66 s.
  Suite 60: 3626/3626 (bloque T-003 de 10 pruebas); recert 98 MASTER-PASS.
- Lo que hubo que arreglar (todo en el núcleo salvo el último punto):
  1. `cc1` (37 MB) no cabe en el montículo (64 MiB): `elf::FuenteElf` carga la
     imagen **por trozos de 256 KiB desde el ext2** (`load_segments_fuente`), sin
     leer el fichero entero; `execve` y `elf_load_from_path` la usan.
  2. El cargador reutilizaba como "marco de usuario" la traducción heredada de la
     identidad plana: `cc1` ocupa 0x400000–0x2B06000, que cae sobre el montículo
     físico (0x1000000–0x5000000) → la imagen se escribía ENCIMA del montículo
     (#PF en `BuddyAllocator::dealloc`). Ahora una traducción con `HUGE_PAGE` se
     considera heredada siempre y se mapea un marco nuevo.
  3. Faltaban en la tabla Linux `readv` (19), `umask` (95) y `getrusage` (98):
     `as` fallaba con "can't read: Function not implemented" (musl `fread` grande).
  4. `pkg` + `curl` agotaban los 428 MiB de marcos físicos (brk de `pkg` 238 MiB)
     y el respaldo al montículo se comía 42 MiB del núcleo → PTE con bits
     reservados / triple fallo. Dos medidas: el respaldo respeta una reserva de
     24 MiB (`paging::RESERVA_MONTICULO`, cuenta `RESPALDO_DENEGADO`, el proceso
     recibe ENOMEM) y el asignador de `portix-rt` retrocede el cursor al liberar
     el último bloque grande y hace `realloc` in situ (pico de marcos 109782 →
     95647, `respaldo_heap=0`).
- Pendiente (no bloquea): `pkg` sigue cargando el `.apk` entero en memoria (brk
  186 MiB con gcc); `vfork` se sirve como `fork` completo; `/proc/self/exe` no
  se ha necesitado.

### T-004 · H3 — glibc (abre "cualquier distro")  — CERRADO (build 186 esencial; build 187: firma OpenPGP de `InRelease` + glibc en ancho, suite 63 3650/3650, recert 100 MASTER-PASS; `.rpm` queda fuera, ver abajo)
- **Build 187 (26-10-2026)**: (B) `InRelease` verificado con la clave OpenPGP del archivo de Debian (`pgp.rs`, RSA-4096 PKCS#1 v1.5, `pkg deb-verify`, `prueba_t004b.sh` rechaza un byte cambiado); (C) `pkg deb-add bash coreutils grep sed findutils diffutils` (21 `.deb` más, `Provides` virtuales) y `prueba_t004c.sh` → `[T004C-PASS]` con 11 utilidades de Debian. Arreglos de núcleo: `sysinfo` 112 B (era 128 → stack smashing en `sort`), `mmap` de fichero por trozos (PANIC con libcrypto 6,5 MiB), **`dirfd` real en las `*at`** (`con_dirfd`; `find` daba vacío), `fchdir`/`statfs`/`fstatfs` (120 B)/`tgkill`/`tkill`/`fadvise64`/`getpeername`/`getpgrp` en el dispatch Linux.
- Deuda de T-004 (no bloquea): las señales no se entregan sobre la pila de `sigaltstack` (sólo se registra por PID); `linkat`/`renameat` con dos `dirfd` distintos siguen resolviendo contra `cwd`; `cwd` del proceso es de 64 B (PATH_MAX 4096 → ENAMETOOLONG); `df` imprime `-` sin `/proc/self/mountinfo`; quedan `vec![0u8; len]` con `len` de usuario en `syscall/mod.rs` (~L3435, 3677, 4648, 4710, 4816) que pueden hacer PANIC con tamaños grandes; OpenPGP: no se validan caducidad/revocación ni firmas Ed25519 (basta una firma RSA de clave confiada); `#!/bin/bash` no resuelve (bash sólo en `/usr/bin/bash`: merged-/usr sin `/bin -> usr/bin`); 27 `mincore`, 326 `copy_file_range`, 192/195 `*xattr` → ENOSYS (inocuos).
- Prueba de aceptación cumplida: **`pkg deb-add hello`** descarga por HTTPS de
  `deb.debian.org` el índice `trixie/main/amd64/Packages.gz` (13,3 MB, 68 825
  paquetes, recorrido en flujo), resuelve dependencias en orden topológico
  (`gcc-14-base → libgcc-s1 → libc6 → hello`), verifica `Size` + `SHA256` del
  índice sobre cada `.deb` (SHA-256 en flujo), desempaqueta `ar` → `control.tar.xz`
  / `data.tar.xz` (descompresor **xz/LZMA2 propio en Rust**, `pkg/src/xz.rs`) y
  despliega los ficheros sin modificarlos; crea `/lib64 -> usr/lib64` (merged-/usr).
  Después `scripts/prueba_t004.sh` → **`[T004-PASS]`**: `ld-linux-x86-64.so.2`
  (225 672 B) y `libc.so.6` (1 995 216 B) de Debian; `libc.so.6` ejecutada
  directamente imprime "GNU C Library (Debian GLIBC 2.41-12+deb13u4)"; GNU Hello
  (`/usr/bin/hello`, ELF PIE dinámico contra glibc) corre con `--greeting` y
  `--version` (mismo texto que en un Linux anfitrión) y acepta un argumento de
  100 000 bytes. Flujo limpio en QEMU: `deb-add hello` ≈ 60 s, el guion 3-5 s.
- Lo que hubo que arreglar en el núcleo (encontrado con glibc, no con musl):
  1. `TCGETS` escribía 60 B (termios de Portix) donde Linux escribe 36 B → la
     glibc estática moría con `*** stack smashing detected ***`.
  2. Al entrar en Ring 3 los GPR no iban a cero: glibc toma **RDX como
     `rtld_fini`** y saltaba a basura (`#PF RIP=0x1B`).
  3. `time(2)` (nr 201) faltaba (glibc sin vDSO lo usa; vía `rtc::now_epoch()`).
  4. **RSP de entrada alineado a 16**: ambas rutas de construcción de pila
     alineaban la base a 8 y contaban palabras; ld-linux hacía `movaps` sobre la
     pila → `#GP`. (Con musl funcionó por suerte de tamaños.)
  5. **Pila de usuario extensible hasta 8 MiB** (`process::PILA_MAX`,
     `paging::crecer_pila`): fallo de página no-presente bajo los 64 KiB iniciales
     mapea un marco a cero (W+U+NX); `fork` copia las páginas crecidas y
     `exit`/`execve` las liberan. (`pkg` ya desbordaba los 64 KiB con un búfer de
     64 KiB en la pila: `#PF EC=6` en `tar_entero`.)
  6. **Argumentos largos**: `execve` copiaba cada cadena truncada a 512 B y como
     mucho 64 argumentos; ahora 128 KiB por cadena (MAX_ARG_STRLEN), 512 KiB en
     total (E2BIG si más) y 1024 argv/envp; las cadenas que no caben en la pila
     inicial van a páginas de la pila extendida.
  7. **PT_INTERP ausente ⇒ `execve` falla con ENOENT** (antes caía a `/bin/ldd`
     y el programa saltaba a 0). Se admite además `/usr` + ruta del intérprete.
  8. `write(2)` sobre una **tubería llena bloqueaba mal**: devolvía EAGAIN o cuenta
     corta a un fd bloqueante (`head -c 100000 /dev/zero | wc -c` daba 4096);
     ahora espera al lector (tope 10 s), EPIPE si se va. `sendfile(2)` (que usa
     busybox `head`/`cat`) acepta `/dev/zero`/`/dev/null` como entrada y bloquea igual.
- Integridad: HTTPS (TLS propio) + SHA-256 del índice. **Pendiente**: verificar
  la firma OpenPGP de `InRelease` (RSA-4096/SHA-512) para que la confianza no
  dependa sólo del canal; los scripts de mantenedor NO se ejecutan (se anuncia).
- Pendiente de T-004 (no bloquea): `.rpm` (cpio + zstd) y Fedora; `Packages.gz`
  se guarda comprimido y cada pasada de `resolver` lo vuelve a inflar (3-4
  pasadas ≈ 40 s); sólo LZMA2 sin filtros BCJ en `xz.rs` (Debian no los usa en
  `data.tar.xz`); `rseq`/`clone3` no han hecho falta para glibc 2.41 (ENOSYS).

### T-005 · Hilos reales → H4 rustc/cargo  — HILOS HECHOS (build 189); queda la parte rustc/cargo
- Certificación b189: suite 66 **3664/3664** + recert 103 MASTER-PASS (la suite 65 dio 33 FAIL por tres causas ajenas al diseño de hilos, corregidas y anotadas en "Deuda menor").
- Hecho en b189: `clone`/`clone3` con `CLONE_VM|CLONE_THREAD|CLONE_SIGHAND|CLONE_SETTLS|
  CLONE_PARENT_SETTID|CLONE_CHILD_SETTID|CLONE_CHILD_CLEARTID` crea un hilo de verdad
  (misma CR3, tabla de descriptores compartida con el líder, brk/mmap del líder, TLS
  propio, kernel stack propio, pila de usuario la que da la libc; `clone3` suma
  `stack + stack_size`); `futex` real con cola de espera (`WAIT`/`WAIT_BITSET` con plazo
  relativo/absoluto, `WAKE`/`WAKE_BITSET`, `REQUEUE`/`CMP_REQUEUE`), antes era un
  despertar espurio; `set_tid_address` guardado y `*tid=0`+WAKE al morir el hilo;
  `exit(2)` termina sólo el hilo (líder con hilos vivos: duerme y el proceso acaba con el
  último), `exit_group` mata al grupo, señal fatal/excepción en un hilo mata al grupo;
  `gettid` ≠ `getpid` (= tgid), `tgkill` comprueba el grupo; `madvise` (28) y
  `clock_nanosleep` (230, TIMER_ABSTIME corregido) en el despacho Linux.
- Prueba (suite, 14 comprobaciones): `scripts/sondas/t005_hilos.c` compilado por el gcc del
  anfitrión (`-static -pthread`, glibc), descargado por red y ejecutado: 4×20000 bajo
  mutex = 80000, gettid/getpid, join con retorno, 200 vueltas de ping-pong con cond,
  join que espera 300 ms reales, pthread_exit en main con hilo vivo. `[T005-PASS]`.
- Límites conocidos (honestos): `kill` al tgid se entrega al líder aunque esté dormido por `pthread_exit`;
  ≥ 4 GiB virtuales por proceso no medido. (`execve` desde un hilo → T-017 HECHO 88ª; `FUTEX_WAKE_OP`/PI →
  T-018 HECHO 88ª; varios núcleos → T-025 E3d.)
- **H4 rustc de la distro — EN CURSO (b203, 89ª)**: sección T-005b del runner: `pkg deb-add rustc`
  (trixie 1.85.1: 56 .deb, 155 MB bajados, 639 MB en el ext2; raíz ampliada a 2048 MB) →
  `/usr/bin/rustc --version` → compilar `hola.rs` dentro de Portix con el gcc-14 de Debian como
  enlazador → ejecutar el binario (Vec, args, hilo+join). Para que fuese viable hubo que quitar
  los dos `fsync` por bloque del camino de escritura ext2/ATA (deb-add 3× más rápido) y corregir
  el defecto 27 (`statx(fd, "", AT_EMPTY_PATH)` → ENOENT; rustc no podía mapear los .rlib).
  Medido: despliegue 730-1513 s según anfitrión; `rustc --version` arranca; `rustc hola.rs`
  compila y ENLAZA dentro de Portix (gcc-14 → collect2 → ld code=0, 10,7 s). Defectos en la
  salida de rustc: 32 (dos hilos cerrando el grupo en paralelo en `exit_group`: el hilo con
  SIGKILL de `terminar_hilo_del_grupo` entraba en `matar_grupo_por_senal`; ahora `salir_solo`,
  build 112) y 33 (`execve` descartaba los fd ≥ 3 sin `fd_close`: los escritores de las
  tuberías heredadas por gcc no se restaban y el `poll` de `Command::output()` nunca veía
  POLLHUP; ahora execve cierra sólo los FD_CLOEXEC y dup/dup2/dup3 limpian el bit; build 113).
  `/usr/local/hola` ya se ejecutó; el cierre de b203 sigue pendiente: dos suites completas verdes
  consecutivas sobre una build identificable y recert. La suite 5 fue 3813/3813; la recert 128
  reconstruyó las imágenes antes de la suite 6 y el entorno se reinició sin guardar su resultado
  final. No se dispone del hash anterior para probar identidad binaria; no declarar el cierre.
- **Calidad del runner (código corregido, pendiente QEMU)**: se exige la verificación de tamaño y
  SHA-256 informada por pkg, instalación individual no vacía y plan completado; el delta T-014b
  requiere ambas lecturas completas y no asigna cero al faltar la primera. El mmap grande se busca
  sólo en la ventana de Java, no en el log entero. `test_verificaciones_b203.py` 19/19; analizador
  independiente 11/11; sobre registros serie reales de las suites 2-5, pkg informó verificación e
  instalación y los deltas fueron 13118/13197/13006/13196. Estos controles no certifican la versión
  modificada en QEMU. Evidencia en `docs/evidencias/b203/`; `build/` no sobrevive instantáneas.
  cargo (75 .deb, 693 MB) queda para después, con el mismo método.

### T-005b · Lenguajes sobre los hilos: C++ → Rust → nasm → Python → Java  — b190 (C++, Rust, nasm, Python); **Java CERRADO históricamente en b201**, con regresiones abiertas en b203 (T-031 y T-033). T-014b y T-020 cerrados con 94a/94b + recert 126. Queda también el defecto 24 (abajo).
- **Estado b203**: en la copia Java con kernel experimental 116, clase y versión pasaron con código 0; tras cambiar señales y compilar kernel 118, la clase volvió a pasar en 78,5 s, pero `java -version` no terminó en 120 s ni en los 20 s de diagnóstico y la sonda quedó incompleta. No extrapolar el PASS anterior al núcleo nuevo. Véanse T-031, T-032 y T-033 y los registros `sonda-java-reuso*.log` en `docs/evidencias/b203/`.
- Orden fijado por el usuario. Cada lenguaje tiene una sonda real (`scripts/sondas/t00{6,7,8,9}_*`)
  que el propio `headless_runner.py` compila/publica en el anfitrión, descarga por red al ext2 del
  invitado y ejecuta; cada comprobación imprime un valor exacto y el programa termina con `[T00n-PASS]`.
- **C++ (b190)**: `t006_cpp.cpp` con `g++ -std=c++17 -static -pthread` (libstdc++ + glibc sin modificar):
  std::thread/mutex (80000), condition_variable (200 vueltas), excepciones dentro de 3 hilos
  (unwinder + TLS de `__cxa_get_globals`), std::async/promise, static local con `__cxa_guard`
  desde 4 hilos, std::map/ostringstream con malloc compartido, sleep_for medido, thread_local con
  destructor por hilo. 15/15 sin tocar el núcleo. Suite 67 3678/3678 + recert 104 (fallo ajeno:
  disco lento del anfitrión, ver Deuda) → suite 68 3692/3692 + recert 105 MASTER-PASS.
- **Rust (b190)**: `t007_rust.rs` con el rustc del anfitrión (`x86_64-unknown-linux-gnu`,
  `-C target-feature=+crt-static`, 5,8 MB): spawn+Arc<Mutex>, Condvar, mpsc (3000 mensajes),
  thread::scope, `panic!` en un hilo → `join()` Err (unwinder) con el mensaje por stderr, Barrier +
  RwLock, park/unpark + sleep medido, thread_local! con Drop y nombres de hilo. 13/13 sin tocar el núcleo.
- **nasm (b190)**: `t008_nasm.asm` (nasm + `ld -static`, 11 KB, sin libc): ABI desnuda — write,
  open/read en ext2, mmap/munmap, `clone(CLONE_THREAD|CHILD_CLEARTID)` a mano con pila propia +
  `lock xadd` + futex sobre el tid que limpia el núcleo, gettid, nanosleep medido. **Hallazgo**:
  `nanosleep` (35) no estaba en el traductor Linux (glibc usa `clock_nanosleep`): añadido.
- **Python (b190)**: `pkg deb-add python3` instala el CPython 3.13.5 REAL de Debian trixie
  (35 .deb: libpython3.13-stdlib, libssl3t64, libsqlite3, libffi8, libdb5.3, tzdata, readline…;
  166-280 s según la red); `t009_python.py`: threading.Thread/Lock (80000), Condition, queue con
  `get_native_id` distintos, fichero + hashlib (libcrypto de Debian), `subprocess.run` de otro
  python (posix_spawn de glibc 2.41 con `closefrom`), sleep medido, `signal.setitimer` → SIGALRM
  con otro hilo ocupado, json en hilo. 18/18. **Costó al núcleo** (todo encontrado con la traza
  `STRACE_PREFIJO`, ahora con valor devuelto): `ioctl FIOCLEX/FIONCLEX` (CPython los usa en
  `open_code`; sin ellos moría con "Failed to import encodings module"), `mremap` (25) en el
  traductor, `close_range` (436) aceptando `CLOSE_RANGE_UNSHARE` (posix_spawn moría con rc=127),
  `setitimer`/`getitimer`/`alarm` (36-38) en el traductor. `python3-minimal` solo no basta
  (sin `queue`): hace falta `python3`.
- Límites honestos: `setitimer` sólo arma `it_value` (sin rearme periódico por `it_interval`);
  `mremap` mueve con copia y pierde los permisos originales (queda RW); el child de `subprocess`
  pasa por posix_spawn (CLONE_VFORK se trata como fork COW); un solo núcleo.
- **Java (HECHO en b191, medido primero)**: `pkg deb-add openjdk-21-jre-headless` (41 .deb,
  42 MB, ~8 min) y `T010Java.class` (compilada en el anfitrión con `javac --release 11`):
  `[T010-PASS]` 8/8 (hilos `synchronized`, ExecutorService, BlockingQueue, ficheros en ext2,
  `Thread.sleep` medido, 100 000 NPE implícitas con el JIT activo, GC con 128 MB de basura,
  excepciones con traza). Cada arranque trazado con `STRACE_PREFIJO` reveló un fallo real:
  1. `readlink(/proc/self/exe)` devolvía `/bin/<nombre truncado a 31>` → `libjli.so` no se
     hallaba vía `$ORIGIN`. Ahora `Process.exe_path` (160 B) real en exec/fork/hilos.
  2. Señales sin `ucontext` (RDX=0), `si_code` "sent by kill" y sin `si_addr`: el JVM
     abortaba "SIGSEGV at pc=0". Ahora `ucontext_t` Linux (gregs, cs/ss, err/trapno/cr2,
     máscara, bloque FXSAVE con los XMM del frame), `si_addr`+`SEGV_MAPERR/ACCERR`,
     `FPE_INTDIV`, `TRAP_BRKPT/TRACE`, zona roja de 128 B, y `rt_sigreturn` restaura desde
     el `ucontext` (el manejador puede cambiar RIP: continuaciones del JIT) y repone los XMM.
  3. `getcpu` (309) y `clock_getres` (229) → ENOSYS; glibc caía al vsyscall `0xFFFFFFFFFF600800`
     (#PF). Implementadas; `flock` (73) añadida al traductor.
  4. `mmap` asignaba marcos a TODO (incluso PROT_NONE): el JVM reserva 128 MB + 240 MB + 1 GB
     y moría "Could not reserve enough space in CodeHeap" con `marcos_libres=0` → **T-014**.
  5. Cifras falsas: `/proc/meminfo` y `sysinfo` devolvían el montículo del núcleo (64 MiB);
     `prlimit64` trataba el recurso 9 (RLIMIT_AS) como NPROC=128 → "Too small maximum heap".
     Ahora marcos físicos reales (439 MB) y límites verdaderos (pila 8 MiB, AS/DATA infinito,
     NPROC = ranuras reales).
  6. `MAX_PROCS` 16 → 64: el JVM arranca 11 hilos y la sonda 4 más (`pthread_create` EAGAIN).
- Límites honestos de Java: el fichero `modules` (134 MB) se mapea con copia completa (sólo lo
  anónimo es perezoso; pico de la sonda 53 336 marcos = 208 MB); CDS avisa "heap region 3
  failed" (funciona sin objetos archivados); un solo núcleo (`availableProcessors()=1`); cada
  NPE interpretada deja una línea `[EXCEPTION] #PF` + `[SIG]` en la serie (7 691 en la sonda).

### T-014 · `mmap` perezoso (reserva sin marco, materialización en #PF)  — HECHO (b191); **T-014b fichero perezoso CERRADO (b201, AGENTS 87ª; suites 94a/94b + recert 126)**
- T-014b: `mmap(fd)` MAP_PRIVATE y MAP_SHARED de sólo lectura perezosos (`mem/vmas.rs`: tramos por CR3; relleno desde
  el VFS en el #PF; fork/munmap/exit; EACCES en `mprotect(PROT_WRITE)` sobre MAP_SHARED O_RDONLY). JVM: `modules`
  (34 389 páginas) ya no se copia; 13 242 páginas leídas en toda la sonda; pico de marcos 48 969 → 25 306. Deuda:
  MAP_SHARED escribible sigue ansioso; `mremap` no mueve tramos (ni PTE perezosas); fichero modificado tras el `mmap`
  y antes del primer acceso se ve modificado (Linux: ETXTBSY/coherencia de caché de páginas).
- Antes toda página `mmap` costaba un marco al instante (y un fichero mapeado se copiaba
  entero). Ahora `mmap` anónimo privado sin `MAP_POPULATE` escribe una PTE NO presente con
  `DEMAND_PAGING_FLAG` y la protección en los bits 52-54; el primer acceso la materializa con
  un marco a cero (`materializar_perezosa`, antes del registro `[EXCEPTION]`: no es una
  anomalía) y un acceso que la protección no permite (PROT_NONE, escritura en RO) acaba en
  SIGSEGV como en Linux. `mprotect` reprotege sin materializar; `munmap` borra la PTE;
  `madvise(MADV_DONTNEED)` devuelve el marco y deja la página perezosa (semántica Linux: la
  siguiente lectura da ceros); fork hereda las reservas (`copiar_perezosas`); `copy_to_user`/
  `copy_from_user` materializan en nombre del usuario; límite 2 GiB por mapeo (ENOMEM).
- Límites: los mapeos de fichero y `MAP_SHARED` siguen siendo ansiosos; `mremap` mueve sólo
  páginas presentes; las reservas gastan tablas de páginas (1 GiB = 4 MiB del montículo).
- Regresión destapada por la suite 71a y corregida: `mapear_perezosa` no partía las páginas
  grandes de identidad (2 MiB / 1 GiB) y escribía la PTE dentro de la memoria física del
  rango (TEST 4910, `MAP_FIXED 0x2000000` → corrupción del montículo del núcleo). Ahora
  `private_leaf_table` parte igual que `map_page`. `mincore` sigue la semántica Linux
  (0 sin tocar, 1 tras escribir) y las pruebas 459/1083 lo exigen en ambos sentidos.
- La raíz ext2 pasa de 384 a 1024 MB: OpenJDK (199 MB) + python3 + gcc no cabían (ENOSPC).
- Traducción Linux añadida en b191: `setsockopt` (54) y `getsockopt` (55), que la JVM pide al usar NIO.

### T-013 · `open(O_CREAT)` nativo en `/usr/bin` devuelve ENOENT  — HECHO (b192)
- Causa real: no era el núcleo. La raíz ext2 se exporta del árbol FAT y **`/usr/bin` no existía**
  (tampoco `/usr/sbin`, `/sbin`, `/opt`, `/srv`, `/run`, `/var/tmp`, `/var/log`); ENOENT era la
  respuesta POSIX correcta a un padre inexistente. `pkg` lo ocultaba porque crea los directorios
  con `mkdir_p`. `build.py` crea ahora el FHS mínimo en la raíz.
- Prueba (suite, `scripts/prueba_t013.sh` con busybox sin modificar): comprueba los 8 directorios,
  crea un guion en `/usr/bin`, lo ejecuta vía `PATH=/bin:/usr/bin`, lo borra y verifica que no
  existe. Falló antes del arreglo (ENOENT) y pasa después.

### T-015 · Cuelgue intermitente en la prueba FIFO de T-001 (visto en suite 72a)  — **HIPÓTESIS DE RETORNO CORTO DESCARTADA; el escenario ya es prueba permanente (1-11-2026)**
- En la pasada 72a el guion `prueba_t001.sh` se detuvo tras `T001 fifo tipo:` (paso 7: `cat fifo > out &`,
  `echo > fifo`, `wait`) y no imprimió `T001 fifo lee:` en 90 s; en la sonda suelta, en `--solo` y en
  la pasada 72b el mismo paso pasa. `fifo_open`, `read` y `write` de tubería tienen topes de 10 s, así
  que el bloqueo largo no está en esas esperas: sospechar de `wait`/`waitpid` del hijo en segundo plano
  o de una carrera entre `pipe_close_*` → `fifo_forget` y una apertura concurrente de la misma ruta.
- Caza 27-10-2026: `scripts/sondas/caza_fifo_t015.sh` (100 repeticiones del paso 7, busybox) → 100/100
  correctas, ninguna iteración > 1 s, en una VM recién arrancada. NO reproducido aislado: el fallo
  depende del estado acumulado de la suite (PIDs ~350, tuberías previas). Siguiente intento: ejecutar la
  caza al final de la sección "Real HTTP Ingest" con `[SYS]` activo y, si cae, volcar `pipes`/`ps`.
- Plan original: repetir el paso 7 en bucle (100×) dentro de una sonda con `[SYS]` activo y guardar el
  log serie; añadir traza en `fifo_open` (pid, acceso, base_lector/base_escritor, resultado de la espera).

#### T-015 · qué se probó (1-11-2026)

- **Hipótesis «el sueño vuelve antes de tiempo» → descartada con datos.** Se instrumentó `sys_sleep_switch`
  (`SUENOS`, `SUENOS_CORTOS`, `SUENOS_CON_SENAL`, `SUENOS_CON_CAMBIO`, `SUENO_PEOR_DEFICIT`, aviso
  `[SUENO-CORTO]` con pid y ticks que faltaron) y se publicó en `/proc/smp` (`suenos: totales … cortos …
  peor_deficit_ticks …`). Fixture nuevo `scripts/sondas/t015_sueno.rs`: **50 sueños de 100 ms**, comparando
  el reloj del núcleo antes y después de cada vuelta. Resultado: **delta mínimo 10, máximo 10, media 10
  ticks; ceros=0, cortos=0, `cortos_kernel=0`, `peor_deficit=0`** y 6,09 s de pared. `nanosleep` duerme
  exactamente lo que se le pide; el «1 de cada 30 sin avanzar» del ciclo anterior era un artefacto de
  medición (dos lecturas servidas dentro del mismo tick), no un retorno corto.
- **El escenario real ya está reproducido como prueba permanente**: `scripts/prueba_t015.sh` repite el paso
  FIFO exacto de T-001 (`cat fifo > out &`, escritura, `wait`, lectura) **N veces en el mismo shell**, con
  marca `[T015-FIFO] i=<n>` por vuelta, de modo que un cuelgue dice en qué vuelta fue. En aislado: **20/20
  vueltas en 3,8 s**, sin un solo cuelgue.
- **Las tres cosas quedan dentro de la suite** (no en sondas sueltas): sección nueva «Validating T-015/T-028»
  en `scripts/headless_runner.py` con **12 comprobaciones** — publica `/proc/smp`, fixtures compilados por el
  anfitrión, 50 sueños exactos, estadística del fixture, contador del núcleo, reloj avanzando con todo
  durmiendo, busybox asegurado, FIFO 10 veces y **peor hueco del barrido ≤ 20 ticks** (medido en la sesión
  de prueba: **2 ticks**).
- **Sin cerrar**: el cuelgue de la pasada 72a no se ha vuelto a ver; no se afirma corregido, se afirma
  **vigilado y reproducible**. Su cierre formal necesita que la suite pase varias veces sin que la marca
  `[T015-FIFO]` se corte.

### T-016 · Recert intermitente en Épica 60  — HECHO (b192, causa: carrera de arranque en las sondas)
- Reproducido en la 8ª de 23 repeticiones: `Microkernel FALLO IPC sincrono`, `Depuracion FALLO trazador`,
  aviso `0 proceso(s) en la tabla, PID actual=0`. Causa: las sondas y el runner daban el arranque por
  terminado en `present done`, pero **init (PID 1) se carga desde la raíz ext2 después** de esa línea; con
  el anfitrión cargado, `certify` corría con la tabla de procesos vacía y el selftest IPC sobre PID 1 no
  encontraba ranura. Arreglo: `headless_runner`, `interactive_runner`, `consagracion_probe`,
  `signal_probe`, `devsuite_probe` y `dbg_probe` esperan `init process PID=1 registered` (o
  `[MODO DEGRADADO]`). La sonda guarda además el log serie en `build/consagracion_fallo.log` si falla.
  Recert 111: MASTER-PASS. No se relajó ninguna comprobación del núcleo.

### T-024 · Pausa de ~60 s sin salida serie en la suite (accept4 IPv6, suite 73a)  — PENDIENTE
- Suite 73a: 3737/3738; el único fallo (`[HTTPD-TLS] conexion TCP aceptada de [::1]:`) no apareció en el
  plazo de 60 s, pero el log serie final la contiene en su sitio y las comprobaciones siguientes sobre la
  MISMA salida pasaron. Los `epoch` del invitado muestran 2 s entre esa prueba y la siguiente: o QEMU quedó
  pausado por el anfitrión (presión de memoria: 2 GB con copias de imagen de 1,1 GB) o el hilo lector del
  runner se atascó. El runner imprime ahora en cada FAIL el tiempo de pared, bytes recibidos y hora.
  Si vuelve a ocurrir con "0 bytes en 60 s" y el invitado sin salto de reloj → anfitrión; si el invitado
  siguió → lector del runner.

### T-025 · SMP real (ROADMAP 5b)  — **CERRADO (b199)**: E1 (b193), E2 (b194), E3a-E3d (b195-b198), `smp on` por defecto (b199). T-026 (IF=0 en caminos largos) CERRADO en b200; queda deuda menor
- **Autorización**: añadir vectores IPI nuevos en `kernel/src/arch/idt.rs` e `isr.asm` (sólo añadidos,
  diff mínimo; nada existente se modifica). `boot.asm`/`stage2.asm`/`linker.ld`/`allocator.rs` intactos.
- **Alcance**: las 3 entregas, cada una certificada con dos suites verdes + recert y **verificada en el
  momento** (ninguna afirmación sin su línea de serie real):
  1. AP boot sin planificar: MADT (ACPI) → trampolín en modo real → modo largo → pila propia → idle.
     Prueba: `/proc/cpuinfo` lista N núcleos con LAPIC ID leídos del hardware y el AP incrementa un
     contador al recibir una IPI (contador visible desde el BSP; falla si el AP no responde).
  2. Locks auditados (marcos, planificador, `PROCESS_TABLE`, `PIPE_TABLE`, consola) + TLB shootdown por
     IPI. Prueba: sonda de estrés con `-smp 4`.
  3. Planificación en los APs: colas por núcleo, `sched_getaffinity` real, `nproc` ≥ 2; cálculo paralelo
     con binarios sin modificar medido más rápido que con 1 núcleo (cifra real, T-011).
- **Suite**: `-smp 2`; sonda extra con `-smp 4`. Sin MADT/APIC o AP mudo: seguir con un núcleo y
  declararlo (`[SMP] 1 nucleo: <motivo>`); una pasada con `-smp 1` exige ese mensaje.
- **Entrega 1 (27-10-2026, b193) — HECHA y verificada al momento**:
  - `kernel/src/arch/smp.rs` reescrito: el módulo anterior era simulación (inventaba 4 núcleos sin MADT,
    `send_ipi` sólo imprimía). Ahora: RSDP (tabla del cargador UEFI o EBDA/0xE0000 en BIOS, sumas de
    control) → XSDT/RSDT → MADT (sólo LAPIC `enabled`/`online capable`; tipos 0, 5, 9); LAPIC MMIO
    mapeada si falta; trampolín `kernel/src/arch/smp_trampolin.asm` (binario plano de 4 KiB en 0x8000,
    `build/smp_trampolin.bin` embebido con `include_bytes!`; ensamblado por `build.py`); INIT-SIPI-SIPI
    por el ICR real; el AP carga GDT/IDT del núcleo (`idt::cargar_tablas_ap`), habilita su LAPIC, lee **su**
    APIC ID y queda en `sti; hlt`. Vectores nuevos 0xF0 prueba / 0xF1 TLB / 0xF2 replanificar / 0xFF espurio
    (`isr.asm` + `idt::instalar_vectores_ipi`, sólo añadidos). `cpu_count()` sigue en **1** (planificables);
    `nucleos_en_linea()` da los reales. `/proc/smp` nuevo: leerlo dispara una IPI 0xF0 a cada AP y escribe
    `ipi_prueba a -> b OK|FALLO`. `/proc/cpuinfo` con APIC ID y MHz medidos.
  - Verificado: BIOS `-smp 1` → `[SMP] 1 nucleo: la MADT solo lista un procesador`; `-smp 2` →
    `[SMP] AP 1 (APIC ID 1): en linea, ve su APIC ID 1, IPI 0xF0 contador 0 -> 1 OK`; `-smp 4` → 3 AP en
    línea, 3 IPI OK; cada `cat /proc/smp` avanza el contador (1→2, 2→3). UEFI (OVMF) `-smp 2`: 4/4.
  - **Bug encontrado en el cargador UEFI** (autorizado por el usuario, cambio mínimo): `BI_TOTAL_SIZE` y
    `BOOTINFO_SIZE` eran 0x1A00 == `BI_FW_OFF`, así que `bootinfo::firmware_tables` descartaba siempre las
    tablas (RSDP) → 0x1B00. Sin ese arreglo Portix bajo UEFI nunca había visto la RSDP.
  - Runner e `interactive_runner`: `-smp` = `PORTIX_SMP` (por defecto 2); `uefi_probe.py` con `-smp 2` y
    4.ª comprobación (AP en línea). Pruebas nuevas en la suite (sección T-013): MADT = N, en línea = N,
    planificables = 1, AP ve su APIC ID, IPI 1→2 y 2→3, sin `FALLO`.
  - Suite 74a (`-smp 2`): 3743/3745; los 2 fallos eran `test_posix` 2707/2715, que esperaban leer 0x03 tras
    `sched_setaffinity({0,1})` porque el SMP falso fingía 4 CPU. Corregidos a la semántica Linux (la máscara
    se recorta a las CPU en línea; en E3 con `nproc ≥ 2` volverán a ver 0x03). Recert 113 MASTER-PASS.
- **E3 autorizada por el usuario (27-10-2026)**: puede modificarse código EXISTENTE de `isr.asm` (swapgs
  por-CPU en `syscall_entry`, stubs ISR/IRQ, `iretq`) y la GDT/TSS de `idt.rs`, con copia del original, diff
  acotado a macros de entrada/salida y la suite como red (dos verdes + recert por etapa). **Por etapas**:
  3a por-CPU + swapgs + consola con cerrojo (sigue planificando 1 CPU); 3b LAPIC timer y colas por núcleo
  (procesos en los AP); 3c `nproc ≥ 2`, afinidad real, medición del paralelo.
- **Entrega 3a (27-10-2026, b195) — estado por CPU + `swapgs` + consola con cerrojo** (CERRADA: suites 78a/78b 3751/3751 + recert 116 MASTER-PASS):
  - `kernel/src/arch/percpu.rs` (nuevo): bloque `PerCpu` `#[repr(C)]` por núcleo (`+0` puntero propio,
    `+8` cima de pila de núcleo del proceso en ejecución, `+16` RSP de usuario de la syscall, índice,
    APIC ID, `pid_actual`, `preempt_ok`). `GS_BASE` = bloque de la CPU en Ring 0, `KERNEL_GS_BASE` = 0
    (usuario). `percpu::instalar()` en el BSP tras `init_idt` y en cada AP en `ap_entrada_rust`.
  - `isr.asm` (zona protegida, autorizada): macros `SWAPGS_ENTRADA n` (swapgs sólo si el CS del frame es de
    Ring 3) en ISR_NOERR/FATAL/ERR, irq0/1/12/14/15, int80 e IPI_STUB; `SWAPGS_SALIDA` (`cli` + swapgs
    condicional por el CS del frame) antes de los 14 `POP_REGS; iretq`, incluido `switch_to_process_asm`;
    `syscall_entry` usa `[gs:16]`/`[gs:8]` en vez de las globales `syscall_user_rsp`/`current_kstack_top`.
    `ring3.rs` hace `swapgs` antes del primer `iretq`. `isr_handlers.rs` ya no sale a Ring 3 con un `iretq`
    crudo (pasa por `switch_to_process_asm`). `FMASK` enmascara también TF/DF/NT/RF (con TF activo el #DB
    saltaba antes del `swapgs` de `syscall_entry`).
  - `CURRENT_PID` y `KERNEL_PREEMPT_OK` globales → por CPU (`percpu::pid_actual()` / `preempt_ok`);
    `current_process_ref()` busca primero el PID de ESTA CPU. `set_tss_rsp0` escribe además `gs:8`.
  - `drivers/serial`: cerrojo entre CPUs con dueño = índice de CPU (reentrante en la misma CPU, tope de espera
    para no colgar la consola). `/proc/smp` añade `gs_invalido` (lecturas de `percpu::actual()` sin GS
    válido: 31 fijas del arranque temprano; si crece, el invariante swapgs está roto) y `cpu_actual`.
  - **Hallazgo (bug latente desde siempre)**: `enter_ring3_asm` "retornaba" al invocador (`process_exit_trampoline`
    → `jmp ret_addr`) con RBX/RBP/R12-R15 basura; el compilador los da por conservados. Se veía como
    `[OK] Proceso PID 17294103305076670291 finalizado` y, al cambiar la asignación de registros con E3a, como
    reinicios (triple fallo) al lanzar el siguiente binario (`httpd`, `ar`). Ahora los seis se salvan en la
    pila del invocador y la vuelta pasa por `.volver` que los restaura. `headless_runner` vuelca el log serie
    en `build/headless_serial_fallo.log` en el primer fallo (diagnóstico sin esperar al cierre).
  - **Hallazgo 2 (bug latente destapado por el PID por CPU)**: las syscalls que ceden la CPU (`sleep`, `yield`,
    `futex`, `waitpid`… 9 puntos) llamaban a `schedule_tick` con el frame de la syscall y CS de usuario; dentro,
    `deliver_pending(frame, None)` tomaba el RAX del frame (= NÚMERO de syscall) como RAX del usuario. Lo tapaba
    `current_process_ref()` (con el proceso en `Blocked` devolvía `None`); al buscar por PID se entregó la señal
    ahí y `time.sleep` interrumpido por SIGALRM devolvía 230 (`clock_nanosleep`) → `OSError [Errno -230]` en la
    sonda T-009 de CPython. Arreglo: `schedule_desde_syscall()` (indicador `PerCpu.en_syscall`) sin entrega de
    señales; las entrega el retorno de la syscall con `Some(resultado)`. Reproducido y verificado con
    `scripts/sondas` ad hoc (a1/a2) y T-009 completo en verde.
  - **Hallazgo 3**: `enter_ring3_setup` hacía `swapgs; iretq` con IF=1: una IRQ0 en esa ventana corría el núcleo
    con el GS del usuario (contador `gs_invalido` +2 por el primer `run`). `cli` antes del `swapgs`.
  - **Hallazgo 4 (la prueba falló en la suite 77a: `gs_invalido: 46`)**: la puerta IDT 0x80 era de *trampa*
    (0xEF, IF=1 al entrar). Una IRQ0 podía entregarse entre el `int 0x80` del usuario y la primera instrucción
    de `int80_handler` (su `swapgs` condicional): la IRQ veía CS=0x08 en la pila, no intercambiaba GS y
    `schedule_tick` corría con el GS del usuario; al volver, `int80_handler` hacía su `swapgs` y el estado
    quedaba otra vez coherente (por eso era esporádico y "se curaba solo": ~1 por cada 100 ms de proceso con
    `int 0x80`, y nunca con binarios glibc/musl que usan `syscall`). Localizado con un `rdmsr GS_BASE` en
    `schedule_tick`: `[GS!IRQ0] cs=0x08 rip=int80_handler+0 rsp=top-0x28`. Corregido: puerta de interrupción
    (0xEE) y `sti` justo después del `swapgs` de `int80_handler`: la ABI nativa siempre corrió con IF=1 dentro
    de la syscall (la recert 116 lo demostró: `certify` espera ticks del PIT dentro de `int 0x80` → 15/16).
  - **Prueba que puede fallar**: `/proc/smp` → `gs_invalido: N` (lecturas de `percpu::actual()` con GS que no
    apunta a ningún bloque tras `instalar()`); la suite exige `gs_invalido: 0 ` en la sección T-025 y al final
    de toda la pasada. El patrón de `APKINDEX de main` ya no fija el tamaño del índice de Alpine (cambia
    upstream: 499737 → 499754 bytes); sigue exigiendo la firma RSA verificada.
- **Entrega 3b (27-10-2026, b196) — TSS/GDT por CPU, temporizador LAPIC por AP, más estado por CPU** (CERRADA: suites 79a/79b 3754/3754 + recert 117 MASTER-PASS):
  - `idt.rs`: `TSS[N]`/`GDT[N]`/`GDT_PTR[N]` (una por CPU; los selectores 0x08..0x20 iguales, el 0x28 apunta
    a la TSS de esa CPU). `cargar_tablas_ap(idx, rsp0)` prepara la TSS del AP (IST1 = pila de #DF propia,
    del montículo; RSP0 = cima de su pila) y carga `tr`: antes los AP **no cargaban TSS** (una IRQ desde
    Ring 3 o un #DF en un AP habría sido fatal). `get_tss_ptr()` devuelve la TSS de la CPU actual, así
    `set_tss_rsp0` ya es correcto por CPU.
  - `percpu.rs`: `exception_cs` (+48) y `ring3_ret_rsp` (+56) pasan de `static mut` globales a `PerCpu`
    (las macros ISR_* y `enter_ring3_asm` escriben por `gs:`); `ticks_lapic` (+64). Desplazamientos
    comprobados en compilación (`offset_of!`).
  - `smp.rs`: cada AP calibra su temporizador LAPIC contra el TSC (50 ms, divisor 16) y lo programa
    periódico a 100 Hz por el vector 0xF3 (`ipi_stub_f3`, nuevo en `isr.asm`); el tick incrementa
    `PerCpu.ticks_lapic` del AP. El BSP sigue con el PIT (reloj de planificación sin cambios en 3b).
  - `/proc/smp` por núcleo: `tss 0x…` (dirección de su TSS), `tr 0x28 OK|FALLO` (**`str` ejecutado en el
    propio AP** por `ejecutar_en_ap`), `ticks_lapic N pit M periodo P reloj OK|FALLO` (relación LAPIC/PIT
    desde el arranque del AP en [0,85, 1,15]; `pit < 50` → FALLO para que no cuente una lectura prematura).
  - **Pruebas que pueden fallar** (suite, sección T-025): cada AP `tr 0x28 OK`; `n_smp` TSS distintas;
    `reloj OK` en todos los AP. **Prueba negativa hecha**: sin el `ltr` del AP → `tr 0x0 FALLO` (y revertido).
  - **Hallazgo 5 (protocolo de arranque)**: 16 pilas estáticas de #DF (256 KiB) llevaron `__bss_end` de
    ~0x5DC000 a 0x61C000 físico y pisaron el `bootinfo` que el cargador deja en **0x600000**: el asignador
    de marcos arrancó con **0 marcos** (`smp_estres` 0/4000 en todos los núcleos). Las pilas de los AP
    pasan al montículo (margen actual 129 KiB) y `build.py` **falla la construcción** si `__bss_end`
    queda a < 64 KiB del `bootinfo`. Mover el `bootinfo` exige tocar `stage2.asm`/EFI (protegidos):
    pendiente de autorización si el núcleo sigue creciendo.
  - Fuera de 3b (→ 3c): `crash_frame` sigue global (sólo diagnóstico, y el `RDI` original de ISR_ERR;
    importa cuando dos CPUs fallen a la vez, es decir, cuando los AP ejecuten Ring 3); `TTY_RX`, búferes
    de teclado/ratón, `DIRFD_BASE`, dns/dhcp son estado de dispositivo (cerrojo, no por CPU).
- **Cierre del Hallazgo 5 (28-10-2026, suite 80a 3754/3754 + `uefi_probe` 4/4)**: con autorización del usuario se tocó
  `stage2.asm`: `bootinfo` BIOS en **0xE00000** y pila de arranque en 0xF00000; el cargador EFI deja elegir al
  firmware (< 16 MiB) con fallback y nunca retorna sin él. Mapa físico bajo documentado en AGENTS.md (83ª).
- **Entrega 3c (28-10-2026, b197) — procesos en los AP y paralelo medido** (CERRADA: suites 81a/81b 3766/3766 + recert 118 MASTER-PASS):
  - `pick_next_ready` por CPU (afinidad, AP sólo procesos con padre/hilo y sin continuación de consola, nunca
    el actual de otra CPU); `preparar_cpu_para`; `crash_frame` por CPU; `smp on|off|estado` (off por defecto:
    `nproc=1`); `sys_getcpu` real; afinidad por defecto 0 = cualquier CPU; `current_raw()` sin `try_lock`.
  - `/bin/paralelo`: serie vs N hijos, resultados verificados (esperados calculados antes del reloj).
    **Medido `-smp 2`**: off → 1,00; on → 1,37-2,00 (5 pasadas), hijo en `cpu=1`, `gs_invalido: 0`.
  - Suite +8 pruebas (control negativo ≤ 1,25; paralelo ≥ 1,25; CPU 1 vista; contadores del AP; `smp off`
    restaura `nproc=1`). Defectos cazados por la medición: afinidad=1 heredada, `run` tomado por el AP antes de
    `enter_ring3` (#GP), `current_raw` con `try_lock` (hijo "reanudado" tras morir).
  - **Hecho en E3d (b198)**: SMP on en toda la suite; hilos en paralelo con pruebas de carrera; red/ext2/tty
    desde un AP con cerrojos reales; `/proc/smp` y `tr_en_ap` dependen del bucle ocioso del AP.
- **85ª parte (29-10-2026, b199, CERRADA) — SMP encendido por defecto y el reloj que se atrasaba**:
  - `SMP_PLANIFICAR` arranca en `true` (build 71): `smp estado` da `planificacion_en_ap=on nproc=2` y
    `/bin/nproc` ve 2 CPU **sin que nadie mande `smp on`** (prueba nueva; la build 70 la falla). El runner ya
    no envía `smp on`; `PORTIX_SMP_ON=0` lo apaga para una pasada de control en un núcleo. `smp off` sigue
    siendo el control negativo de E3c (`nproc=1`).
  - TLB shootdown: tras los 10 ms se espera hasta 500 ms más y se distingue **tarde** (`acuses_tardios`,
    `peor_acuse_us`) de **nunca** (`sin_acuse`). Prueba nueva: `sin_acuse: 0` al final de E3c. En los `--solo`:
    0 tardíos, peor acuse 350 µs (los 5 de 1,6 M de la 87b eran de >10 ms bajo TCG, no IPIs perdidos: por medir
    en la suite completa).
  - **Defecto 20 (medido con tres relojes, T-011)**: la prueba E3b comparaba el LAPIC del AP con el PIT del BSP y
    fallaba a ratos con una relación 1,16. Se añadió el TSC como tercera referencia (`tsc`, `pit_perdidos` en
    `/proc/smp`) y salieron **dos** causas: (a) `tsc_hz_estimate` empezaba a contar en un punto cualquiera
    del periodo del PIT → hasta un 10 % de error sistemático heredado por `esperar_us`, el temporizador LAPIC
    de los AP y la propia medida; ahora espera un flanco y mide 20 ticks (LAPIC a < 2 % del TSC). (b) **El BSP
    pierde interrupciones del PIT** (7-8 % con `-smp 2`): toda syscall entra con IF=0 (FMASK) y los caminos
    largos del núcleo (cargar un ELF desde ext2 al hacer `run`/`execve`: huecos de 265-910 ms; `exit_group`
    de procesos grandes) no las dejan pasar. Como CLOCK_MONOTONIC/REALTIME salen de `TICKS`, **el reloj del
    sistema se atrasaba un 7-8 %**. Hecho: `pit_tick` mide cada hueco con el TSC, **añade los ticks perdidos**
    (reloj a < 0,3 % del TSC) y publica `irq0_perdidos`, `irq0_peor_hueco_us`, `irq0_sitio` (pid, syscall,
    RIP interrumpido). `free_user_heap_pages` pasó a recorrido disperso (no `translate` página a página sobre
    cientos de MB de huecos) con `respirar()` cada 256 páginas. **Abierto → T-026**: que el núcleo deje pasar
    interrupciones en sus caminos largos (exec, E/S de disco); la prueba será `irq0_peor_hueco_us` acotado.
  - Veredicto E3b ahora: LAPIC frente al TSC (±15 %), y `pit_perdidos`/`irq0_*` se imprimen como medida.
  - **Defecto 21 (suite 88a, 3770/3771)**: `println!` de `portix-rt` hacía un `write(2)` por trozo del formato;
    con curl en una CPU y httpd en la otra, la línea `[CURL] cuerpo completo: 65536/65536 bytes` salió partida
    por la serie (la prueba la busca entera). Ahora cada `print!`/`eprint!` compone la línea en un búfer de
    1 KiB y la emite con una sola escritura (como el stdio de la libc). Build 76; suites 89a/89b.
  - Suites 89a y 89b (build 76, SMP por defecto): **3771/3771 y 3771/3771**; recert 122 **FALLO** sólo en la
    sonda `dbg` (56/62): "el objetivo no se detuvo en el breakpoint" (1 de 4 pasadas).
  - **Defecto 22 (sonda dbg, recert 122)**: la parada de `execve` de un proceso trazado y la parada por
    breakpoint hacían `switch_away()`; en un AP sin nadie listo (el trazador corre en el BSP) devolvía 0 y
    el hijo **seguía ejecutando en Ring 3 con estado `Stopped`** (el `[EXEC-STOP]` iba seguido del
    "Hello" del objetivo); en el #BP el AP se iba a `halt_loop` (`cli;hlt`) para siempre. Nuevo
    `process::esperar_mientras_detenido()`: cede la CPU si hay alguien listo y, si no, espera en `hlt` con el
    cerrojo grande suelto hasta que el trazador/SIGCONT lo saque de `Stopped`, conservando `en_cpu` para que
    ninguna otra CPU lo reanude por su cuenta. Build 77: sonda dbg 9/9 válidas (una pasada en MODO DEGRADADO por
    "ATA ningun drive detectado", el intermitente de T-015/T-016, ajeno), con un `[TRAP-STOP]` WATCHPOINT
    atendido en la CPU 1. Suites 90a/90b + recert 123.
  - **CIERRE b199 (build 77)**: suites 90a y 90b **3771/3771 consecutivas** (0 `[EXCEPTION-K]`/`[DOBLE]`/
    `[VOLCADO-SMP]`) + recert 123 **MASTER-PASS** (dbg 62/62). Medido al final de la 90b: 1 587 924 TLB
    shootdowns, `sin_acuse: 0`, 2 tardíos (peor 19 ms); `irq0_perdidos: 14581`, **peor hueco 21,7 s** con el
    BSP en el `mmap` (syscall 9) de la JVM (pid 855): el mapeo ansioso de `modules` (134 MB, deuda T-014) lee
    el fichero entero de ext2 con IF=0. Va a T-026 como caso principal.
- **Entrega 3d (28-10-2026, CERRADA b198) — la suite completa con `smp on` (`PORTIX_SMP_ON=1`)**. Defectos reales
  cazados por correr la suite con dos CPUs (cada uno con su prueba que fallaba antes y pasa después):
  1. Los AP no tenían los MSR de `syscall` (STAR/LSTAR/SFMASK) → `[GS!]` RIP=3 (`idt.rs::configurar_msrs_syscall` en AP).
  2. `execve`/VFS concurrentes corrompían estado → **cerrojo grande del núcleo** (`arch/bkl.rs`, reentrante por CPU,
     se suelta al conmutar y al esperar; `tomar` abre IF durante el giro para no bloquear el acuse de TLB).
  3. `pick_next_ready` elegía el mismo proceso en dos CPUs → `Running` bajo el cerrojo de la tabla.
  4. `ring3_exit_trampoline` (llamado con `call` desde Rust) dejaba `rsp ≡ 0 mod 16` → `movdqa` #GP en
     `net::poll→handle_ipv6` desde el bucle de espera de salida (bug latente desde siempre) → `and rsp,-16`.
  5. **EOF falso en tuberías**: las esperas bloqueantes (pipe/socket/eventfd/inotify) tenían un tope de 10 s; con
     dos CPUs el `execve` de cc1 (38 MB bajo el cerrojo grande) tardaba más y `gcc -v | grep | head` cortaba a
     cc1 con SIGPIPE. Tope único `TOPE_BLOQUEO_TICKS` = 10 min (red contra cuelgue, no semántica).
  6. **Triple fallo tras `FORK`**: la CPU que salía de un proceso seguía sobre el CR3 del muerto mientras el otro
     núcleo lo recogía (`waitpid` libera tablas) y hacía `fork` (reutiliza esos marcos) → PML4 basura → reinicio.
     Ahora `process_exit_trampoline` y `kill_current_process` vuelven a `KERNEL_CR3` al abandonar un contexto.
  7. **Pila del núcleo liberada bajo los pies de otra CPU**: la ranura "pila huérfana" era global (un hilo que moría
     en el AP veía su kernel stack liberada por el siguiente exit en el BSP mientras aún la recorría) y el padre que
     recogía un zombi desde otra CPU liberaba la pila sobre la que el hijo aún ejecutaba el trampolín de salida →
     montículo/tablas corruptos (framebuffer "desaparecido" tras `run java`). Ahora la CPU que sale **se queda con
     su pila** (`diferir_pila_propia`: la retira del `Process` y la deja en la ranura diferida **de esa CPU**, que se
     libera en su siguiente salida, ya sobre otra pila) y abandona las tablas del proceso ANTES de marcar Zombie.
  8. **Matar a un hilo que corre en otra CPU** (`exit_group`, señal fatal del grupo): `process_exit` directo desmontaba
     un contexto en uso. Ahora `terminar_hilo_del_grupo`: si la víctima corre en otra CPU, SIGKILL pendiente + IPI y
     la mata su propia CPU; si está guardada (o ya es Zombie), se desmonta en el acto.
  9. `hilos_del_grupo`/`tids_del_grupo` usaban `try_lock` → con contención real "0 hilos" y el líder cerraba el proceso
     con hilos vivos (T-005 `pthread_exit` en main). Cerrojo real.
  10. Volver al PML4 maestro al abandonar un proceso lo mutaba (`ensure_or_create` clona "la tabla heredada del
      maestro" sobre sí mismo) → `paging::cr3_consola()`: copia del maestro creada una vez y nunca liberada.
  11. `set_current` (consola `run`) degradaba a `Ready` TODOS los `Running`, incluido el proceso de la otra CPU
      (dos CPU sobre el mismo contexto). Ahora sólo el `pid_actual()` de esta CPU. Campo `en_cpu` en `Process`:
      `pick_next_ready` nunca elige un proceso que otra CPU tiene subido.
  12. **Pila de núcleo compartida por dos CPU** (`#UD` con RIP dentro de la pila, `fallo_java2/3/4.log`):
      `schedule_tick` limpiaba `en_cpu`/ponía `Ready` al saliente mientras la CPU seguía sobre SU pila (epílogo
      + `mov rsp` del stub). Si el saliente dormía en el núcleo (`wait_hlt_preemptible`), la otra CPU lo
      reanudaba y ambas escribían la misma pila. Ahora `percpu.saliente` + `liberar_contexto_saliente()`
      llamado por `isr.asm` (macro `LIBERAR_SALIENTE`, cinco puntos `mov rsp`) y por `ap_bucle_ocioso`: el
      saliente sólo se libera YA SOBRE la pila del entrante.
  13. `sys_sleep_switch` y la espera de stdin dormían con `sti; hlt; cli` **reteniendo el cerrojo grande**: un
      hilo dormido en el AP bloqueaba 200 ms toda syscall del BSP (T-005 "pthread_exit en main": el líder no
      podía ni hacer `exit`). Ahora `wait_hlt_preemptible()` (suelta y retoma el CGN).
  14. **#PF espurio** entregado como SIGSEGV: dos hilos tocan la misma página perezosa; el segundo espera el CGN y,
      al entrar, la PTE ya está presente → `materializar_perezosa` devolvía `false` → SIGSEGV a la JVM en
      `memset` (hs_err `libc.so.6+0xae5c9`). Ahora `paging::fallo_espurio` (PTE presente+USER+permiso →
      `invlpg` y reintento); contador `fallos_pf_espurios` en `/proc/smp` (4 en la sonda Java).
  15. **CR3 liberado bajo otra CPU** al `exit_group` de la JVM: el líder liberaba las tablas con un hilo
      (SIGKILL pendiente) aún en el AP → `#PF CR2=0xFEE000B0` en `ipi_handler_rust` ×17 000. (a) `process_exit`
      espera (CGN suelto, IF=1, ≤2 s) a que ninguna otra CPU tenga ese CR3; si no se va, tablas huérfanas trazadas
      (`[E3d] exit PID=…: otra CPU sigue sobre su CR3`). (b) `exit(2)`/`exit_group` desde un hilo secundario ya NO
      cierran al líder si está en otra CPU (`cerrar_lider_dormido` destruía su pila bajo sus pies: #UD RIP=3): le
      dejan `lider_salio`+código+SIGKILL+IPI y su propia CPU lo termina (su bucle "espera al último" lo comprueba).
  16. **`crash_frame` global** (suite 86a, NPE implícitas de la JVM en un núcleo y C1 fallando en el otro):
      el `CAPTURE_FRAME` de `isr.asm` escribía un único `static` y la señal de un hilo se entregaba con el RIP
      del otro (`SIGSEGV … RIP=0x300000080003` del hilo C1 entregado al hilo principal → salto a
      0x3000000010 → `stack smashing detected`). Ahora `PerCpu.crash_frame` (gs:104, `OFF_CRASH_FRAME`, con
      `const_assert` del desplazamiento); `frame()` lee el de ESTA CPU.
  17. `wake_blocked` trataba "objetivo a más de 10 s" como desbordado y despertaba al instante: `nanosleep(100 s)`
      volvía 0 y el `infinite_sleep` de la JVM (tras su error fatal) giraba a 29 MB de serie por minuto. Vencimiento
      con aritmética circular (`(now - objetivo) as i64 >= 0`).
  18. **Continuación de consola heredada en un AP** (suite 86b): `cerrar_lider_dormido` desde el hilo que hace
      `exit_group` en el AP heredaba `ring3_ret_addr` del líder lanzado con `run` y saltaba al llamador del BSP con
      RSP sobre `__stack_top` (#UD RIP=3, máquina muerta antes de test_posix). Ahora, si el proceso es de consola y
      la CPU es un AP, el líder queda `Ready` con SIGKILL y código: sólo el BSP puede elegirlo y lo termina él.
  19. Medida E3c "algún hijo terminó en la CPU 1" miraba sólo el `getcpu` final del hijo: con `smp on` el hijo
      migra al BSP al final y la prueba fallaba con aceleración real 1,55 (87a-prev). `/bin/paralelo` muestrea
      `getcpu` 64 veces en el bucle y publica `cpus_vistas=0x..`; el runner exige el bit 1 en la unión (T-011).
  - **E3d CERRADA (b198, build 70)**: suites 87a y 87b con `PORTIX_SMP_ON=1` **3769/3769 consecutivas** (0 ✗,
    0 `[EXCEPTION-K]`, 0 `[DOBLE]`, 0 `[VOLCADO-SMP]` en los dos logs serie) + recert 121 MASTER-PASS. Medido en
    la 87b: `tlb_shootdowns: 1645429 acuses: 1645424 sin_acuse: 5` (3 ppm, abierto menor); 0 `[EFAULT]` (el
    `posix_spawn: Bad address` no reapareció en 2 suites; sigue armada la traza). Quedan abiertos menores:
    `sin_acuse` residual, RSA httpd ~61 s con SMP, E3b `pit` congelado esporádico (medida), riesgo teórico del
    Zombie-en-trampolín cerrado desde otra CPU (no observado).
  - Suite 86a (build 68): 3765/3769 (T010 colgado por 16+17; sin `[EXCEPTION-K]`); 86b: muerta por 18 tras Java
    (Java 17/17 verde); recert 120 MASTER-PASS. test_posix 521/2707 adaptadas a `smp on` (máscara real, no {0}).
  - Resultado tras 11-15: `--solo "Ingest,T-025 E3d,T-005,Java"` con `smp on` → **141/143** (los 2 son artefactos
    L2/L2b del `--solo`), toda la sección Java verde, 0 `[EXCEPTION-K]` (`/tmp/solo_java6.log`).
  - **ABIERTO menor**: `sin_acuse: 3` de 2 511 988 TLB shootdowns en la sonda Java (0 antes de la JVM); medir en
    la suite completa. E3b `reloj FALLO` esporádico: `pit` no avanzó 150 ms entre dos `cat /proc/smp` (el BSP
    con IF=0 esperando una tarea del AP en `probar_ipi`/`tr_en_ap`): es la medida la que miente, no el LAPIC.
  - Diagnóstico nuevo: excepción en modo núcleo imprime `[EXCEPTION-K] motivo cpu pid RIP RSP EC CR3` antes de la
    pantalla de pánico (antes un AP moría en silencio y sólo "desaparecían" procesos).
  - Instrumentos: `^]` (0x1D) por el serie → `[VOLCADO-SMP]` (PID/última syscall por CPU, dueño del cerrojo,
    tabla de procesos); el runner lo envía al vencer un plazo. `SCHED[c]:` con el índice de CPU. `[EFAULT] nr pid cpu`.
  - **ABIERTO (intermitente, 1 de 4 pasadas)**: `posix_spawn: Bad address` al lanzar `as` desde gcc con el hijo
    del `clone(CLONE_VM|CLONE_VFORK)` en el BSP y el padre en el AP (el hijo salió 127 sin llegar a `[SYS] EXECVE`);
    la traza `[EFAULT]` queda armada para la próxima aparición. Hipótesis: el padre no se suspende con
    CLONE_VFORK (semántica de copia) y ambos corren a la vez por primera vez.
- **Entrega 2 (27-10-2026, b194) — locks + TLB shootdown + estrés `-smp 4`**:
  - `Spinlock::lock_irqsave()` (aditivo) para cerrojos compartidos con ISR. Auditoría: `PROCESS_TABLE`,
    `PIPE_TABLE`, `FIFOS`, `marcos::ESTADO` y el montículo ya eran `Spinlock`/irqsave atómicos (SMP-seguros).
    Los `static mut` restantes (`current_kstack_top`, `ring3_ret_rsp`, `TTY_RX`, `DIRFD_BASE`, búferes de
    teclado/ratón, `crash_frame`) son estado del BSP y deben pasar a **por-CPU en E3** antes de que un AP
    ejecute procesos. La consola serie no lleva cerrojo (sólo el BSP escribe).
  - Tareas de núcleo en los AP: `smp::ejecutar_en_ap(idx, fn, arg)` (IPI 0xF2 despierta; la tarea corre en
    el bucle ocioso, no en la ISR) + `esperar_tarea`. `/proc/smp_estres`: **todos los núcleos a la vez**
    (AP y BSP) hacen 4000 rondas de marco físico firmado/verificado + bloque del montículo firmado + contador
    bajo `Spinlock`; invariantes: contador = núcleos×4000, marcos libres iguales antes/después, 0 firmas
    pisadas, 0 AP sin respuesta, TLB shootdown acusado. `-smp 2`: 8000/8000 en 19 ms; `-smp 4`: 16000/16000
    en 55 ms. **Prueba negativa hecha**: con el incremento sin cerrojo salió `contador: 8247 esperado: 16000
    → FALLO` (la prueba puede fallar).
  - TLB shootdown real: IPI 0xF1 con **acuse** (`TLB_ACUSES`); `paging::flush_tlb()` lo difunde cuando el
    CR3 actual es el del núcleo y `flush_tlb_page()` cuando la página es de la mitad superior (espacio
    compartido). Los espacios de usuario no se difunden hasta E3 (máscara de CPUs por `mm`, cuando un AP
    pueda tenerlos cargados). `/proc/smp`: `tlb_shootdowns: 104 acuses: 104` (`-smp 2`) / `acuses: 312`
    (`-smp 4`), `sin_acuse: 0`.
  - Suite: +4 pruebas E2 (`resultado: OK`, N núcleos, contador exacto, TLB acusado). Sonda fuera de suite
    `scripts/sondas/sonda_smp4.sh` (5/5).
  - **E2 CERRADA (b194)**: suites 75a y 75b **3749/3749 consecutivas** + recert 115 MASTER-PASS.
  - **E1 CERRADA (b193)**: suites 74b y 74c **3745/3745 consecutivas** (`-smp 2`) + recert 114 MASTER-PASS
    (sonda UEFI 4/4 con AP en línea). Siguiente: E2.

### Criterio de cierre (acordado 27-10-2026, tras T-015/T-016)
- Un ticket se certifica sólo con **dos pasadas completas de la suite verdes y seguidas** más la recert.
  Un intermitente en cualquiera de las dos pasadas abre ticket y bloquea el cierre.

### T-017 · `execve` desde un hilo secundario  — CERRADO (88ª, b202) · **reabierto como intermitente de plazo (1-11-2026, suite #1 de `90569def…`)**
- Linux: `execve` en cualquier hilo mata a los demás y el que llama pasa a ser el líder (tgid conservado).
- Hecho: `process::execve_desde_hilo` (ver AGENTS 88ª): termina a los demás hilos con `salir_solo`, espera a
  que sus CPUs los suelten, intercambia identidad con el líder, espera `cr3_en_otra_cpu` antes de liberar el
  espacio antiguo; si ocurre en un AP y hereda la continuación de consola, el BSP reanuda el programa nuevo.
  `/proc/<pid>/status Threads:` real; `getcpu(2)` real.
- Prueba (runner, sección T-017, sonda `scripts/sondas/t017_exec_hilo.c`): hilo atado a la CPU 1 hace `execve`
  con dos hilos ocupados vivos; el hijo exige `getpid()==PID original`, `gettid()==getpid()`, `tgkill(tid viejo)`
  → ESRCH, `Threads: 1` y crear un hilo nuevo; el runner exige `(2 hilos terminados)` y el traspaso al BSP.
- Límite conocido: si el líder sigue en otra CPU tras 2 s (o quedan hilos vivos) `execve` devuelve EAGAIN.

- **[1-11-2026] Intermitencia de plazo, no de comportamiento**: en la **suite #1** de `90569def…` el fixture
  `t017_exec_hilo` agotó el plazo de la sonda —`[DEBUG FAIL TIEMPO]: 61,1 s de pared esperando (plazo 60 s),
  16 635 bytes de serie recibidos`, con `run /usr/local/t017` visible y `[ELF] loading /usr/local/t017`
  cortado— y sus **4 comprobaciones** cayeron. Las comprobaciones del **núcleo** del mismo bloque sí pasaron
  (el execve lo hace el hilo secundario, el llamador hereda la identidad, con 2 CPUs ocurre en el AP,
  exactamente 2 hilos mueren antes de cargar la imagen nueva, ninguna syscall sin traducir). En las suites
  **#2 y #3 de la misma imagen** el mismo bloque pasó **3829/3829**. Conclusión: el fixture no llegó a
  imprimir su veredicto dentro de 60 s bajo la carga de arranque de la pila de red (la serie de esos
  segundos lleva `[NET] WireGuard wg0 online` y `[DHCP] Broadcasting…`), no que `execve` desde un hilo
  se rompiera. **Pendiente en el frente (B) de intermitentes**: dar margen al fixture (o esperar su marca
  final en vez de un tiempo fijo) sin aflojar lo que se afirma.

### T-018 · `FUTEX_WAKE_OP` y futex PI  — CERRADO (88ª, b202: suites 96a/96b 3804/3804 + recert 127)
- Hecho: `FUTEX_WAKE_OP` (SET/ADD/OR/ANDN/XOR, EQ/NE/LT/LE/GT/GE, OPARG_SHIFT), `FUTEX_LOCK_PI`/`LOCK_PI2`/
  `TRYLOCK_PI`/`UNLOCK_PI` con tid del dueño, `FUTEX_WAITERS` y traspaso de propiedad al desbloquear
  (`futex::elegir_pi` + `listo_pi`), EDEADLK/EAGAIN/EPERM como Linux. Sin herencia de prioridad efectiva (el
  planificador no tiene prioridades) ni `FUTEX_OWNER_DIED`/robustez PI.
- Prueba: sonda `scripts/sondas/t018_futex_pi.c` (6 comprobaciones con valores exactos) + `[FUTEX] UNLOCK_PI
  traspaso` en la serie. Medido: 50 s en Portix frente a 1,7 s en Linux (traspaso = bloqueo + reinicio + planificador).
- Defecto 25 encontrado por esta prueba (pila de kernel liberada bajo los pies del trampolín): corregido (AGENTS 88ª).

### T-019 · Topes de seguridad de 10 s en tuberías y FIFO  — PENDIENTE (puede esconder fallos)
- `read`/`write` de tubería y `fifo_open` abandonan a los 10 s devolviendo EOF/cuenta corta: un bloqueo
  real se disfraza de resultado válido (sospechoso también en T-015). Sustituir por bloqueo verdadero
  (despertar por evento: escritura, cierre, señal) sin tope; prueba: `sleep 15 | cat` debe producir EOF a
  los 15 s, no a los 10, y `cat fifo` sin escritor debe seguir bloqueado tras 20 s hasta recibir SIGINT.

### T-020 · Ruido de `[EXCEPTION] #PF`+`[SIG]` por cada NPE de la JVM  — **CERRADO (b201, AGENTS 87ª; suites 94a/94b + recert 126)**
- Hecho: `[EXCEPTION] #PF` sólo cuando no hay manejador; COW/pila/swap en una línea con su `#PF CR2=`; señales con
  manejador contadas por proceso (3 primeras en el serie, total al salir: `senales_por_excepcion=N`). Pruebas: 0 líneas
  `[EXCEPTION] #PF` en la sonda Java y ≥ 1 000 señales declaradas al salir.
- 7691 líneas en una sonda de 54 s. Las excepciones entregadas al usuario como señal con manejador
  instalado no son anomalías: bajar a una línea resumida por proceso al salir (`señales entregadas: n`),
  manteniendo la línea completa cuando no hay manejador (muerte real). El escáner de la suite debe seguir
  fallando ante un `#PF` sin `[SIG]`/`[COW]`/demanda.

### T-021 · `pkg` sin control de espacio en disco  — PENDIENTE
- El ENOSPC de Java apareció como `[CURL-FAIL] errno 28`. `pkg` debe calcular `Installed-Size` + tamaño
  del .deb/.apk, compararlo con `statfs` de la raíz y abortar ANTES de descargar con un mensaje claro
  (`faltan N MB en /`); tras instalar, verificar que no quedan .deb en la caché. Prueba: raíz casi llena
  (fichero de relleno) → `pkg deb-add hello` falla con el mensaje y sin tocar el disco.

### T-022 · El shell de Portix trunca órdenes de más de ~250 caracteres  — PENDIENTE
- Visto al enviar `sh -c '...'` largo: la línea se cortó en silencio. Subir el búfer de línea (≥ 4 KiB) o,
  mejor, hacerlo dinámico, y rechazar con mensaje si se supera. Prueba: orden de 1000 caracteres cuyo
  último argumento se imprime completo.

### T-023 · Trazas `[SYS]`/`[RAMFS]`/`SCHED:` siempre activas  — PENDIENTE
- Inflan el log serie a varios MB y ralentizan (puerto a 38400). Interruptor en tiempo de ejecución
  (`/proc/sys/portix/traza` o `STRACE_PREFIJO`) apagado por defecto; la suite lo enciende sólo en las
  secciones que lo necesitan. Prueba: arranque + `ls` producen < 200 líneas de serie.

### T-006 · Wayland en `portix-wm` + `/dev/dri` (llvmpipe)  — PENDIENTE
- Protocolo Wayland real (wl_shm primero), `xdg-shell`; después DRM/KMS.
- Prueba: un cliente Wayland de distro (p. ej. `weston-terminal` o un programa
  GTK4) abre ventana en el escritorio de Portix.

### T-007 · Wine sin modificar (único camino para `.exe`)  — CONDICIONADO a T-004/T-005/T-006
- Prueba: `notepad.exe` de Wine y después un `.exe` de terceros (7-Zip, PuTTY)
  verificado con el SHA-256 del fabricante. Sin HLE propio.

### T-008 · Drivers reiniciables en caliente (Ring 3)  — PENDIENTE (identidad)
- Mover primero el driver de red (virtio/e1000) a un proceso de usuario con Fast-IPC y acceso MMIO/IRQ delegado; después ATA y USB.
- Prueba: `kill` del proceso del driver de red con una descarga en curso → el supervisor lo relanza y `curl` vuelve a funcionar **sin reiniciar**; el núcleo no registra ningún fallo.

### T-009 · Actualización atómica A/B del sistema  — PENDIENTE (identidad)
- Dos raíces ext2 (A/B), selector de arranque transaccional en `/boot` (bandera + contador de intentos), vuelta atrás automática.
- Prueba: instalar una "actualización" con rootfs corrupto a propósito → la máquina arranca sola con la raíz anterior y lo informa; `e2fsck` del anfitrión confirma que A quedó intacta.

### T-010 · Imagen reproducible bit a bit  — PENDIENTE (identidad)
- Eliminar fuentes de no determinismo en `build.py` (mtimes → `SOURCE_DATE_EPOCH`, UUID/serial de FAT y ext2 fijos, orden de ficheros estable, `mke2fs -E hash_seed`/`-U`).
- Prueba: dos construcciones desde limpio dan el mismo SHA-256 de `portix.img` y `portix-uefi.img`; la recertificación lo exige.

### T-011 · Mediciones reales en lugar de cifras afirmadas  — **CERRADO (24-10-2026, build 174)**
- Nuevo `kernel/src/system/bench.rs`: `medir()` toma en ESTA máquina (a) arranque hasta escritorio = TSC en `present done` (`marcar_escritorio_listo()` en `main.rs`) / frecuencia calibrada con el PIT (incluye firmware porque el TSC cuenta desde el reset); (b) memoria física = reserva fija del núcleo (todo lo que hay bajo `MARCOS_BASE`: código, montículo de 64 MiB, back buffer) + marcos de usuario del asignador de marcos; (c) lectura ext2 = KiB/s leyendo entero `/bin/test_posix` por VFS→ext2→ATA con el TSC. La latencia de syscall la sigue midiendo `/bin/portix-bench` desde Ring 3 (el núcleo lo dice en vez de inventarla).
- El mando `bench` del núcleo, el del terminal de ventanas, el botón y la pestaña "Benchmarks" del Centro de Control muestran sólo esas medidas (`[BENCH-REAL] …`). Eliminados: "48 ns vs 92 ns Linux / 185 Win11", "468 ms", "14,2 MB 170x menor que Windows 11", "99,2 % nativo", "38 Gbps", "850k IOPS", "PipeWire 2,1 ms", y el `[PORTIX-BENCH-PASS]` que el núcleo imprimía sin medir nada.
- Prueba (`verificar_bench_real` en `headless_runner.py` e `interactive_runner.py`): ejecuta `bench` dos veces y exige las cuatro líneas, arranque entre 200 ms y 120 s, 0 < RAM usada < total, lectura > 0 bytes y entre 100 KiB/s y 4 GiB/s, y que las dos ejecuciones no sean idénticas (µs de lectura y RAM) — una constante cableada falla.
- Medido en QEMU (build 174): arranque ≈ 9–12 s (incluye SeaBIOS y el cargador), RAM 85 068 KiB de 524 120 KiB (84 992 fijos + 76 de usuario), ext2 ≈ 2,6–3,6 MiB/s (ATA PIO sin caché: motivo de T-001).
- Deuda hallada: los contadores por orden del buddy (`ALLOC_STATS.free_blocks`) no cuadran (el bloque extraído no se descuenta; la fusión suma en el orden original), así que los bytes ocupados del montículo **no se afirman**; se muestra sólo "asignaciones vivas" (allocs − frees). `allocator.rs` es zona protegida: corregirlo requiere autorización (anotado en deuda menor).

### Descartados (no prometer)
macOS (Mach-O/Cocoa), módulos `.ko` de Linux, systemd como PID 1, Docker
(namespaces), drivers/anticheat de Windows, FreeBSD/illumos salvo demanda.

---

### T-026 · El núcleo corre con IF=0 en caminos largos (exec, E/S de disco, exit de procesos grandes)  — **CERRADO (b200, AGENTS 86ª)**: suites 91a/91b 3776/3776 + recert 124 MASTER-PASS; peor hueco tras la JVM 94/58 ms (era 21,7 s), 119/71 ticks perdidos por pasada (eran 14 581); suite completa 27 min (eran 60-65)
- **Hecho en b200 (build 91)**: `respirar()` en ATA por sector, `sys_mmap` (todas las ramas), carga de ELF, `fork`,
  `mprotect_range`, `mark_range_cow`, `munmap`, `mprotect`, `madvise`; protegido por `IRQSAVE_PROF` (nunca abre IF
  dentro de un `lock_irqsave`). Defecto 23: la consola volvía de cada proceso con IF=0 (hueco de 914 ms entre dos
  `run`): `sti` en el bucle principal. Lote de TLB (un shootdown por `mmap`/`mprotect_range`, no por página; sólo
  donde no se liberan marcos). `memset`/`memcpy` del núcleo con `rep stosq`/`movsq` (eran bucles de un byte:
  78 000 ciclos por marco a cero). Medidas en `/proc/smp`: `respiros: …`, `marcos_perfil_mciclos`, `mmap_perfil_mciclos`.
- **Medido (`--solo`)**: antes de Java `irq0_perdidos=0` (b77: 275-335, peor 914 ms); tras la JVM peor hueco
  **77 ms** (b77: 21,7 s) y 48 perdidos (b77: ~3 400). Pruebas en la suite: < 200 ms / < 150 tras E3c y < 300 ms /
  < 600 tras Java. Cerrado con 91a/91b + recert 124.
- **Revisión b203 pendiente**: suite 4, build 113, incumplió ambos límites tras la JVM: peor hueco
  2804433 µs y 1823 IRQ perdidas; el peor intervalo entre respiros fue 1037601 µs en `ata.rs:539`.
  La suite 5 volvió a pasar (69134 µs, 294 IRQ). La causa de la anomalía no está demostrada;
  no atribuirla automáticamente al anfitrión ni relajar los umbrales. Logs completos en
  `docs/evidencias/b203/serie_suite113d.log.gz` y `suite113d.log.gz`.
- **Revisión b203 (ciclo T-041), tres pasadas más sobre imágenes distintas:**
  suite `t035g` (núcleo `5d287248`) **falló** con `irq0_perdidos=675` (`peor_hueco_us=193364`); suites 1 y 2 de
  este ciclo (imagen `fc20a5aa…`, núcleo `9f9ce890…`) **pasaron** con `413` (`peor_hueco_us=236948`) y `370`
  (`49744`). Cuatro medidas del mismo contador acumulado (294, 675, 413, 370) y una dispersión de casi 5× en el
  peor hueco entre dos ejecuciones del mismo binario dejan el umbral dentro de la banda de ruido de TCG.
  **Atribución**: sensibilidad a la carga y a la duración de los tramos con IF=0 de la JVM, no un camino de
  código concreto; no se cierra el ticket ni se relaja el umbral. Para decidir hace falta KVM o repetir la
  medida aislada (p. ej. `--solo T-025`) con varias réplicas. Logs: `t041-suite1-3814.log` y `t041-suite2-3814.log`.
- **Dato nuevo (ciclo T-042, suite con los arreglos de syscalls):** `irq0_perdidos=54` (**bien**, umbral 150)
  pero `peor_hueco_us=499004` (**499 ms**, umbral 200 ms) y el «sitio» medido es
  `pid=2 syscall=2147483648 rip=0xffffffff8038758c`. Ese número es `0x8000_0000`: el bit con que
  `syscall_dispatch` marca el ABI NATIVA (`anotar_syscall(num | 0x8000_0000, …)`). Es decir, el peor
  hueco del arranque se atribuye a la anotación de una syscall nativa de un proceso temprano, no a una
  llamada Linux concreta: el instrumentador publica un identificador sintético y eso impide culpar a un
  camino de código. Con `irq0_perdidos` holgadamente dentro del umbral y el peor hueco fuera, las dos
  métricas discrepan y apuntan a una espera larga sin ticks perdidos (posiblemente `hlt`/sondeo), no a
  un `cli` largo. **Siguiente paso concreto**: registrar el par (inicio, fin) de cada ventana con IF=0
  junto al RIP de entrada y salida, y correlacionar con el proceso 2 del arranque.
- **Ciclo T-042 · instrumentado y con el hueco explicado (imagen `a1b21105…`, `47ad7ad9…`)**:
  1. **Ventanas con IF=0 medidas de verdad** (`kernel/src/time/pit.rs`): `VentanaIf0` se abre en cada
     syscall **sólo si entra con IF=0** (la puerta `int 0x80` hace `sti` y corre con IF=1; la de
     `syscall` entra con IF=0 por FMASK `0x47700`, que sí limpia el bit 9). Publica
     `ventanas_if0: abiertas N peor_us U pid P abi A nr R a1 a2 a3 cpu C rip_in .. rip_out ..` y
     `irq0_hueco_dentro: 0|1` + `irq0_hueco_ventana_us` + `irq0_hueco_tick` en `/proc/smp`.
     Cada hueco nuevo ≥ 150 ms deja además una línea inmediata `[IRQ0-HUECO]` en la consola, para
     situarlo en el log sin esperar al cierre de la pasada.
  2. **El hueco de 499 ms de la pasada anterior ya tiene dueño**: el `rip` publicado
     `0xffffffff8038758c` **no era una anotación de syscall nativa**, era una dirección de código.
     Simbolizada contra `kernel/target/x86_64-portix/release/kernel` cae en `ipv6::ping6_rtt+844`,
     exactamente el `cli` inmediatamente posterior al `sti; hlt` de su bucle de espera (el volcado QMP
     de esa misma pasada da `RIP=0xffffffff8038758c … HLT=1`). Es decir: **el BSP estaba parado en
     `hlt` con IF=1**, no en un tramo `cli`. Un retraso de 499 ms en ese punto no lo puede causar el
     núcleo (con IF=1 cualquier IRQ entra): es el anfitrión sin dar CPU al vCPU (TCG en un sandbox
     compartido). El `syscall=2147483648` era un señuelo: `IRQ0_SITIO` mezclaba el pid actual con la
     última syscall de la CPU, de otro proceso y de otro momento.
  3. **Predicado corregido sin tocar el umbral**: siguen siendo 200 ms (y 300 ms tras la JVM), pero
     ahora se aplican a lo que el ticket dice medir. `dentro_if0=1` → el hueco cayó dentro de una
     syscall con IF=0: falla y se nombra la syscall. `dentro_if0=0` → el BSP no retenía IF=0: se
     informa como retraso del anfitrión, que es lo que demuestra la medida. Se añadió además una
     prueba de que la instrumentación está publicada (`ventanas_if0`), que puede fallar.
  4. **Defecto nuevo encontrado por la instrumentación (ticket aparte, T-043)**: un `mmap` de la ABI
     Linux que reserva **128 MB con `prot=0`** tarda **1,77 s y 1,89 s** de pared (uno de esos casos,
     `nr=9 a2=0x8000000`, pid 14/16). `ventana_peor_us` mide la llamada entera, no un tramo continuo:
     dentro del bucle hay `respirar()` cada 256 páginas, así que ése no era el hueco del BSP.
  5. **El hueco que sí era del núcleo, y su final**: en la suite completa de este ciclo la atribución
     nueva cazó el único caso real: `[IRQ0-HUECO] us=213019 perdidos=20 dentro_if0=1 pid=846 nr=9
     abi=linux ventana_us=241839`, y `respiros: … peor_tramo_us 212912 en mod.rs:3067` lo confirmó por
     otra vía. Era el bucle de `sys_mmap` invalidando la TLB **dos veces por página** con espera de
     acuse. Corregido en T-043 (lote de 128 páginas): en la suite #2, sobre la imagen con el arreglo,
     el peor hueco tras la JVM baja de **213 019 µs a 76 015 µs** y los ticks perdidos de **418 a 148**;
     el tramo continuo que publica `respiros` pasa de `212 912 µs en mod.rs:3067` (el `mmap`) a
     `76 005 µs en mod.rs:3309` (el `munmap`, ticket aparte).
  6. **La atribución se estrenó con un caso real (suite #3)**: `[IRQ0-HUECO] us=595688 perdidos=58
     dentro_if0=0 pid=0 nr=0 abi=nativa ventana_us=0` → ningún tramo con IF=0 abierto, `pid 0`: el BSP
     estaba en modo usuario cuando llegó el hueco de 595 ms. Antes esto era un fallo de la suite
     («peor hueco < 200 ms»); ahora se informa como retraso del anfitrión, sin tocar el umbral, y la
     comprobación pasa por la razón correcta. Ése es exactamente el valor del instrumento: separar
     «el núcleo retuvo las interrupciones» de «el anfitrión no dio CPU al vCPU».
  5. Sonda de la instrumentación: `scripts/sonda_t026_ventanas.py` (ejecuta un ELF Linux real porque
     el binario nativo entra por `int 0x80` con IF=1 y no abre ventana) y reproducción aislada del
     cuelgue de consola: `scripts/sonda_futex_lider.py`.
- Deuda: el `mmap` de fichero sigue siendo ansioso (T-014); `respirar()` no cubre `free_address_space` de un espacio
  con muchas tablas (ya tenía respiros cada 256 páginas) ni el bucle de `getdents`/ext2 largos; estudiar `sti` al
  entrar en syscalls (plan 2) sigue abierto.
- Medido en la 85ª parte (defecto 20): huecos del PIT de 265-910 ms en el BSP al cargar un ELF (`run`/`execve`),
  **21,7 s en el `mmap` ansioso de `modules` de la JVM (134 MB leídos de ext2 con IF=0; suite 90b)** y al terminar
  procesos grandes; `irq0_perdidos` ≈ 7-8 % de los ticks con `-smp 2`. El reloj ya no se atrasa
  (compensación por TSC), pero la latencia sigue: teclado, red, temporizadores y el tick de planificación no se
  atienden durante el hueco. Causa: FMASK deja IF=0 en toda syscall y el núcleo sólo abre interrupciones en
  puntos de espera explícitos (`sti;hlt;cli`).
- Plan: (1) `respirar()` en los bucles de lectura ATA/ext2 del cargador ELF y en `free_address_space`;
  (2) estudiar `sti` al entrar en syscalls que no tocan estado de IRQ (la mayoría), con `cli` sólo en las
  secciones que comparten datos con ISR. Zona delicada: `isr.asm` (protegida; sólo con autorización).
- Prueba que puede fallar: tras `run paralelo` y un `execve` de gcc, `irq0_peor_hueco_us` < 50 000 en `/proc/smp`
  (hoy 900 000); control negativo: con `respirar()` desactivado la cifra vuelve a subir.

### T-027 · Defecto 24: HotSpot aborta a ratos con SIGSEGV en la entrada de una función de libc (pila del hilo no escribible)  — **ABIERTO: dos hipótesis medidas y descartadas, una carrera endurecida (1-11-2026)**
- Visto 1 vez en 6 pasadas de Java de b201 (suite 93a: `pc=libc.so.6+0x154130` = `push rbp`, tid `sonda-1`, tras T010-4)
  y un cuelgue parecido (safepoint esperando a un hilo "Running") en la build 89 (`--solo`). Antes de b200 no se vio.
  Sospechosos: pilas de glibc reutilizadas (`madvise(MADV_DONTNEED)` → perezosas) + zonas de guarda de HotSpot
  (`mprotect` NONE/RW) + lote de TLB de `mprotect_range` (T-026) o reserva perezosa; nada demostrado.
- Instrumentación lista (b201): `[SIG] SIGSEGV … CR2= EC= PTE=` y volcado del `hs_err_pid*.log` por el runner si falta
  `[T010-PASS]`. Próximo fallo = diagnóstico. Prueba cuando se arregle: 10 pasadas de la sonda Java seguidas en verde.
- **(88ª) Pariente encontrado y corregido — defecto 26**: otra forma del mismo síntoma (abort de HotSpot por SIGSEGV)
  con causa distinta y ya cerrada: fallo de captación de instrucción espurio entre dos CPUs sobre una página de texto
  de `libjvm.so` recién materializada (`CR2 == RIP`, `EC=0x14`, `PTE` presente y ejecutable); `fallo_espurio` no
  reintentaba captaciones. Build 102. El defecto 24 original (`push %rbp`, fallo de ESCRITURA en la pila del hilo)
  sigue sin reproducirse desde que la línea `[SIG]` lleva CR2/EC/PTE; cuando vuelva, esa línea dirá si la PTE estaba
  presente (carrera entre CPUs, como el 26) o ausente/PROT_NONE (guardas de HotSpot sobre pila perezosa).

#### T-027 · qué se midió (1-11-2026)

- **Hipótesis 1 — «el ciclo pila + guarda + reciclaje rompe la escritura»: descartada.** Fixture nuevo
  `scripts/sondas/t027_pila_hilo.rs` + sonda `scripts/sonda_pila_t027.py`: reserva 1 MB `PROT_NONE`, hace
  escribible el tramo útil con `mprotect` (como `os::create_stack` del JVM), escribe en todas sus páginas,
  **baja hasta el fondo con marcos reales** (el `push rbp` que reventaba), recicla con
  `madvise(MADV_DONTNEED)` como glibc y vuelve a escribir. **200 vueltas, 2,1 s, cero SIGSEGV y cero
  escrituras perdidas.**
- **Hipótesis 2 — «carrera `mprotect` ↔ fallo de página en la misma PTE»: endurecida, NO reproducida.**
  El *read-modify-write* de la entrada no era atómico: `mprotect` en una CPU y la materialización de una
  página perezosa en otra podían escribir los dos desde una lectura vieja, y el segundo dejaba la página con
  la protección **anterior** (PROT_NONE incluido) — exactamente «pila del hilo no escribible» y SIGSEGV en
  el primer `push`. Arreglo aplicado en `kernel/src/mem/paging.rs`: cerrojo corto `CERROJO_PTE` para la
  sección crítica y, al materializar, **relectura de la protección vigente** (la entrada manda sobre la
  reserva; si otra CPU ya materializó, se libera el marco en vez de duplicarlo). Contador `pte: carreras`
  en `/proc/smp`.
- **Lo que NO se demuestra**: el estresor `t027_carrera` (dos hilos, 400 vueltas de `mprotect` contra
  escrituras concurrentes) **pasa sin un solo SIGSEGV pero con `carreras=0`**: no llegó a entrar en la
  ventana. Por tanto el arreglo es un **endurecimiento con causa demostrada en el código**, no una
  corrección reproducida. Se declara así para no vender lo que no se ha medido.
- **Siguiente paso para T-027**: reproducir el aborto con la JVM y el núcleo instrumentado (el contador
  `pte: carreras` y el log de `[SIG] SIGSEGV … CR2=…` dirán si el camino es este), o construir un
  estresor que sí entre en la ventana (mprotect de una región grande mientras otra CPU materializa el
  principio).

### T-028 · Defecto 34: `curl --con-servidor-local` (padre en `SYS_SLEEP 150 ticks`) no despierta mientras su hijo httpd espera en `accept` — **CAUSA MEDIDA Y CORREGIDA (1-11-2026)**
- Visto 1 vez (suite 113b, b203) en la prueba "HTTPS por bucle local IPv6". Traza real del runner (`[QMP PILA]`):
  CPU 0 en `hlt` dentro de `wait_hlt_preemptible` ← `tcp_wait_accept` ← `socket_accept_impl` ← `sys_accept4` (httpd PID 11,
  nativa 290 "dentro"); CPU 1 ociosa (pid 0). `[VOLCADO-SMP]`: `curl` PID 10 `Blocked cpu 0 sleep_until 1799` y así sigue
  144 s después (y `tcp_wait_accept`, con tope de 1000 ticks, tampoco vuelve): huele a que el BSP no avanza
  `wake_blocked`/`pit::ticks()` mientras duerme en ese bucle, o a que `preempt_ok` queda en falso y `schedule_tick`
  sale antes de `wake_blocked` (`!in_user && !kernel_preempt`). Nada demostrado. No se reproduce en la pasada siguiente.
- Próximo paso si vuelve: añadir a `[VOLCADO-SMP]` `pit::ticks()` y `preempt_ok` de cada CPU; comprobar si IRQ0 llega
  y si `wake_blocked` se ejecuta en ese tick. El volcado puede emitirse tanto desde el bombeo de TTY en el temporizador
  como desde `tty_read_byte`, de modo que su mera presencia no identifica por sí sola el camino que lo emitió.

#### T-028 · causa medida (1-11-2026)

- **La pista estaba en la propia traza**: `curl` seguía `Blocked cpu 0 sleep_until 1799` más de 144 s
  después, **y** `tcp_wait_accept` —que tiene su propio tope de 1000 ticks— tampoco volvió. Dos plazos
  independientes que no vencen a la vez no son dos bugs de dos suscriptores: es un solo reloj que no
  avanza, o un barrido que no corre.
- **Instrumentación puesta** (permanente, en `kernel/src/process/scheduler.rs`): contadores
  `WAKE_LLAMADAS`, `WAKE_SIN_CERROJO`, `WAKE_DESPERTADOS`, `ESPERA_ATASCADA`, `WAKE_PEOR_SEGUIDOS`,
  `WAKE_PEOR_HUECO_TICKS` y un vigilante `vigilar_reloj_de_esperas()` que se llama en cada tick **antes**
  del retorno temprano; todo publicado en `/proc/smp` en la línea `reloj: ticks … ultimo_barrido …
  sin_cerrojo … peor_seguidos … peor_hueco …`.
- **Medido con `scripts/sonda_reloj_t028.py` + `scripts/sondas/t028_reloj.rs`** (dos hijos durmiendo y el
  padre imprimiendo `ticks` cada 100 ms, con la hora la pone el anfitrión): sobre la imagen anterior,
  **150 ticks (1,5 s) seguidos sin un solo barrido de vencimientos** con procesos `Blocked` con plazo
  esperando (`peor hueco entre barridos = 150 ticks`, 987 barridos en la corrida, racha máxima sin cerrojo 2).
- **Causa**: `schedule_tick` volvía antes de `wake_blocked()` (`if !in_user && !kernel_preempt { return 0; }`)
  y el barrido estaba **detrás** de ese retorno. Mientras todas las CPUs estaban en el núcleo sin
  `preempt_ok` —el estado normal de un sistema ocioso— **nadie despertaba a los que dormían con plazo**.
  El que dormía sólo se salvaba si otra CPU entraba a modo usuario o activaba `preempt_ok`.
- **Arreglo**: el barrido de vencimientos corre en **cada** tick, antes del retorno, venga la CPU de modo
  usuario o esté ociosa (se retiró la llamada duplicada posterior). Despertar es barato y no cambia de
  contexto: sólo marca `Ready`.
- **A/B sobre la misma sonda**: peor hueco entre barridos **150 ticks → 1 tick** (1,5 s → 10 ms); barridos
  por corrida **987 → 1999**; racha máxima sin cerrojo 2 → 1; los mismos 31 despertares, ahora puntuales.
  Evidencia: `docs/evidencias/b203/t028-reloj-antes-despues.txt`.
- **Queda abierto, medido**: en 1 de cada 30 lecturas una espera de 100 ms no ve avanzar el reloj ni un
  tick; hay que separar «`nanosleep` vuelve antes de tiempo» de «la lectura cayó en el mismo tick». Y el
  invitado gana ~292 ticks en 4,16 s de pared del anfitrión, cifra que incluye el ida y vuelta de consola,
  así que **no se concluye** pérdida de tiempo.
- **Relación con los demás intermitentes**: T-015 (el `wait` del shell que no vuelve) y T-033 (`java
  -version` que a veces pasa de 120 s) comparten esta forma —una espera con plazo que no vence—, pero
  **no se cierran hasta reproducirlos** con este arreglo puesto.

### T-029 · Código C propio en el userland distribuido — **CERRADO (1-11-2026: dos suites 3843/3843 y recert 7/7 sobre `e890fc288b29e558…`)**
- La recert 128 invocó `_compile_c_userland` y desplegó `hello_c`/`calc_c` propios; la suite 5 los ejecutó. El
  constructor ya no llama esa función y `build_ring3.py` queda desactivado con error explícito. Las fuentes se conservan.
- Las dos suites sustituyen esas comprobaciones por el ELF original BusyBox de Alpine: exigen descarga HTTPS,
  SHA-256 completo e instalación, saludo y `45 + 55 = 100`, salida exacta y `code=0` del mismo PID. No se modifica
  el binario externo; ejecutar software Linux de terceros de cualquier lenguaje sigue siendo el objetivo.
- Local: 19/19 controles de `test_verificaciones_t029.py`, con negativos y series previas. Build 114 y
  `sonda_imagen_t029.py` 18/18 confirmaron P3 idéntica y ausencia de ambas utilidades, `/bin/doom` y compiladores
  enumerados; el analizador tiene 8/8 controles negativos/positivos. En QEMU `--solo T-029` pasaron descarga,
  SHA-256, instalación, saludo y aritmética del BusyBox real. La pasada parcial fue 13/16 por tres verificaciones
  de cierre cuyos prerrequisitos se omitieron; **no certifica**. Tras reconstruir el entorno, la nueva imagen BIOS
  volvió a pasar **18/18** controles offline de P3 e inodos (`t029-imagen-reconstruida.log`), incluida la ausencia
  de `/bin/doom` y los compiladores enumerados; no se borró su código fuente. Faltan suites completas y recert.

#### T-029 · cierre (1-11-2026)

- **Condición que faltaba**: «suites completas y recert». Cumplida sobre una imagen identificable
  (`build/dist/portix.img` **`e890fc288b29e558…`**, 2 186 280 960 B) **sin reconstruir entre las dos pasadas**:
  - suite #1: `TOTAL: 3843 | PASSED: 3843 | FAILED: 0`
  - suite #2: `TOTAL: 3843 | PASSED: 3843 | FAILED: 0`, cero líneas `[IRQ0-HUECO]`
  - `recertify.py --no-build`: **MASTER-PASS 7/7** (0 hallazgos, depurador 64/64, interactiva 128/128,
    Épica 58 completa, Épica 59 31/31, Épica 60 10/10, UEFI 4/4)
- **Lo que exige la suite y pasó**: descarga HTTPS del BusyBox de Alpine con SHA-256 completo e instalación;
  el ELF C externo imprime el saludo y sale 0 (PID 326); y calcula `45 + 55 = 100` y sale 0 (PID 327) — es
  decir, se ejecuta **software Linux de terceros sin modificar**, no código C propio compilado por el
  constructor (el constructor ya no llama `_compile_c_userland` y las fuentes se conservan sin desplegarse).
- Evidencias: `docs/evidencias/b203/suite-e890fc28-1.log`, `suite-e890fc28-2.log`,
  `recert-e890fc28-master-pass.log`.

### T-030 · Imágenes temporales fuera de `build/dist/` — **CERRADO (1-11-2026: auditoría + dos suites 3843/3843 y recert 7/7 sobre `e890fc288b29e558…`)**
- Las rutas antiguas de suite 6 no cumplían la instrucción. Migrados constructor, particiones/ESP y árboles ISO,
  copia RAW final, discos de silicio, ejecutores, sonda UEFI y sondas de recert a `build/dist/`. Base y copia final
  son distintos archivos. `PORTIX_CONSERVAR_IMG` rechaza rutas externas y archivos existentes; si falla conserva la
  copia privada. QEMU se detiene antes de copiar o borrar su disco. La ISO usa copia del disco de datos.
- `--clean --persistence` conserva el archivo persistente in situ sin cargarlo a RAM; disco nuevo disperso. Eliminada
  la limpieza automática de directorios ajenos en `/home`. `test_rutas_headless_b203.py` 10/10 y
  `test_rutas_build_b203.py` 8/8, con errores simulados, aislamiento, disco disperso de 8 GiB y árbol ISO simulado.
- Build 114 terminó en 151,3 s. Auditoría: P3 coincide con el ext2 temporal; no se hallaron imágenes fuera de dist.
  BIOS QEMU: 12/12 con dispositivos y 6/6 sin ellos (incluye rechazo de PASS sin hardware). UEFI OVMF 4/4.
  BIOS/UEFI conservaron sus SHA-256 tras las sondas. Un SIGINT real cerró QEMU y DNS propios antes del ajuste
  posterior del cierre HTTP; ese ajuste acotado aún no tiene prueba real de SIGINT. **Faltan suites completas
  y recert**; no declarar cerrado. No se han tocado zonas protegidas de arranque.

#### T-030 · cierre (1-11-2026)

- **Condición que faltaba**: «suites completas y recert». Cumplida con las mismas tres piezas de T-029
  (dos suite verdes consecutivas sobre `e890fc288b29e558…` sin reconstruir y recert `--no-build` **7/7**).
- **Lo que la corrida demuestra del cambio de rutas**, sin suponerlo: la ejecución completa se hizo con las
  imágenes **dentro de `build/dist/`** (`portix.img`, `portix-uefi.img` y las copias privadas en
  `build/dist/pruebas/run/`), y las sondas UEFI y BIOS trabajaron sobre **copias privadas**, dejando los
  SHA-256 de las dos imágenes intactos tras las pasadas.
- **Límite declarado**: la prueba con **SIGINT real** del cierre HTTP (el ajuste posterior de la build 114)
  sigue sin repetirse en este ciclo; no se incluye como demostrada. Si aparece una regresión de cierre, se
  anotará aquí.
- Evidencias: mismas tres de T-029.

### T-031 · Defecto 35: `exit_group` y SIGKILL con líder en espera — CERRADO EN SONDA Y EN LA SUITE (b203, ciclo T-042)
- **Historia**: la corrección experimental (kernels 115-118) pasó 10/10 en QEMU y Linux, pero nunca se
  integró en el flujo de cierre. El defecto 35 seguía bloqueando `rustc` de distro (salía con código 32
  cuando dos hilos cerraban el grupo en paralelo).
- **Hecho en este ciclo**:
  1. Sonda nueva `scripts/sonda_t031_b203.py`: compila `scripts/sondas/t034_exit_group.rs` con el rustc del
     anfitrión, la inyecta en una copia privada del ext2 y ejecuta los 6 modos. Exige para cada uno la línea
     EXACTA (`[T034-AP] cpu=1 exit_group=N`, `[T034-WAIT] … observado=10752 … OK`,
     `[T034-SIGKILL] observado=9 … OK`, `[T034-SIGKILL-HILOS] … sin_proceso=true OK`) **y el código de
     salida del PID que anunció `PROC: create`** (no el de un hilo ni el de un hijo asesinado), más un
     control negativo (modo inválido → código 2) y la comprobación de que la consola sigue lanzando procesos.
  2. Sección `T-031` en `scripts/headless_runner.py`, antes de la de C++: 11 comprobaciones, incluida la
     descarga del fixture por red y los dos controles negativos.
- **Resultado**: sonda **8/8** sobre la imagen reconstruida (`t031-sonda-8de8.log`); sección del runner
  **verde** en `--solo T-031` (`t031-suite-solo.log`; los 3 fallos restantes de esa pasada son de otras
  secciones con prerrequisitos omitidos por `--solo`, no de T-031).
- **Lo que desbloqueó**: `pkg deb-add rustc` + `rustc --version` + **`rustc` compilando y enlazando dentro
  de Portix** + el binario resultante ejecutándose con hilos, todo PASS (`t031-rustc-desbloqueado.log`).
  Medido: despliegue 751,6 s; `rustc hola.rs` 87,8 s.
- **Límite**: la sección no se ha ejecutado todavía dentro de una suite completa con recertificación; eso
  queda para el cierre del lote.

### T-032 · Detección de disco ATA falla durante el arranque — ABIERTO, CAUSA NO DEMOSTRADA
- En la primera sonda Java sobre kernel 118, QEMU arrancó BIOS pero el núcleo informó `[ INF ] ATA ningun drive
  detectado` y entró en modo degradado sin PID 1 (`sonda-java-reuso118.log`); no se ejecutó Java. En el único
  reintento la raíz sí montó y la clase se ejecutó. Las imágenes de entrada conservaron sus SHA-256. También
  arrancó una copia T-034 del mismo núcleo; esto no prueba que el problema se haya resuelto.
- Nueva sonda acotada `scripts/sonda_ata_b203.py`: cuatro arranques sin traza y cuatro con traza IDE sobre la
  misma copia BIOS de kernel 118 y Java fueron **8/8** (ATA primario + raíz ext2 + PID 1; 30 s/VM); original
  intacto. Tres trazas habilitaron por QMP las lecturas de estado alternativo durante el segundo scan y comprobaron
  su presencia. La traza y el código muestran dos resets `0x04` por canal en `HardwareInfo::detect_all()` más un
  reset `0x06` por canal en `AtaBus::scan()`. En los arranques correctos se vio estado alternativo `0x00` durante
  los resets y `0x50` tras seleccionar el primario: no prueba por qué falló el arranque anterior. Trazar también
  altera los tiempos. Registros y trazas íntegras comprimidas: `docs/evidencias/b203/t032-*`.
- Siguiente: si se reproduce, conservar la traza fallida y contrastar estados por etapa; estudiar la duplicación sin
  afirmar causalidad. No confundir arranques posteriores exitosos con reparación ni quitar la exigencia de raíz real.

### T-033 · `java -version` excede 120 s de forma variable — ABIERTO, CAUSA NO DEMOSTRADA
- Sobre build 114 excedió 120 s dos veces y una vez terminó durante la ventana diagnóstica. Con kernel experimental
  116 pasó en 46,4 s después de la clase. En kernel 118, tras la clase correcta en 78,5 s, no terminó dentro de
  120 s ni de 20 s diagnósticos; el volcado muestra líder Ready, hilos en mmap/futex y actividad posterior
  (`sonda-java-reuso118b.log`). La prueba terminó con fallo e información incompleta. No se demuestra un cuelgue
  permanente ni que se deba a las últimas modificaciones. El orden en 116/118 fue clase primero para recibir un
  resultado temprano; en la reproducción 114 era versión primero. No ampliar el límite sin autorización.
- **Versión aislada en kernel 118** (`t033-version-aislada118.log`): sin clase previa, superó los 120 s y terminó
  con versión real y PID 2 código 0 dentro de los 20 s diagnósticos: el FAIL por plazo permanece. A 30/60/90 s
  hubo progreso parcial; en el volcado intermedio aparecieron syscalls 202 y 11 por CPU. `tlb_shootdowns` pasó de
  102 a 1 687 740 (delta 1 687 638); páginas desde fichero 0→6077, mmap perezoso máximo 34389, ansioso 8. Tras
  la ejecución se midieron 219296 µs y 171 IRQ perdidas, dentro de los límites respectivos 300000 µs y 600; esta
  medición parcial no sustituye la de la suite. `reloj FALLO` persiste sin diagnóstico.
- **Microsonda real Rust** (`scripts/sondas/t033_tlb.rs`): reserva y libera 4096 páginas PROT_NONE, comprueba
  `mprotect` en tres direcciones antes y después (`ENOMEM=12`) y reutilización de una página. Con CPU 0 y sin
  hilo remoto se contaron **0/0** shootdowns para `mmap`/`munmap`; con un hilo fijado a CPU 1 **4104/4100**.
  Linux y QEMU aprobaron ambos modos, con controles negativos del predicado **13/13**. `mapear_perezosa` y
  `unmap_page` invocan `flush_tlb_page` por página; evidencia de un sobrecoste concreto, **no causalidad total del
  plazo de Java**. Logs `t033-tlb-anfitrion.log` y `t033-tlb-k118.log`. Los contadores incluyen las pequeñas
  llamadas auxiliares de lectura/protección; el tiempo por fase se midió en el anfitrión con sondeo de 50 ms.
- **Sonda SMP con páginas realmente presentes** (`t033b-smp-qemu-base.log`): CPU 1 leyó la página, CPU 0 la
  desmapeó, otra página distinta se materializó y la CPU 1 recibió `SIGSEGV`/`SEGV_MAPERR=1`; tras `MAP_FIXED`
  leyó cero y luego la nueva marca, también en un segundo remapeo directo. El control que omitió `munmap` produjo
  el fallo exacto previsto. En la imagen BIOS reconstruida (SHA-256 `4f1d3dff55f5eadcb4c9985390fb7520d35d39a67f7abbaab1dac02876022522`)
  este caso y los dos controles pasaron, pero la prueba independiente de `PROT_NONE` falló: ver T-035. La imagen
  reconstruida no se identifica como el kernel 118 y no es una imagen dual de cierre.
- **Reutilización física observada** (`t033d-marcos-qemu.log`): QMP pausó la VM y se recorrieron las tablas del
  CR3 real del PID líder. El marco de A `0x548b000` reapareció en B tras `munmap`; después el marco de la nueva A
  `0x548c000` reapareció en C y A se remapeó a `0x548d000`, con bytes y aislamiento comprobados. La segunda CPU
  vio los valores esperados. Estas son dos parejas efectivas, no inferidas del cambio de dirección virtual. El
  fixture añade pausas y QMP detiene la VM; la planificación puede afectar la TLB y no se observa la ventana
  interna de la syscall. No demuestra la seguridad de todas las carreras, acuses perdidos ni silicio real.
- **Antes de optimizar**: en `sys_munmap`, `sys_mmap` y `madvise` se observó en el código la liberación del marco
  antes de `unmap_page`; `flush_tlb_page` ignora el booleano del shootdown. El cerrojo grande serializa parte del
  núcleo: hay un riesgo de orden por investigar, no una explotación reproducida. No agrupar liberaciones con
  traducciones potencialmente vigentes; revisar falta de acuse, COW y permisos. Después medir otra vez con los
  límites intactos y vigilar T-026/T-027.
- Java en kernel 118 **no** queda aprobado globalmente; T-033 sigue abierto.

### T-035 · Defecto 36: `mprotect(PROT_NONE)` dejaba leer páginas materializadas — **CERRADO (1-11-2026, BIOS 30/30 + UEFI 6/6)**
- Se usa T-035 para no confundir este ticket con la sonda histórica T-034 de T-031. En la imagen BIOS identificada
  arriba, `mprotect(addr,4096,PROT_NONE)` devolvió 0 sobre una página previamente tocada; tanto CPU 1 como CPU 0
  leyeron 77, en vez de recibir `SIGSEGV` con `SEGV_ACCERR=2` y la dirección exacta. Cada control que omite la
  llamada detecta esa lectura indebida y sale 1; el mismo fixture en Linux pasa los casos reales y los controles.
  Logs `t033b-smp-qemu-base.log`, `t033c-marcos-qemu.log`, `t033d-marcos-qemu.log`, `t033c-anfitrion.log`.
- `kernel/src/mem/paging.rs::mprotect_page` modifica W/NX de una PTE presente, pero no revoca el acceso USER al
  pedir protección cero; la lectura observada concuerda con el código. Afecta a una página materializada; las
  reservas perezosas PROT_NONE siguen un camino diferente. No se ha corregido ni certificado.
- **Sonda ampliada 31-10** (`t035a-qemu-base.log`): en QEMU BIOS, 9/11 casos; controles negativos,
  `munmap`, `MAP_FIXED`, `fork` y lectura de búfer legible pasan, pero `madvise(PROT_NONE)` y la escritura
  desde `PROT_NONE` fallan. El `fork` pasó con el defecto de protección vigente: sólo demuestra conservación y
  aislamiento de esa marca en ese caso, no que la página estuviera realmente protegida. Linux 11/11.
- **31-10, reserva perezosa**: detectado y corregido experimentalmente un `si_code` erróneo en una reserva
  no materializada `PROT_NONE`; ver T-039. Es distinto del fallo de acceso en páginas materializadas. La misma
  copia BIOS experimental volvió a fallar la protección materializada en CPU 1: `t035d-smp-qemu.log` 3/4.
  Diseño e invariantes de la reparación pendiente: `docs/T035_DISENO_MEMORIA.md`.
- **31-10, pila**: reproducido que una reserva `MAP_FIXED` + `PROT_NONE` en la ventana de pila se convertía
  en página escribible; corregida experimentalmente (T-040, 6/6 en copia BIOS) y verificada la regresión de
  T-039 y el crecimiento legítimo de pila. El núcleo nuevo sigue fallando T-035 materializado 3/4.
- **31-10, corrección de la representación (`t035g`)**: `mprotect(PROT_NONE)` sobre una página
  materializada ahora quita `PRESENT` y conserva en la entrada el marco y la protección
  (`PROTEGIDA_FLAG`, bit 55). La lectura y la escritura prohibidas entregan `SIGSEGV/ACCERR=2` con la
  dirección exacta; `PROT_READ` conserva el contenido (77); `PROT_RW` restaura y escribe (88).
  `munmap`, `MAP_FIXED`, la liberación del espacio y `fork` (copia profunda) reconocen la entrada;
  `for_each_user_page` la incluye para no perder marcos. Evidencia: sonda BIOS completa **21/30** en
  `t035g-completa.log`, con los 9 fallos exactamente en T-036/T-037/T-038 y ningún fallo nuevo;
  `protegida-materializada` y `fork` PASS; sonda SMP T-033 **4/4** (`t035g-smp.log`), incluida la CPU 1;
  fixture Linux **29/29**. `marcos_libres=109409` constante en los 33 procesos de la sonda, sin descenso
  acumulado. **Suite completa** sobre la misma copia: **3813/3814**, único fallo T-026
  (`irq0_perdidos=675`, umbral `<600`, `peor_hueco_us=193364`, sin control de la base en el mismo
  entorno); no es verde. Java `-version` pasó en 42,0 s.
- **Sigue abierto**: ejecución sobre página protegida, reservas con fichero, `madvise` (T-036),
  `mremap`, carreras entre ocultado y acceso desde otra CPU / protocolo de acuse TLB, auditoría de cada
  copia desde usuario, UEFI, hardware físico y dos suites verdes consecutivas con recertificación. La
  imagen UEFI no se regeneró.
- **Próximo**: con la representación ya en el árbol experimental, atacar T-036 (`madvise` sobre NONE/RO) y
  T-037 (atomicidad y `EFAULT` de escrituras) y después el orden de liberación frente al acuse TLB; cerrar
  T-035 exige además la suite completa verde, UEFI regenerado y recertificación

#### T-035 · cierre certificado (1-11-2026)

- **BIOS**: `scripts/sonda_t035_b203.py` con el fixture `t035_memoria` (31 modos, incluidos los controles
  negativos que DEBEN fallar si se omite la operación) → **casos=30/30, aprobados=30, fallidos=0,
  integridad_ext2=PASS** sobre `2958ae9082ab4ca9…`. El caso que definía el defecto,
  `protegida-materializada`, deja la página materializada con `PROT_NONE` y produce **SIGSEGV real con
  `si_code=2` en lectura Y en escritura**, con el contenido intacto al restaurar permisos (`inicial=77
  nueva=88`). `pila-protegida` y `pila-creciente` también pasan: la guarda de una pila protege y la pila
  crece hasta su tope.
- **UEFI (paridad)**: `scripts/sonda_t035_uefi.py` sobre `portix-uefi.img` con OVMF real, mismos modos
  (`protegida-materializada, pila-protegida, fork, perezosa, control-perezosa-desmapeada, pila-creciente`)
  → **6/6 PASS**, ELF idéntico byte a byte e integridad del ext2 verificada tras la corrida.
- **Conclusión**: el defecto queda cerrado con las dos firmas y con controles negativos capaces de detectar
  su omisión. Cualquier regresión futura la caza `sonda_t035_b203.py` (30 casos) o los 6 de UEFI.

### T-036 · `MADV_DONTNEED` no descarta páginas privadas de sólo lectura — CERRADO EN SONDA; 30/30 (b203, ciclo T-042)
- **Causa raíz medida**: `sys_madvise` sólo actuaba cuando `translate()` devolvía la página **presente y
  escribible**. Una página PROTEGIDA con marco (PROT_NONE/PROT_READ, la representación de T-035) no está
  presente, así que `translate` devolvía `None` y el `MADV_DONTNEED` se ignoraba en silencio: al volver a
  permitir el acceso se seguía leyendo el valor anterior (medido: esperado 0, observado **77**).
- **Arreglo**: rama nueva que usa `pte_cruda` + `es_protegida`: libera el marco del proceso, quita la
  entrada y deja una **reserva perezosa con la misma protección** (`prot_guardada`), que es la semántica
  de Linux (el siguiente acceso lee ceros y vuelve a materializarse).
- **Medido**: `madvise` y `madvise-ro` en PASS y, tras cubrir también la página PROT_READ presente
  (la primera versión sólo arreglaba la protegida-ausente), la sonda **completa 30/30** sobre la imagen
  `346655e0…0da1`: `t042-sonda-completa-antes-21de30.log` (antes) y `t042-sonda-completa-30de30.log` (después).

### T-037 · `write` a tubería con búfer inaccesible — CERRADO EN SONDA (b203, ciclo T-042)
- **Dos causas raíz**, ambas medidas:
  1. `verify_user_buffer` sólo comprobaba el RANGO: un búfer desmapeado o bajo PROT_NONE pasaba, y la ruta
     de tubería copiaba por trozos y cortaba al primero que fallaba, devolviendo **cuenta corta (0 o 128)**
     en vez de `EFAULT`. Linux publica atómicamente toda escritura ≤ `PIPE_BUF` (4096): o entra completa o
     falla con `EFAULT` sin dejar nada en el receptor.
     Arreglo: en `sys_write`, para `count ≤ 4096`, se valida el rango COMPLETO con `is_user_range_safe`
     antes de escribir nada; las escrituras mayores conservan la cuenta corta.
  2. `fcntl(fd, F_SETFL, O_NONBLOCK)` devolvía **0 sin guardar nada** en todo lo que no fuera fichero
     (`_ => 0`), así que el `read` posterior bloqueaba aunque el programa hubiera pedido no bloquear (la
     sonda se colgaba 20 s). Arreglo: `F_SETFL` guarda `O_NONBLOCK` (0x800) conservando close-on-exec, que
     es justo el bit que ya consultaban `sys_read` y `sys_write`.
- **Medido**: `pipe-invalida`, `pipe-parcial`, `usuario`, `usuario-lectura` y sus controles en PASS
  (`t042-sonda-pipe-socket.log`); luego 30/30 en la sonda completa.

### T-038 · `socketpair` Linux x86-64 devuelve ENOSYS — CERRADO EN SONDA (b203, ciclo T-042)
- **Causa raíz exacta**: `sys_socketpair` existía, pero sólo estaba despachada con el número **105** del
  dialecto nativo de Portix. En x86-64 Linux `socketpair` es la **53** (la 105 es `setuid`), y la capa de
  compatibilidad `linux_syscall_dispatch_core` no tenía esa rama: cualquier binario de distro recibía
  `ENOSYS` (medido: `rc:-1,errno:38`).
- **Arreglo**: rama `53 =>` en la capa Linux que llama al `sys_socketpair` ya existente.
- **Medido**: `control-socket`, `socket-invalido`, `control-socket-parcial` y `socket-parcial` pasan de
  fallar a PASS. `SOCKETPAIR fds=[3, 4]` en la traza del núcleo. Sonda completa final **30/30**.
- **Nota de método**: la constante `SYS_SOCKETPAIR` de `numbers.rs` sigue valiendo 105 por ser la tabla del
  dialecto NATIVO; el despacho Linux usa números literales de Linux. Esa doble tabla es una trampa para el
  próximo que añada una syscall: no se "arregla" aquí para no tocar el ABI nativo.

### T-039 · Reserva perezosa `PROT_NONE` informa `SEGV_MAPERR` — **CERRADO (1-11-2026: `si_code=2` en BIOS y UEFI, con controles)**
- `mmap` anónimo privado no materializado, protección cero: lectura falló con SIGSEGV y dirección correcta,
  pero `si_code=1` (MAPERR) en vez de 2 (ACCERR). Linux pasó 3/3: control de lectura accesible, control de
  página desmapeada y reserva protegida con restauración a RW y lectura de cero. BIOS base: 2/3, el caso
  real falló (`t035c-controles-anfitrion.log`, `t035c-qemu-base.log`).
- `isr_handlers.rs` clasifica como ACCERR el fallo de usuario sobre una PTE perezosa de protección cero;
  la página realmente desmapeada sigue con MAPERR. Copia BIOS con núcleo experimental `831527e3b0c3325d4cd1db9c37150d226f76f631c00aedbcc3a218ac9370c2a0`:
  9/9 casos seleccionados en `t035d-qemu.log`, incluyendo los controles y restauración. La imagen se inyectó
  sólo para BIOS; UEFI y `/boot` no se actualizaron. No hay suite completa ni recertificación.
- T-035 permanece abierto: la sonda SMP posterior sobre esa misma copia dio 3/4 y volvió a leer 77 tras
  `mprotect(PROT_NONE)` en la CPU 1. No se probaron ejecución ni escritura sobre reserva PROT_NONE, ni
  una reserva dentro de la ventana de crecimiento de pila. El resultado no demuestra seguridad SMP general.

#### T-039 · cierre (1-11-2026)

- **Lo que se exigía**: que un fallo sobre una reserva perezosa con `PROT_NONE` llegue al manejador con
  `si_code=2` (ACCERR) y no `1` (MAPERR), con la dirección correcta y sin materializar la página.
- **Medido en BIOS** (`sonda_t035_b203.py`, modos del caso, imagen `e890fc288b29e558…`): **6/6 casos**,
  integridad del ext2 PASS. Traza literal del caso real:
  `[SIG] SIGSEGV entregado a manejador de usuario PID=4 (page_fault) RIP=0x…C52C CR2=0x0000300000003000 EC=0x4 PTE=0x0`
  → `[T035] modo=perezosa fase=lectura senal=11 codigo=2 faltas=1 direccion=correcta` y
  `fase=restauracion inicial=0 nueva=77` (la página sigue sin materializar y se restaura bien).
- **Controles negativos en la misma corrida** (`control-perezosa-accesible`, `control-perezosa-desmapeada`):
  pasan, es decir, **detectarían** el defecto si volviera (`si_code` equivocado o página de más).
- **Medido en UEFI** (`sonda_t035_uefi.py` con OVMF real): **4/4** con los mismos modos.
- Evidencias: `docs/evidencias/b203/t039-t040-bios-6-modos.log` y `t039-t040-uefi-4-modos.log`.

### T-040 · Una reserva protegida dentro de la ventana de pila se volvía escribible — **CERRADO (1-11-2026: SIGSEGV en la ventana de pila, BIOS y UEFI)**
- `mmap(MAP_FIXED, PROT_NONE)` en la primera página de la ventana de pila
  (`0x7EFFFF801000`) seguido de lectura: en la copia BIOS de T-039 la lectura se resolvía como
  crecimiento de pila, dejaba valor 0 y permitía escribir 77. El caso estaba planteado como
  escenario del diseño T-035 y se reprodujo en `t035e-pila-qemu.log` (2/3: controles y
  crecimiento legítimo PASS).
- La referencia Linux recibió `SIGSEGV/ACCERR=2` para el mismo `mmap` fijo protegido; sus
  controles de mapeo fijo accesible y de página protegida pasaron 2/2
  (`docs/evidencias/b203/t035e-pila-controles.log`). El rango de pila de Portix no es el de
  Linux, por lo que esa comparación sólo acredita la semántica de `MAP_FIXED` + `PROT_NONE`.
- Corrección: `crecer_pila` sólo se intenta si la PTE es un hueco real; una reserva perezosa
  —protegida o no— se resuelve por su propia ruta y, si la protección lo impide, por señal.
  Núcleo experimental `4c1d7d575bc9ba5d3892bfbb93b0f2972163f5fc0cc4e19ba40f755617c07cbe` en copia
  BIOS `11bb23908688f5b53443fe3383ed1ca5163344fd6055c87a00792f20e07642fd`: **6/6** casos
  (`t035f-qemu.log`), incluido `[PILA] ... pagina de pila extendida mapeada` para un hueco
  legítimo y `SIGSEGV/ACCERR` para la reserva. La misma imagen siguió fallando T-035 materializado
  en CPU 1 (`t035f-smp-qemu.log`, 3/4). Sin suites, UEFI ni hardware; no certifica b203.

#### T-040 · cierre (1-11-2026)

- **Lo que se exigía**: que una reserva protegida dentro de la ventana de pila (`0x7EFFFF801000`) NO se
  resolviera como crecimiento de pila — antes la lectura devolvía 0 y permitía escribir 77.
- **Medido en BIOS**: modo `pila-protegida` con
  `[SIG] SIGSEGV … CR2=0x00007EFFFF801000 EC=0x4` →
  `[T035] modo=pila-protegida fase=lectura senal=11 codigo=2 faltas=1 direccion=correcta` y
  `fase=restauracion inicial=0 nueva=77`. El control `control-pila-accesible` pasa (detectaría que la
  protección desapareciera) y `pila-creciente` sigue funcionando: **el crecimiento legítimo de pila no se
  rompió** (`fase=crecimiento inicial=0 nueva=77`).
- **Medido en UEFI**: `pila-protegida` y `pila-creciente` → **4/4** en la misma corrida que T-039.
- Evidencias: mismos dos ficheros del ticket T-039.

### T-042 · La consola moría tras la sección Java y toda la suite caía en cascada — **CERRADO EN SONDA DIRIGIDA (104 PASS / 3 FAIL, los 3 de prerrequisito omitido de `--solo`)**

- **Síntoma medido (suite completa del ciclo T-042 sobre `346655e0…`)**: `TOTAL: 5768 | PASSED: 842 | FAILED: 4926`.
  Los 4 926 «fallos» no eran 4 926 fallos: tras la sección Java la consola dejó de responder y **todas**
  las comprobaciones posteriores fallaron por falta de respuesta (4 911 de ellas son la batería POSIX
  entera repitiendo «PATTERN NOT FOUND AT ALL»). Primero falló `T-026: /proc/smp publica los respiros …
  tras la JVM`, con `[REINTENTO: la orden llego corrupta por el puerto serie]`: el «puerto» no estaba
  corrupto, no había quien leyera.
- **Causa exacta (serie del invitado)**: `[T-005] exit_group: 10 hilos terminados junto al llamador` →
  `[T-005] lider de consola cerrado desde un AP: lo termina el BSP (PID 972)` →
  `SCHED[0]: PID 982 → 972` → `SCHED[0]: PID 972 → 982` → `PROC: exit PID=982` →
  `SCHED: sin procesos listos`. El líder del grupo (el `java` que la consola había lanzado con `run`)
  quedó **vivió y dormido para siempre**: su `run` nunca volvió, la consola esperaba a ese hijo y el
  anillo de entrada (`TTY_RX`) se llenó de órdenes que nadie consumió. El `java` de la primera pasada
  (`java -version`) sí cerraba porque su líder llamaba `exit(2)` y tomaba el camino directo; el de
  `T010Java` quedaba dormido en un **futex** (no en el bucle «espera al último» de `sys_exit`), y su
  bucle de espera no completaba el cierre diferido.
- **Arreglo (`kernel/src/process/scheduler.rs`, `wait_hlt_preemptible`)**: se completa el cierre diferido
  en el **punto único por el que pasa toda espera del núcleo**. Si el proceso en curso tiene
  `lider_salio + salir_solo + SIGKILL interno` y es un líder lanzado por la consola (`ppid == 0`), se
  termina ahí mismo, venga del bucle que venga, dejando en la serie
  `[T-005] cierre diferido del lider completado en su espera: pid=… rip=…` (el `rip` nombra el bucle que
  se lo estaba tragando). No se relajó ningún umbral ni se añadió una espera nueva.
- **Reproducción aislada**: `scripts/sonda_futex_lider.py` + modo nuevo `futex-lider` de
  `scripts/sondas/t034_exit_group.rs` (líder en `FUTEX_WAIT` infinito + 10 hilos de apoyo en futex y
  `nanosleep`, `exit_group(7)` desde el otro CPU). Antes del arreglo el caso simple ya pasaba: **el
  fixture solo no reproducía la JVM**, y queda anotado como límite de la prueba. Lo que sí queda
  demostrado es la reparación en el escenario real: ver abajo.
- **Verificación (imagen `a1b21105…`, `--solo "Terminal Built-in,Java"`)**: **104 PASS / 3 FAIL**, y los
  3 fallos son los de prerrequisito omitido documentados (`L2: debugfs …`, `T-001: debugfs …`, `L2b`)
  que `--solo` produce siempre. Toda la sección Java en verde, y **después de la JVM**:
  `T-026: /proc/smp publica los respiros del nucleo tras la JVM` PASS, `tras la JVM, el BSP no retiene
  IF=0 mas de 300 ms` PASS, `ticks del PIT perdidos … < 600` PASS, `L3: raiz ext2 LIMPIA` PASS.
  La consola siguió atendiendo órdenes tras el cierre del grupo.
- **Reproducción determinista del defecto (hallada en este ciclo)**: el modo `futex-lider` del fixture
  mataba el grupo **antes** de que el líder llegara a dormir, así que no ejercitaba el cierre diferido.
  Se añadió `hilo_mata_grupo_tarde` (espera 1200 ms, con el líder ya dentro de su `futex`). Con eso el
  caso se reproduce **siempre y en 3 s** en los dos firmwares, y en Linux sale con código 7 en 2,3 s:
  `scripts/sonda_paridad_t042.py --ambos`. En Portix, antes del arreglo: la consola no volvía en 40 s.
  Después: `[T-005] cierre diferido del lider completado en su espera: pid=2 rip=…`,
  `PROC: exit PID=2 … code=7`, `[OK] Proceso PID 2 finalizado`, `[RUN] ext2 sincronizado` y la consola
  atiende la orden siguiente.
- **Cuidado con el criterio**: en Portix el prompt `root@portix:` se imprime en el **eco** de la línea
  que se teclea, no al terminar una orden. Una sonda que espere «que vuelva el prompt» da falsos
  negativos. La comprobación correcta es que el líder salga con el código del grupo y que la consola
  conteste a una orden nueva; así quedó en la sonda de paridad.
- **Paridad BIOS/UEFI (misma imagen `c3db4abb…`)**: `sonda_paridad_t042.py` → **BIOS 8/8** y
  **UEFI 8/8** (fixture en el ext2, consola lista, `futex-lider` cierra con code=7 en 2,0–2,1 s,
  consola viva, `ping6` honesto en 0,5 s sin RTT inventado, `/proc/smp` con `ventanas_if0` y control
  final). Un intento BIOS se perdió porque la orden `run` no llegó a la consola nada más arrancar
  (entrada por serie); al repetirlo pasó 8/8 — queda anotado como fragilidad de la sonda, no del núcleo.
- **Pendiente para cerrar**: las dos pasadas completas + `recertify --no-build` de rigor. Hasta entonces
  este ticket queda «cerrado en sonda dirigida y con paridad», no cerrado.

### T-044 · `dbg_probe` daba falsos negativos cuando la consola transportaba volcados binarios — **CERRADO (1-11-2026)**

- **Reproducido por fin, en la recertificación final** sobre `f3d5dacc…`: `dbg_probe` **53/62** en 143,4 s,
  con cinco fallos del bloque del búfer repartido (`--buftest`), no del lenguaje de condiciones como la
  vez anterior. Es decir, **el bloque que falla cambia en cada ocasión**: señal de que el problema no
  está en una fase concreta del depurador.
- **Causa: la consola serie transporta volcados binarios del invitado.** La serie de esa corrida tiene
  21 544 688 bytes de los que **19 114 163 son `0x00` (88,7 %)** y 20,5 M no son imprimibles: hay
  bloques de una página entera (un tramo de **4 095 ceros** seguidos) con punteros `0x7effffff…` de la
  pila del objetivo. Ese torrente **parte las líneas de texto** del depurador.
- **El depurador sí hizo su trabajo**: en esa misma serie están `[SESSION-BUF-PASS]` ×3,
  `[SESSION-BUF-SILENT]` ×3, `[WATCH-CLEARED]` ×4, `ranura=DR0 vigilada=` ×11, `necesita m` ×3 y
  `[BREAKPOINT-PASS]` ×2. Lo que **no** se encuentra son justamente los marcadores que llevan **dígitos
  dentro de la cadena** («4 ranuras armadas», «dispararon=4», «atribuidas a la mitad baja=0»,
  «ranuras activas en el CPU=4»): al perderse o intercalarse bytes, la comparación literal falla aunque
  el sistema haya hecho lo correcto.
- **Por qué es intermitente**: depende de cuánto volcado binario caiga dentro de la ventana que el probe
  compara. De ahí que a veces falle `--ortest` (57/62) y a veces `--buftest` (53/62), y que en cinco
  corridas limpias diera 62/62.
- **Evidencia**: `docs/evidencias/b203/t044-informe-fallo-reproducido.txt` y
  `recert-final-f3d5dacc.log`.
- **Diagnóstico instalado en este ciclo y que ya funciona**: `dbg --ortest` espera cada parada con
  plazo de 15 s e imprime siempre `[SESSION-OR-WAIT]` con el estado crudo (`r`, `stopped`, `vencido`,
  `status`); `scripts/dbg_probe.py` guarda la serie completa en `build/dbg_probe_serie.log`. Gracias a
  eso se pudo pasar de «sin evidencia» a «el marcador está, el transporte lo rompió».
- **Arreglo pendiente, y dónde está**: en T-045 (transporte). Hasta que la consola no transporte datos
  binarios sin codificar, **ninguna prueba de texto con dígitos es fiable**, y por eso T-044 no se cierra
  aunque el depurador esté sano.
- **Trampa anotada**: `esperar_parada` ya existía con otra firma en `dbg`; la nueva es
  `esperar_parada_or` (Rust no admite sobrecarga).

### T-045 · La consola del núcleo transporta volcados binarios que rompen la observabilidad — **ABIERTO (medido)**

- **Medido**: en la serie de `dbg_probe` el **88,7 %** de los bytes son `0x00` y hay bloques de una
  página entera (4 095 ceros seguidos) con punteros de pila `0x7effffff…`. En las suites completas el
  ruido es menor pero constante: **1,486 %** (`f3d5dacc` pasada 1) y **1,740 %** (pasada 2) de bytes no
  imprimibles sobre 7,2–7,5 MB de serie.
- **Consecuencia real**: cualquier comprobación que busque una cadena con dígitos puede fallar aunque el
  sistema haya hecho lo correcto (hoy le pasa a `dbg_probe`, T-044). También rompe los propios
  diagnósticos: en esa corrida, tres `[SESSION-OR-TIMEOUT]` salieron con la fase y el pid sustituidos
  por bytes sueltos.
- **Qué falta por decidir, con dos frentes**:
  1. **Quién vuelca memoria cruda por la consola**: hay que localizar el punto que escribe una página
     entera (probablemente un `mem`/`peek` del depurador o un volcado de pila) y hacer que salga
     **codificado** (hexdump), como ya hace el resto de la consola con `write_bytes_raw`
     (`b'.'` para no imprimibles). Es un cambio acotado y con prueba: la serie de una corrida no debe
     contener ceros.
  2. **Pérdida de bytes** del transporte bajo carga (0,9 % de caracteres `À` incluso en texto): puede
     ser el FIFO del UART del invitado o el drenaje del anfitrión. `serial::write_byte` ya espera THRE
     con límite; queda por medir si el que pierde es el invitado o el pipe del host.
- **Por qué importa más de lo que parece**: la observabilidad es la base de todas las demás pruebas. Un
  sistema que no puede entregar su propio registro de forma fiable no puede demostrar nada con texto.

#### T-044 · cierre (1-11-2026)

- **Recertificación completa sobre la imagen nueva `90569def…`**: `recertify.py --no-build` →
  **[PORTIX-CHRONOS-MASTER-PASS] 7/7** — [0] 0 hallazgos, [2] depurador **64/64** (46,5 s), [3] interactiva
  **128/128**, Épica 58 completa, [4] Épica 59 **31/31**, [5] Épica 60 **10/10**, [7] UEFI **4/4**.
  Evidencia: `docs/evidencias/b203/recert-t045-master-pass.log`.
- **Las tres suites completas de esta imagen** (`docs/evidencias/b203/suite-90569def-1/2/3.log`):
  #1 3825/3829 (los 4 fallos, un solo fixture, ver T-017 abajo), **#2 3829/3829** y **#3 3829/3829**
  (`TOTAL: 3829 | PASSED: 3829 | FAILED: 0` en las dos verdes) y **cero líneas `[IRQ0-HUECO]`** en ambas.
- **El volcado binario no reapareció**: cero avisos `[SERIE-BULTO]` y cero `[SERIE-SIN-TURNO]` en las tres
  suites y en las 20 vueltas del reproductor. El depurador ya no depende del transporte para decir la verdad.

#### T-045 · segunda jornada (1-11-2026): causa del transporte acotada, cerrojo corregido y prueba que duele

- **Dónde NO está el volcado**: las dos suites completas sobre `f3d5dacc…` tienen **0 bytes NUL** en toda la
  serie (537 619 y 537 620 B). El volcado aparece **sólo** en las corridas de `dbg_probe` (una vez con
  19 114 163 NUL, 88,7 %). Es decir: no es el bucle normal de la consola, es un camino que se activa en esa
  sonda.
- **Qué se descartó midiendo**: 20 vueltas seguidas de `dbg --ortest` + `dbg --buftest` en un solo arranque
  (`scripts/repro_t045.py`, imagen instrumentada) → **40/40 bloques limpios, 0 NUL, 0 escrituras >=256 B**.
  El depurador en aislamiento no produce el volcado ni se degrada; el disparo necesita el estado previo de
  la recertificación (suite completa antes de las sondas).
- **Instrumento permanente en el núcleo** (`kernel/src/drivers/serial.rs`): `write_bytes_raw` lleva
  `#[track_caller]` y, para cualquier escritura **>= 256 B**, deja `[SERIE-BULTO] n=<bytes> pid=<pid> en
  <fichero>:<línea>` (máximo 60 avisos). La próxima vez que ocurra, el productor queda **nombrado** en la
  propia serie en vez de dejar «sin evidencia».
- **Defecto real encontrado y corregido — el cerrojo de la consola degradaba a escritura sin turno**: en
  `serial.rs`, `tomar()` devolvía un guarda «falso» cuando se agotaban los ~50 ms de espera y el llamador
  **escribía igual**, así que dos CPUs entrelazaban sus series byte a byte y las líneas del depurador
  perdían sus dígitos (`búfer de 32 B` llegaba como `búfer de <basura> B`). Ahora `tomar()` devuelve
  `Option<GuardSerie>`: **si no hay turno no se escribe**, se cuenta la pérdida y el siguiente escritor con
  turno anuncia `[SERIE-SIN-TURNO] escrituras perdidas=N`. Mejor perder una línea y decirlo que corromper
  las de todo el mundo.
- **Prueba nueva que falla si el defecto vuelve** (`scripts/dbg_probe.py`): dos comprobaciones al final de la
  sonda — «la consola serie transporta sólo texto (ni NUL ni memoria cruda)» (NUL == 0 y no imprimibles
  <= 64) y «ninguna escritura suelta de >=256 B en la consola». La sonda pasa de 62 a **64** comprobaciones.
- **Estado**: el productor del volcado sigue sin nombre (necesita una recertificación con el instrumento);
  el efecto sobre las pruebas de texto ya no puede ocurrir por entrelazado, y si vuelve, la sonda lo dice.

### T-043 · Cada página de `mmap`/`munmap` costaba un shootdown de TLB con acuse — **CORREGIDO EN SONDA Y EN LA SUITE (213 ms → ~1 ms de tramo)**

- **Síntoma medido (imagen `a1b21105…`)**: en la suite completa apareció
  `[IRQ0-HUECO] us=213019 perdidos=20 dentro_if0=1 pid=846 nr=9 abi=linux ventana_us=241839`,
  es decir un hueco de 213 ms **dentro** de un `mmap` de la ABI Linux con las interrupciones cerradas.
  Es el único hueco del ciclo que la atribución nueva marcó como culpa del núcleo (`dentro_if0=1`).
- **Causa exacta**: en la rama perezosa de `sys_mmap` (`kernel/src/syscall/mod.rs`) el bucle hacía
  **dos invalidaciones de TLB con espera de acuse por página** — una en `paging::unmap_page` al
  sustituir la página y otra en `paging::mapear_perezosa` al instalar la nueva entrada. Con un hilo
  vivo en el otro CPU compartiendo el espacio (`cr3_en_otra_cpu`), cada acuse cuesta cientos de µs
  bajo TCG. El comentario del bucle decía «sin lote de TLB aquí»: por eso estaba.
- **Arreglo**: se invalida **una vez por tanda de 128 páginas** (`inicio_lote_tlb`/`fin_lote_tlb`) y los
  marcos se liberan **después** del shootdown de la tanda, no antes: así ningún otro núcleo puede
  reutilizar un marco cuya traducción vieja siga en su TLB. El camino de error (`-12`) también cierra
  el lote. No cambia la semántica de `MAP_FIXED`, ni `mprotect`, ni el reparto de marcos.
- **Prueba que puede fallar** (`scripts/sondas/t026_mmapfixed.rs`, ejecutada por
  `scripts/sonda_t026_ventanas.py`): 8 MB materializados y después `MAP_FIXED` encima, con un hilo
  vigilante en el CPU 1 (sin él el núcleo detecta `cr3_en_otra_cpu == false` y el bucle sale barato:
  así se comprobó que el fixture **no** reproducía nada). Medido en QEMU:
  **antes 90 000 µs (44 µs/página), después 10 000 µs (resolución del reloj del invitado, ≤5 µs/página)**,
  y el tramo continuo con IF=0 que el núcleo publica en `respiros: … peor_tramo_us` bajó de
  `20 944 µs en mod.rs:3067` a `≤ 11 203 µs en elf.rs:212` (el cargador, otro sitio).
- **`munmap`: el mismo arreglo, con A/B medido (cierre de este ciclo)**. El bucle de `sys_munmap`
  (`kernel/src/syscall/mod.rs`) recorría las páginas igual, y encima en el orden peligroso: **liberaba
  el marco ANTES de quitar la PTE**, así que con otro CPU en el mismo espacio el marco podía
  reutilizarse mientras su traducción vieja seguía viva. Ahora usa el mismo lote de 128 páginas y los
  marcos se devuelven **después** del shootdown de su tanda.
  - **A/B con el mismo fixture y el mismo instrumento** (`scripts/sondas/t026_mmapfixed.rs`, 8 MB
    materializados y después `MAP_FIXED`/`munmap` con un hilo vigilante en el CPU 1):
    **sin el arreglo `munmap` = 50 000 µs** y la sonda **falla** (umbral 30 000); **con el arreglo
    = 0 µs**, por debajo de los 10 ms de resolución del reloj del invitado. Evidencia:
    `munmap-antes-t043.log` y `munmap-despues-t043.log`.
  - En la **suite completa sobre la imagen final** (`f3d5dacc…`, 3829/3829): `respiros: … peor_tramo_us`
    deja de señalar el `munmap` y pasa a ser **`ata.rs:539` (espera del disco) con 33 778 µs**, y
    `shootdowns_ahorrados_en_lote` sube a **3 499 169** (antes 1 558 342). Ningún tramo del `mmap` ni
    del `munmap` aparece ya en la lista; el peor hueco del PIT de la pasada fue 275 919 µs con
    `dentro_if0=0`, es decir del anfitrión.
- **Lo que queda**: el cargador de ELF (`elf.rs:212`, ~13 ms) es ahora el tramo más largo y está muy
  por debajo de cualquier umbral; no requiere acción.

### T-041 · `curl` moría con `#PF` al ejecutarse bajo UEFI — CERRADO (paridad BIOS/UEFI restablecida)
- **Síntoma**: bajo OVMF, todo binario propio enlazado en 0x20000000 (`curl`, `init`) moría en la
  PRIMERA instrucción con `#PF CR2=0x0 EC=0x5`; bajo BIOS el mismo `/bin/curl` de la misma imagen
  base arrancaba y sólo fallaba `connect()` (err -111 sin servidor). Determinista en ambos.
- **Diagnóstico (instrumentación temporal, hoy retirada)**: `EC=0x5` tiene P=1 ⇒ no era «página
  ausente» sino violación de protección, con `CR2=0` y `pte_cr2` ausente por ser **página gigante**
  (`cero_flags=0xE3` = `P|RW|PS|A|D`). Los bytes reales en `RIP` eran `00 00 00 00 00 00 00 00`, y la
  PTE del cargador (`0x20005027`, la que escribe `map_page`) apuntaba a `fis=0x20005000`.
- **Causa raíz**: `0x20005000` son 512 MiB + 20 KiB, es decir el **hueco PCI/MMIO** justo por encima de
  la RAM de `-m 512M`. `marcos::liberar()` sólo comprobaba `MARCOS_BASE ≤ p < MARCOS_LIMITE`, sin
  comprobar que el físico estuviera alguna vez declarado RAM; liberar un marco del hueco lo marcaba
  libre y `asignar()` lo entregaba como marco de usuario. `limpiar()` y las copias de los segmentos
  se perdían en el hueco (lecturas 0) ⇒ el proceso ejecutaba ceros. Bajo UEFI el mapa del firmware es
  fragmentado y alguna ruta liberaba un marco de identidad del hueco; bajo BIOS no se daba.
- **Arreglo (kernel, sin crates)**:
  1. `kernel/src/mem/marcos.rs`: segundo mapa de bits `existe` con los marcos que el firmware declaró
     RAM; `liberar()` los ignora y contabiliza `LIBERADOS_IGNORADOS`; `es_marco()` también lo exige.
     Invariante nueva: **el asignador nunca entrega un físico que el firmware no declaró RAM**.
  2. `kernel/src/elf.rs`: sólo se reutiliza una traducción que YA es de usuario (PTE con `USER` y
     físico dinámico). Antes bastaba con que no fuese gigante, y una página de identidad de 4 KiB
     (creada al partir una gigante del firmware) pasaba el filtro.
  3. Observabilidad: la línea `PROC: exit` publica `liberados_ignorados=`.
- **Paridad verificada con la batería idéntica en ambos firmware** (imágenes finales
  `portix.img fc20a5aa…47f6c`, `portix-uefi.img c4ede9b8…9e701`, fixture `525a3493…073a`):
  - `scripts/sonda_t035_uefi.py --firmware uefi|bios --transporte red` → **UEFI 6/6** y **BIOS 6/6**,
    con **descarga real por red con `curl`** (1 211 072 B) e integridad ext2 byte a byte en ambos.
  - Traza de `curl` idéntica: `[CURL] …` ×3, `[CURL-FAIL] connect() fallo (err=-111)`,
    `PROC: exit … code=1 … pico=104 respaldo_heap=0` en los dos firmware.
  - Logs: `t041-diagnostico.log`, `t041-comparacion-pre-fix.log`, `t041-paridad-bios-red.log`,
    `t041-paridad-uefi-red.log`, `t041-build-limpio.log`.
- **Suites y recertificación (hechas en este ciclo)**: sobre esa misma imagen, dos pasadas completas
  consecutivas **3814/3814 con 0 fallos** y recertificación `--no-build` **MASTER-PASS 7/7**.
  Logs: `t041-suite1-3814.log`, `t041-suite2-3814.log`, `t041-recert-master-pass.log`,
  `t041-controles-finales.log`, `t041-huellas.sha256`.
- **Riesgo que deja el diagnóstico**: el `EC`/`CR2` no bastaban para localizar el fallo; lo resolvió
  leer los bytes reales en `RIP` y el físico de la PTE. Esa comprobación (bytes en el punto de fallo
  frente a bytes del fichero) merece quedarse como herramienta de depuración, no sólo como parche
  temporal.

## T-012 · Tiempo de ciclo del desarrollo  — CERRADO (build 188; queda la mejora de `pkg` como deuda)
- Medido (build 187): build 28 s; sonda 4 min; suite completa ~27 min; recert ~4 min; las 2-4 h por cambio eran método (suite+recert por cada cambio, esperas con `sleep` fijo), no QEMU.
- Hecho en b188: `headless_runner.py --solo` (secciones por patrón, resultado NO certificable); copia de la imagen en `build/run/` (disco, no tmpfs; se acabaron los OOM); suite y recert pueden coexistir (recert 227 s mientras corría la suite) **pero en este sandbox de 2 núcleos TCG el paralelismo produce fallos falsos por tiempos límite → se encadenan en serie** (~31 min); `recertify.py` sólo mata VMs huérfanas; `build.py` escribe el GPT disperso (ya no carga 1,3 GB en RAM); caché local de `.apk`/`.deb` en `build/cache-pkgs/` (verificación intacta; `deb-add hello` siempre por red real).
- **Hallazgo honesto**: la caché apenas acorta `deb-add` (21 `.deb`: 149 s sin caché, 153-157 s con caché) porque el tiempo está en el invitado, no en la red: cada pasada de `resolver` vuelve a inflar `Packages.gz` (13 MB → ~70 MB de texto; 3-4 pasadas ≈ 40 s) y el xz/LZMA2 en Ring 3 bajo TCG. **Pendiente**: índice de nombres → offset (una pasada) y/o `Packages` guardado ya inflado por trozos; medir el xz.
- Cierre: suite 64 3651/3651 en 1216 s (20 min) + recert 101 MASTER-PASS 226 s, en serie (24 min en total).
- Suelo físico: sin `/dev/kvm` (TCG), 2 núcleos; el arranque + 3650 comprobaciones en serie ≈ 15 min como mínimo.

## Deuda menor anotada
- **QMP del ejecutor**: se detectó que un evento entre saludo y respuesta de capacidades podía hacer que el cliente
  antiguo entregara el acuse de capacidades como respuesta de la orden posterior. `qmp_seguro.py` exige IDs y
  descarta eventos; `headless_runner.qmp` lo usa manteniendo bytes JSON. Controles 128/128, cinco ciclos reales
  STOP/CONT con estados comprobados y un arranque Portix con traza IDE y cobertura de reset primario (1/1), sin
  sustituir la suite completa. El fallo inicial de inspección física se conserva en `t033c-marcos-qemu.log`.
  Pendiente: el caso TCP que invoca `retener_red(True/False)` ignora todavía sus respuestas; exigir y probar ambos
  acuses antes de usarlo como evidencia inequívoca del filtro de red.
- [b189] Esperas del bus ATA ahora por tiempo (hasta 10 s con TSC), no por 1 M de iteraciones:
  la imagen copiada a `build/run/` (disco) dejaba el anfitrión con escrituras pendientes y
  la primera escritura del superbloque ext2 fallaba por "timeout" en ~40 % de los arranques
  (modo degradado intermitente). Causa raíz: plazo demasiado corto para medios lentos.
  Efecto secundario visto en suite 65: cuando el anfitrión tarda de verdad varios segundos
  en una escritura, `test_ui` (escribe `/tmp/redirection_test.txt` en ext2) se queda en la
  espera y ~29 comprobaciones WM + pty/cow caducan en cascada (3 s de plazo). Medidas b189:
  `PlazoAta` imprime al terminar cuánto duró la espera larga (`[ATA] espera larga terminada:
  N ms`) y `execute_and_verify` amplía el plazo UNA vez a 12 s sólo cuando el kernel ha
  avisado de la espera (el patrón exigido no cambia). Deuda real: el ATA PIO bloquea todo
  el núcleo mientras espera (sin IRQ); se resolverá al sacar el driver a Ring 3 (T-008).
  **Causa raíz encontrada en b190** (telemetría: las 10 esperas largas de la suite 69 duraron
  exactamente 15 046 ms, siempre TIMEOUT, con estado alternativo 0x00 en `wait_ready` tras
  `select_drive`): no era el anfitrión. `test_ui` llamaba a `HardwareInfo::detect_all()` y
  `Disks::detect()` hace SRST del canal IDE y sondea maestro/esclavo con IDENTIFY, dejando
  seleccionado el esclavo (ausente) con nIEN a 0, MIENTRAS el ext2 raíz usa ese canal; una
  de cada ~8 veces la lectura siguiente del bloque 4 veía el dispositivo "ausente" y caía al
  RAMFS (`[RAMFS] create '/tmp/redirection_test.txt'`): con el plazo antiguo de 1 M de
  iteraciones esto fallaba en 0,3 s y **la prueba pasaba en silencio escribiendo en RAM**.
  Arreglo: la instantánea del hardware se toma una vez en el arranque y se reutiliza
  (`HW_CACHE`). Verificado: 83 `test_ui` seguidos sin una sola espera larga.
- [b189] Regla de reinicio de syscall: `rebobinar_syscall` SÓLO cuando `schedule_tick`
  conmuta de verdad. Si se devuelve un valor directo con RIP rebobinado el proceso vuelve a
  ejecutar `syscall` con RAX=valor → ENOSYS (-38). Lo sufrió `sys_futex` con plazo cero
  (TEST 291/1218 en suite 65). Corregido como `esperar_senal`.
- [b189] `--solo` arrancaba igualmente los ayudantes del anfitrión (`openssl s_server :8446`)
  y dejaba huérfanos que rompían la prueba KeyUpdate de la siguiente suite. Corregido: guardas
  `_seccion_activa()` en los ayudantes TLS, `close()` los mata y `matar_qemu_huerfanos`
  también elimina `openssl s_server` con PPID 1.
- ~~Contadores `free_blocks` del buddy inconsistentes~~ **corregido con autorización del usuario (build 176)**: pop en `found_ord`, push en el orden final tras fusión, nuevo `bytes_en_uso`; la suite exige `ocupado + libre == 64 MiB`.
- B.3 MMIO con PD alto UC (Fase B).
- PROT_NONE estricto (PTE no presente).
- `chown`/`link` en ext2 (dentro de T-001).
- El `cd` del shell del núcleo no afecta al cwd de los procesos `run`.
- `inotify`: enganches en ext2/devfs, `IN_MOVED_*`.
- PCC v1.3 (`-c` + enlazador) — baja prioridad una vez exista gcc real (T-003).
