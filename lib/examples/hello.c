/* lib/examples/hello.c — Hello World for PORTIX ring-3 */

#include <portix.h>

int main(int argc, char *argv[], char *envp[]) {
    (void)argc; (void)argv; (void)envp;
    printf("Hello from PORTIX ring-3!\n");
    printf("PID = %d\n", getpid());
    return 0;
}
