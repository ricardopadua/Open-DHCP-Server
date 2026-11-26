use std::io::Write;
use std::path::{Path, PathBuf};

/// Grava num temporário do mesmo diretório, faz fsync e renomeia por cima do original. Quando o
/// diretório não é gravável mas o arquivo é (o painel sem root regravando o opendhcp.ini ou o
/// estado dos leases, em pastas do root), grava no próprio arquivo.
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
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && mode.is_some() => return write_in_place(path, data),
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

/// Abre um arquivo que já existe para regravar do zero (truncado), sem seguir link simbólico.
pub fn open_existing_for_write(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).truncate(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NOFOLLOW
        opts.custom_flags(0o400000);
    }
    opts.open(path)
}

/// Regrava o próprio arquivo (sem temporário). Os backups do .ini são feitos antes.
fn write_in_place(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut f = open_existing_for_write(path)?;
    f.write_all(data)?;
    f.sync_all()
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Pasta só leitura com o arquivo gravável (o painel sem root nas pastas do root): grava no
    /// próprio arquivo. E um link simbólico no lugar do arquivo não é seguido.
    #[test]
    fn write_in_place_when_dir_is_read_only() {
        let dir = std::env::temp_dir().join(format!("opendhcp-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("opendhcp.ini");
        let other = dir.join("outro");
        std::fs::write(&file, b"antes").unwrap();
        std::fs::write(&other, b"intacto").unwrap();
        std::os::unix::fs::symlink(&other, dir.join("link")).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let r = write_atomic(&file, b"depois");
        let link = write_atomic(&dir.join("link"), b"x");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        r.unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"depois");
        assert!(link.is_err());
        assert_eq!(std::fs::read(&other).unwrap(), b"intacto");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
