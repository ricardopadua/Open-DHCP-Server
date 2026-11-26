//! `opendhcp-panel`: servidor do painel web (veja a biblioteca `opendhcp_panel`).

use std::io::BufRead;
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
    if pass.chars().count() < 8 && pass != "admin" {
        bail!("use uma senha com pelo menos 8 caracteres");
    }
    println!("{}", opendhcp_panel::hash_password(pass)?);
    Ok(())
}

async fn serve(path: PathBuf) -> anyhow::Result<()> {
    opendhcp_panel::start(path).await?.run().await
}
