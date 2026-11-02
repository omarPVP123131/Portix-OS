#!/usr/bin/env python3
"""Rechazo de omisiones, códigos, PID y etapas adulteradas de T-035."""
import unittest
from scripts import verificaciones_t035_memoria as v


class Pruebas(unittest.TestCase):
    def texto_vm(self, modo, codigo=None, cuerpo=None, pid=7):
        if codigo is None:
            codigo = 1 if modo.startswith("control-") else 0
        if cuerpo is None:
            cuerpo = "\n".join(v.lineas_esperadas(modo))
        return (f"[R3] enter PID={pid} name='{v.NOMBRE}'\n" + cuerpo + "\n"
                + f"PROC: exit PID={pid} name='{v.NOMBRE}' code={codigo}\nroot@portix:\n")

    def test_todos_los_modos_aceptan_codigo_y_etapas_exactos(self):
        for modo in v.MODOS:
            with self.subTest(modo=modo):
                self.assertEqual(v.resultado_vm(self.texto_vm(modo), modo)[:2], (True, True))

    def test_control_que_no_falla_no_pasa(self):
        self.assertFalse(v.resultado_vm(self.texto_vm("control-munmap", codigo=0), "control-munmap")[0])

    def test_error_generico_no_cuenta_como_control(self):
        cuerpo = "\n".join(v.lineas_esperadas("control-madvise")).replace("observado=valor:77", "observado=otro-error")
        self.assertFalse(v.resultado_vm(self.texto_vm("control-madvise", cuerpo=cuerpo), "control-madvise")[0])

    def test_errno_ajeno_no_cuenta(self):
        cuerpo = "\n".join(v.lineas_esperadas("usuario")).replace("errno=14", "errno=13")
        self.assertFalse(v.resultado_vm(self.texto_vm("usuario", cuerpo=cuerpo), "usuario")[0])

    def test_hijo_no_sustituye_salida_del_lider(self):
        texto = self.texto_vm("fork").replace("PROC: exit PID=7", "PROC: exit PID=8")
        self.assertEqual(v.resultado_vm(texto, "fork")[:2], (False, False))

    def test_leader_sin_entrada_no_pasa(self):
        texto = self.texto_vm("fixed").replace("[R3] enter", "[R3] otra-entrada")
        self.assertEqual(v.resultado_vm(texto, "fixed")[:2], (False, False))

    def test_etapas_desordenadas_fallan(self):
        lineas = v.lineas_esperadas("fork")
        lineas[1], lineas[2] = lineas[2], lineas[1]
        self.assertFalse(v.resultado_vm(self.texto_vm("fork", cuerpo="\n".join(lineas)), "fork")[0])

    def test_etapa_duplicada_falla(self):
        lineas = v.lineas_esperadas("fixed")
        lineas.insert(2, lineas[1])
        self.assertFalse(v.resultado_vm(self.texto_vm("fixed", cuerpo="\n".join(lineas)), "fixed")[0])

    def test_error_adicional_falla(self):
        cuerpo = "\n".join(v.lineas_esperadas("usuario")) + "\n[T035-ERROR] otra cosa"
        self.assertFalse(v.resultado_vm(self.texto_vm("usuario", cuerpo=cuerpo), "usuario")[0])

    def test_control_no_acepta_pass(self):
        cuerpo = "\n".join(v.lineas_esperadas("control-fixed")) + "\n[T035] PASS modo=control-fixed"
        self.assertFalse(v.resultado_vm(self.texto_vm("control-fixed", cuerpo=cuerpo), "control-fixed")[0])

    def test_dos_salidas_del_mismo_lider_fallan(self):
        t = self.texto_vm("munmap").replace("root@portix:", f"PROC: exit PID=7 name='{v.NOMBRE}' code=0\nroot@portix:")
        self.assertEqual(v.resultado_vm(t, "munmap")[:2], (False, True))

    def test_perezosa_no_acepta_maperr_por_accerr(self):
        texto = self.texto_vm("perezosa").replace("senal=11 codigo=2", "senal=11 codigo=1")
        self.assertFalse(v.resultado_vm(texto, "perezosa")[0])

    def test_perezosa_exige_restauracion(self):
        texto = self.texto_vm("perezosa").replace("fase=restauracion inicial=0 nueva=77", "fase=restauracion inicial=1 nueva=77")
        self.assertFalse(v.resultado_vm(texto, "perezosa")[0])

    def test_control_hueco_no_confunde_accerr(self):
        texto = self.texto_vm("control-perezosa-desmapeada").replace("codigo:1,faltas:1", "codigo:2,faltas:1")
        self.assertFalse(v.resultado_vm(texto, "control-perezosa-desmapeada")[0])

    def test_modo_ajeno_lanza_error(self):
        with self.assertRaises(ValueError):
            v.lineas_esperadas("inventado")


if __name__ == "__main__":
    unittest.main()
