use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRef {
    ThreadId(String),
    UnresolvableUpn(String),
    Unknown(String),
}

#[derive(Debug, Default, Deserialize)]
struct Aliases {
    #[serde(default)]
    aliases: BTreeMap<String, String>,
}

pub fn resolve(input: &str, aliases_path: &Path) -> ChatRef {
    if input.starts_with("19:") || input.starts_with("48:") {
        return ChatRef::ThreadId(input.to_string());
    }
    if let Some(thread_id) = alias_lookup(input, aliases_path) {
        return ChatRef::ThreadId(thread_id);
    }
    if input.contains('@') {
        return ChatRef::UnresolvableUpn(input.to_string());
    }
    ChatRef::Unknown(input.to_string())
}

fn alias_lookup(alias: &str, aliases_path: &Path) -> Option<String> {
    let content = fs::read_to_string(aliases_path).ok()?;
    let aliases = toml::from_str::<Aliases>(&content).ok()?;
    aliases.aliases.get(alias).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_raw_thread_id() {
        assert_eq!(
            resolve("19:abc@thread.v2", Path::new("missing.toml")),
            ChatRef::ThreadId("19:abc@thread.v2".to_string())
        );
    }

    #[test]
    fn flags_upn_as_deferred() {
        assert_eq!(
            resolve("user@example.com", Path::new("missing.toml")),
            ChatRef::UnresolvableUpn("user@example.com".to_string())
        );
    }

    #[test]
    fn resolves_alias() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("aliases.toml");
        fs::write(&path, "[aliases]\nteam = '19:abc@thread.v2'\n").expect("write");
        assert_eq!(
            resolve("team", &path),
            ChatRef::ThreadId("19:abc@thread.v2".into())
        );
    }
}
