#!/usr/bin/env python3
"""[CICLO 49] Sonda de arranque UEFI real (QEMU + OVMF).

Arranca build/dist/portix-uefi.img con el firmware OVMF y exige, por el puerto
serie, que el cargador EFI entregue el control, que el núcleo ejecute en la
mitad superior ([HH] ...) y que llegue al escritorio ("present done").
Si no hay OVMF instalado la sonda FALLA (no se finge el arranque UEFI).

Salida final: "SONDA UEFI: n/3 comprobaciones PASS".
"""
import os, subprocess, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
from rutas_imagenes import copia_privada

def find_ovmf():
    for c in ("/usr/share/ovmf/OVMF.fd", "/usr/share/OVMF/OVMF.fd", "/usr/share/qemu/OVMF.fd",
              "/usr/share/edk2/ovmf/OVMF_CODE.fd", str(ROOT / "tools" / "OVMF.fd")):
        if Path(c).is_file():
            return c
    return None

def main():
    img = ROOT / "build" / "dist" / "portix-uefi.img"
    ovmf = find_ovmf()
    comprobaciones = [
        ("EFI:entry", "cargador portix-efi-loader (BOOTX64.EFI) toma el control"),
        ("[HH] nucleo en mitad superior: rip=0xFFFFFFFF80", "nucleo en la mitad superior arrancado por UEFI"),
        ("present done", "escritorio presentado (arranque UEFI completo)"),
        # T-025: bajo OVMF la RSDP llega por la tabla de firmware del cargador y el AP arranca igual.
        ("[SMP] AP 1 (APIC ID 1): en linea, ve su APIC ID 1, IPI 0xF0 contador 0 -> 1 OK", "SMP real bajo UEFI: AP 1 en linea y atiende la IPI 0xF0"),
    ]
    print("=" * 72)
    print("  SONDA DE ARRANQUE UEFI (OVMF)")
    print("=" * 72)
    if ovmf is None or not img.exists():
        print(f"  [FALLO] OVMF={ovmf} imagen={img.exists()}")
        print("\nSONDA UEFI: 0/4 comprobaciones PASS")
        return 1
    try:
        copia = copia_privada(img)
    except (OSError, ValueError) as e:
        print(f"  [FALLO] No se pudo aislar el disco UEFI: {e}")
        print("\nSONDA UEFI: 0/4 comprobaciones PASS")
        return 1
    cmd = ["qemu-system-x86_64", "-bios", ovmf, "-m", "512M", "-smp", "2",
           "-drive", f"file={copia},format=raw,if=ide,index=0,media=disk",
           "-display", "none", "-serial", "stdio", "-no-reboot",
           "-netdev", "user,id=u1", "-device", "e1000,netdev=u1"]
    print("  $ " + " ".join(cmd[:4]) + " ...")
    inicio = time.time()
    buf = b""
    p = None
    terminado = True
    try:
        p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
        os.set_blocking(p.stdout.fileno(), False)
        while time.time() - inicio < 120:
            try:
                chunk = p.stdout.read(65536)
            except (BlockingIOError, TypeError):
                chunk = None
            if chunk:
                buf += chunk
                if b"present done" in buf:
                    break
            elif p.poll() is not None:
                break
            else:
                time.sleep(0.2)
    except OSError as e:
        print(f"  [FALLO] No se pudo iniciar QEMU: {e}")
    finally:
        if p is not None:
            try:
                if p.poll() is None:
                    p.kill()
                p.wait(timeout=3)
            except Exception as e:
                terminado = False
                print(f"  [FALLO] QEMU sigue activo; copia conservada en {copia}: {e}")
        if terminado:
            try:
                copia.unlink()
            except OSError as e:
                terminado = False
                print(f"  [FALLO] No se pudo retirar la copia UEFI: {e}")
    texto = buf.decode("utf-8", "replace")
    (ROOT / "build" / ".uefi_probe.log").write_text(texto)
    passes = 0
    for patron, etiqueta in comprobaciones:
        ok = patron in texto
        passes += 1 if ok else 0
        print(f"  [{'PASS' if ok else 'FALLO'}] {etiqueta}")
    print(f"\nSONDA UEFI: {passes}/{len(comprobaciones)} comprobaciones PASS")
    return 0 if passes == len(comprobaciones) and terminado else 1

if __name__ == "__main__":
    sys.exit(main())
