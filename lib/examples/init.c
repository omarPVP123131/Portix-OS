/* lib/examples/init.c — First userspace process, launches shell */

#include <portix.h>
#include <string.h>

int main(int argc, char *argv[], char *envp[]) {
    (void)argc; (void)argv;
    printf("PORTIX init: starting system...\n");
    char *shell_argv[] = { "/bin/sh", NULL };
    int ret = execve("/bin/sh", shell_argv, envp);
    if (ret < 0) {
        printf("init: exec /bin/sh failed\n");
    }
    return 0;
}
