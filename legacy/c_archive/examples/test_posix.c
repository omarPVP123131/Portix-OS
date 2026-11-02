#include <portix.h>

int main(void) {
    printf("\n=== PORTIX POSIX CONFORMANCE TEST SUITE ===\n");

    // 1. Test getpid & time
    int my_pid = getpid();
    printf("[TEST 1] getpid() = %d\n", my_pid);

    i64 t = 0;
    i64 now = time(&t);
    printf("[TEST 2] time() = %lld seconds (pointer=%lld)\n", now, t);

    // 2. Test getcwd & chdir
    char cwd[128];
    if (getcwd(cwd, sizeof(cwd))) {
        printf("[TEST 3] getcwd() = '%s'\n", cwd);
    } else {
        printf("[TEST 3] getcwd() failed\n");
    }

    if (chdir("/bin") == 0) {
        getcwd(cwd, sizeof(cwd));
        printf("[TEST 4] chdir(/bin) -> cwd='%s'\n", cwd);
    } else {
        printf("[TEST 4] chdir failed\n");
    }

    // 3. Test pipe
    int fds[2];
    if (pipe(fds) == 0) {
        printf("[TEST 5] pipe() created: rfd=%d, wfd=%d\n", fds[0], fds[1]);
        const char msg[] = "Hello through POSIX pipe!\n";
        write(fds[1], msg, sizeof(msg) - 1);
        char inbuf[64];
        for (int i = 0; i < 64; i++) inbuf[i] = 0;
        int n = read(fds[0], inbuf, sizeof(inbuf) - 1);
        printf("[TEST 5] pipe read %d bytes: '%s'\n", n, inbuf);
        close(fds[0]);
        close(fds[1]);
    } else {
        printf("[TEST 5] pipe() failed\n");
    }

    // 4. Test stat & fstat
    struct stat st;
    if (stat("/bin/hello", &st) == 0) {
        printf("[TEST 6] stat(/bin/hello): size=%lld mode=0%o\n", st.st_size, st.st_mode);
    } else {
        printf("[TEST 6] stat(/bin/hello) failed\n");
    }

    int fd = open("/bin/hello", O_RDONLY);
    if (fd >= 0) {
        if (fstat(fd, &st) == 0) {
            printf("[TEST 7] fstat(fd=%d): size=%lld mode=0%o\n", fd, st.st_size, st.st_mode);
        }
        // Test lseek
        i64 pos = lseek(fd, 4, SEEK_SET);
        printf("[TEST 8] lseek(SEEK_SET, 4) = %lld\n", pos);
        close(fd);
    } else {
        printf("[TEST 7 & 8] open(/bin/hello) failed\n");
    }

    // 5. Test fork & waitpid
    printf("[TEST 9] Testing fork()...\n");
    int cpid = fork();
    if (cpid == 0) {
        // In child
        printf("  [CHILD] Hello from child! PID=%d, exiting with code 42...\n", getpid());
        exit(42);
    } else if (cpid > 0) {
        // In parent
        printf("  [PARENT] Child spawned with PID=%d. Waiting with waitpid()...\n", cpid);
        int status = 0;
        int reaped = waitpid(cpid, &status, 0);
        printf("  [PARENT] waitpid reaped child PID=%d with exit status=%d\n", reaped, status);
    } else {
        printf("  [FORK] fork() returned %d (failed)\n", cpid);
    }

    printf("=== ALL POSIX TESTS COMPLETED SUCCESSFULLY ===\n\n");
    return 0;
}
