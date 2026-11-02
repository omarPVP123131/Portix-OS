/* T-017: execve(2) desde un hilo secundario.
 *
 * Linux: cuando un hilo que no es el lider llama a execve, el hilo que llama
 * pasa a ser el lider del grupo (conserva el PID del proceso), todos los
 * demas hilos desaparecen y el programa nuevo arranca con un solo hilo cuyo
 * gettid() == getpid(). Este programa lo comprueba de verdad:
 *
 *   padre: crea 3 hilos. Los hilos 1 y 3 estan OCUPADOS (bucle sin fin, asi
 *          es probable que ocupen otra CPU y haya que matarlos con SIGKILL
 *          desde fuera). El hilo 2 espera 50 ms y hace execve de este mismo
 *          binario con argv[1]="hijo" y T017_PID=<getpid()> en el entorno.
 *          main hace pthread_join del hilo 2 (dormido en un futex).
 *   hijo:  comprueba getpid()==T017_PID (el PID se conservo aunque exec lo
 *          hizo un hilo), gettid()==getpid() (es el lider), que no quedan
 *          otros hilos (/proc/self/status Threads: 1 si existe, o la ausencia
 *          de los tids anteriores) y que puede crear hilos nuevos.
 *
 * Compilado con el gcc del ANFITRION: gcc -O1 -static -pthread. Sin
 * modificar: es la glibc de la distro del anfitrion la que hace clone/futex.
 */
#define _GNU_SOURCE
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <time.h>
#include <sched.h>
#include <sys/syscall.h>

static volatile unsigned long ocupado = 0;
static char ruta[256];

static void *hilo_ocupado(void *a) {
    (void)a;
    for (;;) { __sync_fetch_and_add(&ocupado, 1); }
    return NULL;
}

static void *hilo_exec(void *a) {
    (void)a;
    struct timespec ts = { 0, 50 * 1000 * 1000 };
    nanosleep(&ts, NULL);
    /* Con 2 CPUs este hilo se ata a la CPU 1 (un AP): asi el execve ocurre
     * fuera del BSP, que es el caso dificil (la continuacion de consola del
     * lider no puede vivir en un AP). */
    long nproc = sysconf(_SC_NPROCESSORS_ONLN);
    int cpu = -1;
    if (nproc >= 2) {
        cpu_set_t m; CPU_ZERO(&m); CPU_SET(1, &m);
        if (sched_setaffinity(0, sizeof m, &m) == 0)
            for (int i = 0; i < 500 && (cpu = sched_getcpu()) != 1; i++) sched_yield();
    } else cpu = sched_getcpu();
    char pid_s[32], tid_s[32];
    snprintf(pid_s, sizeof pid_s, "%d", (int)getpid());
    snprintf(tid_s, sizeof tid_s, "%ld", (long)syscall(SYS_gettid));
    setenv("T017_PID", pid_s, 1);
    setenv("T017_TID", tid_s, 1);
    printf("T017-0: execve desde el hilo tid=%s (pid=%s, ocupado=%lu, nproc=%ld, cpu=%d)\n", tid_s, pid_s, ocupado, nproc, cpu);
    fflush(stdout);
    char *argv2[] = { ruta, "hijo", NULL };
    execv(ruta, argv2);
    printf("T017-FAIL: execv fallo: %s (errno %d)\n", strerror(errno), errno);
    fflush(stdout);
    _exit(3);
    return NULL;
}

static void *hilo_nuevo(void *a) { *(int *)a = 42; return a; }

static int hijo(void) {
    int fallos = 0;
    const char *e = getenv("T017_PID");
    const char *t = getenv("T017_TID");
    long pid = (long)getpid(), tid = (long)syscall(SYS_gettid);
    long pid_antes = e ? atol(e) : -1, tid_exec = t ? atol(t) : -1;
    printf("T017-hijo: pid=%ld tid=%ld ppid=%ld pid_antes=%ld tid_que_hizo_exec=%ld\n",
           pid, tid, (long)getppid(), pid_antes, tid_exec);
    if (pid == pid_antes && pid_antes > 0) printf("T017-1: ok (el PID del proceso se conserva aunque exec lo hizo un hilo)\n");
    else { printf("T017-1: FALLO pid=%ld pid_antes=%ld\n", pid, pid_antes); fallos++; }
    if (tid == pid) printf("T017-2: ok (gettid == getpid: el hilo que hizo exec es ahora el lider)\n");
    else { printf("T017-2: FALLO tid=%ld pid=%ld\n", tid, pid); fallos++; }
    if (tid_exec > 0 && tid_exec != pid) {
        /* El tid viejo del hilo que hizo exec ya no debe existir como hilo. */
        int r = (int)syscall(SYS_tgkill, pid, tid_exec, 0);
        if (r != 0 && errno == ESRCH) printf("T017-3: ok (el tid viejo %ld ya no existe: tgkill -> ESRCH)\n", tid_exec);
        else { printf("T017-3: FALLO tgkill(%ld,%ld,0)=%d errno=%d\n", pid, tid_exec, r, errno); fallos++; }
    } else { printf("T017-3: FALLO sin tid viejo en el entorno\n"); fallos++; }
    /* Hilos: Threads: en /proc/self/status si el nucleo lo publica. */
    FILE *f = fopen("/proc/self/status", "r");
    int threads = -1;
    if (f) {
        char linea[256];
        while (fgets(linea, sizeof linea, f)) if (sscanf(linea, "Threads: %d", &threads) == 1) break;
        fclose(f);
    }
    if (threads == 1) printf("T017-4: ok (/proc/self/status Threads: 1)\n");
    else { printf("T017-4: FALLO Threads: %d (esperado 1)\n", threads); fallos++; }
    /* El proceso nuevo puede crear hilos otra vez. */
    pthread_t h; int v = 0; void *ret = NULL;
    if (pthread_create(&h, NULL, hilo_nuevo, &v) == 0 && pthread_join(h, &ret) == 0 && ret == &v && v == 42)
        printf("T017-5: ok (el proceso nuevo crea y une un hilo)\n");
    else { printf("T017-5: FALLO al crear un hilo tras exec\n"); fallos++; }
    if (fallos == 0) printf("[T017-PASS] execve desde un hilo: PID conservado, hilos antiguos eliminados, el programa nuevo corre y crea hilos\n");
    else printf("[T017-FAIL] %d comprobaciones fallidas\n", fallos);
    fflush(stdout);
    return fallos ? 1 : 0;
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc > 1 && strcmp(argv[1], "hijo") == 0) return hijo();
    snprintf(ruta, sizeof ruta, "%s", argv[0]);
    pthread_t h1, h2, h3;
    pthread_create(&h1, NULL, hilo_ocupado, NULL);
    pthread_create(&h3, NULL, hilo_ocupado, NULL);
    pthread_create(&h2, NULL, hilo_exec, NULL);
    printf("T017-padre: pid=%d tid=%ld, 3 hilos creados (2 ocupados + 1 que hara exec); main espera en join\n",
           (int)getpid(), (long)syscall(SYS_gettid));
    pthread_join(h2, NULL);
    /* Si llegamos aqui, exec no reemplazo el proceso. */
    printf("T017-FAIL: main sobrevivio al execve del hilo\n");
    return 2;
}
