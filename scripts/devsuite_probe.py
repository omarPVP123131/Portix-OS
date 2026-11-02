#!/usr/bin/env python3
"""Sonda dirigida Épica 59: arranca Portix OS headless y verifica en runtime la
suite de desarrollo integrada (trazador de syscalls con TSC, profiler de silicio
por proceso, canal /proc/dbg, mapa de memoria por recorrido de tablas y /bin/dbg)."""
import sys, os, re, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from headless_runner import PortixHeadlessRunner

CMDS = [
    ("trace on",           "[TRACE] Comando 'on' aplicado"),
    ("trace",              "TRAZADOR DE LLAMADAS AL SISTEMA"),
    ("profiler",           "PROFILER DE SILICIO POR PROCESO"),
    ("cat /proc/syscalls", "[EPIC-59-PASS] Trazador de syscalls con TSC operativo"),
    ("cat /proc/profiler", "[EPIC-59-PASS] Profiler de CPU por proceso operativo"),
    ("cat /proc/dbg",      "[EPIC-59-PASS] Canal de depuracion del kernel operativo"),
    ("maps 1",             "[EPIC-59-PASS] Mapa de memoria del proceso generado por recorrido real de tablas"),
    ("devreport",          "[EPIC-59-PASS] Suite de desarrollo, trazador y profiler de silicio operativos"),
    ("run dbg",            "Portix Debugger (dbg) v3.6.0"),
    ("run yes",            "yes"),
    ("run dbg --version",  "portix-dbg v3.6.0"),
    ("run dbg 1 maps",     "REGIONES DEL ESPACIO DE DIRECCIONES"),
    ("run dbg 1 profiler", "Perfil de CPU de PID=1"),
    ("run dbg --trace off","Trazador: comando 'off' aplicado"),
    ("run dbg --trace on", "Trazador: comando 'on' aplicado"),
    # [CICLO v6.x] Depurador simbolizado: símbolos, memoria del objetivo,
    # puntos de parada persistentes y vigilancia por hardware.
    ("run dbg --version",           "portix-dbg v3.6.0"),
    ("run yes",                     "yes"),
    ("run dbg 1 symbols main",      "main"),
    ("run dbg 1 disasm main 8",     "DESENSAMBLADO SIMBOLICO"),
    ("run dbg 1 peek 0x20000000 16","VOLCADO DE MEMORIA DEL OBJETIVO"),
    ("run dbg --break 1 main",      "[BP-SET-PASS]"),
    ("run dbg --bp-list 1",         "[BP-VERIFIED]"),
    ("run dbg --bp-clear 1",        "[BP-RESTORED]"),
    ("run dbg --watch 1 0x20003000 8 w", "[WATCH-ARMED]"),
    ("run dbg --watch-list 1",      "[WATCH-VERIFIED]"),
    ("run dbg --watch-clear 1",     "[WATCH-CLEARED]"),
    ("run wp-demo",                 "[DBGSUITE-PASS]"),
]

def main():
    r = PortixHeadlessRunner("build/dist/portix.img", is_iso=False, verbose=False)
    r.start_qemu()
    booted, boot_log = r.wait_for_pattern("present done", timeout=60.0)
    if booted:  # [b192 / T-016] init se carga despues de `present done`
        booted, boot_log = r.wait_for_pattern("init process PID=1 registered", timeout=30.0)
    print("[PROBE] kernel booted:", booted)
    if not booted:
        print(boot_log[-3000:]); r.close(); sys.exit(1)
    time.sleep(0.5)
    idx0 = r.get_log_len()
    r.send_command("")
    ok, _ = r.wait_for_pattern("root@portix:", start_idx=idx0, timeout=20.0)
    print("[PROBE] shell prompt:", ok)
    if not ok:
        r.close(); sys.exit(1)

    passes = 0
    print("\n" + "=" * 72)
    print("  VERIFICACION EN RUNTIME — ÉPICA 59 (SUITE DE DESARROLLO INTEGRADA)")
    print("=" * 72)
    for cmd, pat in CMDS:
        idx = r.get_log_len()
        r.send_command(cmd)
        ok, out = r.wait_for_pattern(pat, start_idx=idx, timeout=25.0)
        print(f"  [{'PASS' if ok else 'FALLO'}] {cmd:<18} -> {pat[:60]}")
        if ok:
            passes += 1
        else:
            print("        (log):", repr(out[-260:]))

    # Carga de trabajo real en Ring 3 para acumular muestras del profiler
    print("\n  >> Generando carga real en Ring 3 (profiler)...")
    idx = r.get_log_len()
    r.send_command("run portix-bench cpu 2")
    ok, out = r.wait_for_pattern("[CPU-BENCH-DONE]", start_idx=idx, timeout=40.0)
    msh = re.search(r"\[CPU-BENCH-SHARE\] quemar_fnv=(\d+)% quemar_collatz=(\d+)%", out)
    share_fnv_tsc = int(msh.group(1)) if msh else -1
    print(f"  [{'PASS' if ok and msh else 'FALLO'}] carga Ring 3 ejecutada (reparto TSC: quemar_fnv={share_fnv_tsc}%)")
    if ok and msh:
        passes += 1

    # Trazas reales generadas por el trazador (actividad posterior al comando)
    r.send_command("trace")
    time.sleep(1.0)
    idx = r.get_log_len()
    r.send_command("cat /proc/syscalls")
    ok, out = r.wait_for_pattern("Trazas registradas", start_idx=idx, timeout=20.0)
    m = re.search(r"Trazas registradas\s*:\s*(\d+)", out)
    registered = int(m.group(1)) if m else 0
    print(f"\n  [{'PASS' if registered > 0 else 'FALLO'}] trazas registradas por el kernel: {registered}")
    if registered > 0:
        passes += 1
    mm = re.search(r"Coste medio por syscall\s*:\s*(\d+) ciclos", out)
    if mm:
        print(f"        coste medio real por syscall: {mm.group(1)} ciclos")

    # Profiler: muestras reales del planificador
    idx = r.get_log_len()
    r.send_command("cat /proc/profiler")
    ok, out = r.wait_for_pattern("Muestras totales", start_idx=idx, timeout=20.0)
    ms = re.search(r"Muestras totales\s*:\s*(\d+)", out)
    samples = int(ms.group(1)) if ms else 0
    print(f"  [{'PASS' if samples > 0 else 'FALLO'}] muestras del profiler acumuladas: {samples}")
    if samples > 0:
        passes += 1
    # [FRENTE C-3] El top de simbolos debe nombrar las funciones REALES del ELF
    # y su reparto de muestras debe coincidir con el reparto medido por TSC.
    out = re.sub(r"\x1b\[[0-9;?]*[ -/]*[@-~]", "", out)
    hot = {}
    for mh in re.finditer(r"^\s*(\d+)\s+(\d+)\s+\d+\s+0x[0-9a-fA-F]+\s+(\S+)\s*$", out, re.M):
        hot.setdefault(mh.group(3), int(mh.group(1)))
    fnv, col = hot.get("quemar_fnv", 0), hot.get("quemar_collatz", 0)
    share_fnv_prof = fnv * 100 // max(1, fnv + col)
    # Orden fnv>collatz exigible sólo con diferencia clara en el TSC (>=60 %): con ~200
    # muestras y ~55 % real el orden puede invertirse por azar; lo que siempre se exige es
    # que ambos símbolos reales aparezcan y que la proporción coincida con el TSC (±12).
    orden_ok = (fnv > col) if share_fnv_tsc >= 60 else True
    top_ok = fnv > 0 and col > 0 and orden_ok and share_fnv_tsc >= 0 and abs(share_fnv_prof - share_fnv_tsc) <= 12
    print(f"  [{'PASS' if top_ok else 'FALLO'}] top de simbolos: quemar_fnv={fnv} quemar_collatz={col} "
          f"(muestreo {share_fnv_prof}% vs TSC {share_fnv_tsc}%, tolerancia 12 puntos)")
    if top_ok:
        passes += 1

    print("\n" + "-" * 72)
    print(f"  RESULTADO: {passes}/{len(CMDS) + 4} comprobaciones PASS")
    print("-" * 72)
    r.close()
    return 0 if passes == len(CMDS) + 2 else 1

if __name__ == "__main__":
    sys.exit(main())
