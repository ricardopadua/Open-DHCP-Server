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
use std::sync::mpsc;
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
    /// O serviço respondeu (ou desistimos de esperar).
    Reachable(String, bool),
    /// Arquivo baixado (ex.: exportar CSV).
    Downloaded(PathBuf),
    /// Resultado de criar o .ini e subir o painel embutido de novo.
    Embedded(Result<String, String>),
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
<script>function go(a, b) {{ if (a === 'start' || a === 'create') {{ b.disabled = true; b.textContent = a === 'start' ? {} : {}; }} window.ipc.postMessage(a); }}</script>
</body></html>"#,
        esc(title),
        esc(text),
        esc(detail),
        serde_quote(tr("starting")),
        serde_quote(tr("creating")),
    )
}

fn serde_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
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
        std::fs::write(&path, default_config(&dir)?)?;
    }
    Ok(path)
}

fn toml_str(p: &Path) -> String {
    format!("'{}'", p.display())
}

fn default_config(dir: &Path) -> anyhow::Result<String> {
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
            "ini_file = '/opt/opendhcp/opendhcp.ini'\nstate_file = '/tmp/opendhcp.state'\nlog_file = '/opt/opendhcp/log/opendhcp%Y%m%d.log'".to_string(),
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
         [auth]\nusername = \"admin\"\n# senha inicial: admin (troque em Minha conta)\npassword_hash = \"{hash}\"\n\n\
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
                    let _ = tx.send(Ok(format!("http://{}/", server.local_addr())));
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

/// Cria o opendhcp.ini numa pasta do sistema, pedindo a senha de administrador (polkit).
fn create_ini_as_admin(ini: &Path) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("opendhcp-{}.ini", std::process::id()));
    std::fs::write(&tmp, opendhcp_panel::DEFAULT_INI).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("pkexec")
        .args(["install", "-D", "-m", "644"])
        .arg(&tmp)
        .arg(ini)
        .status();
    let _ = std::fs::remove_file(&tmp);
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("pkexec: {s}")),
        Err(e) => Err(e.to_string()),
    }
}

fn resolve() -> Target {
    if let Some(url) = arg("--url").or_else(|| std::env::var("OPENDHCP_URL").ok()) {
        return Target::Url(url);
    }
    let system = Path::new(SYSTEM_CONFIG);
    if arg("--config").is_none() && cfg!(unix) && system.exists() {
        if let Some(url) = url_from_config(system) {
            return if reachable(&url) { Target::Url(url) } else { Target::Offline(url) };
        }
    }
    let config = match arg("--config").map(PathBuf::from).map(Ok).unwrap_or_else(user_config) {
        Ok(c) => c,
        Err(e) => return Target::Failed { error: e.to_string(), config: PathBuf::new() },
    };
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
    dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir)
}

fn build_webview(window: &Window, target: &Target, proxy: EventLoopProxy<UserEvent>) -> wry::Result<WebView> {
    let ipc_proxy = proxy.clone();
    let done_proxy = proxy;
    let mut builder = WebViewBuilder::new()
        .with_ipc_handler(move |req| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(req.body().clone()));
        })
        // exportar CSV etc.: salva direto na pasta Downloads
        .with_download_started_handler(|_url, path| {
            let name = path.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "download".into());
            *path = downloads_dir().join(name);
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
        Target::MissingIni { ini, .. } => builder.with_html(missing_html(ini)),
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
    let start = if cfg!(unix) { vec![("start", tr("start"), true), ("retry", tr("retry"), false)] } else { vec![("retry", tr("retry"), true)] };
    notice_page(tr("offline_title"), tr("offline_text"), url, &start)
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

/// Espera o serviço responder (até ~20 s) sem travar a janela.
fn wait_reachable(url: String, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        for _ in 0..40 {
            if reachable(&url) {
                let _ = proxy.send_event(UserEvent::Reachable(url, true));
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let _ = proxy.send_event(UserEvent::Reachable(url, false));
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
    let webview = build_webview(&window, &target, proxy.clone()).expect("não consegui criar o WebView");
    let offline_url = match &target {
        Target::Offline(url) => Some(url.clone()),
        _ => None,
    };
    let failed_config = match &target {
        Target::Failed { config, .. } | Target::MissingIni { config, .. } => Some(config.clone()),
        _ => None,
    };
    let missing_ini = match &target {
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
                        // pede a senha do sistema (polkit) para iniciar o serviço do painel
                        let _ = std::process::Command::new("pkexec").args(["systemctl", "start", "opendhcp-panel"]).spawn();
                        wait_reachable(url.clone(), proxy.clone());
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
                "open_config" => {
                    if let Some(c) = &failed_config {
                        open_path(c);
                    }
                }
                _ => {}
            },
            Event::UserEvent(UserEvent::Reachable(url, true)) => {
                let _ = webview.load_url(&url);
            }
            Event::UserEvent(UserEvent::Reachable(_, false)) => {
                let msg = serde_quote(tr("still_off"));
                let _ = webview.evaluate_script(&format!(
                    "document.getElementById('msg').textContent = {msg}; document.querySelectorAll('button').forEach(b => b.disabled = false);"
                ));
            }
            Event::UserEvent(UserEvent::Embedded(Ok(url))) => {
                let _ = webview.load_url(&url);
            }
            Event::UserEvent(UserEvent::Embedded(Err(e))) => {
                let msg = serde_quote(&format!("{} {e}", tr("create_failed")));
                let _ = webview.evaluate_script(&format!(
                    "document.getElementById('msg').textContent = {msg}; document.querySelectorAll('button').forEach(b => b.disabled = false);"
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
