use crate::api::{chats, client::ApiClient};
use crate::auth::{Session, USER_AGENT};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::output::render_chats_table;
use std::fs;

pub async fn run(limit: usize, json_output: bool) -> Result<(), CliError> {
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let chats = chats::list_chats(&api, limit).await?;
    cache_chats(&chats)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&chats)?);
    } else {
        println!("{}", render_chats_table(&chats));
    }
    Ok(())
}

fn cache_chats(chats: &[chats::ChatSummary]) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    fs::create_dir_all(&paths.cache_dir)?;
    fs::write(
        paths.cache_dir.join("chats.json"),
        serde_json::to_string_pretty(chats)?,
    )?;
    Ok(())
}
