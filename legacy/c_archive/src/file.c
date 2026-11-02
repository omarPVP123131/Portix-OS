#include <portix.h>

#define MAX_FD 32

static int __internal_open(const char *path, int flags, int mode) {
    (void)mode;
    return open(path, flags);
}

int fopen(const char *path, const char *mode) {
    int flags = O_RDONLY;
    if (mode[0] == 'r' && mode[1] == '+') flags = O_RDWR;
    else if (mode[0] == 'w') flags = O_WRONLY;
    else if (mode[0] == 'a') flags = O_WRONLY;
    return __internal_open(path, flags, 0);
}

ssize_t fread(void *buf, size_t size, size_t count, int fd) {
    size_t total = size * count;
    ssize_t r = read(fd, buf, total);
    if (r < 0) return r;
    return r / size;
}

ssize_t fwrite(const void *buf, size_t size, size_t count, int fd) {
    size_t total = size * count;
    ssize_t r = write(fd, buf, total);
    if (r < 0) return r;
    return r / size;
}

int fclose(int fd) {
    return close(fd);
}
