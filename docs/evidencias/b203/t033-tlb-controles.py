#!/usr/bin/env python3
"""Controles negativos del clasificador de reservas y contadores T-033."""
import unittest

if __package__:
    from .verificaciones_t033_tlb import NOMBRE, resultado_tlb
else:
    from verificaciones_t033_tlb import NOMBRE, resultado_tlb


def serie(modo="hilos", pid=7):
    ap = "1" if modo == "hilos" else "sin-hilo"
    return (f"[R3] enter PID={pid} name='{NOMBRE}' entry=0x1\n"
            f"[T033-TLB] listo modo={modo} cpu_lider=0 cpu_ap={ap} paginas=4096\n"
            f"[T033-TLB] modo={modo} comienza=mmap paginas=4096\n"
            f"[T033-TLB] modo={modo} fase=mmap paginas=4096 inicio=100 fin=200 delta=100\n"
            f"[T033-TLB] modo={modo} comienza=munmap paginas=4096\n"
            f"[T033-TLB] modo={modo} fase=munmap paginas=4096 inicio=200 fin=350 delta=150\n"
            f"[T033-TLB] modo={modo} proteccion=3-antes-3-despues errno=12 reutilizacion=OK\n"
            f"[T033-TLB] PASS modo={modo}\n"
            f"PROC: exit PID={pid} name='{NOMBRE}' code=0 marcos_libres=100\n")


class T033TLB(unittest.TestCase):
    def test_serie_completa_con_ambos_modos(self):
        for modo in ("solo", "hilos"):
            ok, termino, _, cifras = resultado_tlb(serie(modo), modo)
            self.assertTrue(ok and termino)
            self.assertEqual(cifras, {"mmap": 100, "munmap": 150})

    def test_no_cuenta_sin_entrada(self):
        self.assertFalse(resultado_tlb(serie().replace("[R3] enter", "ECO [R3] enter"), "hilos")[0])

    def test_no_cuenta_salida_de_otro_pid(self):
        self.assertEqual(resultado_tlb(serie().replace("exit PID=7", "exit PID=8"), "hilos")[:2], (False, False))

    def test_codigo_incorrecto_falla(self):
        self.assertEqual(resultado_tlb(serie().replace("code=0", "code=137"), "hilos")[:2], (False, True))

    def test_requiere_cpu_uno(self):
        self.assertFalse(resultado_tlb(serie().replace("cpu_ap=1", "cpu_ap=0"), "hilos")[0])

    def test_falta_cuenta_falla(self):
        self.assertFalse(resultado_tlb(serie().replace("fase=mmap", "fase=desconocida"), "hilos")[0])

    def test_cuenta_inconsistente_falla(self):
        self.assertFalse(resultado_tlb(serie().replace("fin=200 delta=100", "fin=200 delta=99"), "hilos")[0])

    def test_inicio_no_conecta_con_fin_anterior(self):
        self.assertFalse(resultado_tlb(serie().replace("inicio=200 fin=350 delta=150", "inicio=201 fin=351 delta=150"), "hilos")[0])

    def test_etapa_fuera_de_orden_falla(self):
        texto = serie().replace("comienza=mmap", "comienza=OTRA", 1).replace("comienza=munmap", "comienza=mmap", 1).replace("comienza=OTRA", "comienza=munmap", 1)
        self.assertFalse(resultado_tlb(texto, "hilos")[0])

    def test_error_explicito_falla(self):
        self.assertFalse(resultado_tlb(serie().replace("[T033-TLB] PASS", "[T033-ERROR] 1\n[T033-TLB] PASS"), "hilos")[0])

    def test_semantica_incompleta_falla(self):
        self.assertFalse(resultado_tlb(serie().replace("errno=12", "errno=22"), "hilos")[0])

    def test_registro_duplicado_falla(self):
        linea = "[T033-TLB] modo=hilos fase=mmap paginas=4096 inicio=100 fin=200 delta=100\n"
        self.assertFalse(resultado_tlb(serie().replace(linea, linea * 2), "hilos")[0])

    def test_modo_invalido(self):
        with self.assertRaises(ValueError):
            resultado_tlb(serie(), "otro")


if __name__ == "__main__":
    unittest.main(verbosity=2)
