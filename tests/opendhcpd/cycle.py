#!/usr/bin/env python3
"""DISCOVER -> OFFER -> REQUEST -> ACK para um MAC. Uso: cycle.py <servidor> <nome> [mac-final]"""
import socket, struct, sys

SERVER, NAME = sys.argv[1], sys.argv[2].encode()
last = int(sys.argv[3], 16) if len(sys.argv) > 3 else 0x77
MAC = bytes([0x02, 0x10, 0x20, 0x30, 0x40, last])
MAGIC = bytes([99, 130, 83, 99])


def packet(options, ciaddr=b"\0" * 4, xid=7):
    hdr = struct.pack("!BBBBIHH4s4s4s4s16s64s128s", 1, 1, 6, 0, xid, 0, 0x8000, ciaddr, b"\0" * 4,
                      b"\0" * 4, b"\0" * 4, MAC.ljust(16, b"\0"), b"\0" * 64, b"\0" * 128)
    return hdr + MAGIC + options + b"\xff"


def opt(c, v):
    return bytes([c, len(v)]) + v


s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
s.bind(("0.0.0.0", 68))
s.settimeout(3)
s.sendto(packet(opt(53, b"\x01") + opt(12, NAME)), (SERVER, 67))
offer, _ = s.recvfrom(4096)
yiaddr = offer[16:20]
s.sendto(packet(opt(53, b"\x03") + opt(50, yiaddr) + opt(54, socket.inet_aton(SERVER)) + opt(12, NAME)), (SERVER, 67))
ack, _ = s.recvfrom(4096)
# tipo da mensagem na resposta (opção 53)
o = ack[240:]
mtype = None
i = 0
while i < len(o) and o[i] != 255:
    if o[i] == 0:
        i += 1
        continue
    if o[i] == 53:
        mtype = o[i + 2]
    i += 2 + o[i + 1]
print(f"{NAME[:20]!r}: IP {socket.inet_ntoa(yiaddr)}, resposta tipo {mtype} ({'ACK' if mtype == 5 else 'outro'})")
