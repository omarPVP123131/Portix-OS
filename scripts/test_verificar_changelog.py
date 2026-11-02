#!/usr/bin/env python3
"""Controles negativos y una comparación Git real para el changelog."""
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

from scripts import verificar_changelog as v


class Estructura(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.datos = (v.RAIZ / "CHANGELOG.md").read_bytes()

    def test_changelog_real_pasa(self):
        self.assertEqual(v.comprobar_formato(v.RAIZ, self.datos), [])

    def test_omitir_sin_publicar_falla(self):
        datos = self.datos.replace(b"## [Sin publicar]", b"## [Sin publicar omitido]", 1)
        self.assertTrue(any("[Sin publicar]" in e for e in v.comprobar_formato(v.RAIZ, datos)))

    def test_fecha_invalida_falla(self):
        patron = "Última actualización:".encode() + rb"\s*\d{4}-\d{2}-\d{2}"
        datos = re.sub(patron, "Última actualización: 2026-02-31".encode(), self.datos, count=1)
        self.assertNotEqual(datos, self.datos)
        self.assertIn("fecha de última actualización inválida", v.comprobar_formato(v.RAIZ, datos))

    def test_sin_fecha_falla(self):
        datos = self.datos.replace("Última actualización:".encode(), b"Fecha:", 1)
        self.assertIn("falta la fecha ISO de última actualización", v.comprobar_formato(v.RAIZ, datos))

    def test_cambio_historico_falla(self):
        datos = self.datos.replace(b"## [v0.1.0]", b"## [v0.1.1]", 1)
        self.assertTrue(any("tramo v1.0.0" in e for e in v.comprobar_formato(v.RAIZ, datos)))

    def test_v1_ausente_falla(self):
        datos = self.datos.replace(v.MARCA, b"## [v1.0.1]", 1)
        self.assertTrue(any("exactamente una entrada" in e for e in v.comprobar_formato(v.RAIZ, datos)))

    def test_enlace_roto_falla(self):
        datos = self.datos.replace(b"](AGENTS.md)", b"](archivo-que-no-existe.md)", 1)
        self.assertTrue(any("enlace local inexistente" in e for e in v.comprobar_formato(v.RAIZ, datos)))

    def test_enlace_fuera_del_proyecto_falla(self):
        datos = self.datos.replace(b"](AGENTS.md)", b"](../../fuera.md)", 1)
        self.assertTrue(any("fuera del proyecto" in e for e in v.comprobar_formato(v.RAIZ, datos)))

    def test_invalida_sin_certificacion_falla(self):
        actual, legado = self.datos.split(v.MARCA, 1)
        actual = actual.replace(b"no certific", b"desconocido")
        self.assertTrue(any("certificación" in e for e in v.comprobar_formato(v.RAIZ, actual + v.MARCA + legado)))


class GitReal(unittest.TestCase):
    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.raiz, capture_output=True, check=True, text=True).stdout.strip()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="portix-test-changelog-")
        self.raiz = Path(self.tmp.name)
        self.git("init", "--quiet")
        self.git("config", "user.name", "Prueba local")
        self.git("config", "user.email", "prueba@example.invalid")
        (self.raiz / "kernel").mkdir()
        (self.raiz / "kernel/a.rs").write_text("// base\n")
        (self.raiz / "CHANGELOG.md").write_bytes((v.RAIZ / "CHANGELOG.md").read_bytes())
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        self.base = self.git("rev-parse", "HEAD")

    def tearDown(self):
        self.tmp.cleanup()

    def test_codigo_sin_changelog_rechazado_y_actualizado_aceptado(self):
        (self.raiz / "kernel/a.rs").write_text("// cambio real\n")
        self.git("add", ".")
        self.git("commit", "-qm", "sin changelog")
        self.assertTrue(any("sin actualizar CHANGELOG" in e for e in v.comprobar_cambios_git(self.raiz, self.base)))
        p = self.raiz / "CHANGELOG.md"
        p.write_bytes(p.read_bytes().replace(b"## [Sin publicar]", b"## [Sin publicar]\n\n- Cambio documentado.", 1))
        self.git("add", ".")
        self.git("commit", "-qm", "documentar")
        self.assertEqual(v.comprobar_cambios_git(self.raiz, self.base), [])

    def test_archivo_irrelevante_no_exige_entrada(self):
        (self.raiz / "archivo-local.txt").write_text("nota\n")
        self.git("add", ".")
        self.git("commit", "-qm", "nota")
        self.assertEqual(v.comprobar_cambios_git(self.raiz, self.base), [])

    def test_base_invalida_rechazada(self):
        self.assertTrue(v.comprobar_cambios_git(self.raiz, "HEAD;echo-mal"))


if __name__ == "__main__":
    unittest.main()
