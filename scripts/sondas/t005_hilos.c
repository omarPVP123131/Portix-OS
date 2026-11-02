/* [T-005] Prueba de hilos reales sobre Portix.
 * Cada comprobación imprime "T005-n: ok" o "T005-n: FALLO ..." y el programa
 * termina con [T005-PASS] sólo si todas pasan. Las comprobaciones están
 * diseñadas para FALLAR con la implementación anterior (clone = fork COW,
 * futex = despertar espurio):
 *   1. contador compartido bajo mutex: 4 hilos x 20000 = 80000 (con COW cada
 *      hilo contaría en su copia y main vería 0).
 *   2. gettid distinto por hilo, getpid igual.
 *   3. join devuelve el valor de retorno y ve el búfer escrito por el hilo.
 *   4. ping-pong con variable de condición 200 veces (exige WAKE real).
 *   5. join espera de verdad: el hilo duerme 300 ms antes de poner la bandera.
 *   6. pthread_exit en main con un hilo vivo: el proceso sigue hasta que el
 *      hilo termina (imprime la última línea).
 */
#define _GNU_SOURCE
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <time.h>
#include <sys/syscall.h>

static int fallos = 0;
#define CHECK(n, cond, ...) do { if (cond) printf("T005-%d: ok\n", n); else { printf("T005-%d: FALLO ", n); printf(__VA_ARGS__); printf("\n"); fallos++; } fflush(stdout); } while (0)

/* 1 */
static pthread_mutex_t m1 = PTHREAD_MUTEX_INITIALIZER;
static long contador = 0;
static void *inc(void *a) {
    (void)a;
    for (int i = 0; i < 20000; i++) { pthread_mutex_lock(&m1); contador++; pthread_mutex_unlock(&m1); }
    return NULL;
}

/* 2 */
static pid_t tids[4];
static pid_t pids[4];
static void *ids(void *a) {
    long i = (long)a;
    tids[i] = (pid_t)syscall(SYS_gettid);
    pids[i] = getpid();
    return NULL;
}

/* 3 */
static char *buf3;
static void *escribe(void *a) {
    (void)a;
    buf3 = malloc(64);
    strcpy(buf3, "escrito-por-el-hilo");
    return (void *)0x1234;
}

/* 4 */
static pthread_mutex_t m4 = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t c4 = PTHREAD_COND_INITIALIZER;
static int turno = 0; /* 0 = main, 1 = hilo */
static int vueltas_hilo = 0;
static void *pingpong(void *a) {
    (void)a;
    for (int i = 0; i < 200; i++) {
        pthread_mutex_lock(&m4);
        while (turno != 1) pthread_cond_wait(&c4, &m4);
        vueltas_hilo++;
        turno = 0;
        pthread_cond_signal(&c4);
        pthread_mutex_unlock(&m4);
    }
    return NULL;
}

/* 5 */
static volatile int bandera5 = 0;
static void *tardon(void *a) {
    (void)a;
    struct timespec ts = { 0, 300 * 1000 * 1000 };
    nanosleep(&ts, NULL);
    bandera5 = 1;
    return NULL;
}

/* 6 */
static void *superviviente(void *a) {
    (void)a;
    struct timespec ts = { 0, 200 * 1000 * 1000 };
    nanosleep(&ts, NULL);
    printf("T005-6: ok (el hilo sobrevive al pthread_exit de main)\n");
    fflush(stdout);
    if (fallos == 0) printf("[T005-PASS] hilos reales: memoria compartida, futex real, join, cond, exit por hilo\n");
    else printf("[T005-FAIL] %d comprobaciones fallidas\n", fallos);
    fflush(stdout);
    return NULL;
}

int main(void) {
    pthread_t t[4];
    /* 1 */
    for (long i = 0; i < 4; i++) pthread_create(&t[i], NULL, inc, NULL);
    for (int i = 0; i < 4; i++) pthread_join(t[i], NULL);
    CHECK(1, contador == 80000, "contador=%ld (esperado 80000)", contador);

    /* 2 */
    for (long i = 0; i < 4; i++) pthread_create(&t[i], NULL, ids, (void *)i);
    for (int i = 0; i < 4; i++) pthread_join(t[i], NULL);
    int distintos = 1, mismo_pid = 1;
    for (int i = 0; i < 4; i++) {
        if (pids[i] != getpid()) mismo_pid = 0;
        if (tids[i] == (pid_t)syscall(SYS_gettid)) distintos = 0;
        for (int j = 0; j < i; j++) if (tids[i] == tids[j]) distintos = 0;
    }
    CHECK(2, distintos && mismo_pid, "tids=%d,%d,%d,%d pids=%d,%d,%d,%d main=%d", tids[0], tids[1], tids[2], tids[3], pids[0], pids[1], pids[2], pids[3], getpid());

    /* 3 */
    void *ret = NULL;
    pthread_create(&t[0], NULL, escribe, NULL);
    pthread_join(t[0], &ret);
    CHECK(3, ret == (void *)0x1234 && buf3 && strcmp(buf3, "escrito-por-el-hilo") == 0, "ret=%p buf=%s", ret, buf3 ? buf3 : "(null)");

    /* 4 */
    pthread_create(&t[0], NULL, pingpong, NULL);
    int vueltas_main = 0;
    for (int i = 0; i < 200; i++) {
        pthread_mutex_lock(&m4);
        turno = 1;
        pthread_cond_signal(&c4);
        while (turno != 0) pthread_cond_wait(&c4, &m4);
        vueltas_main++;
        pthread_mutex_unlock(&m4);
    }
    pthread_join(t[0], NULL);
    CHECK(4, vueltas_main == 200 && vueltas_hilo == 200, "main=%d hilo=%d", vueltas_main, vueltas_hilo);

    /* 5 */
    struct timespec a, b;
    clock_gettime(CLOCK_MONOTONIC, &a);
    pthread_create(&t[0], NULL, tardon, NULL);
    pthread_join(t[0], NULL);
    clock_gettime(CLOCK_MONOTONIC, &b);
    long ms = (b.tv_sec - a.tv_sec) * 1000 + (b.tv_nsec - a.tv_nsec) / 1000000;
    CHECK(5, bandera5 == 1 && ms >= 250, "bandera=%d ms=%ld", bandera5, ms);

    /* 6 */
    pthread_create(&t[0], NULL, superviviente, NULL);
    pthread_exit(NULL);
}
