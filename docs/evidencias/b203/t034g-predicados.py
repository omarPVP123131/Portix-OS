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
    esperado = valor if modo in ("directo", "dormido") else 0
    cuerpo = texto[entrada.end():entrada.end() + fin.start()]
    if modo in ("directo", "dormido"):
        marca = f"[T034-AP] cpu=1 exit_group={valor}"
        if modo == "dormido":
            lider = "[T034-LIDER] exit(2) antes del cierre del grupo"
            rama = f"[T-005] lider de consola cerrado desde un AP: lo termina el BSP (PID {pid})"
            if lider not in cuerpo.splitlines() or rama not in cuerpo.splitlines():
                return False, True, f"PID {pid}: no se observó la rama del líder dormido cerrado desde AP"
    elif modo == "wait":
        marca = f"[T034-WAIT] code={valor} esperado={(valor & 255) << 8} observado={(valor & 255) << 8} pid_correcto=true OK"
    elif modo == "sigkill":
        marca = "[T034-SIGKILL] esperado=9 observado=9 pid_correcto=true OK"
    elif modo == "sigkill-hilos":
        m = re.search(r"(?m)^\[T034-SIGKILL-HILOS\] hijo=(\d+) listo=true envio=true esperado=9 observado=9 pid_correcto=true sin_proceso=true OK$", cuerpo)
        if not m:
            return False, True, f"PID {pid}: falta estado exacto de SIGKILL multihilo"
        hijo = m.group(1)
        inicio_ap = cuerpo.find("[T034-VICTIMA] hilo activo en CPU 1\n")
        inicio_lider = cuerpo.find(f"[T-005] lider pid={hijo} salio con hilos vivos: espera al ultimo\n")
        padre = cuerpo.find(f"[T034-PADRE] SIGKILL a hijo={hijo}\n")
        if min(inicio_ap, inicio_lider, padre) < 0 or not (inicio_ap < padre and inicio_lider < padre < m.start()):
            return False, True, f"PID {pid}: no se confirmó el líder dormido antes del SIGKILL externo"
        marca = m.group(0)
    else:
        raise ValueError("modo desconocido")
    if observado != esperado:
        return False, True, f"PID {pid}: código observado {observado}, esperado {esperado}"
    if marca not in cuerpo.splitlines() or "[T034-ERROR]" in cuerpo:
        return False, True, f"PID {pid}: falta la línea exacta esperada"
    return True, True, f"PID {pid}: marca exacta y código {observado}"
