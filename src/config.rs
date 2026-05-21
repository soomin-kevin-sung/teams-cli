use crate::util::fs::{recover_backup, write_atomic};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub root: PathBuf,
    pub state: PathBuf,
    pub aliases: PathBuf,
    pub cache_dir: PathBuf,
    pub secrets_file: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Result<Self, std::io::Error> {
        let root = if let Ok(dir) = std::env::var("TEAMS_STATE_DIR") {
            PathBuf::from(dir)
        } else {
            ProjectDirs::from("com", "earendil", "teams-cli")
                .map(|dirs| dirs.config_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".teams-cli"))
        };
        let cache_dir = root.join("cache");
        fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            state: root.join("state.toml"),
            aliases: root.join("aliases.toml"),
            secrets_file: root.join("secrets.json"),
            root,
            cache_dir,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub schema_version: u32,
    pub identity: Identity,
    pub expiry: Expiry,
    #[serde(default)]
    pub region_gtms: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub tenant_id: String,
    pub user_oid: String,
    pub display_name: String,
    pub upn: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expiry {
    pub aad_access_exp: i64,
    pub skype_exp: i64,
}

impl State {
    pub fn load(path: &Path) -> Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        recover_backup(path)?;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path)?;
        if content.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(toml::from_str(&content)?))
    }

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        write_atomic(path, &toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn empty_from_claims(
        tenant_id: String,
        user_oid: String,
        display_name: String,
        upn: String,
        aad_access_exp: i64,
    ) -> Self {
        Self {
            schema_version: 1,
            identity: Identity {
                tenant_id,
                user_oid,
                display_name,
                upn,
            },
            expiry: Expiry {
                aad_access_exp,
                skype_exp: 0,
            },
            region_gtms: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.toml");
        let mut state = State::empty_from_claims(
            "tenant".into(),
            "oid".into(),
            "name".into(),
            "user@example.com".into(),
            123,
        );
        state
            .region_gtms
            .insert("chatService".into(), "https://example".into());
        state.save(&path).expect("save");
        let loaded = State::load(&path).expect("load").expect("some state");
        assert_eq!(loaded, state);
    }
}
