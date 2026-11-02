// Sonda C++ (fase posterior a T-005): un programa C++17 de verdad, compilado por el
// g++ del anfitrión con `-static -pthread` (libstdc++ + glibc sin modificar), que
// ejercita lo que un binario C++ corriente exige del núcleo:
//   1. std::thread + std::mutex: 4 hilos x 20000 incrementos = 80000.
//   2. std::condition_variable: 200 vueltas de ping-pong (FUTEX_WAIT/WAKE reales).
//   3. Excepciones a través de hilos (unwinder de libgcc, tablas .eh_frame, TLS de
//      __cxa_get_globals por hilo).
//   4. std::async + std::future (promesa cumplida desde otro hilo).
//   5. Estáticos locales con inicialización segura entre hilos (__cxa_guard_*).
//   6. Contenedores y iostreams: std::map, std::vector, std::string, std::sort,
//      std::ostringstream (malloc intensivo compartido entre hilos).
//   7. std::this_thread::sleep_for de 300 ms medido con std::chrono::steady_clock.
//   8. thread_local con constructor/destructor por hilo.
// Cada comprobación imprime "T006-n: ok" o "T006-n: FALLO …"; al final
// "[T006-PASS] …" sólo si todas pasaron. Un fallo devuelve código 1.
#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <future>
#include <map>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>
#include <unistd.h>
#include <sys/syscall.h>

static int fallos = 0;
static void resultado(int n, bool ok, const std::string& detalle) {
    std::printf("T006-%d: %s %s\n", n, ok ? "ok" : "FALLO", detalle.c_str());
    std::fflush(stdout);
    if (!ok) fallos++;
}

struct Rastro {
    long tid;
    Rastro() : tid(syscall(SYS_gettid)) {}
    ~Rastro() { std::printf("  [t006] destructor thread_local en tid=%ld\n", tid); std::fflush(stdout); }
};
static thread_local Rastro rastro;
static std::atomic<int> destructores_tl{0};

int main() {
    std::printf("[t006] C++ con hilos: pid=%d gettid=%ld hw=%u\n", getpid(), (long)syscall(SYS_gettid),
                std::thread::hardware_concurrency());
    std::fflush(stdout);

    // 1. mutex + 4 hilos
    {
        std::mutex m; long total = 0;
        std::vector<std::thread> hs;
        for (int i = 0; i < 4; i++)
            hs.emplace_back([&] { for (int k = 0; k < 20000; k++) { std::lock_guard<std::mutex> g(m); total++; } });
        for (auto& h : hs) h.join();
        resultado(1, total == 80000, "4 hilos x 20000 bajo std::mutex = " + std::to_string(total));
    }

    // 2. condition_variable ping-pong
    {
        std::mutex m; std::condition_variable cv; int turno = 0; int vueltas = 0;
        std::thread t([&] {
            for (int i = 0; i < 200; i++) {
                std::unique_lock<std::unique_lock<std::mutex>::mutex_type> l(m);
                cv.wait(l, [&] { return turno == 1; });
                vueltas++; turno = 0; cv.notify_one();
            }
        });
        for (int i = 0; i < 200; i++) {
            std::unique_lock<std::mutex> l(m);
            turno = 1; cv.notify_one();
            cv.wait(l, [&] { return turno == 0; });
        }
        t.join();
        resultado(2, vueltas == 200, "ping-pong con condition_variable: " + std::to_string(vueltas) + " vueltas");
    }

    // 3. excepciones en hilos
    {
        std::atomic<int> capturadas{0};
        std::vector<std::thread> hs;
        for (int i = 0; i < 3; i++)
            hs.emplace_back([&, i] {
                try { throw std::runtime_error("hilo " + std::to_string(i)); }
                catch (const std::exception& e) { if (std::string(e.what()) == "hilo " + std::to_string(i)) capturadas++; }
            });
        for (auto& h : hs) h.join();
        bool ok = capturadas == 3;
        try { throw 42; } catch (int v) { ok = ok && v == 42; }
        resultado(3, ok, "excepciones lanzadas y capturadas en 3 hilos + main");
    }

    // 4. async/future
    {
        auto f = std::async(std::launch::async, [] { long s = 0; for (int i = 1; i <= 1000; i++) s += i; return s; });
        std::promise<std::string> p; auto fp = p.get_future();
        std::thread t([&] { p.set_value("promesa cumplida"); });
        long s = f.get(); std::string v = fp.get(); t.join();
        resultado(4, s == 500500 && v == "promesa cumplida", "std::async=" + std::to_string(s) + " future='" + v + "'");
    }

    // 5. estático local con inicialización segura
    {
        std::atomic<int> inic{0};
        auto obtener = [&]() -> int& { static int unico = (std::this_thread::sleep_for(std::chrono::milliseconds(20)), inic++, 7); return unico; };
        std::vector<std::thread> hs; std::atomic<int> suma{0};
        for (int i = 0; i < 4; i++) hs.emplace_back([&] { suma += obtener(); });
        for (auto& h : hs) h.join();
        resultado(5, inic == 1 && suma == 28, "static local inicializado 1 vez (__cxa_guard) desde 4 hilos");
    }

    // 6. contenedores e iostreams con malloc compartido
    {
        std::map<std::string, std::vector<int>> mapa; std::mutex m;
        std::vector<std::thread> hs;
        for (int i = 0; i < 4; i++)
            hs.emplace_back([&, i] {
                for (int k = 0; k < 500; k++) {
                    std::ostringstream os; os << "clave" << (k % 10);
                    std::lock_guard<std::mutex> g(m); mapa[os.str()].push_back(i * 1000 + k);
                }
            });
        for (auto& h : hs) h.join();
        size_t n = 0; for (auto& kv : mapa) { std::sort(kv.second.begin(), kv.second.end()); n += kv.second.size(); }
        resultado(6, mapa.size() == 10 && n == 2000, "std::map de " + std::to_string(mapa.size()) + " claves con " + std::to_string(n) + " enteros ordenados");
    }

    // 7. sleep_for medido
    {
        auto t0 = std::chrono::steady_clock::now();
        std::thread t([] { std::this_thread::sleep_for(std::chrono::milliseconds(300)); });
        t.join();
        auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - t0).count();
        resultado(7, ms >= 290 && ms < 3000, "sleep_for(300 ms) en un hilo midio " + std::to_string(ms) + " ms");
    }

    // 8. thread_local con destructor por hilo
    {
        std::vector<long> tids(3); std::vector<std::thread> hs;
        for (int i = 0; i < 3; i++) hs.emplace_back([&, i] { tids[i] = rastro.tid; });
        for (auto& h : hs) h.join();
        bool distintos = tids[0] != tids[1] && tids[1] != tids[2] && tids[0] != tids[2] && tids[0] != rastro.tid;
        resultado(8, distintos, "thread_local con constructor: tid distinto en cada hilo (" + std::to_string(tids[0]) + "," + std::to_string(tids[1]) + "," + std::to_string(tids[2]) + ")");
    }

    if (fallos == 0) std::printf("[T006-PASS] C++17 con hilos: std::thread, mutex, condition_variable, excepciones, async, static local, STL, thread_local\n");
    else std::printf("[T006-FAIL] %d comprobaciones fallaron\n", fallos);
    std::fflush(stdout);
    return fallos == 0 ? 0 : 1;
}
