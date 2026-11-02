#!/usr/bin/env python3
"""Previsualización VNC de Portix; usa copia aislada y sólo detiene sus procesos."""

import os
import signal
import subprocess
import sys
import time

if __package__:
    from .rutas_imagenes import copia_privada
else:
    from rutas_imagenes import copia_privada


def detener(proceso, nombre):
    if proceso is None:
        return True
    try:
        if proceso.poll() is None:
            proceso.terminate()
        proceso.wait(timeout=3)
        return True
    except Exception:
        try:
            proceso.kill()
            proceso.wait(timeout=3)
            return True
        except Exception as e:
            print(f"[ERROR] No se pudo detener {nombre}: {e}")
            return False


def main():
    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    imagen = os.path.join(base_dir, "build", "dist", "portix.img")
    if not os.path.isfile(imagen):
        print(f"[ERROR] No se encontró la imagen en {imagen}")
        return 1
    try:
        copia = copia_privada(imagen)
    except (OSError, ValueError) as e:
        print(f"[ERROR] No se pudo aislar la imagen: {e}")
        return 1

    qemu_proc = None
    ws_proc = None
    salida = 1
    def interrumpir(_sig, _frame):
        raise KeyboardInterrupt
    anterior_int = signal.signal(signal.SIGINT, interrumpir)
    anterior_term = signal.signal(signal.SIGTERM, interrumpir)
    try:
        print("[*] Iniciando QEMU con copia privada dentro de build/dist/...")
        qemu_proc = subprocess.Popen([
            "qemu-system-x86_64", "-m", "512M", "-smp", "1",
            "-drive", f"file={copia},format=raw,if=ide,index=0,media=disk,file.locking=off",
            "-netdev", "user,id=u1", "-device", "e1000,netdev=u1",
            "-vnc", "127.0.0.1:0", "-vga", "std", "-serial", "null",
        ], cwd=base_dir, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        time.sleep(1.0)
        if qemu_proc.poll() is not None:
            print("[ERROR] QEMU falló al iniciar.")
            return 1
        ws_proc = subprocess.Popen([
            "websockify", "--web=/usr/share/novnc", "0.0.0.0:6080", "127.0.0.1:5900",
        ], cwd=base_dir, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        time.sleep(1.0)
        if ws_proc.poll() is not None:
            print("[ERROR] Websockify falló al iniciar.")
            return 1
        print("[OK] Live Preview escuchando en 0.0.0.0:6080")
        while qemu_proc.poll() is None and ws_proc.poll() is None:
            time.sleep(1.0)
        print("[ERROR] Uno de los procesos terminó inesperadamente.")
    except KeyboardInterrupt:
        print("\n[*] Deteniendo Live Preview...")
        salida = 0
    except OSError as e:
        print(f"[ERROR] No se pudo iniciar Live Preview: {e}")
    finally:
        detener(ws_proc, "websockify")
        qemu_detenido = detener(qemu_proc, "QEMU")
        if qemu_detenido:
            try:
                copia.unlink()
            except OSError as e:
                print(f"[ERROR] No se pudo retirar la copia: {e}")
                salida = 1
        else:
            print(f"[ERROR] Se conserva la imagen de una VM activa: {copia}")
            salida = 1
        signal.signal(signal.SIGINT, anterior_int)
        signal.signal(signal.SIGTERM, anterior_term)
    return salida


if __name__ == "__main__":
    sys.exit(main())
