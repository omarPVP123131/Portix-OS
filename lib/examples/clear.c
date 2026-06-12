/* lib/examples/clear.c — Clear terminal screen */

#include <portix.h>

int main(int argc, char *argv[], char *envp[]) {
    (void)argc; (void)argv; (void)envp;
    write(STDOUT_FILENO, "\033[2J\033[H", 7);
    return 0;
}
