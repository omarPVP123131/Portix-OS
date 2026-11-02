#!/usr/bin/env python3
"""Controles para que la regresión T-034 pueda fallar de verdad."""
import unittest
from pathlib import Path

if __package__:
    from .verificaciones_t034 import resultado
else:
    from verificaciones_t034 import resultado


def serie(marca, code=0, pid=2, pid_salida=2):
    return (f"[R3] enter PID={pid} name='/usr/local/t034' entry=0x1\n"
            f"{marca}\nPROC: exit PID={pid_salida} name='/usr/local/t034' code={code} marcos=1\n")


class Pruebas(unittest.TestCase):
    def test_directo_cero(self):
        self.assertEqual(resultado(serie("[T034-AP] cpu=1 exit_group=0"), "directo", 0)[:2], (True, True))

    def test_directo_42(self):
        self.assertTrue(resultado(serie("[T034-AP] cpu=1 exit_group=42", 42), "directo", 42)[0])

    def test_cero_no_pasa_con_137(self):
        ok, termino, _ = resultado(serie("[T034-AP] cpu=1 exit_group=0", 137), "directo", 0)
        self.assertEqual((ok, termino), (False, True))

    def test_otro_pid_no_cuenta(self):
        self.assertEqual(resultado(serie("[T034-AP] cpu=1 exit_group=0", pid_salida=3), "directo", 0)[:2], (False, False))

    def test_cambia_cpu_no_cuenta(self):
        self.assertFalse(resultado(serie("[T034-AP] cpu=0 exit_group=0"), "directo", 0)[0])

    def test_solo_eco_no_cuenta(self):
        self.assertFalse(resultado("run /usr/local/t034 directo 0\n", "directo", 0)[0])

    def test_wait_codigo_exacto(self):
        t = "[T034-WAIT] code=42 esperado=10752 observado=10752 pid_correcto=true OK"
        self.assertTrue(resultado(serie(t), "wait", 42)[0])
        self.assertFalse(resultado(serie(t.replace("observado=10752", "observado=9")), "wait", 42)[0])

    def test_wait_de_lider_dormido_exige_mismo_hijo_y_estado(self):
        t = ("[T-005] lider pid=5 salio con hilos vivos: espera al ultimo\n"
             "[T034-AP] cpu=1 exit_group=42\n"
             "[T034-WAIT-DORMIDO] hijo=5 code=42 esperado=10752 observado=10752 pid_correcto=true OK")
        self.assertTrue(resultado(serie(t), "wait-dormido", 42)[0])
        self.assertFalse(resultado(serie(t.replace("lider pid=5", "lider pid=6")), "wait-dormido", 42)[0])
        self.assertFalse(resultado(serie(t.replace("observado=10752", "observado=9")), "wait-dormido", 42)[0])
        self.assertFalse(resultado(serie(t.replace("cpu=1", "cpu=0")), "wait-dormido", 42)[0])

    def test_sigkill_no_se_confunde_con_salida_normal(self):
        t = "[T034-SIGKILL] esperado=9 observado=9 pid_correcto=true OK"
        self.assertTrue(resultado(serie(t), "sigkill")[0])
        self.assertFalse(resultado(serie(t.replace("observado=9", "observado=0")), "sigkill")[0])

    def test_sigkill_externo_multihilo_exige_pid_y_orden(self):
        t = ("[T034-VICTIMA] hilo activo en CPU 1\n"
             "[T-005] lider pid=5 salio con hilos vivos: espera al ultimo\n"
             "[T034-PADRE] SIGKILL a hijo=5\n"
             "[T034-SIGKILL-HILOS] hijo=5 listo=true envio=true esperado=9 observado=9 "
             "pid_correcto=true sin_proceso=true OK")
        self.assertTrue(resultado(serie(t), "sigkill-hilos")[0])
        self.assertFalse(resultado(serie(t.replace("lider pid=5", "lider pid=6")), "sigkill-hilos")[0])
        self.assertFalse(resultado(serie(t.replace("hijo=5 listo", "hijo=6 listo")), "sigkill-hilos")[0])
        self.assertFalse(resultado(serie(t.replace("observado=9", "observado=0")), "sigkill-hilos")[0])
        self.assertFalse(resultado(serie(t.replace("sin_proceso=true", "sin_proceso=false")), "sigkill-hilos")[0])
        cambiado = t.replace("[T-005] lider pid=5 salio con hilos vivos: espera al ultimo\n", "")
        self.assertFalse(resultado(serie(cambiado), "sigkill-hilos")[0])

    def test_sigkill_multihilo_serie_qemu_real(self):
        ruta = Path(__file__).resolve().parents[1] / "docs/evidencias/b203/t034-k117g-solo.log"
        self.assertTrue(resultado(ruta.read_text(encoding="utf8", errors="replace"), "sigkill-hilos")[0])

    def test_error_explicito_no_cuenta(self):
        t = "[T034-AP] cpu=1 exit_group=0\n[T034-ERROR]"
        self.assertFalse(resultado(serie(t), "directo", 0)[0])

    def test_lider_dormido_exige_rama_de_ap_y_mismo_pid(self):
        t = ("[T034-LIDER] exit(2) antes del cierre del grupo\n"
             "[T034-AP] cpu=1 exit_group=0\n"
             "[T-005] lider de consola cerrado desde un AP: lo termina el BSP (PID 2)")
        self.assertTrue(resultado(serie(t), "dormido", 0)[0])
        self.assertFalse(resultado(serie(t.replace("PID 2)", "PID 3)")), "dormido", 0)[0])
        self.assertFalse(resultado(serie(t.replace("[T034-LIDER]", "[SIN-LIDER]")), "dormido", 0)[0])


if __name__ == "__main__":
    unittest.main(verbosity=2)
