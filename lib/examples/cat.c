/* lib/examples/cat.c — Concatenate files */

#include <portix.h>

int main(int argc, char *argv[], char *envp[]) {
    (void)envp;
    if (argc < 2) {
        printf("Usage: cat <file>\n");
        return 1;
    }
    for (int i = 1; i < argc; i++) {
        int fd = open(argv[i], O_RDONLY);
        if (fd < 0) {
            printf("cat: %s: no such file\n", argv[i]);
            continue;
        }
        char buf[512];
        ssize_t n;
        while ((n = read(fd, buf, sizeof(buf))) > 0) {
            write(STDOUT_FILENO, buf, n);
        }
        close(fd);
    }
    return 0;
}
