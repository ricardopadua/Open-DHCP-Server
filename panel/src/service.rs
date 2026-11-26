use std::time::Duration;

use tokio::process::Command;

use crate::i18n::t;
use crate::tf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Start,
    Stop,
    Restart,
}

impl Action {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "start" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            "restart" => Some(Self::Restart),
            _ => None,
        }
    }
}

const TIMEOUT: Duration = Duration::from_secs(30);

async fn run(argv: &[String]) -> Result<std::process::Output, String> {
    let (prog, args) = argv.split_first().ok_or_else(|| t("comando vazio").to_string())?;
    let fut = Command::new(prog).args(args).kill_on_drop(true).output();
    match tokio::time::timeout(TIMEOUT, fut).await {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => Err(tf!("não consegui executar {}: {}", prog, e)),
        Err(_) => Err(tf!("{} não respondeu em {}s", prog, TIMEOUT.as_secs())),
    }
}

pub async fn status(cfg: &crate::config::Service) -> Status {
    match run(&cfg.status).await {
        Ok(out) if out.status.success() => Status::Running,
        Ok(out) if out.status.code().is_some() => Status::Stopped,
        _ => Status::Unknown,
    }
}

pub async fn act(cfg: &crate::config::Service, action: Action) -> Result<(), String> {
    let argv = match action {
        Action::Start => &cfg.start,
        Action::Stop => &cfg.stop,
        Action::Restart => &cfg.restart,
    };
    let out = run(argv).await?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        Err(if err.is_empty() { tf!("comando saiu com {}", out.status) } else { err.chars().take(300).collect() })
    }
}
