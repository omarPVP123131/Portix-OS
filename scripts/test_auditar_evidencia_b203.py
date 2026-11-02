#!/usr/bin/env python3
"""Controles del analizador de texto; no simulan ni certifican Portix/QEMU."""

import unittest

from auditar_evidencia_b203 import auditar


REGISTRO = """[DEB] libstdc++6_14.2.0-19_amd64.deb: Size OK, SHA256 ab1fa05837aa7a92 OK, control: libstdc++6 14.2.0-19
[DEB-OK] instalados: otro_1 libstdc++6_14.2.0-19 rustc_1
root@portix:/home/user$ cat /proc/smp
mmap_fichero_perezoso: tramos_vivos 0 registrados 971 paginas_desde_fichero 47268 bytes 193413058
root@portix:/home/user$ run pkg deb-add openjdk-21-jre-headless
root@portix:/home/user$ run /usr/lib/jvm/java-21-openjdk-amd64/bin/java -cp /usr/local T010Java
[T010-PASS] terminada
root@portix:/home/user$ cat /proc/smp
mmap_fichero_perezoso: tramos_vivos 0 registrados 1067 paginas_desde_fichero 60464 bytes 247452498
root@portix:/home/user$ run test_posix
"""


class PruebaAuditoria(unittest.TestCase):
    def prueba_falla(self, texto):
        with self.assertRaises(ValueError):
            auditar(texto)

    def test_evidencia_completa(self):
        e = auditar(REGISTRO)
        self.assertEqual((e.paginas_antes, e.paginas_despues, e.delta), (47268, 60464, 13196))
        self.assertEqual(e.prefijo_sha256, "ab1fa05837aa7a92")

    def test_falta_lectura_anterior(self):
        self.prueba_falla(REGISTRO.replace("paginas_desde_fichero 47268", "dato_ausente 47268"))

    def test_falta_lectura_posterior(self):
        self.prueba_falla(REGISTRO.replace("paginas_desde_fichero 60464", "dato_ausente 60464"))

    def test_lectura_vieja_no_sirve(self):
        self.prueba_falla(REGISTRO.replace("root@portix:/home/user$ run pkg deb-add", "root@portix:/home/user$ run otra_orden\nroot@portix:/home/user$ run pkg deb-add"))

    def test_delta_cero_o_negativo(self):
        self.prueba_falla(REGISTRO.replace("paginas_desde_fichero 60464", "paginas_desde_fichero 47268"))
        self.prueba_falla(REGISTRO.replace("paginas_desde_fichero 60464", "paginas_desde_fichero 1"))

    def test_limites(self):
        self.prueba_falla(REGISTRO.replace("paginas_desde_fichero 60464", "paginas_desde_fichero 48267"))
        self.prueba_falla(REGISTRO.replace("paginas_desde_fichero 60464", "paginas_desde_fichero 81657"))
        self.assertEqual(auditar(REGISTRO.replace("paginas_desde_fichero 60464", "paginas_desde_fichero 48268")).delta, 1000)
        self.assertEqual(auditar(REGISTRO.replace("paginas_desde_fichero 60464", "paginas_desde_fichero 81656")).delta, 34388)

    def test_nombre_en_url_no_demuestra_verificacion(self):
        texto = REGISTRO.replace("[DEB] libstdc++6_14.2.0-19_amd64.deb: Size OK, SHA256 ab1fa05837aa7a92 OK, control: libstdc++6 14.2.0-19", "[CURL] https://repo/libstdc++6_14.2.0-19_amd64.deb")
        self.prueba_falla(texto)

    def test_verificar_no_es_instalar(self):
        self.prueba_falla(REGISTRO.replace("[DEB-OK] instalados: otro_1 libstdc++6_14.2.0-19 rustc_1", "[DEB-OK] instalados: otro_1 rustc_1"))

    def test_falta_la_sonda_java(self):
        self.prueba_falla(REGISTRO.replace("[T010-PASS]", "[T010-FAIL]"))

    def test_marca_tardia_no_demuestra_exito_de_java(self):
        self.prueba_falla(REGISTRO.replace("[T010-PASS] terminada", "[T010-FAIL] falló") + "[T010-PASS] tardío\n")

    def test_contadores_ambiguos(self):
        linea = "mmap_fichero_perezoso: tramos_vivos 0 registrados 971 paginas_desde_fichero 47268 bytes 193413058"
        self.prueba_falla(REGISTRO.replace(linea, linea + "\n" + linea))


if __name__ == "__main__":
    unittest.main(verbosity=2)
