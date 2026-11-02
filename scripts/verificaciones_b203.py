#!/usr/bin/env python3
"""Predicados del ejecutor b203; analizan hechos publicados por el invitado.

No descargan paquetes, no calculan el SHA-256 de un .deb y no sustituyen una
prueba en QEMU. Los controles de `pkg` deben proceder de la misma ejecución.
"""

import re

_PAQUETE = "libstdc++6_14.2.0-19"
_VERIFICADO = re.compile(
    r"(?m)^\[DEB\] libstdc\+\+6_14\.2\.0-19_amd64\.deb: Size OK, "
    r"SHA256 ([0-9a-f]{16}) OK, control: libstdc\+\+6 14\.2\.0-19[ \t]*$"
)
_INSTALADO = re.compile(
    r"(?m)^\[DEB\] libstdc\+\+6 14\.2\.0-19 instalado \((\d+) entradas\)[ \t]*$"
)
_PLAN_COMPLETADO = re.compile(r"(?m)^\[DEB-OK\] instalados: ([^\n]+)$")
_CONTADOR = re.compile(
    r"(?m)^mmap_fichero_perezoso: tramos_vivos (\d+) registrados (\d+) "
    r"paginas_desde_fichero (\d+) bytes (\d+)[ \t]*$"
)
_PEREZOSO = re.compile(r"\[SYS\] MMAP perezoso addr=0x[0-9A-Fa-f]+ pages=(\d+) prot=\d+ fd=\d+")
_ANSIOSO = re.compile(r"\[SYS\] MMAP addr=0x[0-9A-Fa-f]+ pages=(\d+) fd=\d+")
_IRQ = re.compile(r"(?m)^irq0_perdidos: (\d+) irq0_peor_hueco_us: (\d+) irq0_sitio: pid (\d+) syscall (\d+) rip (0x[0-9a-fA-F]+)[ \t]*$")


def verificar_libstdcpp(serie):
    """Exige verificación, instalación no vacía y plan completado, en orden.

    La dependencia puede instalarse con rustc o con Java. El prefijo de hash
    se usa sólo para reconocer el informe de pkg: la comparación íntegra de
    SHA-256 se hace dentro de pkg, contra el índice de la distribución.
    """
    serie = serie.replace("\r", "")
    verificaciones = list(_VERIFICADO.finditer(serie))
    if not verificaciones:
        return False, "falta la verificacion completa informada por pkg"
    instalaciones = [m for m in _INSTALADO.finditer(serie) if int(m.group(1)) > 0]
    if not instalaciones:
        return False, "falta la instalacion individual con entradas"
    planes = list(_PLAN_COMPLETADO.finditer(serie))
    for verificado in verificaciones:
        for instalado in instalaciones:
            if instalado.start() <= verificado.end():
                continue
            for plan in planes:
                if plan.start() > instalado.end() and _PAQUETE in plan.group(1).split():
                    return True, f"pkg informo Size y SHA256 OK ({verificado.group(1)}); instalacion y plan completos"
    return False, "faltan instalacion o plan completos posteriores a la verificacion"


def paginas_desde_fichero(texto):
    """Lee exactamente una línea completa de /proc/smp; None si falta o es ambigua."""
    coincidencias = list(_CONTADOR.finditer(texto.replace("\r", "")))
    return int(coincidencias[0].group(3)) if len(coincidencias) == 1 else None


def verificar_delta(antes, despues):
    """Exige dos lecturas y un delta en [1000, 34389); nunca inventa cero."""
    if antes is None or despues is None:
        return False, None
    delta = despues - antes
    return 1000 <= delta < 34389, delta


def verificar_irq_java(texto):
    """Dos límites de T-026; una línea real e inequívoca, sin inventar ceros."""
    halladas = list(_IRQ.finditer(texto.replace("\r", "")))
    if len(halladas) != 1:
        return False, None, None
    perdidos, hueco = map(int, halladas[0].groups()[:2])
    return perdidos < 600 and hueco < 300000, perdidos, hueco


def verificar_mapeos_java(texto):
    """El llamador entrega SOLO la ventana de Java, sin las pruebas anteriores."""
    perezosos = [int(x) for x in _PEREZOSO.findall(texto)]
    ansiosos = [int(x) for x in _ANSIOSO.findall(texto)]
    max_perezoso = max(perezosos, default=0)
    max_ansioso = max(ansiosos, default=0)
    return max_perezoso >= 30000 and max_ansioso < 30000, max_perezoso, max_ansioso
