# Antes de remover (não em atualização): para e desliga os serviços.
if [ -d /run/systemd/system ]; then
  systemctl disable --now opendhcp-panel.service opendhcp.service >/dev/null 2>&1 || true
fi
