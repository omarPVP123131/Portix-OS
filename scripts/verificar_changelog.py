#!/usr/bin/env python3
"""Comprueba el registro actual y, con --base, que acompañe cambios del proyecto.

El tramo v1.0.0 y anterior se preserva por decisión del usuario. La inspección
mecánica detecta omisiones y enlaces rotos; no demuestra la veracidad de lo
escrito. En CI se compara la base común con HEAD; no se inspecciona el árbol
sin confirmar cuando se usa --base.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

RAIZ = Path(__file__).resolve().parents[1]
MARCA = b"## [v1.0.0]"
HUELLA_LEGADO = "033e3920776ab2f7ff4bd8802622cd169fcd313516bbf91f68c755e77131e183"
PREFIJOS_RELEVANTES = ("kernel/", "userland/", "boot/", "scripts/", "docs/", ".github/workflows/")
ARCHIVOS_RELEVANTES = {"AGENTS.md", "ROADMAP.md", "readme.md", "CONTRIBUTING.md"}


def cabecera_actual(datos: bytes) -> bytes:
    """Porción anterior al historial que se conserva, o excepción inequívoca."""
    if datos.count(MARCA) != 1:
        raise ValueError("debe existir exactamente una entrada v1.0.0")
    return datos.split(MARCA, 1)[0]


def comprobar_formato(raiz: Path, datos: bytes) -> list[str]:
    errores: list[str] = []
    try:
        actual = cabecera_actual(datos)
    except ValueError as e:
        return [str(e)]
    legado = datos[len(actual):]
    if hashlib.sha256(legado).hexdigest() != HUELLA_LEGADO:
        errores.append("el tramo v1.0.0 y anterior cambió; requiere revisión explícita")
    try:
        texto = actual.decode("utf-8", "strict")
    except UnicodeDecodeError:
        return errores + ["la sección actual no es UTF-8 válido"]
    if texto.count("## [Sin publicar]") != 1:
        errores.append("debe existir exactamente una sección [Sin publicar]")
    fecha = re.search(r"Última actualización:\s*(\d{4}-\d{2}-\d{2})", texto)
    if fecha is None:
        errores.append("falta la fecha ISO de última actualización")
    else:
        try:
            datetime.date.fromisoformat(fecha.group(1))
        except ValueError:
            errores.append("fecha de última actualización inválida")
    if "no certific" not in texto.lower():
        errores.append("falta distinguir resultados parciales de una certificación")
    for destino in re.findall(r"\]\(([^)]+)\)", texto):
        partes = urlsplit(destino)
        if partes.scheme or partes.netloc or not partes.path:
            continue
        ruta = (raiz / unquote(partes.path)).resolve()
        if not ruta.is_relative_to(raiz.resolve()):
            errores.append(f"enlace fuera del proyecto: {destino}")
        elif not ruta.exists():
            errores.append(f"enlace local inexistente: {destino}")
    return errores


def _git(raiz: Path, *args: str) -> bytes:
    p = subprocess.run(["git", *args], cwd=raiz, capture_output=True, check=False, timeout=15)
    if p.returncode:
        raise RuntimeError(f"git {' '.join(args)} falló: {p.stderr.decode('utf-8', 'replace').strip()}")
    return p.stdout


def comprobar_cambios_git(raiz: Path, base: str) -> list[str]:
    """Exige actualizar la sección actual si el diff incluye archivos relevantes."""
    if not re.fullmatch(r"[0-9a-fA-F]{7,64}", base):
        return ["la base Git debe ser un identificador hexadecimal"]
    try:
        comun = _git(raiz, "merge-base", base, "HEAD").decode("ascii").strip()
        cambios = _git(raiz, "diff", "--name-only", "-z", "--no-renames", "--diff-filter=ACMRTD", comun, "HEAD")
        nombres = [n.decode("utf-8", "surrogateescape") for n in cambios.split(b"\0") if n]
        relevantes = [n for n in nombres if n in ARCHIVOS_RELEVANTES or n.startswith(PREFIJOS_RELEVANTES)]
        if not relevantes:
            return []
        if "CHANGELOG.md" not in nombres:
            return [f"hay {len(relevantes)} archivo(s) relevantes sin actualizar CHANGELOG.md"]
        anterior = _git(raiz, "show", f"{comun}:CHANGELOG.md")
        actual = _git(raiz, "show", "HEAD:CHANGELOG.md")
        if cabecera_actual(anterior) == cabecera_actual(actual):
            return ["cambió el proyecto, pero no la sección actual del changelog"]
    except (OSError, subprocess.TimeoutExpired, RuntimeError, ValueError, UnicodeError) as e:
        return [f"no se pudo comparar el historial Git: {e}"]
    return []


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", help="SHA de la base Git para exigir una actualización en el diff")
    args = ap.parse_args()
    try:
        datos = (RAIZ / "CHANGELOG.md").read_bytes()
    except OSError as e:
        print(f"[FAIL] No se puede leer CHANGELOG.md: {e}", file=sys.stderr)
        return 1
    errores = comprobar_formato(RAIZ, datos)
    if args.base:
        errores += comprobar_cambios_git(RAIZ, args.base)
    if errores:
        for error in errores:
            print(f"[FAIL] {error}", file=sys.stderr)
        return 1
    print("[PASS] Estructura, enlaces locales y tramo histórico del changelog")
    if args.base:
        print("[PASS] Los cambios relevantes actualizan la sección vigente")
    print("[INFO] Esta comprobación no valida la veracidad ni certifica el sistema")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
