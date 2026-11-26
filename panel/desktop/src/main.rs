//! Open DHCP: o painel do servidor DHCP numa janela própria (WebView do sistema: WebKitGTK no
//! Linux, WebView2 no Windows), sem abrir o navegador.
//!
//! O que a janela mostra:
//! - `--url <endereço>` ou `OPENDHCP_URL`: esse painel;
//! - Linux com o serviço instalado (`/etc/opendhcp-panel/panel.toml`): o painel do serviço; se ele
//!   estiver parado, uma tela com o botão para iniciá-lo;
//! - nos outros casos (Windows, ou sem o serviço): o painel roda dentro do próprio aplicativo,
//!   com a configuração em `<pasta de configuração do usuário>/OpenDHCP/panel.toml` (criada na
//!   primeira vez) ou a de `--config <arquivo>`.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Icon, Window, WindowBuilder};
use wry::{WebView, WebViewBuilder};

const SYSTEM_CONFIG: &str = "/etc/opendhcp-panel/panel.toml";
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// O que carregar na janela.
enum Target {
    /// Painel no ar.
    Url(String),
    /// Painel do serviço instalado, mas parado.
    Offline(String),
    /// Linux sem o servidor DHCP instalado: oferecer instalar o que vem dentro do aplicativo.
    Install { config: PathBuf },
    /// Servidor instalado por este aplicativo, mas de outra versão: oferecer atualizar.
    Update(String),
    /// O opendhcp.ini não existe e criar exige permissão de administrador.
    MissingIni { ini: PathBuf, config: PathBuf },
    /// Não deu para iniciar o painel embutido.
    Failed { error: String, config: PathBuf },
}

/// Por que o painel embutido não subiu.
enum StartError {
    MissingIni(PathBuf),
    Other(String),
}

enum UserEvent {
    /// Pedido da tela de aviso (botões).
    Ipc(String),
    /// O serviço do painel ficou no ar (ou o motivo de não ter ficado).
    Reachable(String, Result<(), String>),
    /// Arquivo baixado (ex.: exportar CSV).
    Downloaded(PathBuf),
    /// Resultado de criar o .ini e subir o painel embutido de novo.
    Embedded(Result<String, String>),
    /// Resultado de instalar o servidor DHCP e o serviço do painel (URL do painel do serviço).
    Installed(Result<String, String>),
}

// ---------- textos da própria janela (o painel tem a tradução dele) ----------

#[derive(Clone, Copy)]
enum Lang {
    Pt,
    En,
    Fr,
    Es,
}

fn lang() -> Lang {
    match sys_locale::get_locale().unwrap_or_default().get(..2).unwrap_or("") {
        "pt" => Lang::Pt,
        "fr" => Lang::Fr,
        "es" => Lang::Es,
        _ => Lang::En,
    }
}

fn tr(key: &str) -> &'static str {
    let row: [&'static str; 4] = match key {
        "offline_title" => ["O painel não está rodando", "The panel is not running", "Le panneau n'est pas lancé", "El panel no está en ejecución"],
        "offline_text" => [
            "O serviço opendhcp-panel está instalado, mas não respondeu em",
            "The opendhcp-panel service is installed but did not respond at",
            "Le service opendhcp-panel est installé mais n'a pas répondu à",
            "El servicio opendhcp-panel está instalado, pero no respondió en",
        ],
        "start" => ["Iniciar o painel", "Start the panel", "Démarrer le panneau", "Iniciar el panel"],
        "retry" => ["Tentar de novo", "Try again", "Réessayer", "Reintentar"],
        "starting" => ["Iniciando…", "Starting…", "Démarrage…", "Iniciando…"],
        "still_off" => [
            "O painel ainda não respondeu. Veja: systemctl status opendhcp-panel",
            "The panel still did not respond. Check: systemctl status opendhcp-panel",
            "Le panneau ne répond toujours pas. Voir : systemctl status opendhcp-panel",
            "El panel todavía no respondió. Vea: systemctl status opendhcp-panel",
        ],
        "failed_title" => ["Não foi possível abrir o painel", "Could not open the panel", "Impossible d'ouvrir le panneau", "No se pudo abrir el panel"],
        "failed_text" => [
            "Ajuste os caminhos do servidor DHCP neste arquivo e abra o Open DHCP de novo:",
            "Fix the DHCP server paths in this file and open Open DHCP again:",
            "Corrigez les chemins du serveur DHCP dans ce fichier et rouvrez Open DHCP :",
            "Ajuste las rutas del servidor DHCP en este archivo y vuelva a abrir Open DHCP:",
        ],
        "open_config" => ["Abrir a configuração", "Open the configuration", "Ouvrir la configuration", "Abrir la configuración"],
        "missing_title" => [
            "Falta o arquivo de configuração do servidor DHCP",
            "The DHCP server configuration file is missing",
            "Le fichier de configuration du serveur DHCP est absent",
            "Falta el archivo de configuración del servidor DHCP",
        ],
        "missing_text" => [
            "O opendhcp.ini não existe nesta pasta. Posso criá-lo com a configuração padrão do Open DHCP (você ajusta faixas e opções depois, pelo painel). A pasta é do sistema, então será pedida a senha de administrador.",
            "opendhcp.ini does not exist in this folder. I can create it with the Open DHCP default configuration (you adjust ranges and options later in the panel). The folder belongs to the system, so the administrator password will be requested.",
            "opendhcp.ini n'existe pas dans ce dossier. Je peux le créer avec la configuration par défaut d'Open DHCP (vous ajusterez plages et options ensuite dans le panneau). Le dossier appartient au système : le mot de passe administrateur sera demandé.",
            "opendhcp.ini no existe en esta carpeta. Puedo crearlo con la configuración predeterminada de Open DHCP (después ajusta rangos y opciones en el panel). La carpeta es del sistema, así que se pedirá la contraseña de administrador.",
        ],
        "create" => ["Criar o arquivo", "Create the file", "Créer le fichier", "Crear el archivo"],
        "busy_title" => [
            "A porta do painel está ocupada por outro programa",
            "The panel's port is used by another program",
            "Le port du panneau est utilisé par un autre programme",
            "El puerto del panel está ocupado por otro programa",
        ],
        "busy_text" => [
            "O serviço opendhcp-panel não conseguiu abrir a porta abaixo porque outro programa já a usa. Feche esse programa (ou mude o listen no panel.toml) e tente de novo.",
            "The opendhcp-panel service could not open the port below because another program already uses it. Close that program (or change listen in panel.toml) and try again.",
            "Le service opendhcp-panel n'a pas pu ouvrir le port ci-dessous, déjà utilisé par un autre programme. Fermez ce programme (ou changez listen dans panel.toml) puis réessayez.",
            "El servicio opendhcp-panel no pudo abrir el puerto de abajo porque otro programa ya lo usa. Cierre ese programa (o cambie listen en panel.toml) y vuelva a intentarlo.",
        ],
        "busy_short" => [
            "A porta do painel está ocupada por outro programa:",
            "The panel's port is used by another program:",
            "Le port du panneau est utilisé par un autre programme :",
            "El puerto del panel está ocupado por otro programa:",
        ],
        "update_title" => [
            "Há uma versão nova do servidor DHCP",
            "A new version of the DHCP server is available",
            "Une nouvelle version du serveur DHCP est disponible",
            "Hay una versión nueva del servidor DHCP",
        ],
        "update_text" => [
            "O servidor DHCP e o painel instalados neste computador são de outra versão. Posso atualizá-los com os deste aplicativo; a configuração, os leases e a senha são mantidos. Será pedida a senha de administrador, e o servidor DHCP, se estiver rodando, reinicia.",
            "The DHCP server and panel installed on this computer are a different version. I can update them with the ones in this app; the configuration, leases and password are kept. The administrator password will be asked, and the DHCP server restarts if it is running.",
            "Le serveur DHCP et le panneau installés sur cet ordinateur sont d'une autre version. Je peux les mettre à jour avec ceux de cette application ; la configuration, les baux et le mot de passe sont conservés. Le mot de passe administrateur sera demandé, et le serveur DHCP redémarre s'il est lancé.",
            "El servidor DHCP y el panel instalados en este equipo son de otra versión. Puedo actualizarlos con los de esta aplicación; se mantienen la configuración, las concesiones y la contraseña. Se pedirá la contraseña de administrador, y el servidor DHCP se reinicia si está en ejecución.",
        ],
        "update" => ["Atualizar", "Update", "Mettre à jour", "Actualizar"],
        "updating" => ["Atualizando…", "Updating…", "Mise à jour…", "Actualizando…"],
        "later" => ["Agora não", "Not now", "Pas maintenant", "Ahora no"],
        "install_title" => [
            "O servidor DHCP não está instalado",
            "The DHCP server is not installed",
            "Le serveur DHCP n'est pas installé",
            "El servidor DHCP no está instalado",
        ],
        "install_text" => [
            "O Open DHCP traz o servidor DHCP e o serviço do painel. Posso instalá-los neste computador (em /opt/opendhcp, com os serviços do systemd); será pedida a senha de administrador uma vez. O servidor não começa a entregar IPs sozinho: confira as faixas no painel e clique em Iniciar.",
            "Open DHCP ships the DHCP server and the panel service. I can install them on this computer (in /opt/opendhcp, with systemd services); the administrator password will be asked once. The server does not start handing out IPs by itself: check the ranges in the panel and press Start.",
            "Open DHCP contient le serveur DHCP et le service du panneau. Je peux les installer sur cet ordinateur (dans /opt/opendhcp, avec des services systemd) ; le mot de passe administrateur sera demandé une fois. Le serveur ne distribue pas d'adresses tout seul : vérifiez les plages dans le panneau puis cliquez sur Démarrer.",
            "Open DHCP incluye el servidor DHCP y el servicio del panel. Puedo instalarlos en este equipo (en /opt/opendhcp, con servicios de systemd); se pedirá la contraseña de administrador una vez. El servidor no empieza a entregar IP solo: revise los rangos en el panel y pulse Iniciar.",
        ],
        "install" => ["Instalar o servidor DHCP", "Install the DHCP server", "Installer le serveur DHCP", "Instalar el servidor DHCP"],
        "installing" => ["Instalando…", "Installing…", "Installation…", "Instalando…"],
        "skip" => ["Continuar sem instalar", "Continue without installing", "Continuer sans installer", "Continuar sin instalar"],
        "install_failed" => [
            "A instalação não foi concluída:",
            "The installation did not finish:",
            "L'installation n'a pas abouti :",
            "La instalación no terminó:",
        ],
        "creating" => ["Criando…", "Creating…", "Création…", "Creando…"],
        "create_failed" => [
            "Não foi possível criar o arquivo:",
            "Could not create the file:",
            "Impossible de créer le fichier :",
            "No se pudo crear el archivo:",
        ],
        "saved" => ["Arquivo salvo em", "File saved to", "Fichier enregistré dans", "Archivo guardado en"],
        _ => [""; 4],
    };
    row[lang() as usize]
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Tela de aviso no mesmo visual do painel (sem depender dele estar no ar).
fn notice_page(title: &str, text: &str, detail: &str, buttons: &[(&str, &str, bool)]) -> String {
    let buttons: String = buttons
        .iter()
        .map(|(action, label, primary)| {
            format!(r#"<button class="{}" onclick="go('{action}', this)">{}</button>"#, if *primary { "primary" } else { "light" }, esc(label))
        })
        .collect();
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><style>
:root {{ color-scheme: light dark; }}
body {{ margin:0; min-height:100vh; display:grid; place-items:center; font-family: system-ui, "Segoe UI", sans-serif;
  background: radial-gradient(circle at 20% 10%, rgba(132,90,223,.18), transparent 50%), #f0f1f7; color:#333335; }}
@media (prefers-color-scheme: dark) {{ body {{ background: radial-gradient(circle at 20% 10%, rgba(132,90,223,.25), transparent 50%), #1a1c1e; color:#ebeef2; }}
  .card {{ background:#252729 !important; border-color:#313335 !important; }} code {{ background:#2b2e31 !important; }} }}
.card {{ width:min(34rem, calc(100vw - 2rem)); background:#fff; border:1px solid #e9edf6; border-radius:1rem; padding:2rem;
  box-shadow: 0 1rem 3rem rgba(28,39,60,.15); }}
.logo {{ width:3rem; height:3rem; border-radius:.8rem; background:linear-gradient(135deg,#845adf,#49b6f5); display:grid; place-items:center; color:#fff; font-size:1.5rem; }}
h1 {{ font-size:1.2rem; margin:1rem 0 .5rem; }}
p {{ line-height:1.5; opacity:.85; margin:.25rem 0; }}
code {{ display:block; margin:.75rem 0; padding:.6rem .8rem; border-radius:.5rem; background:#f7f8f9; font-size:.85rem; word-break:break-all; }}
.actions {{ display:flex; gap:.6rem; flex-wrap:wrap; margin-top:1.25rem; }}
button {{ font:inherit; font-size:.9rem; font-weight:600; border-radius:.5rem; padding:.6rem 1rem; cursor:pointer; border:1px solid transparent; }}
button.primary {{ background:#845adf; color:#fff; }}
button.light {{ background:transparent; border-color:#c9cdd6; color:inherit; }}
button:disabled {{ opacity:.6; cursor:wait; }}
#msg {{ margin-top:1rem; font-size:.85rem; color:#e6533c; }}
</style></head><body><div class="card"><div class="logo"><svg viewBox="0 0 24 24" width="26" height="26" fill="none" stroke="white" stroke-width="2" stroke-linecap="round"><path d="M12 18h.01M9.17 15.17a4 4 0 0 1 5.66 0M6.34 12.34a8 8 0 0 1 11.31 0M3.51 9.51a12 12 0 0 1 16.97 0"/></svg></div><h1>{}</h1><p>{}</p><code>{}</code>
<div class="actions">{buttons}</div><div id="msg"></div></div>
<script>function go(a, b) {{ if (a === 'start' || a === 'create' || a === 'install' || a === 'update') {{ b.disabled = true; b.textContent = a === 'start' ? {} : a === 'create' ? {} : a === 'install' ? {} : {}; }} window.ipc.postMessage(a); }}</script>
</body></html>"#,
        esc(title),
        esc(text),
        esc(detail),
        serde_quote(tr("starting")),
        serde_quote(tr("creating")),
        serde_quote(tr("installing")),
        serde_quote(tr("updating")),
    )
}

/// Texto como literal JavaScript seguro (escapa aspas, quebras de linha e `</script>`).
fn serde_quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()).replace("</", "<\\/")
}

/// `esquema://host:porta` de uma URL (sem caminho).
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| format!("{}://{}", scheme.to_ascii_lowercase(), host.to_ascii_lowercase()))
}

// ---------- de onde vem o painel ----------

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// `listen` do panel.toml → URL local.
fn url_from_config(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: toml::Table = text.parse().ok()?;
    let listen = v.get("listen").and_then(|l| l.as_str()).unwrap_or("127.0.0.1:8067");
    let mut addr: SocketAddr = listen.parse().ok()?;
    if addr.ip().is_unspecified() {
        addr.set_ip([127, 0, 0, 1].into());
    }
    Some(format!("http://{addr}/"))
}

/// URL do painel do serviço instalado. O panel.toml do sistema é só do root (0600, tem o hash
/// da senha): sem permissão de leitura, vale o endereço padrão do serviço.
fn system_url() -> String {
    url_from_config(Path::new(SYSTEM_CONFIG)).unwrap_or_else(|| "http://127.0.0.1:8067/".to_string())
}

/// `systemctl is-active opendhcp-panel` (None fora do Linux ou sem systemctl).
fn panel_service_active() -> Option<bool> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", "opendhcp-panel.service"])
        .status()
        .ok()
        .map(|s| s.success())
}

/// Algo responde no endereço do painel, mas o serviço está parado: é outro programa na porta.
fn port_busy(url: &str) -> bool {
    panel_service_active() == Some(false) && reachable(url)
}

/// Espera o serviço do painel ficar ativo e responder (`tries` × 0,5 s). Confere o systemd e não
/// só a porta: outro programa na mesma porta responderia, mas não seria o painel instalado.
fn wait_service(url: &str, tries: u32) -> Result<(), String> {
    for i in 0..tries {
        if panel_service_active() != Some(false) && reachable(url) {
            return Ok(());
        }
        if i + 1 < tries {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    Err(if port_busy(url) { format!("{} {}", tr("busy_short"), host_of(url)) } else { tr("still_off").to_string() })
}

fn host_of(url: &str) -> &str {
    url.trim_start_matches("http://").trim_start_matches("https://").split('/').next().unwrap_or(url)
}

/// O servidor foi instalado por este aplicativo (/usr/local/bin) e os binários diferem dos que
/// vêm com ele.
fn server_outdated(payload: &Path) -> bool {
    let installed = Path::new("/usr/local/bin/opendhcp-panel");
    if !installed.exists() {
        return false;
    }
    let differs = |a: &Path, b: &Path| match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x != y,
        _ => false,
    };
    differs(installed, &payload.join("opendhcp-panel")) || differs(Path::new("/opt/opendhcp/opendhcpd"), &payload.join("opendhcpd"))
}

/// A unit do painel existe (instalada por pacote, install.sh ou por este aplicativo). Sem ela, um
/// panel.toml que sobrou de uma desinstalação não quer dizer que o servidor está instalado.
fn service_installed() -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    ["/etc/systemd/system", "/usr/lib/systemd/system", "/lib/systemd/system"]
        .iter()
        .any(|d| Path::new(d).join("opendhcp-panel.service").exists())
}

fn reachable(url: &str) -> bool {
    let host = url.trim_start_matches("http://").trim_start_matches("https://").split('/').next().unwrap_or("");
    host.to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .is_some_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(800)).is_ok())
}

/// Configuração do painel embutido; criada na primeira vez com os caminhos padrão do sistema.
fn user_config() -> anyhow::Result<PathBuf> {
    let dir = dirs::config_dir().ok_or_else(|| anyhow::anyhow!("sem pasta de configuração"))?.join("OpenDHCP");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("panel.toml");
    if !path.exists() {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600); // tem o hash da senha
        }
        use std::io::Write;
        opts.open(&path)?.write_all(default_config(&dir)?.as_bytes())?;
    }
    // O aplicativo entra pelo link de uso único: a senha de fábrica vira uma aleatória, para que
    // outro usuário da máquina não entre no painel embutido com admin/admin.
    if let Err(e) = opendhcp_panel::replace_default_password(&path) {
        tracing::warn!(error = %e, "não consegui trocar a senha padrão do painel embutido");
    }
    Ok(path)
}

fn toml_str(p: &Path) -> String {
    format!("'{}'", p.display())
}

fn default_config(dir: &Path) -> anyhow::Result<String> {
    // provisória: user_config troca logo em seguida por uma senha aleatória (OsRng)
    let hash = opendhcp_panel::hash_password("admin")?;
    let data = toml_str(dir);
    let backups = toml_str(&dir.join("backups"));
    let (paths, service) = if cfg!(windows) {
        let ps = |cmd: &str| format!(r#"["powershell", "-NoProfile", "-NonInteractive", "-Command", "{cmd}"]"#);
        (
            r"ini_file = 'C:\OpenDHCPServer\OpenDHCPServer.ini'
state_file = 'C:\OpenDHCPServer\OpenDHCPServer.state'
log_file = 'C:\OpenDHCPServer\log\OpenDHCPServer%Y%m%d.log'"
                .to_string(),
            format!(
                "status = {}\nstart = {}\nstop = {}\nrestart = {}",
                ps("if ((Get-Service -Name OpenDHCPServer -ErrorAction Stop).Status -eq 'Running') { exit 0 } else { exit 3 }"),
                ps("Start-Service -Name OpenDHCPServer"),
                ps("Stop-Service -Name OpenDHCPServer"),
                ps("Restart-Service -Name OpenDHCPServer"),
            ),
        )
    } else {
        (
            "ini_file = '/opt/opendhcp/opendhcp.ini'\nstate_file = '/var/lib/opendhcp/opendhcp.state'\nlog_file = '/opt/opendhcp/log/opendhcp%Y%m%d.log'".to_string(),
            [("status", "is-active"), ("start", "start"), ("stop", "stop"), ("restart", "restart")]
                .iter()
                .map(|(k, a)| {
                    if *k == "status" {
                        format!(r#"{k} = ["systemctl", "{a}", "--quiet", "opendhcp"]"#)
                    } else {
                        format!(r#"{k} = ["pkexec", "systemctl", "{a}", "opendhcp"]"#)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };
    Ok(format!(
        "# Open DHCP: configuração do painel embutido no aplicativo.\n\
         # Ajuste os caminhos para os mesmos usados pelo servidor DHCP.\n\
         listen = \"127.0.0.1:0\"\n{paths}\nbackup_dir = {backups}\ndata_dir = {data}\n\n\
         [auth]\nusername = \"admin\"\n# senha aleatória: o aplicativo entra sozinho (link de uso único)\npassword_hash = \"{hash}\"\n\n\
         [service]\n{service}\n"
    ))
}

/// Sobe o painel dentro do aplicativo, numa porta livre de 127.0.0.1.
fn start_embedded(config: &Path) -> Result<String, StartError> {
    let (tx, rx) = mpsc::channel();
    let config = config.to_path_buf();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(Err(StartError::Other(e.to_string())));
                return;
            }
        };
        rt.block_on(async move {
            match opendhcp_panel::start(config).await {
                Ok(server) => {
                    let _ = tx.send(Ok(format!("http://{}{}", server.local_addr(), server.login_link())));
                    if let Err(e) = server.run().await {
                        tracing::error!(error = %e, "o painel embutido parou");
                    }
                }
                Err(e) => {
                    let err = match e.downcast_ref::<opendhcp_panel::MissingIni>() {
                        Some(m) if m.error.kind() == std::io::ErrorKind::PermissionDenied => StartError::MissingIni(m.path.clone()),
                        _ => StartError::Other(format!("{e:#}")),
                    };
                    let _ = tx.send(Err(err));
                }
            }
        });
    });
    rx.recv_timeout(Duration::from_secs(30)).map_err(|e| StartError::Other(e.to_string()))?
}

/// Cria o opendhcp.ini numa pasta do sistema, pedindo a senha de administrador (polkit). O
/// conteúdo vai pela entrada padrão (nada de arquivo temporário em /tmp) e um arquivo que já
/// exista nunca é sobrescrito.
fn create_ini_as_admin(ini: &Path) -> Result<(), String> {
    use std::io::Write;
    let script = r#"set -e; [ -e "$1" ] && exit 3; mkdir -p "$(dirname "$1")"; umask 022; cat > "$1""#;
    let mut child = std::process::Command::new("pkexec")
        .args(["sh", "-c", script, "sh"])
        .arg(ini)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(opendhcp_panel::DEFAULT_INI.as_bytes()).map_err(|e| e.to_string())?;
    }
    match child.wait() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) if s.code() == Some(3) => Err(format!("{} já existe", ini.display())),
        Ok(s) => Err(format!("pkexec: {s}")),
        Err(e) => Err(e.to_string()),
    }
}

// ---------- instalar o servidor DHCP (AppImage) ----------

const UNIT_DHCP: &str = include_str!("../../opendhcp.service");
const UNIT_PANEL: &str = include_str!("../../opendhcp-panel.service");
const PANEL_EXAMPLE: &str = include_str!("../../panel.example.toml");
// regra do polkit (o que o painel, sem root, pode pedir ao systemd), desinstalação pela página
// Manutenção e a configuração do sistema comum a todos os instaladores
const POLKIT_RULE: &str = include_str!("../../packaging/opendhcp-panel.rules");
const UNIT_UNINSTALL: &str = include_str!("../../packaging/opendhcp-uninstall@.service");
const UNINSTALL_SH: &str = include_str!("../../packaging/uninstall.sh");
const SETUP_SH: &str = include_str!("../../packaging/postinst.sh");

/// Pasta com `opendhcpd` e `opendhcp-panel` que vêm com o aplicativo (dentro do AppImage em
/// usr/lib/open-dhcp/server; ao lado do executável em desenvolvimento).
fn server_payload() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("../lib/open-dhcp/server"), dir.join("server"), dir.to_path_buf()]
        .into_iter()
        .find(|d| d.join("opendhcpd").is_file() && d.join("opendhcp-panel").is_file())
}

/// Marca de "continuar sem instalar" (não perguntar de novo).
fn skip_marker() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("OpenDHCP").join("no-server-install"))
}

/// Copia os arquivos para uma pasta privada, empacota com tar e manda para um script que roda
/// como root (pkexec). O conteúdo vai pela entrada padrão: o root não precisa ler a montagem do
/// AppImage, que é só do usuário. Configurações que já existam não são sobrescritas.
fn install_server(payload: &Path) -> Result<(), String> {
    use std::io::Write;
    let stage = std::env::temp_dir().join(format!("open-dhcp-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    {
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(&stage).map_err(|e| e.to_string())?;
    }
    let result = (|| -> Result<(), String> {
        let put = |name: &str, data: &[u8]| std::fs::write(stage.join(name), data).map_err(|e| format!("{name}: {e}"));
        let read = |name: &str| std::fs::read(payload.join(name)).map_err(|e| format!("{name}: {e}"));
        put("opendhcpd", &read("opendhcpd")?)?;
        put("opendhcp-panel", &read("opendhcp-panel")?)?;
        put("opendhcp.service", UNIT_DHCP.as_bytes())?;
        put("opendhcp-panel.service", UNIT_PANEL.replace("/usr/bin/opendhcp-panel", "/usr/local/bin/opendhcp-panel").as_bytes())?;
        put("opendhcp.ini", opendhcp_panel::DEFAULT_INI.as_bytes())?;
        let hash = opendhcp_panel::hash_password("admin").map_err(|e| e.to_string())?;
        let toml: String = PANEL_EXAMPLE
            .lines()
            .map(|l| if l.starts_with("password_hash") { format!("password_hash = \"{hash}\"") } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n");
        put("panel.toml", toml.as_bytes())?;
        put("opendhcp-panel.rules", POLKIT_RULE.as_bytes())?;
        put("opendhcp-uninstall@.service", UNIT_UNINSTALL.as_bytes())?;
        put("uninstall.sh", UNINSTALL_SH.as_bytes())?;
        put("setup.sh", SETUP_SH.as_bytes())?;

        let tar = std::process::Command::new("tar")
            .args(["-c", "-C"])
            .arg(&stage)
            .arg(".")
            .stdout(std::process::Stdio::piped())
            .output()
            .map_err(|e| format!("tar: {e}"))?;
        if !tar.status.success() {
            return Err(format!("tar: {}", tar.status));
        }
        let script = r#"set -e
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
tar -x --no-same-owner -C "$T" -f -
install -d -m 0755 /opt/opendhcp /opt/opendhcp/log
install -m 0755 "$T/opendhcpd" /opt/opendhcp/opendhcpd
install -D -m 0755 "$T/opendhcp-panel" /usr/local/bin/opendhcp-panel
install -D -m 0644 "$T/opendhcp.service" /etc/systemd/system/opendhcp.service
install -D -m 0644 "$T/opendhcp-panel.service" /etc/systemd/system/opendhcp-panel.service
install -D -m 0644 "$T/opendhcp-uninstall@.service" /etc/systemd/system/opendhcp-uninstall@.service
install -D -m 0755 "$T/uninstall.sh" /usr/libexec/opendhcp/uninstall.sh
install -D -m 0644 "$T/opendhcp-panel.rules" /etc/polkit-1/rules.d/50-opendhcp-panel.rules
[ -e /opt/opendhcp/opendhcp.ini ] || install -m 0644 "$T/opendhcp.ini" /opt/opendhcp/opendhcp.ini
[ -e /etc/opendhcp-panel/panel.toml ] || install -D -m 0600 "$T/panel.toml" /etc/opendhcp-panel/panel.toml
# SELinux: contexto padrão dos caminhos (os arquivos vieram de uma pasta temporária)
if command -v restorecon >/dev/null 2>&1; then
  restorecon -R /opt/opendhcp /usr/local/bin/opendhcp-panel /usr/libexec/opendhcp /etc/opendhcp-panel /etc/polkit-1/rules.d/50-opendhcp-panel.rules \
    /etc/systemd/system/opendhcp.service /etc/systemd/system/opendhcp-panel.service /etc/systemd/system/opendhcp-uninstall@.service || true
fi
# usuário do painel (sem root), permissões e serviços: o mesmo passo dos pacotes .deb/.rpm
sh "$T/setup.sh"
command -v restorecon >/dev/null 2>&1 && restorecon -R /var/lib/opendhcp /var/lib/opendhcp-panel || true
"#;
        let mut child = std::process::Command::new("pkexec")
            .args(["sh", "-c", script])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("pkexec: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&tar.stdout).map_err(|e| e.to_string())?;
        }
        match child.wait() {
            Ok(s) if s.success() => Ok(()),
            Ok(s) if s.code() == Some(126) || s.code() == Some(127) => Err("pkexec: autorização negada".into()),
            Ok(s) => Err(format!("pkexec: {s}")),
            Err(e) => Err(e.to_string()),
        }
    })();
    let _ = std::fs::remove_dir_all(&stage);
    result
}

fn install_html() -> String {
    notice_page(tr("install_title"), tr("install_text"), "/opt/opendhcp · opendhcp.service · opendhcp-panel.service", &[("install", tr("install"), true), ("skip", tr("skip"), false)])
}

fn resolve() -> Target {
    if let Some(url) = arg("--url").or_else(|| std::env::var("OPENDHCP_URL").ok()) {
        return Target::Url(url);
    }
    let system = Path::new(SYSTEM_CONFIG);
    if arg("--config").is_none() && cfg!(unix) && system.exists() && service_installed() {
        let url = system_url();
        if cfg!(target_os = "linux") && server_payload().is_some_and(|p| server_outdated(&p)) {
            return Target::Update(url);
        }
        return if wait_service(&url, 1).is_ok() { Target::Url(url) } else { Target::Offline(url) };
    }
    let config = match arg("--config").map(PathBuf::from).map(Ok).unwrap_or_else(user_config) {
        Ok(c) => c,
        Err(e) => return Target::Failed { error: e.to_string(), config: PathBuf::new() },
    };
    // Linux sem o servidor: o aplicativo traz o servidor e oferece instalar (uma vez)
    if cfg!(target_os = "linux")
        && arg("--config").is_none()
        && !service_installed()
        && server_payload().is_some()
        && !skip_marker().is_some_and(|m| m.exists())
    {
        return Target::Install { config };
    }
    match start_embedded(&config) {
        Ok(url) => Target::Url(url),
        Err(StartError::MissingIni(ini)) => Target::MissingIni { ini, config },
        Err(StartError::Other(error)) => Target::Failed { error, config },
    }
}

// ---------- janela ----------

fn window_icon() -> Option<Icon> {
    let decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Icon::from_rgba(buf, info.width, info.height).ok()
}

fn downloads_dir() -> PathBuf {
    // nunca a pasta pessoal: um nome como ".bashrc" vindo do servidor iria parar lá
    dirs::download_dir().unwrap_or_else(std::env::temp_dir)
}

/// Destino de um download: nome sem pastas, sem ponto no começo e sem caracteres de controle,
/// e sem sobrescrever um arquivo que já exista.
fn download_target(suggested: &Path) -> PathBuf {
    let name = suggested.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let name = if name.is_empty() || name.starts_with('.') || name.chars().any(|c| c.is_control()) { "download" } else { name };
    let dir = downloads_dir();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((a, b)) if !a.is_empty() => (a.to_string(), format!(".{b}")),
        _ => (name.to_string(), String::new()),
    };
    let mut path = dir.join(name);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    path
}

/// O que a janela pode fazer agora: as ações internas (IPC) só valem na tela de aviso local, e
/// a navegação fica presa à origem do painel.
#[derive(Default)]
struct Guard {
    notice: bool,
    origin: Option<String>,
}

fn build_webview(window: &Window, target: &Target, proxy: EventLoopProxy<UserEvent>, guard: Arc<Mutex<Guard>>) -> wry::Result<WebView> {
    let ipc_proxy = proxy.clone();
    let done_proxy = proxy;
    let ipc_guard = guard.clone();
    let nav_guard = guard.clone();
    let dl_guard = guard;
    let mut builder = WebViewBuilder::new()
        .with_ipc_handler(move |req| {
            // só a tela de aviso (HTML local, não http) pode pedir ações
            let from_web = req.uri().scheme_str().is_some_and(|s| s.starts_with("http"));
            if ipc_guard.lock().unwrap().notice && !from_web {
                let _ = ipc_proxy.send_event(UserEvent::Ipc(req.body().clone()));
            } else {
                tracing::warn!(uri = %req.uri(), "pedido interno ignorado (fora da tela de aviso)");
            }
        })
        .with_navigation_handler(move |url| {
            if url.starts_with("about:") || url.starts_with("data:") {
                return true;
            }
            let g = nav_guard.lock().unwrap();
            let ok = g.origin.as_deref().is_some_and(|o| origin_of(&url).as_deref() == Some(o));
            if !ok {
                tracing::warn!(%url, "navegação bloqueada: fora do painel");
            }
            ok
        })
        // exportar CSV etc.: salva na pasta Downloads, só se vier do painel
        .with_download_started_handler(move |url, path| {
            let g = dl_guard.lock().unwrap();
            if g.origin.as_deref().is_none_or(|o| origin_of(&url).as_deref() != Some(o)) {
                return false;
            }
            *path = download_target(path);
            true
        })
        .with_download_completed_handler(move |_url, path, ok| {
            if let (true, Some(p)) = (ok, path) {
                let _ = done_proxy.send_event(UserEvent::Downloaded(p));
            }
        });
    builder = match target {
        Target::Url(url) => builder.with_url(url),
        Target::Offline(url) => builder.with_html(offline_html(url)),
        Target::Update(_) => builder.with_html(update_html()),
        Target::MissingIni { ini, .. } => builder.with_html(missing_html(ini)),
        Target::Install { .. } => builder.with_html(install_html()),
        Target::Failed { error, config } => builder.with_html(failed_html(error, config)),
    };
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().expect("janela GTK");
        builder.build_gtk(vbox)
    }
    #[cfg(not(target_os = "linux"))]
    {
        builder.build(window)
    }
}

fn offline_html(url: &str) -> String {
    if port_busy(url) {
        return notice_page(tr("busy_title"), tr("busy_text"), host_of(url), &[("retry", tr("retry"), true)]);
    }
    let start = if cfg!(unix) { vec![("start", tr("start"), true), ("retry", tr("retry"), false)] } else { vec![("retry", tr("retry"), true)] };
    notice_page(tr("offline_title"), tr("offline_text"), url, &start)
}

fn update_html() -> String {
    notice_page(tr("update_title"), tr("update_text"), "/opt/opendhcp · /usr/local/bin/opendhcp-panel", &[("update", tr("update"), true), ("later", tr("later"), false)])
}

fn missing_html(ini: &Path) -> String {
    notice_page(
        tr("missing_title"),
        tr("missing_text"),
        &ini.display().to_string(),
        &[("create", tr("create"), true), ("open_config", tr("open_config"), false)],
    )
}

fn failed_html(error: &str, config: &Path) -> String {
    let text = format!("{} ({error})", tr("failed_text"));
    notice_page(tr("failed_title"), &text, &config.display().to_string(), &[("open_config", tr("open_config"), true)])
}

/// Espera o serviço do painel (até ~20 s) sem travar a janela.
fn wait_reachable(url: String, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let result = wait_service(&url, 40);
        let _ = proxy.send_event(UserEvent::Reachable(url, result));
    });
}

fn open_path(p: &Path) {
    let opener = if cfg!(windows) { "explorer" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(p).spawn();
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "opendhcp_panel=info,open_dhcp=info".into()))
        .init();

    let target = resolve();
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Open DHCP")
        .with_inner_size(LogicalSize::new(1280.0, 840.0))
        .with_min_inner_size(LogicalSize::new(420.0, 520.0))
        .with_window_icon(window_icon())
        .build(&event_loop)
        .expect("não consegui abrir a janela");
    let guard = Arc::new(Mutex::new(Guard {
        notice: !matches!(target, Target::Url(_)),
        origin: match &target {
            Target::Url(u) | Target::Offline(u) | Target::Update(u) => origin_of(u),
            _ => None,
        },
    }));
    let webview = build_webview(&window, &target, proxy.clone(), guard.clone()).expect("não consegui criar o WebView");
    let mut offline_url = match &target {
        Target::Offline(url) | Target::Update(url) => Some(url.clone()),
        _ => None,
    };
    let failed_config = match &target {
        Target::Failed { config, .. } | Target::MissingIni { config, .. } | Target::Install { config } => Some(config.clone()),
        _ => None,
    };
    let mut missing_ini = match &target {
        Target::MissingIni { ini, .. } => Some(ini.clone()),
        _ => None,
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            Event::UserEvent(UserEvent::Ipc(action)) => match action.as_str() {
                "start" => {
                    if let Some(url) = &offline_url {
                        // pede a senha do sistema (polkit) para iniciar o serviço do painel; se o
                        // systemctl falhar, mostra o erro na hora em vez de esperar o painel
                        let (url, proxy) = (url.clone(), proxy.clone());
                        std::thread::spawn(move || {
                            let out = std::process::Command::new("pkexec")
                                .args(["systemctl", "start", "opendhcp-panel.service"])
                                .stderr(std::process::Stdio::piped())
                                .output();
                            let result = match out {
                                Ok(o) if o.status.success() => wait_service(&url, 40),
                                Ok(o) if matches!(o.status.code(), Some(126) | Some(127)) => Err("pkexec: autorização negada".to_string()),
                                Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
                                Err(e) => Err(format!("pkexec: {e}")),
                            };
                            let _ = proxy.send_event(UserEvent::Reachable(url, result));
                        });
                    }
                }
                "retry" => {
                    if let Some(url) = &offline_url {
                        wait_reachable(url.clone(), proxy.clone());
                    }
                }
                "create" => {
                    if let (Some(ini), Some(config)) = (missing_ini.clone(), failed_config.clone()) {
                        let proxy = proxy.clone();
                        std::thread::spawn(move || {
                            let result = create_ini_as_admin(&ini).and_then(|_| {
                                start_embedded(&config).map_err(|e| match e {
                                    StartError::MissingIni(p) => p.display().to_string(),
                                    StartError::Other(s) => s,
                                })
                            });
                            let _ = proxy.send_event(UserEvent::Embedded(result));
                        });
                    }
                }
                "install" | "update" => {
                    if let Some(payload) = server_payload() {
                        let proxy = proxy.clone();
                        std::thread::spawn(move || {
                            let result = install_server(&payload).and_then(|_| {
                                let url = system_url();
                                // espera o serviço do painel subir (até ~20 s)
                                wait_service(&url, 40).map(|_| url)
                            });
                            let _ = proxy.send_event(UserEvent::Installed(result));
                        });
                    }
                }
                "later" => {
                    if let Some(url) = &offline_url {
                        wait_reachable(url.clone(), proxy.clone());
                    }
                }
                "skip" => {
                    if let Some(m) = skip_marker() {
                        let _ = std::fs::write(m, b"");
                    }
                    if let Some(config) = failed_config.clone() {
                        let proxy = proxy.clone();
                        std::thread::spawn(move || {
                            let result = start_embedded(&config).map_err(|e| match e {
                                StartError::MissingIni(p) => p.display().to_string(),
                                StartError::Other(s) => s,
                            });
                            let _ = proxy.send_event(UserEvent::Embedded(result));
                        });
                    }
                }
                "open_config" => {
                    if let Some(c) = &failed_config {
                        open_path(c);
                    }
                }
                _ => {}
            },
            Event::UserEvent(UserEvent::Reachable(url, Ok(()))) => {
                *guard.lock().unwrap() = Guard { notice: false, origin: origin_of(&url) };
                offline_url = None;
                let _ = webview.load_url(&url);
            }
            Event::UserEvent(UserEvent::Reachable(_, Err(e))) => {
                let msg = serde_quote(&e);
                // o botão volta ao texto original (estava "Iniciando…")
                let label = serde_quote(tr("start"));
                let _ = webview.evaluate_script(&format!("document.querySelectorAll('button.primary').forEach(b => {{ if (b.getAttribute('onclick') === \"go('start', this)\") b.textContent = {label}; }});"));
                let _ = webview.evaluate_script(&format!(
                    "document.getElementById('msg').textContent = {msg}; document.querySelectorAll('button').forEach(b => b.disabled = false);"
                ));
            }
            Event::UserEvent(UserEvent::Embedded(Ok(url))) => {
                *guard.lock().unwrap() = Guard { notice: false, origin: origin_of(&url) };
                missing_ini = None;
                let _ = webview.load_url(&url);
            }
            Event::UserEvent(UserEvent::Embedded(Err(e))) => {
                let msg = serde_quote(&format!("{} {e}", tr("create_failed")));
                let _ = webview.evaluate_script(&format!(
                    "document.getElementById('msg').textContent = {msg}; document.querySelectorAll('button').forEach(b => b.disabled = false);"
                ));
            }
            Event::UserEvent(UserEvent::Installed(Ok(url))) => {
                *guard.lock().unwrap() = Guard { notice: false, origin: origin_of(&url) };
                offline_url = None;
                let _ = webview.load_url(&url);
            }
            Event::UserEvent(UserEvent::Installed(Err(e))) => {
                let msg = serde_quote(&format!("{} {e}", tr("install_failed")));
                let label = serde_quote(if offline_url.is_some() { tr("update") } else { tr("install") });
                let _ = webview.evaluate_script(&format!(
                    "document.getElementById('msg').textContent = {msg}; document.querySelectorAll('button').forEach(b => {{ b.disabled = false; }}); document.querySelector('button.primary').textContent = {label};"
                ));
            }
            Event::UserEvent(UserEvent::Downloaded(path)) => {
                let msg = serde_quote(&format!("{} {}", tr("saved"), path.display()));
                let _ = webview.evaluate_script(&format!("alert({msg})"));
            }
            _ => {}
        }
        let _ = &window;
    });
}
