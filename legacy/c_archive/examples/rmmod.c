/* lib/examples/rmmod.c — Userland rmmod utility for PORTIX */
#include <portix.h>
#include <string.h>

int main(int argc, char **argv) {
    const char *name = (argc >= 2) ? argv[1] : "demo_mod.ko";
    printf("[RMMOD] Descargando modulo '%s' del kernel...\n", name);

    if (delete_module(name, 0) < 0) {
        printf("rmmod: fallo al descargar modulo '%s'\n", name);
        return 1;
    }

    printf("[RMMOD] Modulo '%s' descargado con exito del Ring 0!\n", name);
    return 0;
}
