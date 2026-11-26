# Após instalar/atualizar: pastas de trabalho e o serviço do painel ligado.
# O servidor DHCP (unit opendhcp) não é iniciado sozinho: inicie pelo painel quando o
# opendhcp.ini estiver pronto para a sua rede.
mkdir -p /opt/opendhcp/log /var/lib/opendhcp-panel
if [ -d /run/systemd/system ]; then
  systemctl daemon-reload >/dev/null 2>&1 || true
  systemctl enable opendhcp-panel.service >/dev/null 2>&1 || true
  systemctl restart opendhcp-panel.service >/dev/null 2>&1 || true
fi
