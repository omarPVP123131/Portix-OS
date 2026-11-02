/* lib/examples/insmod.c — Userland insmod utility for PORTIX */
#include <portix.h>
#include <string.h>

#define MAX_MOD_SIZE (256 * 1024)

static char mod_buf[MAX_MOD_SIZE];

int main(int argc, char **argv) {
    const char *path = (argc >= 2) ? argv[1] : "/lib/modules/demo_mod.ko";

    printf("[INSMOD] Cargando modulo '%s'...\n", path);

    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        printf("insmod: no se pudo abrir el archivo '%s'\n", path);
        return 1;
    }

    int total_read = 0;
    while (total_read < MAX_MOD_SIZE) {
        int n = read(fd, mod_buf + total_read, MAX_MOD_SIZE - total_read);
        if (n <= 0) break;
        total_read += n;
    }
    close(fd);

    if (total_read == 0) {
        printf("insmod: archivo de modulo vacio o error de lectura\n");
        return 1;
    }

    printf("[INSMOD] Leidos %d bytes. Solicitando SYS_INIT_MODULE...\n", total_read);

    // Extract module name from path
    const char *slash = strrchr(path, '/');
    const char *name = slash ? slash + 1 : path;

    if (init_module(mod_buf, (unsigned long)total_read, name) < 0) {
        printf("insmod: error al inicializar modulo del kernel '%s'\n", name);
        return 1;
    }

    printf("[INSMOD] Modulo '%s' cargado con exito en Ring 0!\n", name);
    return 0;
}
