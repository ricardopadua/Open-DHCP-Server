#!/bin/sh
# Remove o Open DHCP instalado pelo aplicativo (AppImage) ou pelo install.sh.
# Uso: uninstall.sh keep|purge. Com purge, apaga também a configuração, os leases, os backups e
# o usuário do painel; com keep, eles ficam para uma reinstalação.
MODE="$1"
# dá tempo de o painel responder antes de ser parado
sleep 2
systemctl disable --now opendhcp.service opendhcp-panel.service || true
rm -f /etc/systemd/system/opendhcp.service /etc/systemd/system/opendhcp-panel.service \
  /usr/lib/systemd/system/opendhcp.service /usr/lib/systemd/system/opendhcp-panel.service \
  /usr/local/bin/opendhcp-panel /usr/bin/opendhcp-panel /usr/bin/open-dhcp /opt/opendhcp/opendhcpd \
  /usr/share/applications/open-dhcp.desktop /usr/share/icons/hicolor/256x256/apps/open-dhcp.png \
  /etc/polkit-1/rules.d/50-opendhcp-panel.rules /usr/share/polkit-1/rules.d/50-opendhcp-panel.rules
rm -rf /usr/share/doc/open-dhcp
if [ "$MODE" = purge ]; then
  rm -rf /opt/opendhcp /etc/opendhcp-panel /var/lib/opendhcp /var/lib/opendhcp-panel
  userdel opendhcp-panel 2>/dev/null || true
  groupdel opendhcp-panel 2>/dev/null || true
fi
# por último, a própria desinstalação
rm -f /etc/systemd/system/opendhcp-uninstall@.service /usr/lib/systemd/system/opendhcp-uninstall@.service
rm -rf /usr/libexec/opendhcp
systemctl daemon-reload
systemctl reset-failed opendhcp.service opendhcp-panel.service 2>/dev/null || true
