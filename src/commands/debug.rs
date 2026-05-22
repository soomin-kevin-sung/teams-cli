use crate::api::{chats, client::ApiClient, messages};
use crate::auth::{Session, USER_AGENT};
use crate::commands::target;
use crate::config::AppPaths;
use crate::error::CliError;
use serde_json::json;

pub async fn raw_messages(
    thread_id: &str,
    limit: usize,
    json_output: bool,
) -> Result<(), CliError> {
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let limit = limit.min(100);
    let raw = messages::read_messages_raw(&api, thread_id, limit).await?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "debug": true,
                "thread_id": thread_id,
                "limit": limit,
                "raw": raw
            }))?
        );
    } else {
        println!("{}", serde_json::to_string_pretty(&raw)?);
    }
    Ok(())
}

pub async fn raw_chats(limit: usize, json_output: bool) -> Result<(), CliError> {
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let limit = limit.min(300);
    let raw = chats::list_chats_raw(&api, limit).await?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "debug": true,
                "limit": limit,
                "raw": raw
            }))?
        );
    } else {
        println!("{}", serde_json::to_string_pretty(&raw)?);
    }
    Ok(())
}

pub async fn send_html(target: &str, html: &str, json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let resolution = target::resolve_send_target(&api, &paths, target).await?;
    let sent = messages::send_html_message(&api, &resolution.thread_id, html).await?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "debug": true,
                "sent": true,
                "format": "html",
                "target": target,
                "thread_id": resolution.thread_id,
                "client_message_id": sent.client_message_id,
                "id": sent.id,
                "html_length": html.chars().count()
            }))?
        );
    } else if let Some(id) = sent.id {
        println!("Sent debug HTML: {id}");
    } else {
        println!(
            "Sent debug HTML: {} (server message id was not returned)",
            sent.client_message_id
        );
    }
    Ok(())
}
