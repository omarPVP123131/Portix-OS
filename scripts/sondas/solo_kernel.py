# [T-003] Compila SOLO el kernel (y el stage2 con su nº de sectores) para
# iterar con scripts/sondas/inyectar_kernel.sh sin rehacer la imagen completa.
import sys, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import build
s = build.build_kernel()
build.assemble_stage2(s)
print("FIN kernel sectores", s)
