use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::fs::{recover_backup, write_atomic};
use crate::util::json::print_pretty;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Aliases {
    #[serde(default)]
    aliases: BTreeMap<String, String>,
}

pub async fn list(json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let aliases = load_aliases(&paths.aliases)?;
    if json_output {
        print_pretty(&json!({
            "ok": true,
            "aliases_file": "aliases.toml",
            "aliases": aliases.aliases
        }))?;
    } else if aliases.aliases.is_empty() {
        println!("No aliases configured.");
    } else {
        for (name, thread_id) in aliases.aliases {
            println!("{name}\t{thread_id}");
        }
    }
    Ok(())
}

pub async fn set(name: &str, thread_id: &str, json_output: bool) -> Result<(), CliError> {
    validate_alias_name(name)?;
    validate_thread_id(thread_id)?;
    let paths = AppPaths::resolve()?;
    let mut aliases = load_aliases(&paths.aliases)?;
    aliases
        .aliases
        .insert(name.to_string(), thread_id.to_string());
    save_aliases(&paths.aliases, &aliases)?;
    if json_output {
        print_pretty(&json!({
            "ok": true,
            "alias": name,
            "thread_id": thread_id,
            "aliases_file": "aliases.toml"
        }))?;
    } else {
        println!("Alias set: {name} -> {thread_id}");
    }
    Ok(())
}

pub async fn remove(name: &str, json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let mut aliases = load_aliases(&paths.aliases)?;
    let removed = aliases.aliases.remove(name);
    save_aliases(&paths.aliases, &aliases)?;
    if json_output {
        print_pretty(&json!({
            "ok": true,
            "alias": name,
            "removed": removed.is_some(),
            "aliases_file": "aliases.toml"
        }))?;
    } else if removed.is_some() {
        println!("Alias removed: {name}");
    } else {
        println!("Alias not found: {name}");
    }
    Ok(())
}

fn load_aliases(path: &Path) -> Result<Aliases, CliError> {
    recover_backup(path).map_err(|error| {
        alias_config_error(
            path,
            "could not recover aliases file",
            "recover_error",
            Some(error.kind().to_string()),
        )
    })?;
    if !path.exists() {
        return Ok(Aliases::default());
    }
    let content = fs::read_to_string(path).map_err(|error| {
        alias_config_error(
            path,
            "could not read aliases file",
            "read_error",
            Some(error.kind().to_string()),
        )
    })?;
    let aliases: Aliases = toml::from_str(&content).map_err(|_| {
        alias_config_error(path, "aliases file is invalid TOML", "parse_error", None)
    })?;
    validate_alias_values(path, &aliases)?;
    Ok(aliases)
}

fn save_aliases(path: &Path, aliases: &Aliases) -> Result<(), CliError> {
    write_atomic(path, &toml::to_string_pretty(aliases)?)?;
    Ok(())
}

fn validate_alias_name(name: &str) -> Result<(), CliError> {
    let valid = !name.trim().is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(CliError::structured(
            "invalid_alias",
            "alias name must contain only ASCII letters, numbers, '.', '-', or '_'",
            json!({ "alias": name }),
            2,
        ))
    }
}

fn validate_thread_id(thread_id: &str) -> Result<(), CliError> {
    if is_thread_id(thread_id) {
        Ok(())
    } else {
        Err(CliError::structured(
            "invalid_thread_id",
            "alias value must be a raw Teams thread id",
            json!({ "thread_id": thread_id }),
            2,
        ))
    }
}

fn validate_alias_values(path: &Path, aliases: &Aliases) -> Result<(), CliError> {
    if aliases.aliases.values().all(|value| is_thread_id(value)) {
        Ok(())
    } else {
        Err(alias_config_error(
            path,
            "aliases file contains a non-thread-id value",
            "invalid_value",
            None,
        ))
    }
}

fn is_thread_id(value: &str) -> bool {
    value.starts_with("19:") || value.starts_with("48:")
}

fn alias_config_error(
    _path: &Path,
    message: &'static str,
    reason: &'static str,
    diagnostic: Option<String>,
) -> CliError {
    CliError::structured(
        "alias_config_error",
        format!("{message}: aliases.toml"),
        json!({
            "file": "aliases.toml",
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
    fn alias_name_validation_is_strict() {
        assert!(validate_alias_name("team.alpha-1").is_ok());
        assert!(validate_alias_name("bad name").is_err());
    }

    #[test]
    fn alias_values_must_be_thread_ids() {
        let mut aliases = Aliases::default();
        aliases.aliases.insert("bad".into(), "not-a-thread".into());

        let error =
            validate_alias_values(Path::new("aliases.toml"), &aliases).expect_err("invalid value");

        assert_eq!(error.code(), "alias_config_error");
    }
}
