.global _start
.extern main
.extern exit

.section .text
_start:
    # Clear frame pointer for stack unwinding
    xor %rbp, %rbp

    # Pop argc into %rdi
    pop %rdi

    # %rsi points to argv array (now top of stack)
    mov %rsp, %rsi

    # Align stack to 16 bytes for System V AMD64 ABI before calling main
    and $-16, %rsp

    # Call int main(int argc, char **argv)
    call main

    # Pass return code in %edi to exit
    mov %eax, %edi
    call exit

    # If exit returns, force hang or syscall exit
    mov $60, %rax
    xor %rdi, %rdi
    syscall
