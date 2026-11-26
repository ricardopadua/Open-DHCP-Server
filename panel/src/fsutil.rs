use std::io::Write;
use std::path::{Path, PathBuf};

/// Grava em `arquivo.tmp` no mesmo diretório, faz fsync e renomeia por cima do original.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = tmp_path(path);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".panel-tmp");
    path.with_file_name(name)
}

/// Backup de um arquivo: nome no diretório, quando foi feito e tamanho.
pub struct BackupFile {
    pub name: String,
    pub when: Option<chrono::NaiveDateTime>,
    pub size: u64,
}

/// Backups de `path` em `dir`, do mais novo para o mais antigo.
pub fn list_backups(path: &Path, dir: &Path) -> Vec<BackupFile> {
    let base = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let prefix = format!("{base}.");
    let mut out: Vec<BackupFile> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let stamp = name.strip_prefix(&prefix)?.to_string();
                    let meta = e.metadata().ok().filter(|m| m.is_file())?;
                    let when = chrono::NaiveDateTime::parse_from_str(&stamp, "%Y%m%d-%H%M%S%.3f").ok();
                    Some(BackupFile { name, when, size: meta.len() })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// Copia o arquivo para `<dir_backup>/<nome>.<AAAAmmdd-HHMMSS>` e mantém só os `keep` mais novos.
pub fn backup(path: &Path, dir: &Path, keep: usize) -> std::io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(dir)?;
    let base = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let dest = dir.join(format!("{base}.{stamp}"));
    std::fs::copy(path, &dest)?;

    let prefix = format!("{base}.");
    let mut olds: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)))
        .collect();
    olds.sort();
    while olds.len() > keep {
        let _ = std::fs::remove_file(olds.remove(0));
    }
    Ok(Some(dest))
}
