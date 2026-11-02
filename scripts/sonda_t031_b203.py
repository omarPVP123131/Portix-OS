"""Sonda T-031 (defecto 35): cierre de grupo desde un hilo en otra CPU.

Ejecuta el fixture `scripts/sondas/t034_exit_group.rs` en una copia privada de la
imagen BIOS y exige, para cada modo, el valor EXACTO que imprime el programa y el
código de salida del proceso. Un fallo del núcleo debe hacer FALLAR la sonda.

Modos y predicados (todos medidos, ninguno acepta un mensaje genérico):

  directo N        `[T034-AP] cpu=1 exit_group=N` y `PROC: exit ... code=N`
  dormido N        `[T034-LIDER] exit(2) antes del cierre del grupo`,
                   `[T034-AP] cpu=1 exit_group=N` y `code=N`
  wait N           `[T034-WAIT] code=N esperado=E observado=E pid_correcto=true OK`
  wait-dormido N   `[T034-WAIT-DORMIDO] ... OK`
  sigkill          `[T034-SIGKILL] esperado=9 observado=9 pid_correcto=true OK`
  sigkill-hilos    `[T034-SIGKILL-HILOS] ... esperado=9 observado=9 pid_correcto=true
                   sin_proceso=true OK`
  control          modo inválido -> código 2 (la sonda puede fallar)

En cualquier modo la aparición de `[T034-ERROR]` anula el resultado.
"""
import argparse
import hashlib
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

from sonda_t035_uefi import Vm, DEBUGFS, ext2_debugfs  # noqa: E402
from sonda_imagen_t029 import localizar_ext2  # noqa: E402
from rutas_imagenes import copia_privada  # noqa: E402

FUENTE = ROOT / "scripts/sondas/t034_exit_group.rs"
BINARIO = ROOT / "build/packages/t034_exit_group"
IMAGEN = ROOT / "build/dist/portix.img"
NOMBRE = "/usr/local/t034"

# (etiqueta, argumentos, patrón exigido, código de salida exigido)
CASOS = [
    ("directo", ["directo", "42"], r"\[T034-AP\] cpu=1 exit_group=42", 42),
    ("dormido", ["dormido", "7"], r"\[T034-AP\] cpu=1 exit_group=7", 7),
    ("wait", ["wait", "42"], r"\[T034-WAIT\] code=42 esperado=10752 observado=10752 pid_correcto=true OK", 0),
    ("wait-dormido", ["wait-dormido", "7"], r"\[T034-WAIT-DORMIDO\] .* esperado=1792 observado=1792 pid_correcto=true OK", 0),
    ("sigkill", ["sigkill"], r"\[T034-SIGKILL\] esperado=9 observado=9 pid_correcto=true OK", 0),
    ("sigkill-hilos", ["sigkill-hilos"],
     r"\[T034-SIGKILL-HILOS\] .* esperado=9 observado=9 pid_correcto=true sin_proceso=true OK", 0),
]
# El líder dormido debe imprimir su línea antes del cierre del grupo.
EXTRA = {"dormido": r"\[T034-LIDER\] exit\(2\) antes del cierre del grupo",
         "wait-dormido": r"\[T034-LIDER\] exit\(2\) antes del cierre del grupo"}


def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for t in iter(lambda: f.read(1 << 20), b""):
            h.update(t)
    return h.hexdigest()


def compilar():
    if not FUENTE.is_file():
        return None, "falta la fuente del fixture"
    r = subprocess.run(["rustc", "-O", "-C", "target-feature=+crt-static",
                        "-C", "strip=symbols", str(FUENTE), "-o", str(BINARIO)],
                       capture_output=True, text=True, timeout=300)
    if r.returncode != 0 or not BINARIO.is_file():
        return None, (r.stderr or "rustc falló")[-800:]
    return BINARIO, ""


def ejecutar(vm, argumentos, limite=90):
    """Envía `run <args>` y devuelve (texto, código_de_salida o None)."""
    marca = len(vm.log)
    vm.enviar("run " + " ".join([NOMBRE] + list(argumentos)))
    fin = time.monotonic() + limite
    lider = None
    while time.monotonic() < fin:
        vm.leer(1)
        texto = vm.log[marca:].decode("utf-8", "replace")
        if lider is None:
            m = re.search(r"PROC: create PID=(\d+) name='[^']*t034'", texto)
            if m:
                lider = int(m.group(1))
                continue
        if lider is not None:
            # Código de salida DEL PROCESO LANZADO, no del hilo ni de un hijo
            # asesinado: se exige el mismo PID que anunció `PROC: create`.
            m = re.search(rf"PROC: exit PID={lider} name='[^']*t034' code=(\d+)", texto)
            if m:
                vm.leer(1)
                return vm.log[marca:].decode("utf-8", "replace"), int(m.group(1))
        if "[T034-ERROR]" in texto and re.search(r"PROC: exit PID=", texto):
            break
    return vm.log[marca:].decode("utf-8", "replace"), None


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--limite", type=int, default=90, help="segundos por caso")
    ap.add_argument("--casos", default="", help="lista separada por comas")
    ap.add_argument("--arranque", type=int, default=420, help="segundos para llegar a la consola")
    ap.add_argument("--conservar", action="store_true", help="no borrar la copia privada")
    args = ap.parse_args()

    if DEBUGFS is None:
        print("[FAIL] falta debugfs: no se puede inyectar el fixture", flush=True)
        return 1
    if not IMAGEN.is_file():
        print(f"[FAIL] falta la imagen {IMAGEN}", flush=True)
        return 1
    binario, error = compilar()
    if binario is None:
        print(f"[FAIL] no se pudo compilar el fixture: {error}", flush=True)
        return 1
    print(f"[INFO] BIOS={IMAGEN.name} SHA-256={sha256(IMAGEN)}", flush=True)
    print(f"[INFO] fixture SHA-256={sha256(BINARIO)} bytes={BINARIO.stat().st_size}", flush=True)

    seleccion = [c for c in CASOS if not args.casos or c[0] in args.casos.split(",")]

    copia = copia_privada(IMAGEN)
    total = None
    try:
        offset, total = localizar_ext2(copia.open("rb").read(512), copia.stat().st_size)
        ext2_debugfs(copia, offset, f"rm {NOMBRE}")
        ext2_debugfs(copia, offset, f"write {BINARIO} {NOMBRE}")
        estado = subprocess.run([DEBUGFS, "-R", f"stat {NOMBRE}", f"{copia}?offset={offset}"],
                                capture_output=True, text=True, timeout=60).stdout
        if not re.search(r"^Inode:\s*[1-9]\d*\s+Type:\s*regular", estado, re.MULTILINE):
            print("[FAIL] el fixture no quedó como fichero regular en el ext2", flush=True)
            return 1
        print(f"[PASS] fixture inyectado en el ext2 MBR (offset {offset})", flush=True)

        vm = Vm(copia, None)
        aprobados = 0
        try:
            espera = args.arranque
            # El prompt no se reimprime hasta que llega entrada: se espera el
            # escritorio y la carga de init, y se envía una línea vacía. El
            # veredicto NO depende del eco del prompt.
            if not vm.esperar("present done", espera):
                print(f"[FAIL] la imagen BIOS no llegó al escritorio en {espera} s", flush=True)
                print(vm.log.decode("utf-8", "replace")[-1200:], flush=True)
                return 1
            if not vm.esperar(r"\[ELF\] loaded init", 60):
                print("[FAIL] init (PID 1) no quedó cargado", flush=True)
                print(vm.log.decode("utf-8", "replace")[-800:], flush=True)
                return 1
            vm.enviar("")
            if not vm.esperar(r"root@portix:", 30):
                print("[INFO] el prompt no se reimprimió sin entrada; se sigue enviando "
                      "órdenes por la consola serie", flush=True)
            if False:
                print(f"[FAIL] la imagen BIOS no llegó a la consola en {espera} s", flush=True)
                cola = vm.log.decode("utf-8", "replace")[-1200:]
                print("--- últimas líneas de la consola ---", flush=True)
                print(cola, flush=True)
                return 1
            print("[PASS] arranque BIOS hasta la consola", flush=True)
            vm.enviar("")
            vm.leer(1)

            for etiqueta, argumentos, patron, esperado in seleccion:
                texto, codigo = ejecutar(vm, argumentos, args.limite)
                hay_error = "[T034-ERROR]" in texto
                patron_ok = re.search(patron, texto) is not None
                extra_ok = True
                if etiqueta in EXTRA:
                    extra_ok = re.search(EXTRA[etiqueta], texto) is not None
                salida_ok = codigo == esperado
                ok = patron_ok and extra_ok and salida_ok and not hay_error
                aprobados += 1 if ok else 0
                detalle = []
                if not patron_ok:
                    detalle.append(f"patrón ausente ({patron})")
                if not extra_ok:
                    detalle.append("falta la línea del líder dormido")
                if not salida_ok:
                    detalle.append(f"code={codigo}, esperado {esperado}")
                if hay_error:
                    m = re.search(r"\[T034-ERROR\][^\r\n]*", texto)
                    detalle.append("error del fixture: " + (m.group(0) if m else "?"))
                print(f"[{'PASS' if ok else 'FAIL'}] T-031 {etiqueta}: "
                      f"{'observaciones exactas y salida ' + str(esperado) if ok else '; '.join(detalle)}",
                      flush=True)
                if not ok:
                    print(texto[-1500:], flush=True)

            # Control negativo: el propio fixture debe poder fallar.
            texto, codigo = ejecutar(vm, ["modo-invalido"], 40)
            control_ok = codigo == 2 and "uso: t034_exit_group" in texto
            aprobados += 1 if control_ok else 0
            print(f"[{'PASS' if control_ok else 'FAIL'}] T-031 control negativo: modo inválido -> "
                  f"code={codigo} (esperado 2) con el uso impreso", flush=True)

            # El núcleo sigue vivo después de todos los cierres de grupo.
            vm.enviar("run /bin/true")
            vivo = vm.esperar(r"name='/bin/true' code=0", 60)
            aprobados += 1 if vivo else 0
            print(f"[{'PASS' if vivo else 'FAIL'}] T-031 cierre: la consola sigue viva y lanza "
                  f"/bin/true con code=0 tras los cierres de grupo", flush=True)
        finally:
            vm.detener()
    finally:
        if not args.conservar:
            copia.unlink(missing_ok=True)

    casos = len(seleccion) + 2
    print(f"SONDA T-031: {aprobados}/{casos} PASS; imagen={IMAGEN.name} "
          f"SHA-256={sha256(IMAGEN)}; no certifica b203 por sí sola", flush=True)
    return 0 if aprobados == casos else 1


if __name__ == "__main__":
    sys.exit(main())
