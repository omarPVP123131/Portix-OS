#!/usr/bin/env python3
"""Sonda dirigida Épica 60: verifica en runtime la consagración de Portix OS
v6.0 'Chronos' — congelación de la ABI multi-arquitectura, certificación final
de estabilidad bare-metal y lanzamiento maestro de las 4,000 fases."""
import sys, os, re, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from headless_runner import PortixHeadlessRunner

CMDS = [
    ("abifreeze",            "CONGELACION DE LA ABI MULTI-ARQUITECTURA"),
    ("cat /proc/abi",        "[EPIC-60-PASS] ABI multi-arquitectura congelada y verificada"),
    ("certify",              "CERTIFICACION FINAL DE ESTABILIDAD BARE-METAL"),
    ("cat /proc/certification", "Estabilidad bare-metal certificada sin fallos"),
    ("epic60",               "LANZAMIENTO MAESTRO"),
    ("cat /proc/release",    "[PORTIX-CHRONOS-MASTER-PASS]"),
]

def main():
    r = PortixHeadlessRunner("build/dist/portix.img", is_iso=False, verbose=False)
    r.start_qemu()
    booted, boot_log = r.wait_for_pattern("present done", timeout=60.0)
    if booted:
        # [b192 / T-016] init (PID 1) se carga DESPUÉS de `present done`; sin esta espera la
        # certificación corría a veces con 0 procesos (IPC y trazador en FALLO).
        booted, boot_log = r.wait_for_pattern("init process PID=1 registered", timeout=30.0)
    print("[PROBE] kernel booted (PID 1 registrado):", booted)
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
    print("  VERIFICACION EN RUNTIME — ÉPICA 60 (CONSAGRACION v6.0 'CHRONOS')")
    print("=" * 72)
    for cmd, pat in CMDS:
        idx = r.get_log_len()
        r.send_command(cmd)
        ok, out = r.wait_for_pattern(pat, start_idx=idx, timeout=25.0)
        print(f"  [{'PASS' if ok else 'FALLO'}] {cmd:<22} -> {pat[:56]}")
        if ok:
            passes += 1
        else:
            print("        (log):", repr(out[-300:]))

    # Congelación de la ABI: huella digital y recuento de syscalls
    idx = r.get_log_len()
    r.send_command("cat /proc/abi")
    ok, out = r.wait_for_pattern("Huella FNV-1a", start_idx=idx, timeout=20.0)
    digest = re.search(r"0x([0-9a-f]{16})", out)
    syscalls = re.search(r"Syscalls del ABI nativo\s*:\s*(\d+)", out)
    d_ok = digest is not None and digest.group(1) != "0" * 16
    s_ok = syscalls is not None and int(syscalls.group(1)) > 300
    print(f"  [{'PASS' if d_ok else 'FALLO'}] huella FNV-1a de la ABI: 0x{digest.group(1) if digest else '?'}")
    print(f"  [{'PASS' if s_ok else 'FALLO'}] syscalls nativas congeladas: {syscalls.group(1) if syscalls else '?'}")
    passes += (1 if d_ok else 0) + (1 if s_ok else 0)

    # Certificación: comprobaciones vivas y ausencia de fallos
    idx = r.get_log_len()
    r.send_command("cat /proc/certification")
    ok, out = r.wait_for_pattern("Correctas:", start_idx=idx, timeout=25.0)
    m = re.search(r"Correctas:\s*(\d+)\s*\|\s*Fallidas:\s*(\d+)\s*\|\s*Avisos:\s*(\d+)", out)
    if m:
        okc, fail, warn = int(m.group(1)), int(m.group(2)), int(m.group(3))
        print(f"  [{'PASS' if fail == 0 and okc >= 12 else 'FALLO'}] certificacion en vivo: {okc} correctas, {fail} fallidas, {warn} avisos")
        if fail == 0 and okc >= 12:
            passes += 1
        else:
            # [b192] Diagnóstico: qué comprobaciones concretas fallaron o avisaron (recert 110 dio 13/2/1 sin detalle)
            for ln in out.splitlines():
                if re.search(r"\b(FALLO|FAIL|AVISO|WARN)\b", ln) and "Correctas" not in ln:
                    print("        (detalle):", ln.strip()[:200])
    else:
        print("  [FALLO] no se pudo leer el recuento de la certificacion")

    # Segunda ronda: estabilidad reproducible
    idx = r.get_log_len()
    r.send_command("certify")
    ok, out = r.wait_for_pattern("Rondas de certificacion ejecutadas", start_idx=idx, timeout=25.0)
    r2 = re.search(r"Rondas de certificacion ejecutadas:\s*(\d+)", out)
    rounds = int(r2.group(1)) if r2 else 0
    print(f"  [{'PASS' if rounds >= 2 else 'FALLO'}] rondas de certificacion ejecutadas: {rounds}")
    if rounds >= 2:
        passes += 1

    total = len(CMDS) + 4
    print("\n" + "-" * 72)
    print(f"  RESULTADO: {passes}/{total} comprobaciones PASS")
    if passes < total:
        # [b192 / T-016] Log serie completo de la pasada fallida para diagnosticar el intermitente
        try:
            with r.log_lock:
                _todo = r.log
            with open("build/consagracion_fallo.log", "w", encoding="utf-8", errors="replace") as _fh: _fh.write(_todo)
            print("  log serie guardado en build/consagracion_fallo.log")
        except Exception as _e:
            print(f"  (no se pudo guardar el log serie: {_e})")
    print("-" * 72)
    r.close()
    return 0 if passes == total else 1

if __name__ == "__main__":
    sys.exit(main())
