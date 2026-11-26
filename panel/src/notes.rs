//! Notificações: os alertas viram itens que a pessoa lê e fecha. Um alerta fechado não volta
//! enquanto a situação for a mesma; volta se acontecer algo novo (`rev` muda) ou se a situação
//! acabar e começar de novo. Alertas que já se resolveram continuam na lista (como
//! "resolvido") até serem fechados.

use chrono::{DateTime, Local};

/// Botão opcional da notificação.
#[derive(Debug, Clone, PartialEq)]
pub enum NoteAction {
    /// Link (href, rótulo).
    Link(&'static str, &'static str),
    /// Ação do serviço por POST (`/service/{acao}`, rótulo, confirmação).
    Service(&'static str, &'static str, &'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    /// Identifica a situação (ex.: "svc-stopped", "range:10.0.0.1-10.0.0.9").
    pub key: String,
    /// Muda quando há uma ocorrência nova da mesma situação (ex.: último evento do log).
    pub rev: String,
    pub tone: &'static str,
    pub text: String,
    pub action: Option<NoteAction>,
}

impl Alert {
    pub fn new(key: impl Into<String>, tone: &'static str, text: impl Into<String>) -> Self {
        Self { key: key.into(), rev: String::new(), tone, text: text.into(), action: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub id: u64,
    pub alert: Alert,
    pub since: DateTime<Local>,
    /// `None` enquanto a situação continua; hora em que se resolveu.
    pub resolved: Option<DateTime<Local>>,
    pub dismissed: bool,
}

impl Note {
    pub fn visible(&self) -> bool {
        !self.dismissed
    }
    pub fn active(&self) -> bool {
        self.resolved.is_none()
    }
}

/// Quantas notificações resolvidas e não lidas ficam guardadas.
const MAX_RESOLVED: usize = 50;

/// Atualiza a lista com os alertas de agora. Devolve `true` se algo mudou.
pub fn sync(notes: &mut Vec<Note>, alerts: Vec<Alert>, now: DateTime<Local>, next_id: &mut u64) -> bool {
    let before = notes.clone();
    for a in alerts.iter() {
        match notes.iter_mut().find(|n| n.active() && n.alert.key == a.key) {
            Some(n) => {
                if n.dismissed && n.alert.rev != a.rev {
                    n.dismissed = false; // aconteceu de novo
                    n.since = now;
                }
                n.alert = a.clone();
            }
            None => {
                *next_id += 1;
                notes.push(Note { id: *next_id, alert: a.clone(), since: now, resolved: None, dismissed: false });
            }
        }
    }
    for n in notes.iter_mut() {
        if n.active() && !alerts.iter().any(|a| a.key == n.alert.key) {
            n.resolved = Some(now);
        }
    }
    // fechada e resolvida: some de vez (se voltar, é uma notificação nova)
    notes.retain(|n| !n.dismissed || n.active());
    let resolved = notes.iter().filter(|n| !n.active()).count();
    if resolved > MAX_RESOLVED {
        let mut drop = resolved - MAX_RESOLVED;
        notes.retain(|n| {
            if drop > 0 && !n.active() {
                drop -= 1;
                false
            } else {
                true
            }
        });
    }
    *notes != before
}

/// Fecha uma notificação (`Some(id)`) ou todas (`None`). Devolve `true` se algo mudou.
pub fn dismiss(notes: &mut Vec<Note>, id: Option<u64>) -> bool {
    let mut changed = false;
    for n in notes.iter_mut().filter(|n| id.is_none_or(|id| n.id == id)) {
        if !n.dismissed {
            n.dismissed = true;
            changed = true;
        }
    }
    notes.retain(|n| !n.dismissed || n.active());
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(key: &str, rev: &str) -> Alert {
        Alert { rev: rev.into(), ..Alert::new(key, "bad", key) }
    }

    #[test]
    fn ciclo_de_vida() {
        let now = Local::now();
        let (mut notes, mut id) = (Vec::new(), 0);
        assert!(sync(&mut notes, vec![a("svc", "")], now, &mut id));
        assert_eq!(notes.len(), 1);
        // fechada: não volta enquanto a situação for a mesma
        assert!(dismiss(&mut notes, Some(1)));
        sync(&mut notes, vec![a("svc", "")], now, &mut id);
        assert!(notes.iter().all(|n| !n.visible()));
        // ocorrência nova da mesma situação: volta
        sync(&mut notes, vec![a("svc", "2")], now, &mut id);
        assert_eq!(notes.iter().filter(|n| n.visible()).count(), 1);
        // resolveu: fica como resolvida até fechar
        sync(&mut notes, vec![], now, &mut id);
        assert!(notes[0].visible() && !notes[0].active());
        assert!(dismiss(&mut notes, None));
        assert!(notes.is_empty());
        // fechada, resolvida e começou de novo: notificação nova
        sync(&mut notes, vec![a("svc", "")], now, &mut id);
        dismiss(&mut notes, None);
        sync(&mut notes, vec![], now, &mut id);
        sync(&mut notes, vec![a("svc", "")], now, &mut id);
        assert_eq!(notes.iter().filter(|n| n.visible()).count(), 1);
    }
}
