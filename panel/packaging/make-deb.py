#!/usr/bin/env python3
"""Monta um .deb a partir de uma árvore já instalada (sem dpkg-deb).

Uso: make-deb.py <raiz> <saída.deb> <versão> <postinst> <prerm>

Um .deb é um arquivo `ar` com debian-binary, control.tar.gz e data.tar.gz; aqui os arquivos
ficam com dono root e as permissões da árvore.
"""
import gzip
import hashlib
import io
import os
import sys
import tarfile
import time

root, out, version, postinst, prerm = sys.argv[1:6]

CONTROL = f"""Package: open-dhcp
Version: {version}
Architecture: amd64
Maintainer: Open DHCP Server fork <noreply@github.com>
Section: net
Priority: optional
Depends: libc6, libgtk-3-0, libwebkit2gtk-4.1-0
Homepage: https://github.com/ricardopadua/Open-DHCP-Server
Description: Open DHCP Server with a web panel and a desktop app
 opendhcpd (Open DHCP Server, 64-bit build), the opendhcp-panel service
 (real-time monitoring, reservations, ranges, options, backups) and the
 Open DHCP desktop app that shows the panel in its own window.
"""

CONFFILES = ["/etc/opendhcp-panel/panel.toml", "/opt/opendhcp/opendhcp.ini"]


def tar_gz(members):
    buf = io.BytesIO()
    with gzip.GzipFile(fileobj=buf, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.GNU_FORMAT) as tar:
            for name, data, mode in members:
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = len(data), mode, int(time.time())
                info.uid = info.gid = 0
                info.uname = info.gname = "root"
                tar.addfile(info, io.BytesIO(data))
    return buf.getvalue()


def data_tar():
    buf = io.BytesIO()
    md5 = []
    with gzip.GzipFile(fileobj=buf, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.GNU_FORMAT) as tar:
            for dirpath, dirnames, filenames in os.walk(root):
                dirnames.sort()
                for name in sorted(dirnames) + sorted(filenames):
                    full = os.path.join(dirpath, name)
                    arc = "./" + os.path.relpath(full, root)
                    info = tar.gettarinfo(full, arcname=arc)
                    info.uid = info.gid = 0
                    info.uname = info.gname = "root"
                    if info.isfile():
                        with open(full, "rb") as f:
                            data = f.read()
                        md5.append(f"{hashlib.md5(data).hexdigest()}  {arc[2:]}\n")
                        tar.addfile(info, io.BytesIO(data))
                    else:
                        tar.addfile(info)
    return buf.getvalue(), "".join(md5)


def script(path, guard):
    body = open(path).read()
    return f"#!/bin/sh\nset -e\n{guard}\n{body}".encode()


data, md5sums = data_tar()
control = tar_gz([
    ("./control", CONTROL.encode(), 0o644),
    ("./conffiles", ("\n".join(CONFFILES) + "\n").encode(), 0o644),
    ("./md5sums", md5sums.encode(), 0o644),
    ("./postinst", script(postinst, '[ "$1" = configure ] || exit 0'), 0o755),
    ("./prerm", script(prerm, '[ "$1" = remove ] || exit 0'), 0o755),
])


def ar_member(name, data):
    head = f"{name:<16}{int(time.time()):<12}{0:<6}{0:<6}{0o100644:<8o}{len(data):<10}`\n".encode()
    return head + data + (b"\n" if len(data) % 2 else b"")


with open(out, "wb") as f:
    f.write(b"!<arch>\n")
    f.write(ar_member("debian-binary", b"2.0\n"))
    f.write(ar_member("control.tar.gz", control))
    f.write(ar_member("data.tar.gz", data))
print(out)
