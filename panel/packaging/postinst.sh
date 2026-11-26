# Após instalar/atualizar: pastas de trabalho e o serviço do painel ligado.
# O servidor DHCP (unit opendhcp) não é iniciado sozinho: inicie pelo painel quando o
# opendhcp.ini estiver pronto para a sua rede.
mkdir -p /opt/opendhcp/log
install -d -m 0700 /var/lib/opendhcp /var/lib/opendhcp-panel

# 1.2.x guardava o estado dos leases em /tmp: passa a usar /var/lib/opendhcp. Os leases antigos
# só vêm junto se o arquivo for do root (em /tmp qualquer usuário poderia ter plantado um).
OLD=/tmp/opendhcp.state
NEW=/var/lib/opendhcp/opendhcp.state
if [ -f "$OLD" ] && [ ! -L "$OLD" ] && [ ! -e "$NEW" ] && [ "$(stat -c %u "$OLD")" = 0 ]; then
  cp "$OLD" "$NEW" && chmod 0644 "$NEW"
fi
if [ -f /etc/opendhcp-panel/panel.toml ]; then
  sed -i 's|^state_file = "/tmp/opendhcp.state"|state_file = "/var/lib/opendhcp/opendhcp.state"|' /etc/opendhcp-panel/panel.toml
  chmod 0600 /etc/opendhcp-panel/panel.toml
fi

if [ -d /run/systemd/system ]; then
  systemctl daemon-reload >/dev/null 2>&1 || true
  systemctl enable opendhcp-panel.service >/dev/null 2>&1 || true
  systemctl restart opendhcp-panel.service >/dev/null 2>&1 || true
  # o DHCP já rodando passa a usar a unit nova (estado fora do /tmp, isolamento)
  if systemctl is-active --quiet opendhcp.service; then
    systemctl restart opendhcp.service >/dev/null 2>&1 || true
  fi
fi
