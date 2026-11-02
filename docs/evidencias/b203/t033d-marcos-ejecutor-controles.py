#!/usr/bin/env python3
"""Controles negativos del monitor QMP y de su atribución a CR3/PID."""
import io
import json
import unittest
from types import SimpleNamespace
from unittest.mock import call, patch

if __package__:
    from . import sonda_marcos_b203 as s
else:
    import sonda_marcos_b203 as s


def paquete(valor): return (json.dumps(valor) + "\r\n").encode()


class SocketFalso:
    def __init__(self, respuestas):
        self.entrada = io.BytesIO(b"".join(respuestas))
        self.enviados = []
        self.ruta = None
    def __enter__(self): return self
    def __exit__(self, *args): return False
    def settimeout(self, valor): self.timeout = valor
    def connect(self, ruta): self.ruta = ruta
    def makefile(self, modo): return self.entrada
    def sendall(self, datos): self.enviados.append(json.loads(datos))


def socket_qmp(respuesta, previo_cap=(), previo_consulta=()):
    return SocketFalso([paquete({"QMP": {}}), *[paquete(x) for x in previo_cap],
                        paquete({"return": {}, "id": "capacidad-t033"}),
                        *[paquete(x) for x in previo_consulta], paquete(respuesta)])


class Pruebas(unittest.TestCase):
    def test_qmp_acepta_eventos_antes_de_ambos_acuses(self):
        sk = socket_qmp({"return": {"running": False}, "id": "consulta-t033"},
                        previo_cap=[{"event": "STOP"}], previo_consulta=[{"event": "RESUME"}])
        with patch.object(s.socket, "socket", return_value=sk):
            resultado = s.respuesta_qmp(SimpleNamespace(qmp_path="/tmp/qmp-falso"), "query-status")
        self.assertEqual(resultado, {"running": False})
        self.assertEqual(sk.ruta, "/tmp/qmp-falso")
        self.assertEqual([x["id"] for x in sk.enviados], ["capacidad-t033", "consulta-t033"])
        self.assertEqual(sk.enviados[1]["execute"], "query-status")

    def test_qmp_no_acepta_error(self):
        sk = socket_qmp({"error": {"desc": "falló"}, "id": "consulta-t033"})
        with patch.object(s.socket, "socket", return_value=sk), self.assertRaises(ValueError):
            s.respuesta_qmp(SimpleNamespace(qmp_path="falso"), "query-status")

    def test_qmp_no_acepta_json_malformado(self):
        sk = SocketFalso([paquete({"QMP": {}}), b"falso\n"])
        with patch.object(s.socket, "socket", return_value=sk), self.assertRaises(ValueError):
            s.respuesta_qmp(SimpleNamespace(qmp_path="falso"), "query-status")

    def test_qmp_no_confunde_acuse_de_otro_id(self):
        sk = socket_qmp({"return": {}, "id": "otra-consulta"})
        with patch.object(s.socket, "socket", return_value=sk), self.assertRaises(ValueError):
            s.respuesta_qmp(SimpleNamespace(qmp_path="falso"), "query-status")

    def test_qmp_no_acepta_saludo_falso(self):
        sk = SocketFalso([paquete({"return": {}})])
        with patch.object(s.socket, "socket", return_value=sk), self.assertRaises(ValueError):
            s.respuesta_qmp(SimpleNamespace(qmp_path="falso"), "query-status")

    def test_qmp_no_acepta_acuse_de_capacidades_como_estado(self):
        sk = SocketFalso([paquete({"QMP": {}}), paquete({"return": {}, "id": "capacidad-t033"}),
                          paquete({"return": {}, "id": "capacidad-t033"})])
        with patch.object(s.socket, "socket", return_value=sk), self.assertRaises(ValueError):
            s.respuesta_qmp(SimpleNamespace(qmp_path="falso"), "query-status")

    def test_lectura_fisica_exige_direccion_y_ancho(self):
        with patch.object(s, "respuesta_qmp", return_value="0000000000001000: 0x0000000000005007\r\n") as consulta:
            self.assertEqual(s.leer_fisica(None, 0x1000, "gx"), 0x5007)
        consulta.assert_called_once_with(None, "human-monitor-command", {"command-line": "xp /1gx 0x1000"})

    def test_lectura_byte(self):
        with patch.object(s, "respuesta_qmp", return_value="0000000000005000: 0xb7\r\n"):
            self.assertEqual(s.leer_fisica(None, 0x5000, "bx"), 183)

    def test_no_aceptar_otra_direccion(self):
        with patch.object(s, "respuesta_qmp", return_value="0000000000001008: 0x0000\r\n"), self.assertRaises(ValueError):
            s.leer_fisica(None, 0x1000, "gx")

    def test_no_aceptar_byte_de_16_bits(self):
        with patch.object(s, "respuesta_qmp", return_value="0000000000001000: 0x01b7\r\n"), self.assertRaises(ValueError):
            s.leer_fisica(None, 0x1000, "bx")

    def test_no_aceptar_dos_palabras(self):
        with patch.object(s, "respuesta_qmp", return_value="0000000000001000: 0x1 0x2\r\n"), self.assertRaises(ValueError):
            s.leer_fisica(None, 0x1000, "gx")

    def test_orden_stop_exige_estado_pausado(self):
        with patch.object(s, "respuesta_qmp", side_effect=[{}, {"running": False, "status": "paused"}]) as consulta:
            s.cambiar_pausa(None, True)
        self.assertEqual(consulta.call_args_list, [call(None, "stop"), call(None, "query-status")])

    def test_orden_stop_no_acepta_qemu_activo(self):
        with patch.object(s, "respuesta_qmp", side_effect=[{}, {"running": True}]), self.assertRaises(ValueError):
            s.cambiar_pausa(None, True)

    def test_orden_cont_exige_estado_activo(self):
        with patch.object(s, "respuesta_qmp", side_effect=[{}, {"running": True}]) as consulta:
            s.cambiar_pausa(None, False)
        self.assertEqual(consulta.call_args_list, [call(None, "cont"), call(None, "query-status")])

    def test_cr3_y_pid_del_lider_inequivocos(self):
        t = ("PROC: create PID=5 name='/usr/local/t033smp' entry=0x123 cr3=0x4D56000\n"
             "[R3] enter PID=5 name='/usr/local/t033smp' entry=0x123\n")
        self.assertEqual(s.cr3_de_lider(t), 0x4d56000)
        with self.assertRaises(ValueError): s.cr3_de_lider(t.replace("enter PID=5", "enter PID=6"))
        with self.assertRaises(ValueError): s.cr3_de_lider(t + t.splitlines()[0] + "\n")

    def test_caminar_tabla_usa_lecturas_fisicas_y_cache(self):
        va = 0x12345000
        indices = [(va >> n) & 511 for n in (39, 30, 21, 12)]
        mem = {0x1000 + indices[0]*8: 0x2007, 0x2000 + indices[1]*8: 0x3007,
               0x3000 + indices[2]*8: 0x4007, 0x4000 + indices[3]*8: 0x5007,
               0x4000 + (indices[3]+1)*8: 0x6007, 0x4000 + (indices[3]+2)*8: 0x7007}
        direcciones = {"a": va, "b": va + 4096, "c": va + 8192}
        def leer(_, d, ancho): return mem[d] if ancho == "gx" else {0x5000: 49, 0x6000: 183, 0x7000: 195}[d]
        with patch.object(s, "leer_fisica", side_effect=leer) as funcion:
            c = s.capturar_fase(None, 0x1000, direcciones)
        self.assertEqual((c["a"]["fisica"], c["b"]["fisica"], c["c"]["fisica"]), (0x5000, 0x6000, 0x7000))
        self.assertEqual(funcion.call_count, 9)


if __name__ == "__main__":
    unittest.main(verbosity=2)
