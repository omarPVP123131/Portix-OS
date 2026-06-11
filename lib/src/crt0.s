# lib/src/crt0.s — Ring-3 _start entry point for PORTIX (GAS syntax)
#
# Called by the kernel's IRETQ to ring 3.
# Sets up stack, calls _init, then main(), then exit().
# The kernel passes no arguments for now (no argv/envp).

.text
.globl _start
.type _start, @function

_start:
    xor %rbp, %rbp          # clear frame pointer
    call _init               # global constructors
    mov (%rsp), %rdi        # argc = [rsp]
    lea 8(%rsp), %rsi       # argv = rsp + 8
    lea 16(%rsp,%rdi,8), %rdx  # envp = rsp + 8 + (argc+1)*8
    call main
    mov %eax, %edi           # exit(code)
    call _exit
    hlt                      # should never reach here

.globl _init
.type _init, @function
_init:
    ret
