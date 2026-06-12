/* lib/examples/help.c — List available commands for PORTIX */

#include <portix.h>

int main(int argc, char *argv[], char *envp[]) {
    (void)argc; (void)argv; (void)envp;
    printf("PORTIX Ring-3 Commands:\n");
    printf("  help      - show this help\n");
    printf("  clear     - clear terminal screen\n");
    printf("  ls        - list directory contents\n");
    printf("  cat       - display file contents\n");
    printf("  echo      - echo arguments\n");
    printf("  uptime    - show system uptime\n");
    printf("  hello     - ring-3 demo program\n");
    printf("  exit      - exit shell\n");
    return 0;
}
