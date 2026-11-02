#!/usr/bin/env python3
"""[FRENTE C-3] Sonda: carga Ring 3 conocida (portix-bench cpu 3) y top de simbolos de /proc/profiler."""
import sys, os, re, time
sys.path.insert(0, '/home/user/portix/scripts'); os.chdir('/home/user/portix')
from headless_runner import PortixHeadlessRunner
ANSI = re.compile(r'\x1b\[[0-9;?]*[ -/]*[@-~]')
r = PortixHeadlessRunner("build/dist/portix.img", is_iso=False, verbose=False)
r.start_qemu(); r.wait_for_pattern("present done", timeout=90.0)
time.sleep(0.5); idx0 = r.get_log_len(); r.send_command(""); r.wait_for_pattern("root@portix:", start_idx=idx0, timeout=25.0)
idx = r.get_log_len(); r.send_command("run portix-bench cpu 3")
ok, out = r.wait_for_pattern("[CPU-BENCH-DONE]", start_idx=idx, timeout=60.0)
print("bench:", ok); print([l for l in ANSI.sub('',out).splitlines() if "CPU-BENCH" in l])
idx = r.get_log_len(); r.send_command("cat /proc/profiler")
ok, out = r.wait_for_pattern("[EPIC-59-PASS]", start_idx=idx, timeout=20.0)
out = ANSI.sub('', out)
i = out.find("PERFILES POR PROCESO"); j = out.find("GUIA DE INTERPRETACION"); print(out[i:j])
r.close()
