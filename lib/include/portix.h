#ifndef _PORTIX_H
#define _PORTIX_H

typedef unsigned long long u64;
typedef unsigned int       u32;
typedef unsigned short     u16;
typedef unsigned char      u8;
typedef long long          i64;
typedef int                i32;
typedef short              i16;
typedef signed char        i8;

typedef u64 size_t;
typedef i64 ssize_t;

#define NULL ((void*)0)
#define EOF (-1)

typedef __builtin_va_list va_list;
#define va_start(v,l) __builtin_va_start(v,l)
#define va_end(v)     __builtin_va_end(v)
#define va_arg(v,t)   __builtin_va_arg(v,t)
#define va_copy(d,s)  __builtin_va_copy(d,s)

#define SYS_EXIT   0
#define SYS_WRITE  1
#define SYS_GETPID 2
#define SYS_YIELD  3
#define SYS_SLEEP  4
#define SYS_READ   5
#define SYS_OPEN   6
#define SYS_CLOSE  7
#define SYS_BRK      8
#define SYS_MMAP     9
#define SYS_GETDIRENTS 10
#define SYS_EXECVE   11
#define SYS_DUP2     12
#define SYS_UPTIME   13
#define SYS_SEND     14
#define SYS_RECV     15
#define SYS_REG_IRQ  16

#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR   2

#define PROT_READ  1
#define PROT_WRITE 2
#define PROT_EXEC  4

#define MAP_PRIVATE   2
#define MAP_ANONYMOUS 32

#define STDIN_FILENO  0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2

static inline u64 syscall(u64 num, u64 a1, u64 a2, u64 a3, u64 a4, u64 a5) {
    u64 ret;
    register u64 r10 asm("r10") = a4;
    register u64 r8  asm("r8")  = a5;
    asm volatile("int $0x80"
        : "=a"(ret)
        : "a"(num), "D"(a1), "S"(a2), "d"(a3), "r"(r10), "r"(r8)
        : "rcx", "r11", "memory");
    return ret;
}

static inline u64 syscall0(u64 num) {
    return syscall(num, 0, 0, 0, 0, 0);
}

static inline u64 syscall1(u64 num, u64 a1) {
    return syscall(num, a1, 0, 0, 0, 0);
}

static inline u64 syscall4(u64 num, u64 a1, u64 a2, u64 a3, u64 a4) {
    return syscall(num, a1, a2, a3, a4, 0);
}

static inline u64 syscall2(u64 num, u64 a1, u64 a2) {
    return syscall(num, a1, a2, 0, 0, 0);
}

static inline u64 syscall3(u64 num, u64 a1, u64 a2, u64 a3) {
    return syscall(num, a1, a2, a3, 0, 0);
}

static inline void exit(int code) {
    syscall1(SYS_EXIT, code);
    __builtin_unreachable();
}

static inline ssize_t write(int fd, const void *buf, size_t count) {
    return (ssize_t)syscall3(SYS_WRITE, fd, (u64)buf, count);
}

static inline ssize_t read(int fd, void *buf, size_t count) {
    return (ssize_t)syscall3(SYS_READ, fd, (u64)buf, count);
}

static inline int open(const char *path, int flags) {
    return (int)syscall2(SYS_OPEN, (u64)path, flags);
}

static inline int close(int fd) {
    return (int)syscall1(SYS_CLOSE, fd);
}

static inline int getpid(void) {
    return (int)syscall0(SYS_GETPID);
}

static inline void yield(void) {
    syscall0(SYS_YIELD);
}

static inline void sleep(u64 ticks) {
    syscall1(SYS_SLEEP, ticks);
}

static inline void *brk(void *addr) {
    return (void*)syscall1(SYS_BRK, (u64)addr);
}

static inline void *mmap(void *addr, size_t len, int prot, int flags) {
    return (void*)syscall4(SYS_MMAP, (u64)addr, len, prot, flags);
}

// ── Dirent (SYS_GETDIRENTS) ───────────────────────────────────────────
#define DT_UNKNOWN 0
#define DT_FILE    1
#define DT_DIR     2

struct __attribute__((packed)) portix_dirent {
    unsigned long long d_ino;      // 8 bytes
    unsigned long long d_off;      // 8 bytes
    unsigned short     d_reclen;   // 2 bytes (total entry size)
    unsigned char      d_type;     // 1 byte (DT_UNKNOWN/DT_FILE/DT_DIR)
    char               d_name[];   // variable, null-terminated
};

#define DIRENT_HEADER_SIZE 19

static inline int getdents(const char *path, void *buf, unsigned long count) {
    return (int)syscall3(SYS_GETDIRENTS, (u64)path, (u64)buf, count);
}

static inline int execve(const char *path, char *const argv[], char *const envp[]) {
    return (int)syscall3(SYS_EXECVE, (u64)path, (u64)argv, (u64)envp);
}

static inline int dup2(int oldfd, int newfd) {
    return (int)syscall2(SYS_DUP2, (u64)oldfd, (u64)newfd);
}

static inline unsigned long long uptime(void) {
    return syscall0(SYS_UPTIME);
}

// ── IPC (SYS_SEND, SYS_RECV, SYS_REG_IRQ) ─────────────────────────────

#define IPC_MSG_SIZE 64

struct __attribute__((packed)) ipc_msg {
    unsigned long long src_pid;
    unsigned long long dst_pid;
    unsigned long long msg_type;
    unsigned char      data[40];
};

static inline int ipc_send(unsigned long long dst_pid, unsigned long long msg_type,
                           const void *data, unsigned long long data_len) {
    return (int)syscall4(SYS_SEND, dst_pid, msg_type, (unsigned long long)data, data_len);
}

static inline int ipc_recv(void *buf, unsigned long long len) {
    return (int)syscall2(SYS_RECV, (unsigned long long)buf, len);
}

static inline int ipc_register_irq(unsigned long long irq, unsigned long long pid) {
    return (int)syscall2(SYS_REG_IRQ, irq, pid);
}

int putchar(int c);
int puts(const char *s);
int printf(const char *fmt, ...);
int sprintf(char *buf, const char *fmt, ...);
int snprintf(char *buf, size_t n, const char *fmt, ...);
int fputs(const char *s, int fd);
char *fgets(char *buf, int n, int fd);

int fopen(const char *path, const char *mode);
ssize_t fread(void *buf, size_t size, size_t count, int fd);
ssize_t fwrite(const void *buf, size_t size, size_t count, int fd);
int fclose(int fd);

void *malloc(size_t size);
void free(void *ptr);
void *calloc(size_t nmemb, size_t size);
void *realloc(void *ptr, size_t size);

void *memcpy(void *dest, const void *src, size_t n);
void *memset(void *s, int c, size_t n);
void *memmove(void *dest, const void *src, size_t n);
int memcmp(const void *s1, const void *s2, size_t n);
size_t strlen(const char *s);
int strcmp(const char *s1, const char *s2);
char *strcpy(char *dest, const char *src);
char *strncpy(char *dest, const char *src, size_t n);
char *strcat(char *dest, const char *src);

#endif
