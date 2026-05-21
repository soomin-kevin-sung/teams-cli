use std::fs;
use std::io;
use std::path::Path;

pub fn recover_backup(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if !parent.exists() {
        return Ok(());
    }
    let Some(file_name) = path.file_name().and_then(|file_name| file_name.to_str()) else {
        return Ok(());
    };
    let backup_prefix = format!("{file_name}.");
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(&backup_prefix) && name.ends_with(".bak") {
            fs::rename(entry.path(), path)?;
            break;
        }
    }
    Ok(())
}

pub fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    recover_backup(path)?;
    let file_name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .unwrap_or("state");
    let tmp = path.with_file_name(format!("{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&tmp, content)?;
    if !path.exists() {
        return fs::rename(&tmp, path);
    }

    let backup = path.with_file_name(format!("{file_name}.{}.bak", uuid::Uuid::new_v4()));
    fs::rename(path, &backup)?;
    match fs::rename(&tmp, path) {
        Ok(()) => {
            let _ = fs::remove_file(&backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            let _ = fs::rename(&backup, path);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.toml");
        fs::write(&path, "old").expect("seed");

        write_atomic(&path, "new").expect("replace");

        assert_eq!(fs::read_to_string(path).expect("read"), "new");
    }

    #[test]
    fn recover_backup_restores_missing_canonical_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.toml");
        let backup = dir.path().join("state.toml.test.bak");
        fs::write(&backup, "old").expect("backup");

        recover_backup(&path).expect("recover");

        assert_eq!(fs::read_to_string(path).expect("read"), "old");
    }
}
