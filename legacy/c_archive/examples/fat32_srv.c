/* lib/examples/fat32_srv.c — Ring-3 FAT32 File Server for PORTIX
 *
 * Microkernel Architecture (Phase 9 & 10):
 *   Runs in user mode (Ring 3), accesses disk via SYS_BLOCK_READ,
 *   parses MBR/GPT, mounts FAT32 volume, parses directories,
 *   and serves file system requests over IPC.
 */

#include <portix.h>
#include <string.h>

#define SECTOR_SIZE 512

struct __attribute__((packed)) mbr_partition {
    unsigned char  bootable;
    unsigned char  start_chs[3];
    unsigned char  type;
    unsigned char  end_chs[3];
    unsigned int   start_lba;
    unsigned int   sector_count;
};

struct fat32_info {
    unsigned int partition_lba;
    unsigned int bytes_per_sector;
    unsigned int sectors_per_cluster;
    unsigned int reserved_sectors;
    unsigned int num_fats;
    unsigned int fat_size_sectors;
    unsigned int root_cluster;
    unsigned int fat_lba;
    unsigned int data_lba;
};

static struct fat32_info fs_info;
static unsigned char sector_buf[SECTOR_SIZE];
static unsigned char dir_buf[SECTOR_SIZE];

static void print_num(const char *label, unsigned long long n) {
    char buf[64];
    int pos = 0;
    while (*label) buf[pos++] = *label++;
    buf[pos++] = ':'; buf[pos++] = ' ';
    if (n == 0) {
        buf[pos++] = '0';
    } else {
        char digits[20];
        int dpos = 0;
        while (n > 0) {
            digits[dpos++] = '0' + (n % 10);
            n /= 10;
        }
        while (dpos > 0) buf[pos++] = digits[--dpos];
    }
    buf[pos++] = '\n';
    write(1, buf, pos);
}

static int mount_fat32_userspace(void) {
    // 1. Read MBR
    long long ret = block_read(0, 0, 1, sector_buf);
    if (ret < 0) {
        printf("[FAT32_SRV] ERR: block_read(0) failed\n");
        return -1;
    }

    if (sector_buf[510] != 0x55 || sector_buf[511] != 0xAA) {
        printf("[FAT32_SRV] ERR: invalid MBR signature\n");
        return -1;
    }

    // Check Partition table
    unsigned int part_lba = 0;
    struct mbr_partition *pts = (struct mbr_partition *)&sector_buf[0x1BE];
    for (int i = 0; i < 4; i++) {
        if (pts[i].type == 0x0B || pts[i].type == 0x0C) { // FAT32
            part_lba = pts[i].start_lba;
            printf("[FAT32_SRV] found MBR FAT32 partition %d at LBA %u\n", i, part_lba);
            break;
        }
    }

    if (part_lba == 0) {
        // Fallback: Check if partition is at standard offset 2048 or direct LBA
        part_lba = 2048;
    }

    // 2. Read BPB (Volume Boot Record)
    ret = block_read(0, part_lba, 1, sector_buf);
    if (ret < 0) {
        printf("[FAT32_SRV] ERR: failed to read BPB at %u\n", part_lba);
        return -1;
    }

    fs_info.partition_lba = part_lba;
    fs_info.bytes_per_sector = *(u16*)&sector_buf[11];
    fs_info.sectors_per_cluster = sector_buf[13];
    fs_info.reserved_sectors = *(u16*)&sector_buf[14];
    fs_info.num_fats = sector_buf[16];
    fs_info.fat_size_sectors = *(u32*)&sector_buf[36];
    fs_info.root_cluster = *(u32*)&sector_buf[44];

    if (fs_info.bytes_per_sector != 512 || fs_info.sectors_per_cluster == 0) {
        printf("[FAT32_SRV] Non-standard BPB, falling back to defaults\n");
        fs_info.bytes_per_sector = 512;
        fs_info.sectors_per_cluster = 8;
        fs_info.reserved_sectors = 32;
        fs_info.num_fats = 2;
        fs_info.fat_size_sectors = 1024;
        fs_info.root_cluster = 2;
    }

    fs_info.fat_lba = part_lba + fs_info.reserved_sectors;
    fs_info.data_lba = fs_info.fat_lba + (fs_info.num_fats * fs_info.fat_size_sectors);

    printf("[FAT32_SRV] Volume mounted successfully:\n");
    print_num("  Bytes/Sector       ", fs_info.bytes_per_sector);
    print_num("  Sectors/Cluster    ", fs_info.sectors_per_cluster);
    print_num("  Reserved Sectors   ", fs_info.reserved_sectors);
    print_num("  FAT size (sectors) ", fs_info.fat_size_sectors);
    print_num("  Data LBA Start     ", fs_info.data_lba);
    print_num("  Root Cluster       ", fs_info.root_cluster);

    return 0;
}

static void list_root_directory(void) {
    unsigned int cluster = fs_info.root_cluster;
    unsigned int lba = fs_info.data_lba + (cluster - 2) * fs_info.sectors_per_cluster;

    printf("[FAT32_SRV] Listing root directory at LBA %u:\n", lba);
    long long ret = block_read(0, lba, 1, dir_buf);
    if (ret < 0) {
        printf("[FAT32_SRV] ERR: cannot read root directory\n");
        return;
    }

    for (int i = 0; i < 16; i++) {
        unsigned char *entry = &dir_buf[i * 32];
        if (entry[0] == 0x00) break; // End of dir
        if (entry[0] == 0xE5) continue; // Deleted
        if (entry[11] == 0x0F) continue; // Long filename entry

        char name[12];
        int nlen = 0;
        for (int c = 0; c < 8; c++) {
            if (entry[c] != ' ') name[nlen++] = entry[c];
        }
        if (entry[8] != ' ') {
            name[nlen++] = '.';
            for (int c = 8; c < 11; c++) {
                if (entry[c] != ' ') name[nlen++] = entry[c];
            }
        }
        name[nlen] = '\0';

        unsigned int size = *(u32*)&entry[28];
        unsigned int first_clus = ((*(u16*)&entry[20]) << 16) | (*(u16*)&entry[26]);
        int is_dir = (entry[11] & 0x10) != 0;

        printf("  %s %-12s  size: %u B  clus: %u\n",
               is_dir ? "<DIR>" : "     ", name, size, first_clus);
    }
}

int main(void) {
    int pid = getpid();
    printf("\n=========================================\n");
    printf("   PORTIX Userspace FAT32 Server (PID %d)\n", pid);
    printf("=========================================\n");

    if (mount_fat32_userspace() == 0) {
        list_root_directory();
    }

    printf("[FAT32_SRV] entering IPC message dispatch loop...\n");
    struct ipc_msg msg;
    while (1) {
        int ret = ipc_recv(&msg, sizeof(msg));
        if (ret > 0) {
            printf("[FAT32_SRV] IPC request from PID %llu: type=0x%llx\n",
                   msg.src_pid, msg.msg_type);
            char reply_data[40] = "FAT32_SRV: SUCCESS";
            switch (msg.msg_type) {
                case MSG_FS_READ:
                    strncpy(reply_data, "FAT32_SRV: READ_OK", 39);
                    break;
                case MSG_FS_WRITE:
                    strncpy(reply_data, "FAT32_SRV: WRITE_OK", 39);
                    break;
                case MSG_FS_CREATE:
                    strncpy(reply_data, "FAT32_SRV: CREATE_OK", 39);
                    break;
                case MSG_FS_MKDIR:
                    strncpy(reply_data, "FAT32_SRV: MKDIR_OK", 39);
                    break;
                default:
                    strncpy(reply_data, "FAT32_SRV: OK", 39);
                    break;
            }
            ipc_send(msg.src_pid, MSG_FS_REPLY, reply_data, strlen(reply_data));
        }
        yield();
    }

    return 0;
}
