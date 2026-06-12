#include "portix.h"

#define SECTOR_SIZE 512
#define MBR_SIGNATURE_OFFSET 510

struct chs {
    unsigned char head;
    unsigned char sector_cyl;
    unsigned char cyl;
};

struct partition_entry {
    unsigned char  bootable;
    struct chs     start_chs;
    unsigned char  type;
    struct chs     end_chs;
    unsigned int   start_lba;
    unsigned int   sector_count;
};

static void print_partition(int i, struct partition_entry *p) {
    char buf[64];
    int pos = 0;
    buf[pos++] = '0' + i;
    buf[pos++] = ':';
    buf[pos++] = ' ';
    if (p->type == 0) {
        buf[pos++] = 'E'; buf[pos++] = 'm'; buf[pos++] = 'p'; buf[pos++] = 't'; buf[pos++] = 'y';
    } else {
        buf[pos++] = 't'; buf[pos++] = 'y'; buf[pos++] = 'p'; buf[pos++] = 'e'; buf[pos++] = '=';
        unsigned char t = p->type;
        if (t >= 100) { buf[pos++] = '0' + t / 100; t %= 100; }
        if (t >= 10)  { buf[pos++] = '0' + t / 10; t %= 10; }
        buf[pos++] = '0' + t;
        buf[pos++] = ' ';
        unsigned int start = p->start_lba;
        buf[pos++] = 'L'; buf[pos++] = 'B'; buf[pos++] = 'A'; buf[pos++] = '=';
        if (start >= 1000000000) { buf[pos++] = '0' + start / 1000000000; start %= 1000000000; }
        if (start >= 100000000)  { buf[pos++] = '0' + start / 100000000; start %= 100000000; }
        if (start >= 10000000)   { buf[pos++] = '0' + start / 10000000; start %= 10000000; }
        if (start >= 1000000)    { buf[pos++] = '0' + start / 1000000; start %= 1000000; }
        if (start >= 100000)     { buf[pos++] = '0' + start / 100000; start %= 100000; }
        if (start >= 10000)      { buf[pos++] = '0' + start / 10000; start %= 10000; }
        if (start >= 1000)       { buf[pos++] = '0' + start / 1000; start %= 1000; }
        if (start >= 100)        { buf[pos++] = '0' + start / 100; start %= 100; }
        if (start >= 10)         { buf[pos++] = '0' + start / 10; start %= 10; }
        buf[pos++] = '0' + start;
        buf[pos++] = ' ';
        unsigned int cnt = p->sector_count;
        buf[pos++] = 'c'; buf[pos++] = 'n'; buf[pos++] = 't'; buf[pos++] = '=';
        if (cnt >= 100000)   { buf[pos++] = '0' + cnt / 100000; cnt %= 100000; }
        if (cnt >= 10000)    { buf[pos++] = '0' + cnt / 10000; cnt %= 10000; }
        if (cnt >= 1000)     { buf[pos++] = '0' + cnt / 1000; cnt %= 1000; }
        if (cnt >= 100)      { buf[pos++] = '0' + cnt / 100; cnt %= 100; }
        if (cnt >= 10)       { buf[pos++] = '0' + cnt / 10; cnt %= 10; }
        buf[pos++] = '0' + cnt;
    }
    buf[pos++] = '\n';
    write(1, buf, pos);
}

int main(void) {
    write(1, "[ATADRV] starting, registering IRQ14\n", 38);
    int pid = getpid();
    ipc_register_irq(14, pid);

    write(1, "[ATADRV] reading MBR (block 0)...\n", 34);
    unsigned char mbr[SECTOR_SIZE];
    long long bytes = block_read(0, 0, 1, mbr);
    if (bytes < 0) {
        write(1, "[ATADRV] ERROR: block_read failed\n", 35);
        return 1;
    }

    write(1, "[ATADRV] MBR signature: 0x", 25);
    char hex[3];
    unsigned char sig_hi = mbr[MBR_SIGNATURE_OFFSET];
    unsigned char sig_lo = mbr[MBR_SIGNATURE_OFFSET + 1];
    const char *h = "0123456789ABCDEF";
    hex[0] = h[sig_hi >> 4]; hex[1] = h[sig_hi & 0xF]; hex[2] = 0;
    write(1, hex, 2);
    hex[0] = h[sig_lo >> 4]; hex[1] = h[sig_lo & 0xF];
    write(1, hex, 2);
    write(1, "\n", 1);

    write(1, "[ATADRV] Partitions:\n", 21);
    for (int i = 0; i < 4; i++) {
        struct partition_entry *p = (struct partition_entry *)&mbr[0x1BE + i * 16];
        print_partition(i, p);
    }

    write(1, "[ATADRV] entering IRQ wait loop\n", 33);

    struct ipc_msg msg;
    while (1) {
        int ret = ipc_recv(&msg, sizeof(msg));
        if (ret > 0) {
            if (msg.msg_type == 0xFF) {
                write(1, "[ATADRV] IRQ", 12);
                unsigned char irq = msg.data[0];
                char ic[4];
                int ip = 0;
                if (irq >= 10) { ic[ip++] = '0' + irq / 10; irq %= 10; }
                ic[ip++] = '0' + irq;
                ic[ip++] = '\n';
                write(1, ic, ip);
            }
        }
        yield();
    }

    return 0;
}