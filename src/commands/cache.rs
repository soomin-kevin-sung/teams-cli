use crate::api::{chats, client::ApiClient};
use crate::auth::{Session, USER_AGENT};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::chat_cache;
use crate::util::json::print_pretty;
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
struct CacheMetadata {
    path: PathBuf,
    file: &'static str,
    exists: bool,
    schema_version: Option<u32>,
    owner_present: bool,
    bytes: Option<u64>,
    modified: Option<String>,
    chat_count: Option<usize>,
}

pub async fn info(json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let metadata = cache_metadata(&paths)?;
    if json_output {
        print_pretty(&json!({
            "ok": true,
            "cache": {
                "file": metadata.file,
                "exists": metadata.exists,
                "schema_version": metadata.schema_version,
                "owner_present": metadata.owner_present,
                "bytes": metadata.bytes,
                "modified": metadata.modified,
                "chat_count": metadata.chat_count
            }
        }))?;
    } else if metadata.exists {
        println!(
            "Cache: {} ({} chats, {} bytes, modified {})",
            metadata.path.display(),
            metadata.chat_count.unwrap_or(0),
            metadata.bytes.unwrap_or(0),
            metadata.modified.as_deref().unwrap_or("-")
        );
    } else {
        println!("Cache missing: {}", metadata.path.display());
    }
    Ok(())
}

pub async fn refresh(limit: usize, json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let chats = chats::list_chats(&api, limit).await?;
    let owner = chat_cache::owner_from_api(&api).await;
    chat_cache::write(&paths, &owner, &chats)?;
    if json_output {
        print_pretty(&json!({
            "ok": true,
            "cache": {
                "file": chat_cache::cache_file_label(),
                "refreshed": true,
                "chat_count": chats.len()
            }
        }))?;
    } else {
        println!("Cache refreshed: {} chats", chats.len());
    }
    Ok(())
}

pub async fn clear(json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let path = chat_cache::cache_path(&paths);
    let existed = chat_cache::clear(&paths)?;
    if json_output {
        print_pretty(&json!({
            "ok": true,
            "cache": {
                "file": chat_cache::cache_file_label(),
                "cleared": existed
            }
        }))?;
    } else if existed {
        println!("Cache cleared: {}", path.display());
    } else {
        println!("Cache already empty: {}", path.display());
    }
    Ok(())
}

fn cache_metadata(paths: &AppPaths) -> Result<CacheMetadata, CliError> {
    let path = chat_cache::cache_path(paths);
    if !path.exists() {
        return Ok(CacheMetadata {
            path,
            file: chat_cache::cache_file_label(),
            exists: false,
            schema_version: None,
            owner_present: false,
            bytes: None,
            modified: None,
            chat_count: None,
        });
    }
    let metadata = fs::metadata(&path)?;
    let modified = metadata
        .modified()
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .map(|time| time.to_rfc3339());
    let cache = chat_cache::read(paths)?.ok_or_else(chat_cache::cache_corrupt)?;
    Ok(CacheMetadata {
        path,
        file: chat_cache::cache_file_label(),
        exists: true,
        schema_version: Some(cache.schema_version),
        owner_present: cache.owner.is_some(),
        bytes: Some(metadata.len()),
        modified,
        chat_count: Some(cache.chats.len()),
    })
}
