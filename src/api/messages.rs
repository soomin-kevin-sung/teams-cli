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
}

pub async fn send_message(
    api: &ApiClient,
    thread_id: &str,
    body_plain: &str,
) -> Result<String, ApiError> {
    let display_name = api.display_name().await;
    let encoded_thread_id = percent_encode_path_segment(thread_id);
    let url = format!(
        "{}/v1/users/ME/conversations/{}/messages",
        api.chat_service().await,
        encoded_thread_id
    );
    let body = SendMessageRequest {
        content: format!("<p>{}</p>", html_escape(body_plain)),
        messagetype: "RichText/Html",
        contenttype: "text",
        clientmessageid: chrono::Utc::now().timestamp_millis().to_string(),
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
    response
        .id
        .or(response.id_pascal)
        .ok_or_else(|| ApiError::Http {
            status: 201,
            body: "send response did not include a message id".to_string(),
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
}
