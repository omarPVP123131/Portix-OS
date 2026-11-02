#!/usr/bin/env python3
"""Predicados de la reproducción Java; ningún eco ni PID ajeno es suficiente."""
import re

NOMBRE = "/usr/lib/jvm/java-21-openjdk-am"
MARCADOR = ("[T010-PASS] OpenJDK 21 de Debian sin modificar: hilos, pool, "
            "BlockingQueue, ficheros ext2, sleep, NPE con JIT, GC, excepciones")


def resultado_java(serie, modo):
    if modo not in ("version", "clase"):
        raise ValueError("modo de Java inválido")
    texto = serie.replace("\r", "")
    entradas = list(re.finditer(r"(?m)^\[R3\] enter PID=(\d+) name='" + re.escape(NOMBRE) + r"'", texto))
    if len(entradas) != 1:
        return False, "falta entrada Ring 3 inequívoca"
    entrada = entradas[0]
    pid = entrada.group(1)
    fin = re.search(r"(?m)^PROC: exit PID=" + re.escape(pid) + r" name='" + re.escape(NOMBRE) + r"' code=(\d+)\b", texto[entrada.end():])
    if not fin:
        return False, f"falta salida de PID {pid}"
    if fin.group(1) != "0":
        return False, f"PID {pid} terminó con código {fin.group(1)}"
    ventana = texto[entrada.end():entrada.end() + fin.start()]
    if modo == "version":
        a = re.search(r'^openjdk version "21\.[^"\n]+"[^\n]*$', ventana, re.MULTILINE)
        b = re.search(r'^OpenJDK 64-Bit Server VM \(build 21\.[^\n]+$', ventana, re.MULTILINE)
        if not a or not b or a.start() >= b.start():
            return False, "faltan las líneas reales de versión y JVM en orden"
    else:
        if re.search(r"(?m)^\[T010-FAIL\]", ventana):
            return False, "la propia sonda informó fallo"
        posiciones = []
        for i in range(1, 9):
            halladas = list(re.finditer(rf"(?m)^T010-{i}: ok [^\n]+$", ventana))
            if len(halladas) != 1:
                return False, f"falta o está duplicado T010-{i}: ok"
            posiciones.append(halladas[0].start())
        marca = re.search(r"(?m)^" + re.escape(MARCADOR) + r"$", ventana)
        if posiciones != sorted(posiciones) or not marca or marca.start() <= posiciones[-1]:
            return False, "falta o está desordenado el veredicto final de la clase"
    return True, f"PID {pid}: salida comprobada y código 0"
