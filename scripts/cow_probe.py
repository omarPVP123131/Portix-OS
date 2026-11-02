import sys, os, re, time
sys.path.insert(0, '/home/user/portix/scripts')
os.chdir('/home/user/portix')
from headless_runner import PortixHeadlessRunner
ANSI = re.compile(r'\x1b\[[0-9;?]*[ -/]*[@-~]')
r = PortixHeadlessRunner("build/dist/portix.img", is_iso=False, verbose=False)
r.start_qemu()
ok, log = r.wait_for_pattern("present done", timeout=90.0)
time.sleep(0.5); idx0 = r.get_log_len(); r.send_command(""); r.wait_for_pattern("root@portix:", start_idx=idx0, timeout=25.0)
def cmd(orden, patron, t=60.0, keys=()):
    idx = r.get_log_len(); r.send_command(orden)
    ok, out = r.wait_for_pattern(patron, start_idx=idx, timeout=t); out = ANSI.sub('', out)
    print(f"[{'PASS' if ok else 'FALLO'}] {orden} -> {patron}")
    for l in out.splitlines():
        if any(k in l for k in keys): print("   ", l.strip()[:200])
    return ok
cmd("cat /proc/frames", "[COW-REFCOUNT]", keys=("[COW",))
cmd("run test_posix", "ALL POSIX TESTS COMPLETED", t=240.0, keys=("[TEST 4904]", "[TEST 9]", "Refcount"))
cmd("cat /proc/frames", "[COW-REFCOUNT]", keys=("[COW",))
cmd("run hello", "Hello", t=20.0)
cmd("run hello", "Hello", t=20.0)
cmd("cat /proc/frames", "[COW-REFCOUNT]", keys=("[COW",))
cmd("cat /proc/meminfo", "MemFree", keys=("MemFree",))
r.close()
