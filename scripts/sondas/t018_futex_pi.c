/* T-018: FUTEX_WAKE_OP y mutex con herencia de prioridad (FUTEX_LOCK_PI /
 * FUTEX_UNLOCK_PI / FUTEX_TRYLOCK_PI) tal como los usa la glibc del anfitrion.
 *
 *  T018-1  mutex PTHREAD_PRIO_INHERIT: 8 hilos x 5000 secciones criticas con
 *          trabajo dentro (contencion real: el nucleo debe bloquear en LOCK_PI
 *          y TRASPASAR la propiedad en UNLOCK_PI). Contador == 40000.
 *  T018-2  pthread_mutex_trylock sobre un mutex PI que tiene otro hilo -> EBUSY
 *          (FUTEX_TRYLOCK_PI devuelve EAGAIN al nucleo; glibc lo traduce).
 *  T018-3  volver a tomar un mutex PI que ya tenemos (PTHREAD_MUTEX_ERRORCHECK)
 *          -> EDEADLK.
 *  T018-4  FUTEX_WAKE_OP directo: 4 hilos esperan en A y 2 en B; una sola
 *          llamada hace *B += 1 (de 0 a 1), despierta a los de A y, como el
 *          valor anterior de B era 0 (CMP_EQ 0), tambien a los de B: devuelve 6.
 *  T018-5  segunda WAKE_OP con la misma condicion sobre B (ahora 1): la
 *          comparacion no se cumple y solo despierta a los de A (4).
 *  T018-6  pthread_cond_broadcast con 8 esperadores: los 8 salen.
 *
 * Compilado con el gcc del ANFITRION: gcc -O1 -static -pthread (glibc sin modificar).
 */
#define _GNU_SOURCE
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <limits.h>
#include <linux/futex.h>
#include <sys/syscall.h>
#include <time.h>

static int fallos = 0;
#define OK(cond, num, msg, ...) do { if (cond) printf("T018-" #num ": ok (" msg ")\n", ##__VA_ARGS__); \
    else { printf("T018-" #num ": FALLO (" msg ")\n", ##__VA_ARGS__); fallos++; } } while (0)

/* ---------- T018-1/2/3: mutex PI ---------- */
static pthread_mutex_t mpi;
static volatile long contador = 0;
static volatile long trabajo = 0;

static void *hilo_pi(void *a) {
    (void)a;
    for (int i = 0; i < 5000; i++) {
        pthread_mutex_lock(&mpi);
        long c = contador;
        for (int k = 0; k < 50; k++) trabajo += k;   /* seccion critica con trabajo */
        contador = c + 1;
        pthread_mutex_unlock(&mpi);
    }
    return NULL;
}

static pthread_mutex_t m_busy;
static volatile int busy_listo = 0;
static void *hilo_busy(void *a) {
    (void)a;
    pthread_mutex_lock(&m_busy);
    __sync_synchronize();
    busy_listo = 1;
    struct timespec ts = { 0, 200 * 1000 * 1000 };
    nanosleep(&ts, NULL);
    pthread_mutex_unlock(&m_busy);
    return NULL;
}

/* ---------- T018-4/5: FUTEX_WAKE_OP ---------- */
static int fa = 0, fb = 0;
static volatile int despiertos_a = 0, despiertos_b = 0, en_espera = 0;

static void *espera_en(void *p) {
    int *w = (int *)p;
    __sync_fetch_and_add(&en_espera, 1);
    for (;;) {
        long r = syscall(SYS_futex, w, FUTEX_WAIT_PRIVATE, 0, NULL, NULL, 0);
        if (r == 0) break;                 /* despertado de verdad */
        if (errno == EAGAIN) break;        /* el valor ya cambio (B) */
        /* EINTR u otro: reintentar */
    }
    if (w == &fa) __sync_fetch_and_add(&despiertos_a, 1); else __sync_fetch_and_add(&despiertos_b, 1);
    return NULL;
}

static void esperar_inscritos(int n) {
    /* Los esperadores deben estar DENTRO del futex antes del WAKE_OP; se da
     * tiempo despues de verlos entrar en la funcion. */
    while (en_espera < n) sched_yield();
    struct timespec ts = { 0, 300 * 1000 * 1000 };
    nanosleep(&ts, NULL);
}

/* ---------- T018-6: cond broadcast ---------- */
static pthread_mutex_t mc = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cv = PTHREAD_COND_INITIALIZER;
static int cond_listo = 0, cond_esperando = 0, cond_salidos = 0;
static void *hilo_cond(void *a) {
    (void)a;
    pthread_mutex_lock(&mc);
    cond_esperando++;
    while (!cond_listo) pthread_cond_wait(&cv, &mc);
    cond_salidos++;
    pthread_mutex_unlock(&mc);
    return NULL;
}

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    pthread_mutexattr_t at;
    pthread_mutexattr_init(&at);
    int rp = pthread_mutexattr_setprotocol(&at, PTHREAD_PRIO_INHERIT);
    pthread_mutexattr_settype(&at, PTHREAD_MUTEX_ERRORCHECK);
    pthread_mutex_init(&mpi, &at);
    pthread_mutex_init(&m_busy, &at);
    printf("T018-0: setprotocol(PRIO_INHERIT)=%d, nproc=%ld\n", rp, sysconf(_SC_NPROCESSORS_ONLN));

    /* 1 */
    pthread_t h[8];
    for (int i = 0; i < 8; i++) pthread_create(&h[i], NULL, hilo_pi, NULL);
    for (int i = 0; i < 8; i++) pthread_join(h[i], NULL);
    OK(contador == 40000, 1, "8 hilos x 5000 con mutex PI: contador=%ld", contador);

    /* 2 */
    pthread_t hb;
    pthread_create(&hb, NULL, hilo_busy, NULL);
    while (!busy_listo) sched_yield();
    int rt = pthread_mutex_trylock(&m_busy);
    OK(rt == EBUSY, 2, "trylock de un mutex PI ajeno = %d (EBUSY=%d)", rt, EBUSY);
    pthread_join(hb, NULL);

    /* 3 */
    pthread_mutex_lock(&mpi);
    int rd = pthread_mutex_lock(&mpi);
    OK(rd == EDEADLK, 3, "relock del propio mutex PI errorcheck = %d (EDEADLK=%d)", rd, EDEADLK);
    pthread_mutex_unlock(&mpi);

    /* 4 */
    pthread_t wa[4], wb[2];
    for (int i = 0; i < 4; i++) pthread_create(&wa[i], NULL, espera_en, &fa);
    for (int i = 0; i < 2; i++) pthread_create(&wb[i], NULL, espera_en, &fb);
    esperar_inscritos(6);
    long r4 = syscall(SYS_futex, &fa, FUTEX_WAKE_OP_PRIVATE, INT_MAX, (void *)(long)INT_MAX, &fb,
                      FUTEX_OP(FUTEX_OP_ADD, 1, FUTEX_OP_CMP_EQ, 0));
    for (int i = 0; i < 4; i++) pthread_join(wa[i], NULL);
    for (int i = 0; i < 2; i++) pthread_join(wb[i], NULL);
    OK(r4 == 6 && fb == 1 && despiertos_a == 4 && despiertos_b == 2, 4,
       "WAKE_OP devuelve %ld (esperado 6), B=%d, despiertos A=%d B=%d", r4, fb, despiertos_a, despiertos_b);

    /* 5 */
    en_espera = 0; despiertos_a = 0; despiertos_b = 0;
    for (int i = 0; i < 4; i++) pthread_create(&wa[i], NULL, espera_en, &fa);
    esperar_inscritos(4);
    long r5 = syscall(SYS_futex, &fa, FUTEX_WAKE_OP_PRIVATE, INT_MAX, (void *)(long)INT_MAX, &fb,
                      FUTEX_OP(FUTEX_OP_ADD, 1, FUTEX_OP_CMP_EQ, 0));
    for (int i = 0; i < 4; i++) pthread_join(wa[i], NULL);
    OK(r5 == 4 && fb == 2 && despiertos_a == 4, 5, "WAKE_OP con CMP_EQ fallido devuelve %ld (esperado 4), B=%d", r5, fb);

    /* 6 */
    pthread_t hc[8];
    for (int i = 0; i < 8; i++) pthread_create(&hc[i], NULL, hilo_cond, NULL);
    for (;;) { pthread_mutex_lock(&mc); int e = cond_esperando; pthread_mutex_unlock(&mc); if (e == 8) break; sched_yield(); }
    struct timespec ts = { 0, 100 * 1000 * 1000 }; nanosleep(&ts, NULL);
    pthread_mutex_lock(&mc); cond_listo = 1; pthread_cond_broadcast(&cv); pthread_mutex_unlock(&mc);
    for (int i = 0; i < 8; i++) pthread_join(hc[i], NULL);
    OK(cond_salidos == 8, 6, "pthread_cond_broadcast despierta a los 8 (%d)", cond_salidos);

    if (fallos == 0) printf("[T018-PASS] FUTEX_WAKE_OP y mutex PI (LOCK/UNLOCK/TRYLOCK_PI con traspaso) correctos\n");
    else printf("[T018-FAIL] %d comprobaciones fallidas\n", fallos);
    return fallos ? 1 : 0;
}
