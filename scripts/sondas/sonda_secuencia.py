# [T-003] Sonda secuencial: envia cada orden y ESPERA a que vuelva el prompt
# (o agote SONDA_TIMEOUT s) antes de la siguiente. Log en /tmp/ext2.log.
#   SONDA_TIMEOUT=900 python3 scripts/sondas/sonda_secuencia.py "run pkg add gcc" "run gcc --version"
import sys, time, os
sys.path.insert(0, 'scripts')
from headless_runner import PortixHeadlessRunner
r = PortixHeadlessRunner(os.environ.get('SONDA_IMG', 'build/dist/portix.img'), timeout=30, verbose=False)
r.start_http_server(8000); r.start_https_server(port=8443)
r.start_qemu()
ok, _ = r.wait_for_pattern("present done", 0, 60)
T = float(os.environ.get("SONDA_TIMEOUT", "600"))
try:
    for c in sys.argv[1:]:
        idx = r.get_log_len()
        t0 = time.time()
        r.send_command(c)
        time.sleep(2.0)
        # El terminal repite la orden dos veces (eco crudo y linea de prompt):
        # se espera primero al eco con prompt y SOLO DESPUES al prompt nuevo.
        eco, _ = r.wait_for_pattern("$ " + c, start_idx=idx, timeout=min(T, 30))
        base = r.log.find("$ " + c, idx) + len(c) + 2 if eco else idx + len(c) + 4
        fin, _ = r.wait_for_pattern("root@portix:", start_idx=base, timeout=T)
        print(f"[{time.time()-t0:7.1f}s] {'ok ' if fin else 'T/O'} {c}", flush=True)
        if not fin: break
finally:
    open('/tmp/ext2.log', 'w').write(r.log)
    print("boot", ok)
    r.close()
