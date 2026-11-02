/* lib/examples/sh.c — Interactive Ring-3 POSIX Shell for PORTIX
 *
 * Capabilities:
 *   - Fork-and-exec process isolation (shell persists across commands)
 *   - UNIX Pipelines (cmd1 | cmd2) with kernel pipe() and dup2()
 *   - Stream Redirection (> output, >> append, < input)
 *   - Environment Variables ($PATH, $USER, $PWD, $?, export KEY=VAL)
 *   - Built-ins: cd, pwd, export, echo, clear, help, exit
 *   - Shell Script Execution (sh script.sh)
 */

#include <portix.h>
#include <string.h>

#define MAX_ARGS 32
#define MAX_LINE 256
#define MAX_ENVS 32
#define MAX_ENV_LEN 64

struct env_var {
    char key[32];
    char val[MAX_ENV_LEN];
};

static struct env_var env_table[MAX_ENVS];
static int env_count = 0;
static int last_exit_status = 0;

static const char *get_env(const char *key) {
    for (int i = 0; i < env_count; i++) {
        if (strcmp(env_table[i].key, key) == 0) {
            return env_table[i].val;
        }
    }
    return "";
}

static void set_env(const char *key, const char *val) {
    for (int i = 0; i < env_count; i++) {
        if (strcmp(env_table[i].key, key) == 0) {
            strncpy(env_table[i].val, val, MAX_ENV_LEN - 1);
            env_table[i].val[MAX_ENV_LEN - 1] = '\0';
            return;
        }
    }
    if (env_count < MAX_ENVS) {
        strncpy(env_table[env_count].key, key, 31);
        env_table[env_count].key[31] = '\0';
        strncpy(env_table[env_count].val, val, MAX_ENV_LEN - 1);
        env_table[env_count].val[MAX_ENV_LEN - 1] = '\0';
        env_count++;
    }
}

static void init_env(void) {
    set_env("USER", "root");
    set_env("HOME", "/home/user");
    set_env("PWD", "/home/user");
    set_env("PATH", "/bin");
    set_env("SHELL", "/bin/sh");
}

static int read_line(int fd, char *buf, int max_len) {
    int idx = 0;
    while (idx < max_len - 1) {
        char c;
        int n = read(fd, &c, 1);
        if (n <= 0) {
            if (idx == 0) return -1; // EOF
            break;
        }
        if (c == '\r') continue;
        if (c == '\n') break;
        if (c == '\b' || c == 0x7F) {
            if (idx > 0) {
                idx--;
                if (fd == STDIN_FILENO) {
                    write(STDOUT_FILENO, "\b \b", 3);
                }
            }
            continue;
        }
        buf[idx++] = c;
        if (fd == STDIN_FILENO) {
            write(STDOUT_FILENO, &c, 1);
        }
    }
    buf[idx] = '\0';
    if (fd == STDIN_FILENO && idx > 0) {
        write(STDOUT_FILENO, "\n", 1);
    }
    return idx;
}

static void expand_variables(char *dest, const char *src, int max_len) {
    int d = 0;
    int s = 0;
    while (src[s] && d < max_len - 1) {
        if (src[s] == '$') {
            s++;
            if (src[s] == '?') {
                s++;
                char num[16];
                int p = 0;
                int v = last_exit_status;
                if (v == 0) num[p++] = '0';
                else {
                    char tmp[16]; int tp = 0;
                    while (v > 0) { tmp[tp++] = '0' + (v % 10); v /= 10; }
                    while (tp > 0) num[p++] = tmp[--tp];
                }
                num[p] = '\0';
                for (int i = 0; i < p && d < max_len - 1; i++) dest[d++] = num[i];
                continue;
            }
            char var_key[32];
            int vk = 0;
            while (src[s] && (src[s] == '_' || (src[s] >= 'A' && src[s] <= 'Z') || (src[s] >= 'a' && src[s] <= 'z') || (src[s] >= '0' && src[s] <= '9')) && vk < 31) {
                var_key[vk++] = src[s++];
            }
            var_key[vk] = '\0';
            const char *val = get_env(var_key);
            while (*val && d < max_len - 1) dest[d++] = *val++;
        } else {
            dest[d++] = src[s++];
        }
    }
    dest[d] = '\0';
}

static int parse_args(char *cmd_line, char **argv_out, char **redir_in, char **redir_out, int *append_out) {
    int argc = 0;
    char *p = cmd_line;
    *redir_in = NULL;
    *redir_out = NULL;
    *append_out = 0;

    while (*p) {
        while (*p == ' ' || *p == '\t') p++;
        if (!*p) break;

        // Check I/O redirection
        if (*p == '>') {
            p++;
            if (*p == '>') {
                *append_out = 1;
                p++;
            }
            while (*p == ' ' || *p == '\t') p++;
            *redir_out = p;
            while (*p && *p != ' ' && *p != '\t') p++;
            if (*p) *p++ = '\0';
            continue;
        }
        if (*p == '<') {
            p++;
            while (*p == ' ' || *p == '\t') p++;
            *redir_in = p;
            while (*p && *p != ' ' && *p != '\t') p++;
            if (*p) *p++ = '\0';
            continue;
        }

        if (argc < MAX_ARGS) {
            argv_out[argc++] = p;
        }
        while (*p && *p != ' ' && *p != '\t') p++;
        if (*p) *p++ = '\0';
    }
    argv_out[argc] = NULL;
    return argc;
}

static void resolve_executable_path(char *out_path, const char *cmd) {
    if (cmd[0] == '/' || (cmd[0] == '.' && cmd[1] == '/')) {
        strcpy(out_path, cmd);
        return;
    }
    // Search /bin/
    strcpy(out_path, "/bin/");
    strcat(out_path, cmd);
}

static int execute_single_command(char *cmd_str, char *envp[]) {
    char *argv[MAX_ARGS + 1];
    char *redir_in = NULL;
    char *redir_out = NULL;
    int append_out = 0;

    int argc = parse_args(cmd_str, argv, &redir_in, &redir_out, &append_out);
    if (argc == 0) return 0;

    // Builtins
    if (strcmp(argv[0], "exit") == 0) {
        int code = (argc > 1) ? 0 : 0;
        exit(code);
    }
    if (strcmp(argv[0], "cd") == 0) {
        const char *target = (argc > 1) ? argv[1] : get_env("HOME");
        if (chdir(target) == 0) {
            char new_cwd[128];
            if (getcwd(new_cwd, sizeof(new_cwd))) {
                set_env("PWD", new_cwd);
            }
        } else {
            printf("sh: cd: %s: No such directory\n", target);
        }
        return 0;
    }
    if (strcmp(argv[0], "pwd") == 0) {
        printf("%s\n", get_env("PWD"));
        return 0;
    }
    if (strcmp(argv[0], "export") == 0) {
        if (argc > 1) {
            char *eq = strchr(argv[1], '=');
            if (eq) {
                *eq = '\0';
                set_env(argv[1], eq + 1);
            }
        } else {
            for (int i = 0; i < env_count; i++) {
                printf("declare -x %s=\"%s\"\n", env_table[i].key, env_table[i].val);
            }
        }
        return 0;
    }
    if (strcmp(argv[0], "clear") == 0) {
        printf("\033[2J\033[H");
        return 0;
    }
    if (strcmp(argv[0], "help") == 0) {
        printf("PORTIX POSIX Shell (sh) — Built-in commands:\n");
        printf("  cd [dir]         Change current working directory\n");
        printf("  pwd              Print working directory\n");
        printf("  export KEY=VAL   Set environment variable\n");
        printf("  echo [args...]   Display arguments with variable expansion\n");
        printf("  clear            Clear screen\n");
        printf("  exit [code]      Terminate shell\n");
        printf("  cmd1 | cmd2      Pipeline execution\n");
        printf("  cmd > file       Output redirection\n");
        printf("  cmd < file       Input redirection\n");
        return 0;
    }

    char exec_path[128];
    resolve_executable_path(exec_path, argv[0]);

    // Fork and execute
    int pid = fork();
    if (pid < 0) {
        printf("sh: fork failed\n");
        return -1;
    }

    if (pid == 0) {
        // Child: handle redirection
        if (redir_in) {
            int rfd = open(redir_in, O_RDONLY);
            if (rfd < 0) {
                printf("sh: cannot open %s for reading\n", redir_in);
                exit(1);
            }
            dup2(rfd, STDIN_FILENO);
            close(rfd);
        }
        if (redir_out) {
            int flags = O_WRONLY | O_CREAT | (append_out ? O_APPEND : O_TRUNC);
            int wfd = open(redir_out, flags);
            if (wfd < 0) {
                printf("sh: cannot open %s for writing\n", redir_out);
                exit(1);
            }
            dup2(wfd, STDOUT_FILENO);
            close(wfd);
        }

        execve(exec_path, argv, envp);
        printf("sh: %s: command not found\n", argv[0]);
        exit(127);
    }

    // Parent waits for child
    int status = 0;
    waitpid(pid, &status, 0);
    last_exit_status = status;
    return status;
}

static int execute_pipeline(char *cmd_line, char *envp[]) {
    char *pipe_pos = strchr(cmd_line, '|');
    if (!pipe_pos) {
        return execute_single_command(cmd_line, envp);
    }

    *pipe_pos = '\0';
    char *cmd1 = cmd_line;
    char *cmd2 = pipe_pos + 1;

    int pfd[2];
    if (pipe(pfd) < 0) {
        printf("sh: pipe creation failed\n");
        return -1;
    }

    // Child 1 (Writer)
    int pid1 = fork();
    if (pid1 == 0) {
        dup2(pfd[1], STDOUT_FILENO);
        close(pfd[0]);
        close(pfd[1]);
        execute_single_command(cmd1, envp);
        exit(0);
    }

    // Child 2 (Reader)
    int pid2 = fork();
    if (pid2 == 0) {
        dup2(pfd[0], STDIN_FILENO);
        close(pfd[0]);
        close(pfd[1]);
        execute_single_command(cmd2, envp);
        exit(0);
    }

    close(pfd[0]);
    close(pfd[1]);

    int status1 = 0, status2 = 0;
    waitpid(pid1, &status1, 0);
    waitpid(pid2, &status2, 0);
    last_exit_status = status2;
    return status2;
}

int main(int argc, char *argv_env[], char *envp[]) {
    init_env();

    // Check if running a script: sh <script.sh>
    if (argc > 1) {
        int sfd = open(argv_env[1], O_RDONLY);
        if (sfd < 0) {
            printf("sh: cannot open script '%s'\n", argv_env[1]);
            return 1;
        }
        char raw_line[MAX_LINE];
        char exp_line[MAX_LINE];
        while (read_line(sfd, raw_line, MAX_LINE) >= 0) {
            if (raw_line[0] == '#' || raw_line[0] == '\0') continue;
            expand_variables(exp_line, raw_line, MAX_LINE);
            execute_pipeline(exp_line, envp);
        }
        close(sfd);
        return last_exit_status;
    }

    // Interactive Shell Mode
    printf("=========================================\n");
    printf("   PORTIX Interactive Userspace Shell    \n");
    printf("   Type 'help' for built-in commands     \n");
    printf("=========================================\n");

    char raw_line[MAX_LINE];
    char exp_line[MAX_LINE];

    while (1) {
        printf("root@portix:%s$ ", get_env("PWD"));

        int len = read_line(STDIN_FILENO, raw_line, MAX_LINE);
        if (len < 0) {
            printf("\nexit\n");
            break;
        }
        if (len == 0) continue;

        expand_variables(exp_line, raw_line, MAX_LINE);
        execute_pipeline(exp_line, envp);
    }

    return 0;
}
