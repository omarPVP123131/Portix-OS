#!/usr/bin/env python3
"""Predicado estricto de la sonda TLB; no inventa éxito ni contadores."""
import re

NOMBRE = "/usr/local/t033tlb"


def resultado_tlb(texto, modo):
    if modo not in ("solo", "hilos"):
        raise ValueError("modo inválido")
    texto = texto.replace("\r", "")
    entradas = list(re.finditer(r"(?m)^\[R3\] enter PID=(\d+) name='" + re.escape(NOMBRE) + r"'(?:\s|$)", texto))
    if len(entradas) != 1:
        return False, False, "sin entrada Ring 3 inequívoca", None
    entrada = entradas[0]
    pid = entrada.group(1)
    fin = re.search(r"(?m)^PROC: exit PID=" + re.escape(pid) + r" name='" + re.escape(NOMBRE) + r"' code=(-?\d+)(?:\s|$)", texto[entrada.end():])
    if not fin:
        return False, False, f"PID {pid} no terminó", None
    cuerpo = texto[entrada.end():entrada.end() + fin.start()]
    if int(fin.group(1)) != 0:
        return False, True, f"PID {pid} salió con código {fin.group(1)}", None
    if "[T033-ERROR]" in cuerpo:
        return False, True, "la sonda informó un error", None
    ap = "1" if modo == "hilos" else "sin-hilo"
    literales = (
        f"[T033-TLB] listo modo={modo} cpu_lider=0 cpu_ap={ap} paginas=4096",
        f"[T033-TLB] modo={modo} comienza=mmap paginas=4096",
        f"[T033-TLB] modo={modo} comienza=munmap paginas=4096",
        f"[T033-TLB] modo={modo} proteccion=3-antes-3-despues errno=12 reutilizacion=OK",
        f"[T033-TLB] PASS modo={modo}",
    )
    for literal in literales:
        if cuerpo.splitlines().count(literal) != 1:
            return False, True, f"falta o se duplicó: {literal}", None
    cifras = []
    for fase in ("mmap", "munmap"):
        patron = (rf"(?m)^\[T033-TLB\] modo={modo} fase={fase} paginas=4096 "
                  rf"inicio=(\d+) fin=(\d+) delta=(\d+)$")
        halladas = list(re.finditer(patron, cuerpo))
        if len(halladas) != 1:
            return False, True, f"contador {fase} ausente o ambiguo", None
        a, b, delta = map(int, halladas[0].groups())
        if b < a or b - a != delta:
            return False, True, f"contador {fase} incoherente", None
        cifras.append((a, b, delta, halladas[0].start()))
    if cifras[0][1] != cifras[1][0]:
        return False, True, "contadores de ambas fases no enlazan", None
    posiciones = [cuerpo.index(literales[0]), cuerpo.index(literales[1]), cifras[0][3],
                  cuerpo.index(literales[2]), cifras[1][3], cuerpo.index(literales[3]), cuerpo.index(literales[4])]
    if posiciones != sorted(posiciones) or len(set(posiciones)) != len(posiciones):
        return False, True, "etapas fuera de orden", None
    return True, True, f"PID {pid}: semántica exacta, salida 0", {"mmap": cifras[0][2], "munmap": cifras[1][2]}
