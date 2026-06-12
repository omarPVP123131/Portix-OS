/* lib/examples/init.c — First userspace process, launches drivers + shell */

#include <portix.h>
#include <string.h>

static void launch(const char *path, char *argv[], char *envp[]) {
    printf("init: launching %s...\n", path);
    int pid = execve(path, argv, envp);
    if (pid < 0) {
        printf("init: %s failed to start\n", path);
    } else {
        printf("init: %s started as PID %d\n", path, pid);
    }
}

int main(int argc, char *argv[], char *envp[]) {
    (void)argc; (void)argv;
    printf("PORTIX init: starting system...\n");

    char *kbd_argv[] = { "/bin/kbd_drv", NULL };
    launch("/bin/kbd_drv", kbd_argv, envp);

    char *pci_argv[] = { "/bin/pci_drv", NULL };
    launch("/bin/pci_drv", pci_argv, envp);

    char *fb_argv[] = { "/bin/fb_drv", NULL };
    launch("/bin/fb_drv", fb_argv, envp);

    char *shell_argv[] = { "/bin/sh", NULL };
    int ret = execve("/bin/sh", shell_argv, envp);
    if (ret < 0) {
        printf("init: exec /bin/sh failed\n");
    }
    return 0;
}
