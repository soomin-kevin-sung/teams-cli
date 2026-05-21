use crate::api::{chats, client::ApiClient, messages};
use crate::auth::{Session, USER_AGENT};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::chat_ref::{resolve, ChatRef};
use serde_json::json;
use std::fs;

pub async fn run(chat: &str, message: &str, json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let thread_id = resolve_send_target(&api, &paths, chat).await?;
    let sent = messages::send_message(&api, &thread_id, message).await?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": sent.id,
                "client_message_id": sent.client_message_id,
                "chat": thread_id
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

async fn resolve_send_target(
    api: &ApiClient,
    paths: &AppPaths,
    target: &str,
) -> Result<String, CliError> {
    match resolve(target, &paths.aliases) {
        ChatRef::ThreadId(thread_id) => Ok(thread_id),
        ChatRef::Lookup(value) => lookup_thread_id(api, paths, &value).await,
    }
}

async fn lookup_thread_id(
    api: &ApiClient,
    paths: &AppPaths,
    target: &str,
) -> Result<String, CliError> {
    if is_self_target(target) {
        return lookup_self_thread_id(api, paths).await;
    }

    let cached = load_cached_chats(paths)?;
    match unique_match(target, &cached) {
        Ok(Some(thread_id)) => return Ok(thread_id),
        Ok(None) => {}
        Err(error) => tracing::debug!("cached chat target lookup failed: {error}"),
    }

    let fresh = chats::list_chats(api, 100).await?;
    cache_chats(paths, &fresh)?;
    if let Some(thread_id) = unique_match(target, &fresh)? {
        return Ok(thread_id);
    }

    Err(CliError::Other(format!(
        "could not resolve chat target '{target}'. Pass a raw 19:… thread id, define an alias in {}, or run `teams list-chats -n 100 --json` to inspect available chats.",
        paths.aliases.display()
    )))
}

async fn lookup_self_thread_id(api: &ApiClient, paths: &AppPaths) -> Result<String, CliError> {
    let cached = load_cached_chats(paths)?;
    if let Some(thread_id) = find_self_thread_id(&cached) {
        return Ok(thread_id);
    }

    let fresh = chats::list_chats(api, 100).await?;
    cache_chats(paths, &fresh)?;
    if let Some(thread_id) = find_self_thread_id(&fresh) {
        return Ok(thread_id);
    }

    Err(CliError::Other(
        "could not resolve self chat. Run `teams list-chats -n 100 --json` and look for the Teams self notes thread, or pass `48:notes` directly if your tenant exposes it."
            .to_string(),
    ))
}

fn load_cached_chats(paths: &AppPaths) -> Result<Vec<chats::ChatSummary>, CliError> {
    let path = paths.cache_dir.join("chats.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn cache_chats(paths: &AppPaths, chats: &[chats::ChatSummary]) -> Result<(), CliError> {
    fs::create_dir_all(&paths.cache_dir)?;
    fs::write(
        paths.cache_dir.join("chats.json"),
        serde_json::to_string_pretty(chats)?,
    )?;
    Ok(())
}

fn unique_match(target: &str, chats: &[chats::ChatSummary]) -> Result<Option<String>, CliError> {
    let mut all_matches = chats
        .iter()
        .filter(|chat| chat_matches_target(chat, target))
        .collect::<Vec<_>>();
    all_matches.dedup_by(|left, right| left.id == right.id);

    let matches = all_matches
        .iter()
        .copied()
        .filter(is_sendable_chat)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => Ok(None),
        [chat] => Ok(Some(chat.id.clone())),
        _ => Err(CliError::Other(ambiguous_target_message(target, &matches))),
    }
    .and_then(|result| {
        if result.is_none() && !all_matches.is_empty() {
            Err(CliError::Other(format!(
                "chat target '{target}' matched only channel/system entries. Sending to channels is not implemented yet; use an existing 1:1 or group chat."
            )))
        } else {
            Ok(result)
        }
    })
}

fn is_sendable_chat(chat: &&chats::ChatSummary) -> bool {
    matches!(
        chat.kind,
        chats::ChatKind::OneToOne | chats::ChatKind::Group
    )
}

fn is_self_target(target: &str) -> bool {
    matches!(
        normalize(target).as_str(),
        "me" | "self" | "myself" | "notes" | "self notes" | "saved messages" | "chat with self"
    )
}

fn find_self_thread_id(chats: &[chats::ChatSummary]) -> Option<String> {
    chats
        .iter()
        .find(|chat| chat.id.eq_ignore_ascii_case("48:notes"))
        .or_else(|| chats.iter().find(|chat| is_self_only_one_to_one(chat)))
        .map(|chat| chat.id.clone())
}

fn is_self_only_one_to_one(chat: &chats::ChatSummary) -> bool {
    matches!(chat.kind, chats::ChatKind::OneToOne)
        && !chat.members.is_empty()
        && chat.members.iter().all(is_self_member)
}

fn chat_matches_target(chat: &chats::ChatSummary, target: &str) -> bool {
    let target_norm = normalize(target);
    if target_norm.is_empty() {
        return false;
    }

    if target.contains('@') {
        return chat.members.iter().any(|member| {
            !is_self_member(member)
                && member
                    .user_principal_name
                    .as_deref()
                    .is_some_and(|upn| normalize(upn) == target_norm)
        });
    }

    chat.title
        .as_deref()
        .is_some_and(|title| normalize(title) == target_norm)
        || chat.members.iter().any(|member| {
            !is_self_member(member)
                && (member
                    .display_name
                    .as_deref()
                    .is_some_and(|name| normalize(name) == target_norm)
                    || member
                        .user_principal_name
                        .as_deref()
                        .is_some_and(|upn| normalize(upn) == target_norm))
        })
}

fn is_self_member(member: &chats::ChatMember) -> bool {
    member
        .role
        .as_deref()
        .is_some_and(|role| role.eq_ignore_ascii_case("self"))
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

fn ambiguous_target_message(target: &str, matches: &[&chats::ChatSummary]) -> String {
    let candidates = matches
        .iter()
        .take(10)
        .map(|chat| {
            format!(
                "- {} ({}) {}",
                chat.title.as_deref().unwrap_or("(untitled)"),
                chat.kind,
                chat.id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "chat target '{target}' matched multiple chats. Use a raw thread id or define an alias.\n{candidates}"
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::chats::{ChatKind, ChatMember, ChatSummary};

    fn chat(
        id: &str,
        kind: ChatKind,
        title: Option<&str>,
        members: Vec<ChatMember>,
    ) -> ChatSummary {
        ChatSummary {
            id: id.to_string(),
            kind,
            title: title.map(ToString::to_string),
            last_message_at: None,
            last_message_preview: None,
            members,
        }
    }

    fn member(display_name: &str, upn: &str) -> ChatMember {
        ChatMember {
            display_name: Some(display_name.to_string()),
            user_principal_name: Some(upn.to_string()),
            ..Default::default()
        }
    }

    fn self_member(display_name: &str, upn: &str) -> ChatMember {
        ChatMember {
            role: Some("Self".to_string()),
            ..member(display_name, upn)
        }
    }

    #[test]
    fn matches_one_to_one_by_email() {
        let chats = vec![chat(
            "19:peer_self@unq.gbl.spaces",
            ChatKind::OneToOne,
            Some("Peer"),
            vec![member("Peer", "peer@example.com")],
        )];

        assert_eq!(
            unique_match("peer@example.com", &chats).expect("match"),
            Some("19:peer_self@unq.gbl.spaces".to_string())
        );
    }

    #[test]
    fn ignores_signed_in_user_member_when_matching() {
        let chats = vec![
            chat(
                "19:a_self@unq.gbl.spaces",
                ChatKind::OneToOne,
                Some("Peer A"),
                vec![
                    self_member("Current User", "me@example.com"),
                    member("Peer A", "a@example.com"),
                ],
            ),
            chat(
                "19:b_self@unq.gbl.spaces",
                ChatKind::OneToOne,
                Some("Peer B"),
                vec![
                    self_member("Current User", "me@example.com"),
                    member("Peer B", "b@example.com"),
                ],
            ),
        ];

        assert_eq!(unique_match("Current User", &chats).expect("match"), None);
        assert_eq!(unique_match("me@example.com", &chats).expect("match"), None);
    }

    #[test]
    fn rejects_ambiguous_display_names() {
        let chats = vec![
            chat(
                "19:a_self@unq.gbl.spaces",
                ChatKind::OneToOne,
                Some("Alex"),
                vec![member("Alex", "a@example.com")],
            ),
            chat(
                "19:b_self@unq.gbl.spaces",
                ChatKind::OneToOne,
                Some("Alex"),
                vec![member("Alex", "b@example.com")],
            ),
        ];

        assert!(unique_match("Alex", &chats)
            .expect_err("ambiguous")
            .to_string()
            .contains("matched multiple chats"));
    }

    #[test]
    fn refuses_channel_title_matches() {
        let chats = vec![chat(
            "19:channel@thread.tacv2",
            ChatKind::Channel,
            Some("Announcements"),
            Vec::new(),
        )];

        assert!(unique_match("Announcements", &chats)
            .expect_err("channel")
            .to_string()
            .contains("channels is not implemented"));
    }

    #[test]
    fn recognizes_self_targets() {
        assert!(is_self_target("me"));
        assert!(is_self_target("Self"));
        assert!(is_self_target("self notes"));
        assert!(!is_self_target("current user"));
    }

    #[test]
    fn resolves_system_notes_as_self_thread() {
        let chats = vec![chat("48:notes", ChatKind::System, None, Vec::new())];

        assert_eq!(find_self_thread_id(&chats), Some("48:notes".to_string()));
    }

    #[test]
    fn resolves_self_only_one_to_one_as_self_thread() {
        let chats = vec![chat(
            "19:self_self@unq.gbl.spaces",
            ChatKind::OneToOne,
            Some("Current User"),
            vec![self_member("Current User", "me@example.com")],
        )];

        assert_eq!(
            find_self_thread_id(&chats),
            Some("19:self_self@unq.gbl.spaces".to_string())
        );
    }
}
