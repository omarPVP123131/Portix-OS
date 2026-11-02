#!/usr/bin/env python3
"""[CICLO 49 / T1] Obtiene binarios ESTÁTICOS de distribuciones Linux reales,
sin modificar, para probar la compatibilidad binaria de Portix.

Cada binario se descarga del espejo oficial de la distribución, se verifica el
SHA-256 del paquete y el del binario extraído (valores fijados aquí: si la
distro cambiase el fichero, el build lo detecta en vez de aceptar cualquier
cosa). Nunca se toca el binario: Portix debe ejecutarlo tal cual.

Uso: fetch_distro.py <directorio_destino>   -> escribe <destino>/busybox.static
"""
import hashlib, io, sys, tarfile, urllib.request
from pathlib import Path

BINARIOS = {
    "busybox.static": {
        "distro": "Alpine Linux v3.20 (main/x86_64)",
        "url": "https://dl-cdn.alpinelinux.org/alpine/v3.20/main/x86_64/busybox-static-1.36.1-r31.apk",
        "sha256_apk": "1ca8a2894b073edea2a04231f335ad48795f92617c5eacae59458c51202068ed",
        "miembro": "bin/busybox.static",
        "sha256_bin": "6d4ae568988ee24beb9dac4afdac4df67f90bbaed6ca47628da35ff5eb632a4c",
        "tamano": 1025960,
    },
    # [CICLO 50 / T1b] Cargador dinámico de musl (ld-musl == libc.so) y busybox
    # DINÁMICO (PIE, PT_INTERP=/lib/ld-musl-x86_64.so.1): el hito "ejecutables
    # dinámicos de distro" se mide con estos dos ficheros sin modificar.
    "ld-musl-x86_64.so.1": {
        "distro": "Alpine Linux v3.20 (main/x86_64) paquete musl 1.2.5-r4",
        "url": "https://dl-cdn.alpinelinux.org/alpine/v3.20/main/x86_64/musl-1.2.5-r4.apk",
        "sha256_apk": "b255cdcf4f64eb301f4dbd40d1fbdc0fcb021613b3f0d124a41eeff62e2bc7d8",
        "miembro": "lib/ld-musl-x86_64.so.1",
        "sha256_bin": "42ce1c3a16ba225fb6714311b4071d96a6bc177c296cb84aa56ad41c0be77e21",
        "tamano": 649832,
    },
    "busybox.dyn": {
        "distro": "Alpine Linux v3.20 (main/x86_64) paquete busybox 1.36.1-r31",
        "url": "https://dl-cdn.alpinelinux.org/alpine/v3.20/main/x86_64/busybox-1.36.1-r31.apk",
        "sha256_apk": "a7c2c34d0e1c60684d3f0a27e1c939c9e25a60514fe2294dfbe7a6534fe531c4",
        "miembro": "bin/busybox",
        "sha256_bin": "b7de36c48f4c8b08a256200f3d6ccf20b9aade3219e356563a5ec3f12db78d7b",
        "tamano": 808712,
    },
}

def sha256(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()

def obtener(nombre: str, destino: Path, log=print) -> Path | None:
    meta = BINARIOS[nombre]
    salida = destino / nombre
    if salida.exists() and sha256(salida.read_bytes()) == meta["sha256_bin"]:
        log(f"    [DISTRO] {nombre}: ya presente y verificado (SHA-256 {meta['sha256_bin'][:16]}…)")
        return salida
    try:
        log(f"    [DISTRO] descargando {meta['url']}")
        with urllib.request.urlopen(meta["url"], timeout=60) as r:
            apk = r.read()
    except Exception as e:  # sin red: se informa y la prueba T1 fallará (no se finge)
        log(f"    [DISTRO] SIN RED: no se pudo descargar {nombre} ({e}); la prueba T1 fallará")
        return None
    if sha256(apk) != meta["sha256_apk"]:
        log(f"    [DISTRO] SHA-256 del paquete NO coincide: {sha256(apk)} != {meta['sha256_apk']}; se descarta")
        return None
    # Un .apk es un tar.gz (concatenación de varios; tarfile lee el que contiene los ficheros)
    with tarfile.open(fileobj=io.BytesIO(apk), mode="r:gz") as t:
        datos = None
        for m in t:
            if m.name.lstrip("./") == meta["miembro"]:
                datos = t.extractfile(m).read()
                break
    if datos is None or sha256(datos) != meta["sha256_bin"] or len(datos) != meta["tamano"]:
        log(f"    [DISTRO] binario {meta['miembro']} ausente o con SHA-256 inesperado; se descarta")
        return None
    destino.mkdir(parents=True, exist_ok=True)
    salida.write_bytes(datos)
    log(f"    [DISTRO] {nombre}: {len(datos)} B de {meta['distro']}, SHA-256 verificado")
    return salida

# ── [CICLO 50 / T1c] Paquetes de distro con VARIOS ficheros (tcc + musl-dev) ──
# Cada entrada: .apk oficial (SHA-256 fijado) y los miembros que se instalan en
# Portix con su ruta original. El contenido no se modifica. `copias` duplica un
# fichero bajo otra ruta cuando la distro usa un enlace simbólico (FAT no tiene).
PAQUETES = {
    "tcc": {
        "distro": "Alpine Linux v3.22 (community/x86_64) tcc 0.9.27_git20250106-r0",
        "apks": [
            ("https://dl-cdn.alpinelinux.org/alpine/v3.22/community/x86_64/tcc-0.9.27_git20250106-r0.apk",
             "db4720f2f9c6c5cf46812d52af151cbeaa95c0e4676aa956b3e57b2c3ad9802a"),
            ("https://dl-cdn.alpinelinux.org/alpine/v3.22/community/x86_64/tcc-libs-0.9.27_git20250106-r0.apk",
             "f0d06346af0ffd0d4bca073d56b51e6baaa2825b82996a58ddf6a31dba5df2d0"),
            ("https://dl-cdn.alpinelinux.org/alpine/v3.22/community/x86_64/tcc-dev-0.9.27_git20250106-r0.apk",
             "5bd123d6b22a9e39c5e26a79759573c47243dd1d04bcdc26ce8275ca35fd91f7"),
            # libtcc1.a: runtime que tcc enlaza en cada programa
            ("https://dl-cdn.alpinelinux.org/alpine/v3.22/community/x86_64/tcc-libs-static-0.9.27_git20250106-r0.apk",
             "2aab7f70b8f8918ebe431f9b77193a6f43b72c9206b0e5c7b35fe3975e64f3a9"),
        ],
        "prefijos": ("usr/bin/tcc", "usr/lib/libtcc.so", "usr/lib/tcc/"),
        "copias": [],
    },
    "musl-dev": {
        "distro": "Alpine Linux v3.20 (main/x86_64) musl-dev 1.2.5-r4",
        "apks": [
            ("https://dl-cdn.alpinelinux.org/alpine/v3.20/main/x86_64/musl-dev-1.2.5-r4.apk",
             "e9cca1709f3a740976f22cc27b825261de062d97e7de2d08c41243017c389338"),
        ],
        # cabeceras y crt*.o; libc.a (9 MB) no hace falta para enlazar dinámico
        "prefijos": ("usr/include/", "usr/lib/crt1.o", "usr/lib/crti.o", "usr/lib/crtn.o", "usr/lib/Scrt1.o"),
        # /usr/lib/libc.so -> ../../lib/ld-musl-x86_64.so.1 en la distro
        "copias": [("usr/lib/libc.so", "ld-musl-x86_64.so.1")],
    },
}

def obtener_paquete(nombre: str, destino: Path, log=print):
    """Devuelve [(ruta_portix, bytes)] con los miembros del paquete, o None si
    no hay red o algún SHA-256 no coincide (la prueba correspondiente fallará)."""
    meta = PAQUETES[nombre]
    entradas = []
    for url, sha_apk in meta["apks"]:
        try:
            log(f"    [DISTRO] descargando {url}")
            with urllib.request.urlopen(url, timeout=60) as r:
                apk = r.read()
        except Exception as e:
            log(f"    [DISTRO] SIN RED: no se pudo descargar {nombre} ({e})")
            return None
        if sha256(apk) != sha_apk:
            log(f"    [DISTRO] SHA-256 de {url} NO coincide: {sha256(apk)}; se descarta")
            return None
        with tarfile.open(fileobj=io.BytesIO(apk), mode="r:gz") as t:
            for m in t:
                if not m.isfile():
                    continue
                nom = m.name.lstrip("./")
                if any(nom == p or (p.endswith("/") and nom.startswith(p)) for p in meta["prefijos"]):
                    entradas.append(("/" + nom, t.extractfile(m).read()))
    for ruta_portix, fich in meta["copias"]:
        origen = destino / fich
        if origen.exists():
            entradas.append(("/" + ruta_portix, origen.read_bytes()))
    if not entradas:
        log(f"    [DISTRO] {nombre}: sin miembros; se descarta")
        return None
    total = sum(len(d) for _, d in entradas)
    log(f"    [DISTRO] {nombre}: {len(entradas)} ficheros, {total} B de {meta['distro']}, SHA-256 verificado")
    return entradas

if __name__ == "__main__":
    dest = Path(sys.argv[1] if len(sys.argv) > 1 else "build/packages")
    ok = all(obtener(n, dest) is not None for n in BINARIOS)
    sys.exit(0 if ok else 1)
