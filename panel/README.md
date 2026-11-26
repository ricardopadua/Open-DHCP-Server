# opendhcp-panel

Painel web de administração e monitoramento em tempo real para o Open DHCP Server, escrito em
Rust. É um programa separado: **o servidor DHCP (`opendhcpd`) não muda**. O painel:

- mostra **ao vivo** (Server-Sent Events, sem recarregar a página) os dispositivos com IP, a
  ocupação das faixas, o estado do serviço e os eventos do servidor;
- edita o `opendhcp.ini` (reservas, faixas e opções globais), **preservando comentários e a ordem**
  do arquivo e fazendo backup antes de cada gravação;
- inicia, para e reinicia o serviço por comandos configuráveis (por padrão `systemctl`).

![Visão geral](docs/visao-geral.jpg)

![Leases ativos no tema escuro](docs/leases-escuro.jpg)

## Telas

| Tela | O que faz |
|---|---|
| Visão geral | Cartão do serviço (Iniciar/Parar/Reiniciar), indicadores, gráfico de eventos das últimas 2 h, uso das faixas, ocupação geral e atividade recente |
| Atividade | Feed ao vivo do log do servidor: IP entregue, renovação, oferta, liberação, conflito, recusa, pedido sem faixa; filtros por tipo e texto. **Dias anteriores** pelo calendário (histórico em disco) |
| Leases ativos | Dispositivos com IP agora, com destaque para os **novos** e **renovados**, fabricante pelo MAC (ou "MAC aleatório") e apelido; **Fixar IP** (vira reserva) e **Liberar** |
| Dispositivo | Clicando num MAC: fabricante, situação atual, reserva, histórico de eventos daquele aparelho, e **apelido e anotações** (guardados em `data_dir/devices.json`, fora do `.ini`) |
| Reservas | IP fixo por MAC (`[00:11:22:33:44:55]` + `IP=`), nome do host, bloqueio de dispositivo, se está com IP agora. Botão **IP livre** sugere o primeiro IP disponível; aviso (e recusa ao salvar) se o IP estiver com outro dispositivo. **Importar/exportar CSV** (`mac,ip,hostname`, tudo ou nada) |
| Faixas | `[RANGE_SET]`: faixas `DHCPRange`, máscara, gateway, DNS e tempo de lease, com a ocupação de cada faixa. Máscara por prefixo (/8 a /32), faixa pelo primeiro IP + quantidade, rede/broadcast/IPs utilizáveis calculados ao digitar, lease em minutos/horas/dias |
| Opções globais | `[GLOBAL_OPTIONS]`: domínio, máscara, gateway, DNS e lease padrão |
| Minha conta | Troca o usuário e a senha do painel (grava o novo hash no `panel.toml` e encerra as outras sessões) |

**Idiomas:** português, inglês, francês e espanhol. O seletor (globo, ao lado do sino) fica no
topo de todas as telas e no login; a escolha vale por 1 ano (cookie `odp_lang`). Na primeira
visita o painel usa o idioma do navegador (`Accept-Language`) e, se não for nenhum desses, o
português.

**Alertas** no topo de todas as telas: serviço parado, faixa com 80%/90% ocupada, pedidos de IP
sem faixa correspondente (dispositivos sem rede) e conflitos de IP nos últimos 15 minutos.

Opções que o painel não edita (filtros, opções numéricas, PXE etc.) aparecem como "outras
opções" e são mantidas no arquivo exatamente como estão. O servidor só lê o `.ini` ao iniciar,
então alterações valem depois de **Reiniciar** (o painel mostra um aviso com o botão). Os leases
ativos não se perdem: o servidor os grava no arquivo de estado e relê ao subir.

## Como o tempo real funciona

Um monitor no painel confere a cada segundo:

- o **arquivo de estado** (`-s`), que o servidor regrava a cada IP entregue ou renovado;
- o **log** (`-l`, com `%Y%m%d`): lê só as linhas novas, troca de arquivo na virada do dia e, ao
  iniciar, carrega o fim do log de hoje e de ontem para o histórico;
- o **estado do serviço** (a cada 3 s).

Quando algo muda, cada navegador conectado em `/live` recebe só os blocos da página aberta,
já renderizados (e escapados) no servidor. Se a conexão cair, a página tenta de novo sozinha e
volta para o login se a sessão tiver expirado.

## Instalação

```sh
cargo build --release
sudo install -m 755 target/release/opendhcp-panel /usr/local/bin/
sudo mkdir -p /etc/opendhcp-panel
sudo cp panel.example.toml /etc/opendhcp-panel/panel.toml
echo admin | opendhcp-panel hash-password   # cole o hash em [auth] password_hash
sudo cp opendhcp.service opendhcp-panel.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now opendhcp opendhcp-panel
```

Ajuste em `panel.toml` os caminhos `ini_file`, `state_file` e `log_file` para os mesmos usados no
`opendhcpd` (`-i`, `-s` e `-l`). O painel abre em `http://127.0.0.1:8067`.
Com a senha de fábrica `admin`, todas as telas mostram um aviso até ela ser trocada em
**Minha conta**.

## Segurança

- Login com senha em hash Argon2; sessão em cookie `HttpOnly` + `SameSite=Strict`; token CSRF em
  todo formulário; bloqueio de 5 minutos após 5 senhas erradas pelo mesmo IP.
- Tudo o que vem dos dispositivos (nome do host no log e no arquivo de estado) é escapado no HTML;
  CSP sem script nem estilo inline.
- Entradas são validadas (MAC, IPv4, máscara, faixas sem sobreposição, limite de 125 faixas) e
  valores com quebra de linha são recusados, para não injetar seções no `.ini`.
- Se o `.ini` mudar entre abrir e salvar um formulário, a gravação é recusada (sem sobrescrever
  a alteração de outra pessoa).
- Ouve só em `127.0.0.1` por padrão. Para acesso remoto, use um proxy com HTTPS e
  `secure_cookie = true`. Não exponha o painel sem TLS na mesma rede dos dispositivos.

## Traduções

Os textos ficam em português no código, marcados com `t("...")`, `tf!("... {}", valor)`,
`tm(...)` (com HTML), `n_("...")` (constantes) ou `msg!(...)` (mensagens traduzidas na hora de
mostrar). As traduções estão em `src/i18n_table.rs`, uma linha por texto:
`[português, inglês, francês, espanhol]`. O `cargo test` falha se algum texto marcado ficar sem
tradução, se sobrar tradução sem uso ou se o número de `{}` não bater.

## Desenvolvimento

```sh
cargo test     # INI, arquivo de estado (struct data8, 104 bytes), log, monitor, alertas, login
cargo run -- serve -c panel.toml
```

Para testar sem root/DHCP real, aponte os comandos de `[service]` para um script que simule o
serviço e use uma cópia do `opendhcp.ini`; um script que escreva linhas no log e regrave o
arquivo de estado (no formato acima) faz a tela se mexer como num servidor de verdade.

**Histórico:** os eventos do log ficam num SQLite em `data_dir/history.sqlite` (por padrão, 90
dias; `history_days`), sem duplicar quando o mesmo trecho do log é relido. Ele alimenta os dias
anteriores da tela Atividade e o histórico de cada dispositivo, e guarda as notificações fechadas
para não voltarem quando o painel reinicia.

Limitações atuais: o layout do arquivo de estado é o do Linux 64 bits (x86_64/aarch64); o painel
não edita `[LISTEN_ON]`, `[REPLICATION_SERVERS]` nem filtros de faixa.
