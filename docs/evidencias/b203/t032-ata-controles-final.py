#!/usr/bin/env python3
"""Controles positivos y negativos del clasificador de arranque T-032."""
import contextlib
import io
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

if __package__:
    from . import sonda_ata_b203 as sonda
    from .sonda_ata_b203 import argumentos_traza, clasificar_arranque, conmutar_estado_alternativo, traza_incluye_reset_primario
else:
    import sonda_ata_b203 as sonda
    from sonda_ata_b203 import argumentos_traza, clasificar_arranque, conmutar_estado_alternativo, traza_incluye_reset_primario

EVIDENCIA = Path(__file__).resolve().parents[1] / "docs/evidencias/b203"
BUENA = ("ATA [pri/master] ATA   LBA48  2085 MiB  QEMU HARDDISK\n"
         "[FS] dev0_kind=1\n"
         "[EXT2] montado en / (raiz): lba=75776 bloque=4096\n"
         "[R3] init process PID=1 registered\n")


class Pruebas(unittest.TestCase):
    def test_traza_solo_incluye_eventos_acotados(self):
        a = argumentos_traza(Path("/home/user/portix/docs/evidencias/b203/traza.log"))
        self.assertIn("enable=ide_ctrl_write", a)
        self.assertIn("enable=ide_bus_exec_cmd", a)
        self.assertIn("enable=ide_ioport_read", a)
        self.assertNotIn("enable=ide_data_readw", a)
        self.assertNotIn("enable=ide_status_read", a)
        self.assertEqual(a[-1], "file=/home/user/portix/docs/evidencias/b203/traza.log")

    def test_qmp_no_acepta_error_ni_json_invalido(self):
        class Falso:
            def __init__(self, dato): self.dato = dato
            def qmp(self, *_): return self.dato
        self.assertTrue(conmutar_estado_alternativo(Falso(b'{"return": {}}'), True))
        self.assertFalse(conmutar_estado_alternativo(Falso(b'{"error": {}}'), True))
        self.assertFalse(conmutar_estado_alternativo(Falso(b'ERR'), True))

    def test_traza_exige_lectura_durante_reset_primario(self):
        iniciar = 'ide_ctrl_write IDE PIO wr @ 0x3f6 (Device Control); val 0x06; bus 0x1\n'
        leer = 'ide_status_read IDE PIO rd @ 0x3f6 (Alt Status); val 0x00; bus 0x1\n'
        acabar = 'ide_ctrl_write IDE PIO wr @ 0x3f6 (Device Control); val 0x02; bus 0x1\n'
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'ide.log'
            for contenido, esperado in ((iniciar + leer + acabar, True),
                                        (leer + iniciar + acabar, False),
                                        (iniciar + acabar + leer, False),
                                        (iniciar + leer.replace('0x3f6', '0x376') + acabar, False)):
                p.write_text(contenido)
                self.assertEqual(traza_incluye_reset_primario(p), esperado)

    def test_error_al_cerrar_conserva_fallo_y_log(self):
        class Proceso:
            args = ["qemu"]
            def poll(self): return None
        class Ejecutor:
            def __init__(self, *args, **kw):
                self.log, self.log_lock, self.results = "[MODO DEGRADADO]", threading.Lock(), {}
                self.proc = Proceso()
            def start_qemu(self, **kw): pass
            def close(self): raise OSError("cierre ficticio")
        with tempfile.TemporaryDirectory() as d, patch.object(sonda, "PortixHeadlessRunner", Ejecutor):
            salida = Path(d) / "serie.log"
            ok, detalle, _ = sonda.pasada(Path(d) / "entrada.dat", salida, 5)
            self.assertFalse(ok)
            self.assertIn("excepción al cerrar", detalle)
            self.assertIn("resultado=FAIL", salida.read_text())

    def test_integridad_fallida_no_cambia_el_numero_de_arranques(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "entrada.dat"
            p.touch()
            args = ["sonda", "--img", str(p), "--sha256", "a" * 64, "--etiqueta", "test", "--repeticiones", "2"]
            flujo = io.StringIO()
            with patch.object(sonda, "EVIDENCIA", Path(d)), patch.object(sonda, "dentro_de_dist", return_value=True), \
                 patch.object(sonda, "sha256", side_effect=["a" * 64, "b" * 64]), \
                 patch.object(sonda, "pasada", return_value=(True, "ok", 0.0)), \
                 patch("sys.argv", args), contextlib.redirect_stdout(flujo):
                self.assertEqual(sonda.main(), 1)
            self.assertIn("2/2 arranques; integridad=FAIL", flujo.getvalue())

    def test_arranque_completo(self):
        self.assertEqual(clasificar_arranque(BUENA)[:2], (True, True))

    def test_modo_degradado_no_es_aprobado(self):
        self.assertEqual(clasificar_arranque(BUENA + "[MODO DEGRADADO] sin raiz\n")[:2], (False, True))

    def test_sin_init_no_es_concluyente(self):
        self.assertEqual(clasificar_arranque(BUENA.replace("[R3] init process PID=1 registered\n", ""))[:2], (False, False))

    def test_disco_secundario_no_sustituye_al_primario(self):
        self.assertFalse(clasificar_arranque(BUENA.replace("pri/master", "sec/master"))[0])

    def test_atapi_no_es_disco_ata(self):
        self.assertFalse(clasificar_arranque(BUENA.replace(" ATA   LBA48", " ATAPI LBA48"))[0])

    def test_capacidad_cero_no_cuenta(self):
        self.assertFalse(clasificar_arranque(BUENA.replace("2085 MiB", "0 MiB"))[0])

    def test_raiz_ausente_falla(self):
        self.assertFalse(clasificar_arranque(BUENA.replace("montado en / (raiz)", "montado en /tmp"))[0])

    def test_init_como_subcadena_no_cuenta(self):
        self.assertFalse(clasificar_arranque(BUENA.replace("[R3] init", "FALSO [R3] init"))[0])

    def test_device_0_incorrecto_falla(self):
        self.assertFalse(clasificar_arranque(BUENA.replace("dev0_kind=1", "dev0_kind=0"))[0])

    def test_serie_real_con_raiz(self):
        texto = (EVIDENCIA / "sonda-java-reuso118b.log").read_text(encoding="utf8", errors="replace")
        self.assertTrue(clasificar_arranque(texto)[0])

    def test_serie_real_fallida(self):
        texto = (EVIDENCIA / "sonda-java-reuso118.log").read_text(encoding="utf8", errors="replace")
        self.assertEqual(clasificar_arranque(texto)[:2], (False, True))


if __name__ == "__main__":
    unittest.main(verbosity=2)
