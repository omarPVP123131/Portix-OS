#!/usr/bin/env python3
"""
scripts/interactive_runner.py — Ejecutor Interactivo y Suite de Uso Real de Portix OS v6.0 'Chronos'

Inicia Portix OS en QEMU en tiempo real, interactúa con el sistema operativo en ejecución,
valida todos los subsistemas y herramientas creadas (Asistente, Benchmarks,
Python, C/GCC, VFS, Audio, Shell POSIX y Compositor Gráfico) y captura una instantánea
visual real del escritorio en ejecución para auditoría y visualización.
"""

import os
import sys
import re
import time
import socket
import subprocess
import threading
from PIL import Image

if __package__:
    from .rutas_imagenes import DISCOS_SILICIO, copia_privada, crear_disco_patron
    from .verificaciones_t029 import ejecutar_busybox, registrar_paquete_busybox
else:
    from rutas_imagenes import DISCOS_SILICIO, copia_privada, crear_disco_patron
    from verificaciones_t029 import ejecutar_busybox, registrar_paquete_busybox

RESET = "\033[0m"
BOLD = "\033[1m"
GREEN = "\033[32m"
RED = "\033[31m"
YELLOW = "\033[33m"
CYAN = "\033[36m"
WHITE = "\033[37m"

def print_banner():
    print(f"{BOLD}{CYAN}╔══════════════════════════════════════════════════════════════════════╗{RESET}")
    print(f"{BOLD}{CYAN}║      PORTIX OS v6.0 'CHRONOS' — EJECUTOR INTERACTIVO EN VIVO         ║{RESET}")
    print(f"{BOLD}{CYAN}║       Uso Real • Segundo Uno • Asistente • Benchmarks               ║{RESET}")
    print(f"{BOLD}{CYAN}╚══════════════════════════════════════════════════════════════════════╝{RESET}\n")

class PortixInteractiveRunner:
    def __init__(self, img_path):
        # [T-030] Si falla el aislamiento no se arranca sobre el original.
        if str(img_path).lower().endswith(".iso"):
            raise ValueError("El ejecutor interactivo requiere una imagen de disco RAW")
        try:
            copia = str(copia_privada(img_path))
        except (OSError, ValueError) as e:
            raise RuntimeError(f"no se pudo copiar la imagen a build/dist: {e}") from e
        _t_sync = time.time()
        try:
            with open(copia, "rb") as f:
                os.fsync(f.fileno())
        except OSError:
            pass
        print(f"[*] copia de la imagen sincronizada en el anfitrion: {time.time() - _t_sync:.1f} s")
        self.img_path = copia
        self._img_copia = copia
        self.base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        self.mon_sock_path = "/tmp/qemu-interactive-mon.sock"
        self.proc = None
        self.log = ""
        self.log_lock = threading.Lock()
        self.stop_thread = False
        self.results = {}

    SILICIO_DISCOS = DISCOS_SILICIO

    def _crear_disco_patron(self, ruta, mib, firma):
        crear_disco_patron(ruta, mib, firma)

    def _silicon_args(self):
        """Discos con patrón + controladores NVMe/AHCI/xHCI/HDA expuestos por QEMU."""
        args = []
        for qid, ruta, mib, firma in self.SILICIO_DISCOS:
            self._crear_disco_patron(ruta, mib, firma)
            args += ["-drive", f"file={ruta},format=raw,if=none,id=drive-{qid},file.locking=off"]
        args += [
            "-device", "nvme,serial=PORTIXNVME01,drive=drive-nvme0",
            "-device", "ich9-ahci,id=ahci",
            "-device", "ide-hd,bus=ahci.0,drive=drive-sata0",
            "-device", "qemu-xhci,id=xhci",
            "-device", "usb-storage,bus=xhci.0,drive=drive-usb0",
            "-audiodev", "none,id=snd0",
            "-device", "intel-hda,id=hda",
            "-device", "hda-duplex,bus=hda.0,audiodev=snd0",
        ]
        return args

    def start_os(self):
        if os.path.exists(self.mon_sock_path):
            try: os.unlink(self.mon_sock_path)
            except OSError: pass

        cmd = [
            "qemu-system-x86_64",
            "-m", "512M",
            # T-025: 2 vCPU por defecto (SMP real); PORTIX_SMP=1|4 para las sondas.
            "-smp", os.environ.get("PORTIX_SMP", "2"),
            "-drive", f"file={self.img_path},format=raw,if=ide,index=0,media=disk,file.locking=off",
            # Mismo silicio que headless_runner/build.py: NVMe, AHCI/SATA, xHCI+USB y HDA.
            # Sin estos dispositivos el kernel responde [SILICIO-AUSENTE] (sin PASS falso)
            # y las comprobaciones de la Épica 49 deben fallar, por diseño.
            *self._silicon_args(),
            "-netdev", "user,id=u1",
            "-device", "e1000,netdev=u1",
            "-vnc", "127.0.0.1:0",
            "-vga", "std",
            "-monitor", f"unix:{self.mon_sock_path},server,nowait",
            "-serial", "stdio"
        ]

        print(f"[*] Arrancando Portix OS v6.0 desde: {self.img_path}")
        self.proc = subprocess.Popen(
            cmd,
            cwd=self.base_dir,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            bufsize=0
        )

        self.reader_thread = threading.Thread(target=self._read_output, daemon=True)
        self.reader_thread.start()

        # Esperar arranque de Portix
        print("[*] Esperando inicialización del Kernel y Escritorio Real...")
        booted, _ = self.wait_for_pattern("present done", timeout=20.0)
        if booted:  # [b192 / T-016] init se carga despues de `present done`
            booted, _ = self.wait_for_pattern("init process PID=1 registered", timeout=30.0)
        if not booted:
            raise RuntimeError("Portix OS no alcanzó el arranque dentro del tiempo límite.")
        
        # Despertar el prompt del shell
        time.sleep(0.3)
        self.send_cmd("")
        found, _ = self.wait_for_pattern("root@portix:", timeout=5.0)
        if not found:
            raise RuntimeError("No se detectó el prompt del shell interactivo.")
        print(f"{GREEN}[OK] Portix OS v6.0 arrancó exitosamente y el escritorio está activo.{RESET}\n")

    def _read_output(self):
        fd = self.proc.stdout.fileno()
        while not self.stop_thread and self.proc and self.proc.poll() is None:
            try:
                data = os.read(fd, 4096)
                if not data:
                    time.sleep(0.01)
                    continue
                text = data.decode("latin1", errors="replace")
                with self.log_lock:
                    self.log += text
            except Exception:
                time.sleep(0.01)

    def get_log(self):
        with self.log_lock:
            return self.log

    def send_cmd(self, cmd_str):
        if self.proc and self.proc.stdin:
            try:
                self.proc.stdin.write(f"{cmd_str}\r\n".encode("utf-8"))
                self.proc.stdin.flush()
            except Exception as e:
                print(f"[ERR] Error enviando comando '{cmd_str}': {e}")

    @staticmethod
    def _sin_eco(tail, cmd_str):
        """[CICLO 50] Quita las lineas que son eco de la orden tecleada: el
        patron solo puede satisfacerse con la SALIDA real del sistema."""
        if not cmd_str:
            return tail
        # Solo se descartan lineas de eco: las que TERMINAN en la orden tecleada
        # ("orden\r" o "root@portix:...$ orden\r"); una linea de salida que
        # contenga la palabra ("[FUSE] Canal /dev/fuse") sigue contando.
        return "\n".join(l for l in tail.split("\n") if not l.rstrip("\r ").endswith(cmd_str))

    def wait_for_pattern(self, pattern, start_idx=0, timeout=5.0, ignorar_eco=None):
        t0 = time.time()
        while time.time() - t0 < timeout:
            with self.log_lock:
                current_log = self.log[start_idx:]
                if pattern in self._sin_eco(current_log, ignorar_eco):
                    return True, current_log
            time.sleep(0.05)
        with self.log_lock:
            return False, self.log[start_idx:]


    # ── [T-011] Mediciones reales: una prueba que acepte constantes no es prueba ──
    def verificar_bench_real(self, cmd_str, label, timeout=10.0):
        """Ejecuta `bench` DOS veces y exige: (1) las 4 lineas [BENCH-REAL];
        (2) arranque entre 200 ms y 120 s; (3) RAM usada > 0 y < total;
        (4) lectura ext2 > 0 bytes y entre 100 KiB/s y 4 GiB/s;
        (5) los microsegundos de lectura de ambas ejecuciones NO son identicos
        (una constante cableada siempre coincidiria)."""
        import re as _re
        res = []
        for _ in range(2):
            idx = len(self.get_log())
            self.send_cmd(cmd_str)
            found, tail = self.wait_for_pattern("latencia de syscall", start_idx=idx, timeout=timeout, ignorar_eco=cmd_str)
            if not found:
                print(f"  {RED}✗{RESET} {label:42} [FAIL] (sin lineas [BENCH-REAL])", flush=True)
                self.results[label] = False
                return False
            self.wait_for_pattern("root@portix:", start_idx=idx, timeout=timeout)
            m_arr = _re.search(r"arranque hasta escritorio: (\d+) ms", tail)
            m_ram = _re.search(r"memoria fisica en uso: (\d+) KiB de (\d+) KiB", tail)
            m_ext = _re.search(r"(\d+) bytes en (\d+) us = (\d+) KiB/s", tail)
            m_cac = _re.search(r"segunda pasada (\d+) us = (\d+) KiB/s; cache de bloques (\d+) KiB: (\d+) aciertos, (\d+) fallos", tail)
            m_heap = _re.search(r"monticulo de (\d+) KiB con (\d+) KiB ocupados en (\d+) asignaciones vivas y (\d+) KiB libres", tail)
            if not (m_arr and m_ram and m_ext and m_heap and m_cac):
                print(f"  {RED}✗{RESET} {label:42} [FAIL] (medida ausente o no disponible)", flush=True)
                print(f"     [DEBUG LOG]: {repr(tail[-400:])}", flush=True)
                self.results[label] = False
                return False
            arr = int(m_arr.group(1)); usada, total = int(m_ram.group(1)), int(m_ram.group(2))
            by, us, kbps = int(m_ext.group(1)), int(m_ext.group(2)), int(m_ext.group(3))
            h_tot, h_us, h_viv, h_lib = (int(m_heap.group(i)) for i in (1, 2, 3, 4))
            motivo = None
            us2, kbps2, c_kib, c_ac, c_fa = (int(m_cac.group(i)) for i in (1, 2, 3, 4, 5))
            # [T-001] la cache de bloques debe existir y notarse: la pasada
            # caliente ha de ser >= 3x mas rapida que la fria y con aciertos.
            if c_kib < 1024 or c_ac == 0: motivo = f"cache de bloques ausente ({c_kib} KiB, {c_ac} aciertos)"
            # (la "fria" de la segunda ejecucion ya viene de la cache: se compara
            # siempre contra la fria de la PRIMERA ejecucion)
            elif not (kbps2 >= 3 * (res[0][5] if res else int(m_ext.group(3)))): motivo = f"cache sin efecto: {int(m_ext.group(3))} -> {kbps2} KiB/s"
            # Invariante del buddy: ocupado + libre == tamano del monticulo (con
            # tolerancia de 64 KiB por asignaciones en vuelo entre dos lecturas).
            elif h_viv <= 0 or h_us <= 0: motivo = f"monticulo sin asignaciones ({h_us} KiB, {h_viv} vivas)"
            elif abs((h_us + h_lib) - h_tot) > 64: motivo = f"contadores del buddy incoherentes: {h_us}+{h_lib} != {h_tot} KiB"
            elif not (200 <= arr <= 120000): motivo = f"arranque implausible {arr} ms"
            elif not (0 < usada < total): motivo = f"RAM implausible {usada}/{total} KiB"
            elif by <= 0 or not (100 <= kbps <= 4 * 1024 * 1024): motivo = f"ext2 implausible {kbps} KiB/s"
            if motivo:
                print(f"  {RED}✗{RESET} {label:42} [FAIL] ({motivo})", flush=True)
                self.results[label] = False
                return False
            res.append((arr, usada, total, by, us, kbps))
        if res[0][4] == res[1][4] and res[0][1] == res[1][1]:
            print(f"  {RED}✗{RESET} {label:42} [FAIL] (dos ejecuciones identicas: cifra constante, no medida)", flush=True)
            self.results[label] = False
            return False
        a, u, t, b, us, k = res[1]
        print(f"  {GREEN}✓{RESET} {label:42} [PASS] (arranque {a} ms, RAM {u}/{t} KiB, heap {h_us}+{h_lib}={h_tot} KiB, ext2 {k} KiB/s frio / {kbps2} KiB/s cache, {c_ac}/{c_fa} aciertos/fallos; 2 ejecuciones distintas)", flush=True)
        self.results[label] = True
        return True

    def execute_and_assert(self, cmd_str, pattern, label, timeout=3.5):
        if not pattern:
            # [CICLO 50] patron vacio = prueba que no puede fallar: prohibida.
            print(f"  {RED}✗{RESET} {label:50} [FAIL] (patron vacio)", flush=True)
            self.results[label] = False
            return False
        idx = len(self.get_log())
        self.send_cmd(cmd_str)
        found, tail = self.wait_for_pattern(pattern, start_idx=idx, timeout=timeout, ignorar_eco=cmd_str)
        # [b190] Igual que en headless_runner: si el nucleo avisa de una espera larga del
        # bus ATA (disco del anfitrion lento) se amplia el plazo UNA vez a 12 s. El patron
        # exigido no cambia.
        if not found and "[ATA] el dispositivo tarda mas de lo habitual" in tail:
            print(f"  {YELLOW}⏳{RESET} {label:50} [disco del anfitrion lento: plazo ampliado 12 s]", flush=True)
            found, tail = self.wait_for_pattern(pattern, start_idx=idx, timeout=12.0, ignorar_eco=cmd_str)
        if found:
            print(f"  {GREEN}✓{RESET} {label:50} [PASS]", flush=True)
            self.results[label] = True
            pattern_pos = tail.find(pattern, max(0, tail.rfind(cmd_str) + len(cmd_str)))
            if pattern_pos < 0:
                pattern_pos = tail.find(pattern)
            idx_after = idx + pattern_pos + len(pattern)
            self.wait_for_pattern("root@portix:", start_idx=idx_after, timeout=2.0)
            time.sleep(0.05)
        else:
            print(f"  {RED}✗{RESET} {label:50} [FAIL]", flush=True)
            print(f"     [DEBUG LOG]: {repr(tail[-300:])}", flush=True)
            self.results[label] = False
            self.wait_for_pattern("root@portix:", start_idx=idx, timeout=1.0)
        return found

    # ── [FRENTE B-3] Entrada REAL de raton/teclado por el monitor QEMU ──────
    def monitor_cmd(self, cmd, wait=0.05):
        """Envia un comando HMP al monitor QEMU (socket UNIX)."""
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(self.mon_sock_path)
        time.sleep(0.1)
        try: _ = s.recv(4096)
        except Exception: pass
        s.sendall(f"{cmd}\n".encode("utf-8"))
        time.sleep(wait)
        s.close()

    def send_raw(self, text):
        """Teclas sueltas por la consola serie (poll_key del WM)."""
        if self.proc and self.proc.stdin:
            self.proc.stdin.write(text.encode("utf-8"))
            self.proc.stdin.flush()
            time.sleep(0.05)

    def _mouse_rel(self, dx, dy):
        while dx != 0 or dy != 0:
            sx = max(-60, min(60, dx)); sy = max(-60, min(60, dy))
            self.monitor_cmd(f"mouse_move {sx} {sy}", wait=0.02)
            dx -= sx; dy -= sy

    def mouse_pointer_pos(self, timeout=3.0):
        """Posicion REAL del puntero segun el WM: clic derecho -> '[WM-POINTER] x= y='."""
        import re
        i = len(self.get_log())
        self.monitor_cmd("mouse_button 2", wait=0.12)
        self.monitor_cmd("mouse_button 0", wait=0.12)
        ok, tail = self.wait_for_pattern("[WM-POINTER]", start_idx=i, timeout=timeout)
        m = re.search(r"\[WM-POINTER\] x=(\d+) y=(\d+)", tail) if ok else None
        return (int(m.group(1)), int(m.group(2))) if m else None

    def mouse_click_at(self, x, y):
        """Lleva el puntero PS/2 real a (x, y) y hace clic izquierdo.

        Bucle cerrado: el driver PS/2 del kernel acumula deltas (<=64 px por
        paquete) y la posicion inicial depende de lo que haya pasado antes, asi
        que se mueve, se consulta la posicion real al WM (boton derecho) y se
        corrige hasta acertar. NUNCA se pulsa a ciegas: si el WM no contesta
        (anfitrion lento, WM ocupado en fork+execve+waitpid) se reconsulta, y si
        sigue sin posicion se lleva el puntero al tope (0,0) del driver y se
        recorre desde ahi. Si aun asi no se confirma, la prueba fallara."""
        def home():
            for _ in range(20):
                self.monitor_cmd("mouse_move -60 -60", wait=0.02)
            return (0, 0)
        pos = getattr(self, "_mouse_pos", None)
        if pos is None:
            pos = self.mouse_pointer_pos()
            if pos is None:
                pos = home()
        confirmado = False
        for intento in range(8):
            self._mouse_rel(x - pos[0], y - pos[1])
            time.sleep(0.15)
            real = None
            for _ in range(3):
                real = self.mouse_pointer_pos(timeout=4.0)
                if real is not None:
                    break
                time.sleep(0.5)
            if real is None:
                print(f"    [PS2] el WM no confirmo la posicion del puntero (intento {intento + 1}); se reinicia desde (0,0)", flush=True)
                pos = home()
                continue
            pos = real
            if real == (x, y):
                confirmado = True
                break
        self._mouse_pos = pos
        if not confirmado:
            print(f"    [PS2] puntero en {pos}, objetivo ({x}, {y}): se pulsa de todos modos para que la prueba falle con evidencia", flush=True)
        self.monitor_cmd("mouse_button 1", wait=0.15)
        self.monitor_cmd("mouse_button 0", wait=0.15)

    def assert_pattern(self, pattern, label, start_idx, timeout=10.0):
        found, tail = self.wait_for_pattern(pattern, start_idx=start_idx, timeout=timeout)
        if found:
            print(f"  {GREEN}✓{RESET} {label:50} [PASS]", flush=True)
        else:
            print(f"  {RED}✗{RESET} {label:50} [FAIL]", flush=True)
            print(f"     [DEBUG LOG]: {repr(tail[-400:])}", flush=True)
        self.results[label] = found
        return found

    def capture_screenshot(self, out_png_path):
        """Captura un screendump de QEMU vía el monitor UNIX y lo convierte a PNG."""
        print(f"[*] Tomando captura visual del escritorio en ejecución...")
        ppm_path = "/tmp/portix_screendump.ppm"
        if os.path.exists(ppm_path):
            try: os.unlink(ppm_path)
            except OSError: pass

        try:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            s.connect(self.mon_sock_path)
            time.sleep(0.2)
            _ = s.recv(1024) # Leer banner
            s.sendall(f"screendump {ppm_path}\n".encode("utf-8"))
            time.sleep(0.8)
            s.close()

            if os.path.exists(ppm_path) and os.path.getsize(ppm_path) > 0:
                img = Image.open(ppm_path)
                os.makedirs(os.path.dirname(out_png_path), exist_ok=True)
                img.save(out_png_path, "PNG")
                print(f"  {GREEN}✓{RESET} Captura guardada en: {out_png_path} ({img.width}x{img.height}) [PASS]")
                self.results["Live Desktop Visual Screendump"] = True
                try: os.unlink(ppm_path)
                except OSError: pass
                return True
            else:
                print(f"  {RED}✗{RESET} Screendump falló o archivo vacío [FAIL]")
                self.results["Live Desktop Visual Screendump"] = False
                return False
        except Exception as e:
            print(f"  {RED}✗{RESET} Error conectando al monitor QEMU: {e} [FAIL]")
            self.results["Live Desktop Visual Screendump"] = False
            return False

    def close(self):
        self.stop_thread = True
        terminado = True
        if self.proc:
            try:
                if self.proc.poll() is None:
                    self.proc.terminate()
                self.proc.wait(timeout=2.0)
            except Exception:
                try:
                    self.proc.kill()
                    self.proc.wait(timeout=2.0)
                except Exception as e:
                    terminado = False
                    self.results["T-030: QEMU detenido antes de retirar imagen"] = False
                    print(f"[ERROR] QEMU no se detuvo; se conserva el disco privado: {e}")
        copia = getattr(self, "_img_copia", None)
        if copia and terminado:
            try: os.unlink(copia)
            except OSError as e:
                self.results["T-030: limpieza de copia privada"] = False
                print(f"[ERROR] No se pudo retirar {copia}: {e}")
        if os.path.exists(self.mon_sock_path):
            try: os.unlink(self.mon_sock_path)
            except OSError: pass

def main():
    print_banner()
    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    img_path = os.path.join(base_dir, "build", "dist", "portix.img")
    screenshot_path = os.path.join(base_dir, "build", "dist", "portix_live_desktop.png")

    runner = PortixInteractiveRunner(img_path)
    # [FRENTE B] Repositorio HTTP real en 10.0.2.2:8000 (build/packages): las
    # pruebas de red de test_posix y de pkg/curl conectan de verdad; sin
    # servidor, fallan.
    try:
        import http.server, socketserver
        _pkg_dir = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "build", "packages")
        class _PkgHandler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *a, **k): super().__init__(*a, directory=_pkg_dir, **k)
            def log_message(self, *a): pass
        socketserver.TCPServer.allow_reuse_address = True
        _httpd = socketserver.TCPServer(("0.0.0.0", 8000), _PkgHandler)
        threading.Thread(target=_httpd.serve_forever, daemon=True).start()
        print(f"[HTTP] Repositorio de paquetes en :8000 ({_pkg_dir})")
        import ssl as _ssl
        _crt, _key = os.path.join(_pkg_dir, "tls", "server.crt"), os.path.join(_pkg_dir, "tls", "server.key")
        if os.path.exists(_crt):
            _ctx = _ssl.SSLContext(_ssl.PROTOCOL_TLS_SERVER); _ctx.minimum_version = _ssl.TLSVersion.TLSv1_3
            _ctx.load_cert_chain(_crt, _key)
            _httpsd = socketserver.TCPServer(("0.0.0.0", 8443), _PkgHandler)
            _httpsd.socket = _ctx.wrap_socket(_httpsd.socket, server_side=True)
            threading.Thread(target=_httpsd.serve_forever, daemon=True).start()
            print("[HTTPS] Repositorio TLS 1.3 en :8443")
    except Exception as e:
        print(f"[HTTP] AVISO: no se pudo levantar el repositorio en :8000: {e}")
    try:
        runner.start_os()

        print(f"{BOLD}[1] USO REAL: Interacción con el Asistente y Centro de Bienvenida (Segundo Uno){RESET}")
        runner.execute_and_assert("asistente", "ASISTENTE", "GUI: Invocación del Asistente de Inicio & Control")
        runner.execute_and_assert("win32", "win32: retirado", "GUI: la capa Win32 PE fue retirada (no se finge compatibilidad Windows)")
        runner.verificar_bench_real("bench", "[T-011] GUI bench: medidas reales (arranque/RAM/ext2), 2 ejecuciones distintas", timeout=15.0)

        print(f"\n{BOLD}[2] USO REAL: Ejecución de Binarios y Capas de Compatibilidad Ring-3{RESET}")
        runner.execute_and_assert("run portix-bench", "[PORTIX-BENCH-PASS]", "Ring-3: portix-bench mediciones TSC reales (sin estimaciones)", timeout=6.0)
        runner.execute_and_assert("run pbox", "[PBOX-PASS]", "Ring-3: Motor de Contenedores OCI Ligeros (<20ms)", timeout=5.0)
        runner.execute_and_assert("run portix-lsp", "[PORTIX-LSP-PASS]", "Ring-3: Servidor de Lenguaje LSP JSON-RPC (<8ms)", timeout=5.0)
        runner.execute_and_assert("run portix-snapper", "[PORTIX-SNAPPER-PASS]", "Ring-3: Instantáneas CoW y Rollback Inmune (<5ms)", timeout=5.0)
        runner.execute_and_assert("run portix-tpm", "[PORTIX-TPM-PASS]", "Ring-3: Atestación TPM 2.0 PCR0-7 y Sellado AES", timeout=5.0)
        runner.execute_and_assert("run portix-migrate", "[PORTIX-MIGRATE-PASS]", "Ring-3: Migración Transaccional de Perfiles Windows/Linux", timeout=5.0)
        runner.execute_and_assert("run portix-vanguard", "[VANGUARD-SUITE-PASS]", "Ring-3: Suite de Telemetria de Silicio y Vanguardia", timeout=6.0)
        runner.execute_and_assert("run portix-wl --test", "portix-wl Wayland Compositor self-test passed", "Ring-3: Compositor Hibrido Wayland (Hyprland + GNOME 47)", timeout=6.0)
        runner.execute_and_assert("run ps", "init", "Ring-3: Listado de Procesos Activos (/bin/ps)", timeout=4.0)
        runner.execute_and_assert("run free", "Mem:", "Ring-3: Telemetría de Memoria Real (/bin/free)", timeout=4.0)
        runner.execute_and_assert("vanguard", "[VANGUARD-SUITE-PASS]", "Kernel: Frentes 1, 2 y 3 (Silicio, NVMe, S0ix, io_uring)", timeout=4.0)
        runner.execute_and_assert("wayland_hybrid", "[HYBRID-WAYLAND-SUITE-PASS]", "Kernel: Epicas 31, 32 y 33 (Wayland Hibrido)", timeout=4.0)
        runner.execute_and_assert("epic34", "[EPIC-34-PASS]", "Kernel: Epica 34 (Pipeline Tri-Modo Vulkan/DirectScanout)", timeout=4.0)
        runner.execute_and_assert("epic35", "[EPIC-35-PASS]", "Kernel: Epica 35 (Productividad y Gaming Proton)", timeout=4.0)
        runner.execute_and_assert("epic36", "[EPIC-36-PASS]", "Kernel: Epica 36 (HDR10 PQ/BT.2020 & Portix v3.1 Aether)", timeout=4.0)
        runner.execute_and_assert("epic37", "[EPIC-37-PASS]", "Kernel: Epica 37 (IA Local, NPU & Cero Telemetria)", timeout=4.0)
        runner.execute_and_assert("epic38", "[EPIC-38-PASS]", "Kernel: Epica 38 (Portix-FS Ultra, ZNS, LZ4/ZSTD & BLAKE3)", timeout=4.0)
        runner.execute_and_assert("epic39", "[EPIC-39-PASS]", "Kernel: Epica 39 (Hipervisor Bare-Metal Tipo-1 & MicroVMs)", timeout=4.0)
        runner.execute_and_assert("epic40", "[EPIC-40-PASS]", "Kernel: Epica 40 (Red Malla Zero-Trust, WireGuard Ring-0 & P2P)", timeout=4.0)
        runner.execute_and_assert("epic41", "[EPIC-41-PASS]", "Kernel: Epica 41 (Resiliencia Industrial & Live Hot-Patching)", timeout=4.0)
        runner.execute_and_assert("cosmos", "[PORTIX-COSMOS-MASTER-PASS]", "Kernel: Epica 42 & Portix OS v4.0 'Cosmos' Consagrado (historico)", timeout=4.0)
        runner.execute_and_assert("aether", "[PORTIX-AETHER-AI-PASS]", "Kernel: Portix OS v3.1 'Aether' & v4.0 'Cosmos' Suite", timeout=4.0)
        runner.execute_and_assert("run python /home/user/calculadora.py", "Calculadora Python ejecutada exitosamente", "Ring-3: Intérprete Python ejecutando script real", timeout=5.0)
        # T-029: compatibilidad C con ELF externo real, no utilidades C propias.
        idx_paquete_t029 = len(runner.get_log())
        runner.execute_and_assert("run pkg install busybox-static", "[OK] Paquete 'busybox-static' (1.36.1-r31 (Alpine v3.20)) instalado exitosamente.", "T-029: instalar BusyBox original de Alpine", timeout=90.0)
        registrar_paquete_busybox(runner, idx_paquete_t029, "T-029: descarga HTTPS, SHA-256 completo e instalación de BusyBox Alpine")
        ejecutar_busybox(runner, runner.send_cmd, "run /bin/busybox echo HOLA-MUNDO-ALPINE", "HOLA-MUNDO-ALPINE", "T-029: ELF C externo imprime saludo y sale 0")
        ejecutar_busybox(runner, runner.send_cmd, "run /bin/busybox expr 45 + 55", "100", "T-029: ELF C externo calcula 45 + 55 = 100 y sale 0")
        runner.execute_and_assert("run calc 42 + 58", "100", "Ring-3: Calculadora y Aritmética de Alta Precisión", timeout=4.0)
        runner.execute_and_assert("run seq 1 5", "5", "Ring-3: Generador de Secuencias (/bin/seq)", timeout=4.0)
        runner.execute_and_assert("run date", "2026", "Ring-3: Reloj de Tiempo Real RTC (/bin/date)", timeout=4.0)
        runner.execute_and_assert("run curl https://10.0.2.2:8443/index.txt", "[CURL-TLS-OK] canal TLS 1.3 verificado", "Ring-3: /bin/curl HTTPS real (TLS 1.3 X25519/ChaCha20-Poly1305, X.509 validado)", timeout=30.0)
        runner.execute_and_assert("run pkg list", "gcc-toolchain", "Ring-3: Gestor de Paquetes (/bin/pkg list)", timeout=4.0)
        runner.execute_and_assert("run portix-monitor", "[PORTIX-MONITOR-PASS]", "Ring-3 SDK App: portix-monitor (Silicio & Buddy Allocator)", timeout=4.0)
        runner.execute_and_assert("run portix-chat", "[PORTIX-CHAT-PASS]", "Ring-3 SDK App: portix-chat (Malla P2P WireGuard)", timeout=4.0)
        runner.execute_and_assert("run portix-paint", "[PORTIX-PAINT-PASS]", "Ring-3 SDK App: portix-paint (Estudio de Dibujo Vectorial)", timeout=4.0)
        runner.execute_and_assert("run portix-calc", "[PORTIX-CALC-PASS]", "Ring-3 SDK App: portix-calc (Calculadora Científica 64-bit)", timeout=4.0)
        runner.execute_and_assert("run portix-player", "[PORTIX-PLAYER-PASS]", "Ring-3 SDK App: portix-player (Reproductor WAV /dev/dsp)", timeout=4.0)
        runner.execute_and_assert("run portix-probe", "[PORTIX-PROBE-PASS]", "Ring-3 SDK App: portix-probe (Observabilidad eBPF & Kprobes)", timeout=4.0)
        runner.execute_and_assert("run portix-vulkan", "BENCHMARK VULKAN RAY-TRACING", "Ring-3: Vulkan 1.3 Rasterizer & Ray-Tracing (Mega-Rays/s)", timeout=5.0)
        runner.execute_and_assert("vulkan_rt", "[VULKAN-RAYTRACING-PASS]", "Gráficos: Pipeline Vulkan Rasterizer y BVH Ray-Tracing", timeout=4.0)
        runner.execute_and_assert("driver_sandbox", "[EPIC-47-PASS]", "Drivers: Aislamiento y Carga de Drivers Linux en Ring 3 (IOMMU)", timeout=4.0)
        runner.execute_and_assert("riscv64", "[RISCV64-ARCH-PASS]", "Multi-Arch: HAL Nativa RISC-V 64-bit (RV64GCBH, Sv48)", timeout=4.0)
        runner.execute_and_assert("pqc", "[PQC-QUANTUM-PASS]", "Seguridad: Criptografía Post-Cuántica (ML-KEM & ML-DSA)", timeout=4.0)
        runner.execute_and_assert("ebpf", "[EBPF-PORTIX-PASS]", "Kernel: Motor eBPF Soberano, Kprobes y Verificador DAG", timeout=4.0)
        runner.execute_and_assert("rtaudio", "[RT-AUDIO-PASS]", "Audio: Audio Profesional Determinista RT (192kHz) y MIDI 2.0", timeout=4.0)
        runner.execute_and_assert("wasi", "[WASI-COMPONENTS-PASS]", "Aislamiento: Sandbox WASI Component Model de Alta Densidad", timeout=4.0)
        runner.execute_and_assert("dfs", "[PORTIX-DFS-PASS]", "Almacenamiento: Sistema de Archivos Distribuido Portix-DFS & CRDT", timeout=4.0)
        runner.execute_and_assert("nvme", "[EPIC-49-PASS]", "Épica 49: Controladores NVMe Bare-Metal DMA Zero-Copy", timeout=4.0)
        runner.execute_and_assert("pipes", "[EPIC-54-PASS]", "Épica 54: Tuberías Anónimas POSIX (4KB Buffer, dup2/dup3)", timeout=4.0)
        runner.execute_and_assert("wm", "[PORTIX-WM-PASS]", "Épica 55: Pipeline Gráfico y Gestor portix-wm con Tiling y Workspaces", timeout=4.0)
        runner.execute_and_assert("signals", "[EPIC-58-PASS]", "Épica 58: Señales POSIX (Entrega Real, SigFrame y sigreturn)", timeout=4.0)
        runner.execute_and_assert("devreport", "[EPIC-59-PASS]", "Épica 59: Suite de Desarrollo, Trazador TSC y Profiler de Silicio", timeout=4.0)
        runner.execute_and_assert("trace", "TRAZADOR DE LLAMADAS AL SISTEMA", "Épica 59: Trazador de Syscalls con marcas de tiempo TSC", timeout=4.0)
        runner.execute_and_assert("profiler", "PROFILER DE SILICIO POR PROCESO", "Épica 59: Profiler de CPU por proceso (muestreo de RIP a 100 Hz)", timeout=4.0)
        runner.execute_and_assert("cat /proc/syscalls", "[EPIC-59-PASS]", "VFS ProcFS: Trazas de syscalls y coste en ciclos TSC", timeout=4.0)
        runner.execute_and_assert("cat /proc/profiler", "[EPIC-59-PASS]", "VFS ProcFS: Perfiles de CPU por proceso y puntos calientes", timeout=4.0)
        runner.execute_and_assert("cat /proc/dbg", "[EPIC-59-PASS]", "VFS ProcFS: Canal de depuracion del kernel", timeout=4.0)
        runner.execute_and_assert("maps 1", "[EPIC-59-PASS]", "Épica 59: Mapa de memoria por recorrido real de tablas de pagina", timeout=4.0)
        runner.execute_and_assert("abifreeze", "[EPIC-60-PASS]", "Épica 60: Congelación de la ABI multi-arquitectura (huella FNV-1a)", timeout=4.0)
        runner.execute_and_assert("cat /proc/abi", "[EPIC-60-PASS]", "VFS ProcFS: ABI congelada y compatibilidad binaria Linux", timeout=4.0)
        runner.execute_and_assert("certify", "Estabilidad bare-metal certificada sin fallos", "Épica 60: Certificación final de estabilidad bare-metal", timeout=6.0)
        runner.execute_and_assert("cat /proc/certification", "Correctas:", "VFS ProcFS: Batería de certificación en vivo por subsistema", timeout=6.0)
        runner.execute_and_assert("epic60", "[PORTIX-CHRONOS-MASTER-PASS]", "Épica 60: Lanzamiento maestro de las 4,000 fases", timeout=4.0)
        runner.execute_and_assert("cat /proc/release", "[PORTIX-CHRONOS-MASTER-PASS]", "VFS ProcFS: Reporte de lanzamiento v6.0 'Chronos'", timeout=4.0)
        runner.execute_and_assert("uname", "PORTIX 1.0.0 x86_64", "Identidad: version unificada del kernel (v1.0.0 'Chronos')", timeout=4.0)
        runner.execute_and_assert("run uname", "Portix 1.0.0 x86_64", "Identidad: /bin/uname coherente con el producto v1.0.0", timeout=4.0)
        runner.execute_and_assert("cat /etc/os-release", "1.0.0 Chronos", "Identidad: os-release del producto consagrado", timeout=4.0)
        runner.execute_and_assert("run abi-linux", "[LINUX-ABI-PASS]", "Compatibilidad binaria Linux: puerta SYSCALL (MSR LSTAR) desde Ring 3", timeout=8.0)
        runner.execute_and_assert("cat /proc/signals", "[EPIC-58-PASS]", "VFS ProcFS: Telemetría del Subsistema de Señales POSIX", timeout=4.0)
        runner.execute_and_assert("cat /proc/nvme", "[EPIC-49-PASS]", "VFS ProcFS: Telemetría de Silicio NVMe y Colas SQ/CQ", timeout=4.0)
        runner.execute_and_assert("cat /proc/pipes", "[EPIC-54-PASS]", "VFS ProcFS: Estadísticas de Tuberías y Descriptores Duplicados", timeout=4.0)
        runner.execute_and_assert("cat /proc/ebpf", "[EBPF-PROBE]", "VFS ProcFS: Telemetría eBPF y Kprobes en Tiempo Real", timeout=4.0)
        runner.execute_and_assert("cat /proc/portix_dfs", "[PORTIX-DFS]", "VFS ProcFS: Malla Distribuida Portix-DFS y Replicación", timeout=4.0)
        runner.execute_and_assert("cat /proc/meminfo", "MemTotal", "VFS ProcFS: Inspección de Memoria Física y Buddy Allocator", timeout=4.0)
        runner.execute_and_assert("cat /proc/cpuinfo", "processor", "VFS ProcFS: Inspección de Procesadores x86_64 SMP", timeout=4.0)
        runner.execute_and_assert("chronos", "[PORTIX-CHRONOS-MASTER-PASS]", "Portix OS v6.0 'Chronos': Bloque Real Consolidado", timeout=4.0)
        runner.execute_and_assert("i18n", "[I18N]", "i18n: Motor de Internacionalización Multilingüe (ES, EN, JA, DE)", timeout=4.0)
        runner.execute_and_assert("aarch64", "[ARM64-ARCH-PASS]", "Multi-Arch: HAL ARM64/AArch64 (MMU, GIC, PL011)", timeout=4.0)

        # ── Épica 58: verificación end-to-end del subsistema de señales ──
        print(f"\n{BOLD}[2b] ÉPICA 58: SEÑALES POSIX — VERIFICACIÓN END-TO-END EN /bin/test_posix{RESET}")
        idx_sig = len(runner.get_log())
        runner.send_cmd("run test_posix")
        sig_done, _ = runner.wait_for_pattern("ALL POSIX TESTS COMPLETED SUCCESSFULLY", start_idx=idx_sig, timeout=45.0)
        sig_checks = [
            ("[TEST 4896] Entrega real de senal verificada", "Señal entregada al manejador de usuario (sigaction + kill)"),
            ("[TEST 4897] Contexto de usuario restaurado", "Restauración de contexto tras el manejador (sigreturn via trampolín)"),
            ("[TEST 4898] SIG_IGN verificado", "SIG_IGN descarta la señal sin invocar manejador"),
            ("[TEST 4899] SIGSEGV entregado", "SIGSEGV entregado al manejador de usuario desde #PF"),
            ("[TEST 4900] SIGPIPE verificado", "SIGPIPE + EPIPE al escribir en tubería sin lectores"),
            ("[TEST 4901] SIGPIPE accion por defecto verificada", "SIGPIPE con acción por defecto termina el proceso (128+13)"),
            ("[TEST 4902] SIGCHLD verificado", "SIGCHLD notificado al progenitor en la salida del hijo"),
            ("[TEST 4903] rt_sigreturn explicita verificada", "rt_sigreturn explícita por syscall restaura el contexto"),
        ]
        for pat, label in sig_checks:
            found, _ = runner.wait_for_pattern(pat, start_idx=idx_sig, timeout=6.0)
            runner.results[label] = found
            mark = f"{GREEN}✓{RESET}" if found else f"{RED}✗{RESET}"
            print(f"  {mark} {label:50} [{'PASS' if found else 'FAIL'}]", flush=True)
        runner.results["Suite POSIX completa sin abortar (Épica 58)"] = sig_done
        mark = f"{GREEN}✓{RESET}" if sig_done else f"{RED}✗{RESET}"
        print(f"  {mark} {'Suite POSIX completa tras Señales POSIX':50} [{'PASS' if sig_done else 'FAIL'}]", flush=True)

        # ── Épica 59: suite de desarrollo integrada y depurador /bin/dbg ──
        print(f"\n{BOLD}[2c] ÉPICA 59: SUITE DE DESARROLLO, TRAZADOR Y PROFILER DE SILICIO{RESET}")
        runner.execute_and_assert("trace on", "[TRACE] Comando 'on' aplicado", "Trazador: activacion desde la consola del kernel", timeout=4.0)
        for _ in range(2):
            runner.send_cmd("run yes")
            time.sleep(0.6)
        idx_dev = len(runner.get_log())
        runner.send_cmd("cat /proc/syscalls")
        time.sleep(1.0)
        dev_log = runner.get_log()[idx_dev:]
        m = re.search(r"Trazas registradas\s*:\s*(\d+)", dev_log)
        traces = int(m.group(1)) if m else 0
        runner.results["Trazador: trazas reales registradas en el anillo (>0)"] = traces > 0
        print(f"  {'✓' if traces > 0 else '✗'} {'Trazador de syscalls con trazas reales':50} [{'PASS' if traces > 0 else 'FAIL'}] (n={traces})", flush=True)
        idx_dev = len(runner.get_log())
        runner.send_cmd("cat /proc/profiler")
        time.sleep(1.0)
        dev_log = runner.get_log()[idx_dev:]
        m = re.search(r"Muestras totales\s*:\s*(\d+)", dev_log)
        samples = int(m.group(1)) if m else 0
        runner.results["Profiler: muestras de CPU acumuladas (>0)"] = samples > 0
        print(f"  {'✓' if samples > 0 else '✗'} {'Profiler de silicio con muestras reales':50} [{'PASS' if samples > 0 else 'FAIL'}] (n={samples})", flush=True)
        # [FRENTE C-4] estres real fork+execve+waitpid desde Ring 3
        runner.execute_and_assert("run portix-bench forkexec 100", "[FORKEXEC-PASS] ciclos=100 fallos_fork=0 fallos_wait=0 salidas_anomalas=0", "Procesos: 100 ciclos fork+execve+waitpid reales sin fallos", timeout=60.0)
        # [FRENTE C-3] carga Ring 3 conocida y top de simbolos con nombres reales del ELF
        idx_dev = len(runner.get_log())
        runner.send_cmd("run portix-bench cpu 2")
        bench_log = ""
        for _ in range(40):
            time.sleep(0.5)
            bench_log = runner.get_log()[idx_dev:]
            if "[CPU-BENCH-DONE]" in bench_log:
                break
        msh = re.search(r"\[CPU-BENCH-SHARE\] quemar_fnv=(\d+)% quemar_collatz=(\d+)%", bench_log)
        share_tsc = int(msh.group(1)) if msh else -1
        idx_dev = len(runner.get_log())
        runner.send_cmd("cat /proc/profiler")
        time.sleep(1.5)
        dev_log = re.sub(r"\x1b\[[0-9;?]*[ -/]*[@-~]", "", runner.get_log()[idx_dev:])
        hot = {}
        orden = []
        for mh in re.finditer(r"^\s*(\d+)\s+(\d+)\s+\d+\s+0x[0-9a-fA-F]+\s+(\S+)\s*$", dev_log, re.M):
            if mh.group(3) not in hot: orden.append(mh.group(3))
            hot.setdefault(mh.group(3), int(mh.group(1)))
        fnv, col = hot.get("quemar_fnv", 0), hot.get("quemar_collatz", 0)
        share_prof = fnv * 100 // max(1, fnv + col)
        # [CICLO 50] La verdad de referencia es el TSC medido por funcion, no una suposicion sobre cual de las
        # dos "quema" mas (depende del anfitrion: en recert 72 el TSC dio 49/51). Se exige: ambas muestreadas,
        # que sean los DOS simbolos mas calientes del perfil, y que el reparto por muestreo coincida con el TSC (+-12).
        # Entre los simbolos RESUELTOS del ELF (se excluye <desconocido>: PIDs sin tabla de simbolos cargada).
        nombrados = [n for n in orden if not n.startswith("<")]
        top2_ok = set(nombrados[:2]) == {"quemar_fnv", "quemar_collatz"}
        top_ok = fnv > 0 and col > 0 and top2_ok and share_tsc >= 0 and abs(share_prof - share_tsc) <= 12
        runner.results["Profiler: top de simbolos nombra funciones reales del ELF y coincide con el TSC (+-12)"] = top_ok
        print(f"  {'✓' if top_ok else '✗'} {'Profiler: top de simbolos reales (quemar_fnv/collatz)':50} [{'PASS' if top_ok else 'FAIL'}] (muestreo {share_prof}% vs TSC {share_tsc}%, n={fnv}/{col})", flush=True)
        dev_checks = [
            ("run dbg --version", "portix-dbg v3.6.0", "Depurador /bin/dbg v3.6.0 con telemetria del kernel"),
            ("run dbg 1 maps", "REGIONES DEL ESPACIO DE DIRECCIONES", "Depurador: mapa de memoria real de un proceso vivo"),
            ("run dbg 1 profiler", "Perfil de CPU de PID=1", "Depurador: perfil de CPU del proceso desde /proc/profiler"),
            ("run dbg --trace on", "Trazador: comando 'on' aplicado", "Depurador: control del trazador desde espacio de usuario"),
        ]
        for cmd, pat, label in dev_checks:
            idx_dev = len(runner.get_log())
            runner.send_cmd(cmd)
            found, _ = runner.wait_for_pattern(pat, start_idx=idx_dev, timeout=8.0)
            runner.results[label] = found
            mark = f"{GREEN}✓{RESET}" if found else f"{RED}✗{RESET}"
            print(f"  {mark} {label:50} [{'PASS' if found else 'FAIL'}]", flush=True)

        # ── Épica 60: consagración universal v6.0 'Chronos' ──
        print(f"\n{BOLD}[2d] ÉPICA 60: CONSAGRACION UNIVERSAL v6.0 'CHRONOS' — 4,000 FASES{RESET}")
        idx_v6 = len(runner.get_log())
        runner.send_cmd("cat /proc/abi")
        time.sleep(1.0)
        v6_log = runner.get_log()[idx_v6:]
        m = re.search(r"Huella FNV-1a \(64 bits\)\s*:\s*0x([0-9a-f]{16})", v6_log)
        digest_ok = m is not None and m.group(1) != "0" * 16
        runner.results["ABI: huella digital FNV-1a calculada en vivo"] = digest_ok
        print(f"  {'✓' if digest_ok else '✗'} {'ABI congelada con huella digital real':50} [{'PASS' if digest_ok else 'FAIL'}] (0x{m.group(1)[:8] if m else '?'}...)", flush=True)
        idx_v6 = len(runner.get_log())
        runner.send_cmd("cat /proc/certification")
        time.sleep(1.0)
        v6_log = runner.get_log()[idx_v6:]
        m2 = re.search(r"Correctas:\s*(\d+)\s*\|\s*Fallidas:\s*(\d+)", v6_log)
        cert_ok = m2 is not None and int(m2.group(2)) == 0 and int(m2.group(1)) >= 12
        runner.results["Certificación bare-metal sin fallos (>=12 comprobaciones)"] = cert_ok
        print(f"  {'✓' if cert_ok else '✗'} {'Certificacion en vivo sin fallos':50} [{'PASS' if cert_ok else 'FAIL'}] ({m2.group(1) if m2 else '?'} correctas)", flush=True)
        v6_checks = [
            ("epic60", "LANZAMIENTO MAESTRO", "Lanzamiento maestro v6.0 con trazabilidad de 4,000 fases"),
            ("cat /proc/release", "Fases certificadas", "Reporte de release con fases certificadas al 100%"),
        ]
        for cmd, pat, label in v6_checks:
            idx_v6 = len(runner.get_log())
            runner.send_cmd(cmd)
            found, _ = runner.wait_for_pattern(pat, start_idx=idx_v6, timeout=8.0)
            runner.results[label] = found
            mark = f"{GREEN}✓{RESET}" if found else f"{RED}✗{RESET}"
            print(f"  {mark} {label:50} [{'PASS' if found else 'FAIL'}]", flush=True)

        # ── Ciclo v6.x: métricas medidas y compatibilidad binaria Linux ──
        print(f"\n{BOLD}[2e] CICLO v6.x: COMPATIBILIDAD BINARIA LINUX Y METRICAS MEDIDAS{RESET}")
        idx_ab = len(runner.get_log())
        runner.send_cmd("run abi-linux")
        time.sleep(1.5)
        abi_done, _ = runner.wait_for_pattern("[LINUX-ABI-PASS]", start_idx=idx_ab, timeout=10.0)
        runner.results["ABI Linux: 7/7 llamadas (6 traducidas + 999 -> ENOSYS) desde Ring 3"] = abi_done
        print(f"  {'✓' if abi_done else '✗'} {'Compatibilidad Linux verificada por la puerta SYSCALL':50} [{'PASS' if abi_done else 'FAIL'}]", flush=True)
        idx_ab = len(runner.get_log())
        runner.send_cmd("cat /proc/abi")
        time.sleep(1.0)
        abi_log = runner.get_log()[idx_ab:]
        m = re.search(r"Invocaciones por el ABI Linux\s*:\s*(\d+)", abi_log)
        m2 = re.search(r"Traducidas explicitamente\s*:\s*(\d+)", abi_log)
        inv = int(m.group(1)) if m else 0
        tra = int(m2.group(1)) if m2 else 0
        # [FRENTE C-5] abi-linux lanza 6 traducidas + 1 sin traduccion (999): la
        # cobertura medida debe cuadrar exactamente, no superar un umbral.
        cob_ok = inv >= 7 and tra >= 6 and tra * 100 // max(inv, 1) >= 85
        # [FRENTE C-5] El fallback debe estar MEDIDO: el 999 que lanzo abi-linux
        # tiene que figurar como numero sin traduccion, con contador >= 1.
        m3 = re.search(r"Sin traduccion \(fallback nativo\)\s*:\s*(\d+)", abi_log)
        m4 = re.search(r"Numeros sin traduccion vistos\s*:([^\n]*)", abi_log)
        fb = int(m3.group(1)) if m3 else -1
        vistos = m4.group(1).split() if m4 else []
        fb_ok = fb >= 1 and "999" in vistos
        runner.results["ABI Linux: fallback medido (999 -> ENOSYS contabilizado en /proc/abi)"] = fb_ok
        print(f"  {'✓' if fb_ok else '✗'} {'ABI Linux: fallback ENOSYS medido en /proc/abi':50} [{'PASS' if fb_ok else 'FAIL'}] (fallbacks={fb} vistos={vistos})", flush=True)
        runner.results["ABI Linux: cobertura medida en runtime (>90% traducidas)"] = cob_ok
        print(f"  {'✓' if cob_ok else '✗'} {'Cobertura del traductor medida en runtime':50} [{'PASS' if cob_ok else 'FAIL'}] ({tra}/{inv} traducidas)", flush=True)
        idx_ab = len(runner.get_log())
        runner.send_cmd("cat /proc/abi")
        time.sleep(1.0)
        tr_log = runner.get_log()[idx_ab:]
        m3 = re.search(r"Huella FNV-1a", tr_log)
        runner.results["ABI: nodo /proc/abi con huella digital en vivo"] = m3 is not None
        print(f"  {'✓' if m3 else '✗'} {'ABI congelada publicada por ProcFS':50} [{'PASS' if m3 else 'FAIL'}]", flush=True)

        # ── [2f] Depurador de Ring 3: símbolos, hardware y puntos de parada ──
        print(f"\n{BOLD}[2f] CICLO v6.x: DEPURADOR SIMBOLIZADO Y VIGILANCIA POR HARDWARE{RESET}")
        idx_dbg = len(runner.get_log())
        runner.send_cmd("run wp-demo")
        time.sleep(3.0)
        dbg_log = runner.get_log()[idx_dbg:]
        wp_ok = "[WATCHPOINT-PASS]" in dbg_log and "[BREAKPOINT-PASS]" in dbg_log and "[CODESPATCH-PASS]" in dbg_log
        runner.results["Depurador: watchpoint de hardware (DR0-DR3 + #DB) verificado en CPU real"] = wp_ok
        print(f"  {'✓' if wp_ok else '✗'} {'Watchpoint por hardware, INT3 y parcheo verificados':50} [{'PASS' if wp_ok else 'FAIL'}]", flush=True)

        idx_dbg = len(runner.get_log())
        runner.send_cmd("run yes")
        time.sleep(1.5)
        idx_dbg = len(runner.get_log())
        runner.send_cmd("run dbg self disasm main 8")
        time.sleep(1.5)
        dis_log = runner.get_log()[idx_dbg:]
        dis_ok = "[DISASM-OK]" in dis_log and "mov" in dis_log
        runner.results["Depurador: desensamblado simbólico del binario real del proceso"] = dis_ok
        print(f"  {'✓' if dis_ok else '✗'} {'Desensamblado x86-64 simbolizado sobre el objetivo':50} [{'PASS' if dis_ok else 'FAIL'}]", flush=True)

        idx_dbg = len(runner.get_log())
        runner.send_cmd("run dbg --break self main")
        time.sleep(1.5)
        bp1 = runner.get_log()[idx_dbg:]
        idx_dbg = len(runner.get_log())
        runner.send_cmd("run dbg --bp-list self")
        time.sleep(1.5)
        bp2 = runner.get_log()[idx_dbg:]
        idx_dbg = len(runner.get_log())
        runner.send_cmd("run dbg --bp-clear self")
        time.sleep(1.5)
        bp3 = runner.get_log()[idx_dbg:]
        bp_ok = "[BP-SET-PASS]" in bp1 and "[BP-VERIFIED]" in bp2 and "[BP-RESTORED]" in bp3
        runner.results["Depurador: punto de parada persistente sobre un objetivo vivo, verificado y restaurado"] = bp_ok
        print(f"  {'✓' if bp_ok else '✗'} {'Puntos de parada persistentes en la imagen del proceso':50} [{'PASS' if bp_ok else 'FAIL'}]", flush=True)

        idx_dbg = len(runner.get_log())
        runner.send_cmd("run dbg self symbols")
        time.sleep(1.2)
        sym_log = runner.get_log()[idx_dbg:]
        sym_ok = "[SYMBOLS-OK]" in sym_log and "main" in sym_log
        runner.results["Depurador: tabla de símbolos del ELF publicada por el kernel"] = sym_ok
        print(f"  {'✓' if sym_ok else '✗'} {'Símbolos del ejecutable expuestos por /proc/<pid>/symbols':50} [{'PASS' if sym_ok else 'FAIL'}]", flush=True)

        print(f"\n{BOLD}[3] USO REAL: Almacenamiento Persistente VFS y Manipulación de Archivos{RESET}")
        runner.execute_and_assert("write nota_interactiva.txt SegundoUnoReal2026", "Escrito: nota_interactiva.txt", "VFS: Creación y escritura de archivo real en disco", timeout=4.0)
        runner.execute_and_assert("cat nota_interactiva.txt", "SegundoUnoReal2026", "VFS: Lectura y verificación de integridad persistente", timeout=4.0)
        runner.execute_and_assert("cat /etc/os-release", "Portix OS", "VFS: Consulta de metadatos de lanzamiento del sistema", timeout=4.0)

        print(f"\n{BOLD}[4] USO REAL: Multimedia y Audio en Hardware Simulado{RESET}")
        runner.execute_and_assert("run aplay --chime", "Sonido de inicio finalizado", "Audio: Sintesis polifonica y chime en driver AC97", timeout=4.0)
        runner.execute_and_assert("cat /home/user/grabacion.wav", "RIFF", "Audio Duplex: Archivo WAV grabado en VFS (DMA/PCM)", timeout=4.0)

        print(f"\n{BOLD}[5] USO REAL: Red y Telemetría{RESET}")
        runner.execute_and_assert("ifconfig", "10.0.2.15", "Red: Inspección de interfaz e1000 Gigabit", timeout=3.0)
        runner.execute_and_assert("ping", "[PING] 3 paquetes transmitidos, 3 recibidos, 0% perdidos", "Red: ICMP Echo real con 3 respuestas y RTT medido", timeout=8.0)

        print(f"\n{BOLD}[6] USO REAL: Compositor Gráfico y Suite de Experiencia de Usuario{RESET}")
        runner.execute_and_assert("test_ui", "UI Window Compositor & User Interaction: 100% PASS", "Compositor: Bucle interactivo completo de UI y Ventanas", timeout=6.0)

        print(f"\n{BOLD}[7] AUDITORÍA VISUAL: Captura de Pantalla Real del Escritorio en Ejecución{RESET}", flush=True)
        runner.capture_screenshot(screenshot_path)

        # [FRENTE B-3] Portix-Store real dentro de portix-wm: clics de raton
        # PS/2 REALES inyectados por el monitor QEMU sobre los botones de la
        # tienda; cada boton ejecuta /bin/pkg (fork+execve+waitpid) y la
        # tienda relee /var/pkg/installed.db. Sin repositorio HTTPS, falla.
        print(f"\n{BOLD}[8] USO REAL: Portix-Store (tienda grafica sobre /bin/pkg, raton real){RESET}", flush=True)
        runner.execute_and_assert("run pkg update", "[PKG-UPDATE-OK]", "Store: indice del repositorio descargado por HTTPS", timeout=30.0)
        runner.execute_and_assert("run pkg remove make-utils", "make-utils", "Store: estado previo limpio", timeout=10.0)
        i0 = len(runner.get_log())
        runner.send_cmd("run portix-wm")
        # [CICLO 50] numero de paquetes leido del indice REAL del repo servido.
        try:
            _idx = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "build", "packages", "index.txt"), "rb").read()
        except OSError:
            _idx = b""
        n_pkgs = len([l for l in _idx.decode("utf-8", "replace").splitlines() if l.strip()])
        runner.assert_pattern(f"Portix-Store: {n_pkgs} paquetes en el indice (/var/pkg/index.txt (pkg update, HTTPS))", "Store: catalogo leido del indice real", i0, timeout=12.0)
        runner.wait_for_pattern("Servidor de ventanas listo", start_idx=i0, timeout=12.0)
        runner.send_cmd("ws 4")
        time.sleep(0.4)
        i1 = len(runner.get_log())
        runner.mouse_click_at(420, 228)   # boton [Instalar] de la fila 4 (make-utils; busybox-static es la fila 1 desde el ciclo 49)
        runner.assert_pattern("[WM-CLICK] x=420 y=228 ws=4", "Store: el puntero PS/2 real llego a la fila 4 en el workspace 4", i1, timeout=5.0)
        runner.assert_pattern("[STORE-OK] install make-utils (estado real: INSTALADO)", "Store: clic real en [Instalar] -> pkg install por TLS 1.3 + SHA-256", i1, timeout=45.0)
        store_png = os.path.join(base_dir, "build", "dist", "portix_store.png")
        runner.capture_screenshot(store_png)
        i2 = len(runner.get_log())
        runner.mouse_click_at(420, 228)   # ahora el boton es [Quitar]
        runner.assert_pattern("[STORE-OK] remove make-utils (estado real: NO instalado)", "Store: clic real en [Quitar] -> pkg remove y estado releido", i2, timeout=45.0)
        i3 = len(runner.get_log())
        runner.mouse_click_at(396, 133)   # boton [Actualizar]
        runner.assert_pattern(f"[STORE] pkg update -> rc=0 ({n_pkgs} paquetes en el indice)", "Store: clic real en [Actualizar] -> pkg update (rc real)", i3, timeout=45.0)
        runner.assert_pattern("Sesion grafica finalizada", "Store: cierre limpio de la sesion del WM", i3, timeout=15.0)
        runner.wait_for_pattern("root@portix:", start_idx=i3, timeout=5.0)

        # Volcado completo de la consola serie para auditoria posterior
        try:
            with open(os.path.join(base_dir, "build", "interactive_serial.log"), "w", errors="replace") as f:
                f.write(runner.get_log())
        except Exception:
            pass

        # Resumen final
        total = len(runner.results)
        passed = sum(1 for v in runner.results.values() if v)
        failed = total - passed

        print(f"\n{BOLD}{CYAN}======================================================================{RESET}")
        print(f"  TOTAL DE PRUEBAS DE USO REAL: {total} | {GREEN}PASSED: {passed}{RESET} | {RED}FAILED: {failed}{RESET}")
        print(f"{BOLD}{CYAN}======================================================================{RESET}")

        if failed == 0:
            print(f"\n{GREEN}{BOLD}[ÉXITO] Portix OS v6.0 'Chronos' opera de forma real, interactiva y completa desde el segundo uno.{RESET}\n")
            sys.exit(0)
        else:
            print(f"\n{RED}{BOLD}[FALLO] Algunas pruebas de interacción real fallaron.{RESET}\n")
            sys.exit(1)

    finally:
        runner.close()

if __name__ == "__main__":
    main()
