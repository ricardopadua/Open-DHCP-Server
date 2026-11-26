use std::io::Write;
use std::path::{Path, PathBuf};

/// Grava num temporário do mesmo diretório, faz fsync e renomeia por cima do original.
///
/// O temporário tem nome imprevisível e é criado com `create_new` (não segue nem reaproveita um
/// link ou arquivo plantado por outro usuário) e já com a permissão do original, ou 0600 para
/// arquivo novo; assim o conteúdo nunca fica legível por outros durante a gravação.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mode = std::fs::metadata(path).ok().map(|m| m.permissions());
    let tmp = loop {
        let candidate = tmp_path(path);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            opts.mode(mode.as_ref().map(|p| p.mode() & 0o7777).unwrap_or(0o600));
        }
        match opts.open(&candidate) {
            Ok(mut f) => {
                let written = f.write_all(data).and_then(|_| f.sync_all());
                if let Err(e) = written {
                    let _ = std::fs::remove_file(&candidate);
                    return Err(e);
                }
                break candidate;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    if let Some(p) = mode {
        let _ = std::fs::set_permissions(&tmp, p);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".{}.tmp", crate::auth::random_token().get(..16).unwrap_or("x")));
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
    #[cfg(unix)]
    {
        // backups têm a configuração inteira: só o dono lê
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
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
