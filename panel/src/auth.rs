use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand::RngCore;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("falha ao gerar hash: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

/// Compara sem curto-circuito (tokens CSRF).
pub fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn random_token() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[derive(Debug, Clone)]
pub struct Flash {
    pub ok: bool,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub user: String,
    pub csrf: String,
    pub expires: Instant,
    pub flash: Option<Flash>,
}

const MAX_FAILS: u32 = 5;
const LOCK: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct Sessions {
    map: Mutex<HashMap<String, Session>>,
    fails: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl Sessions {
    pub fn create(&self, user: &str, ttl: Duration) -> String {
        let token = random_token();
        let s = Session { user: user.to_string(), csrf: random_token(), expires: Instant::now() + ttl, flash: None };
        let mut map = self.map.lock().unwrap();
        let now = Instant::now();
        map.retain(|_, s| s.expires > now);
        map.insert(token.clone(), s);
        token
    }

    pub fn get(&self, token: &str) -> Option<Session> {
        let mut map = self.map.lock().unwrap();
        match map.get_mut(token) {
            Some(s) if s.expires > Instant::now() => {
                let out = s.clone();
                s.flash = None; // a mensagem aparece uma vez só
                Some(out)
            }
            Some(_) => {
                map.remove(token);
                None
            }
            None => None,
        }
    }

    /// Sessão válida, sem consumir a mensagem pendente (usado pelo fluxo ao vivo).
    pub fn is_valid(&self, token: &str) -> bool {
        self.map.lock().unwrap().get(token).is_some_and(|s| s.expires > Instant::now())
    }

    pub fn set_flash(&self, token: &str, ok: bool, text: impl Into<String>) {
        if let Some(s) = self.map.lock().unwrap().get_mut(token) {
            s.flash = Some(Flash { ok, text: text.into() });
        }
    }

    pub fn remove(&self, token: &str) {
        self.map.lock().unwrap().remove(token);
    }

    pub fn locked_out(&self, ip: IpAddr) -> bool {
        let mut fails = self.fails.lock().unwrap();
        match fails.get(&ip) {
            Some((n, since)) if *n >= MAX_FAILS && since.elapsed() < LOCK => true,
            Some((_, since)) if since.elapsed() >= LOCK => {
                fails.remove(&ip);
                false
            }
            _ => false,
        }
    }

    pub fn record_fail(&self, ip: IpAddr) {
        let mut fails = self.fails.lock().unwrap();
        let e = fails.entry(ip).or_insert((0, Instant::now()));
        e.0 += 1;
        e.1 = Instant::now();
    }

    pub fn clear_fails(&self, ip: IpAddr) {
        self.fails.lock().unwrap().remove(&ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_e_verificacao() {
        let h = hash_password("segredo").unwrap();
        assert!(verify_password("segredo", &h));
        assert!(!verify_password("outra", &h));
        assert!(!verify_password("segredo", "lixo"));
    }

    #[test]
    fn bloqueio_apos_falhas() {
        let s = Sessions::default();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..MAX_FAILS {
            assert!(!s.locked_out(ip));
            s.record_fail(ip);
        }
        assert!(s.locked_out(ip));
        s.clear_fails(ip);
        assert!(!s.locked_out(ip));
    }

    #[test]
    fn flash_aparece_uma_vez() {
        let s = Sessions::default();
        let t = s.create("admin", Duration::from_secs(60));
        s.set_flash(&t, true, "ok");
        assert!(s.get(&t).unwrap().flash.is_some());
        assert!(s.get(&t).unwrap().flash.is_none());
    }
}
