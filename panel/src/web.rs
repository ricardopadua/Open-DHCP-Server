use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::extract::{ConnectInfo, FromRequestParts, Path as UrlPath, Query, Request, State};
use axum::http::{header, request::Parts, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use futures_util::{Stream, StreamExt};
use maud::{html, Markup};
use sha2::{Digest, Sha256};
use tokio_stream::wrappers::WatchStream;

use crate::auth::{self, Session, Sessions};
use crate::config::Config;
use crate::dhcp::{self, OptionsInput, RangeSetInput, ReservationInput};
use crate::ini::IniDoc;
use crate::leases;
use crate::monitor::{Monitor, Snapshot};
use crate::notes::{Alert, NoteAction};
use crate::service::{self, Action, Status};
use crate::views::{self, csrf_input, error_box, icon, Shell};

const COOKIE: &str = "odp_session";
const CSS: &str = include_str!("../assets/style.css");
const JS: &str = include_str!("../assets/app.js");
const THEME_JS: &str = include_str!("../assets/theme.js");

pub struct App {
    pub cfg: Config,
    pub sessions: Sessions,
    pub monitor: Arc<Monitor>,
    write_lock: tokio::sync::Mutex<()>,
    /// panel.toml (gravado quando a pessoa troca usuário/senha em "Minha conta").
    config_path: PathBuf,
    /// (usuário, hash) atuais; começam com os do panel.toml.
    creds: RwLock<(String, String)>,
    default_password: AtomicBool,
    restart_rev: std::sync::atomic::AtomicU64,
}

fn restart_alert(app: &App) -> Alert {
    Alert {
        rev: app.restart_rev.load(Ordering::SeqCst).to_string(), // cada alteração nova avisa de novo
        action: Some(NoteAction::Service("restart", "Reiniciar agora", "Reiniciar o servidor DHCP agora?")),
        ..Alert::new("restart", "info", "A configuração mudou. Reinicie o serviço para o DHCP passar a usar as alterações.")
    }
}

fn password_alert() -> Alert {
    Alert {
        action: Some(NoteAction::Link("/account", "Trocar senha")),
        ..Alert::new("default-password", "warn", "Você está usando a senha padrão. Troque-a para proteger o painel.")
    }
}

/// Senha de fábrica: o painel avisa até que ela seja trocada.
pub const DEFAULT_PASSWORD: &str = "admin";

type Shared = Arc<App>;
type Fields = HashMap<String, String>;

pub fn router(cfg: Config, monitor: Arc<Monitor>, config_path: PathBuf) -> Router {
    let default_password = auth::verify_password(DEFAULT_PASSWORD, &cfg.auth.password_hash);
    let creds = RwLock::new((cfg.auth.username.clone(), cfg.auth.password_hash.clone()));
    let app = Arc::new(App {
        config_path,
        creds,
        default_password: AtomicBool::new(default_password),
        restart_rev: std::sync::atomic::AtomicU64::new(0),
        cfg,
        sessions: Sessions::default(),
        monitor,
        write_lock: tokio::sync::Mutex::new(()),
    });
    if default_password {
        app.monitor.put_extra(password_alert());
    }
    Router::new()
        .route("/", get(dashboard))
        .route("/live", get(live))
        .route("/login", get(login_page).post(login))
        .route("/logout", post(logout))
        .route("/activity", get(activity_page))
        .route("/leases", get(leases_page))
        .route("/leases/release", post(release_lease))
        .route("/reservations", get(reservations_page))
        .route("/reservations/new", get(reservation_new).post(reservation_create))
        .route("/reservations/{n}/edit", get(reservation_edit).post(reservation_update))
        .route("/reservations/{n}/delete", post(reservation_delete))
        .route("/ranges", get(ranges_page))
        .route("/ranges/new", get(range_new).post(range_create))
        .route("/ranges/{n}/edit", get(range_edit).post(range_update))
        .route("/ranges/{n}/delete", post(range_delete))
        .route("/options", get(options_page).post(options_update))
        .route("/service/{action}", post(service_action))
        .route("/account", get(account_page).post(account_update))
        .route("/notes/dismiss", post(dismiss_note))
        .route("/static/style.css", get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CSS) }))
        .route("/static/app.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], JS) }))
        .route("/static/theme.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], THEME_JS) }))
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}

async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; frame-ancestors 'none'; form-action 'self'; base-uri 'none'"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

// ---------- sessão ----------

pub struct Ctx {
    token: String,
    s: Session,
}

impl FromRequestParts<Shared> for Ctx {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, app: &Shared) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar.get(COOKIE).map(|c| c.value().to_string()).unwrap_or_default();
        match app.sessions.get(&token) {
            Some(s) => Ok(Ctx { token, s }),
            None => Err(Redirect::to("/login").into_response()),
        }
    }
}

impl Ctx {
    #[allow(clippy::result_large_err)] // o erro já é a resposta HTTP pronta
    fn check_csrf(&self, f: &Fields) -> Result<(), Response> {
        let sent = f.get("csrf").map(String::as_str).unwrap_or("");
        if auth::ct_eq(sent, &self.s.csrf) {
            Ok(())
        } else {
            Err((StatusCode::FORBIDDEN, "Formulário expirado; recarregue a página.").into_response())
        }
    }
}

fn field<'a>(f: &'a Fields, k: &str) -> &'a str {
    f.get(k).map(String::as_str).unwrap_or("")
}

fn flash(app: &App, ctx: &Ctx, ok: bool, text: impl Into<String>, to: &str) -> Response {
    app.sessions.set_flash(&ctx.token, ok, text);
    Redirect::to(to).into_response()
}

/// Página completa com o layout comum.
fn page(app: &App, ctx: &Ctx, title: &str, nav: &str, live: &str, body: Markup) -> Markup {
    let snap = app.monitor.snapshot();
    views::shell(
        Shell {
            title,
            nav,
            live,
            user: &ctx.s.user,
            csrf: &ctx.s.csrf,
            flash: ctx.s.flash.as_ref().map(|f| (f.ok, f.text.as_str())),
        },
        &snap,
        body,
    )
}

// ---------- arquivo .ini ----------

struct Loaded {
    doc: IniDoc,
    rev: String,
    latin1: bool,
}

fn rev_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn load_ini(path: &Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("não consegui ler {}: {e}", path.display()))?;
    let rev = rev_of(&bytes);
    // O .ini original pode ter acentos em Latin-1; nesse caso gravamos de volta em Latin-1.
    let (text, latin1) = match String::from_utf8(bytes) {
        Ok(t) => (t, false),
        Err(e) => (e.into_bytes().iter().map(|b| *b as char).collect(), true),
    };
    Ok(Loaded { doc: IniDoc::parse(&text), rev, latin1 })
}

/// Aplica `change` ao .ini sob trava, conferindo se ninguém mudou o arquivo desde que o
/// formulário foi aberto (`rev`). Faz backup antes de gravar.
async fn mutate_ini<T>(
    app: &App,
    rev: &str,
    change: impl FnOnce(&mut IniDoc) -> Result<T, String>,
) -> Result<T, String> {
    let out = {
        let _guard = app.write_lock.lock().await;
        let path = &app.cfg.ini_file;
        let loaded = load_ini(path)?;
        if loaded.rev != rev {
            return Err("O arquivo de configuração foi alterado por outra pessoa ou processo. Recarregue a página e refaça a alteração.".into());
        }
        let mut doc = loaded.doc.clone();
        let out = change(&mut doc)?;
        let text = doc.to_text();
        let bytes: Vec<u8> = if loaded.latin1 {
            text.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect()
        } else {
            text.into_bytes()
        };
        crate::fsutil::backup(path, &app.cfg.backup_dir, app.cfg.backups_keep)
            .map_err(|e| format!("não consegui fazer o backup em {}: {e}", app.cfg.backup_dir.display()))?;
        crate::fsutil::write_atomic(path, &bytes).map_err(|e| format!("não consegui gravar {}: {e}", path.display()))?;
        app.restart_rev.fetch_add(1, Ordering::SeqCst);
        tracing::info!(file = %path.display(), "configuração alterada pelo painel");
        out
    };
    app.monitor.set_extra("restart", Some(restart_alert(app))).await;
    app.monitor.poke().await;
    Ok(out)
}

// ---------- tempo real ----------

#[derive(serde::Deserialize)]
struct LiveQuery {
    page: Option<String>,
}

/// Pedaços (seletor -> HTML) que a página `page` atualiza.
fn live_patch(snap: &Snapshot, page: &str, csrf: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    let mut put = |k: &str, v: Markup| {
        m.insert(k.to_string(), serde_json::Value::String(v.into_string()));
    };
    put("#svc-badge", views::status_badge(snap.status));
    put("#alerts", views::frag_alerts(snap, csrf));
    put("#notif-btn", views::frag_notif_btn(snap));
    put("#notif-list", views::frag_notif_list(snap, csrf));
    put("#updated", views::frag_updated(snap));
    match page {
        "dashboard" => {
            put("#dash-kpis", views::frag_dash_kpis(snap));
            put("#dash-service", views::frag_dash_service(snap, csrf));
            put("#dash-chart", views::frag_dash_chart(snap));
            put("#dash-ranges", views::frag_dash_ranges(snap));
            put("#dash-donut", views::frag_dash_donut(snap));
            put("#dash-feed", views::frag_timeline(snap, 8));
        }
        "leases" => {
            put("#lease-count", views::frag_lease_count(snap));
            put("#lease-wrap", views::frag_lease_table(snap, csrf));
        }
        "activity" => {
            put("#act-stats", views::frag_activity_stats(snap));
            put("#feed", views::frag_feed(snap));
        }
        _ => {}
    }
    m
}

async fn live(
    State(app): State<Shared>,
    ctx: Ctx,
    Query(q): Query<LiveQuery>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let page = q.page.unwrap_or_default();
    let token = ctx.token.clone();
    let csrf = ctx.s.csrf.clone();
    let stream = WatchStream::new(app.monitor.subscribe()).map(move |_| {
        if !app.sessions.is_valid(&token) {
            return Ok(SseEvent::default().event("logout").data("1"));
        }
        let snap = app.monitor.snapshot();
        let patch = live_patch(&snap, &page, &csrf);
        Ok(SseEvent::default().event("patch").data(serde_json::Value::Object(patch).to_string()))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

// ---------- login ----------

async fn login_page(jar: CookieJar, State(app): State<Shared>) -> Response {
    let token = jar.get(COOKIE).map(|c| c.value().to_string()).unwrap_or_default();
    if app.sessions.is_valid(&token) {
        return Redirect::to("/").into_response();
    }
    views::login_page(None).into_response()
}

async fn login(
    State(app): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(f): Form<Fields>,
) -> Response {
    let ip = peer.ip();
    if app.sessions.locked_out(ip) {
        return (StatusCode::TOO_MANY_REQUESTS, views::login_page(Some("Muitas tentativas. Aguarde 5 minutos."))).into_response();
    }
    let user = field(&f, "username").to_string();
    let pass = field(&f, "password").to_string();
    let (username, hash) = app.creds.read().unwrap().clone();
    let pass_ok = tokio::task::spawn_blocking(move || auth::verify_password(&pass, &hash)).await.unwrap_or(false);
    let ok = pass_ok && auth::ct_eq(&user, &username);
    if !ok {
        app.sessions.record_fail(ip);
        tracing::warn!(%ip, "login recusado");
        tokio::time::sleep(Duration::from_millis(600)).await;
        return (StatusCode::UNAUTHORIZED, views::login_page(Some("Usuário ou senha inválidos."))).into_response();
    }
    app.sessions.clear_fails(ip);
    let ttl = Duration::from_secs(app.cfg.session_hours * 3600);
    let token = app.sessions.create(&user, ttl);
    tracing::info!(%ip, %user, "login");
    let cookie = Cookie::build((COOKIE, token))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(app.cfg.secure_cookie)
        .path("/")
        .build(); // cookie de sessão do navegador; a validade real fica no servidor
    (jar.add(cookie), Redirect::to("/")).into_response()
}

async fn logout(State(app): State<Shared>, ctx: Ctx, jar: CookieJar, Form(f): Form<Fields>) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    app.sessions.remove(&ctx.token);
    (jar.remove(Cookie::build(COOKIE).path("/")), Redirect::to("/login")).into_response()
}

// ---------- monitoramento ----------

async fn dashboard(State(app): State<Shared>, ctx: Ctx) -> Response {
    let snap = app.monitor.snapshot();
    let body = views::dashboard_body(&snap, &ctx.s.csrf);
    page(&app, &ctx, "Visão geral", "dashboard", "dashboard", body).into_response()
}

async fn activity_page(State(app): State<Shared>, ctx: Ctx) -> Response {
    let snap = app.monitor.snapshot();
    let body = views::activity_body(&snap);
    page(&app, &ctx, "Atividade", "activity", "activity", body).into_response()
}

async fn leases_page(State(app): State<Shared>, ctx: Ctx) -> Response {
    let snap = app.monitor.snapshot();
    let body = views::leases_body(&snap, &ctx.s.csrf, &app.cfg.state_file.display().to_string());
    page(&app, &ctx, "Leases ativos", "leases", "leases", body).into_response()
}

async fn service_action(
    State(app): State<Shared>,
    ctx: Ctx,
    UrlPath(action): UrlPath<String>,
    Form(f): Form<Fields>,
) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    let Some(act) = Action::parse(&action) else { return StatusCode::NOT_FOUND.into_response() };
    let result = {
        let _guard = app.write_lock.lock().await;
        tracing::info!(user = %ctx.s.user, ?act, "ação no serviço");
        service::act(&app.cfg.service, act).await
    };
    app.monitor.poke().await;
    match result {
        Ok(()) => {
            if matches!(act, Action::Start | Action::Restart) {
                app.monitor.set_extra("restart", None).await;
            }
            let msg = match act {
                Action::Start => "Serviço iniciado.",
                Action::Stop => "Serviço parado.",
                Action::Restart => "Serviço reiniciado com a configuração atual.",
            };
            flash(&app, &ctx, true, msg, "/")
        }
        Err(e) => flash(&app, &ctx, false, format!("Falha ao executar a ação: {e}"), "/"),
    }
}

async fn release_lease(State(app): State<Shared>, ctx: Ctx, Form(f): Form<Fields>) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    let Some(mac) = crate::ini::normalize_mac(field(&f, "mac")) else {
        return flash(&app, &ctx, false, "MAC inválido.", "/leases");
    };
    let (result, restart) = {
        let _guard = app.write_lock.lock().await;
        let status = service::status(&app.cfg.service).await;
        if status == Status::Unknown {
            return flash(&app, &ctx, false, "Não consegui saber se o serviço está rodando; nada foi alterado.", "/leases");
        }
        let was_running = status == Status::Running;
        if was_running {
            if let Err(e) = service::act(&app.cfg.service, Action::Stop).await {
                return flash(&app, &ctx, false, format!("Não consegui parar o serviço: {e}"), "/leases");
            }
        }
        let result = leases::remove_mac(&app.cfg.state_file, &mac);
        let restart = if was_running { service::act(&app.cfg.service, Action::Start).await } else { Ok(()) };
        (result, restart)
    };
    app.monitor.poke().await;
    tracing::info!(user = %ctx.s.user, %mac, "lease liberado");
    match (result, restart) {
        (Ok(0), Ok(())) => flash(&app, &ctx, false, format!("Nenhum lease encontrado para {mac}."), "/leases"),
        (Ok(_), Ok(())) => flash(&app, &ctx, true, format!("Lease de {mac} liberado."), "/leases"),
        (Err(e), Ok(())) => flash(&app, &ctx, false, format!("Não consegui alterar o arquivo de leases: {e}"), "/leases"),
        (_, Err(e)) => flash(&app, &ctx, false, format!("ATENÇÃO: o serviço foi parado e não voltou: {e}"), "/"),
    }
}

// ---------- formulários (componentes) ----------

fn form_card(action: &str, rev: &str, csrf: &str, cancel: &str, fields: Markup, extra: &[(String, String)], extra_title: &str) -> Markup {
    html! {
        form.card.form method="post" action=(action) {
            (csrf_input(csrf))
            input type="hidden" name="rev" value=(rev);
            div.fields { (fields) }
            @if !extra.is_empty() {
                div.extra {
                    p.muted.small { (extra_title) }
                    ul { @for (k, v) in extra { li.mono { (k) "=" (v) } } }
                }
            }
            div.form-actions {
                a.btn.light href=(cancel) { "Cancelar" }
                button.btn.primary type="submit" { (icon("check")) "Salvar" }
            }
        }
    }
}

fn delete_form(action: &str, rev: &str, csrf: &str, confirm: &str, label: &str) -> Markup {
    html! {
        form.danger-zone method="post" action=(action) data-confirm=(confirm) {
            (csrf_input(csrf))
            input type="hidden" name="rev" value=(rev);
            button.btn.soft-danger type="submit" { (icon("trash")) (label) }
        }
    }
}

// ---------- reservas ----------

async fn reservations_page(State(app): State<Shared>, ctx: Ctx) -> Response {
    let (list, err) = match load_ini(&app.cfg.ini_file) {
        Ok(l) => (dhcp::reservations(&l.doc), None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let snap = app.monitor.snapshot();
    let now = views::now_ts();
    let body = html! {
        (error_box(err.as_deref()))
        section.card {
            div.card-head {
                h2.card-title { (list.len()) " reserva(s)" }
                div.row {
                    label.search { (icon("search")) input type="search" placeholder="Filtrar" data-filter="#res-table" aria-label="Filtrar"; }
                    a.btn.primary href="/reservations/new" { (icon("plus")) "Nova reserva" }
                }
            }
            @if list.is_empty() {
                p.empty { "Nenhuma reserva. Crie uma para que um dispositivo receba sempre o mesmo IP." }
            } @else {
                div.table-wrap {
                    table #res-table {
                        thead { tr { th { "MAC" } th { "IP" } th { "Nome do host" } th { "Agora" } th { "Outras opções" } th.actions { "" } } }
                        tbody {
                            @for r in &list {
                                @let online = snap.leases.iter().any(|l| l.mac == r.mac && l.is_active(now));
                                tr {
                                    td.mono { (r.mac) }
                                    td.mono { @match &r.ip { Some(ip) => (ip), None => span.tag.bad { (icon("lock")) "Bloqueado" } } }
                                    td { @match &r.hostname { Some(h) => (h), None => span.muted { "—" } } }
                                    td { @if online { span.badge.ok { span.dot {} "Com IP" } } @else { span.muted.small { "sem lease" } } }
                                    td { @if r.extra.is_empty() { span.muted { "—" } } @else { span.tag { (r.extra.len()) " opção(ões)" } } }
                                    td.actions { a.btn.small.light href={"/reservations/" (r.n) "/edit"} { (icon("edit")) "Editar" } }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    page(&app, &ctx, "Reservas", "reservations", "other", body).into_response()
}

struct ResForm<'a> {
    n: Option<usize>,
    rev: &'a str,
    mac: &'a str,
    ip: &'a str,
    hostname: &'a str,
    extra: &'a [(String, String)],
    err: Option<&'a str>,
}

fn reservation_form(csrf: &str, rf: ResForm) -> Markup {
    let action = match rf.n { Some(n) => format!("/reservations/{n}/edit"), None => "/reservations/new".into() };
    let fields = html! {
        label { "Endereço MAC" input.mono name="mac" value=(rf.mac) placeholder="00:11:22:33:44:55" required; }
        label { "IP fixo" input.mono name="ip" value=(rf.ip) placeholder="192.168.0.50";
            small { "Deixe vazio para bloquear o dispositivo (ele nunca recebe IP)." } }
        label { "Nome do host (opcional)" input name="hostname" value=(rf.hostname) placeholder="impressora-sala"; }
    };
    html! {
        (error_box(rf.err))
        (form_card(&action, rf.rev, csrf, "/reservations", fields, rf.extra, "Outras opções desta reserva no .ini (mantidas como estão):"))
        @if let Some(n) = rf.n {
            (delete_form(&format!("/reservations/{n}/delete"), rf.rev, csrf, &format!("Excluir a reserva de {}?", rf.mac), "Excluir reserva"))
        }
    }
}

async fn reservation_new(State(app): State<Shared>, ctx: Ctx, Query(q): Query<Fields>) -> Response {
    let loaded = match load_ini(&app.cfg.ini_file) {
        Ok(l) => l,
        Err(e) => return flash(&app, &ctx, false, e, "/reservations"),
    };
    let form = reservation_form(
        &ctx.s.csrf,
        ResForm { n: None, rev: &loaded.rev, mac: field(&q, "mac"), ip: field(&q, "ip"), hostname: field(&q, "hostname"), extra: &[], err: None },
    );
    page(&app, &ctx, "Nova reserva", "reservations", "other", form).into_response()
}

async fn reservation_create(State(app): State<Shared>, ctx: Ctx, Form(f): Form<Fields>) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    let input = ReservationInput { mac: field(&f, "mac"), ip: field(&f, "ip"), hostname: field(&f, "hostname") };
    match mutate_ini(&app, field(&f, "rev"), |doc| dhcp::add_reservation(doc, &input)).await {
        Ok(mac) => flash(&app, &ctx, true, format!("Reserva de {mac} criada."), "/reservations"),
        Err(e) => {
            let form = reservation_form(
                &ctx.s.csrf,
                ResForm { n: None, rev: field(&f, "rev"), mac: input.mac, ip: input.ip, hostname: input.hostname, extra: &[], err: Some(&e) },
            );
            (StatusCode::UNPROCESSABLE_ENTITY, page(&app, &ctx, "Nova reserva", "reservations", "other", form)).into_response()
        }
    }
}

async fn reservation_edit(State(app): State<Shared>, ctx: Ctx, UrlPath(n): UrlPath<usize>) -> Response {
    let loaded = match load_ini(&app.cfg.ini_file) {
        Ok(l) => l,
        Err(e) => return flash(&app, &ctx, false, e, "/reservations"),
    };
    let Some(r) = dhcp::reservations(&loaded.doc).into_iter().find(|r| r.n == n) else {
        return flash(&app, &ctx, false, "Reserva não encontrada.", "/reservations");
    };
    let form = reservation_form(
        &ctx.s.csrf,
        ResForm {
            n: Some(n),
            rev: &loaded.rev,
            mac: &r.mac,
            ip: r.ip.as_deref().unwrap_or(""),
            hostname: r.hostname.as_deref().unwrap_or(""),
            extra: &r.extra,
            err: None,
        },
    );
    page(&app, &ctx, "Editar reserva", "reservations", "other", form).into_response()
}

async fn reservation_update(State(app): State<Shared>, ctx: Ctx, UrlPath(n): UrlPath<usize>, Form(f): Form<Fields>) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    let input = ReservationInput { mac: field(&f, "mac"), ip: field(&f, "ip"), hostname: field(&f, "hostname") };
    match mutate_ini(&app, field(&f, "rev"), |doc| dhcp::update_reservation(doc, n, &input)).await {
        Ok(mac) => flash(&app, &ctx, true, format!("Reserva de {mac} atualizada."), "/reservations"),
        Err(e) => {
            let form = reservation_form(
                &ctx.s.csrf,
                ResForm { n: Some(n), rev: field(&f, "rev"), mac: input.mac, ip: input.ip, hostname: input.hostname, extra: &[], err: Some(&e) },
            );
            (StatusCode::UNPROCESSABLE_ENTITY, page(&app, &ctx, "Editar reserva", "reservations", "other", form)).into_response()
        }
    }
}

async fn reservation_delete(State(app): State<Shared>, ctx: Ctx, UrlPath(n): UrlPath<usize>, Form(f): Form<Fields>) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    match mutate_ini(&app, field(&f, "rev"), |doc| dhcp::delete_reservation(doc, n)).await {
        Ok(mac) => flash(&app, &ctx, true, format!("Reserva de {mac} excluída."), "/reservations"),
        Err(e) => flash(&app, &ctx, false, e, "/reservations"),
    }
}

// ---------- faixas ----------

async fn ranges_page(State(app): State<Shared>, ctx: Ctx) -> Response {
    let snap = app.monitor.snapshot();
    let (sets, doc, err) = match load_ini(&app.cfg.ini_file) {
        Ok(l) => (dhcp::range_sets(&l.doc), l.doc, None),
        Err(e) => (Vec::new(), IniDoc::parse(""), Some(e)),
    };
    let usage = views::range_usage(&doc, &snap.leases, views::now_ts());
    let body = html! {
        (error_box(err.as_deref()))
        div.page-actions { a.btn.primary href="/ranges/new" { (icon("plus")) "Novo conjunto de faixas" } }
        @if sets.is_empty() {
            section.card { p.empty { "Nenhum [RANGE_SET] no arquivo. Sem faixas, o servidor só atende reservas." } }
        }
        div.grid.g-2 {
            @for rs in &sets {
                section.card {
                    div.card-head {
                        h2.card-title { "Conjunto #" (rs.n + 1) }
                        a.btn.small.light href={"/ranges/" (rs.n) "/edit"} { (icon("edit")) "Editar" }
                    }
                    @if rs.ranges.is_empty() {
                        p.empty { "Sem faixas ativas (só comentários ou opções)." }
                    } @else {
                        ul.range-list {
                            @for u in usage.iter().filter(|u| u.set == rs.n) {
                                li {
                                    div.range-top { span.mono { (u.start) " – " (u.end) } span.muted.small { (u.used) " de " (u.total) " em uso" } }
                                    (views::usage_bar(u.used, u.total))
                                }
                            }
                        }
                    }
                    dl.opts {
                        div { dt { "Máscara" } dd.mono { (rs.opts.mask.as_deref().unwrap_or("—")) } }
                        div { dt { "Gateway" } dd.mono { (rs.opts.router.as_deref().unwrap_or("—")) } }
                        div { dt { "DNS" } dd.mono { (rs.opts.dns.as_deref().unwrap_or("—")) } }
                        div { dt { "Lease" } dd.mono { (rs.opts.lease.as_deref().map(|s| s.parse::<u32>().map(crate::events::fmt_secs).unwrap_or(s.to_string())).unwrap_or("padrão".into())) } }
                    }
                    @if !rs.extra.is_empty() {
                        p.muted.small { (rs.extra.len()) " outra(s) opção(ões)/filtro(s) neste conjunto, mantidos como estão." }
                    }
                }
            }
        }
    };
    page(&app, &ctx, "Faixas dinâmicas", "ranges", "other", body).into_response()
}

struct RangeForm<'a> {
    n: Option<usize>,
    rev: &'a str,
    ranges: &'a str,
    mask: &'a str,
    router: &'a str,
    dns: &'a str,
    lease: &'a str,
    extra: &'a [(String, String)],
    err: Option<&'a str>,
}

fn range_form(csrf: &str, rf: RangeForm) -> Markup {
    let action = match rf.n { Some(n) => format!("/ranges/{n}/edit"), None => "/ranges/new".into() };
    let fields = html! {
        label.span2 { "Faixas (uma por linha)"
            textarea.mono name="ranges" rows="4" placeholder="192.168.0.100-192.168.0.200" required { (rf.ranges) } }
        label { "Máscara de rede" input.mono name="mask" value=(rf.mask) placeholder="255.255.255.0"; }
        label { "Gateway (roteador)" input.mono name="router" value=(rf.router) placeholder="192.168.0.1"; }
        label { "Servidores DNS" input.mono name="dns" value=(rf.dns) placeholder="192.168.0.1, 8.8.8.8"; }
        label { "Tempo de lease (segundos)" input.mono name="lease" value=(rf.lease) placeholder="36000" inputmode="numeric";
            small { "Vazio usa o padrão das opções globais; 0 = sem expiração." } }
    };
    html! {
        (error_box(rf.err))
        (form_card(&action, rf.rev, csrf, "/ranges", fields, rf.extra, "Outras opções e filtros deste conjunto (mantidos como estão):"))
        @if let Some(n) = rf.n {
            (delete_form(&format!("/ranges/{n}/delete"), rf.rev, csrf, "Excluir este conjunto de faixas inteiro, com opções, filtros e comentários?", "Excluir conjunto"))
        }
    }
}

fn range_input(f: &Fields) -> RangeSetInput<'_> {
    RangeSetInput {
        ranges: field(f, "ranges"),
        opts: OptionsInput { mask: field(f, "mask"), router: field(f, "router"), dns: field(f, "dns"), lease: field(f, "lease") },
    }
}

async fn range_new(State(app): State<Shared>, ctx: Ctx) -> Response {
    let loaded = match load_ini(&app.cfg.ini_file) {
        Ok(l) => l,
        Err(e) => return flash(&app, &ctx, false, e, "/ranges"),
    };
    let form = range_form(
        &ctx.s.csrf,
        RangeForm { n: None, rev: &loaded.rev, ranges: "", mask: "", router: "", dns: "", lease: "", extra: &[], err: None },
    );
    page(&app, &ctx, "Novo conjunto de faixas", "ranges", "other", form).into_response()
}

async fn range_save(app: Shared, ctx: Ctx, n: Option<usize>, f: Fields) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    let input = range_input(&f);
    let result = mutate_ini(&app, field(&f, "rev"), |doc| match n {
        Some(n) => dhcp::update_range_set(doc, n, &input),
        None => dhcp::add_range_set(doc, &input).map(|_| ()),
    })
    .await;
    match result {
        Ok(()) => flash(&app, &ctx, true, "Faixas salvas.", "/ranges"),
        Err(e) => {
            let form = range_form(
                &ctx.s.csrf,
                RangeForm {
                    n,
                    rev: field(&f, "rev"),
                    ranges: input.ranges,
                    mask: input.opts.mask,
                    router: input.opts.router,
                    dns: input.opts.dns,
                    lease: input.opts.lease,
                    extra: &[],
                    err: Some(&e),
                },
            );
            let title = if n.is_some() { "Editar faixas" } else { "Novo conjunto de faixas" };
            (StatusCode::UNPROCESSABLE_ENTITY, page(&app, &ctx, title, "ranges", "other", form)).into_response()
        }
    }
}

async fn range_create(State(app): State<Shared>, ctx: Ctx, Form(f): Form<Fields>) -> Response {
    range_save(app, ctx, None, f).await
}

async fn range_update(State(app): State<Shared>, ctx: Ctx, UrlPath(n): UrlPath<usize>, Form(f): Form<Fields>) -> Response {
    range_save(app, ctx, Some(n), f).await
}

async fn range_edit(State(app): State<Shared>, ctx: Ctx, UrlPath(n): UrlPath<usize>) -> Response {
    let loaded = match load_ini(&app.cfg.ini_file) {
        Ok(l) => l,
        Err(e) => return flash(&app, &ctx, false, e, "/ranges"),
    };
    let Some(rs) = dhcp::range_sets(&loaded.doc).into_iter().find(|r| r.n == n) else {
        return flash(&app, &ctx, false, "Conjunto de faixas não encontrado.", "/ranges");
    };
    let ranges = rs.ranges.join("\n");
    let form = range_form(
        &ctx.s.csrf,
        RangeForm {
            n: Some(n),
            rev: &loaded.rev,
            ranges: &ranges,
            mask: rs.opts.mask.as_deref().unwrap_or(""),
            router: rs.opts.router.as_deref().unwrap_or(""),
            dns: rs.opts.dns.as_deref().unwrap_or(""),
            lease: rs.opts.lease.as_deref().unwrap_or(""),
            extra: &rs.extra,
            err: None,
        },
    );
    page(&app, &ctx, "Editar faixas", "ranges", "other", form).into_response()
}

async fn range_delete(State(app): State<Shared>, ctx: Ctx, UrlPath(n): UrlPath<usize>, Form(f): Form<Fields>) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    match mutate_ini(&app, field(&f, "rev"), |doc| dhcp::delete_range_set(doc, n)).await {
        Ok(()) => flash(&app, &ctx, true, "Conjunto de faixas excluído.", "/ranges"),
        Err(e) => flash(&app, &ctx, false, e, "/ranges"),
    }
}

// ---------- opções globais ----------

fn options_form(csrf: &str, rev: &str, v: [&str; 5], extra: &[(String, String)], err: Option<&str>) -> Markup {
    let [domain, mask, router, dns, lease] = v;
    let fields = html! {
        p.muted.span2 { "Valem para todos os dispositivos, a menos que a faixa ou a reserva defina outro valor." }
        label { "Domínio" input name="domain" value=(domain) placeholder="empresa.local"; }
        label { "Máscara de rede" input.mono name="mask" value=(mask) placeholder="255.255.255.0"; }
        label { "Gateway (roteador)" input.mono name="router" value=(router) placeholder="192.168.0.1"; }
        label { "Servidores DNS" input.mono name="dns" value=(dns) placeholder="192.168.0.1, 8.8.8.8"; }
        label { "Tempo de lease padrão (segundos)" input.mono name="lease" value=(lease) placeholder="36000" inputmode="numeric";
            small { "0 = sem expiração." } }
    };
    html! {
        (error_box(err))
        (form_card("/options", rev, csrf, "/", fields, extra, "Outras opções globais no .ini (mantidas como estão):"))
    }
}

async fn options_page(State(app): State<Shared>, ctx: Ctx) -> Response {
    let loaded = match load_ini(&app.cfg.ini_file) {
        Ok(l) => l,
        Err(e) => return flash(&app, &ctx, false, e, "/"),
    };
    let g = dhcp::globals(&loaded.doc);
    let o = &g.opts;
    let form = options_form(
        &ctx.s.csrf,
        &loaded.rev,
        [
            g.domain.as_deref().unwrap_or(""),
            o.mask.as_deref().unwrap_or(""),
            o.router.as_deref().unwrap_or(""),
            o.dns.as_deref().unwrap_or(""),
            o.lease.as_deref().unwrap_or(""),
        ],
        &g.extra,
        None,
    );
    page(&app, &ctx, "Opções globais", "options", "other", form).into_response()
}

async fn options_update(State(app): State<Shared>, ctx: Ctx, Form(f): Form<Fields>) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    let opts = OptionsInput { mask: field(&f, "mask"), router: field(&f, "router"), dns: field(&f, "dns"), lease: field(&f, "lease") };
    let domain = field(&f, "domain");
    match mutate_ini(&app, field(&f, "rev"), |doc| dhcp::update_globals(doc, domain, &opts)).await {
        Ok(()) => flash(&app, &ctx, true, "Opções globais salvas.", "/options"),
        Err(e) => {
            let form = options_form(&ctx.s.csrf, field(&f, "rev"), [domain, opts.mask, opts.router, opts.dns, opts.lease], &[], Some(&e));
            (StatusCode::UNPROCESSABLE_ENTITY, page(&app, &ctx, "Opções globais", "options", "other", form)).into_response()
        }
    }
}

// ---------- minha conta ----------

fn account_form(csrf: &str, user: &str, err: Option<&str>) -> Markup {
    html! {
        (error_box(err))
        form.card.form method="post" action="/account" autocomplete="off" {
            (csrf_input(csrf))
            div.fields {
                label { "Usuário" input name="username" value=(user) required maxlength="32" autocomplete="username";
                    small { "Letras, números, ponto, hífen ou sublinhado." } }
                label { "Senha atual" input type="password" name="current" required autocomplete="current-password"; }
                label { "Nova senha" input type="password" name="new" required minlength="8" autocomplete="new-password";
                    small { "Pelo menos 8 caracteres." } }
                label { "Repita a nova senha" input type="password" name="confirm" required minlength="8" autocomplete="new-password"; }
            }
            div.form-actions {
                a.btn.light href="/" { "Cancelar" }
                button.btn.primary type="submit" { (icon("check")) "Salvar" }
            }
        }
        p.muted.small { "Ao salvar, as outras sessões abertas do painel são encerradas. A nova senha fica gravada (em hash) no "
            span.mono { "panel.toml" } "." }
    }
}

async fn account_page(State(app): State<Shared>, ctx: Ctx) -> Response {
    let body = account_form(&ctx.s.csrf, &ctx.s.user, None);
    page(&app, &ctx, "Minha conta", "account", "other", body).into_response()
}

fn valid_username(u: &str) -> bool {
    (1..=32).contains(&u.len()) && u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

async fn account_update(State(app): State<Shared>, ConnectInfo(peer): ConnectInfo<SocketAddr>, ctx: Ctx, Form(f): Form<Fields>) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    let user = field(&f, "username").trim().to_string();
    let (new, confirm) = (field(&f, "new").to_string(), field(&f, "confirm").to_string());
    let fail = |msg: &str| {
        let body = account_form(&ctx.s.csrf, &user, Some(msg));
        (StatusCode::UNPROCESSABLE_ENTITY, page(&app, &ctx, "Minha conta", "account", "other", body)).into_response()
    };
    if app.sessions.locked_out(peer.ip()) {
        return fail("Muitas tentativas. Aguarde 5 minutos.");
    }
    let current_hash = app.creds.read().unwrap().1.clone();
    let current = field(&f, "current").to_string();
    let ok = tokio::task::spawn_blocking(move || auth::verify_password(&current, &current_hash)).await.unwrap_or(false);
    if !ok {
        app.sessions.record_fail(peer.ip());
        tokio::time::sleep(Duration::from_millis(600)).await;
        return fail("A senha atual está errada.");
    }
    if !valid_username(&user) {
        return fail("Usuário inválido: use de 1 a 32 letras, números, ponto, hífen ou sublinhado.");
    }
    if new.chars().count() < 8 {
        return fail("A nova senha precisa ter pelo menos 8 caracteres.");
    }
    if new != confirm {
        return fail("A confirmação não confere com a nova senha.");
    }
    let hash = match tokio::task::spawn_blocking(move || auth::hash_password(&new)).await {
        Ok(Ok(h)) => h,
        _ => return fail("Não consegui gerar o hash da senha."),
    };
    let saved = {
        let _guard = app.write_lock.lock().await;
        std::fs::read_to_string(&app.config_path)
            .map_err(anyhow::Error::from)
            .and_then(|text| crate::config::set_auth(&text, &user, &hash))
            .and_then(|text| {
                crate::fsutil::write_atomic(&app.config_path, text.as_bytes())?;
                Ok(())
            })
    };
    if let Err(e) = saved {
        tracing::error!(error = %e, "não consegui gravar a nova senha");
        return fail(&format!("Não consegui gravar {}: {e}", app.config_path.display()));
    }
    *app.creds.write().unwrap() = (user.clone(), hash);
    app.default_password.store(false, Ordering::SeqCst);
    app.monitor.set_extra("default-password", None).await;
    app.sessions.clear_fails(peer.ip());
    app.sessions.keep_only(&ctx.token, &user);
    tracing::info!(%user, "usuário/senha do painel alterados");
    flash(&app, &ctx, true, "Usuário e senha atualizados.", "/")
}

// ---------- notificações ----------

/// Fecha uma notificação (`id`) ou todas (`id=all`). Pelo app.js vem por fetch e responde 204;
/// sem JavaScript, volta para a página.
async fn dismiss_note(State(app): State<Shared>, ctx: Ctx, headers: axum::http::HeaderMap, Form(f): Form<Fields>) -> Response {
    if let Err(r) = ctx.check_csrf(&f) {
        return r;
    }
    let id = match field(&f, "id") {
        "all" => None,
        v => match v.parse() {
            Ok(n) => Some(n),
            Err(_) => return (StatusCode::BAD_REQUEST, "id inválido").into_response(),
        },
    };
    app.monitor.dismiss(id).await;
    if headers.get("x-requested-with").is_some_and(|v| v == "fetch") {
        StatusCode::NO_CONTENT.into_response()
    } else {
        Redirect::to("/").into_response()
    }
}
