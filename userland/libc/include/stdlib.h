#ifndef _STDLIB_H
#define _STDLIB_H

#include <sys/types.h>

#define NULL ((void *)0)

#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1

void *malloc(size_t size);
void free(void *ptr);
void *calloc(size_t nmemb, size_t size);
void *realloc(void *ptr, size_t size);

void exit(int status);
int atoi(const char *nptr);
long atol(const char *nptr);
int abs(int j);

#endif /* _STDLIB_H */
