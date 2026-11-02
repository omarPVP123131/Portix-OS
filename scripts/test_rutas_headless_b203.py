#!/usr/bin/env python3
"""Pruebas cortas de rutas/aislamiento; no sustituyen un arranque QEMU."""

import contextlib
import io
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

if __package__:
    from .headless_runner import (DIST_IMAGENES, ROOT_PROYECTO, RUN_IMAGENES,
                                  PortixHeadlessRunner, ejecutar_con_limpieza, imagen_dentro_de_dist)
else:
    from headless_runner import (DIST_IMAGENES, ROOT_PROYECTO, RUN_IMAGENES,
                                 PortixHeadlessRunner, ejecutar_con_limpieza, imagen_dentro_de_dist)


class RutasHeadless(unittest.TestCase):
    def setUp(self):
        Path(RUN_IMAGENES).mkdir(parents=True, exist_ok=True)
        self.temporal = tempfile.TemporaryDirectory(prefix="test-rutas-", dir=RUN_IMAGENES)
        self.addCleanup(self.temporal.cleanup)
        self.dir = Path(self.temporal.name)
        self.origen = self.dir / (self.dir.name + "-original.img")
        self.origen.write_bytes(b"PORTIX-TEST-ORIGINAL")

    def crear_ejecutor(self):
        with contextlib.redirect_stdout(io.StringIO()):
            return PortixHeadlessRunner(str(self.origen), silicon=False)

    def test_copia_aislada_y_limpieza(self):
        r = self.crear_ejecutor()
        copia = Path(r.img_path)
        self.assertNotEqual(copia, self.origen)
        self.assertTrue(imagen_dentro_de_dist(str(copia)))
        self.assertEqual(copia.read_bytes(), self.origen.read_bytes())
        copia.write_bytes(b"MODIFICADA")
        self.assertEqual(self.origen.read_bytes(), b"PORTIX-TEST-ORIGINAL")
        with mock.patch.dict(os.environ, {}, clear=False), contextlib.redirect_stdout(io.StringIO()):
            os.environ.pop("PORTIX_CONSERVAR_IMG", None)
            r.close()
        self.assertFalse(copia.exists())
        self.assertTrue(self.origen.exists())

    def test_fallo_al_copiar_no_usa_original(self):
        with mock.patch("shutil.copyfile", side_effect=OSError("sin espacio")):
            with self.assertRaisesRegex(RuntimeError, "no se pudo copiar"):
                self.crear_ejecutor()
        self.assertEqual(self.origen.read_bytes(), b"PORTIX-TEST-ORIGINAL")

    def test_dos_copias_del_mismo_origen_no_colisionan(self):
        a = self.crear_ejecutor()
        b = self.crear_ejecutor()
        self.assertNotEqual(a.img_path, b.img_path)
        with mock.patch.dict(os.environ, {}, clear=False), contextlib.redirect_stdout(io.StringIO()):
            os.environ.pop("PORTIX_CONSERVAR_IMG", None)
            a.close()
            self.assertTrue(Path(b.img_path).exists())
            b.close()

    def test_solo_rutas_dentro_de_dist_y_sin_escape(self):
        self.assertTrue(imagen_dentro_de_dist(str(self.dir / "imagen.img")))
        self.assertFalse(imagen_dentro_de_dist(str(Path(ROOT_PROYECTO) / "build" / "fuera.img")))
        self.assertFalse(imagen_dentro_de_dist(str(Path(DIST_IMAGENES) / ".." / "fuera.img")))
        enlace = self.dir / "enlace-externo"
        enlace.symlink_to("/tmp", target_is_directory=True)
        self.assertFalse(imagen_dentro_de_dist(str(enlace / "imagen.img")))

    def test_conservar_fuera_se_rechaza_y_no_se_crea(self):
        fuera = Path(ROOT_PROYECTO) / "build" / (self.dir.name + "-no-crear.img")
        self.assertFalse(fuera.exists())
        r = self.crear_ejecutor()
        copia = Path(r.img_path)
        with mock.patch.dict(os.environ, {"PORTIX_CONSERVAR_IMG": str(fuera)}), contextlib.redirect_stdout(io.StringIO()):
            r.close()
        self.assertFalse(fuera.exists())
        self.assertTrue(copia.exists())  # conservada para recuperación tras el error
        self.assertIn(False, r.results.values())
        copia.unlink()

    def test_conservar_dentro_de_dist(self):
        destino = self.dir / "conservada.img"
        r = self.crear_ejecutor()
        with mock.patch.dict(os.environ, {"PORTIX_CONSERVAR_IMG": str(destino)}), contextlib.redirect_stdout(io.StringIO()):
            r.close()
        self.assertEqual(destino.read_bytes(), b"PORTIX-TEST-ORIGINAL")
        self.assertNotIn(False, r.results.values())

    def test_conservar_no_sobrescribe_archivo_existente(self):
        destino = self.dir / "existente.img"
        destino.write_bytes(b"DATOS AJENOS")
        r = self.crear_ejecutor()
        copia = Path(r.img_path)
        with mock.patch.dict(os.environ, {"PORTIX_CONSERVAR_IMG": str(destino)}), contextlib.redirect_stdout(io.StringIO()):
            r.close()
        self.assertEqual(destino.read_bytes(), b"DATOS AJENOS")
        self.assertTrue(copia.exists())
        self.assertIn(False, r.results.values())
        copia.unlink()

    def test_iso_usa_copia_privada_como_disco_escribible(self):
        # Ambos archivos de prueba permanecen bajo dist; no se arranca QEMU.
        iso = self.dir / "prueba.iso"
        base = self.dir / "portix.img"
        iso.write_bytes(b"ISO")
        base.write_bytes(b"DISCO ORIGINAL")
        modulo = PortixHeadlessRunner.__module__
        with mock.patch(modulo + ".DIST_IMAGENES", str(self.dir)), contextlib.redirect_stdout(io.StringIO()):
            r = PortixHeadlessRunner(str(iso), is_iso=True, silicon=False)
        copia = Path(r._disco_iso)
        self.assertEqual(r.img_path, str(iso))
        self.assertNotEqual(copia, base)
        self.assertEqual(copia.read_bytes(), b"DISCO ORIGINAL")
        with mock.patch.dict(os.environ, {}, clear=False), contextlib.redirect_stdout(io.StringIO()):
            os.environ.pop("PORTIX_CONSERVAR_IMG", None)
            r.close()
        self.assertFalse(copia.exists())
        self.assertEqual(base.read_bytes(), b"DISCO ORIGINAL")

    def test_cierre_detiene_proceso_antes_de_retirar_imagen(self):
        r = self.crear_ejecutor()
        copia = Path(r.img_path)
        acciones = []
        class Proceso:
            def poll(self): return None
            def terminate(self): acciones.append("terminate")
            def wait(self, timeout):
                self_existe = copia.exists()
                if not self_existe:
                    raise AssertionError("se borró la imagen antes de esperar a QEMU")
                acciones.append("wait")
        r.proc = Proceso()
        with mock.patch.dict(os.environ, {}, clear=False), contextlib.redirect_stdout(io.StringIO()):
            os.environ.pop("PORTIX_CONSERVAR_IMG", None)
            r.close()
        self.assertEqual(acciones, ["terminate", "wait"])
        self.assertFalse(copia.exists())

    def test_cierre_es_idempotente(self):
        r = self.crear_ejecutor()
        with mock.patch.dict(os.environ, {}, clear=False), contextlib.redirect_stdout(io.StringIO()):
            os.environ.pop("PORTIX_CONSERVAR_IMG", None)
            r.close()
            r.close()
        self.assertNotIn(False, r.results.values())

    def test_interrupcion_cierra_y_no_se_convierte_en_pass(self):
        r = self.crear_ejecutor()
        copia = Path(r.img_path)
        with mock.patch.object(r, "run_tests", side_effect=KeyboardInterrupt), \
             mock.patch.dict(os.environ, {}, clear=False), contextlib.redirect_stdout(io.StringIO()):
            os.environ.pop("PORTIX_CONSERVAR_IMG", None)
            with self.assertRaises(KeyboardInterrupt):
                ejecutar_con_limpieza(r)
        self.assertFalse(copia.exists())
        self.assertTrue(self.origen.exists())

    def test_excepcion_cierra_y_se_propaga(self):
        r = self.crear_ejecutor()
        copia = Path(r.img_path)
        with mock.patch.object(r, "run_tests", side_effect=RuntimeError("fallo de prueba")), \
             mock.patch.dict(os.environ, {}, clear=False), contextlib.redirect_stdout(io.StringIO()):
            os.environ.pop("PORTIX_CONSERVAR_IMG", None)
            with self.assertRaisesRegex(RuntimeError, "fallo de prueba"):
                ejecutar_con_limpieza(r)
        self.assertFalse(copia.exists())

    def test_interrupcion_conserva_solo_si_se_pide(self):
        r = self.crear_ejecutor()
        destino = self.dir / "interrumpida.img"
        with mock.patch.object(r, "run_tests", side_effect=KeyboardInterrupt), \
             mock.patch.dict(os.environ, {"PORTIX_CONSERVAR_IMG": str(destino)}), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(KeyboardInterrupt):
                ejecutar_con_limpieza(r)
        self.assertEqual(destino.read_bytes(), b"PORTIX-TEST-ORIGINAL")

    def test_discos_de_hardware_en_dist_y_sin_hardware_no_se_inventan(self):
        for _, ruta, _, _ in PortixHeadlessRunner.SILICIO_DISCOS:
            self.assertTrue(imagen_dentro_de_dist(str(Path(ROOT_PROYECTO) / ruta)))
        r = self.crear_ejecutor()
        self.assertEqual(r._silicon_args(), [])
        with mock.patch.dict(os.environ, {}, clear=False), contextlib.redirect_stdout(io.StringIO()):
            os.environ.pop("PORTIX_CONSERVAR_IMG", None)
            r.close()


if __name__ == "__main__":
    unittest.main(verbosity=2)
