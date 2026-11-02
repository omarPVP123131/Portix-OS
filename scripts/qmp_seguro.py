#!/usr/bin/env python3
"""Peticiones QMP correlacionadas por ID, tolerando eventos asíncronos."""
import json
import socket


def consultar_qmp(ruta, comando, argumentos=None, plazo=3):
    """Devuelve sólo el `return` de la petición correspondiente.

    No confunde eventos STOP/RESUME ni el acuse de qmp_capabilities con la
    respuesta posterior. Levanta una excepción si el ID o formato no coinciden.
    """
    def recibir(entrada):
        linea = entrada.readline(1024 * 1024)
        if not linea or not linea.endswith(b"\n"):
            raise ValueError("QMP cerró o envió un objeto incompleto")
        try:
            obj = json.loads(linea.decode("utf-8"))
        except (UnicodeError, json.JSONDecodeError, TypeError) as e:
            raise ValueError(f"QMP inválido: {e}") from e
        if not isinstance(obj, dict):
            raise ValueError("objeto QMP inválido")
        return obj

    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sk:
        sk.settimeout(plazo)
        sk.connect(ruta)
        with sk.makefile("rb") as entrada:
            saludo = recibir(entrada)
            if "QMP" not in saludo:
                raise ValueError("saludo QMP ausente")
            def solicitar(nombre, args, identidad):
                peticion = {"execute": nombre, "arguments": args or {}, "id": identidad}
                sk.sendall(json.dumps(peticion).encode("utf-8") + b"\n")
                for _ in range(256):
                    obj = recibir(entrada)
                    if "event" in obj:
                        continue
                    if obj.get("id") != identidad:
                        raise ValueError("QMP devolvió un ID inesperado")
                    if "error" in obj:
                        raise ValueError(f"QMP error: {obj['error']}")
                    if "return" not in obj:
                        raise ValueError("QMP sin return")
                    return obj["return"]
                raise ValueError("demasiados eventos QMP sin respuesta")
            solicitar("qmp_capabilities", {}, "capacidad-t033")
            return solicitar(comando, argumentos, "consulta-t033")
