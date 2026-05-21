use crate::api::{client::ApiClient, messages};
use crate::auth::{Session, USER_AGENT};
use crate::commands::target::{self, TargetResolution, TargetSource};
use crate::config::AppPaths;
use crate::error::CliError;
use serde_json::json;
use std::io::Read;

const MAX_MESSAGE_BYTES: usize = 64 * 1024;

pub async fn run(
    channel: &str,
    message: Option<&str>,
    read_stdin: bool,
    confirm_thread_id: Option<&str>,
    reply_chain_id: Option<&str>,
    dry_run: bool,
    json_output: bool,
) -> Result<(), CliError> {
    validate_message_args(message, read_stdin)?;
    let paths = AppPaths::resolve()?;
    if dry_run {
        if let Some(resolution) = target::resolve_local_target(&paths, channel)? {
            validate_channel_resolution(channel, &resolution)?;
            validate_confirm_thread_id(&resolution, confirm_thread_id)?;
            let message = message_body(message, read_stdin)?;
            print_dry_run(channel, &message, &resolution, reply_chain_id, json_output)?;
            return Ok(());
        }
    }

    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let resolution = target::resolve_post_target(&api, &paths, channel).await?;
    validate_channel_resolution(channel, &resolution)?;
    validate_confirm_thread_id(&resolution, confirm_thread_id)?;
    require_confirmation_for_json_post(&resolution, confirm_thread_id, dry_run, json_output)?;
    let message = message_body(message, read_stdin)?;
    if dry_run {
        print_dry_run(channel, &message, &resolution, reply_chain_id, json_output)?;
        return Ok(());
    }

    let thread_id = resolution.thread_id.clone();
    let sent = messages::post_channel_message(&api, &thread_id, &message, reply_chain_id).await?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "posted": true,
                "dry_run": false,
                "id": sent.id,
                "client_message_id": sent.client_message_id,
                "target": channel,
                "channel": thread_id,
                "thread_id": thread_id,
                "reply_chain_id": reply_chain_id,
                "chat_summary": resolution.chat
            }))?
        );
    } else if let Some(id) = sent.id {
        println!("Posted: {id}");
    } else {
        println!(
            "Posted: {} (server message id was not returned)",
            sent.client_message_id
        );
    }
    Ok(())
}

fn print_dry_run(
    target: &str,
    message: &str,
    resolution: &TargetResolution,
    reply_chain_id: Option<&str>,
    json_output: bool,
) -> Result<(), CliError> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "posted": false,
                "dry_run": true,
                "target": target,
                "channel": resolution.thread_id,
                "thread_id": resolution.thread_id,
                "reply_chain_id": reply_chain_id,
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
            "Dry run: would post to {} ({:?})",
            target_label(resolution),
            resolution.source
        );
    }
    Ok(())
}

fn message_body(message: Option<&str>, read_stdin: bool) -> Result<String, CliError> {
    match (message, read_stdin) {
        (Some(message), false) => {
            validate_message_size(message)?;
            Ok(message.to_string())
        }
        (None, true) => {
            let mut body = String::new();
            std::io::stdin()
                .take(MAX_MESSAGE_BYTES as u64 + 1)
                .read_to_string(&mut body)?;
            validate_message_size(&body)?;
            Ok(body)
        }
        (Some(_), true) | (None, false) => unreachable!("message arguments are prevalidated"),
    }
}

fn validate_message_args(message: Option<&str>, read_stdin: bool) -> Result<(), CliError> {
    match (message, read_stdin) {
        (Some(_), true) => Err(CliError::structured(
            "invalid_arguments",
            "MESSAGE cannot be provided together with --stdin",
            json!({}),
            2,
        )),
        (None, false) => Err(CliError::structured(
            "invalid_arguments",
            "MESSAGE is required unless --stdin is used",
            json!({}),
            2,
        )),
        _ => Ok(()),
    }
}

fn validate_message_size(message: &str) -> Result<(), CliError> {
    if message.len() <= MAX_MESSAGE_BYTES {
        Ok(())
    } else {
        Err(CliError::structured(
            "message_too_large",
            format!("message body must be at most {MAX_MESSAGE_BYTES} bytes"),
            json!({ "max_bytes": MAX_MESSAGE_BYTES }),
            2,
        ))
    }
}

fn require_confirmation_for_json_post(
    resolution: &TargetResolution,
    confirm_thread_id: Option<&str>,
    dry_run: bool,
    json_output: bool,
) -> Result<(), CliError> {
    if dry_run
        || !json_output
        || confirm_thread_id.is_some()
        || matches!(
            resolution.source,
            TargetSource::RawThreadId | TargetSource::Alias
        )
    {
        return Ok(());
    }

    Err(CliError::structured(
        "confirmation_required",
        "post --json requires --confirm-thread-id for resolved channel targets",
        json!({
            "target": resolution.target,
            "thread_id": resolution.thread_id,
            "source": resolution.source
        }),
        2,
    ))
}

fn validate_confirm_thread_id(
    resolution: &TargetResolution,
    confirm_thread_id: Option<&str>,
) -> Result<(), CliError> {
    let Some(confirm_thread_id) = confirm_thread_id else {
        return Ok(());
    };
    if resolution.thread_id == confirm_thread_id {
        return Ok(());
    }
    Err(CliError::structured(
        "target_confirmation_mismatch",
        "resolved thread id does not match --confirm-thread-id",
        json!({
            "resolved_thread_id": resolution.thread_id,
            "confirm_thread_id": confirm_thread_id
        }),
        2,
    ))
}

fn validate_channel_resolution(
    target: &str,
    resolution: &TargetResolution,
) -> Result<(), CliError> {
    if resolution.thread_id.contains("@thread.tacv2") {
        return Ok(());
    }
    Err(CliError::structured(
        "unsupported_target",
        format!("post target '{target}' is not a channel thread id"),
        json!({
            "target": target,
            "thread_id": resolution.thread_id
        }),
        2,
    ))
}

fn target_label(resolution: &TargetResolution) -> String {
    let title = resolution
        .chat
        .as_ref()
        .and_then(|chat| chat.title.as_deref())
        .unwrap_or("(raw channel id)");
    format!("{title} {}", resolution.thread_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::target::{TargetCandidate, TargetResolution, TargetSource};

    #[test]
    fn confirms_thread_id_for_post() {
        let resolution = TargetResolution {
            target: "General".into(),
            thread_id: "19:a@thread.tacv2".into(),
            source: TargetSource::RawThreadId,
            chat: None,
        };

        assert!(validate_confirm_thread_id(&resolution, Some("19:a@thread.tacv2")).is_ok());
        assert_eq!(
            validate_confirm_thread_id(&resolution, Some("19:b@thread.tacv2"))
                .expect_err("mismatch")
                .code(),
            "target_confirmation_mismatch"
        );
    }

    #[test]
    fn rejects_non_channel_target() {
        let resolution = TargetResolution {
            target: "Chat".into(),
            thread_id: "19:a@thread.v2".into(),
            source: TargetSource::RawThreadId,
            chat: Some(TargetCandidate {
                id: "19:a@thread.v2".into(),
                kind: crate::api::chats::ChatKind::Group,
                title: Some("Chat".into()),
                members: Vec::new(),
            }),
        };

        assert_eq!(
            validate_channel_resolution("Chat", &resolution)
                .expect_err("unsupported")
                .code(),
            "unsupported_target"
        );
    }

    #[test]
    fn message_args_require_body() {
        assert!(validate_message_args(None, false).is_err());
        assert_eq!(message_body(Some("hello"), false).expect("body"), "hello");
        assert!(validate_message_args(Some("hello"), true).is_err());
    }
}
