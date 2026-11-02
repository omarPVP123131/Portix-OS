#!/usr/bin/env python3
"""Veredictos estrictos para la prueba de salida de grupos desde otro núcleo."""
import re

NOMBRE = "/usr/local/t034"


def resultado(texto, modo, valor=0):
    texto = texto.replace("\r", "")
    entradas = list(re.finditer(r"(?m)^\[R3\] enter PID=(\d+) name='" + re.escape(NOMBRE) + r"'(?:\s|$)", texto))
    if len(entradas) != 1:
        return False, False, "sin entrada inequívoca de t034"
    entrada = entradas[0]
    pid = entrada.group(1)
    fin = re.search(r"(?m)^PROC: exit PID=" + re.escape(pid) + r" name='" + re.escape(NOMBRE) + r"' code=(-?\d+)(?:\s|$)", texto[entrada.end():])
    if not fin:
        return False, False, f"PID {pid} no ha terminado"
    observado = int(fin.group(1))
    esperado = valor if modo == "directo" else 0
    cuerpo = texto[entrada.end():entrada.end() + fin.start()]
    if modo == "directo":
        marca = f"[T034-AP] cpu=1 exit_group={valor}"
    elif modo == "wait":
        marca = f"[T034-WAIT] code={valor} esperado={(valor & 255) << 8} observado={(valor & 255) << 8} pid_correcto=true OK"
    elif modo == "sigkill":
        marca = "[T034-SIGKILL] esperado=9 observado=9 pid_correcto=true OK"
    else:
        raise ValueError("modo desconocido")
    if observado != esperado:
        return False, True, f"PID {pid}: código observado {observado}, esperado {esperado}"
    if marca not in cuerpo.splitlines() or "[T034-ERROR]" in cuerpo:
        return False, True, f"PID {pid}: falta la línea exacta esperada"
    return True, True, f"PID {pid}: marca exacta y código {observado}"
