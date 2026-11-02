#!/usr/bin/env python3
"""Controles negativos para que la sonda Java no acepte ecos ni procesos ajenos."""
import gzip
import re
import unittest
from pathlib import Path

if __package__:
    from .verificaciones_java_b203 import NOMBRE, MARCADOR, resultado_java
    from .sonda_java_reuso_b203 import ordenes_java, resumen_progreso_java, cpu_qemu_segundos
else:
    from verificaciones_java_b203 import NOMBRE, MARCADOR, resultado_java
    from sonda_java_reuso_b203 import ordenes_java, resumen_progreso_java, cpu_qemu_segundos


def entrada(pid="7"):
    return f"[R3] enter PID={pid} name='{NOMBRE}' entry=0x1\n"


def salida(pid="7", code="0"):
    return f"PROC: exit PID={pid} name='{NOMBRE}' code={code} marcos_libres=1\n"


def version():
    return ('openjdk version "21.0.12.1" 2026-08-18\n'
            'OpenJDK Runtime Environment (build 21.0.12.1)\n'
            'OpenJDK 64-Bit Server VM (build 21.0.12.1, mixed mode)\n')


def clase():
    return "".join(f"T010-{i}: ok detalle\n" for i in range(1, 9)) + MARCADOR + "\n"


class Java(unittest.TestCase):
    def test_progreso_cuenta_actividad_sin_inferir_exito(self):
        texto = ("[R3] enter PID=1\r\n[T-005] hilo tid=2\n[SYS] MMAP perezoso x\n"
                 "SCHED[0]: cambio\nSCHED: wake PID 2\n[RAMFS] OPEN -> fd=4\n")
        resumen = resumen_progreso_java(texto)
        for esperado in ("lineas=6", "hilos=1", "mmap=1", "cambios_sched=2", "ultima=[RAMFS] OPEN"):
            self.assertIn(esperado, resumen)
        self.assertNotIn("PASS", resumen)

    def test_progreso_sin_actividad(self):
        self.assertIn("sin actividad registrada", resumen_progreso_java("eco\n"))

    def test_cpu_de_pid_inexistente_no_inventa_cero(self):
        self.assertIsNone(cpu_qemu_segundos(-1))

    def test_ordenes_conservan_ambas_pruebas_y_limites(self):
        normal = ordenes_java()
        primero = ordenes_java(True)
        self.assertEqual([(x[1], x[2]) for x in normal], [("version", 120), ("clase", 180)])
        self.assertEqual(primero, tuple(reversed(normal)))

    def test_version_real(self):
        self.assertTrue(resultado_java(entrada() + version() + salida(), "version")[0])

    def test_clase_completa(self):
        self.assertTrue(resultado_java(entrada() + clase() + salida(), "clase")[0])

    def test_solo_eco_no_cuenta(self):
        self.assertFalse(resultado_java("run java -version\n" + version(), "version")[0])

    def test_salida_de_otro_pid_no_cuenta(self):
        self.assertFalse(resultado_java(entrada() + version() + salida(pid="8"), "version")[0])

    def test_codigo_no_cero_falla(self):
        self.assertFalse(resultado_java(entrada() + clase() + salida(code="1"), "clase")[0])

    def test_falta_un_subtest(self):
        self.assertFalse(resultado_java(entrada() + clase().replace("T010-7: ok detalle\n", "") + salida(), "clase")[0])

    def test_falta_marcador(self):
        self.assertFalse(resultado_java(entrada() + clase().replace(MARCADOR, "otra línea") + salida(), "clase")[0])

    def test_marcador_antes_de_pruebas_falla(self):
        self.assertFalse(resultado_java(entrada() + MARCADOR + "\n" + clase().replace(MARCADOR, "") + salida(), "clase")[0])

    def test_error_explicito_falla(self):
        self.assertFalse(resultado_java(entrada() + clase() + "[T010-FAIL] 1\n" + salida(), "clase")[0])

    def test_linea_fuera_del_proceso_falla(self):
        self.assertFalse(resultado_java(version() + entrada() + salida(), "version")[0])

    def test_duplicado_ambiguo_falla(self):
        self.assertFalse(resultado_java(entrada() + clase() + clase() + salida(), "clase")[0])

    def test_modo_invalido_falla(self):
        with self.assertRaises(ValueError):
            resultado_java("", "otro")

    def test_serie_real_suite_113e(self):
        ruta = Path(__file__).resolve().parents[1] / "docs/evidencias/b203/serie_suite113e.log.gz"
        with gzip.open(ruta, "rt", encoding="utf8", errors="replace") as f:
            serie = f.read().replace("\r", "")
        for pid, modo in (("937", "version"), ("950", "clase")):
            principio = serie.index(f"[R3] enter PID={pid} name='{NOMBRE}'")
            fin = re.search(rf"(?m)^PROC: exit PID={pid} name='{re.escape(NOMBRE)}' code=0[^\n]*$", serie[principio:])
            self.assertIsNotNone(fin)
            ventana = serie[principio:principio + fin.end()]
            self.assertTrue(resultado_java(ventana, modo)[0], modo)


if __name__ == "__main__":
    unittest.main(verbosity=2)
