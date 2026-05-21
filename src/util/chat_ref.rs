use crate::error::CliError;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRef {
    ThreadId(String),
    Lookup(String),
}

#[derive(Debug, Default, Deserialize)]
struct Aliases {
    #[serde(default)]
    aliases: BTreeMap<String, String>,
}

pub fn resolve(input: &str, aliases_path: &Path) -> Result<ChatRef, CliError> {
    if input.starts_with("19:") || input.starts_with("48:") {
        return Ok(ChatRef::ThreadId(input.to_string()));
    }
    if let Some(thread_id) = alias_lookup(input, aliases_path)? {
        return Ok(ChatRef::ThreadId(thread_id));
    }
    Ok(ChatRef::Lookup(input.to_string()))
}

fn alias_lookup(alias: &str, aliases_path: &Path) -> Result<Option<String>, CliError> {
    if !aliases_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(aliases_path).map_err(|error| {
        alias_config_error(
            aliases_path,
            "could not read aliases file",
            "read_error",
            Some(error.kind().to_string()),
        )
    })?;
    let aliases = toml::from_str::<Aliases>(&content).map_err(|_| {
        alias_config_error(
            aliases_path,
            "aliases file is invalid TOML",
            "parse_error",
            None,
        )
    })?;
    Ok(aliases.aliases.get(alias).cloned())
}

fn alias_config_error(
    path: &Path,
    message: &'static str,
    reason: &'static str,
    diagnostic: Option<String>,
) -> CliError {
    CliError::structured(
        "alias_config_error",
        format!("{message}: {}", path.display()),
        json!({
            "path": path.display().to_string(),
            "reason": reason,
            "diagnostic": diagnostic
        }),
        2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_raw_thread_id() {
        assert_eq!(
            resolve("19:abc@thread.v2", Path::new("missing.toml")).expect("resolve"),
            ChatRef::ThreadId("19:abc@thread.v2".to_string())
        );
    }

    #[test]
    fn treats_upn_as_lookup_target() {
        assert_eq!(
            resolve("user@example.com", Path::new("missing.toml")).expect("resolve"),
            ChatRef::Lookup("user@example.com".to_string())
        );
    }

    #[test]
    fn resolves_alias() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("aliases.toml");
        fs::write(&path, "[aliases]\nteam = '19:abc@thread.v2'\n").expect("write");
        assert_eq!(
            resolve("team", &path).expect("resolve"),
            ChatRef::ThreadId("19:abc@thread.v2".into())
        );
    }

    #[test]
    fn malformed_alias_file_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("aliases.toml");
        fs::write(&path, "[aliases\nteam = '19:abc@thread.v2'\n").expect("write");

        let error = resolve("team", &path).expect_err("invalid aliases");

        assert_eq!(error.code(), "alias_config_error");
    }
}
