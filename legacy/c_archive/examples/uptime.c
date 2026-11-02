/* lib/examples/uptime.c — Show system uptime via PIT ticks */

#include <portix.h>

int main(int argc, char *argv[], char *envp[]) {
    (void)argc; (void)argv; (void)envp;
    u64 ticks = uptime();
    u64 seconds = ticks / 100;
    u64 minutes = seconds / 60;
    u64 hours = minutes / 60;
    minutes %= 60;
    seconds %= 60;
    printf("PORTIX Uptime: %u h %u m %u s\n", hours, minutes, seconds);
    printf("PIT ticks since boot: %u\n", ticks);
    return 0;
}
