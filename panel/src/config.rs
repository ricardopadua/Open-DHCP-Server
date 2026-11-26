use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Endereço do painel. Padrão só local; para acesso pela rede, use um proxy com HTTPS.
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    /// `opendhcp.ini` usado pelo servidor (`-i`).
    pub ini_file: PathBuf,
    /// Arquivo de estado dos leases (`-s`; padrão do servidor: /tmp/opendhcp.state).
    #[serde(default = "default_state")]
    pub state_file: PathBuf,
    /// Log do servidor (`-l`), com os mesmos `%Y%m%d` do opendhcpd. Opcional: sem ele a tela
    /// Atividade fica vazia, mas leases e serviço continuam ao vivo.
    #[serde(default)]
    pub log_file: Option<String>,
    /// Cópias do .ini antes de cada alteração.
    #[serde(default = "default_backup_dir")]
    pub backup_dir: PathBuf,
    #[serde(default = "default_backups_keep")]
    pub backups_keep: usize,
    /// Marque `true` quando o painel estiver atrás de HTTPS (cookie só via TLS).
    #[serde(default)]
    pub secure_cookie: bool,
    #[serde(default = "default_session_hours")]
    pub session_hours: u64,
    pub auth: Auth,
    #[serde(default)]
    pub service: Service,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Auth {
    pub username: String,
    /// Hash Argon2 gerado com `opendhcp-panel hash-password`.
    pub password_hash: String,
}

/// Comandos de controle do serviço (argv, sem shell). `status` deve sair com 0 quando ativo.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub status: Vec<String>,
    pub start: Vec<String>,
    pub stop: Vec<String>,
    pub restart: Vec<String>,
}

impl Default for Service {
    fn default() -> Self {
        let sc = |a: &str| vec!["systemctl".to_string(), a.to_string(), "opendhcp".to_string()];
        Self { status: sc("is-active"), start: sc("start"), stop: sc("stop"), restart: sc("restart") }
    }
}

fn default_listen() -> SocketAddr {
    "127.0.0.1:8067".parse().unwrap()
}
fn default_state() -> PathBuf {
    "/tmp/opendhcp.state".into()
}
fn default_backup_dir() -> PathBuf {
    "/var/lib/opendhcp-panel/backups".into()
}
fn default_backups_keep() -> usize {
    30
}
fn default_session_hours() -> u64 {
    8
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("não consegui ler {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("configuração inválida em {}", path.display()))?;
        if cfg.auth.username.trim().is_empty() {
            bail!("auth.username está vazio");
        }
        if !cfg.auth.password_hash.starts_with("$argon2") {
            bail!("auth.password_hash precisa ser um hash Argon2 (use `opendhcp-panel hash-password`)");
        }
        for (name, cmd) in [
            ("status", &cfg.service.status),
            ("start", &cfg.service.start),
            ("stop", &cfg.service.stop),
            ("restart", &cfg.service.restart),
        ] {
            if cmd.is_empty() {
                bail!("service.{name} está vazio");
            }
        }
        Ok(cfg)
    }
}

/// Troca `username` e `password_hash` da seção `[auth]` no texto do panel.toml, mantendo o resto
/// (comentários, ordem) como está. O resultado é conferido relendo o TOML.
pub fn set_auth(text: &str, username: &str, hash: &str) -> anyhow::Result<String> {
    let mut out = String::with_capacity(text.len() + 64);
    let mut section = String::new();
    let (mut user_done, mut hash_done) = (false, false);
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with('[') {
            section = t.trim_matches(|c| c == '[' || c == ']').trim().to_string();
        }
        let key = t.split('=').next().unwrap_or("").trim();
        let nl = if line.ends_with('\n') { "\n" } else { "" };
        if section == "auth" && t.contains('=') && !t.starts_with('#') && key == "username" {
            out.push_str(&format!("username = \"{username}\"{nl}"));
            user_done = true;
        } else if section == "auth" && t.contains('=') && !t.starts_with('#') && key == "password_hash" {
            out.push_str(&format!("password_hash = \"{hash}\"{nl}"));
            hash_done = true;
        } else {
            out.push_str(line);
        }
    }
    if !(user_done && hash_done) {
        bail!("não encontrei username e password_hash na seção [auth]");
    }
    let cfg: Config = toml::from_str(&out).context("o panel.toml ficaria inválido")?;
    if cfg.auth.username != username || cfg.auth.password_hash != hash {
        bail!("o panel.toml não ficou com os valores esperados");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_auth_keeps_rest() {
        let text = "# topo\nini_file = \"/x.ini\"\n\n[auth]\n# comentário\nusername = \"admin\"\npassword_hash = \"$argon2id$a\"\n\n[service]\nstatus = [\"true\"]\nstart = [\"true\"]\nstop = [\"true\"]\nrestart = [\"true\"]\n";
        let out = set_auth(text, "ricardo", "$argon2id$b").unwrap();
        assert!(out.contains("# comentário\nusername = \"ricardo\"\npassword_hash = \"$argon2id$b\"\n"));
        assert!(out.starts_with("# topo\nini_file"));
        assert!(out.contains("[service]\nstatus"));
        assert!(set_auth("ini_file = \"/x\"\n", "a", "b").is_err());
    }
}
