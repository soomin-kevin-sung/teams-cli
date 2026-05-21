use crate::api::{client::ApiClient, messages};
use crate::auth::{Session, USER_AGENT};
use crate::commands::target::{self, TargetResolution};
use crate::config::AppPaths;
use crate::error::CliError;
use serde_json::json;

pub async fn run(
    chat: &str,
    message: &str,
    dry_run: bool,
    json_output: bool,
) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    if dry_run {
        if let Some(resolution) = target::resolve_local_target(&paths, chat)? {
            print_dry_run(chat, message, &resolution, json_output)?;
            return Ok(());
        }
    }

    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let resolution = target::resolve_send_target(&api, &paths, chat).await?;
    if dry_run {
        print_dry_run(chat, message, &resolution, json_output)?;
        return Ok(());
    }

    let thread_id = resolution.thread_id.clone();
    let sent = messages::send_message(&api, &thread_id, message).await?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "sent": true,
                "dry_run": false,
                "id": sent.id,
                "client_message_id": sent.client_message_id,
                "target": chat,
                "chat": thread_id,
                "thread_id": thread_id,
                "chat_summary": resolution.chat
            }))?
        );
    } else if let Some(id) = sent.id {
        println!("Sent: {id}");
    } else {
        println!(
            "Sent: {} (server message id was not returned)",
            sent.client_message_id
        );
    }
    Ok(())
}

fn print_dry_run(
    target: &str,
    message: &str,
    resolution: &TargetResolution,
    json_output: bool,
) -> Result<(), CliError> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "sent": false,
                "dry_run": true,
                "target": target,
                "chat": resolution.thread_id,
                "thread_id": resolution.thread_id,
                "chat_summary": resolution.chat,
                "message": {
                    "content_type": "RichText/Html",
                    "text_length": message.chars().count(),
                    "html_escaped": true
                }
            }))?
        );
    } else {
        println!(
            "Dry run: would send to {} ({:?})",
            target_label(resolution),
            resolution.source
        );
    }
    Ok(())
}

fn target_label(resolution: &TargetResolution) -> String {
    let title = resolution
        .chat
        .as_ref()
        .and_then(|chat| chat.title.as_deref())
        .unwrap_or("(raw thread id)");
    format!("{title} {}", resolution.thread_id)
}
