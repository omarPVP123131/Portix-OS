/* lib/examples/mkdir.c — POSIX mkdir userland utility */
#include <portix.h>

int main(int argc, char **argv) {
    if (argc < 2) {
        printf("mkdir: creando directorio por defecto '/home/user/prueba'\n");
        if (mkdir("/home/user/prueba", 0755) < 0) {
            printf("mkdir: no se pudo crear el directorio '/home/user/prueba'\n");
            return 1;
        }
        printf("mkdir: '/home/user/prueba' creado con exito\n");
        return 0;
    }
    for (int i = 1; i < argc; i++) {
        if (mkdir(argv[i], 0755) < 0) {
            printf("mkdir: no se pudo crear el directorio '%s'\n", argv[i]);
            return 1;
        }
    }
    return 0;
}
