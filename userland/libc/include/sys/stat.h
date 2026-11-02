#ifndef _SYS_STAT_H
#define _SYS_STAT_H

#include <sys/types.h>

struct stat {
    unsigned long   st_dev;
    unsigned long   st_ino;
    unsigned long   st_nlink;
    unsigned int    st_mode;
    unsigned int    st_uid;
    unsigned int    st_gid;
    unsigned int    __pad0;
    unsigned long   st_rdev;
    long            st_size;
    long            st_blksize;
    long            st_blocks;
    long            st_atime;
    long            st_atime_nsec;
    long            st_mtime;
    long            st_mtime_nsec;
    long            st_ctime;
    long            st_ctime_nsec;
    long            __unused[3];
};

#define S_IFMT   0170000
#define S_IFDIR  0040000
#define S_IFCHR  0020000
#define S_IFBLK  0060000
#define S_IFREG  0100000
#define S_IFIFO  0010000
#define S_IFLNK  0120000
#define S_IFSOCK 0140000

#define S_ISREG(m)  (((m) & S_IFMT) == S_IFREG)
#define S_ISDIR(m)  (((m) & S_IFMT) == S_IFDIR)
#define S_ISCHR(m)  (((m) & S_IFMT) == S_IFCHR)
#define S_ISBLK(m)  (((m) & S_IFMT) == S_IFBLK)
#define S_ISFIFO(m) (((m) & S_IFMT) == S_IFIFO)
#define S_ISLNK(m)  (((m) & S_IFMT) == S_IFLNK)
#define S_ISSOCK(m) (((m) & S_IFMT) == S_IFSOCK)

int stat(const char *pathname, struct stat *statbuf);
int fstat(int fd, struct stat *statbuf);
int lstat(const char *pathname, struct stat *statbuf);
int mkdir(const char *pathname, mode_t mode);

#endif /* _SYS_STAT_H */
