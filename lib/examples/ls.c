/* lib/examples/ls.c — List directory contents */

#include <portix.h>
#include <string.h>

int main(int argc, char *argv[], char *envp[]) {
    (void)argc; (void)envp;
    const char *path = argv[1] ? argv[1] : "/";

    char buf[512];
    int ret = getdents(path, buf, sizeof(buf));
    if (ret < 0) {
        printf("ls: %s: no such directory\n", path);
        return 1;
    }

    unsigned long off = 0;
    while (off < (unsigned long)ret) {
        struct portix_dirent *d = (struct portix_dirent *)(buf + off);
        if (d->d_reclen == 0 || off + d->d_reclen > (unsigned long)ret) break;
        if (d->d_name[0] != '.')
            printf("%s  ", d->d_name);
        off += d->d_reclen;
    }
    printf("\n");
    return 0;
}
