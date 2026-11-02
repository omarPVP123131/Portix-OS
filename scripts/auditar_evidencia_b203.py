#!/usr/bin/env python3
"""Audita dos comprobaciones de b203 sobre un registro serie REAL de QEMU.

Es una auditoría independiente del runner; no arranca QEMU ni certifica una
suite. El mensaje de SHA-256 demuestra que pkg informó haber verificado el
paquete; este análisis de texto no vuelve a calcular el hash del .deb.

Uso: python3 scripts/auditar_evidencia_b203.py build/serie_suite113e.log
"""

import argparse
import gzip
import re
import sys
from dataclasses import dataclass
from pathlib import Path

_PAQUETE = "libstdc++6_14.2.0-19"
_COMANDO_INSTALACION = "run pkg deb-add openjdk-21-jre-headless"
_COMANDO_JAVA = "run /usr/lib/jvm/java-21-openjdk-amd64/bin/java -cp /usr/local T010Java"
_PROMPT = re.compile(r"(?m)^root@portix:[^\n]*?\$ ([^\n]+)$")
_CONTADOR = re.compile(
    r"(?m)^mmap_fichero_perezoso: tramos_vivos (\d+) registrados (\d+) "
    r"paginas_desde_fichero (\d+) bytes (\d+)\s*$"
)
_VERIFICADO = re.compile(
    r"(?m)^\[DEB\] libstdc\+\+6_14\.2\.0-19_amd64\.deb: Size OK, "
    r"SHA256 ([0-9a-f]{16}) OK, control: libstdc\+\+6 14\.2\.0-19\s*$"
)
_INSTALADOS = re.compile(r"(?m)^\[DEB-OK\] instalados: ([^\n]+)$")


@dataclass(frozen=True)
class Evidencia:
    paginas_antes: int
    paginas_despues: int
    prefijo_sha256: str

    @property
    def delta(self):
        return self.paginas_despues - self.paginas_antes


def _un_contador(texto, contexto):
    lecturas = list(_CONTADOR.finditer(texto))
    if len(lecturas) != 1:
        raise ValueError(f"{contexto}: se esperaba una lectura completa, encontradas {len(lecturas)}")
    return int(lecturas[0].group(3))


def auditar(texto):
    """Exige verificación e instalación de libstdc++6 y dos lecturas acotadas.

    La lectura inicial debe pertenecer a la orden inmediatamente anterior a la
    instalación de Java; la final, a la primera orden tras [T010-PASS]. No se
    reemplaza una lectura ausente por cero ni se aceptan nombres en una URL.
    """
    texto = texto.replace("\r", "")
    comandos = list(_PROMPT.finditer(texto))
    instalar = next((i for i, m in enumerate(comandos) if m.group(1).strip() == _COMANDO_INSTALACION), None)
    if instalar is None or instalar == 0:
        raise ValueError("no aparece la orden de instalación de Java con su lectura previa")
    if comandos[instalar - 1].group(1).strip() != "cat /proc/smp":
        raise ValueError("la orden anterior a Java no es cat /proc/smp")
    antes = _un_contador(texto[comandos[instalar - 1].end():comandos[instalar].start()], "lectura anterior")

    previo_java = texto[:comandos[instalar].start()]
    verificado = _VERIFICADO.search(previo_java)
    if not verificado:
        raise ValueError("no hay confirmación completa de tamaño y SHA-256 de libstdc++6")
    instalacion = next(
        (m for m in _INSTALADOS.finditer(previo_java, verificado.end()) if _PAQUETE in m.group(1).split()),
        None,
    )
    if instalacion is None:
        raise ValueError("no hay confirmación posterior de instalación de libstdc++6")

    indice_java = next(
        (i for i in range(instalar + 1, len(comandos)) if comandos[i].group(1).strip() == _COMANDO_JAVA),
        None,
    )
    if indice_java is None:
        raise ValueError("no aparece la orden de la sonda Java")
    siguiente = indice_java + 1
    if siguiente >= len(comandos) or comandos[siguiente].group(1).strip() != "cat /proc/smp":
        raise ValueError("falta la lectura de /proc/smp inmediatamente posterior a Java")
    salida_java = texto[comandos[indice_java].end():comandos[siguiente].start()]
    if not re.search(r"(?m)^\[T010-PASS\]", salida_java):
        raise ValueError("la sonda Java no declaró T010-PASS antes de la siguiente orden")
    limite = comandos[siguiente + 1].start() if siguiente + 1 < len(comandos) else len(texto)
    despues = _un_contador(texto[comandos[siguiente].end():limite], "lectura posterior")
    delta = despues - antes
    if not 1000 <= delta < 34389:
        raise ValueError(f"delta fuera del intervalo exigido [1000, 34389): {delta}")
    return Evidencia(antes, despues, verificado.group(1))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("registro", nargs="+", type=Path, help="registro serie guardado por el runner")
    args = parser.parse_args(argv)
    fallos = 0
    for ruta in args.registro:
        try:
            if ruta.suffix == ".gz":
                with gzip.open(ruta, "rt", encoding="utf-8", errors="replace") as archivo:
                    contenido = archivo.read()
            else:
                contenido = ruta.read_text(encoding="utf-8", errors="replace")
            resultado = auditar(contenido)
        except (OSError, EOFError, ValueError) as exc:
            fallos += 1
            print(f"[FALLO] {ruta}: {exc}")
            continue
        print(
            f"[PASS] {ruta}: libstdc++6 verificada e instalada según pkg "
            f"(prefijo SHA-256={resultado.prefijo_sha256}); "
            f"páginas antes={resultado.paginas_antes}, después={resultado.paginas_despues}, "
            f"delta={resultado.delta}"
        )
    return 1 if fallos else 0


if __name__ == "__main__":
    sys.exit(main())
