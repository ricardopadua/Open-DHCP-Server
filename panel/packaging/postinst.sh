# Configuração do sistema depois de instalar ou atualizar (pacotes .deb/.rpm, install.sh e o
# instalador do aplicativo). Pode rodar várias vezes.
# O servidor DHCP (unit opendhcp) não é iniciado sozinho: inicie pelo painel quando o
# opendhcp.ini estiver pronto para a sua rede.

# O painel roda com um usuário próprio, sem login e sem root: uma falha nele não vira root na
# máquina. O que ele pode pedir ao systemd está na regra do polkit (50-opendhcp-panel.rules).
if ! getent passwd opendhcp-panel >/dev/null 2>&1; then
  useradd --system --user-group --no-create-home --home-dir /var/lib/opendhcp-panel \
    --shell /usr/sbin/nologin opendhcp-panel >/dev/null 2>&1 || true
fi
G=root
getent group opendhcp-panel >/dev/null 2>&1 && G=opendhcp-panel

mkdir -p /opt/opendhcp/log
# leases do servidor: o painel lê e, ao liberar um lease, regrava o arquivo (a pasta continua do
# root: o painel não cria nem troca arquivos nela)
install -d -m 0750 /var/lib/opendhcp
# (install -d não muda a permissão de uma pasta que já existe: até a 1.2.8 ela era 0700)
chmod 0750 /var/lib/opendhcp || true
chgrp "$G" /var/lib/opendhcp || true

# 1.2.x guardava o estado dos leases em /tmp: passa a usar /var/lib/opendhcp. Os leases antigos
# só vêm junto se o arquivo for do root (em /tmp qualquer usuário poderia ter plantado um).
OLD=/tmp/opendhcp.state
NEW=/var/lib/opendhcp/opendhcp.state
if [ -f "$OLD" ] && [ ! -L "$OLD" ] && [ ! -e "$NEW" ] && [ "$(stat -c %u "$OLD")" = 0 ]; then
  cp "$OLD" "$NEW" || true
fi
if [ -f "$NEW" ] && [ ! -L "$NEW" ]; then
  chgrp "$G" "$NEW" || true
  chmod 0660 "$NEW" || true
fi

# opendhcp.ini: lido pelo servidor (root) e regravado pelo painel no próprio arquivo
if [ -f /opt/opendhcp/opendhcp.ini ] && [ ! -L /opt/opendhcp/opendhcp.ini ]; then
  chgrp "$G" /opt/opendhcp/opendhcp.ini || true
  chmod 0664 /opt/opendhcp/opendhcp.ini || true
fi

# configuração e dados do painel: só dele (o panel.toml tem o hash da senha)
install -d -m 0700 /var/lib/opendhcp-panel /etc/opendhcp-panel
if [ "$G" = opendhcp-panel ]; then
  chown -R opendhcp-panel:opendhcp-panel /var/lib/opendhcp-panel /etc/opendhcp-panel || true
fi
if [ -f /etc/opendhcp-panel/panel.toml ]; then
  sed -i 's|^state_file = "/tmp/opendhcp.state"|state_file = "/var/lib/opendhcp/opendhcp.state"|' /etc/opendhcp-panel/panel.toml || true
  chmod 0600 /etc/opendhcp-panel/panel.toml || true
fi

if [ -d /run/systemd/system ]; then
  systemctl daemon-reload >/dev/null 2>&1 || true
  systemctl enable opendhcp-panel.service >/dev/null 2>&1 || true
  # numa atualização, os serviços passam a usar os binários e as units novas
  systemctl restart opendhcp-panel.service >/dev/null 2>&1 || true
  systemctl try-restart opendhcp.service >/dev/null 2>&1 || true
fi
