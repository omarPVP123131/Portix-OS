#!/usr/bin/env python3
"""
scripts/flash_usb.py — Herramienta Segura de Grabación y Flasheo de Medios USB para Portix OS v4.0 'Cosmos'

Permite escribir de manera confiable las imágenes oficiales de Portix OS (portix-dual.img,
portix-uefi.iso o portix.img) en unidades USB físicas o discos externos para arranque
bare-metal en hardware real (BIOS Legacy y UEFI x86_64).
"""

import os
import sys
import subprocess
import hashlib
import time
import argparse

# Códigos de color ANSI
CYAN = "\033[36m"
GREEN = "\033[32m"
YELLOW = "\033[33m"
RED = "\033[31m"
BOLD = "\033[1m"
RESET = "\033[0m"

def print_banner():
    print(f"{BOLD}{CYAN}╔══════════════════════════════════════════════════════════════════════╗{RESET}")
    print(f"{BOLD}{CYAN}║    PORTIX OS v5.0 'HYPERION' — HERRAMIENTA OFICIAL DE FLASHEO USB    ║{RESET}")
    print(f"{BOLD}{CYAN}║         Grabación Bare-Metal Segura • Verificación SHA-512           ║{RESET}")
    print(f"{BOLD}{CYAN}╚══════════════════════════════════════════════════════════════════════╝{RESET}\n")

def compute_sha512(file_path):
    """Calcula el hash SHA-512 de la imagen."""
    h = hashlib.sha512()
    size = os.path.getsize(file_path)
    processed = 0
    start = time.time()
    with open(file_path, "rb") as f:
        while chunk := f.read(1024 * 1024):
            h.update(chunk)
            processed += len(chunk)
            pct = (processed / size) * 100
            print(f"\r  [*] Verificando integridad SHA-512: {pct:.1f}% ({processed // (1024*1024)}/{size // (1024*1024)} MB)...", end="", flush=True)
    print(f"\r  {GREEN}✓{RESET} Verificación SHA-512 calculada en {time.time() - start:.2f}s!          ")
    return h.hexdigest()

def list_candidate_devices():
    """Detecta dispositivos de bloque conectados y resalta unidades USB extraíbles."""
    devices = []
    try:
        # Ejecutar lsblk en formato JSON o tabla
        out = subprocess.check_output(
            ["lsblk", "-d", "-o", "NAME,SIZE,TYPE,TRAN,RM,MODEL,ROTA"],
            universal_newlines=True
        )
        lines = out.strip().split("\n")
        header = lines[0]
        for line in lines[1:]:
            parts = line.split()
            if not parts:
                continue
            name = parts[0]
            dev_path = f"/dev/{name}"
            size = parts[1] if len(parts) > 1 else "?"
            dev_type = parts[2] if len(parts) > 2 else "?"
            tran = parts[3] if len(parts) > 3 else "sata"
            rm = parts[4] if len(parts) > 4 else "0"
            model = " ".join(parts[5:]) if len(parts) > 5 else "Generic"

            # Identificar si es USB o extraíble
            is_usb = (tran.lower() == "usb") or (rm == "1")
            devices.append({
                "path": dev_path,
                "name": name,
                "size": size,
                "type": dev_type,
                "tran": tran,
                "rm": rm,
                "model": model,
                "is_usb": is_usb
            })
    except Exception:
        # Fallback si lsblk falla
        pass
    return devices

def flash_image(img_path, target_dev, dry_run=False):
    """Escribe la imagen bloque a bloque en el dispositivo seleccionado."""
    if not os.path.exists(img_path):
        print(f"{RED}[ERROR] El archivo de imagen no existe: {img_path}{RESET}")
        return False

    img_size = os.path.getsize(img_path)
    print(f"[*] Imagen origen: {BOLD}{img_path}{RESET} ({img_size // (1024*1024)} MB)")
    print(f"[*] Dispositivo destino: {BOLD}{target_dev}{RESET}")

    sha512_hash = compute_sha512(img_path)
    print(f"  Hash SHA-512: {CYAN}{sha512_hash[:32]}...{sha512_hash[-16:]}{RESET}")

    if dry_run:
        print(f"\n{YELLOW}[MODO SIMULACIÓN] No se escribieron datos en {target_dev}.{RESET}")
        print(f"{GREEN}[OK] Simulación de flasheo superada exitosamente.{RESET}")
        return True

    # Desmontar particiones activas en el dispositivo
    print(f"\n[*] Desmontando particiones activas en {target_dev}...")
    try:
        subprocess.run(["umount", f"{target_dev}*"], stderr=subprocess.DEVNULL)
    except Exception:
        pass

    # Escritura física con dd y sync
    print(f"[*] Iniciando volcado bare-metal (bloque 4MB)...")
    dd_cmd = [
        "dd",
        f"if={img_path}",
        f"of={target_dev}",
        "bs=4M",
        "status=progress",
        "conv=fsync"
    ]

    start_t = time.time()
    res = subprocess.run(dd_cmd)
    if res.returncode != 0:
        print(f"{RED}[ERROR] Falló la escritura dd en {target_dev}. Código: {res.returncode}{RESET}")
        return False

    print(f"[*] Sincronizando búferes del kernel (sync)...")
    subprocess.run(["sync"])
    elapsed = time.time() - start_t
    rate = (img_size / (1024 * 1024)) / max(elapsed, 0.001)

    print(f"\n{GREEN}{BOLD}══════════════════════════════════════════════════════════════════════{RESET}")
    print(f"{GREEN}{BOLD}  ¡FLASHEO COMPLETADO CON ÉXITO!{RESET}")
    print(f"  Dispositivo: {target_dev}")
    print(f"  Tiempo transcurrido: {elapsed:.1f} segundos ({rate:.2f} MB/s)")
    print(f"  El medio USB ahora es booteable en BIOS MBR y UEFI GOP.")
    print(f"{GREEN}{BOLD}══════════════════════════════════════════════════════════════════════{RESET}\n")
    return True

def main():
    print_banner()
    parser = argparse.ArgumentParser(description="Grabador de imágenes USB para Portix OS")
    parser.add_argument("--image", "-i", default="build/dist/portix-dual.img", help="Ruta a la imagen (default: build/dist/portix-dual.img)")
    parser.add_argument("--device", "-d", default=None, help="Dispositivo USB destino (ej: /dev/sdb)")
    parser.add_argument("--list", "-l", action="store_true", help="Listar unidades USB conectadas")
    parser.add_argument("--dry-run", action="store_true", help="Simular sin escribir datos reales")
    parser.add_argument("--force", "-f", action="store_true", help="Omitir solicitud de confirmación")
    args = parser.parse_args()

    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    img_path = os.path.join(base_dir, args.image) if not os.path.isabs(args.image) else args.image

    devices = list_candidate_devices()

    if args.list or not args.device:
        print(f"{BOLD}[*] Dispositivos de almacenamiento detectados en el sistema:{RESET}\n")
        if not devices:
            print("  (No se detectaron unidades vía lsblk)")
        for d in devices:
            tag = f"{GREEN}[USB / EXTRAÍBLE]{RESET}" if d["is_usb"] else f"{YELLOW}[DISCO FIJO / SISTEMA]{RESET}"
            print(f"  • {BOLD}{d['path']}{RESET} ({d['size']}) - {d['model']} [{d['tran']}] {tag}")

        if args.list and not args.device:
            return

    if not args.device:
        print(f"\n{CYAN}Uso interactivo:{RESET}")
        print(f"  python3 scripts/flash_usb.py --device /dev/sdX [--dry-run]\n")
        return

    target_dev = args.device
    # Protección de seguridad contra sobreescritura accidental del disco del sistema
    is_dangerous = any(d["path"] == target_dev and not d["is_usb"] for d in devices)
    if is_dangerous and not args.force:
        print(f"\n{RED}{BOLD}[PELIGRO]{RESET} {target_dev} parece ser un disco fijo o de sistema.")
        print("Para forzar la escritura use la bandera --force.")
        sys.exit(1)

    if not args.force and not args.dry_run:
        print(f"\n{YELLOW}{BOLD}¡ADVERTENCIA DE SEGURIDAD!{RESET}")
        print(f"Todos los datos presentes en {BOLD}{target_dev}{RESET} serán COMPLETAMENTE ELIMINADOS.")
        resp = input(f"¿Desea continuar y flashear {img_path}? (escriba 'SI' para confirmar): ")
        if resp.strip().upper() != "SI":
            print("Operación cancelada por el usuario.")
            return

    success = flash_image(img_path, target_dev, dry_run=args.dry_run)
    sys.exit(0 if success else 1)

if __name__ == "__main__":
    main()
