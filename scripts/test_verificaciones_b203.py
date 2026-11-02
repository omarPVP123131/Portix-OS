#!/usr/bin/env python3
"""Pruebas cortas del ejecutor; los controles sintéticos no certifican QEMU."""

import contextlib
import gzip
import io
import threading
import unittest
from pathlib import Path

if __package__:
    from .headless_runner import PortixHeadlessRunner
    from .verificaciones_b203 import (paginas_desde_fichero, verificar_delta, verificar_irq_java,
                                     verificar_libstdcpp, verificar_mapeos_java)
else:
    from headless_runner import PortixHeadlessRunner
    from verificaciones_b203 import (paginas_desde_fichero, verificar_delta, verificar_irq_java,
                                    verificar_libstdcpp, verificar_mapeos_java)

VERIFICACION = "[DEB] libstdc++6_14.2.0-19_amd64.deb: Size OK, SHA256 ab1fa05837aa7a92 OK, control: libstdc++6 14.2.0-19"
INSTALACION = "[DEB] libstdc++6 14.2.0-19 instalado (8 entradas)"
PLAN = "[DEB-OK] instalados: otro_1 libstdc++6_14.2.0-19 rustc_1"
SERIE_BUENA = VERIFICACION + "\n" + INSTALACION + "\n" + PLAN + "\n"
CONTADOR = "mmap_fichero_perezoso: tramos_vivos 0 registrados 971 paginas_desde_fichero 47268 bytes 193413058"
MMAP_LENTO = "[SYS] MMAP perezoso addr=0x300000000000 pages=34389 prot=1 fd=4"
MMAP_ANSIOSO = "[SYS] MMAP addr=0x300000000000 pages=30000 fd=4"


class PaqueteDebian(unittest.TestCase):
    def comprobar_fallo(self, texto):
        ok, _ = verificar_libstdcpp(texto)
        self.assertFalse(ok)

    def test_confirmacion_completa(self):
        ok, detalle = verificar_libstdcpp(SERIE_BUENA.replace("\n", "\r\n"))
        self.assertTrue(ok)
        self.assertIn("ab1fa05837aa7a92", detalle)

    def test_nombre_en_url_no_basta(self):
        self.comprobar_fallo("[CURL] https://repo/libstdc++6_14.2.0-19_amd64.deb\n" + INSTALACION + "\n" + PLAN)

    def test_falta_confirmacion_individual(self):
        self.comprobar_fallo(VERIFICACION + "\n" + PLAN)

    def test_falta_plan_completo(self):
        self.comprobar_fallo(VERIFICACION + "\n" + INSTALACION)

    def test_plan_anterior_no_vale(self):
        self.comprobar_fallo(PLAN + "\n" + VERIFICACION + "\n" + INSTALACION)

    def test_instalacion_anterior_no_vale(self):
        self.comprobar_fallo(INSTALACION + "\n" + VERIFICACION + "\n" + PLAN)

    def test_instalacion_vacia_no_vale(self):
        self.comprobar_fallo(SERIE_BUENA.replace("(8 entradas)", "(0 entradas)"))

    def test_otro_paquete_o_version_no_valen(self):
        self.comprobar_fallo(SERIE_BUENA.replace("libstdc++6_14.2.0-19 rustc_1", "libstdc++6_14.2.0-19-extra rustc_1"))
        self.comprobar_fallo(SERIE_BUENA.replace("control: libstdc++6 14.2.0-19", "control: libstdc++6 14.2.0-20"))

    def test_firma_de_informe_malformada(self):
        self.comprobar_fallo(SERIE_BUENA.replace("SHA256 ab1fa05837aa7a92 OK", "SHA256 incorrecto OK"))
        self.comprobar_fallo(SERIE_BUENA.replace("Size OK", "Size FAIL"))

    def test_un_segundo_intento_puede_completar(self):
        ok, _ = verificar_libstdcpp(VERIFICACION + "\nfallo de instalación\n" + SERIE_BUENA)
        self.assertTrue(ok)


class ContadoresMmap(unittest.TestCase):
    def test_contador_completo_y_cero_real(self):
        self.assertEqual(paginas_desde_fichero(CONTADOR + "\r\n"), 47268)
        self.assertEqual(paginas_desde_fichero(CONTADOR.replace("47268", "0")), 0)

    def test_ausente_incompleto_y_ambiguo(self):
        self.assertIsNone(paginas_desde_fichero("mmap_fichero_perezoso:"))
        self.assertIsNone(paginas_desde_fichero(CONTADOR.replace("bytes 193413058", "bytes")))
        self.assertIsNone(paginas_desde_fichero(CONTADOR + "\n" + CONTADOR))

    def test_delta_nunca_inventa_cero(self):
        self.assertEqual(verificar_delta(None, 13000), (False, None))
        self.assertEqual(verificar_delta(47268, None), (False, None))
        self.assertEqual(verificar_delta(0, 1000), (True, 1000))
        self.assertEqual(verificar_delta(47268, 47267), (False, -1))

    def test_limites_exactos(self):
        for delta, valido in ((999, False), (1000, True), (34388, True), (34389, False)):
            with self.subTest(delta=delta):
                self.assertEqual(verificar_delta(47268, 47268 + delta), (valido, delta))

    def test_mapeo_grande_de_java_y_ansioso(self):
        self.assertEqual(verificar_mapeos_java(MMAP_LENTO), (True, 34389, 0))
        self.assertEqual(verificar_mapeos_java(MMAP_LENTO + "\n" + MMAP_ANSIOSO), (False, 34389, 30000))
        self.assertEqual(verificar_mapeos_java("sin mapeos de Java"), (False, 0, 0))

    def test_mapeo_de_otra_seccion_no_vale(self):
        toda_la_serie = MMAP_LENTO + "\nINICIO-JAVA\nsin mapeos de Java"
        ventana = toda_la_serie.split("INICIO-JAVA", 1)[1]
        self.assertTrue(verificar_mapeos_java(toda_la_serie)[0])
        self.assertFalse(verificar_mapeos_java(ventana)[0])


class LimitesIRQ(unittest.TestCase):
    @staticmethod
    def linea(perdidos, hueco):
        return f"irq0_perdidos: {perdidos} irq0_peor_hueco_us: {hueco} irq0_sitio: pid 9 syscall 11 rip 0xff\n"

    def test_dos_limites_estrictos(self):
        self.assertEqual(verificar_irq_java(self.linea(599, 299999)), (True, 599, 299999))
        self.assertEqual(verificar_irq_java(self.linea(600, 299999)), (False, 600, 299999))
        self.assertEqual(verificar_irq_java(self.linea(599, 300000)), (False, 599, 300000))

    def test_ausente_ambiguo_o_incompleto_falla(self):
        for texto in ("", self.linea(1, 2) * 2, "irq0_perdidos: 1 irq0_peor_hueco_us: 2\n"):
            self.assertEqual(verificar_irq_java(texto), (False, None, None))

    def test_linea_de_ejecucion_real_aislada(self):
        ruta = Path(__file__).resolve().parents[1] / "docs/evidencias/b203/t033-version-aislada118.log"
        texto = ruta.read_text(encoding="utf8", errors="replace")
        tramo = texto[texto.rfind("cat /proc/smp"):]
        self.assertEqual(verificar_irq_java(tramo), (True, 171, 219296))


class IntegracionEjecutor(unittest.TestCase):
    @staticmethod
    def ejecutor(log, solo=None, seccion="Java"):
        r = PortixHeadlessRunner.__new__(PortixHeadlessRunner)
        r.log, r.log_lock, r.results = log, threading.Lock(), {}
        r.solo, r.seccion_actual = solo, seccion
        r.omitidas, r._tramos = 0, []
        return r

    def test_registra_exito_y_fallo_reales_del_predicado(self):
        for log, esperado in ((SERIE_BUENA, True), ("https://repo/libstdc++6_14.2.0-19.deb", False)):
            with self.subTest(esperado=esperado), contextlib.redirect_stdout(io.StringIO()):
                r = self.ejecutor(log)
                self.assertEqual(r.verificar_libstdcpp_java(), esperado)
                self.assertEqual(list(r.results.values()), [esperado])
                self.assertEqual(len(r._tramos), 1)

    def test_solo_salta_java_sin_falso_veredicto(self):
        with contextlib.redirect_stdout(io.StringIO()):
            r = self.ejecutor("", solo=["T-005b"])
            self.assertTrue(r.verificar_libstdcpp_java())
            self.assertEqual(r.results, {})
            self.assertEqual(r.omitidas, 1)


class RegistrosReales(unittest.TestCase):
    def test_cuatro_pasadas_guardadas(self):
        directorio = Path(__file__).resolve().parent.parent / "docs/evidencias/b203"
        for letra in "bcde":
            with self.subTest(pasada=letra):
                ruta = directorio / f"serie_suite113{letra}.log.gz"
                with gzip.open(ruta, "rt", encoding="utf-8", errors="replace") as f:
                    serie = f.read()
                self.assertTrue(verificar_libstdcpp(serie)[0])
                deb = serie.index("run pkg deb-add openjdk-21-jre-headless")
                antes = serie.rfind("cat /proc/smp", 0, deb)
                java = serie.index("run /usr/lib/jvm/java-21-openjdk-amd64/bin/java -version", deb)
                clase = serie.index("run /usr/lib/jvm/java-21-openjdk-amd64/bin/java -cp /usr/local T010Java", java)
                despues = serie.index("cat /proc/smp", clase)
                fin = serie.index("run test_posix", despues)
                self.assertGreaterEqual(antes, 0)
                anterior = paginas_desde_fichero(serie[antes:deb])
                posterior = paginas_desde_fichero(serie[despues:fin])
                self.assertTrue(verificar_delta(anterior, posterior)[0])
                self.assertEqual(verificar_mapeos_java(serie[java:despues])[1], 34389)
                self.assertTrue(verificar_mapeos_java(serie[java:despues])[0])


if __name__ == "__main__":
    unittest.main(verbosity=2)
