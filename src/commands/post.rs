use crate::api::{client::ApiClient, messages};
use crate::auth::{Session, USER_AGENT};
use crate::commands::target::{self, TargetResolution, TargetSource};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::rich_text::{prepare_message, MessageFormat, PreparedMessage};
use serde_json::json;
use std::fs;
use std::io::Read;

const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_CARD_BYTES: usize = 64 * 1024;
const EXPERIMENTAL_CARD_POST_ENV: &str = "TEAMS_ALLOW_EXPERIMENTAL_CARD_POST";

pub struct ChannelOptions<'a> {
    pub channel: &'a str,
    pub message: Option<&'a str>,
    pub read_stdin: bool,
    pub format: &'a str,
    pub card_json: Option<&'a str>,
    pub confirm_thread_id: Option<&'a str>,
    pub dry_run: bool,
    pub json_output: bool,
}

pub async fn channel(options: ChannelOptions<'_>) -> Result<(), CliError> {
    let ChannelOptions {
        channel,
        message,
        read_stdin,
        format,
        card_json,
        confirm_thread_id,
        dry_run,
        json_output,
    } = options;
    let format = MessageFormat::parse(format)?;
    validate_message_args(message, read_stdin, format, card_json, dry_run)?;
    let paths = AppPaths::resolve()?;
    if card_json.is_some() && !dry_run && !experimental_card_post_enabled() {
        let resolution = target::resolve_local_channel_target(&paths, channel)?;
        if let Some(resolution) = &resolution {
            validate_confirm_thread_id(resolution, confirm_thread_id)?;
        }
        let card = card_body(card_json)?;
        return Err(unsupported_card_post_error(
            channel,
            card.as_ref().expect("card_json produced a card"),
            resolution.as_ref(),
        ));
    }
    if dry_run {
        if let Some(resolution) = target::resolve_local_channel_target(&paths, channel)? {
            validate_confirm_thread_id(&resolution, confirm_thread_id)?;
            let message = message_body(message, read_stdin)?;
            let message = message
                .as_deref()
                .map(|message| prepare_message(message, format));
            let card = card_body(card_json)?;
            print_dry_run(
                channel,
                message.as_ref(),
                card.as_ref(),
                &resolution,
                json_output,
            )?;
            return Ok(());
        }
    }

    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let resolution = target::resolve_channel_target(&api, &paths, channel).await?;
    validate_confirm_thread_id(&resolution, confirm_thread_id)?;
    require_confirmation_for_json_post(&resolution, confirm_thread_id, dry_run, json_output)?;
    let raw_message = message_body(message, read_stdin)?;
    let prepared_message = raw_message
        .as_deref()
        .map(|message| prepare_message(message, format));
    let card = card_body(card_json)?;
    if dry_run {
        print_dry_run(
            channel,
            prepared_message.as_ref(),
            card.as_ref(),
            &resolution,
            json_output,
        )?;
        return Ok(());
    }

    let thread_id = resolution.thread_id.clone();
    let (sent, format) = if let Some(card) = card {
        (
            messages::send_swift_adaptive_card(&api, &thread_id, &card).await?,
            "adaptive_card",
        )
    } else {
        let Some(message) = prepared_message else {
            return Err(CliError::structured(
                "invalid_arguments",
                "MESSAGE is required unless --stdin or --card-json is used",
                json!({}),
                2,
            ));
        };
        let sent = if matches!(message.format, MessageFormat::Text) {
            let raw_message = raw_message
                .as_deref()
                .expect("raw message exists when prepared message exists");
            messages::send_message(&api, &thread_id, raw_message).await?
        } else {
            messages::send_html_message(&api, &thread_id, &message.html).await?
        };
        (sent, message.format.as_str())
    };
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "posted": true,
                "dry_run": false,
                "format": format,
                "id": sent.id,
                "client_message_id": sent.client_message_id,
                "target": channel,
                "channel": thread_id,
                "thread_id": thread_id,
                "channel_summary": resolution.chat
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
    message: Option<&PreparedMessage>,
    card: Option<&serde_json::Value>,
    resolution: &TargetResolution,
    json_output: bool,
) -> Result<(), CliError> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "posted": false,
                "dry_run": true,
                "resolved": true,
                "requires_confirmation": requires_confirmation(resolution),
                "confirm_thread_id": resolution.thread_id,
                "target": target,
                "channel": resolution.thread_id,
                "thread_id": resolution.thread_id,
                "channel_summary": resolution.chat,
                "message": message.map(|message| json!({
                    "content_type": "RichText/Html",
                    "format": message.format.as_str(),
                    "text_length": message.input_chars,
                    "html_length": message.html_chars(),
                    "html_escaped": message.html_escaped(),
                    "markdown_converted": message.markdown_converted()
                })),
                "card": card.map(card_summary)
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

fn message_body(message: Option<&str>, read_stdin: bool) -> Result<Option<String>, CliError> {
    match (message, read_stdin) {
        (Some(message), false) => {
            validate_message_size(message)?;
            Ok(Some(message.to_string()))
        }
        (None, true) => {
            let mut body = String::new();
            std::io::stdin()
                .take(MAX_MESSAGE_BYTES as u64 + 1)
                .read_to_string(&mut body)?;
            validate_message_size(&body)?;
            Ok(Some(body))
        }
        (None, false) => Ok(None),
        (Some(_), true) => unreachable!("message arguments are prevalidated"),
    }
}

fn card_body(card_json: Option<&str>) -> Result<Option<serde_json::Value>, CliError> {
    let Some(path) = card_json else {
        return Ok(None);
    };
    let content = fs::read_to_string(path).map_err(|error| {
        CliError::structured(
            "invalid_card_json",
            format!("could not read Adaptive Card JSON file: {error}"),
            json!({ "path": path }),
            2,
        )
    })?;
    let content = content.trim_start_matches('\u{feff}');
    validate_card_size(content)?;
    let card = serde_json::from_str::<serde_json::Value>(content).map_err(|error| {
        CliError::structured(
            "invalid_card_json",
            format!("Adaptive Card JSON is invalid: {error}"),
            json!({ "path": path }),
            2,
        )
    })?;
    validate_adaptive_card(&card)?;
    Ok(Some(card))
}

fn validate_message_args(
    message: Option<&str>,
    read_stdin: bool,
    format: MessageFormat,
    card_json: Option<&str>,
    dry_run: bool,
) -> Result<(), CliError> {
    if message.is_some() && read_stdin {
        return Err(CliError::structured(
            "invalid_arguments",
            "MESSAGE cannot be provided together with --stdin",
            json!({}),
            2,
        ));
    }
    if card_json.is_some() && (message.is_some() || read_stdin) {
        return Err(CliError::structured(
            "invalid_arguments",
            "--card-json cannot be provided together with MESSAGE or --stdin",
            json!({}),
            2,
        ));
    }
    if card_json.is_some() && !matches!(format, MessageFormat::Text) {
        return Err(CliError::structured(
            "invalid_arguments",
            "--format cannot be provided together with --card-json",
            json!({ "format": format.as_str() }),
            2,
        ));
    }
    if message.is_none() && !read_stdin && card_json.is_none() && !dry_run {
        return Err(CliError::structured(
            "invalid_arguments",
            "MESSAGE is required unless --stdin or --card-json is used",
            json!({}),
            2,
        ));
    }
    Ok(())
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

fn validate_card_size(card: &str) -> Result<(), CliError> {
    if card.len() <= MAX_CARD_BYTES {
        Ok(())
    } else {
        Err(CliError::structured(
            "card_too_large",
            format!("Adaptive Card JSON must be at most {MAX_CARD_BYTES} bytes"),
            json!({ "max_bytes": MAX_CARD_BYTES }),
            2,
        ))
    }
}

fn validate_adaptive_card(card: &serde_json::Value) -> Result<(), CliError> {
    if !card.is_object() {
        return Err(CliError::structured(
            "invalid_card_json",
            "Adaptive Card JSON must be an object",
            json!({}),
            2,
        ));
    }
    if card.get("type").and_then(serde_json::Value::as_str) != Some("AdaptiveCard") {
        return Err(CliError::structured(
            "invalid_card_json",
            "Adaptive Card JSON must have type \"AdaptiveCard\"",
            json!({}),
            2,
        ));
    }
    Ok(())
}

fn experimental_card_post_enabled() -> bool {
    std::env::var_os(EXPERIMENTAL_CARD_POST_ENV).is_some()
}

fn unsupported_card_post_error(
    channel: &str,
    card: &serde_json::Value,
    resolution: Option<&TargetResolution>,
) -> CliError {
    CliError::structured(
        "unsupported_card_post",
        "Adaptive Card posts over Teams chatService require RichText/Media_Card, which the current user client is not allowed to send",
        json!({
            "target": channel,
            "thread_id": resolution.map(|resolution| resolution.thread_id.as_str()),
            "card": card_summary(card),
            "env_override": EXPERIMENTAL_CARD_POST_ENV,
            "observed": {
                "attachments_payload": "accepted but stored as a blank message",
                "swift_media_card": "rejected with Client not allowed to send RichText/Media_Card message",
                "swift_richtext_html": "accepted but stored as plain summary text"
            }
        }),
        2,
    )
}

fn card_summary(card: &serde_json::Value) -> serde_json::Value {
    json!({
        "content_type": messages::ADAPTIVE_CARD_CONTENT_TYPE,
        "json_bytes": serde_json::to_string(card).map(|text| text.len()).unwrap_or(0),
        "version": card.get("version").and_then(serde_json::Value::as_str),
        "body_elements": card
            .get("body")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .unwrap_or(0),
        "actions": card
            .get("actions")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .unwrap_or(0)
    })
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
        "post channel --json requires --confirm-thread-id for resolved name/title targets",
        json!({
            "target": resolution.target,
            "thread_id": resolution.thread_id,
            "source": resolution.source
        }),
        2,
    ))
}

fn requires_confirmation(resolution: &TargetResolution) -> bool {
    !matches!(
        resolution.source,
        TargetSource::RawThreadId | TargetSource::Alias
    )
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
        .unwrap_or("(raw channel id)");
    format!("{title} {}", resolution.thread_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::chats::ChatKind;

    #[test]
    fn json_post_requires_confirmation_for_lookup_targets() {
        let resolution = TargetResolution {
            target: "Announcements".into(),
            thread_id: "19:channel@thread.tacv2".into(),
            source: TargetSource::FreshChats,
            chat: None,
        };

        let error = require_confirmation_for_json_post(&resolution, None, false, true)
            .expect_err("confirmation required");

        assert_eq!(error.code(), "confirmation_required");
    }

    #[test]
    fn dry_run_json_shape_uses_channel_fields() {
        let resolution = TargetResolution {
            target: "Announcements".into(),
            thread_id: "19:channel@thread.tacv2".into(),
            source: TargetSource::RawThreadId,
            chat: Some(target::TargetCandidate {
                id: "19:channel@thread.tacv2".into(),
                kind: ChatKind::Channel,
                title: Some("Announcements".into()),
                members: Vec::new(),
            }),
        };

        let message = prepare_message("hello", MessageFormat::Text);
        assert!(print_dry_run("Announcements", Some(&message), None, &resolution, true).is_ok());
    }

    #[test]
    fn card_body_accepts_utf8_bom() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("card.json");
        fs::write(
            &path,
            "\u{feff}{\"type\":\"AdaptiveCard\",\"version\":\"1.2\",\"body\":[]}",
        )
        .expect("write");

        let card = card_body(Some(path.to_str().expect("utf8")))
            .expect("card")
            .expect("some");

        assert_eq!(card["type"], "AdaptiveCard");
    }
}
