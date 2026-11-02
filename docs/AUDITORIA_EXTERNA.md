# Portix OS — Auditoría general del proyecto para un revisor externo

**Fecha:** 25 de octubre de 2026 · **Estado de referencia:** build 180, suite headless 56b (3 593/3 593), recertificación 94 (`[PORTIX-CHRONOS-MASTER-PASS]`).
**Autor:** el agente de desarrollo, a petición del responsable del proyecto. Este documento intenta ser tan útil para encontrar defectos como para presentar logros: cada afirmación de "funciona" va acompañada de cómo comprobarla desde fuera, y cada debilidad conocida está escrita sin suavizar.

---

## 1. Qué es Portix en una frase (y qué no es)

> Portix OS es un núcleo en Rust `no_std`, sin dependencias externas, para x86-64, que arranca en BIOS y UEFI, y que **ejecuta binarios ELF de Linux sin modificar** (busybox estático y dinámico, `ld-musl`, `tcc` de Alpine) sobre una raíz ext2 real verificable con `e2fsck`, y **cuyas afirmaciones se pueden verificar desde fuera** con una suite automática que arranca el sistema en QEMU.

Lo que **no** es, y el proyecto ya lo ha dejado por escrito (`ROADMAP.md` §2-quater, `docs/TICKETS.md`):
- No es compatible con Windows (la capa "Win32 PE" fue **eliminada** en la build 173 por ser una demostración de 4 funciones que se anunciaba como compatibilidad).
- No ejecuta todavía binarios con glibc (sólo musl), ni paquetes `.apk`/`.deb`/`.rpm` reales (usa un formato propio `.pkg` desde un espejo propio).
- No tiene hilos reales en espacio de usuario (sin `rustc`/`cargo` dentro del sistema).
- No se ha arrancado en hardware físico dentro de este ciclo de desarrollo: toda la verificación es en QEMU (BIOS SeaBIOS y UEFI OVMF), con dispositivos emulados reales (NVMe, AHCI, xHCI, Intel HDA, e1000).
- No es un microkernel "puro": los controladores de dispositivo siguen en Ring 0. El gestor de ventanas y los servicios de usuario sí están en Ring 3.

---

## 2. Cifras del repositorio (medidas sobre el árbol, no estimadas)

| Magnitud | Valor |
|---|---|
| Núcleo (Rust) | 80 805 líneas en `kernel/src` |
| Ensamblador (arranque + ISR) | 2 608 líneas |
| Espacio de usuario (Rust) | 85 401 líneas, **132 binarios** en `userland/bin` |
| Dependencias externas del núcleo (`kernel/Cargo.toml`) | **0** |
| `.unwrap()` en el núcleo | **0** (política: ningún pánico no manejado) |
| Fichero más grande | `kernel/src/syscall/mod.rs`: **13 885 líneas** (ver §6) |
| Infraestructura de pruebas (Python) | 16 828 líneas (`scripts/`) |
| Pruebas de la suite headless | 3 593 comprobaciones; además suite interactiva (126), Épica 58 señales, Épica 59 trazador (31), cadena `dbg` (62), ABI (10), UEFI (3) |
| Imágenes | `build/dist/portix.img` 100 MiB (BIOS/MBR: P1 arranque, P2 FAT32 33 MiB `/boot` sólo lectura, P3 ext2 64 MiB raíz); `portix-uefi.img` 130 MiB; ISO híbrida |
| Control de versiones | **El directorio no es un repositorio git** (ver §6) |

---

## 3. Lo que está implementado **y verificado** (con cómo comprobarlo)

La regla del proyecto desde el ciclo 50 es: *una prueba que no puede fallar no es una prueba*. La suite rechaza patrones vacíos, exige `errno` exactos y hace comprobaciones desde el anfitrión (fuera del sistema) cuando es posible. Un auditor puede reproducir todo con tres órdenes (§8).

### 3.1 Arranque y plataforma
- **BIOS** (MBR → `boot/boot.asm` → `boot/stage2.asm` → núcleo en mitad superior `0xFFFFFFFF80200000`) y **UEFI** (cargador propio en Rust `boot/efi`). Recertificación "Ciclo 49: arranque UEFI real (OVMF)" 3/3.
- Mapa de memoria de arranque (E820/UEFI), paginación de 4 niveles con NX, asignador de marcos físicos independiente del montículo (`mem/marcos.rs`), montículo buddy de 64 MiB. Tiempo medido de arranque hasta escritorio: **8–11 s en QEMU** (incluye firmware; medido por TSC, no estimado).
- IDT/ISR, PIT, RTC, APIC básico, TSC calibrado contra el PIT.

### 3.2 Procesos y ABI Linux
- Procesos Ring 3 con `fork`/`execve`/`wait4`/`clone3`, señales POSIX con reinicio `ERESTARTSYS` (Épica 58, certificada), `brk`/`mmap` (anónimo y de fichero con `MAP_FIXED`), pila inicial AMD64 con `auxv` (lo que exige `ld-musl`), `rlimit`, `prlimit64`, `getrandom`, `rseq` (no-op aceptado).
- **Dos tablas de syscalls**: la nativa de Portix y la **traducción de números Linux** para binarios ajenos; lo no traducido devuelve `-ENOSYS` y se contabiliza (`/proc/abi`, mando `abicoverage`), nunca cae en un syscall nativo arbitrario. En esta misma ronda se descubrió que faltaban 23 syscalls de ficheros (`link`, `chown`, `truncate`, `rename`, `fsync`…) — ya añadidos; es un recordatorio de que la cobertura de la tabla Linux se verifica binario a binario, no está completa a priori.
- **Binarios de distribución sin modificar que funcionan** (descargados por HTTPS desde el propio sistema y verificados por SHA-256 en la suite): busybox estático de Alpine (no-PIE, 0x400000), busybox dinámico PIE con `PT_INTERP` cargado por el `ld-musl` real, `tcc` dinámico + `libtcc.so` + `musl-dev` compilando y ejecutando C dentro de Portix.
- `test_posix`: ~3 200 comprobaciones de conformidad propias, ejecutadas en cada suite; 15 (2885–3176, "toolchain") fallan de forma conocida y aceptada (§6).

### 3.3 Almacenamiento y sistemas de ficheros
- **Raíz ext2 real** (rev 1, `mke2fs` estándar) de lectura/escritura: inodos, directorios, mapas de bits, superbloques de respaldo, enlaces simbólicos, enlaces duros, `chown` con regla POSIX, `truncate`, indirecto simple/doble/**triple**, caché de bloques de escritura inmediata (2 MiB). **Verificado desde el anfitrión**: `e2fsck -fn` devuelve 0 tras toda la suite; `debugfs` lee los ficheros escritos por Portix y ve el árbol `(TIND)` de un fichero disperso de 4,5 GiB.
- Rendimiento medido (QEMU, ATA PIO): 4,3 MiB/s en frío → 35 MiB/s desde caché.
- FAT32 de `/boot` sólo lectura bajo el mismo contrato; ramfs para `/tmp` y `/dev/shm`; devfs, procfs.
- Controladores: ATA PIO (el que sirve la raíz), NVMe, AHCI, ATAPI/ISO9660, xHCI (USB), Intel HDA/AC97 — los cuatro últimos con "ensayo de silicio" en la suite que **falla si el dispositivo no está presente** (y una prueba negativa que arranca sin ellos).
- `fsync`/`sync` reales (vuelcan superbloque/GDT y emiten `FLUSH CACHE`).

### 3.4 Red
- Pila propia: e1000, ARP, IPv4/IPv6 (bucle local), ICMP, UDP, TCP con ventana dinámica, control de flujo, SACK negociado, retransmisión por RTO y rápida; DHCP; DNS; cliente HTTP/HTTPS (`curl` propio) usado por `pkg` para descargar paquetes reales desde `dl-cdn.alpinelinux.org` y desde el servidor de pruebas; `httpd`, `telnetd`, `ssh`/`sshd` propios.
- La suite levanta servidores HTTP/HTTPS en el anfitrión y comprueba descargas con SHA-256.

### 3.5 Gráficos, ventanas y consola
- Framebuffer lineal (VESA/GOP), compositor con back buffer, **gestor de ventanas `portix-wm` en Ring 3** con protocolo propio y clientes (`winclient`, `wp-demo`), terminal gráfico, "Centro de Control" con pestañas; suite interactiva por el monitor de QEMU con entrada real de ratón/teclado y capturas comparadas (126/126).
- El panel "Benchmarks" muestra únicamente **mediciones reales de la máquina** (arranque, memoria física, lectura ext2) desde la build 174; las comparativas con Linux/Windows fueron retiradas.

### 3.6 Depuración y observabilidad
- `/bin/dbg` (puntos de ruptura/observación en Ring 3, 62/62), trazador por TSC y perfilador de muestreo del núcleo con símbolos (Épica 59, 31/31 con tolerancia estadística), `strace`, `portix-fuzz` (10 000 syscalls con argumentos hostiles sin pánico), `portix-bench` (latencia de syscall por TSC desde Ring 3).

### 3.7 Infraestructura de pruebas (es la mayor fortaleza del proyecto)
- `scripts/headless_runner.py`: arranca la imagen en QEMU (copia privada), habla por el puerto serie, levanta servidores, ejecuta 3 593 comprobaciones; cada orden se reintenta **una vez** sólo si el eco serie llegó corrupto.
- `scripts/recertify.py`: auditoría estática de las pruebas (0 tautologías, 0 `errno` genérico), suite interactiva, señales, trazador, `dbg`, ABI congelada, UEFI.
- Comprobaciones **fuera del sistema**: `e2fsck`, `debugfs`, SHA-256 de lo descargado, cifras de `bench` con rangos plausibles y dos ejecuciones no idénticas.
- `AGENTS.md`: bitácora de 66 entradas con cada decisión, cada fallo y cada cifra medida; `docs/TICKETS.md`: plan con criterios de aceptación.

---

## 4. Lo bueno (fortalezas que un externo puede confirmar)

1. **Verificabilidad**. No hay que creer al autor: `python3 scripts/build.py --no-run && python3 scripts/headless_runner.py && python3 scripts/recertify.py --no-build` reconstruye y recertifica todo en ~30 minutos, y las comprobaciones de anfitrión (e2fsprogs, SHA-256) no dependen del código de Portix.
2. **Compatibilidad binaria demostrada con software ajeno real**, no con programas escritos a medida: busybox, `ld-musl`, `tcc` tal como los distribuye Alpine.
3. **Disciplina de errores**: 0 `unwrap` en el núcleo, `errno` exactos exigidos por la suite, fuzzing de syscalls sin pánico.
4. **Sin dependencias**: el núcleo entero es auditable sin salir del repositorio; ninguna crate externa.
5. **Cultura reciente de honestidad**: en las últimas tres builds se retiraron cifras inventadas ("48 ns vs Linux", "170x menos RAM que Windows"), la capa Win32 ficticia y un `fsync`/`chown` que devolvían 0 sin hacer nada; se sustituyeron por mediciones y por pruebas que fallan. La bitácora registra esos hallazgos con nombre y apellido.
6. **Arranque dual BIOS/UEFI** con núcleo en mitad superior, zona históricamente frágil, protegida por certificación en cada build.
7. **Documentación de decisiones**: `AGENTS.md` y `TICKETS.md` permiten reconstruir por qué cada cosa está como está.

---

## 5. Lo malo (debilidades, riesgos y deuda, sin suavizar)

### 5.1 Herencia de las "4 000 fases"
El proyecto pasó por una etapa (documentada en `docs/ROADMAP_*`, `RELEASE_NOTES_v4.0_COSMOS.md`, `BENCHMARKS_COMPARATIVE.md`) en la que se declararon "épicas" con nombres grandilocuentes y módulos cuyo nombre promete más de lo que hay: `drivers/gpu/{vaapi,drm,gem_ttm}.rs`, `drivers/storage/{luks,nix_store,ext4,fuse,immutable_cow,portix_dfs}.rs`, `net/{wireguard,smb_thunderbolt,wifi_80211,xdp}.rs`, `containers/`, `wasm/`, `security/`, `lkm/`, binarios como `portix-vulkan`, `portix-bluetooth`, `java`, `python`, `git`, `rustc`, `cargo-portix`, `portix-tpm`.
**Estado real**: una parte son estructuras y demostraciones que imprimen un `[X-PASS]` tras una comprobación mínima o inexistente; **no deben tomarse como funcionalidad** salvo que aparezcan en §3 con prueba externa. El proyecto decidió conservarlos como bitácora (directiva del responsable) y auditar/podar progresivamente: ya se eliminaron las pruebas tautológicas (recert "0 tautologías"), pero el código y parte de la documentación antigua siguen ahí. **Para un auditor, la regla práctica es: si no está en `docs/TICKETS.md` como cerrado con prueba, o en §3 de este documento, considérese no implementado.**

### 5.2 Arquitectura
- **`kernel/src/syscall/mod.rs` tiene 13 885 líneas** en un solo fichero: es el mayor riesgo de mantenibilidad del núcleo y el lugar donde más veces han aparecido comportamientos fingidos (por ejemplo `lchown`/`chown` devolviendo 0). Debe trocearse por subsistemas.
- **No es microkernel en el sentido estricto**: controladores en Ring 0; la promesa "drivers reiniciables en Ring 3" es el ticket T-008, sin empezar.
- **Un solo núcleo de CPU** efectivo para el planificador; no hay SMP verificado.
- **Sin hilos reales** (`clone` con `CLONE_THREAD`): bloquea `rustc`, `cargo`, la mayoría de software moderno (T-005).
- **Sólo musl**: glibc (T-004) es la puerta a "cualquier distro"; hoy un binario de Debian/Fedora no arranca.
- **ATA PIO** sirve la raíz (3–5 MiB/s en frío); NVMe/AHCI existen pero no son el camino de la raíz.
- Queda **código FAT "de rescate"** en el arranque y en syscalls (`OpenFileInfo`, `load_ring3_init`, `fat32_list_dir`) heredado de cuando la raíz era FAT: resto pendiente de T-001.
- FIFOs (`S_IFIFO`) sólo en ramfs; el resto de nodos especiales en ext2 devuelven EPERM.
- La caché de bloques es de escritura inmediata y directa (sin asociatividad): segura, pero lejos de una caché de páginas unificada.
- Arranque de 8–11 s en QEMU: razonable para un sistema sin optimizar, pero lejos de cualquier cifra "sub-segundo" que se haya escrito en documentos antiguos.

### 5.3 Verificación: límites honestos
- **Todo en QEMU**. Hardware real no probado en este ciclo. Los "ensayos de silicio" verifican que los controladores hablan con dispositivos emulados reales por QEMU, no con silicio físico.
- **15 pruebas de `test_posix` (2885–3176) fallan de forma permanente y aceptada**: corresponden a la vieja cadena de herramientas simulada ("Grand Master", `gcc`/`cargo` internos). Deberían eliminarse o reescribirse contra las herramientas reales (tcc hoy, gcc musl en T-003).
- Hay pruebas **sensibles al tiempo del anfitrión** (ventana dinámica TCP, `portix-fuzz`, Épica 59): en esta ronda hubo tres fallos transitorios que pasaron al repetir. Están documentados, no ocultos, pero un CI estricto los vería como inestabilidad.
- La prueba negativa "modo degradado sin P3" todavía no existe.

### 5.4 Proceso y gobernanza
- **No hay control de versiones**: el árbol no es un repositorio git. La trazabilidad depende de `AGENTS.md`. Es el defecto de proceso más grave para un externo: no hay diffs, ni *blame*, ni posibilidad de bisecar.
- **Un solo autor efectivo** (un agente dirigido por una persona): *bus factor* 1.
- **Documentación voluminosa y desigual**: 27 ficheros en `docs/`, varios obsoletos o contradictorios con el estado real (roadmaps de 500/1000/1500/2100/3000/4000 fases, `BENCHMARKS_COMPARATIVE.md`). Los vigentes son `ROADMAP.md`, `docs/TICKETS.md`, `AGENTS.md`, `docs/MITAD_SUPERIOR.md`, `docs/AUDITORIA_PRUEBAS.md`, `docs/PORTIX_STATUS_REAL_VERIFICADO.md` y este documento.
- **Seguridad no auditada**: sin revisión externa de aislamiento entre procesos, validación de punteros de usuario (existe `verify_user_*`, y el fuzzer la ejercita), ni de la pila TCP frente a tráfico hostil.
- Licencia y origen del código de terceros en `build/packages` (busybox, musl, tcc de Alpine): se descargan sin modificar; conviene documentar explícitamente las licencias en el repositorio.

---

## 6. Lo que falta: plan vigente con criterios de aceptación

Fuente: `docs/TICKETS.md`. Orden acordado con el responsable.

| Ticket | Qué | Prueba que lo cierra | Estado |
|---|---|---|---|
| T-011 | Medir en vez de afirmar | `bench` con rangos plausibles, dos ejecuciones distintas, invariante del buddy | **Cerrado** (b175/176) |
| T-001 | ext2 completo y rápido | caché ≥3×, TIND visto por `debugfs`, `link`/`chown`/`fsync` reales, FIFOs, arranque sin P3 en modo degradado explícito | **Cerrado** (b182) |
| T-002 | `.apk` real de Alpine (formato, firma, dependencias) | `pkg add` de un `.apk` oficial y ejecución del binario | **Cerrado** (b183: `pkg add tcc tcc-libs-static musl-dev` desde `dl-cdn.alpinelinux.org`, firma RSA + C:Q1 + datahash, enlaces simbólicos seguidos; suite 59 3616/3616, recert 97) |
| T-003 | `gcc` de Alpine (musl) compilando dentro de Portix | `make` de un proyecto C real | Pendiente |
| T-004 | glibc + `.deb`/`.rpm` | binario de Debian sin modificar arranca | Pendiente; abre "cualquier distro" |
| T-005 | Hilos reales | `rustc`/`cargo` compilan un crate | Pendiente |
| T-006 | Wayland en `portix-wm` + `/dev/dri` (llvmpipe) | cliente Wayland ajeno dibuja | Pendiente |
| T-007 | Wine sin modificar (único camino para `.exe`) | condicionado a T-004/005/006 | Condicionado |
| T-008 | Controladores reiniciables en Ring 3 | matar el driver de red → `curl` vuelve a funcionar | Pendiente (identidad) |
| T-009 | Actualización A/B atómica | raíz corrupta → arranca la anterior | Pendiente (identidad) |
| T-010 | Imagen reproducible bit a bit | mismo SHA-256 en dos builds | Pendiente (identidad) |

Deuda menor anotada: MMIO con PD alto UC, `PROT_NONE` estricto, `inotify` en ext2/devfs, `cd` del shell del núcleo sin efecto en `run`, PCC v1.3 (compilador propio, baja prioridad frente a gcc real).

---

## 7. Madurez por área (semáforo)

| Área | Madurez | Comentario |
|---|---|---|
| Arranque BIOS/UEFI, memoria, paginación | 🟢 | Certificado en cada build; zona protegida |
| Procesos, señales, ABI Linux (musl) | 🟢 | Software ajeno real funciona; tabla Linux incompleta pero medible |
| ext2 raíz | 🟢 | Verificado por e2fsprogs; rendimiento aún modesto |
| Red (QEMU) | 🟡 | Funciona y descarga de Internet; sin auditoría de seguridad ni pruebas de carga |
| Gráficos/ventanas | 🟡 | Real y probado por entrada emulada; protocolo propio, sin Wayland |
| Herramientas de desarrollo dentro del sistema | 🟡 | tcc real sí; gcc/rustc no (T-003/T-005) |
| Gestión de paquetes | 🟡 | Descarga y SHA-256 reales; formato propio, no `.apk` |
| Controladores NVMe/AHCI/USB/HDA | 🟡 | Hablan con dispositivos emulados; no son el camino de la raíz; sin hardware físico |
| Módulos "épicos" (GPU, LUKS, WireGuard, Vulkan, Java, TPM…) | 🔴 | No considerar implementados; pendientes de poda o reescritura |
| Microkernel (drivers fuera de Ring 0), SMP, hilos | 🔴 | No existen todavía |
| Proceso (git, CI, revisión externa, licencias) | 🔴 | Sin control de versiones ni CI |

---

## 8. Cómo auditar en 30 minutos (reproducible)

```bash
bash scripts/entorno.sh                      # toolchain Rust nightly, nasm, qemu, e2fsprogs
export PATH=$HOME/.cargo/bin:$PATH
python3 scripts/build.py --no-run            # construye núcleo, userland e imágenes en build/dist
python3 -u scripts/headless_runner.py --timeout 30   # ~17 min; debe terminar en FAILED: 0
python3 -u scripts/recertify.py --no-build           # ~6 min; debe imprimir [PORTIX-CHRONOS-MASTER-PASS]
```
Comprobaciones independientes del código de Portix que el auditor puede hacer a mano:
- `dd if=build/dist/portix.img bs=512 skip=73728 count=131072 of=/tmp/p3.img && e2fsck -fn /tmp/p3.img` (tras una ejecución, con la copia `/tmp/portix-run-*.img`).
- `debugfs -R "stat /var/t001/disperso" /tmp/p3.img` → debe mostrar `(TIND)`.
- En el sistema: `bench` (cifras medidas), `run portix-bench` (latencia de syscall), `run /bin/busybox uname -m` tras `run pkg install busybox-static`.
- Buscar afirmaciones sin prueba: `grep -rn "PASS\]" kernel/src/ui kernel/src/system | grep -v BENCH-REAL` y contrastar cada una con la suite.

---

## 9. Veredicto

Portix es hoy un **núcleo experimental serio y honesto en su núcleo duro** (arranque, memoria, procesos, ABI Linux con musl, ext2, red básica, pruebas) rodeado de una **corteza de código y documentación heredada que promete más de lo que hay** y que el propio proyecto ha empezado a podar con criterio. Su activo diferencial no es una cifra de rendimiento sino la **verificabilidad**: cualquiera puede reconstruirlo y ver fallar sus pruebas.

Las tres recomendaciones más urgentes para un externo serían, por este orden:
1. **Poner el árbol bajo git** y ejecutar la suite como CI en cada cambio; sin esto, el resto de garantías dependen de la bitácora.
2. **Podar o etiquetar explícitamente** los módulos y binarios "épicos" no verificados (una lista en `TICKETS.md` con "retirado"/"reescribir"), y borrar las 15 pruebas permanentemente fallidas de `test_posix`.
3. **Trocear `syscall/mod.rs`** antes de T-004 (glibc), que añadirá decenas de syscalls más.

Con eso hecho, el plan T-001 → T-002 → T-003 → T-004 es coherente con el titular del proyecto y cada paso tiene una prueba que puede fallar.
