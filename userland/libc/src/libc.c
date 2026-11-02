#include <sys/types.h>
#include <sys/stat.h>
#include <sys/mman.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <errno.h>
#include "syscall.h"

int errno = 0;

/* ─── Syscall Wrappers ─── */

ssize_t write(int fd, const void *buf, size_t count) {
    long ret = __syscall3(SYS_WRITE, (long)fd, (long)buf, (long)count);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (ssize_t)ret;
}

ssize_t read(int fd, void *buf, size_t count) {
    long ret = __syscall3(SYS_READ, (long)fd, (long)buf, (long)count);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (ssize_t)ret;
}

int open(const char *pathname, int flags, ...) {
    long ret = __syscall3(SYS_OPEN, (long)pathname, (long)flags, 0644);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

int close(int fd) {
    long ret = __syscall1(SYS_CLOSE, (long)fd);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

off_t lseek(int fd, off_t offset, int whence) {
    long ret = __syscall3(SYS_LSEEK, (long)fd, (long)offset, (long)whence);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (off_t)ret;
}

pid_t getpid(void) {
    return (pid_t)__syscall0(SYS_GETPID);
}

pid_t getppid(void) {
    return (pid_t)__syscall0(SYS_GETPPID);
}

uid_t getuid(void) {
    return (uid_t)__syscall0(SYS_GETUID);
}

gid_t getgid(void) {
    return (gid_t)__syscall0(SYS_GETGID);
}

pid_t fork(void) {
    long ret = __syscall0(SYS_FORK);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (pid_t)ret;
}

int execve(const char *filename, char *const argv[], char *const envp[]) {
    long ret = __syscall3(SYS_EXECVE, (long)filename, (long)argv, (long)envp);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

int unlink(const char *pathname) {
    long ret = __syscall1(SYS_UNLINK, (long)pathname);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

int mkdir(const char *pathname, mode_t mode) {
    long ret = __syscall2(SYS_MKDIR, (long)pathname, (long)mode);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

int rmdir(const char *pathname) {
    long ret = __syscall1(SYS_RMDIR, (long)pathname);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

int chdir(const char *path) {
    long ret = __syscall1(SYS_CHDIR, (long)path);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

int dup(int oldfd) {
    long ret = __syscall1(SYS_DUP, (long)oldfd);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

int dup2(int oldfd, int newfd) {
    long ret = __syscall2(SYS_DUP2, (long)oldfd, (long)newfd);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

int pipe(int pipefd[2]) {
    long ret = __syscall1(SYS_PIPE, (long)pipefd);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

void _exit(int status) {
    __syscall1(SYS_EXIT, (long)status);
    while (1) {}
}

void exit(int status) {
    _exit(status);
}

void *mmap(void *addr, size_t length, int prot, int flags, int fd, off_t offset) {
    long ret = __syscall6(SYS_MMAP, (long)addr, (long)length, (long)prot, (long)flags, (long)fd, (long)offset);
    if (ret < 0) {
        errno = (int)(-ret);
        return MAP_FAILED;
    }
    return (void *)ret;
}

int munmap(void *addr, size_t length) {
    long ret = __syscall2(SYS_MUNMAP, (long)addr, (long)length);
    if (ret < 0) {
        errno = (int)(-ret);
        return -1;
    }
    return (int)ret;
}

/* ─── String Functions ─── */

size_t strlen(const char *s) {
    size_t len = 0;
    while (s && s[len]) len++;
    return len;
}

int strcmp(const char *s1, const char *s2) {
    while (*s1 && (*s1 == *s2)) {
        s1++;
        s2++;
    }
    return *(const unsigned char *)s1 - *(const unsigned char *)s2;
}

int strncmp(const char *s1, const char *s2, size_t n) {
    while (n && *s1 && (*s1 == *s2)) {
        s1++;
        s2++;
        n--;
    }
    if (n == 0) return 0;
    return *(const unsigned char *)s1 - *(const unsigned char *)s2;
}

char *strcpy(char *dest, const char *src) {
    char *d = dest;
    while ((*d++ = *src++)) {}
    return dest;
}

char *strncpy(char *dest, const char *src, size_t n) {
    size_t i;
    for (i = 0; i < n && src[i] != '\0'; i++) {
        dest[i] = src[i];
    }
    for (; i < n; i++) {
        dest[i] = '\0';
    }
    return dest;
}

void *memset(void *s, int c, size_t n) {
    unsigned char *p = (unsigned char *)s;
    while (n--) *p++ = (unsigned char)c;
    return s;
}

void *memcpy(void *dest, const void *src, size_t n) {
    unsigned char *d = (unsigned char *)dest;
    const unsigned char *s = (const unsigned char *)src;
    while (n--) *d++ = *s++;
    return dest;
}

void *memmove(void *dest, const void *src, size_t n) {
    unsigned char *d = (unsigned char *)dest;
    const unsigned char *s = (const unsigned char *)src;
    if (d < s) {
        while (n--) *d++ = *s++;
    } else {
        d += n;
        s += n;
        while (n--) *--d = *--s;
    }
    return dest;
}

int memcmp(const void *s1, const void *s2, size_t n) {
    const unsigned char *p1 = (const unsigned char *)s1;
    const unsigned char *p2 = (const unsigned char *)s2;
    while (n--) {
        if (*p1 != *p2) return *p1 - *p2;
        p1++;
        p2++;
    }
    return 0;
}

/* ─── Memory Allocator (Simple Bump/Mmap Allocator) ─── */

typedef struct BlockHeader {
    size_t size;
    int is_free;
    struct BlockHeader *next;
} BlockHeader;

static BlockHeader *heap_head = NULL;

void *malloc(size_t size) {
    if (size == 0) return NULL;
    // Align to 16 bytes
    size = (size + 15) & ~15;

    BlockHeader *curr = heap_head;
    while (curr) {
        if (curr->is_free && curr->size >= size) {
            curr->is_free = 0;
            return (void *)((char *)curr + sizeof(BlockHeader));
        }
        curr = curr->next;
    }

    size_t total_size = size + sizeof(BlockHeader);
    size_t mmap_size = (total_size + 4095) & ~4095;
    void *chunk = mmap(NULL, mmap_size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (chunk == MAP_FAILED) return NULL;

    BlockHeader *block = (BlockHeader *)chunk;
    block->size = mmap_size - sizeof(BlockHeader);
    block->is_free = 0;
    block->next = heap_head;
    heap_head = block;

    return (void *)((char *)block + sizeof(BlockHeader));
}

void free(void *ptr) {
    if (!ptr) return;
    BlockHeader *block = (BlockHeader *)((char *)ptr - sizeof(BlockHeader));
    block->is_free = 1;
}

void *calloc(size_t nmemb, size_t size) {
    size_t total = nmemb * size;
    void *p = malloc(total);
    if (p) memset(p, 0, total);
    return p;
}

int abs(int j) {
    return (j < 0) ? -j : j;
}

int atoi(const char *nptr) {
    if (!nptr) return 0;
    while (*nptr == ' ' || *nptr == '\t' || *nptr == '\n') nptr++;
    int sign = 1;
    if (*nptr == '-') { sign = -1; nptr++; }
    else if (*nptr == '+') { nptr++; }
    int res = 0;
    while (*nptr >= '0' && *nptr <= '9') {
        res = res * 10 + (*nptr - '0');
        nptr++;
    }
    return res * sign;
}

long atol(const char *nptr) {
    if (!nptr) return 0;
    while (*nptr == ' ' || *nptr == '\t' || *nptr == '\n') nptr++;
    long sign = 1;
    if (*nptr == '-') { sign = -1; nptr++; }
    else if (*nptr == '+') { nptr++; }
    long res = 0;
    while (*nptr >= '0' && *nptr <= '9') {
        res = res * 10 + (*nptr - '0');
        nptr++;
    }
    return res * sign;
}

/* ─── Standard I/O (puts, putchar, printf) ─── */

int putchar(int c) {
    char ch = (char)c;
    write(STDOUT_FILENO, &ch, 1);
    return c;
}

int puts(const char *s) {
    size_t len = strlen(s);
    write(STDOUT_FILENO, s, len);
    char nl = '\n';
    write(STDOUT_FILENO, &nl, 1);
    return 0;
}

static void print_num(long val, int base, int is_signed, int width, char pad) {
    char buf[32];
    int idx = 0;
    unsigned long uval;
    int neg = 0;

    if (is_signed && val < 0) {
        neg = 1;
        uval = (unsigned long)(-val);
    } else {
        uval = (unsigned long)val;
    }

    if (uval == 0) {
        buf[idx++] = '0';
    } else {
        while (uval > 0) {
            int rem = uval % base;
            buf[idx++] = (rem < 10) ? ('0' + rem) : ('a' + rem - 10);
            uval /= base;
        }
    }

    if (neg) {
        buf[idx++] = '-';
    }

    while (idx < width) {
        buf[idx++] = pad;
    }

    for (int i = idx - 1; i >= 0; i--) {
        putchar(buf[i]);
    }
}

int snprintf(char *str, size_t size, const char *format, ...) {
    va_list ap;
    va_start(ap, format);
    size_t written = 0;

    while (*format && written + 1 < size) {
        if (*format == '%') {
            format++;
            if (*format == 's') {
                const char *s = va_arg(ap, const char *);
                if (!s) s = "(null)";
                while (*s && written + 1 < size) {
                    str[written++] = *s++;
                }
            } else if (*format == 'd' || *format == 'i' || *format == 'u' || *format == 'x') {
                char buf[32];
                int idx = 0;
                unsigned long uval;
                int base = (*format == 'x') ? 16 : 10;
                int is_neg = 0;
                if (*format == 'd' || *format == 'i') {
                    long val = va_arg(ap, int);
                    if (val < 0) { is_neg = 1; uval = -val; }
                    else { uval = val; }
                } else {
                    uval = va_arg(ap, unsigned int);
                }
                if (uval == 0) buf[idx++] = '0';
                else {
                    while (uval > 0) {
                        int rem = uval % base;
                        buf[idx++] = (rem < 10) ? ('0' + rem) : ('a' + rem - 10);
                        uval /= base;
                    }
                }
                if (is_neg && written + 1 < size) str[written++] = '-';
                for (int i = idx - 1; i >= 0 && written + 1 < size; i--) {
                    str[written++] = buf[i];
                }
            } else if (*format == 'c') {
                int c = va_arg(ap, int);
                if (written + 1 < size) str[written++] = (char)c;
            } else {
                if (written + 1 < size) str[written++] = *format;
            }
        } else {
            str[written++] = *format;
        }
        format++;
    }
    if (size > 0) str[written] = '\0';
    va_end(ap);
    return (int)written;
}

int sprintf(char *str, const char *format, ...) {
    va_list ap;
    va_start(ap, format);
    int ret = vsnprintf(str, 1024, format, ap);
    va_end(ap);
    return ret;
}

int vsnprintf(char *str, size_t size, const char *format, va_list ap) {
    size_t written = 0;

    while (*format && written + 1 < size) {
        if (*format == '%') {
            format++;
            if (*format == 's') {
                const char *s = va_arg(ap, const char *);
                if (!s) s = "(null)";
                while (*s && written + 1 < size) {
                    str[written++] = *s++;
                }
            } else if (*format == 'd' || *format == 'i' || *format == 'u' || *format == 'x') {
                char buf[32];
                int idx = 0;
                unsigned long uval;
                int base = (*format == 'x') ? 16 : 10;
                int is_neg = 0;
                if (*format == 'd' || *format == 'i') {
                    long val = va_arg(ap, int);
                    if (val < 0) { is_neg = 1; uval = -val; }
                    else { uval = val; }
                } else {
                    uval = va_arg(ap, unsigned int);
                }
                if (uval == 0) buf[idx++] = '0';
                else {
                    while (uval > 0) {
                        int rem = uval % base;
                        buf[idx++] = (rem < 10) ? ('0' + rem) : ('a' + rem - 10);
                        uval /= base;
                    }
                }
                if (is_neg && written + 1 < size) str[written++] = '-';
                for (int i = idx - 1; i >= 0 && written + 1 < size; i--) {
                    str[written++] = buf[i];
                }
            } else if (*format == 'c') {
                int c = va_arg(ap, int);
                if (written + 1 < size) str[written++] = (char)c;
            } else {
                if (written + 1 < size) str[written++] = *format;
            }
        } else {
            str[written++] = *format;
        }
        format++;
    }
    if (size > 0) str[written] = '\0';
    return (int)written;
}

int printf(const char *format, ...) {
    va_list ap;
    va_start(ap, format);

    while (*format) {
        if (*format == '%') {
            format++;
            int width = 0;
            char pad = ' ';
            if (*format == '0') {
                pad = '0';
                format++;
            }
            while (*format >= '0' && *format <= '9') {
                width = width * 10 + (*format - '0');
                format++;
            }

            switch (*format) {
                case 's': {
                    const char *s = va_arg(ap, const char *);
                    if (!s) s = "(null)";
                    while (*s) putchar(*s++);
                    break;
                }
                case 'd':
                case 'i': {
                    long val = va_arg(ap, int);
                    print_num(val, 10, 1, width, pad);
                    break;
                }
                case 'u': {
                    unsigned long val = va_arg(ap, unsigned int);
                    print_num(val, 10, 0, width, pad);
                    break;
                }
                case 'x': {
                    unsigned long val = va_arg(ap, unsigned int);
                    print_num(val, 16, 0, width, pad);
                    break;
                }
                case 'c': {
                    int c = va_arg(ap, int);
                    putchar(c);
                    break;
                }
                case '%': {
                    putchar('%');
                    break;
                }
                default:
                    putchar('%');
                    putchar(*format);
                    break;
            }
        } else {
            putchar(*format);
        }
        format++;
    }

    va_end(ap);
    return 0;
}
