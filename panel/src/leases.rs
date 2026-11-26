//! Arquivo de estado do Open DHCP Server (`-s`, padrão `/tmp/opendhcp.state`).
//!
//! É um vetor de `struct data8` (opendhcpd.h) gravado com `fwrite`. Layout em Linux x86_64/aarch64
//! (conferido com `sizeof`/`offsetof` no g++): 104 bytes por registro, little-endian.
//!
//! | offset | campo      | tipo        |
//! |--------|------------|-------------|
//! | 0      | dhcpInd    | u16         |
//! | 2      | bp_hlen    | u8          |
//! | 3      | local      | u8          |
//! | 4      | source     | u32         |
//! | 8      | ip         | u32 (ordem de rede) |
//! | 16     | expiry     | i64 (time_t) |
//! | 24     | bp_chaddr  | [u8; 16]    |
//! | 40     | hostname   | [u8; 64] (C string) |

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::Path;

pub const RECORD_SIZE: usize = 104;
/// O servidor grava `INT_MAX` como validade "infinita".
pub const INFINITE: i64 = i32::MAX as i64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub index: u16,
    pub mac: String,
    pub ip: Ipv4Addr,
    pub expiry: i64,
    pub hostname: String,
    pub local: bool,
}

impl Lease {
    pub fn is_active(&self, now: i64) -> bool {
        self.expiry >= now
    }
}

fn parse_record(r: &[u8]) -> Option<Lease> {
    let index = u16::from_le_bytes([r[0], r[1]]);
    let hlen = r[2] as usize;
    if hlen == 0 || hlen > 16 {
        return None;
    }
    let ip = Ipv4Addr::new(r[8], r[9], r[10], r[11]);
    let expiry = i64::from_le_bytes(r[16..24].try_into().ok()?);
    // O servidor grava bp_hlen = 16 mesmo para MAC de 6 bytes; usamos os bytes não nulos
    // até o comprimento real (6 para Ethernet).
    let chaddr = &r[24..24 + hlen];
    let len = if hlen == 16 && chaddr[6..].iter().all(|b| *b == 0) { 6 } else { hlen };
    let mac = chaddr[..len].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":");
    let host = &r[40..104];
    let end = host.iter().position(|b| *b == 0).unwrap_or(host.len());
    let hostname = String::from_utf8_lossy(&host[..end]).into_owned();
    Some(Lease { index, mac, ip, expiry, hostname, local: r[3] != 0 })
}

/// Lê todos os registros; registros com o mesmo MAC ficam só com o de maior índice.
pub fn parse(bytes: &[u8]) -> Vec<Lease> {
    let mut by_mac: HashMap<String, Lease> = HashMap::new();
    for chunk in bytes.chunks_exact(RECORD_SIZE) {
        if let Some(l) = parse_record(chunk) {
            match by_mac.get(&l.mac) {
                Some(old) if old.index > l.index => {}
                _ => {
                    by_mac.insert(l.mac.clone(), l);
                }
            }
        }
    }
    let mut out: Vec<Lease> = by_mac.into_values().collect();
    out.sort_by_key(|l| u32::from(l.ip));
    out
}

pub fn read(path: &Path) -> std::io::Result<Vec<Lease>> {
    match std::fs::read(path) {
        Ok(b) => Ok(parse(&b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

fn record_mac(r: &[u8]) -> Option<String> {
    parse_record(r).map(|l| l.mac)
}

/// Remove do arquivo todos os registros do MAC (só com o servidor parado: ele relê o arquivo ao
/// subir e o IP volta a ficar livre). Grava num temporário e renomeia. Retorna quantos removeu.
pub fn remove_mac(path: &Path, mac: &str) -> std::io::Result<usize> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut kept = Vec::with_capacity(bytes.len());
    let mut removed = 0;
    for chunk in bytes.chunks_exact(RECORD_SIZE) {
        if record_mac(chunk).as_deref() == Some(mac) {
            removed += 1;
        } else {
            kept.extend_from_slice(chunk);
        }
    }
    if removed > 0 {
        crate::fsutil::write_atomic(path, &kept)?;
    }
    Ok(removed)
}

/// Monta um registro no mesmo formato do servidor (usado nos testes).
#[cfg(test)]
pub fn encode(index: u16, mac: &str, ip: Ipv4Addr, expiry: i64, hostname: &str) -> [u8; RECORD_SIZE] {
    let mut r = [0u8; RECORD_SIZE];
    r[0..2].copy_from_slice(&index.to_le_bytes());
    r[2] = 16;
    r[3] = 1;
    r[8..12].copy_from_slice(&ip.octets());
    r[16..24].copy_from_slice(&expiry.to_le_bytes());
    for (i, p) in mac.split(':').take(16).enumerate() {
        r[24 + i] = u8::from_str_radix(p, 16).unwrap_or(0);
    }
    let h = hostname.as_bytes();
    let n = h.len().min(63);
    r[40..40 + n].copy_from_slice(&h[..n]);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_registro_no_formato_do_servidor() {
        let mut b = Vec::new();
        b.extend(encode(1, "00:41:42:41:42:00", Ipv4Addr::new(192, 168, 0, 10), 2_000_000_000, "pc-sala"));
        b.extend(encode(2, "00:41:42:41:42:00", Ipv4Addr::new(192, 168, 0, 11), 2_000_000_100, "pc-sala"));
        b.extend(encode(3, "aa:bb:cc:dd:ee:ff", Ipv4Addr::new(192, 168, 0, 5), INFINITE, ""));
        b.extend([0u8; 10]); // lixo no fim (registro incompleto) é ignorado
        let ls = parse(&b);
        assert_eq!(ls.len(), 2);
        assert_eq!(ls[0].ip, Ipv4Addr::new(192, 168, 0, 5));
        assert_eq!(ls[1].mac, "00:41:42:41:42:00");
        assert_eq!(ls[1].ip, Ipv4Addr::new(192, 168, 0, 11));
        assert_eq!(ls[1].hostname, "pc-sala");
    }

    #[test]
    fn remove_mac_regrava_sem_o_cliente() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("opendhcp.state");
        let mut b = Vec::new();
        b.extend(encode(1, "00:00:00:00:00:01", Ipv4Addr::new(10, 0, 0, 1), INFINITE, "a"));
        b.extend(encode(2, "00:00:00:00:00:02", Ipv4Addr::new(10, 0, 0, 2), INFINITE, "b"));
        std::fs::write(&p, &b).unwrap();
        assert_eq!(remove_mac(&p, "00:00:00:00:00:01").unwrap(), 1);
        let ls = read(&p).unwrap();
        assert_eq!(ls.len(), 1);
        assert_eq!(ls[0].hostname, "b");
    }
}
