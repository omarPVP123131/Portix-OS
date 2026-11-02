#ifndef _STDIO_H
#define _STDIO_H

#include <sys/types.h>
#include <stdarg.h>

#define EOF (-1)

#define stdin  0
#define stdout 1
#define stderr 2

int puts(const char *s);
int putchar(int c);
int getchar(void);
int printf(const char *format, ...);
int sprintf(char *str, const char *format, ...);
int snprintf(char *str, size_t size, const char *format, ...);
int vsnprintf(char *str, size_t size, const char *format, va_list ap);

#endif /* _STDIO_H */
