#!/usr/bin/env python3
"""Respaldo del ÁRBOL FUENTE de Portix OS (sin artefactos de compilación).

Motivación medida: durante el incremento del lenguaje general de condiciones un
archivo del kernel (`kernel/src/process/types.rs`) quedó destruido por una
edición defectuosa y NO había ninguna copia con la que restaurarlo —el proyecto
no está bajo control de versiones dentro del entorno de trabajo—. Se recuperó
del snapshot de hidratación del espacio de trabajo, pero la lección es que un
incremento cerrado debe dejar su propio respaldo.

Qué guarda: kernel/, userland/, boot/, scripts/, docs/, plans/, manual/, módulos
y los documentos maestros (AGENTS.md, ROADMAP.md, CHANGELOG.md...). Qué NO
guarda: `target/`, `build/` (incluye `build/dist/`), `backup/`, ni las capturas
`*.ppm`/`*.png` de la raíz — todo ello es artefacto regenerable o voluminoso.

Uso:  python3 scripts/backup_src.py [--nombre <etiqueta>]
Salida: ruta del archivo creado (por defecto `backup/portix-src-<fecha>.tar.gz`).
"""
import os
import sys
import tarfile
import datetime

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXCLUIR_DIRS = {"target", "build", "backup", ".git", "__pycache__",
                "node_modules", ".cargo", ".rustup"}
EXCLUIR_EXT = (".ppm", ".png", ".iso", ".img", ".vdi", ".vmdk", ".o", ".bin")


def incluir(nombre: str) -> bool:
    partes = nombre.split(os.sep)
    if any(p in EXCLUIR_DIRS for p in partes):
        return False
    return not nombre.endswith(EXCLUIR_EXT)


def main() -> int:
    etiqueta = None
    if "--nombre" in sys.argv:
        i = sys.argv.index("--nombre")
        if i + 1 < len(sys.argv):
            etiqueta = sys.argv[i + 1]
    fecha = datetime.datetime.now().strftime("%Y%m%d-%H%M")
    nombre = etiqueta or f"portix-src-{fecha}"
    destino_dir = os.path.join(ROOT, "backup")
    os.makedirs(destino_dir, exist_ok=True)
    destino = os.path.join(destino_dir, f"{nombre}.tar.gz")

    escritos = 0
    with tarfile.open(destino, "w:gz") as tf:
        for base, dirs, files in os.walk(ROOT):
            dirs[:] = [d for d in dirs if d not in EXCLUIR_DIRS]
            for f in files:
                ruta = os.path.join(base, f)
                rel = os.path.relpath(ruta, ROOT)
                if not incluir(rel):
                    continue
                try:
                    tf.add(ruta, arcname=os.path.join("portix", rel))
                    escritos += 1
                except OSError:
                    continue
    size = os.path.getsize(destino)
    print(f"[BACKUP] {escritos} archivos -> {os.path.relpath(destino, ROOT)} ({size/1048576:.2f} MB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
