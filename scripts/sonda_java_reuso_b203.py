#!/usr/bin/env python3
"""Reproduce Java en una copia privada de un disco ya instalado; no certifica b203."""
import argparse
import os
import time
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
    from .rutas_imagenes import dentro_de_dist
    from .verificaciones_b203 import paginas_desde_fichero, verificar_delta, verificar_irq_java, verificar_mapeos_java
    from .verificaciones_java_b203 import resultado_java
else:
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import dentro_de_dist
    from verificaciones_b203 import paginas_desde_fichero, verificar_delta, verificar_irq_java, verificar_mapeos_java
    from verificaciones_java_b203 import resultado_java


def ordenes_java(clase_primero=False):
    java = "run /usr/lib/jvm/java-21-openjdk-amd64/bin/java"
    version = (java + " -version", "version", 120)
    clase = (java + " -cp /usr/local T010Java", "clase", 180)
    return (clase, version) if clase_primero else (version, clase)


def resumen_progreso_java(texto):
    """Actividad observada, no una prueba de éxito ni de ausencia de bloqueo."""
    texto = texto.replace("\r", "")
    lineas = texto.splitlines()
    relevantes = [l for l in lineas if l.startswith(("[RAMFS]", "[PROCFS]", "[SYS]", "[T-005]", "SCHED", "[R3]", "PROC: exit"))]
    return (f"lineas={len(lineas)} hilos={sum(l.startswith('[T-005] hilo tid=') for l in lineas)} "
            f"mmap={sum(l.startswith('[SYS] MMAP') for l in lineas)} "
            f"cambios_sched={sum(l.startswith('SCHED') for l in lineas)} "
            f"ultima={relevantes[-1][:140] if relevantes else 'sin actividad registrada'}")


def cpu_qemu_segundos(pid):
    """CPU acumulada por el proceso QEMU en el anfitrión, si /proc existe."""
    try:
        resto = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return (int(resto[11]) + int(resto[12])) / os.sysconf("SC_CLK_TCK")
    except (OSError, ValueError, IndexError):
        return None


def ejecutar_java(r, comando, modo, limite, progreso=False):
    inicio = r.get_log_len()
    t0 = time.monotonic()
    cpu_inicio = cpu_qemu_segundos(r.proc.pid) if progreso and r.proc is not None else None
    siguiente = 30
    volcado_intermedio = False
    r.send_command(comando)
    detalle = "sin entrada ni salida concluyente"
    texto = ""
    ok = False
    while time.monotonic() - t0 < limite:
        with r.log_lock:
            texto = r.log[inicio:]
        ok, detalle = resultado_java(texto, modo)
        if ok or "terminó con código" in detalle:
            break
        if r.proc is None or r.proc.poll() is not None:
            detalle = "QEMU terminó antes de completar la orden"
            break
        transcurrido = time.monotonic() - t0
        if progreso and transcurrido >= siguiente:
            cpu = cpu_qemu_segundos(r.proc.pid)
            cpu_txt = f"{cpu - cpu_inicio:.1f}" if cpu is not None and cpu_inicio is not None else "no disponible"
            print(f"[T-033 PROGRESO] {modo} t={transcurrido:.1f}s cpu_qemu={cpu_txt}s {resumen_progreso_java(texto)}", flush=True)
            siguiente += 30
            if transcurrido >= 60 and not volcado_intermedio:
                try:
                    r._stdin_write(b"\x1d")
                    r.proc.stdin.flush()
                    print("[T-033 INFO] Se solicitó un único volcado SMP intermedio", flush=True)
                except Exception as exc:
                    print(f"[T-033 INFO] No se pudo solicitar volcado intermedio: {exc}", flush=True)
                volcado_intermedio = True
        time.sleep(0.1)
    tiempo = time.monotonic() - t0
    print(f"[{'PASS' if ok else 'FAIL'}] Java {modo}: {detalle}; {tiempo:.1f} s, límite {limite} s", flush=True)
    if ok:
        # No confundir el prompt que precede al eco con el posterior a la salida.
        pos = texto.rfind("PROC: exit PID=")
        listo, _ = r.wait_for_pattern("root@portix:", start_idx=inicio + max(pos, 0), timeout=10)
        if not listo:
            print("[FAIL] El prompt no volvió después de la salida de Java", flush=True)
            return False, False, inicio
        return True, True, inicio
    # Diagnóstico sin convertir el plazo agotado en éxito; esperar como máximo
    # 20 s adicionales para ver si el proceso terminó tarde y poder continuar.
    try:
        r._stdin_write(b"\x1d")
        r.proc.stdin.flush()
    except Exception as exc:
        print(f"[INFO] No se pudo solicitar volcado SMP: {exc}", flush=True)
    t_diag = time.monotonic()
    while time.monotonic() - t_diag < 20:
        with r.log_lock:
            texto = r.log[inicio:]
        tardio, estado = resultado_java(texto, modo)
        if tardio:
            print(f"[INFO] Java {modo} terminó tarde ({estado}); el FAIL por plazo se conserva", flush=True)
            pos = texto.rfind("PROC: exit PID=")
            listo, _ = r.wait_for_pattern("root@portix:", start_idx=inicio + max(pos, 0), timeout=5)
            return False, listo, inicio
        if "terminó con código" in estado:
            pos = texto.rfind("PROC: exit PID=")
            listo, _ = r.wait_for_pattern("root@portix:", start_idx=inicio + max(pos, 0), timeout=5)
            return False, listo, inicio
        if r.proc is None or r.proc.poll() is not None:
            break
        time.sleep(0.1)
    print(f"[INFO] Java {modo} no terminó en la ventana diagnóstica; no se envía otra orden", flush=True)
    return False, False, inicio


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--img", required=True, type=Path, help="imagen detenida y verificada bajo build/dist")
    ap.add_argument("--clase-primero", action="store_true", help="probar antes la clase con hilos, sin alterar los límites")
    args = ap.parse_args()
    imagen = args.img.resolve()
    if not dentro_de_dist(imagen) or not imagen.is_file():
        print("[FAIL] Falta imagen bajo build/dist")
        return 1
    print("[INFO] Reutilización diagnóstica: no instala paquetes y no es suite ni certificación.", flush=True)
    r = None
    fallos = 0
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=True, silicon=True, conservar=False)
        r.start_qemu()
        init, _ = r.wait_for_pattern("init process PID=1 registered", timeout=40)
        if not init:
            print("[FAIL] La imagen no arrancó hasta init", flush=True)
            return 1
        # La consola no presenta prompt hasta recibir una orden. El eco se
        # excluye: se exige el texto de salida real y se espera el prompt final.
        consola = r.execute_and_verify("echo JAVA-REUSO-LISTO", "JAVA-REUSO-LISTO", "Consola lista", timeout=10)
        if not consola:
            print("[FAIL] La consola no respondió a la orden de disponibilidad", flush=True)
            return 1
        idx = r.get_log_len()
        lectura = r.execute_and_verify("cat /proc/smp", "mmap_fichero_perezoso:", "Contador antes de Java", timeout=8)
        with r.log_lock:
            antes = paginas_desde_fichero(r.log[idx:])
        if not lectura or antes is None:
            print("[FAIL] Contador inicial ausente o ambiguo", flush=True)
            return 1
        print(f"[INFO] Páginas antes de Java: {antes}", flush=True)
        inicio_java = r.get_log_len()
        ordenes = ordenes_java(args.clase_primero)
        print("[INFO] Orden: " + ", ".join(modo for _, modo, _ in ordenes), flush=True)
        for comando, modo, limite in ordenes:
            ok, disponible, _ = ejecutar_java(r, comando, modo, limite)
            fallos += not ok
            if not disponible:
                print("[INCOMPLETA] Orden restante y contador final omitidos: el proceso anterior no terminó", flush=True)
                return 1
        idx = r.get_log_len()
        lectura = r.execute_and_verify("cat /proc/smp", "respiros: abiertos", "Contador después de Java", timeout=8)
        with r.log_lock:
            tramo_final = r.log[idx:]
            tramo_java = r.log[inicio_java:idx]
        despues = paginas_desde_fichero(tramo_final)
        ok_delta, delta = verificar_delta(antes, despues)
        ok_mapa, perezoso, ansioso = verificar_mapeos_java(tramo_java)
        print(f"[{'PASS' if lectura and ok_delta else 'FAIL'}] Delta de páginas: {antes} -> {despues}, delta={delta}; exigido 1000 <= delta < 34389", flush=True)
        print(f"[{'PASS' if ok_mapa else 'FAIL'}] Mapeo JVM: perezoso máximo={perezoso}, ansioso máximo={ansioso}", flush=True)
        fallos += not (lectura and ok_delta)
        fallos += not ok_mapa
        ok_irq, perdidos, hueco = verificar_irq_java(tramo_final)
        print(f"[{'PASS' if ok_irq else 'FAIL'}] T-026: hueco={hueco} us (<300000), perdidos={perdidos} (<600)", flush=True)
        fallos += not ok_irq
    finally:
        if r is not None:
            r.close()
            if any(v is False for v in r.results.values()):
                fallos += 1
    print(f"SONDA JAVA REUSO: fallos={fallos}; diagnóstico parcial, NO certifica.", flush=True)
    return 1 if fallos else 0


if __name__ == "__main__":
    raise SystemExit(main())
