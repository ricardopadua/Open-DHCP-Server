//! Painel web de administração para o Open DHCP Server.
//!
//! Não altera o servidor DHCP: edita o `opendhcp.ini`, lê o arquivo de estado dos leases e
//! controla o serviço por comandos configuráveis (por padrão `systemctl`).

mod auth;
mod config;
mod devices;
mod diff;
mod dhcp;
mod events;
mod fsutil;
mod history;
mod i18n;
mod i18n_table;
mod ini;
mod leases;
mod monitor;
mod notes;
mod service;
mod views;
mod web;

use std::io::BufRead;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{bail, Context};

const USAGE: &str = "\
Uso:
  opendhcp-panel serve [-c panel.toml]   inicia o painel (padrão: /etc/opendhcp-panel/panel.toml)
  opendhcp-panel hash-password           lê uma senha da entrada padrão e imprime o hash Argon2
";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "opendhcp_panel=info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hash-password") => hash_password(),
        Some("serve") => serve(config_path(&args[1..])?).await,
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            Ok(())
        }
        _ => {
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    }
}

fn config_path(args: &[String]) -> anyhow::Result<PathBuf> {
    match args {
        [] => Ok("/etc/opendhcp-panel/panel.toml".into()),
        [flag, path] if flag == "-c" || flag == "--config" => Ok(path.into()),
        _ => bail!("argumentos inválidos\n{USAGE}"),
    }
}

fn hash_password() -> anyhow::Result<()> {
    eprintln!("Digite a senha e tecle Enter:");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).context("não consegui ler a senha")?;
    let pass = line.trim_end_matches(['\r', '\n']);
    if pass.chars().count() < 8 && pass != web::DEFAULT_PASSWORD {
        bail!("use uma senha com pelo menos 8 caracteres");
    }
    println!("{}", auth::hash_password(pass)?);
    Ok(())
}

async fn serve(path: PathBuf) -> anyhow::Result<()> {
    let cfg = config::Config::load(&path)?;
    if !cfg.ini_file.exists() {
        bail!("ini_file não existe: {}", cfg.ini_file.display());
    }
    let listen = cfg.listen;
    if !listen.ip().is_loopback() && !cfg.secure_cookie {
        tracing::warn!(
            "o painel está aberto na rede sem HTTPS (secure_cookie = false); prefira 127.0.0.1 atrás de um proxy com TLS"
        );
    }
    let history_path = cfg.data_dir.join("history.sqlite");
    let history = match history::History::open(&history_path) {
        Ok(h) => Some(std::sync::Arc::new(h)),
        Err(e) => {
            tracing::error!(file = %history_path.display(), error = %e, "não consegui abrir o histórico; seguindo só com a memória");
            None
        }
    };
    let monitor = monitor::Monitor::new(cfg.clone(), history);
    tokio::spawn(monitor.clone().run());
    let app = web::router(cfg, monitor, path);
    let listener = tokio::net::TcpListener::bind(listen).await.with_context(|| format!("não consegui abrir {listen}"))?;
    tracing::info!("painel em http://{listen}");
    // Sem "graceful shutdown": as conexões ao vivo (SSE) ficam abertas e ele esperaria para sempre.
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
