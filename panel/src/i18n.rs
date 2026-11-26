//! Idiomas do painel: português (texto-base, usado como chave), inglês, francês e espanhol.
//!
//! - `t("texto")` traduz um texto fixo;
//! - `tf!("{} reserva(s)", n)` traduz e preenche `{}` (ou `{0}`, `{1}` quando a tradução precisa
//!   de outra ordem);
//! - `tm("Configure {} no panel.toml", &[html!{ code { "log_file" } }])` faz o mesmo com HTML
//!   (o texto é escapado);
//! - `n_("texto")` só marca um texto guardado em constante, traduzido depois com `t`;
//! - `msg!(...)` guarda o modelo e os valores, para traduzir na hora de mostrar (alertas que vivem
//!   no monitor e são vistos por pessoas com idiomas diferentes).
//!
//! O idioma vale para a requisição inteira (`LANG.scope` no middleware). Sem idioma definido
//! (testes, tarefas de fundo), fica o português. Um teste confere que todo texto marcado tem as
//! três traduções.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

use maud::{html, Markup, PreEscaped};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Pt,
    En,
    Fr,
    Es,
}

pub const ALL: [Lang; 4] = [Lang::Pt, Lang::En, Lang::Fr, Lang::Es];

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Pt => "pt",
            Lang::En => "en",
            Lang::Fr => "fr",
            Lang::Es => "es",
        }
    }

    /// Valor do atributo `lang` do HTML.
    pub fn html(self) -> &'static str {
        match self {
            Lang::Pt => "pt-BR",
            other => other.code(),
        }
    }

    /// Nome do idioma nele mesmo.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Pt => "Português",
            Lang::En => "English",
            Lang::Fr => "Français",
            Lang::Es => "Español",
        }
    }

    pub fn from_code(s: &str) -> Option<Lang> {
        ALL.into_iter().find(|l| s.eq_ignore_ascii_case(l.code()))
    }

    /// Primeiro idioma conhecido do cabeçalho `Accept-Language` (na ordem de preferência).
    pub fn from_accept(header: &str) -> Option<Lang> {
        let mut prefs: Vec<(f32, &str)> = header
            .split(',')
            .filter_map(|part| {
                let mut it = part.trim().split(';');
                let tag = it.next()?.trim();
                let q = it
                    .find_map(|p| p.trim().strip_prefix("q="))
                    .and_then(|q| q.parse().ok())
                    .unwrap_or(1.0);
                Some((q, tag))
            })
            .collect();
        prefs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        prefs.into_iter().find_map(|(_, tag)| Lang::from_code(tag.split('-').next().unwrap_or("")))
    }

    fn index(self) -> usize {
        self as usize
    }
}

tokio::task_local! {
    pub static LANG: Lang;
}

/// Idioma da requisição atual.
pub fn lang() -> Lang {
    LANG.try_with(|l| *l).unwrap_or_default()
}

fn table() -> &'static HashMap<&'static str, &'static [&'static str; 4]> {
    static T: OnceLock<HashMap<&'static str, &'static [&'static str; 4]>> = OnceLock::new();
    T.get_or_init(|| crate::i18n_table::TABLE.iter().map(|row| (row[0], row)).collect())
}

/// Traduz `pt` para o idioma atual (sem tradução, devolve o próprio texto).
pub fn t(pt: &str) -> &str {
    let l = lang();
    if l == Lang::Pt {
        return pt;
    }
    match table().get(pt) {
        Some(row) => row[l.index()],
        None => pt,
    }
}

/// Marca um texto para tradução sem traduzir (constantes; use `t` ao mostrar).
pub const fn n_(pt: &'static str) -> &'static str {
    pt
}

/// Troca `{}` pelos valores na ordem, ou `{0}`, `{1}`... pelo valor daquela posição.
pub fn fill(tpl: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(tpl.len() + 16);
    let mut next = 0;
    let mut rest = tpl;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if after[..close].chars().all(|c| c.is_ascii_digit()) => {
                let idx = if close == 0 {
                    next += 1;
                    next - 1
                } else {
                    after[..close].parse().unwrap_or(usize::MAX)
                };
                if let Some(a) = args.get(idx) {
                    out.push_str(&a.to_string());
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `tf!("modelo {}", valor, ...)`: traduz o modelo e preenche.
#[macro_export]
macro_rules! tf {
    ($tpl:literal $(, $a:expr)* $(,)?) => {
        $crate::i18n::fill($crate::i18n::t($tpl), &[$(&$a as &dyn ::std::fmt::Display),*])
    };
}

/// Como `tf!`, mas com pedaços de HTML no lugar de `{}`; o texto do modelo é escapado.
pub fn tm(tpl: &str, args: &[Markup]) -> Markup {
    let tpl = t(tpl);
    let marks: Vec<String> = (0..args.len()).map(|i| format!("\u{1}{i}\u{1}")).collect();
    let refs: Vec<&dyn Display> = marks.iter().map(|m| m as &dyn Display).collect();
    let text = fill(tpl, &refs);
    let escaped = html! { (text) }.into_string();
    let mut out = String::with_capacity(escaped.len() + 64);
    let mut parts = escaped.split('\u{1}');
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    while let (Some(idx), Some(text)) = (parts.next(), parts.next()) {
        if let Some(a) = idx.parse::<usize>().ok().and_then(|i| args.get(i)) {
            out.push_str(&a.0);
        }
        out.push_str(text);
    }
    PreEscaped(out)
}

/// Mensagem guardada para traduzir depois (no idioma de quem vê).
#[derive(Debug, Clone, PartialEq)]
pub struct Msg {
    pub tpl: &'static str,
    pub args: Vec<String>,
}

impl Msg {
    pub fn render(&self) -> String {
        let refs: Vec<&dyn Display> = self.args.iter().map(|a| a as &dyn Display).collect();
        fill(t(self.tpl), &refs)
    }
}

impl From<&'static str> for Msg {
    fn from(tpl: &'static str) -> Self {
        Msg { tpl, args: Vec::new() }
    }
}

/// `msg!("modelo {}", valor, ...)`.
#[macro_export]
macro_rules! msg {
    ($tpl:literal $(, $a:expr)* $(,)?) => {
        $crate::i18n::Msg { tpl: $tpl, args: vec![$(($a).to_string()),*] }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn preenche_e_reordena() {
        assert_eq!(fill("{} de {}", &[&1, &"x"]), "1 de x");
        assert_eq!(fill("{1} / {0}", &[&"a", &"b"]), "b / a");
        assert_eq!(fill("sem {chave}", &[]), "sem {chave}");
    }

    #[test]
    fn idioma_do_navegador() {
        assert_eq!(Lang::from_accept("fr-CA,fr;q=0.9,en;q=0.8"), Some(Lang::Fr));
        assert_eq!(Lang::from_accept("de-DE,es;q=0.5,en;q=0.7"), Some(Lang::En));
        assert_eq!(Lang::from_accept("de"), None);
    }

    #[test]
    fn traduz_no_escopo_e_escapa_html() {
        let out = LANG.sync_scope(Lang::En, || (t("Reservas").to_string(), tm("Configure {} no panel.toml para ver a atividade do servidor.", &[html! { code { "<x>" } }]).into_string()));
        assert_eq!(out.0, "Reservations");
        assert!(out.1.contains("<code>&lt;x&gt;</code>"), "{}", out.1);
        assert_eq!(t("Reservas"), "Reservas"); // fora do escopo: português
    }

    /// Lê um literal de string Rust a partir de `s` (logo depois da aspa de abertura).
    fn literal(s: &str) -> Option<String> {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(out),
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '\n' => {
                        // continuação de linha: pula os espaços seguintes
                        let rest: String = chars.clone().collect();
                        let skip = rest.len() - rest.trim_start().len();
                        for _ in 0..skip {
                            chars.next();
                        }
                    }
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
        None
    }

    fn marked_keys() -> BTreeSet<String> {
        let sources = [
            include_str!("views.rs"),
            include_str!("web.rs"),
            include_str!("dhcp.rs"),
            include_str!("events.rs"),
            include_str!("monitor.rs"),
            include_str!("service.rs"),
            include_str!("notes.rs"),
        ];
        let mut keys = BTreeSet::new();
        for src in sources {
            for marker in ["t(", "tf!(", "tm(", "n_(", "msg!("] {
                for (i, _) in src.match_indices(marker) {
                    // `t(` também casa no fim de outros nomes (ex.: `count(`); exige limite
                    let before = src[..i].chars().last().unwrap_or(' ');
                    if before.is_alphanumeric() || before == '_' {
                        continue;
                    }
                    // o texto pode começar na linha seguinte
                    let Some(rest) = src[i + marker.len()..].trim_start().strip_prefix('"') else { continue };
                    if let Some(k) = literal(rest) {
                        keys.insert(k);
                    }
                }
            }
        }
        keys
    }

    fn placeholders(s: &str) -> usize {
        let refs: Vec<String> = (0..10).map(|i| format!("\u{1}{i}")).collect();
        let args: Vec<&dyn Display> = refs.iter().map(|r| r as &dyn Display).collect();
        fill(s, &args).matches('\u{1}').count()
    }

    #[test]
    fn todo_texto_marcado_tem_traducao() {
        let keys = marked_keys();
        assert!(keys.len() > 100, "achei só {} textos marcados", keys.len());
        let missing: Vec<&String> = keys.iter().filter(|k| !table().contains_key(k.as_str())).collect();
        assert!(missing.is_empty(), "sem tradução ({}):\n{}", missing.len(), missing.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>().join("\n"));
        let unused: Vec<&&str> = table().keys().filter(|k| !keys.contains(**k)).collect();
        assert!(unused.is_empty(), "traduções que ninguém usa: {unused:?}");
        for row in crate::i18n_table::TABLE {
            for (i, tr) in row.iter().enumerate() {
                assert!(!tr.trim().is_empty(), "tradução vazia ({i}) para {:?}", row[0]);
                assert_eq!(placeholders(tr), placeholders(row[0]), "número de {{}} diferente ({i}) em {:?}", row[0]);
            }
        }
        let dup = crate::i18n_table::TABLE.len() - table().len();
        assert_eq!(dup, 0, "chaves repetidas na tabela");
    }
}
