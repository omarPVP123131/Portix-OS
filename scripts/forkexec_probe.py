#!/usr/bin/env python3
"""[FRENTE C-4] Sonda: N ciclos fork+execve+waitpid desde Ring 3 y /proc/frames antes/despues (uso: forkexec_probe.py [N])."""
import sys, os, re, time
sys.path.insert(0, '/home/user/portix/scripts'); os.chdir('/home/user/portix')
from headless_runner import PortixHeadlessRunner
ANSI = re.compile(r'\x1b\[[0-9;?]*[ -/]*[@-~]')
N = sys.argv[1] if len(sys.argv) > 1 else "200"
r = PortixHeadlessRunner("build/dist/portix.img", is_iso=False, verbose=False)
r.start_qemu(); r.wait_for_pattern("present done", timeout=90.0)
time.sleep(0.5); idx0 = r.get_log_len(); r.send_command(""); r.wait_for_pattern("root@portix:", start_idx=idx0, timeout=25.0)
def frames():
    idx = r.get_log_len(); r.send_command("cat /proc/frames")
    ok, out = r.wait_for_pattern("root@portix:", start_idx=idx, timeout=15.0)
    out = ANSI.sub('', out); return [l for l in out.splitlines() if "COW" in l]
print("\n".join(frames()))
idx = r.get_log_len(); r.send_command(f"run portix-bench forkexec {N}")
t=time.time(); ok, out = r.wait_for_pattern("FORKEXEC-", start_idx=idx, timeout=300.0)
print(f"{time.time()-t:.1f}s"); print("\n".join(l for l in ANSI.sub('',out).splitlines() if "FORKEXEC" in l or "PF" in l or "panic" in l.lower()))
time.sleep(1); print("\n".join(frames()))
r.close()
