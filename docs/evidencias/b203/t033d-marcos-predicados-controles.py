#!/usr/bin/env python3
"""Controles de tablas x86-64, estados de PTE y reutilización física observada."""
import copy
import unittest

if __package__:
    from .verificaciones_t033_marcos import FASES, marca_fase, caminar_pte, evaluar_capturas
else:
    from verificaciones_t033_marcos import FASES, marca_fase, caminar_pte, evaluar_capturas


def presente(p, b): return {"pte": p | 7, "fisica": p, "byte": b}
def reserva(): return {"pte": (3 << 52) | (1 << 10) | 6, "fisica": None, "byte": None}
def ausente(): return {"pte": 0, "fisica": None, "byte": None}


def capturas():
    return {
        "antes": {"a": presente(0x100000, 49), "b": reserva(), "c": reserva()},
        "liberada": {"a": ausente(), "b": presente(0x100000, 183), "c": reserva()},
        "remapeada": {"a": presente(0x101000, 90), "b": presente(0x100000, 183), "c": reserva()},
        "segunda-liberada": {"a": ausente(), "b": presente(0x100000, 183), "c": presente(0x101000, 195)},
        "segunda-remapeada": {"a": presente(0x102000, 109), "b": presente(0x100000, 183), "c": presente(0x101000, 195)},
    }


class Marcos(unittest.TestCase):
    def test_fases_y_dos_reutilizaciones(self):
        ok, _, reusos = evaluar_capturas(capturas())
        self.assertTrue(ok)
        self.assertEqual(reusos, {"primera": True, "segunda": True})

    def test_sin_reutilizacion_no_se_inventa(self):
        c = capturas()
        for fase in ("liberada", "remapeada", "segunda-liberada", "segunda-remapeada"):
            c[fase]["b"] = presente(0x110000, 183)
        ok, detalle, reusos = evaluar_capturas(c)
        self.assertFalse(ok)
        self.assertIn("no observada", detalle)
        self.assertEqual(reusos, {"primera": False, "segunda": True})

    def test_segundo_reuso_no_se_inventa(self):
        c = capturas()
        for fase in ("segunda-liberada", "segunda-remapeada"):
            c[fase]["c"] = presente(0x111000, 195)
        ok, _, reusos = evaluar_capturas(c)
        self.assertFalse(ok)
        self.assertEqual(reusos, {"primera": True, "segunda": False})

    def test_pte_a_retirar_debe_ser_cero(self):
        c = capturas()
        c["liberada"]["a"] = reserva()
        self.assertFalse(evaluar_capturas(c)[0])

    def test_pagina_perezosa_no_es_presente(self):
        c = capturas()
        c["antes"]["b"] = presente(0x104000, 0)
        self.assertFalse(evaluar_capturas(c)[0])

    def test_bit_perezoso_requerido(self):
        c = capturas()
        c["antes"]["c"] = ausente()
        self.assertFalse(evaluar_capturas(c)[0])

    def test_byte_de_marco_exacto(self):
        c = capturas()
        c["segunda-remapeada"]["a"]["byte"] = 90
        self.assertFalse(evaluar_capturas(c)[0])

    def test_rechaza_alias_de_dos_marcos_presentes(self):
        c = capturas()
        c["remapeada"]["a"] = presente(0x100000, 90)
        self.assertFalse(evaluar_capturas(c)[0])

    def test_pte_y_marco_deben_coincidir(self):
        c = capturas()
        c["antes"]["a"]["fisica"] = 0x103000
        self.assertFalse(evaluar_capturas(c)[0])

    def test_marco_testigo_no_puede_cambiar_entre_fases(self):
        c = capturas()
        c["remapeada"]["b"] = presente(0x120000, 183)
        self.assertFalse(evaluar_capturas(c)[0])

    def test_no_contar_fase_ausente(self):
        c = capturas()
        del c["antes"]
        self.assertFalse(evaluar_capturas(c)[0])

    def test_no_contar_fase_adicional(self):
        c = capturas()
        c["extra"] = copy.deepcopy(c["antes"])
        self.assertFalse(evaluar_capturas(c)[0])

    def test_marca_exacta_y_direcciones(self):
        t = "[T033-MARCOS] fase=antes a=0x1000 b=0x2000 c=0x3000\n"
        self.assertEqual(marca_fase(t, "antes"), {"a": 0x1000, "b": 0x2000, "c": 0x3000})

    def test_marca_duplicada_rechazada(self):
        t = "[T033-MARCOS] fase=antes a=0x1000 b=0x2000 c=0x3000\n"
        self.assertIsNone(marca_fase(t + t, "antes"))

    def test_marca_con_virtual_duplicada_rechazada(self):
        self.assertIsNone(marca_fase("[T033-MARCOS] fase=antes a=0x1000 b=0x1000 c=0x3000\n", "antes"))

    def test_marca_desalineada_rechazada(self):
        self.assertIsNone(marca_fase("[T033-MARCOS] fase=antes a=0x1001 b=0x2000 c=0x3000\n", "antes"))

    def test_marca_en_kernel_rechazada(self):
        self.assertIsNone(marca_fase("[T033-MARCOS] fase=antes a=0xffff800000001000 b=0x2000 c=0x3000\n", "antes"))

    def tablas(self):
        va = 0x12345000
        indices = [(va >> s) & 511 for s in (39, 30, 21, 12)]
        memoria = {0x1000 + indices[0]*8: 0x2000 | 7,
                   0x2000 + indices[1]*8: 0x3000 | 7,
                   0x3000 + indices[2]*8: 0x4000 | 7,
                   0x4000 + indices[3]*8: 0x5000 | 7}
        return va, memoria

    def test_recorrido_cuatro_niveles(self):
        va, mem = self.tablas()
        self.assertEqual(caminar_pte(0x1000, va, mem.__getitem__), {"pte": 0x5007, "fisica": 0x5000})

    def test_pte_ausente_conserva_bits(self):
        va, mem = self.tablas()
        mem[0x4000 + ((va >> 12) & 511)*8] = (1 << 10) | 6
        self.assertEqual(caminar_pte(0x1000, va, mem.__getitem__), {"pte": 1030, "fisica": None})

    def test_intermedia_no_presente(self):
        va, mem = self.tablas()
        mem[0x1000 + ((va >> 39) & 511)*8] = 0
        with self.assertRaises(ValueError): caminar_pte(0x1000, va, mem.__getitem__)

    def test_intermedia_supervisor_rechazada(self):
        va, mem = self.tablas()
        mem[0x1000 + ((va >> 39) & 511)*8] = 0x2003
        with self.assertRaises(ValueError): caminar_pte(0x1000, va, mem.__getitem__)

    def test_pagina_grande_no_es_pte_de_4k(self):
        va, mem = self.tablas()
        mem[0x3000 + ((va >> 21) & 511)*8] |= 0x80
        with self.assertRaises(ValueError): caminar_pte(0x1000, va, mem.__getitem__)

    def test_cr3_desalineado(self):
        va, mem = self.tablas()
        with self.assertRaises(ValueError): caminar_pte(0x1001, va, mem.__getitem__)

    def test_pagina_presente_supervisor_rechazada(self):
        va, mem = self.tablas()
        mem[0x4000 + ((va >> 12) & 511)*8] = 0x5003
        with self.assertRaises(ValueError): caminar_pte(0x1000, va, mem.__getitem__)


if __name__ == "__main__":
    unittest.main(verbosity=2)
