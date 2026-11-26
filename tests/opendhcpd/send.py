#!/usr/bin/env python3
"""Envia pacotes DHCP de teste (válidos e malformados) para o servidor do namespace de teste e
confere se ele continua respondendo. Uso: send.py <ip-do-servidor>"""
import socket, struct, sys, time

SERVER = sys.argv[1]
MAGIC = bytes([99, 130, 83, 99])


def packet(hlen=6, chaddr=b"\x02\x11\x22\x33\x44\x55", options=b"", xid=1):
    hdr = struct.pack("!BBBBIHH4s4s4s4s16s64s128s", 1, 1, hlen, 0, xid, 0, 0x8000,
                      b"\0" * 4, b"\0" * 4, b"\0" * 4, b"\0" * 4,
                      chaddr.ljust(16, b"\0")[:16], b"A" * 64, b"B" * 128)
    return hdr + MAGIC + options


def opt(code, value):
    return bytes([code, len(value)]) + value


def discover(**kw):
    return opt(53, b"\x01") + kw.get("extra", b"") + b"\xff"


s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
s.bind(("0.0.0.0", 68))
s.settimeout(2)


def send(name, data):
    s.sendto(data, (SERVER, 67))
    try:
        reply, _ = s.recvfrom(4096)
        print(f"{name}: resposta de {len(reply)} bytes")
    except socket.timeout:
        print(f"{name}: sem resposta")


# 1. pedido normal
send("normal", packet(options=discover(extra=opt(12, b"notebook-ana"))))
# 2. nome com quebra de linha e HTML (injeção em log e na página de status)
send("nome-malicioso", packet(chaddr=b"\x02\xaa\xbb\xcc\xdd\x01",
     options=discover(extra=opt(12, b"evil\n[01-Jan-26 00:00:00] Host fake<script>x</script>"))))
# 3. hlen maior que o campo (estourava chaddr/nome gerado/log)
send("hlen-255", packet(hlen=255, chaddr=b"\x02\xaa\xbb\xcc\xdd\x02", options=discover()))
# 4. tamanho máximo de mensagem gigante + opções grandes ecoadas (82 e 118)
send("messsize-65535", packet(chaddr=b"\x02\xaa\xbb\xcc\xdd\x03",
     options=discover(extra=opt(57, b"\xff\xff") + opt(82, b"R" * 255) + opt(118, b"S" * 255))))
# 5. opção cortada: diz ter 200 bytes mas o pacote acaba antes
send("opcao-cortada", packet(chaddr=b"\x02\xaa\xbb\xcc\xdd\x04", options=opt(53, b"\x01") + bytes([12, 200]) + b"x" * 10))
# 6. opções numéricas sem o tamanho certo
send("opcoes-curtas", packet(chaddr=b"\x02\xaa\xbb\xcc\xdd\x05", options=opt(53, b"") + opt(50, b"\x01") + opt(57, b"") + b"\xff"))
# 7. de novo um pedido normal: o servidor continua vivo?
time.sleep(0.5)
send("normal-depois", packet(chaddr=b"\x02\x11\x22\x33\x44\x66", options=discover(extra=opt(12, b"tv-sala"))))
