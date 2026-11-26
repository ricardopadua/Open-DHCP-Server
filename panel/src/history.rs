//! Histórico em disco (SQLite em `<data_dir>/history.sqlite`):
//!
//! - eventos do log do servidor, para ver dias anteriores e o histórico de cada dispositivo
//!   depois que o painel reinicia (sem duplicar quando o mesmo trecho do log é relido);
//! - notificações fechadas, para não voltarem quando o painel reinicia.
//!
//! Se o arquivo não puder ser aberto, o painel continua só com a memória.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection};

use crate::events::{Event, Kind};

const TS_FMT: &str = "%Y-%m-%d %H:%M:%S";

pub struct History {
    conn: Mutex<Connection>,
}

fn row_to_event(r: &rusqlite::Row) -> rusqlite::Result<Event> {
    let ts: String = r.get(1)?;
    let kind: String = r.get(2)?;
    Ok(Event {
        seq: r.get::<_, i64>(0)? as u64,
        ts: NaiveDateTime::parse_from_str(&ts, TS_FMT).ok(),
        kind: Kind::from_code(&kind),
        mac: r.get(3)?,
        host: r.get(4)?,
        ip: r.get(5)?,
        secs: r.get::<_, Option<i64>>(6)?.map(|s| s as u32),
        text: r.get(7)?,
    })
}

const COLS: &str = "id, ts, kind, mac, host, ip, secs, text";

impl History {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS events (
                 id   INTEGER PRIMARY KEY,
                 ts   TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 mac  TEXT,
                 host TEXT,
                 ip   TEXT,
                 secs INTEGER,
                 text TEXT NOT NULL,
                 UNIQUE (ts, text)
             );
             CREATE INDEX IF NOT EXISTS events_mac ON events (mac, ts);
             CREATE TABLE IF NOT EXISTS dismissed (key TEXT PRIMARY KEY, rev TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS occupancy (ts TEXT PRIMARY KEY, used INTEGER NOT NULL, total INTEGER NOT NULL);",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Grava os eventos; os que já existem (mesma hora e mesmo texto) são ignorados.
    pub fn insert(&self, events: &[Event]) -> rusqlite::Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut n = 0;
        {
            let mut st = tx.prepare_cached(
                "INSERT OR IGNORE INTO events (ts, kind, mac, host, ip, secs, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for e in events {
                let ts = e.ts.map(|t| t.format(TS_FMT).to_string()).unwrap_or_default();
                n += st.execute(params![ts, e.kind.code(), e.mac, e.host, e.ip, e.secs, e.text])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    fn query(&self, sql: &str, args: impl rusqlite::Params) -> Vec<Event> {
        let conn = self.conn.lock().unwrap();
        let result = conn.prepare_cached(sql).and_then(|mut st| st.query_map(args, row_to_event)?.collect());
        result.unwrap_or_else(|e| {
            tracing::error!(error = %e, "falha ao ler o histórico");
            Vec::new()
        })
    }

    /// Os `limit` eventos mais recentes, do mais antigo para o mais novo.
    pub fn recent(&self, limit: usize) -> Vec<Event> {
        let mut v = self.query(&format!("SELECT {COLS} FROM events ORDER BY ts DESC, id DESC LIMIT ?1"), [limit as i64]);
        v.reverse();
        v
    }

    /// Eventos de um dia, do mais novo para o mais antigo.
    pub fn day(&self, day: NaiveDate, limit: usize) -> Vec<Event> {
        let from = day.and_hms_opt(0, 0, 0).unwrap().format(TS_FMT).to_string();
        let to = day.and_hms_opt(23, 59, 59).unwrap().format(TS_FMT).to_string();
        self.query(
            &format!("SELECT {COLS} FROM events WHERE ts BETWEEN ?1 AND ?2 ORDER BY ts DESC, id DESC LIMIT ?3"),
            params![from, to, limit as i64],
        )
    }

    /// Eventos de um MAC, do mais novo para o mais antigo.
    pub fn by_mac(&self, mac: &str, limit: usize) -> Vec<Event> {
        self.query(&format!("SELECT {COLS} FROM events WHERE mac = ?1 ORDER BY ts DESC, id DESC LIMIT ?2"), params![mac, limit as i64])
    }

    /// Dia do evento mais antigo guardado.
    pub fn first_day(&self) -> Option<NaiveDate> {
        let conn = self.conn.lock().unwrap();
        let ts: Option<String> = conn.query_row("SELECT MIN(ts) FROM events WHERE ts <> ''", [], |r| r.get(0)).ok().flatten();
        ts.and_then(|t| NaiveDateTime::parse_from_str(&t, TS_FMT).ok()).map(|t| t.date())
    }

    /// Apaga eventos com mais de `days` dias (0 = guarda tudo).
    pub fn prune(&self, days: u32, now: NaiveDateTime) -> usize {
        if days == 0 {
            return 0;
        }
        let limit = (now - chrono::Duration::days(i64::from(days))).format(TS_FMT).to_string();
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM occupancy WHERE ts < ?1", [&limit]);
        conn.execute("DELETE FROM events WHERE ts < ?1", [limit]).unwrap_or(0)
    }

    /// Ocupação das faixas no intervalo de 5 minutos `bucket` (a última medição do intervalo vale).
    pub fn record_occupancy(&self, bucket: NaiveDateTime, used: u64, total: u64) {
        let conn = self.conn.lock().unwrap();
        let r = conn.execute(
            "INSERT OR REPLACE INTO occupancy (ts, used, total) VALUES (?1, ?2, ?3)",
            params![bucket.format(TS_FMT).to_string(), used as i64, total as i64],
        );
        if let Err(e) = r {
            tracing::error!(error = %e, "falha ao gravar a ocupação");
        }
    }

    /// Medições desde `since` (uma a cada 5 min); com `hourly`, o pico de cada hora.
    pub fn occupancy(&self, since: NaiveDateTime, hourly: bool) -> Vec<(NaiveDateTime, u64, u64)> {
        let sql = if hourly {
            "SELECT substr(ts, 1, 13) || ':00:00', MAX(used), MAX(total) FROM occupancy WHERE ts >= ?1 GROUP BY substr(ts, 1, 13) ORDER BY 1"
        } else {
            "SELECT ts, used, total FROM occupancy WHERE ts >= ?1 ORDER BY ts"
        };
        let conn = self.conn.lock().unwrap();
        let result = conn.prepare_cached(sql).and_then(|mut st| {
            st.query_map([since.format(TS_FMT).to_string()], |r| {
                let ts: String = r.get(0)?;
                Ok((ts, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
        });
        result
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(ts, u, t)| NaiveDateTime::parse_from_str(&ts, TS_FMT).ok().map(|d| (d, u, t)))
            .collect()
    }

    pub fn dismissed(&self) -> HashMap<String, String> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .prepare("SELECT key, rev FROM dismissed")
            .and_then(|mut st| st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect());
        result.unwrap_or_default()
    }

    pub fn set_dismissed(&self, key: &str, rev: &str) {
        let conn = self.conn.lock().unwrap();
        if let Err(e) = conn.execute("INSERT OR REPLACE INTO dismissed (key, rev) VALUES (?1, ?2)", params![key, rev]) {
            tracing::error!(error = %e, "falha ao gravar notificação fechada");
        }
    }

    /// Esquece as notificações fechadas cuja situação já acabou.
    pub fn forget_dismissed_except(&self, active: &[String]) {
        let conn = self.conn.lock().unwrap();
        let keys: Vec<String> = conn
            .prepare("SELECT key FROM dismissed")
            .and_then(|mut st| st.query_map([], |r| r.get(0))?.collect())
            .unwrap_or_default();
        for k in keys.iter().filter(|k| !active.contains(k)) {
            let _ = conn.execute("DELETE FROM dismissed WHERE key = ?1", [k]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::parse_line;

    #[test]
    fn grava_sem_duplicar_e_consulta() {
        let dir = tempfile::tempdir().unwrap();
        let h = History::open(&dir.path().join("x/history.sqlite")).unwrap();
        let evs: Vec<Event> = [
            "[25-Nov-26 23:59:00] Host aa:bb:cc:dd:ee:01 (a) allotted 10.0.0.2 for 60 seconds",
            "[26-Nov-26 08:00:00] Host aa:bb:cc:dd:ee:01 (a) renewed 10.0.0.2 for 60 seconds",
            "[26-Nov-26 08:01:00] IP Address 10.0.0.3 released by Host aa:bb:cc:dd:ee:02 (b)",
        ]
        .iter()
        .enumerate()
        .map(|(i, l)| parse_line(l, i as u64).unwrap())
        .collect();
        assert_eq!(h.insert(&evs).unwrap(), 3);
        assert_eq!(h.insert(&evs).unwrap(), 0, "o mesmo trecho do log relido não duplica");
        let recent = h.recent(10);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].kind, Kind::Allot, "do mais antigo para o mais novo");
        assert_eq!(recent[2].kind, Kind::Release);
        let day = h.day(NaiveDate::from_ymd_opt(2026, 11, 26).unwrap(), 10);
        assert_eq!(day.len(), 2);
        assert_eq!(day[0].kind, Kind::Release, "do mais novo para o mais antigo");
        assert_eq!(h.by_mac("aa:bb:cc:dd:ee:01", 10).len(), 2);
        assert_eq!(h.by_mac("aa:bb:cc:dd:ee:01", 10)[0].secs, Some(60));
        assert_eq!(h.first_day(), NaiveDate::from_ymd_opt(2026, 11, 25));
        let now = NaiveDate::from_ymd_opt(2026, 11, 27).unwrap().and_hms_opt(8, 0, 30).unwrap();
        assert_eq!(h.prune(0, now), 0);
        assert_eq!(h.prune(1, now), 2, "apaga o que tem mais de 1 dia (antes de 26/11 08:00:30)");
        assert_eq!(h.recent(10).len(), 1);
    }

    #[test]
    fn ocupacao_por_intervalo_e_por_hora() {
        let dir = tempfile::tempdir().unwrap();
        let h = History::open(&dir.path().join("h.sqlite")).unwrap();
        let t = |hh: u32, mm: u32| NaiveDate::from_ymd_opt(2026, 11, 26).unwrap().and_hms_opt(hh, mm, 0).unwrap();
        h.record_occupancy(t(10, 0), 5, 100);
        h.record_occupancy(t(10, 0), 7, 100); // mesmo intervalo: fica a última
        h.record_occupancy(t(10, 5), 30, 100);
        h.record_occupancy(t(11, 0), 12, 100);
        let all = h.occupancy(t(0, 0), false);
        assert_eq!(all.iter().map(|x| x.1).collect::<Vec<_>>(), [7, 30, 12]);
        let hourly = h.occupancy(t(0, 0), true);
        assert_eq!(hourly, [(t(10, 0), 30, 100), (t(11, 0), 12, 100)]);
        assert_eq!(h.occupancy(t(10, 30), false).len(), 1);
    }

    #[test]
    fn lembra_notificacoes_fechadas() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.sqlite");
        let h = History::open(&path).unwrap();
        h.set_dismissed("svc-stopped", "");
        h.set_dismissed("norange", "x");
        drop(h);
        let h = History::open(&path).unwrap();
        assert_eq!(h.dismissed().len(), 2);
        h.forget_dismissed_except(&["norange".to_string()]);
        assert_eq!(h.dismissed().get("norange").map(String::as_str), Some("x"));
        assert_eq!(h.dismissed().len(), 1);
    }
}
