//! Monitor em tempo real: vigia o arquivo de estado (leases), o `.ini`, o log do servidor e o
//! estado do serviço. A cada mudança publica uma nova versão num `watch`, que as conexões SSE
//! (`/live`) usam para mandar os pedaços de página atualizados ao navegador.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::watch;

use crate::config::Config;
use crate::events::{self, Event};
use crate::ini::IniDoc;
use crate::leases::{self, Lease};
use crate::service::{self, Status};

/// Quantos eventos do log ficam em memória.
pub const MAX_EVENTS: usize = 2000;
/// Quanto do fim do log é lido ao iniciar o painel.
const INITIAL_TAIL: u64 = 512 * 1024;
/// Por quanto tempo um lease novo/renovado fica destacado.
pub const FRESH_FOR: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Default)]
pub struct LogInfo {
    pub configured: bool,
    pub path: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub version: u64,
    pub status: Status,
    pub leases: Vec<Lease>,
    pub lease_error: Option<String>,
    pub doc: IniDoc,
    pub ini_error: Option<String>,
    /// MAC -> (quando o lease mudou, se é um dispositivo novo e não uma renovação).
    pub fresh: HashMap<String, (Instant, bool)>,
    /// Mais antigo primeiro.
    pub events: VecDeque<Event>,
    pub log: LogInfo,
    pub updated: chrono::DateTime<chrono::Local>,
}

impl Snapshot {
    pub fn empty() -> Self {
        Self {
            version: 0,
            status: Status::Unknown,
            leases: Vec::new(),
            lease_error: None,
            doc: IniDoc::parse(""),
            ini_error: None,
            fresh: HashMap::new(),
            events: VecDeque::new(),
            log: LogInfo::default(),
            updated: chrono::Local::now(),
        }
    }

    pub fn is_fresh(&self, mac: &str) -> bool {
        self.fresh.get(mac).is_some_and(|(t, _)| t.elapsed() < FRESH_FOR)
    }

    /// `Some(true)` = dispositivo novo, `Some(false)` = renovou ou trocou de IP.
    pub fn fresh_kind(&self, mac: &str) -> Option<bool> {
        self.fresh.get(mac).filter(|(t, _)| t.elapsed() < FRESH_FOR).map(|(_, new)| *new)
    }
}

type Sig = Option<(SystemTime, u64)>;

struct Inner {
    state_sig: Sig,
    ini_sig: Sig,
    log_path: Option<PathBuf>,
    log_off: u64,
    log_partial: String,
    seq: u64,
    tick: u64,
    last_bump: Instant,
}

pub struct Monitor {
    cfg: Config,
    snap: RwLock<Arc<Snapshot>>,
    tx: watch::Sender<u64>,
    inner: tokio::sync::Mutex<Inner>,
}

fn sig(path: &std::path::Path) -> Sig {
    std::fs::metadata(path).ok().map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
}

impl Monitor {
    pub fn new(cfg: Config) -> Arc<Self> {
        let (tx, _) = watch::channel(0);
        Arc::new(Self {
            cfg,
            snap: RwLock::new(Arc::new(Snapshot::empty())),
            tx,
            inner: tokio::sync::Mutex::new(Inner {
                state_sig: None,
                ini_sig: None,
                log_path: None,
                log_off: 0,
                log_partial: String::new(),
                seq: 0,
                tick: 0,
                last_bump: Instant::now(),
            }),
        })
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snap.read().unwrap().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.tx.subscribe()
    }

    pub async fn run(self: Arc<Self>) {
        let mut every = tokio::time::interval(Duration::from_secs(1));
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            every.tick().await;
            self.tick(false).await;
        }
    }

    /// Força uma leitura imediata (depois de uma ação no serviço ou de gravar o .ini).
    pub async fn poke(&self) {
        self.tick(true).await;
    }

    async fn tick(&self, force: bool) {
        let mut inner = self.inner.lock().await;
        inner.tick += 1;
        let old = self.snapshot();
        let mut next: Snapshot = (*old).clone();
        let mut changed = old.version == 0;

        // leases
        let s = sig(&self.cfg.state_file);
        if force || s != inner.state_sig || old.version == 0 {
            inner.state_sig = s;
            match leases::read(&self.cfg.state_file) {
                Ok(list) => {
                    let before: HashMap<&str, &Lease> = old.leases.iter().map(|l| (l.mac.as_str(), l)).collect();
                    if old.version != 0 {
                        for l in &list {
                            let change = match before.get(l.mac.as_str()) {
                                Some(b) if b.ip != l.ip || b.expiry != l.expiry => Some(false),
                                Some(_) => None,
                                None => Some(true),
                            };
                            if let Some(new) = change {
                                next.fresh.insert(l.mac.clone(), (Instant::now(), new));
                            }
                        }
                    }
                    next.leases = list;
                    next.lease_error = None;
                }
                Err(e) => next.lease_error = Some(format!("não consegui ler {}: {e}", self.cfg.state_file.display())),
            }
            changed |= next.leases != old.leases || next.lease_error != old.lease_error;
        }

        // .ini
        let s = sig(&self.cfg.ini_file);
        if force || s != inner.ini_sig || old.version == 0 {
            inner.ini_sig = s;
            match std::fs::read(&self.cfg.ini_file) {
                Ok(b) => {
                    let text = String::from_utf8(b)
                        .unwrap_or_else(|e| e.into_bytes().iter().map(|c| *c as char).collect());
                    next.doc = IniDoc::parse(&text);
                    next.ini_error = None;
                }
                Err(e) => next.ini_error = Some(format!("não consegui ler {}: {e}", self.cfg.ini_file.display())),
            }
            changed = true;
        }

        // log
        if let Some(pattern) = &self.cfg.log_file {
            let new_events = self.read_log(&mut inner, pattern, &mut next.log);
            if !new_events.is_empty() {
                next.events.extend(new_events);
                while next.events.len() > MAX_EVENTS {
                    next.events.pop_front();
                }
                changed = true;
            }
            changed |= next.log.error != old.log.error || next.log.path != old.log.path;
        }

        // serviço (a cada 3 s ou quando pedido)
        if force || inner.tick % 3 == 1 || old.version == 0 {
            next.status = service::status(&self.cfg.service).await;
            changed |= next.status != old.status;
        }

        // destaques que venceram e relógio relativo ("em 3 min") a cada 30 s
        let before = next.fresh.len();
        next.fresh.retain(|_, (t, _)| t.elapsed() < FRESH_FOR);
        changed |= next.fresh.len() != before;
        if inner.last_bump.elapsed() >= Duration::from_secs(30) {
            changed = true;
        }

        if changed || force {
            inner.last_bump = Instant::now();
            next.version = old.version + 1;
            next.updated = chrono::Local::now();
            let v = next.version;
            *self.snap.write().unwrap() = Arc::new(next);
            let _ = self.tx.send(v);
        }
    }

    fn read_log(&self, inner: &mut Inner, pattern: &str, info: &mut LogInfo) -> Vec<Event> {
        info.configured = true;
        let path = PathBuf::from(chrono::Local::now().format(pattern).to_string());
        let mut out = Vec::new();

        // Virou o dia: termina o arquivo anterior e passa para o novo desde o começo.
        if inner.log_path.as_ref() != Some(&path) {
            if let Some(prev) = inner.log_path.clone() {
                out.extend(self.read_from(inner, &prev));
                inner.log_off = 0;
                inner.log_partial.clear();
            } else {
                // Primeira leitura: o fim do arquivo de ontem (se o nome muda por dia; dá o
                // histórico das últimas horas logo depois da meia-noite) e o fim do de hoje.
                let yesterday = PathBuf::from((chrono::Local::now() - chrono::Duration::days(1)).format(pattern).to_string());
                if yesterday != path {
                    if let Ok(meta) = std::fs::metadata(&yesterday) {
                        inner.log_off = meta.len().saturating_sub(INITIAL_TAIL);
                        inner.log_partial = if inner.log_off > 0 { "\u{0}".into() } else { String::new() };
                        out.extend(self.read_from(inner, &yesterday));
                        inner.log_partial.clear();
                    }
                }
                inner.log_off = 0;
                if let Ok(meta) = std::fs::metadata(&path) {
                    inner.log_off = meta.len().saturating_sub(INITIAL_TAIL);
                    if inner.log_off > 0 {
                        inner.log_partial = "\u{0}".into(); // descarta até o primeiro \n
                    }
                }
            }
            inner.log_path = Some(path.clone());
        }
        info.path = Some(path.clone());
        match std::fs::metadata(&path) {
            Ok(_) => {
                info.error = None;
                out.extend(self.read_from(inner, &path));
            }
            Err(_) => {
                info.error = Some(format!(
                    "Arquivo de log {} não encontrado. Inicie o opendhcpd com -l e LogLevel=Normal ou All.",
                    path.display()
                ))
            }
        }
        out
    }

    fn read_from(&self, inner: &mut Inner, path: &std::path::Path) -> Vec<Event> {
        let Ok(mut f) = std::fs::File::open(path) else { return Vec::new() };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < inner.log_off {
            // arquivo truncado/recriado
            inner.log_off = 0;
            inner.log_partial.clear();
        }
        if len == inner.log_off || f.seek(SeekFrom::Start(inner.log_off)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        let _ = (&mut f).take(4 * 1024 * 1024).read_to_end(&mut buf);
        inner.log_off += buf.len() as u64;
        let chunk = String::from_utf8_lossy(&buf);
        let mut text = std::mem::take(&mut inner.log_partial);
        text.push_str(&chunk);
        let skip_first = text.starts_with('\u{0}');
        let mut lines: Vec<&str> = text.split('\n').collect();
        let tail = lines.pop().unwrap_or("").to_string();
        if skip_first && !lines.is_empty() {
            lines.remove(0);
        }
        let mut out = Vec::new();
        for line in lines {
            inner.seq += 1;
            if let Some(ev) = events::parse_line(line, inner.seq) {
                out.push(ev);
            }
        }
        inner.log_partial = tail;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn cfg(dir: &std::path::Path) -> Config {
        let toml = format!(
            r#"ini_file = "{d}/o.ini"
state_file = "{d}/o.state"
log_file = "{d}/log%Y.log"
[auth]
username = "a"
password_hash = "$argon2id$x"
[service]
status = ["true"]
start = ["true"]
stop = ["true"]
restart = ["true"]
"#,
            d = dir.display()
        );
        toml::from_str(&toml).unwrap()
    }

    #[tokio::test]
    async fn acompanha_log_e_leases() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("o.ini"), "[RANGE_SET]\nDHCPRange=10.0.0.1-10.0.0.9\n").unwrap();
        let log = dir.path().join(chrono::Local::now().format("log%Y.log").to_string());
        std::fs::write(&log, "[26-Nov-26 03:30:48] Host aa:bb:cc:dd:ee:01 (a) allotted 10.0.0.2 for 60 seconds\n").unwrap();
        let m = Monitor::new(cfg(dir.path()));
        m.tick(false).await;
        let s = m.snapshot();
        assert_eq!(s.status, Status::Running);
        assert_eq!(s.events.len(), 1);
        assert!(s.leases.is_empty());

        // linha incompleta só entra quando terminar
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        write!(f, "[26-Nov-26 03:31:00] IP Address 10.0.0.2 released by Host aa:bb:cc:dd:ee:01 (a)").unwrap();
        f.flush().unwrap();
        m.tick(false).await;
        assert_eq!(m.snapshot().events.len(), 1);
        writeln!(f).unwrap();
        m.tick(false).await;
        let s = m.snapshot();
        assert_eq!(s.events.len(), 2);
        assert_eq!(s.events[1].kind, events::Kind::Release);

        // lease novo fica destacado
        let rec = leases::encode(1, "aa:bb:cc:dd:ee:02", "10.0.0.3".parse().unwrap(), leases::INFINITE, "b");
        std::fs::write(dir.path().join("o.state"), rec).unwrap();
        let mut rx = m.subscribe();
        m.tick(false).await;
        assert!(rx.has_changed().unwrap());
        let s = m.snapshot();
        assert_eq!(s.leases.len(), 1);
        assert!(s.is_fresh("aa:bb:cc:dd:ee:02"));
        rx.borrow_and_update();

        // truncado: volta ao começo
        std::fs::write(&log, "[26-Nov-26 04:00:00] Open DHCP Server Stopped !\n").unwrap();
        m.tick(false).await;
        assert_eq!(m.snapshot().events.back().unwrap().kind, events::Kind::ServerStop);
    }
}
