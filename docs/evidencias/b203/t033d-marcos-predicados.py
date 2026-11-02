#!/usr/bin/env python3
"""Verificaciones puras para la inspección QMP de tablas y marcos de T-033."""
import re

FASES = ("antes", "liberada", "remapeada", "segunda-liberada", "segunda-remapeada")
DIRECCIONES = ("a", "b", "c")
MASCARA_FISICA = 0x000F_FFFF_FFFF_F000
PRESENTE = 1
USUARIO = 4
GRANDE = 0x80
PEREZOSA = 1 << 10


def marca_fase(texto, fase):
    """Extrae una marca única y tres direcciones virtuales distintas y alineadas."""
    if fase not in FASES:
        raise ValueError("fase inválida")
    patron = (rf"(?m)^\[T033-MARCOS\] fase={re.escape(fase)} "
              r"a=0x([0-9a-f]+) b=0x([0-9a-f]+) c=0x([0-9a-f]+)$")
    halladas = list(re.finditer(patron, texto.replace("\r", "")))
    if len(halladas) != 1:
        return None
    direcciones = tuple(int(x, 16) for x in halladas[0].groups())
    if len(set(direcciones)) != 3 or any(x < 0x1000 or x >= 0x0000800000000000 or x & 0xfff for x in direcciones):
        return None
    return dict(zip(DIRECCIONES, direcciones))


def caminar_pte(cr3, virtual, leer64):
    """Recorre tablas x86-64 reales mediante lector físico; rechaza páginas grandes."""
    if cr3 <= 0 or cr3 & 0xfff or cr3 & ~MASCARA_FISICA:
        raise ValueError("CR3 inválido")
    if virtual < 0x1000 or virtual >= 0x0000800000000000 or virtual & 0xfff:
        raise ValueError("virtual inválida")
    tabla = cr3
    for desplazamiento in (39, 30, 21):
        entrada = leer64(tabla + ((virtual >> desplazamiento) & 511) * 8)
        if not isinstance(entrada, int) or entrada < 0 or entrada >= (1 << 64):
            raise ValueError("lectura de entrada inválida")
        if entrada & (PRESENTE | USUARIO) != (PRESENTE | USUARIO):
            raise ValueError("tabla intermedia ausente o no accesible a usuario")
        if desplazamiento in (30, 21) and entrada & GRANDE:
            raise ValueError("tabla intermedia es página grande")
        tabla = entrada & MASCARA_FISICA
        if not tabla:
            raise ValueError("tabla intermedia sin dirección física")
    pte = leer64(tabla + ((virtual >> 12) & 511) * 8)
    if not isinstance(pte, int) or pte < 0 or pte >= (1 << 64):
        raise ValueError("PTE inválida")
    if pte & PRESENTE:
        if not pte & USUARIO:
            raise ValueError("página presente no accesible a usuario")
        fisica = pte & MASCARA_FISICA
        if not fisica:
            raise ValueError("página presente en marco físico cero")
    else:
        fisica = None
    return {"pte": pte, "fisica": fisica}


def evaluar_capturas(capturas):
    """Valida estados y bytes; devuelve sólo reutilizaciones físicas observadas.

    Sin igualdad de marco no hay evidencia de reutilización; no se declara que
    el asignador sea incorrecto por elegir otro marco.
    """
    if set(capturas) != set(FASES):
        return False, "faltan o sobran fases", None
    estados = {
        "antes": (("P", 49), ("D", None), ("D", None)),
        "liberada": (("Z", None), ("P", 183), ("D", None)),
        "remapeada": (("P", 90), ("P", 183), ("D", None)),
        "segunda-liberada": (("Z", None), ("P", 183), ("P", 195)),
        "segunda-remapeada": (("P", 109), ("P", 183), ("P", 195)),
    }
    for fase in FASES:
        cap = capturas[fase]
        if set(cap) != set(DIRECCIONES):
            return False, f"{fase}: faltan direcciones", None
        for nombre, (clase, byte) in zip(DIRECCIONES, estados[fase]):
            dato = cap[nombre]
            if not isinstance(dato, dict) or set(dato) != {"pte", "fisica", "byte"}:
                return False, f"{fase}/{nombre}: registro inválido", None
            pte, fisica, observado = dato["pte"], dato["fisica"], dato["byte"]
            if not isinstance(pte, int) or pte < 0 or pte >= 1 << 64:
                return False, f"{fase}/{nombre}: PTE inválida", None
            if clase == "P":
                if (pte & 7) != 7 or not isinstance(fisica, int) or fisica <= 0 or fisica & 0xfff or fisica != (pte & MASCARA_FISICA) or observado != byte:
                    return False, f"{fase}/{nombre}: página presente o byte inesperados", None
            elif clase == "D":
                if pte & PRESENTE or not pte & PEREZOSA or fisica is not None or observado is not None:
                    return False, f"{fase}/{nombre}: reserva perezosa inesperada", None
            elif pte != 0 or fisica is not None or observado is not None:
                return False, f"{fase}/{nombre}: PTE no retirada", None
        marcos_presentes = [cap[n]["fisica"] for n in DIRECCIONES if cap[n]["fisica"] is not None]
        if len(marcos_presentes) != len(set(marcos_presentes)):
            return False, f"{fase}: alias físico entre páginas presentes", None
    b_marcos = {capturas[fase]["b"]["fisica"] for fase in FASES[1:]}
    c_marcos = {capturas[fase]["c"]["fisica"] for fase in FASES[3:]}
    if len(b_marcos) != 1 or len(c_marcos) != 1:
        return False, "cambió inesperadamente un marco testigo vivo", None
    reutilizaciones = {
        "primera": capturas["antes"]["a"]["fisica"] == capturas["liberada"]["b"]["fisica"],
        "segunda": capturas["remapeada"]["a"]["fisica"] == capturas["segunda-liberada"]["c"]["fisica"],
    }
    if not all(reutilizaciones.values()):
        return False, "reutilización física no observada en todas las parejas; no es fallo del asignador", reutilizaciones
    return True, "dos reutilizaciones físicas observadas; páginas presentes distintas", reutilizaciones
