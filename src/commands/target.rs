use crate::api::{chats, client::ApiClient};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::chat_cache;
use crate::util::chat_ref::{resolve, ChatRef};
use serde::Serialize;
use serde_json::json;

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
    lookup_thread_id(api, paths, target).await
}

pub async fn resolve_channel_target(
    api: &ApiClient,
    paths: &AppPaths,
    target: &str,
) -> Result<TargetResolution, CliError> {
    if let Some(resolution) = resolve_local_channel_target(paths, target)? {
        return Ok(resolution);
    }
    lookup_channel_thread_id(api, paths, target).await
}

pub fn resolve_local_channel_target(
    paths: &AppPaths,
    target: &str,
) -> Result<Option<TargetResolution>, CliError> {
    let Some(resolution) = resolve_local_target(paths, target)? else {
        return Ok(None);
    };
    validate_channel_resolution(target, &resolution)?;
    Ok(Some(resolution))
}

pub fn resolve_local_target(
    paths: &AppPaths,
    target: &str,
) -> Result<Option<TargetResolution>, CliError> {
    match resolve(target, &paths.aliases)? {
        ChatRef::ThreadId(thread_id) => Ok(Some(thread_resolution(target, thread_id))),
        ChatRef::Lookup(_) => Ok(None),
    }
}

async fn lookup_thread_id(
    api: &ApiClient,
    paths: &AppPaths,
    target: &str,
) -> Result<TargetResolution, CliError> {
    let owner = chat_cache::owner_from_api(api).await;
    let cached = chat_cache::load_for_owner(paths, &owner)?;
    if is_self_target(target) {
        if let Some(chat) = find_self_chat(&cached) {
            return Ok(target_resolution(target, chat, TargetSource::SelfTarget));
        }
        let fresh = chats::list_chats(api, 100).await?;
        chat_cache::write(paths, &owner, &fresh)?;
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

    if let Some(resolution) = resolve_from_chats(target, &cached, TargetSource::CachedChats)? {
        return Ok(resolution);
    }

    let fresh = chats::list_chats(api, 100).await?;
    chat_cache::write(paths, &owner, &fresh)?;
    if let Some(resolution) = resolve_from_chats(target, &fresh, TargetSource::FreshChats)? {
        return Ok(resolution);
    }

    Err(CliError::structured(
        "target_not_found",
        format!(
            "could not resolve chat target '{target}'. Pass a raw 19:... thread id, define an alias, or run `teams list-chats -n 100 --json` to inspect available chats."
        ),
        json!({
            "target": target,
            "alias_file": "aliases.toml"
        }),
        2,
    ))
}

async fn lookup_channel_thread_id(
    api: &ApiClient,
    paths: &AppPaths,
    target: &str,
) -> Result<TargetResolution, CliError> {
    let owner = chat_cache::owner_from_api(api).await;
    let cached = chat_cache::load_for_owner(paths, &owner)?;
    if let Some(resolution) = resolve_from_channels(target, &cached, TargetSource::CachedChats)? {
        return Ok(resolution);
    }

    let fresh = chats::list_chats(api, 200).await?;
    chat_cache::write(paths, &owner, &fresh)?;
    if let Some(resolution) = resolve_from_channels(target, &fresh, TargetSource::FreshChats)? {
        return Ok(resolution);
    }

    Err(CliError::structured(
        "channel_target_not_found",
        format!(
            "could not resolve channel target '{target}'. Pass a raw 19:...@thread.tacv2 id, define an alias, or run `teams list-chats -n 200 --json` to inspect available channels."
        ),
        json!({
            "target": target,
            "alias_file": "aliases.toml"
        }),
        2,
    ))
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

fn resolve_from_channels(
    target: &str,
    chats: &[chats::ChatSummary],
    source: TargetSource,
) -> Result<Option<TargetResolution>, CliError> {
    let all_matches = matching_chats(target, chats);
    let matches = all_matches
        .iter()
        .copied()
        .filter(is_channel_chat)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] if all_matches.is_empty() => Ok(None),
        [] => Err(non_channel_target_error(target, &all_matches)),
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

fn is_channel_chat(chat: &&chats::ChatSummary) -> bool {
    matches!(chat.kind, chats::ChatKind::Channel)
}

fn validate_channel_resolution(
    target: &str,
    resolution: &TargetResolution,
) -> Result<(), CliError> {
    if is_channel_thread_id(&resolution.thread_id) {
        Ok(())
    } else {
        Err(CliError::structured(
            "invalid_channel_target",
            format!(
                "channel target '{target}' resolved to a non-channel thread id. Use a 19:...@thread.tacv2 channel id or a channel alias."
            ),
            json!({
                "target": target,
                "thread_id": resolution.thread_id,
                "expected_suffix": "@thread.tacv2"
            }),
            2,
        ))
    }
}

fn is_channel_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with("19:") && thread_id.ends_with("@thread.tacv2")
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
            "chat target '{target}' matched only channel/system entries. Use `teams post channel` for channel posts, or use an existing 1:1 or group chat."
        ),
        json!({
            "target": target,
            "candidates": candidates(matches)
        }),
        2,
    )
}

fn non_channel_target_error(target: &str, matches: &[&chats::ChatSummary]) -> CliError {
    CliError::structured(
        "invalid_channel_target",
        format!(
            "channel target '{target}' matched only non-channel entries. Use a channel thread id, channel alias, or exact cached channel title."
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
        assert!(error.to_string().contains("teams post channel"));
        assert_eq!(error.code(), "unsupported_target");
    }

    #[test]
    fn resolves_channel_title_for_channel_posts() {
        let chats = vec![chat(
            "19:channel@thread.tacv2",
            ChatKind::Channel,
            Some("Announcements"),
            Vec::new(),
        )];

        assert_eq!(
            resolve_from_channels("Announcements", &chats, TargetSource::CachedChats)
                .expect("match")
                .expect("some")
                .thread_id,
            "19:channel@thread.tacv2"
        );
    }

    #[test]
    fn rejects_non_channel_title_for_channel_posts() {
        let chats = vec![chat(
            "19:chat@thread.v2",
            ChatKind::Group,
            Some("Announcements"),
            Vec::new(),
        )];

        let error = resolve_from_channels("Announcements", &chats, TargetSource::CachedChats)
            .expect_err("not a channel");

        assert_eq!(error.code(), "invalid_channel_target");
    }

    #[test]
    fn validates_raw_channel_thread_ids() {
        let resolution = TargetResolution {
            target: "19:channel@thread.tacv2".into(),
            thread_id: "19:channel@thread.tacv2".into(),
            source: TargetSource::RawThreadId,
            chat: None,
        };
        assert!(validate_channel_resolution("19:channel@thread.tacv2", &resolution).is_ok());

        let resolution = TargetResolution {
            thread_id: "19:chat@thread.v2".into(),
            ..resolution
        };
        assert_eq!(
            validate_channel_resolution("19:chat@thread.v2", &resolution)
                .expect_err("not channel")
                .code(),
            "invalid_channel_target"
        );
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
