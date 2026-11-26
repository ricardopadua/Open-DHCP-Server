//! HTML do painel (maud escapa todo texto). As partes que mudam sozinhas têm `id` e são
//! renderizadas pelas mesmas funções na página inteira e no fluxo ao vivo (`/live`).

use std::net::Ipv4Addr;

use chrono::{Duration as CDuration, NaiveDateTime};
use maud::{html, Markup, DOCTYPE};

use crate::dhcp;
use crate::events::{self, Event, Kind};
use crate::ini::IniDoc;
use crate::leases::{self, Lease};
use crate::monitor::Snapshot;
use crate::notes::{Alert, Note, NoteAction};
use crate::service::Status;

// ---------- ícones (traço 24x24, estilo Tabler) ----------

pub fn icon(name: &str) -> Markup {
    let path = match name {
        "home" => "M5 12H3l9-9 9 9h-2M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-7M9 21v-6a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v6",
        "pulse" => "M3 12h4l3 8 4-16 3 8h4",
        "devices" => "M13 9a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1h-6a1 1 0 0 1-1-1zM18 8V5a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h9M16 9h2",
        "pin" => "M9 11a3 3 0 1 0 6 0 3 3 0 0 0-6 0M17.66 16.66 13.41 20.9a2 2 0 0 1-2.83 0l-4.24-4.24a8 8 0 1 1 11.32 0z",
        "ranges" => "M4 6h16M4 12h10M4 18h6M18 15v6M15 18h6",
        "settings" => "M10.33 4.32c.43-1.76 2.93-1.76 3.35 0a1.72 1.72 0 0 0 2.57 1.07c1.54-.94 3.31.83 2.37 2.37a1.72 1.72 0 0 0 1.06 2.57c1.76.43 1.76 2.93 0 3.35a1.72 1.72 0 0 0-1.07 2.57c.94 1.54-.83 3.31-2.37 2.37a1.72 1.72 0 0 0-2.57 1.06c-.43 1.76-2.93 1.76-3.35 0a1.72 1.72 0 0 0-2.57-1.07c-1.54.94-3.31-.83-2.37-2.37a1.72 1.72 0 0 0-1.06-2.57c-1.76-.43-1.76-2.93 0-3.35a1.72 1.72 0 0 0 1.07-2.57c-.94-1.54.83-3.31 2.37-2.37 1 .61 2.3.07 2.57-1.07zM9 12a3 3 0 1 0 6 0 3 3 0 0 0-6 0",
        "power" => "M7 6a7.75 7.75 0 1 0 10 0M12 4v8",
        "refresh" => "M20 11A8.1 8.1 0 0 0 4.5 9M4 5v4h4M4 13a8.1 8.1 0 0 0 15.5 2m.5 4v-4h-4",
        "play" => "M7 4v16l13-8z",
        "stop" => "M5 7a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2z",
        "logout" => "M14 8V6a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h7a2 2 0 0 0 2-2v-2M9 12h12l-3-3m0 6 3-3",
        "sun" => "M12 12m-4 0a4 4 0 1 0 8 0 4 4 0 1 0-8 0M3 12h1m8-9v1m8 8h1m-9 8v1M5.6 5.6l.7.7m12.1-.7-.7.7m0 11.4.7.7m-12.1-.7-.7.7",
        "moon" => "M12 3h.39a7.5 7.5 0 0 0 7.92 12.45A9 9 0 1 1 12 3z",
        "alert" => "M12 9v4M10.36 3.6 2.28 17.1a1.91 1.91 0 0 0 1.63 2.86h16.18a1.91 1.91 0 0 0 1.63-2.86L13.64 3.6a1.91 1.91 0 0 0-3.28 0zM12 16h.01",
        "check" => "M5 12l5 5L20 7",
        "plus" => "M12 5v14M5 12h14",
        "search" => "M10 10m-7 0a7 7 0 1 0 14 0 7 7 0 1 0-14 0M21 21l-6-6",
        "bolt" => "M13 3v7h6l-8 11v-7H5l8-11",
        "wifi" => "M12 18h.01M9.17 15.17a4 4 0 0 1 5.66 0M6.34 12.34a8 8 0 0 1 11.31 0M3.51 9.51a12 12 0 0 1 16.97 0",
        "lock" => "M5 13a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2zM11 16a1 1 0 1 0 2 0 1 1 0 0 0-2 0M8 11V7a4 4 0 1 1 8 0v4",
        "edit" => "M4 20h4L18.5 9.5a2.83 2.83 0 0 0-4-4L4 16v4M13.5 6.5l4 4",
        "bell" => "M10 5a2 2 0 1 1 4 0 7 7 0 0 1 4 6v3a4 4 0 0 0 2 3H4a4 4 0 0 0 2-3v-3a7 7 0 0 1 4-6M9 17v1a3 3 0 0 0 6 0v-1",
        "x" => "M18 6 6 18M6 6l12 12",
        "trash" => "M4 7h16M10 11v6M14 11v6M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2l1-12M9 7V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v3",
        _ => "M12 12m-8 0a8 8 0 1 0 16 0 8 8 0 1 0-16 0",
    };
    html! {
        svg.ico viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {
            path d=(path) {}
        }
    }
}

// ---------- utilidades ----------

pub fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn now_local() -> NaiveDateTime {
    chrono::Local::now().naive_local()
}

pub fn fmt_expiry(exp: i64, now: i64) -> (String, String) {
    if exp >= leases::INFINITE {
        return ("Sem expiração".into(), String::new());
    }
    let when = chrono::DateTime::from_timestamp(exp, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%d/%m %H:%M").to_string())
        .unwrap_or_default();
    let left = exp - now;
    let rel = if left <= 0 {
        "expirado".to_string()
    } else if left < 60 {
        "em menos de 1 min".to_string()
    } else if left < 3600 {
        format!("em {} min", left / 60)
    } else if left < 86400 {
        format!("em {}h{:02}", left / 3600, (left % 3600) / 60)
    } else if left < 2 * 86400 {
        "em 1 dia".to_string()
    } else {
        format!("em {} dias", left / 86400)
    };
    (when, rel)
}

fn ago(ts: Option<NaiveDateTime>, now: NaiveDateTime) -> String {
    let Some(ts) = ts else { return String::new() };
    let s = (now - ts).num_seconds();
    if s < 0 {
        ts.format("%d/%m %H:%M:%S").to_string()
    } else if s < 10 {
        "agora".into()
    } else if s < 60 {
        format!("há {s} s")
    } else if s < 3600 {
        format!("há {} min", s / 60)
    } else if s < 86400 {
        format!("há {}h", s / 3600)
    } else {
        ts.format("%d/%m %H:%M").to_string()
    }
}

pub fn url_enc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

pub fn csrf_input(csrf: &str) -> Markup {
    html! { input type="hidden" name="csrf" value=(csrf); }
}

pub fn status_badge(s: Status) -> Markup {
    let (cls, txt) = match s {
        Status::Running => ("ok", "Em execução"),
        Status::Stopped => ("bad", "Parado"),
        Status::Unknown => ("warn", "Desconhecido"),
    };
    html! { span.badge.(cls) { span.dot {} (txt) } }
}

/// Barra de ocupação em SVG (o CSP não permite `style` inline).
pub fn usage_bar(used: u64, total: u64) -> Markup {
    let pct = if total == 0 { 0.0 } else { used as f64 * 100.0 / total as f64 };
    let tone = if pct >= 90.0 { "bad" } else if pct >= 75.0 { "warn" } else { "ok" };
    html! {
        div.usage title=(format!("{pct:.1}% em uso")) {
            svg.bar viewBox="0 0 100 6" preserveAspectRatio="none" aria-hidden="true" {
                rect.track x="0" y="0" width="100" height="6" rx="3" {}
                @if pct > 0.0 {
                    rect.(tone) x="0" y="0" width=(format!("{:.2}", pct.max(1.5))) height="6" rx="3" {}
                }
            }
            span.pct { (format!("{pct:.0}%")) }
        }
    }
}

pub fn error_box(err: Option<&str>) -> Markup {
    html! { @if let Some(e) = err { div.alert.bad role="alert" { (icon("alert")) span { (e) } } } }
}

// ---------- dados derivados ----------

pub struct RangeUse {
    pub set: usize,
    pub start: Ipv4Addr,
    pub end: Ipv4Addr,
    pub total: u64,
    pub used: u64,
}

pub fn range_usage(doc: &IniDoc, leases: &[Lease], now: i64) -> Vec<RangeUse> {
    dhcp::all_ranges(doc)
        .into_iter()
        .map(|(set, a, b)| {
            let (lo, hi) = (u32::from(a), u32::from(b));
            let used = leases.iter().filter(|l| l.is_active(now) && (lo..=hi).contains(&u32::from(l.ip))).count() as u64;
            RangeUse { set, start: a, end: b, total: (hi - lo) as u64 + 1, used }
        })
        .collect()
}

fn recent(snap: &Snapshot, kind: Kind, mins: i64, now: NaiveDateTime) -> Vec<&Event> {
    let since = now - CDuration::minutes(mins);
    snap.events.iter().filter(|e| e.kind == kind && e.ts.is_some_and(|t| t >= since)).collect()
}

pub fn alerts(snap: &Snapshot, now: i64, now_l: NaiveDateTime) -> Vec<Alert> {
    let mut out = Vec::new();
    match snap.status {
        Status::Stopped => out.push(Alert {
            action: Some(NoteAction::Service("start", "Iniciar", "")),
            ..Alert::new("svc-stopped", "bad", "O serviço DHCP está parado: nenhum dispositivo consegue pegar ou renovar IP.")
        }),
        Status::Unknown => out.push(Alert::new("svc-unknown", "warn", "Não consegui consultar o estado do serviço DHCP.")),
        Status::Running => {}
    }
    for (k, e) in [("ini-error", &snap.ini_error), ("lease-error", &snap.lease_error)] {
        if let Some(e) = e {
            out.push(Alert::new(k, "bad", e.clone()));
        }
    }
    for u in range_usage(&snap.doc, &snap.leases, now) {
        let pct = u.used as f64 * 100.0 / u.total.max(1) as f64;
        if pct >= 80.0 {
            let tone = if pct >= 90.0 { "bad" } else { "warn" };
            out.push(Alert {
                rev: tone.into(), // passou de 80% para 90%: avisa de novo
                action: Some(NoteAction::Link("/ranges", "Ver faixas")),
                ..Alert::new(
                    format!("range:{}-{}", u.start, u.end),
                    tone,
                    format!("A faixa {} – {} está com {pct:.0}% ocupada ({} livres).", u.start, u.end, u.total - u.used),
                )
            });
        }
    }
    let nr = recent(snap, Kind::NoRange, 15, now_l);
    if let Some(last) = nr.last() {
        out.push(Alert {
            rev: last.seq.to_string(),
            action: Some(NoteAction::Link("/activity", "Ver atividade")),
            ..Alert::new(
                "norange",
                "bad",
                format!(
                    "{} pedido(s) de IP sem faixa nos últimos 15 min (último: {} pela interface {}). Esses dispositivos estão sem rede.",
                    nr.len(),
                    last.mac.as_deref().unwrap_or("?"),
                    last.ip.as_deref().unwrap_or("?")
                ),
            )
        });
    }
    let dc = recent(snap, Kind::Decline, 15, now_l);
    if let Some(last) = dc.last() {
        out.push(Alert {
            rev: last.seq.to_string(),
            action: Some(NoteAction::Link("/activity", "Ver atividade")),
            ..Alert::new(
                "decline",
                "warn",
                format!(
                    "{} conflito(s) de IP nos últimos 15 min (último: {} já estava em uso na rede).",
                    dc.len(),
                    last.ip.as_deref().unwrap_or("?")
                ),
            )
        });
    }
    if let Some(e) = &snap.log.error {
        out.push(Alert::new("log-error", "warn", e.clone()));
    }
    out
}

/// (início do intervalo, entregues, renovações, problemas)
pub type Bucket = (NaiveDateTime, u32, u32, u32);

/// Eventos por intervalo de 5 min nas últimas 2 h.
pub fn activity_buckets(snap: &Snapshot, now: NaiveDateTime) -> Vec<Bucket> {
    const N: i64 = 24;
    const STEP: i64 = 5;
    let start = now - CDuration::minutes(N * STEP);
    let mut b: Vec<Bucket> =
        (0..N).map(|i| (start + CDuration::minutes(i * STEP), 0, 0, 0)).collect();
    for e in &snap.events {
        let Some(t) = e.ts else { continue };
        if t < start || t > now {
            continue;
        }
        let i = (((t - start).num_seconds()) / (STEP * 60)).clamp(0, N - 1) as usize;
        match e.kind {
            Kind::Allot => b[i].1 += 1,
            Kind::Renew => b[i].2 += 1,
            Kind::NoRange | Kind::Decline | Kind::Nak => b[i].3 += 1,
            _ => {}
        }
    }
    b
}

// ---------- layout ----------

pub struct Shell<'a> {
    pub title: &'a str,
    pub nav: &'a str,
    /// Página ao vivo (`/live?page=`): dashboard, leases, activity ou other.
    pub live: &'a str,
    pub user: &'a str,
    pub csrf: &'a str,
    pub flash: Option<(bool, &'a str)>,
}

/// (href, chave, rótulo, ícone)
type NavItem = (&'static str, &'static str, &'static str, &'static str);

const NAV: [(&str, &[NavItem]); 3] = [
    ("Monitoramento", &[
        ("/", "dashboard", "Visão geral", "home"),
        ("/activity", "activity", "Atividade", "pulse"),
        ("/leases", "leases", "Leases ativos", "devices"),
    ]),
    ("Configuração", &[
        ("/reservations", "reservations", "Reservas", "pin"),
        ("/ranges", "ranges", "Faixas", "ranges"),
        ("/options", "options", "Opções globais", "settings"),
    ]),
    ("Sistema", &[
        ("/account", "account", "Minha conta", "lock"),
    ]),
];

pub fn shell(s: Shell, snap: &Snapshot, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="pt-BR" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (s.title) " · Open DHCP" }
                script src="/static/theme.js" {}
                link rel="stylesheet" href="/static/style.css";
                script src="/static/app.js" defer {}
            }
            body data-live=(s.live) {
                div.shell {
                    aside.side #side {
                        a.brand href="/" {
                            span.logo { (icon("wifi")) }
                            span { strong { "Open DHCP" } small { "Painel de controle" } }
                        }
                        nav {
                            @for (group, items) in NAV {
                                p.nav-group { (group) }
                                @for (href, key, label, ic) in items.iter() {
                                    a.nav-item href=(href) class=[(s.nav == *key).then_some("active")] {
                                        (icon(ic)) span { (label) }
                                    }
                                }
                            }
                        }
                        div.side-foot {
                            div.side-card {
                                span.muted.small { "Serviço DHCP" }
                                div #svc-badge { (status_badge(snap.status)) }
                            }
                        }
                    }
                    div.main {
                        header.topbar {
                            button.icon-btn.menu-btn type="button" data-toggle="side" aria-label="Menu" { (icon("ranges")) }
                            div.crumbs {
                                span.muted { "Open DHCP" } span.sep { "/" } strong { (s.title) }
                            }
                            div.top-right {
                                details.notif #notif {
                                    summary.icon-btn #notif-btn aria-label="Notificações" title="Notificações" { (frag_notif_btn(snap)) }
                                    div.notif-panel #notif-list { (frag_notif_list(snap, s.csrf)) }
                                }
                                span.live-pill #live-pill title="Atualização em tempo real" { span.dot {} span.txt { "Conectando" } }
                                button.icon-btn type="button" data-theme-toggle aria-label="Alternar tema" title="Alternar tema claro/escuro" {
                                    span.only-light { (icon("moon")) } span.only-dark { (icon("sun")) }
                                }
                                div.user {
                                    span.avatar { (s.user.chars().next().unwrap_or('?').to_uppercase().to_string()) }
                                    a.user-name href="/account" title="Minha conta" { (s.user) }
                                    form.inline method="post" action="/logout" {
                                        (csrf_input(s.csrf))
                                        button.icon-btn type="submit" title="Sair" aria-label="Sair" { (icon("logout")) }
                                    }
                                }
                            }
                        }
                        main.content {
                            div.page-head {
                                h1 { (s.title) }
                                span.muted.small #updated { "Atualizado às " (snap.updated.format("%H:%M:%S")) }
                            }
                            @if let Some((ok, text)) = s.flash {
                                div.alert.row.(if ok { "ok" } else { "bad" }) role="status" {
                                    (icon(if ok { "check" } else { "alert" })) span.grow { (text) }
                                    button.icon-btn.x type="button" data-close title="Fechar" aria-label="Fechar" { (icon("x")) }
                                }
                            }
                            div #alerts { (frag_alerts(snap, s.csrf)) }
                            (body)
                        }
                        footer.foot { span.muted.small { "Open DHCP Server · painel " (env!("CARGO_PKG_VERSION")) } }
                    }
                }
            }
        }
    }
}

pub fn service_button(csrf: &str, action: &str, label: &str, class: &str, ic: &str, confirm: Option<&str>) -> Markup {
    html! {
        form.inline method="post" action={"/service/" (action)} data-confirm=[confirm] {
            (csrf_input(csrf))
            button.btn.(class) type="submit" { (icon(ic)) (label) }
        }
    }
}

fn note_action(a: &Alert, csrf: &str, small: bool) -> Markup {
    let cls = if small { "small" } else { "" };
    html! {
        @match &a.action {
            Some(NoteAction::Link(href, label)) => a.btn.light.(cls) href=(href) { (label) },
            Some(NoteAction::Service(act, label, confirm)) => {
                form.inline method="post" action={"/service/" (act)} data-confirm=[(!confirm.is_empty()).then_some(*confirm)] {
                    (csrf_input(csrf))
                    button.btn.primary.(cls) type="submit" { (label) }
                }
            }
            None => {}
        }
    }
}

fn dismiss_button(id: Option<u64>, csrf: &str, label: &str, cls: &str) -> Markup {
    html! {
        form.inline method="post" action="/notes/dismiss" data-ajax {
            (csrf_input(csrf))
            input type="hidden" name="id" value=(id.map(|i| i.to_string()).unwrap_or_else(|| "all".into()));
            @if cls == "x" {
                button.icon-btn.x type="submit" title=(label) aria-label=(label) { (icon("x")) }
            } @else {
                button.btn.light.small type="submit" { (label) }
            }
        }
    }
}

/// Alertas ativos e não fechados, no alto da página.
pub fn frag_alerts(snap: &Snapshot, csrf: &str) -> Markup {
    html! {
        @for n in snap.notes.iter().filter(|n| n.visible() && n.active()) {
            div.alert.row.(n.alert.tone) role="alert" {
                (icon("alert"))
                span.grow { (n.alert.text) }
                (note_action(&n.alert, csrf, false))
                (dismiss_button(Some(n.id), csrf, "Fechar", "x"))
            }
        }
    }
}

pub fn frag_notif_btn(snap: &Snapshot) -> Markup {
    let n = snap.notes.iter().filter(|n| n.visible()).count();
    html! {
        (icon("bell"))
        @if n > 0 { span.notif-count { (if n > 99 { "99+".to_string() } else { n.to_string() }) } }
    }
}

fn note_item(n: &Note, csrf: &str) -> Markup {
    html! {
        li.note.(n.alert.tone) .resolved[!n.active()] {
            span.note-dot {}
            div.note-body {
                p { (n.alert.text) }
                span.muted.small {
                    @if let Some(r) = n.resolved { "Resolvido às " (r.format("%H:%M")) }
                    @else { "Desde " (n.since.format("%d/%m %H:%M")) }
                }
                @if n.active() { div.note-act { (note_action(&n.alert, csrf, true)) } }
            }
            (dismiss_button(Some(n.id), csrf, "Marcar como lida", "x"))
        }
    }
}

pub fn frag_notif_list(snap: &Snapshot, csrf: &str) -> Markup {
    let list: Vec<&Note> = snap.notes.iter().rev().filter(|n| n.visible()).collect();
    html! {
        div.notif-head {
            strong { "Notificações" }
            @if !list.is_empty() { (dismiss_button(None, csrf, "Limpar todas", "")) }
        }
        @if list.is_empty() {
            p.notif-empty.muted.small { (icon("check")) " Nenhuma notificação." }
        } @else {
            ul.notes { @for n in list { (note_item(n, csrf)) } }
        }
    }
}

pub fn frag_updated(snap: &Snapshot) -> Markup {
    html! { "Atualizado às " (snap.updated.format("%H:%M:%S")) }
}

// ---------- visão geral ----------

pub fn dashboard_body(snap: &Snapshot, csrf: &str) -> Markup {
    html! {
        section.grid.kpis #dash-kpis { (frag_dash_kpis(snap)) }
        section.grid.g-4-8 {
            div.card.hero #dash-service { (frag_dash_service(snap, csrf)) }
            div.card {
                div.card-head {
                    h2.card-title { "Atividade nas últimas 2 horas" }
                    div.legend { span.lg.primary { "IP entregue" } span.lg.info { "Renovação" } span.lg.danger { "Problema" } }
                }
                div #dash-chart { (frag_dash_chart(snap)) }
            }
        }
        section.grid.g-8-4 {
            div.card {
                div.card-head { h2.card-title { "Uso das faixas" } a.btn.light.small href="/ranges/new" { (icon("plus")) "Nova faixa" } }
                div #dash-ranges { (frag_dash_ranges(snap)) }
            }
            div.card {
                div.card-head { h2.card-title { "Ocupação geral" } }
                div #dash-donut { (frag_dash_donut(snap)) }
            }
        }
        section.card {
            div.card-head { h2.card-title { "Atividade recente" } a.btn.light.small href="/activity" { "Ver tudo" } }
            div #dash-feed { (frag_timeline(snap, 8)) }
        }
    }
}

fn kpi(icon_name: &str, tone: &str, label: &str, value: String, sub: Markup, href: &str) -> Markup {
    html! {
        a.card.kpi href=(href) {
            span.kpi-icon.(tone) { (icon(icon_name)) }
            div.kpi-body {
                span.kpi-label { (label) }
                strong.kpi-value { (value) }
                span.kpi-sub { (sub) }
            }
        }
    }
}

pub fn frag_dash_kpis(snap: &Snapshot) -> Markup {
    let now = now_ts();
    let now_l = now_local();
    let active = snap.leases.iter().filter(|l| l.is_active(now)).count();
    let usage = range_usage(&snap.doc, &snap.leases, now);
    let total: u64 = usage.iter().map(|u| u.total).sum();
    let used: u64 = usage.iter().map(|u| u.used).sum();
    let reservations = dhcp::reservations(&snap.doc).len();
    let hour_ago = now_l - CDuration::hours(1);
    let last_hour: Vec<&Event> = snap.events.iter().filter(|e| e.ts.is_some_and(|t| t >= hour_ago)).collect();
    let new_hour = last_hour.iter().filter(|e| e.kind == Kind::Allot).count();
    let problems = last_hour.iter().filter(|e| matches!(e.kind, Kind::NoRange | Kind::Decline | Kind::Nak)).count();
    let fresh = snap.leases.iter().filter(|l| snap.fresh_kind(&l.mac) == Some(true)).count();
    html! {
        (kpi("devices", "primary", "Dispositivos com IP", active.to_string(),
            html! { @if fresh > 0 { span.up { "+" (fresh) " agora" } } @else { "leases ativos" } }, "/leases"))
        (kpi("ranges", "success", "Endereços livres", total.saturating_sub(used).to_string(),
            html! { "de " (total) " nas faixas" }, "/ranges"))
        (kpi("pin", "info", "Reservas", reservations.to_string(), html! { "IPs fixos por MAC" }, "/reservations"))
        (kpi("pulse", if problems > 0 { "danger" } else { "warning" }, "Eventos na última hora", last_hour.len().to_string(),
            html! { (new_hour) " novos" @if problems > 0 { " · " span.down { (problems) " problema(s)" } } }, "/activity"))
    }
}

pub fn frag_dash_service(snap: &Snapshot, csrf: &str) -> Markup {
    let (title, text) = match snap.status {
        Status::Running => ("Servidor DHCP ativo", "Atendendo pedidos de IP na rede."),
        Status::Stopped => ("Servidor DHCP parado", "Nenhum dispositivo consegue pegar ou renovar IP."),
        Status::Unknown => ("Estado desconhecido", "Não consegui consultar o serviço."),
    };
    let state = match snap.status {
        Status::Running => "on",
        Status::Stopped => "off",
        Status::Unknown => "unk",
    };
    html! {
        div.hero-body.(state) {
            div.hero-top {
                span.hero-icon { (icon("power")) }
                (status_badge(snap.status))
            }
            h2 { (title) }
            p { (text) }
            div.hero-actions {
                @if snap.status != Status::Running {
                    (service_button(csrf, "start", "Iniciar", "white", "play", None))
                }
                @if snap.status != Status::Stopped {
                    (service_button(csrf, "restart", "Reiniciar", "white", "refresh", Some("Reiniciar o servidor DHCP?")))
                    (service_button(csrf, "stop", "Parar", "ghost-white", "stop", Some("Parar o servidor DHCP? Os dispositivos não vão conseguir pegar nem renovar IP enquanto ele estiver parado.")))
                }
            }
        }
    }
}

pub fn frag_dash_chart(snap: &Snapshot) -> Markup {
    if !snap.log.configured {
        return html! { p.empty { "Configure "code { "log_file" } " no panel.toml para ver a atividade do servidor." } };
    }
    let now = now_local();
    let b = activity_buckets(snap, now);
    // escala "redonda" (múltiplo de 4) para as marcas 0, metade e topo serem inteiras
    let peak = b.iter().map(|x| x.1 + x.2 + x.3).max().unwrap_or(0).max(4);
    let max = peak.div_ceil(4) as f64 * 4.0;
    let (w, h, gap) = (20.0_f64, 120.0_f64, 5.0_f64);
    let total: u32 = b.iter().map(|x| x.1 + x.2 + x.3).sum();
    html! {
        div.chart {
            div.chart-y { span { (max as u32) } span { (max as u32 / 2) } span { "0" } }
            div.chart-plot {
                svg viewBox=(format!("0 0 {} {}", b.len() as f64 * w, h)) preserveAspectRatio="none" role="img" aria-label="Eventos por 5 minutos" {
                    line.grid x1="0" x2="480" y1="0.5" y2="0.5" {}
                    line.grid x1="0" x2="480" y1=(h / 2.0) y2=(h / 2.0) {}
                    line.grid x1="0" x2="480" y1=(h - 0.5) y2=(h - 0.5) {}
                    @for (i, (t, a, r, p)) in b.iter().enumerate() {
                        @let x = i as f64 * w + gap / 2.0;
                        @let ha = *a as f64 / max * h;
                        @let hr = *r as f64 / max * h;
                        @let hp = *p as f64 / max * h;
                        g {
                            title { (t.format("%H:%M")) " – " (a) " entregue(s), " (r) " renovação(ões), " (p) " problema(s)" }
                            rect.hit x=(i as f64 * w) y="0" width=(w) height=(h) {}
                            @if *r > 0 { rect.info x=(x) y=(h - hr) width=(w - gap) height=(hr) rx="2" {} }
                            @if *a > 0 { rect.primary x=(x) y=(h - hr - ha) width=(w - gap) height=(ha) rx="2" {} }
                            @if *p > 0 { rect.danger x=(x) y=(h - hr - ha - hp) width=(w - gap) height=(hp) rx="2" {} }
                        }
                    }
                }
                div.chart-x {
                    @for (i, (t, ..)) in b.iter().enumerate() {
                        @if i % 6 == 0 { span { (t.format("%H:%M")) } }
                    }
                    span { "agora" }
                }
            }
        }
        @if total == 0 { p.muted.small.center { "Nenhum evento nas últimas 2 horas." } }
    }
}

pub fn frag_dash_ranges(snap: &Snapshot) -> Markup {
    let usage = range_usage(&snap.doc, &snap.leases, now_ts());
    html! {
        @if usage.is_empty() {
            p.empty { "Nenhuma faixa dinâmica configurada." }
        } @else {
            div.table-wrap {
                table {
                    thead { tr { th { "Faixa" } th { "Conjunto" } th.num { "Em uso" } th.num { "Livres" } th.wide { "Ocupação" } } }
                    tbody {
                        @for u in &usage {
                            tr {
                                td.mono { (u.start) " – " (u.end) }
                                td { a href={"/ranges/" (u.set) "/edit"} { "#" (u.set + 1) } }
                                td.num { (u.used) } td.num { (u.total - u.used) }
                                td { (usage_bar(u.used, u.total)) }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn frag_dash_donut(snap: &Snapshot) -> Markup {
    let now = now_ts();
    let usage = range_usage(&snap.doc, &snap.leases, now);
    let total: u64 = usage.iter().map(|u| u.total).sum();
    let used: u64 = usage.iter().map(|u| u.used).sum();
    let reserved_active = snap
        .leases
        .iter()
        .filter(|l| l.is_active(now) && !usage.iter().any(|u| (u32::from(u.start)..=u32::from(u.end)).contains(&u32::from(l.ip))))
        .count();
    let pct = if total == 0 { 0.0 } else { used as f64 * 100.0 / total as f64 };
    let tone = if pct >= 90.0 { "bad" } else if pct >= 75.0 { "warn" } else { "ok" };
    html! {
        div.donut-wrap {
            svg.donut viewBox="0 0 42 42" role="img" aria-label=(format!("{pct:.0}% ocupado")) {
                circle.track cx="21" cy="21" r="15.915" fill="none" stroke-width="5" {}
                circle.(tone) cx="21" cy="21" r="15.915" fill="none" stroke-width="5"
                    stroke-dasharray=(format!("{:.2} {:.2}", pct, 100.0 - pct)) stroke-dashoffset="25"
                    stroke-linecap=(if pct >= 4.0 { "round" } else { "butt" }) {}
                text.donut-num x="21" y="21" { (format!("{pct:.0}%")) }
                text.donut-sub x="21" y="27" { "ocupado" }
            }
            ul.donut-legend {
                li { span.sw.(tone) {} "Em uso" strong { (used) } }
                li { span.sw.track {} "Livres" strong { (total.saturating_sub(used)) } }
                li { span.sw.info {} "Fora das faixas (reservas)" strong { (reserved_active) } }
            }
        }
    }
}

fn kind_tag(k: Kind) -> Markup {
    html! { span.tag.(k.tone()) { (k.label()) } }
}

pub fn frag_timeline(snap: &Snapshot, limit: usize) -> Markup {
    let now = now_local();
    html! {
        @if !snap.log.configured {
            p.empty { "Configure " code { "log_file" } " no panel.toml para acompanhar a atividade." }
        } @else if snap.events.is_empty() {
            p.empty { "Nenhum evento registrado no log ainda." }
        } @else {
            ul.timeline {
                @for e in snap.events.iter().rev().take(limit) {
                    li.(e.kind.tone()) {
                        span.tl-dot {}
                        div.tl-body {
                            div.tl-top { (kind_tag(e.kind)) span.muted.small { (ago(e.ts, now)) } }
                            p { (events::describe(e)) }
                        }
                    }
                }
            }
        }
    }
}

// ---------- atividade ----------

pub fn activity_body(snap: &Snapshot) -> Markup {
    html! {
        section.grid.kpis #act-stats { (frag_activity_stats(snap)) }
        section.card {
            div.card-head {
                h2.card-title { "Eventos do servidor" }
                div.row {
                    div.chips data-chips="#feed-table" {
                        button.chip.active type="button" data-kind="" { "Todos" }
                        button.chip type="button" data-kind="ok" { "Entregues" }
                        button.chip type="button" data-kind="renew" { "Renovações" }
                        button.chip type="button" data-kind="warn" { "Liberados" }
                        button.chip type="button" data-kind="bad" { "Problemas" }
                    }
                    label.search { (icon("search")) input type="search" placeholder="Filtrar por MAC, IP ou nome" data-filter="#feed-table" aria-label="Filtrar"; }
                }
            }
            div #feed { (frag_feed(snap)) }
        }
    }
}

pub fn frag_activity_stats(snap: &Snapshot) -> Markup {
    let now_l = now_local();
    let hour_ago = now_l - CDuration::hours(1);
    let last: Vec<&Event> = snap.events.iter().filter(|e| e.ts.is_some_and(|t| t >= hour_ago)).collect();
    let count = |ks: &[Kind]| last.iter().filter(|e| ks.contains(&e.kind)).count().to_string();
    html! {
        (kpi("check", "success", "IPs entregues (1 h)", count(&[Kind::Allot]), html! { "novos dispositivos ou IPs" }, "/activity"))
        (kpi("refresh", "info", "Renovações (1 h)", count(&[Kind::Renew]), html! { "dispositivos mantendo o IP" }, "/activity"))
        (kpi("logout", "warning", "Liberados (1 h)", count(&[Kind::Release]), html! { "devolvidos pelos dispositivos" }, "/activity"))
        (kpi("alert", "danger", "Problemas (1 h)", count(&[Kind::NoRange, Kind::Decline, Kind::Nak]), html! { "sem faixa, conflito ou recusa" }, "/activity"))
    }
}

pub fn frag_feed(snap: &Snapshot) -> Markup {
    let now = now_local();
    html! {
        @if !snap.log.configured {
            p.empty { "O painel não está lendo o log do servidor. Configure " code { "log_file" } " no panel.toml (o mesmo caminho do " code { "-l" } " do opendhcpd)." }
        } @else if snap.events.is_empty() {
            p.empty { "Nenhum evento no log ainda. Com " code { "LogLevel=Normal" } " o servidor registra IPs entregues, renovações e liberações; com " code { "All" } ", também as ofertas." }
        } @else {
            div.table-wrap {
                table #feed-table {
                    thead { tr { th { "Quando" } th { "Evento" } th { "Descrição" } th { "MAC" } th { "IP" } } }
                    tbody {
                        @for e in snap.events.iter().rev().take(400) {
                            @let kind_key = if e.kind == Kind::Renew { "renew" } else { e.kind.tone() };
                            tr data-kind=(kind_key) {
                                td.nowrap title=[e.ts.map(|t| t.format("%d/%m/%Y %H:%M:%S").to_string())] {
                                    (e.ts.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_default())
                                    " " span.muted.small { (ago(e.ts, now)) }
                                }
                                td { (kind_tag(e.kind)) }
                                td { (events::describe(e)) }
                                td.mono { (e.mac.as_deref().unwrap_or("")) }
                                td.mono { @if !matches!(e.kind, Kind::NoRange) { (e.ip.as_deref().unwrap_or("")) } }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------- leases ----------

pub fn leases_body(snap: &Snapshot, csrf: &str, state_file: &str) -> Markup {
    html! {
        section.card {
            div.card-head {
                h2.card-title #lease-count { (frag_lease_count(snap)) }
                label.search { (icon("search")) input type="search" placeholder="Filtrar por MAC, IP ou nome" data-filter="#lease-table" aria-label="Filtrar"; }
            }
            div #lease-wrap { (frag_lease_table(snap, csrf)) }
        }
        p.hint {
            "Lidos de " code { (state_file) } " e atualizados sozinhos. Para liberar um lease, o painel para o serviço por alguns segundos, tira o dispositivo do arquivo e inicia o serviço de novo; o dispositivo pede um IP novo na próxima renovação."
        }
    }
}

pub fn frag_lease_count(snap: &Snapshot) -> Markup {
    let n = snap.leases.iter().filter(|l| l.is_active(now_ts())).count();
    html! { (n) " dispositivo(s) com IP" }
}

pub fn frag_lease_table(snap: &Snapshot, csrf: &str) -> Markup {
    let now = now_ts();
    let reserved: Vec<String> = dhcp::reservations(&snap.doc).into_iter().map(|r| r.mac).collect();
    let mut active: Vec<&Lease> = snap.leases.iter().filter(|l| l.is_active(now)).collect();
    active.sort_by_key(|l| (!snap.is_fresh(&l.mac), u32::from(l.ip)));
    html! {
        @if let Some(e) = &snap.lease_error { (error_box(Some(e))) }
        @if active.is_empty() {
            p.empty { "Nenhum dispositivo com IP no momento." }
        } @else {
            div.table-wrap {
                table #lease-table {
                    thead { tr { th { "IP" } th { "MAC" } th { "Nome do host" } th { "Expira" } th { "Tipo" } th.actions { "" } } }
                    tbody {
                        @for l in &active {
                            @let (when, rel) = fmt_expiry(l.expiry, now);
                            @let is_res = reserved.contains(&l.mac);
                            tr class=[snap.is_fresh(&l.mac).then_some("fresh")] {
                                td.mono {
                                    (l.ip)
                                    @match snap.fresh_kind(&l.mac) {
                                        Some(true) => { " " span.tag.primary.small { "novo" } }
                                        Some(false) => { " " span.tag.info.small { "renovado" } }
                                        None => {}
                                    }
                                }
                                td.mono { (l.mac) }
                                td { @if l.hostname.is_empty() { span.muted { "—" } } @else { (l.hostname) } }
                                td.nowrap { (when) " " span.muted.small { (rel) } }
                                td { @if is_res { span.tag.info { "Reserva" } } @else { span.tag { "Dinâmico" } } }
                                td.actions {
                                    @if !is_res {
                                        a.btn.small.light href={"/reservations/new?mac=" (l.mac) "&ip=" (l.ip) "&hostname=" (url_enc(&l.hostname))} { (icon("pin")) "Fixar IP" }
                                    }
                                    form.inline method="post" action="/leases/release"
                                        data-confirm={"Liberar o IP " (l.ip) " de " (l.mac) "? O serviço DHCP será parado e iniciado de novo por alguns segundos."} {
                                        (csrf_input(csrf))
                                        input type="hidden" name="mac" value=(l.mac);
                                        button.btn.small.soft-danger type="submit" { "Liberar" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------- login ----------

pub fn login_page(err: Option<&str>) -> Markup {
    html! {
        (DOCTYPE)
        html lang="pt-BR" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Entrar · Open DHCP" }
                script src="/static/theme.js" {}
                link rel="stylesheet" href="/static/style.css";
            }
            body.login-body {
                div.login-wrap {
                    div.login-art {
                        span.logo.big { (icon("wifi")) }
                        h2 { "Open DHCP" }
                        p { "Acompanhe os dispositivos da rede em tempo real e gerencie faixas, reservas e o serviço DHCP num só lugar." }
                    }
                    form.card.login method="post" action="/login" {
                        h1 { "Entrar" }
                        p.muted { "Use o usuário configurado no panel.toml." }
                        (error_box(err))
                        label { "Usuário" input name="username" autocomplete="username" required autofocus; }
                        label { "Senha" input name="password" type="password" autocomplete="current-password" required; }
                        button.btn.primary.full type="submit" { "Entrar" }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::parse_line;

    fn snap_with(lines: &[&str]) -> Snapshot {
        let mut s = Snapshot::empty();
        s.doc = IniDoc::parse("[RANGE_SET]\nDHCPRange=10.0.0.1-10.0.0.10\n");
        s.log.configured = true;
        for (i, l) in lines.iter().enumerate() {
            s.events.push_back(parse_line(l, i as u64).unwrap());
        }
        s
    }

    #[test]
    fn alertas_de_faixa_cheia_e_sem_faixa() {
        let now_l = now_local();
        let ts = now_l.format("%d-%b-%y %H:%M:%S").to_string();
        let mut s = snap_with(&[&format!("[{ts}] No Matching DHCP Range for DHCPDISCOVER for aa:bb:cc:dd:ee:01 (x) from interface 10.9.0.1")]);
        s.status = Status::Running;
        for i in 1..=9u8 {
            s.leases.push(Lease { index: i as u16, mac: format!("aa:00:00:00:00:{i:02x}"), ip: Ipv4Addr::new(10, 0, 0, i), expiry: leases::INFINITE, hostname: String::new(), local: true });
        }
        let a = alerts(&s, now_ts(), now_l);
        assert!(a.iter().any(|a| a.text.contains("90% ocupada")), "{:?}", a.iter().map(|a| &a.text).collect::<Vec<_>>());
        assert!(a.iter().any(|a| a.text.contains("sem faixa")));
        s.status = Status::Stopped;
        assert!(alerts(&s, now_ts(), now_l)[0].text.contains("parado"));
    }

    #[test]
    fn grafico_agrupa_por_5_minutos() {
        let now_l = now_local();
        let t = |m: i64| (now_l - CDuration::minutes(m)).format("%d-%b-%y %H:%M:%S").to_string();
        let s = snap_with(&[
            &format!("[{}] Host aa:bb:cc:dd:ee:01 (a) allotted 10.0.0.2 for 60 seconds", t(1)),
            &format!("[{}] Host aa:bb:cc:dd:ee:02 (b) renewed 10.0.0.3 for 60 seconds", t(2)),
            &format!("[{}] Host aa:bb:cc:dd:ee:03 (c) allotted 10.0.0.4 for 60 seconds", t(300)),
        ]);
        let b = activity_buckets(&s, now_l);
        assert_eq!(b.len(), 24);
        assert_eq!(b.iter().map(|x| x.1).sum::<u32>(), 1, "evento de 5 h atrás fica fora");
        assert_eq!(b[23].1 + b[23].2, 2);
    }

    #[test]
    fn hostname_malicioso_e_escapado() {
        let mut s = snap_with(&["[26-Nov-26 03:30:48] Host aa:bb:cc:dd:ee:01 (<img src=x onerror=alert(1)>) allotted 10.0.0.2 for 60 seconds"]);
        s.leases.push(Lease { index: 1, mac: "aa:bb:cc:dd:ee:01".into(), ip: Ipv4Addr::new(10, 0, 0, 2), expiry: leases::INFINITE, hostname: "<b>x</b>".into(), local: true });
        let html = frag_feed(&s).into_string() + &frag_lease_table(&s, "t").into_string() + &frag_timeline(&s, 5).into_string();
        assert!(!html.contains("<img"));
        assert!(!html.contains("<b>x"));
        assert!(html.contains("&lt;img"));
    }
}
