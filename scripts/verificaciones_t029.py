#!/usr/bin/env python3
"""Predicados estrictos para las sustituciones T-029 (binario Alpine real)."""

import re
import time


def paquete_busybox_verificado(texto):
    """Exige, en orden, descarga HTTPS, SHA-256 completo e instalación."""
    descarga = re.search(r"\[\+\] Descargando https://10\.0\.2\.2:8443/busybox-static\.pkg -> [^\r\n]+", texto)
    if not descarga:
        return False, "falta descarga HTTPS del paquete Alpine"
    completada = re.search(r"\[\+\] Descarga completada: [1-9][0-9]* bytes", texto[descarga.end():])
    if not completada:
        return False, "falta confirmación de descarga no vacía"
    tras_descarga = descarga.end() + completada.end()
    verificacion = re.search(r"\[\+\] SHA-256 verificado: [0-9a-fA-F]{64}\b", texto[tras_descarga:])
    if not verificacion:
        return False, "falta SHA-256 completo posterior a la descarga"
    desde = tras_descarga + verificacion.end()
    instalacion = "[OK] Paquete 'busybox-static' (1.36.1-r31 (Alpine v3.20)) instalado exitosamente."
    if instalacion not in texto[desde:]:
        return False, "falta confirmación posterior de instalación"
    return True, "descarga, SHA-256 e instalación confirmadas en orden"


def salida_busybox(texto, linea_esperada):
    """Exige línea exacta entre la entrada Ring 3 y salida 0 del MISMO PID."""
    if not linea_esperada or "\n" in linea_esperada or "\r" in linea_esperada:
        return False, "línea esperada inválida"
    entrada = re.compile(r"^\[R3\] enter PID=(\d+) name='/bin/busybox'(?:\s|$)", re.MULTILINE)
    for m in entrada.finditer(texto):
        pid = m.group(1)
        salida = re.search(r"^PROC: exit PID=" + re.escape(pid) + r" name='/bin/busybox' code=(-?\d+)(?:\s|$)",
                           texto[m.end():], re.MULTILINE)
        if salida is None:
            continue
        cuerpo = texto[m.end():m.end() + salida.start()]
        if salida.group(1) != "0":
            return False, f"busybox PID={pid} salió con código {salida.group(1)}"
        if linea_esperada not in cuerpo.splitlines():
            return False, f"busybox PID={pid} no emitió la línea exacta esperada"
        return True, f"busybox PID={pid}, línea exacta y salida 0"
    return False, "falta entrada y salida del mismo PID de /bin/busybox"


def registrar_paquete_busybox(runner, inicio, etiqueta):
    """Registra el predicado de instalación sin inventar PASS en --solo."""
    if hasattr(runner, "_seccion_activa") and not runner._seccion_activa():
        runner.omitidas += 1
        return True
    with runner.log_lock:
        texto = runner.log[inicio:]
    ok, detalle = paquete_busybox_verificado(texto)
    runner.results[etiqueta] = ok
    print(f"  {'✓' if ok else '✗'} {etiqueta} [{'PASS' if ok else 'FAIL'}] ({detalle})", flush=True)
    return ok


def ejecutar_busybox(runner, enviar, comando, linea_esperada, etiqueta, timeout=15.0):
    """Envía una orden y exige línea exacta y salida 0 del PID del ELF externo."""
    if hasattr(runner, "_seccion_activa") and not runner._seccion_activa():
        runner.omitidas += 1
        return True
    with runner.log_lock:
        inicio = len(runner.log)
    if hasattr(runner, "_tramos"):
        runner._tramos.append((inicio, etiqueta))
    enviar(comando)
    limite = time.monotonic() + timeout
    ok = False
    detalle = "sin salida del proceso"
    texto = ""
    while time.monotonic() < limite:
        with runner.log_lock:
            texto = runner.log[inicio:]
        ok, detalle = salida_busybox(texto, linea_esperada)
        if ok:
            break
        if runner.proc is not None and runner.proc.poll() is not None:
            break
        time.sleep(0.05)
    runner.results[etiqueta] = ok
    print(f"  {'✓' if ok else '✗'} {etiqueta} [{'PASS' if ok else 'FAIL'}] ({detalle})", flush=True)
    if ok:
        # El prompt de la orden se emite antes de la entrada Ring 3; se busca
        # únicamente después de la salida observada, no en el eco previo.
        fin = texto.rfind("PROC: exit PID=")
        runner.wait_for_pattern("root@portix:", start_idx=inicio + max(fin, 0), timeout=5.0)
    return ok
