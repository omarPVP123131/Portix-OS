#!/usr/bin/env python3
"""Sonda BIOS breve en QEMU, con y sin dispositivos; no certifica la suite."""
import sys
import time
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
else:
    from headless_runner import PortixHeadlessRunner

ROOT = Path(__file__).resolve().parents[1]
IMG = ROOT / "build/dist/portix.img"
EVIDENCIA = ROOT / "docs/evidencias/b203"


def pasada(con_silicio):
    modo = "con-silicio" if con_silicio else "sin-silicio"
    r = None
    texto = ""
    fallo = None
    t0 = time.monotonic()
    try:
        r = PortixHeadlessRunner(str(IMG), silicon=con_silicio, conservar=False)
        r.start_qemu()
        ok, _ = r.wait_for_pattern("init process PID=1 registered", timeout=75)
        with r.log_lock:
            texto = r.log
        if not ok:
            fallo = "no apareció init antes del límite de 75 s"
    except Exception as e:
        fallo = str(e)
    finally:
        if r is not None:
            r.close()
            if any(v is False for v in r.results.values()):
                fallo = fallo or "el ejecutor informó un fallo al cerrar QEMU o el disco"
    (EVIDENCIA / f"serie-bios114-{modo}.log").write_text(texto, encoding="utf8")
    checks = [
        ("kernel mitad superior", "[HH] nucleo en mitad superior: rip=0xFFFFFFFF80" in texto),
        ("segundo núcleo atiende IPI", "[SMP] AP 1 (APIC ID 1): en linea, ve su APIC ID 1, IPI 0xF0 contador 0 -> 1 OK" in texto),
        ("escritorio", "present done" in texto),
        ("init PID 1", "init process PID=1 registered" in texto),
    ]
    if con_silicio:
        for patron, etiqueta in (
            ("[SILICIO-NVME-LBA0]", "NVMe: lectura DMA"),
            ("[SILICIO-NVME-ROUNDTRIP]", "NVMe: escritura y relectura"),
            ("[SILICIO-SATA-ROUNDTRIP]", "SATA: escritura y relectura"),
            ("[SILICIO-USB-DESCRIPTOR]", "USB: descriptor real"),
            ("[SILICIO-USB-ROUNDTRIP]", "USB: escritura y relectura"),
            ("[SILICIO-HDA] controlador=SI codecs=1 identidad_por_verbos=SI", "HDA: identidad del códec"),
            ("[SILICIO-PASS]", "veredicto de silicio presente"),
        ):
            checks.append((etiqueta, patron in texto))
        checks.append(("sin veredicto de ausencia", "[SILICIO-AUSENTE]" not in texto))
    else:
        checks.extend((("declara hardware ausente", "[SILICIO-AUSENTE]" in texto),
                       ("no declara PASS sin hardware", "[SILICIO-PASS]" not in texto)))
    if fallo:
        print(f"[FAIL] {modo}: {fallo}")
        checks.append(("arranque completo", False))
    malos = 0
    for etiqueta, ok in checks:
        print(f"[{'PASS' if ok else 'FAIL'}] BIOS {modo}: {etiqueta}", flush=True)
        malos += not ok
    print(f"BIOS {modo}: {len(checks)-malos}/{len(checks)} PASS, {time.monotonic()-t0:.1f} s")
    return malos


def main():
    if not IMG.is_file():
        print(f"[FAIL] Falta imagen {IMG}")
        return 1
    EVIDENCIA.mkdir(parents=True, exist_ok=True)
    fallos = pasada(True) + pasada(False)
    print(f"SONDA BIOS: fallos={fallos} (no sustituye las suites completas)")
    return 1 if fallos else 0


if __name__ == "__main__":
    sys.exit(main())
