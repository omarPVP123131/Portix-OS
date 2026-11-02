#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(int argc, char **argv) {
    printf("\n╔══════════════════════════════════════════════════════╗\n");
    printf("║  HOLA MUNDO DESDE C (Ring-3 Userland en Portix OS)   ║\n");
    printf("╚══════════════════════════════════════════════════════╝\n\n");

    printf("[C-USERLAND] Ejecutando binario ELF compilado con GCC estándar.\n");
    printf("[C-USERLAND] Argumentos recibidos (argc = %d):\n", argc);
    for (int i = 0; i < argc; i++) {
        printf("  argv[%d] = %s\n", i, argv[i]);
    }

    pid_t pid = getpid();
    uid_t uid = getuid();
    printf("[C-USERLAND] Identidad POSIX: PID = %d, UID = %d\n", pid, uid);

    // Test dynamic memory allocation with malloc
    char *buffer = (char *)malloc(128);
    if (buffer) {
        snprintf(buffer, 128, "[C-USERLAND] Memoria dinamica asignada con malloc() en 0x%x\n", (unsigned int)((size_t)buffer & 0xFFFFFFFF));
        printf("%s", buffer);
        free(buffer);
        printf("[C-USERLAND] Memoria liberada correctamente con free().\n");
    }

    printf("[C-USERLAND] Prueba completada con exito (return 0).\n\n");
    return 0;
}
