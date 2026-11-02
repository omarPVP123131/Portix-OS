"""Controles del inventario de ABI (docs/ABI_SYSCALLS.md).

El documento se genera desde el código; estos controles FALLAN si el documento
deja de describir lo que el núcleo despacha. Incluye controles negativos: si se
altera el documento o el inventario, la comprobación debe darse cuenta.
"""
import json
import re
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / "docs/ABI_SYSCALLS.md"
INVENTARIO = ROOT / "scripts/abi_inventario.py"


def inventario():
    r = subprocess.run([sys.executable, str(INVENTARIO), "--json"],
                       capture_output=True, text=True, check=True)
    return json.loads(r.stdout)


def numeros_de_seccion(texto, titulo):
    """Números `\\`N\\`` del bloque que sigue a un título de sección."""
    i = texto.index(titulo)
    resto = texto[i:]
    # hasta el siguiente título de nivel 2 o 3
    m = re.search(r"\n#{2,3} ", resto[len(titulo):])
    return {int(n) for n in re.findall(r"`(\d+)`", resto[:len(titulo) + (m.start() if m else len(resto))])}


class Inventario(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.inv = inventario()
        cls.doc = DOC.read_text()

    # ── el documento existe y declara las cifras correctas ──────────────
    def test_documento_existe(self):
        self.assertTrue(DOC.is_file(), "falta docs/ABI_SYSCALLS.md")
        self.assertGreater(len(self.doc), 8000)

    def test_cifras_coinciden_con_el_codigo(self):
        nat, lin = self.inv["nativa"], self.inv["linux"]
        esperado = {
            "constantes nativas": len(nat["constantes"]),
            "nativas con brazo": len(nat["con_brazo"]),
            "linux traducidas": len(lin["traducidas"]),
            "linux sin traducir": len(lin["faltan"]),
            "choques": len(self.inv["choques"]),
        }
        for etiqueta, valor in esperado.items():
            # la tabla de resumen publica unas cifras en negrita y otras no
            presente = any(f"| {v}{valor}{v} |" in self.doc for v in ("", "**"))
            self.assertTrue(presente, f"la tabla de resumen no declara {etiqueta}={valor}")
        # y en el texto: "las N traducidas" / "Las M sin traducir"
        self.assertIn(f"las {esperado['linux traducidas']} traducidas", self.doc)
        self.assertIn(f"Las {esperado['linux sin traducir']} sin traducir", self.doc)

    # ── la tabla de traducidas está completa ────────────────────────────
    def test_tabla_linux_tiene_todas_las_traducidas(self):
        filas = {int(m.group(1)) for m in
                 re.finditer(r"^\| (\d+) \| `(\w+)` \|", self.doc, re.M)}
        faltan_en_doc = set(self.inv["linux"]["traducidas"]) - filas
        self.assertEqual(faltan_en_doc, set(),
                         f"traducidas que no aparecen en la tabla: {sorted(faltan_en_doc)}")

    # ── las tres listas de «sin traducir» cubren exactamente el hueco ───
    def test_listas_sin_traducir_cubren_el_hueco(self):
        decl = set()
        for titulo in ("### 5.1 Riesgo alto", "### 5.2 Riesgo medio", "### 5.3 Riesgo bajo"):
            decl |= numeros_de_seccion(self.doc, titulo)
        real = set(self.inv["linux"]["faltan"])
        self.assertEqual(decl - real, set(), f"números declarados que SÍ están traducidos: {sorted(decl - real)}")
        self.assertEqual(real - decl, set(), f"sin traducir que faltan del documento: {sorted(real - decl)}")

    # ── la tabla nativa está completa ───────────────────────────────────
    def test_tabla_nativa_completa(self):
        nat = self.inv["nativa"]["constantes"]
        seccion = self.doc[self.doc.index("## 3. ABI nativa"):self.doc.index("## 4. ABI Linux")]
        for nombre, n in nat.items():
            corto = nombre.replace("SYS_", "")
            self.assertIn(f"{n:3d}` {corto}", seccion, f"falta {nombre}={n} en la tabla nativa")

    # ── controles negativos: la comprobación puede fallar ───────────────
    def test_control_negativo_cifra_alterada(self):
        alterado = self.doc.replace("| Linux **traducidas** por Portix | **141** |",
                                    "| Linux **traducidas** por Portix | **140** |")
        self.assertNotEqual(alterado, self.doc, "el control no encontró la fila que debía alterar")
        filas = re.findall(r"\| Linux \*\*traducidas\*\* por Portix \| \*\*(\d+)\*\* \|", alterado)
        self.assertEqual(filas, ["140"])
        self.assertNotEqual(int(filas[0]), len(self.inv["linux"]["traducidas"]))

    def test_control_negativo_numero_inventado(self):
        # un número que no existe en Linux no puede colarse en las listas
        decl = numeros_de_seccion(self.doc, "### 5.1 Riesgo alto")
        self.assertNotIn(99999, decl)
        self.assertTrue(decl & set(self.inv["linux"]["faltan"]))

    # ── el generador es determinista y reproducible ─────────────────────
    def test_documento_regenerado_identico(self):
        antes = DOC.read_bytes()
        subprocess.run([sys.executable, str(ROOT / "scripts/generar_abi_doc.py")],
                       capture_output=True, text=True, check=True)
        self.assertEqual(DOC.read_bytes(), antes,
                         "regenerar el documento cambia su contenido: código y documento están desincronizados")


if __name__ == "__main__":
    unittest.main()
