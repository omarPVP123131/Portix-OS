#!/usr/bin/env python3
"""T-033: Java -version en un arranque limpio; una VM y plazo original de 120 s."""
import argparse
import re
import time
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
    from .rutas_imagenes import dentro_de_dist
    from .sonda_ata_b203 import clasificar_arranque, sha256
    from .sonda_java_reuso_b203 import ejecutar_java, ordenes_java
    from .verificaciones_b203 import paginas_desde_fichero, verificar_delta, verificar_irq_java, verificar_mapeos_java
else:
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import dentro_de_dist
    from sonda_ata_b203 import clasificar_arranque, sha256
    from sonda_java_reuso_b203 import ejecutar_java, ordenes_java
    from verificaciones_b203 import paginas_desde_fichero, verificar_delta, verificar_irq_java, verificar_mapeos_java


def ejecutar_en_vm(r):
    inicio = time.monotonic()
    while time.monotonic() - inicio < 30:
        with r.log_lock:
            serie = r.log
        ok, terminado, detalle = clasificar_arranque(serie)
        if terminado:
            break
        if r.proc is None or r.proc.poll() is not None:
            detalle = "QEMU terminó antes de init"
            break
        time.sleep(0.05)
    else:
        ok, detalle = False, "arranque excedió 30 s"
    print(f"[{'PASS' if ok else 'FAIL'}] T-033 arranque: {detalle}", flush=True)
    if not ok:
        print("[INCOMPLETA] Java no se ejecutó; no se reintenta el arranque", flush=True)
        return 1

    if not r.execute_and_verify("echo JAVA-VERSION-LISTA", "JAVA-VERSION-LISTA", "Consola lista", timeout=10):
        print("[INCOMPLETA] Consola no respondió; Java no se ejecutó", flush=True)
        return 1
    idx = r.get_log_len()
    lectura = r.execute_and_verify("cat /proc/smp", "mmap_fichero_perezoso:", "Contador antes de Java", timeout=8)
    with r.log_lock:
        antes = paginas_desde_fichero(r.log[idx:])
    if not lectura or antes is None:
        print("[FAIL] Contador inicial ausente o ambiguo; Java no se ejecutó", flush=True)
        return 1
    print(f"[T-033] Páginas iniciales: {antes}; se ejecuta sólo versión, sin clase previa", flush=True)
    comienzo_java = r.get_log_len()
    comando, modo, limite = ordenes_java()[0]
    ok, disponible, _ = ejecutar_java(r, comando, modo, limite, progreso=True)
    fallos = int(not ok)
    if not disponible:
        print("[INCOMPLETA] Contador final omitido: Java no terminó en la ventana diagnóstica", flush=True)
        return max(fallos, 1)

    idx = r.get_log_len()
    lectura = r.execute_and_verify("cat /proc/smp", "respiros: abiertos", "Contador después de Java", timeout=8)
    with r.log_lock:
        final = r.log[idx:]
        tramo_java = r.log[comienzo_java:idx]
    despues = paginas_desde_fichero(final)
    ok_delta, delta = verificar_delta(antes, despues)
    ok_mapa, perezoso, ansioso = verificar_mapeos_java(tramo_java)
    print(f"[{'PASS' if lectura and ok_delta else 'FAIL'}] Páginas: {antes} -> {despues}, delta={delta}; exigido 1000 <= delta < 34389", flush=True)
    print(f"[{'PASS' if ok_mapa else 'FAIL'}] Mapeo JVM: perezoso máximo={perezoso}, ansioso máximo={ansioso}", flush=True)
    fallos += int(not (lectura and ok_delta)) + int(not ok_mapa)
    ok_irq, perdidos, hueco = verificar_irq_java(final)
    print(f"[{'PASS' if ok_irq else 'FAIL'}] T-026: hueco={hueco} us (<300000), perdidos={perdidos} (<600)", flush=True)
    fallos += int(not ok_irq)
    return fallos


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--img", type=Path, required=True)
    ap.add_argument("--sha256", required=True)
    args = ap.parse_args()
    imagen = args.img.resolve()
    if not dentro_de_dist(imagen) or not imagen.is_file() or not re.fullmatch(r"[0-9a-f]{64}", args.sha256):
        print("[FAIL] Imagen o SHA-256 inválidos; imagen debe estar en build/dist", flush=True)
        return 1
    antes = sha256(imagen)
    if antes != args.sha256:
        print(f"[FAIL] SHA-256 de imagen distinto: {antes}", flush=True)
        return 1
    print(f"[T-033] Imagen verificada {antes}; sólo java -version, plazo 120 s; no certifica", flush=True)
    r = None
    fallos = 1
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=True, silicon=True, conservar=False)
        r.start_qemu()
        fallos = ejecutar_en_vm(r)
    finally:
        if r is not None:
            r.close()
            if any(v is False for v in r.results.values()):
                fallos += 1
    despues = sha256(imagen)
    if antes != despues:
        print(f"[FAIL] Imagen original modificada: {antes} -> {despues}", flush=True)
        fallos += 1
    else:
        print(f"[PASS] Imagen original intacta: {despues}", flush=True)
    print(f"T-033 VERSIÓN AISLADA: fallos={fallos}; clase omitida por diseño; no certifica b203.", flush=True)
    return 1 if fallos else 0


if __name__ == "__main__":
    raise SystemExit(main())
