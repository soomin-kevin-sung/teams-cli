use crate::api::{client::ApiClient, messages};
use crate::auth::{Session, USER_AGENT};
use crate::commands::target::{self, TargetResolution};
use crate::config::AppPaths;
use crate::error::CliError;
use serde_json::json;

pub async fn run(target: &str, limit: usize, json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let resolution = target::resolve_send_target(&api, &paths, target).await?;
    let messages = messages::read_messages(&api, &resolution.thread_id, limit).await?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "read": true,
                "target": target,
                "thread_id": resolution.thread_id,
                "chat": resolution.thread_id,
                "chat_summary": resolution.chat,
                "count": messages.len(),
                "messages": messages
            }))?
        );
    } else {
        println!(
            "Read {} message(s) from {}",
            messages.len(),
            target_label(&resolution)
        );
        for message in messages {
            let sender = message
                .sender
                .as_ref()
                .and_then(|sender| sender.display_name.as_deref())
                .unwrap_or("(unknown)");
            let created_at = message
                .created_at
                .map(|time| time.to_rfc3339())
                .unwrap_or_else(|| "-".to_string());
            let text = message.content_text.as_deref().unwrap_or("");
            println!("[{created_at}] {sender}: {text}");
        }
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
