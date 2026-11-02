; Sonda nasm (fase posterior a Rust): programa en ensamblador x86-64 puro, sin libc,
; ensamblado por el nasm del anfitrión y enlazado con ld (ELF estático de Linux sin
; modificar). Habla con el núcleo SOLO por la ABI de syscalls de Linux, así que lo que
; se comprueba es la ABI desnuda de Portix, sin que glibc/libstdc++ tapen nada:
;   1. write(1) a stdout.
;   2. open(O_CREAT|O_TRUNC|O_WRONLY) + write + close; open(O_RDONLY) + read + close:
;      lo leído es idéntico a lo escrito (en /usr/local, ext2 real).
;   3. mmap(MAP_PRIVATE|MAP_ANONYMOUS, 64 KiB) + patrón + munmap.
;   4. clone(CLONE_VM|FS|FILES|SIGHAND|THREAD|SYSVSEM|CHILD_CLEARTID) a mano con pila
;      propia: el hilo hace 1000 `lock xadd` sobre un contador compartido y exit(0);
;      main espera con futex(FUTEX_WAIT) sobre la palabra que el núcleo pone a 0.
;   5. getpid == gettid en main; gettid del hilo distinto del pid.
;   6. nanosleep(100 ms) medido con clock_gettime(CLOCK_MONOTONIC): >= 100 ms.
;   7. exit_group(0) tras imprimir [T008-PASS]; cualquier fallo -> [T008-FAIL n] y exit_group(1).
; Ensamblar: nasm -f elf64 t008_nasm.asm -o t008.o && ld -static -o t008_nasm t008.o

BITS 64
global _start

%define SYS_read            0
%define SYS_write           1
%define SYS_open            2
%define SYS_close           3
%define SYS_mmap            9
%define SYS_munmap          11
%define SYS_nanosleep       35
%define SYS_getpid          39
%define SYS_clone           56
%define SYS_exit            60
%define SYS_futex           202
%define SYS_gettid          186
%define SYS_clock_gettime   228
%define SYS_exit_group      231

%define O_RDONLY  0
%define O_WRONLY  1
%define O_CREAT   0o100
%define O_TRUNC   0o1000

%define CLONE_FLAGS 0x00050f00 | 0x00200000 ; VM|FS|FILES|SIGHAND|THREAD|SYSVSEM | CHILD_CLEARTID
%define FUTEX_WAIT 0

section .data
    msg_ini:    db "[t008] nasm x86-64 sin libc: hablando con la ABI de Portix", 10
    msg_ini_len equ $ - msg_ini
    ruta:       db "/usr/local/t008.txt", 0
    contenido:  db "Portix ABI desnuda desde nasm: 0123456789", 10
    cont_len    equ $ - contenido
    ok1: db "T008-1: ok write a stdout", 10
    ok1_len equ $ - ok1
    ok2: db "T008-2: ok open/write/close + open/read/close en ext2: contenido identico", 10
    ok2_len equ $ - ok2
    ok3: db "T008-3: ok mmap anonimo de 64 KiB escrito, verificado y liberado", 10
    ok3_len equ $ - ok3
    ok4: db "T008-4: ok clone(CLONE_THREAD) a mano: 1000 lock xadd del hilo y futex sobre CHILD_CLEARTID", 10
    ok4_len equ $ - ok4
    ok5: db "T008-5: ok getpid == gettid en main y gettid del hilo distinto", 10
    ok5_len equ $ - ok5
    ok6: db "T008-6: ok nanosleep(100 ms) medido con CLOCK_MONOTONIC >= 100 ms", 10
    ok6_len equ $ - ok6
    pass: db "[T008-PASS] ensamblador puro: write, open/read, mmap, clone+futex, gettid, nanosleep por la ABI de Linux", 10
    pass_len equ $ - pass
    fail: db "[T008-FAIL "
    fail_n: db "?]", 10
    fail_len equ $ - fail
    ts_sleep:   dq 0, 100000000      ; 0 s, 100 ms

section .bss
    buf:        resb 256
    contador:   resq 1
    tid_hilo:   resd 1
    tid_visto:  resq 1
    ts0:        resq 2
    ts1:        resq 2
    pila_hilo:  resb 16384
    pila_fin:

section .text

; escribe(rsi=ptr, rdx=len) a stdout
escribe:
    mov eax, SYS_write
    mov edi, 1
    syscall
    ret

; fallo(dil = digito)
fallo:
    mov [fail_n], dil
    mov rsi, fail
    mov rdx, fail_len
    call escribe
    mov eax, SYS_exit_group
    mov edi, 1
    syscall

_start:
    mov rsi, msg_ini
    mov rdx, msg_ini_len
    call escribe

    ; ── 1. write ──
    mov rsi, ok1
    mov rdx, ok1_len
    call escribe
    cmp rax, ok1_len
    je .p2
    mov dil, '1'
    call fallo

.p2:
    ; ── 2. fichero ──
    mov eax, SYS_open
    mov rdi, ruta
    mov esi, O_WRONLY | O_CREAT | O_TRUNC
    mov edx, 0o644
    syscall
    test rax, rax
    js .f2
    mov r12, rax
    mov eax, SYS_write
    mov rdi, r12
    mov rsi, contenido
    mov rdx, cont_len
    syscall
    cmp rax, cont_len
    jne .f2
    mov eax, SYS_close
    mov rdi, r12
    syscall
    mov eax, SYS_open
    mov rdi, ruta
    mov esi, O_RDONLY
    xor edx, edx
    syscall
    test rax, rax
    js .f2
    mov r12, rax
    mov eax, SYS_read
    mov rdi, r12
    mov rsi, buf
    mov rdx, 256
    syscall
    cmp rax, cont_len
    jne .f2
    mov eax, SYS_close
    mov rdi, r12
    syscall
    ; comparar
    mov rcx, cont_len
    mov rsi, buf
    mov rdi, contenido
    repe cmpsb
    jne .f2
    mov rsi, ok2
    mov rdx, ok2_len
    call escribe
    jmp .p3
.f2:
    mov dil, '2'
    call fallo

.p3:
    ; ── 3. mmap ──
    mov eax, SYS_mmap
    xor edi, edi
    mov esi, 65536
    mov edx, 3              ; PROT_READ|PROT_WRITE
    mov r10d, 0x22          ; MAP_PRIVATE|MAP_ANONYMOUS
    mov r8, -1
    xor r9d, r9d
    syscall
    cmp rax, -4096
    ja .f3
    mov r12, rax
    ; escribir patrón i & 0xff en cada byte
    xor ecx, ecx
.llena:
    mov byte [r12 + rcx], cl
    inc ecx
    cmp ecx, 65536
    jb .llena
    ; verificar
    xor ecx, ecx
.verif:
    mov al, [r12 + rcx]
    cmp al, cl
    jne .f3
    inc ecx
    cmp ecx, 65536
    jb .verif
    mov eax, SYS_munmap
    mov rdi, r12
    mov esi, 65536
    syscall
    test rax, rax
    jnz .f3
    mov rsi, ok3
    mov rdx, ok3_len
    call escribe
    jmp .p4
.f3:
    mov dil, '3'
    call fallo

.p4:
    ; ── 4. clone hilo a mano ──
    mov dword [tid_hilo], 1          ; distinto de 0 hasta que el núcleo lo limpie
    mov eax, SYS_clone
    mov edi, CLONE_FLAGS
    lea rsi, [pila_fin - 64]         ; pila del hijo (alineada a 16)
    xor edx, edx                     ; parent_tid
    mov r10, tid_hilo                ; child_tid (CHILD_CLEARTID)
    xor r8d, r8d                     ; tls
    syscall
    test rax, rax
    js .f4
    jz hilo                          ; rax == 0: somos el hijo (misma pila? NO: el hijo corre en rsi)
    mov r13, rax                     ; tid devuelto al padre
    ; esperar a que el núcleo ponga tid_hilo a 0
.espera:
    mov eax, [tid_hilo]
    test eax, eax
    jz .despierto
    mov eax, SYS_futex
    mov rdi, tid_hilo
    mov esi, FUTEX_WAIT
    mov edx, 1                       ; valor esperado (el tid sigue != 0 -> esperamos mientras valga 1... )
    ; el núcleo escribe el tid del hijo en *child_tid con CHILD_SETTID? No lo pedimos: vale 1 hasta el exit.
    xor r10d, r10d
    syscall
    jmp .espera
.despierto:
    cmp qword [contador], 1000
    jne .f4
    ; tid visto por el hilo debe ser el que devolvió clone
    cmp [tid_visto], r13
    jne .f4
    mov rsi, ok4
    mov rdx, ok4_len
    call escribe
    jmp .p5
.f4:
    mov dil, '4'
    call fallo

.p5:
    ; ── 5. getpid == gettid en main, y el hilo tuvo otro ──
    mov eax, SYS_getpid
    syscall
    mov r12, rax
    mov eax, SYS_gettid
    syscall
    cmp rax, r12
    jne .f5
    cmp [tid_visto], r12
    je .f5
    mov rsi, ok5
    mov rdx, ok5_len
    call escribe
    jmp .p6
.f5:
    mov dil, '5'
    call fallo

.p6:
    ; ── 6. nanosleep medido ──
    mov eax, SYS_clock_gettime
    mov edi, 1                       ; CLOCK_MONOTONIC
    mov rsi, ts0
    syscall
    test rax, rax
    jnz .f6
    mov eax, SYS_nanosleep
    mov rdi, ts_sleep
    xor esi, esi
    syscall
    test rax, rax
    jnz .f6
    mov eax, SYS_clock_gettime
    mov edi, 1
    mov rsi, ts1
    syscall
    ; dt_ns = (s1-s0)*1e9 + (ns1-ns0)
    mov rax, [ts1]
    sub rax, [ts0]
    imul rax, rax, 1000000000
    add rax, [ts1 + 8]
    sub rax, [ts0 + 8]
    cmp rax, 100000000
    jl .f6
    mov rcx, 5000000000
    cmp rax, rcx
    jg .f6
    mov rsi, ok6
    mov rdx, ok6_len
    call escribe
    jmp .fin
.f6:
    mov dil, '6'
    call fallo

.fin:
    mov rsi, pass
    mov rdx, pass_len
    call escribe
    mov eax, SYS_exit_group
    xor edi, edi
    syscall

hilo:
    ; pila nueva: rsp = pila_fin - 64
    mov eax, SYS_gettid
    syscall
    mov [tid_visto], rax
    mov ecx, 1000
.bucle:
    mov eax, 1
    lock xadd [contador], rax
    dec ecx
    jnz .bucle
    mov eax, SYS_exit                ; exit del HILO (no exit_group): el núcleo limpia tid_hilo y despierta
    xor edi, edi
    syscall
    ud2

