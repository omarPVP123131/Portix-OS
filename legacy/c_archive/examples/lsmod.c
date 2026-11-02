/* lib/examples/lsmod.c — Userland lsmod utility for PORTIX */
#include <portix.h>
#include <string.h>

static char buf[4096];

int main(int argc, char **argv) {
    (void)argc; (void)argv;
    memset(buf, 0, sizeof(buf));
    int n = query_module(buf, sizeof(buf) - 1);
    if (n < 0) {
        printf("lsmod: error al consultar modulos del kernel\n");
        return 1;
    }
    buf[n] = '\0';
    printf("%s", buf);
    return 0;
}
