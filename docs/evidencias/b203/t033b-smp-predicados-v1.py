#!/usr/bin/env python3
"""Predicados inequívocos para T-033 SMP; los controles deben fallar por su causa exacta."""
import re

NOMBRE = "/usr/local/t033smp"
MODOS = ("control-munmap", "munmap", "control-proteccion", "proteccion")


def lineas_esperadas(modo):
    if modo not in MODOS:
        raise ValueError("modo inválido")
    prefijo = f"[T033-SMP] modo={modo} "
    lineas = [prefijo + "inicio cpu_lider=0 cpu_ap=1 paginas=1"]
    if modo in ("munmap", "control-munmap"):
        lineas.append(prefijo + "fase=presente valor=49")
        if modo == "control-munmap":
            lineas.append("[T033-SMP-ERROR] modo=control-munmap fase=desmapeada esperado=senal:11,codigo:1 observado=valor:49,codigo:0,faltas:0")
        else:
            lineas.extend((prefijo + "fase=desmapeada senal=11 codigo=1 direccion=correcta faltas=1",
                           prefijo + "fase=remapeada inicial=0 nueva=90 candidata=183 aislada=si",
                           prefijo + "fase=remapeo-directo inicial=0 nueva=109 candidatas=183,195 aisladas=si"))
    else:
        lineas.append(prefijo + "fase=presente valor=77")
        if modo == "control-proteccion":
            lineas.append("[T033-SMP-ERROR] modo=control-proteccion fase=protegida esperado=senal:11,codigo:2 observado=valor:77,codigo:0,faltas:0")
        else:
            lineas.extend((prefijo + "fase=protegida senal=11 codigo=2 direccion=correcta faltas=1",
                           prefijo + "fase=restaurada valor=77"))
    lineas.append(prefijo + "fin cpu_lider=0 cpu_ap=1")
    if not modo.startswith("control-"):
        lineas.append(f"[T033-SMP] PASS modo={modo}")
    return lineas


def resultado_smp(texto, modo):
    """(aprobado, terminado, detalle). El PID líder debe salir exactamente 0 o 1.

    Para los controles sólo se acepta la anomalía deliberada específica, no un
    error genérico ni una señal fatal. Otros PIDs de hilos no sustituyen al líder.
    """
    esperadas = lineas_esperadas(modo)
    texto = texto.replace("\r", "")
    patron_entrada = r"(?m)^\[R3\] enter PID=(\d+) name='" + re.escape(NOMBRE) + r"'(?:\s|$)"
    entradas = list(re.finditer(patron_entrada, texto))
    if len(entradas) != 1:
        return False, False, "sin entrada Ring 3 inequívoca"
    entrada = entradas[0]
    pid = entrada.group(1)
    patron_salida = (r"(?m)^PROC: exit PID=" + re.escape(pid) + r" name='" + re.escape(NOMBRE)
                     + r"' code=(-?\d+)(?:\s|$)")
    salidas = list(re.finditer(patron_salida, texto[entrada.end():]))
    if not salidas:
        return False, False, f"PID líder {pid} no terminó"
    if len(salidas) != 1:
        return False, True, f"PID líder {pid} tiene salidas ambiguas"
    salida = salidas[0]
    cuerpo = texto[entrada.end():entrada.end() + salida.start()]
    lineas = cuerpo.splitlines()
    errores = [linea for linea in lineas if linea.startswith("[T033-SMP-ERROR]")]
    codigo_esperado = 1 if modo.startswith("control-") else 0
    codigo = int(salida.group(1))
    if codigo != codigo_esperado:
        detalle_error = f"; {errores[0]}" if len(errores) == 1 else ""
        return False, True, f"PID {pid} salió {codigo}, esperado {codigo_esperado}{detalle_error}"
    if modo.startswith("control-"):
        if len(errores) != 1 or errores[0] not in esperadas:
            return False, True, "control negativo sin la anomalía exacta"
        if any(linea.startswith("[T033-SMP] PASS") for linea in lineas):
            return False, True, "un control negativo declaró PASS"
    elif errores:
        return False, True, "la sonda informó un error"
    posiciones = []
    for literal in esperadas:
        if lineas.count(literal) != 1:
            return False, True, f"falta o se duplicó: {literal}"
        posiciones.append(lineas.index(literal))
    if posiciones != sorted(posiciones) or len(set(posiciones)) != len(posiciones):
        return False, True, "etapas fuera de orden"
    if modo.startswith("control-"):
        return True, True, f"PID {pid}: detectó la omisión deliberada y salió 1"
    return True, True, f"PID {pid}: observaciones exactas y salida 0"
