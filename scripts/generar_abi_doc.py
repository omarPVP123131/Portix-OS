#!/usr/bin/env python3
"""Genera docs/ABI_SYSCALLS.md a partir del código del núcleo.

Las tablas y los números salen de `scripts/abi_inventario.py`; este script sólo
añade la prosa. Se usa una plantilla con marcadores @@…@@ para no escapar llaves
de Rust dentro de f-strings.
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SALIDA = ROOT / "docs/ABI_SYSCALLS.md"

ALTO = {23, 40, 46, 47, 48, 58, 97, 100, 105, 106, 109, 112, 113, 114, 115, 116,
        117, 118, 119, 120, 121, 124, 128, 129, 132, 135, 140, 141, 142, 143,
        144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 155, 157, 161, 163,
        165, 166, 167, 168, 169, 170, 171, 172, 173, 179, 186, 190, 191, 192,
        195, 196}
TRIVIAL = ("shm", "sem", "msg", "mq_", "kexec", "perf", "bpf", "userfaultfd",
           "memfd", "pidfd", "io_uring", "keyctl", "request_key", "add_key",
           "finit_module", "init_module", "delete_module", "reboot", "swapon",
           "swapoff", "quotactl", "acct", "nfsservctl", "lookup_dcookie",
           "tuxcall", "uselib", "ustat", "sysfs", "vhangup", "pivot_root",
           "chroot", "mount", "umount2", "settimeofday", "clock_settime",
           "adjtimex", "sethostname", "setdomainname", "iopl", "ioperm",
           "personality", "capget", "capset", "kcmp", "process_vm",
           "modify_ldt", "arch_prctl", "ioprio", "membarrier", "pkey_",
           "copy_file_range", "preadv2", "pwritev2", "rseq", "move_pages",
           "mbind", "set_mempolicy", "get_mempolicy", "migrate_pages",
           "readahead", "sync_file_range", "splice", "tee", "vmsplice")

NOTAS = {
    0: "lectura; una página perezosa se materializa en nombre del usuario",
    1: "escritura; desde T-042, con `count ≤ 4096` valida el rango COMPLETO → `EFAULT` en vez de "
       "cuenta corta (atomicidad de `PIPE_BUF`)",
    9: "`mmap` perezoso (reserva sin marco) y respaldo por fichero (T-014b)",
    11: "`munmap` no destruye imagen ELF, heap de `brk` ni pila del proceso",
    16: "`ioctl`: TCGETS de 60 B corregido en T-004",
    28: "`MADV_DONTNEED` también descarta páginas protegidas y de sólo lectura desde T-042",
    53: "**añadida en T-042**; antes `ENOSYS` (38) porque sólo existía como 105 del dialecto nativo",
    56: "`clone` con CLONE_VM|THREAD|SIGHAND|SETTLS|PARENT_SETTID|CHILD_CLEARTID (hilos reales, T-005)",
    59: "carga el intérprete `PT_INTERP` antes de arrancar el ELF",
    60: "termina **sólo el hilo llamador**",
    202: "futex real con cola: WAIT/WAIT_BITSET/WAKE/WAKE_BITSET/REQUEUE/CMP_REQUEUE y variantes PI",
    221: "`F_SETFL` guarda `O_NONBLOCK` (0x800) en tuberías y socketpair desde T-042; antes devolvía "
         "0 sin guardar nada",
    231: "`exit_group`; cerrado con la sonda T-031 (cierre desde un hilo en CPU 1)",
    257: "`openat`; `statx` con `AT_EMPTY_PATH` corregido en b203",
    435: "`clone3` (stack + stack_size)",
}

PLANTILLA = r"""# ABI de syscalls de Portix — inventario, traducción y guía de trabajo

**Estado:** 31 de octubre de 2026 · lote b203, ciclo T-042.
**Fuente:** generado a partir del código (`kernel/src/syscall/numbers.rs`, `kernel/src/syscall/mod.rs`,
`kernel/src/arch/isr.asm`) y de `unistd_64.h` del anfitrión. **No** es una declaración de intenciones:
es lo que el núcleo despacha hoy.

> Regenerar: `python3 scripts/generar_abi_doc.py`
> Comprobar que no envejece: `python3 -m unittest scripts.test_abi_inventario`

---

## 0. Resumen

| | Número | Qué significa |
|---|---:|---|
| Constantes de la **ABI nativa** | @@NAT_CONST@@ | dialecto propio de Portix (`numbers.rs`) |
| Nativas con brazo en el despachador | @@NAT_BRAZO@@ | sin brazo: @@NAT_SIN_BRAZO@@ |
| Llamadas **Linux x86-64** que existen | @@LIN_TOTAL@@ | referencia del anfitrión |
| Linux **traducidas** por Portix | **@@LIN_IMPL@@** | con brazo explícito en `linux_syscall_match` |
| Linux **sin traducir** | **@@LIN_FALTAN@@** | devuelven `-ENOSYS (38)` y se contabilizan |
| Números nativos que en Linux son **otra llamada** | @@CHOQUES@@ | el motivo de la sección 1 |

---

## 1. Hay dos ABI, y se eligen por la instrucción, no por una bandera

| Puerta | Instrucción | Despachador | Tabla de números | Quién entra |
|---|---|---|---|---|
| **Nativa** | `int 0x80` | `syscall_dispatch()` → `syscall_dispatch_core()` | `numbers.rs` | userland propio de Portix |
| **Linux x86-64** | `syscall` | `linux_syscall_dispatch()` → `linux_syscall_match()` | literales de Linux | ELF de distro sin modificar |

`kernel/src/arch/isr.asm`: `int80_handler` llama a `syscall_dispatch` (línea 577);
`syscall_entry` llama a `linux_syscall_dispatch` (línea 623).

**Las dos tablas se pisan.** De los números nativos que también existen en Linux, @@CHOQUES@@ significan
otra cosa distinta:

| nr | Portix (nativa) | Linux x86-64 |
|---:|---|---|
| 0 | `SYS_EXIT` | `read` |
| 2 | `SYS_GETPID` | `open` |
| 5 | `SYS_READ` | `fstat` |
| 11 | `SYS_EXECVE` | `munmap` |
| 23 | `SYS_WAITPID` | `select` |
| 105 | `SYS_SOCKETPAIR` | `setuid` |

Mientras cada binario entre por su puerta la colisión es inocua; **se vuelve un fallo de seguridad en
cuanto algo delegue de una capa a la otra**. Eso fue `T-038` (el `socketpair` de Linux estaba sólo en el
número 105 del dialecto nativo, así que la 53 real devolvía `ENOSYS`) y es la razón por la que el brazo
por defecto de la capa Linux **no** delega en la nativa (`FRENTE C-5`): un binario Linux podría haber
ejecutado `exit`, `execve` o `ioport` del núcleo.

---

## 2. Reglas de oro

1. **No se reutiliza un número** de la tabla nativa. Se añade al final y se documenta aquí.
2. Una syscall **de Linux se escribe con el número de Linux** en `linux_syscall_match`, nunca con la
   constante `SYS_*` nativa. Copiar la constante es exactamente lo que produjo T-038.
3. **La capa Linux nunca delega en la nativa**: si falta un brazo se devuelve `-ENOSYS (38)` y se cuenta.
4. Toda llamada nueva lleva **prueba con valor exacto** (errno concreto, código de salida concreto) **y un
   control negativo** que la pueda hacer fallar.
5. **Se valida el rango de usuario completo** antes de escribir cuando la publicación es atómica
   (`count ≤ 4096`, `PIPE_BUF`): o entra todo, o `EFAULT`. Para escrituras grandes se admite cuenta corta.
6. **Los flags se guardan de verdad.** Devolver 0 sin guardar convierte `O_NONBLOCK` en decorativo y
   cuelga la siguiente lectura (segunda causa de T-037).
7. Lo que no está aquí **no está prometido**: `abicoverage` y `/proc/abi` son una medición de la sesión.

---

## 3. ABI nativa de Portix (@@NAT_CONST@@ constantes, @@NAT_BRAZO@@ con brazo)

Userland propio. Incluye llamadas **que no son POSIX** y no tienen equivalente Linux: `REG_IRQ`,
`IOPORT`, `IOREAD`, `IOWRITE`, `MMAP_DEVICE`, `GET_MOUSE`, `POLL_KEY`, `BLOCK_READ`.
Declaradas sin brazo: @@NAT_SIN_BRAZO@@ — hoy caen en el brazo por defecto y devuelven `ENOSYS`.

@@TABLA_NAT@@

---

## 4. ABI Linux x86-64: las @@LIN_IMPL@@ traducidas

De las @@LIN_TOTAL@@ llamadas de x86-64, el núcleo traduce **@@LIN_IMPL@@**. La última columna sólo se
rellena donde el comportamiento es propio o se ha corregido con medida.

@@TABLA_LINUX@@

---

## 5. Las @@LIN_FALTAN@@ sin traducir, por riesgo real

Clasificadas por impacto en un binario de distro, no por rareza.

### 5.1 Riesgo alto — @@N_ALTO@@ que un programa normal puede usar

@@ALTO@@

**Las que más duelen, y por qué:**

- **`97 getrlimit`**: `prlimit64` (302) sí está y glibc moderna lo usa, pero el software estático y
  antiguo llama `getrlimit` y recibe `ENOSYS`.
- **`105 setuid`, `106 setgid`, `113/114 setreuid/setregid`, `117-120 get/setresuid/gid`**: cualquier
  `su`, `login` o `sudo` falla aquí. Devolver `ENOSYS` es más honesto que fingir éxito, y también más
  rompedor: hay que decidirlo a propósito.
- **`121 getpgid`, `109 setpgid`, `112 setsid`, `124 getsid`**: control de trabajos y demonios. Sin
  `setsid` no hay servicio que se despegue de la terminal (`getpgrp` = 111 sí está).
- **`46 sendmsg`, `47 recvmsg`, `48 shutdown`**: camino normal de `sendmmsg`, de parte de Python y de
  TLS; hoy sólo existen `sendto`/`recvfrom`/`send`/`recv`.
- **`23 select`**: `poll` (7) está; `select` lo usan programas viejos y scripts.
- **`40 sendfile`**: servidores de ficheros; los modernos caen a `read`+`write`, los antiguos no.
- **`100 times`, `132 utime`, `140/141 get/setpriority`, `149-152 mlock/munlock*`,
  `128 rt_sigtimedwait`, `129 rt_sigqueueinfo`, `135 personality`**: uso corriente en herramientas del
  sistema.

### 5.2 Riesgo medio — @@N_MEDIO@@ de subsistema

@@MEDIO@@

### 5.3 Riesgo bajo — @@N_BAJO@@ exóticas o de kernel

@@BAJO@@

Incluye IPC System V, `io_uring`, `bpf`, `perf_event_open`, `kcmp`, `mount`/`pivot_root`/`chroot`,
`reboot`, `swapon`, `keyctl`… Implementarlas es legítimo; **no** es lo que separa a Portix de «ejecuta
binarios de distro».

---

## 6. Cómo se añade o corrige una syscall

### Caso A — falta la llamada (T-038, `socketpair`)

1. **Medir primero**: ejecutar el programa que falla y leer `[ABI] syscall Linux sin traduccion nr=N`.
   Ése es el número que hay que mirar, no el que uno supone.
2. Ver si la función ya existe en el núcleo (`sys_socketpair` existía y funcionaba por la puerta nativa).
3. Añadir el brazo **con el número de Linux**:

```rust
53 => { // sys_socketpair(domain, type, protocol, sv)
    SyscallResult(sys_socketpair(a1 as i32, a2 as i32, a3 as i32, a4 as usize) as u64, 0)
}
```

4. **Prueba que pueda fallar**: la sonda ya tenía los modos `socket-invalido` y `socket-parcial`, que
   fallaban con `rc:-1,errno:38`. Tras el arreglo, PASS. Ése es el criterio: mismos predicados exactos,
   antes rojo y después verde.
5. Actualizar este documento, `docs/TICKETS.md` y `CHANGELOG.md`.

### Caso B — la llamada existe pero el comportamiento es incorrecto (T-037)

1. La sonda ya comparaba con Linux (`rc:-1,errno:14`) y el núcleo devolvía cuenta corta.
2. Se buscó **la causa**, no el síntoma: `verify_user_buffer` validaba el rango y la ruta de tubería
   copiaba por trozos con `break`.
3. Se corrigió respetando Linux (atomicidad de `PIPE_BUF`) sin cambiar las escrituras grandes.
4. Se reejecutó la batería completa: `21/30 → 30/30`.

### Lista de comprobación

- [ ] Número correcto **para la capa** (Linux frente a nativa).
- [ ] Sin colisión nueva que pueda confundir a otro.
- [ ] Punteros de usuario validados **por página**, no sólo por rango.
- [ ] `errno` exacto y **efecto nulo** al fallar (nada escrito, nada asignado).
- [ ] Prueba con valor exacto **y** control negativo.
- [ ] Este documento + ticket + changelog actualizados.

---

## 7. Cómo se depura

| Herramienta | Qué da | Activación |
|---|---|---|
| `[ABI] syscall Linux sin traduccion nr=N -> ENOSYS` | el número exacto que falta | siempre, en consola serie |
| `[SYS] …` | traza de las llamadas implementadas | siempre |
| `STRACE_PREFIJO` (`kernel/src/syscall/mod.rs`) | nr, a1..a3 y valor devuelto **para un binario concreto** | compilar con `Some("/usr/bin/…")` |
| `abicoverage` / `/proc/abi` | contador y mapa de bits de números sin traducir vistos **en la sesión** | comando de consola |
| `[EFAULT] nr=… pid=… cpu=…` | quién provocó un `EFAULT` | siempre |

**Regla de lectura:** si un binario «falla raro», lo primero es buscar `[ABI]` en la consola. Si aparece,
no es un bug de comportamiento: es una llamada que no existe.

---

## 8. Cómo se prueba

- **Sonda de referencia**: `scripts/sondas/t035_memoria.rs` (31 modos) mediante
  `scripts/sonda_t035_b203.py` (BIOS) y `scripts/sonda_t035_uefi.py` (UEFI, misma batería).
  Cada modo imprime `[T035] modo=X inicio` y acaba en `[T035] PASS/ERROR modo=X fase=… esperado=…
  observado=…`; el anfitrión compara **etapas exactas y el código de salida del PID líder**.
- **Cierre de grupo e hilos**: `scripts/sonda_t031_b203.py` (8/8) y la sección `T-031` de
  `headless_runner.py`.
- **Referencia Linux**: los mismos fixtures compilados con glibc en el anfitrión, para distinguir «Portix
  está mal» de «el fixture está mal».

---

## 9. Trampas conocidas

1. **Doble tabla.** `SYS_SOCKETPAIR = 105` es correcto para la ABI **nativa** y no se toca; la capa Linux
   usa literales. Leer `numbers.rs` como si fuera Linux produce errores como T-038.
2. **`numbers.rs` no describe la ABI Linux.**
3. El texto de `devsuite.rs` decía que un número sin brazo «se delegaba al ABI nativo»: dejó de ser cierto
   en `FRENTE C-5` y **se corrigió en este ciclo**.
4. **`fcntl` que devuelve 0 sin guardar** cuelga la siguiente lectura (T-037).
5. **`MADV_DONTNEED` sobre páginas no presentes** se ignoraba en silencio (T-036).

---

## Anexo — verificarse

```bash
python3 scripts/abi_inventario.py                 # resumen (los números de esta página)
python3 scripts/abi_inventario.py --tablas        # tablas markdown desde el código
python3 -m unittest scripts.test_abi_inventario   # el documento no se ha desviado del código
```

El control **falla** si el número de traducidas o de sin traducir del documento deja de coincidir con el
código: así el inventario no envejece en silencio.
"""


def compacto(nums, ref):
    filas, fila = [], []
    for n in nums:
        fila.append(f"`{n}` {ref.get(n, '?')}")
        if len(fila) == 3:
            filas.append(" · ".join(fila))
            fila = []
    if fila:
        filas.append(" · ".join(fila))
    return "\n".join(f"- {f}" for f in filas)


def main():
    inv = json.loads(subprocess.run([sys.executable, str(ROOT / "scripts/abi_inventario.py"), "--json"],
                                    capture_output=True, text=True, check=True).stdout)
    nat, lin = inv["nativa"], inv["linux"]
    ref = {int(k): v for k, v in inv["referencia"].items()}
    impl, faltan = lin["traducidas"], lin["faltan"]

    grupos = {"alto": [], "medio": [], "bajo": []}
    for n in faltan:
        nombre = ref.get(n, "?")
        if n in ALTO:
            grupos["alto"].append(n)
        elif any(t in nombre for t in TRIVIAL):
            grupos["bajo"].append(n)
        else:
            grupos["medio"].append(n)

    pares = sorted(nat["constantes"].items(), key=lambda x: x[1])
    filas_nat = []
    for k in range(0, len(pares), 4):
        celdas = [f"`{n:3d}` {c.replace('SYS_', '')}" for c, n in pares[k:k + 4]]
        celdas += [""] * (4 - len(celdas))
        filas_nat.append("| " + " | ".join(celdas) + " |")
    tabla_nat = "| | | | |\n|---|---|---|---|\n" + "\n".join(filas_nat)

    filas_lin = ["| nr | llamada | comportamiento propio y notas |", "|---:|---|---|"]
    for n in impl:
        filas_lin.append(f"| {n} | `{ref.get(n, '?')}` | {NOTAS.get(n, '')} |")

    sin_brazo = ", ".join(f"`{x.replace('SYS_', '')}`" for x in nat["sin_brazo"])
    doc = PLANTILLA
    for marca, valor in [
        ("@@NAT_CONST@@", len(nat["constantes"])),
        ("@@NAT_BRAZO@@", len(nat["con_brazo"])),
        ("@@NAT_SIN_BRAZO@@", sin_brazo),
        ("@@LIN_TOTAL@@", lin["total_linux"]),
        ("@@LIN_IMPL@@", len(impl)),
        ("@@LIN_FALTAN@@", len(faltan)),
        ("@@CHOQUES@@", len(inv["choques"])),
        ("@@TABLA_NAT@@", tabla_nat),
        ("@@TABLA_LINUX@@", "\n".join(filas_lin)),
        ("@@N_ALTO@@", len(grupos["alto"])),
        ("@@N_MEDIO@@", len(grupos["medio"])),
        ("@@N_BAJO@@", len(grupos["bajo"])),
        ("@@ALTO@@", compacto(grupos["alto"], ref)),
        ("@@MEDIO@@", compacto(grupos["medio"], ref)),
        ("@@BAJO@@", compacto(grupos["bajo"], ref)),
    ]:
        doc = doc.replace(marca, str(valor))
    SALIDA.write_text(doc)
    print(f"{SALIDA.relative_to(ROOT)}: {len(doc)} bytes")
    print(f"  nativas={len(nat['constantes'])} linux_impl={len(impl)} "
          f"faltan={len(faltan)} (alto={len(grupos['alto'])}, medio={len(grupos['medio'])}, "
          f"bajo={len(grupos['bajo'])}) choques={len(inv['choques'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
