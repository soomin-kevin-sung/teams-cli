use crate::api::{chats::ChatSummary, client::ApiClient};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::fs::{recover_backup, write_atomic};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

const CACHE_FILE_LABEL: &str = "cache/chats.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheOwner {
    pub tenant_id: String,
    pub user_oid: String,
    pub upn: String,
}

#[derive(Debug, Clone)]
pub struct CacheRead {
    pub schema_version: u32,
    pub owner: Option<CacheOwner>,
    pub chats: Vec<ChatSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatCacheFile {
    schema_version: u32,
    owner: CacheOwner,
    chats: Vec<ChatSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CacheDocument {
    Current(ChatCacheFile),
    Legacy(Vec<ChatSummary>),
}

pub async fn owner_from_api(api: &ApiClient) -> CacheOwner {
    CacheOwner {
        tenant_id: api.tenant_id().await,
        user_oid: api.user_oid().await,
        upn: api.upn().await,
    }
}

pub fn cache_path(paths: &AppPaths) -> PathBuf {
    paths.cache_dir.join("chats.json")
}

pub fn cache_file_label() -> &'static str {
    CACHE_FILE_LABEL
}

pub fn read(paths: &AppPaths) -> Result<Option<CacheRead>, CliError> {
    let path = cache_path(paths);
    recover_backup(&path)?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    let document = serde_json::from_str::<CacheDocument>(&content).map_err(|_| cache_corrupt())?;
    Ok(Some(match document {
        CacheDocument::Current(file) => CacheRead {
            schema_version: file.schema_version,
            owner: Some(file.owner),
            chats: file.chats,
        },
        CacheDocument::Legacy(chats) => CacheRead {
            schema_version: 1,
            owner: None,
            chats,
        },
    }))
}

pub fn load_for_owner(
    paths: &AppPaths,
    expected_owner: &CacheOwner,
) -> Result<Vec<ChatSummary>, CliError> {
    let Some(cache) = read(paths)? else {
        return Ok(Vec::new());
    };
    if cache.owner.as_ref() == Some(expected_owner) {
        Ok(cache.chats)
    } else {
        Ok(Vec::new())
    }
}

pub fn write(paths: &AppPaths, owner: &CacheOwner, chats: &[ChatSummary]) -> Result<(), CliError> {
    let file = ChatCacheFile {
        schema_version: 2,
        owner: owner.clone(),
        chats: chats.to_vec(),
    };
    write_atomic(&cache_path(paths), &serde_json::to_string_pretty(&file)?)?;
    Ok(())
}

pub fn clear(paths: &AppPaths) -> Result<bool, CliError> {
    let path = cache_path(paths);
    let existed = path.exists();
    if existed {
        fs::remove_file(path)?;
    }
    Ok(existed)
}

pub fn cache_corrupt() -> CliError {
    CliError::structured(
        "cache_corrupt",
        "chat cache is not valid JSON; run `teams cache clear` or `teams cache refresh`",
        json!({ "file": CACHE_FILE_LABEL }),
        2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_cache_without_owner_does_not_match_expected_owner() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = AppPaths {
            root: dir.path().to_path_buf(),
            state: dir.path().join("state.toml"),
            aliases: dir.path().join("aliases.toml"),
            cache_dir: dir.path().join("cache"),
            secrets_file: dir.path().join("secrets.json"),
        };
        fs::create_dir_all(&paths.cache_dir).expect("cache dir");
        fs::write(cache_path(&paths), "[]").expect("write");

        let chats = load_for_owner(
            &paths,
            &CacheOwner {
                tenant_id: "tenant".into(),
                user_oid: "oid".into(),
                upn: "user@example.com".into(),
            },
        )
        .expect("load");

        assert!(chats.is_empty());
    }
}
