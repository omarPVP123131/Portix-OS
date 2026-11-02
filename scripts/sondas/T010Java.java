// [b191 / T-005b] Sonda Java: OpenJDK 21 de Debian trixie (openjdk-21-jre-headless) sin
// modificar sobre Portix. Compilada en el anfitrión con `javac --release 11` y ejecutada
// en el invitado con `java -cp /usr/local T010Java`. Cada prueba imprime "T010-n: ok" o
// "T010-n: FALLO ..." y la última línea sólo sale si TODAS pasaron (una prueba que no
// puede fallar no es una prueba).
import java.io.*;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;

public class T010Java {
    static int fallos = 0;
    static void res(int n, boolean ok, String detalle) {
        System.out.println("T010-" + n + (ok ? ": ok " : ": FALLO ") + detalle);
        if (!ok) fallos++;
    }

    // Método caliente con comprobación de nulo implícita: el JIT (C1/C2) la compila como
    // un acceso directo y confía en que el SIGSEGV del núcleo traiga ucontext/si_addr
    // correctos para reanudar en el stub de NullPointerException.
    static int largo(String s) { return s.length(); }

    public static void main(String[] args) throws Exception {
        System.out.println("[t010] " + System.getProperty("java.vm.name") + " "
            + System.getProperty("java.runtime.version") + " en " + System.getProperty("os.name")
            + " hilos=" + Runtime.getRuntime().availableProcessors()
            + " memMax=" + (Runtime.getRuntime().maxMemory() >> 20) + "M");

        // 1. Hilos con contador sincronizado
        final int[] c = {0};
        Thread[] hs = new Thread[4];
        for (int i = 0; i < 4; i++) {
            hs[i] = new Thread(() -> { for (int k = 0; k < 20000; k++) synchronized (c) { c[0]++; } }, "sonda-" + i);
            hs[i].start();
        }
        for (Thread h : hs) h.join();
        res(1, c[0] == 80000, "4 hilos x 20000 synchronized = " + c[0]);

        // 2. ExecutorService + Future (parque de hilos, futex, Callable)
        ExecutorService ex = Executors.newFixedThreadPool(3);
        List<Future<Long>> fs = new ArrayList<>();
        for (int i = 1; i <= 6; i++) { final int n = i; fs.add(ex.submit(() -> { long s = 0; for (int k = 1; k <= n * 1000; k++) s += k; return s; })); }
        long suma = 0; for (Future<Long> f : fs) suma += f.get(10, TimeUnit.SECONDS);
        ex.shutdown();
        res(2, ex.awaitTermination(5, TimeUnit.SECONDS) && suma == 45510500L, "6 Callable en pool de 3 = " + suma);

        // 3. Productor/consumidor con BlockingQueue y wait/notify implícito
        BlockingQueue<Integer> q = new ArrayBlockingQueue<>(8);
        AtomicLong recibido = new AtomicLong();
        Thread cons = new Thread(() -> { try { for (int i = 0; i < 300; i++) recibido.addAndGet(q.take()); } catch (InterruptedException e) {} });
        cons.start();
        for (int i = 0; i < 300; i++) q.put(i);
        cons.join(10000);
        res(3, recibido.get() == 44850, "BlockingQueue 300 elementos suma=" + recibido.get());

        // 4. Ficheros en ext2: escribir, releer, listar, borrar
        Path p = Paths.get("/usr/local/t010.txt");
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 2000; i++) sb.append("linea ").append(i).append('\n');
        Files.write(p, sb.toString().getBytes("UTF-8"));
        List<String> lineas = Files.readAllLines(p);
        boolean okF = lineas.size() == 2000 && lineas.get(1999).equals("linea 1999") && Files.size(p) == sb.length();
        Files.delete(p);
        res(4, okF && !Files.exists(p), "2000 lineas escritas/releidas/borradas en /usr/local (" + sb.length() + " bytes)");

        // 5. Thread.sleep medido con System.nanoTime (clock_gettime MONOTONIC + futex con plazo)
        long t0 = System.nanoTime();
        Thread.sleep(300);
        long ms = (System.nanoTime() - t0) / 1_000_000;
        res(5, ms >= 290 && ms < 1500, "Thread.sleep(300) = " + ms + " ms");

        // 6. NullPointerException implícita en bucle caliente (JIT + SIGSEGV con ucontext)
        int npe = 0; String[] arr = new String[]{"abc", null};
        for (int i = 0; i < 200000; i++) { try { largo(arr[i & 1]); } catch (NullPointerException e) { npe++; } }
        res(6, npe == 100000, "100000 NPE implicitas capturadas en 200000 iteraciones (JIT activo)");

        // 7. Colecciones, ordenación, cadenas, formato y GC bajo presión
        Random r = new Random(42); List<Integer> l = new ArrayList<>();
        for (int i = 0; i < 100000; i++) l.add(r.nextInt(1000000));
        Collections.sort(l);
        boolean orden = true; for (int i = 1; i < l.size(); i++) if (l.get(i - 1) > l.get(i)) { orden = false; break; }
        byte[][] basura = new byte[64][]; for (int i = 0; i < 2000; i++) basura[i % 64] = new byte[64 * 1024];
        String fmt = String.format("%05d|%.3f|%s", 42, Math.PI, Integer.toHexString(255));
        res(7, orden && fmt.equals("00042|3.142|ff"), "sort de 100000 enteros, 128 MB de basura recogida, formato '" + fmt + "'");

        // 8. Excepción con traza de pila, propiedades y entorno
        boolean okE = false;
        try { Object o = Integer.valueOf(1); String s = (String) o; System.out.println(s); }
        catch (ClassCastException e) { okE = e.getStackTrace().length > 0 && e.getStackTrace()[0].getMethodName().equals("main"); }
        res(8, okE && System.getenv("PATH") != null && System.getProperty("user.dir").startsWith("/"), "ClassCastException con traza, getenv(PATH), user.dir=" + System.getProperty("user.dir"));

        if (fallos == 0)
            System.out.println("[T010-PASS] OpenJDK 21 de Debian sin modificar: hilos, pool, BlockingQueue, ficheros ext2, sleep, NPE con JIT, GC, excepciones");
        else
            System.out.println("[T010-FAIL] " + fallos + " pruebas fallidas");
        System.exit(fallos == 0 ? 0 : 1);
    }
}
