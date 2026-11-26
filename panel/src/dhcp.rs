//! Regras de negócio sobre o `opendhcp.ini`: reservas (clientes estáticos), conjuntos de faixas
//! (`[RANGE_SET]`) e opções globais. Toda entrada é validada antes de chegar ao arquivo.

use crate::tf;
use std::net::Ipv4Addr;

use crate::ini::{normalize_mac, parse_ip, parse_range, IniDoc, Section};

/// Limite do servidor (opendhcpd.h): 125 faixas no total.
pub const MAX_RANGES: usize = 125;

pub const KEY_RANGE: &str = "DHCPRange";
pub const KEY_MASK: &str = "SubnetMask";
pub const KEY_ROUTER: &str = "Router";
pub const KEY_DNS: &str = "DomainServer";
pub const KEY_LEASE: &str = "AddressTime";
pub const KEY_DOMAIN: &str = "DomainName";

#[derive(Debug, Clone)]
pub struct Reservation {
    pub n: usize,
    pub section: Section,
    pub mac: String,
    pub ip: Option<String>,
    pub hostname: Option<String>,
    /// Outras opções da seção (preservadas, só exibidas).
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub mask: Option<String>,
    pub router: Option<String>,
    pub dns: Option<String>,
    pub lease: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RangeSet {
    pub n: usize,
    #[allow(dead_code)] // usado nos testes e reservado para a tela de edição avançada
    pub section: Section,
    pub ranges: Vec<String>,
    pub opts: Options,
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct Globals {
    pub domain: Option<String>,
    pub opts: Options,
    pub extra: Vec<(String, String)>,
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn is_key(k: &str, names: &[&str]) -> bool {
    names.iter().any(|n| k.eq_ignore_ascii_case(n))
}

fn read_opts(doc: &IniDoc, sec: &Section) -> Options {
    Options {
        mask: doc.value(sec, KEY_MASK),
        router: doc.value(sec, KEY_ROUTER),
        dns: doc.value(sec, KEY_DNS),
        lease: doc.value(sec, KEY_LEASE),
    }
}

fn extras(doc: &IniDoc, sec: &Section, known: &[&str]) -> Vec<(String, String)> {
    doc.entries(sec).into_iter().filter(|e| !is_key(&e.key, known)).map(|e| (e.key, e.value)).collect()
}

pub fn reservations(doc: &IniDoc) -> Vec<Reservation> {
    doc.sections()
        .into_iter()
        .filter_map(|s| normalize_mac(&s.name).map(|mac| (s, mac)))
        .enumerate()
        .map(|(n, (s, mac))| Reservation {
            n,
            mac,
            ip: doc.value(&s, "IP").filter(|v| !v.is_empty()),
            hostname: doc.value(&s, "HostName").map(|v| unquote(&v)).filter(|v| !v.is_empty()),
            extra: extras(doc, &s, &["IP", "HostName"]),
            section: s,
        })
        .collect()
}

pub fn range_sets(doc: &IniDoc) -> Vec<RangeSet> {
    doc.sections_named("RANGE_SET")
        .into_iter()
        .enumerate()
        .map(|(n, s)| RangeSet {
            n,
            ranges: doc.values(&s, KEY_RANGE),
            opts: read_opts(doc, &s),
            extra: extras(doc, &s, &[KEY_RANGE, KEY_MASK, KEY_ROUTER, KEY_DNS, KEY_LEASE]),
            section: s,
        })
        .collect()
}

pub fn globals(doc: &IniDoc) -> Globals {
    match doc.sections_named("GLOBAL_OPTIONS").into_iter().next() {
        Some(s) => Globals {
            domain: doc.value(&s, KEY_DOMAIN).map(|v| unquote(&v)),
            opts: read_opts(doc, &s),
            extra: extras(doc, &s, &[KEY_DOMAIN, KEY_MASK, KEY_ROUTER, KEY_DNS, KEY_LEASE]),
        },
        None => Globals::default(),
    }
}

/// Todas as faixas configuradas, com o número do conjunto de origem.
pub fn all_ranges(doc: &IniDoc) -> Vec<(usize, Ipv4Addr, Ipv4Addr)> {
    range_sets(doc)
        .into_iter()
        .flat_map(|rs| {
            let n = rs.n;
            rs.ranges.into_iter().filter_map(move |r| parse_range(&r).map(|(a, b)| (n, a, b)))
        })
        .collect()
}

// ---------- validação de entrada ----------

fn clean(s: &str) -> Result<String, String> {
    let t = s.trim();
    if t.chars().any(|c| c.is_control()) {
        return Err(tf!("valor com caractere inválido (quebra de linha ou controle)"));
    }
    Ok(t.to_string())
}

fn opt_ip(label: &str, s: &str) -> Result<Option<String>, String> {
    let t = clean(s)?;
    if t.is_empty() {
        return Ok(None);
    }
    parse_ip(&t).map(|ip| Some(ip.to_string())).ok_or_else(|| tf!("{}: \"{}\" não é um IPv4 válido", label, t))
}

fn opt_ip_list(label: &str, s: &str) -> Result<Option<String>, String> {
    let t = clean(s)?;
    if t.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    for part in t.split([',', ' ', ';']).filter(|p| !p.is_empty()) {
        out.push(parse_ip(part).ok_or_else(|| tf!("{}: \"{}\" não é um IPv4 válido", label, part))?.to_string());
    }
    Ok(Some(out.join(", ")))
}

fn opt_mask(s: &str) -> Result<Option<String>, String> {
    let Some(m) = opt_ip(&tf!("Máscara"), s)? else { return Ok(None) };
    let bits = u32::from(m.parse::<Ipv4Addr>().unwrap());
    if bits.leading_ones() + bits.trailing_zeros() != 32 {
        return Err(tf!("Máscara: \"{}\" não é uma máscara de rede válida", m));
    }
    Ok(Some(m))
}

fn opt_lease(s: &str) -> Result<Option<String>, String> {
    let t = clean(s)?;
    if t.is_empty() {
        return Ok(None);
    }
    t.parse::<u32>()
        .map(|n| Some(n.to_string()))
        .map_err(|_| tf!("Tempo de lease: \"{}\" precisa ser um número de segundos (0 = infinito)", t))
}

fn valid_hostname(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 63
        && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !h.starts_with(['-', '.'])
}

pub struct OptionsInput<'a> {
    pub mask: &'a str,
    pub router: &'a str,
    pub dns: &'a str,
    pub lease: &'a str,
}

fn write_opts(doc: &mut IniDoc, sec_name: &str, nth: usize, o: &OptionsInput) -> Result<(), String> {
    let mask = opt_mask(o.mask)?;
    let router = opt_ip_list(&tf!("Gateway"), o.router)?;
    let dns = opt_ip_list("DNS", o.dns)?;
    let lease = opt_lease(o.lease)?;
    for (key, val) in [(KEY_MASK, mask), (KEY_ROUTER, router), (KEY_DNS, dns), (KEY_LEASE, lease)] {
        let sec = doc.sections_named(sec_name)[nth].clone();
        doc.set_value(&sec, key, val.as_deref());
    }
    Ok(())
}

// ---------- reservas ----------

pub struct ReservationInput<'a> {
    pub mac: &'a str,
    pub ip: &'a str,
    pub hostname: &'a str,
}

/// `editing`: número da reserva sendo editada (para não acusar duplicidade com ela mesma).
fn validate_reservation(
    doc: &IniDoc,
    input: &ReservationInput,
    editing: Option<usize>,
) -> Result<(String, Option<String>, Option<String>), String> {
    let mac = normalize_mac(&clean(input.mac)?).ok_or_else(|| tf!("MAC inválido: use o formato 00:11:22:33:44:55"))?;
    let ip = opt_ip("IP", input.ip)?;
    let hostname = clean(input.hostname)?;
    if !hostname.is_empty() && !valid_hostname(&hostname) {
        return Err(tf!("Nome do host: use só letras, números, ponto e hífen (até 63)"));
    }
    for r in reservations(doc) {
        if Some(r.n) == editing {
            continue;
        }
        if r.mac == mac {
            return Err(tf!("Já existe uma reserva para o MAC {}", mac));
        }
        if let Some(ipv) = ip.as_ref().filter(|v| r.ip.as_ref() == Some(*v)) {
            return Err(tf!("O IP {} já está reservado para {}", ipv, r.mac));
        }
    }
    Ok((mac, ip, (!hostname.is_empty()).then_some(hostname)))
}

pub fn add_reservation(doc: &mut IniDoc, input: &ReservationInput) -> Result<String, String> {
    let (mac, ip, hostname) = validate_reservation(doc, input, None)?;
    let mut entries = vec![("IP".to_string(), ip.unwrap_or_default())];
    if let Some(h) = hostname {
        entries.push(("HostName".to_string(), h));
    }
    doc.append_section(&mac, &entries);
    Ok(mac)
}

pub fn update_reservation(doc: &mut IniDoc, n: usize, input: &ReservationInput) -> Result<String, String> {
    let current = reservations(doc).into_iter().find(|r| r.n == n).ok_or_else(|| tf!("Reserva não encontrada"))?;
    let (mac, ip, hostname) = validate_reservation(doc, input, Some(n))?;
    if mac != current.mac {
        // Troca só o cabeçalho; as demais opções do cliente ficam.
        doc.remove_section(&Section { end: current.section.header + 1, ..current.section.clone() });
        doc.insert_section_header(current.section.header, &mac);
    }
    let sec = reservations(doc).into_iter().find(|r| r.n == n).ok_or_else(|| tf!("Reserva não encontrada"))?.section;
    doc.set_value(&sec, "IP", Some(ip.as_deref().unwrap_or("")));
    let sec = reservations(doc).into_iter().find(|r| r.n == n).unwrap().section;
    doc.set_value(&sec, "HostName", hostname.as_deref());
    Ok(mac)
}

pub fn delete_reservation(doc: &mut IniDoc, n: usize) -> Result<String, String> {
    let r = reservations(doc).into_iter().find(|r| r.n == n).ok_or_else(|| tf!("Reserva não encontrada"))?;
    doc.remove_section(&r.section);
    Ok(r.mac)
}

// ---------- sugestão de IP, importação e exportação (CSV) ----------

/// Primeiro IP livre para uma nova reserva, na rede da primeira faixa (máscara do conjunto ou
/// global; sem máscara, /24). Evita endereço de rede, broadcast, gateways, IPs reservados e os
/// que estão com lease agora (`in_use`). Prefere IPs fora das faixas dinâmicas; se não houver,
/// usa um livre dentro delas.
pub fn suggest_ip(doc: &IniDoc, in_use: &[Ipv4Addr]) -> Option<Ipv4Addr> {
    let sets = range_sets(doc);
    let set = sets.iter().find(|s| s.ranges.iter().any(|r| parse_range(r).is_some()))?;
    let (first, _) = set.ranges.iter().find_map(|r| parse_range(r))?;
    let g = globals(doc).opts;
    let pick = |own: &Option<String>, global: &Option<String>| own.clone().or_else(|| global.clone());
    let mask = pick(&set.opts.mask, &g.mask).and_then(|m| m.parse::<Ipv4Addr>().ok()).filter(|m| prefix_of(*m).is_some()).unwrap_or(mask_of(24));
    let gateways: Vec<Ipv4Addr> =
        pick(&set.opts.router, &g.router).map(|r| r.split([',', ' ', ';']).filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    let reserved: Vec<Ipv4Addr> = reservations(doc).iter().filter_map(|r| r.ip.as_ref()?.parse().ok()).collect();
    let dynamic = all_ranges(doc);
    let (net, bcast) = net_bounds(first, mask);
    let (lo, hi) = (u32::from(net).saturating_add(1), u32::from(bcast).saturating_sub(1));
    let free = |ip: Ipv4Addr| !gateways.contains(&ip) && !reserved.contains(&ip) && !in_use.contains(&ip);
    let in_dynamic = |ip: Ipv4Addr| dynamic.iter().any(|(_, a, b)| (u32::from(*a)..=u32::from(*b)).contains(&u32::from(ip)));
    let candidates = (lo..=hi).take(65536).map(Ipv4Addr::from);
    candidates.clone().find(|ip| free(*ip) && !in_dynamic(*ip)).or_else(|| candidates.clone().find(|ip| free(*ip)))
}

fn csv_field(s: &str) -> String {
    // planilhas executam células que começam com = + - @ (injeção de fórmula)
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        return csv_field(&format!("'{s}"));
    }
    if s.contains([',', ';', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Reservas em CSV: `mac,ip,hostname` (IP vazio = dispositivo bloqueado).
pub fn export_csv(doc: &IniDoc) -> String {
    let mut out = String::from("mac,ip,hostname\n");
    for r in reservations(doc) {
        out.push_str(&format!(
            "{},{},{}\n",
            r.mac,
            csv_field(r.ip.as_deref().unwrap_or("")),
            csv_field(r.hostname.as_deref().unwrap_or(""))
        ));
    }
    out
}

/// Separa uma linha de CSV (vírgula ou ponto e vírgula, aspas duplas opcionais).
fn csv_split(line: &str) -> Vec<String> {
    let sep = if line.contains(';') && !line.contains(',') { ';' } else { ',' };
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => out.push(std::mem::take(&mut cur).trim().to_string()),
            c => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

#[derive(Debug, Default, PartialEq)]
pub struct ImportResult {
    pub created: usize,
    pub updated: usize,
}

/// Importa reservas de um CSV (`mac,ip,hostname`; cabeçalho opcional). Tudo ou nada: com um
/// erro em qualquer linha, nada é alterado. Com `update`, um MAC que já tem reserva é atualizado
/// em vez de recusado.
pub fn import_csv(doc: &mut IniDoc, text: &str, update: bool) -> Result<ImportResult, String> {
    let mut work = doc.clone();
    let mut res = ImportResult::default();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols = csv_split(line);
        if cols.first().is_some_and(|c| c.eq_ignore_ascii_case("mac")) {
            continue;
        }
        let get = |k: usize| cols.get(k).map(String::as_str).unwrap_or("");
        let input = ReservationInput { mac: get(0), ip: get(1), hostname: get(2) };
        let existing = normalize_mac(get(0)).and_then(|m| reservations(&work).into_iter().find(|r| r.mac == m));
        let result = match existing {
            Some(r) if update => update_reservation(&mut work, r.n, &input).map(|_| res.updated += 1),
            _ => add_reservation(&mut work, &input).map(|_| res.created += 1),
        };
        result.map_err(|e| tf!("Linha {}: {}", i + 1, e))?;
    }
    if res.created + res.updated == 0 {
        return Err(tf!("Nenhuma reserva no texto. Use uma linha por dispositivo: mac,ip,nome"));
    }
    *doc = work;
    Ok(res)
}

// ---------- rede (máscara, prefixo, sub-rede) ----------

/// Máscara do prefixo `/p` (0 a 32).
pub fn mask_of(prefix: u8) -> Ipv4Addr {
    Ipv4Addr::from(if prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(prefix.min(32))) })
}

/// Prefixo de uma máscara contínua (`255.255.255.0` → 24).
pub fn prefix_of(mask: Ipv4Addr) -> Option<u8> {
    let bits = u32::from(mask);
    (bits.leading_ones() + bits.trailing_zeros() == 32).then(|| bits.leading_ones() as u8)
}

/// (rede, broadcast) do IP com a máscara.
pub fn net_bounds(ip: Ipv4Addr, mask: Ipv4Addr) -> (Ipv4Addr, Ipv4Addr) {
    let (i, m) = (u32::from(ip), u32::from(mask));
    (Ipv4Addr::from(i & m), Ipv4Addr::from((i & m) | !m))
}

/// Confere as faixas contra a máscara (cabem na mesma rede, sem o endereço de rede nem o
/// broadcast) e os gateways (dentro da rede e fora das faixas, para não serem entregues).
fn check_network(ranges: &[(Ipv4Addr, Ipv4Addr)], mask: Option<Ipv4Addr>, routers: &[Ipv4Addr]) -> Result<(), String> {
    let Some(mask) = mask else { return Ok(()) };
    let Some(prefix) = prefix_of(mask) else { return Ok(()) };
    for (a, b) in ranges {
        let (net, bcast) = net_bounds(*a, mask);
        if net_bounds(*b, mask).0 != net {
            return Err(tf!("A faixa {}-{} não cabe na rede {}/{} (máscara {}).", a, b, net, prefix, mask));
        }
        if prefix <= 30 && *a == net {
            return Err(tf!("A faixa {}-{} começa no endereço de rede {}, que não pode ser entregue.", a, b, net));
        }
        if prefix <= 30 && *b == bcast {
            return Err(tf!("A faixa {}-{} termina no broadcast {}, que não pode ser entregue.", a, b, bcast));
        }
    }
    if let Some((first, _)) = ranges.first() {
        let (net, _) = net_bounds(*first, mask);
        for r in routers {
            if net_bounds(*r, mask).0 != net {
                return Err(tf!("O gateway {} está fora da rede {}/{} das faixas.", r, net, prefix));
            }
            if let Some((a, b)) = ranges.iter().find(|(a, b)| (u32::from(*a)..=u32::from(*b)).contains(&u32::from(*r))) {
                return Err(tf!("O gateway {} está dentro da faixa {}-{}: ele poderia ser entregue a um dispositivo.", r, a, b));
            }
        }
    }
    Ok(())
}

/// Máscara e gateways que valerão para o conjunto: os dele ou, sem eles, os das opções globais.
fn effective_net(doc: &IniDoc, o: &OptionsInput) -> Result<(Option<Ipv4Addr>, Vec<Ipv4Addr>), String> {
    let g = globals(doc).opts;
    let mask = match opt_mask(o.mask)? {
        Some(m) => Some(m),
        None => g.mask.as_deref().and_then(|m| opt_mask(m).ok().flatten()),
    };
    let routers = match opt_ip_list(&tf!("Gateway"), o.router)? {
        Some(r) => Some(r),
        None => g.router.as_deref().and_then(|r| opt_ip_list("", r).ok().flatten()),
    };
    let routers = routers.map(|r| r.split(", ").filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    Ok((mask.and_then(|m| m.parse().ok()), routers))
}

// ---------- faixas ----------

pub struct RangeSetInput<'a> {
    pub ranges: &'a str,
    pub opts: OptionsInput<'a>,
}

fn parse_ranges_text(text: &str) -> Result<Vec<(Ipv4Addr, Ipv4Addr)>, String> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let line = clean(line)?;
        let r = parse_range(&line.replace(' ', "")).ok_or_else(|| {
            tf!("Faixa \"{}\" inválida: use INÍCIO-FIM, por exemplo 192.168.0.100-192.168.0.200", line)
        })?;
        out.push(r);
    }
    if out.is_empty() {
        return Err(tf!("Informe ao menos uma faixa"));
    }
    Ok(out)
}

fn check_overlaps(doc: &IniDoc, new: &[(Ipv4Addr, Ipv4Addr)], editing: Option<usize>) -> Result<(), String> {
    let others: Vec<(Ipv4Addr, Ipv4Addr)> =
        all_ranges(doc).into_iter().filter(|(n, ..)| Some(*n) != editing).map(|(_, a, b)| (a, b)).collect();
    if others.len() + new.len() > MAX_RANGES {
        return Err(tf!("O servidor aceita no máximo {} faixas no total", MAX_RANGES));
    }
    let all: Vec<(Ipv4Addr, Ipv4Addr)> = others.iter().chain(new.iter()).copied().collect();
    for (i, (a1, b1)) in new.iter().enumerate() {
        for (j, (a2, b2)) in all.iter().enumerate() {
            if j == others.len() + i {
                continue;
            }
            if u32::from(*a1) <= u32::from(*b2) && u32::from(*a2) <= u32::from(*b1) {
                return Err(tf!("A faixa {}-{} se sobrepõe a {}-{}", a1, b1, a2, b2));
            }
        }
    }
    Ok(())
}

pub fn add_range_set(doc: &mut IniDoc, input: &RangeSetInput) -> Result<usize, String> {
    let ranges = parse_ranges_text(input.ranges)?;
    check_overlaps(doc, &ranges, None)?;
    let (mask, routers) = effective_net(doc, &input.opts)?;
    check_network(&ranges, mask, &routers)?;
    let sets = doc.sections_named("RANGE_SET");
    // Fica junto dos outros [RANGE_SET] (a ordem importa: o primeiro completa opções de clientes
    // estáticos); sem nenhum, antes de [GLOBAL_OPTIONS] ou do primeiro cliente estático.
    let at = match sets.last() {
        Some(last) => {
            let mut end = last.end;
            while end > last.header + 1 && doc_line_blank(doc, end - 1) {
                end -= 1;
            }
            end
        }
        None => doc
            .sections()
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case("GLOBAL_OPTIONS") || normalize_mac(&s.name).is_some())
            .map(|s| s.header)
            .unwrap_or(usize::MAX),
    };
    let entries: Vec<(String, String)> =
        ranges.iter().map(|(a, b)| (KEY_RANGE.to_string(), format!("{a}-{b}"))).collect();
    doc.insert_section(at, "RANGE_SET", &entries);
    let n = sets.len();
    write_opts(doc, "RANGE_SET", n, &input.opts)?;
    Ok(n)
}

fn doc_line_blank(doc: &IniDoc, i: usize) -> bool {
    doc.line(i).is_some_and(|l| l.trim().is_empty())
}

pub fn update_range_set(doc: &mut IniDoc, n: usize, input: &RangeSetInput) -> Result<(), String> {
    let sec = doc.sections_named("RANGE_SET").get(n).cloned().ok_or_else(|| tf!("Conjunto de faixas não encontrado"))?;
    let ranges = parse_ranges_text(input.ranges)?;
    check_overlaps(doc, &ranges, Some(n))?;
    let (mask, routers) = effective_net(doc, &input.opts)?;
    check_network(&ranges, mask, &routers)?;
    let values: Vec<String> = ranges.iter().map(|(a, b)| format!("{a}-{b}")).collect();
    doc.set_values(&sec, KEY_RANGE, &values);
    write_opts(doc, "RANGE_SET", n, &input.opts)
}

pub fn delete_range_set(doc: &mut IniDoc, n: usize) -> Result<(), String> {
    let sec = doc.sections_named("RANGE_SET").get(n).cloned().ok_or_else(|| tf!("Conjunto de faixas não encontrado"))?;
    doc.remove_section(&sec);
    Ok(())
}

// ---------- opções globais ----------

pub fn update_globals(doc: &mut IniDoc, domain: &str, opts: &OptionsInput) -> Result<(), String> {
    let domain = clean(domain)?;
    if !domain.is_empty() && !valid_hostname(&domain) {
        return Err(tf!("Domínio: use só letras, números, ponto e hífen"));
    }
    if doc.sections_named("GLOBAL_OPTIONS").is_empty() {
        let at = doc.sections().into_iter().find(|s| normalize_mac(&s.name).is_some()).map(|s| s.header);
        doc.insert_section(at.unwrap_or(usize::MAX), "GLOBAL_OPTIONS", &[]);
    }
    let sec = doc.sections_named("GLOBAL_OPTIONS")[0].clone();
    let quoted = (!domain.is_empty()).then(|| format!("\"{domain}\""));
    doc.set_value(&sec, KEY_DOMAIN, quoted.as_deref());
    write_opts(doc, "GLOBAL_OPTIONS", 0, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> IniDoc {
        IniDoc::parse(include_str!("../../opendhcp.ini"))
    }

    #[test]
    fn sugere_ip_livre_fora_das_faixas() {
        let mut doc = IniDoc::parse("[RANGE_SET]\nDHCPRange=192.168.0.100-192.168.0.200\nSubnetMask=255.255.255.0\nRouter=192.168.0.1\n\n[00:00:00:00:00:01]\nIP=192.168.0.2\n");
        let used: Vec<Ipv4Addr> = vec!["192.168.0.3".parse().unwrap()];
        assert_eq!(suggest_ip(&doc, &used), Some("192.168.0.4".parse().unwrap()));
        // rede cheia fora das faixas: usa uma livre dentro
        doc = IniDoc::parse("[RANGE_SET]\nDHCPRange=10.0.0.1-10.0.0.6\nSubnetMask=255.255.255.248\n");
        assert_eq!(suggest_ip(&doc, &["10.0.0.1".parse().unwrap()]), Some("10.0.0.2".parse().unwrap()));
        assert_eq!(suggest_ip(&IniDoc::parse(""), &[]), None);
    }

    #[test]
    fn importa_e_exporta_csv() {
        let mut doc = IniDoc::parse("[RANGE_SET]\nDHCPRange=10.0.0.100-10.0.0.200\n");
        let csv = "mac;ip;hostname\nAA-BB-CC-DD-EE-01;10.0.0.10;impressora\n\"aa:bb:cc:dd:ee:02\";;\n";
        assert_eq!(import_csv(&mut doc, csv, false).unwrap(), ImportResult { created: 2, updated: 0 });
        let out = export_csv(&doc);
        assert!(out.starts_with("mac,ip,hostname\naa:bb:cc:dd:ee:01,10.0.0.10,impressora\naa:bb:cc:dd:ee:02,,\n"), "{out}");
        // já existe: sem `update` recusa e não muda nada
        let before = doc.to_text();
        let err = import_csv(&mut doc, "aa:bb:cc:dd:ee:03,10.0.0.11,\naa:bb:cc:dd:ee:01,10.0.0.12,", false).unwrap_err();
        assert!(err.starts_with("Linha 2:"), "{err}");
        assert_eq!(doc.to_text(), before, "tudo ou nada");
        let r = import_csv(&mut doc, "aa:bb:cc:dd:ee:01,10.0.0.12,nova", true).unwrap();
        assert_eq!(r, ImportResult { created: 0, updated: 1 });
        assert!(export_csv(&doc).contains("aa:bb:cc:dd:ee:01,10.0.0.12,nova"));
        assert!(import_csv(&mut doc, "\n# só comentário\n", false).is_err());
    }

    #[test]
    fn mascara_prefixo_e_rede() {
        assert_eq!(mask_of(24), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(mask_of(26), Ipv4Addr::new(255, 255, 255, 192));
        assert_eq!(mask_of(32), Ipv4Addr::new(255, 255, 255, 255));
        assert_eq!(mask_of(0), Ipv4Addr::new(0, 0, 0, 0));
        assert_eq!(prefix_of(Ipv4Addr::new(255, 255, 255, 192)), Some(26));
        assert_eq!(prefix_of(Ipv4Addr::new(255, 0, 255, 0)), None);
        let (net, b) = net_bounds(Ipv4Addr::new(192, 168, 0, 70), mask_of(26));
        assert_eq!((net, b), (Ipv4Addr::new(192, 168, 0, 64), Ipv4Addr::new(192, 168, 0, 127)));
    }

    #[test]
    fn faixa_precisa_caber_na_rede_e_gateway_fora_dela() {
        let r = |a: &str, b: &str| (a.parse().unwrap(), b.parse().unwrap());
        let m = Some(mask_of(26));
        let gw: Vec<Ipv4Addr> = vec!["192.168.0.65".parse().unwrap()];
        assert!(check_network(&[r("192.168.0.66", "192.168.0.126")], m, &gw).is_ok());
        assert!(check_network(&[r("192.168.0.66", "192.168.0.130")], m, &gw).unwrap_err().contains("não cabe"));
        assert!(check_network(&[r("192.168.0.64", "192.168.0.100")], m, &[]).unwrap_err().contains("endereço de rede"));
        assert!(check_network(&[r("192.168.0.66", "192.168.0.127")], m, &[]).unwrap_err().contains("broadcast"));
        assert!(check_network(&[r("192.168.0.60", "192.168.0.70")], None, &[]).is_ok(), "sem máscara não confere");
        let gw_in: Vec<Ipv4Addr> = vec!["192.168.0.70".parse().unwrap()];
        assert!(check_network(&[r("192.168.0.66", "192.168.0.126")], m, &gw_in).unwrap_err().contains("dentro da faixa"));
        let gw_out: Vec<Ipv4Addr> = vec!["10.0.0.1".parse().unwrap()];
        assert!(check_network(&[r("192.168.0.66", "192.168.0.126")], m, &gw_out).unwrap_err().contains("fora da rede"));
        // /31 e /32: todos os endereços valem
        assert!(check_network(&[r("10.0.0.0", "10.0.0.1")], Some(mask_of(31)), &[]).is_ok());
    }

    fn opts<'a>() -> OptionsInput<'a> {
        OptionsInput { mask: "", router: "", dns: "", lease: "" }
    }

    #[test]
    fn le_o_ini_de_exemplo_do_projeto() {
        let doc = sample();
        let rs = reservations(&doc);
        assert_eq!(rs.len(), 4);
        assert_eq!(rs[0].mac, "00:41:42:41:42:00");
        assert_eq!(rs[0].ip.as_deref(), Some("192.168.0.200"));
        assert_eq!(rs[3].ip, None);
        let sets = range_sets(&doc);
        assert_eq!(sets.len(), 3);
        assert_eq!(sets[0].ranges, ["192.168.0.1-192.168.0.254"]);
        assert!(sets[1].ranges.is_empty());
    }

    #[test]
    fn cria_edita_e_apaga_reserva() {
        let mut doc = sample();
        let err = add_reservation(&mut doc, &ReservationInput { mac: "00-41-42-41-42-00", ip: "10.0.0.1", hostname: "" });
        assert!(err.unwrap_err().contains("Já existe"));
        let err = add_reservation(&mut doc, &ReservationInput { mac: "aa:bb:cc:dd:ee:01", ip: "192.168.0.200", hostname: "" });
        assert!(err.unwrap_err().contains("já está reservado"));
        let err = add_reservation(&mut doc, &ReservationInput { mac: "aa:bb:cc:dd:ee:01", ip: "10.0.0.1", hostname: "x\n[GLOBAL_OPTIONS]" });
        assert!(err.is_err());

        add_reservation(&mut doc, &ReservationInput { mac: "AA:BB:CC:DD:EE:01", ip: "192.168.0.50", hostname: "impressora" }).unwrap();
        let r = reservations(&doc).pop().unwrap();
        assert_eq!((r.mac.as_str(), r.ip.as_deref(), r.hostname.as_deref()), ("aa:bb:cc:dd:ee:01", Some("192.168.0.50"), Some("impressora")));

        update_reservation(&mut doc, 1, &ReservationInput { mac: "00:41:42:41:42:06", ip: "192.168.0.212", hostname: "" }).unwrap();
        let r = &reservations(&doc)[1];
        assert_eq!((r.mac.as_str(), r.ip.as_deref()), ("00:41:42:41:42:06", Some("192.168.0.212")));
        // comentários da seção continuam lá
        assert!(doc.to_text().contains("[00:41:42:41:42:06]\n#This is a client with MAC addr 00:41:42:41:42:05\nIP=192.168.0.212"));

        let before = reservations(&doc).len();
        delete_reservation(&mut doc, 1).unwrap();
        assert_eq!(reservations(&doc).len(), before - 1);
        assert_eq!(range_sets(&doc).len(), 3);
    }

    #[test]
    fn faixas_validam_sobreposicao_e_ficam_juntas() {
        let mut doc = sample();
        let err = add_range_set(&mut doc, &RangeSetInput { ranges: "192.168.0.250-192.168.1.10", opts: opts() });
        assert!(err.unwrap_err().contains("sobrepõe"));
        let n = add_range_set(
            &mut doc,
            &RangeSetInput {
                ranges: "10.1.0.10-10.1.0.200\n",
                opts: OptionsInput { mask: "255.255.255.0", router: "10.1.0.1", dns: "1.1.1.1 8.8.8.8", lease: "3600" },
            },
        )
        .unwrap();
        assert_eq!(n, 3);
        let sets = range_sets(&doc);
        assert_eq!(sets[3].ranges, ["10.1.0.10-10.1.0.200"]);
        assert_eq!(sets[3].opts.dns.as_deref(), Some("1.1.1.1, 8.8.8.8"));
        // o novo conjunto fica antes de [GLOBAL_OPTIONS]
        let names: Vec<String> = doc.sections().into_iter().map(|s| s.name).collect();
        let pos_new = sets[3].section.header;
        let pos_global = doc.sections_named("GLOBAL_OPTIONS")[0].header;
        assert!(pos_new < pos_global, "{names:?}");

        assert!(update_range_set(&mut doc, 0, &RangeSetInput { ranges: "1.1.1.1-1.1.1.0", opts: opts() }).is_err());
        let err = update_range_set(&mut doc, 0, &RangeSetInput { ranges: "x", opts: OptionsInput { mask: "255.0.255.0", ..opts() } });
        assert!(err.is_err());
        update_range_set(&mut doc, 0, &RangeSetInput { ranges: "192.168.0.10-192.168.0.99", opts: OptionsInput { mask: "255.255.255.0", ..opts() } }).unwrap();
        assert_eq!(range_sets(&doc)[0].opts.mask.as_deref(), Some("255.255.255.0"));
        delete_range_set(&mut doc, 3).unwrap();
        assert_eq!(range_sets(&doc).len(), 3);
    }

    #[test]
    fn opcoes_globais() {
        let mut doc = sample();
        update_globals(&mut doc, "empresa.local", &OptionsInput { mask: "255.255.255.0", router: "192.168.0.1", dns: "", lease: "36000" }).unwrap();
        let g = globals(&doc);
        assert_eq!(g.domain.as_deref(), Some("empresa.local"));
        assert_eq!(g.opts.router.as_deref(), Some("192.168.0.1"));
        // a grafia original (SubNetMask) não é ativa no exemplo, então entra a nova
        assert!(doc.to_text().contains("DomainName=\"empresa.local\""));
        assert!(update_globals(&mut doc, "a b", &opts()).is_err());
    }
}
