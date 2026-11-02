#!/usr/bin/env python3
"""Paridad BIOS/UEFI de las correcciones del ciclo T-042 (misma batería en ambos).

La misma batería, sobre la misma imagen y con el mismo fixture, en los dos
firmwares (`--firmware bios` sin firmware, `--firmware uefi` con OVMF). Si un
firmware no arranca, el fixture no se inyecta o QEMU no existe, la sonda FALLA:
no se finge una prueba UEFI.

Batería por firmware (cada punto puede fallar por separado):
  A. La consola responde antes de empezar.
  B. `futex-lider`: un líder lanzado por `run` dormido en futex cuyo último hilo
     muere en el otro CPU se cierra (`PROC: exit … code=7`) y el prompt vuelve.
  C. La consola sigue atendiendo después: el defecto de T-042 era quedarse muda.
  D. `ping6 2001:db8::9` contesta con su motivo (`sin ruta al destino`) sin
     inventar RTT y sin agotar el plazo (arreglo de IPv6).
  E. `/proc/smp` publica `ventanas_if0:` (instrumentación de T-026 viva en los
     dos firmwares).
  F. `echo PARIDAD_OK` responde: control final de consola viva.

Uso:
    python3 scripts/sonda_paridad_t042.py --firmware bios
    python3 scripts/sonda_paridad_t042.py --firmware uefi
    python3 scripts/sonda_paridad_t042.py --ambos      # BIOS y después UEFI
"""
import argparse
import hashlib
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from rutas_imagenes import copia_privada, dentro_de_dist  # noqa: E402
from sonda_ata_b203 import sha256  # noqa: E402
from sonda_t035_uefi import (  # noqa: E402
    DEBUGFS, Vm, ext2_debugfs, ext2_gpt, ovmf,
)

BINARIO = ROOT / "build/packages/t034_exit_group"
NOMBRE = "/usr/local/t034"
UEFI = ROOT / "build/dist/portix-uefi.img"
BIOS = ROOT / "build/dist/portix.img"
CODIGO_GRUPO = "7"          # exit_group(7) del modo `futex-lider`
LIMITE_GRUPO = 40           # s: el caso bueno tarda ~3 s
LIMITE_IPV6 = 20            # s: la corrección contesta en <1 s


def ext2_bios(imagen):
    """Offset (bytes) de la partición ext2 de una imagen MBR."""
    from sonda_imagen_t029 import localizar_ext2
    offset, _tam = localizar_ext2(imagen.open("rb").read(512), imagen.stat().st_size)
    return offset


class Bateria:
    def __init__(self, etq):
        self.etq = etq
        self.resultados = []

    def punto(self, nombre, ok, detalle):
        self.resultados.append((nombre, ok, detalle))
        print(f"[{'PASS' if ok else 'FAIL'}] T-042 {self.etq} · {nombre}: {detalle}", flush=True)
        return ok

    def todo_ok(self):
        return all(ok for _, ok, _ in self.resultados)


def esperar_prompt(vm, desde, segundos):
    """Espera el prompt contando sólo lo escrito DESPUÉS de `desde`."""
    fin = time.monotonic() + segundos
    while time.monotonic() < fin:
        vm.leer(0.4)
        if b"root@portix:" in vm.log[desde:]:
            return True
    return b"root@portix:" in vm.log[desde:]


def texto_desde(vm, desde):
    return vm.log[desde:].decode("utf-8", "replace")


def probar_firmware(firmware):
    es_uefi = firmware == "uefi"
    etq = "UEFI" if es_uefi else "BIOS"
    fw = ovmf() if es_uefi else None
    imagen = UEFI if es_uefi else BIOS
    bat = Bateria(etq)
    if (es_uefi and fw is None) or not imagen.is_file() or DEBUGFS is None or not BINARIO.is_file():
        print(f"[FAIL] {etq}: firmware={fw} imagen={imagen.is_file()} "
              f"debugfs={DEBUGFS} fixture={BINARIO.is_file()}", flush=True)
        return False, bat
    print(f"[INFO] {etq}={imagen.name} SHA-256={sha256(imagen)}; OVMF={fw}; "
          f"fixture={sha256(BINARIO)}", flush=True)

    copia = copia_privada(imagen)
    if not dentro_de_dist(copia):
        print("[FAIL] la copia privada quedó fuera de build/dist", flush=True)
        return False, bat
    vm = None
    try:
        offset = ext2_gpt(copia)[0] if es_uefi else ext2_bios(copia)
        ext2_debugfs(copia, offset, f"write {BINARIO} {NOMBRE}")
        estado = subprocess.run([DEBUGFS, "-R", f"stat {NOMBRE}", f"{copia}?offset={offset}"],
                                capture_output=True, text=True, timeout=30,
                                env=dict(os.environ, LC_ALL="C")).stdout
        inyectado = re.search(r"^Inode:\s*[1-9]\d*\s+Type:\s*regular", estado, re.MULTILINE) is not None
        if not bat.punto("fixture en el ext2", inyectado,
                         f"{'regular en ' + ('GPT' if es_uefi else 'MBR') if inyectado else 'no quedó como fichero regular'}"):
            return False, bat

        vm = Vm(copia, fw)
        if not vm.esperar(r"\[HH\]", 90) or not vm.esperar("present done", 150):
            bat.punto("arranque", False, "no llegó al escritorio")
            return False, bat
        # Bajo UEFI el prompt sólo se reimprime al recibir entrada.
        vm.enviar("")
        inicio = len(vm.log)
        if not esperar_prompt(vm, inicio, 60):
            bat.punto("consola lista", False, "el prompt no apareció")
            return False, bat
        bat.punto("consola lista", True, "prompt tras una línea vacía")

        # ── B y C: el líder dormido se cierra y la consola sobrevive.
        # En Portix el prompt se imprime al ECO de la línea que se teclea, no al
        # terminar una orden; por eso «volvió el prompt» no sirve como señal y se
        # comprueba lo que de verdad importa: que el proceso líder salga con el
        # código del grupo y que la consola atienda la orden siguiente.
        inicio = len(vm.log)
        t0 = time.monotonic()
        vm.enviar(f"run {NOMBRE} futex-lider")
        cerrado = False
        while time.monotonic() - t0 < LIMITE_GRUPO:
            vm.leer(0.4)
            t = texto_desde(vm, inicio)
            if re.search(r"\[OK\] Proceso PID \d+ finalizado", t):
                cerrado = True
                break
        t = texto_desde(vm, inicio)
        dur = time.monotonic() - t0
        m_pid = re.search(r"\[RUN\] Ejecutando PID (\d+)", t)
        pid = m_pid.group(1) if m_pid else None
        m = re.search((r"PROC: exit PID=" + (pid if pid else r"\d+") +
                       r" name='[^']*' code=(-?\d+)"), t)
        codigo_ok = m is not None and m.group(1) == CODIGO_GRUPO
        bat.punto("futex-lider cierra el grupo", cerrado and codigo_ok,
                  f"{'cierra' if cerrado else 'NO cierra'} en {dur:.1f} s, líder pid={pid}, "
                  f"code={m.group(1) if m else '?'} (esperado {CODIGO_GRUPO})")
        if not (cerrado and codigo_ok):
            print(t[-1500:], flush=True)
            return False, bat

        # C: la consola sigue viva (el defecto era quedarse muda para siempre).
        inicio = len(vm.log)
        vm.enviar("echo CONSOLA_VIVA")
        vivo = False
        fin_c = time.monotonic() + 15
        while time.monotonic() < fin_c:
            vm.leer(0.4)
            if texto_desde(vm, inicio).count("CONSOLA_VIVA") >= 2:   # eco + salida
                vivo = True
                break
        bat.punto("consola viva tras el cierre", vivo,
                  "atiende la orden siguiente" if vivo else "la consola quedó muda (defecto T-042)")

        # ── D: IPv6 honesto, sin RTT inventado y sin agotar el plazo.
        inicio = len(vm.log)
        t0 = time.monotonic()
        vm.enviar("ping6 2001:db8::9")
        encontrado = None
        while time.monotonic() - t0 < LIMITE_IPV6:
            vm.leer(0.4)
            t = texto_desde(vm, inicio)
            m = re.search(r"sin ruta al destino|destino inalcanzable|puerto inalcanzable", t)
            if m:
                encontrado = m.group(0)
                break
        dur = time.monotonic() - t0
        # La consola tiene que seguir atendiendo justo después (si `ping6` colgara,
        # la orden siguiente no llegaría nunca).
        vm.enviar("echo IPV6_OK")
        atiende = False
        fin_d = time.monotonic() + 15
        while time.monotonic() < fin_d:
            vm.leer(0.4)
            if texto_desde(vm, inicio).count("IPV6_OK") >= 2:
                atiende = True
                break
        t = texto_desde(vm, inicio)
        rtt_inventado = re.search(r"rtt=\d+", t) is not None
        bat.punto("ping6 a destino inexistente",
                  encontrado is not None and not rtt_inventado and atiende and dur < LIMITE_IPV6,
                  f"motivo={'sí' if encontrado else 'NO'} ({encontrado}), rtt_inventado={rtt_inventado}, "
                  f"consola_atendio={atiende}, {dur:.1f} s")

        # ── E: instrumentación de T-026 publicada.
        inicio = len(vm.log)
        vm.enviar("cat /proc/smp")
        ok = vm.esperar("ventanas_if0:", 20)
        vent = re.search(r"ventanas_if0: abiertas (\d+) peor_us (\d+) pid (\d+) abi (\w+) nr (\d+)",
                         texto_desde(vm, inicio))
        hueco = re.search(r"irq0_peor_hueco_us: (\d+)", texto_desde(vm, inicio))
        dentro = re.search(r"irq0_hueco_dentro: (\S+)", texto_desde(vm, inicio))
        if vent:
            print(f"[INFO] {etq}: ventanas_if0 abiertas={vent.group(1)} peor_us={vent.group(2)} "
                  f"pid={vent.group(3)} abi={vent.group(4)} nr={vent.group(5)}; "
                  f"irq0_peor_hueco_us={hueco.group(1) if hueco else '?'} "
                  f"dentro={dentro.group(1) if dentro else '?'}", flush=True)
        bat.punto("/proc/smp publica ventanas con IF=0", ok and vent is not None,
                  "línea completa" if vent else "ausente o incompleta")

        # ── F: control final.
        inicio = len(vm.log)
        vm.enviar("echo PARIDAD_OK")
        final = False
        fin_f = time.monotonic() + 15
        while time.monotonic() < fin_f:
            vm.leer(0.4)
            if texto_desde(vm, inicio).count("PARIDAD_OK") >= 2:
                final = True
                break
        bat.punto("control final de consola", final, "responde" if final else "no responde")

        # Integridad del fixture tras toda la batería.
        r = subprocess.run([DEBUGFS, "-R", f"cat {NOMBRE}", f"{copia}?offset={offset}"],
                           capture_output=True, timeout=60, env=dict(os.environ, LC_ALL="C"))
        igual = r.returncode == 0 and hashlib.sha256(r.stdout).hexdigest() == sha256(BINARIO)
        bat.punto("fixture intacto en el ext2", igual,
                  f"SHA-256={hashlib.sha256(r.stdout).hexdigest()[:16]}…")
        return bat.todo_ok(), bat
    finally:
        if vm is not None:
            vm.detener()
        copia.unlink(missing_ok=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--firmware", choices=("uefi", "bios"), default=None)
    ap.add_argument("--ambos", action="store_true", help="BIOS y después UEFI en la misma corrida")
    args = ap.parse_args()
    if not args.ambos and args.firmware is None:
        print("[FAIL] indique --firmware bios|uefi o --ambos", flush=True)
        return 2
    firmwares = ("bios", "uefi") if args.ambos else (args.firmware,)
    resumen = []
    ok_total = True
    for fw in firmwares:
        ok, bat = probar_firmware(fw)
        ok_total = ok_total and ok
        aprobados = sum(1 for _, o, _ in bat.resultados if o)
        resumen.append(f"{'UEFI' if fw == 'uefi' else 'BIOS'} {aprobados}/{len(bat.resultados)}")
    print(f"SONDA PARIDAD T-042: {' · '.join(resumen)}; no certifica b203", flush=True)
    if len(firmwares) == 2 and ok_total:
        print("[PASS] paridad BIOS/UEFI: los dos firmwares pasan la misma batería", flush=True)
    return 0 if ok_total else 1


if __name__ == "__main__":
    raise SystemExit(main())
