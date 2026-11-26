//! Dados do painel sobre os dispositivos, fora do `opendhcp.ini`:
//!
//! - apelido e anotações por MAC, num JSON próprio (`<data_dir>/devices.json`);
//! - fabricante pelo MAC (OUI), lido da lista do IEEE instalada no sistema (pacote `hwdata` ou
//!   `ieee-data`) ou do `manuf` do Wireshark. Sem a lista, o painel só não mostra o fabricante.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alias: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl DeviceInfo {
    fn is_empty(&self) -> bool {
        self.alias.is_empty() && self.notes.is_empty()
    }
}

pub const MAX_ALIAS: usize = 64;
pub const MAX_NOTES: usize = 1000;

/// Apelidos e anotações (MAC normalizado → dados), gravados a cada alteração.
pub struct DeviceStore {
    path: PathBuf,
    map: Mutex<BTreeMap<String, DeviceInfo>>,
}

impl DeviceStore {
    /// Lê o arquivo (se não existir, começa vazio). Um arquivo corrompido é guardado ao lado
    /// com `.bad` para não ser sobrescrito sem ninguém ver.
    pub fn open(path: PathBuf) -> Self {
        let map = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::error!(file = %path.display(), error = %e, "devices.json inválido; começando vazio");
                let _ = std::fs::copy(&path, path.with_extension("json.bad"));
                BTreeMap::new()
            }),
            Err(_) => BTreeMap::new(),
        };
        Self { path, map: Mutex::new(map) }
    }

    pub fn get(&self, mac: &str) -> DeviceInfo {
        self.map.lock().unwrap().get(mac).cloned().unwrap_or_default()
    }

    #[cfg(test)]
    pub fn all(&self) -> BTreeMap<String, DeviceInfo> {
        self.map.lock().unwrap().clone()
    }

    pub fn alias(&self, mac: &str) -> Option<String> {
        self.map.lock().unwrap().get(mac).map(|d| d.alias.clone()).filter(|a| !a.is_empty())
    }

    /// Grava (ou apaga, se vier vazio) e salva o arquivo.
    pub fn set(&self, mac: &str, info: DeviceInfo) -> std::io::Result<()> {
        let mut map = self.map.lock().unwrap();
        let before = map.clone();
        if info.is_empty() {
            map.remove(mac);
        } else {
            map.insert(mac.to_string(), info);
        }
        let json = serde_json::to_vec_pretty(&*map).map_err(std::io::Error::other)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if let Err(e) = crate::fsutil::write_atomic(&self.path, &json) {
            *map = before;
            return Err(e);
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Fabricante de um MAC.
#[derive(Debug, Clone, PartialEq)]
pub enum Vendor {
    Known(String),
    /// Bit "administrado localmente": MAC aleatório (privacidade de celulares e notebooks) ou
    /// de máquina virtual; não identifica fabricante.
    Random,
}

#[derive(Default)]
pub struct Oui {
    map: HashMap<u32, String>,
}

/// Onde as distribuições costumam instalar a lista.
pub const OUI_PATHS: [&str; 4] =
    ["/usr/share/hwdata/oui.txt", "/usr/share/ieee-data/oui.txt", "/usr/share/misc/oui.txt", "/usr/share/wireshark/manuf"];

impl Oui {
    pub fn load(configured: Option<&Path>) -> Self {
        let candidates: Vec<PathBuf> = match configured {
            Some(p) => vec![p.to_path_buf()],
            None => OUI_PATHS.iter().map(PathBuf::from).collect(),
        };
        for path in candidates {
            if let Ok(text) = std::fs::read(&path) {
                let text = String::from_utf8_lossy(&text);
                let map = parse_oui(&text);
                if !map.is_empty() {
                    tracing::info!(file = %path.display(), vendors = map.len(), "lista de fabricantes (OUI) carregada");
                    return Self { map };
                }
            }
        }
        tracing::info!("nenhuma lista de fabricantes (OUI) encontrada; instale o pacote hwdata ou ieee-data para ver o fabricante");
        Self::default()
    }

    pub fn vendor(&self, mac: &str) -> Option<Vendor> {
        let bytes: Vec<u8> = mac.split(':').filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
        if bytes.len() < 3 {
            return None;
        }
        if bytes[0] & 0x02 != 0 {
            return Some(Vendor::Random);
        }
        let key = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        self.map.get(&key).map(|v| Vendor::Known(v.clone()))
    }
}

fn hex3(s: &str) -> Option<u32> {
    let h: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (h.len() == 6).then(|| u32::from_str_radix(&h, 16).ok()).flatten()
}

/// Aceita o `oui.txt` do IEEE (`28-6F-B9   (hex)\t\tNome`) e o `manuf` do Wireshark
/// (`00:00:0C\tCisco\tCisco Systems, Inc`; prefixos maiores que 24 bits são ignorados).
fn parse_oui(text: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        if let Some((prefix, name)) = line.split_once("(hex)") {
            if let Some(k) = hex3(prefix.trim()) {
                let name = name.trim();
                if !name.is_empty() {
                    map.insert(k, name.to_string());
                }
            }
        } else if !line.starts_with('#') {
            let mut cols = line.split('\t');
            let (Some(prefix), Some(short)) = (cols.next(), cols.next()) else { continue };
            if prefix.contains('/') {
                continue;
            }
            if let Some(k) = hex3(prefix) {
                let name = cols.next().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(short.trim());
                map.insert(k, name.to_string());
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_os_dois_formatos_de_lista() {
        let ieee = "OUI/MA-L\t\tOrganization\n\n28-6F-B9   (hex)\t\tNokia Shanghai Bell Co., Ltd.\n286FB9     (base 16)\t\tNokia\n";
        let manuf = "# comentário\n00:00:0C\tCisco\tCisco Systems, Inc\n00:1B:C5:00:00:00/36\tX\tY\nFC:FB:FB\tCisco\n";
        let m = parse_oui(ieee);
        assert_eq!(m.get(&0x286FB9).map(String::as_str), Some("Nokia Shanghai Bell Co., Ltd."));
        let m = parse_oui(manuf);
        assert_eq!(m.get(&0x00000C).map(String::as_str), Some("Cisco Systems, Inc"));
        assert_eq!(m.get(&0xFCFBFB).map(String::as_str), Some("Cisco"));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn fabricante_e_mac_aleatorio() {
        let oui = Oui { map: parse_oui("28-6F-B9   (hex)\t\tNokia\n") };
        assert_eq!(oui.vendor("28:6f:b9:00:00:01"), Some(Vendor::Known("Nokia".into())));
        assert_eq!(oui.vendor("da:a1:19:00:00:01"), Some(Vendor::Random));
        assert_eq!(oui.vendor("00:11:22:33:44:55"), None);
    }

    #[test]
    fn apelido_grava_e_apaga() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/devices.json");
        let store = DeviceStore::open(path.clone());
        store.set("aa:bb:cc:dd:ee:01", DeviceInfo { alias: "Impressora".into(), notes: "RH".into() }).unwrap();
        let again = DeviceStore::open(path.clone());
        assert_eq!(again.alias("aa:bb:cc:dd:ee:01").as_deref(), Some("Impressora"));
        again.set("aa:bb:cc:dd:ee:01", DeviceInfo::default()).unwrap();
        assert!(DeviceStore::open(path).all().is_empty());
    }
}
