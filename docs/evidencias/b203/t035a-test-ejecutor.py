#!/usr/bin/env python3
"""Controles de selección y aislamiento del ejecutor T-035."""
import unittest
from types import SimpleNamespace
from unittest.mock import patch
from scripts import sonda_t035_b203 as s


class Pruebas(unittest.TestCase):
    def test_defecto_ejecuta_todos(self):
        self.assertEqual(s.seleccionar_modos(""), s.MODOS)

    def test_seleccion_ordena_como_se_pide(self):
        self.assertEqual(s.seleccionar_modos("fork,madvise"), ("fork", "madvise"))

    def test_modo_inventado_se_rechaza(self):
        with self.assertRaises(ValueError):
            s.seleccionar_modos("fork,inventado")

    def test_duplicado_se_rechaza(self):
        with self.assertRaises(ValueError):
            s.seleccionar_modos("fork,fork")

    def test_lista_con_elemento_vacio_se_rechaza(self):
        with self.assertRaises(ValueError):
            s.seleccionar_modos("fork,")

    def test_prohibe_inspeccionar_vm_activa(self):
        r = SimpleNamespace(proc=SimpleNamespace(poll=lambda: None), _img_copia="no-abrir.img")
        with patch("builtins.open", side_effect=AssertionError("no se debe abrir")):
            self.assertFalse(s.verificar_elf_disco_detenido(r, "0" * 64))

    def test_vm_sin_copia_no_pasa(self):
        r = SimpleNamespace(proc=SimpleNamespace(poll=lambda: 0), _img_copia=None)
        self.assertFalse(s.verificar_elf_disco_detenido(r, "0" * 64))


if __name__ == "__main__":
    unittest.main()
