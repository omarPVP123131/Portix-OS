#!/usr/bin/env python3
"""Ubicación y aislamiento de imágenes del anfitrión (T-030)."""

import os
import shutil
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "build" / "dist"
RUN = DIST / "pruebas" / "run"
DISCOS_SILICIO = (
    ("nvme0", DIST / "pruebas" / "nvme.img", 64, b"PORTIX-NVME-SILICIO-LBA0"),
    ("sata0", DIST / "pruebas" / "sata.img", 64, b"PORTIX-AHCI-SATA-LBA0"),
    ("usb0", DIST / "pruebas" / "usb.img", 16, b"PORTIX-XHCI-USB-LBA0"),
)


def dentro_de_dist(ruta):
    """Sólo rutas reales bajo dist; rechaza escapes por '..' o enlaces."""
    try:
        # Si build o dist redirigen fuera del proyecto, no se crean imágenes allí.
        if DIST.resolve() != DIST:
            return False
        Path(ruta).expanduser().resolve().relative_to(DIST)
        return True
    except (ValueError, OSError, RuntimeError):
        return False


def copia_privada(origen):
    """Copia aislada única en dist; si falla, elimina sólo el archivo creado."""
    origen = Path(origen)
    if not origen.is_file():
        raise FileNotFoundError(f"no existe la imagen de origen: {origen}")
    if not dentro_de_dist(RUN):
        raise ValueError("el directorio de copia escapa de build/dist")
    RUN.mkdir(parents=True, exist_ok=True)
    fd, nombre = tempfile.mkstemp(prefix=f"portix-run-{os.getpid()}-", suffix=".img", dir=RUN)
    os.close(fd)
    try:
        shutil.copyfile(origen, nombre)
        return Path(nombre)
    except BaseException:
        try:
            os.unlink(nombre)
        except OSError:
            pass
        raise


def conservar_exclusivo(origen, destino):
    """Conserva una imagen sin sobrescribir un destino ya existente."""
    if not dentro_de_dist(destino):
        raise ValueError("el destino debe estar dentro de build/dist")
    destino = Path(destino).expanduser().resolve()
    creado = False
    try:
        with open(origen, "rb") as fuente, open(destino, "xb") as salida:
            creado = True
            shutil.copyfileobj(fuente, salida, 8 * 1024 * 1024)
    except BaseException:
        if creado:
            try:
                destino.unlink()
            except OSError:
                pass
        raise


def crear_disco_patron(ruta, mib, firma):
    """Crea el periférico sólo si no existe; nunca pisa datos desconocidos."""
    ruta = Path(ruta)
    if not dentro_de_dist(ruta) or ruta.is_symlink():
        raise ValueError(f"disco fuera de build/dist o enlace: {ruta}")
    tamano = mib * 1024 * 1024
    if ruta.exists():
        if ruta.is_file() and ruta.stat().st_size == tamano:
            with ruta.open("rb") as f:
                if f.read(len(firma)) == firma:
                    return False
        raise FileExistsError(f"disco existente con contenido inesperado; no se sobrescribe: {ruta}")
    ruta.parent.mkdir(parents=True, exist_ok=True)
    sector = bytearray(512)
    sector[:len(firma)] = firma
    for i in range(64, 512):
        sector[i] = (i * 7 + 3) & 0xFF
    creado = False
    try:
        with ruta.open("xb") as f:
            creado = True
            f.write(sector)
            f.truncate(tamano)
    except BaseException:
        if creado:
            try:
                ruta.unlink()
            except OSError:
                pass
        raise
    return True
