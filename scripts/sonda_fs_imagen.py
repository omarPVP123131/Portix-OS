#!/usr/bin/env python3
"""Inspección de ext2 sin escribir en una imagen RAW detenida."""
import argparse
import hashlib
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

if __package__:
    from .rutas_imagenes import RUN, dentro_de_dist
    from .sonda_imagen_t029 import localizar_ext2
else:
    from rutas_imagenes import RUN, dentro_de_dist
    from sonda_imagen_t029 import localizar_ext2


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("imagen", type=Path)
    args = ap.parse_args()
    imagen = args.imagen.resolve()
    if not dentro_de_dist(imagen) or not imagen.is_file():
        print("[FAIL] La imagen debe existir dentro de build/dist/")
        return 1
    e2fsck = shutil.which("e2fsck") or next((p for p in ("/usr/sbin/e2fsck", "/sbin/e2fsck") if Path(p).is_file()), None)
    debugfs = shutil.which("debugfs") or next((p for p in ("/usr/sbin/debugfs", "/sbin/debugfs") if Path(p).is_file()), None)
    if not e2fsck or not debugfs:
        print("[FAIL] e2fsck o debugfs no disponibles")
        return 1
    RUN.mkdir(parents=True, exist_ok=True)
    fd, nombre = tempfile.mkstemp(prefix="fsck-java-", suffix=".img", dir=RUN)
    os.close(fd)
    extraida = Path(nombre)
    try:
        with imagen.open("rb") as f:
            offset, tamano = localizar_ext2(f.read(512), imagen.stat().st_size)
            f.seek(offset)
            h = hashlib.sha256()
            with extraida.open("wb") as g:
                restante = tamano
                while restante:
                    bloque = f.read(min(restante, 8 * 1024 * 1024))
                    if not bloque:
                        raise ValueError("imagen truncada")
                    h.update(bloque)
                    g.write(bloque)
                    restante -= len(bloque)
            print(f"[INFO] P3: {tamano} bytes, SHA-256 {h.hexdigest()}", flush=True)
        p = subprocess.run([e2fsck, "-fn", str(extraida)], capture_output=True, text=True, timeout=120)
        print(p.stdout + p.stderr, end="")
        print(f"[{'PASS' if p.returncode == 0 else 'FAIL'}] e2fsck -fn: salida {p.returncode}")
        if p.returncode != 0:
            return 1
        env = dict(os.environ, LC_ALL="C")
        for ruta in ("/usr/lib/jvm/java-21-openjdk-amd64/bin/java", "/usr/local/T010Java.class"):
            r = subprocess.run([debugfs, "-R", f"stat {ruta}", str(extraida)], capture_output=True, text=True, timeout=10, env=env)
            texto = r.stdout + r.stderr
            ok = r.returncode == 0 and "Type: regular" in texto and "Inode:" in texto
            print(f"[{'PASS' if ok else 'FAIL'}] {ruta} es archivo regular")
            if not ok:
                return 1
        r_clase = subprocess.run([debugfs, "-R", "cat /usr/local/T010Java.class", str(extraida)], capture_output=True, timeout=10, env=env)
        clase_host = Path(__file__).resolve().parent / "sondas/T010Java.class"
        esperado = clase_host.read_bytes()
        ok_clase = r_clase.returncode == 0 and bool(esperado) and r_clase.stdout == esperado
        print(f"[{'PASS' if ok_clase else 'FAIL'}] clase idéntica al anfitrión, SHA-256 {hashlib.sha256(r_clase.stdout).hexdigest()}")
        if not ok_clase:
            return 1
        r = subprocess.run([debugfs, "-R", "cat /var/pkg/deb/installed", str(extraida)], capture_output=True, text=True, timeout=10, env=env)
        texto = r.stdout
        for paquete in ("openjdk-21-jre-headless", "libstdc++6"):
            ok = r.returncode == 0 and any(l.split() and l.split()[0] == paquete for l in texto.splitlines())
            print(f"[{'PASS' if ok else 'FAIL'}] base .deb contiene {paquete}")
            if not ok:
                return 1
        print("SONDA FS IMAGEN: PASS; no se modificó la imagen original")
        return 0
    finally:
        extraida.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(main())
