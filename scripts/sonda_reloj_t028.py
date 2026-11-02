#!/usr/bin/env python3
"""Sonda del reloj y del despertador por tiempo (T-028 / T-015 / T-033).

Pregunta concreta: cuando todo el sistema está ocioso y sólo hay procesos
durmiendo, ¿el reloj del núcleo sigue avanzando al ritmo que marca el
anfitrión? En T-028 dos plazos independientes (`sleep_until` del `curl` y el
tope de 1000 ticks de `tcp_wait_accept`) no vencieron a la vez, lo que sólo se
explica si lo que se paró fue el reloj —o el barrido que despierta a los que
duermen— y no la syscall de turno.

El anfitrión pone la hora; el invitado sólo aporta números que puede leer de
`/proc/smp` (`reloj: ticks …`). Una prueba que no pueda fallar no es prueba:
si los ticks no acompañan al tiempo de pared, esta sonda sale 1.

Uso:  python3 -u scripts/sonda_reloj_t028.py [--img build/dist/portix.img]
"""
import argparse
import os
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from headless_runner import PortixHeadlessRunner  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
BINARIO = ROOT / "build/packages/t028_reloj"


def dentro_de_dist(p) -> bool:
    try:
        p.resolve().relative_to((ROOT / "build/dist").resolve())
        return True
    except ValueError:
        return False


def leer_reloj(r) -> dict:
    """Línea `reloj:` de /proc/smp, tal cual la publica el núcleo."""
    idx = r.get_log_len()
    r.send_command("cat /proc/smp")
    r.wait_for_pattern("reloj: ticks", start_idx=idx, timeout=20)
    with r.log_lock:
        trozo = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
    m = re.search(r"reloj: ticks (\d+) ultimo_barrido (\d+) llamadas (\d+) sin_cerrojo (\d+) "
                  r"seguidos_sin_cerrojo (\d+) despertados (\d+) atascada (\d+)"
                  r"(?: peor_seguidos (\d+) peor_hueco (\d+))?", trozo)
    if not m:
        return {}
    claves = ("ticks", "ultimo_barrido", "llamadas", "sin_cerrojo", "seguidos", "despertados",
              "atascada", "peor_seguidos", "peor_hueco")
    vals = [int(x) if x else 0 for x in m.groups()]
    return {k: v for k, v in zip(claves, vals)}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--img", default=str(ROOT / "build/dist/portix.img"))
    args = ap.parse_args()
    imagen = Path(args.img)
    if not dentro_de_dist(imagen) or not imagen.is_file():
        print("[FAIL] imagen fuera de build/dist o inexistente", flush=True)
        return 1
    if not BINARIO.is_file():
        print("[FAIL] falta build/packages/t028_reloj (rustc +crt-static)", flush=True)
        return 1

    r = None
    try:
        r = PortixHeadlessRunner(str(imagen), verbose=False, silicon=True, conservar=False)
        r.start_http_server(port=8000)
        r.start_qemu()
        ok, _ = r.wait_for_pattern("init process PID=1 registered", timeout=90)
        if not ok:
            print("[FAIL] no arrancó init", flush=True)
            return 1
        if not r.execute_and_verify(
                "run curl -o /usr/local/t028rel http://10.0.2.2:8000/t028_reloj",
                f"guardado exitosamente en '/usr/local/t028rel' ({BINARIO.stat().st_size} bytes)",
                "descarga del fixture del reloj", timeout=60):
            print("[FAIL] no se pudo descargar el fixture", flush=True)
            return 1

        # ── 1. Frecuencia del reloj, medida contra el anfitrión ────────────
        idx = r.get_log_len()
        t_h0 = time.time()
        r.send_command("run /usr/local/t028rel base")
        ok, _ = r.wait_for_pattern("[T028-BASE]", start_idx=idx, timeout=60)
        t_h1 = time.time()
        with r.log_lock:
            texto = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
        m = re.search(r"\[T028-BASE\] ticks por segundo = (\d+)", texto)
        if not ok or not m:
            print("[FAIL] el fixture no midió la frecuencia del reloj", flush=True)
            print(texto[-800:])
            return 1
        hz = int(m.group(1))
        print(f"[INFO] reloj declarado por el invitado: {hz} ticks/s; "
              f"el invitado tardó {t_h1 - t_h0:.2f} s de pared en su segundo", flush=True)
        if hz == 0:
            print("[FAIL] el reloj no dio ni un tick en un segundo de espera", flush=True)
            return 1

        # ── 2. El sistema ocioso con procesos durmiendo ────────────────────
        antes = leer_reloj(r)
        if not antes:
            print("[FAIL] /proc/smp no publica la línea `reloj:`", flush=True)
            return 1
        print(f"[INFO] antes: ticks={antes['ticks']} barridos={antes['llamadas']} "
              f"sin_cerrojo={antes['sin_cerrojo']} despertados={antes['despertados']}", flush=True)
        idx = r.get_log_len()
        t0 = time.time()
        r.send_command("run /usr/local/t028rel reloj 30")
        fin, _ = r.wait_for_pattern("[T028-RELOJ-PASS]", start_idx=idx, timeout=120)
        fallo, _ = r.wait_for_pattern("[T028-FAIL]", start_idx=idx, timeout=1)
        t1 = time.time()
        with r.log_lock:
            texto = r.log[idx:] if isinstance(r.log, str) else r.log.decode("utf-8", "replace")
        despues = leer_reloj(r)

        muestras = [(int(a), int(b), int(c)) for a, b, c, _ in
                    re.findall(r"\[T028-RELOJ\] i=(\d+) ticks=(\d+) delta=(\d+) sin_avanzar=(\d+)", texto)]
        estancadas = sum(1 for s in muestras if s[2] == 0)
        print(f"[INFO] muestras={len(muestras)} sin_avanzar={estancadas} "
              f"pared={t1 - t0:.2f} s fin={fin} fallo={fallo}", flush=True)
        if not muestras:
            print("[FAIL] el fixture no imprimió ni una muestra (se colgó al dormir)", flush=True)
            print(texto[-600:])
            return 1
        mpeor = re.search(r"\[T028-RELOJ-STATS\] iteraciones=(\d+) peor_delta_ticks=(\d+)", texto)
        peor_dormir = int(mpeor.group(2)) if mpeor else -1
        print(f"[INFO] peor espera de 100 ms medida en ticks = {peor_dormir} "
              f"({peor_dormir * 10} ms a 100 Hz)", flush=True)
        delta_ticks = muestras[-1][1] - muestras[0][1]
        esperado = hz * (t1 - t0)
        print(f"[INFO] ticks ganados={delta_ticks} esperados≈{esperado:.0f} "
              f"({100.0 * delta_ticks / esperado if esperado else 0:.0f} % del tiempo de pared)", flush=True)
        if despues:
            print(f"[INFO] después: ticks={despues['ticks']} barridos={despues['llamadas']} "
                  f"sin_cerrojo={despues['sin_cerrojo']} despertados={despues['despertados']} "
                  f"atascada={despues['atascada']}", flush=True)

        fallos = []
        if not fin or fallo:
            fallos.append("el fixture se colgó o declaró el reloj parado")
        if estancadas:
            fallos.append(f"{estancadas} lecturas del reloj sin avanzar")
        if esperado and delta_ticks < 0.5 * esperado:
            fallos.append(f"el reloj perdió tiempo: {delta_ticks} ticks frente a {esperado:.0f} esperados")
        if despues and despues["atascada"] > antes["atascada"]:
            fallos.append("el núcleo registró un barrido de vencimientos atascado")
        hueco = despues.get("peor_hueco", 0) if despues else 0
        racha = despues.get("peor_seguidos", 0) if despues else 0
        print(f"[INFO] peor racha sin cerrojo={racha} ticks; peor hueco entre barridos={hueco} ticks "
              f"({hueco * 10} ms a 100 Hz)", flush=True)
        # Un barrido perdido de vez en cuando no rompe un plazo; una racha larga sí:
        # mientras el barrido no corre, un `sleep_until` vencido no despierta a nadie.
        if racha > 20:
            fallos.append(f"el despertador estuvo {racha} barridos seguidos sin cerrojo")
        if hueco > 50:
            fallos.append(f"{hueco} ticks ({hueco * 10} ms) sin ningún barrido de vencimientos")
        # Una espera de 100 ms que se va a 300 ms o más significa que el que
        # duerme no se despierta cuando toca: misma clase de defecto que T-028.
        if peor_dormir > 30:
            fallos.append(f"una espera de 100 ms tardó {peor_dormir * 10} ms")
        if despues and despues["despertados"] == antes["despertados"] and estancadas:
            fallos.append("nadie fue despertado por tiempo en toda la corrida")

        if fallos:
            print("[FAIL] " + "; ".join(fallos), flush=True)
            return 1
        print("[PASS] con todos los procesos durmiendo, el reloj avanza al ritmo del anfitrión y el "
              "despertador por tiempo toma el cerrojo y despierta", flush=True)
        return 0
    finally:
        if r is not None:
            try:
                r.close()
            except Exception:  # noqa: BLE001
                pass


if __name__ == "__main__":
    sys.exit(main())
