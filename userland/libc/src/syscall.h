#ifndef _PORTIX_SYSCALL_H
#define _PORTIX_SYSCALL_H

#define SYS_EXIT          0
#define SYS_WRITE         1
#define SYS_GETPID        2
#define SYS_YIELD         3
#define SYS_SLEEP         4
#define SYS_READ          5
#define SYS_OPEN          6
#define SYS_CLOSE         7
#define SYS_BRK           8
#define SYS_MMAP          9
#define SYS_GETDIRENTS    10
#define SYS_EXECVE        11
#define SYS_DUP2          12
#define SYS_UPTIME        13
#define SYS_FORK          22
#define SYS_WAITPID       23
#define SYS_LSEEK         24
#define SYS_STAT          25
#define SYS_FSTAT         26
#define SYS_KILL          27
#define SYS_GETCWD        28
#define SYS_CHDIR         29
#define SYS_UNLINK        30
#define SYS_PIPE          31
#define SYS_TIME          32
#define SYS_MKDIR         33
#define SYS_GETUID        43
#define SYS_SETUID        44
#define SYS_GETGID        45
#define SYS_SETGID        46
#define SYS_MUNMAP        47
#define SYS_DUP           48
#define SYS_GETPPID       50
#define SYS_RMDIR         56

static inline long __syscall0(long n) {
    long ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(n)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static inline long __syscall1(long n, long a1) {
    long ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(n), "D"(a1)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static inline long __syscall2(long n, long a1, long a2) {
    long ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(n), "D"(a1), "S"(a2)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static inline long __syscall3(long n, long a1, long a2, long a3) {
    long ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(n), "D"(a1), "S"(a2), "d"(a3)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static inline long __syscall4(long n, long a1, long a2, long a3, long a4) {
    long ret;
    register long r10 __asm__("r10") = a4;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(n), "D"(a1), "S"(a2), "d"(a3), "r"(r10)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static inline long __syscall5(long n, long a1, long a2, long a3, long a4, long a5) {
    long ret;
    register long r10 __asm__("r10") = a4;
    register long r8  __asm__("r8")  = a5;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(n), "D"(a1), "S"(a2), "d"(a3), "r"(r10), "r"(r8)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static inline long __syscall6(long n, long a1, long a2, long a3, long a4, long a5, long a6) {
    long ret;
    register long r10 __asm__("r10") = a4;
    register long r8  __asm__("r8")  = a5;
    register long r9  __asm__("r9")  = a6;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(n), "D"(a1), "S"(a2), "d"(a3), "r"(r10), "r"(r8), "r"(r9)
        : "rcx", "r11", "memory"
    );
    return ret;
}

#endif /* _PORTIX_SYSCALL_H */
