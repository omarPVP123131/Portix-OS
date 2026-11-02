#!/usr/bin/env python3
"""Espera el fin de una suite y conserva su evidencia fuera de build/.

No inicia pruebas ni modifica su veredicto. Espera al proceso mediante pidfd,
con un plazo máximo; después guarda los registros comprimidos y un resumen.
"""

import argparse
import gzip
import hashlib
import os
import re
import select
import shutil
import sys
from pathlib import Path

from auditar_evidencia_b203 import auditar


def conservar(origen, destino):
    temporal = destino.with_suffix(destino.suffix + ".tmp")
    try:
        with origen.open("rb") as entrada, temporal.open("wb") as archivo:
            with gzip.GzipFile(fileobj=archivo, mode="wb", compresslevel=1, mtime=0) as salida:
                shutil.copyfileobj(entrada, salida)
        temporal.replace(destino)
    finally:
        temporal.unlink(missing_ok=True)


def resumen(registro, serie):
    lineas = []
    if registro.is_file():
        texto = re.sub(r"\x1b\[[0-9;]*m", "", registro.read_text(encoding="utf-8", errors="replace"))
        totales = re.findall(r"(?m)^\s*TOTAL:\s*(\d+)\s*\|\s*PASSED:\s*(\d+)\s*\|\s*FAILED:\s*(\d+)", texto)
        fines = re.findall(r"(?m)^FIN=(\d+)\s*$", texto)
        if totales and fines:
            total, aprobadas, fallidas = (int(x) for x in totales[-1])
            codigo = int(fines[-1])
            completo = total > 0 and aprobadas + fallidas == total
            verde = completo and fallidas == 0 and aprobadas == total and codigo == 0
            lineas.append(f"Suite: total={total}, aprobadas={aprobadas}, fallidas={fallidas}, FIN={codigo}; "
                          + ("VERDE" if verde else "NO VERDE O INCOMPLETA"))
        else:
            lineas.append("Suite: INCOMPLETA; falta el total o FIN.")
    else:
        lineas.append("Suite: sin registro de resultados.")
    if serie.is_file():
        try:
            e = auditar(serie.read_text(encoding="utf-8", errors="replace"))
        except ValueError as exc:
            lineas.append(f"Auditoría adicional b203: FALLO o registro incompleto: {exc}")
        else:
            lineas.append(f"Auditoría adicional b203: PASS; páginas {e.paginas_antes} → {e.paginas_despues}, "
                          f"delta={e.delta}; pkg informó SHA-256 correcto, prefijo={e.prefijo_sha256}.")
    else:
        lineas.append("Auditoría adicional b203: sin registro serie.")
    lineas.append("La auditoría de texto no recalcula el hash del paquete ni sustituye la suite o la recertificación.")
    return "\n".join(lineas) + "\n"


def actualizar_sumas(destino):
    archivos = sorted([*destino.glob("*.log.gz"), *destino.glob("*.log"),
                       *destino.glob("*.txt.gz"), *destino.glob("*-estado.txt")])
    lineas = []
    for ruta in archivos:
        h = hashlib.sha256()
        with ruta.open("rb") as f:
            for trozo in iter(lambda: f.read(1024 * 1024), b""):
                h.update(trozo)
        lineas.append(f"{h.hexdigest()}  {ruta.name}\n")
    temporal = destino / "SHA256SUMS.tmp"
    temporal.write_text("".join(lineas), encoding="utf-8")
    temporal.replace(destino / "SHA256SUMS")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", required=True, type=int, help="PID del supervisor que escribe FIN y copia el serie")
    parser.add_argument("--registro", required=True, type=Path)
    parser.add_argument("--serie", required=True, type=Path)
    parser.add_argument("--destino", required=True, type=Path)
    parser.add_argument("--nombre", required=True)
    parser.add_argument("--plazo", type=int, default=4600, help="máximo de espera, en segundos")
    args = parser.parse_args()
    if args.pid <= 0 or args.plazo <= 0 or not re.fullmatch(r"[A-Za-z0-9_-]+", args.nombre):
        parser.error("PID, plazo o nombre inválidos")
    try:
        fd = os.pidfd_open(args.pid)
    except (AttributeError, OSError) as exc:
        print(f"No se puede esperar al proceso {args.pid}: {exc}", file=sys.stderr)
        return 1
    try:
        espera = select.poll()
        espera.register(fd, select.POLLIN)
        print(f"Esperando al supervisor {args.pid}; plazo máximo {args.plazo} s.", flush=True)
        if not espera.poll(args.plazo * 1000):
            print("Plazo agotado: no se presenta una suite todavía en ejecución como terminada.", file=sys.stderr)
            return 1
    finally:
        os.close(fd)

    args.destino.mkdir(parents=True, exist_ok=True)
    for origen, nombre in ((args.registro, f"{args.nombre}.log.gz"),
                            (args.serie, f"serie_{args.nombre}.log.gz")):
        if origen.is_file():
            conservar(origen, args.destino / nombre)
    (args.destino / f"{args.nombre}-estado.txt").write_text(resumen(args.registro, args.serie), encoding="utf-8")
    actualizar_sumas(args.destino)
    print(resumen(args.registro, args.serie), end="", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
