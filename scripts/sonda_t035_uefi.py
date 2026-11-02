#!/usr/bin/env python3
"""T-035 bajo UEFI real (QEMU + OVMF), con el fixture inyectado en el ext2.

Complementa a `uefi_probe.py` (sólo arranque). Arranca `portix-uefi.img` con
OVMF, ejecuta los modos del fixture por la consola serie y los valida con los
MISMOS predicados que la sonda BIOS. El ELF se escribe en la partición GPT+ext2
con `debugfs` sobre una COPIA PRIVADA (nunca sobre la imagen de build/dist).

Transporte por defecto: inyección directa. `--transporte red` usa `curl` dentro
del invitado, que es una ruta distinta y puede fallar por defecto propios.

Si falta OVMF o la imagen, FALLA: no se finge una prueba UEFI.
"""
import argparse
import hashlib
import os
import re
import shutil
import struct
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from rutas_imagenes import copia_privada, dentro_de_dist  # noqa: E402
from sonda_ata_b203 import sha256  # noqa: E402
from verificaciones_t035_memoria import MODOS, NOMBRE, resultado_vm  # noqa: E402

BINARIO = ROOT / "build/packages/t035_memoria"
FUENTE = ROOT / "scripts/sondas/t035_memoria.rs"
UEFI = ROOT / "build/dist/portix-uefi.img"
BIOS = ROOT / "build/dist/portix.img"
# Etiqueta del firmware en curso: la misma batería se ejecuta en ambos.
ETQ = "UEFI"
DEBUGFS = shutil.which("debugfs") or next(
    (p for p in ("/usr/sbin/debugfs", "/sbin/debugfs") if Path(p).is_file()), None)


def ovmf():
    for c in ("/usr/share/ovmf/OVMF.fd", "/usr/share/OVMF/OVMF.fd",
              "/usr/share/qemu/OVMF.fd", "/usr/share/edk2/ovmf/OVMF_CODE.fd"):
        if Path(c).is_file():
            return c
    return None


def ext2_gpt(imagen):
    """(offset, tamaño) de la partición ext2 en una imagen GPT. Exige magic."""
    with open(imagen, "rb") as f:
        f.seek(512)
        hdr = f.read(92)
        if hdr[:8] != b"EFI PART":
            raise ValueError("cabecera GPT ausente")
        lba = struct.unpack_from("<Q", hdr, 72)[0]
        n, sz = struct.unpack_from("<I", hdr, 80)[0], struct.unpack_from("<I", hdr, 84)[0]
        f.seek(lba * 512)
        tabla = f.read(n * sz)
    mejor = None
    for i in range(n):
        e = tabla[i * sz:(i + 1) * sz]
        if e[0:16] == bytes(16):
            continue
        inicio, fin = struct.unpack_from("<QQ", e, 32)
        if inicio and fin >= inicio:
            mejor = (inicio * 512, (fin - inicio + 1) * 512)
    if mejor is None:
        raise ValueError("GPT sin partición de datos")
    with open(imagen, "rb") as f:
        f.seek(mejor[0] + 1024 + 56)
        if f.read(2) != b"\x53\xef":
            raise ValueError("la partición de datos no es ext2")
    return mejor


def ext2_debugfs(imagen, offset, *ordenes):
    destino = f"{imagen}?offset={offset}"
    for orden in ordenes:
        r = subprocess.run([DEBUGFS, "-w", "-R", orden, destino],
                           capture_output=True, text=True, timeout=60,
                           env=dict(os.environ, LC_ALL="C"))
        if r.returncode != 0:
            raise RuntimeError(f"debugfs {orden}: {r.stdout}{r.stderr}")


class Vm:
    def __init__(self, imagen, firmware):
        arranque = ["-bios", firmware] if firmware else []   # sin firmware = BIOS legacy
        self.proc = subprocess.Popen(
            ["qemu-system-x86_64", *arranque, "-m", "512M", "-smp", "2",
             "-drive", f"file={imagen},format=raw,if=ide,index=0,media=disk",
             "-display", "none", "-serial", "stdio", "-no-reboot",
             "-netdev", "user,id=u1", "-device", "e1000,netdev=u1"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        os.set_blocking(self.proc.stdout.fileno(), False)
        self.log = b""

    def leer(self, segundos):
        fin = time.monotonic() + segundos
        while time.monotonic() < fin:
            try:
                trozo = self.proc.stdout.read(65536)
            except (BlockingIOError, TypeError):
                trozo = None
            if trozo:
                self.log += trozo
                fin = time.monotonic() + 0.4
            elif self.proc.poll() is not None:
                return
            else:
                time.sleep(0.05)

    def esperar(self, patron, segundos):
        fin = time.monotonic() + segundos
        while time.monotonic() < fin:
            if re.search(patron.encode(), self.log):
                return True
            self.leer(0.5)
        return bool(re.search(patron.encode(), self.log))

    def enviar(self, linea):
        self.proc.stdin.write((linea + "\n").encode())
        self.proc.stdin.flush()

    def detener(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=3)


def probar(vm, modo, limite):
    inicio = len(vm.log)
    vm.enviar(f"run {NOMBRE} {modo}")
    fin = time.monotonic() + limite
    while time.monotonic() < fin:
        vm.leer(0.5)
        texto = vm.log[inicio:].decode("utf-8", "replace")
        if "PROC: exit PID=" in texto and "root@portix:" in texto.split("PROC: exit PID=", 1)[1]:
            break
    texto = vm.log[inicio:].decode("utf-8", "replace")
    ok, etapas_ok, detalle = resultado_vm(texto, modo)
    print(f"[{'PASS' if ok else 'FAIL'}] T-035 {ETQ} {modo}: "
          f"código y etapas {'exactos' if etapas_ok else 'distintos'}; {detalle}", flush=True)
    if not ok:
        print(texto[-3000:], flush=True)
    return ok


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--modos", default="protegida-materializada,pila-protegida,fork,"
                                       "perezosa,control-perezosa-desmapeada,pila-creciente")
    ap.add_argument("--limite", type=int, default=25)
    ap.add_argument("--transporte", choices=("inyeccion", "red"), default="inyeccion")
    ap.add_argument("--firmware", choices=("uefi", "bios"), default="uefi",
                    help="misma batería sobre OVMF (uefi) o BIOS legacy (bios)")
    args = ap.parse_args()
    modos = tuple(args.modos.split(","))
    if not modos or len(set(modos)) != len(modos) or any(m not in MODOS for m in modos):
        print("[FAIL] modos inválidos o duplicados", flush=True)
        return 2
    global ETQ
    es_uefi = args.firmware == "uefi"
    ETQ = "UEFI" if es_uefi else "BIOS"
    firmware = ovmf() if es_uefi else None
    imagen = UEFI if es_uefi else BIOS
    if (es_uefi and firmware is None) or not imagen.is_file() or DEBUGFS is None:
        print(f"[FAIL] firmware={ETQ}/{firmware} imagen={imagen.is_file()} debugfs={DEBUGFS}", flush=True)
        print(f"SONDA {ETQ} T-035: 0 casos PASS", flush=True)
        return 1
    print(f"[INFO] {ETQ}={imagen.name} SHA-256={sha256(imagen)}; OVMF={firmware}; "
          f"ELF={sha256(BINARIO)}; fuente={sha256(FUENTE)}; transporte={args.transporte}; "
          f"modos={','.join(modos)}", flush=True)

    copia = copia_privada(imagen)
    if not dentro_de_dist(copia):
        print("[FAIL] la copia privada quedó fuera de build/dist", flush=True)
        return 1
    if es_uefi:
        offset, _ = ext2_gpt(copia)
    else:
        from sonda_imagen_t029 import localizar_ext2
        # `localizar_ext2` ya devuelve BYTES de desplazamiento.
        offset, _tam = localizar_ext2(copia.open("rb").read(512), copia.stat().st_size)
    print(f"[INFO] ext2 {'GPT' if es_uefi else 'MBR'} en offset {offset} de la copia privada", flush=True)
    if args.transporte == "inyeccion":
        ext2_debugfs(copia, offset, f"write {BINARIO} {NOMBRE}")
        estado = subprocess.run([DEBUGFS, "-R", f"stat {NOMBRE}",
                                 f"{copia}?offset={offset}"],
                                capture_output=True, text=True, timeout=30).stdout
        if not re.search(r"^Inode:\s*[1-9]\d*\s+Type:\s*regular", estado, re.MULTILINE):
            print("[FAIL] el fixture no quedó como fichero regular en el ext2", flush=True)
            return 1
        print(f"[PASS] T-035 {ETQ}: fixture inyectado en el ext2 {'GPT' if es_uefi else 'MBR'} (copia privada)", flush=True)

    vm = Vm(copia, firmware)
    try:
        if not vm.esperar(r"\[HH\]", 90):
            print(f"[FAIL] {ETQ}: el núcleo no arrancó en la mitad superior", flush=True)
            return 1
        if not vm.esperar("present done", 120):
            print(f"[FAIL] {ETQ} no llegó al escritorio", flush=True)
            return 1
        # Bajo UEFI el prompt no se reimprime hasta que llega entrada: se envía
        # una línea vacía. Su ausencia NO invalida la prueba; lo que cuenta es
        # que los comandos se ejecuten y la consola responda.
        vm.enviar("")
        if vm.esperar(r"root@portix:", 40):
            print(f"[PASS] T-035 {ETQ}: núcleo en mitad superior, escritorio y consola", flush=True)
        else:
            print(f"[INFO] {ETQ}: el prompt no se reimprimió sin entrada; se sigue "
                  "enviando comandos por consola serie", flush=True)
        if args.transporte == "red":
            vm.enviar(f"run curl -o {NOMBRE} http://10.0.2.2:8000/{BINARIO.name}")
            if not vm.esperar(f"guardado exitosamente en '{NOMBRE}' "
                              f"\\({BINARIO.stat().st_size} bytes\\)", 90):
                print(f"[FAIL] {ETQ}: curl no descargó el fixture", flush=True)
                return 1
        resultados = [probar(vm, m, args.limite) for m in modos]
    finally:
        vm.detener()

    r = subprocess.run([DEBUGFS, "-R", f"cat {NOMBRE}", f"{copia}?offset={offset}"],
                       capture_output=True, timeout=30, env=dict(os.environ, LC_ALL="C"))
    esperado = BINARIO.read_bytes()
    encontrado = hashlib.sha256(r.stdout).hexdigest()
    integridad = r.returncode == 0 and r.stdout == esperado
    print(f"[{'PASS' if integridad else 'FAIL'}] ELF ext2 {ETQ} idéntico byte a byte; "
          f"SHA-256={encontrado}; bytes={len(r.stdout)}", flush=True)
    aprobados = sum(resultados)
    print(f"SONDA {ETQ} T-035: {aprobados}/{len(modos)} casos PASS; "
          f"integridad_ext2={'PASS' if integridad else 'FAIL'}; no certifica b203", flush=True)
    copia.unlink(missing_ok=True)  # la copia privada no debe acumularse en build/dist
    return 0 if aprobados == len(modos) and integridad else 1


if __name__ == "__main__":
    sys.exit(main())
