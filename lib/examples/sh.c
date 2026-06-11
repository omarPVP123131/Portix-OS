/* lib/examples/sh.c — Minimal shell for PORTIX */

#include <portix.h>
#include <string.h>

#define MAX_ARGS 32
#define MAX_LINE 256

static char line[MAX_LINE];
static char *argv[MAX_ARGS + 1];

static int read_line(char *buf, int n) {
    char *p = fgets(buf, n, STDIN_FILENO);
    if (!p) return 0;
    int len = strlen(buf);
    while (len > 0 && (buf[len-1] == '\n' || buf[len-1] == '\r'))
        buf[--len] = '\0';
    return len;
}

static int parse_line(char *line, char **argv_out) {
    int argc = 0;
    char *p = line;
    while (*p) {
        while (*p == ' ' || *p == '\t') p++;
        if (!*p) break;
        argv_out[argc++] = p;
        while (*p && *p != ' ' && *p != '\t') p++;
        if (*p) *p++ = '\0';
    }
    argv_out[argc] = NULL;
    return argc;
}

int main(int argc, char *argv_env[], char *envp[]) {
    (void)argc; (void)argv_env; (void)envp;
    char cwd[128] = "/";

    while (1) {
        printf("portix:%s$ ", cwd);

        int len = read_line(line, MAX_LINE);
        if (len <= 0) {
            printf("\n");
            break;
        }

        int argc = parse_line(line, argv);
        if (argc == 0) continue;

        if (strcmp(argv[0], "exit") == 0) {
            printf("Goodbye!\n");
            break;
        }
        if (strcmp(argv[0], "help") == 0) {
            printf("Built-in commands:\n");
            printf("  help    - show this help\n");
            printf("  exit    - exit shell\n");
            printf("  clear   - clear screen\n");
            printf("  ls      - list directory\n");
            printf("  cat     - show file\n");
            printf("  echo    - print args\n");
            printf("  *       - any other command runs via execve\n");
            continue;
        }
        if (strcmp(argv[0], "clear") == 0) {
            printf("\033[2J\033[H");
            continue;
        }

        /* Try to run the program via execve */
        char full_path[128];
        if (argv[0][0] != '/' && argv[0][0] != '.') {
            strcpy(full_path, "/bin/");
            strcat(full_path, argv[0]);
        } else {
            strcpy(full_path, argv[0]);
        }
        argv[0] = full_path;

        int ret = execve(full_path, argv, envp);
        if (ret < 0) {
            printf("sh: %s: command not found\n", full_path);
        }
    }

    return 0;
}
