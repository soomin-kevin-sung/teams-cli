use crate::api::{client::ApiClient, messages};
use crate::auth::{Session, USER_AGENT};
use crate::commands::target::{self, TargetResolution, TargetSource};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::rich_text::{prepare_message, MessageFormat, PreparedMessage};
use serde_json::json;
use std::io::Read;

const MAX_MESSAGE_BYTES: usize = 64 * 1024;

pub async fn run(
    chat: &str,
    message: Option<&str>,
    read_stdin: bool,
    format: &str,
    confirm_thread_id: Option<&str>,
    dry_run: bool,
    json_output: bool,
) -> Result<(), CliError> {
    let format = MessageFormat::parse(format)?;
    validate_message_args(message, read_stdin)?;
    let paths = AppPaths::resolve()?;
    if dry_run {
        if let Some(resolution) = target::resolve_local_target(&paths, chat)? {
            validate_confirm_thread_id(&resolution, confirm_thread_id)?;
            let message = message_body(message, read_stdin)?;
            let prepared = prepare_message(&message, format);
            print_dry_run(chat, &prepared, &resolution, json_output)?;
            return Ok(());
        }
    }

    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let resolution = target::resolve_send_target(&api, &paths, chat).await?;
    validate_confirm_thread_id(&resolution, confirm_thread_id)?;
    require_confirmation_for_json_send(&resolution, confirm_thread_id, dry_run, json_output)?;
    let message = message_body(message, read_stdin)?;
    let prepared = prepare_message(&message, format);
    if dry_run {
        print_dry_run(chat, &prepared, &resolution, json_output)?;
        return Ok(());
    }

    let thread_id = resolution.thread_id.clone();
    let sent = if matches!(prepared.format, MessageFormat::Text) {
        messages::send_message(&api, &thread_id, &message).await?
    } else {
        messages::send_html_message(&api, &thread_id, &prepared.html).await?
    };
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "sent": true,
                "dry_run": false,
                "format": prepared.format.as_str(),
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
    message: &PreparedMessage,
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
                    "format": message.format.as_str(),
                    "text_length": message.input_chars,
                    "html_length": message.html_chars(),
                    "html_escaped": message.html_escaped(),
                    "markdown_converted": message.markdown_converted()
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

fn require_confirmation_for_json_send(
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
        "send --json requires --confirm-thread-id for resolved name/title targets",
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

fn target_label(resolution: &TargetResolution) -> String {
    let title = resolution
        .chat
        .as_ref()
        .and_then(|chat| chat.title.as_deref())
        .unwrap_or("(raw thread id)");
    format!("{title} {}", resolution.thread_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::target::{TargetResolution, TargetSource};

    #[test]
    fn confirm_thread_id_rejects_mismatch() {
        let resolution = TargetResolution {
            target: "a".into(),
            thread_id: "19:a@thread.v2".into(),
            source: TargetSource::RawThreadId,
            chat: None,
        };

        let error =
            validate_confirm_thread_id(&resolution, Some("19:b@thread.v2")).expect_err("mismatch");

        assert_eq!(error.code(), "target_confirmation_mismatch");
    }

    #[test]
    fn json_send_requires_confirmation_for_lookup_targets() {
        let resolution = TargetResolution {
            target: "Alex".into(),
            thread_id: "19:a@thread.v2".into(),
            source: TargetSource::FreshChats,
            chat: None,
        };

        let error = require_confirmation_for_json_send(&resolution, None, false, true)
            .expect_err("confirmation required");

        assert_eq!(error.code(), "confirmation_required");
    }

    #[test]
    fn message_args_require_message_or_stdin() {
        assert!(validate_message_args(None, false).is_err());
        assert_eq!(message_body(Some("hello"), false).expect("body"), "hello");
        assert!(validate_message_args(Some("hello"), true).is_err());
    }

    #[test]
    fn invalid_format_is_structured() {
        let error = MessageFormat::parse("rtf").expect_err("invalid");

        assert_eq!(error.code(), "invalid_arguments");
    }

    #[test]
    fn message_body_rejects_oversized_arguments() {
        let oversized = "a".repeat(MAX_MESSAGE_BYTES + 1);
        let error = message_body(Some(&oversized), false).expect_err("oversized");

        assert_eq!(error.code(), "message_too_large");
    }
}
