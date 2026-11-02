#!/usr/bin/env python3
"""Servidor DNS mínimo y REAL (RFC 1035) para el banco de pruebas de Portix.

Escucha en [::1]:53 (y 127.0.0.1:53 si se pide) y responde:
  repo.portix.local   A    10.0.2.2      AAAA  fec0::2
  localhost           A    127.0.0.1     AAAA  ::1
Cualquier otro nombre -> NXDOMAIN. slirp (QEMU) reenvía las consultas que el
invitado hace a fec0::3 al primer "nameserver" IPv6 de /etc/resolv.conf, así
que con "nameserver ::1" las consultas AAAA de Portix llegan aquí.
"""
import socket, struct, sys

ZONA = {
    "repo.portix.local": {1: bytes([10, 0, 2, 2]), 28: socket.inet_pton(socket.AF_INET6, "fec0::2")},
    "localhost": {1: bytes([127, 0, 0, 1]), 28: socket.inet_pton(socket.AF_INET6, "::1")},
}

def nombre(q, off):
    labels = []
    while True:
        l = q[off]; off += 1
        if l == 0: break
        labels.append(q[off:off + l].decode("ascii", "replace")); off += l
    return ".".join(labels).lower(), off

def responder(q):
    if len(q) < 12: return None
    ident, flags, qd = struct.unpack("!HHH", q[:6])
    if qd < 1: return None
    name, off = nombre(q, 12)
    qtype, qclass = struct.unpack("!HH", q[off:off + 4]); off += 4
    pregunta = q[12:off]
    rr = ZONA.get(name, {}).get(qtype)
    if name in ZONA and rr is not None:
        resp = struct.pack("!HHHHHH", ident, 0x8180, 1, 1, 0, 0) + pregunta
        resp += b"\xc0\x0c" + struct.pack("!HHIH", qtype, 1, 60, len(rr)) + rr
    elif name in ZONA:
        resp = struct.pack("!HHHHHH", ident, 0x8180, 1, 0, 0, 0) + pregunta  # NOERROR sin datos
    else:
        resp = struct.pack("!HHHHHH", ident, 0x8183, 1, 0, 0, 0) + pregunta  # NXDOMAIN
    return resp

def main():
    bind6 = ("::1", 53)
    s6 = socket.socket(socket.AF_INET6, socket.SOCK_DGRAM)
    s6.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s6.bind(bind6)
    print("[DNS6] escuchando en [::1]:53", flush=True)
    while True:
        q, peer = s6.recvfrom(512)
        r = responder(q)
        if r: s6.sendto(r, peer)

if __name__ == "__main__":
    main()
