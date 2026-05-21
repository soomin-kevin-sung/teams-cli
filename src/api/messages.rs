use super::client::{ApiClient, ApiError, AuthStyle};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize)]
struct SendMessageRequest<'a> {
    content: String,
    messagetype: &'static str,
    contenttype: &'static str,
    clientmessageid: String,
    imdisplayname: &'a str,
    properties: SendMessageProperties,
}

#[derive(Debug, Serialize)]
struct SendMessageProperties {
    importance: &'static str,
    subject: Option<String>,
    links: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SendMessageResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default, rename = "Id")]
    id_pascal: Option<String>,
    #[serde(default, rename = "messageId")]
    message_id: Option<String>,
    #[serde(default, rename = "MessageId")]
    message_id_pascal: Option<String>,
}

impl SendMessageResponse {
    fn server_message_id(self) -> Option<String> {
        self.id
            .or(self.id_pascal)
            .or(self.message_id)
            .or(self.message_id_pascal)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SentMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub client_message_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<MessageSender>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_is_self: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_html: Option<String>,
    pub attachments: Vec<MessageAttachment>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MessageSender {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_principal_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MessageAttachment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_url: Option<bool>,
    #[serde(skip_serializing)]
    pub url: Option<String>,
}

pub async fn send_message(
    api: &ApiClient,
    thread_id: &str,
    body_plain: &str,
) -> Result<SentMessage, ApiError> {
    let display_name = api.display_name().await;
    let encoded_thread_id = percent_encode_path_segment(thread_id);
    let url = format!(
        "{}/v1/users/ME/conversations/{}/messages",
        api.chat_service().await,
        encoded_thread_id
    );
    let client_message_id = chrono::Utc::now().timestamp_millis().to_string();
    let body = SendMessageRequest {
        content: format!("<p>{}</p>", html_escape(body_plain)),
        messagetype: "RichText/Html",
        contenttype: "text",
        clientmessageid: client_message_id.clone(),
        imdisplayname: &display_name,
        properties: SendMessageProperties {
            importance: "",
            subject: None,
            links: Vec::new(),
        },
    };
    let response = api
        .post_json::<SendMessageResponse, _>(&url, AuthStyle::SkypeHeader, &body)
        .await?;
    Ok(SentMessage {
        id: response.server_message_id(),
        client_message_id,
    })
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendMessageMutationData {
    send_message: Option<SendMessageMutationResult>,
}

#[derive(Debug, Deserialize)]
struct SendMessageMutationResult {
    id: Option<String>,
}

pub async fn post_channel_message(
    api: &ApiClient,
    thread_id: &str,
    body_plain: &str,
    reply_chain_id: Option<&str>,
) -> Result<SentMessage, ApiError> {
    let url = format!("{}/graphql", api.csa_base().await);
    let client_message_id = chrono::Utc::now().timestamp_millis().to_string();
    let mut message = json!({
        "content": format!("<p>{}</p>", html_escape(body_plain)),
        "clientMessageId": client_message_id,
        "messageType": "RichText/Html",
        "importance": "Standard"
    });
    if let Some(reply_chain_id) = reply_chain_id {
        message["replyChainId"] = json!(reply_chain_id);
    }
    let body = json!({
        "operationName": "sendMessage",
        "query": "mutation sendMessage($convId: ID!, $message: SendMessageInput, $action: SendMessageAction) { sendMessage(convId: $convId, message: $message, action: $action) { id } }",
        "variables": {
            "convId": thread_id,
            "message": message,
            "action": "Create"
        }
    });
    let response = api
        .post_json::<GraphQlResponse<SendMessageMutationData>, _>(
            &url,
            AuthStyle::BearerAadPlusSkype,
            &body,
        )
        .await?;
    if let Some(error) = response.errors.first() {
        return Err(ApiError::Http {
            status: 200,
            body: error.message.clone(),
        });
    }
    Ok(SentMessage {
        id: response
            .data
            .and_then(|data| data.send_message)
            .and_then(|message| message.id),
        client_message_id,
    })
}

pub async fn read_messages(
    api: &ApiClient,
    thread_id: &str,
    limit: usize,
) -> Result<Vec<ChatMessage>, ApiError> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let encoded_thread_id = percent_encode_path_segment(thread_id);
    let chat_service = api.chat_service().await;
    let chat_svc_agg = api.chat_svc_agg().await;
    let candidates = [
        (
            format!(
                "{chat_service}/v1/users/ME/conversations/{encoded_thread_id}/messages?view=msnp24Equivalent&pageSize={limit}"
            ),
            AuthStyle::SkypeHeader,
        ),
        (
            format!(
                "{chat_service}/v1/users/ME/conversations/{encoded_thread_id}/messages?view=msnp24Equivalent"
            ),
            AuthStyle::SkypeHeader,
        ),
        (
            format!(
                "{chat_svc_agg}/api/v1/users/ME/conversations/{encoded_thread_id}/messages?view=msnp24Equivalent&pageSize={limit}"
            ),
            AuthStyle::SkypeHeader,
        ),
        (
            format!(
                "{chat_service}/v1/users/ME/conversations/{encoded_thread_id}/messages?pageSize={limit}"
            ),
            AuthStyle::BearerSkype,
        ),
    ];

    let mut last_error = None;
    let mut empty_success = None;
    for (url, style) in candidates {
        match api.get_json::<serde_json::Value>(&url, style).await {
            Ok(value) => {
                let mut messages = normalize_messages(&value);
                messages.truncate(limit);
                if !messages.is_empty() {
                    return Ok(messages);
                }
                empty_success = Some(messages);
            }
            Err(ApiError::Http { status, body }) if matches!(status, 400 | 401 | 403 | 404) => {
                last_error = Some(ApiError::Http { status, body });
            }
            Err(ApiError::NotFound(body)) => last_error = Some(ApiError::NotFound(body)),
            Err(error) => return Err(error),
        }
    }

    if let Some(messages) = empty_success {
        return Ok(messages);
    }

    Err(last_error
        .unwrap_or_else(|| ApiError::NotFound("no message endpoint succeeded".to_string())))
}

pub fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\n', "<br/>")
}

pub fn percent_encode_path_segment(input: &str) -> String {
    url::form_urlencoded::byte_serialize(input.as_bytes()).collect()
}

fn normalize_messages(value: &serde_json::Value) -> Vec<ChatMessage> {
    candidate_message_arrays(value)
        .into_iter()
        .flat_map(|array| array.iter())
        .filter_map(normalize_message)
        .collect()
}

fn candidate_message_arrays(value: &serde_json::Value) -> Vec<&Vec<serde_json::Value>> {
    let mut arrays = Vec::new();
    if let Some(array) = value.as_array() {
        arrays.push(array);
    }
    for key in ["messages", "value", "items", "results"] {
        if let Some(array) = value.get(key).and_then(serde_json::Value::as_array) {
            arrays.push(array);
        }
    }
    if let Some(array) = value
        .pointer("/conversation/messages")
        .and_then(serde_json::Value::as_array)
    {
        arrays.push(array);
    }
    arrays
}

fn normalize_message(value: &serde_json::Value) -> Option<ChatMessage> {
    let content_html = sanitize_text_field(
        string_at(value, &["content", "body", "html", "message"])
            .or_else(|| value.pointer("/body/content").and_then(as_string)),
    );
    let content_text = content_html
        .as_deref()
        .map(html_to_text)
        .filter(|text| !text.is_empty())
        .or_else(|| sanitize_text_field(string_at(value, &["text", "plainText", "preview"])));
    let message = ChatMessage {
        id: string_at(value, &["id", "messageId", "serverMessageId"]),
        client_message_id: string_at(value, &["clientmessageid", "clientMessageId"]),
        created_at: first_datetime(
            value,
            &[
                "/originalarrivaltime",
                "/originalArrivalTime",
                "/composetime",
                "/composeTime",
                "/createdDateTime",
                "/created_at",
                "/timestamp",
            ],
        ),
        sender: sender_from_message(value),
        sender_is_self: None,
        message_type: string_at(value, &["messagetype", "messageType", "type"]),
        content_type: string_at(value, &["contenttype", "contentType"])
            .or_else(|| value.pointer("/body/contentType").and_then(as_string)),
        content_text,
        content_html,
        attachments: attachments_from_message(value),
    };
    (message.id.is_some()
        || message.client_message_id.is_some()
        || message.content_text.is_some()
        || message.content_html.is_some())
    .then_some(message)
}

fn sender_from_message(value: &serde_json::Value) -> Option<MessageSender> {
    let from = value.get("from");
    let from_user_id = value.pointer("/from/user/id").and_then(as_string);
    let mut sender = MessageSender {
        mri: string_at(value, &["from", "sender", "user"])
            .filter(|text| text.starts_with("8:"))
            .or_else(|| from.and_then(|from| string_at(from, &["mri", "id", "userId"])))
            .filter(|text| text.starts_with("8:"))
            .or_else(|| {
                from_user_id
                    .as_deref()
                    .filter(|id| id.starts_with("8:"))
                    .map(ToString::to_string)
            }),
        object_id: from
            .and_then(|from| {
                string_at(
                    from,
                    &["objectId", "aadObjectId", "aadId", "oid", "userObjectId"],
                )
            })
            .or_else(|| value.pointer("/from/user/objectId").and_then(as_string))
            .or_else(|| {
                from_user_id
                    .as_deref()
                    .filter(|id| looks_like_guid(id))
                    .map(ToString::to_string)
            }),
        display_name: sanitize_sender_field(
            string_at(value, &["imdisplayname", "imDisplayName", "displayName"])
                .or_else(|| from.and_then(|from| string_at(from, &["displayName", "name"])))
                .or_else(|| value.pointer("/from/user/displayName").and_then(as_string)),
        ),
        user_principal_name: sanitize_sender_field(
            from.and_then(|from| string_at(from, &["userPrincipalName", "upn", "email", "mail"]))
                .or_else(|| {
                    value
                        .pointer("/from/user/userPrincipalName")
                        .and_then(as_string)
                }),
        ),
    };
    if sender.object_id.is_none() {
        sender.object_id = oid_from_mri(sender.mri.as_deref());
    }
    (!is_empty_sender(&sender)).then_some(sender)
}

fn attachments_from_message(value: &serde_json::Value) -> Vec<MessageAttachment> {
    ["attachments", "files"]
        .iter()
        .filter_map(|key| value.get(*key).and_then(serde_json::Value::as_array))
        .flat_map(|attachments| attachments.iter())
        .filter_map(attachment_from_value)
        .collect()
}

fn attachment_from_value(value: &serde_json::Value) -> Option<MessageAttachment> {
    let url = string_at(
        value,
        &[
            "url",
            "contentUrl",
            "contenturl",
            "downloadUrl",
            "previewUrl",
        ],
    );
    let attachment = MessageAttachment {
        id: string_at(value, &["id", "itemid", "itemId"]),
        name: string_at(value, &["name", "title", "filename", "fileName"]),
        content_type: string_at(value, &["contentType", "contenttype", "type"]),
        has_url: url.is_some().then_some(true),
        url,
    };
    (attachment.id.is_some()
        || attachment.name.is_some()
        || attachment.content_type.is_some()
        || attachment.url.is_some())
    .then_some(attachment)
}

fn string_at(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(as_string))
        .filter(|text| !text.trim().is_empty())
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

fn html_to_text(input: &str) -> String {
    let mut text = input.replace("\r\n", "\n");
    text = text
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
        .replace("</p>", "\n")
        .replace("</div>", "\n");
    decode_entities(&strip_tags(&text))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
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

        let probe = chars.clone();
        let mut entity = String::new();
        let mut found_semicolon = false;
        for next in probe {
            if next == ';' {
                found_semicolon = true;
                break;
            }
            if next.is_whitespace() || entity.len() >= 12 {
                break;
            }
            entity.push(next);
        }

        if !found_semicolon {
            output.push('&');
            continue;
        }

        for _ in 0..=entity.chars().count() {
            chars.next();
        }

        if let Some(decoded) = decode_entity(&entity) {
            output.push(decoded);
        } else {
            output.push('&');
            output.push_str(&entity);
            output.push(';');
        }
    }
    output
}

fn sanitize_text_field(value: Option<String>) -> Option<String> {
    value.map(|text| {
        text.chars()
            .map(|ch| {
                if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
                    ' '
                } else {
                    ch
                }
            })
            .collect()
    })
}

fn sanitize_sender_field(value: Option<String>) -> Option<String> {
    value.map(|text| {
        text.chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect()
    })
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

fn is_empty_sender(sender: &MessageSender) -> bool {
    sender.mri.is_none()
        && sender.object_id.is_none()
        && sender.display_name.is_none()
        && sender.user_principal_name.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_covers_special_chars_and_newline() {
        assert_eq!(html_escape("<&>\"\nnext"), "&lt;&amp;&gt;&quot;<br/>next");
    }

    #[test]
    fn percent_encodes_thread_id() {
        assert_eq!(
            percent_encode_path_segment("19:abc@thread.v2"),
            "19%3Aabc%40thread.v2"
        );
    }

    #[test]
    fn extracts_known_message_id_fields() {
        for (field, expected) in [
            ("id", "a"),
            ("Id", "b"),
            ("messageId", "c"),
            ("MessageId", "d"),
        ] {
            let json = format!(r#"{{ "{field}": "{expected}" }}"#);
            let response: SendMessageResponse = serde_json::from_str(&json).expect("response");

            assert_eq!(response.server_message_id().as_deref(), Some(expected));
        }
    }

    #[test]
    fn missing_server_message_id_is_allowed() {
        let response: SendMessageResponse = serde_json::from_str("{}").expect("response");

        assert_eq!(response.server_message_id(), None);
    }

    #[test]
    fn normalizes_chat_service_messages() {
        let value = serde_json::json!({
            "messages": [
                {
                    "id": "msg1",
                    "clientmessageid": "client1",
                    "from": "8:orgid:11111111-1111-1111-1111-111111111111",
                    "imdisplayname": "Alex",
                    "originalarrivaltime": "2026-05-21T01:02:03Z",
                    "messagetype": "RichText/Html",
                    "contenttype": "text",
                    "content": "<p>Hello&nbsp;<b>world</b></p>",
                    "attachments": [
                        {
                            "id": "file1",
                            "name": "report.pdf",
                            "contentType": "reference",
                            "contentUrl": "https://example.test/report.pdf"
                        }
                    ]
                }
            ]
        });

        let messages = normalize_messages(&value);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id.as_deref(), Some("msg1"));
        assert_eq!(messages[0].content_text.as_deref(), Some("Hello world"));
        assert_eq!(
            messages[0]
                .sender
                .as_ref()
                .and_then(|sender| sender.display_name.as_deref()),
            Some("Alex")
        );
        assert_eq!(
            messages[0].attachments[0].name.as_deref(),
            Some("report.pdf")
        );
        let serialized = serde_json::to_value(&messages[0]).expect("serialize");
        assert_eq!(
            serialized["attachments"][0]["has_url"],
            serde_json::Value::Bool(true)
        );
        assert!(serialized["attachments"][0].get("url").is_none());
    }

    #[test]
    fn normalizes_graph_like_messages() {
        let value = serde_json::json!({
            "value": [
                {
                    "id": "msg2",
                    "createdDateTime": "2026-05-21T01:02:03Z",
                    "from": {
                        "user": {
                            "id": "22222222-2222-2222-2222-222222222222",
                            "displayName": "Blair",
                            "userPrincipalName": "blair@example.com"
                        }
                    },
                    "body": {
                        "contentType": "html",
                        "content": "<div>Line 1<br>Line 2</div>"
                    }
                }
            ]
        });

        let messages = normalize_messages(&value);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content_text.as_deref(), Some("Line 1 Line 2"));
        assert_eq!(
            messages[0]
                .sender
                .as_ref()
                .and_then(|sender| sender.user_principal_name.as_deref()),
            Some("blair@example.com")
        );
        assert_eq!(
            messages[0]
                .sender
                .as_ref()
                .and_then(|sender| sender.object_id.as_deref()),
            Some("22222222-2222-2222-2222-222222222222")
        );
        assert_eq!(
            messages[0]
                .sender
                .as_ref()
                .and_then(|sender| sender.mri.as_deref()),
            None
        );
    }

    #[test]
    fn entity_decoder_preserves_plain_ampersands() {
        assert_eq!(
            html_to_text("<p>R&D and A & B &bogus;</p>"),
            "R&D and A & B &bogus;"
        );
    }

    #[test]
    fn normalizer_sanitizes_control_characters() {
        let value = serde_json::json!({
            "messages": [
                {
                    "id": "msg3",
                    "imdisplayname": "A\u{001b}[31m",
                    "plainText": "hello\u{0008}there"
                }
            ]
        });

        let messages = normalize_messages(&value);

        assert_eq!(messages[0].content_text.as_deref(), Some("hello there"));
        assert_eq!(
            messages[0]
                .sender
                .as_ref()
                .and_then(|sender| sender.display_name.as_deref()),
            Some("A [31m")
        );
    }
}
