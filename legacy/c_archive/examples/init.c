/* lib/examples/init.c — Master PID 1 Init Daemon for PORTIX
 *
 * Microkernel Architecture:
 *   Spawns essential Ring-3 user space system servers:
 *     - /bin/fat32_srv (userspace filesystem)
 *     - /bin/kbd_drv   (keyboard driver)
 *   Then launches the primary interactive shell:
 *     - /bin/sh
 */

#include <portix.h>
#include <string.h>

static int launch(const char *path, char *argv[], char *envp[]) {
    printf("[INIT] launching %s...\n", path);
    int pid = fork();
    if (pid == 0) {
        // Child
        execve(path, argv, envp);
        printf("[INIT] ERR: execve %s failed\n", path);
        exit(1);
    } else if (pid > 0) {
        printf("[INIT] %s running as PID %d\n", path, pid);
        return pid;
    } else {
        printf("[INIT] ERR: fork failed for %s\n", path);
        return -1;
    }
}

int main(int argc, char *argv[], char *envp[]) {
    (void)argc; (void)argv;
    printf("\n=========================================\n");
    printf("   PORTIX System Master Init (PID 1)     \n");
    printf("=========================================\n");

    // Launch background services
    char *srv_argv[] = { "/bin/fat32_srv", NULL };
    launch("/bin/fat32_srv", srv_argv, envp);

    // Launch primary interactive shell
    char *sh_argv[] = { "/bin/sh", NULL };
    int sh_pid = launch("/bin/sh", sh_argv, envp);

    // Reap zombie children
    while (1) {
        int status = 0;
        int reaped = waitpid(-1, &status, 0);
        if (reaped == sh_pid) {
            printf("[INIT] Shell exited with status %d. Relaunching...\n", status);
            sh_pid = launch("/bin/sh", sh_argv, envp);
        }
        yield();
    }

    return 0;
}
