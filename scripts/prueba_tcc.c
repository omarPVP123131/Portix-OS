/* [CICLO 50 / T1c] Compilado DENTRO de Portix por el tcc de Alpine (sin modificar),
 * contra las cabeceras y crt de musl-dev de Alpine, y ejecutado por ld-musl. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fib(int n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }

int main(int argc, char **argv) {
    char buf[64];
    snprintf(buf, sizeof buf, "fib(20)=%d strlen=%zu", fib(20), strlen("portix"));
    printf("[TCC-RUN] %s argc=%d\n", buf, argc);
    FILE *f = fopen("/tmp/tcc_out.txt", "w");
    if (!f) { printf("[TCC-RUN] fopen fallo\n"); return 2; }
    fprintf(f, "%d\n", fib(20) * 2);
    fclose(f);
    return 0;
}
