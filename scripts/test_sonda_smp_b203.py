#!/usr/bin/env python3
"""Controles del ejecutor acotado y de la inspección del disco detenido."""
import hashlib
import io
import threading
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

if __package__:
    from . import sonda_smp_b203 as s
    from .test_verificaciones_t033_smp import serie
else:
    import sonda_smp_b203 as s
    from test_verificaciones_t033_smp import serie


class Proceso:
    def __init__(self, activo=True): self.activo = activo
    def poll(self): return None if self.activo else 0


class SimuladorRunner:
    def __init__(self, modo="munmap", activo=True):
        self.log = ""
        self.log_lock = threading.Lock()
        self.proc = Proceso(activo)
        self.proc.stdin = io.BytesIO()
        self.httpd = object()
        self.modo = modo
        self.ordenes = []
    def get_log_len(self): return len(self.log)
    def send_command(self, orden):
        self.ordenes.append(orden)
        self.log += serie(self.modo)
    def wait_for_pattern(self, *args, **kwargs): return True, "root@portix:"
    def _stdin_write(self, datos): self.proc.stdin.write(datos)
    def execute_and_verify(self, *args, **kwargs): return True


class Pruebas(unittest.TestCase):
    def test_modo_positivo_exacto(self):
        r = SimuladorRunner()
        self.assertEqual(s.probar(r, "munmap", limite=1), (True, True))
        self.assertEqual(r.ordenes, ["run /usr/local/t033smp munmap"])

    def test_control_negativo_es_esperado_y_no_exito_generico(self):
        r = SimuladorRunner("control-munmap")
        self.assertEqual(s.probar(r, "control-munmap", limite=1), (True, True))

    def test_error_del_caso_real_no_se_transforma_en_exito(self):
        r = SimuladorRunner("munmap")
        original = r.send_command
        def fallo(orden):
            original(orden)
            r.log = r.log.replace("code=0", "code=1")
        r.send_command = fallo
        self.assertEqual(s.probar(r, "munmap", limite=1), (False, True))

    def test_no_continuar_sin_retorno_de_consola(self):
        r = SimuladorRunner()
        r.wait_for_pattern = lambda *a, **kw: (False, "")
        self.assertEqual(s.probar(r, "munmap", limite=1), (False, False))

    def test_sin_servidor_no_arranca_pruebas(self):
        r = SimuladorRunner()
        r.httpd = None
        self.assertEqual(s.ejecutar_en_vm(r), ([], False))
        self.assertEqual(r.ordenes, [])

    def test_si_no_descarga_no_ejecuta_elf(self):
        r = SimuladorRunner()
        r.execute_and_verify = lambda *a, **kw: False
        with patch.object(s, "clasificar_arranque", return_value=(True, True, "arrancó")):
            self.assertEqual(s.ejecutar_en_vm(r), ([], False))
        self.assertEqual(r.ordenes, [])

    def test_omitir_restantes_si_consola_ocupada(self):
        r = SimuladorRunner()
        with patch.object(s, "clasificar_arranque", return_value=(True, True, "arrancó")), \
             patch.object(s, "probar", return_value=(False, False)) as probar:
            resultado, descargado = s.ejecutar_en_vm(r)
        self.assertTrue(descargado)
        self.assertEqual(resultado, [(s.MODOS[0], False)])
        self.assertEqual(probar.call_count, 1)

    def test_continuar_tras_fallo_con_consola_libre(self):
        r = SimuladorRunner()
        with patch.object(s, "clasificar_arranque", return_value=(True, True, "arrancó")), \
             patch.object(s, "probar", side_effect=[(False, True), (True, True), (True, True), (True, True)]) as probar:
            resultado, descargado = s.ejecutar_en_vm(r)
        self.assertTrue(descargado)
        self.assertEqual(len(resultado), 4)
        self.assertEqual(probar.call_count, 4)

    def test_no_leer_disco_de_vm_activa(self):
        r = SimpleNamespace(proc=Proceso(True), _img_copia="no-importa")
        with patch.object(s.subprocess, "run") as ejecutar:
            self.assertFalse(s.verificar_binario_en_disco_detenido(r, "0" * 64))
        ejecutar.assert_not_called()

    def verificar_ext2_falso(self, datos, retorno, hash_esperado):
        s.RUN.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="t033-control-", dir=s.RUN) as tmp:
            carpeta = Path(tmp)
            imagen = carpeta / "disco.img"
            imagen.write_bytes(b"X" * 512 + b"part")
            binario = carpeta / "sonda.bin"
            binario.write_bytes(b"sonda-rust-original")
            r = SimpleNamespace(proc=Proceso(False), _img_copia=str(imagen))
            fake = SimpleNamespace(returncode=retorno, stdout=datos)
            with patch.object(s, "BINARIO", binario), patch.object(s, "localizar_ext2", return_value=(512, 4)), \
                 patch.object(s.shutil, "which", return_value="debugfs"), patch.object(s.subprocess, "run", return_value=fake):
                return s.verificar_binario_en_disco_detenido(r, hash_esperado)

    def test_elf_comparado_byte_a_byte_y_hash(self):
        esperado = b"sonda-rust-original"
        self.assertTrue(self.verificar_ext2_falso(esperado, 0, hashlib.sha256(esperado).hexdigest()))

    def test_no_aceptar_elf_distinto(self):
        esperado = b"sonda-rust-original"
        self.assertFalse(self.verificar_ext2_falso(b"sonda-rust-adulterada", 0, hashlib.sha256(esperado).hexdigest()))

    def test_no_aceptar_hash_distinto(self):
        self.assertFalse(self.verificar_ext2_falso(b"sonda-rust-original", 0, "0" * 64))

    def test_no_aceptar_error_debugfs(self):
        esperado = b"sonda-rust-original"
        self.assertFalse(self.verificar_ext2_falso(esperado, 1, hashlib.sha256(esperado).hexdigest()))


if __name__ == "__main__":
    unittest.main(verbosity=2)
