//! Diferença linha a linha entre dois textos (maior subsequência comum), para comparar um backup
//! do `.ini` com o arquivo atual.

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// Igual nos dois (número da linha em `a`, em `b`, texto).
    Same(usize, usize, String),
    /// Só em `a`.
    Del(usize, String),
    /// Só em `b`.
    Add(usize, String),
}

/// Acima disso (linhas × linhas) a tabela da LCS ficaria grande demais: mostra tudo como trocado.
const MAX_CELLS: usize = 4_000_000;

pub fn lines(a: &str, b: &str) -> Vec<Line> {
    let a: Vec<&str> = a.lines().collect();
    let b: Vec<&str> = b.lines().collect();
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_CELLS {
        let mut out: Vec<Line> = a.iter().enumerate().map(|(i, l)| Line::Del(i + 1, l.to_string())).collect();
        out.extend(b.iter().enumerate().map(|(j, l)| Line::Add(j + 1, l.to_string())));
        return out;
    }
    // lcs[i][j] = tamanho da maior subsequência comum de a[i..] e b[j..]
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(n.max(m));
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push(Line::Same(i + 1, j + 1, a[i].to_string()));
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out.push(Line::Add(j + 1, b[j].to_string()));
            j += 1;
        } else {
            out.push(Line::Del(i + 1, a[i].to_string()));
            i += 1;
        }
    }
    out
}

/// Trecho a mostrar: linhas alteradas com `context` linhas iguais em volta; as demais viram
/// `None` (um marcador "N linhas iguais").
pub fn with_context(diff: &[Line], context: usize) -> Vec<Result<&Line, usize>> {
    let changed: Vec<usize> = diff.iter().enumerate().filter(|(_, l)| !matches!(l, Line::Same(..))).map(|(i, _)| i).collect();
    let near = |i: usize| changed.iter().any(|&c| c.abs_diff(i) <= context);
    let mut out = Vec::new();
    let mut hidden = 0;
    for (i, l) in diff.iter().enumerate() {
        if near(i) {
            if hidden > 0 {
                out.push(Err(hidden));
                hidden = 0;
            }
            out.push(Ok(l));
        } else {
            hidden += 1;
        }
    }
    if hidden > 0 {
        out.push(Err(hidden));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diferenca_e_contexto() {
        let a = "[RANGE_SET]\nDHCPRange=10.0.0.1-10.0.0.9\nSubnetMask=255.0.0.0\n\n[GLOBAL_OPTIONS]\nRouter=10.0.0.1\n";
        let b = "[RANGE_SET]\nDHCPRange=10.0.0.1-10.0.0.50\nSubnetMask=255.0.0.0\n\n[GLOBAL_OPTIONS]\nRouter=10.0.0.1\nDomainServer=1.1.1.1\n";
        let d = lines(a, b);
        assert!(d.contains(&Line::Del(2, "DHCPRange=10.0.0.1-10.0.0.9".into())));
        assert!(d.contains(&Line::Add(2, "DHCPRange=10.0.0.1-10.0.0.50".into())));
        assert!(d.contains(&Line::Add(7, "DomainServer=1.1.1.1".into())));
        assert_eq!(d.iter().filter(|l| matches!(l, Line::Same(..))).count(), 5);
        let shown = with_context(&d, 1);
        assert!(shown.iter().any(|x| x.is_err()), "linhas iguais longe das mudanças ficam escondidas");
        assert!(lines("x\ny", "x\ny").iter().all(|l| matches!(l, Line::Same(..))));
        assert_eq!(with_context(&lines("x\ny", "x\ny"), 3), vec![Err(2)]);
    }
}
