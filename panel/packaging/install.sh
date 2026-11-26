#!/bin/sh
# Instala o Open DHCP a partir do .tar.gz (para distribuições sem .deb/.rpm). Rode como root:
#   sudo ./install.sh
# Copia os arquivos para /usr/bin, /opt/opendhcp, /etc/opendhcp-panel e as units do systemd,
# sem sobrescrever opendhcp.ini e panel.toml que já existam.
set -e
cd "$(dirname "$0")"
[ "$(id -u)" -eq 0 ] || { echo "Rode como root: sudo ./install.sh" >&2; exit 1; }

for f in usr/bin/opendhcp-panel usr/bin/open-dhcp opt/opendhcp/opendhcpd; do
  install -Dm755 "$f" "/$f"
done
for f in usr/lib/systemd/system/opendhcp.service usr/lib/systemd/system/opendhcp-panel.service \
         usr/share/applications/open-dhcp.desktop usr/share/icons/hicolor/256x256/apps/open-dhcp.png \
         usr/share/doc/open-dhcp/README.md usr/share/doc/open-dhcp/LICENSE; do
  install -Dm644 "$f" "/$f"
done
[ -e /opt/opendhcp/opendhcp.ini ] || install -Dm644 opt/opendhcp/opendhcp.ini /opt/opendhcp/opendhcp.ini
[ -e /etc/opendhcp-panel/panel.toml ] || install -Dm600 etc/opendhcp-panel/panel.toml /etc/opendhcp-panel/panel.toml
mkdir -p /opt/opendhcp/log
install -d -m 0700 /var/lib/opendhcp /var/lib/opendhcp-panel

if [ -d /run/systemd/system ]; then
  systemctl daemon-reload
  systemctl enable --now opendhcp-panel.service
fi
echo "Pronto. Abra o Open DHCP pelo menu de aplicativos (usuário admin, senha admin; o painel pede a troca no primeiro acesso)."
echo "O servidor DHCP não foi iniciado: ajuste o opendhcp.ini pelo painel e clique em Iniciar."
