/* lib/examples/kbd_drv.c — Ring-3 PS/2 Keyboard Driver for PORTIX
 *
 * Lifecycle:
 *   1. Probe: register IRQ1 and PS/2 ports (0x60, 0x64)
 *   2. Init: acknowledge any pending data
 *   3. Handle IRQ: on IPC IRQ notification, read scancode from port
 *   4. Forward: write scancodes to /dev/kbd for shell/user processes
 *
 * Run as: kbd_drv  (spawned by init)
 */

#include <portix.h>
#include <string.h>

#define PS2_DATA 0x60
#define PS2_CMD  0x64

static char hex_chars[] = "0123456789ABCDEF";

static void print_hex(unsigned char b) {
    char buf[3];
    buf[0] = hex_chars[b >> 4];
    buf[1] = hex_chars[b & 0xF];
    buf[2] = 0;
    write(1, buf, 2);
}

static void print_str(const char *s) {
    write(1, s, strlen(s));
}

int main(void) {
    int pid = getpid();
    print_str("[KBD_DRV] starting PID ");
    print_hex(pid);
    print_str("\n");

    // Phase 1: Probe — register IRQ1 and I/O ports
    ipc_register_irq(1, pid);
    ioport_register(PS2_DATA, 1);
    ioport_register(PS2_CMD, 1);

    print_str("[KBD_DRV] registered IRQ1, ports 0x60/0x64\n");

    // Phase 2: Init — read any pending data from PS/2 controller
    unsigned char status = ioport_in(PS2_CMD);
    while (status & 0x01) {
        unsigned char junk = ioport_in(PS2_DATA);
        (void)junk;
        print_str("[KBD_DRV] flushed stale byte: ");
        print_hex(junk);
        print_str("\n");
        status = ioport_in(PS2_CMD);
    }

    // Open /dev/kbd for sharing scancodes with shell
    int kbd_fd = open_dev("kbd");
    if (kbd_fd < 0) {
        print_str("[KBD_DRV] WARN: /dev/kbd open failed (no devfs)\n");
    }

    print_str("[KBD_DRV] entering IRQ wait loop\n");

    // Phase 3: Handle IRQ loop
    struct ipc_msg msg;
    while (1) {
        int ret = ipc_recv(&msg, sizeof(msg));
        if (ret > 0) {
            if (msg.msg_type == 0xFF) {
                // IRQ1 fired — read scancode
                unsigned char status = ioport_in(PS2_CMD);
                if (status & 0x01) {
                    unsigned char scancode = ioport_in(PS2_DATA);
                    print_str("[KBD_DRV] scancode: 0x");
                    print_hex(scancode);
                    print_str("\n");

                    // Write to /dev/kbd if opened
                    if (kbd_fd >= 0) {
                        write(kbd_fd, &scancode, 1);
                    }
                }
            }
        }
        yield();
    }

    return 0;
}
