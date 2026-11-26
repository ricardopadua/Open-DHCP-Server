//! Leitura e escrita do `opendhcp.ini` preservando comentários, linhas em branco e a ordem.
//!
//! Regras do Open DHCP Server: seções são `[NOME]`; qualquer linha que comece com pontuação
//! (`#`, `;`, ...) é comentário; o resto é `chave=valor`. Chaves são comparadas sem diferenciar
//! maiúsculas de minúsculas.

use std::net::Ipv4Addr;

#[derive(Debug, Clone)]
pub struct IniDoc {
    lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    /// Índice da linha `[NOME]`.
    pub header: usize,
    /// Índice (exclusivo) onde a seção termina: próximo cabeçalho ou fim do arquivo.
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub line: usize,
    pub key: String,
    pub value: String,
}

fn header_name(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.len() >= 2 && t.starts_with('[') && t.ends_with(']') {
        Some(t[1..t.len() - 1].trim())
    } else {
        None
    }
}

fn is_comment_or_blank(line: &str) -> bool {
    match line.trim_start().chars().next() {
        None => true,
        Some(c) => c.is_ascii_punctuation(),
    }
}

fn parse_entry(line: &str) -> Option<(String, String)> {
    if is_comment_or_blank(line) {
        return None;
    }
    let t = line.trim();
    match t.split_once('=') {
        Some((k, v)) => Some((k.trim().to_string(), v.trim().to_string())),
        // Linhas sem "=" (ex.: IPs em [LISTEN_ON]) viram entradas sem valor.
        None => Some((t.to_string(), String::new())),
    }
}

impl IniDoc {
    pub fn parse(text: &str) -> Self {
        let lines = text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect();
        Self { lines }
    }

    pub fn to_text(&self) -> String {
        let mut out = self.lines.join("\n");
        out.push('\n');
        out
    }

    pub fn sections(&self) -> Vec<Section> {
        let mut out: Vec<Section> = Vec::new();
        for (i, line) in self.lines.iter().enumerate() {
            if let Some(name) = header_name(line) {
                if let Some(last) = out.last_mut() {
                    last.end = i;
                }
                out.push(Section { name: name.to_string(), header: i, end: self.lines.len() });
            }
        }
        out
    }

    pub fn sections_named(&self, name: &str) -> Vec<Section> {
        self.sections().into_iter().filter(|s| s.name.eq_ignore_ascii_case(name)).collect()
    }

    pub fn entries(&self, sec: &Section) -> Vec<Entry> {
        (sec.header + 1..sec.end)
            .filter_map(|i| {
                parse_entry(&self.lines[i]).map(|(key, value)| Entry { line: i, key, value })
            })
            .collect()
    }

    pub fn values(&self, sec: &Section, key: &str) -> Vec<String> {
        self.entries(sec)
            .into_iter()
            .filter(|e| e.key.eq_ignore_ascii_case(key))
            .map(|e| e.value)
            .collect()
    }

    pub fn value(&self, sec: &Section, key: &str) -> Option<String> {
        self.values(sec, key).into_iter().next()
    }

    /// Troca todas as entradas `key` da seção por `values` (lista vazia remove a chave).
    /// Mantém a grafia original da chave e a posição da primeira ocorrência; se a chave não
    /// existir, insere depois da última linha não vazia da seção.
    pub fn set_values(&mut self, sec: &Section, key: &str, values: &[String]) {
        let existing: Vec<Entry> =
            self.entries(sec).into_iter().filter(|e| e.key.eq_ignore_ascii_case(key)).collect();
        let key_name = existing.first().map(|e| e.key.clone()).unwrap_or_else(|| key.to_string());
        let new_lines: Vec<String> = values.iter().map(|v| format!("{key_name}={v}")).collect();

        let insert_at = match existing.first() {
            Some(first) => first.line,
            None => {
                let mut pos = sec.end;
                while pos > sec.header + 1 && self.lines[pos - 1].trim().is_empty() {
                    pos -= 1;
                }
                pos
            }
        };
        // Remove de trás para frente para não deslocar os índices anteriores.
        for e in existing.iter().rev() {
            self.lines.remove(e.line);
        }
        for (n, l) in new_lines.into_iter().enumerate() {
            self.lines.insert(insert_at + n, l);
        }
    }

    pub fn set_value(&mut self, sec: &Section, key: &str, value: Option<&str>) {
        let values: Vec<String> = value.map(|v| vec![v.to_string()]).unwrap_or_default();
        self.set_values(sec, key, &values);
    }

    /// Remove a seção inteira (cabeçalho, entradas e comentários até o próximo cabeçalho).
    pub fn remove_section(&mut self, sec: &Section) {
        self.lines.drain(sec.header..sec.end);
    }

    /// Insere uma seção nova na posição `at` (índice de linha), com uma linha em branco antes.
    pub fn insert_section(&mut self, at: usize, name: &str, entries: &[(String, String)]) {
        let mut block = vec![String::new(), format!("[{name}]")];
        block.extend(entries.iter().map(|(k, v)| format!("{k}={v}")));
        let at = at.min(self.lines.len());
        for (n, l) in block.into_iter().enumerate() {
            self.lines.insert(at + n, l);
        }
    }

    /// Insere só a linha `[NOME]` (usado para renomear uma seção mantendo o conteúdo).
    pub fn insert_section_header(&mut self, at: usize, name: &str) {
        self.lines.insert(at.min(self.lines.len()), format!("[{name}]"));
    }

    pub fn line(&self, i: usize) -> Option<&str> {
        self.lines.get(i).map(String::as_str)
    }

    pub fn append_section(&mut self, name: &str, entries: &[(String, String)]) {
        let at = self.lines.len();
        self.insert_section(at, name, entries);
    }
}

/// Seções de cliente estático são nomeadas pelo MAC (`[00:41:42:41:42:00]`).
pub fn normalize_mac(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.trim().split([':', '-']).collect();
    if parts.len() != 6 {
        return None;
    }
    let mut out = Vec::with_capacity(6);
    for p in parts {
        if p.len() != 2 || !p.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        out.push(p.to_ascii_lowercase());
    }
    Some(out.join(":"))
}

pub fn parse_ip(s: &str) -> Option<Ipv4Addr> {
    s.trim().parse().ok()
}

/// `a.b.c.d-e.f.g.h`
pub fn parse_range(s: &str) -> Option<(Ipv4Addr, Ipv4Addr)> {
    let (a, b) = s.split_once('-')?;
    let (a, b) = (parse_ip(a)?, parse_ip(b)?);
    (u32::from(a) <= u32::from(b)).then_some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "#comentario\n[GLOBAL_OPTIONS]\n;Router=1.1.1.1\nSubNetMask=255.255.255.0\n\n[RANGE_SET]\nDHCPRange=10.0.0.1-10.0.0.9\n#x\nDHCPRange=10.0.1.1-10.0.1.9\n\n[00:41:42:41:42:00]\nIP=192.168.0.200\n";

    #[test]
    fn le_secoes_e_entradas_ignorando_comentarios() {
        let doc = IniDoc::parse(SAMPLE);
        let secs = doc.sections();
        assert_eq!(secs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["GLOBAL_OPTIONS", "RANGE_SET", "00:41:42:41:42:00"]);
        assert_eq!(doc.value(&secs[0], "router"), None);
        assert_eq!(doc.value(&secs[0], "subnetmask").as_deref(), Some("255.255.255.0"));
        assert_eq!(doc.values(&secs[1], "DHCPRange").len(), 2);
    }

    #[test]
    fn set_values_preserva_comentarios_e_grafia() {
        let mut doc = IniDoc::parse(SAMPLE);
        let sec = doc.sections_named("RANGE_SET")[0].clone();
        doc.set_values(&sec, "dhcprange", &["10.9.0.1-10.9.0.5".into()]);
        let sec = doc.sections_named("GLOBAL_OPTIONS")[0].clone();
        doc.set_value(&sec, "SubnetMask", Some("255.255.0.0"));
        let sec = doc.sections_named("GLOBAL_OPTIONS")[0].clone();
        doc.set_value(&sec, "Router", Some("10.9.0.254"));
        let text = doc.to_text();
        assert!(text.contains("DHCPRange=10.9.0.1-10.9.0.5\n#x\n"));
        assert!(!text.contains("10.0.1.1"));
        assert!(text.contains("SubNetMask=255.255.0.0"));
        assert!(text.contains(";Router=1.1.1.1\nSubNetMask=255.255.0.0\nRouter=10.9.0.254\n\n[RANGE_SET]"));
        assert!(text.starts_with("#comentario\n"));
    }

    #[test]
    fn remove_e_insere_secao() {
        let mut doc = IniDoc::parse(SAMPLE);
        let sec = doc.sections()[2].clone();
        doc.remove_section(&sec);
        doc.append_section("aa:bb:cc:dd:ee:ff", &[("IP".into(), "192.168.0.5".into())]);
        let secs = doc.sections();
        assert_eq!(secs.len(), 3);
        assert_eq!(secs[2].name, "aa:bb:cc:dd:ee:ff");
        assert_eq!(doc.value(&secs[2], "IP").as_deref(), Some("192.168.0.5"));
    }

    #[test]
    fn mac_e_faixa() {
        assert_eq!(normalize_mac("AA-BB-CC-00-11-22").as_deref(), Some("aa:bb:cc:00:11:22"));
        assert_eq!(normalize_mac("RANGE_SET"), None);
        assert!(parse_range("10.0.0.1-10.0.0.9").is_some());
        assert!(parse_range("10.0.0.9-10.0.0.1").is_none());
    }
}
