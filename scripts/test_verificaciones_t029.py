#!/usr/bin/env python3
"""Controles positivos y negativos de las pruebas de binario Linux T-029."""

import gzip
import io
import threading
import unittest
from contextlib import redirect_stdout
from pathlib import Path

if __package__:
    from .verificaciones_t029 import (ejecutar_busybox, paquete_busybox_verificado,
                                      registrar_paquete_busybox, salida_busybox)
else:
    from verificaciones_t029 import (ejecutar_busybox, paquete_busybox_verificado,
                                     registrar_paquete_busybox, salida_busybox)

ROOT = Path(__file__).resolve().parents[1]
SHA = "ab" * 32
DESCARGA = "[+] Descargando https://10.0.2.2:8443/busybox-static.pkg -> /usr/local/pkg/cache/busybox-static.pkg\n"
COMPLETA = "[+] Descarga completada: 1026000 bytes\n"
VERIFICADA = "[+] SHA-256 verificado: " + SHA + "\n"
INSTALADA = "[OK] Paquete 'busybox-static' (1.36.1-r31 (Alpine v3.20)) instalado exitosamente.\n"


def proceso(pid="33", salida="100", code="0", pid_salida=None):
    return ("root@portix:$ run /bin/busybox expr 45 + 55\r\n"
            f"[R3] enter PID={pid} name='/bin/busybox' entry=0x400000\r\n"
            f"{salida}\n"
            f"PROC: exit PID={pid_salida or pid} name='/bin/busybox' code={code} marcos_libres=3\r\n"
            "root@portix:$ ")


class Paquete(unittest.TestCase):
    def test_completo(self):
        self.assertTrue(paquete_busybox_verificado(DESCARGA + COMPLETA + VERIFICADA + INSTALADA)[0])

    def test_sin_red_no_basta_nombre_en_url(self):
        self.assertFalse(paquete_busybox_verificado(VERIFICADA + INSTALADA)[0])

    def test_descarga_no_confirmada_no_basta(self):
        self.assertFalse(paquete_busybox_verificado(DESCARGA + VERIFICADA + INSTALADA)[0])

    def test_descarga_vacia_no_basta(self):
        self.assertFalse(paquete_busybox_verificado(DESCARGA + COMPLETA.replace("1026000", "0") + VERIFICADA + INSTALADA)[0])

    def test_hash_incompleto_no_basta(self):
        self.assertFalse(paquete_busybox_verificado(DESCARGA + COMPLETA + VERIFICADA[:-10] + "\n" + INSTALADA)[0])

    def test_instalacion_anterior_no_basta(self):
        self.assertFalse(paquete_busybox_verificado(INSTALADA + DESCARGA + COMPLETA + VERIFICADA)[0])

    def test_sin_instalacion_no_basta(self):
        self.assertFalse(paquete_busybox_verificado(DESCARGA + COMPLETA + VERIFICADA)[0])


class ProcesoExterno(unittest.TestCase):
    def test_linea_y_cero(self):
        self.assertTrue(salida_busybox(proceso(), "100")[0])

    def test_subcadena_1000_no_es_100(self):
        self.assertFalse(salida_busybox(proceso(salida="1000"), "100")[0])

    def test_codigo_de_error_no_es_exito(self):
        self.assertFalse(salida_busybox(proceso(code="2"), "100")[0])

    def test_otro_pid_no_es_la_misma_ejecucion(self):
        self.assertFalse(salida_busybox(proceso(pid_salida="34"), "100")[0])

    def test_eco_no_es_salida(self):
        self.assertFalse(salida_busybox(proceso(salida="run /bin/busybox echo HOLA"), "HOLA")[0])

    def test_nombre_de_otro_binario_no_vale(self):
        self.assertFalse(salida_busybox(proceso().replace("'/bin/busybox'", "'/bin/echo'"), "100")[0])

    def test_sin_entrada_ring3_no_vale(self):
        self.assertFalse(salida_busybox(proceso().replace("[R3] enter", "[R3] otra"), "100")[0])

    def test_linea_vacia_no_es_prueba(self):
        self.assertFalse(salida_busybox(proceso(), "")[0])

    def test_series_reales_previas(self):
        for letra in "bcde":
            p = ROOT / "docs" / "evidencias" / "b203" / f"serie_suite113{letra}.log.gz"
            with gzip.open(p, "rt", encoding="utf8", errors="replace") as f:
                texto = f.read()
            with self.subTest(serie=letra):
                self.assertTrue(paquete_busybox_verificado(texto)[0])
                self.assertTrue(salida_busybox(texto, "x86_64")[0])


class Integracion(unittest.TestCase):
    class Proc:
        def poll(self): return None
    class Runner:
        def __init__(self):
            self.log_lock = threading.Lock()
            self.log = ""
            self.results = {}
            self.proc = Integracion.Proc()
            self._tramos = []
            self.omitidas = 0
            self.activa = True
        def _seccion_activa(self): return self.activa
        def wait_for_pattern(self, *args, **kwargs): return True, "root@portix:"

    def test_ejecutor_registra_exito_real(self):
        r = self.Runner()
        def enviar(_): r.log += proceso()
        with redirect_stdout(io.StringIO()):
            ok = ejecutar_busybox(r, enviar, "orden", "100", "prueba", timeout=0.2)
        self.assertTrue(ok)
        self.assertIs(r.results["prueba"], True)

    def test_ejecutor_falla_con_salida_distinta(self):
        r = self.Runner()
        def enviar(_): r.log += proceso(salida="101")
        with redirect_stdout(io.StringIO()):
            ok = ejecutar_busybox(r, enviar, "orden", "100", "prueba", timeout=0.1)
        self.assertFalse(ok)
        self.assertIs(r.results["prueba"], False)

    def test_seccion_omitida_no_registra_pass(self):
        r = self.Runner()
        r.activa = False
        with redirect_stdout(io.StringIO()):
            ejecutar_busybox(r, lambda _: self.fail("no debe enviar"), "orden", "100", "prueba")
            registrar_paquete_busybox(r, 0, "paquete")
        self.assertEqual(r.results, {})
        self.assertEqual(r.omitidas, 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
