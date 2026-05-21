use super::client::{ApiClient, ApiError, AuthStyle};
use serde::{Deserialize, Serialize};

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
}
