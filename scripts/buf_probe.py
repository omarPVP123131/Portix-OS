#!/usr/bin/env python3
"""Sonda rápida del frente C-1: búfer repartido entre DR0-DR3 (`dbg --buftest`)."""
import sys, os, re, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from headless_runner import PortixHeadlessRunner

ANSI = re.compile(r'\x1b\[[0-9;?]*[ -/]*[@-~]')

def main():
    r = PortixHeadlessRunner("build/dist/portix.img", is_iso=False, verbose=False)
    r.start_qemu()
    booted, log = r.wait_for_pattern("present done", timeout=90.0)
    print(f"[BUF-PROBE] arranque: {booted}")
    if not booted:
        print(log[-2000:]); r.close(); return 1
    time.sleep(0.5)
    idx0 = r.get_log_len()
    r.send_command("")
    r.wait_for_pattern("root@portix:", start_idx=idx0, timeout=25.0)

    def cmd(orden, patron, t=60.0):
        idx = r.get_log_len()
        r.send_command(orden)
        ok, out = r.wait_for_pattern(patron, start_idx=idx, timeout=t)
        out = ANSI.sub('', out)
        print(f"[{'PASS' if ok else 'FALLO'}] {orden} -> {patron}")
        for l in out.splitlines():
            l = l.strip()
            if any(k in l for k in ("[SESSION-BUF", "[WATCH-", "[WATCHPOINT]", "[TRAP-STOP]", "ranura=DR", "[SESSION-FAIL]", "DR0 ->", "DR1 ->", "DR2 ->", "DR3 ->")):
                print("   ", l[:160])
        return ok

    ok1 = cmd("run dbg --buftest hello", "[SESSION-BUF-PASS]")
    r.send_command("run yes"); time.sleep(1.0)
    ok2 = cmd("run dbg --watch self 0x20003000 32 w", "[WATCH-ARMED]", t=30.0)
    ok3 = cmd("run dbg --watch-list self", "[WATCH-VERIFIED] 4/4", t=30.0)
    ok4 = cmd("run dbg --watch-clear self", "[WATCH-CLEARED]", t=30.0)
    ok5 = cmd("run dbg --watch self 0x20003000 64 w", "[WATCH-FAIL]", t=30.0)
    r.close()
    todo = all([ok1, ok2, ok3, ok4, ok5])
    print(f"[BUF-PROBE] RESULTADO: {'PASS' if todo else 'FAIL'} ({sum([ok1,ok2,ok3,ok4,ok5])}/5)")
    return 0 if todo else 1

if __name__ == "__main__":
    sys.exit(main())
