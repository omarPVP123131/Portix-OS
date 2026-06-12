/* lib/examples/pci_drv.c — Ring-3 PCI bus enumerator for PORTIX
 *
 * Enumerates all PCI devices via PCI config space (ports 0xCF8/0xCFC).
 * Demonstrates SYS_IOPORT/IOREAD/IOWRITE from ring-3.
 *
 * Run as: pci_drv  (spawned by init or manually)
 */

#include <portix.h>
#include <string.h>

#define PCI_CONFIG_ADDR  0xCF8
#define PCI_CONFIG_DATA  0xCFC

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

/* Read a 32-bit value from PCI config space */
static unsigned int pci_read(unsigned char bus, unsigned char dev,
                              unsigned char func, unsigned char offset) {
    unsigned int addr = 0x80000000U
        | ((unsigned int)bus << 16)
        | ((unsigned int)dev << 11)
        | ((unsigned int)func << 8)
        | (offset & 0xFC);

    ioport_out(PCI_CONFIG_ADDR, addr & 0xFF);
    ioport_out(PCI_CONFIG_ADDR + 1, (addr >> 8) & 0xFF);
    ioport_out(PCI_CONFIG_ADDR + 2, (addr >> 16) & 0xFF);
    ioport_out(PCI_CONFIG_ADDR + 3, (addr >> 24) & 0xFF);

    unsigned int result = 0;
    result |= (unsigned int)ioport_in(PCI_CONFIG_DATA);
    result |= (unsigned int)ioport_in(PCI_CONFIG_DATA + 1) << 8;
    result |= (unsigned int)ioport_in(PCI_CONFIG_DATA + 2) << 16;
    result |= (unsigned int)ioport_in(PCI_CONFIG_DATA + 3) << 24;
    return result;
}

int main(void) {
    int pid = getpid();
    print_str("[PCI_DRV] starting PID ");
    print_hex(pid, 2);
    print_str("\n");

    // Register PCI config ports
    ioport_register(PCI_CONFIG_ADDR, 1);
    ioport_register(PCI_CONFIG_ADDR + 1, 1);
    ioport_register(PCI_CONFIG_ADDR + 2, 1);
    ioport_register(PCI_CONFIG_ADDR + 3, 1);
    ioport_register(PCI_CONFIG_DATA, 1);
    ioport_register(PCI_CONFIG_DATA + 1, 1);
    ioport_register(PCI_CONFIG_DATA + 2, 1);
    ioport_register(PCI_CONFIG_DATA + 3, 1);

    print_str("[PCI_DRV] registered ports 0xCF8-0xCFC\n");

    // Enumerate PCI bus
    print_str("PCI Devices:\n");
    print_str("Bus Dev Fnc Vendor Device Class\n");

    for (unsigned char bus = 0; bus < 1; bus++) {  // Bus 0 only for now
        for (unsigned char dev = 0; dev < 32; dev++) {
            unsigned int id = pci_read(bus, dev, 0, 0);
            unsigned short vendor = id & 0xFFFF;
            unsigned short device_id = (id >> 16) & 0xFFFF;

            if (vendor == 0xFFFF) continue;  // No device

            // Read class code
            unsigned int class_rev = pci_read(bus, dev, 0, 8);
            unsigned char class_code = (class_rev >> 24) & 0xFF;
            unsigned char subclass = (class_rev >> 16) & 0xFF;

            print_str("  ");
            print_hex(bus, 2); print_str(" ");
            print_hex(dev, 2); print_str(" ");
            print_hex(0, 2); print_str("  ");
            print_hex(vendor, 4); print_str(" ");
            print_hex(device_id, 4); print_str(" ");
            print_hex(class_code, 2); print_str(".");
            print_hex(subclass, 2); print_str("\n");
        }
    }

    print_str("[PCI_DRV] enumeration complete\n");
    return 0;
}
