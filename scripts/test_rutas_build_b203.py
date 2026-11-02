#!/usr/bin/env python3
"""Pruebas de rutas y limpieza del constructor; sin QEMU ni imágenes reales."""

import contextlib
import io
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

if __package__:
    from . import build, rutas_imagenes
    from .interactive_runner import PortixInteractiveRunner
else:
    import build, rutas_imagenes
    from interactive_runner import PortixInteractiveRunner


class RutasBuild(unittest.TestCase):
    def setUp(self):
        rutas_imagenes.RUN.mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(prefix="test-build-", dir=rutas_imagenes.RUN)
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)

    def test_imagen_base_y_distribucion_son_distintas_y_estan_en_dist(self):
        rutas = (build.DISK_IMG, build.RAW_COPY, build.PERSISTENT_DISK, build.ESP_IMG,
                 build.DATA_PART_IMG, build.BOOT_PART_IMG, build.EXT2_PART_IMG,
                 build.ISO_IMG, build.ISO_UEFI_IMG, build.DUAL_IMG, build.UEFI_IMG,
                 build.VSIM_IMG, build.VDI_IMG, build.VMDK_IMG)
        self.assertNotEqual(build.DISK_IMG, build.RAW_COPY)
        for ruta in rutas:
            with self.subTest(ruta=ruta):
                self.assertTrue(rutas_imagenes.dentro_de_dist(ruta))

    def test_create_raw_copia_base_a_distribucion_sin_samefile(self):
        w = self.base / "trabajo"
        w.mkdir()
        def crear(nombre, data):
            p = w / nombre
            p.write_bytes(data)
            return p
        boot = crear("boot.bin", b"boot")
        stage = crear("stage2.bin", b"stage2")
        kernel = crear("kernel.bin", b"kernel")
        boot_part = crear("boot-part.img", b"particion")
        base = w / "base.img"
        dist = self.base / "distribucion.img"
        valores = dict(DISK_IMG=base, RAW_COPY=dist, BOOTBIN=boot, STAGE2BIN=stage,
                       KERNELBIN=kernel, BOOT_PART_IMG=boot_part,
                       DATA_PART_MB=0, DISK_MIN_MB=1)
        with mock.patch.multiple(build, **valores), mock.patch.object(build, "step"), \
             mock.patch.object(build, "log"), mock.patch.object(build, "_build_data_part"), \
             mock.patch.object(build, "_build_boot_part", return_value=1), \
             mock.patch.object(build, "_build_ext2_part", return_value=0), \
             mock.patch.object(build, "_inject_pt"):
            build.create_raw(1)
        self.assertTrue(base.exists())
        self.assertTrue(dist.exists())
        self.assertFalse(os.path.samefile(base, dist))
        self.assertEqual(base.read_bytes(), dist.read_bytes())

    def test_clean_conserva_disco_grande_en_su_sitio_sin_leerlo_en_ram(self):
        b = self.base / "build"
        d = b / "dist"
        d.mkdir(parents=True)
        disco = d / "persistente.img"
        with disco.open("wb") as f:
            f.write(b"INICIO")
            f.truncate(8 * 1024 ** 3)  # disperso: no reserva 8 GiB físicos
            f.seek(8 * 1024 ** 3 - 3)
            f.write(b"FIN")
        inode = disco.stat().st_ino
        (d / "otro.img").write_bytes(b"borrar")
        (b / "intermedio.bin").write_bytes(b"borrar")
        valores = dict(BUILD=b, DIST=d, PERSISTENT_DISK=disco)
        with mock.patch.multiple(build, **valores), mock.patch.object(build, "step"), \
             mock.patch.object(build, "log"), mock.patch.object(build, "arg_val", return_value="8192"), \
             mock.patch.object(Path, "read_bytes", side_effect=AssertionError("no leer imágenes enteras")):
            build.clean()
        self.assertEqual(disco.stat().st_ino, inode)
        self.assertEqual(disco.stat().st_size, 8 * 1024 ** 3)
        with disco.open("rb") as f:
            self.assertEqual(f.read(6), b"INICIO")
            f.seek(-3, os.SEEK_END)
            self.assertEqual(f.read(), b"FIN")
        self.assertFalse((d / "otro.img").exists())
        self.assertFalse((b / "intermedio.bin").exists())

    def test_clean_rechaza_dist_como_enlace(self):
        b = self.base / "build"
        b.mkdir()
        d = b / "dist"
        d.symlink_to("/tmp", target_is_directory=True)
        with mock.patch.multiple(build, BUILD=b, DIST=d, PERSISTENT_DISK=d / "p.img"), \
             mock.patch.object(build, "step"), mock.patch.object(build, "log"):
            with self.assertRaisesRegex(RuntimeError, "enlaces simbólicos"):
                build.clean()
        self.assertTrue(d.is_symlink())

    def test_iso_temporal_dual_en_dist_y_limpieza(self):
        w = self.base / "trabajo"
        w.mkdir()
        efi = w / "BOOTX64.EFI"
        efi.write_bytes(b"EFI")
        iso = self.base / "dual.iso"
        def esp(path):
            self.assertTrue(rutas_imagenes.dentro_de_dist(path))
            path.write_bytes(b"ESP")
        def ejecutar(cmd):
            tree = Path(cmd[-1])
            self.assertTrue(rutas_imagenes.dentro_de_dist(tree))
            self.assertEqual((tree / "boot" / "boot_cd.img").read_bytes(), b"BOOT")
            self.assertEqual((tree / "efi.img").read_bytes(), b"ESP")
            iso.write_bytes(b"ISO")
            return True
        with mock.patch.multiple(build, IMAGENES_TRABAJO=w, EFIBIN=efi, ISO_IMG=iso), \
             mock.patch.object(build, "_xorriso_path", return_value="xorriso-falso"), \
             mock.patch.object(build, "_make_boot_cd_img", return_value=(b"BOOT", 1)), \
             mock.patch.object(build, "_build_esp_fat32", side_effect=esp), \
             mock.patch.object(build, "run_safe", side_effect=ejecutar), mock.patch.object(build, "log"):
            self.assertTrue(build._try_xorriso_dual())
        self.assertEqual(iso.read_bytes(), b"ISO")
        self.assertFalse((w / "_isotree_dual").exists())
        self.assertFalse((w / "portix-iso-dual-esp.img").exists())

    def test_disco_silicio_no_sobrescribe_fichero_desconocido(self):
        ruta = self.base / "periferico.img"
        ruta.write_bytes(b"DATOS DE OTRO")
        with self.assertRaises(FileExistsError):
            rutas_imagenes.crear_disco_patron(ruta, 1, b"FIRMA")
        self.assertEqual(ruta.read_bytes(), b"DATOS DE OTRO")

    def test_disco_silicio_valido_se_reusa_y_conserva_patron(self):
        ruta = self.base / "periferico.img"
        self.assertTrue(rutas_imagenes.crear_disco_patron(ruta, 1, b"FIRMA"))
        self.assertFalse(rutas_imagenes.crear_disco_patron(ruta, 1, b"FIRMA"))
        self.assertEqual(ruta.stat().st_size, 1024 * 1024)
        with ruta.open("rb") as f:
            self.assertEqual(f.read(5), b"FIRMA")

    def test_interactivo_aisla_y_no_usa_original_si_falla(self):
        origen = self.base / "original.img"
        origen.write_bytes(b"ORIGINAL")
        with contextlib.redirect_stdout(io.StringIO()):
            r = PortixInteractiveRunner(str(origen))
        copia = Path(r.img_path)
        self.assertTrue(rutas_imagenes.dentro_de_dist(copia))
        self.assertNotEqual(copia, origen)
        r.close()
        self.assertFalse(copia.exists())
        self.assertEqual(origen.read_bytes(), b"ORIGINAL")
        with mock.patch("shutil.copyfile", side_effect=OSError("sin espacio")):
            with self.assertRaisesRegex(RuntimeError, "no se pudo copiar"):
                PortixInteractiveRunner(str(origen))
        self.assertEqual(origen.read_bytes(), b"ORIGINAL")


if __name__ == "__main__":
    unittest.main(verbosity=2)
