#!/usr/bin/env python3
"""Sonda dirigida Épica 58: arranca Portix OS headless, ejecuta /bin/test_posix
y captura los resultados de las pruebas 4896-4903 (señales POSIX)."""
import sys, os, re, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from headless_runner import PortixHeadlessRunner

r = PortixHeadlessRunner("build/dist/portix.img", is_iso=False, verbose=False)
r.start_qemu()
booted, boot_log = r.wait_for_pattern("present done", timeout=60.0)
if booted:  # [b192 / T-016] init se carga despues de `present done`
    booted, boot_log = r.wait_for_pattern("init process PID=1 registered", timeout=30.0)
print("[PROBE] kernel booted:", booted)
time.sleep(0.5)
idx0 = r.get_log_len()
r.send_command("")
ok, _ = r.wait_for_pattern("root@portix:", start_idx=idx0, timeout=20.0)
print("[PROBE] shell prompt:", ok)
if not ok:
    print(boot_log[-3000:]); r.close(); sys.exit(1)

r.send_command("")
idx = r.get_log_len()
r.send_command("run test_posix")
finished, log = r.wait_for_pattern("ALL POSIX TESTS COMPLETED SUCCESSFULLY", start_idx=idx, timeout=420.0)
open("build/posix_full_log.txt", "w").write(log)
print("[PROBE] suite completada:", finished, "| bytes:", len(log))

print("\n" + "=" * 72)
print("  RESULTADOS ÉPICA 58 (ENTREGA REAL DE SEÑALES)")
print("=" * 72)
hits = re.findall(r"\[TEST 489[6-9]\][^\r\n]*|\[TEST 490[0-3]\][^\r\n]*", log)
for h in hits:
    print("  " + h.strip())

print("\n--- eventos [SIG] únicos ---")
seen = []
for m in re.finditer(r"\[SIG\][^\r\n]*", log):
    if m.group(0) not in seen:
        seen.append(m.group(0))
for s in seen[:25]:
    print("  " + s.strip())

fails = [h for h in hits if "fallid" in h or "failed" in h]
print("\n  pruebas Épica 58 capturadas:", len(hits), "| con fallo:", len(fails))
r.close()
