#!/usr/bin/env bash
# Gera os pacotes do Open DHCP em panel/dist/release-<versão>/:
#   open-dhcp-<v>-linux-amd64.deb, open-dhcp-<v>-linux-x86_64.rpm, open-dhcp-<v>-linux-x86_64.AppImage,
#   open-dhcp-<v>-linux-x86_64.tar.gz, open-dhcp-<v>-windows-x86_64.zip e SHA256SUMS.txt.
#
# Requisitos (Linux): Rust, g++, rpmbuild, python3, cargo-bundle (AppImage) e o pacote de
# desenvolvimento do WebKitGTK 4.1 (webkit2gtk4.1-devel / libwebkit2gtk-4.1-dev).
# Windows (compilado daqui): alvo x86_64-pc-windows-gnu, cargo-zigbuild e zig (pip install ziglang).
# A versão vem do Cargo.toml do painel.
set -euo pipefail
cd "$(dirname "$0")/.."
PANEL="$PWD"
V="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
OUT="$PANEL/dist/release-$V"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
rm -rf "$OUT"; mkdir -p "$OUT"
echo "== Open DHCP $V"

# ---------- Linux ----------
cargo build --release -p opendhcp-panel -p open-dhcp
g++ -O2 -include time.h -include pthread.h -o target/release/opendhcpd ../opendhcpd.cpp -lpthread

# panel.toml do pacote: senha inicial "admin" (o painel avisa até ser trocada)
HASH="$(printf 'admin\n' | target/release/opendhcp-panel hash-password 2>/dev/null)"
sed -e "s|^password_hash = .*|password_hash = \"$HASH\"|" panel.example.toml > "$WORK/panel.toml"

ROOT="$WORK/root"
install -Dm755 target/release/opendhcp-panel "$ROOT/usr/bin/opendhcp-panel"
install -Dm755 target/release/open-dhcp "$ROOT/usr/bin/open-dhcp"
install -Dm755 target/release/opendhcpd "$ROOT/opt/opendhcp/opendhcpd"
install -Dm644 ../opendhcp.ini "$ROOT/opt/opendhcp/opendhcp.ini"
install -dm755 "$ROOT/opt/opendhcp/log"
install -Dm600 "$WORK/panel.toml" "$ROOT/etc/opendhcp-panel/panel.toml"
install -Dm644 opendhcp.service "$ROOT/usr/lib/systemd/system/opendhcp.service"
install -Dm644 opendhcp-panel.service "$ROOT/usr/lib/systemd/system/opendhcp-panel.service"
install -Dm644 packaging/open-dhcp.desktop "$ROOT/usr/share/applications/open-dhcp.desktop"
install -Dm644 desktop/assets/icon.png "$ROOT/usr/share/icons/hicolor/256x256/apps/open-dhcp.png"
install -Dm644 README.md "$ROOT/usr/share/doc/open-dhcp/README.md"
install -Dm644 ../LICENSE "$ROOT/usr/share/doc/open-dhcp/LICENSE"

# .deb
python3 packaging/make-deb.py "$ROOT" "$OUT/open-dhcp-$V-linux-amd64.deb" "$V" packaging/postinst.sh packaging/prerm.sh

# .rpm
RPMTOP="$WORK/rpm"; mkdir -p "$RPMTOP"/{BUILD,RPMS,SPECS,SOURCES,SRPMS}
{
  cat <<SPEC
%global debug_package %{nil}
%global __brp_check_rpaths %{nil}
Name: open-dhcp
Version: $V
Release: 1
Summary: Open DHCP Server with a web panel and a desktop app
License: GPL-2.0-only
URL: https://github.com/ricardopadua/Open-DHCP-Server
Requires: webkit2gtk4.1, gtk3

%description
opendhcpd (Open DHCP Server, 64-bit build), the opendhcp-panel service (real-time
monitoring, reservations, ranges, options, backups) and the Open DHCP desktop app
that shows the panel in its own window.

%install
cp -a "$ROOT"/. %{buildroot}/

%post
$(cat packaging/postinst.sh)

%preun
if [ \$1 -eq 0 ]; then
$(cat packaging/prerm.sh)
fi

%files
/usr/bin/opendhcp-panel
/usr/bin/open-dhcp
%dir /opt/opendhcp
/opt/opendhcp/opendhcpd
%config(noreplace) /opt/opendhcp/opendhcp.ini
%dir /opt/opendhcp/log
%dir /etc/opendhcp-panel
%config(noreplace) %attr(600,root,root) /etc/opendhcp-panel/panel.toml
/usr/lib/systemd/system/opendhcp.service
/usr/lib/systemd/system/opendhcp-panel.service
/usr/share/applications/open-dhcp.desktop
/usr/share/icons/hicolor/256x256/apps/open-dhcp.png
%doc /usr/share/doc/open-dhcp/README.md
%license /usr/share/doc/open-dhcp/LICENSE
SPEC
} > "$RPMTOP/SPECS/open-dhcp.spec"
rpmbuild -bb --quiet --define "_topdir $RPMTOP" "$RPMTOP/SPECS/open-dhcp.spec"
cp "$RPMTOP"/RPMS/x86_64/open-dhcp-"$V"-1.*.x86_64.rpm "$OUT/open-dhcp-$V-linux-x86_64.rpm"

# .AppImage (só o aplicativo; usa o painel do serviço instalado ou roda o painel embutido)
(cd desktop && rm -rf ../target/release/bundle/appimage && cargo bundle --release --format appimage)
cp target/release/bundle/appimage/*.AppImage "$OUT/open-dhcp-$V-linux-x86_64.AppImage"

# .tar.gz (mesmos arquivos dos pacotes + install.sh)
TD="open-dhcp-$V-linux-x86_64"
mkdir -p "$WORK/$TD"
cp -a "$ROOT"/. "$WORK/$TD/"
cp packaging/install.sh "$WORK/$TD/install.sh"; chmod 755 "$WORK/$TD/install.sh"
tar -czf "$OUT/$TD.tar.gz" -C "$WORK" --owner=0 --group=0 "$TD"

# ---------- Windows ----------
if command -v cargo-zigbuild >/dev/null 2>&1; then
  cargo zigbuild --release -p open-dhcp -p opendhcp-panel --target x86_64-pc-windows-gnu
  WD="open-dhcp-$V-windows-x86_64"
  mkdir -p "$WORK/$WD"
  cp target/x86_64-pc-windows-gnu/release/open-dhcp.exe "$WORK/$WD/OpenDHCP.exe"
  cp target/x86_64-pc-windows-gnu/release/opendhcp-panel.exe "$WORK/$WD/"
  DLL="$(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -path '*webview2-com-sys-*/x64/WebView2Loader.dll' | sort | tail -1)"
  cp "$DLL" "$WORK/$WD/WebView2Loader.dll"
  cp packaging/README-windows.txt "$WORK/$WD/README.txt"
  cp ../LICENSE "$WORK/$WD/LICENSE.txt"
  (cd "$WORK" && zip -qr "$OUT/$WD.zip" "$WD")
else
  echo "aviso: cargo-zigbuild não encontrado; pacote do Windows não gerado" >&2
fi

(cd "$OUT" && sha256sum -- open-dhcp-* > SHA256SUMS.txt && cat SHA256SUMS.txt)
echo "Pacotes em $OUT"
