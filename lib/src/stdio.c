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
