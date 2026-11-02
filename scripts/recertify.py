#!/usr/bin/env python3
"""
scripts/recertify.py — Re-consagración de Portix OS v6.0 'Chronos'

Reconstruye la distribución desde cero y ejecuta TODAS las suites de
certificación, emitiendo un informe final con veredicto único.

Uso:
    python3 scripts/recertify.py                # reconstruir + certificar todo
    python3 scripts/recertify.py --no-build     # certificar sobre imágenes ya construidas
    python3 scripts/recertify.py --only 60      # reconstruir y ejecutar sólo la sonda indicada

Suites ejecutadas:
    1. interactive_runner.py   — 100+ pruebas de uso real en QEMU (consola + escritorio)
    2. signal_probe.py         — Épica 58: señales POSIX, suite completa de 4,903 pruebas
    3. devsuite_probe.py       — Épica 59: trazador TSC, profiler de silicio y /bin/dbg
    4. consagracion_probe.py   — Épica 60: ABI congelada y certificación bare-metal
    5. dbg_probe.py            — ciclo v6.x: cadena de depuración de Ring 3

NOTA DE ENTORNO: `build/` no forma parte de las instantáneas del espacio de
trabajo, por lo que las imágenes deben reconstruirse en cada sesión; este script
existe precisamente para que esa reconstrucción y su certificación sean un único
comando reproducible.
"""
import argparse
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
os.chdir(ROOT)

GREEN = "\033[92m"
RED = "\033[91m"
BOLD = "\033[1m"
CYAN = "\033[96m"
RESET = "\033[0m"


def saneamiento(etiqueta):
    """Termina instancias QEMU residuales antes de reconstruir o certificar.

    El runner arranca QEMU con `file.locking=off`, de modo que una VM que no se
    cerró puede seguir escribiendo en `build/dist/portix.img` mientras el maestro de
    construcción la reescribe: el resultado es un FAT32 que la suite siguiente no
    consigue montar ("no se pudo montar FAT32 en ningun dispositivo") y decenas
    de fallos en cascada que no tienen nada que ver con el código."""
    # [b188] Solo se terminan las VMs HUERFANAS (su runner murio: PPID 1). Todos los
    # runners trabajan ya sobre copias privadas en build/dist/pruebas/run/, asi que una suite
    # (headless_runner) puede correr EN PARALELO con la recertificacion sin que
    # esta la mate: antes `pkill -f qemu-system-x86_64` tumbaba cualquier VM viva.
    huerfanas = []
    try:
        ps = subprocess.run(["ps", "-eo", "pid=,ppid=,args="], capture_output=True, text=True).stdout
        for linea in ps.splitlines():
            partes = linea.split(None, 2)
            if len(partes) == 3 and "qemu-system-x86_64" in partes[2] and partes[1] == "1":
                huerfanas.append(partes[0])
    except Exception:
        pass
    if huerfanas:
        subprocess.run(["kill", "-9", *huerfanas], capture_output=True)
        print(f"      [saneamiento] {etiqueta}: se terminaron {len(huerfanas)} VM(s) huerfana(s)")
        time.sleep(1.0)
    subprocess.run(["sync"], capture_output=True)


def run(cmd, timeout, env_extra=None):
    env = dict(os.environ)
    env["PATH"] = os.path.expanduser("~/.cargo/bin") + ":" + env.get("PATH", "")
    if env_extra:
        env.update(env_extra)
    t0 = time.time()
    res = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=env)
    return res.returncode, res.stdout + res.stderr, time.time() - t0


def banner():
    print(f"{BOLD}{CYAN}")
    print("  ╔══════════════════════════════════════════════════════════════════╗")
    print("  ║      PORTIX OS v6.0 'CHRONOS' — RE-CONSAGRACION INTEGRAL         ║")
    print("  ║      Reconstruccion + certificacion de las 4,000 fases           ║")
    print("  ╚══════════════════════════════════════════════════════════════════╝")
    print(RESET, end="")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--no-build", action="store_true", help="no reconstruir las imágenes")
    ap.add_argument("--only", choices=["58", "59", "60", "dbg", "interactive"], help="ejecutar sólo una suite")
    args = ap.parse_args()

    banner()
    resultados = []

    # ── 0. Auditoría estática de pruebas (ciclo 50) ─────────────────────────
    # Si existe una prueba tautológica o que acepte un error genérico como
    # éxito, NO se certifica nada: se corta aquí.
    print(f"{BOLD}[0/7] Auditoria estricta de pruebas (tautologias / errno generico){RESET}")
    rc, out, dt = run([sys.executable, "scripts/auditoria_pruebas.py", "--estricto"], timeout=120)
    m = re.search(r"AUDITORIA TEST_POSIX:\s*hallazgos=(\d+)", out)
    hallazgos = int(m.group(1)) if m else -1
    ok = rc == 0 and hallazgos == 0
    print(f"      {'OK' if ok else 'FALLO'}  hallazgos={hallazgos}  ({dt:.1f}s)")
    resultados.append(("Auditoria estricta de pruebas (0 tautologias, 0 errno generico)", ok, f"hallazgos={hallazgos}"))
    if not ok:
        print(out[-3000:])
        print(f"\n{RED}[PORTIX-CHRONOS-FAIL] La auditoria de pruebas encontro hallazgos{RESET}")
        return 1

    # ── 1. Reconstrucción ───────────────────────────────────────────────────
    if not args.no_build:
        print(f"{BOLD}[1/7] Reconstruyendo la distribucion (kernel + userland + imagenes){RESET}")
        saneamiento("antes de reconstruir")
        rc, out, dt = run([sys.executable, "scripts/build.py", "--no-run"], timeout=1800)
        ok = rc == 0 and "ISO" in out
        print(f"      {'OK' if ok else 'FALLO'}  build.py --no-run  ({dt:.1f}s)")
        if not ok:
            print(out[-2500:])
            return 1
        resultados.append(("Reconstruccion de imagenes (BIOS+UEFI)", ok, ""))
    else:
        print(f"{BOLD}[1/7] Reconstruccion omitida (--no-build){RESET}")

    # ── 2..5. Suites de certificación ───────────────────────────────────────
    suites = [
        ("interactive", "Suite interactiva de uso real (QEMU)", [sys.executable, "scripts/interactive_runner.py"], 1800,
         r"TOTAL DE PRUEBAS DE USO REAL:\s*(\d+).*?PASSED:\s*(\d+).*?FAILED:\s*(\d+)"),
        ("58", "Épica 58 — Señales POSIX (suite completa)", [sys.executable, "scripts/signal_probe.py"], 900,
         r"suite completada:\s*(True|False)"),
        ("59", "Épica 59 — Trazador TSC y profiler de silicio", [sys.executable, "scripts/devsuite_probe.py"], 900,
         r"RESULTADO:\s*(\d+)/(\d+)"),
        ("dbg", "Ciclo v6.x — cadena de depuracion de Ring 3 (/bin/dbg)", [sys.executable, "scripts/dbg_probe.py"], 900,
         r"SONDA DE DEPURACIÓN:\s*(\d+)/(\d+) comprobaciones PASS"),
        ("60", "Épica 60 — ABI congelada y certificacion bare-metal", [sys.executable, "scripts/consagracion_probe.py"], 900,
         r"RESULTADO:\s*(\d+)/(\d+)"),
        # [CICLO 49] El nucleo vive en la mitad superior: el arranque UEFI real
        # (OVMF) se certifica en cada ronda, no solo el BIOS.
        ("uefi", "Ciclo 49 — arranque UEFI real (OVMF) con nucleo en la mitad superior", [sys.executable, "scripts/uefi_probe.py"], 300,
         r"SONDA UEFI:\s*(\d+)/(\d+)"),
    ]

    paso = 2
    for clave, titulo, cmd, timeout, patron in suites:
        if args.only and args.only != clave:
            continue
        print(f"\n{BOLD}[{paso}/7] {titulo}{RESET}")
        paso += 1
        saneamiento("antes de la suite")
        try:
            rc, out, dt = run(cmd, timeout=timeout)
        except subprocess.TimeoutExpired:
            print(f"      {RED}FALLO{RESET}  tiempo agotado ({timeout}s)")
            resultados.append((titulo, False, "timeout"))
            continue

        logp = os.path.join("build", ".recert_%s.log" % clave)
        try:
            with open(logp, "w") as fh:
                fh.write(out)
        except OSError:
            pass

        m = re.search(patron, out, re.S)
        if clave == "interactive" and m:
            total, passed, failed = (int(x) for x in m.groups())
            ok = failed == 0 and passed == total and total > 0
            detalle = f"{passed}/{total} PASS, {failed} FAIL"
        elif clave == "58" and m:
            ok = m.group(1) == "True"
            hits = len(re.findall(r"\[TEST 490[0-3]\]", out)) + len(re.findall(r"\[TEST 489[6-9]\]", out))
            detalle = f"suite completada={m.group(1)}, pruebas Épica 58 capturadas={hits}"
        elif m:
            got, tot = int(m.group(1)), int(m.group(2))
            ok = got == tot and tot > 0
            detalle = f"{got}/{tot} PASS"
        else:
            ok, detalle = False, "sin patron de resultado"

        color = GREEN if ok else RED
        print(f"      {color}{'OK' if ok else 'FALLO'}{RESET}  {detalle}  ({dt:.1f}s)")
        if not ok:
            tail = [l for l in out.splitlines() if "FALLO" in l or "FAIL]" in l][-6:]
            for l in tail:
                print("        " + l.strip()[:110])
        resultados.append((titulo, ok, detalle))

    # ── Informe final ───────────────────────────────────────────────────────
    print(f"\n{BOLD}{CYAN}  INFORME DE RE-CONSAGRACION — PORTIX OS v6.0 'CHRONOS'{RESET}")
    print("  " + "─" * 68)
    for titulo, ok, detalle in resultados:
        color = GREEN if ok else RED
        print(f"  {color}{'[PASS]' if ok else '[FALLO]'}{RESET} {titulo:52} {detalle}")
    print("  " + "─" * 68)

    todo_ok = bool(resultados) and all(ok for _, ok, _ in resultados)
    if todo_ok:
        print(f"  {GREEN}{BOLD}[PORTIX-CHRONOS-MASTER-PASS] Sistema reconstruido y certificado al 100%{RESET}")
    else:
        print(f"  {RED}{BOLD}[PORTIX-CHRONOS-FAIL] Alguna suite no alcanzo el 100%{RESET}")
    print()
    return 0 if todo_ok else 1


if __name__ == "__main__":
    sys.exit(main())
