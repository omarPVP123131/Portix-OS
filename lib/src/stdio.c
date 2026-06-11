#include <portix.h>

int putchar(int c) {
    char ch = (char)c;
    if (write(STDOUT_FILENO, &ch, 1) < 0)
        return EOF;
    return (unsigned char)ch;
}

int puts(const char *s) {
    while (*s) {
        if (putchar(*s++) < 0) return EOF;
    }
    if (putchar('\n') < 0) return EOF;
    return 0;
}

static void print_u64(u64 val, int base, int pad) {
    char buf[65];
    char *p = buf + sizeof(buf) - 1;
    *p = '\0';
    const char *digits = "0123456789abcdef";
    int len = 0;
    do {
        *--p = digits[val % base];
        val /= base;
        len++;
    } while (val > 0);
    while (len < pad) {
        *--p = '0';
        len++;
    }
    write(STDOUT_FILENO, p, len);
}

static int vsnprintf(char *buf, size_t n, const char *fmt, va_list args) {
    int count = 0;
    while (*fmt) {
        if (count >= (int)n - 1) break;
        if (*fmt != '%') {
            if (buf) buf[count] = *fmt;
            count++; fmt++;
            continue;
        }
        fmt++;
        int pad = 0;
        if (*fmt == '0') {
            fmt++;
            while (*fmt >= '0' && *fmt <= '9') {
                pad = pad * 10 + (*fmt - '0');
                fmt++;
            }
        } else if (*fmt >= '1' && *fmt <= '9') {
            pad = 0;
            while (*fmt >= '0' && *fmt <= '9') {
                pad = pad * 10 + (*fmt - '0');
                fmt++;
            }
        }
        int start = count;
        switch (*fmt) {
            case 'd': {
                i64 v = va_arg(args, i64);
                if (v < 0) { if (buf && count < (int)n-1) buf[count] = '-'; count++; v = -v; }
                char tmp[24]; int ti = 0;
                do { tmp[ti++] = '0' + (v % 10); v /= 10; } while (v > 0);
                while (ti < pad) tmp[ti++] = '0';
                while (ti > 0) { if (buf && count < (int)n-1) buf[count] = tmp[--ti]; count++; }
                break;
            }
            case 'u': {
                u64 v = va_arg(args, u64);
                char tmp[24]; int ti = 0;
                do { tmp[ti++] = '0' + (v % 10); v /= 10; } while (v > 0);
                while (ti < pad) tmp[ti++] = '0';
                while (ti > 0) { if (buf && count < (int)n-1) buf[count] = tmp[--ti]; count++; }
                break;
            }
            case 'x': {
                u64 v = va_arg(args, u64);
                char tmp[24]; int ti = 0;
                do { int d = v % 16; tmp[ti++] = d < 10 ? '0'+d : 'a'+d-10; v /= 16; } while (v > 0);
                while (ti < pad) tmp[ti++] = '0';
                while (ti > 0) { if (buf && count < (int)n-1) buf[count] = tmp[--ti]; count++; }
                break;
            }
            case 's': {
                const char *s = va_arg(args, const char*);
                if (!s) s = "(null)";
                while (*s) { if (buf && count < (int)n-1) buf[count] = *s; count++; s++; }
                break;
            }
            case 'c': {
                char ch = (char)va_arg(args, int);
                if (buf && count < (int)n-1) buf[count] = ch; count++;
                break;
            }
            case '%':
                if (buf && count < (int)n-1) buf[count] = '%'; count++;
                break;
            default:
                if (buf && count < (int)n-1) buf[count] = '%'; count++;
                if (buf && count < (int)n-1) buf[count] = *fmt; count++;
                break;
        }
        fmt++;
    }
    if (buf && count < (int)n) buf[count] = '\0';
    else if (buf && n > 0) buf[n-1] = '\0';
    return count;
}

int printf(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    int count = 0;
    while (*fmt) {
        if (*fmt != '%') {
            putchar(*fmt++);
            count++;
            continue;
        }
        fmt++;
        int pad = 0;
        if (*fmt == '0') {
            fmt++;
            while (*fmt >= '0' && *fmt <= '9') {
                pad = pad * 10 + (*fmt - '0');
                fmt++;
            }
        } else if (*fmt >= '1' && *fmt <= '9') {
            pad = 0;
            while (*fmt >= '0' && *fmt <= '9') {
                pad = pad * 10 + (*fmt - '0');
                fmt++;
            }
        }
        switch (*fmt) {
            case 'd': {
                i64 v = va_arg(args, i64);
                if (v < 0) { putchar('-'); v = -v; count++; }
                print_u64((u64)v, 10, pad);
                count += (pad ? pad : 1);
                break;
            }
            case 'u': {
                u64 v = va_arg(args, u64);
                print_u64(v, 10, pad);
                count += (pad ? pad : 1);
                break;
            }
            case 'x': {
                u64 v = va_arg(args, u64);
                print_u64(v, 16, pad);
                count += (pad ? pad : 1);
                break;
            }
            case 's': {
                const char *s = va_arg(args, const char*);
                if (!s) s = "(null)";
                while (*s) { putchar(*s++); count++; }
                break;
            }
            case 'c': {
                char ch = (char)va_arg(args, int);
                putchar(ch); count++;
                break;
            }
            case '%':
                putchar('%'); count++;
                break;
            default:
                putchar('%'); putchar(*fmt); count += 2;
                break;
        }
        fmt++;
    }
    va_end(args);
    return count;
}

int sprintf(char *buf, const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    int n = vsnprintf(buf, (size_t)-1, fmt, args);
    va_end(args);
    return n;
}

int snprintf(char *buf, size_t n, const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    int r = vsnprintf(buf, n, fmt, args);
    va_end(args);
    return r;
}

int fputs(const char *s, int fd) {
    size_t len = 0;
    while (s[len]) len++;
    if (write(fd, s, len) < 0) return EOF;
    return 0;
}

char *fgets(char *buf, int n, int fd) {
    int i = 0;
    while (i < n - 1) {
        char c;
        ssize_t r;
        while ((r = read(fd, &c, 1)) <= 0) {
            if (r < 0) break;
        }
        if (r < 0) break;
        buf[i++] = c;
        if (c == '\n') break;
    }
    if (i == 0) return NULL;
    buf[i] = '\0';
    return buf;
}
