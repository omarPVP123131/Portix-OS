#!/usr/bin/env python3
"""Comprueba en QEMU que /proc/smp publica la instrumentación de T-026.

No certifica nada: verifica que el núcleo mide
  - las ventanas con IF=0 (`ventanas_if0: abiertas/peor_us/pid/abi/nr/rip_in/rip_out`),
  - si el peor hueco del PIT cayó dentro de una (irq0_hueco_dentro) o fuera,
  - el tick en que ocurrió el peor hueco (irq0_hueco_tick),
y que los valores son coherentes (peor_us > 0, abiertas > 0, rip dentro del núcleo).
"""
import argparse
import re
import sys
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
    from .rutas_imagenes import dentro_de_dist
else:
    from headless_runner import PortixHeadlessRunner
    from rutas_imagenes import dentro_de_dist

ROOT = Path(__file__).resolve().parents[1]

CAMPOS = {
    "irq0_perdidos": r"irq0_perdidos: (\d+)",
    "irq0_peor_hueco_us": r"irq0_peor_hueco_us: (\d+)",
    "irq0_hueco_dentro": r"irq0_hueco_dentro: (\S+)",
    "irq0_hueco_tick": r"irq0_hueco_tick: (\d+)",
    "irq0_hueco_ventana_us": r"irq0_hueco_ventana_us: (\d+)",
    "ventanas_abiertas": r"ventanas_if0: abiertas (\d+)",
    "ventana_peor_us": r"ventanas_if0: .*?peor_us (\d+)",
    "ventana_pid": r"ventanas_if0: .*?pid (\d+)",
    "ventana_abi": r"ventanas_if0: .*?abi (\w+)",
    "ventana_nr": r"ventanas_if0: .*?nr (\d+)",
    "ventana_a1": r"ventanas_if0: .*?a1 (0x[0-9a-fA-F]+)",
    "ventana_a2": r"ventanas_if0: .*?a2 (0x[0-9a-fA-F]+)",
    "ventana_a3": r"ventanas_if0: .*?a3 (0x[0-9a-fA-F]+)",
    "ventana_rip_in": r"ventanas_if0: .*?rip_in (0x[0-9a-fA-F]+)",
    "ventana_rip_out": r"ventanas_if0: .*?rip_out (0x[0-9a-fA-F]+)",
    # El tramo CONTINUO con IF=0 es lo que de verdad mide el ticket: entre dos
    # `respirar()` la CPU no atiende ninguna IRQ. `ventana_peor_us` mide la
    # syscall entera (que puede tener decenas de respiros dentro).
    "respiros_abiertos": r"respiros: abiertos (\d+)",
    "respiro_peor_tramo_us": r"respiros: .*?peor_tramo_us (\d+)",
    "respiro_sitio": r"respiros: .*?peor_tramo_us \d+ en (\S+)",
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--img", required=True, type=Path)
    ap.add_argument("--serial", type=Path, default=None, help="volcar la consola serie completa aquí (evidencia)")
    args = ap.parse_args()
    imagen = args.img.resolve()
    if not dentro_de_dist(imagen) or not imagen.is_file():
        print("[FAIL] imagen fuera de build/dist o inexistente", flush=True)
        return 1
    r = None
    fallos = []
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=False, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        ok, _ = r.wait_for_pattern("init process PID=1 registered", timeout=60)
        if not ok:
            print("[FAIL] no arrancó init", flush=True)
            return 1
        # [T-026] El binario nativo entra por `int 0x80`, cuya puerta hace `sti`:
        # corre con IF=1 y NO abre ventana. Hacen falta syscalls de la ABI LINUX
        # (instrucción `syscall`, IF=0 por FMASK) para medir de verdad, así que
        # se ejecuta el fixture Rust estático (ELF Linux) con hilos y futex.
        binario = ROOT / "build/packages/t034_exit_group"
        if not binario.is_file():
            print("[FAIL] falta build/packages/t034_exit_group (rustc +crt-static)", flush=True)
            return 1
        size = binario.stat().st_size
        if not r.execute_and_verify(
                "run curl -o /usr/local/t034 http://10.0.2.2:8000/t034_exit_group",
                f"guardado exitosamente en '/usr/local/t034' ({size} bytes)",
                "descarga del fixture Linux para medir ventanas", timeout=60):
            print("[FAIL] no se pudo descargar el fixture", flush=True)
            return 1
        # [T-043] La reproducción mínima del bucle caro de `sys_mmap`: MAP_FIXED
        # sobre 8 MB ya materializados. Se mide su propio tiempo y el aumento del
        # tramo continuo con IF=0 publicado por `/proc/smp`.
        fx = ROOT / "build/packages/t026_mmapfixed"
        if not fx.is_file():
            print("[FAIL] falta build/packages/t026_mmapfixed (rustc +crt-static)", flush=True)
            return 1
        if not r.execute_and_verify(
                "run curl -o /usr/local/t026fx http://10.0.2.2:8000/t026_mmapfixed",
                f"guardado exitosamente en '/usr/local/t026fx' ({fx.stat().st_size} bytes)",
                "descarga del fixture MAP_FIXED", timeout=60):
            print("[FAIL] no se pudo descargar el fixture MAP_FIXED", flush=True)
            return 1
        idx = r.get_log_len()
        r.send_command("cat /proc/smp")
        r.wait_for_pattern("respiros: abiertos", start_idx=idx, timeout=20)
        with r.log_lock:
            base = re.search(r"respiros: abiertos \d+ saltados_irqsave \d+ peor_tramo_us (\d+)", r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace"))
        tramo_base = int(base.group(1)) if base else 0
        idx = r.get_log_len()
        r.send_command("run /usr/local/t026fx")
        if not r.wait_for_pattern("[T026-OK] fin", start_idx=idx, timeout=90):
            print("[FAIL] el fixture MAP_FIXED no terminó", flush=True)
            return 1
        r.wait_for_pattern("root@portix:", start_idx=idx, timeout=20)
        with r.log_lock:
            texto_fx = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
        mfx = re.search(r"\[T026\] MAP_FIXED sobre (\d+) paginas materializadas: (\d+) us", texto_fx)
        mmu = re.search(r"\[T026\] munmap de (\d+) paginas materializadas: (\d+) us", texto_fx)
        idx = r.get_log_len()
        r.send_command("cat /proc/smp")
        r.wait_for_pattern("respiros: abiertos", start_idx=idx, timeout=20)
        with r.log_lock:
            desp = re.search(r"respiros: abiertos \d+ saltados_irqsave \d+ peor_tramo_us (\d+)", r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace"))
        tramo_desp = int(desp.group(1)) if desp else 0
        print(f"[INFO][T-043] MAP_FIXED 8 MB: fixture={mfx.group(2) if mfx else '?'} us; "
              f"munmap 8 MB: {mmu.group(2) if mmu else '?'} us; "
              f"tramo continuo con IF=0 antes={tramo_base} us despues={tramo_desp} us", flush=True)
        if mfx is None:
            fallos.append("el fixture no imprimió su tiempo de MAP_FIXED")
        else:
            exigir_t043 = int(mfx.group(2))
            # Antes del arreglo: 2048 páginas → 90 000 us (44 us/página, un
            # shootdown con acuse por página y otro por `mapear_perezosa`).
            # Con el lote de 128 páginas: ~5 000 us.
            if mmu is None:
                fallos.append("el fixture no imprimió su tiempo de munmap")
            elif int(mmu.group(2)) >= 30_000:
                fallos.append(f"munmap de 8 MB tarda {mmu.group(2)} us (umbral 30 000): el shootdown por página ha vuelto en munmap")
            if exigir_t043 >= 30_000:
                fallos.append(f"MAP_FIXED sobre 8 MB tarda {exigir_t043} us (umbral 30 000): el shootdown por página ha vuelto")
            # El tramo global es informativo: depende de lo que ya hubiera hecho
            # el arranque. Lo que discrimina es el tiempo del propio MAP_FIXED.

        for cmd in ("run /usr/local/t034 futex-lider", "run /usr/local/t034 directo 42",
                    "ping 10.0.2.2"):
            r.send_command(cmd)
            r.wait_for_pattern("root@portix:", timeout=25)
        idx = r.get_log_len()
        r.send_command("cat /proc/smp")
        ok, _ = r.wait_for_pattern("ventanas_if0:", start_idx=idx, timeout=20)
        if not ok:
            print("[FAIL] /proc/smp no publica `ventanas_if0:` (instrumentación ausente)", flush=True)
            return 1
        with r.log_lock:
            texto = r.log[idx:]
        v = {}
        for nombre, pat in CAMPOS.items():
            m = re.search(pat, texto)
            v[nombre] = m.group(1) if m else None
        print("[INFO] medido en /proc/smp:")
        for k, val in v.items():
            print(f"    {k:22} {val}")

        def exigir(cond, texto_fallo):
            if not cond:
                fallos.append(texto_fallo)

        exigir(v["ventanas_abiertas"] is not None and int(v["ventanas_abiertas"]) > 0,
               "no se abrió ninguna ventana con IF=0 (contador a 0)")
        exigir(v["ventana_peor_us"] is not None and int(v["ventana_peor_us"]) > 0,
               "la peor ventana con IF=0 mide 0 µs")
        exigir(v["ventana_abi"] in ("nativa", "linux"),
               f"ABI de la peor ventana no reconocida: {v['ventana_abi']!r}")
        rip = v["ventana_rip_in"]
        exigir(rip is not None and int(rip, 16) >= 0xffffffff80000000,
               f"rip_in de la peor ventana fuera del núcleo: {rip!r}")
        # [T-026] El número que de verdad decide: tramo continuo sin IRQ.
        exigir(v["respiro_peor_tramo_us"] is not None,
               "/proc/smp no publica `respiros: … peor_tramo_us`")
        if v["respiro_peor_tramo_us"] is not None:
            tramo = int(v["respiro_peor_tramo_us"])
            print(f"[INFO] tramo continuo mas largo con IF=0: {tramo} us en {v['respiro_sitio']}")
            exigir(tramo < 200_000,
                   f"tramo continuo con IF=0 de {tramo} us (umbral 200 000) en {v['respiro_sitio']}")
        # [T-043] Informativo, NO fallo: la syscall entera puede tardar segundos
        # con respiros dentro; es lentitud, no pérdida de interrupciones.
        if v["ventana_peor_us"] and int(v["ventana_peor_us"]) >= 200_000:
            print(f"[INFO][T-043] la syscall mas lenta (nr={v['ventana_nr']} a1={v['ventana_a1']} "
                  f"a2={v['ventana_a2']} a3={v['ventana_a3']}) tarda {v['ventana_peor_us']} us "
                  f"de pared (abierto en T-043; su tramo continuo esta acotado por los respiros)")
        exigir(v["irq0_hueco_dentro"] in ("0", "1", "sin_medir"),
               f"irq0_hueco_dentro no publica veredicto: {v['irq0_hueco_dentro']!r}")
        if v["irq0_hueco_dentro"] in ("0", "1"):
            exigir(v["irq0_hueco_tick"] is not None,
                   "hay veredicto de hueco pero no el tick en que ocurrió")
        # Coherencia: si el hueco cayó DENTRO de un syscall con IF=0, la ventana
        # tuvo que estar abierta al menos tanto como el propio hueco.
        if v["irq0_hueco_dentro"] == "1" and v["irq0_hueco_ventana_us"] and v["irq0_peor_hueco_us"]:
            exigir(int(v["irq0_hueco_ventana_us"]) >= int(v["irq0_peor_hueco_us"]),
                   "dentro=1 pero la ventana era más corta que el hueco: medida incoherente")
        if fallos:
            for f in fallos:
                print(f"[FAIL] {f}", flush=True)
            return 1
        print("[PASS] T-026: la instrumentación de ventanas IF=0 publica valores coherentes", flush=True)
        return 0
    finally:
        if r is not None:
            if args.serial is not None:
                try:
                    with open(args.serial, "w", encoding="utf-8", errors="replace") as fh:
                        fh.write(r.log if isinstance(r.log, str) else r.log.decode("utf-8", "replace"))
                    print(f"[INFO] consola serie volcada en {args.serial}", flush=True)
                except Exception as e:  # noqa: BLE001
                    print(f"[INFO] no se pudo volcar la serie: {e}", flush=True)
            r.close()


if __name__ == "__main__":
    raise SystemExit(main())
