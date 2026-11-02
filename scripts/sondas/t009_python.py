# Sonda Python (fase posterior a nasm): corre sobre el CPython 3.13 REAL de Debian trixie
# instalado con `pkg deb-add python3-minimal` (libpython3.13 + libc6 + libexpat + zlib + ssl
# sin modificar, cargados por el ld-linux de Debian). Ejercita lo que un script corriente
# pide al núcleo: hilos de verdad (pthread por debajo), locks (futex), colas, ficheros,
# subprocess (fork+execve+pipes+waitpid), tiempo y señales.
import os, sys, time, threading, queue, signal, subprocess, hashlib, json

fallos = 0
def resultado(n, ok, detalle):
    global fallos
    print(f"T009-{n}: {'ok' if ok else 'FALLO'} {detalle}", flush=True)
    if not ok: fallos += 1

print(f"[t009] CPython {sys.version.split()[0]} de Debian sobre Portix: pid={os.getpid()} tid={threading.get_native_id()} exe={sys.executable}", flush=True)

# 1. 4 hilos x 20000 bajo Lock
total = 0; lock = threading.Lock()
def sumar():
    global total
    for _ in range(20000):
        with lock: total += 1
hs = [threading.Thread(target=sumar) for _ in range(4)]
for h in hs: h.start()
for h in hs: h.join()
resultado(1, total == 80000, f"4 threading.Thread x 20000 bajo Lock = {total}")

# 2. Condition ping-pong
cond = threading.Condition(); turno = [0]; vueltas = [0]
def pong():
    for _ in range(200):
        with cond:
            cond.wait_for(lambda: turno[0] == 1)
            vueltas[0] += 1; turno[0] = 0; cond.notify()
h = threading.Thread(target=pong); h.start()
for _ in range(200):
    with cond:
        turno[0] = 1; cond.notify()
        cond.wait_for(lambda: turno[0] == 0)
h.join()
resultado(2, vueltas[0] == 200, f"ping-pong con Condition: {vueltas[0]} vueltas")

# 3. queue productor/consumidor + native_id distintos
q = queue.Queue(); ids = set()
def productor(p):
    ids.add(threading.get_native_id())
    for k in range(1000): q.put(p * 1000 + k)
    q.put(None)
hs = [threading.Thread(target=productor, args=(p,)) for p in range(3)]
for h in hs: h.start()
fin = 0; n = 0; suma = 0
while fin < 3:
    v = q.get()
    if v is None: fin += 1
    else: n += 1; suma += v
for h in hs: h.join()
resultado(3, n == 3000 and suma == sum(range(3000)) and len(ids) == 3 and os.getpid() not in ids,
          f"queue: {n} mensajes, suma {suma}, {len(ids)} native_id distintos del pid")

# 4. ficheros en ext2 + hashlib
ruta = "/usr/local/t009.txt"
datos = "".join(f"linea {i}\n" for i in range(500))
with open(ruta, "w") as f: f.write(datos)
with open(ruta) as f: leido = f.read()
h = hashlib.sha256(leido.encode()).hexdigest()
resultado(4, leido == datos and os.stat(ruta).st_size == len(datos), f"fichero de {len(datos)} B escrito y releido en ext2, sha256={h[:16]}")

# 5. subprocess (fork + execve + pipes + waitpid) con un binario de Debian
r = subprocess.run(["/usr/bin/python3.13", "-c", "import sys; print(sum(range(101))); sys.exit(7)"], capture_output=True, text=True)
resultado(5, r.returncode == 7 and r.stdout.strip() == "5050", f"subprocess: hijo python devolvio rc={r.returncode} stdout={r.stdout.strip()!r}")

# 6. time.sleep en hilo medido con monotonic
t0 = time.monotonic()
h = threading.Thread(target=lambda: time.sleep(0.3)); h.start(); h.join()
ms = (time.monotonic() - t0) * 1000
resultado(6, 290 <= ms < 3000, f"time.sleep(0.3) en un hilo midio {ms:.0f} ms")

# 7. señal: SIGALRM entregada al hilo principal mientras otro hilo trabaja
recibida = []
signal.signal(signal.SIGALRM, lambda s, f: recibida.append(s))
ocupado = threading.Thread(target=lambda: sum(range(2_000_000))); ocupado.start()
signal.setitimer(signal.ITIMER_REAL, 0.1)
t0 = time.monotonic()
while not recibida and time.monotonic() - t0 < 3: time.sleep(0.01)
ocupado.join()
resultado(7, recibida == [signal.SIGALRM], f"SIGALRM de setitimer entregada al hilo principal: {recibida}")

# 8. json + excepción cruzando hilos (ThreadPoolExecutor no está en python3-minimal: se usa Thread)
caja = {}
def trabajo():
    try:
        json.loads("{malformado")
    except json.JSONDecodeError as e:
        caja["err"] = type(e).__name__
    caja["json"] = json.dumps({"os": "Portix", "n": [1, 2, 3]}, sort_keys=True)
h = threading.Thread(target=trabajo); h.start(); h.join()
resultado(8, caja.get("err") == "JSONDecodeError" and caja.get("json") == '{"n": [1, 2, 3], "os": "Portix"}', f"json en hilo: {caja}")

if fallos == 0:
    print("[T009-PASS] CPython 3.13 de Debian sin modificar: threading, Lock, Condition, queue, ficheros, subprocess, sleep, SIGALRM, json", flush=True)
    sys.exit(0)
print(f"[T009-FAIL] {fallos} comprobaciones fallaron", flush=True)
sys.exit(1)
