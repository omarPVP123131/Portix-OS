#!/usr/bin/env python3
"""El ejecutor general no debe confundir eventos QMP o acuses de capacidades."""
import json
import unittest
from unittest.mock import patch

if __package__:
    from . import qmp_seguro as q
    from .headless_runner import PortixHeadlessRunner
    from .test_sonda_marcos_b203 import SocketFalso, paquete, socket_qmp
else:
    import qmp_seguro as q
    from headless_runner import PortixHeadlessRunner
    from test_sonda_marcos_b203 import SocketFalso, paquete, socket_qmp


def runner():
    obj = object.__new__(PortixHeadlessRunner)
    obj.qmp_path = "/tmp/qmp-falso"
    return obj


class QMPGeneral(unittest.TestCase):
    def test_evento_antes_de_capacidades_no_falsifica_acuse_del_comando(self):
        sk = socket_qmp({"error": {"class": "GenericError", "desc": "desconocido"}, "id": "consulta-t033"},
                        previo_cap=[{"event": "STOP"}])
        with patch.object(q.socket, "socket", return_value=sk):
            respuesta = runner().qmp("comando-invalido")
        self.assertTrue(respuesta.startswith(b"ERR "))
        with self.assertRaises((ValueError, UnicodeError)):
            json.loads(respuesta.decode())

    def test_human_monitor_conserva_json_y_contenido(self):
        sk = socket_qmp({"return": "CPU#0\r\nRIP=123\r\n", "id": "consulta-t033"},
                        previo_consulta=[{"event": "RESUME"}])
        with patch.object(q.socket, "socket", return_value=sk):
            r = runner().qmp("human-monitor-command", {"command-line": "info registers -a"})
        self.assertEqual(json.loads(r), {"return": "CPU#0\r\nRIP=123\r\n"})
        self.assertEqual(sk.enviados[-1]["arguments"], {"command-line": "info registers -a"})

    def test_qmp_malformado_no_da_return(self):
        sk = SocketFalso([paquete({"QMP": {}}), paquete({"return": {}, "id": "capacidad-t033"}),
                          paquete({"return": {}, "id": "capacidad-t033"})])
        with patch.object(q.socket, "socket", return_value=sk):
            respuesta = runner().qmp("trace-event-set-state", {"name": "ide_status_read", "enable": True})
        self.assertTrue(respuesta.startswith(b"ERR "))

    def test_return_vacio_exige_id_de_orden_real(self):
        sk = socket_qmp({"return": {}, "id": "consulta-t033"}, previo_cap=[{"event": "STOP"}])
        with patch.object(q.socket, "socket", return_value=sk):
            respuesta = runner().qmp("trace-event-set-state", {"name": "ide_status_read", "enable": True})
        self.assertEqual(json.loads(respuesta), {"return": {}})
        self.assertEqual(sk.enviados[-1]["id"], "consulta-t033")

    def test_conexion_caida_devuelve_error_no_exito(self):
        with patch.object(q.socket, "socket", side_effect=OSError("sin conexión")):
            self.assertTrue(runner().qmp("query-status").startswith(b"ERR "))

    def test_sin_return_explicito_se_rechaza(self):
        sk = socket_qmp({"id": "consulta-t033"})
        with patch.object(q.socket, "socket", return_value=sk):
            self.assertTrue(runner().qmp("query-status").startswith(b"ERR "))


if __name__ == "__main__":
    unittest.main(verbosity=2)
