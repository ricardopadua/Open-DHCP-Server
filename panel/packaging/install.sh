#!/bin/sh
# Instala o Open DHCP a partir do .tar.gz (para distribuições sem .deb/.rpm). Rode como root:
#   sudo ./install.sh
# Copia os arquivos para /usr/bin, /opt/opendhcp, /etc/opendhcp-panel, as units do systemd e a
# regra do polkit, sem sobrescrever opendhcp.ini e panel.toml que já existam, e cria o usuário
# opendhcp-panel (o painel não roda como root).
set -e
cd "$(dirname "$0")"
[ "$(id -u)" -eq 0 ] || { echo "Rode como root: sudo ./install.sh" >&2; exit 1; }

for f in usr/bin/opendhcp-panel usr/bin/open-dhcp opt/opendhcp/opendhcpd usr/libexec/opendhcp/uninstall.sh; do
  install -Dm755 "$f" "/$f"
done
for f in usr/lib/systemd/system/opendhcp.service usr/lib/systemd/system/opendhcp-panel.service \
         usr/lib/systemd/system/opendhcp-uninstall@.service usr/share/polkit-1/rules.d/50-opendhcp-panel.rules \
         usr/share/applications/open-dhcp.desktop usr/share/icons/hicolor/256x256/apps/open-dhcp.png \
         usr/share/doc/open-dhcp/README.md usr/share/doc/open-dhcp/LICENSE; do
  install -Dm644 "$f" "/$f"
done
[ -e /opt/opendhcp/opendhcp.ini ] || install -Dm644 opt/opendhcp/opendhcp.ini /opt/opendhcp/opendhcp.ini
[ -e /etc/opendhcp-panel/panel.toml ] || install -Dm600 etc/opendhcp-panel/panel.toml /etc/opendhcp-panel/panel.toml
# usuário do painel, permissões e serviços (o mesmo passo dos pacotes .deb/.rpm)
sh ./setup.sh
echo "Pronto. Abra o Open DHCP pelo menu de aplicativos (usuário admin, senha admin; o painel pede a troca no primeiro acesso)."
echo "O servidor DHCP não foi iniciado: ajuste o opendhcp.ini pelo painel e clique em Iniciar."
