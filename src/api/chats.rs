use super::client::{ApiClient, ApiError, AuthStyle};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSummary {
    pub id: String,
    pub kind: ChatKind,
    pub title: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_message_preview: Option<String>,
    pub members: Vec<ChatMember>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMember {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_principal_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl ChatMember {
    pub fn label(&self) -> Option<String> {
        self.display_name
            .as_deref()
            .or(self.user_principal_name.as_deref())
            .or(self.mri.as_deref())
            .or(self.object_id.as_deref())
            .map(ToString::to_string)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatKind {
    OneToOne,
    Group,
    Channel,
    System,
    Unknown,
}

impl std::fmt::Display for ChatKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::OneToOne => "1:1",
            Self::Group => "group",
            Self::Channel => "channel",
            Self::System => "system",
            Self::Unknown => "unknown",
        };
        formatter.write_str(text)
    }
}

pub async fn list_chats(api: &ApiClient, limit: usize) -> Result<Vec<ChatSummary>, ApiError> {
    let endpoints = vec![
        (
            "https://teams.microsoft.com/api/csa/api/v1/teams/users/me/updates?isPrefetch=false&enableMembershipSummary=true".to_string(),
            AuthStyle::BearerSkype,
        ),
        (
            format!(
                "{}/api/v1/teams/users/me/updates?isPrefetch=false&enableMembershipSummary=true",
                api.csa_base().await
            ),
            AuthStyle::BearerSkype,
        ),
        (
            format!(
                "{}/api/v1/teams/users/me/groupchats?skipMeetingChats=true&pageSize={limit}",
                api.csa_base().await
            ),
            AuthStyle::BearerSkype,
        ),
        (
            format!(
                "{}/api/v1/teams/users/ME/conversations?view=mychats&pageSize={limit}",
                api.csa_base().await
            ),
            AuthStyle::BearerSkype,
        ),
        (
            format!(
                "{}/api/v2/users/ME/conversations?view=mychats&pageSize={limit}",
                api.chat_svc_agg().await
            ),
            AuthStyle::SkypeHeader,
        ),
        (
            format!(
                "{}/v1/users/ME/conversations?view=mychats&pageSize={limit}",
                api.chat_service().await
            ),
            AuthStyle::SkypeHeader,
        ),
    ];

    let mut last_error = None;
    for (url, style) in endpoints {
        match api.get_json::<serde_json::Value>(&url, style).await {
            Ok(value) => {
                let mut chats: Vec<_> = normalize_chats(&value).into_iter().take(limit).collect();
                enrich_user_metadata(api, &mut chats).await;
                return Ok(chats);
            }
            Err(ApiError::Http { status, body }) if matches!(status, 401 | 403 | 404) => {
                last_error = Some(ApiError::Http { status, body });
            }
            Err(ApiError::NotFound(body)) => last_error = Some(ApiError::NotFound(body)),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| ApiError::NotFound("no chat endpoint succeeded".to_string())))
}

async fn enrich_user_metadata(api: &ApiClient, chats: &mut [ChatSummary]) {
    let own_oid = api.user_oid().await;
    let own_member = ChatMember {
        mri: Some(format!("8:orgid:{own_oid}")),
        object_id: Some(own_oid.clone()),
        display_name: Some(api.display_name().await),
        user_principal_name: Some(api.upn().await),
        role: Some("Self".to_string()),
    };
    let peer_refs = one_to_one_peer_refs(chats, &own_oid);
    let peer_profiles = fetch_user_profiles(api, &peer_refs)
        .await
        .unwrap_or_else(|error| {
            tracing::debug!("batch user profile lookup failed: {error}");
            BTreeMap::new()
        });

    for chat in chats {
        if !matches!(chat.kind, ChatKind::OneToOne) {
            continue;
        }
        let Some(peer_oid) = one_to_one_peer_oid(&chat.id, &own_oid) else {
            continue;
        };

        upsert_member(&mut chat.members, own_member.clone());

        let peer_mri = format!("8:orgid:{peer_oid}");
        let mut peer = ChatMember {
            mri: Some(peer_mri.clone()),
            object_id: Some(peer_oid.clone()),
            ..Default::default()
        };
        if let Some(title) = chat.title.as_deref().filter(|title| !title.is_empty()) {
            peer.display_name = Some(title.to_string());
        }
        if let Some(profile) = peer_profiles
            .get(&peer_mri.to_ascii_lowercase())
            .or_else(|| peer_profiles.get(&peer_oid.to_ascii_lowercase()))
        {
            merge_member(&mut peer, profile);
        } else if let Ok(Some(profile)) = fetch_user_profile(api, &peer_mri, &peer_oid).await {
            merge_member(&mut peer, &profile);
        }
        upsert_member(&mut chat.members, peer.clone());

        if chat.title.as_deref().is_none_or(str::is_empty)
            || chat_title_matches_member(&chat.title, &own_member)
        {
            chat.title = peer
                .display_name
                .clone()
                .or(peer.user_principal_name.clone());
        }
    }
}

fn chat_title_matches_member(title: &Option<String>, member: &ChatMember) -> bool {
    let Some(title) = title.as_deref() else {
        return false;
    };
    let title = title.trim();
    member
        .display_name
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case(title))
        || member
            .user_principal_name
            .as_deref()
            .is_some_and(|upn| upn.eq_ignore_ascii_case(title))
}

fn one_to_one_peer_refs(chats: &[ChatSummary], own_oid: &str) -> Vec<(String, String)> {
    let mut refs = Vec::new();
    for chat in chats {
        if !matches!(chat.kind, ChatKind::OneToOne) {
            continue;
        }
        let Some(peer_oid) = one_to_one_peer_oid(&chat.id, own_oid) else {
            continue;
        };
        let peer_mri = format!("8:orgid:{peer_oid}");
        if !refs.iter().any(|(mri, _)| mri == &peer_mri) {
            refs.push((peer_mri, peer_oid));
        }
    }
    refs
}

fn upsert_member(members: &mut Vec<ChatMember>, incoming: ChatMember) {
    if is_empty_member(&incoming) {
        return;
    }
    if let Some(existing) = members
        .iter_mut()
        .find(|member| members_match(member, &incoming))
    {
        merge_member(existing, &incoming);
        return;
    }
    members.push(incoming);
}

fn members_match(left: &ChatMember, right: &ChatMember) -> bool {
    member_keys(left)
        .iter()
        .any(|key| member_keys(right).contains(key))
}

fn member_keys(member: &ChatMember) -> Vec<String> {
    [
        member.mri.as_deref(),
        member.object_id.as_deref(),
        member.user_principal_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.trim().is_empty())
    .map(|value| value.to_ascii_lowercase())
    .collect()
}

fn merge_member(target: &mut ChatMember, source: &ChatMember) {
    if target.mri.is_none() {
        target.mri = source.mri.clone();
    }
    if target.object_id.is_none() {
        target.object_id = source.object_id.clone();
    }
    if target.display_name.is_none() {
        target.display_name = source.display_name.clone();
    }
    if target.user_principal_name.is_none() {
        target.user_principal_name = source.user_principal_name.clone();
    }
    if target.role.is_none() {
        target.role = source.role.clone();
    }
}

fn is_empty_member(member: &ChatMember) -> bool {
    member.mri.is_none()
        && member.object_id.is_none()
        && member.display_name.is_none()
        && member.user_principal_name.is_none()
        && member.role.is_none()
}

async fn fetch_user_profile(
    api: &ApiClient,
    peer_mri: &str,
    peer_oid: &str,
) -> Result<Option<ChatMember>, ApiError> {
    let chat_service = api.chat_service().await;
    let middle_tier = api.middle_tier().await;
    let encoded_mri = percent_encode_path_segment(peer_mri);
    let encoded_oid = percent_encode_path_segment(peer_oid);
    let candidates = [
        (
            format!("{chat_service}/v1/users/ME/contacts/{encoded_mri}"),
            AuthStyle::SkypeHeader,
        ),
        (
            format!("{middle_tier}/beta/users/{encoded_mri}/profile"),
            AuthStyle::BearerAadPlusSkype,
        ),
        (
            format!("{middle_tier}/beta/users/{encoded_mri}/externalsearchv3?includeTFLUsers=true"),
            AuthStyle::BearerAadPlusSkype,
        ),
        (
            format!("{middle_tier}/beta/users/{encoded_oid}/externalsearchv3?includeTFLUsers=true"),
            AuthStyle::BearerAadPlusSkype,
        ),
    ];

    for (url, style) in candidates {
        tracing::debug!("trying user profile lookup: {url}");
        match api.get_json::<serde_json::Value>(&url, style).await {
            Ok(value) => {
                tracing::debug!("user profile lookup response: {}", json_shape(&value));
                if let Some(mut member) = member_from_lookup_response(&value) {
                    if member.mri.is_none() {
                        member.mri = Some(peer_mri.to_string());
                    }
                    if member.object_id.is_none() {
                        member.object_id = Some(peer_oid.to_string());
                    }
                    return Ok(Some(member));
                }
            }
            Err(error) => {
                tracing::debug!("user profile lookup candidate failed: {error}");
            }
        }
    }
    Ok(None)
}

async fn fetch_user_profiles(
    api: &ApiClient,
    peer_refs: &[(String, String)],
) -> Result<BTreeMap<String, ChatMember>, ApiError> {
    if peer_refs.is_empty() {
        return Ok(BTreeMap::new());
    }

    let middle_tier = api.middle_tier().await;
    let fetch_url = format!(
        "{middle_tier}/beta/users/fetch?isMailAddress=false&canBeSmtpAddress=false&enableGuest=true&includeIBBarredUsers=false&skypeTeamsInfo=true"
    );
    let mris = peer_refs
        .iter()
        .map(|(mri, _)| mri.as_str())
        .collect::<Vec<_>>();
    tracing::debug!("trying batch user profile lookup: {fetch_url}");
    let value = api
        .post_json::<serde_json::Value, _>(&fetch_url, AuthStyle::BearerAadPlusSkype, &mris)
        .await?;
    tracing::debug!("batch user profile lookup response: {}", json_shape(&value));

    let mut profiles = BTreeMap::new();
    for mut member in members_from_lookup_response(&value) {
        if member.object_id.is_none() {
            member.object_id = oid_from_mri(member.mri.as_deref());
        }
        for key in member_keys(&member) {
            profiles.insert(key, member.clone());
        }
    }

    for (mri, oid) in peer_refs {
        if let Some(profile) = profiles
            .get(&mri.to_ascii_lowercase())
            .or_else(|| profiles.get(&oid.to_ascii_lowercase()))
            .cloned()
        {
            profiles.insert(mri.to_ascii_lowercase(), profile.clone());
            profiles.insert(oid.to_ascii_lowercase(), profile);
        }
    }

    Ok(profiles)
}

fn json_shape(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let keys = map.keys().take(8).cloned().collect::<Vec<_>>().join(", ");
            format!("object[{keys}]")
        }
        serde_json::Value::Array(array) => format!("array(len={})", array.len()),
        other => other.to_string(),
    }
}

fn normalize_chats(value: &serde_json::Value) -> Vec<ChatSummary> {
    let user_index = user_index(value);
    candidate_arrays(value)
        .into_iter()
        .flat_map(|array| array.iter())
        .filter_map(|value| normalize_chat(value, &user_index))
        .collect()
}

fn candidate_arrays(value: &serde_json::Value) -> Vec<&Vec<serde_json::Value>> {
    let mut arrays = Vec::new();
    if let Some(array) = value.as_array() {
        arrays.push(array);
    }
    for key in ["value", "conversations", "chats", "items", "threads"] {
        if let Some(array) = value.get(key).and_then(serde_json::Value::as_array) {
            arrays.push(array);
        }
    }
    arrays
}

fn normalize_chat(
    value: &serde_json::Value,
    user_index: &BTreeMap<String, ChatMember>,
) -> Option<ChatSummary> {
    let id = string_at(
        value,
        &["id", "threadId", "conversationId", "chatId", "mri"],
    )?;
    let title = string_at(value, &["topic", "displayName", "title", "name"])
        .or_else(|| value.pointer("/threadProperties/topic").and_then(as_string))
        .or_else(|| value.pointer("/conversation/topic").and_then(as_string))
        .or_else(|| default_system_title(&id));
    let members = extract_members(value, user_index);
    let kind = infer_kind(&id, value, &members);
    let last_message_preview = string_at(
        value,
        &[
            "lastMessagePreview",
            "preview",
            "lastMessageContent",
            "lastMessageText",
        ],
    )
    .or_else(|| value.pointer("/lastMessage/content").and_then(as_string))
    .map(|text| message_preview(&text));
    let last_message_at = first_datetime(
        value,
        &[
            "/lastMessage/originalarrivaltime",
            "/lastMessage/composetime",
            "/properties/lastimreceivedtime",
            "/lastMessageAt",
            "/lastMessageTime",
            "/lastUpdatedTime",
        ],
    );

    Some(ChatSummary {
        id,
        kind,
        title,
        last_message_at,
        last_message_preview,
        members,
    })
}

fn default_system_title(id: &str) -> Option<String> {
    match id {
        "48:notes" => Some("Self notes".to_string()),
        _ => None,
    }
}

fn user_index(value: &serde_json::Value) -> BTreeMap<String, ChatMember> {
    let mut index = BTreeMap::new();
    for key in ["users", "userProfiles", "profiles", "people"] {
        let Some(container) = value.get(key) else {
            continue;
        };
        index_user_container(container, &mut index);
    }
    if let Some(metadata) = value.get("metadata") {
        for key in ["users", "userProfiles", "profiles", "people"] {
            if let Some(container) = metadata.get(key) {
                index_user_container(container, &mut index);
            }
        }
    }
    index
}

fn index_user_container(container: &serde_json::Value, index: &mut BTreeMap<String, ChatMember>) {
    match container {
        serde_json::Value::Array(users) => {
            for user in users {
                index_user_value(None, user, index);
            }
        }
        serde_json::Value::Object(users) => {
            for (key, user) in users {
                index_user_value(Some(key), user, index);
            }
        }
        _ => {}
    }
}

fn index_user_value(
    key: Option<&str>,
    value: &serde_json::Value,
    index: &mut BTreeMap<String, ChatMember>,
) {
    let Some(mut member) = member_from_value(value) else {
        return;
    };
    if member.mri.is_none() {
        if let Some(key) = key.filter(|key| key.starts_with("8:")) {
            member.mri = Some(key.to_string());
        }
    }
    if member.object_id.is_none() {
        if let Some(key) = key.filter(|key| looks_like_guid(key)) {
            member.object_id = Some(key.to_string());
        }
    }
    for key in member_keys(&member) {
        index.insert(key, member.clone());
    }
}

fn string_at(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(as_string))
}

fn as_string(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(ToString::to_string)
}

fn first_datetime(value: &serde_json::Value, pointers: &[&str]) -> Option<DateTime<Utc>> {
    pointers.iter().find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(as_string)
            .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
            .map(|dt| dt.with_timezone(&Utc))
    })
}

fn extract_members(
    value: &serde_json::Value,
    user_index: &BTreeMap<String, ChatMember>,
) -> Vec<ChatMember> {
    let mut members = Vec::new();
    for raw_member in ["members", "roster", "participants"]
        .iter()
        .filter_map(|key| value.get(*key).and_then(serde_json::Value::as_array))
        .flat_map(|members| members.iter())
    {
        if let Some(mut member) = member_from_value(raw_member) {
            if let Some(profile) = member_keys(&member)
                .iter()
                .find_map(|key| user_index.get(key))
            {
                merge_member(&mut member, profile);
            }
            upsert_member(&mut members, member);
        }
    }
    members
}

fn infer_kind(id: &str, value: &serde_json::Value, members: &[ChatMember]) -> ChatKind {
    if id.starts_with("48:") {
        return ChatKind::System;
    }
    if id.contains("@thread.tacv2") {
        return ChatKind::Channel;
    }
    if id.contains("@thread.v2") {
        return ChatKind::Group;
    }
    if id.contains("@unq.gbl.spaces") {
        return ChatKind::OneToOne;
    }
    match string_at(value, &["type", "chatType", "threadType"]).as_deref() {
        Some("OneOnOne") | Some("oneOnOne") => ChatKind::OneToOne,
        Some("Group") | Some("group") => ChatKind::Group,
        Some("Channel") | Some("channel") => ChatKind::Channel,
        Some("System") | Some("system") => ChatKind::System,
        _ if members.len() == 2 => ChatKind::OneToOne,
        _ => ChatKind::Unknown,
    }
}

fn one_to_one_peer_oid(id: &str, own_oid: &str) -> Option<String> {
    let (left, right) = one_to_one_id_parts(id)?;
    if left.eq_ignore_ascii_case(own_oid) {
        Some(right)
    } else {
        Some(left)
    }
}

fn one_to_one_id_parts(id: &str) -> Option<(String, String)> {
    let rest = id.strip_prefix("19:")?.strip_suffix("@unq.gbl.spaces")?;
    let (left, right) = rest.split_once('_')?;
    Some((left.to_string(), right.to_string()))
}

fn member_from_lookup_response(value: &serde_json::Value) -> Option<ChatMember> {
    members_from_lookup_response(value).into_iter().next()
}

fn members_from_lookup_response(value: &serde_json::Value) -> Vec<ChatMember> {
    let mut members = Vec::new();
    if let Some(member) = member_from_value(value) {
        members.push(member);
    }
    for member in candidate_arrays(value)
        .into_iter()
        .flat_map(|array| array.iter())
        .filter_map(member_from_value)
    {
        upsert_member(&mut members, member);
    }
    members
}

fn member_from_value(value: &serde_json::Value) -> Option<ChatMember> {
    if let Some(text) = value.as_str() {
        return Some(member_from_identifier(text));
    }

    let mri = string_at(
        value,
        &[
            "mri", "Mri", "userMri", "userMRI", "id", "userId", "skypeMri",
        ],
    )
    .filter(|text| text.starts_with("8:"));
    let object_id = string_at(
        value,
        &[
            "objectId",
            "objectID",
            "aadObjectId",
            "aadObjectID",
            "aadId",
            "oid",
            "userObjectId",
        ],
    )
    .or_else(|| oid_from_mri(mri.as_deref()))
    .or_else(|| string_at(value, &["id", "userId"]).filter(|text| looks_like_guid(text)));
    let member = ChatMember {
        mri,
        object_id,
        display_name: display_name_from_value(value),
        user_principal_name: string_at(
            value,
            &[
                "userPrincipalName",
                "upn",
                "email",
                "mail",
                "smtp",
                "loginName",
            ],
        )
        .filter(|text| !text.trim().is_empty()),
        role: string_at(value, &["role", "userRole", "relationship"])
            .filter(|text| !text.trim().is_empty()),
    };
    (!is_empty_member(&member)).then_some(member)
}

fn member_from_identifier(value: &str) -> ChatMember {
    if value.starts_with("8:") {
        ChatMember {
            mri: Some(value.to_string()),
            object_id: oid_from_mri(Some(value)),
            ..Default::default()
        }
    } else if looks_like_guid(value) {
        ChatMember {
            mri: Some(format!("8:orgid:{value}")),
            object_id: Some(value.to_string()),
            ..Default::default()
        }
    } else if value.contains('@') {
        ChatMember {
            user_principal_name: Some(value.to_string()),
            ..Default::default()
        }
    } else {
        ChatMember {
            display_name: Some(value.to_string()),
            ..Default::default()
        }
    }
}

fn oid_from_mri(value: Option<&str>) -> Option<String> {
    value
        .and_then(|mri| mri.strip_prefix("8:orgid:"))
        .filter(|oid| looks_like_guid(oid))
        .map(ToString::to_string)
}

fn looks_like_guid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
}

fn display_name_from_value(value: &serde_json::Value) -> Option<String> {
    string_at(
        value,
        &[
            "displayName",
            "displayname",
            "name",
            "imDisplayName",
            "imdisplayname",
            "userPrincipalName",
        ],
    )
    .or_else(|| value.pointer("/profile/displayName").and_then(as_string))
    .filter(|name| !name.trim().is_empty())
}

fn percent_encode_path_segment(input: &str) -> String {
    url::form_urlencoded::byte_serialize(input.as_bytes()).collect()
}

fn message_preview(input: &str) -> String {
    let mut text = input.replace("\r\n", "\n");
    text = replace_tag_with_attr(&text, "img", "alt", "[image]");
    text = replace_tag_with_attr(&text, "URIObject", "type", "[attachment]");
    text = replace_title_tags(&text);
    text = text
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
        .replace("</p>", "\n")
        .replace("</div>", "\n");
    let without_tags = strip_tags(&text);
    decode_entities(&without_tags)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn replace_title_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let Some(start) = rest.find("<Title>") else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..start]);
        let after_start = &rest[start + "<Title>".len()..];
        let Some(end) = after_start.find("</Title>") else {
            output.push_str(after_start);
            break;
        };
        output.push_str("[attachment: ");
        output.push_str(&after_start[..end]);
        output.push(']');
        rest = &after_start[end + "</Title>".len()..];
    }
    output
}

fn replace_tag_with_attr(input: &str, tag: &str, attr: &str, fallback: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    let open = format!("<{tag}");
    loop {
        let Some(start) = rest.find(&open) else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..start]);
        let after_start = &rest[start..];
        let Some(end) = after_start.find('>') else {
            output.push_str(after_start);
            break;
        };
        let tag_text = &after_start[..=end];
        if let Some(value) = attr_value(tag_text, attr) {
            output.push_str(fallback.trim_end_matches(']'));
            output.push_str(": ");
            output.push_str(&value);
            output.push(']');
        } else {
            output.push_str(fallback);
        }
        rest = &after_start[end + 1..];
    }
    output
}

fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')?;
    Some(tag[start..start + end].to_string())
}

fn strip_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn decode_entities(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '&' {
            output.push(ch);
            continue;
        }
        let mut entity = String::new();
        while let Some(&next) = chars.peek() {
            chars.next();
            if next == ';' {
                break;
            }
            if entity.len() > 12 {
                break;
            }
            entity.push(next);
        }
        match decode_entity(&entity) {
            Some(decoded) => output.push(decoded),
            None => {
                output.push('&');
                output.push_str(&entity);
                output.push(';');
            }
        }
    }
    output
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_strips_html_and_decodes_entities() {
        assert_eq!(
            message_preview("<p>Hello&nbsp;&#51004;</p><p><b>world</b></p>"),
            "Hello 으 world"
        );
    }

    #[test]
    fn preview_summarizes_images() {
        assert_eq!(
            message_preview("<p><img alt=\"sample\" src=\"https://example.test/a.png\"></p>"),
            "[image: sample]"
        );
    }

    #[test]
    fn peer_oid_uses_other_side_of_one_to_one_thread_id() {
        assert_eq!(
            one_to_one_peer_oid("19:peer_self@unq.gbl.spaces", "self").as_deref(),
            Some("peer")
        );
    }

    #[test]
    fn detects_self_title() {
        let member = ChatMember {
            display_name: Some("Current User".to_string()),
            user_principal_name: Some("me@example.com".to_string()),
            ..Default::default()
        };

        assert!(chat_title_matches_member(
            &Some("current user".to_string()),
            &member
        ));
        assert!(chat_title_matches_member(
            &Some("ME@example.com".to_string()),
            &member
        ));
        assert!(!chat_title_matches_member(
            &Some("Other User".to_string()),
            &member
        ));
    }

    #[test]
    fn labels_self_notes_system_chat() {
        let value = serde_json::json!({ "id": "48:notes" });
        let chat = normalize_chat(&value, &BTreeMap::new()).expect("chat");

        assert!(matches!(chat.kind, ChatKind::System));
        assert_eq!(chat.title.as_deref(), Some("Self notes"));
    }
}
