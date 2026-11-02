#!/usr/bin/env python3
"""Predicados exactos de T-035: cada control debe detectar su omisión concreta."""
import re

NOMBRE = "/usr/local/t035mem"
MODOS = ("control-munmap", "munmap", "control-fixed", "fixed", "control-fork", "fork",
         "control-madvise", "madvise", "control-madvise-ro", "madvise-ro",
         "control-madvise-rw", "madvise-rw", "control-usuario", "usuario", "usuario-lectura",
         "control-pipe", "pipe-invalida", "control-socket", "socket-invalido",
         "control-pipe-parcial", "pipe-parcial", "control-socket-parcial", "socket-parcial",
         "control-perezosa-accesible", "control-perezosa-desmapeada", "perezosa")


def lineas_esperadas(modo):
    if modo not in MODOS:
        raise ValueError("modo inválido")
    p = f"[T035] modo={modo} "
    e = f"[T035-ERROR] modo={modo} "
    intermedias = {
        "control-munmap": [e + "fase=desmapeada esperado=rc:-1,errno:12 observado=rc:0,errno:0"],
        "munmap": [p + "fase=desmapeada rc=-1 errno=12", p + "fase=remapeada inicial=0 nueva=42"],
        "control-fixed": [e + "fase=map-fixed esperado=valor:0 observado=valor:77"],
        "fixed": [p + "fase=map-fixed inicial=0 nueva=99", p + "fase=limpieza rc=-1 errno=12"],
        "control-fork": [e + "fase=aislamiento esperado=valor:77 observado=valor:99"],
        "fork": [p + "fase=hijo estado=0", p + "fase=aislamiento padre=77 hijo=99"],
        "control-madvise": [e + "fase=descarte esperado=valor:0 observado=valor:77"],
        "madvise": [p + "fase=descarte valor=0"],
        "control-madvise-ro": [e + "fase=descarte esperado=valor:0 observado=valor:77"],
        "madvise-ro": [p + "fase=descarte valor=0"],
        "control-madvise-rw": [e + "fase=descarte esperado=valor:0 observado=valor:77"],
        "madvise-rw": [p + "fase=descarte valor=0"],
        "control-pipe": [e + "fase=write-invalida esperado=rc:-1,errno:14 observado=rc:1,errno:0"],
        "pipe-invalida": [p + "fase=write-invalida rc=-1 errno=14 receptor-vacio=si"],
        "control-socket": [e + "fase=write-invalida esperado=rc:-1,errno:14 observado=rc:1,errno:0"],
        "socket-invalido": [p + "fase=write-invalida rc=-1 errno=14 receptor-vacio=si"],
        "control-pipe-parcial": [e + "fase=write-invalida esperado=rc:-1,errno:14 observado=rc:256,errno:0"],
        "pipe-parcial": [p + "fase=write-invalida rc=-1 errno=14 receptor-vacio=si"],
        "control-socket-parcial": [e + "fase=write-invalida esperado=rc:-1,errno:14 observado=rc:256,errno:0"],
        "socket-parcial": [p + "fase=write-invalida rc=-1 errno=14 receptor-vacio=si"],
        "control-perezosa-accesible": [e + "fase=lectura esperado=senal:11,codigo:2,faltas:1 observado=valor:0,codigo:0,faltas:0"],
        "control-perezosa-desmapeada": [e + "fase=lectura esperado=senal:11,codigo:2,faltas:1 observado=valor:256,codigo:1,faltas:1"],
        "perezosa": [p + "fase=lectura senal=11 codigo=2 faltas=1 direccion=correcta", p + "fase=restauracion inicial=0 nueva=77"],
        "control-usuario": [e + "fase=write esperado=rc:-1,errno:14 observado=rc:1,errno:0"],
        "usuario": [p + "fase=write rc=-1 errno=14"],
        "usuario-lectura": [p + "fase=write rc=1 valor=77"],
    }[modo]
    resultado = [p + "inicio", *intermedias, p + "fin"]
    if not modo.startswith("control-"):
        resultado.append(f"[T035] PASS modo={modo}")
    return resultado


def evaluar_cuerpo(texto, modo, codigo):
    """Acepta sólo salida, orden y código exactos; no trata un error genérico como control."""
    esperado = lineas_esperadas(modo)
    codigo_esperado = 1 if modo.startswith("control-") else 0
    if codigo != codigo_esperado:
        return False, f"salida {codigo}, esperada {codigo_esperado}"
    observadas = [l for l in texto.replace("\r", "").splitlines()
                  if l.startswith("[T035]") or l.startswith("[T035-ERROR]")]
    if observadas != esperado:
        return False, f"etapas distintas: observadas={observadas!r}; esperadas={esperado!r}"
    return True, "código y etapas exactos"


def resultado_vm(texto, modo):
    """(aprobado, terminado, detalle); una salida de hijo no sustituye al PID líder."""
    texto = texto.replace("\r", "")
    nombre = re.escape(NOMBRE)
    entradas = list(re.finditer(r"(?m)^\[R3\] enter PID=(\d+) name='" + nombre + r"'(?:\s|$)", texto))
    if len(entradas) != 1:
        return False, False, "sin entrada Ring 3 inequívoca"
    entrada = entradas[0]
    pid = entrada.group(1)
    patron = r"(?m)^PROC: exit PID=" + re.escape(pid) + r" name='" + nombre + r"' code=(-?\d+)(?:\s|$)"
    salidas = list(re.finditer(patron, texto[entrada.end():]))
    if not salidas:
        return False, False, f"PID líder {pid} no terminó"
    if len(salidas) != 1:
        return False, True, f"salida ambigua del PID {pid}"
    salida = salidas[0]
    cuerpo = texto[entrada.end():entrada.end() + salida.start()]
    codigo = int(salida.group(1))
    ok, detalle = evaluar_cuerpo(cuerpo, modo, codigo)
    return ok, True, f"PID {pid}: {detalle}"
