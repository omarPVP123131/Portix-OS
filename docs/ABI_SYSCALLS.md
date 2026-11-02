# ABI de syscalls de Portix — inventario, traducción y guía de trabajo

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
| Constantes de la **ABI nativa** | 354 | dialecto propio de Portix (`numbers.rs`) |
| Nativas con brazo en el despachador | 352 | sin brazo: `READLINK_FULL`, `SYMLINK_FULL` |
| Llamadas **Linux x86-64** que existen | 375 | referencia del anfitrión |
| Linux **traducidas** por Portix | **141** | con brazo explícito en `linux_syscall_match` |
| Linux **sin traducir** | **234** | devuelven `-ENOSYS (38)` y se contabilizan |
| Números nativos que en Linux son **otra llamada** | 333 | el motivo de la sección 1 |

---

## 1. Hay dos ABI, y se eligen por la instrucción, no por una bandera

| Puerta | Instrucción | Despachador | Tabla de números | Quién entra |
|---|---|---|---|---|
| **Nativa** | `int 0x80` | `syscall_dispatch()` → `syscall_dispatch_core()` | `numbers.rs` | userland propio de Portix |
| **Linux x86-64** | `syscall` | `linux_syscall_dispatch()` → `linux_syscall_match()` | literales de Linux | ELF de distro sin modificar |

`kernel/src/arch/isr.asm`: `int80_handler` llama a `syscall_dispatch` (línea 577);
`syscall_entry` llama a `linux_syscall_dispatch` (línea 623).

**Las dos tablas se pisan.** De los números nativos que también existen en Linux, 333 significan
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

## 3. ABI nativa de Portix (354 constantes, 352 con brazo)

Userland propio. Incluye llamadas **que no son POSIX** y no tienen equivalente Linux: `REG_IRQ`,
`IOPORT`, `IOREAD`, `IOWRITE`, `MMAP_DEVICE`, `GET_MOUSE`, `POLL_KEY`, `BLOCK_READ`.
Declaradas sin brazo: `READLINK_FULL`, `SYMLINK_FULL` — hoy caen en el brazo por defecto y devuelven `ENOSYS`.

| | | | |
|---|---|---|---|
| `  0` EXIT | `  1` WRITE | `  2` GETPID | `  3` YIELD |
| `  4` SLEEP | `  5` READ | `  6` OPEN | `  7` CLOSE |
| `  8` BRK | `  9` MMAP | ` 10` GETDIRENTS | ` 11` EXECVE |
| ` 12` DUP2 | ` 13` UPTIME | ` 14` SEND | ` 15` RECV |
| ` 16` REG_IRQ | ` 17` BLOCK_READ | ` 18` IOPORT | ` 19` IOREAD |
| ` 20` IOWRITE | ` 21` MMAP_DEVICE | ` 22` FORK | ` 23` WAITPID |
| ` 24` LSEEK | ` 25` STAT | ` 26` FSTAT | ` 27` KILL |
| ` 28` GETCWD | ` 29` CHDIR | ` 30` UNLINK | ` 31` PIPE |
| ` 32` TIME | ` 33` MKDIR | ` 34` SOCKET | ` 35` BIND |
| ` 36` SENDTO | ` 37` RECVFROM | ` 38` INIT_MODULE | ` 39` DELETE_MODULE |
| ` 40` QUERY_MODULE | ` 41` GET_MOUSE | ` 42` POLL_KEY | ` 43` GETUID |
| ` 44` SETUID | ` 45` GETGID | ` 46` SETGID | ` 47` MUNMAP |
| ` 48` DUP | ` 49` FCNTL | ` 50` GETPPID | ` 51` UNAME |
| ` 52` FTRUNCATE | ` 53` TRUNCATE | ` 54` GETEUID | ` 55` GETEGID |
| ` 56` RMDIR | ` 57` ACCESS | ` 58` RENAME | ` 59` SETEUID |
| ` 60` SETEGID | ` 61` PIPE2 | ` 62` NANOSLEEP | ` 63` GETPGRP |
| ` 64` SETPGID | ` 65` GETPGID | ` 66` SETSID | ` 67` GETSID |
| ` 68` UMASK | ` 69` POLL | ` 70` CLOCK_GETTIME | ` 71` CLOCK_GETRES |
| ` 72` STATVFS | ` 73` SIGACTION | ` 74` SIGPROCMASK | ` 75` SIGPENDING |
| ` 76` GETRLIMIT | ` 77` SETRLIMIT | ` 78` GETPRIORITY | ` 79` SETPRIORITY |
| ` 80` READLINK | ` 81` SYMLINK | ` 82` LSTAT | ` 83` TIMES |
| ` 84` GETGROUPS | ` 85` SETGROUPS | ` 86` FSYNC | ` 87` FDATASYNC |
| ` 88` CHMOD | ` 89` FCHMOD | ` 90` CHOWN | ` 91` FCHOWN |
| ` 92` PAUSE | ` 93` ALARM | ` 94` SCHED_YIELD | ` 95` GETRESUID |
| ` 96` SETRESUID | ` 97` GETRESGID | ` 98` SETRESGID | ` 99` LINK |
| `100` READV | `101` WRITEV | `102` PREAD64 | `103` PWRITE64 |
| `104` MPROTECT | `105` SOCKETPAIR | `106` GETTID | `107` SCHED_GETPARAM |
| `108` SCHED_SETPARAM | `109` SCHED_GETSCHEDULER | `110` SCHED_SETSCHEDULER | `111` PRCTL |
| `112` SYNC | `113` FSTATVFS | `114` EVENTFD2 | `115` DUP3 |
| `116` TGKILL | `117` SCHED_GET_PRIORITY_MAX | `118` SCHED_GET_PRIORITY_MIN | `119` FUTEX |
| `120` EPOLL_CREATE1 | `121` EPOLL_CTL | `122` EPOLL_WAIT | `123` SYSINFO |
| `124` GETRANDOM | `125` OPENAT | `126` MKDIRAT | `127` UNLINKAT |
| `128` READLINKAT | `129` FSTATAT | `130` IOCTL | `131` MEMFD_CREATE |
| `132` SETSOCKOPT | `133` GETSOCKOPT | `134` SETHOSTNAME | `135` GETHOSTNAME |
| `136` GETTIMEOFDAY | `137` GETITIMER | `138` SETITIMER | `139` MKNOD |
| `140` FCHDIR | `141` SYMLINKAT | `142` LINKAT | `143` RENAMEAT |
| `144` FCHMODAT | `145` FCHOWNAT | `146` FACCESSAT | `147` UTIMENSAT |
| `148` GETRUSAGE | `149` PRLIMIT64 | `150` CLOCK_NANOSLEEP | `151` TIMERFD_CREATE |
| `152` TIMERFD_SETTIME | `153` TIMERFD_GETTIME | `154` SIGNALFD4 | `155` POSIX_OPENPT |
| `156` PTSNAME_R | `157` FADVISE64 | `158` FALLOCATE | `159` MEMBARRIER |
| `160` COPY_FILE_RANGE | `161` SPLICE | `162` TEE | `163` SYNCFS |
| `164` STATX | `165` CLOSE_RANGE | `166` PIDFD_OPEN | `167` PIDFD_SEND_SIGNAL |
| `168` GETCPU | `169` MREMAP | `170` PPOLL | `171` FSTATFS |
| `172` READAHEAD | `173` EPOLL_PWAIT | `174` PREADV | `175` PWRITEV |
| `176` FACCESSAT2 | `177` MSYNC | `178` MINCORE | `179` MADVISE |
| `180` MLOCK | `181` MUNLOCK | `182` MLOCKALL | `183` MUNLOCKALL |
| `184` RENAMEAT2 | `185` MEMFD_SECRET | `186` EPOLL_PWAIT2 | `187` PROCESS_MADVISE |
| `188` SCHED_YIELD_POSIX | `189` SETTIMEOFDAY | `190` SYNC_FILE_RANGE | `191` VMSPLICE |
| `192` KCMP | `193` PERSONALITY | `194` SETNS | `195` UNSHARE |
| `196` PIVOT_ROOT | `197` LOOKUP_DCOOKIE | `198` GETRANDOM2 | `199` SCHED_SETAFFINITY |
| `200` SCHED_GETAFFINITY | `201` CLOCK_SETTIME | `202` GETDENTS64 | `203` PIDFD_GETFD |
| `204` OPENAT2 | `205` PROCESS_MRELEASE | `206` SET_MEMPOLICY | `207` GET_MEMPOLICY |
| `208` IO_PGETEVENTS | `209` FSMOUNT | `210` MOVE_MOUNT | `211` CAPGET |
| `212` CAPSET | `213` SCHED_GETATTR | `214` SCHED_SETATTR | `215` FSOPEN |
| `216` FSPICK | `217` FSCONFIG | `218` QUOTACTL | `219` LANDLOCK_CREATE_RULESET |
| `220` LANDLOCK_ADD_RULE | `221` LANDLOCK_RESTRICT_SELF | `222` SECCOMP | `223` STATMOUNT |
| `224` LISTMOUNT | `225` SHUTDOWN | `226` GETSOCKNAME | `227` GETPEERNAME |
| `228` SENDMSG | `229` RECVMSG | `230` SHMGET | `231` SHMAT |
| `232` SHMDT | `233` SHMCTL | `234` SEMGET | `235` SEMOP |
| `236` SEMCTL | `237` MSGGET | `238` MSGSND | `239` MSGRCV |
| `240` MSGCTL | `241` TIMER_CREATE | `242` TIMER_SETTIME | `243` TIMER_GETTIME |
| `244` TIMER_GETOVERRUN | `245` TIMER_DELETE | `246` FUTEX_WAITV | `247` MLOCK2 |
| `248` IOPRIO_GET | `249` IOPRIO_SET | `250` SCHED_RR_GET_INTERVAL | `251` GET_ROBUST_LIST |
| `252` SET_ROBUST_LIST | `253` RESTART_SYSCALL | `254` KEXEC_LOAD | `255` KEXEC_FILE_LOAD |
| `256` PKEY_MPROTECT | `257` PKEY_ALLOC | `258` PKEY_FREE | `259` RSEQ |
| `260` IO_URING_SETUP | `261` IO_URING_ENTER | `262` IO_URING_REGISTER | `263` PREADV2 |
| `264` PWRITEV2 | `265` SETDOMAINNAME | `266` SWAPON | `267` SWAPOFF |
| `268` IOPERM | `269` IOPL | `270` USERFAULTFD | `271` MQ_OPEN |
| `272` MQ_UNLINK | `273` MQ_TIMEDSEND | `274` MQ_TIMEDRECEIVE | `275` MQ_NOTIFY |
| `276` MQ_GETSETATTR | `277` SET_THREAD_AREA | `278` GET_THREAD_AREA | `279` SET_TID_ADDRESS |
| `280` FANOTIFY_INIT | `281` FANOTIFY_MARK | `282` NAME_TO_HANDLE_AT | `283` OPEN_BY_HANDLE_AT |
| `284` FINIT_MODULE | `285` REMAP_FILE_PAGES | `286` SENDFILE | `287` CONNECT |
| `288` LISTEN | `289` ACCEPT | `290` ACCEPT4 | `291` INOTIFY_INIT |
| `292` INOTIFY_INIT1 | `293` INOTIFY_ADD_WATCH | `294` INOTIFY_RM_WATCH | `295` SETXATTR |
| `296` GETXATTR | `297` LISTXATTR | `298` REMOVEXATTR | `299` FSETXATTR |
| `300` FGETXATTR | `301` LSETXATTR | `302` LGETXATTR | `303` LLISTXATTR |
| `304` LREMOVEXATTR | `305` FLISTXATTR | `306` FREMOVEXATTR | `307` MKNODAT |
| `308` FUTIMESAT | `309` SELECT | `310` PSELECT6 | `311` ADJTIMEX |
| `312` CLOCK_ADJTIME | `313` MIGRATE_PAGES | `314` MOVE_PAGES | `315` QUOTACTL_FD |
| `316` EXECVEAT | `317` PERF_EVENT_OPEN | `318` BPF | `319` MOUNT_SETATTR |
| `320` SET_MEMPOLICY_HOME_NODE | `321` CACHESTAT | `322` SIGALTSTACK | `323` RT_SIGSUSPEND |
| `324` RT_SIGPENDING | `325` RT_SIGQUEUEINFO | `326` RT_SIGTIMEDWAIT | `327` ARCH_PRCTL |
| `328` FLOCK | `329` LCHOWN | `330` UTIMES | `331` CHROOT |
| `332` GETDOMAINNAME | `333` EVENTFD | `334` RT_TGSIGQUEUEINFO | `335` RECVMMSG |
| `336` SENDMMSG | `337` PROCESS_VM_READV | `338` PROCESS_VM_WRITEV | `339` SEMTIMEDOP |
| `340` ACCT | `341` USTAT | `342` SYSLOG | `343` VHANGUP |
| `344` MODIFY_LDT | `345` SETFSUID | `346` SETFSGID | `347` PTRACE |
| `348` MOUNT | `349` UMOUNT2 | `350` SYMLINK_FULL | `351` READLINK_FULL |
| `352` RT_SIGRETURN | `353` FB_INFO |  |  |

---

## 4. ABI Linux x86-64: las 141 traducidas

De las 375 llamadas de x86-64, el núcleo traduce **141**. La última columna sólo se
rellena donde el comportamiento es propio o se ha corregido con medida.

| nr | llamada | comportamiento propio y notas |
|---:|---|---|
| 0 | `read` | lectura; una página perezosa se materializa en nombre del usuario |
| 1 | `write` | escritura; desde T-042, con `count ≤ 4096` valida el rango COMPLETO → `EFAULT` en vez de cuenta corta (atomicidad de `PIPE_BUF`) |
| 2 | `open` |  |
| 3 | `close` |  |
| 4 | `stat` |  |
| 5 | `fstat` |  |
| 6 | `lstat` |  |
| 7 | `poll` |  |
| 8 | `lseek` |  |
| 9 | `mmap` | `mmap` perezoso (reserva sin marco) y respaldo por fichero (T-014b) |
| 10 | `mprotect` |  |
| 11 | `munmap` | `munmap` no destruye imagen ELF, heap de `brk` ni pila del proceso |
| 12 | `brk` |  |
| 13 | `rt_sigaction` |  |
| 14 | `rt_sigprocmask` |  |
| 15 | `rt_sigreturn` |  |
| 16 | `ioctl` | `ioctl`: TCGETS de 60 B corregido en T-004 |
| 17 | `pread64` |  |
| 18 | `pwrite64` |  |
| 19 | `readv` |  |
| 20 | `writev` |  |
| 21 | `access` |  |
| 22 | `pipe` |  |
| 24 | `sched_yield` |  |
| 25 | `mremap` |  |
| 26 | `msync` |  |
| 28 | `madvise` | `MADV_DONTNEED` también descarta páginas protegidas y de sólo lectura desde T-042 |
| 32 | `dup` |  |
| 33 | `dup2` |  |
| 34 | `pause` |  |
| 35 | `nanosleep` |  |
| 36 | `getitimer` |  |
| 37 | `alarm` |  |
| 38 | `setitimer` |  |
| 39 | `getpid` |  |
| 41 | `socket` |  |
| 42 | `connect` |  |
| 43 | `accept` |  |
| 44 | `sendto` |  |
| 45 | `recvfrom` |  |
| 49 | `bind` |  |
| 50 | `listen` |  |
| 51 | `getsockname` |  |
| 52 | `getpeername` |  |
| 53 | `socketpair` | **añadida en T-042**; antes `ENOSYS` (38) porque sólo existía como 105 del dialecto nativo |
| 54 | `setsockopt` |  |
| 55 | `getsockopt` |  |
| 56 | `clone` | `clone` con CLONE_VM|THREAD|SIGHAND|SETTLS|PARENT_SETTID|CHILD_CLEARTID (hilos reales, T-005) |
| 57 | `fork` |  |
| 59 | `execve` | carga el intérprete `PT_INTERP` antes de arrancar el ELF |
| 60 | `exit` | termina **sólo el hilo llamador** |
| 61 | `wait4` |  |
| 62 | `kill` |  |
| 63 | `uname` |  |
| 72 | `fcntl` |  |
| 73 | `flock` |  |
| 74 | `fsync` |  |
| 75 | `fdatasync` |  |
| 76 | `truncate` |  |
| 77 | `ftruncate` |  |
| 79 | `getcwd` |  |
| 80 | `chdir` |  |
| 81 | `fchdir` |  |
| 82 | `rename` |  |
| 83 | `mkdir` |  |
| 84 | `rmdir` |  |
| 85 | `creat` |  |
| 86 | `link` |  |
| 87 | `unlink` |  |
| 88 | `symlink` |  |
| 89 | `readlink` |  |
| 90 | `chmod` |  |
| 91 | `fchmod` |  |
| 92 | `chown` |  |
| 93 | `fchown` |  |
| 94 | `lchown` |  |
| 95 | `umask` |  |
| 96 | `gettimeofday` |  |
| 98 | `getrusage` |  |
| 99 | `sysinfo` |  |
| 101 | `ptrace` |  |
| 102 | `getuid` |  |
| 104 | `getgid` |  |
| 107 | `geteuid` |  |
| 108 | `getegid` |  |
| 110 | `getppid` |  |
| 111 | `getpgrp` |  |
| 127 | `rt_sigpending` |  |
| 130 | `rt_sigsuspend` |  |
| 131 | `sigaltstack` |  |
| 133 | `mknod` |  |
| 137 | `statfs` |  |
| 138 | `fstatfs` |  |
| 157 | `prctl` |  |
| 158 | `arch_prctl` |  |
| 162 | `sync` |  |
| 165 | `mount` |  |
| 166 | `umount2` |  |
| 186 | `gettid` |  |
| 200 | `tkill` |  |
| 201 | `time` |  |
| 202 | `futex` | futex real con cola: WAIT/WAIT_BITSET/WAKE/WAKE_BITSET/REQUEUE/CMP_REQUEUE y variantes PI |
| 203 | `sched_setaffinity` |  |
| 204 | `sched_getaffinity` |  |
| 217 | `getdents64` |  |
| 218 | `set_tid_address` |  |
| 221 | `fadvise64` | `F_SETFL` guarda `O_NONBLOCK` (0x800) en tuberías y socketpair desde T-042; antes devolvía 0 sin guardar nada |
| 228 | `clock_gettime` |  |
| 229 | `clock_getres` |  |
| 230 | `clock_nanosleep` |  |
| 231 | `exit_group` | `exit_group`; cerrado con la sonda T-031 (cierre desde un hilo en CPU 1) |
| 234 | `tgkill` |  |
| 257 | `openat` | `openat`; `statx` con `AT_EMPTY_PATH` corregido en b203 |
| 258 | `mkdirat` |  |
| 259 | `mknodat` |  |
| 260 | `fchownat` |  |
| 262 | `newfstatat` |  |
| 263 | `unlinkat` |  |
| 264 | `renameat` |  |
| 265 | `linkat` |  |
| 266 | `symlinkat` |  |
| 267 | `readlinkat` |  |
| 268 | `fchmodat` |  |
| 269 | `faccessat` |  |
| 273 | `set_robust_list` |  |
| 274 | `get_robust_list` |  |
| 280 | `utimensat` |  |
| 288 | `accept4` |  |
| 290 | `eventfd2` |  |
| 291 | `epoll_create1` |  |
| 292 | `dup3` |  |
| 293 | `pipe2` |  |
| 302 | `prlimit64` |  |
| 306 | `syncfs` |  |
| 309 | `getcpu` |  |
| 318 | `getrandom` |  |
| 332 | `statx` |  |
| 334 | `rseq` |  |
| 435 | `clone3` | `clone3` (stack + stack_size) |
| 436 | `close_range` |  |
| 439 | `faccessat2` |  |

---

## 5. Las 234 sin traducir, por riesgo real

Clasificadas por impacto en un binario de distro, no por rareza.

### 5.1 Riesgo alto — 56 que un programa normal puede usar

- `23` select · `40` sendfile · `46` sendmsg
- `47` recvmsg · `48` shutdown · `58` vfork
- `97` getrlimit · `100` times · `105` setuid
- `106` setgid · `109` setpgid · `112` setsid
- `113` setreuid · `114` setregid · `115` getgroups
- `116` setgroups · `117` setresuid · `118` getresuid
- `119` setresgid · `120` getresgid · `121` getpgid
- `124` getsid · `128` rt_sigtimedwait · `129` rt_sigqueueinfo
- `132` utime · `135` personality · `140` getpriority
- `141` setpriority · `142` sched_setparam · `143` sched_getparam
- `144` sched_setscheduler · `145` sched_getscheduler · `146` sched_get_priority_max
- `147` sched_get_priority_min · `148` sched_rr_get_interval · `149` mlock
- `150` munlock · `151` mlockall · `152` munlockall
- `153` vhangup · `155` pivot_root · `161` chroot
- `163` acct · `167` swapon · `168` swapoff
- `169` reboot · `170` sethostname · `171` setdomainname
- `172` iopl · `173` ioperm · `179` quotactl
- `190` fsetxattr · `191` getxattr · `192` lgetxattr
- `195` llistxattr · `196` flistxattr

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

### 5.2 Riesgo medio — 96 de subsistema

- `27` mincore · `78` getdents · `103` syslog
- `122` setfsuid · `123` setfsgid · `156` _sysctl
- `160` setrlimit · `174` create_module · `177` get_kernel_syms
- `178` query_module · `183` afs_syscall · `185` security
- `188` setxattr · `189` lsetxattr · `193` fgetxattr
- `194` listxattr · `197` removexattr · `198` lremovexattr
- `199` fremovexattr · `205` set_thread_area · `206` io_setup
- `207` io_destroy · `208` io_getevents · `209` io_submit
- `210` io_cancel · `211` get_thread_area · `213` epoll_create
- `214` epoll_ctl_old · `215` epoll_wait_old · `216` remap_file_pages
- `219` restart_syscall · `222` timer_create · `223` timer_settime
- `224` timer_gettime · `225` timer_getoverrun · `226` timer_delete
- `232` epoll_wait · `233` epoll_ctl · `235` utimes
- `236` vserver · `247` waitid · `253` inotify_init
- `254` inotify_add_watch · `255` inotify_rm_watch · `261` futimesat
- `270` pselect6 · `271` ppoll · `272` unshare
- `281` epoll_pwait · `282` signalfd · `283` timerfd_create
- `284` eventfd · `285` fallocate · `286` timerfd_settime
- `287` timerfd_gettime · `289` signalfd4 · `294` inotify_init1
- `295` preadv · `296` pwritev · `297` rt_tgsigqueueinfo
- `300` fanotify_init · `301` fanotify_mark · `303` name_to_handle_at
- `304` open_by_handle_at · `305` clock_adjtime · `308` setns
- `314` sched_setattr · `315` sched_getattr · `316` renameat2
- `317` seccomp · `322` execveat · `325` mlock2
- `333` io_pgetevents · `335` uretprobe · `428` open_tree
- `430` fsopen · `431` fsconfig · `433` fspick
- `437` openat2 · `440` process_madvise · `441` epoll_pwait2
- `444` landlock_create_ruleset · `445` landlock_add_rule · `446` landlock_restrict_self
- `448` process_mrelease · `449` futex_waitv · `451` cachestat
- `452` fchmodat2 · `453` map_shadow_stack · `454` futex_wake
- `455` futex_wait · `456` futex_requeue · `459` lsm_get_self_attr
- `460` lsm_set_self_attr · `461` lsm_list_modules · `462` mseal

### 5.3 Riesgo bajo — 82 exóticas o de kernel

- `29` shmget · `30` shmat · `31` shmctl
- `64` semget · `65` semop · `66` semctl
- `67` shmdt · `68` msgget · `69` msgsnd
- `70` msgrcv · `71` msgctl · `125` capget
- `126` capset · `134` uselib · `136` ustat
- `139` sysfs · `154` modify_ldt · `159` adjtimex
- `164` settimeofday · `175` init_module · `176` delete_module
- `180` nfsservctl · `181` getpmsg · `182` putpmsg
- `184` tuxcall · `187` readahead · `212` lookup_dcookie
- `220` semtimedop · `227` clock_settime · `237` mbind
- `238` set_mempolicy · `239` get_mempolicy · `240` mq_open
- `241` mq_unlink · `242` mq_timedsend · `243` mq_timedreceive
- `244` mq_notify · `245` mq_getsetattr · `246` kexec_load
- `248` add_key · `249` request_key · `250` keyctl
- `251` ioprio_set · `252` ioprio_get · `256` migrate_pages
- `275` splice · `276` tee · `277` sync_file_range
- `278` vmsplice · `279` move_pages · `298` perf_event_open
- `299` recvmmsg · `307` sendmmsg · `310` process_vm_readv
- `311` process_vm_writev · `312` kcmp · `313` finit_module
- `319` memfd_create · `320` kexec_file_load · `321` bpf
- `323` userfaultfd · `324` membarrier · `326` copy_file_range
- `327` preadv2 · `328` pwritev2 · `329` pkey_mprotect
- `330` pkey_alloc · `331` pkey_free · `424` pidfd_send_signal
- `425` io_uring_setup · `426` io_uring_enter · `427` io_uring_register
- `429` move_mount · `432` fsmount · `434` pidfd_open
- `438` pidfd_getfd · `442` mount_setattr · `443` quotactl_fd
- `447` memfd_secret · `450` set_mempolicy_home_node · `457` statmount
- `458` listmount

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
