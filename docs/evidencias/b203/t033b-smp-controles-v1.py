#!/usr/bin/env python3
"""Controles positivos, negativos y de falsificación del predicado T-033 SMP."""
import unittest

if __package__:
    from .verificaciones_t033_smp import MODOS, NOMBRE, lineas_esperadas, resultado_smp
else:
    from verificaciones_t033_smp import MODOS, NOMBRE, lineas_esperadas, resultado_smp


def serie(modo="munmap", pid=7):
    codigo = 1 if modo.startswith("control-") else 0
    lineas = [f"[R3] enter PID={pid} name='{NOMBRE}' entry=0x1", *lineas_esperadas(modo),
              f"PROC: exit PID={pid} name='{NOMBRE}' code={codigo} marcos_libres=100"]
    return "\n".join(lineas) + "\n"


class VerificacionSMP(unittest.TestCase):
    def test_cuatro_series_exactas(self):
        for modo in MODOS:
            with self.subTest(modo=modo):
                self.assertEqual(resultado_smp(serie(modo), modo)[:2], (True, True))

    def test_falta_entrada(self):
        t = serie().replace("[R3] enter", "ECO [R3] enter")
        self.assertEqual(resultado_smp(t, "munmap")[:2], (False, False))

    def test_entrada_duplicada(self):
        t = serie().replace("[R3] enter PID=7", "[R3] enter PID=7", 1)
        linea = t.splitlines()[0]
        self.assertEqual(resultado_smp(t.replace(linea, linea + "\n" + linea), "munmap")[:2], (False, False))

    def test_nombre_distinto(self):
        self.assertFalse(resultado_smp(serie().replace(NOMBRE, "/otro"), "munmap")[0])

    def test_hilo_no_sustituye_lider(self):
        t = serie().replace("exit PID=7", "exit PID=8")
        self.assertEqual(resultado_smp(t, "munmap")[:2], (False, False))

    def test_salida_duplicada(self):
        t = serie()
        linea = t.splitlines()[-1]
        self.assertEqual(resultado_smp(t + linea + "\n", "munmap")[:2], (False, True))

    def test_codigo_real_debe_ser_cero(self):
        self.assertEqual(resultado_smp(serie().replace("code=0", "code=1"), "munmap")[:2], (False, True))

    def test_codigo_control_debe_ser_uno(self):
        t = serie("control-munmap").replace("code=1", "code=137")
        self.assertEqual(resultado_smp(t, "control-munmap")[:2], (False, True))

    def test_control_no_acepta_error_generico(self):
        t = serie("control-munmap").replace("observado=valor:49,codigo:0,faltas:0", "señal inesperada")
        self.assertFalse(resultado_smp(t, "control-munmap")[0])

    def test_control_no_acepta_valor_distinto(self):
        t = serie("control-proteccion").replace("observado=valor:77", "observado=valor:0")
        self.assertFalse(resultado_smp(t, "control-proteccion")[0])

    def test_control_no_acepta_pass_falso(self):
        t = serie("control-munmap").replace(
            "[T033-SMP] modo=control-munmap fin",
            "[T033-SMP] PASS modo=control-munmap\n[T033-SMP] modo=control-munmap fin")
        self.assertFalse(resultado_smp(t, "control-munmap")[0])

    def test_control_no_acepta_dos_errores(self):
        t = serie("control-munmap").replace("fin cpu_lider", "fin cpu_lider", 1)
        pos = t.index("[T033-SMP] modo=control-munmap fin")
        t = t[:pos] + "[T033-SMP-ERROR] otro\n" + t[pos:]
        self.assertFalse(resultado_smp(t, "control-munmap")[0])

    def test_cpu_0_y_1_requeridas(self):
        self.assertFalse(resultado_smp(serie().replace("cpu_ap=1", "cpu_ap=0"), "munmap")[0])

    def test_maperr_exacto(self):
        t = serie().replace("senal=11 codigo=1", "senal=11 codigo=2")
        self.assertFalse(resultado_smp(t, "munmap")[0])

    def test_accerr_exacto(self):
        t = serie("proteccion").replace("senal=11 codigo=2", "senal=11 codigo=1")
        self.assertFalse(resultado_smp(t, "proteccion")[0])

    def test_una_sola_falta(self):
        self.assertFalse(resultado_smp(serie().replace("faltas=1", "faltas=2"), "munmap")[0])

    def test_direccion_correcta(self):
        self.assertFalse(resultado_smp(serie().replace("direccion=correcta", "direccion=otra"), "munmap")[0])

    def test_cero_nuevo_exacto(self):
        self.assertFalse(resultado_smp(serie().replace("directo inicial=0", "directo inicial=90"), "munmap")[0])

    def test_aislamiento_requerido(self):
        self.assertFalse(resultado_smp(serie().replace("aisladas=si", "aisladas=no"), "munmap")[0])

    def test_falta_etapa(self):
        self.assertFalse(resultado_smp(serie().replace("fase=remapeada", "fase=otra"), "munmap")[0])

    def test_etapa_duplicada(self):
        t = serie()
        linea = lineas_esperadas("munmap")[1]
        self.assertFalse(resultado_smp(t.replace(linea, linea + "\n" + linea), "munmap")[0])

    def test_orden_requerido(self):
        t = serie()
        a, b = lineas_esperadas("munmap")[2:4]
        self.assertFalse(resultado_smp(t.replace(a, "TEMP").replace(b, a).replace("TEMP", b), "munmap")[0])

    def test_error_adicional_en_positivo(self):
        t = serie().replace("[T033-SMP] PASS", "[T033-SMP-ERROR] ejemplo\n[T033-SMP] PASS")
        self.assertFalse(resultado_smp(t, "munmap")[0])

    def test_marca_despues_de_salida_no_cuenta(self):
        t = serie()
        linea = lineas_esperadas("munmap")[-1]
        self.assertFalse(resultado_smp(t.replace(linea + "\n", "") + linea + "\n", "munmap")[0])

    def test_modo_desconocido(self):
        with self.assertRaises(ValueError):
            resultado_smp(serie(), "otro")


if __name__ == "__main__":
    unittest.main(verbosity=2)
