/* lib/examples/fb_drv.c — Ring-3 Framebuffer Driver for PORTIX
 *
 * Maps the framebuffer physical memory into ring-3 address space
 * via SYS_MMAP_DEVICE and draws patterns directly.
 *
 * Demonstrates direct MMIO access from userspace.
 *
 * Run as: fb_drv  (spawned by init)
 */

#include <portix.h>
#include <string.h>

/* Framebuffer physical address (set by bootloader, typically 0xFD000000+) */
#define FB_PHYS  0xFD000000
#define FB_SIZE  0x400000   /* 4 MB */

static char hex_chars[] = "0123456789ABCDEF";

static void print_hex(unsigned int v, int digits) {
    char buf[16];
    for (int i = digits - 1; i >= 0; i--) {
        buf[i] = hex_chars[v & 0xF];
        v >>= 4;
    }
    buf[digits] = 0;
    write(1, buf, digits);
}

static void print_str(const char *s) {
    write(1, s, strlen(s));
}

static void put_pixel(unsigned int *fb, int x, int y,
                      unsigned char r, unsigned char g, unsigned char b) {
    unsigned int *p = fb + y * 1024 + x;  /* 1024 px wide */
    *p = (r << 16) | (g << 8) | b;
}

int main(void) {
    int pid = getpid();
    print_str("[FB_DRV] starting PID ");
    print_hex(pid, 2);
    print_str("\n");

    // Map framebuffer physical memory into ring-3
    unsigned int *fb = (unsigned int *)mmap_device(FB_PHYS, FB_SIZE);
    if ((unsigned long long)fb == (unsigned long long)-1 || fb == NULL) {
        print_str("[FB_DRV] ERROR: mmap_device failed (FB not mappable)\n");
        print_str("[FB_DRV] continuing without FB access\n");
        return 1;
    }

    print_str("[FB_DRV] framebuffer mapped at 0x");
    print_hex((unsigned int)(unsigned long long)fb, 8);
    print_str("\n");

    // Draw a test pattern: gradient
    for (int y = 0; y < 200; y++) {
        for (int x = 0; x < 1024; x++) {
            put_pixel(fb, x, y, x & 0xFF, y & 0xFF, (x + y) & 0xFF);
        }
    }

    print_str("[FB_DRV] drew test pattern\n");
    print_str("[FB_DRV] entering idle loop\n");

    // Idle
    while (1) {
        yield();
    }

    return 0;
}
