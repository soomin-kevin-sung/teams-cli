use crate::api::{client::ApiClient, messages};
use crate::auth::{Session, USER_AGENT};
use crate::commands::target::{self, TargetResolution};
use crate::config::AppPaths;
use crate::error::CliError;
use chrono::{DateTime, Utc};
use serde_json::json;

const MAX_READ_LIMIT: usize = 100;

pub async fn run(
    target: &str,
    limit: usize,
    since: Option<&str>,
    before: Option<&str>,
    json_output: bool,
) -> Result<(), CliError> {
    let effective_limit = limit.min(MAX_READ_LIMIT);
    let since = parse_rfc3339_filter("since", since)?;
    let before = parse_rfc3339_filter("before", before)?;
    let has_time_filter = since.is_some() || before.is_some();
    let fetch_limit = if effective_limit == 0 {
        0
    } else if has_time_filter {
        MAX_READ_LIMIT
    } else {
        effective_limit
    };
    let paths = AppPaths::resolve()?;
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let resolution = target::resolve_send_target(&api, &paths, target).await?;
    let own_oid = api.user_oid().await;
    let mut messages = messages::read_messages(&api, &resolution.thread_id, fetch_limit).await?;
    for message in &mut messages {
        let is_self = message
            .sender
            .as_ref()
            .and_then(|sender| sender.object_id.as_deref())
            .is_some_and(|oid| oid.eq_ignore_ascii_case(&own_oid));
        message.sender_is_self = Some(is_self);
    }
    messages.retain(|message| message_matches_time_filter(message.created_at, since, before));
    messages.truncate(effective_limit);

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
                "limit": effective_limit,
                "requested_limit": limit,
                "fetched_limit": fetch_limit,
                "since": since,
                "before": before,
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
            println!(
                "[{}] {}: {}",
                sanitize_terminal(&created_at),
                sanitize_terminal(sender),
                sanitize_terminal(text)
            );
        }
    }

    Ok(())
}

fn parse_rfc3339_filter(
    name: &'static str,
    value: Option<&str>,
) -> Result<Option<DateTime<Utc>>, CliError> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|time| time.with_timezone(&Utc))
                .map_err(|_| {
                    CliError::structured(
                        "invalid_timestamp",
                        format!("--{name} must be an RFC3339 timestamp"),
                        json!({ "field": name, "value": value }),
                        2,
                    )
                })
        })
        .transpose()
}

fn message_matches_time_filter(
    created_at: Option<DateTime<Utc>>,
    since: Option<DateTime<Utc>>,
    before: Option<DateTime<Utc>>,
) -> bool {
    let Some(created_at) = created_at else {
        return since.is_none() && before.is_none();
    };
    let after_since = match since {
        Some(since) => created_at >= since,
        None => true,
    };
    let before_end = match before {
        Some(before) => created_at < before,
        None => true,
    };
    after_since && before_end
}

fn target_label(resolution: &TargetResolution) -> String {
    let title = resolution
        .chat
        .as_ref()
        .and_then(|chat| chat.title.as_deref())
        .unwrap_or("(raw thread id)");
    format!("{title} {}", resolution.thread_id)
}

fn sanitize_terminal(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_sanitizer_removes_control_sequences() {
        assert_eq!(sanitize_terminal("hi\u{1b}[31m\rthere"), "hi [31m there");
    }

    #[test]
    fn time_filter_is_inclusive_since_exclusive_before() {
        let time = DateTime::parse_from_rfc3339("2026-05-21T00:00:00Z")
            .expect("time")
            .with_timezone(&Utc);
        assert!(message_matches_time_filter(Some(time), Some(time), None));
        assert!(!message_matches_time_filter(Some(time), None, Some(time)));
    }
}
