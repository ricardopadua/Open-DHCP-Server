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


use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{bail, Context};

pub use auth::hash_password;

/// Painel pronto para atender: configuração lida, histórico e monitor iniciados, porta aberta.
pub struct Server {
    listener: tokio::net::TcpListener,
    app: axum::Router,
}

impl Server {
    /// Endereço em que o painel está ouvindo (útil com `listen = "127.0.0.1:0"`).
    pub fn local_addr(&self) -> SocketAddr {
        self.listener.local_addr().expect("endereço do listener")
    }

    pub async fn run(self) -> anyhow::Result<()> {
        // Sem "graceful shutdown": as conexões ao vivo (SSE) ficam abertas e ele esperaria para sempre.
        axum::serve(self.listener, self.app.into_make_service_with_connect_info::<SocketAddr>()).await?;
        Ok(())
    }
}

/// Lê o `panel.toml`, inicia o monitor e abre a porta (precisa de um runtime Tokio).
pub async fn start(path: PathBuf) -> anyhow::Result<Server> {
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
    tracing::info!("painel em http://{}", listener.local_addr()?);
    Ok(Server { listener, app })
}
