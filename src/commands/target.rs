use crate::api::{chats, client::ApiClient};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::chat_ref::{resolve, ChatRef};
use serde::Serialize;
use serde_json::json;
use std::fs;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSource {
    RawThreadId,
    Alias,
    CachedChats,
    FreshChats,
    SelfTarget,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetCandidate {
    pub id: String,
    pub kind: chats::ChatKind,
    pub title: Option<String>,
    pub members: Vec<chats::ChatMember>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetResolution {
    pub target: String,
    pub thread_id: String,
    pub source: TargetSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<TargetCandidate>,
}

pub async fn resolve_send_target(
    api: &ApiClient,
    paths: &AppPaths,
    target: &str,
) -> Result<TargetResolution, CliError> {
    if let Some(resolution) = resolve_local_target(paths, target)? {
        return Ok(resolution);
    }
    lookup_fresh_thread_id(api, paths, target).await
}

pub fn resolve_local_target(
    paths: &AppPaths,
    target: &str,
) -> Result<Option<TargetResolution>, CliError> {
    match resolve(target, &paths.aliases)? {
        ChatRef::ThreadId(thread_id) => Ok(Some(thread_resolution(target, thread_id))),
        ChatRef::Lookup(value) => resolve_cached_lookup(paths, &value),
    }
}

async fn lookup_fresh_thread_id(
    api: &ApiClient,
    paths: &AppPaths,
    target: &str,
) -> Result<TargetResolution, CliError> {
    if is_self_target(target) {
        let fresh = chats::list_chats(api, 100).await?;
        cache_chats(paths, &fresh)?;
        if let Some(chat) = find_self_chat(&fresh) {
            return Ok(target_resolution(target, chat, TargetSource::SelfTarget));
        }
        return Err(CliError::structured(
            "self_chat_not_found",
            "could not resolve self chat. Run `teams list-chats -n 100 --json` and look for the Teams self notes thread, or pass `48:notes` directly if your tenant exposes it.",
            json!({ "target": target, "expected_thread_id": "48:notes" }),
            2,
        ));
    }

    let fresh = chats::list_chats(api, 100).await?;
    cache_chats(paths, &fresh)?;
    if let Some(resolution) = resolve_from_chats(target, &fresh, TargetSource::FreshChats)? {
        return Ok(resolution);
    }

    Err(CliError::structured(
        "target_not_found",
        format!(
            "could not resolve chat target '{target}'. Pass a raw 19:... thread id, define an alias in {}, or run `teams list-chats -n 100 --json` to inspect available chats.",
            paths.aliases.display()
        ),
        json!({
            "target": target,
            "aliases_path": paths.aliases.display().to_string()
        }),
        2,
    ))
}

fn resolve_cached_lookup(
    paths: &AppPaths,
    target: &str,
) -> Result<Option<TargetResolution>, CliError> {
    let cached = load_cached_chats(paths)?;
    if is_self_target(target) {
        return Ok(find_self_chat(&cached)
            .map(|chat| target_resolution(target, chat, TargetSource::SelfTarget)));
    }
    resolve_from_chats(target, &cached, TargetSource::CachedChats)
}

fn thread_resolution(target: &str, thread_id: String) -> TargetResolution {
    TargetResolution {
        target: target.to_string(),
        thread_id,
        source: if target.starts_with("19:") || target.starts_with("48:") {
            TargetSource::RawThreadId
        } else {
            TargetSource::Alias
        },
        chat: None,
    }
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

fn resolve_from_chats(
    target: &str,
    chats: &[chats::ChatSummary],
    source: TargetSource,
) -> Result<Option<TargetResolution>, CliError> {
    let all_matches = matching_chats(target, chats);
    let matches = all_matches
        .iter()
        .copied()
        .filter(is_sendable_chat)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] if all_matches.is_empty() => Ok(None),
        [] => Err(unsupported_target_error(target, &all_matches)),
        [chat] => Ok(Some(target_resolution(target, chat, source))),
        _ => Err(ambiguous_target_error(target, &matches)),
    }
}

fn matching_chats<'a>(
    target: &str,
    chats: &'a [chats::ChatSummary],
) -> Vec<&'a chats::ChatSummary> {
    let mut matches = Vec::new();
    for chat in chats
        .iter()
        .filter(|chat| chat_matches_target(chat, target))
    {
        if !matches
            .iter()
            .any(|existing: &&chats::ChatSummary| existing.id == chat.id)
        {
            matches.push(chat);
        }
    }
    matches
}

fn target_resolution(
    target: &str,
    chat: &chats::ChatSummary,
    source: TargetSource,
) -> TargetResolution {
    TargetResolution {
        target: target.to_string(),
        thread_id: chat.id.clone(),
        source,
        chat: Some(target_candidate(chat)),
    }
}

fn target_candidate(chat: &chats::ChatSummary) -> TargetCandidate {
    TargetCandidate {
        id: chat.id.clone(),
        kind: chat.kind.clone(),
        title: chat.title.clone(),
        members: chat.members.clone(),
    }
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

fn find_self_chat(chats: &[chats::ChatSummary]) -> Option<&chats::ChatSummary> {
    chats
        .iter()
        .find(|chat| chat.id.eq_ignore_ascii_case("48:notes"))
        .or_else(|| chats.iter().find(|chat| is_self_only_one_to_one(chat)))
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

fn ambiguous_target_error(target: &str, matches: &[&chats::ChatSummary]) -> CliError {
    CliError::structured(
        "ambiguous_target",
        ambiguous_target_message(target, matches),
        json!({
            "target": target,
            "candidates": candidates(matches)
        }),
        2,
    )
}

fn unsupported_target_error(target: &str, matches: &[&chats::ChatSummary]) -> CliError {
    CliError::structured(
        "unsupported_target",
        format!(
            "chat target '{target}' matched only channel/system entries. Sending to channels is not implemented yet; use an existing 1:1 or group chat."
        ),
        json!({
            "target": target,
            "candidates": candidates(matches)
        }),
        2,
    )
}

fn candidates(matches: &[&chats::ChatSummary]) -> Vec<TargetCandidate> {
    matches
        .iter()
        .take(10)
        .map(|chat| target_candidate(chat))
        .collect()
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
            resolve_from_chats("peer@example.com", &chats, TargetSource::CachedChats)
                .expect("match")
                .expect("some")
                .thread_id,
            "19:peer_self@unq.gbl.spaces"
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

        assert!(
            resolve_from_chats("Current User", &chats, TargetSource::CachedChats)
                .expect("match")
                .is_none()
        );
        assert!(
            resolve_from_chats("me@example.com", &chats, TargetSource::CachedChats)
                .expect("match")
                .is_none()
        );
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

        let error =
            resolve_from_chats("Alex", &chats, TargetSource::CachedChats).expect_err("ambiguous");
        assert!(error.to_string().contains("matched multiple chats"));
        assert_eq!(error.code(), "ambiguous_target");
    }

    #[test]
    fn refuses_channel_title_matches() {
        let chats = vec![chat(
            "19:channel@thread.tacv2",
            ChatKind::Channel,
            Some("Announcements"),
            Vec::new(),
        )];

        let error = resolve_from_chats("Announcements", &chats, TargetSource::CachedChats)
            .expect_err("channel");
        assert!(error.to_string().contains("channels is not implemented"));
        assert_eq!(error.code(), "unsupported_target");
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

        assert_eq!(
            find_self_chat(&chats).map(|chat| chat.id.as_str()),
            Some("48:notes")
        );
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
            find_self_chat(&chats).map(|chat| chat.id.as_str()),
            Some("19:self_self@unq.gbl.spaces")
        );
    }
}
