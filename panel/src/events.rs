//! Leitura das linhas de log do `opendhcpd` (`-l arquivo%Y%m%d.log`).
//!
//! Formato gravado pelo servidor (logThread): `[26-Nov-26 03:30:48] mensagem`. As mensagens de
//! DHCP que interessam ao painel (opendhcpd.cpp):
//!
//! - `Host <mac> (<nome>) allotted <ip> for <n> seconds`   (ACK de IP novo)
//! - `Host <mac> (<nome>) renewed <ip> for <n> seconds`    (ACK de renovação)
//! - `Host <mac> (<nome>) offered <ip>`                    (OFFER; só com LogLevel=All)
//! - `BOOTP Host <mac> (<nome>) allotted <ip>`
//! - `IP Address <ip> released by Host <mac> (<nome>)`
//! - `IP Address <ip> declined by Host <mac> (<nome>), locked`
//! - `DHCPREQUEST from Host <mac> (<nome>) without Discover, NAKed`
//! - `No Matching DHCP Range for DHCPDISCOVER for <mac> (<nome>) from interface <ip>`
//! - `Static DHCP Host <mac> (<nome>) has No IP, DHCPDISCOVER ignored`

use chrono::NaiveDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Allot,
    Renew,
    Offer,
    Release,
    Decline,
    Nak,
    NoRange,
    Blocked,
    ServerStop,
    Info,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Allot => "IP entregue",
            Kind::Renew => "Renovação",
            Kind::Offer => "Oferta",
            Kind::Release => "Liberado",
            Kind::Decline => "Conflito",
            Kind::Nak => "Recusado",
            Kind::NoRange => "Sem faixa",
            Kind::Blocked => "Bloqueado",
            Kind::ServerStop => "Servidor",
            Kind::Info => "Info",
        }
    }

    /// Classe de cor do selo (ok/warn/bad ou neutro).
    pub fn tone(self) -> &'static str {
        match self {
            Kind::Allot => "ok",
            Kind::Renew | Kind::Offer | Kind::Info => "",
            Kind::Release | Kind::Blocked | Kind::ServerStop => "warn",
            Kind::Decline | Kind::Nak | Kind::NoRange => "bad",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub seq: u64,
    pub ts: Option<NaiveDateTime>,
    pub kind: Kind,
    pub mac: Option<String>,
    pub host: Option<String>,
    pub ip: Option<String>,
    /// Segundos de lease (allotted/renewed).
    pub secs: Option<u32>,
    pub text: String,
}

fn parse_ts(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%d-%b-%y %H:%M:%S").ok()
}

/// `<mac> (<nome>)<resto>` → (mac, nome, resto). O nome pode ter espaços e parênteses; o fim
/// dele é o último `)` antes de `suffix_hint` (ou o último `)` da linha).
fn mac_host<'a>(s: &'a str, suffix_hint: &str) -> Option<(String, String, &'a str)> {
    let (mac, rest) = s.split_once(" (")?;
    let close = if suffix_hint.is_empty() { rest.rfind(')')? } else { rest.rfind(&format!("){suffix_hint}"))? };
    let host = &rest[..close];
    let tail = &rest[close + 1..];
    let mac = mac.trim();
    if mac.is_empty() || !mac.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
        return None;
    }
    Some((mac.to_ascii_lowercase(), host.to_string(), tail))
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

pub fn parse_line(line: &str, seq: u64) -> Option<Event> {
    let line = line.trim_end();
    if line.is_empty() {
        return None;
    }
    let (ts, msg) = match line.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((t, m)) => (parse_ts(t), m),
        None => (None, line),
    };
    let mut ev = Event { seq, ts, kind: Kind::Info, mac: None, host: None, ip: None, secs: None, text: msg.to_string() };

    let host_verb = |rest: &str, ev: &mut Event, verb: &str, kind: Kind| -> bool {
        if let Some((mac, host, tail)) = mac_host(rest, &format!(" {verb} ")) {
            let tail = tail.trim_start().strip_prefix(verb).unwrap_or(tail).trim();
            ev.kind = kind;
            ev.mac = Some(mac);
            ev.host = Some(host);
            ev.ip = Some(first_word(tail).to_string());
            ev.secs = tail.split(" for ").nth(1).and_then(|s| first_word(s).parse().ok());
            true
        } else {
            false
        }
    };

    if let Some(rest) = msg.strip_prefix("BOOTP Host ") {
        host_verb(rest, &mut ev, "allotted", Kind::Allot);
    } else if let Some(rest) = msg.strip_prefix("Host ") {
        let _ = host_verb(rest, &mut ev, "allotted", Kind::Allot)
            || host_verb(rest, &mut ev, "renewed", Kind::Renew)
            || host_verb(rest, &mut ev, "offered", Kind::Offer);
    } else if let Some(rest) = msg.strip_prefix("IP Address ") {
        let ip = first_word(rest).to_string();
        for (verb, kind) in [("released", Kind::Release), ("declined", Kind::Decline)] {
            if let Some(by) = rest.split_once(&format!(" {verb} by Host ")).map(|x| x.1) {
                if let Some((mac, host, _)) = mac_host(by, "") {
                    ev.kind = kind;
                    ev.mac = Some(mac);
                    ev.host = Some(host);
                    ev.ip = Some(ip.clone());
                }
            }
        }
    } else if let Some(rest) = msg.strip_prefix("DHCPREQUEST from Host ") {
        if let Some((mac, host, _)) = mac_host(rest, " without Discover") {
            ev.kind = Kind::Nak;
            ev.mac = Some(mac);
            ev.host = Some(host);
        }
    } else if let Some(rest) = msg.strip_prefix("No Matching DHCP Range for DHCPDISCOVER for ") {
        if let Some((mac, host, tail)) = mac_host(rest, " from interface ") {
            ev.kind = Kind::NoRange;
            ev.mac = Some(mac);
            ev.host = Some(host);
            ev.ip = tail.trim().strip_prefix("from interface ").map(|s| first_word(s).to_string());
        }
    } else if let Some(rest) = msg.strip_prefix("Static DHCP Host ") {
        if let Some((mac, host, _)) = mac_host(rest, " has No IP") {
            ev.kind = Kind::Blocked;
            ev.mac = Some(mac);
            ev.host = Some(host);
        }
    } else if msg.starts_with("Open DHCP Server Stopped") {
        ev.kind = Kind::ServerStop;
    }
    if ev.host.as_deref() == Some("") {
        ev.host = None;
    }
    Some(ev)
}

/// Texto em português para o feed.
pub fn describe(ev: &Event) -> String {
    let who = match (&ev.host, &ev.mac) {
        (Some(h), Some(m)) => format!("{h} ({m})"),
        (None, Some(m)) => m.clone(),
        _ => String::new(),
    };
    let ip = ev.ip.as_deref().unwrap_or("?");
    let dur = ev.secs.map(fmt_secs).map(|d| format!(" por {d}")).unwrap_or_default();
    match ev.kind {
        Kind::Allot => format!("{who} recebeu {ip}{dur}"),
        Kind::Renew => format!("{who} renovou {ip}{dur}"),
        Kind::Offer => format!("{ip} oferecido a {who}"),
        Kind::Release => format!("{who} devolveu {ip}"),
        Kind::Decline => format!("{who} recusou {ip}: o IP já está em uso na rede (bloqueado pelo servidor)"),
        Kind::Nak => format!("Pedido de {who} recusado (renovação sem DISCOVER)"),
        Kind::NoRange => format!("{who} pediu IP pela interface {ip}, mas nenhuma faixa atende essa rede"),
        Kind::Blocked => format!("{who} está bloqueado (reserva sem IP); pedido ignorado"),
        Kind::ServerStop => "Servidor DHCP parado".to_string(),
        Kind::Info => ev.text.clone(),
    }
}

pub fn fmt_secs(s: u32) -> String {
    match s {
        0 => "tempo indeterminado".into(),
        s if s % 86400 == 0 => format!("{} dia(s)", s / 86400),
        s if s % 3600 == 0 => format!("{}h", s / 3600),
        s if s >= 3600 => format!("{}h{:02}", s / 3600, (s % 3600) / 60),
        s if s % 60 == 0 => format!("{} min", s / 60),
        s => format!("{s} s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Event {
        parse_line(s, 1).unwrap()
    }

    #[test]
    fn mensagens_do_servidor() {
        let e = p("[26-Nov-26 03:30:48] Host 3c:52:82:1a:9f:01 (notebook-ana) allotted 192.168.0.101 for 36000 seconds");
        assert_eq!(e.kind, Kind::Allot);
        assert_eq!(e.mac.as_deref(), Some("3c:52:82:1a:9f:01"));
        assert_eq!(e.host.as_deref(), Some("notebook-ana"));
        assert_eq!(e.ip.as_deref(), Some("192.168.0.101"));
        assert_eq!(e.secs, Some(36000));
        assert_eq!(e.ts.unwrap().to_string(), "2026-11-26 03:30:48");
        assert_eq!(describe(&e), "notebook-ana (3c:52:82:1a:9f:01) recebeu 192.168.0.101 por 10h");

        assert_eq!(p("[26-Nov-26 03:30:48] Host aa:bb:cc:dd:ee:ff (x) renewed 10.0.0.5 for 3600 seconds").kind, Kind::Renew);
        let e = p("[26-Nov-26 03:30:48] Host aa:bb:cc:dd:ee:ff () offered 10.0.0.5");
        assert_eq!((e.kind, e.host.clone(), e.ip.as_deref()), (Kind::Offer, None, Some("10.0.0.5")));
        let e = p("[26-Nov-26 03:30:48] IP Address 10.0.0.7 released by Host aa:bb:cc:dd:ee:01 (pc (sala))");
        assert_eq!((e.kind, e.host.as_deref(), e.ip.as_deref()), (Kind::Release, Some("pc (sala)"), Some("10.0.0.7")));
        let e = p("[26-Nov-26 03:30:48] IP Address 10.0.0.8 declined by Host aa:bb:cc:dd:ee:02 (tv), locked");
        assert_eq!((e.kind, e.ip.as_deref()), (Kind::Decline, Some("10.0.0.8")));
        let e = p("[26-Nov-26 03:30:48] DHCPREQUEST from Host aa:bb:cc:dd:ee:03 (cel) without Discover, NAKed");
        assert_eq!((e.kind, e.mac.as_deref()), (Kind::Nak, Some("aa:bb:cc:dd:ee:03")));
        let e = p("[26-Nov-26 03:30:48] No Matching DHCP Range for DHCPDISCOVER for aa:bb:cc:dd:ee:04 (vm) from interface 10.9.0.1");
        assert_eq!((e.kind, e.ip.as_deref()), (Kind::NoRange, Some("10.9.0.1")));
        assert_eq!(p("[26-Nov-26 03:30:48] Static DHCP Host aa:bb:cc:dd:ee:05 (x) has No IP, DHCPDISCOVER ignored").kind, Kind::Blocked);
        assert_eq!(p("[26-Nov-26 03:30:48] Open DHCP Server Stopped !").kind, Kind::ServerStop);
        assert_eq!(p("[26-Nov-26 03:30:48] BOOTP Host aa:bb:cc:dd:ee:06 (pxe) allotted 10.0.0.9").kind, Kind::Allot);
    }

    #[test]
    fn linhas_desconhecidas_viram_info() {
        let e = p("Open DHCP Server Version 1.64 Linux Build 1041");
        assert_eq!((e.kind, e.ts), (Kind::Info, None));
        let e = p("[26-Nov-26 03:30:48] Host lixo sem formato");
        assert_eq!(e.kind, Kind::Info);
        assert!(parse_line("   ", 1).is_none());
    }
}
