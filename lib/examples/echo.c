/* lib/examples/echo.c — Echo arguments */

#include <portix.h>

int main(int argc, char *argv[], char *envp[]) {
    (void)envp;
    for (int i = 1; i < argc; i++) {
        if (i > 1) putchar(' ');
        printf("%s", argv[i]);
    }
    putchar('\n');
    return 0;
}
