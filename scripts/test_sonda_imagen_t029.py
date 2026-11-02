#!/usr/bin/env python3
"""Controles negativos del verificador de imagen T-029."""
import struct
import tempfile
import unittest
from pathlib import Path

if __package__:
    from .sonda_imagen_t029 import estado_inode, huella_en_flujo, localizar_ext2
    from .rutas_imagenes import RUN
else:
    from sonda_imagen_t029 import estado_inode, huella_en_flujo, localizar_ext2
    from rutas_imagenes import RUN


def mbr(tipo=0x83, inicio=1, longitud=4):
    b = bytearray(512)
    b[510:512] = b"\x55\xaa"
    b[0x1BE + 32 + 4] = tipo
    struct.pack_into("<II", b, 0x1BE + 32 + 8, inicio, longitud)
    return bytes(b)


class SondaImagen(unittest.TestCase):
    def test_particion_valida(self):
        self.assertEqual(localizar_ext2(mbr(), 4096), (512, 2048))

    def test_mbr_sin_firma_falla(self):
        with self.assertRaises(ValueError): localizar_ext2(bytes(512), 4096)

    def test_tipo_incorrecto_falla(self):
        with self.assertRaises(ValueError): localizar_ext2(mbr(tipo=0x0c), 4096)

    def test_fuera_del_disco_falla(self):
        with self.assertRaises(ValueError): localizar_ext2(mbr(inicio=6, longitud=4), 4096)

    def test_inodo_regular(self):
        self.assertEqual(estado_inode(0, "Inode: 51   Type: regular    Mode: 0755\n", "/bin/hello"), "regular")

    def test_ausencia_especifica(self):
        self.assertEqual(estado_inode(0, "/bin/doom: File not found by ext2_lookup\n", "/bin/doom"), "ausente")

    def test_error_generico_no_se_confunde_con_ausencia(self):
        self.assertEqual(estado_inode(0, "Bad magic number while opening filesystem", "/bin/doom"), "error")
        self.assertEqual(estado_inode(1, "/bin/doom: File not found by ext2_lookup", "/bin/doom"), "error")
        self.assertEqual(estado_inode(0, "/bin/otro: File not found by ext2_lookup", "/bin/doom"), "error")

    def test_imagen_truncada_falla(self):
        RUN.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=RUN) as d:
            f = Path(d) / "imagen.img"
            f.write_bytes(b"abc")
            with self.assertRaises(ValueError): huella_en_flujo(f, 0, 4)


if __name__ == "__main__": unittest.main(verbosity=2)
